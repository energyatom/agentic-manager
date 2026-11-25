//! `register` (story 3.25): checks `docs/deferred-register.md` against the
//! ticket tree. Offline and read-only: it reads the register file and the
//! ticket tree ([`crate::stories`]) and writes nothing.
//!
//! **Normal mode** (`cargo xtask register`, in `just check`) fails on a
//! malformed table or row, a duplicate id, an unknown Kind, Must close or
//! Status, or a Target naming a story the tree does not have, and lists
//! every entry whose Must close is still marked `(assumed)` as a warning on
//! stderr, by id, without failing.
//!
//! **Release mode** (`--release --gate <epic>.<id>`, run by the release gate
//! stories 4.13 and 11.8) keeps every normal finding and also fails on every
//! entry still marked `(assumed)`, whatever its Status, and on every `open`
//! entry with Must close `yes` (an assumed yes counts) that is due by the
//! gate: its Target is `owner`, `unassigned` or `none`, or no story it names
//! builds after the gate (the gate story itself is due). An open must-close
//! entry naming a story that builds after the gate is carried forward:
//! listed on stdout, never a finding. Build order is table order, never id
//! order. There is no switch that turns a warning into a failure or a
//! release finding into a warning.
//!
//! The table is the first non-blank line after `## Register` (the exact
//! header), its separator, and every following line that starts with `|`.
//! A second `## Register`, a missing separator, a non-blank line straight
//! after the last row (GitHub renders it as a row), and any `|` line after
//! the table's end are findings, so no row slips past the check.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::Failure;
use crate::stories::{self, Book};

/// The register table's columns, in order.
const COLUMNS: [&str; 8] = [
    "Id",
    "Kind",
    "Source",
    "Item",
    "Decision",
    "Target",
    "Must close",
    "Status",
];
const KINDS: [&str; 4] = ["deferred", "accepted risk", "residual", "owner question"];
const MUST_CLOSE: [&str; 4] = ["yes", "no", "yes (assumed)", "no (assumed)"];
const STATUSES: [&str; 3] = ["open", "accepted", "closed"];
const TARGET_FORMS: &str = "owner, unassigned, none, or <epic>.<id> stories separated by commas";

/// What the task reads, and how.
pub struct Inputs {
    /// The register file.
    pub register: PathBuf,
    /// The ticket tree's root (it holds the top-level `tickets.toml`).
    pub tickets: PathBuf,
    /// Paths under this folder are shown relative to it.
    pub display_root: PathBuf,
    /// Release mode's gate story, `(epic, id)`; `None` is normal mode.
    pub release: Option<(u32, u32)>,
}

/// `--release` and `--gate` as given: both or neither, and the gate is
/// `<epic>.<id>`.
pub fn gate(release: bool, gate: Option<&str>) -> Result<Option<(u32, u32)>, String> {
    match (release, gate) {
        (false, None) => Ok(None),
        (true, None) => Err("--release needs --gate <epic>.<id>, the release gate story".into()),
        (false, Some(g)) => Err(format!(
            "--gate {g} needs --release; the normal mode takes no gate"
        )),
        (true, Some(g)) => stories::parse_story(g).map(Some).ok_or_else(|| {
            format!("--gate `{g}` is not <epic>.<id> (two positive integers joined by a dot)")
        }),
    }
}

/// Runs the check. Warnings (normal mode) go to stderr and carried-forward
/// lines (release mode) to stdout before it returns; findings and the
/// summary are printed by `main`.
pub fn run(inputs: &Inputs) -> Result<String, Failure> {
    let shown = |path: &Path| -> String {
        path.strip_prefix(&inputs.display_root)
            .unwrap_or(path)
            .display()
            .to_string()
    };
    let file = shown(&inputs.register);
    let text = std::fs::read_to_string(&inputs.register)
        .map_err(|e| Failure::Setup(format!("{file}: cannot read the register: {e}")))?;
    let book = stories::load(&inputs.tickets, &shown).map_err(Failure::Setup)?;
    if let Some(g) = inputs.release
        && let Some(why) = book.missing(g)
    {
        return Err(Failure::Setup(format!(
            "--gate {}.{}: the gate is not a story of the ticket tree: {why}\n{}",
            g.0,
            g.1,
            crate::USAGE
        )));
    }

    let table = parse_table(&text);
    let mut findings = table.findings;
    let mut checked = Vec::new();
    let mut first_at: HashMap<&str, usize> = HashMap::new();
    for row in &table.rows {
        let (entry, mut found) = check_row(row, &book);
        if is_id(&row.cells[0]) {
            if let Some(first) = first_at.get(row.cells[0].as_str()) {
                found.insert(
                    0,
                    Finding::at(
                        row.line,
                        format!("{}: duplicate id, first at line {first}", row.label()),
                    ),
                );
            } else {
                first_at.insert(&row.cells[0], row.line);
            }
        }
        findings.extend(found);
        if let Some(entry) = entry {
            checked.push(entry);
        }
    }
    findings.sort_by_key(|f| f.line);

    let entries = table.rows.len();
    let assumed: Vec<&Entry> = checked.iter().filter(|e| e.must_close.assumed).collect();
    let summary = match inputs.release {
        None => {
            for e in &assumed {
                eprintln!(
                    "warning: {}: Must close is still marked (assumed): {}; Target {}",
                    e.label, e.must_close.text, e.target_text
                );
            }
            format!(
                "{entries} entries checked against {} stories in {} epics; {} still marked (assumed), listed as warnings",
                book.stories(),
                book.epics(),
                assumed.len()
            )
        }
        Some(g) => {
            let gate = Gate::new(g, &book);
            let (due, carried) = release(&checked, &book, &gate);
            for line in &carried {
                println!("{line}");
            }
            let due_count = due.iter().filter(|f| f.due).count();
            findings.extend(due.into_iter().map(|f| f.finding));
            format!(
                "release gate {}: {entries} entries; {} still marked (assumed); {due_count} due; {} carried forward",
                gate.name,
                assumed.len(),
                carried.len()
            )
        }
    };

    if findings.is_empty() {
        Ok(summary)
    } else {
        Err(Failure::Findings(
            findings
                .into_iter()
                .map(|f| match f.line {
                    Some(n) => format!("{file}:{n}: {}", f.words),
                    None => format!("{file}: {}", f.words),
                })
                .collect(),
        ))
    }
}

/// One finding: its line in the register (none for the file as a whole)
/// and its words.
struct Finding {
    line: Option<usize>,
    words: String,
}

impl Finding {
    fn at(line: usize, words: String) -> Self {
        Finding {
            line: Some(line),
            words,
        }
    }
}

/// One row of the register table: its line and its trimmed cells.
struct Row {
    line: usize,
    cells: Vec<String>,
}

impl Row {
    /// How a finding names the row: its id, or its first cell quoted when
    /// that is not an id (the finding's `<file>:<line>` names the line).
    fn label(&self) -> String {
        let first = self.cells.first().map(String::as_str).unwrap_or_default();
        if is_id(first) {
            first.to_string()
        } else {
            format!("row with Id `{first}`")
        }
    }
}

/// The register table: its rows and the findings about its shape.
struct Table {
    rows: Vec<Row>,
    findings: Vec<Finding>,
}

/// Finds the table under `## Register` and splits its rows.
fn parse_table(text: &str) -> Table {
    let lines: Vec<&str> = text.lines().collect();
    let mut findings = Vec::new();
    let headings: Vec<usize> = (0..lines.len())
        .filter(|&i| lines[i].trim() == "## Register")
        .collect();
    let Some(&heading) = headings.first() else {
        findings.push(Finding {
            line: None,
            words: "no \"## Register\" table".into(),
        });
        return Table {
            rows: Vec::new(),
            findings,
        };
    };
    for &again in &headings[1..] {
        findings.push(Finding::at(
            again + 1,
            format!(
                "a second \"## Register\" heading; the register table is the one under line {}",
                heading + 1
            ),
        ));
    }

    let mut i = heading + 1;
    while i < lines.len() && lines[i].trim().is_empty() {
        i += 1;
    }
    let header_words = format!(
        "the register table's header is not | {} |",
        COLUMNS.join(" | ")
    );
    let mut rows = Vec::new();
    // The table's last line (1-based); a `|` line after it is a stray row.
    let end = if i == lines.len() {
        findings.push(Finding::at(
            heading + 1,
            "no table follows \"## Register\"".into(),
        ));
        heading + 1
    } else if !is_table_line(lines[i]) {
        findings.push(Finding::at(i + 1, header_words));
        i + 1
    } else {
        if split_row(lines[i]) != COLUMNS {
            findings.push(Finding::at(i + 1, header_words));
        }
        let mut last = i + 1;
        i += 1;
        if i < lines.len() && is_separator(lines[i]) {
            last = i + 1;
            i += 1;
        } else {
            findings.push(Finding::at(
                last,
                format!(
                    "the register table's header is not followed by its separator row: {} cells of dashes, such as |----|",
                    COLUMNS.len()
                ),
            ));
        }
        while i < lines.len() && is_table_line(lines[i]) {
            rows.push(Row {
                line: i + 1,
                cells: split_row(lines[i]),
            });
            last = i + 1;
            i += 1;
        }
        // GitHub renders a non-blank line straight after the last row as
        // one more row, even without its leading `|` (review R1).
        if i < lines.len() && !lines[i].trim().is_empty() {
            findings.push(Finding::at(
                i + 1,
                format!(
                    "a line straight after the register table's last line ({last}) is rendered as one more row; start it with `|` or leave a blank line before it"
                ),
            ));
        }
        last
    };
    for (j, line) in lines.iter().enumerate().skip(end) {
        if is_table_line(line) {
            findings.push(Finding::at(
                j + 1,
                format!("a table row after the register table ended at line {end}"),
            ));
        }
    }
    Table { rows, findings }
}

fn is_table_line(line: &str) -> bool {
    line.trim_start().starts_with('|')
}

/// A separator row: as many cells as the header, each dashes with an
/// optional `:` at either end.
fn is_separator(line: &str) -> bool {
    let cells = split_row(line);
    cells.len() == COLUMNS.len()
        && cells.iter().all(|c| {
            let c = c.strip_prefix(':').unwrap_or(c);
            let c = c.strip_suffix(':').unwrap_or(c);
            !c.is_empty() && c.bytes().all(|b| b == b'-')
        })
}

/// Splits a table line into trimmed cells: one leading and one trailing `|`
/// are dropped, cells split at every `|` not preceded by `\`, and `\|` is a
/// literal pipe. Code spans get no special treatment (GFM).
fn split_row(line: &str) -> Vec<String> {
    let line = line.trim();
    let line = line.strip_prefix('|').unwrap_or(line);
    let line = match line.strip_suffix('|') {
        Some(rest) if !rest.ends_with('\\') => rest,
        _ => line,
    };
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                cell.push('|');
                chars.next();
            }
            '|' => cells.push(std::mem::take(&mut cell).trim().to_string()),
            _ => cell.push(c),
        }
    }
    cells.push(cell.trim().to_string());
    cells
}

/// `DR-` and three digits (`DR-001` to `DR-999`), or more digits with no
/// leading zero (`DR-1000`).
fn is_id(cell: &str) -> bool {
    let Some(digits) = cell.strip_prefix("DR-") else {
        return false;
    };
    let all_digits = !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit());
    all_digits
        && match digits.len() {
            0..=2 => false,
            3 => digits != "000",
            _ => !digits.starts_with('0'),
        }
}

/// A Target cell, parsed.
enum Target {
    Owner,
    Unassigned,
    Nothing,
    Stories(Vec<(u32, u32)>),
}

/// A Must close cell, parsed.
struct MustClose {
    yes: bool,
    assumed: bool,
    text: String,
}

#[derive(PartialEq)]
enum Status {
    Open,
    Accepted,
    Closed,
}

/// A row whose Must close parsed; the release mode reads it.
struct Entry {
    line: usize,
    label: String,
    target_text: String,
    /// `None` when the Target is malformed or names a missing story.
    target: Option<Target>,
    must_close: MustClose,
    /// `None` when the Status is unknown.
    status: Option<Status>,
}

/// Every normal finding on one row, and the row's entry when its Must close
/// parsed.
fn check_row(row: &Row, book: &Book) -> (Option<Entry>, Vec<Finding>) {
    let label = row.label();
    let mut found = Vec::new();
    let mut say = |words: String| found.push(Finding::at(row.line, format!("{label}: {words}")));
    if row.cells.len() != COLUMNS.len() {
        say(format!(
            "has {} cells; the header defines {} ({})",
            row.cells.len(),
            COLUMNS.len(),
            COLUMNS.join(", ")
        ));
        return (None, found);
    }
    let cell = |column: &str| -> &str {
        let at = COLUMNS
            .iter()
            .position(|c| *c == column)
            .unwrap_or_default();
        &row.cells[at]
    };

    if !is_id(cell("Id")) {
        say(format!(
            "Id `{}` is not DR- and three digits (DR-001 to DR-999), or more digits with no leading zero",
            cell("Id")
        ));
    }
    let kind = cell("Kind");
    if !KINDS.contains(&kind) {
        say(format!(
            "unknown Kind `{kind}`; allowed: {}",
            KINDS.join(", ")
        ));
    }
    let source = cell("Source");
    if !source_starts_with_story(source) {
        say(format!(
            "Source `{source}` does not start with the <epic>.<id> story whose plan recorded it"
        ));
    }
    for column in ["Item", "Decision"] {
        if cell(column).is_empty() {
            say(format!("{column} is empty"));
        }
    }

    let target_text = cell("Target");
    let target = match parse_target(target_text) {
        Err(why) => {
            say(format!(
                "Target `{target_text}` {why}; allowed: {TARGET_FORMS}"
            ));
            None
        }
        Ok(Target::Stories(list)) => {
            let missing: Vec<String> = list.iter().filter_map(|s| book.missing(*s)).collect();
            if missing.is_empty() {
                Some(Target::Stories(list))
            } else {
                say(format!("Target names {}", missing.join("; and names ")));
                None
            }
        }
        Ok(keyword) => Some(keyword),
    };

    let must = cell("Must close");
    let must_close = if MUST_CLOSE.contains(&must) {
        Some(MustClose {
            yes: must.starts_with("yes"),
            assumed: must.ends_with(" (assumed)"),
            text: must.to_string(),
        })
    } else {
        say(format!(
            "unknown Must close `{must}`; allowed: {}",
            MUST_CLOSE.join(", ")
        ));
        None
    };

    let status = match cell("Status") {
        "open" => Some(Status::Open),
        "accepted" => Some(Status::Accepted),
        "closed" => Some(Status::Closed),
        other => {
            say(format!(
                "unknown Status `{other}`; allowed: {}",
                STATUSES.join(", ")
            ));
            None
        }
    };

    let entry = must_close.map(|must_close| Entry {
        line: row.line,
        label: label.clone(),
        target_text: target_text.to_string(),
        target,
        must_close,
        status,
    });
    (entry, found)
}

/// A Source starts with `<epic>.<id>`, followed by its end, a space, `,`,
/// `;` or `(`.
fn source_starts_with_story(source: &str) -> bool {
    let end = source.find([' ', ',', ';', '(']).unwrap_or(source.len());
    stories::parse_story(&source[..end]).is_some()
}

/// Exactly `owner`, `unassigned` or `none`, or stories separated by commas
/// (spaces allowed): no empty element, and no keyword among stories.
fn parse_target(text: &str) -> Result<Target, String> {
    match text {
        "owner" => return Ok(Target::Owner),
        "unassigned" => return Ok(Target::Unassigned),
        "none" => return Ok(Target::Nothing),
        _ => {}
    }
    let mut list = Vec::new();
    for element in text.split(',').map(str::trim) {
        if element.is_empty() {
            return Err("has an empty element".into());
        }
        if ["owner", "unassigned", "none"].contains(&element) {
            return Err(format!("mixes the keyword `{element}` with stories"));
        }
        let story = stories::parse_story(element)
            .ok_or_else(|| format!("holds `{element}`, which is not <epic>.<id>"))?;
        list.push(story);
    }
    Ok(Target::Stories(list))
}

/// The gate story: its name and build position.
struct Gate {
    name: String,
    position: usize,
}

impl Gate {
    fn new((epic, id): (u32, u32), book: &Book) -> Self {
        Gate {
            name: format!("{epic}.{id}"),
            position: book
                .position((epic, id))
                .expect("run refuses a gate the book does not hold"),
        }
    }
}

/// A release finding; `due` counts it in the summary.
struct ReleaseFinding {
    finding: Finding,
    due: bool,
}

/// The release mode's findings (every assumed entry, then due ones, in
/// register order) and its carried-forward lines.
fn release(entries: &[Entry], book: &Book, gate: &Gate) -> (Vec<ReleaseFinding>, Vec<String>) {
    let mut found = Vec::new();
    let mut carried = Vec::new();
    for e in entries {
        if e.must_close.assumed {
            found.push(ReleaseFinding {
                finding: Finding::at(
                    e.line,
                    format!(
                        "{}: still marked (assumed) (Must close `{}`, Target {}): gate {} needs it confirmed",
                        e.label, e.must_close.text, e.target_text, gate.name
                    ),
                ),
                due: false,
            });
        }
        if e.status != Some(Status::Open) || !e.must_close.yes {
            continue;
        }
        let reason = match &e.target {
            None => continue,
            Some(Target::Owner) => "a decision is pending",
            Some(Target::Unassigned) => "no story is named yet",
            Some(Target::Nothing) => "names no story, so nothing can close it",
            Some(Target::Stories(list)) => {
                let after: Vec<String> = list
                    .iter()
                    .filter(|s| book.position(**s).is_some_and(|p| p > gate.position))
                    .map(|(epic, id)| format!("{epic}.{id}"))
                    .collect();
                if !after.is_empty() {
                    carried.push(format!(
                        "carried forward past gate {}: {} (Target {}; after the gate: {})",
                        gate.name,
                        e.label,
                        e.target_text,
                        after.join(", ")
                    ));
                    continue;
                }
                "every story it names builds by the gate"
            }
        };
        found.push(ReleaseFinding {
            finding: Finding::at(
                e.line,
                format!(
                    "{}: due by gate {}: open with Must close `{}` and Target {}: {reason}",
                    e.label, gate.name, e.must_close.text, e.target_text
                ),
            ),
            due: true,
        });
    }
    (found, carried)
}
