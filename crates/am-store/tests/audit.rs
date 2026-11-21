//! Story 1.7: the hash-chained audit log (AD-25, R10). One test per row of
//! the story's I/O matrix at the store level: redacted `args` per command,
//! the JSON Lines export and its chain check (clean, altered, removed,
//! reordered, not JSON), audited migrations, and a chain written by story
//! 1.6 binaries (no `args`, the spike's hash formula) that keeps verifying
//! after the migration. The RULE-16 trust test is
//! `am-e2e/tests/trust/rule_16_audit_log.rs`; the CLI is `am-app/tests/audit_cli.rs`.
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use am_domain::{Actor, Usage};
use am_store::{
    OpenReport, Reader, SCHEMA_VERSION, Store, StoreError, TriggerRow, WriteCommand, verify_jsonl,
};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn sys() -> Actor {
    Actor::System {
        component: "test".into(),
    }
}

/// The runtime folder the store opens (story 1.13): `rt/` in `root`.
fn layout(root: &Path) -> am_domain::RuntimeLayout {
    am_domain::RuntimeLayout::at(root.join("rt"))
}

fn db(root: &Path) -> PathBuf {
    layout(root).state_db()
}

fn open_project(root: &Path) -> Result<(Store, OpenReport), StoreError> {
    am_store::open_runtime(&layout(root), "test-mac".into())
}

fn sha256_hex(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

/// A project after a run, a settle, a trigger and an `Audit` with a hash.
fn populated(root: &Path) -> Store {
    let (store, _) = open_project(root).unwrap();
    let w = store.writer();
    w.exec(
        sys(),
        WriteCommand::CreateRun {
            run_id: "run-1".into(),
            workflow: "wf-1".into(),
            origin: "manual".into(),
            steps: vec!["a".into(), "b".into()],
        },
    )
    .unwrap();
    let steps = store.reader().unwrap().steps("run-1").unwrap();
    let (a, b) = (steps[0].id.clone(), steps[1].id.clone());
    w.exec(
        sys(),
        WriteCommand::StartStep {
            step_run_id: a.clone(),
        },
    )
    .unwrap();
    for (call, seq) in [("call-1", 1), ("call-2", 2)] {
        w.exec(
            sys(),
            WriteCommand::ReserveCall {
                step_run_id: a.clone(),
                call_id: call.into(),
                call_seq: seq,
                reserved_micro_usd: 5000,
                rate_snapshot: "rates-1".into(),
            },
        )
        .unwrap();
    }
    w.exec(
        sys(),
        WriteCommand::SettleCall {
            call_id: "call-1".into(),
            usage: Some(Usage {
                input_tokens: 11,
                output_tokens: 7,
            }),
            cost_micro_usd: Some(420),
        },
    )
    .unwrap();
    w.exec(
        sys(),
        WriteCommand::MarkLost {
            call_id: "call-2".into(),
        },
    )
    .unwrap();
    w.exec(
        sys(),
        WriteCommand::FinishStep {
            step_run_id: a,
            status: "succeeded".into(),
            output: Some("the first step's answer".into()),
        },
    )
    .unwrap();
    w.exec(
        sys(),
        WriteCommand::FinishStep {
            step_run_id: b,
            status: "skipped".into(),
            output: None,
        },
    )
    .unwrap();
    w.exec(
        sys(),
        WriteCommand::FinishRun {
            run_id: "run-1".into(),
            status: "succeeded".into(),
        },
    )
    .unwrap();
    w.exec(
        sys(),
        WriteCommand::UpsertTrigger {
            trigger: TriggerRow {
                id: "trigger-1".into(),
                cron: "0 9 * * *".into(),
                tz: "UTC".into(),
                catch_up: "once".into(),
                state: "active".into(),
                next_fire: Some("2026-11-21T09:00:00.000Z".into()),
                last_fired: None,
            },
        },
    )
    .unwrap();
    w.exec(
        sys(),
        WriteCommand::RecordFire {
            trigger_id: "trigger-1".into(),
            scheduled_for: "2026-11-21T09:00:00.000Z".into(),
            kind: "on_time".into(),
            next_fire: "2026-11-22T09:00:00.000Z".into(),
        },
    )
    .unwrap();
    w.exec(
        sys(),
        WriteCommand::Audit {
            action: "folder.read".into(),
            target: "root-1:notes/a.md".into(),
            outcome: "ok".into(),
            hash: Some(sha256_hex("the file's bytes")),
        },
    )
    .unwrap();
    store
}

fn export_of(reader: &Reader) -> (String, u64, String) {
    let mut out = Vec::new();
    let (count, head) = reader.export_audit_jsonl(&mut out).unwrap();
    (String::from_utf8(out).unwrap(), count, head)
}

fn export(store: &Store) -> (String, u64, String) {
    export_of(&store.reader().unwrap())
}

fn entries(text: &str) -> Vec<Value> {
    text.lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn entry<'a>(all: &'a [Value], action: &str) -> &'a Value {
    all.iter()
        .find(|e| e["action"] == action)
        .unwrap_or_else(|| panic!("no {action} entry"))
}

// ------------------------------------------------------------ export shape

#[test]
fn export_has_one_object_per_entry_with_the_nine_fields() {
    let dir = tempfile::tempdir().unwrap();
    let store = populated(dir.path());
    let (text, count, head) = export(&store);
    let all = entries(&text);
    assert_eq!(all.len() as u64, count);
    let rows: i64 = Connection::open(db(dir.path()))
        .unwrap()
        .query_row("SELECT count(*) FROM audit_log", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows as u64, count);
    for e in &all {
        let mut keys: Vec<&str> = e.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(
            keys,
            vec![
                "action",
                "actor",
                "args",
                "at",
                "entry_hash",
                "outcome",
                "prev_hash",
                "seq",
                "target"
            ]
        );
        assert!(e["actor"].is_object(), "{e}");
    }
    assert_eq!(all[0]["prev_hash"], "genesis");
    assert_eq!(all.last().unwrap()["entry_hash"], head.as_str());
    let ok = verify_jsonl(text.as_bytes()).unwrap();
    assert_eq!((ok.entries, ok.head), (count, head));
    assert_eq!(store.reader().unwrap().verify_audit_chain().unwrap(), None);
}

#[test]
fn args_are_redacted_ids_hashes_counts_and_statuses() {
    let dir = tempfile::tempdir().unwrap();
    let store = populated(dir.path());
    let (text, _, _) = export(&store);
    let all = entries(&text);

    assert_eq!(
        entry(&all, "run.created")["args"],
        json!({"origin": "manual", "steps": 2, "workflow": "wf-1"})
    );
    assert_eq!(
        entry(&all, "model_call.settled")["args"],
        json!({"cost_micro_usd": 420, "input_tokens": 11, "output_tokens": 7})
    );
    assert_eq!(
        entry(&all, "model_call.reserved")["args"]["reserved_micro_usd"],
        5000
    );
    let finished: Vec<&Value> = all
        .iter()
        .filter(|e| e["action"] == "step.finished")
        .collect();
    assert_eq!(
        finished[0]["args"],
        json!({"output_sha256": sha256_hex("the first step's answer"), "status": "succeeded"})
    );
    assert_eq!(
        finished[1]["args"],
        json!({"output_sha256": null, "status": "skipped"})
    );
    assert_eq!(
        entry(&all, "run.finished")["args"],
        json!({"status": "succeeded"})
    );
    assert_eq!(
        entry(&all, "folder.read")["args"],
        json!({"sha256": sha256_hex("the file's bytes")})
    );
    assert_eq!(entry(&all, "trigger.fired")["args"]["kind"], "on_time");
    assert!(!text.contains("the first step's answer"), "{text}");
}

#[test]
fn an_audit_entry_without_a_hash_has_null_args() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = open_project(dir.path()).unwrap();
    store
        .writer()
        .exec(
            sys(),
            WriteCommand::Audit {
                action: "egress.blocked".into(),
                target: "host-1".into(),
                outcome: "not on the allow-list".into(),
                hash: None,
            },
        )
        .unwrap();
    let (text, _, _) = export(&store);
    assert_eq!(
        entry(&entries(&text), "egress.blocked")["args"],
        Value::Null
    );
    verify_jsonl(text.as_bytes()).unwrap();
}

// ------------------------------------------------------------ chain check

#[test]
fn an_altered_middle_line_is_named_by_seq_and_line() {
    let dir = tempfile::tempdir().unwrap();
    let store = populated(dir.path());
    let (text, _, _) = export(&store);
    let mut all = entries(&text);
    let i = all.len() / 2;
    let seq = all[i]["seq"].as_i64().unwrap();
    all[i]["outcome"] = json!("refused");
    let bad: String = all
        .iter()
        .map(|e| serde_json::to_string(e).unwrap() + "\n")
        .collect();
    let brk = verify_jsonl(bad.as_bytes()).unwrap_err();
    assert_eq!((brk.line, brk.seq), (i as u64 + 1, Some(seq)));
}

#[test]
fn a_removed_line_names_the_first_entry_after_it() {
    let dir = tempfile::tempdir().unwrap();
    let store = populated(dir.path());
    let (text, _, _) = export(&store);
    let mut lines: Vec<&str> = text.lines().collect();
    let i = lines.len() / 2;
    let next_seq = entries(&text)[i + 1]["seq"].as_i64().unwrap();
    lines.remove(i);
    let bad = lines.join("\n") + "\n";
    let brk = verify_jsonl(bad.as_bytes()).unwrap_err();
    // The entry that followed the removed one now sits on line i + 1.
    assert_eq!((brk.line, brk.seq), (i as u64 + 1, Some(next_seq)));
    assert!(brk.reason.contains("missing"), "{}", brk.reason);
}

#[test]
fn swapped_lines_name_the_first_entry_out_of_place() {
    let dir = tempfile::tempdir().unwrap();
    let store = populated(dir.path());
    let (text, _, _) = export(&store);
    let mut lines: Vec<&str> = text.lines().collect();
    let i = lines.len() / 2;
    lines.swap(i, i + 1);
    let seq_now_first = entries(&text)[i + 1]["seq"].as_i64().unwrap();
    let bad = lines.join("\n") + "\n";
    let brk = verify_jsonl(bad.as_bytes()).unwrap_err();
    assert_eq!((brk.line, brk.seq), (i as u64 + 1, Some(seq_now_first)));
}

/// The export with line `index` (0-based) rewritten by `edit`.
fn edited(text: &str, index: usize, edit: impl FnOnce(&mut Value)) -> String {
    let mut all = entries(text);
    edit(&mut all[index]);
    all.iter()
        .map(|e| serde_json::to_string(e).unwrap() + "\n")
        .collect()
}

/// The first line (0-based) whose `args` is an object, after the migrations.
fn first_with_args(text: &str) -> usize {
    entries(text)
        .iter()
        .position(|e| e["args"].is_object())
        .expect("an entry with args")
}

#[test]
fn moving_args_into_the_outcome_through_a_nul_is_named() {
    let dir = tempfile::tempdir().unwrap();
    let store = populated(dir.path());
    let (text, _, _) = export(&store);
    let i = first_with_args(&text);
    let seq = entries(&text)[i]["seq"].as_i64().unwrap();
    // The hashed bytes are the same: `outcome NUL args NUL` either way.
    let bad = edited(&text, i, |e| {
        let args = serde_json::to_string(&e["args"]).unwrap();
        let outcome = format!("{}\u{0}{args}", e["outcome"].as_str().unwrap());
        e["outcome"] = json!(outcome);
        e["args"] = Value::Null;
    });
    let brk = verify_jsonl(bad.as_bytes()).unwrap_err();
    assert_eq!((brk.line, brk.seq), (i as u64 + 1, Some(seq)));
    assert!(brk.reason.contains("NUL"), "{}", brk.reason);
}

#[test]
fn the_writer_refuses_a_field_with_a_nul_byte() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = open_project(dir.path()).unwrap();
    let (_, before, _) = export(&store);
    let err = store
        .writer()
        .exec(
            sys(),
            WriteCommand::Audit {
                action: "folder.read".into(),
                target: "root-1:a.md".into(),
                outcome: "ok\u{0}{\"sha256\":\"x\"}".into(),
                hash: None,
            },
        )
        .unwrap_err();
    assert!(matches!(err, StoreError::Invalid(_)), "{err:?}");
    assert_eq!(export(&store).1, before, "nothing written");
}

#[test]
fn an_edited_middle_seq_is_named() {
    let dir = tempfile::tempdir().unwrap();
    let store = populated(dir.path());
    let (text, _, _) = export(&store);
    let i = entries(&text).len() / 2;
    let bad = edited(&text, i, |e| e["seq"] = json!(9999));
    let brk = verify_jsonl(bad.as_bytes()).unwrap_err();
    assert_eq!((brk.line, brk.seq), (i as u64 + 1, Some(9999)));
    assert!(
        brk.reason.contains(&format!("expected entry {}", i + 1)),
        "{}",
        brk.reason
    );
}

#[test]
fn an_extra_top_level_field_is_named() {
    let dir = tempfile::tempdir().unwrap();
    let store = populated(dir.path());
    let (text, _, _) = export(&store);
    let i = entries(&text).len() / 2;
    let bad = edited(&text, i, |e| e["approved_by"] = json!("someone"));
    let brk = verify_jsonl(bad.as_bytes()).unwrap_err();
    assert_eq!(brk.line, i as u64 + 1);
    assert!(brk.reason.contains("approved_by"), "{}", brk.reason);
}

#[test]
fn an_extra_actor_field_is_named() {
    let dir = tempfile::tempdir().unwrap();
    let store = populated(dir.path());
    let (text, _, _) = export(&store);
    let i = entries(&text).len() / 2;
    let bad = edited(&text, i, |e| e["actor"]["person"] = json!("someone"));
    let brk = verify_jsonl(bad.as_bytes()).unwrap_err();
    assert_eq!(brk.line, i as u64 + 1);
    assert!(brk.reason.contains("actor"), "{}", brk.reason);
}

#[test]
fn an_audit_hash_that_is_not_sha256_hex_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = open_project(dir.path()).unwrap();
    let (_, before, _) = export(&store);
    for hash in [
        "the row's private content".to_string(),
        sha256_hex("x").to_uppercase(),
        sha256_hex("x")[..63].to_string(),
        format!("{}0", sha256_hex("x")),
    ] {
        let err = store
            .writer()
            .exec(
                sys(),
                WriteCommand::Audit {
                    action: "folder.read".into(),
                    target: "root-1:a.md".into(),
                    outcome: "ok".into(),
                    hash: Some(hash.clone()),
                },
            )
            .unwrap_err();
        assert!(matches!(err, StoreError::Invalid(_)), "{hash}: {err:?}");
    }
    let (text, after, _) = export(&store);
    assert_eq!(after, before, "nothing written");
    assert!(!text.contains("private content"));
}

#[test]
fn a_line_that_is_not_json_is_named() {
    let dir = tempfile::tempdir().unwrap();
    let store = populated(dir.path());
    let (text, _, _) = export(&store);
    let mut lines: Vec<&str> = text.lines().collect();
    lines.insert(3, "this is {not json");
    let bad = lines.join("\n") + "\n";
    let brk = verify_jsonl(bad.as_bytes()).unwrap_err();
    assert_eq!((brk.line, brk.seq), (4, None));
    assert!(brk.to_string().contains("line 4"), "{brk}");
}

#[test]
fn an_empty_export_verifies_with_no_entries() {
    let ok = verify_jsonl("".as_bytes()).unwrap();
    assert_eq!((ok.entries, ok.head.as_str()), (0, "genesis"));
}

// ------------------------------------------------------------ reading

#[test]
fn export_reads_while_the_writer_holds_the_project() {
    let dir = tempfile::tempdir().unwrap();
    let store = populated(dir.path());
    // A separate read-only Reader on the file, the way the CLI opens it.
    let reader = Reader::open(&db(dir.path())).unwrap();
    let (text, count, _) = export_of(&reader);
    assert!(count > 0);
    verify_jsonl(text.as_bytes()).unwrap();
    drop(store);
}

#[test]
fn export_refuses_a_database_newer_than_the_binary() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.db");
    am_store::testing::database_at_version(&path, SCHEMA_VERSION + 1).unwrap();
    let reader = Reader::open(&path).unwrap();
    let mut out = Vec::new();
    let err = reader.export_audit_jsonl(&mut out).unwrap_err();
    assert!(matches!(err, StoreError::TooNew { .. }), "{err:?}");
    assert!(out.is_empty());
}

// ------------------------------------------------------------ migrations

fn migrated_targets(text: &str) -> Vec<String> {
    entries(text)
        .iter()
        .filter(|e| e["action"] == "store.migrated")
        .map(|e| {
            assert_eq!(e["args"], Value::Null, "{e}");
            assert_eq!(
                e["actor"],
                json!({"kind": "system", "component": "migrations"})
            );
            e["target"].as_str().unwrap().to_string()
        })
        .collect()
}

#[test]
fn a_new_project_audits_every_migration_step_the_first_included() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = open_project(dir.path()).unwrap();
    let (text, _, _) = export(&store);
    let want: Vec<String> = (1..=SCHEMA_VERSION)
        .map(|v| format!("schema version {v}"))
        .collect();
    assert_eq!(migrated_targets(&text), want);
    let first = &entries(&text)[0];
    assert_eq!(first["target"], "schema version 1");
    assert_eq!(first["outcome"], "from version 0 to 1");
    verify_jsonl(text.as_bytes()).unwrap();
}

#[test]
fn an_older_project_audits_the_one_step_it_applies() {
    let dir = tempfile::tempdir().unwrap();
    let ag = layout(dir.path()).dir().to_path_buf();
    std::fs::create_dir(&ag).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&ag, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    am_store::testing::database_at_version(&ag.join("state.db"), SCHEMA_VERSION - 1).unwrap();
    let (store, report) = open_project(dir.path()).unwrap();
    assert_eq!(report.applied.len(), 1);
    let (text, _, _) = export(&store);
    assert_eq!(
        migrated_targets(&text),
        vec![format!("schema version {SCHEMA_VERSION}")]
    );
    let e = entry(&entries(&text), "store.migrated").clone();
    let backup = report.applied[0].backup.clone().unwrap();
    assert_eq!(
        e["outcome"],
        format!(
            "from version {} to {SCHEMA_VERSION}, backup: {}",
            SCHEMA_VERSION - 1,
            backup.display()
        )
    );
}

// ------------------------------------------------------------ 1.6 chain

/// The spike's (story 1.6) formula: sha256 over prev, at, actor, action,
/// target and outcome, each followed by a NUL byte.
fn spike_hash(
    prev: &str,
    at: &str,
    actor: &str,
    action: &str,
    target: &str,
    outcome: &str,
) -> String {
    let mut h = Sha256::new();
    for part in [prev, at, actor, action, target, outcome] {
        h.update(part.as_bytes());
        h.update([0u8]);
    }
    hex::encode(h.finalize())
}

#[test]
fn a_chain_written_by_1_6_binaries_verifies_after_migrating_and_writing() {
    let dir = tempfile::tempdir().unwrap();
    let ag = layout(dir.path()).dir().to_path_buf();
    std::fs::create_dir(&ag).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&ag, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let path = ag.join("state.db");
    am_store::testing::database_at_version(&path, 2).unwrap();
    // Entries as 1.6 wrote them: no args column, the spike's formula.
    {
        let c = Connection::open(&path).unwrap();
        let actor = r#"{"kind":"system","component":"scheduler"}"#;
        let mut prev = "genesis".to_string();
        for (i, (action, target)) in [("run.created", "run-0"), ("run.finished", "run-0")]
            .iter()
            .enumerate()
        {
            let at = format!("2026-11-19T10:00:0{i}.000Z");
            let h = spike_hash(&prev, &at, actor, action, target, "ok");
            c.execute(
                "INSERT INTO audit_log (at, actor, action, target, outcome, prev_hash, entry_hash) VALUES (?1, ?2, ?3, ?4, 'ok', ?5, ?6)",
                params![at, actor, action, target, prev, h],
            )
            .unwrap();
            prev = h;
        }
    }
    let old_rows: Vec<(i64, String)> = {
        let c = Connection::open(&path).unwrap();
        c.prepare("SELECT seq, entry_hash FROM audit_log ORDER BY seq")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };

    // An older binary's database still exports and verifies (read-only, no args column).
    let (text, count, _) = export_of(&Reader::open(&path).unwrap());
    assert_eq!(count, 2);
    verify_jsonl(text.as_bytes()).unwrap();

    let (store, _) = open_project(dir.path()).unwrap();
    // run-0 has no row: FinishRun updates nothing and still audits.
    store
        .writer()
        .exec(
            sys(),
            WriteCommand::FinishRun {
                run_id: "run-0".into(),
                status: "succeeded".into(),
            },
        )
        .unwrap();
    store
        .writer()
        .exec(
            sys(),
            WriteCommand::Audit {
                action: "folder.read".into(),
                target: "root-1:a.md".into(),
                outcome: "ok".into(),
                hash: Some(sha256_hex("bytes")),
            },
        )
        .unwrap();

    let (text, count, head) = export(&store);
    // One migration entry per step from version 2 (one step when story 1.7
    // wrote this; story 1.8 added the config History tables).
    let steps = (SCHEMA_VERSION - 2) as u64;
    assert_eq!(count, 2 + steps + 2, "two old, the migrations, two new");
    let all = entries(&text);
    // The old entries were never recomputed.
    for (e, (seq, h)) in all.iter().zip(&old_rows) {
        assert_eq!(e["seq"], *seq);
        assert_eq!(e["entry_hash"], h.as_str());
        assert_eq!(e["args"], Value::Null);
    }
    let ok = verify_jsonl(text.as_bytes()).unwrap();
    assert_eq!((ok.entries, ok.head), (count, head));
    assert_eq!(store.reader().unwrap().verify_audit_chain().unwrap(), None);
}
