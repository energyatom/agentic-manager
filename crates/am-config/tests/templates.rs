//! Story 1.5 acceptance tests: the shipped templates under `templates/` (the
//! Blank department, the eight role templates and the optional sample company)
//! load through the generic loader with no findings, and the sample holds what
//! the reference workload defines. Written before the templates exist.
//!
//! The sample's ids are department vocabulary and may not appear in `crates/`
//! (AD-23), so its expectations are read from `fixtures/templates/sample.json`.
#![forbid(unsafe_code)]

mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use am_config::{ConfigSnapshot, Seat, SourceOutput, Trigger, load};
use am_domain::capability::Capability;
use am_domain::workspace::Access;
use common::set;
use serde_json::Value;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn templates() -> PathBuf {
    repo().join("templates")
}

/// The sample's expectations (vocabulary lives under fixtures/).
fn expect() -> Value {
    let path = repo().join("fixtures/templates/sample.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap()
}

fn sample() -> PathBuf {
    templates().join("samples").join(s(&expect()["sample"]))
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap_or_else(|| panic!("not a string: {v}"))
}

fn strs(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap_or_else(|| panic!("not a list: {v}"))
        .iter()
        .map(|x| s(x).to_string())
        .collect()
}

fn ids<'a>(v: impl IntoIterator<Item = &'a String>) -> BTreeSet<String> {
    v.into_iter().cloned().collect()
}

/// Loads a template folder and requires no finding at all (no error, no
/// warning, so no YAML comment either).
fn clean_snapshot(root: &Path) -> ConfigSnapshot {
    assert!(root.is_dir(), "{} does not exist", root.display());
    let loaded = load(root).expect("the template folder exists");
    assert!(
        loaded.findings.is_empty(),
        "{}: {:#?}",
        root.display(),
        loaded.findings
    );
    loaded.snapshot.expect("no errors gives a snapshot")
}

/// A template YAML file as data, for the run-graph keys the loader accepts
/// but does not keep yet (AD-40..AD-42).
fn yaml(path: &Path) -> Value {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_saphyr::from_str::<Value>(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The frontmatter of a seat file as data.
fn frontmatter(path: &Path) -> Value {
    let text = std::fs::read_to_string(path).unwrap();
    let rest = text.strip_prefix("---\n").expect("frontmatter opens");
    let end = rest.find("\n---\n").expect("frontmatter closes");
    serde_saphyr::from_str::<Value>(&rest[..end]).unwrap()
}

fn workflow_file(id: &str) -> PathBuf {
    sample().join("workflows").join(format!("{id}.yaml"))
}

fn steps(workflow: &Value) -> Vec<Value> {
    workflow["steps"].as_array().expect("steps").clone()
}

/// Micro-units of the company currency.
fn money(v: &Value) -> Option<i64> {
    Some(v.as_i64().expect("whole units") * 1_000_000)
}

fn seat<'a>(snap: &'a ConfigSnapshot, id: &str) -> &'a Seat {
    snap.seats()
        .get(id)
        .unwrap_or_else(|| panic!("no seat {id}"))
}

// Row: Blank department.

#[test]
fn blank_department_validates_as_one_team() {
    let snap = clean_snapshot(&templates().join("departments/blank"));
    assert_eq!(snap.company().teams.len(), 1, "one team");
    assert!(snap.seats().is_empty(), "no seats");
    assert!(snap.workflows().is_empty(), "no workflows");
}

#[test]
fn blank_is_the_only_department_template() {
    let names: BTreeSet<String> = std::fs::read_dir(templates().join("departments"))
        .expect("templates/departments exists")
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with('.'))
        .collect();
    assert_eq!(names, set(&["blank"]), "no domain department template");
}

// Row: each role template.

/// (folder and seat id, role name, capabilities on besides folder_access).
const ROLES: &[(&str, &str, &[Capability])] = &[
    ("head", "Head", &[]),
    (
        "coordinator",
        "Coordinator",
        &[Capability::HandoffAsk, Capability::Memory],
    ),
    (
        "analyst",
        "Analyst",
        &[Capability::Tables, Capability::Calculate],
    ),
    (
        "researcher",
        "Researcher",
        &[Capability::WebSearch, Capability::Browse],
    ),
    ("reviewer", "Reviewer", &[]),
    ("writer", "Writer", &[]),
    ("specialist", "Specialist", &[]),
    ("human", "Human seat", &[]),
];

#[test]
fn the_role_templates_are_exactly_the_eight_generic_roles() {
    let names: BTreeSet<String> = std::fs::read_dir(templates().join("roles"))
        .expect("templates/roles exists")
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with('.'))
        .collect();
    let want: Vec<&str> = ROLES.iter().map(|r| r.0).collect();
    assert_eq!(names, set(&want));
}

#[test]
fn every_role_template_validates_with_its_fields() {
    for (id, role, extra) in ROLES {
        let root = templates().join("roles").join(id);
        let snap = clean_snapshot(&root);
        assert_eq!(snap.seats().len(), 1, "{id}: one seat");
        let seat = seat(&snap, id);
        assert_eq!(seat.role, *role, "{id}");
        assert!(!seat.instructions.trim().is_empty(), "{id}: instructions");
        assert!(
            seat.goal.as_deref().is_some_and(|g| !g.trim().is_empty()),
            "{id}: goal"
        );
        assert!(
            seat.backstory
                .as_deref()
                .is_some_and(|b| !b.trim().is_empty()),
            "{id}: backstory"
        );
        assert!(seat.budget.is_some(), "{id}: default budget");
        assert!(seat.skills.is_empty(), "{id}: skills empty");
        assert!(seat.knowledge.is_empty(), "{id}: knowledge empty");
        assert!(seat.links.is_empty(), "{id}: links empty");
        // The empty lists are written out, so the installer copies them.
        let front = frontmatter(&root.join(format!("seats/{id}.md")));
        for key in ["skills", "knowledge", "links", "tools", "capabilities"] {
            assert!(!front[key].is_null(), "{id}: `{key}` is written");
        }
        assert!(
            front["capabilities"]["folder_access"]["on"] == true,
            "{id}: folder_access is written on"
        );
        for (cap, setting) in &seat.capabilities {
            let want = *cap == Capability::FolderAccess || extra.contains(cap);
            assert_eq!(setting.on, want, "{id}: {cap:?}");
            assert_eq!(setting.provider, "builtin", "{id}: {cap:?}");
        }
        if *id == "human" {
            assert!(seat.model.is_none(), "a human seat has no model");
            assert!(seat.fallback.is_empty(), "a human seat has no fallback");
        } else {
            assert!(seat.model.is_some(), "{id}: model");
            assert!(!seat.fallback.is_empty(), "{id}: ordered fallback");
        }
        if *id == "coordinator" {
            assert_eq!(snap.workflows().len(), 1, "the learning workflow");
            let learning = snap.workflows().values().next().unwrap();
            assert!(learning.triggers.is_empty(), "manual only");
            assert!(
                learning
                    .steps
                    .iter()
                    .all(|s| s.seat.as_deref() == Some("coordinator")),
                "{learning:#?}"
            );
            assert!(seat.grants.contains("propose_change"), "proposes changes");
            assert!(
                seat.description
                    .as_deref()
                    .is_some_and(|d| d.contains("Ask a seat")),
                "the default seat for Ask a seat"
            );
        } else {
            assert!(snap.workflows().is_empty(), "{id}: no workflow");
        }
    }
}

// Row: sample.

#[test]
fn sample_validates_and_holds_its_people_and_teams() {
    let e = expect();
    let snap = clean_snapshot(&sample());
    let owner = s(&e["owner"]);
    let mut want: BTreeSet<String> = BTreeSet::new();
    for team in e["teams"].as_array().unwrap() {
        want.extend(strs(&team["seats"]));
    }
    assert_eq!(want.len(), 9, "nine AI seats");
    want.insert(owner.to_string());
    assert_eq!(ids(snap.seats().keys()), want, "the AI seats and the Owner");

    let o = seat(&snap, owner);
    assert!(o.model.is_none(), "the Owner is a human seat");
    assert_eq!(o.role, s(&e["owner_role"]));
    assert_eq!(o.team, s(&e["owner_team"]), "the Owner's own unit");
    let hats: Vec<String> = snap.company().hats.iter().map(|h| h.name.clone()).collect();
    assert_eq!(hats, strs(&e["hats"]), "the Owner's hats");

    let teams = &snap.company().teams;
    let want_teams = e["teams"].as_array().unwrap();
    assert_eq!(teams.len(), 3, "two departments and the Owner's unit");
    assert_eq!(teams.len(), want_teams.len());
    for (team, want) in teams.iter().zip(want_teams) {
        assert_eq!(team.id, s(&want["id"]));
        assert_eq!(team.accepts, strs(&want["accepts"]), "{}", team.id);
        for id in strs(&want["seats"]) {
            let seat = seat(&snap, &id);
            assert_eq!(seat.team, team.id, "{id}");
            assert!(seat.goal.is_some() && seat.backstory.is_some(), "{id}");
            assert!(seat.avatar.is_some(), "{id}: avatar tint");
            assert!(seat.model.is_some(), "{id}: model");
            assert!(!seat.fallback.is_empty(), "{id}: ordered model list");
            assert!(seat.budget.is_some(), "{id}: budget");
        }
    }
    for head in strs(&e["heads"]) {
        assert_eq!(seat(&snap, &head).reports_to.as_deref(), Some(owner));
    }
    // RULE-20: every seat's model is local; the reference cloud seats keep
    // their cloud model as the first fallback.
    let is_local = |account: &str| account == "local-lmstudio" || account == "local-ollama";
    for id in strs(&e["local"])
        .into_iter()
        .chain(strs(&e["cloud_fallback"]))
    {
        let model = seat(&snap, &id).model.as_ref().unwrap();
        assert!(is_local(&model.account), "{id} runs on a local model");
    }
    for id in strs(&e["cloud_fallback"]) {
        let first = &seat(&snap, &id).fallback[0];
        assert!(
            !is_local(&first.account),
            "{id}: cloud model is the fallback"
        );
    }
    for account in snap.accounts() {
        assert!(account.secret_ref.is_some(), "{}: secret_ref", account.id);
    }
    assert_eq!(snap.company().currency, s(&e["currency"]));
}

#[test]
fn sample_table_has_its_key_transitions_view_and_grant() {
    let e = &expect()["table"];
    let snap = clean_snapshot(&sample());
    let table = &snap.tables()[s(&e["id"])];
    assert_eq!(table.team, s(&e["team"]));
    assert_eq!(table.key.as_deref(), Some(s(&e["key"])));
    for column in strs(&e["columns"]) {
        assert!(table.columns.iter().any(|c| c.id == column), "{column}");
    }
    let status = table
        .columns
        .iter()
        .find(|c| c.column_type == "status")
        .expect("a status column");
    assert_eq!(status.values, strs(&e["values"]));
    let pairs: BTreeSet<(String, String)> = status
        .transitions
        .iter()
        .map(|t| (t.from.clone(), t.to.clone()))
        .collect();
    let want: BTreeSet<(String, String)> = e["transitions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            let p = strs(p);
            (p[0].clone(), p[1].clone())
        })
        .collect();
    assert_eq!(pairs, want);
    assert!(
        table.views.iter().any(|v| v.name == s(&e["view"])),
        "{:?}",
        table.views
    );
    assert_eq!(table.grants, strs(&e["grants"]), "the read grant");
}

#[test]
fn sample_workflows_have_their_triggers_and_caps() {
    let e = expect();
    let snap = clean_snapshot(&sample());
    let want: BTreeSet<String> = e["workflows"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    assert_eq!(ids(snap.workflows().keys()), want);
    for (id, team) in e["workflows"].as_object().unwrap() {
        let wf = &snap.workflows()[id];
        assert_eq!(wf.team, s(team), "{id}");
        let caps = wf.caps.as_ref().unwrap_or_else(|| panic!("{id}: caps"));
        assert_eq!(
            caps.scheduled_per_run,
            money(&e["caps"]["scheduled_per_run"]),
            "{id}"
        );
        assert_eq!(caps.per_run, money(&e["caps"]["per_run"]), "{id}");
        assert_eq!(caps.per_week, money(&e["caps"]["per_week"]), "{id}");
    }
    let sched = &e["schedule"];
    assert_eq!(
        snap.workflows()[s(&sched["workflow"])].triggers,
        [Trigger::Schedule {
            cron: s(&sched["cron"]).into(),
            timezone: None,
            catch_up: s(&sched["catch_up"]).into(),
        }]
    );
    for (id, on) in e["events"].as_object().unwrap() {
        assert_eq!(
            snap.workflows()[id].triggers,
            [Trigger::Event {
                on: s(on).into(),
                only_if: "succeeded".into(),
            }],
            "{id}"
        );
    }
    for id in strs(&e["manual"]) {
        assert!(snap.workflows()[&id].triggers.is_empty(), "{id}: manual");
    }
}

#[test]
fn sample_cross_department_flow_fans_out_hands_off_and_has_both_gates() {
    let e = &expect()["report_flow"];
    let path = workflow_file(s(&e["workflow"]));
    let steps = steps(&yaml(&path));
    let fan_out = steps
        .iter()
        .find(|s| s["seats"].is_array())
        .expect("a fan-out step");
    assert_eq!(strs(&fan_out["seats"]), strs(&e["fan_out"]));
    let fan_in = steps
        .iter()
        .find(|s| s["join"].is_object())
        .expect("a fan-in step");
    assert_eq!(fan_in["seat"], e["fan_in"]);
    assert_eq!(fan_in["join"]["from"], fan_out["id"]);
    assert_eq!(fan_in["join"]["quorum"], e["quorum"]);
    assert_eq!(fan_in["join"]["timeout"], e["timeout"]);
    assert_eq!(fan_in["join"]["on_branch_failure"], e["on_branch_failure"]);
    let handoffs: Vec<&str> = steps
        .iter()
        .filter_map(|s| s["handoff"]["to"].as_str())
        .collect();
    for target in strs(&e["handoffs_to"]) {
        assert!(
            handoffs.contains(&target.as_str()),
            "a hand-off into {target}: {handoffs:?}"
        );
    }
    let reviewed = steps
        .iter()
        .find(|s| s["handoff"]["review"]["seat"] == e["reviewer"])
        .expect("the review gate on a hand-off");
    assert_eq!(reviewed["seat"], e["report_seat"]);
    assert_eq!(
        reviewed["handoff"]["review"]["max_returns"],
        e["max_returns"]
    );
    // The Owner's human step is the Inbox gate itself.
    let last = steps.last().unwrap();
    assert_eq!(last["seat"], e["last_seat"]);
    assert!(last["review"].is_null(), "no step-level review: {last:#}");
    let snap = clean_snapshot(&sample());
    assert!(
        seat(&snap, s(&e["last_seat"])).model.is_none(),
        "a human seat"
    );
    // The producing step moves the rows it used to handed-off.
    let produce = steps
        .iter()
        .find(|st| st["id"] == e["producer"])
        .expect("the producing step");
    let instruction = s(&produce["instruction"]);
    assert!(
        instruction.contains("handed-off") && instruction.contains("table_write"),
        "{instruction}"
    );
    let text = std::fs::read_to_string(&path).unwrap();
    for doc in strs(&e["documents"]) {
        assert!(text.contains(&doc), "{doc}");
    }
}

#[test]
fn sample_chained_and_folder_flows_name_their_seats_folder_and_documents() {
    let e = expect();
    let f2 = &e["chained_flows"];
    let update = steps(&yaml(&workflow_file(s(&f2["update"]))));
    let mut seats: Vec<String> = Vec::new();
    for step in &update {
        match step["seats"].as_array() {
            Some(_) => seats.extend(strs(&step["seats"])),
            None => seats.push(s(&step["seat"]).to_string()),
        }
    }
    let snap = clean_snapshot(&sample());
    for id in strs(&f2["update_seats"]) {
        assert!(seats.contains(&id), "{id} in {seats:?}");
        let instructions = &seat(&snap, &id).instructions;
        assert!(
            instructions.contains("Your share"),
            "{id}: its share of rows"
        );
    }
    let follow = std::fs::read_to_string(workflow_file(s(&f2["follow"]))).unwrap();
    for name in strs(&f2["follow_names"]) {
        assert!(follow.contains(&name), "{name}");
    }

    let f3 = &e["folder_flow"];
    let path = workflow_file(s(&f3["workflow"]));
    let steps = steps(&yaml(&path));
    let read = steps
        .iter()
        .find(|s| s["over_folder"].is_string())
        .expect("a fan-out over a folder");
    assert_eq!(read["over_folder"], f3["folder"]);
    let routes = read["routes"].as_array().expect("routes");
    assert_eq!(routes.len(), f3["routes"].as_array().unwrap().len());
    for want in f3["routes"].as_array().unwrap() {
        let route = routes
            .iter()
            .find(|r| r["seat"] == want["seat"])
            .unwrap_or_else(|| panic!("a route to {}", want["seat"]));
        assert_eq!(strs(&route["match"]), strs(&want["match"]));
    }
    let gather = steps
        .iter()
        .find(|s| s["join"].is_object())
        .expect("a fan-in step");
    assert_eq!(gather["seat"], f3["gatherer"]);
    assert_eq!(gather["join"]["from"], read["id"]);
    assert_eq!(gather["join"]["timeout"], f3["timeout"]);
    assert_eq!(gather["join"]["on_branch_failure"], f3["on_branch_failure"]);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains(s(&f3["document"])), "{text}");
}

#[test]
fn sample_folder_is_linked_read_only_to_four_seats() {
    let f3 = &expect()["folder_flow"];
    let snap = clean_snapshot(&sample());
    let mut linked = BTreeSet::new();
    for (id, seat) in snap.seats() {
        for folder in &seat.folders {
            assert_eq!(folder.id, s(&f3["folder"]), "{id}");
            assert_eq!(folder.path, s(&f3["path"]), "{id}");
            assert_eq!(folder.access, Access::Read, "{id}: read-only");
            linked.insert(id.clone());
        }
    }
    assert_eq!(linked.len(), 4);
    assert_eq!(linked, ids(&strs(&f3["linked"])));
    let fallback = strs(&f3["text_fallback"]);
    for id in &linked {
        assert_eq!(seat(&snap, id).text_fallback, fallback.contains(id), "{id}");
    }
}

#[test]
fn sample_folder_holds_the_synthetic_files() {
    let e = expect();
    let dir = sample().join(s(&e["folder_flow"]["path"]));
    let mut by_ext: std::collections::BTreeMap<String, Vec<PathBuf>> = Default::default();
    for entry in std::fs::read_dir(&dir).expect("the sample folder exists") {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let stem = name.split('.').next().unwrap();
        assert!(!stem.contains('-'), "{name}: single-word file names");
        let ext = path.extension().unwrap().to_string_lossy().into_owned();
        by_ext.entry(ext).or_default().push(path);
    }
    let want = e["files"].as_object().unwrap();
    assert_eq!(by_ext.len(), want.len(), "nothing else: {by_ext:?}");
    for (ext, count) in want {
        assert_eq!(
            by_ext.get(ext).map_or(0, Vec::len) as u64,
            count.as_u64().unwrap(),
            "{ext}: {by_ext:?}"
        );
    }
    // One PDF has a text layer and one has none.
    let text_layers: Vec<bool> = by_ext["pdf"]
        .iter()
        .map(|p| {
            let bytes = std::fs::read(p).unwrap();
            assert!(bytes.starts_with(b"%PDF-"), "{}", p.display());
            String::from_utf8_lossy(&bytes).contains("/Font")
        })
        .collect();
    assert!(text_layers.contains(&true) && text_layers.contains(&false));
    for p in &by_ext["png"] {
        assert!(std::fs::read(p).unwrap().starts_with(b"\x89PNG\r\n\x1a\n"));
    }
    for p in &by_ext["docx"] {
        assert!(std::fs::read(p).unwrap().starts_with(b"PK\x03\x04"));
    }
}

// Row: grants by capability.

#[test]
fn sample_grants_follow_the_capabilities_on_the_builtin_provider() {
    let e = expect();
    let snap = clean_snapshot(&sample());
    for id in strs(&e["capabilities"]["web_search_and_browse"]) {
        let seat = seat(&snap, &id);
        assert!(seat.capabilities[&Capability::WebSearch].on, "{id}");
        assert!(seat.capabilities[&Capability::Browse].on, "{id}");
    }
    for id in strs(&e["capabilities"]["tables"]) {
        assert!(
            seat(&snap, &id).capabilities[&Capability::Tables].on,
            "{id}"
        );
    }
    for want in e["grants"].as_array().unwrap() {
        let seat = seat(&snap, s(&want["seat"]));
        for tool in strs(&want["has"]) {
            assert!(seat.grants.contains(&tool), "{}: {tool}", seat.id);
        }
    }
    for id in strs(&e["folder_flow"]["text_fallback"]) {
        assert!(seat(&snap, &id).text_fallback, "{id}");
    }
    for seat in snap.seats().values() {
        for (cap, setting) in &seat.capabilities {
            assert_eq!(setting.provider, "builtin", "{}: {cap:?}", seat.id);
        }
    }
}

#[test]
fn sample_skills_sources_and_knowledge() {
    let e = expect();
    let snap = clean_snapshot(&sample());
    let skills = e["skills"].as_object().unwrap();
    assert_eq!(
        ids(snap.skills().keys()),
        skills.keys().cloned().collect::<BTreeSet<_>>()
    );
    for (skill, seats) in skills {
        for id in strs(seats) {
            assert_eq!(seat(&snap, &id).skills, std::slice::from_ref(skill), "{id}");
        }
        let folder = sample().join("skills").join(skill);
        assert!(!folder.join("scripts").exists(), "{skill}: no scripts");
    }

    let drop = &e["drop_source"];
    let notes = &e["knowledge_source"];
    assert_eq!(
        ids(snap.sources().keys()),
        set(&[s(&drop["id"]), s(&notes["id"])])
    );
    for (id, source) in snap.sources() {
        assert!(
            matches!(source.trigger, Some(Trigger::Schedule { .. })),
            "{id}: scheduled"
        );
    }
    let source = &snap.sources()[s(&drop["id"])];
    assert_eq!(source.source_type, s(&drop["type"]));
    assert_eq!(source.owner_seat, s(&drop["owner"]));
    assert!(source.account.is_none());
    match &source.output {
        SourceOutput::Table {
            table,
            key,
            mapping,
        } => {
            assert_eq!(table, s(&drop["table"]));
            assert_eq!(key, s(&drop["key"]));
            assert!(mapping.contains_key("status"), "status is mapped");
        }
        other => panic!("the drop Source loads a table, not {other:?}"),
    }
    let source = &snap.sources()[s(&notes["id"])];
    assert_eq!(source.source_type, s(&notes["type"]));
    assert!(source.limits.max_loads_per_day.is_some(), "limits");
    assert_eq!(seat(&snap, &source.owner_seat).team, s(&notes["team"]));
    assert_eq!(
        source.output,
        SourceOutput::Knowledge {
            scope: "team".into()
        }
    );
    let dir = sample().join(s(&notes["dir"]));
    let files = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .count();
    assert!(files > 0, "the team Knowledge notes ship");
}
