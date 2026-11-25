//! Story 3.25: `cargo xtask register` checks `docs/deferred-register.md`
//! against the ticket tree. Normal mode (in `just check`) fails on a
//! malformed row, a duplicate id, an unknown Kind, Must close or Status, or
//! a Target naming no story, and lists every `(assumed)` entry as a warning
//! by id. Release mode (`--release --gate <epic>.<id>`, run by stories 4.13
//! and 11.8) also fails on every `(assumed)` entry and on every open
//! must-close entry due by the gate, and lists the ones carried forward.
//! Build order is table order, never id order. Written first, from the
//! plan's I/O matrix (rows named on each test); the planted register files
//! and ticket tree live in `xtask/tests/fixtures/register/`.
#![forbid(unsafe_code)]

mod common;

use std::path::{Path, PathBuf};
use std::process::Output;

use common::{repo_root, stderr, stdout, write, xtask};

const FIXTURES: &str = "xtask/tests/fixtures/register";

/// The planted tree's files, relative to its root.
const TREE_FILES: [&str; 4] = [
    "tickets.toml",
    "epic-one/tickets/tickets.toml",
    "epic-three/tickets/tickets.toml",
    "epic-two/tickets/tickets.toml",
];

fn fixture(name: &str) -> PathBuf {
    repo_root().join(FIXTURES).join(name)
}

/// The planted ticket tree: build order 1.1, 1.2, 1.5, 1.3, 1.4, 3.1, 3.2,
/// 3.3, 2.1, 2.2.
fn tree() -> PathBuf {
    fixture("tickets")
}

fn text(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// `cargo xtask register --register <register> --tickets <tickets> <extra…>`.
fn run(register: &Path, tickets: &Path, extra: &[&str]) -> Output {
    let register = register.to_string_lossy().into_owned();
    let tickets = tickets.to_string_lossy().into_owned();
    let mut args = vec!["register", "--register", &register, "--tickets", &tickets];
    args.extend_from_slice(extra);
    xtask(&args)
}

/// Normal mode on the planted register file `name` and the planted tree.
fn check(name: &str) -> Output {
    run(&fixture(name), &tree(), &[])
}

/// Release mode at `gate` on the planted register file `name`.
fn release(name: &str, gate: &str) -> Output {
    run(&fixture(name), &tree(), &["--release", "--gate", gate])
}

/// The 1-based numbers of the lines of `text` for which `pick` holds.
fn lines_where(text: &str, pick: impl Fn(&str) -> bool) -> Vec<usize> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| pick(l))
        .map(|(i, _)| i + 1)
        .collect()
}

/// The lines of register file `name` that hold the row `id`, in order.
fn rows(name: &str, id: &str) -> Vec<usize> {
    let start = format!("| {id} |");
    let found = lines_where(&text(&fixture(name)), |l| l.starts_with(&start));
    assert!(!found.is_empty(), "{name} has a row {id}");
    found
}

/// The stderr lines that report on line `line` of register file `name`.
fn at<'e>(err: &'e str, name: &str, line: usize) -> Vec<&'e str> {
    let key = format!("/{name}:{line}: ");
    err.lines().filter(|l| l.contains(&key)).collect()
}

/// The findings on row `id` of register file `name`: each line reads
/// `<file>:<line>: <id>…`.
fn findings<'e>(err: &'e str, name: &str, id: &str) -> Vec<&'e str> {
    let line = rows(name, id)[0];
    let key = format!("/{name}:{line}: {id}");
    err.lines().filter(|l| l.contains(&key)).collect()
}

/// The count in main's `register: FAILED with <n> finding(s)` line.
fn failed(err: &str) -> Option<usize> {
    err.lines().find_map(|l| {
        l.strip_prefix("register: FAILED with ")?
            .strip_suffix(" finding(s)")?
            .parse()
            .ok()
    })
}

/// The id of every `warning:` line on stderr, in order.
fn warned_ids(err: &str) -> Vec<String> {
    err.lines()
        .filter_map(|l| l.strip_prefix("warning: "))
        .map(|rest| {
            rest.chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
                .collect()
        })
        .collect()
}

/// A temporary copy of the planted tree with every file passed through
/// `change` (its path relative to the tree, its text); `None` leaves the
/// file out. The copy's path is canonical, so the words name it as given.
fn copy_tree_with(change: impl Fn(&str, String) -> Option<String>) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    for rel in TREE_FILES {
        if let Some(changed) = change(rel, text(&tree().join(rel))) {
            write(tmp.path(), rel, &changed);
        }
    }
    let root = tmp.path().canonicalize().unwrap();
    (tmp, root)
}

/// `text` with its first line equal (trimmed) to `from` replaced by `to`
/// (no lines removes it).
fn replace_line(text: &str, from: &str, to: &[&str]) -> String {
    let mut done = false;
    let mut out = Vec::new();
    for line in text.lines() {
        if !done && line.trim() == from {
            done = true;
            out.extend(to.iter().map(|s| s.to_string()));
        } else {
            out.push(line.to_string());
        }
    }
    assert!(done, "the planted file has a line `{from}`");
    out.join("\n") + "\n"
}

/// The line of the nearest `header` line at or before line `line` of `text`.
fn header_before(text: &str, line: usize, header: &str) -> usize {
    lines_where(text, |l| l.trim() == header)
        .into_iter()
        .filter(|&n| n <= line)
        .max()
        .unwrap_or_else(|| panic!("a {header} before line {line}"))
}

/// The one line of `text` that equals `wanted` once trimmed.
fn line_of(text: &str, wanted: &str) -> usize {
    let found = lines_where(text, |l| l.trim() == wanted);
    assert_eq!(found.len(), 1, "one line `{wanted}`: {found:?}");
    found[0]
}

/// A change to one file of a copied tree (see [`copy_tree_with`]).
type Change = Box<dyn Fn(&str, String) -> Option<String>>;

/// Replaces the line `from` of the tree file `file` with `to`.
fn edit(file: &'static str, from: &'static str, to: &'static [&'static str]) -> Change {
    Box::new(move |rel: &str, t: String| {
        Some(if rel == file {
            replace_line(&t, from, to)
        } else {
            t
        })
    })
}

/// The body lines of `recipe` in the justfile (until the next unindented line).
fn recipe<'j>(justfile: &'j str, name: &str) -> Option<Vec<&'j str>> {
    let mut lines = justfile.lines();
    lines.find(|l| l.starts_with(&format!("{name}:")))?;
    Some(
        lines
            .take_while(|l| l.starts_with(' ') || l.starts_with('\t'))
            .map(str::trim)
            .collect(),
    )
}

// ------------------------------------------------------------ normal mode

/// Row 1: today's register passes; one warning per `(assumed)` entry, in
/// register order, and nothing else on stderr. The expected set and counts
/// come from the files themselves, so the test stays green as the owner
/// confirms entries.
#[test]
fn the_repository_register_passes_and_warns_each_assumed_entry() {
    let root = repo_root();
    let register = text(&root.join("docs/deferred-register.md"));
    let table: Vec<Vec<String>> = register
        .lines()
        .skip_while(|l| l.trim() != "## Register")
        .filter(|l| l.starts_with("| DR-"))
        .map(|l| l.split('|').map(|c| c.trim().to_string()).collect())
        .collect();
    assert!(!table.is_empty(), "the register has rows");
    let assumed: Vec<String> = table
        .iter()
        .filter(|cells| cells[7].ends_with(" (assumed)"))
        .map(|cells| cells[1].clone())
        .collect();

    // Review R5: counted without the scan's rules (no slug, no table
    // header). The epics are the folders that hold `tickets/tickets.toml`;
    // the stories are the lines of those files that are exactly `id = `
    // and digits. The top-level file's own `id = ` lines cross-check the
    // folder count.
    let tickets = root.join("_bmad-output/initiative-agentic-manager/tickets");
    let id_lines = |text: &str| {
        text.lines()
            .filter(|l| {
                l.strip_prefix("id = ")
                    .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
            })
            .count()
    };
    let epic_files: Vec<PathBuf> = std::fs::read_dir(&tickets)
        .unwrap()
        .map(|e| e.unwrap().path().join("tickets/tickets.toml"))
        .filter(|p| p.is_file())
        .collect();
    let epics = epic_files.len();
    let stories: usize = epic_files.iter().map(|p| id_lines(&text(p))).sum();
    assert_eq!(
        id_lines(&text(&tickets.join("tickets.toml"))),
        epics,
        "every epic id in tickets.toml has its folder, and no folder is extra"
    );
    assert!(
        epics > 0 && stories > 0,
        "the ticket tree has epics and stories"
    );

    let out = xtask(&["register"]);
    let err = stderr(&out);
    assert_eq!(out.status.code(), Some(0), "today's register passes: {err}");
    for line in err.lines() {
        assert!(
            line.starts_with("warning: DR-"),
            "stderr holds only the assumed warnings: {line}"
        );
    }
    assert_eq!(
        warned_ids(&err),
        assumed,
        "one warning per (assumed) entry, in register order"
    );
    assert_eq!(
        stdout(&out).trim_end(),
        format!(
            "register: {} entries checked against {stories} stories in {epics} epics; {} still marked (assumed), listed as warnings",
            table.len(),
            assumed.len()
        )
    );
}

/// Row 2: every Kind, Status and Target form, every value confirmed: no
/// warning. Spike 1.1 is a story, and so are 3.2 and 2.2 (epics listed out
/// of id order).
#[test]
fn a_register_with_every_value_confirmed_passes_with_no_warning() {
    let out = check("clean.md");
    let err = stderr(&out);
    assert_eq!(out.status.code(), Some(0), "clean.md passes: {err}");
    assert!(err.trim().is_empty(), "no warning and no finding: {err}");
    assert_eq!(
        stdout(&out).trim_end(),
        "register: 7 entries checked against 10 stories in 3 epics; 0 still marked (assumed), listed as warnings"
    );
}

/// Row 3: an `(assumed)` Must close is a warning naming the id, its value
/// and its Target, and the check still passes.
#[test]
fn an_assumed_entry_is_a_warning_by_id_and_still_passes() {
    let out = check("assumed.md");
    let err = stderr(&out);
    assert_eq!(out.status.code(), Some(0), "an assumed entry passes: {err}");
    assert_eq!(warned_ids(&err), vec!["DR-003", "DR-005"], "{err}");
    let lines: Vec<&str> = err.lines().collect();
    assert_eq!(lines.len(), 2, "nothing but the two warnings: {err}");
    for (line, id, value, target) in [
        (lines[0], "DR-003", "yes (assumed)", "1.5, 2.1"),
        (lines[1], "DR-005", "no (assumed)", "unassigned"),
    ] {
        assert!(line.contains(id), "{line}");
        assert!(
            line.contains(value),
            "names its Must close `{value}`: {line}"
        );
        assert!(line.contains(target), "names its Target `{target}`: {line}");
    }
    assert_eq!(
        stdout(&out).trim_end(),
        "register: 7 entries checked against 10 stories in 3 epics; 2 still marked (assumed), listed as warnings"
    );
}

/// Row 4: a Target naming no story fails, one finding per entry, naming
/// the id and the story; the other rows pass. The comment `# id = 9` in
/// the planted epic 1 is not an entry.
#[test]
fn a_target_naming_no_story_fails_with_the_entry_id() {
    let name = "no-such-story.md";
    let out = check(name);
    let err = stderr(&out);
    assert_eq!(out.status.code(), Some(1), "fails with findings: {err}");
    assert_eq!(failed(&err), Some(3), "one finding per entry: {err}");
    for (id, words) in [
        (
            "DR-002",
            "Target names 1.9, which is not an entry of epic 1's tickets.toml",
        ),
        (
            "DR-003",
            "Target names 4.1: there is no epic 4 in tickets.toml",
        ),
        (
            "DR-004",
            "Target names 2.7, which is not an entry of epic 2's tickets.toml",
        ),
    ] {
        let found = findings(&err, name, id);
        assert_eq!(found.len(), 1, "{id}: one finding: {err}");
        assert!(found[0].contains(words), "{id}: `{words}`: {err}");
    }
    for id in ["DR-001", "DR-005"] {
        assert!(findings(&err, name, id).is_empty(), "{id} passes: {err}");
    }
}

/// Row 5: a row with 7 or 9 cells fails by id; its cells are not checked
/// further.
#[test]
fn a_row_missing_or_adding_a_column_fails_with_the_entry_id() {
    let name = "missing-column.md";
    let out = check(name);
    let err = stderr(&out);
    assert_eq!(out.status.code(), Some(1), "fails with findings: {err}");
    assert_eq!(failed(&err), Some(2), "one finding per row: {err}");
    for (id, cells) in [("DR-002", 7), ("DR-003", 9)] {
        let found = findings(&err, name, id);
        assert_eq!(found.len(), 1, "{id}: one finding: {err}");
        let words = format!(
            "has {cells} cells; the header defines 8 (Id, Kind, Source, Item, Decision, Target, Must close, Status)"
        );
        assert!(found[0].contains(&words), "{id}: `{words}`: {err}");
    }
}

/// Row 6: a duplicate id fails on the later copy, naming the first line.
#[test]
fn a_duplicate_id_fails_naming_it_and_the_first_line() {
    let name = "duplicate-id.md";
    let copies = rows(name, "DR-002");
    assert_eq!(copies.len(), 2, "the planted id is on two rows");
    let out = check(name);
    let err = stderr(&out);
    assert_eq!(out.status.code(), Some(1), "fails with findings: {err}");
    assert_eq!(
        failed(&err),
        Some(1),
        "one finding for the later copy: {err}"
    );
    let key = format!("/{name}:{}: DR-002", copies[1]);
    let found: Vec<&str> = err.lines().filter(|l| l.contains(&key)).collect();
    assert_eq!(found.len(), 1, "named on the later copy's line: {err}");
    let words = format!("duplicate id, first at line {}", copies[0]);
    assert!(found[0].contains(&words), "`{words}`: {err}");
}

/// Row 7: an unknown Status fails by id, with the value and the allowed
/// list.
#[test]
fn an_unknown_status_fails_with_the_entry_id() {
    let name = "unknown-status.md";
    let out = check(name);
    let err = stderr(&out);
    assert_eq!(out.status.code(), Some(1), "fails with findings: {err}");
    assert_eq!(failed(&err), Some(1), "{err}");
    let found = findings(&err, name, "DR-002");
    assert_eq!(found.len(), 1, "{err}");
    assert!(
        found[0].contains("unknown Status `done`; allowed: open, accepted, closed"),
        "{err}"
    );
}

/// Row 8: unknown Kind and Must close values fail by id, with the value and
/// the allowed list; matches are exact and case-sensitive, and a Must close
/// that is not exactly `<yes|no> (assumed)` is no warning.
#[test]
fn an_unknown_kind_or_must_close_fails_with_the_entry_id() {
    let name = "unknown-values.md";
    let out = check(name);
    let err = stderr(&out);
    assert_eq!(out.status.code(), Some(1), "fails with findings: {err}");
    assert_eq!(failed(&err), Some(5), "one finding per row: {err}");
    let kinds = "deferred, accepted risk, residual, owner question";
    let must = "yes, no, yes (assumed), no (assumed)";
    for (id, column, value, allowed) in [
        ("DR-001", "Kind", "`risk`", kinds),
        ("DR-002", "Kind", "`Deferred`", kinds),
        ("DR-003", "Must close", "`maybe`", must),
        ("DR-004", "Must close", "`yes(assumed)`", must),
        ("DR-005", "Must close", "`assumed`", must),
    ] {
        let found = findings(&err, name, id);
        assert_eq!(found.len(), 1, "{id}: one finding: {err}");
        for word in [column, value, allowed] {
            assert!(found[0].contains(word), "{id}: names `{word}`: {err}");
        }
    }
    assert!(findings(&err, name, "DR-006").is_empty(), "{err}");
    assert!(!err.contains("warning:"), "no row is assumed: {err}");
}

/// Row 9: malformed cells fail by id; a malformed id is named by line with
/// its first cell quoted, and its other cells are still checked.
#[test]
fn malformed_cells_fail_with_the_entry_id_or_line() {
    let name = "malformed-cells.md";
    let out = check(name);
    let err = stderr(&out);
    assert_eq!(out.status.code(), Some(1), "fails with findings: {err}");
    assert_eq!(failed(&err), Some(8), "{err}");

    let bad_id = rows(name, "DR-7")[0];
    let found = at(&err, name, bad_id);
    assert_eq!(found.len(), 2, "the id and the Status, by line: {err}");
    assert!(
        found.iter().all(|l| l.contains("DR-7")),
        "quotes `DR-7`: {err}"
    );
    assert!(
        found.iter().any(|l| l.contains("`done`")),
        "its Status is still checked: {err}"
    );

    for (id, column, value) in [
        ("DR-002", "Target", "1.2 1.3"),
        ("DR-003", "Target", "owner, 1.2"),
        ("DR-004", "Target", "1.2,"),
        ("DR-005", "Item", ""),
        ("DR-006", "Decision", ""),
        ("DR-008", "Source", "Sofia, REPLAN"),
    ] {
        let found = findings(&err, name, id);
        assert_eq!(found.len(), 1, "{id}: one finding: {err}");
        assert!(found[0].contains(column), "{id}: names {column}: {err}");
        assert!(found[0].contains(value), "{id}: names `{value}`: {err}");
    }
    assert!(findings(&err, name, "DR-001").is_empty(), "{err}");
}

/// Row 10: the table is the one under `## Register`, with the exact header,
/// and no row may follow it after a blank line.
#[test]
fn the_table_must_be_the_one_the_header_defines() {
    let out = check("no-table.md");
    let err = stderr(&out);
    assert_eq!(out.status.code(), Some(1), "no table fails: {err}");
    assert!(
        err.contains("/no-table.md: no \"## Register\" table"),
        "{err}"
    );

    let name = "header-renamed.md";
    let header = lines_where(&text(&fixture(name)), |l| l.starts_with("| Id |"))[0];
    let out = check(name);
    let err = stderr(&out);
    assert_eq!(out.status.code(), Some(1), "a renamed header fails: {err}");
    assert!(
        at(&err, name, header)
            .iter()
            .any(|l| l.contains("the register table's header is not")),
        "names the header's line {header}: {err}"
    );

    let name = "row-after-table.md";
    let stray = rows(name, "DR-003")[0];
    let out = check(name);
    let err = stderr(&out);
    assert_eq!(out.status.code(), Some(1), "a stray row fails: {err}");
    assert!(
        at(&err, name, stray)
            .iter()
            .any(|l| l.contains("a table row after the register table ended at line")),
        "names the stray row's line {stray}: {err}"
    );
}

/// Row 12: the ticket tree is read exactly or not at all. Each changed copy
/// stops with exit 2 and `register: <file>:<line>: <what>`, and no finding
/// list; the unchanged copy reads as the planted tree does.
#[test]
fn an_unreadable_ticket_tree_stops_with_file_and_line() {
    let clean = fixture("clean.md");
    let (_keep, copy) = copy_tree_with(|_, t| Some(t));
    let out = run(&clean, &copy, &[]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "the unchanged copy reads: {}",
        stderr(&out)
    );

    let top = text(&tree().join("tickets.toml"));
    let one = text(&tree().join("epic-one/tickets/tickets.toml"));
    let three = text(&tree().join("epic-three/tickets/tickets.toml"));

    // Each case: what it plants, the change, the file the words name, and
    // the lines they may name (none: the file alone).
    let epic_two_slug = line_of(&top, "slug = \"epic-two\"");
    let epic_three_slug = line_of(&top, "slug = \"epic-three\"");
    let epic_two_id = line_of(&top, "id = 2");
    let one_id2 = line_of(&one, "id = 2");
    let one_id3 = line_of(&one, "id = 3");
    let three_id2 = line_of(&three, "id = 2");
    let three_id3 = line_of(&three, "id = 3");
    let cases: Vec<(&str, Change, &str, Vec<usize>)> = vec![
        (
            "an epic folder missing",
            Box::new(|rel: &str, t: String| (!rel.starts_with("epic-two/")).then_some(t)) as Change,
            "tickets.toml",
            vec![
                header_before(&top, epic_two_slug, "[[epic]]"),
                epic_two_slug,
            ],
        ),
        (
            "an [[entry]] with no id",
            edit("epic-one/tickets/tickets.toml", "id = 3", &[]),
            "epic-one/tickets/tickets.toml",
            vec![header_before(&one, one_id3, "[[entry]]")],
        ),
        (
            "an [[entry]] with two id lines",
            edit(
                "epic-one/tickets/tickets.toml",
                "id = 2",
                &["id = 2", "id = 6"],
            ),
            "epic-one/tickets/tickets.toml",
            vec![header_before(&one, one_id2, "[[entry]]"), one_id2 + 1],
        ),
        (
            "an entry id twice in one epic",
            edit("epic-three/tickets/tickets.toml", "id = 3", &["id = 2"]),
            "epic-three/tickets/tickets.toml",
            vec![header_before(&three, three_id3, "[[entry]]"), three_id3],
        ),
        (
            "an epic id twice",
            edit("tickets.toml", "id = 2", &["id = 3"]),
            "tickets.toml",
            vec![header_before(&top, epic_two_id, "[[epic]]"), epic_two_id],
        ),
        (
            "a multi-line string",
            edit(
                "epic-three/tickets/tickets.toml",
                "id = 2",
                &["id = 2", "notes = \"\"\"", "id = 8", "\"\"\""],
            ),
            "epic-three/tickets/tickets.toml",
            vec![three_id2 + 1],
        ),
        (
            "the top-level tickets.toml missing",
            Box::new(|rel: &str, t: String| (rel != "tickets.toml").then_some(t)) as Change,
            "tickets.toml",
            vec![],
        ),
        (
            "an [[epic]] with no slug",
            edit("tickets.toml", "slug = \"epic-three\"", &[]),
            "tickets.toml",
            vec![header_before(&top, epic_three_slug, "[[epic]]")],
        ),
    ];

    for (what, change, file, lines) in cases {
        let (_keep, copy) = copy_tree_with(change);
        let out = run(&clean, &copy, &[]);
        let err = stderr(&out);
        assert_eq!(out.status.code(), Some(2), "{what}: exit 2: {err}");
        let first = err.lines().next().unwrap_or_default();
        assert!(
            first.starts_with("register: "),
            "{what}: the words start `register: `: {err}"
        );
        let path = copy.join(file).display().to_string();
        let named = if lines.is_empty() {
            first.contains(&path)
        } else {
            lines
                .iter()
                .any(|n| first.contains(&format!("{path}:{n}: ")))
        };
        assert!(
            named,
            "{what}: names {path} at one of lines {lines:?}: {err}"
        );
        if what == "an epic folder missing" {
            assert!(
                first.contains("epic-two"),
                "{what}: names the folder: {err}"
            );
        }
        assert!(
            !err.contains("FAILED") && !err.contains("DR-"),
            "{what}: no finding list: {err}"
        );
    }
}

// ----------------------------------------------------------- release mode

/// Row 13: `--release` and `--gate` need each other, and the gate is an
/// existing `<epic>.<id>`; anything else exits 2 with words, then the usage.
#[test]
fn release_mode_needs_a_known_gate() {
    let cases: [(&[&str], &[&str]); 5] = [
        (&["--release"], &["--gate"]),
        (&["--gate", "3.2"], &["--release"]),
        (&["--release", "--gate", "3"], &["gate", "3"]),
        (&["--release", "--gate", "x.y"], &["gate", "x.y"]),
        (&["--release", "--gate", "3.9"], &["3.9"]),
    ];
    for (args, words) in cases {
        let out = run(&fixture("clean.md"), &tree(), args);
        let err = stderr(&out);
        assert_eq!(out.status.code(), Some(2), "{args:?}: exit 2: {err}");
        let first = err.lines().next().unwrap_or_default();
        assert!(
            !first.contains("unknown option"),
            "{args:?}: the options are known: {err}"
        );
        for word in words {
            assert!(
                first.contains(word),
                "{args:?}: the words name `{word}`: {err}"
            );
        }
        assert!(
            err.contains("usage: cargo xtask") && err.contains("--release --gate <epic>.<id>"),
            "{args:?}: then the usage, with the register task: {err}"
        );
        assert!(stdout(&out).is_empty(), "{args:?}: {}", stdout(&out));
    }
}

/// Row 14: every `(assumed)` entry fails at the gate, whatever its Status,
/// naming its id, Target and the gate. Release mode prints no warnings.
#[test]
fn release_mode_fails_on_each_assumed_entry() {
    let name = "release.md";
    let out = release(name, "3.2");
    let err = stderr(&out);
    assert_eq!(out.status.code(), Some(1), "fails at the gate: {err}");
    for (id, target) in [("DR-001", "2.2"), ("DR-002", "1.2")] {
        let found = findings(&err, name, id);
        assert_eq!(found.len(), 1, "{id}: one finding, as assumed: {err}");
        for word in ["still marked (assumed)", target, "gate 3.2"] {
            assert!(found[0].contains(word), "{id}: names `{word}`: {err}");
        }
    }
    assert!(
        findings(&err, name, "DR-009")
            .iter()
            .any(|l| l.contains("still marked (assumed)") && l.contains("gate 3.2")),
        "an assumed and due entry is named as assumed: {err}"
    );
    assert!(
        !stdout(&out).contains("DR-001"),
        "Must close no (assumed) is never carried"
    );
    assert!(
        !err.contains("warning:"),
        "warnings are normal mode only: {err}"
    );
}

/// Row 15: an open must-close entry is due by the gate when its Target is
/// `owner`, `unassigned` or `none`, or every story it names builds by the
/// gate (the gate itself included); each is named with `due by gate <g>`,
/// its Target and the reason. An assumed and due entry is named twice.
#[test]
fn release_mode_fails_on_each_open_must_close_entry_due_by_the_gate() {
    let name = "release.md";
    let out = release(name, "3.2");
    let err = stderr(&out);
    assert_eq!(out.status.code(), Some(1), "fails at the gate: {err}");
    assert_eq!(failed(&err), Some(11), "3 assumed and 8 due: {err}");
    let built = "every story it names builds by the gate";
    for (id, target, reason) in [
        ("DR-003", "owner", "a decision is pending"),
        ("DR-004", "unassigned", "no story is named yet"),
        ("DR-005", "none", "names no story"),
        ("DR-006", "1.4", built),
        ("DR-007", "1.4, 3.1", built),
        ("DR-008", "3.2", built),
        ("DR-009", "3.1", built),
        ("DR-010", "1.5", built),
    ] {
        let due: Vec<&str> = findings(&err, name, id)
            .into_iter()
            .filter(|l| l.contains("due by gate 3.2"))
            .collect();
        assert_eq!(due.len(), 1, "{id}: due by gate 3.2: {err}");
        // Review R5: `Target <t>:`, so DR-008's `3.2` is not met by the gate's.
        let named = format!("Target {target}:");
        assert!(due[0].contains(&named), "{id}: names `{named}`: {err}");
        assert!(due[0].contains(reason), "{id}: says `{reason}`: {err}");
    }
    assert_eq!(
        findings(&err, name, "DR-009").len(),
        2,
        "assumed and due: named twice: {err}"
    );

    let out = release(name, "1.3");
    let err = stderr(&out);
    assert_eq!(out.status.code(), Some(1), "{err}");
    assert!(
        findings(&err, name, "DR-010")
            .iter()
            .any(|l| l.contains("due by gate 1.3") && l.contains(built)),
        "1.5 is listed before 1.3, so DR-010 is due at gate 1.3: {err}"
    );
}

/// Row 16: an open must-close entry naming a story after the gate is
/// carried forward: never a finding, one stdout line each, in register
/// order, even when the run fails.
#[test]
fn release_mode_lists_entries_carried_forward_without_failing() {
    let name = "release.md";
    let out = release(name, "3.2");
    let err = stderr(&out);
    assert_eq!(out.status.code(), Some(1), "other rows fail: {err}");
    for id in ["DR-011", "DR-012", "DR-013"] {
        assert!(
            findings(&err, name, id).is_empty(),
            "{id} is no finding: {err}"
        );
    }
    assert_eq!(
        stdout(&out).lines().collect::<Vec<_>>(),
        vec![
            "carried forward past gate 3.2: DR-011 (Target 2.1; after the gate: 2.1)",
            "carried forward past gate 3.2: DR-012 (Target 1.1, 3.3; after the gate: 3.3)",
            "carried forward past gate 3.2: DR-013 (Target 3.3; after the gate: 3.3)",
        ]
    );

    let out = release(name, "1.3");
    assert!(
        stdout(&out)
            .lines()
            .any(|l| l == "carried forward past gate 1.3: DR-006 (Target 1.4; after the gate: 1.4)"),
        "1.4 builds after 1.3: {}",
        stdout(&out)
    );
    assert!(findings(&stderr(&out), name, "DR-006").is_empty());
}

/// Rows 17 and 18: with each failing entry confirmed, closed or set to Must
/// close `no`, the gate passes and still lists the carried entries; closed,
/// accepted and not must-close entries are named nowhere.
#[test]
fn release_mode_passes_once_each_entry_is_settled() {
    let out = release("release-confirmed.md", "3.2");
    let err = stderr(&out);
    assert_eq!(
        out.status.code(),
        Some(0),
        "the settled register passes: {err}"
    );
    assert!(err.trim().is_empty(), "nothing on stderr: {err}");
    assert_eq!(
        stdout(&out).lines().collect::<Vec<_>>(),
        vec![
            "carried forward past gate 3.2: DR-011 (Target 2.1; after the gate: 2.1)",
            "carried forward past gate 3.2: DR-012 (Target 1.1, 3.3; after the gate: 3.3)",
            "carried forward past gate 3.2: DR-013 (Target 3.3; after the gate: 3.3)",
            "register: release gate 3.2: 16 entries; 0 still marked (assumed); 0 due; 3 carried forward",
        ]
    );

    for name in ["release.md", "release-confirmed.md"] {
        let out = release(name, "3.2");
        let (err, said) = (stderr(&out), stdout(&out));
        for id in ["DR-014", "DR-015", "DR-016"] {
            assert!(
                !err.contains(id) && !said.contains(id),
                "{name}: {id} is ignored: {err}{said}"
            );
        }
    }
}

/// Row 19: release mode keeps every normal finding.
#[test]
fn release_mode_keeps_the_normal_findings() {
    let name = "unknown-status.md";
    let out = release(name, "3.2");
    let err = stderr(&out);
    assert_eq!(out.status.code(), Some(1), "fails: {err}");
    assert_eq!(failed(&err), Some(1), "{err}");
    let found = findings(&err, name, "DR-002");
    assert_eq!(found.len(), 1, "{err}");
    assert!(
        found[0].contains("unknown Status `done`; allowed: open, accepted, closed"),
        "{err}"
    );
}

/// Rows 11, 15 and 16: build order is table order. Epic 2 is listed after
/// epic 3, so at gate 3.2 a Target 2.1 is carried; in epic 1, entry 5 is
/// listed before entry 3, so at gate 1.3 a Target 1.5 is due and 1.4 is
/// carried.
#[test]
fn build_order_is_table_order_never_id_order() {
    let name = "release.md";
    let out = release(name, "3.2");
    assert!(findings(&stderr(&out), name, "DR-011").is_empty());
    assert!(
        stdout(&out)
            .lines()
            .any(|l| l == "carried forward past gate 3.2: DR-011 (Target 2.1; after the gate: 2.1)"),
        "2.1 builds after 3.2: {}",
        stdout(&out)
    );

    let out = release(name, "1.3");
    let err = stderr(&out);
    assert_eq!(out.status.code(), Some(1), "{err}");
    assert!(
        findings(&err, name, "DR-010")
            .iter()
            .any(|l| l.contains("due by gate 1.3")),
        "1.5 builds before 1.3: {err}"
    );
    assert!(findings(&err, name, "DR-006").is_empty(), "{err}");
    assert!(
        stdout(&out)
            .lines()
            .any(|l| l == "carried forward past gate 1.3: DR-006 (Target 1.4; after the gate: 1.4)"),
        "1.4 builds after 1.3: {}",
        stdout(&out)
    );
}

// ------------------------------------------------------- the real gates

/// Row 20: `just check` runs the normal mode on every PR, not behind a
/// feature, an ignore or a flag: `check` lists `register`, whose body is
/// exactly `cargo xtask register`; xtask has no features and the alias
/// passes none; xtask is a default member, so `cargo nextest run` runs this
/// file's tests; nextest has no default filter; and CI runs `just check` on
/// macOS and the workspace's tests on Linux.
#[test]
fn just_check_runs_the_register_check_in_normal_mode() {
    let repo = repo_root();
    let justfile = text(&repo.join("justfile"));
    let check = justfile
        .lines()
        .find_map(|l| l.strip_prefix("check:"))
        .expect("a `check` recipe");
    assert!(
        check.split_whitespace().any(|w| w == "register"),
        "`just check` runs register: {check}"
    );
    assert_eq!(
        recipe(&justfile, "register"),
        Some(vec!["cargo xtask register"]),
        "the register recipe is the normal mode, with no --release and no --gate"
    );

    let manifest = text(&repo.join("xtask/Cargo.toml"));
    assert!(
        !manifest.contains("[features]"),
        "xtask has no feature to put the check behind"
    );
    let cargo_config = text(&repo.join(".cargo/config.toml"));
    assert!(
        cargo_config
            .lines()
            .any(|l| l.trim() == "xtask = \"run --quiet --package xtask --\""),
        "`cargo xtask` passes no feature: {cargo_config}"
    );
    let workspace = text(&repo.join("Cargo.toml"));
    let defaults: String = workspace
        .split("default-members")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .expect("a default-members list")
        .to_string();
    assert!(
        defaults.contains("\"xtask\""),
        "xtask is a default member: {defaults}"
    );
    for config in [".config/nextest.toml", "nextest.toml"] {
        if let Ok(t) = std::fs::read_to_string(repo.join(config)) {
            assert!(!t.contains("default-filter"), "{config} filters tests");
        }
    }
    let own = text(&repo.join("xtask/tests/register.rs"));
    for line in own.lines().map(str::trim) {
        assert!(
            !line.starts_with("#[ignore")
                && !line.starts_with("#![cfg")
                && !line.starts_with("#[cfg_attr"),
            "this file skips a test: {line}"
        );
    }

    let ci = text(&repo.join(".github/workflows/ci.yml"));
    let runs: Vec<&str> = ci
        .lines()
        .filter_map(|l| l.trim().strip_prefix("run: "))
        .collect();
    assert!(
        runs.contains(&"just check"),
        "macOS runs `just check`: {runs:?}"
    );
    assert!(
        runs.contains(&"cargo nextest run --workspace --exclude am-desktop"),
        "Linux runs the workspace's tests: {runs:?}"
    );
}

/// Row 21: the real gates resolve against today's tree (exit 0 or 1, never
/// 2) and the output names the gate. Pass or fail is not pinned: confirming
/// the assumed values is 4.13's owner step.
#[test]
fn the_real_release_gates_resolve() {
    for gate in ["4.13", "11.8"] {
        let out = xtask(&["register", "--release", "--gate", gate]);
        let (err, said) = (stderr(&out), stdout(&out));
        assert!(
            matches!(out.status.code(), Some(0 | 1)),
            "gate {gate} resolves (exit 0 or 1, never 2): {err}"
        );
        let words = format!("gate {gate}");
        assert!(
            err.contains(&words) || said.contains(&words),
            "the output names gate {gate}: {err}{said}"
        );
    }
}

// ------------------------------------------- implementer-added refusals
//
// Added by the implementer at build time (story 3.25): the refusals the
// plan names only in its Design Notes ("Parsing rules"), which the matrix
// does not cover: a second `## Register`, a missing separator, `'''`, and a
// quoted or non-positive id in the ticket tree.

/// `clean.md` passed through `change`, written to a temporary register file
/// named `name`; returns the tempdir (kept alive) and the file's path.
fn planted_register(name: &str, change: impl Fn(&str) -> String) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), name, &change(&text(&fixture("clean.md"))));
    let path = tmp.path().canonicalize().unwrap().join(name);
    (tmp, path)
}

/// Implementer-added: a second `## Register` heading is a finding on its
/// own line; the first table is still the register and still passes.
#[test]
fn a_second_register_heading_fails_naming_its_line() {
    let name = "second-heading.md";
    let (_keep, register) = planted_register(name, |t| format!("{t}\n## Register\n"));
    let second = lines_where(&text(&register), |l| l.trim() == "## Register")[1];
    let out = run(&register, &tree(), &[]);
    let err = stderr(&out);
    assert_eq!(out.status.code(), Some(1), "fails: {err}");
    assert_eq!(failed(&err), Some(1), "one finding: {err}");
    assert!(
        at(&err, name, second)
            .iter()
            .any(|l| l.contains("a second \"## Register\" heading")),
        "names the second heading's line {second}: {err}"
    );
}

/// Implementer-added: a header with no separator row after it is a finding
/// on the header's line; the rows that follow are still checked as rows.
#[test]
fn a_missing_separator_fails_naming_the_header_line() {
    let name = "no-separator.md";
    let (_keep, register) = planted_register(name, |t| {
        replace_line(
            t,
            "|----|------|--------|------|----------|--------|------------|--------|",
            &[],
        )
    });
    let header = lines_where(&text(&register), |l| l.starts_with("| Id |"))[0];
    let out = run(&register, &tree(), &[]);
    let err = stderr(&out);
    assert_eq!(out.status.code(), Some(1), "fails: {err}");
    assert_eq!(failed(&err), Some(1), "one finding, no row finding: {err}");
    assert!(
        at(&err, name, header)
            .iter()
            .any(|l| l.contains("separator")),
        "names the header's line {header}: {err}"
    );

    // A separator with the wrong cell count is no separator either.
    let (_keep, register) = planted_register(name, |t| {
        replace_line(
            t,
            "|----|------|--------|------|----------|--------|------------|--------|",
            &["|----|------|--------|------|----------|--------|------------|"],
        )
    });
    let out = run(&register, &tree(), &[]);
    let err = stderr(&out);
    assert_eq!(out.status.code(), Some(1), "fails: {err}");
    assert!(
        at(&err, name, header)
            .iter()
            .any(|l| l.contains("separator")),
        "a 7-cell separator: {err}"
    );
}

/// Implementer-added: the ticket-tree refusals outside the matrix. Each
/// changed copy stops with exit 2 and `register: <file>:<line>: <what>`,
/// and no finding list.
#[test]
fn a_literal_multi_line_string_or_a_quoted_or_non_positive_id_stops_the_tree_scan() {
    let clean = fixture("clean.md");
    let top = text(&tree().join("tickets.toml"));
    let one = text(&tree().join("epic-one/tickets/tickets.toml"));
    let three = text(&tree().join("epic-three/tickets/tickets.toml"));
    let one_id3 = line_of(&one, "id = 3");
    let three_id2 = line_of(&three, "id = 2");
    let top_id3 = line_of(&top, "id = 3");
    let cases: Vec<(&str, Change, &str, usize, &str)> = vec![
        (
            "a ''' string",
            edit(
                "epic-three/tickets/tickets.toml",
                "id = 2",
                &["id = 2", "notes = '''", "id = 8", "'''"],
            ),
            "epic-three/tickets/tickets.toml",
            three_id2 + 1,
            "multi-line string",
        ),
        (
            "a quoted id key, which is not read",
            edit("epic-one/tickets/tickets.toml", "id = 3", &["\"id\" = 3"]),
            "epic-one/tickets/tickets.toml",
            header_before(&one, one_id3, "[[entry]]"),
            "no `id",
        ),
        (
            "a quoted id value",
            edit("epic-one/tickets/tickets.toml", "id = 3", &["id = \"3\""]),
            "epic-one/tickets/tickets.toml",
            one_id3,
            "not a positive integer",
        ),
        (
            "an entry id of zero",
            edit("epic-one/tickets/tickets.toml", "id = 3", &["id = 0"]),
            "epic-one/tickets/tickets.toml",
            one_id3,
            "not a positive integer",
        ),
        (
            "a negative epic id",
            edit("tickets.toml", "id = 3", &["id = -3"]),
            "tickets.toml",
            top_id3,
            "not a positive integer",
        ),
    ];
    for (what, change, file, line, words) in cases {
        let (_keep, copy) = copy_tree_with(change);
        let out = run(&clean, &copy, &[]);
        let err = stderr(&out);
        assert_eq!(out.status.code(), Some(2), "{what}: exit 2: {err}");
        let first = err.lines().next().unwrap_or_default();
        let path = copy.join(file).display().to_string();
        assert!(
            first.starts_with("register: ") && first.contains(&format!("{path}:{line}: ")),
            "{what}: names {path}:{line}: {err}"
        );
        assert!(first.contains(words), "{what}: says `{words}`: {err}");
        assert!(
            !err.contains("FAILED") && !err.contains("DR-"),
            "{what}: no finding list: {err}"
        );
    }
}

// ------------------------------------------------- review round 1 (R1-R4)
//
// Written by the tester from the plan's "Review triage round 1", failing
// first.

/// Review R1: GitHub renders a non-blank line straight after the table as
/// another row, even without its leading `|`, so the check names that line
/// in both modes instead of letting the row slip past.
#[test]
fn a_line_straight_after_the_table_fails_naming_its_line() {
    let name = "row-without-pipe.md";
    let stray = lines_where(&text(&fixture(name)), |l| l.starts_with("DR-003 |"))[0];
    for (mode, out) in [("normal", check(name)), ("release", release(name, "3.2"))] {
        let err = stderr(&out);
        assert_eq!(out.status.code(), Some(1), "{mode}: fails: {err}");
        assert!(
            !at(&err, name, stray).is_empty(),
            "{mode}: names the line {stray} straight after the table: {err}"
        );
    }
}

/// Review R2: a `[` line that is not a recognised table header is refused,
/// never skipped: a quoted `[["entry"]]` would drop an entry, and a
/// spaced `[entry . meta]` would leave its keys to the table above.
#[test]
fn an_unrecognised_table_header_stops_the_tree_scan() {
    let clean = fixture("clean.md");
    let one = text(&tree().join("epic-one/tickets/tickets.toml"));
    let first_entry = lines_where(&one, |l| l.trim() == "[[entry]]")[0];
    let one_id2 = line_of(&one, "id = 2");
    let file = "epic-one/tickets/tickets.toml";
    let cases: Vec<(&str, Change, usize)> = vec![
        (
            "a quoted [[\"entry\"]] header",
            edit(file, "[[entry]]", &["[[\"entry\"]]"]),
            first_entry,
        ),
        (
            "a spaced [entry . meta] header",
            edit(
                file,
                "id = 2",
                &["id = 2", "[entry . meta]", "note = \"planted\""],
            ),
            one_id2 + 1,
        ),
    ];
    for (what, change, line) in cases {
        let (_keep, copy) = copy_tree_with(change);
        let out = run(&clean, &copy, &[]);
        let err = stderr(&out);
        assert_eq!(out.status.code(), Some(2), "{what}: exit 2: {err}");
        let first = err.lines().next().unwrap_or_default();
        let path = copy.join(file).display().to_string();
        assert!(
            first.starts_with("register: ") && first.contains(&format!("{path}:{line}: ")),
            "{what}: names {path}:{line}: {err}"
        );
        assert!(
            !err.contains("FAILED") && !err.contains("DR-"),
            "{what}: no finding list: {err}"
        );
    }
}

/// Review R2: comments are skipped before the multi-line string refusal,
/// so a comment that quotes `'''` or `"""` blocks nothing.
#[test]
fn a_comment_quoting_a_multi_line_delimiter_is_still_a_comment() {
    let (_keep, copy) = copy_tree_with(|_, t| {
        Some(format!(
            "# A comment may quote ''' or \"\"\"; it opens no string.\n{}",
            t.replace(
                "id = 2\n",
                "id = 2\n  # an indented comment quoting ''' inside a table\n"
            )
        ))
    });
    let out = run(&fixture("clean.md"), &copy, &[]);
    let err = stderr(&out);
    assert_eq!(
        out.status.code(),
        Some(0),
        "the comments block nothing: {err}"
    );
    assert_eq!(
        stdout(&out).trim_end(),
        "register: 7 entries checked against 10 stories in 3 epics; 0 still marked (assumed), listed as warnings"
    );
}

/// Review R3: a slug is one plain folder name, and no two epics share one.
/// A slug that climbs out (`../..`) or is absolute is refused even when it
/// leads to a readable file, and a shared slug is refused, so two epics
/// never read the same entries. Each names the top-level file and the
/// offending slug's line.
#[test]
fn a_slug_must_be_one_plain_folder_name_used_once() {
    let clean = fixture("clean.md");
    let top = text(&tree().join("tickets.toml"));
    let two_slug = line_of(&top, "slug = \"epic-two\"");
    let three_slug = line_of(&top, "slug = \"epic-three\"");
    let epic_two = text(&tree().join("epic-two/tickets/tickets.toml"));

    // The planted tree under <tmp>/x/y, with epic 2's slug replaced by
    // `slug` and its file written at <tmp>/<epic_two_at>/tickets/tickets.toml.
    let planted = |slug: &dyn Fn(&Path) -> String, epic_two_at: &str| {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let tickets = base.join("x/y");
        for rel in TREE_FILES {
            let t = text(&tree().join(rel));
            match rel {
                "tickets.toml" => {
                    let line = format!("slug = \"{}\"", slug(&base));
                    let t = replace_line(&t, "slug = \"epic-two\"", &[line.as_str()]);
                    write(&tickets, rel, &t);
                }
                "epic-two/tickets/tickets.toml" => write(
                    &base,
                    &format!("{epic_two_at}/tickets/tickets.toml"),
                    &epic_two,
                ),
                _ => write(&tickets, rel, &t),
            }
        }
        (tmp, tickets)
    };

    let shared = copy_tree_with(edit(
        "tickets.toml",
        "slug = \"epic-three\"",
        &["slug = \"epic-one\""],
    ));
    let cases = vec![
        (
            "a slug that climbs out",
            planted(&|_| "../..".into(), "."),
            two_slug,
        ),
        (
            "an absolute slug",
            planted(&|base| base.join("abs").display().to_string(), "abs"),
            two_slug,
        ),
        ("two epics sharing a slug", shared, three_slug),
    ];
    for (what, (_keep, tickets), line) in cases {
        let out = run(&clean, &tickets, &[]);
        let err = stderr(&out);
        assert_eq!(out.status.code(), Some(2), "{what}: exit 2: {err}");
        let first = err.lines().next().unwrap_or_default();
        let path = tickets.join("tickets.toml").display().to_string();
        assert!(
            first.starts_with("register: ") && first.contains(&format!("{path}:{line}: ")),
            "{what}: names {path}:{line}: {err}"
        );
        assert!(
            !err.contains("FAILED") && !err.contains("DR-"),
            "{what}: no finding list: {err}"
        );
    }
}

/// Review R4: each task takes only its own flags; a flag another task
/// takes is refused with exit 2, naming it, before anything runs.
#[test]
fn each_task_refuses_a_flag_it_does_not_take() {
    for (args, flags) in [
        (&["register", "--list", "x"][..], &["--list"][..]),
        (
            &["seam-guard", "--release", "--gate", "4.13"][..],
            &["--release", "--gate"][..],
        ),
    ] {
        let out = xtask(args);
        let err = stderr(&out);
        assert_eq!(out.status.code(), Some(2), "{args:?}: exit 2: {err}");
        let first = err.lines().next().unwrap_or_default();
        assert!(
            flags.iter().any(|f| first.contains(f)),
            "{args:?}: the first line names {flags:?}: {err}"
        );
        assert!(
            stdout(&out).is_empty(),
            "{args:?}: nothing ran: {}",
            stdout(&out)
        );
    }
}
