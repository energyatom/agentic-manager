//! Epic 3 story 6 (AD-21, AD-29, AD-35, AD-46; owner Q3; matrix rows 7, 8,
//! 28 and 31): a workflow step declares the one table it writes and, from
//! story 3.6b, the document it versions.
//!
//! ```yaml
//! - id: collect
//!   seat: writer
//!   instruction: …
//!   table: entries          # a table of the seat's own team; the seat has Tables on
//! - id: summarise
//!   seat: writer
//!   instruction: …
//!   document: summary-note  # kebab-case; one team per document id
//! ```
//!
//! Declaring a table binds a write; it never grants one (only the Tables
//! capability grants `table_write`, AD-46). Every refusal is a finding with
//! its file, line and fix, so `agentic validate` names it and `agentic run`
//! starts nothing: a table that does not exist, a table of another team
//! than the seat's, a seat without Tables, `table:` on a gate step, a
//! `document:` that is not kebab-case or that two teams' workflows declare,
//! a document step with `output_schema`, `loop_back` or `gate`, and
//! `document:` on a fan-out step (the document halves, story 3.6b).
//!
//! Row 40 (owner, 2026-11-24): a fan-out step (`seats:` or `over_folder:`)
//! that declares `table:`, or whose seats hold `table_write`, cannot write
//! a table until story 3.14. It is a warning (never an error) at the step,
//! saying so and naming the fix; the project still loads. On a fan-out step
//! `table:` is checked for existence only. The Company A sample's one such
//! step gets exactly that warning (its ids are read from
//! `fixtures/templates/sample.json`, AD-23). Written before the key exists
//! (today it is reported as an unknown key) and before the warning.
#![forbid(unsafe_code)]

mod common;

use std::path::{Path, PathBuf};

use am_config::load;
use common::{errors, project_with, snapshot, warnings};
use serde_json::Value;

const COMPANY: &str = "schema_version: 2\nname: Small Project\ntimezone: UTC\ncurrency: EUR\nteams:\n  - id: core\n    name: Core team\n  - id: side\n    name: Side team\n";

const SEATS: [(&str, &str); 4] = [
    (
        "seats/writer.md",
        "---\nschema_version: 2\nname: Writer\nrole: Analyst\nteam: core\nmodel:\n  account: local-lmstudio\n  model: example-local\ncapabilities:\n  tables:\n    on: true\n---\nRecord the entries.\n",
    ),
    (
        "seats/reader.md",
        "---\nschema_version: 2\nname: Reader\nrole: Analyst\nteam: core\nmodel:\n  account: local-lmstudio\n  model: example-local\n---\nRead the entries.\n",
    ),
    (
        "seats/outsider.md",
        "---\nschema_version: 2\nname: Outsider\nrole: Writer\nteam: side\nmodel:\n  account: local-lmstudio\n  model: example-local\ncapabilities:\n  tables:\n    on: true\n---\nWrite for the other team.\n",
    ),
    (
        "seats/owner.md",
        "---\nschema_version: 2\nname: Owner\nrole: Reviewer\nteam: core\ndescription: A person; no model.\n---\nA person fills this seat.\n",
    ),
];

const TABLES: [(&str, &str); 2] = [
    (
        "tables/entries.yaml",
        "schema_version: 2\nname: Entries\nteam: core\nkey: ref\ncolumns:\n  - id: ref\n    type: text\n    required: true\n  - id: title\n    type: text\n  - id: state\n    type: status\n    values:\n      - new\n      - done\n",
    ),
    (
        "tables/others.yaml",
        "schema_version: 2\nname: Others\nteam: side\nkey: ref\ncolumns:\n  - id: ref\n    type: text\n    required: true\n",
    ),
];

const SCHEMA: (&str, &str) = (
    "schemas/line.json",
    "{\n  \"schema_version\": 2,\n  \"$schema\": \"https://json-schema.org/draft/2020-12/schema\",\n  \"type\": \"object\",\n  \"required\": [\"line\"],\n  \"properties\": { \"line\": { \"type\": \"string\" } }\n}\n",
);

const WF: &str = "workflows/flow.yaml";
const OTHER_WF: &str = "workflows/other.yaml";

const COLLECT: &str =
    "  - id: collect\n    seat: writer\n    instruction: Record the entries.\n    table: entries\n";

/// A project with two teams, the four seats, the two tables, the schema
/// `line` and `workflows/flow.yaml` (team `core`) holding `steps` (each
/// step's YAML lines, two-space indented under `steps:`), plus `more`.
fn project(steps: &str, more: &[(&str, &str)]) -> tempfile::TempDir {
    let flow = format!("schema_version: 2\nname: Flow\nteam: core\nsteps:\n{steps}");
    let mut files: Vec<(&str, &str)> = vec![("company.yaml", COMPANY), SCHEMA];
    files.extend_from_slice(&SEATS);
    files.extend_from_slice(&TABLES);
    files.push((WF, &flow));
    files.extend_from_slice(more);
    project_with(&files)
}

/// The step `id` of the workflow `flow`.
fn step_of(root: &Path, id: &str) -> am_config::Step {
    snapshot(root).workflows()["flow"]
        .steps
        .iter()
        .find(|s| s.id == id)
        .unwrap_or_else(|| panic!("no step {id}"))
        .clone()
}

/// The 1-based line of `file` under `root` that holds `marker`.
fn line_of(root: &Path, file: &str, marker: &str) -> u32 {
    let text = std::fs::read_to_string(root.join(file)).unwrap();
    text.lines()
        .position(|l| l.contains(marker))
        .map(|i| i as u32 + 1)
        .unwrap_or_else(|| panic!("no `{marker}` in {file}:\n{text}"))
}

/// Neither key is reported as unknown.
fn assert_keys_known(found: &[am_config::Finding]) {
    for f in found {
        assert!(
            !f.message.contains("unknown key 'table'")
                && !f.message.contains("unknown key 'document'"),
            "the keys are known: {f:#?}"
        );
    }
}

/// One error in `file` at a line holding one of `markers`, whose message
/// holds every one of `words` and which has a fix; the project gives no
/// snapshot.
fn assert_refused_at(root: &Path, file: &str, markers: &[&str], words: &[&str]) {
    let loaded = load(root).unwrap();
    let lines: Vec<u32> = markers.iter().map(|m| line_of(root, file, m)).collect();
    let hits: Vec<&am_config::Finding> = errors(&loaded.findings)
        .into_iter()
        .filter(|f| {
            f.file == file && lines.contains(&f.line) && words.iter().all(|w| f.message.contains(w))
        })
        .collect();
    assert!(
        !hits.is_empty(),
        "an error at {file}:{lines:?} naming {words:?}; found {:#?}",
        loaded.findings
    );
    for f in &hits {
        assert!(!f.fix.trim().is_empty(), "a fix: {f:#?}");
    }
    assert_keys_known(&loaded.findings);
    assert!(loaded.snapshot.is_none(), "an error gives no snapshot");
}

/// Row 31: `table:` parses into the step; a step that declares none has
/// none; it is not a run-graph key; no finding.
#[test]
fn a_step_declares_its_table() {
    let dir = project(
        &format!(
            "{COLLECT}  - id: plain\n    seat: reader\n    instruction: Say what was recorded.\n"
        ),
        &[],
    );
    let found = load(dir.path()).unwrap().findings;
    assert!(errors(&found).is_empty(), "{found:#?}");
    assert_keys_known(&found);
    let root = dir.path();

    assert_eq!(step_of(root, "collect").table.as_deref(), Some("entries"));
    assert_eq!(step_of(root, "plain").table, None);
    for id in ["collect", "plain"] {
        assert!(step_of(root, id).run_graph.is_empty(), "{id}");
    }
}

/// Row 31: a `table:` naming no `tables/<id>.yaml` is an error at its line
/// naming the id, with a fix.
#[test]
fn an_unknown_table_is_an_error_with_file_line_and_fix() {
    let dir = project(
        "  - id: collect\n    seat: writer\n    instruction: Record the entries.\n    table: ghost\n",
        &[],
    );
    assert_refused_at(dir.path(), WF, &["table: ghost"], &["ghost"]);
}

/// Rows 7 and 31 (AD-21): a seat writes only its own team's table. The
/// seat `writer` (team `core`) declaring `others` (team `side`) is an
/// error at the `table:` line naming the table and both teams.
#[test]
fn a_table_of_another_team_is_refused() {
    let dir = project(
        "  - id: collect\n    seat: writer\n    instruction: Record the entries.\n    table: others\n",
        &[],
    );
    assert_refused_at(
        dir.path(),
        WF,
        &["table: others"],
        &["others", "side", "core"],
    );
}

/// Rows 8 and 31 (AD-46): declaring a table grants nothing. The seat
/// `reader` has the Tables capability off, so its step's `table:` is an
/// error at that line naming the seat and the capability.
#[test]
fn a_seat_without_tables_declares_no_table() {
    let dir = project(
        "  - id: collect\n    seat: reader\n    instruction: Record the entries.\n    table: entries\n",
        &[],
    );
    assert_refused_at(dir.path(), WF, &["table: entries"], &["reader"]);
    let loaded = load(dir.path()).unwrap();
    assert!(
        errors(&loaded.findings)
            .iter()
            .any(|f| f.message.to_lowercase().contains("tables")),
        "the finding names the Tables capability: {:#?}",
        loaded.findings
    );
}

/// Row 28, the table half (AD-36): a gate makes no model call, so it
/// declares no table: `table:` on a `gate: review` step is an error at the
/// key's line (or the gate's), naming `table`, with a fix.
#[test]
fn a_gate_step_declares_no_table() {
    let dir = project(
        &format!(
            "{COLLECT}  - id: approve\n    seat: owner\n    instruction: Approve it.\n    gate: review\n    table: entries\n"
        ),
        &[],
    );
    let root = dir.path();
    let text = std::fs::read_to_string(root.join(WF)).unwrap();
    let gate_start = text.find("  - id: approve").unwrap();
    let first_line = text[..gate_start].lines().count() as u32 + 1;
    let lines: Vec<u32> = text
        .lines()
        .enumerate()
        .filter(|(i, l)| {
            *i as u32 + 1 >= first_line
                && (l.contains("table: entries") || l.contains("gate: review"))
        })
        .map(|(i, _)| i as u32 + 1)
        .collect();
    let loaded = load(root).unwrap();
    let hits: Vec<&am_config::Finding> = errors(&loaded.findings)
        .into_iter()
        .filter(|f| f.file == WF && lines.contains(&f.line) && f.message.contains("table"))
        .collect();
    assert!(
        !hits.is_empty(),
        "an error at {WF}:{lines:?} naming `table`; found {:#?}",
        loaded.findings
    );
    for f in &hits {
        assert!(!f.fix.trim().is_empty(), "a fix: {f:#?}");
    }
    assert_keys_known(&loaded.findings);
    assert!(loaded.snapshot.is_none(), "no snapshot");
}

// ------------------------------------------------------------------ row 40

/// The 1-based lines of step `id`'s block in a workflow's `text`: its
/// `- id:` line and every line under it, up to the next step or the end.
fn step_block(text: &str, id: &str) -> Vec<u32> {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines
        .iter()
        .position(|l| l.trim_start() == format!("- id: {id}"))
        .unwrap_or_else(|| panic!("no step {id}:\n{text}"));
    let indent = lines[start].len() - lines[start].trim_start().len();
    let mut block = vec![start as u32 + 1];
    for (i, l) in lines.iter().enumerate().skip(start + 1) {
        let lead = l.len() - l.trim_start().len();
        let next_item = lead == indent && l.trim_start().starts_with("- ");
        if !l.trim().is_empty() && (lead < indent || next_item) {
            break;
        }
        block.push(i as u32 + 1);
    }
    block
}

/// The warnings of `found` that say a step cannot write a table until
/// story 3.14.
fn fan_out_warnings(found: &[am_config::Finding]) -> Vec<&am_config::Finding> {
    warnings(found)
        .into_iter()
        .filter(|f| f.message.contains("3.14"))
        .collect()
}

/// The project at `root` loads (no error, a snapshot) with exactly one
/// fan-out warning: in `file`, at a line of step `id`'s block, saying the
/// fan-out step `id` cannot write a table until story 3.14 and that
/// `agentic run` refuses it, with a fix.
fn assert_warned(root: &Path, file: &str, id: &str) {
    let loaded = load(root).unwrap();
    assert!(
        errors(&loaded.findings).is_empty(),
        "a warning, never an error: {:#?}",
        loaded.findings
    );
    assert_keys_known(&loaded.findings);
    let found = fan_out_warnings(&loaded.findings);
    assert_eq!(found.len(), 1, "one warning: {:#?}", loaded.findings);
    let w = found[0];
    let text = std::fs::read_to_string(root.join(file)).unwrap();
    assert_eq!(w.file, file, "{w:#?}");
    assert!(
        step_block(&text, id).contains(&w.line),
        "at the step `{id}` ({:?}): {w:#?}",
        step_block(&text, id)
    );
    for word in [id, "fan-out", "table", "3.14", "agentic run"] {
        assert!(w.message.contains(word), "names `{word}`: {w:#?}");
    }
    assert!(!w.fix.trim().is_empty(), "a fix: {w:#?}");
    assert!(loaded.snapshot.is_some(), "a warning keeps the snapshot");
}

/// Row 40 (owner, 2026-11-24): a fan-out step that would write a table is
/// warned, never refused: one whose seats include a seat with Tables on
/// (`writer` holds `table_write`), one that declares `table:` on seats
/// without it, and an `over_folder:` step that declares `table:`. Each
/// gets one warning at the step, naming it, story 3.14 and `agentic run`,
/// with a fix; the project loads. A fan-out step whose seats lack
/// `table_write` and that declares no table gets none.
#[test]
fn a_fan_out_step_that_writes_a_table_is_warned_with_the_reason() {
    let dir = project(
        "  - id: spread\n    seats:\n      - writer\n      - reader\n    instruction: Record your share.\n",
        &[],
    );
    assert_warned(dir.path(), WF, "spread");

    let dir = project(
        "  - id: spread\n    seats:\n      - reader\n    instruction: Record your share.\n    table: entries\n",
        &[],
    );
    assert_warned(dir.path(), WF, "spread");
    assert_eq!(
        step_of(dir.path(), "spread").table.as_deref(),
        Some("entries"),
        "a fan-out step's table is kept"
    );

    let dir = project(
        "  - id: spread\n    over_folder: inputs\n    routes:\n      - match:\n          - \"*.csv\"\n        seat: reader\n    instruction: Record each file's entries.\n    table: entries\n",
        &[],
    );
    assert_warned(dir.path(), WF, "spread");

    let dir = project(
        "  - id: spread\n    seats:\n      - reader\n    instruction: Read your share.\n",
        &[],
    );
    let found = load(dir.path()).unwrap().findings;
    assert!(
        fan_out_warnings(&found).is_empty(),
        "no table and no table_write: no warning: {found:#?}"
    );
    assert!(errors(&found).is_empty(), "{found:#?}");
}

/// Row 40 with row 31: on a fan-out step `table:` is still checked for
/// existence: a table that does not exist is an error at its line.
#[test]
fn a_fan_out_steps_unknown_table_is_an_error() {
    let dir = project(
        "  - id: spread\n    seats:\n      - writer\n    instruction: Record your share.\n    table: ghost\n",
        &[],
    );
    assert_refused_at(dir.path(), WF, &["table: ghost"], &["ghost"]);
}

/// The repository root.
fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Row 40 on the Company A sample (its ids are read from
/// `fixtures/templates/sample.json`, AD-23): the sample loads with exactly
/// one finding, the fan-out warning on the chained flow's update workflow,
/// at its fan-out step (the one step there with `seats:`); its other
/// fan-out steps (the report flow's branches, the folder flow's read),
/// whose seats lack `table_write`, get none.
#[test]
fn the_sample_f2_update_gets_exactly_that_warning() {
    let expect: Value = serde_json::from_str(
        &std::fs::read_to_string(repo().join("fixtures/templates/sample.json")).unwrap(),
    )
    .unwrap();
    let sample = repo()
        .join("templates/samples")
        .join(expect["sample"].as_str().unwrap());
    let workflow = expect["chained_flows"]["update"].as_str().unwrap();
    let file = format!("workflows/{workflow}.yaml");
    let text = std::fs::read_to_string(sample.join(&file)).unwrap();
    let fan_out: Vec<String> = text
        .lines()
        .filter_map(|l| l.trim_start().strip_prefix("- id: "))
        .map(str::to_string)
        .filter(|id| {
            let lines: Vec<&str> = text.lines().collect();
            step_block(&text, id)
                .iter()
                .any(|n| lines[*n as usize - 1].trim_start().starts_with("seats:"))
        })
        .collect();
    assert_eq!(fan_out.len(), 1, "one fan-out step in {file}: {fan_out:?}");

    assert_warned(&sample, &file, &fan_out[0]);
    let loaded = load(&sample).unwrap();
    assert_eq!(
        loaded.findings.len(),
        1,
        "that warning is the sample's only finding: {:#?}",
        loaded.findings
    );
}

// ------------------------------------------------- review round 1 (3.23)

/// Review round 1, R8 (manager, 2026-11-25): one fan-out decision. am-config
/// computes it once, as `Step.fan_out`, and validate's warning and `agentic
/// run`'s refusal (`am_config::fan_out_table_refusal`) both follow it. A
/// step with `over_folder: ~` (a key written with no value), its own
/// `seat:` and `table:` gets one answer from all three, whichever it is.
/// The controls: a step listing `seats:` fans out; a plain step does not.
#[test]
fn an_empty_over_folder_gets_one_fan_out_decision() {
    let dir = project(
        &format!(
            "  - id: odd\n    seat: writer\n    over_folder: ~\n    instruction: Record the entries.\n    table: entries\n  - id: spread\n    seats:\n      - writer\n    instruction: Record your share.\n{}",
            COLLECT
        ),
        &[],
    );
    let root = dir.path();
    let loaded = load(root).unwrap();
    assert!(
        errors(&loaded.findings).is_empty(),
        "{:#?}",
        loaded.findings
    );
    let text = std::fs::read_to_string(root.join(WF)).unwrap();
    let odd_lines = step_block(&text, "odd");
    let warned = fan_out_warnings(&loaded.findings)
        .iter()
        .any(|f| f.file == WF && odd_lines.contains(&f.line));
    let snap = snapshot(root);
    let odd = step_of(root, "odd");

    assert_eq!(
        odd.fan_out, warned,
        "Step.fan_out and validate's warning agree: {:#?}",
        loaded.findings
    );
    assert_eq!(
        am_config::fan_out_table_refusal(&snap, &odd).is_some(),
        warned,
        "agentic run's check and validate's warning agree"
    );

    assert!(step_of(root, "spread").fan_out, "`seats:` fans out");
    assert!(!step_of(root, "collect").fan_out, "a plain step does not");
}

/// Row 31, the document half (story 3.6b): `document:` parses into the
/// step; a step that declares none has none; it is not a run-graph key; no
/// finding.
#[test]
fn a_step_declares_its_document() {
    let dir = project(
        &format!(
            "{COLLECT}  - id: summarise\n    seat: writer\n    instruction: Write the summary note.\n    document: summary-note\n"
        ),
        &[],
    );
    let found = load(dir.path()).unwrap().findings;
    assert!(errors(&found).is_empty(), "{found:#?}");
    assert_keys_known(&found);
    let root = dir.path();

    let summarise = step_of(root, "summarise");
    assert_eq!(
        (summarise.table.as_deref(), summarise.document.as_deref()),
        (None, Some("summary-note"))
    );
    assert_eq!(step_of(root, "collect").document, None);
    assert!(summarise.run_graph.is_empty());
}

/// Rows 28 and 31 (AD-29, AD-36): a document step's version is its final
/// message, so it takes no `output_schema`, `loop_back` or `gate`. Each is
/// an error at the key's line (or the clashing key's), naming `document`.
#[test]
fn a_document_step_takes_no_schema_loop_back_or_gate() {
    for (what, steps, markers, word) in [
        (
            "with output_schema",
            format!(
                "{COLLECT}  - id: summarise\n    seat: writer\n    instruction: Write the note.\n    output_schema: line\n    document: summary-note\n"
            ),
            vec!["document: summary-note", "output_schema: line"],
            "document",
        ),
        (
            "with loop_back",
            format!(
                "{COLLECT}  - id: summarise\n    seat: writer\n    instruction: Write the note.\n    loop_back: {{ to: collect }}\n    document: summary-note\n"
            ),
            vec!["document: summary-note", "loop_back:"],
            "document",
        ),
        (
            "on a gate",
            format!(
                "{COLLECT}  - id: approve\n    seat: owner\n    instruction: Approve it.\n    gate: review\n    document: summary-note\n"
            ),
            vec!["document: summary-note", "gate: review"],
            "document",
        ),
    ] {
        let dir = project(&steps, &[]);
        let root = dir.path();
        // The marker of the key on the clashing step (the `table:` of the
        // first step, `collect`, is not one).
        let text = std::fs::read_to_string(root.join(WF)).unwrap();
        let clash_start = text
            .find(match what {
                w if w.contains("gate") => "  - id: approve",
                _ => "  - id: summarise",
            })
            .unwrap();
        let first_line = text[..clash_start].lines().count() as u32 + 1;
        let lines: Vec<u32> = text
            .lines()
            .enumerate()
            .filter(|(i, l)| *i as u32 + 1 >= first_line && markers.iter().any(|m| l.contains(m)))
            .map(|(i, _)| i as u32 + 1)
            .collect();
        assert!(!lines.is_empty(), "{what}: markers in {text}");
        let loaded = load(root).unwrap();
        let hits: Vec<&am_config::Finding> = errors(&loaded.findings)
            .into_iter()
            .filter(|f| f.file == WF && lines.contains(&f.line) && f.message.contains(word))
            .collect();
        assert!(
            !hits.is_empty(),
            "{what}: an error at {WF}:{lines:?} naming `{word}`; found {:#?}",
            loaded.findings
        );
        for f in &hits {
            assert!(!f.fix.trim().is_empty(), "{what}: a fix: {f:#?}");
        }
        assert_keys_known(&loaded.findings);
        assert!(loaded.snapshot.is_none(), "{what}: no snapshot");
    }
}

/// Row 28, the document half (story 3.6b): a fan-out step (`seats:`)
/// versions no document: `document:` on it is an error at the key's line
/// (or the `seats:` line), naming `document`, with a fix.
#[test]
fn a_fan_out_step_declares_no_document() {
    let dir = project(
        &format!(
            "{COLLECT}  - id: spread\n    seats: [writer, reader]\n    instruction: Each writes a note.\n    document: summary-note\n"
        ),
        &[],
    );
    assert_refused_at(
        dir.path(),
        WF,
        &["document: summary-note", "seats: [writer, reader]"],
        &["document"],
    );
}

/// Row 31 (owner Q3): a document id is kebab-case and belongs to one team.
/// `Summary_Note` is an error at its line; the same id declared by a `core`
/// workflow and a `side` workflow is an error naming the id; declared twice
/// by one team's workflows it is fine.
#[test]
fn one_document_belongs_to_one_team() {
    let dir = project(
        &format!(
            "{COLLECT}  - id: summarise\n    seat: writer\n    instruction: Write the note.\n    document: Summary_Note\n"
        ),
        &[],
    );
    assert_refused_at(
        dir.path(),
        WF,
        &["document: Summary_Note"],
        &["Summary_Note"],
    );

    let side_flow = "schema_version: 2\nname: Other\nteam: side\nsteps:\n  - id: note\n    seat: outsider\n    instruction: Write the note.\n    document: summary-note\n";
    let dir = project(
        &format!(
            "{COLLECT}  - id: summarise\n    seat: writer\n    instruction: Write the note.\n    document: summary-note\n"
        ),
        &[(OTHER_WF, side_flow)],
    );
    let root = dir.path();
    let loaded = load(root).unwrap();
    let at_core = line_of(root, WF, "document: summary-note");
    let at_side = line_of(root, OTHER_WF, "document: summary-note");
    let hits: Vec<&am_config::Finding> = errors(&loaded.findings)
        .into_iter()
        .filter(|f| {
            ((f.file == WF && f.line == at_core) || (f.file == OTHER_WF && f.line == at_side))
                && f.message.contains("summary-note")
        })
        .collect();
    assert!(
        !hits.is_empty(),
        "an error at a `document: summary-note` line naming the id; found {:#?}",
        loaded.findings
    );
    for f in &hits {
        assert!(!f.fix.trim().is_empty(), "a fix: {f:#?}");
    }
    assert!(loaded.snapshot.is_none(), "an error gives no snapshot");

    // One team's two workflows may both version it.
    let core_flow = "schema_version: 2\nname: Again\nteam: core\nsteps:\n  - id: again\n    seat: writer\n    instruction: Write the note again.\n    document: summary-note\n";
    let dir = project(
        &format!(
            "{COLLECT}  - id: summarise\n    seat: writer\n    instruction: Write the note.\n    document: summary-note\n"
        ),
        &[("workflows/again.yaml", core_flow)],
    );
    let found = load(dir.path()).unwrap().findings;
    assert!(
        errors(&found).is_empty(),
        "one team, two workflows: {found:#?}"
    );
}
