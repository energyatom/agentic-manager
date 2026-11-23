//! Epic 3 story 3 (AD-35, AD-36; Decision 1; matrix row 26): a workflow
//! step's hand-off and loop keys.
//!
//! ```yaml
//! - id: check
//!   seat: third
//!   instruction: …
//!   loop_back: { to: draft, reasons: reasons, max: 2 }   # `reasons`, `max` default as shown
//!   limits: { iterations: 8, tool_calls: 20, wall_clock: 10m }
//! - id: approve
//!   seat: owner
//!   instruction: Approve or return the version.
//!   gate: review
//! ```
//!
//! `loop_back.to` names an earlier step of the same workflow; `max` is 0 to
//! 5 (default 2). `limits` sets the step attempt's caps: `iterations` 1 to
//! 50 (default 8), `tool_calls` 0 to 200 (default 20), `wall_clock` a
//! duration (`30s`, `10m`, `2h`) from 10 s to 2 h (default 10 min); the
//! defaults are the owner's placeholders (Question 7). `gate: review` makes
//! the step a human gate: it cannot be the first step and declares neither
//! `loop_back` nor `limits`. Every refusal is a finding with its file, line
//! and fix, so `agentic validate` names it and `agentic run` starts
//! nothing. The run-graph keys stay as 3.1 left them. Written before the
//! keys exist (today each is reported as an unknown key).
#![forbid(unsafe_code)]

mod common;

use std::path::Path;
use std::time::Duration;

use am_config::{Gate, LoopBack, StepLimits, load, load_in};
use common::{errors, fixture, project_with, snapshot};

const SEATS: [(&str, &str); 4] = [
    (
        "seats/first.md",
        "---\nschema_version: 2\nname: First\nrole: Writer\nteam: core\nmodel:\n  account: local-lmstudio\n  model: example-local\n---\nWrite it.\n",
    ),
    (
        "seats/second.md",
        "---\nschema_version: 2\nname: Second\nrole: Writer\nteam: core\nmodel:\n  account: local-lmstudio\n  model: example-local\n---\nEdit it.\n",
    ),
    (
        "seats/third.md",
        "---\nschema_version: 2\nname: Third\nrole: Reviewer\nteam: core\nmodel:\n  account: local-lmstudio\n  model: example-local\n---\nCheck it.\n",
    ),
    (
        "seats/owner.md",
        "---\nschema_version: 2\nname: Owner\nrole: Reviewer\nteam: core\ndescription: A person; no model.\n---\nA person fills this seat.\n",
    ),
];

const WF: &str = "workflows/flow.yaml";

/// A project with the four seats and `workflows/flow.yaml` holding `steps`
/// (each step's YAML lines, two-space indented under `steps:`).
fn flow(steps: &str) -> tempfile::TempDir {
    let text = format!("schema_version: 2\nname: Flow\nteam: core\nsteps:\n{steps}");
    let mut files: Vec<(&str, &str)> = SEATS.to_vec();
    files.push((WF, &text));
    project_with(&files)
}

const DRAFT: &str = "  - id: draft\n    seat: first\n    instruction: Write it.\n";
const EDIT: &str = "  - id: edit\n    seat: second\n    instruction: Edit it.\n";

/// The step `id` of the workflow `flow` in the project at `root`.
fn step_of(root: &Path, id: &str) -> am_config::Step {
    snapshot(root).workflows()["flow"]
        .steps
        .iter()
        .find(|s| s.id == id)
        .unwrap_or_else(|| panic!("no step {id}"))
        .clone()
}

/// The error findings of the project at `root` in `WF` whose message holds
/// every one of `words`; each has a fix, and the project gives no
/// snapshot.
fn refused<'a>(found: &'a [am_config::Finding], words: &[&str]) -> Vec<&'a am_config::Finding> {
    let hits: Vec<&am_config::Finding> = errors(found)
        .into_iter()
        .filter(|f| f.file == WF && words.iter().all(|w| f.message.contains(w)))
        .collect();
    for f in &hits {
        assert!(!f.fix.trim().is_empty(), "a fix: {f:#?}");
    }
    hits
}

/// Asserts one error in `WF` at the line holding `marker` whose message
/// holds every one of `words`, and no snapshot.
fn assert_refused(text_of_steps: &str, marker: &str, words: &[&str]) {
    let dir = flow(text_of_steps);
    let loaded = load(dir.path()).unwrap();
    let text = std::fs::read_to_string(dir.path().join(WF)).unwrap();
    let line = text
        .lines()
        .position(|l| l.contains(marker))
        .map(|i| i as u32 + 1)
        .unwrap_or_else(|| panic!("no `{marker}` in {text}"));
    let hits = refused(&loaded.findings, words);
    assert!(
        hits.iter().any(|f| f.line == line),
        "an error at {WF}:{line} naming {words:?}; found {:#?}",
        loaded.findings
    );
    for f in &loaded.findings {
        assert!(
            !f.message.contains("unknown key 'loop_back'")
                && !f.message.contains("unknown key 'gate'")
                && !f.message.contains("unknown key 'limits'"),
            "the keys are known: {f:#?}"
        );
    }
    assert!(loaded.snapshot.is_none(), "an error gives no snapshot");
}

/// Decision 1: the three keys parse, with their defaults.
#[test]
fn loop_back_gate_and_limits_parse_with_defaults() {
    let dir = flow(&format!(
        "{DRAFT}{EDIT}    limits: {{ iterations: 3 }}\n  - id: check\n    seat: third\n    instruction: Check it.\n    loop_back: {{ to: draft }}\n  - id: polish\n    seat: second\n    instruction: Polish it.\n    loop_back: {{ to: edit, reasons: issues, max: 5 }}\n    limits: {{ iterations: 50, tool_calls: 0, wall_clock: 2h }}\n  - id: approve\n    seat: owner\n    instruction: Approve it.\n    gate: review\n  - id: tidy\n    seat: first\n    instruction: Tidy it.\n    limits: {{ wall_clock: 30s, tool_calls: 200 }}\n"
    ));
    let found = load(dir.path()).unwrap().findings;
    assert!(errors(&found).is_empty(), "{found:#?}");
    let root = dir.path();

    assert_eq!(
        StepLimits::default(),
        StepLimits {
            iterations: 8,
            tool_calls: 20,
            wall_clock: Duration::from_secs(600),
        }
    );
    let draft = step_of(root, "draft");
    assert_eq!(
        (draft.loop_back, draft.gate, draft.limits),
        (None, None, StepLimits::default())
    );
    assert_eq!(
        step_of(root, "edit").limits,
        StepLimits {
            iterations: 3,
            ..StepLimits::default()
        }
    );
    assert_eq!(
        step_of(root, "check").loop_back,
        Some(LoopBack {
            to: "draft".into(),
            reasons: "reasons".into(),
            max: 2,
        })
    );
    let polish = step_of(root, "polish");
    assert_eq!(
        polish.loop_back,
        Some(LoopBack {
            to: "edit".into(),
            reasons: "issues".into(),
            max: 5,
        })
    );
    assert_eq!(
        polish.limits,
        StepLimits {
            iterations: 50,
            tool_calls: 0,
            wall_clock: Duration::from_secs(2 * 3600),
        }
    );
    let approve = step_of(root, "approve");
    assert_eq!(approve.gate, Some(Gate::Review));
    assert_eq!(approve.loop_back, None);
    assert_eq!(approve.limits, StepLimits::default());
    assert_eq!(
        step_of(root, "tidy").limits,
        StepLimits {
            iterations: 8,
            tool_calls: 200,
            wall_clock: Duration::from_secs(30),
        }
    );
    // None of them is a run-graph key (those stay refused by `agentic run`).
    for id in ["draft", "edit", "check", "polish", "approve", "tidy"] {
        assert!(step_of(root, id).run_graph.is_empty(), "{id}");
    }
}

/// Row 26: `loop_back.to` must name an earlier step of the workflow; an
/// unknown step, a later one or the step itself is refused at its line.
#[test]
fn a_loop_back_to_an_unknown_or_later_step_is_refused() {
    assert_refused(
        &format!(
            "{DRAFT}  - id: check\n    seat: third\n    instruction: Check it.\n    loop_back: {{ to: ghost }}\n"
        ),
        "to: ghost",
        &["loop_back", "ghost"],
    );
    assert_refused(
        &format!(
            "  - id: check\n    seat: third\n    instruction: Check it.\n    loop_back: {{ to: draft }}\n{DRAFT}"
        ),
        "to: draft",
        &["loop_back", "draft", "earlier"],
    );
    assert_refused(
        &format!(
            "{DRAFT}  - id: check\n    seat: third\n    instruction: Check it.\n    loop_back: {{ to: check }}\n"
        ),
        "to: check",
        &["loop_back", "check", "earlier"],
    );
    assert_refused(
        &format!(
            "{DRAFT}  - id: check\n    seat: third\n    instruction: Check it.\n    loop_back: {{ max: 2 }}\n"
        ),
        "max: 2",
        &["loop_back", "'to'"],
    );
    assert_refused(
        &format!(
            "{DRAFT}  - id: check\n    seat: third\n    instruction: Check it.\n    loop_back: {{ to: draft, maxx: 2 }}\n"
        ),
        "maxx",
        &["unknown key 'maxx'"],
    );
}

/// Row 26: `loop_back.max` is a whole number from 0 to 5.
#[test]
fn a_maximum_out_of_range_is_refused() {
    for max in ["6", "-1", "two"] {
        assert_refused(
            &format!(
                "{DRAFT}  - id: check\n    seat: third\n    instruction: Check it.\n    loop_back: {{ to: draft, max: {max} }}\n"
            ),
            &format!("max: {max}"),
            &["'max'"],
        );
    }
    // The ends of the range are fine.
    for max in [0, 5] {
        let dir = flow(&format!(
            "{DRAFT}  - id: check\n    seat: third\n    instruction: Check it.\n    loop_back: {{ to: draft, max: {max} }}\n"
        ));
        let found = load(dir.path()).unwrap().findings;
        assert!(errors(&found).is_empty(), "max {max}: {found:#?}");
    }
}

/// Row 26: a gate cannot be the first step, cannot declare `loop_back` or
/// `limits` (it makes no model call), and `review` is its one kind.
#[test]
fn a_gate_cannot_be_first_or_loop_back() {
    assert_refused(
        &format!(
            "  - id: approve\n    seat: owner\n    instruction: Approve it.\n    gate: review\n{DRAFT}"
        ),
        "gate: review",
        &["gate", "first"],
    );
    assert_refused(
        &format!(
            "{DRAFT}  - id: approve\n    seat: owner\n    instruction: Approve it.\n    gate: review\n    loop_back: {{ to: draft }}\n"
        ),
        "loop_back: { to: draft }",
        &["gate", "loop_back"],
    );
    assert_refused(
        &format!(
            "{DRAFT}  - id: approve\n    seat: owner\n    instruction: Approve it.\n    gate: review\n    limits: {{ iterations: 3 }}\n"
        ),
        "limits: { iterations: 3 }",
        &["gate", "limits"],
    );
    assert_refused(
        &format!(
            "{DRAFT}  - id: approve\n    seat: owner\n    instruction: Approve it.\n    gate: approve\n"
        ),
        "gate: approve",
        &["gate", "review"],
    );
}

/// Row 26: each cap is a whole number (or, for `wall_clock`, a duration)
/// in its range; an unknown key under `limits` is named with the near one.
#[test]
fn a_limit_out_of_range_is_refused() {
    for (limits, marker, words) in [
        ("{ iterations: 0 }", "iterations: 0", &["'iterations'"][..]),
        (
            "{ iterations: 51 }",
            "iterations: 51",
            &["'iterations'"][..],
        ),
        (
            "{ tool_calls: 201 }",
            "tool_calls: 201",
            &["'tool_calls'"][..],
        ),
        (
            "{ tool_calls: -1 }",
            "tool_calls: -1",
            &["'tool_calls'"][..],
        ),
        (
            "{ wall_clock: 5s }",
            "wall_clock: 5s",
            &["'wall_clock'"][..],
        ),
        (
            "{ wall_clock: 3h }",
            "wall_clock: 3h",
            &["'wall_clock'"][..],
        ),
        (
            "{ wall_clock: ten minutes }",
            "wall_clock: ten minutes",
            &["'wall_clock'"][..],
        ),
        (
            "{ iteration: 3 }",
            "iteration: 3",
            &["unknown key 'iteration'"][..],
        ),
    ] {
        assert_refused(
            &format!("{DRAFT}{EDIT}    limits: {limits}\n"),
            marker,
            words,
        );
    }
    // "did you mean" for the near key.
    let dir = flow(&format!("{DRAFT}{EDIT}    limits: {{ iteration: 3 }}\n"));
    let found = load(dir.path()).unwrap().findings;
    let hit = refused(&found, &["unknown key 'iteration'"]);
    assert!(
        hit.iter().any(|f| f.fix.contains("iterations")),
        "{found:#?}"
    );
    // The ends of each range are fine.
    for limits in [
        "{ iterations: 1, tool_calls: 0, wall_clock: 10s }",
        "{ iterations: 50, tool_calls: 200, wall_clock: 2h }",
        "{ wall_clock: 120m }",
    ] {
        let dir = flow(&format!("{DRAFT}{EDIT}    limits: {limits}\n"));
        let found = load(dir.path()).unwrap().findings;
        assert!(errors(&found).is_empty(), "{limits}: {found:#?}");
    }
}

/// The story's fixture `fixtures/projects/hand-offs` is a valid project (as
/// `agentic validate --template` checks it): `relay` loops back from
/// `check` to `draft`, `gated` holds a gate after its first step and
/// `capped` sets its step's caps.
#[test]
fn the_fixture_validates() {
    let root = fixture("hand-offs");
    let loaded = load_in(&root, am_config::LoadOptions::template(&root)).unwrap();
    assert!(loaded.findings.is_empty(), "{:#?}", loaded.findings);
    let snap = loaded.snapshot.expect("a snapshot");
    let wf = snap.workflows();
    assert_eq!(
        wf.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["capped", "gated", "relay"]
    );
    let relay: Vec<&am_config::Step> = wf["relay"].steps.iter().collect();
    assert_eq!(
        relay.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
        vec!["draft", "edit", "check"]
    );
    assert_eq!(
        relay[2].loop_back,
        Some(LoopBack {
            to: "draft".into(),
            reasons: "reasons".into(),
            max: 2,
        })
    );
    let gated = &wf["gated"].steps;
    assert_eq!(gated[1].id, "approve");
    assert_eq!(gated[1].gate, Some(Gate::Review));
    assert_eq!(gated[1].seat.as_deref(), Some("owner"));
    assert!(
        snap.seats()["owner"].model.is_none(),
        "the gate's seat has no model"
    );
    assert_eq!(
        wf["capped"].steps[0].limits,
        StepLimits {
            iterations: 3,
            tool_calls: 2,
            wall_clock: Duration::from_secs(30),
        }
    );
}
