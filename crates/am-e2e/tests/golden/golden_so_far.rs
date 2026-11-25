//! Epic 3 story 24, "Golden run so far on every PR" (owner quality plan
//! part 1, 2026-11-24; golden copy, owner 2026-11-25, Q1 and Q3; AD-23,
//! AD-33; the plan's matrix rows 1 to 17).
//!
//! The sample company is validated as `just validate` validates it, then
//! run through the shipped `agentic run` in process, twice per pass:
//!
//! - the real flow (the flow `fixtures/templates/sample.json` names), as
//!   the sample holds it: pinned to its refusal, with the step, the exit
//!   code, the words and no run row;
//! - the cut-down copy (`fixtures/golden/so-far/workflow.yaml`), the real
//!   flow with exactly the entries of `golden.json`'s removed list taken
//!   out, on one FakeLlm script per seat: its journal is the snapshot,
//!   compared with the hand-written `snapshot.txt`.
//!
//! The drift test proves the copy cannot drift from the real flow; the
//! fixture-only test that the copy is never a template. Each later story
//! that changes what the flow can run shrinks the removed list, edits the
//! copy and moves both expected results in its commit; story 3.13 deletes
//! the copy once the list is empty.
//!
//! `just check` runs this target (`cargo nextest run`) with no feature, no
//! ignore and no recipe of its own. No real model: FakeLlm, FakeClock
//! (through `RunContext.clock`), FakeProjectIdentity, FakeTerminal and a
//! temporary HOME per stage. No network and no keychain, structurally: the
//! run is wired with no provider but the router and a tripwire on an empty
//! `FakeEgress`, and with no secret port; what is proved is that wiring
//! (the tripwire records no request), not a count of sockets or keychain
//! calls. Every sample id is read from `fixtures/`; this folder spells none.
#![forbid(unsafe_code)]

#[path = "../engine/support/mod.rs"]
mod support;

#[path = "support/mod.rs"]
mod rig;

use std::collections::BTreeSet;
use std::sync::Arc;

use am_config::ModelRef;
use am_domain::{ChatRequest, Instruction, LlmProvider, Message, Part, Role};
use rig::*;

/// The `stop` lines of a snapshot.
fn stop_lines(snapshot: &str) -> Vec<String> {
    tagged(snapshot)
        .into_iter()
        .filter(|(tag, _)| tag == "stop")
        .map(|(_, line)| line)
        .collect()
}

/// A diff's `-` and `+` lines.
fn diff_lines(text: &str, sign: char) -> Vec<String> {
    let prefix = format!("  {sign} ");
    text.lines()
        .filter_map(|l| l.strip_prefix(prefix.as_str()))
        .map(str::to_string)
        .collect()
}

/// The word after `<name> ` in a prompt line (`bytes`, `sha256`).
fn word_after<'a>(line: &'a str, name: &str) -> &'a str {
    line.split_once(&format!(" {name} "))
        .and_then(|(_, rest)| rest.split_whitespace().next())
        .unwrap_or_else(|| panic!("no `{name}` in {line:?}"))
}

/// The byte count of a prompt line.
fn bytes_of(line: &str) -> usize {
    word_after(line, "bytes").parse().unwrap()
}

/// Rows 1, 3 to 6, 15 and 17: the sample validates with no error, the real
/// flow is refused, the copy runs to its end on the scripts, and the
/// normalised snapshot equals the hand-written one; every scripted turn is
/// served, one script per seat of the flow that has a model; the golden
/// sources spell no sample id.
#[tokio::test]
async fn golden_so_far() {
    let g = golden();

    let pass = pass(&g, Plant::None).await;

    assert_snapshot(&g.expected, &pass.snapshot);
    let snapshot = load(&g, None);
    let real = flow_of(&g, &snapshot);
    let with_models: BTreeSet<String> = seats_with_models(&snapshot, &real).into_keys().collect();
    let scripted: BTreeSet<String> = g.scripts.keys().cloned().collect();
    assert_eq!(
        scripted, with_models,
        "one script per seat of the flow that has a model (fixtures/golden/so-far/scripts/)"
    );
    assert!(
        flow_seats(&real).len() > with_models.len(),
        "the flow has a seat with no model, and it has no script"
    );
    for (seat, (served, total)) in &pass.counts {
        assert_eq!(
            served, total,
            "every scripted turn of `{seat}` is served: {served} of {total}"
        );
    }
    let spelled = spelled_ids(&g, &real);
    assert!(
        spelled.is_empty(),
        "the golden sources spell sample ids, which live only in fixtures/ (AD-23): {spelled:#?}"
    );
}

/// Row 2: the real flow, as the sample holds it, is refused before any run
/// row: exit 1, no run row in that copy's journal, the factory never
/// asked, and the step and words the expected snapshot pins.
#[tokio::test]
async fn the_real_flow_is_refused_before_any_run() {
    let g = golden();

    let pass = pass(&g, Plant::None).await;

    let stopped = &pass.stopped;
    assert_eq!(stopped.code, 1, "{}", stopped.err);
    assert_eq!(stopped.runs, 0, "no run row: {}", stopped.err);
    assert_eq!(stopped.asked, 0, "no provider was asked for");
    assert!(stopped.step.is_some(), "refused at a step: {}", stopped.err);
    assert!(
        stopped.err.contains("No run was started"),
        "{}",
        stopped.err
    );
    assert_eq!(
        stop_lines(&pass.snapshot),
        stop_lines(&g.expected),
        "the pinned refusal (fixtures/golden/so-far/snapshot.txt)"
    );
}

/// Row 7: two full passes, each in its own temporary folders and HOMEs,
/// give the same normalised snapshot.
#[tokio::test]
async fn the_snapshot_is_the_same_on_a_second_run() {
    let g = golden();

    let first = pass(&g, Plant::None).await;
    let second = pass(&g, Plant::None).await;

    if let Err(m) = compare(&first.snapshot, &second.snapshot) {
        panic!("two passes differ at {}:\n{}", m.tags.join(", "), m.text);
    }
}

/// Row 8: the first turn of the seat of the copy's first step gets other
/// text, in memory: the compare fails first at that step, showing both
/// output lines, and otherwise only at the step that receives that output
/// as its hand-on (review R5), at its trusted part.
#[tokio::test]
async fn a_planted_script_change_fails_naming_the_step() {
    let g = golden();
    if let Some(why) = plant_skip(&g, Plant::ScriptText) {
        eprintln!("skipped: {why}");
        return;
    }
    let copy = flow_of(&g, &load(&g, Some(&g.copy)));

    let pass = pass(&g, Plant::ScriptText).await;

    let m = compare(&g.expected, &pass.snapshot)
        .expect_err("a planted script change fails the compare");
    let first = format!("step {}", pass.first_step);
    let next = copy.steps.get(1).map(|s| format!("step {}", s.id));
    assert_eq!(m.tags.first(), Some(&first), "{}", m.text);
    assert!(
        m.tags[1..].iter().all(|t| Some(t) == next.as_ref()),
        "{}",
        m.text
    );
    assert!(
        m.text.contains(&format!("step `{}`:", pass.first_step)),
        "{}",
        m.text
    );
    let minus = diff_lines(&m.text, '-');
    let plus = diff_lines(&m.text, '+');
    let outputs = |lines: &[String]| -> Vec<String> {
        lines
            .iter()
            .filter(|l| l.contains(": output "))
            .cloned()
            .collect()
    };
    assert_eq!(outputs(&minus).len(), 1, "{}", m.text);
    assert_eq!(outputs(&plus).len(), 1, "{}", m.text);
    assert!(outputs(&plus)[0].ends_with(PLANTED), "{}", m.text);
    for line in minus
        .iter()
        .chain(&plus)
        .filter(|l| !l.contains(": output "))
    {
        assert!(
            line.contains(" trusted `"),
            "only the hand-on moves: {}",
            m.text
        );
    }
    assert_eq!(pass.code, 0, "the copy still runs to its end");
}

/// Row 9: the installed copy's first step gets a suffix on its instruction,
/// in the temporary project only: the compare fails at that step, at its
/// step instruction's byte count.
#[tokio::test]
async fn a_planted_change_to_the_copys_first_step_fails_naming_it() {
    let g = golden();

    let pass = pass(&g, Plant::CopyFirstStep).await;

    let m = compare(&g.expected, &pass.snapshot)
        .expect_err("a planted change to the copy's first step fails the compare");
    assert_eq!(
        m.tags,
        vec![format!("step {}", pass.first_step)],
        "{}",
        m.text
    );
    // The step instruction, as journaled and as sent (review R5).
    let minus = diff_lines(&m.text, '-');
    let plus = diff_lines(&m.text, '+');
    assert_eq!((minus.len(), plus.len()), (2, 2), "{}", m.text);
    assert!(minus[0].contains("`Step instruction`"), "{}", m.text);
    assert!(minus[1].contains(" user 1.1 instruction "), "{}", m.text);
    for (was, now) in minus.iter().zip(&plus) {
        assert_eq!(bytes_of(now), bytes_of(was) + PLANTED.len(), "{}", m.text);
        assert_ne!(
            word_after(now, "sha256"),
            word_after(was, "sha256"),
            "{}",
            m.text
        );
    }
    assert_eq!(
        golden().copy,
        g.copy,
        "the fixture is never changed by a plant"
    );
}

/// Review R5: the steps are journaled with their inputs by label only
/// (AD-43), so the snapshot also holds each request the model was sent,
/// part by part, fingerprinted. When every hand-on reaches its step empty
/// (the router records it so), the compare fails at each step that
/// receives one, and nowhere else.
#[tokio::test]
async fn an_empty_hand_on_fails_naming_the_steps_that_receive_it() {
    let g = golden();
    if let Some(why) = plant_skip(&g, Plant::EmptyHandOn) {
        eprintln!("skipped: {why}");
        return;
    }
    let copy = flow_of(&g, &load(&g, Some(&g.copy)));

    let pass = pass(&g, Plant::EmptyHandOn).await;

    let m = compare(&g.expected, &pass.snapshot).expect_err("an empty hand-on fails the compare");
    let receiving: Vec<String> = copy.steps[1..]
        .iter()
        .map(|s| format!("step {}", s.id))
        .collect();
    assert_eq!(m.tags, receiving, "{}", m.text);
    for line in diff_lines(&m.text, '+') {
        assert!(line.contains(" trusted `"), "{}", m.text);
        assert!(line.contains(" bytes 0 "), "{}", m.text);
    }
}

/// Row 10: the real flow's first step loses its run-graph keys, in that
/// stage's copy of the sample only: the compare fails at `stop`, where the
/// expected line names the first step and the actual one the next refused
/// step; the copy's run is unchanged.
#[tokio::test]
async fn a_planted_change_to_the_real_flows_first_step_fails_naming_it() {
    let g = golden();
    if let Some(why) = plant_skip(&g, Plant::RealFirstStep) {
        eprintln!("skipped: {why}");
        return;
    }
    let first = flow_of(&g, &load(&g, None)).steps[0].id.clone();

    let pass = pass(&g, Plant::RealFirstStep).await;

    let m = compare(&g.expected, &pass.snapshot)
        .expect_err("a planted change to the real flow fails the compare");
    assert!(m.tags.contains(&"stop".to_string()), "{}", m.text);
    assert!(
        m.tags.iter().all(|t| !t.starts_with("step ")),
        "the copy's run is unchanged: {}",
        m.text
    );
    assert!(
        diff_lines(&m.text, '-').contains(&format!("stop: step {first}")),
        "{}",
        m.text
    );
    let moved = diff_lines(&m.text, '+')
        .into_iter()
        .find(|l| l.starts_with("stop: step "))
        .unwrap_or_else(|| panic!("the actual stop names a step: {}", m.text));
    assert_ne!(moved, format!("stop: step {first}"), "{}", m.text);
    assert_eq!(pass.stopped.runs, 0, "still refused before a run row");
}

/// Row 11 and the owner's drift conditions (Q1, Q3): the copy equals the
/// real flow minus exactly the listed keys and steps, as text (byte for
/// byte) and as am-config loads it; every entry names a restoring story.
/// Review R2: the planted cases pick their targets from the list's shape,
/// and run on the shipped list and on the shapes it takes as stories land
/// (keys only, whole steps only, one entry, none), each copy cut from the
/// real flow in memory; a case a shape does not allow is skipped, in words.
#[test]
fn the_copy_is_the_real_flow_minus_the_removed_list() {
    let g = golden();
    let real_text = g.real();
    let book = g.story_book();
    let real = flow_of(&g, &load(&g, None));
    let copy = flow_of(&g, &load(&g, Some(&g.copy)));

    let shipped: Vec<String> = text_drift(&real_text, &g.copy, &g.list)
        .into_iter()
        .chain(structure_drift(&real, &copy, &g.list))
        .chain(list_problems(&g.list, &real_text, &book))
        .collect();
    assert!(
        shipped.is_empty(),
        "the shipped copy and list drift: {shipped:#?}"
    );

    let mut keys_only = g.list.clone();
    keys_only.removed_steps.clear();
    let mut steps_only = g.list.clone();
    steps_only.removed_keys.clear();
    let mut one = g.list.clone();
    one.removed_keys.truncate(1);
    one.removed_steps.clear();
    let mut none = g.list.clone();
    none.removed_keys.clear();
    none.removed_steps.clear();
    let shapes = [
        ("the shipped list", g.list.clone(), g.copy.clone()),
        ("its keys only", keys_only.clone(), String::new()),
        ("its whole steps only", steps_only.clone(), String::new()),
        ("its first key only", one.clone(), String::new()),
        ("an empty list", none.clone(), String::new()),
    ];
    let mut missed = Vec::new();
    for (shape, list, copy_text) in shapes {
        let copy_text = if copy_text.is_empty() {
            cut(&real_text, &list).unwrap().text
        } else {
            copy_text
        };
        let planted = planted_drift_cases(&g, &list, &copy_text, &book);
        for why in &planted.skipped {
            eprintln!("{shape}: skipped: {why}");
        }
        missed.extend(planted.missed.iter().map(|m| format!("{shape}: {m}")));
    }
    assert!(missed.is_empty(), "{missed:#?}");
}

/// Review R4: a restoring story is a story of the epic that comes after
/// this one in `tickets.toml`'s table order (the build order) and is not
/// built (its plan says `built` or `done`). Read from the repository, and
/// planted on a book in memory.
#[test]
fn a_restoring_story_comes_after_this_one_and_is_not_built() {
    let g = golden();
    let book = g.story_book();
    let tag = |id: u32| vec![format!("{}.{id}", book.epic)];

    let restoring = book.restoring();
    assert!(!restoring.is_empty(), "{book:?}");
    for entry in entries(&g.list) {
        let until = match &entry {
            Entry::Key(k) => &k.until,
            Entry::Step(s) => &s.until,
        };
        assert!(
            story_problems("x", until, &book).is_empty(),
            "`{}` names a story that may restore it",
            entry.name()
        );
    }
    let own = book.order.iter().position(|&id| id == book.own).unwrap();
    let built_before = book.order[..own]
        .iter()
        .rev()
        .find(|id| book.built.contains(id))
        .copied()
        .expect("a built story before this one");
    let problems = story_problems("the planted entry", &tag(built_before), &book);
    assert!(
        problems.iter().any(|p| p.contains("does not come after")),
        "{problems:?}"
    );

    let planted = StoryBook {
        epic: 7,
        own: 30,
        order: vec![5, 30, 12, 40],
        built: [5, 12].into_iter().collect(),
    };
    let said = |id: u32| story_problems("the entry", &[format!("7.{id}")], &planted).join("; ");
    assert!(
        said(5).contains("does not come after story 7.30"),
        "{}",
        said(5)
    );
    assert!(said(30).contains("does not come after"), "{}", said(30));
    assert!(said(12).contains("is built already"), "{}", said(12));
    assert!(
        said(99).contains("is not a story of epic 7"),
        "{}",
        said(99)
    );
    assert_eq!(said(40), "", "after this one and not built");
    assert_eq!(planted.restoring(), vec![40]);
    assert!(
        story_problems("the entry", &["8.40".to_string()], &planted)
            .join("")
            .contains("is not a story of epic 7")
    );
}

/// Review R1 (the list only shrinks): each entry of the removed list, put
/// back alone into the copy, is refused by `agentic run` at that entry's
/// step (and, for a key, naming that key), before any run row and with no
/// model call. A stale entry, one a story has made runnable, fails and
/// names itself; shown on a planted entry naming a key `agentic run`
/// honours today.
#[tokio::test]
async fn each_removed_entry_put_back_alone_is_refused_at_its_step() {
    let g = golden();

    let mut problems = Vec::new();
    for entry in entries(&g.list) {
        if let Err(why) = put_back(&g, &g.list, &entry).await {
            problems.push(why);
        }
    }

    assert!(problems.is_empty(), "{problems:#?}");
    let copy = flow_of(&g, &load(&g, Some(&g.copy)));
    let step = copy.steps.last().unwrap().id.clone();
    let stale = RemovedKey {
        step: step.clone(),
        key: "instruction".into(),
        until: Vec::new(),
    };
    let mut planted = g.list.clone();
    planted.removed_keys.push(stale.clone());
    let why = put_back(&g, &planted, &Entry::Key(stale))
        .await
        .expect_err("a stale entry fails");
    assert!(why.contains(&format!("`{step}.instruction`")), "{why}");
}

/// Row 12: the removed list tracks what the product refuses. Each removed
/// key is a run-graph key of its step; no step before the real flow's stop
/// step has one; the stop step's listed keys are the keys its refusal
/// names; and no removed step comes before it. So the story that honours a
/// key fails here until it shrinks the list and edits the copy.
#[tokio::test]
async fn the_removed_list_is_what_the_real_flow_refuses() {
    let g = golden();
    let real = flow_of(&g, &load(&g, None));

    let pass = pass(&g, Plant::None).await;

    let problems = tracks_product(&g.list, &real, &pass.stopped);
    assert!(problems.is_empty(), "{problems:#?}");

    // A list that no longer takes out the key the refusal names.
    let step = pass.stopped.step.clone().unwrap();
    let mut honoured = g.list.clone();
    honoured.removed_keys.retain(|k| k.step != step);
    let problems = tracks_product(&honoured, &real, &pass.stopped);
    assert!(
        problems.iter().any(|p| p.contains(&format!("`{step}`"))),
        "{problems:#?}"
    );

    // A list that takes out a key `agentic run` honours (not a run-graph
    // key).
    let mut plain = g.list.clone();
    plain.removed_keys.push(RemovedKey {
        step: step.clone(),
        key: "instruction".into(),
        until: Vec::new(),
    });
    let problems = tracks_product(&plain, &real, &pass.stopped);
    assert!(
        problems
            .iter()
            .any(|p| p.contains("`agentic run` honours it")),
        "{problems:#?}"
    );
}

/// Row 13: the copy and its list are fixtures only, never a template: they
/// sit in `fixtures/golden/so-far/`, which holds no `company.yaml`; no file
/// under `templates/` has their bytes; `just validate`'s list and globs
/// never reach that folder; and `agentic validate --template` does not take
/// it as a template.
#[test]
fn the_copy_is_a_fixture_never_a_template() {
    let problems = fixture_only_problems();
    assert!(problems.is_empty(), "{problems:#?}");

    assert!(glob_match("fixtures/*/so-far/", "fixtures/golden/so-far/"));
    assert!(glob_match("fixtures/golden/*/", "fixtures/golden/so-far/"));
    assert!(!glob_match(
        "fixtures/projects/valid*/",
        "fixtures/golden/so-far/"
    ));
    assert!(!glob_match(
        "templates/samples/*/",
        "fixtures/golden/so-far/"
    ));

    let ran = validate(&[
        "--template".to_string(),
        home().to_string_lossy().into_owned(),
    ]);
    assert_ne!(ran.code, 0, "{}{}", ran.out, ran.err);
    assert!(
        ran.out.contains("company.yaml") || ran.err.contains("company.yaml"),
        "not a project or a template: {}{}",
        ran.out,
        ran.err
    );
}

/// Row 16 (review R6: structural, like the keychain claim): no model, and
/// no network or keychain wired on the run's path. The factory was asked
/// only for the flow's seats' primary models (the router), and the
/// tripwire's egress, the one egress on the path, holds no request after a
/// pass; the tripwire does record one when it is asked (a fallback model),
/// with no credential. `RunContext` is built field by field in the rig
/// (`run_flow`), so no secret port exists on this path: a later one fails
/// to compile there until the golden run fakes it and counts zero keychain
/// calls (register DR-173). Nothing here counts sockets or keychain calls.
#[tokio::test]
async fn no_network_or_keychain_is_wired_on_the_runs_path() {
    let g = golden();

    let pass = pass(&g, Plant::None).await;

    assert_eq!(pass.requests, 0, "no request reached the tripwire");
    assert!(
        !pass.asked.is_empty(),
        "the copy's run asked for its models"
    );
    for model in &pass.asked {
        assert!(
            pass.primaries.contains(model),
            "asked only for the flow's seats' primary models, not {model:?}"
        );
    }

    let snapshot = load(&g, None);
    let real = flow_of(&g, &snapshot);
    let fallback = seats_with_models(&snapshot, &real)
        .into_keys()
        .flat_map(|seat| snapshot.seats()[&seat].fallback.clone())
        .find(|m| {
            !pass
                .primaries
                .contains(&(m.account.clone(), m.model.clone()))
        })
        .expect("a seat of the flow has a fallback model");
    let factory = Factory::new(Router::new(&snapshot, &g.scripts), pass.primaries.clone());
    let provider: Arc<dyn LlmProvider> = (factory.providers())(&ModelRef {
        account: fallback.account.clone(),
        model: fallback.model.clone(),
    })
    .unwrap();
    assert_eq!(provider.account_id(), TRIPWIRE);
    let request = ChatRequest {
        model: fallback.model.clone(),
        system: vec![],
        messages: vec![Message {
            role: Role::User,
            parts: vec![Part::Instruction(Instruction::authored("tripwire"))],
        }],
        tools: vec![],
        max_output_tokens: 16,
    };
    let sent = provider
        .stream(
            request,
            am_engine::TokenSignal::signal(tokio_util::sync::CancellationToken::new()),
            Arc::new(am_testkit::CollectEvents::default()),
        )
        .await;
    assert!(sent.is_err(), "the tripwire's egress has no route");
    let requests = factory.egress.requests();
    assert_eq!(requests.len(), 1, "the tripwire records the request");
    assert!(
        requests[0].url.starts_with(TRIPWIRE_URL),
        "{}",
        requests[0].url
    );
    assert!(requests[0].auth.is_none(), "with no credential");
}
