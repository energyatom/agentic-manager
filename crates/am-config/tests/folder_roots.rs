//! Story 1.14 acceptance tests (AD-30 as amended 2026-11-20 and 2026-11-22):
//! which folders a seat may link. Matrix rows "Cloud roots", "Refused
//! roots", "In-project" and "In-project refused", plus the boundaries the
//! plan lists (recent project roots, a folder holding `company.yaml` found
//! by walking up, the name-only subtree walk that follows no link and stops
//! in words, and the config names equal to what the loader reads). Written
//! before the policy exists.
//!
//! Every project here is written in a temporary folder at `schema_version:
//! 2`, with an injected home folder, through the `LoadOptions` fields the
//! plan adds: `home: Option<PathBuf>` and `recent_roots: Vec<PathBuf>`.
//! Every refusal is an error on the folder's `path` line whose message says
//! "not allowed", with a fix.
#![forbid(unsafe_code)]

mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use am_config::{Finding, LoadOptions, Loaded, Severity, config_files, load_in};
use common::{COMPANY_V2, write};

const SEAT: &str = "seats/keeper.md";
/// The line of the folder's `path:` in [`keeper`].
const PATH_LINE: u32 = 8;

/// A seat linking one folder: `path` on line 8, `access` on line 9.
fn keeper(path: &str, access: &str) -> String {
    format!(
        "---\nschema_version: 2\nname: Keeper\nrole: Writer\nteam: core\nfolders:\n  - id: work\n    path: \"{path}\"\n    access: {access}\n---\nKeep the notes up to date.\n"
    )
}

/// A home folder and a project, both canonical, in one temporary folder.
struct Env {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    project: PathBuf,
}

impl Env {
    /// The project lies outside the home folder.
    fn new() -> Env {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let home = base.join("home");
        let project = base.join("proj");
        Env::make(tmp, home, project)
    }

    /// The project lies in the home folder, at `~/proj`.
    fn in_home() -> Env {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let home = base.join("home");
        let project = home.join("proj");
        Env::make(tmp, home, project)
    }

    fn make(tmp: tempfile::TempDir, home: PathBuf, project: PathBuf) -> Env {
        std::fs::create_dir_all(&home).unwrap();
        write(&project, "company.yaml", COMPANY_V2);
        write(&project, SEAT, &keeper("files/in", "read"));
        Env {
            _tmp: tmp,
            home,
            project,
        }
    }

    /// Creates the folder `rel` under the home folder.
    fn home_dir(&self, rel: &str) -> PathBuf {
        let p = self.home.join(rel);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// Creates the folder `rel` in the project.
    fn project_dir(&self, rel: &str) -> PathBuf {
        let p = self.project.join(rel);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn link(&self, path: &str, access: &str) {
        write(&self.project, SEAT, &keeper(path, access));
    }

    fn load(&self, recent_roots: Vec<PathBuf>) -> Loaded {
        let opts = LoadOptions {
            home: Some(self.home.clone()),
            recent_roots,
            ..LoadOptions::default()
        };
        load_in(&self.project, opts).expect("the project folder exists")
    }

    /// The errors on the seat file after linking `path` with `access`.
    fn errors_for(&self, path: &str, access: &str, recent: Vec<PathBuf>) -> Vec<Finding> {
        self.link(path, access);
        self.load(recent)
            .findings
            .into_iter()
            .filter(|f| f.file == SEAT && f.severity == Severity::Error)
            .collect()
    }

    /// Asserts the link validates (no error on the seat file).
    fn allowed(&self, path: &str, access: &str, recent: Vec<PathBuf>) {
        let errors = self.errors_for(path, access, recent);
        assert!(errors.is_empty(), "{path} ({access}): {errors:#?}");
    }

    /// Asserts exactly one refusal at the path line, with a fix; returns it.
    fn refused(&self, path: &str, access: &str, recent: Vec<PathBuf>) -> Finding {
        let errors = self.errors_for(path, access, recent);
        assert_eq!(errors.len(), 1, "{path} ({access}): {errors:#?}");
        let f = errors.into_iter().next().unwrap();
        assert_eq!(f.line, PATH_LINE, "{path} ({access}): {f:#?}");
        assert!(f.message.contains("not allowed"), "{path}: {f:#?}");
        assert!(!f.fix.is_empty(), "{path}: {f:#?}");
        f
    }
}

// ------------------------------------------------------------- cloud roots

#[test]
fn folders_in_cloud_storage_and_icloud_drive_validate() {
    let env = Env::new();
    for path in [
        "~/Library/CloudStorage/OneDrive-Fixture/x",
        "~/Library/CloudStorage/OneDrive-Fixture",
        "~/Library/CloudStorage/GoogleDrive-someone@example.org/My Drive/notes",
        "~/Library/Mobile Documents/com~apple~CloudDocs/x",
        "~/Library/Mobile Documents/com~apple~CloudDocs/x/y",
    ] {
        env.home_dir(path.trim_start_matches("~/"));
        env.allowed(path, "read", vec![]);
    }
    let loaded = env.load(vec![]);
    let snap = loaded.snapshot.expect("a cloud folder keeps the snapshot");
    assert_eq!(
        snap.seats()["keeper"].folders[0].path,
        "~/Library/Mobile Documents/com~apple~CloudDocs/x/y"
    );
}

#[test]
fn a_cloud_folder_may_be_linked_with_write_access() {
    let env = Env::new();
    env.home_dir("Library/CloudStorage/OneDrive-Fixture/out");
    env.allowed(
        "~/Library/CloudStorage/OneDrive-Fixture/out",
        "read_write",
        vec![],
    );
}

// ------------------------------------------------------------- refused roots

#[test]
fn library_its_cloud_parents_other_containers_and_runtime_folders_are_refused() {
    let env = Env::new();
    for path in [
        "~/Library",
        "~/Library/",
        "~/library/x",
        "~/Library/Preferences",
        "~/Library/CloudStorage",
        "~/Library/CloudStorage/",
        "~/Library/Mobile Documents",
        "~/Library/Mobile Documents/com~apple~Pages",
        "~/Library/Mobile Documents/com~apple~Pages/x",
        "~/Library/Mobile Documents/com~apple~Numbers/x",
        "~/Library/Mobile Documents/com~apple~Keynote/x",
        // The runtime base and the app data dir (AD-32).
        "~/Library/Application Support/agentic-manager-projects",
        "~/Library/Application Support/agentic-manager-projects/0192f0a0-0000-7000-8000-000000000001",
        "~/Library/Application Support/agentic-manager",
        // Climbing out of a cloud root lands in ~/Library.
        "~/Library/CloudStorage/../Preferences",
        "~/Library/CloudStorage/OneDrive-Fixture/../../Preferences",
        "~/Library/Mobile Documents/com~apple~CloudDocs/../com~apple~Pages",
    ] {
        env.home_dir(path.trim_start_matches("~/"));
        let f = env.refused(path, "read", vec![]);
        // The fix names the two roots that may be linked.
        assert!(
            f.fix.contains("~/Library/CloudStorage"),
            "{path}: {}",
            f.fix
        );
        assert!(f.fix.contains("com~apple~CloudDocs"), "{path}: {}", f.fix);
    }
}

#[test]
fn the_root_the_home_folder_the_project_and_its_parent_are_refused() {
    let env = Env::new();
    let name = env
        .project
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    for path in [
        "/".to_string(),
        "~".to_string(),
        "~/".to_string(),
        ".".to_string(),
        "..".to_string(),
        format!("../{name}"),
        format!("../{name}/files/.."),
    ] {
        env.refused(&path, "read", vec![]);
    }
}

// ------------------------------------------------------------- recent projects

#[test]
fn a_recent_project_root_its_insides_and_its_parents_are_refused() {
    let env = Env::new();
    let other = env.home_dir("work/other");
    write(&other, "company.yaml", COMPANY_V2);
    env.home_dir("work/other/files");
    env.home_dir("elsewhere");
    let recent = || vec![other.clone()];
    for path in ["~/work/other", "~/work/other/files", "~/work"] {
        let f = env.refused(path, "read", recent());
        assert!(f.message.contains("project"), "{path}: {f:#?}");
    }
    env.allowed("~/elsewhere", "read", recent());
    // Not recent and not open: its subfolder may be linked read-only.
    env.allowed("~/work/other/files", "read", vec![]);
}

#[test]
fn write_access_is_refused_inside_any_folder_holding_company_yaml() {
    let env = Env::new();
    let other = env.home_dir("someproj");
    write(&other, "company.yaml", COMPANY_V2);
    env.home_dir("someproj/data/deeper");
    env.home_dir("plain/data");
    for path in ["~/someproj/data", "~/someproj/data/deeper"] {
        let f = env.refused(path, "read_write", vec![]);
        assert!(f.message.contains("project"), "{path}: {f:#?}");
        assert!(f.fix.contains("access: read"), "{path}: {}", f.fix);
        env.allowed(path, "read", vec![]);
    }
    env.allowed("~/plain/data", "read_write", vec![]);
}

// ------------------------------------------------------------- in-project

#[test]
fn a_read_only_project_subfolder_without_config_names_validates() {
    let env = Env::new();
    let inbox = env.project_dir("files/in/sub");
    std::fs::write(inbox.join("a.pdf"), b"%PDF-1.4\n").unwrap();
    std::fs::write(env.project.join("files/in/notes.txt"), b"notes\n").unwrap();
    env.link("files/in", "read");
    let loaded = env.load(vec![]);
    assert!(
        loaded.findings.iter().all(|f| f.file != SEAT),
        "{:#?}",
        loaded.findings
    );
    let snap = loaded.snapshot.expect("no errors gives a snapshot");
    assert_eq!(snap.seats()["keeper"].folders[0].path, "files/in");
}

#[test]
fn a_project_subfolder_linked_with_write_access_is_refused() {
    let env = Env::new();
    env.project_dir("files/in");
    let f = env.refused("files/in", "read_write", vec![]);
    assert!(f.fix.contains("access: read"), "{}", f.fix);
}

#[test]
fn a_config_bearing_project_path_is_refused_naming_the_name() {
    let env = Env::new();
    env.project_dir("runtime-seed/knowledge");
    env.project_dir("files/Seats/x");
    for (path, name) in [
        ("seats", "seats"),
        ("runtime-seed", "runtime-seed"),
        ("runtime-seed/knowledge", "runtime-seed"),
        ("files/Seats/x", "seats"),
    ] {
        let f = env.refused(path, "read", vec![]);
        assert!(f.message.to_lowercase().contains(name), "{path}: {f:#?}");
        assert!(f.fix.contains("outside"), "{path}: {}", f.fix);
    }
}

#[test]
fn a_project_subfolder_that_gains_a_config_named_entry_is_refused_naming_it() {
    let seat_file = keeper("files/in", "read");
    // The runtime names come from am_domain's constants (the repo's
    // runtime-paths check refuses them as literals under crates/).
    let (open_marker, legacy) = (
        am_domain::runtime::OPEN_MARKER,
        am_domain::runtime::LEGACY_DIR,
    );
    let marker_entry = format!("sub/{open_marker}");
    let legacy_entry = format!("sub/{}/x", legacy.replacen("a", "A", 1));
    for (entry, name) in [
        ("seats/keeper.md", "seats"),
        ("deep/er/Company.YAML", "company.yaml"),
        ("tools.yaml", "tools.yaml"),
        (".git/HEAD", ".git"),
        (marker_entry.as_str(), open_marker),
        (legacy_entry.as_str(), legacy),
        ("models/accounts.yaml", "models"),
    ] {
        let env = Env::new();
        env.project_dir("files/in");
        env.allowed("files/in", "read", vec![]);
        write(&env.project.join("files/in"), entry, &seat_file);
        let f = env.refused("files/in", "read", vec![]);
        assert!(f.message.to_lowercase().contains(name), "{entry}: {f:#?}");
        assert!(f.fix.contains("move"), "{entry}: {}", f.fix);
    }
}

#[test]
fn a_project_subfolder_is_judged_by_where_it_lands_however_written() {
    let env = Env::in_home();
    env.project_dir("files/in");
    env.allowed("~/proj/files/in", "read", vec![]);
    env.refused("~/proj/files/in", "read_write", vec![]);
    env.refused("~/proj/seats", "read", vec![]);
    env.refused("~/proj", "read", vec![]);
    env.refused("../proj/seats", "read", vec![]);
    env.allowed("../proj/files/in", "read", vec![]);
}

// ------------------------------------------------------------- the walk

#[cfg(unix)]
#[test]
fn the_subtree_walk_reads_names_and_never_follows_a_link() {
    let env = Env::new();
    let inbox = env.project_dir("files/in");
    // A link to the project root: following it would find `seats`.
    std::os::unix::fs::symlink(&env.project, inbox.join("up")).unwrap();
    env.allowed("files/in", "read", vec![]);

    // A link whose own name is a config name is refused, wherever it points.
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), inbox.join("seats")).unwrap();
    let f = env.refused("files/in", "read", vec![]);
    assert!(f.message.contains("seats"), "{f:#?}");
}

#[cfg(unix)]
#[test]
fn a_walk_that_is_denied_refuses_the_link_in_words() {
    use std::os::unix::fs::PermissionsExt;
    let env = Env::new();
    let private = env.project_dir("files/in/private");
    std::fs::write(private.join("a.txt"), b"a\n").unwrap();
    std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o000)).unwrap();
    let readable = std::fs::read_dir(&private).is_ok();
    if !readable {
        let f = env.refused("files/in", "read", vec![]);
        assert!(f.message.to_lowercase().contains("cannot"), "{f:#?}");
        assert!(f.message.contains("private"), "{f:#?}");
    }
    std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o700)).unwrap();
    if readable {
        eprintln!("running as a user who reads any folder; the denied walk is not exercised");
    }
}

#[test]
fn a_walk_past_100000_entries_refuses_the_link_in_words() {
    let env = Env::new();
    let many = env.project_dir("files/in/many");
    for i in 0..100_001 {
        std::fs::File::create(many.join(format!("f{i}"))).unwrap();
    }
    let f = env.refused("files/in", "read", vec![]);
    assert!(f.message.contains("100,000"), "{f:#?}");
}

// ------------------------------------------------------------- config names

/// AD-30: the config names are exactly the folders and files the loader
/// reads, plus the runtime names (`.agentic`, `runtime-seed`,
/// `.agentic-open`) and `.git`.
#[test]
fn the_config_names_equal_what_the_loader_reads() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for rel in [
        "company.yaml",
        "tools.yaml",
        "models/accounts.yaml",
        "models/prices.yaml",
        "models/capabilities.yaml",
        "seats/a.md",
        "skills/b/SKILL.md",
        "workflows/w.yaml",
        "tables/t.yaml",
        "schemas/s.json",
        "dashboards/d.yaml",
        "sources/s.yaml",
        // Not config: an owner's folder and a stray file.
        "files/seats.md",
        "files/notes.yaml",
        "notes.txt",
    ] {
        write(root, rel, "schema_version: 2\n");
    }
    let read: BTreeSet<String> = config_files(root)
        .iter()
        .map(|f| f.split('/').next().unwrap().to_string())
        .collect();
    let runtime: BTreeSet<String> = [
        am_domain::runtime::LEGACY_DIR,
        am_domain::runtime::RUNTIME_SEED,
        am_domain::runtime::OPEN_MARKER,
        ".git",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let names: BTreeSet<String> = am_domain::CONFIG_NAMES
        .iter()
        .map(|s| s.to_string())
        .collect();
    let want: BTreeSet<String> = read.union(&runtime).cloned().collect();
    assert_eq!(names, want);
    assert!(
        am_domain::runtime::seed_knowledge(Path::new("/p"))
            .starts_with(Path::new("/p").join(am_domain::runtime::RUNTIME_SEED))
    );
}
