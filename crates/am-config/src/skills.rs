//! The skill loader (AD-38): `skills/<id>/` folders in the open Agent Skills
//! format. A skill folder holds `SKILL.md` and an optional `references/`
//! folder of plain-text files, nothing else: no `scripts/`, no other file
//! type and no executable bit. Every file is read through the loader, so it
//! counts in the snapshot hash and gets the secret check. Nothing here is
//! run, and nothing a skill says reaches a seat's grants.

use std::fs::Metadata;
use std::path::Path;

use crate::finding::Report;
use crate::{Loader, checks, frontmatter, kinds, node};

const REFERENCE_EXTENSIONS: &[&str] = &["md", "txt", "csv", "json", "yaml", "yml"];
const REFERENCE_MAX_BYTES: u64 = 256 * 1024;
const REFERENCE_MAX_FILES: usize = 20;
const ONLY: &str = "a skill folder holds SKILL.md and references/ only";

/// The entries of `dir` with their metadata (not following links), sorted
/// by name. Only `.DS_Store` (Finder's own file) is skipped: every other
/// hidden entry gets the same checks as any entry.
fn listing(dir: &Path) -> Vec<(String, Metadata)> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<(String, Metadata)> = read
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let meta = std::fs::symlink_metadata(e.path()).ok()?;
            (name != ".DS_Store").then_some((name, meta))
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[cfg(unix)]
fn executable(meta: &Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn executable(_meta: &Metadata) -> bool {
    false
}

/// Reports a link or an executable file; true when the entry is fine so far.
fn plain_entry(rep: &mut Report, name: &str, meta: &Metadata) -> bool {
    if meta.file_type().is_symlink() {
        rep.error(
            1,
            format!("'{name}' is a link; links are not accepted in a skill folder"),
            "replace the link with the file itself, or remove it",
        );
        return false;
    }
    if meta.is_file() && executable(meta) {
        rep.error(
            1,
            format!("'{name}' is executable; no file in a skill folder may be"),
            "clear the executable bit (chmod a-x) or remove the file; skills are instructions only",
        );
        return false;
    }
    true
}

/// Walks `skills/`, declaring every skill folder and reading each one.
pub(crate) fn load_all(loader: &mut Loader, root: &Path) {
    for (name, meta) in listing(&root.join("skills")) {
        let rel = format!("skills/{name}");
        if meta.file_type().is_symlink() {
            loader.report(&rel).error(
                1,
                "a link is not accepted in skills/",
                "put the skill folder itself in skills/",
            );
            continue;
        }
        if !meta.is_dir() {
            loader.report(&rel).warning(
                1,
                "this file is not read",
                "skills/ holds one folder per skill, each with a SKILL.md",
            );
            continue;
        }
        if !checks::is_kebab(&name) {
            loader.report(&rel).error(
                1,
                format!("skill folder '{name}' is not a kebab-case id"),
                format!(
                    "rename the folder to skills/{}/ (lowercase words joined by hyphens; the folder name is the skill id)",
                    checks::kebab_of(&name)
                ),
            );
            continue;
        }
        loader.declared.skills.insert(name.clone());
        load_one(loader, root, &name);
    }
}

fn load_one(loader: &mut Loader, root: &Path, id: &str) {
    let base = format!("skills/{id}");
    let mut has_skill_md = false;
    let mut has_references = false;
    for (name, meta) in listing(&root.join(&base)) {
        let rel = format!("{base}/{name}");
        let mut rep = Report::new(&rel);
        if plain_entry(&mut rep, &name, &meta) {
            if meta.is_dir() {
                match name.as_str() {
                    "references" => has_references = true,
                    "scripts" => rep.error(
                        1,
                        "a skill may not hold a scripts/ folder: skills are instructions only and no code runs",
                        "remove the scripts/ folder and describe the steps in SKILL.md instead",
                    ),
                    _ => rep.error(
                        1,
                        format!("folder '{name}/' is not accepted in a skill folder"),
                        ONLY,
                    ),
                }
            } else if name == "SKILL.md" {
                has_skill_md = true;
            } else {
                rep.error(
                    1,
                    format!("file '{name}' is not accepted in a skill folder"),
                    format!(
                        "{ONLY}; put reference material in references/ as {} files",
                        REFERENCE_EXTENSIONS.join(", ")
                    ),
                );
            }
        }
        loader.merge(rep);
    }
    if !has_skill_md {
        loader.report(&format!("{base}/SKILL.md")).error(
            1,
            "missing SKILL.md",
            format!("add {base}/SKILL.md with `name: {id}` and `description:` frontmatter"),
        );
        return;
    }
    let references = if has_references {
        load_references(loader, root, &base)
    } else {
        Default::default()
    };
    let rel = format!("{base}/SKILL.md");
    let Some(text) = loader.read(root, &rel) else {
        return;
    };
    let mut rep = Report::new(&rel);
    let fix = "start SKILL.md with a line `---`, then `name:` and `description:`, another `---`, then the instructions";
    if let Some((front, offset, body)) = frontmatter(&mut rep, &text, fix)
        && let Some(node) = kinds::yaml_plain(&mut rep, front, offset)
    {
        let body_line = crate::body_first_line(&text, body);
        checks::text_secrets(&mut rep, body, body_line);
        if let Some(mut skill) = kinds::skill(&mut rep, id, &node, body, body_line) {
            skill.references = references;
            loader.skills.insert(id.to_string(), skill);
        }
    }
    loader.merge(rep);
}

/// For a yaml, yml or json reference that parses: the key-name secret check
/// (a line the prefix scan already flagged is not flagged twice) and, for
/// yaml, the comment check. One that does not parse keeps the prefix scan
/// only, with no new finding.
fn structured_checks(rep: &mut Report, ext: &str, text: &str) {
    if !matches!(ext, "yaml" | "yml" | "json") {
        return;
    }
    let Ok(root) = node::parse(text, 0) else {
        return;
    };
    let flagged: Vec<u32> = rep.findings.iter().map(|f| f.line).collect();
    let mut walk = Report::new(&rep.file);
    checks::secrets(&mut walk, &root);
    rep.findings.extend(
        walk.findings
            .into_iter()
            .filter(|f| !flagged.contains(&f.line)),
    );
    if ext != "json" {
        checks::comments(rep, &node::comment_lines(text, 0));
    }
}

fn load_references(
    loader: &mut Loader,
    root: &Path,
    base: &str,
) -> std::collections::BTreeMap<String, String> {
    let dir = format!("{base}/references");
    let mut out = std::collections::BTreeMap::new();
    let entries = listing(&root.join(&dir));
    let files = entries.iter().filter(|(_, m)| m.is_file()).count();
    if files > REFERENCE_MAX_FILES {
        loader.report(&dir).error(
            1,
            format!(
                "references/ holds {files} files; a skill may have at most {REFERENCE_MAX_FILES}"
            ),
            "keep the 20 most useful reference files and remove the rest",
        );
    }
    for (name, meta) in entries {
        let rel = format!("{dir}/{name}");
        let mut rep = Report::new(&rel);
        if !plain_entry(&mut rep, &name, &meta) {
            loader.merge(rep);
            continue;
        }
        let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
        if meta.is_dir()
            || !ext
                .as_deref()
                .is_some_and(|e| REFERENCE_EXTENSIONS.contains(&e))
        {
            let what = if meta.is_dir() { "folder" } else { "file" };
            rep.error(
                1,
                format!("{what} '{name}' is not accepted in references/"),
                format!(
                    "references/ holds {} files only, with no folders inside",
                    REFERENCE_EXTENSIONS.join(", ")
                ),
            );
            loader.merge(rep);
            continue;
        }
        if meta.len() > REFERENCE_MAX_BYTES {
            rep.error(
                1,
                format!(
                    "'{name}' is {} bytes; a reference file may be at most 256 KB",
                    meta.len()
                ),
                "split the file or keep only the part the skill needs",
            );
            loader.merge(rep);
            continue;
        }
        loader.merge(rep);
        if let Some(text) = loader.read(root, &rel) {
            let mut rep = Report::new(&rel);
            checks::text_secrets(&mut rep, &text, 1);
            structured_checks(&mut rep, ext.as_deref().unwrap_or(""), &text);
            loader.merge(rep);
            out.insert(name, text);
        }
    }
    out
}
