//! Epic 3 story 7, "Cost settles per call and rolls up" (AD-7, AD-8, AD-9,
//! AD-24, AD-25; owner Q3, Q4, Q5, Q13, Q14): the writer computes each
//! call's amounts, in USD and in the company currency, from the terms the
//! reservation journals; a settlement prices the provider's usage with
//! those same terms, or settles at the reservation when usage is missing,
//! and stamps `settled_at`; a local call is 0 with its tokens and time; the
//! ledger keeps what was reserved and settles once (migration 0013's
//! triggers); and the audit holds amounts, terms and flags, never text.
//!
//! The API pinned here (tester, 2026-11-25; the plan's "API the tests
//! fix"):
//! - `WriteCommand::ReserveCall { step_run_id, call_id, call_seq, price:
//!   CallPrice, rate: Rate, counted_input_tokens: u64, max_output_tokens:
//!   u32, week: String, team: String, taint, purpose, prompt }`; no amount
//!   and no `rate_snapshot` field (owner Q4: the writer computes them).
//! - The settle commands and `CallSettlement` carry usage only: no
//!   `cost_micro_usd`.
//! - `LedgerRow` gains the 0013 columns (`None` on an old row) and
//!   `created_at`; `CallRow` gains `created_at` and `settled_at`.
//! - `Reader::spend()` returns one `SpendRow` per call of every run, joined
//!   to its run, step key, attempt and seat.
//!
//! Amounts use the matrix's fake terms: 3.00 and 15.00 USD per million
//! tokens, 0.92 EUR per USD, `max_output_tokens` 4,096. Written before the
//! API exists (tests first, 2026-11-25).
#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use am_domain::money::{CallPrice, Rate};
use am_domain::{Actor, InboxKind, RuntimeLayout, Taint, Usage};
use am_store::{
    AttemptWrites, CallPurpose, CallRow, CallSettlement, LedgerRow, NewInboxItem, NewPromptPart,
    OpenReport, PromptKind, PromptSlot, Reader, SCHEMA_VERSION, Store, StoreError, WriteCommand,
    new_id,
};
use rusqlite::types::Value as Cell;
use rusqlite::{Connection, OpenFlags, params};
use serde_json::{Value, json};
use uuid::Uuid;

/// The pinned config snapshot's hash.
const HASH: &str = "0a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f9";
/// The rate snapshot: a price file's sha256 hex (owner Q6).
const SNAPSHOT: &str = "3f9c0a1b2c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7";
/// The price file's `updated:` date.
const UPDATED: &str = "2026-11-01";
/// The budget week the engine stamps (company-local Monday).
const WEEK: &str = "2026-11-23";
const TEAM: &str = "core";
const SEAT: &str = "seat-writer";
const DOCUMENT: &str = "summary-note";
/// Free text that must never reach the audit log (AD-25).
const CANARY_PROMPT: &str = "canary-prompt-text-7c41e0";
const CANARY_REPLY: &str = "canary-reply-text-93b2d5";

fn engine() -> Actor {
    Actor::System {
        component: "engine".into(),
    }
}

/// The matrix's cloud terms (fake): 3.00 in, 15.00 out, USD per million.
fn cloud() -> CallPrice {
    CallPrice {
        input_micro_usd_per_mtok: 3_000_000,
        output_micro_usd_per_mtok: 15_000_000,
        local: false,
    }
}

/// 0.92 EUR per USD (fake), from the price file `SNAPSHOT`.
fn eur() -> Rate {
    Rate {
        currency: "EUR".into(),
        company_micro_per_usd: Some(920_000),
        snapshot: SNAPSHOT.into(),
        prices_updated: Some(UPDATED.into()),
    }
}

/// A local-only run's rate: EUR company, no price file, no rate.
fn no_rate() -> Rate {
    Rate {
        currency: "EUR".into(),
        company_micro_per_usd: None,
        snapshot: "none".into(),
        prices_updated: None,
    }
}

fn usage(input_tokens: u64, output_tokens: u64) -> Option<Usage> {
    Some(Usage {
        input_tokens,
        output_tokens,
    })
}

fn millis(ts: &str) -> i64 {
    ts.parse::<jiff::Timestamp>()
        .unwrap_or_else(|e| panic!("{ts}: {e}"))
        .as_millisecond()
}

// ------------------------------------------------------------------ the rig

/// One started step attempt.
#[derive(Debug, Clone)]
struct Attempt {
    key: String,
    id: String,
}

struct Rig {
    _dir: tempfile::TempDir,
    db: PathBuf,
    store: Store,
}

fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state.db");
    let store = Store::open(&db).unwrap();
    Rig {
        _dir: dir,
        db,
        store,
    }
}

impl Rig {
    fn reader(&self) -> Reader {
        self.store.reader().unwrap()
    }

    fn exec(&self, cmd: WriteCommand) -> Result<(), StoreError> {
        self.store.writer().exec(engine(), cmd)
    }

    /// A run of `steps`: its id.
    fn run(&self, steps: &[&str]) -> String {
        let id = Uuid::now_v7().to_string();
        self.exec(WriteCommand::CreateRun {
            run_id: id.clone(),
            workflow: "wf".into(),
            origin: "manual".into(),
            steps: steps.iter().map(|s| s.to_string()).collect(),
            config_hash: Some(HASH.into()),
        })
        .unwrap();
        id
    }

    /// Starts the latest attempt of `key` as [`SEAT`], with `prompt`.
    fn begin_with(
        &self,
        run: &str,
        key: &str,
        document: Option<&str>,
        prompt: Vec<NewPromptPart>,
    ) -> Attempt {
        let row = self
            .reader()
            .steps(run)
            .unwrap()
            .into_iter()
            .filter(|s| s.step_key == key)
            .max_by_key(|s| s.attempt)
            .unwrap_or_else(|| panic!("no step {key}"));
        self.exec(WriteCommand::StartAttempt {
            step_run_id: row.id.clone(),
            inputs: vec![],
            taint: Taint::Clean,
            prompt,
            writes: AttemptWrites {
                seat: SEAT.into(),
                table: None,
                document: document.map(str::to_string),
            },
        })
        .unwrap();
        Attempt {
            key: key.into(),
            id: row.id,
        }
    }

    fn begin(&self, run: &str, key: &str) -> Attempt {
        self.begin_with(run, key, None, vec![])
    }

    /// The command that reserves the attempt's next call.
    fn reserve_cmd(
        &self,
        a: &Attempt,
        call: &str,
        price: CallPrice,
        rate: Rate,
        counted: u64,
        max: u32,
    ) -> WriteCommand {
        let seq = self.reader().calls_for_step(&a.id).unwrap().len() as i64 + 1;
        WriteCommand::ReserveCall {
            step_run_id: a.id.clone(),
            call_id: call.into(),
            call_seq: seq,
            price,
            rate,
            counted_input_tokens: counted,
            max_output_tokens: max,
            week: WEEK.into(),
            team: TEAM.into(),
            taint: Taint::Clean,
            purpose: CallPurpose::Turn,
            prompt: vec![],
        }
    }

    /// Reserves the attempt's next call on `price` and `rate`: its id.
    fn reserve(&self, a: &Attempt, price: CallPrice, rate: Rate, counted: u64, max: u32) -> String {
        let call = new_id();
        self.exec(self.reserve_cmd(a, &call, price, rate, counted, max))
            .unwrap();
        call
    }

    /// The matrix's reservation: 1,000 counted tokens and 4,096 out, on the
    /// cloud terms at 0.92 EUR per USD.
    fn reserve_cloud(&self, a: &Attempt) -> String {
        self.reserve(a, cloud(), eur(), 1_000, 4_096)
    }

    fn ledger(&self, run: &str) -> Vec<LedgerRow> {
        self.reader().ledger_for_run(run).unwrap()
    }

    fn entry_of(&self, run: &str, call: &str) -> LedgerRow {
        self.ledger(run)
            .into_iter()
            .find(|l| l.model_call_id == call)
            .unwrap_or_else(|| panic!("no ledger entry for {call}"))
    }

    fn call(&self, run: &str, call: &str) -> CallRow {
        self.reader()
            .calls_for_run(run)
            .unwrap()
            .into_iter()
            .find(|c| c.id == call)
            .unwrap_or_else(|| panic!("no call {call}"))
    }

    fn audit(&self) -> Vec<Value> {
        let mut out = Vec::new();
        self.reader().export_audit_jsonl(&mut out).unwrap();
        String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    fn audit_text(&self) -> String {
        let mut out = Vec::new();
        self.reader().export_audit_jsonl(&mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    /// The one audit entry `action` targeting `target`.
    fn entry(&self, action: &str, target: &str) -> Value {
        let found: Vec<Value> = self
            .audit()
            .into_iter()
            .filter(|e| e["action"] == action && e["target"] == target)
            .collect();
        assert_eq!(found.len(), 1, "one {action} for {target}: {found:#?}");
        found[0].clone()
    }

    fn raw(&self) -> Connection {
        Connection::open(&self.db).unwrap()
    }

    fn count(&self, table: &str) -> i64 {
        let c = Connection::open_with_flags(&self.db, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        c.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }

    /// Every ledger row and model call, raw, for "nothing was written".
    fn money_state(&self) -> (Vec<Vec<Cell>>, Vec<Vec<Cell>>, usize) {
        let c = Connection::open_with_flags(&self.db, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let ledger = cells_of(&c, "ledger", &columns(&self.db, "ledger"));
        let calls = cells_of(&c, "model_calls", &columns(&self.db, "model_calls"));
        (ledger, calls, self.audit().len())
    }
}

fn failure_item(a: &Attempt) -> NewInboxItem {
    NewInboxItem {
        id: new_id(),
        kind: InboxKind::Failure,
        payload: Some(a.id.clone()),
        taint: Taint::Clean,
        title: "A step stopped".into(),
        body: format!("Step `{}` stopped.", a.key),
        max_returns: None,
    }
}

fn draft_item(a: &Attempt) -> NewInboxItem {
    NewInboxItem {
        id: new_id(),
        kind: InboxKind::Draft,
        payload: Some(a.id.clone()),
        taint: Taint::Clean,
        title: format!("Draft: {DOCUMENT}"),
        body: format!("Step `{}` wrote a version of {DOCUMENT}.", a.key),
        max_returns: None,
    }
}

// ------------------------------------------------------------------ reserve

/// Row 1 (AD-8, AD-9; owner Q1, Q4, Q5): the reservation is journaled with
/// both amounts the writer computed from the terms it carries (64,440
/// micro-USD and 59,285 micro-EUR for 1,000 counted tokens and 4,096 out),
/// the terms themselves, the counted tokens, `max_output_tokens`, the rate,
/// its snapshot and date, the week and the team; the call is `issued`, not
/// settled. `model_call.reserved` holds exactly those amounts and terms
/// (row 26). A reservation the writer cannot price (an overflowing product,
/// row 15; a cloud price with no rate, owner Q6) is refused with nothing
/// written.
#[test]
fn a_reservation_journals_both_amounts_and_its_terms() {
    let r = rig();
    let run = r.run(&["draft"]);
    let a = r.begin(&run, "draft");

    let call = r.reserve_cloud(&a);

    let l = r.entry_of(&run, &call);
    assert_eq!(l.state, "reserved");
    assert_eq!(l.reserved_micro_usd, 64_440);
    assert_eq!(l.reserved_micro_company, Some(59_285));
    assert_eq!(l.settled_micro_usd, None);
    assert_eq!(l.settled_micro_company, None);
    assert_eq!(l.currency.as_deref(), Some("EUR"));
    assert_eq!(l.rate_snapshot, SNAPSHOT);
    assert_eq!(l.prices_updated.as_deref(), Some(UPDATED));
    assert_eq!(l.input_micro_usd_per_mtok, Some(3_000_000));
    assert_eq!(l.output_micro_usd_per_mtok, Some(15_000_000));
    assert_eq!(l.company_micro_per_usd, Some(920_000));
    assert_eq!(l.counted_input_tokens, Some(1_000));
    assert_eq!(l.max_output_tokens, Some(4_096));
    assert_eq!(l.local, Some(false));
    assert_eq!(l.week.as_deref(), Some(WEEK));
    assert_eq!(l.team.as_deref(), Some(TEAM));
    millis(&l.created_at);
    let c = r.call(&run, &call);
    assert_eq!((c.status.as_str(), c.settled), ("issued", false));
    assert_eq!(c.settled_at, None);
    millis(&c.created_at);

    assert_eq!(
        r.entry("model_call.reserved", &call)["args"],
        json!({
            "call_seq": 1,
            "company_micro_per_usd": 920_000,
            "counted_input_tokens": 1_000,
            "currency": "EUR",
            "input_micro_usd_per_mtok": 3_000_000,
            "local": 0,
            "max_output_tokens": 4_096,
            "output_micro_usd_per_mtok": 15_000_000,
            "prices_updated": UPDATED,
            "purpose": "turn",
            "rate_snapshot": SNAPSHOT,
            "reserved_micro_company": 59_285,
            "reserved_micro_usd": 64_440,
            "step_run_id": a.id,
            "taint": 0,
            "team": TEAM,
            "week": WEEK,
        })
    );

    // Refused, with nothing written: a product that does not fit (row 15)
    // and a cloud call with no rate for the company currency (owner Q6).
    let before = r.money_state();
    let dearest = CallPrice {
        input_micro_usd_per_mtok: i64::MAX,
        output_micro_usd_per_mtok: i64::MAX,
        local: false,
    };
    let overflow = r.exec(r.reserve_cmd(&a, &new_id(), dearest, eur(), u64::MAX, u32::MAX));
    assert!(overflow.is_err(), "an overflowing reservation is refused");
    assert!(
        !overflow.unwrap_err().to_string().trim().is_empty(),
        "in words"
    );
    let unpriced = r.exec(r.reserve_cmd(&a, &new_id(), cloud(), no_rate(), 1_000, 4_096));
    assert!(unpriced.is_err(), "a cloud call with no rate is refused");
    assert_eq!(r.money_state(), before, "nothing was written");
}

// ------------------------------------------------------------------ settle

/// Row 2 (AD-9; owner Q3, Q4): usage 1,200/90 settles at the reserved terms
/// in both currencies (4,950 micro-USD, 4,554 micro-EUR); the call keeps
/// its tokens, is `settled` and stamps `settled_at` (not before it was
/// created); the reservation's own amounts and terms are as they were.
/// `model_call.settled` holds the amounts, the currency, the tokens and
/// both flags (`at_reservation` and `over_reservation` false), nothing
/// else (row 26).
#[test]
fn usage_settles_both_currencies_at_the_reserved_terms() {
    let r = rig();
    let run = r.run(&["draft"]);
    let a = r.begin(&run, "draft");
    let call = r.reserve_cloud(&a);
    let reserved = r.entry_of(&run, &call);

    r.exec(WriteCommand::SettleCall {
        call_id: call.clone(),
        usage: usage(1_200, 90),
    })
    .unwrap();

    let l = r.entry_of(&run, &call);
    assert_eq!(l.state, "settled");
    assert_eq!(l.settled_micro_usd, Some(4_950));
    assert_eq!(l.settled_micro_company, Some(4_554));
    assert_eq!(
        LedgerRow {
            settled_micro_usd: None,
            settled_micro_company: None,
            state: "reserved".into(),
            ..l.clone()
        },
        reserved,
        "what was reserved is as it was"
    );
    let c = r.call(&run, &call);
    assert_eq!((c.status.as_str(), c.settled), ("settled", true));
    assert_eq!((c.input_tokens, c.output_tokens), (Some(1_200), Some(90)));
    let settled_at = c.settled_at.clone().expect("settled_at is stamped");
    assert!(millis(&settled_at) >= millis(&c.created_at), "{c:?}");

    assert_eq!(
        r.entry("model_call.settled", &call)["args"],
        json!({
            "at_reservation": false,
            "currency": "EUR",
            "input_tokens": 1_200,
            "output_tokens": 90,
            "over_reservation": false,
            "settled_micro_company": 4_554,
            "settled_micro_usd": 4_950,
        })
    );
}

/// Row 3 (AD-9 literal; owner Q3): no usage settles at the reservation in
/// both currencies, by every settle command (`SettleCall`,
/// `SettleAndFinishStep`, `FailWithItem`); the call holds no tokens; the
/// audit says `at_reservation: true`.
#[test]
fn missing_usage_settles_at_the_reservation_in_both_currencies() {
    let r = rig();
    let run = r.run(&["one", "two"]);

    let one = r.begin(&run, "one");
    let first = r.reserve_cloud(&one);
    r.exec(WriteCommand::SettleCall {
        call_id: first.clone(),
        usage: None,
    })
    .unwrap();
    let second = r.reserve_cloud(&one);
    r.exec(WriteCommand::SettleAndFinishStep {
        call_id: second.clone(),
        usage: None,
        step_run_id: one.id.clone(),
        status: "succeeded".into(),
        output: Some("one line".into()),
    })
    .unwrap();
    let two = r.begin(&run, "two");
    let third = r.reserve_cloud(&two);
    r.exec(WriteCommand::FailWithItem {
        step_run_id: two.id.clone(),
        settle: Some(CallSettlement {
            call_id: third.clone(),
            usage: None,
        }),
        output: None,
        item: failure_item(&two),
    })
    .unwrap();

    for (call, action) in [
        (&first, "model_call.settled"),
        (&second, "model_call.settled+step.finished"),
        (&third, "model_call.settled+step.finished"),
    ] {
        let l = r.entry_of(&run, call);
        assert_eq!(l.state, "settled", "{action}");
        assert_eq!(l.settled_micro_usd, Some(64_440), "{action}");
        assert_eq!(l.settled_micro_company, Some(59_285), "{action}");
        let c = r.call(&run, call);
        assert_eq!((c.input_tokens, c.output_tokens), (None, None), "{action}");
        assert!(c.settled_at.is_some(), "{action}: {c:?}");
        let args = &r.entry(action, call)["args"];
        assert_eq!(args["at_reservation"], json!(true), "{action}: {args}");
        assert_eq!(args["over_reservation"], json!(false), "{action}: {args}");
        assert_eq!(args["settled_micro_usd"], json!(64_440), "{action}: {args}");
        assert_eq!(
            args["settled_micro_company"],
            json!(59_285),
            "{action}: {args}"
        );
        assert_eq!(args["currency"], json!("EUR"), "{action}: {args}");
    }
}

/// Row 4 (owner Q3): usage above the estimate settles at the reported
/// usage, above the reservation (30,000/4,000: 150,000 micro-USD and
/// 138,000 micro-EUR against 64,440 and 59,285), with no cap check and no
/// refusal; the audit says `over_reservation: true`.
#[test]
fn usage_above_the_estimate_settles_as_reported() {
    let r = rig();
    let run = r.run(&["draft"]);
    let a = r.begin(&run, "draft");
    let call = r.reserve_cloud(&a);

    r.exec(WriteCommand::SettleAndFinishStep {
        call_id: call.clone(),
        usage: usage(30_000, 4_000),
        step_run_id: a.id.clone(),
        status: "succeeded".into(),
        output: Some("a long answer".into()),
    })
    .unwrap();

    let l = r.entry_of(&run, &call);
    assert_eq!(l.state, "settled");
    assert_eq!(l.reserved_micro_usd, 64_440);
    assert_eq!(l.settled_micro_usd, Some(150_000));
    assert_eq!(l.settled_micro_company, Some(138_000));
    let args = &r.entry("model_call.settled+step.finished", &call)["args"];
    assert_eq!(args["over_reservation"], json!(true), "{args}");
    assert_eq!(args["at_reservation"], json!(false), "{args}");
    assert_eq!(args["settled_micro_usd"], json!(150_000), "{args}");
    assert_eq!(args["settled_micro_company"], json!(138_000), "{args}");
}

/// Row 5 (Conventions, Money; owner Q8): a local call is reserved and
/// settled at 0 in both currencies, `local`, with no rate needed (a
/// local-only run's snapshot is `none`), whatever terms it carries; it
/// keeps its tokens, and its time is `settled_at` minus `created_at`.
#[test]
fn a_local_reservation_is_zero_with_tokens_and_time() {
    let r = rig();
    let run = r.run(&["check", "again"]);
    let a = r.begin(&run, "check");

    let call = r.reserve(&a, CallPrice::local(), no_rate(), 900, 4_096);
    let l = r.entry_of(&run, &call);
    assert_eq!(l.reserved_micro_usd, 0);
    assert_eq!(l.reserved_micro_company, Some(0));
    assert_eq!(l.local, Some(true));
    assert_eq!(l.currency.as_deref(), Some("EUR"));
    assert_eq!(l.company_micro_per_usd, None);
    assert_eq!(l.rate_snapshot, "none");
    assert_eq!(l.prices_updated, None);
    assert_eq!(l.counted_input_tokens, Some(900));
    assert_eq!(
        r.entry("model_call.reserved", &call)["args"]["local"],
        json!(1)
    );

    std::thread::sleep(std::time::Duration::from_millis(30));
    r.exec(WriteCommand::SettleAndFinishStep {
        call_id: call.clone(),
        usage: usage(1_200, 90),
        step_run_id: a.id.clone(),
        status: "succeeded".into(),
        output: Some("fine".into()),
    })
    .unwrap();

    let l = r.entry_of(&run, &call);
    assert_eq!(l.state, "settled");
    assert_eq!(l.settled_micro_usd, Some(0));
    assert_eq!(l.settled_micro_company, Some(0));
    let c = r.call(&run, &call);
    assert_eq!((c.input_tokens, c.output_tokens), (Some(1_200), Some(90)));
    let took = millis(c.settled_at.as_deref().expect("settled_at")) - millis(&c.created_at);
    assert!(took >= 30, "the call's time is kept: {took} ms");

    // A local Account that the price file prices by mistake is still 0.
    let b = r.begin(&run, "again");
    let priced_local = CallPrice {
        local: true,
        ..cloud()
    };
    let other = r.reserve(&b, priced_local, eur(), 900, 4_096);
    r.exec(WriteCommand::SettleCall {
        call_id: other.clone(),
        usage: usage(1_200, 90),
    })
    .unwrap();
    let l = r.entry_of(&run, &other);
    assert_eq!(
        (
            l.reserved_micro_usd,
            l.reserved_micro_company,
            l.settled_micro_usd,
            l.settled_micro_company,
            l.local
        ),
        (0, Some(0), Some(0), Some(0), Some(true))
    );
}

/// Row 10 (AD-8; owner Q14): a lost call settles at its reservation in
/// both currencies and stamps `settled_at`; a call already settled keeps
/// what it settled at.
#[test]
fn a_lost_call_settles_both_currencies_at_its_reservation() {
    let r = rig();
    let run = r.run(&["draft"]);
    let a = r.begin(&run, "draft");
    let open = r.reserve_cloud(&a);

    r.exec(WriteCommand::MarkLost {
        call_id: open.clone(),
    })
    .unwrap();

    let l = r.entry_of(&run, &open);
    assert_eq!(l.state, "settled");
    assert_eq!(l.settled_micro_usd, Some(64_440));
    assert_eq!(l.settled_micro_company, Some(59_285));
    let c = r.call(&run, &open);
    assert_eq!((c.status.as_str(), c.settled), ("lost", true));
    assert!(c.settled_at.is_some(), "{c:?}");

    // A settled call is unchanged by a later MarkLost.
    let paid = r.reserve_cloud(&a);
    r.exec(WriteCommand::SettleCall {
        call_id: paid.clone(),
        usage: usage(1_200, 90),
    })
    .unwrap();
    let (entry, call) = (r.entry_of(&run, &paid), r.call(&run, &paid));
    r.exec(WriteCommand::MarkLost {
        call_id: paid.clone(),
    })
    .unwrap();
    assert_eq!(r.entry_of(&run, &paid), entry);
    assert_eq!(r.call(&run, &paid), call);
    assert_eq!(entry.settled_micro_usd, Some(4_950));
}

// ------------------------------------------------------------------ integrity

/// Row 24 (owner Q13): migration 0013's triggers keep the ledger as
/// append-only as the journals: a reserve-time column never changes; the
/// settled columns are set once, together with `state` going from
/// `reserved` to `settled`; no row is deleted or replaced; no amount is
/// negative; a `local` row's amounts are 0. Each raw statement is aborted
/// with the row as it was; the writer refuses a second settlement. A
/// well-formed raw insert (the control) is accepted, so each refusal is its
/// own rule's.
#[test]
fn the_ledger_refuses_changes_to_what_was_reserved() {
    let r = rig();
    let run = r.run(&["draft"]);
    let a = r.begin(&run, "draft");
    let open = r.reserve_cloud(&a);
    let paid = r.reserve_cloud(&a);
    r.exec(WriteCommand::SettleCall {
        call_id: paid.clone(),
        usage: usage(1_200, 90),
    })
    .unwrap();
    let before = r.money_state();
    let raw = r.raw();
    let refused = |sql: &str, what: &str| {
        let done = raw.execute_batch(sql);
        assert!(done.is_err(), "{what} must be aborted: {sql}");
        assert_eq!(r.money_state(), before, "{what}: every row as it was");
    };

    // Reserve-time columns, on the open row and on the settled row.
    for call in [&open, &paid] {
        for (column, value) in [
            ("id", "'another-id'"),
            ("model_call_id", "'another-call'"),
            ("run_id", "'another-run'"),
            ("kind", "'service'"),
            ("reserved_micro_usd", "1"),
            ("rate_snapshot", "'none'"),
            ("created_at", "'2020-01-01T00:00:00.000Z'"),
            ("currency", "'USD'"),
            ("reserved_micro_company", "1"),
            ("input_micro_usd_per_mtok", "1"),
            ("output_micro_usd_per_mtok", "1"),
            ("company_micro_per_usd", "1000000"),
            ("prices_updated", "'2020-01-01'"),
            ("counted_input_tokens", "1"),
            ("max_output_tokens", "1"),
            ("local", "1"),
            ("week", "'2020-01-06'"),
            ("team", "'side'"),
        ] {
            refused(
                &format!("UPDATE ledger SET {column} = {value} WHERE model_call_id = '{call}'"),
                &format!("changing {column}"),
            );
        }
    }
    // A settled row settles once.
    for (sql, what) in [
        ("settled_micro_usd = 1", "a second USD settlement"),
        ("settled_micro_company = 1", "a second company settlement"),
        ("state = 'reserved'", "un-settling"),
        (
            "settled_micro_usd = 1, settled_micro_company = 1, state = 'settled'",
            "settling again",
        ),
    ] {
        refused(
            &format!("UPDATE ledger SET {sql} WHERE model_call_id = '{paid}'"),
            what,
        );
    }
    // An open row settles only with its state, both amounts at once, and
    // never below zero.
    for (sql, what) in [
        ("settled_micro_usd = 5", "an amount without the state"),
        ("state = 'settled'", "the state without the amounts"),
        (
            "settled_micro_usd = 5, state = 'settled'",
            "the USD amount without the company amount",
        ),
        (
            "settled_micro_usd = -1, settled_micro_company = -1, state = 'settled'",
            "a negative settlement",
        ),
    ] {
        refused(
            &format!("UPDATE ledger SET {sql} WHERE model_call_id = '{open}'"),
            what,
        );
    }
    refused(
        &format!("DELETE FROM ledger WHERE model_call_id = '{paid}'"),
        "a delete",
    );
    refused("DELETE FROM ledger", "deleting every row");
    refused(
        &format!(
            "INSERT OR REPLACE INTO ledger SELECT * FROM ledger WHERE model_call_id = '{paid}'"
        ),
        "a replace",
    );

    // New rows: a call of its own with no ledger row yet.
    raw.execute(
        "INSERT INTO model_calls (id, step_run_id, call_seq, status, settled, created_at, purpose) VALUES ('raw-call', ?1, 9, 'issued', 0, '2026-11-25T10:00:00.000Z', 'turn')",
        params![a.id],
    )
    .unwrap();
    let before = r.money_state();
    let insert = |reserved: i64, company: i64, local: i64| {
        format!(
            "INSERT INTO ledger (id, model_call_id, run_id, kind, reserved_micro_usd, state, rate_snapshot, created_at, currency, reserved_micro_company, input_micro_usd_per_mtok, output_micro_usd_per_mtok, company_micro_per_usd, prices_updated, counted_input_tokens, max_output_tokens, local, week, team) VALUES ('raw-entry', 'raw-call', '{run}', 'model', {reserved}, 'reserved', '{SNAPSHOT}', '2026-11-25T10:00:00.000Z', 'EUR', {company}, 3000000, 15000000, 920000, '{UPDATED}', 1000, 4096, {local}, '{WEEK}', '{TEAM}')"
        )
    };
    for (sql, what) in [
        (insert(-1, 0, 0), "a negative USD reservation"),
        (insert(0, -1, 0), "a negative company reservation"),
        (
            insert(64_440, 59_285, 1),
            "a local row that costs something",
        ),
        (insert(0, 5, 1), "a local row with a company amount"),
    ] {
        assert!(raw.execute_batch(&sql).is_err(), "{what} must be aborted");
        assert_eq!(r.money_state(), before, "{what}: nothing written");
    }
    raw.execute_batch(&insert(64_440, 59_285, 0))
        .expect("the control: a well-formed row is accepted");
    assert_eq!(r.count("ledger"), 3);
    drop(raw);

    // The writer settles a call once.
    let before = r.money_state();
    let again = r.exec(WriteCommand::SettleCall {
        call_id: paid.clone(),
        usage: usage(1, 1),
    });
    assert!(again.is_err(), "a second settlement is refused");
    assert_eq!(r.money_state(), before);
}

// ------------------------------------------------------------------ audit

/// Row 26 (AD-25): every settle path audits the amounts in both
/// currencies, the currency and the two flags, never `cost_micro_usd`; no
/// prompt text and no reply reaches any entry, no new `*_sha256` argument
/// appears (3.5's `output_sha256` is the only one), and the chain verifies.
#[test]
fn reserve_and_settle_audit_amounts_never_text() {
    let r = rig();
    let run = r.run(&["a", "b", "c"]);
    let prompt = vec![NewPromptPart {
        slot: PromptSlot::Step,
        kind: PromptKind::Instruction,
        label: "Step instruction".into(),
        taint: Taint::Clean,
        text: Some(format!("Do the work. {CANARY_PROMPT}")),
    }];
    let reply = format!("The answer. {CANARY_REPLY}");

    // SettleCall, then SettleAndFinishStep.
    let a = r.begin_with(&run, "a", None, prompt.clone());
    let a1 = r.reserve_cloud(&a);
    r.exec(WriteCommand::SettleCall {
        call_id: a1.clone(),
        usage: usage(1_200, 90),
    })
    .unwrap();
    let a2 = r.reserve_cloud(&a);
    r.exec(WriteCommand::SettleAndFinishStep {
        call_id: a2.clone(),
        usage: usage(1_200, 90),
        step_run_id: a.id.clone(),
        status: "succeeded".into(),
        output: Some(reply.clone()),
    })
    .unwrap();
    // SettleAndLoopBack.
    let b = r.begin_with(&run, "b", None, prompt.clone());
    let b1 = r.reserve_cloud(&b);
    r.exec(WriteCommand::SettleAndLoopBack {
        call_id: b1.clone(),
        usage: usage(1_200, 90),
        step_run_id: b.id.clone(),
        output: format!(r#"{{"reasons":["{CANARY_REPLY}"]}}"#),
        to: "a".into(),
    })
    .unwrap();
    // SettleAndFinishDocumentStep, on another run.
    let doc_run = r.run(&["summarise"]);
    let d = r.begin_with(&doc_run, "summarise", Some(DOCUMENT), prompt.clone());
    let doc_call = r.reserve_cloud(&d);
    r.exec(WriteCommand::SettleAndFinishDocumentStep {
        call_id: doc_call.clone(),
        usage: usage(1_200, 90),
        step_run_id: d.id.clone(),
        output: format!("# Note\n\n{CANARY_REPLY}\n"),
        version_id: new_id(),
        document_id: DOCUMENT.into(),
        expected_version: 1,
        item: draft_item(&d),
    })
    .unwrap();
    // FailWithItem, on a third run.
    let fail_run = r.run(&["only"]);
    let f = r.begin_with(&fail_run, "only", None, prompt);
    let fail_call = r.reserve_cloud(&f);
    r.exec(WriteCommand::FailWithItem {
        step_run_id: f.id.clone(),
        settle: Some(CallSettlement {
            call_id: fail_call.clone(),
            usage: usage(1_200, 90),
        }),
        output: Some(reply.clone()),
        item: failure_item(&f),
    })
    .unwrap();

    for (call, action) in [
        (&a1, "model_call.settled"),
        (&a2, "model_call.settled+step.finished"),
        (&b1, "model_call.settled+step.looped_back"),
        (&doc_call, "model_call.settled+step.finished"),
        (&fail_call, "model_call.settled+step.finished"),
    ] {
        let args = &r.entry(action, call)["args"];
        assert_eq!(args["settled_micro_usd"], json!(4_950), "{action}: {args}");
        assert_eq!(
            args["settled_micro_company"],
            json!(4_554),
            "{action}: {args}"
        );
        assert_eq!(args["currency"], json!("EUR"), "{action}: {args}");
        assert_eq!(args["input_tokens"], json!(1_200), "{action}: {args}");
        assert_eq!(args["output_tokens"], json!(90), "{action}: {args}");
        assert_eq!(args["at_reservation"], json!(false), "{action}: {args}");
        assert_eq!(args["over_reservation"], json!(false), "{action}: {args}");
        assert!(
            args.get("cost_micro_usd").is_none(),
            "{action}: the caller's amount is gone (owner Q4): {args}"
        );
    }
    for call in [&a1, &a2, &b1, &doc_call, &fail_call] {
        let args = &r.entry("model_call.reserved", call)["args"];
        assert_eq!(args["reserved_micro_usd"], json!(64_440), "{args}");
        assert_eq!(args["reserved_micro_company"], json!(59_285), "{args}");
        assert_eq!(args["rate_snapshot"], json!(SNAPSHOT), "{args}");
        assert_eq!(args["week"], json!(WEEK), "{args}");
        assert_eq!(args["team"], json!(TEAM), "{args}");
    }

    let text = r.audit_text();
    for canary in [CANARY_PROMPT, CANARY_REPLY] {
        assert!(!text.contains(canary), "{canary} is in the audit log");
    }
    let hashes: BTreeSet<String> = r
        .audit()
        .iter()
        .filter_map(|e| e["args"].as_object().cloned())
        .flat_map(|args| args.keys().cloned().collect::<Vec<_>>())
        .filter(|k| k.ends_with("_sha256"))
        .collect();
    assert!(
        hashes.iter().all(|k| k == "output_sha256"),
        "no new hash of free text: {hashes:?}"
    );
    assert_eq!(r.reader().verify_audit_chain().unwrap(), None);
}

// ------------------------------------------------------------------ review round 1

/// A raw read-write connection with `recursive_triggers` OFF, as an outside
/// tool opens the database (the store's own connections turn it on).
fn outside(r: &Rig) -> Connection {
    let raw = r.raw();
    raw.pragma_update(None, "recursive_triggers", "OFF")
        .unwrap();
    let on: i64 = raw
        .pragma_query_value(None, "recursive_triggers", |row| row.get(0))
        .unwrap();
    assert_eq!(on, 0, "recursive triggers are off on this connection");
    raw
}

/// Every ledger row's rowid and call, in rowid order.
fn ledger_rowids(r: &Rig) -> Vec<(i64, String)> {
    let c = Connection::open_with_flags(&r.db, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let mut st = c
        .prepare("SELECT rowid, model_call_id FROM ledger ORDER BY rowid")
        .unwrap();
    st.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// A raw ledger insert for the call `call` of `run`: `state` and the
/// settled columns as given, the matrix's cloud terms, and the price and
/// rate columns as given.
#[allow(clippy::too_many_arguments)]
fn raw_entry(
    id: &str,
    call: &str,
    run: &str,
    state: &str,
    settled: (&str, &str),
    terms: (i64, i64),
    per_usd: &str,
) -> String {
    format!(
        "INSERT INTO ledger (id, model_call_id, run_id, kind, reserved_micro_usd, settled_micro_usd, state, rate_snapshot, created_at, currency, reserved_micro_company, settled_micro_company, input_micro_usd_per_mtok, output_micro_usd_per_mtok, company_micro_per_usd, prices_updated, counted_input_tokens, max_output_tokens, local, week, team) VALUES ('{id}', '{call}', '{run}', 'model', 64440, {}, '{state}', '{SNAPSHOT}', '2026-11-25T10:00:00.000Z', 'EUR', 59285, {}, {}, {}, {per_usd}, '{UPDATED}', 1000, 4096, 0, '{WEEK}', '{TEAM}')",
        settled.0, settled.1, terms.0, terms.1
    )
}

/// A model call of the attempt `a` made by hand, with no ledger row.
fn raw_call(raw: &Connection, a: &Attempt, id: &str, seq: i64) {
    raw.execute(
        "INSERT INTO model_calls (id, step_run_id, call_seq, status, settled, created_at, purpose) VALUES (?1, ?2, ?3, 'issued', 0, '2026-11-25T10:00:00.000Z', 'turn')",
        params![id, a.id, seq],
    )
    .unwrap();
}

/// Review round 1, M2 (owner Q13): with recursive triggers off, an
/// `UPDATE OR REPLACE` that moves a reserved row onto a settled row's rowid
/// (which would delete the settled row without firing the DELETE trigger)
/// is aborted, and so is any change of a row's rowid; a raw insert that is
/// not `reserved`, or that carries a settled amount, is aborted. Every row
/// and rowid is as it was; a well-formed reserved insert is accepted (the
/// control).
#[test]
fn a_ledger_row_is_never_replaced_through_its_rowid_or_inserted_settled() {
    let r = rig();
    let run = r.run(&["draft"]);
    let a = r.begin(&run, "draft");
    let open = r.reserve_cloud(&a);
    let paid = r.reserve_cloud(&a);
    r.exec(WriteCommand::SettleCall {
        call_id: paid.clone(),
        usage: usage(1_200, 90),
    })
    .unwrap();
    let raw = outside(&r);
    let (before, rowids) = (r.money_state(), ledger_rowids(&r));
    let aborted = |sql: &str, what: &str| {
        assert!(
            raw.execute_batch(sql).is_err(),
            "{what} must be aborted: {sql}"
        );
        assert_eq!(r.money_state(), before, "{what}: every row as it was");
        assert_eq!(ledger_rowids(&r), rowids, "{what}: every rowid as it was");
    };

    aborted(
        &format!(
            "UPDATE OR REPLACE ledger SET rowid = (SELECT rowid FROM ledger WHERE model_call_id = '{paid}') WHERE model_call_id = '{open}'"
        ),
        "moving a reserved row onto a settled row's rowid",
    );
    aborted(
        &format!("UPDATE ledger SET rowid = rowid + 1000 WHERE model_call_id = '{open}'"),
        "changing a reserved row's rowid",
    );
    assert_eq!(r.entry_of(&run, &paid).settled_micro_usd, Some(4_950));

    raw_call(&raw, &a, "raw-call", 9);
    let (before, rowids) = (r.money_state(), ledger_rowids(&r));
    let aborted = |sql: &str, what: &str| {
        assert!(
            raw.execute_batch(sql).is_err(),
            "{what} must be aborted: {sql}"
        );
        assert_eq!(r.money_state(), before, "{what}: nothing written");
        assert_eq!(ledger_rowids(&r), rowids, "{what}: nothing written");
    };
    let terms = (3_000_000, 15_000_000);
    aborted(
        &raw_entry(
            "raw-entry",
            "raw-call",
            &run,
            "settled",
            ("64440", "59285"),
            terms,
            "920000",
        ),
        "a row inserted settled",
    );
    aborted(
        &raw_entry(
            "raw-entry",
            "raw-call",
            &run,
            "reserved",
            ("4950", "NULL"),
            terms,
            "920000",
        ),
        "a reserved row inserted with a USD settlement",
    );
    aborted(
        &raw_entry(
            "raw-entry",
            "raw-call",
            &run,
            "reserved",
            ("NULL", "4554"),
            terms,
            "920000",
        ),
        "a reserved row inserted with a company settlement",
    );
    raw.execute_batch(&raw_entry(
        "raw-entry",
        "raw-call",
        &run,
        "reserved",
        ("NULL", "NULL"),
        terms,
        "920000",
    ))
    .expect("the control: a reserved row with no settlement is accepted");
    assert_eq!(r.count("ledger"), 3);
}

/// Review round 1, M3 (AD-9, owner Q13): 0013's insert trigger refuses
/// negative price terms and a rate of 0 or below; the writer refuses a
/// reservation whose currency is not three capitals, whose week is not an
/// ISO date, or whose team is not a config id, with nothing written. The
/// controls are accepted.
#[test]
fn the_ledger_refuses_bad_terms_and_the_writer_bad_words() {
    let r = rig();
    let run = r.run(&["draft"]);
    let a = r.begin(&run, "draft");
    let raw = outside(&r);
    raw_call(&raw, &a, "raw-call", 9);
    let before = r.money_state();
    for (what, terms, per_usd) in [
        ("a negative input price", (-1, 15_000_000), "920000"),
        ("a negative output price", (3_000_000, -1), "920000"),
        ("a rate of 0", (3_000_000, 15_000_000), "0"),
        ("a negative rate", (3_000_000, 15_000_000), "-920000"),
    ] {
        let sql = raw_entry(
            "raw-entry",
            "raw-call",
            &run,
            "reserved",
            ("NULL", "NULL"),
            terms,
            per_usd,
        );
        assert!(raw.execute_batch(&sql).is_err(), "{what} must be aborted");
        assert_eq!(r.money_state(), before, "{what}: nothing written");
    }
    raw.execute_batch(&raw_entry(
        "raw-entry",
        "raw-call",
        &run,
        "reserved",
        ("NULL", "NULL"),
        (3_000_000, 15_000_000),
        "NULL",
    ))
    .expect("the control: no rate at all (NULL) is not a bad rate");
    drop(raw);

    let before = r.money_state();
    let words = |currency: &str, week: &str, team: &str| {
        let seq = r.reader().calls_for_step(&a.id).unwrap().len() as i64 + 1;
        WriteCommand::ReserveCall {
            step_run_id: a.id.clone(),
            call_id: new_id(),
            call_seq: seq,
            price: cloud(),
            rate: Rate {
                currency: currency.into(),
                ..eur()
            },
            counted_input_tokens: 1_000,
            max_output_tokens: 4_096,
            week: week.into(),
            team: team.into(),
            taint: Taint::Clean,
            purpose: CallPurpose::Turn,
            prompt: vec![],
        }
    };
    for (what, cmd) in [
        ("a lower-case currency", words("eur", WEEK, TEAM)),
        ("a four-letter currency", words("EURO", WEEK, TEAM)),
        ("an empty currency", words("", WEEK, TEAM)),
        ("an ISO week label", words("EUR", "2026-W48", TEAM)),
        ("a date that is not one", words("EUR", "2026-13-01", TEAM)),
        ("an empty week", words("EUR", "", TEAM)),
        ("a team that is not an id", words("EUR", WEEK, "Core Team")),
        ("a team with a slash", words("EUR", WEEK, "core/side")),
    ] {
        let refused = r.exec(cmd);
        assert!(refused.is_err(), "{what} is refused");
        assert!(
            !refused.unwrap_err().to_string().trim().is_empty(),
            "{what}: in words"
        );
        assert_eq!(r.money_state(), before, "{what}: nothing written");
    }
    r.exec(words("EUR", WEEK, TEAM))
        .expect("the control: three capitals, an ISO date and a config id");
}

/// Review round 1, M8 (AD-25): `model_call.lost` audits the amounts the
/// lost call settled at (its reservation, in USD and the company currency)
/// and the currency, exactly.
#[test]
fn a_lost_call_audits_what_it_settled_at() {
    let r = rig();
    let run = r.run(&["draft"]);
    let a = r.begin(&run, "draft");
    let call = r.reserve_cloud(&a);

    r.exec(WriteCommand::MarkLost {
        call_id: call.clone(),
    })
    .unwrap();

    assert_eq!(
        r.entry("model_call.lost", &call)["args"],
        json!({
            "currency": "EUR",
            "settled_micro_company": 59_285,
            "settled_micro_usd": 64_440,
        })
    );
}

/// Review round 1, M10 (AD-8, AD-9): once a call's tokens and `settled_at`
/// are set, they never change, also from an outside tool with recursive
/// triggers off: a raw update of either count or of `settled_at`, to
/// another value or to NULL, is aborted, on a call settled from usage and
/// on one marked lost. An open call still settles through the writer (the
/// control).
#[test]
fn a_calls_tokens_and_settled_at_never_change_once_set() {
    let r = rig();
    let run = r.run(&["draft"]);
    let a = r.begin(&run, "draft");
    let paid = r.reserve_cloud(&a);
    r.exec(WriteCommand::SettleCall {
        call_id: paid.clone(),
        usage: usage(1_200, 90),
    })
    .unwrap();
    let lost = r.reserve_cloud(&a);
    r.exec(WriteCommand::MarkLost {
        call_id: lost.clone(),
    })
    .unwrap();
    let open = r.reserve_cloud(&a);
    let raw = outside(&r);
    let before = r.money_state();
    for (call, set) in [
        (&paid, "input_tokens = 1"),
        (&paid, "output_tokens = 1"),
        (&paid, "input_tokens = NULL"),
        (&paid, "output_tokens = NULL"),
        (&paid, "settled_at = '2020-01-01T00:00:00.000Z'"),
        (&paid, "settled_at = NULL"),
        (&lost, "settled_at = '2020-01-01T00:00:00.000Z'"),
        (&lost, "settled_at = NULL"),
    ] {
        let sql = format!("UPDATE model_calls SET {set} WHERE id = '{call}'");
        assert!(raw.execute_batch(&sql).is_err(), "{sql} must be aborted");
        assert_eq!(r.money_state(), before, "{sql}: as it was");
    }
    drop(raw);
    r.exec(WriteCommand::SettleCall {
        call_id: open.clone(),
        usage: usage(1_200, 90),
    })
    .expect("the control: an open call settles once");
    let c = r.call(&run, &open);
    assert_eq!((c.input_tokens, c.output_tokens), (Some(1_200), Some(90)));
    assert!(c.settled_at.is_some());
}

// ------------------------------------------------------------------ spend

/// Owner Q12, Q13, Q15 (tester's: the reader `agentic status` rolls up
/// from): one row per call of every run, oldest run first, in step, attempt
/// and call order, joined to its run, workflow, step key, attempt and
/// seat (`step_runs.seat`, journaled at start), with the ledger's team,
/// week, currency, amounts, `local`, snapshot and date, and the call's
/// tokens and times.
#[test]
fn spend_reads_one_row_per_call_with_its_step_seat_team_and_week() {
    let r = rig();
    let run = r.run(&["draft", "check"]);
    let draft = r.begin(&run, "draft");
    let cloud_call = r.reserve_cloud(&draft);
    r.exec(WriteCommand::SettleAndFinishStep {
        call_id: cloud_call.clone(),
        usage: usage(1_200, 90),
        step_run_id: draft.id.clone(),
        status: "succeeded".into(),
        output: Some("a line".into()),
    })
    .unwrap();
    let check = r.begin(&run, "check");
    let local_call = r.reserve(&check, CallPrice::local(), eur(), 700, 512);
    let later = r.run(&["draft"]);
    let open_attempt = r.begin(&later, "draft");
    let open = r.reserve_cloud(&open_attempt);

    let rows = r.reader().spend().unwrap();

    let ids: Vec<&str> = rows.iter().map(|s| s.model_call_id.as_str()).collect();
    assert_eq!(
        ids,
        vec![cloud_call.as_str(), local_call.as_str(), open.as_str()]
    );
    let s = &rows[0];
    assert_eq!(
        (
            s.run_id.as_str(),
            s.workflow.as_str(),
            s.step_key.as_str(),
            s.attempt
        ),
        (run.as_str(), "wf", "draft", 1)
    );
    assert_eq!(s.step_run_id, draft.id);
    assert_eq!(s.seat.as_deref(), Some(SEAT));
    assert_eq!(s.team.as_deref(), Some(TEAM));
    assert_eq!(s.week.as_deref(), Some(WEEK));
    assert_eq!(s.currency.as_deref(), Some("EUR"));
    assert_eq!(s.state, "settled");
    assert_eq!(s.local, Some(false));
    assert_eq!(
        (s.reserved_micro_usd, s.settled_micro_usd),
        (64_440, Some(4_950))
    );
    assert_eq!(
        (s.reserved_micro_company, s.settled_micro_company),
        (Some(59_285), Some(4_554))
    );
    assert_eq!((s.input_tokens, s.output_tokens), (Some(1_200), Some(90)));
    assert_eq!(s.rate_snapshot, SNAPSHOT);
    assert_eq!(s.prices_updated.as_deref(), Some(UPDATED));
    assert_eq!(s.company_micro_per_usd, Some(920_000));
    assert!(millis(s.settled_at.as_deref().unwrap()) >= millis(&s.created_at));
    let l = &rows[1];
    assert_eq!(
        (l.step_key.as_str(), l.local, l.state.as_str()),
        ("check", Some(true), "reserved")
    );
    assert_eq!(
        (l.reserved_micro_usd, l.reserved_micro_company),
        (0, Some(0))
    );
    let o = &rows[2];
    assert_eq!(
        (o.run_id.as_str(), o.state.as_str()),
        (later.as_str(), "reserved")
    );
    assert_eq!((o.settled_micro_usd, o.settled_at.as_deref()), (None, None));
}

// ------------------------------------------------------------------ migration 0013
//
// Row 25 (AD-7, AD-35; owner Q13): 0013 adds the cost columns behind AD-7's
// pre-migration copy, in the pattern of 3.6b's 0012 tests
// (`tests/documents.rs`).

/// The schema version before migration 0013.
const BEFORE_COSTS: i64 = 12;

fn layout(root: &Path) -> RuntimeLayout {
    RuntimeLayout::at(root.join("rt"))
}

fn open_project(root: &Path) -> Result<(Store, OpenReport), StoreError> {
    am_store::open_runtime(&layout(root), "test-mac".into())
}

/// The runtime folder made by hand, owner-only, as an earlier open left it.
fn existing_runtime(root: &Path) {
    let dir = layout(root).dir().to_path_buf();
    std::fs::create_dir(&dir).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
}

fn user_version(conn: &Connection) -> i64 {
    conn.pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap()
}

fn columns(db: &Path, table: &str) -> Vec<String> {
    let c = Connection::open(db).unwrap();
    let mut st = c
        .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
        .unwrap();
    st.query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// A schema-12 database in the project's runtime folder, made by hand.
fn database_at_12(root: &Path) -> PathBuf {
    existing_runtime(root);
    let path = layout(root).state_db();
    am_store::testing::database_at_version(&path, BEFORE_COSTS).unwrap();
    path
}

/// Rows in every table a run touches, as the writers up to 3.6b made them
/// at version 12: three runs (one running, one failed, one with a document
/// version); attempts with their journaled writes; settled, open and repair
/// calls with their ledger rows (one with a USD amount, as the spike made);
/// inputs, tool calls, rows, Inbox items (a Review, a dismissed Failure, a
/// Draft), prompt parts, a document version, a trigger and a fire.
fn rows_at_12(path: &Path) {
    let step = "id, run_id, step_key, seq, attempt, status, output, taint, updated_at, started_at, finished_at";
    let call = "id, step_run_id, call_seq, status, settled, input_tokens, output_tokens, created_at, purpose";
    let entry = "id, model_call_id, run_id, kind, reserved_micro_usd, settled_micro_usd, state, rate_snapshot, created_at";
    let tool = "id, step_run_id, call_seq, tool, status, created_at, finished_at";
    let image = "table_id, row_id, version, data, taint, actor, run_id, step_run_id, seat, call_seq, tool_call_id, op, created_at";
    let row = "table_id, row_id, data, version, taint, created_at, updated_at";
    let input = "step_run_id, kind, from_step_run, label, taint, created_at, from_item, from_row";
    let part =
        "step_run_id, call_seq, ord, slot, kind, label, taint, text, bytes, sha256, created_at";
    let sql = r#"
        INSERT INTO runs (id, workflow, origin, status, created_at, updated_at, config_hash, finished_at)
          VALUES ('r1', 'wf', 'manual', 'running', '2026-11-25T10:00:00.000Z', '2026-11-25T10:00:09.000Z', '@HASH@', NULL);
        INSERT INTO runs (id, workflow, origin, status, created_at, updated_at, config_hash, finished_at)
          VALUES ('r2', 'wf', 'schedule', 'failed', '2026-11-25T11:00:00.000Z', '2026-11-25T11:00:09.000Z', NULL, '2026-11-25T11:00:09.000Z');
        INSERT INTO runs (id, workflow, origin, status, created_at, updated_at, config_hash, finished_at)
          VALUES ('r3', 'wf', 'manual', 'succeeded', '2026-11-25T12:00:00.000Z', '2026-11-25T12:00:09.000Z', '@HASH@', '2026-11-25T12:00:09.000Z');
        INSERT INTO step_runs (@STEP@)
          VALUES ('r1-a', 'r1', 'a', 0, 1, 'queued', NULL, 0, '2026-11-25T10:00:00.000Z', NULL, NULL);
        INSERT INTO step_runs (@STEP@)
          VALUES ('r1-b', 'r1', 'b', 1, 1, 'queued', NULL, 0, '2026-11-25T10:00:00.000Z', NULL, NULL);
        INSERT INTO step_runs (@STEP@)
          VALUES ('r2-a', 'r2', 'a', 0, 1, 'queued', NULL, 0, '2026-11-25T11:00:00.000Z', NULL, NULL);
        INSERT INTO step_runs (@STEP@)
          VALUES ('r3-d', 'r3', 'd', 0, 1, 'queued', NULL, 0, '2026-11-25T12:00:00.000Z', NULL, NULL);
        UPDATE step_runs SET status = 'running', seat = 'seat-writer', writes_table = 'entries', taint = 1,
          started_at = '2026-11-25T10:00:01.000Z', updated_at = '2026-11-25T10:00:01.000Z' WHERE id = 'r1-a';
        INSERT INTO tool_calls (@TOOL@)
          VALUES ('t1', 'r1-a', 1, 'table_write', 'issued', '2026-11-25T10:00:02.000Z', NULL);
        INSERT INTO row_history (@IMAGE@)
          VALUES ('entries', '@ROW1@', 1, '{"ref":"r-1"}', 1, '@SEAT@', 'r1', 'r1-a', 'seat-writer', 1, 't1', 'insert', '2026-11-25T10:00:02.100Z');
        INSERT INTO table_rows (@ROW@)
          VALUES ('entries', '@ROW1@', '{"ref":"r-1"}', 1, 1, '2026-11-25T10:00:02.100Z', '2026-11-25T10:00:02.100Z');
        UPDATE tool_calls SET status = 'succeeded', finished_at = '2026-11-25T10:00:02.100Z',
          result_kind = 'engine', result_label = 'tool:table_write' WHERE id = 't1';
        UPDATE step_runs SET status = 'succeeded', output = 'out of a', finished_at = '2026-11-25T10:00:03.000Z',
          updated_at = '2026-11-25T10:00:03.000Z' WHERE id = 'r1-a';
        UPDATE step_runs SET status = 'running', seat = 'seat-writer', taint = 1,
          started_at = '2026-11-25T10:00:04.000Z', updated_at = '2026-11-25T10:00:09.000Z' WHERE id = 'r1-b';
        INSERT INTO tool_calls (@TOOL@)
          VALUES ('t2', 'r1-b', 1, 'fixture_read', 'issued', '2026-11-25T10:00:06.000Z', NULL);
        UPDATE tool_calls SET status = 'succeeded', finished_at = '2026-11-25T10:00:07.000Z',
          result_kind = 'untrusted', result_label = 'tool:fixture_read' WHERE id = 't2';
        UPDATE step_runs SET status = 'failed', started_at = '2026-11-25T11:00:01.000Z',
          finished_at = '2026-11-25T11:00:09.000Z', updated_at = '2026-11-25T11:00:09.000Z' WHERE id = 'r2-a';
        UPDATE step_runs SET status = 'running', seat = 'seat-writer', writes_document = 'summary-note',
          started_at = '2026-11-25T12:00:01.000Z', updated_at = '2026-11-25T12:00:01.000Z' WHERE id = 'r3-d';
        INSERT INTO model_calls (@CALL@)
          VALUES ('c1', 'r1-a', 1, 'settled', 1, 12, 7, '2026-11-25T10:00:02.000Z', 'turn');
        INSERT INTO model_calls (@CALL@)
          VALUES ('c4', 'r1-a', 2, 'settled', 1, 9, 4, '2026-11-25T10:00:02.500Z', 'repair');
        INSERT INTO model_calls (@CALL@)
          VALUES ('c2', 'r1-b', 1, 'issued', 0, NULL, NULL, '2026-11-25T10:00:05.000Z', 'turn');
        INSERT INTO model_calls (@CALL@)
          VALUES ('c3', 'r2-a', 1, 'settled', 1, 3, 0, '2026-11-25T11:00:02.000Z', 'turn');
        INSERT INTO model_calls (@CALL@)
          VALUES ('c5', 'r3-d', 1, 'settled', 1, 40, 12, '2026-11-25T12:00:02.000Z', 'turn');
        INSERT INTO ledger (@ENTRY@)
          VALUES ('l1', 'c1', 'r1', 'model', 1500, 1200, 'settled', 'spike-2026-11-19', '2026-11-25T10:00:02.000Z');
        INSERT INTO ledger (@ENTRY@)
          VALUES ('l4', 'c4', 'r1', 'model', 0, 0, 'settled', 'none', '2026-11-25T10:00:02.500Z');
        INSERT INTO ledger (@ENTRY@)
          VALUES ('l2', 'c2', 'r1', 'model', 0, NULL, 'reserved', 'none', '2026-11-25T10:00:05.000Z');
        INSERT INTO ledger (@ENTRY@)
          VALUES ('l3', 'c3', 'r2', 'model', 0, 0, 'settled', 'none', '2026-11-25T11:00:02.000Z');
        INSERT INTO ledger (@ENTRY@)
          VALUES ('l5', 'c5', 'r3', 'model', 0, 0, 'settled', 'none', '2026-11-25T12:00:02.000Z');
        UPDATE step_runs SET status = 'succeeded', output = '# Note

One line.', finished_at = '2026-11-25T12:00:03.000Z',
          updated_at = '2026-11-25T12:00:03.000Z' WHERE id = 'r3-d';
        INSERT INTO document_versions (id, document_id, version, run_id, attempt, step_run_id, taint, bytes, created_at)
          VALUES ('v1', 'summary-note', 1, 'r3', 1, 'r3-d', 0, 17, '2026-11-25T12:00:03.000Z');
        INSERT INTO step_inputs (@INPUT@)
          VALUES ('r1-b', 'step_output', 'r1-a', 'step:a@1', 1, '2026-11-25T10:00:04.000Z', NULL, NULL);
        INSERT INTO step_inputs (@INPUT@)
          VALUES ('r1-b', 'row', NULL, 'row:entries/@ROW1@@1', 1, '2026-11-25T10:00:04.000Z', NULL,
                  (SELECT seq FROM row_history WHERE table_id = 'entries' AND row_id = '@ROW1@' AND version = 1));
        INSERT INTO inbox_items (id, kind, status, run_id, step_run_id, payload, taint, title, body, created_at, max_returns)
          VALUES ('i1', 'review', 'open', 'r1', 'r1-b', 'r1-a', 1, 'Review', 'Step b waits for your review.', '2026-11-25T10:00:09.000Z', 2);
        INSERT INTO inbox_items (id, kind, status, run_id, step_run_id, payload, taint, title, body, created_at, resolved_at, resolved_by, decision)
          VALUES ('i2', 'failure', 'resolved', 'r2', 'r2-a', 'r2-a', 0, 'A step stopped', 'Step a stopped.', '2026-11-25T11:00:09.000Z', '2026-11-25T11:05:00.000Z', '@HUMAN@', 'dismiss');
        INSERT INTO inbox_items (id, kind, status, run_id, step_run_id, payload, taint, title, body, created_at)
          VALUES ('i3', 'draft', 'open', 'r3', 'r3-d', 'r3-d', 0, 'Draft: summary-note', 'Step d wrote version 1 of summary-note.', '2026-11-25T12:00:03.000Z');
        INSERT INTO prompt_parts (@PART@)
          VALUES ('r1-b', 0, 1, 'step', 'instruction', 'Step instruction', 0, 'Do the work of b.', 17, NULL, '2026-11-25T10:00:04.000Z');
        INSERT INTO prompt_parts (@PART@)
          VALUES ('r1-b', 0, 2, 'untrusted', 'untrusted', 'step:a@1', 1, NULL, 0, NULL, '2026-11-25T10:00:04.000Z');
        INSERT INTO triggers (id, cron, tz, catch_up, state, next_fire, last_fired)
          VALUES ('trg', '0 9 * * *', 'UTC', 'once', 'active', '2026-11-26T09:00:00.000Z', NULL);
        INSERT INTO fires (trigger_id, scheduled_for, fired_at, kind)
          VALUES ('trg', '2026-11-25T09:00:00.000Z', '2026-11-25T09:00:01.000Z', 'on_time');
    "#;
    let seat = json!({
        "kind": "seat",
        "seat": SEAT,
        "run": "0193f1a2-0000-7000-8000-000000000001",
        "step": "a",
    })
    .to_string();
    let human = json!({ "kind": "human", "person": "owner", "hat": "Reviewer" }).to_string();
    let sql = sql
        .replace("@STEP@", step)
        .replace("@CALL@", call)
        .replace("@ENTRY@", entry)
        .replace("@TOOL@", tool)
        .replace("@IMAGE@", image)
        .replace("@ROW@", row)
        .replace("@INPUT@", input)
        .replace("@PART@", part)
        .replace("@HASH@", HASH)
        .replace("@ROW1@", ROW_1)
        .replace("@SEAT@", &seat)
        .replace("@HUMAN@", &human);
    Connection::open(path).unwrap().execute_batch(&sql).unwrap();
}

/// The id of the row the fixture's seat wrote.
const ROW_1: &str = "0193f1a2-0000-7000-8000-00000000c001";

/// Every table's columns and rows (raw cells, in rowid order) as `path`
/// holds them now.
type Tables = BTreeMap<String, (Vec<String>, Vec<Vec<Cell>>)>;

fn tables_of(path: &Path) -> Tables {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let names: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let mut out = BTreeMap::new();
    for name in names {
        let cols = columns(path, &name);
        out.insert(name.clone(), (cols.clone(), cells_of(&conn, &name, &cols)));
    }
    out
}

fn cells_of(conn: &Connection, table: &str, cols: &[String]) -> Vec<Vec<Cell>> {
    let mut st = conn
        .prepare(&format!(
            "SELECT {} FROM \"{table}\" ORDER BY rowid",
            cols.join(", ")
        ))
        .unwrap();
    let n = st.column_count();
    st.query_map([], |r| {
        (0..n)
            .map(|i| r.get::<_, Cell>(i))
            .collect::<rusqlite::Result<Vec<Cell>>>()
    })
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap()
}

/// The number of files under `dir`, recursively (0 when it is missing).
fn walk_count(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| {
            let p = e.path();
            if p.is_dir() { walk_count(&p) } else { 1 }
        })
        .sum()
}

/// The columns 0013 adds to `ledger` (owner Q13).
const LEDGER_COLUMNS: [&str; 12] = [
    "currency",
    "reserved_micro_company",
    "settled_micro_company",
    "input_micro_usd_per_mtok",
    "output_micro_usd_per_mtok",
    "company_micro_per_usd",
    "prices_updated",
    "counted_input_tokens",
    "max_output_tokens",
    "local",
    "week",
    "team",
];

/// Row 25: every row of every table the schema-12 database held is the
/// same value for value after 0013 (in its old columns; the two logs a
/// migration appends to keep their old rows as a prefix); the
/// pre-migration copy the report names exists, opens at version 12 and
/// holds every row as it was, also after a reopen; the step is reported
/// 12 to 13 and audited `store.migrated`.
#[test]
fn migration_0013_keeps_every_row_and_the_pre_migration_copy() {
    let dir = tempfile::tempdir().unwrap();
    let path = database_at_12(dir.path());
    rows_at_12(&path);
    let before = tables_of(&path);
    for table in [
        "runs",
        "step_runs",
        "model_calls",
        "ledger",
        "step_inputs",
        "tool_calls",
        "inbox_items",
        "prompt_parts",
        "table_rows",
        "row_history",
        "document_versions",
        "triggers",
        "fires",
    ] {
        assert!(!before[table].1.is_empty(), "{table} holds rows to keep");
    }
    for column in LEDGER_COLUMNS {
        assert!(
            !before["ledger"].0.contains(&column.to_string()),
            "ledger.{column} is 0013's"
        );
    }
    assert!(
        !before["model_calls"].0.contains(&"settled_at".to_string()),
        "model_calls.settled_at is 0013's"
    );

    let (store, report) = open_project(dir.path()).unwrap();

    // A later story may add a migration after it: the 12 to 13 step is
    // applied on the way, not necessarily last.
    const { assert!(SCHEMA_VERSION > BEFORE_COSTS) };
    assert_eq!(report.version, SCHEMA_VERSION);
    let step = report
        .applied
        .iter()
        .find(|m| m.from == BEFORE_COSTS)
        .expect("the 12 to 13 step is reported");
    assert_eq!(step.to, BEFORE_COSTS + 1);
    let backup = step.backup.clone().expect("a pre-migration copy (AD-7)");
    assert!(backup.is_file(), "{}", backup.display());
    assert!(
        report
            .lines()
            .iter()
            .any(|l| l.contains("from version 12 to 13")),
        "{:?}",
        report.lines()
    );

    let after = tables_of(&path);
    let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    for (table, (cols, rows)) in &before {
        let (_, now) = after
            .get(table)
            .unwrap_or_else(|| panic!("{table} was dropped"));
        let old_columns_now = cells_of(&conn, table, cols);
        if table == "audit_log" || table == "migration_reports" {
            assert!(
                old_columns_now.starts_with(rows),
                "{table}: the old entries are kept, in order"
            );
        } else {
            assert_eq!(
                &old_columns_now, rows,
                "{table}: every row, value for value"
            );
            assert_eq!(now.len(), rows.len(), "{table}: no row added or lost");
        }
    }
    drop(conn);

    let copy_is_as_it_was = |what: &str| {
        let copy = Connection::open_with_flags(&backup, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        assert_eq!(user_version(&copy), BEFORE_COSTS, "{what}");
        drop(copy);
        assert_eq!(
            tables_of(&backup),
            before,
            "{what}: the copy holds every row"
        );
    };
    copy_is_as_it_was("after the upgrade");

    let reader = store.reader().unwrap();
    let mut out = Vec::new();
    reader.export_audit_jsonl(&mut out).unwrap();
    let all: Vec<Value> = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert!(
        all.iter().any(|e| e["action"] == "store.migrated"
            && e["outcome"]
                .as_str()
                .is_some_and(|o| o.starts_with("from version 12 to 13"))),
        "{all:#?}"
    );
    assert_eq!(reader.verify_audit_chain().unwrap(), None);
    drop(reader);
    drop(store);
    let (_store, again) = open_project(dir.path()).unwrap();
    assert!(again.applied.is_empty(), "reopening applies nothing");
    copy_is_as_it_was("after a reopen");
}

/// Row 25 (owner Q13 (a)): 0013 adds the cost columns to `ledger` and
/// `settled_at` to `model_calls`, NULL on every old row (no backfill); the
/// readers read an old row's company amount, terms, week and team as
/// unknown and its USD amounts as they were. An old open call still
/// settles once migrated (resume's `MarkLost`, 3.9): at its USD
/// reservation, its company amount still unknown, `settled_at` stamped.
#[test]
fn migration_0013_adds_the_cost_columns_null_on_old_rows() {
    let dir = tempfile::tempdir().unwrap();
    let path = database_at_12(dir.path());
    rows_at_12(&path);

    let (store, _) = open_project(dir.path()).unwrap();

    let have = columns(&path, "ledger");
    for column in LEDGER_COLUMNS {
        assert!(
            have.contains(&column.to_string()),
            "ledger.{column}: {have:?}"
        );
    }
    assert!(
        columns(&path, "model_calls").contains(&"settled_at".to_string()),
        "{:?}",
        columns(&path, "model_calls")
    );
    let c = Connection::open(&path).unwrap();
    for column in LEDGER_COLUMNS {
        let set: i64 = c
            .query_row(
                &format!("SELECT COUNT(*) FROM ledger WHERE {column} IS NOT NULL"),
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(set, 0, "ledger.{column} is NULL on every old row");
    }
    let stamped: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM model_calls WHERE settled_at IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(stamped, 0, "no old call has a settled_at");
    drop(c);

    let reader = store.reader().unwrap();
    let old = reader.ledger_for_run("r1").unwrap();
    assert_eq!(old.len(), 3);
    let spike = old.iter().find(|l| l.model_call_id == "c1").unwrap();
    assert_eq!(
        (
            spike.reserved_micro_usd,
            spike.settled_micro_usd,
            spike.state.as_str()
        ),
        (1500, Some(1200), "settled")
    );
    assert_eq!(spike.rate_snapshot, "spike-2026-11-19");
    for l in &old {
        assert_eq!(l.currency, None, "{l:?}");
        assert_eq!(l.reserved_micro_company, None, "{l:?}");
        assert_eq!(l.settled_micro_company, None, "{l:?}");
        assert_eq!(l.input_micro_usd_per_mtok, None, "{l:?}");
        assert_eq!(l.company_micro_per_usd, None, "{l:?}");
        assert_eq!(l.local, None, "{l:?}");
        assert_eq!(l.week, None, "{l:?}");
        assert_eq!(l.team, None, "{l:?}");
    }
    for c in reader.calls_for_run("r1").unwrap() {
        assert_eq!(c.settled_at, None, "{c:?}");
    }
    let spend = reader.spend().unwrap();
    assert_eq!(spend.len(), 5, "every old call is read");
    assert!(
        spend
            .iter()
            .all(|s| s.currency.is_none() && s.team.is_none())
    );
    assert_eq!(
        reader.document_versions(DOCUMENT).unwrap().len(),
        1,
        "the version as it was"
    );
    drop(reader);

    // The old open call settles once migrated.
    store
        .writer()
        .exec(
            engine(),
            WriteCommand::MarkLost {
                call_id: "c2".into(),
            },
        )
        .unwrap();
    let reader = store.reader().unwrap();
    let lost = reader
        .ledger_for_run("r1")
        .unwrap()
        .into_iter()
        .find(|l| l.model_call_id == "c2")
        .unwrap();
    assert_eq!(
        (
            lost.state.as_str(),
            lost.settled_micro_usd,
            lost.settled_micro_company
        ),
        ("settled", Some(0), None)
    );
    let call = reader
        .calls_for_run("r1")
        .unwrap()
        .into_iter()
        .find(|c| c.id == "c2")
        .unwrap();
    assert_eq!(call.status, "lost");
    assert!(call.settled_at.is_some(), "{call:?}");
}

/// Row 25: a database already at version 13 opens with nothing applied and
/// no new pre-migration copy.
#[test]
fn reopening_a_version_13_database_applies_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = database_at_12(dir.path());
    rows_at_12(&path);
    let (store, first) = open_project(dir.path()).unwrap();
    assert!(
        first
            .applied
            .iter()
            .any(|m| (m.from, m.to) == (BEFORE_COSTS, BEFORE_COSTS + 1)),
        "{first:?}"
    );
    drop(store);
    let before = tables_of(&path);
    let backups = |root: &Path| -> usize { walk_count(&layout(root).dir().join("backups")) };
    let copies = backups(dir.path());

    let (_store, again) = open_project(dir.path()).unwrap();

    assert!(again.applied.is_empty(), "{again:?}");
    assert_eq!(again.version, SCHEMA_VERSION);
    let c = Connection::open(&path).unwrap();
    assert_eq!(user_version(&c), SCHEMA_VERSION);
    drop(c);
    assert_eq!(backups(dir.path()), copies, "no new pre-migration copy");
    let after = tables_of(&path);
    for table in before.keys().filter(|t| *t != "audit_log") {
        assert_eq!(after[table], before[table], "{table}");
    }
}

/// Row 25: the step fails closed and names its backup. A `settled_at`
/// column added to `model_calls` by hand stops 0013; the database is left
/// at version 12 with every row, and the error holds the copy's path.
#[test]
fn a_failing_0013_names_the_backup_and_leaves_version_12() {
    let dir = tempfile::tempdir().unwrap();
    let path = database_at_12(dir.path());
    rows_at_12(&path);
    Connection::open(&path)
        .unwrap()
        .execute_batch("ALTER TABLE model_calls ADD COLUMN settled_at TEXT;")
        .unwrap();
    let before = tables_of(&path);

    let refused = match open_project(dir.path()) {
        Ok((_, report)) => panic!("the forged column must fail 0013 closed: {report:?}"),
        Err(e) => e,
    };

    let words = refused.to_string();
    match &refused {
        StoreError::MigrationFailed {
            from, to, backup, ..
        } => {
            assert_eq!((*from, *to), (BEFORE_COSTS, BEFORE_COSTS + 1));
            let backup = backup.as_ref().expect("the step names its backup");
            assert!(backup.is_file(), "the pre-migration copy is kept");
            assert!(
                words.contains(&backup.display().to_string()),
                "the error names the backup: {words}"
            );
        }
        other => panic!("MigrationFailed expected: {other:?}"),
    }
    let c = Connection::open(&path).unwrap();
    assert_eq!(user_version(&c), BEFORE_COSTS, "the step rolled back");
    drop(c);
    assert_eq!(tables_of(&path), before, "every row as it was");
}
