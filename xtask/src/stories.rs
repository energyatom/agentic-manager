//! The story book (story 3.25): which `<epic>.<id>` stories exist, and the
//! build position of each, read from the ticket tree.
//!
//! The top-level `tickets.toml` gives each `[[epic]]` table's `id` and
//! `slug`, in table order; each epic's `<slug>/tickets/tickets.toml` gives
//! each `[[entry]]` table's `id`, in table order. Every entry counts, of
//! any `type`. Build order is table order (epics in their table order, each
//! epic's entries in theirs); ids are never sorted or compared.
//!
//! There is no TOML crate: a strict line scan reads the two keys, and
//! anything it cannot read exactly is refused with `<file>:<line>: <what>`
//! (the caller exits 2). A multi-line string (`"""` or `'''`, outside a
//! comment line), a `[` line that is not a plain `[[name]]` or `[name]`
//! header, a table with no `id` or two, an `[[epic]]` with no `slug` or
//! two, a slug that is not one plain folder name or is another epic's, an
//! id that is not a positive integer, a duplicate id, and a missing file
//! or folder are refused. A quoted or dotted key (`"id" = 3`) is not read, so its
//! table has no id and is refused too. Nothing is guessed.

use std::collections::HashMap;
use std::path::Path;

/// The stories of the ticket tree, in build order.
pub struct Book {
    /// Every story's position in build order.
    position: HashMap<(u32, u32), usize>,
    /// The epic ids, in table order.
    epics: Vec<u32>,
}

impl Book {
    /// The build position of story `(epic, id)`, if it exists.
    pub fn position(&self, story: (u32, u32)) -> Option<usize> {
        self.position.get(&story).copied()
    }

    /// Whether the top-level `tickets.toml` has an `[[epic]]` with this id.
    pub fn has_epic(&self, epic: u32) -> bool {
        self.epics.contains(&epic)
    }

    /// How many stories the book holds.
    pub fn stories(&self) -> usize {
        self.position.len()
    }

    /// How many epics the book holds.
    pub fn epics(&self) -> usize {
        self.epics.len()
    }

    /// Why `story` is not in the book, in words; `None` when it is.
    pub fn missing(&self, (epic, id): (u32, u32)) -> Option<String> {
        if self.position((epic, id)).is_some() {
            None
        } else if self.has_epic(epic) {
            Some(format!(
                "{epic}.{id}, which is not an entry of epic {epic}'s tickets.toml"
            ))
        } else {
            Some(format!(
                "{epic}.{id}: there is no epic {epic} in tickets.toml"
            ))
        }
    }
}

/// `<epic>.<id>`: two positive decimal integers with no sign and no leading
/// zero, joined by one dot.
pub fn parse_story(text: &str) -> Option<(u32, u32)> {
    let (epic, id) = text.split_once('.')?;
    Some((positive(epic)?, positive(id)?))
}

/// A positive decimal integer: ASCII digits, no leading zero.
fn positive(text: &str) -> Option<u32> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) || text.starts_with('0') {
        return None;
    }
    text.parse().ok()
}

/// Reads the ticket tree at `tickets`. Paths in the words are shown by
/// `shown`.
pub fn load(tickets: &Path, shown: &dyn Fn(&Path) -> String) -> Result<Book, String> {
    let top_path = tickets.join("tickets.toml");
    let top_file = shown(&top_path);
    let top = std::fs::read_to_string(&top_path)
        .map_err(|e| format!("{top_file}: cannot read the ticket tree's tickets.toml: {e}"))?;
    let epics = scan(&top, &top_file, "epic")?;

    let mut position = HashMap::new();
    let mut ids = Vec::new();
    for epic in &epics {
        let (slug, slug_line) = epic
            .slug
            .clone()
            .expect("scan refuses an [[epic]] with no slug");
        let path = tickets.join(&slug).join("tickets").join("tickets.toml");
        let file = shown(&path);
        let text = std::fs::read_to_string(&path).map_err(|e| {
            format!(
                "{top_file}:{slug_line}: epic {}'s folder `{slug}` has no readable {file}: {e}",
                epic.id
            )
        })?;
        for entry in scan(&text, &file, "entry")? {
            let at = position.len();
            position.insert((epic.id, entry.id), at);
        }
        ids.push(epic.id);
    }
    Ok(Book {
        position,
        epics: ids,
    })
}

/// One `[[epic]]` or `[[entry]]` table, as the scan read it.
struct Table {
    id: u32,
    /// The slug and its line (`[[epic]]` tables only).
    slug: Option<(String, usize)>,
}

/// A table while the scan is inside it.
struct Open {
    line: usize,
    id: Option<(u32, usize)>,
    slug: Option<(String, usize)>,
}

/// The `[[<name>]]` tables of one file, in table order, each with its id
/// (and, for `epic`, its slug); every other table is skipped.
fn scan(text: &str, file: &str, name: &str) -> Result<Vec<Table>, String> {
    let mut tables: Vec<Table> = Vec::new();
    let mut first_line: HashMap<u32, usize> = HashMap::new();
    let mut slugs: HashMap<String, usize> = HashMap::new();
    let mut open: Option<Open> = None;
    let finish = |open: Open,
                  tables: &mut Vec<Table>,
                  first_line: &mut HashMap<u32, usize>|
     -> Result<(), String> {
        let Some((id, id_line)) = open.id else {
            return Err(format!(
                "{file}:{}: this [[{name}]] has no `id = <n>` line",
                open.line
            ));
        };
        if name == "epic" && open.slug.is_none() {
            return Err(format!(
                "{file}:{}: this [[epic]] has no `slug = \"…\"` line",
                open.line
            ));
        }
        if let Some(first) = first_line.insert(id, id_line) {
            return Err(format!(
                "{file}:{id_line}: [[{name}]] id {id} appears twice; the first is at line {first}"
            ));
        }
        tables.push(Table {
            id,
            slug: open.slug,
        });
        Ok(())
    };

    for (index, raw) in text.lines().enumerate() {
        let n = index + 1;
        let line = raw.trim();
        // A comment opens no string, so it is skipped first (review R2).
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.contains("\"\"\"") || line.contains("'''") {
            return Err(format!(
                "{file}:{n}: a multi-line string (\"\"\" or '''), which the line scan cannot read"
            ));
        }
        if line.starts_with('[') && table_header(line).is_none() {
            return Err(format!(
                "{file}:{n}: `{line}` is not a table header the line scan reads: [[name]] or [name], the name bare keys joined by dots"
            ));
        }
        if let Some(header) = table_header(line) {
            if let Some(done) = open.take() {
                finish(done, &mut tables, &mut first_line)?;
            }
            if header == format!("[[{name}]]") {
                open = Some(Open {
                    line: n,
                    id: None,
                    slug: None,
                });
            }
            continue;
        }
        let Some(table) = open.as_mut() else {
            continue;
        };
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "id" => {
                if let Some((_, first)) = table.id {
                    return Err(format!(
                        "{file}:{n}: a second `id` in the [[{name}]] at line {}; the first is at line {first}",
                        table.line
                    ));
                }
                let value = without_comment(value);
                let id = positive(value)
                    .ok_or_else(|| format!("{file}:{n}: id `{value}` is not a positive integer"))?;
                table.id = Some((id, n));
            }
            "slug" if name == "epic" => {
                if let Some((_, first)) = table.slug {
                    return Err(format!(
                        "{file}:{n}: a second `slug` in the [[epic]] at line {}; the first is at line {first}",
                        table.line
                    ));
                }
                let slug = basic_string(value)
                    .filter(|s| is_folder_name(s))
                    .ok_or_else(|| {
                        format!(
                            "{file}:{n}: slug {} is not one plain folder name: a \"…\" string of letters, digits, `-`, `_` and `.`, not starting with a dot",
                            value.trim()
                        )
                    })?;
                if let Some(first) = slugs.insert(slug.clone(), n) {
                    return Err(format!(
                        "{file}:{n}: slug `{slug}` is already the slug at line {first}; two epics never share a folder"
                    ));
                }
                table.slug = Some((slug, n));
            }
            _ => {}
        }
    }
    if let Some(done) = open.take() {
        finish(done, &mut tables, &mut first_line)?;
    }
    Ok(tables)
}

/// The header of a table line, `[[name]]` or `[name]` with the spaces inside
/// the brackets removed, when `line` is one: the name has bare-key
/// characters and dots, and a `#` comment may follow.
fn table_header(line: &str) -> Option<String> {
    let line = without_comment(line);
    let (open, close, inner) = if let Some(rest) = line.strip_prefix("[[") {
        ("[[", "]]", rest.strip_suffix("]]")?)
    } else {
        ("[", "]", line.strip_prefix('[')?.strip_suffix(']')?)
    };
    let name = inner.trim();
    let bare = |key: &str| {
        !key.is_empty()
            && key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    };
    name.split('.')
        .all(bare)
        .then(|| format!("{open}{name}{close}"))
}

/// One plain folder name (review R3): letters, digits, `-`, `_` and `.`,
/// not starting with a dot, so never `.`, `..`, a path or a hidden folder.
fn is_folder_name(slug: &str) -> bool {
    !slug.starts_with('.')
        && slug
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

/// `text` up to a `#` comment, trimmed (for a value that holds no string).
fn without_comment(text: &str) -> &str {
    text.split('#').next().unwrap_or_default().trim()
}

/// The content of a basic string `"…"` with no escape and no quote inside,
/// followed by nothing or a `#` comment; `None` for anything else or an
/// empty string.
fn basic_string(value: &str) -> Option<String> {
    let rest = value.trim().strip_prefix('"')?;
    let (content, after) = rest.split_once('"')?;
    let after = after.trim();
    let clean = !content.is_empty() && !content.contains('\\');
    (clean && (after.is_empty() || after.starts_with('#'))).then(|| content.to_string())
}
