//! Epic 3 story 7, "Cost settles per call and rolls up" (AD-8, AD-9,
//! AD-16; owner Q1 to Q4, Q6, Q8, Q11, Q14): every call the engine makes is
//! reserved before it is made, from the request's counted prompt tokens,
//! `max_output_tokens` and the plan's price terms, then settled from the
//! provider's usage (or at the reservation) in USD and in the company
//! currency; a local call is 0 with its tokens and time; the week is the
//! run clock's in company time; an edited price file gives the next run a
//! new snapshot; and the cleanup after a store error settles the open call.
//!
//! The API pinned here (tester, 2026-11-25): `StepPlan.price:
//! Option<CallPrice>` (`None` only on a gate; a non-gate step without one
//! is refused before any row); `RunPlan.money: RunMoney { rate, timezone,
//! week_start }`; the ledger and call readers' new fields
//! (`tests/ledger.rs` in am-store).
//!
//! Prices and rates are fake test values: 3.00 and 15.00 USD per million
//! tokens, 0.92 EUR per USD. FakeLlm and FakeClock only; no network.
//! Written before the API exists (tests first, 2026-11-25).
#![forbid(unsafe_code)]

mod support;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use am_config::ModelRef;
use am_domain::money::{CallPrice, Rate};
use am_domain::{
    Actor, BoxFuture, CancelSignal, ChatOutcome, ChatRequest, EventSink, Instruction, LlmProvider,
    ModelCapabilities, ProviderError, StopReason, UntrustedData, Usage,
};
use am_engine::run::{Limits, RunError, RunMoney, RunOutcome, RunPlan, StepPlan, run_plan};
use am_store::{LedgerRow, Reader, WriteCommand, Writer};
use am_testkit::{FakeClock, FakeLlm, ScriptedTurn};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use support::*;
use tokio_util::sync::CancellationToken;

/// The rate snapshot of the plans built here: a price file's sha256 hex.
const SNAPSHOT: &str = "3f9c0a1b2c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7";
/// The price file's `updated:` date.
const UPDATED: &str = "2026-11-01";
/// [`CLOCK`] (2026-11-24T10:00Z) in New York is Tuesday 05:00 EST: the
/// week from Monday 23 November.
const WEEK: &str = "2026-11-23";
/// Free text that must never reach the audit log (AD-25).
const CANARY_PROMPT: &str = "canary-prompt-text-51d8aa";
const CANARY_REPLY: &str = "canary-reply-text-0e6f92";

/// The matrix's cloud terms (fake): 3.00 in, 15.00 out, USD per million.
fn cloud() -> CallPrice {
    CallPrice {
        input_micro_usd_per_mtok: 3_000_000,
        output_micro_usd_per_mtok: 15_000_000,
        local: false,
    }
}

/// A company in EUR (0.92 per USD, fake) in New York, weeks from Monday.
fn eur_money() -> RunMoney {
    RunMoney {
        rate: Rate {
            currency: "EUR".into(),
            company_micro_per_usd: Some(920_000),
            snapshot: SNAPSHOT.into(),
            prices_updated: Some(UPDATED.into()),
        },
        timezone: jiff::tz::TimeZone::get("America/New_York").unwrap(),
        week_start: jiff::civil::Weekday::Monday,
    }
}

/// [`step`] on the cloud terms with the matrix's 4,096 output tokens.
fn priced(key: &str, provider: Arc<dyn LlmProvider>) -> StepPlan {
    StepPlan {
        price: Some(cloud()),
        max_output_tokens: 4_096,
        ..step(key, provider)
    }
}

/// [`plan`] with [`eur_money`].
fn priced_plan(steps: Vec<StepPlan>) -> RunPlan {
    RunPlan {
        money: eur_money(),
        ..plan(steps)
    }
}

async fn run(p: &Project, plan: &RunPlan) -> RunOutcome {
    run_plan(&p.store, owner(), "manual", plan, CancellationToken::new())
        .await
        .expect("the run is journaled")
}

/// The reservation the writer must compute for `counted` prompt tokens and
/// `max` output tokens on [`cloud`] at 0.92: each amount the exact value
/// rounded up (owner Q1, Q5).
fn reservation(counted: i64, max: i64) -> (i64, i64) {
    let exact = counted as i128 * 3_000_000 + max as i128 * 15_000_000;
    let up = |n: i128, d: i128| ((n + d - 1) / d) as i64;
    (up(exact, 1_000_000), up(exact * 920_000, 1_000_000_000_000))
}

/// Every byte a request carries as text: its system parts and every
/// message part (a floor for the count, whatever else it covers).
fn text_bytes(request: &ChatRequest) -> usize {
    request.system.iter().map(|i| i.text().len()).sum::<usize>()
        + request
            .messages
            .iter()
            .flat_map(|m| m.parts.iter())
            .map(|p| part_text(p).len())
            .sum::<usize>()
}

fn millis(ts: &str) -> i64 {
    ts.parse::<jiff::Timestamp>()
        .unwrap_or_else(|e| panic!("{ts}: {e}"))
        .as_millisecond()
}

/// The one entry `action` targeting `target`.
fn entry(j: &Journal, action: &str, target: &str) -> Value {
    let found: Vec<Value> = j
        .entries(action)
        .into_iter()
        .filter(|e| e["target"] == target)
        .collect();
    assert_eq!(found.len(), 1, "one {action} for {target}: {found:#?}");
    found[0].clone()
}

fn the_entry(j: &Journal) -> LedgerRow {
    assert_eq!(j.ledger.len(), 1, "{:#?}", j.ledger);
    j.ledger[0].clone()
}

// ------------------------------------------------------------------ rows 1, 2, 26

/// Rows 1, 2 and 26 (AD-8, AD-9): when the provider is called, the call's
/// reservation is already committed: `reserved`, priced from the request's
/// counted prompt tokens (at least its text bytes / 3) and the step's 4,096
/// output tokens at the plan's terms, in USD and EUR, with the terms, the
/// rate, its snapshot and date, the week (the run clock's, in New York) and
/// the seat's team, and audited `model_call.reserved`. After the call, usage
/// 1,200/90 settles it at 4,950 micro-USD and 4,554 micro-EUR with the
/// tokens and `settled_at`. No prompt or reply text reaches the audit, no
/// egress is audited, and the chain verifies.
#[tokio::test]
async fn each_call_is_reserved_before_it_is_made_then_settled() {
    let p = project();
    let llm = Arc::new(FakeLlm::new(vec![
        ScriptedTurn::text(format!("A line. {CANARY_REPLY}")).usage(1_200, 90),
    ]));
    let spy = Spy::new(&p.db, llm);
    let draft = StepPlan {
        instruction: Instruction::authored(format!("Draft one line. {CANARY_PROMPT}")),
        ..priced("draft", spy.clone())
    };

    let outcome = run(&p, &priced_plan(vec![draft])).await;

    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    let seen = spy.seen();
    assert_eq!(seen.len(), 1, "one call");
    let at_call = &seen[0];
    let l = the_entry(at_call);
    assert_eq!(l.state, "reserved", "reserved before the call");
    assert_eq!((l.settled_micro_usd, l.settled_micro_company), (None, None));
    let counted = l.counted_input_tokens.expect("the counted prompt tokens");
    let floor = text_bytes(&spy.requests()[0]).div_ceil(3) as i64;
    assert!(
        counted >= floor,
        "the count covers every part: {counted} < {floor}"
    );
    let (usd, company) = reservation(counted, 4_096);
    assert_eq!(l.reserved_micro_usd, usd);
    assert_eq!(l.reserved_micro_company, Some(company));
    assert_eq!(l.max_output_tokens, Some(4_096));
    assert_eq!(l.input_micro_usd_per_mtok, Some(3_000_000));
    assert_eq!(l.output_micro_usd_per_mtok, Some(15_000_000));
    assert_eq!(l.company_micro_per_usd, Some(920_000));
    assert_eq!(l.currency.as_deref(), Some("EUR"));
    assert_eq!(l.rate_snapshot, SNAPSHOT);
    assert_eq!(l.prices_updated.as_deref(), Some(UPDATED));
    assert_eq!(l.local, Some(false));
    assert_eq!(l.week.as_deref(), Some(WEEK));
    assert_eq!(l.team.as_deref(), Some(TEAM));
    let call = at_call.calls[0].id.clone();
    let reserved = entry(at_call, "model_call.reserved", &call);
    assert_eq!(
        reserved["args"]["reserved_micro_usd"],
        json!(usd),
        "{reserved}"
    );
    assert_eq!(
        reserved["args"]["reserved_micro_company"],
        json!(company),
        "{reserved}"
    );
    assert_eq!(reserved["args"]["counted_input_tokens"], json!(counted));
    assert!(
        at_call
            .actions()
            .iter()
            .all(|a| !a.starts_with("model_call.settled")),
        "nothing settles before the call: {:?}",
        at_call.actions()
    );

    let j = journal(&p.db);
    let l = the_entry(&j);
    assert_eq!(l.state, "settled");
    assert_eq!(l.settled_micro_usd, Some(4_950));
    assert_eq!(l.settled_micro_company, Some(4_554));
    assert_eq!(l.reserved_micro_usd, usd, "the reservation is as it was");
    let c = &j.calls[0];
    assert_eq!((c.input_tokens, c.output_tokens), (Some(1_200), Some(90)));
    let settled_at = c.settled_at.as_deref().expect("settled_at");
    assert!(millis(settled_at) >= millis(&c.created_at));
    let settled = entry(&j, "model_call.settled+step.finished", &call);
    assert_eq!(
        settled["args"]["settled_micro_usd"],
        json!(4_950),
        "{settled}"
    );
    assert_eq!(
        settled["args"]["settled_micro_company"],
        json!(4_554),
        "{settled}"
    );
    assert_eq!(settled["args"]["at_reservation"], json!(false), "{settled}");
    assert_eq!(
        settled["args"]["over_reservation"],
        json!(false),
        "{settled}"
    );

    let export = serde_json::to_string(&j.audit).unwrap();
    for canary in [CANARY_PROMPT, CANARY_REPLY] {
        assert!(!export.contains(canary), "{canary} is in the audit log");
    }
    assert!(
        j.actions().iter().all(|a| !a.starts_with("egress")),
        "row 29: no egress: {:?}",
        j.actions()
    );
    assert_eq!(
        Reader::open(&p.db).unwrap().verify_audit_chain().unwrap(),
        None
    );
}

// ------------------------------------------------------------------ row 3

/// Row 3 (AD-9): a reply with no usage settles at its reservation in both
/// currencies; the call holds no tokens; the audit says `at_reservation`.
#[tokio::test]
async fn a_call_with_no_usage_settles_at_its_reservation() {
    let p = project();

    let outcome = run(&p, &priced_plan(vec![priced("draft", texts(&["done"]))])).await;

    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    let j = journal(&p.db);
    let l = the_entry(&j);
    assert_eq!(l.state, "settled");
    assert_eq!(l.settled_micro_usd, Some(l.reserved_micro_usd));
    assert_eq!(l.settled_micro_company, l.reserved_micro_company);
    assert!(l.reserved_micro_usd > 61_440, "{l:?}");
    assert_eq!(
        (j.calls[0].input_tokens, j.calls[0].output_tokens),
        (None, None)
    );
    let settled = entry(&j, "model_call.settled+step.finished", &j.calls[0].id);
    assert_eq!(settled["args"]["at_reservation"], json!(true), "{settled}");
}

// ------------------------------------------------------------------ row 5

/// Row 5 (Conventions, Money; owner Q8): a step on this Mac is reserved
/// and settled at 0 in both currencies, `local`, with no rate (the plan's
/// snapshot `none`); its call keeps usage 1,200/90 and its time
/// (`settled_at` minus `created_at`, at least the reply's delay).
#[tokio::test]
async fn a_local_call_costs_nothing_and_keeps_tokens_and_time() {
    let p = project();
    let llm = Arc::new(FakeLlm::new(vec![
        ScriptedTurn::text("checked")
            .usage(1_200, 90)
            .delay(Duration::from_millis(60)),
    ]));

    let outcome = run(&p, &plan(vec![step("check", llm)])).await;

    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    let j = journal(&p.db);
    let l = the_entry(&j);
    assert_eq!(l.local, Some(true));
    assert_eq!(
        (
            l.reserved_micro_usd,
            l.reserved_micro_company,
            l.settled_micro_usd,
            l.settled_micro_company
        ),
        (0, Some(0), Some(0), Some(0))
    );
    assert_eq!(l.currency.as_deref(), Some("EUR"));
    assert_eq!(l.rate_snapshot, "none");
    assert!(l.counted_input_tokens.is_some_and(|c| c > 0), "{l:?}");
    let c = &j.calls[0];
    assert_eq!((c.input_tokens, c.output_tokens), (Some(1_200), Some(90)));
    let took = millis(c.settled_at.as_deref().expect("settled_at")) - millis(&c.created_at);
    assert!(took >= 60, "the call's time: {took} ms");
}

// ------------------------------------------------------------------ row 7

/// A provider that ends its turn cut off at the output limit, with usage.
struct CutOff;

impl LlmProvider for CutOff {
    fn account_id(&self) -> String {
        "cut-off".into()
    }
    fn capabilities(&self, _model: &str) -> ModelCapabilities {
        ModelCapabilities::default()
    }
    fn check_file_parts(&self, _request: &ChatRequest) -> Result<(), ProviderError> {
        Ok(())
    }
    fn stream(
        &self,
        _request: ChatRequest,
        _cancel: Arc<dyn CancelSignal>,
        _sink: Arc<dyn EventSink>,
    ) -> BoxFuture<Result<ChatOutcome, ProviderError>> {
        Box::pin(async {
            Ok(ChatOutcome {
                text: "a reply that runs on and on".into(),
                tool_calls: vec![],
                usage: Some(Usage {
                    input_tokens: 2_000,
                    output_tokens: 4_096,
                }),
                stop: StopReason::MaxTokens,
            })
        })
    }
}

/// Row 7 (AD-9 literal; owner Q3): a failed call settles from its usage
/// when the provider reported one, else at its reservation, and the step
/// fails as today: a provider error (no usage: at the reservation), a
/// reply cut off at `max_tokens` with usage 2,000/4,096 (67,440 micro-USD,
/// 62,045 micro-EUR) and a call cancelled at the wall-clock cap (at the
/// reservation).
#[tokio::test]
async fn a_failed_call_settles_at_its_reservation() {
    // A provider error.
    let p = project();
    let failing = Arc::new(FakeLlm::new(vec![ScriptedTurn::fail(
        ProviderError::Http {
            status: 500,
            body: "the provider is down".into(),
        },
    )]));
    let outcome = run(&p, &priced_plan(vec![priced("draft", failing)])).await;
    assert_eq!(outcome.status, "failed", "{outcome:?}");
    let j = journal(&p.db);
    let l = the_entry(&j);
    assert_eq!(j.steps[0].status, "failed");
    assert_eq!(l.state, "settled");
    assert_eq!(l.settled_micro_usd, Some(l.reserved_micro_usd));
    assert_eq!(l.settled_micro_company, l.reserved_micro_company);
    assert_eq!(j.calls[0].input_tokens, None);

    // Cut off at the output limit, with usage.
    let p = project();
    let outcome = run(&p, &priced_plan(vec![priced("draft", Arc::new(CutOff))])).await;
    assert_eq!(outcome.status, "failed", "{outcome:?}");
    let j = journal(&p.db);
    let l = the_entry(&j);
    assert_eq!(j.steps[0].status, "failed");
    assert_eq!(l.settled_micro_usd, Some(67_440));
    assert_eq!(l.settled_micro_company, Some(62_045));
    assert_eq!(
        (j.calls[0].input_tokens, j.calls[0].output_tokens),
        (Some(2_000), Some(4_096))
    );

    // Cancelled at the wall-clock cap.
    let p = project();
    let hanging = Arc::new(FakeLlm::new(vec![ScriptedTurn::hang()]));
    let capped = StepPlan {
        limits: Limits {
            wall_clock: Duration::from_millis(100),
            ..Limits::default()
        },
        ..priced("draft", hanging)
    };
    let outcome = run(&p, &priced_plan(vec![capped])).await;
    assert_eq!(outcome.status, "failed", "{outcome:?}");
    let j = journal(&p.db);
    let l = the_entry(&j);
    assert_eq!(j.steps[0].status, "failed");
    assert_eq!(l.state, "settled");
    assert_eq!(l.settled_micro_usd, Some(l.reserved_micro_usd));
    assert_eq!(l.settled_micro_company, l.reserved_micro_company);
}

// ------------------------------------------------------------------ row 8

/// Row 8 (AD-36; story 3.5): the repair call has its own reservation,
/// counted from its own request (which holds the invalid reply and the
/// repair instruction besides the first request), and its own settlement;
/// both roll up under the same attempt.
#[tokio::test]
async fn a_repair_call_is_reserved_from_its_own_request() {
    let p = project();
    let invalid = "not json at all";
    let llm = Arc::new(FakeLlm::new(vec![
        ScriptedTurn::text(invalid).usage(500, 20),
        ScriptedTurn::text(r#"{"line":"ok"}"#).usage(800, 10),
    ]));
    let spy = Spy::new(&p.db, llm);

    let outcome = run(
        &p,
        &priced_plan(vec![with_schema(
            priced("draft", spy.clone()),
            line_schema(),
        )]),
    )
    .await;

    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    let j = journal(&p.db);
    assert_eq!(j.calls.len(), 2);
    assert_eq!(
        j.calls[0].step_run_id, j.calls[1].step_run_id,
        "one attempt"
    );
    assert_eq!(j.ledger.len(), 2, "a reservation each");
    let entry_of = |call: &str| -> LedgerRow {
        j.ledger
            .iter()
            .find(|l| l.model_call_id == call)
            .cloned()
            .unwrap()
    };
    let (turn, repair) = (entry_of(&j.calls[0].id), entry_of(&j.calls[1].id));
    let (c1, c2) = (
        turn.counted_input_tokens.unwrap(),
        repair.counted_input_tokens.unwrap(),
    );
    let more = ((invalid.len() + REPAIR.len()) / 3) as i64;
    assert!(
        c2 - c1 >= more,
        "the repair's own request is counted: {c1} then {c2}, at least {more} more"
    );
    assert_eq!(
        (repair.reserved_micro_usd, repair.reserved_micro_company),
        {
            let (u, c) = reservation(c2, 4_096);
            (u, Some(c))
        }
    );
    // Each settles from its own usage: 500/20 and 800/10.
    assert_eq!(
        (turn.settled_micro_usd, turn.settled_micro_company),
        (Some(1_800), Some(1_656))
    );
    assert_eq!(
        (repair.settled_micro_usd, repair.settled_micro_company),
        (Some(2_550), Some(2_346))
    );
    // At the repair call, the turn was settled and the repair reserved.
    let at_repair = &spy.seen()[1];
    let states: Vec<&str> = at_repair.ledger.iter().map(|l| l.state.as_str()).collect();
    assert_eq!(states, vec!["settled", "reserved"]);
}

// ------------------------------------------------------------------ rows 17, 18

/// A provider that moves the run's clock to `then` when it is called, then
/// plays its model's turn.
struct MovesTheClock {
    clock: Arc<FakeClock>,
    then: &'static str,
    llm: Arc<FakeLlm>,
}

impl LlmProvider for MovesTheClock {
    fn account_id(&self) -> String {
        "moves-the-clock".into()
    }
    fn capabilities(&self, model: &str) -> ModelCapabilities {
        self.llm.capabilities(model)
    }
    fn check_file_parts(&self, request: &ChatRequest) -> Result<(), ProviderError> {
        self.llm.check_file_parts(request)
    }
    fn stream(
        &self,
        request: ChatRequest,
        cancel: Arc<dyn CancelSignal>,
        sink: Arc<dyn EventSink>,
    ) -> BoxFuture<Result<ChatOutcome, ProviderError>> {
        self.clock.set(self.then);
        self.llm.stream(request, cancel, sink)
    }
}

/// Rows 17 and 18 (owner Q11): each reservation's week is the date its
/// company-local week starts, at the instant the run's clock gives when it
/// is reserved (never the writer's own clock): across New York's DST
/// changes, and at Kolkata's year boundary. Two calls of one run either side
/// of Monday 00:00 fall in two weeks.
#[tokio::test]
async fn the_week_is_the_run_clocks_in_company_time() {
    for (zone, instant, week) in [
        ("America/New_York", "2026-11-02T04:30:00Z", "2026-10-26"),
        ("America/New_York", "2026-11-02T05:00:00Z", "2026-11-02"),
        ("America/New_York", "2026-03-09T03:30:00Z", "2026-03-02"),
        ("America/New_York", "2026-03-09T04:30:00Z", "2026-03-09"),
        ("Asia/Kolkata", "2026-12-31T19:00:00Z", "2026-12-28"),
        ("Asia/Kolkata", "2027-01-03T18:30:00Z", "2027-01-04"),
    ] {
        let p = project();
        let plan = RunPlan {
            clock: FakeClock::at(instant),
            money: RunMoney {
                timezone: jiff::tz::TimeZone::get(zone).unwrap(),
                ..eur_money()
            },
            ..priced_plan(vec![priced("draft", texts(&["done"]))])
        };
        let outcome = run(&p, &plan).await;
        assert_eq!(outcome.status, "succeeded", "{zone} {instant}: {outcome:?}");
        let l = the_entry(&journal(&p.db));
        assert_eq!(l.week.as_deref(), Some(week), "{zone} {instant}");
    }

    // The clock moves past Monday 00:00 EST between the two calls.
    let p = project();
    let clock = FakeClock::at("2026-11-02T04:30:00Z");
    let mover = Arc::new(MovesTheClock {
        clock: clock.clone(),
        then: "2026-11-02T05:00:00Z",
        llm: texts(&["first"]),
    });
    let plan = RunPlan {
        clock,
        ..priced_plan(vec![
            priced("draft", mover),
            priced("check", texts(&["second"])),
        ])
    };
    let outcome = run(&p, &plan).await;
    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    let j = journal(&p.db);
    let weeks: Vec<Option<String>> = j
        .calls
        .iter()
        .map(|c| {
            j.ledger
                .iter()
                .find(|l| l.model_call_id == c.id)
                .unwrap()
                .week
                .clone()
        })
        .collect();
    assert_eq!(
        weeks,
        vec![
            Some("2026-10-26".to_string()),
            Some("2026-11-02".to_string())
        ]
    );
}

// ------------------------------------------------------------------ row 11

/// `fixtures/accounts/costs.yaml` as this Mac's Accounts file (owner-only).
fn with_costs_accounts(cli: &Cli) {
    use std::os::unix::fs::PermissionsExt;
    let dir = cli.places.user.dir().to_path_buf();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = cli.places.user.accounts_file();
    let bytes = std::fs::read(am_testkit::fixture("accounts/costs.yaml")).unwrap();
    std::fs::write(&path, bytes).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
}

/// The costs fixture's models: the cloud Account's on `cloud`, this Mac's
/// on `local`.
fn by_account(cloud: Arc<FakeLlm>, local: Arc<FakeLlm>) -> Providers {
    Arc::new(move |m: &ModelRef| match m.account.as_str() {
        "cloud" => Ok(cloud.clone() as Arc<dyn LlmProvider>),
        "local-lmstudio" => Ok(local.clone() as Arc<dyn LlmProvider>),
        other => Err(format!("the test has no provider for Account {other}")),
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Row 11 (AD-16; owner Q6): `agentic run` reads the prices and the rate
/// once, from the snapshot the run pins, and journals the price file's
/// sha256 as each entry's snapshot id; editing the file between two runs
/// gives the second run's entries the new id, terms and date, and leaves
/// the first run's as they were. `agentic status` names both dates and
/// both snapshots.
#[tokio::test]
async fn an_edited_price_file_gives_the_next_run_a_new_snapshot() {
    let cli = cli();
    with_costs_accounts(&cli);
    let root = cli.copy_of("costs");
    let prices = root.join("models/prices.yaml");
    let providers = || {
        by_account(
            Arc::new(FakeLlm::new(vec![
                ScriptedTurn::text("A line.").usage(1_200, 90),
            ])),
            Arc::new(FakeLlm::new(vec![
                ScriptedTurn::text("Fine.").usage(900, 70),
            ])),
        )
    };

    let first = cli.run(&root, "pair", providers()).await;
    assert_eq!(first.code, 0, "{}", first.all());
    let old = sha256_hex(&std::fs::read(&prices).unwrap());
    let text = std::fs::read_to_string(&prices).unwrap();
    let edited = text
        .replace("updated: 2026-11-01", "updated: 2026-11-20")
        .replace("input_usd_per_mtok: 3.0", "input_usd_per_mtok: 4.0");
    assert_ne!(edited, text);
    std::fs::write(&prices, &edited).unwrap();
    let new = sha256_hex(edited.as_bytes());
    let second = cli.run(&root, "pair", providers()).await;
    assert_eq!(second.code, 0, "{}", second.all());

    let reader = Reader::open(&cli.db(&root)).unwrap();
    let cloud_entry = |run: &str| -> LedgerRow {
        reader
            .ledger_for_run(run)
            .unwrap()
            .into_iter()
            .find(|l| l.local == Some(false))
            .unwrap_or_else(|| panic!("no cloud entry in run {run}"))
    };
    let (run1, run2) = (first.run_line().0, second.run_line().0);
    let a = cloud_entry(&run1);
    assert_eq!(a.rate_snapshot, old);
    assert_eq!(a.input_micro_usd_per_mtok, Some(3_000_000));
    assert_eq!(a.prices_updated.as_deref(), Some("2026-11-01"));
    assert_eq!(a.settled_micro_usd, Some(4_950));
    let b = cloud_entry(&run2);
    assert_eq!(b.rate_snapshot, new);
    assert_eq!(b.input_micro_usd_per_mtok, Some(4_000_000));
    assert_eq!(b.prices_updated.as_deref(), Some("2026-11-20"));
    assert_eq!(b.settled_micro_usd, Some(6_150), "1,200 x 4 + 90 x 15");
    for run in [&run1, &run2] {
        for l in reader.ledger_for_run(run).unwrap() {
            assert_eq!(
                &l.rate_snapshot,
                if run == &run1 { &old } else { &new },
                "every entry of a run has its run's snapshot: {l:?}"
            );
        }
    }

    let status = cli.status(&root);
    assert_eq!(status.code, 0, "{}", status.all());
    for words in ["2026-11-01", "2026-11-20", &old[..12], &new[..12]] {
        assert!(
            status.out.contains(words),
            "status names {words}: {}",
            status.out
        );
    }
}

// ------------------------------------------------------------------ row 16

/// The counted prompt tokens and USD reservation of one run of `step`.
async fn counted_for(step: StepPlan) -> (i64, i64) {
    let p = project();
    let outcome = run(&p, &priced_plan(vec![step])).await;
    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    let l = the_entry(&journal(&p.db));
    (l.counted_input_tokens.unwrap(), l.reserved_micro_usd)
}

/// Row 16 (owner Q2): prompt tokens are counted as UTF-8 bytes / 3,
/// rounded up, over every system part, every message part and each offered
/// tool's definition. A 3,000-byte longer instruction, or 1,000 three-byte
/// characters, counts exactly 1,000 more tokens and reserves 3,000
/// micro-USD more at 3.00 per million; so does a 3,000-byte longer system
/// part; a 3,000-byte plan input counts at least 1,000 more; an offered
/// tool counts more.
#[tokio::test]
async fn a_longer_prompt_reserves_more_by_the_rule() {
    let base = priced("draft", texts(&["done"]));
    let (c0, r0) = counted_for(base.clone()).await;

    let longer = StepPlan {
        instruction: Instruction::authored(format!(
            "{}{}",
            instruction_of("draft"),
            "x".repeat(3_000)
        )),
        ..base.clone()
    };
    assert_eq!(counted_for(longer).await, (c0 + 1_000, r0 + 3_000));

    let wide = StepPlan {
        instruction: Instruction::authored(format!(
            "{}{}",
            instruction_of("draft"),
            "\u{20ac}".repeat(1_000)
        )),
        ..base.clone()
    };
    assert_eq!(
        counted_for(wide).await,
        (c0 + 1_000, r0 + 3_000),
        "bytes, not characters"
    );

    let system = StepPlan {
        system: vec![Instruction::authored(format!(
            "You are the seat of step draft.{}",
            "y".repeat(3_000)
        ))],
        ..base.clone()
    };
    assert_eq!(counted_for(system).await, (c0 + 1_000, r0 + 3_000));

    let input = StepPlan {
        inputs: vec![UntrustedData::text("test:page", "z".repeat(3_000))],
        ..base.clone()
    };
    let (ci, _) = counted_for(input).await;
    assert!(ci >= c0 + 1_000, "a plan input counts: {c0} then {ci}");

    let (ct, rt) = counted_for(writing(base)).await;
    assert!(ct > c0 && rt > r0, "an offered tool counts: {c0} then {ct}");
}

// ------------------------------------------------------------------ owner Q14

/// A provider that, while called, finishes its own step `failed` through
/// the writer, so the engine's settle-and-finish is refused by the store
/// with the call still open; then it replies with usage.
struct FinishesItsOwnStep {
    db: PathBuf,
    writer: Writer,
}

impl LlmProvider for FinishesItsOwnStep {
    fn account_id(&self) -> String {
        "finishes-its-own-step".into()
    }
    fn capabilities(&self, _model: &str) -> ModelCapabilities {
        ModelCapabilities::default()
    }
    fn check_file_parts(&self, _request: &ChatRequest) -> Result<(), ProviderError> {
        Ok(())
    }
    fn stream(
        &self,
        _request: ChatRequest,
        _cancel: Arc<dyn CancelSignal>,
        _sink: Arc<dyn EventSink>,
    ) -> BoxFuture<Result<ChatOutcome, ProviderError>> {
        let running = journal(&self.db)
            .steps
            .into_iter()
            .find(|s| s.status == "running")
            .expect("the step is running");
        self.writer
            .exec(
                Actor::System {
                    component: "test".into(),
                },
                WriteCommand::FinishStep {
                    step_run_id: running.id,
                    status: "failed".into(),
                    output: None,
                },
            )
            .unwrap();
        Box::pin(async {
            Ok(ChatOutcome {
                text: "done".into(),
                tool_calls: vec![],
                usage: Some(Usage {
                    input_tokens: 1_200,
                    output_tokens: 90,
                }),
                stop: StopReason::EndTurn,
            })
        })
    }
}

/// Owner Q14 (DR-109, folded in): after a store error, the engine's
/// cleanup also marks the open call lost, once: it settles at its
/// reservation in both currencies with `settled_at`, so no reservation is
/// left open for ever; the run is named as stopped.
#[tokio::test]
async fn the_store_error_cleanup_settles_the_open_call() {
    let p = project();
    let saboteur = Arc::new(FinishesItsOwnStep {
        db: p.db.clone(),
        writer: p.store.writer(),
    });

    let stopped = run_plan(
        &p.store,
        owner(),
        "manual",
        &priced_plan(vec![priced("draft", saboteur)]),
        CancellationToken::new(),
    )
    .await
    .expect_err("the store refused the settlement");

    assert!(matches!(stopped, RunError::Stopped { .. }), "{stopped:?}");
    let j = journal(&p.db);
    assert_eq!(j.calls.len(), 1);
    let c = &j.calls[0];
    assert_eq!((c.status.as_str(), c.settled), ("lost", true), "{c:?}");
    assert!(c.settled_at.is_some(), "{c:?}");
    let l = the_entry(&j);
    assert_eq!(l.state, "settled");
    assert_eq!(l.settled_micro_usd, Some(l.reserved_micro_usd));
    assert_eq!(l.settled_micro_company, l.reserved_micro_company);
    assert_eq!(
        j.entries("model_call.lost").len(),
        1,
        "once: {:?}",
        j.actions()
    );
    assert_eq!(j.runs[0].status, "failed");
}

// ------------------------------------------------------------------ Code Map: run_on

/// Code Map (`run_on`): a step that calls a model and has no price is
/// refused before any row is written, naming the step; a gate makes no
/// call and needs none.
#[tokio::test]
async fn a_step_without_a_price_is_refused_before_any_row() {
    let p = project();
    let unpriced = StepPlan {
        price: None,
        ..step("draft", never())
    };

    let refused = run_plan(
        &p.store,
        owner(),
        "manual",
        &plan(vec![unpriced]),
        CancellationToken::new(),
    )
    .await;

    let err = refused.expect_err("an unpriced step never runs");
    assert!(err.to_string().contains("draft"), "names the step: {err}");
    let j = journal(&p.db);
    assert!(j.runs.is_empty() && j.ledger.is_empty(), "{:#?}", j.runs);

    // A gate with no price runs (it waits for a person).
    let p = project();
    let outcome = run(
        &p,
        &plan(vec![
            step("draft", texts(&["a version"])),
            StepPlan {
                price: None,
                ..gate("approve", never())
            },
        ]),
    )
    .await;
    assert_eq!(outcome.status, "waiting_human", "{outcome:?}");
}

// ------------------------------------------------------------------ row 29

/// Row 29 (no cap is checked, epic 4 story 2): a reservation is never
/// refused for its size: 2,000,000 output tokens at 15.00 per million
/// reserves over 30 USD and the step runs; no Budget ask is raised.
#[tokio::test]
async fn a_large_reservation_is_never_refused() {
    let p = project();
    let large = StepPlan {
        max_output_tokens: 2_000_000,
        ..priced("draft", texts(&["done"]))
    };

    let outcome = run(&p, &priced_plan(vec![large])).await;

    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    let j = journal(&p.db);
    let l = the_entry(&j);
    assert!(l.reserved_micro_usd >= 30_000_000, "{l:?}");
    assert!(j.items.is_empty(), "no Budget ask: {:#?}", j.items);
}

// ------------------------------------------------------------------ row 23

/// A run of the costs fixture's `pair` written through the store's own
/// commands (no CLI): `draft`, of seat `writer`, reserves one call on the
/// cloud terms at `rate` in the week from 2026-11-23 (team `core`), settled
/// from `usage`.
fn written_run(db: &std::path::Path, rate: Rate, usage: Usage) -> String {
    let store = am_store::Store::open(db).unwrap();
    let w = store.writer();
    let engine = Actor::System {
        component: "engine".into(),
    };
    let run_id = am_store::new_id();
    w.exec(
        engine.clone(),
        WriteCommand::CreateRun {
            run_id: run_id.clone(),
            workflow: "pair".into(),
            origin: "manual".into(),
            steps: vec!["draft".into(), "check".into()],
            config_hash: None,
        },
    )
    .unwrap();
    let draft = store
        .reader()
        .unwrap()
        .steps(&run_id)
        .unwrap()
        .into_iter()
        .find(|s| s.step_key == "draft")
        .unwrap();
    w.exec(
        engine.clone(),
        WriteCommand::StartAttempt {
            step_run_id: draft.id.clone(),
            inputs: vec![],
            taint: am_domain::Taint::Clean,
            prompt: vec![],
            writes: am_store::AttemptWrites {
                seat: "writer".into(),
                table: None,
                document: None,
            },
        },
    )
    .unwrap();
    let call = am_store::new_id();
    w.exec(
        engine.clone(),
        WriteCommand::ReserveCall {
            step_run_id: draft.id,
            call_id: call.clone(),
            call_seq: 1,
            price: cloud(),
            rate,
            counted_input_tokens: 1_000,
            max_output_tokens: 4_096,
            week: WEEK.into(),
            team: TEAM.into(),
            taint: am_domain::Taint::Clean,
            purpose: am_store::CallPurpose::Turn,
            prompt: vec![],
        },
    )
    .unwrap();
    w.exec(
        engine,
        WriteCommand::SettleCall {
            call_id: call,
            usage: Some(usage),
        },
    )
    .unwrap();
    run_id
}

/// Row 23 (owner Q12, Q13 (a)): a ledger row from before 0013 has no
/// company amount: `agentic status` shows its run with `-` for it beside
/// its USD amount, and the footnote; rows in two company currencies (EUR
/// from `agentic run`, then INR after a currency change, fake rate 80 per
/// USD) are summed per currency (`EUR … + INR …`), never converted, and
/// USD sums them all. Here, not in am-app's `status_spend.rs`: it writes
/// the pre-0013 rows into a version-12 database by hand, and am-app's tests
/// have no SQLite library (tester, 2026-11-25).
#[tokio::test]
async fn status_shows_old_rows_and_mixed_currencies_honestly() {
    let cli = cli();
    with_costs_accounts(&cli);
    let root = cli.copy_of("costs");
    cli.open_once(&root);
    let db = cli.db(&root);
    std::fs::remove_file(&db).unwrap();
    am_store::testing::database_at_version(&db, 12).unwrap();
    let old = "0193f1a2-0000-7000-8000-0000000000aa";
    let sql = r#"
        INSERT INTO runs (id, workflow, origin, status, created_at, updated_at, config_hash, finished_at)
          VALUES ('@RUN@', 'pair', 'manual', 'succeeded', '2026-11-20T10:00:00.000Z', '2026-11-20T10:00:05.000Z', NULL, '2026-11-20T10:00:05.000Z');
        INSERT INTO step_runs (id, run_id, step_key, seq, attempt, status, output, taint, updated_at, started_at, finished_at)
          VALUES ('old-draft', '@RUN@', 'draft', 0, 1, 'queued', NULL, 0, '2026-11-20T10:00:00.000Z', NULL, NULL);
        UPDATE step_runs SET status = 'running', seat = 'writer', started_at = '2026-11-20T10:00:01.000Z',
          updated_at = '2026-11-20T10:00:01.000Z' WHERE id = 'old-draft';
        INSERT INTO model_calls (id, step_run_id, call_seq, status, settled, input_tokens, output_tokens, created_at, purpose)
          VALUES ('old-call', 'old-draft', 1, 'settled', 1, 300, 40, '2026-11-20T10:00:02.000Z', 'turn');
        INSERT INTO ledger (id, model_call_id, run_id, kind, reserved_micro_usd, settled_micro_usd, state, rate_snapshot, created_at)
          VALUES ('old-entry', 'old-call', '@RUN@', 'model', 1500, 1200, 'settled', 'none', '2026-11-20T10:00:02.000Z');
        UPDATE step_runs SET status = 'succeeded', output = 'an old line', finished_at = '2026-11-20T10:00:04.000Z',
          updated_at = '2026-11-20T10:00:04.000Z' WHERE id = 'old-draft';
    "#
    .replace("@RUN@", old);
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute_batch(&sql)
        .unwrap();
    let providers = by_account(
        Arc::new(FakeLlm::new(vec![
            ScriptedTurn::text("A line.").usage(1_200, 90),
        ])),
        Arc::new(FakeLlm::new(vec![
            ScriptedTurn::text("Fine.").usage(900, 70),
        ])),
    );

    // `agentic run` migrates the database (12 to 13) and runs in EUR.
    let ran = cli.run(&root, "pair", providers).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    // Usage 1,000/100 on the cloud terms: 4,500 micro-USD, 360,000
    // micro-INR at 80 per USD.
    let inr = Rate {
        currency: "INR".into(),
        company_micro_per_usd: Some(80_000_000),
        snapshot: sha256_hex(b"another price file"),
        prices_updated: Some("2026-11-15".into()),
    };
    written_run(
        &cli.db(&root),
        inr,
        Usage {
            input_tokens: 1_000,
            output_tokens: 100,
        },
    );

    let st = cli.status(&root);

    assert_eq!(st.code, 0, "{}", st.all());
    let old_line = st
        .out
        .lines()
        .find(|l| l.starts_with(old))
        .unwrap_or_else(|| panic!("the old run is listed: {}", st.out));
    assert!(
        old_line.contains("- · USD 0.0012"),
        "an unknown company amount is `-`, never 0: {old_line}"
    );
    assert!(old_line.contains("300/40 tokens"), "{old_line}");
    assert!(
        st.out
            .contains("recorded before company-currency settlement"),
        "the footnote: {}",
        st.out
    );
    let week: Vec<&str> = st
        .out
        .lines()
        .filter(|l| {
            l.trim_start()
                .starts_with("week 2026-W48 (from 2026-11-23)")
        })
        .collect();
    assert_eq!(week.len(), 1, "{}", st.out);
    assert!(
        week[0].contains("EUR 0.004554 + INR 0.36") || week[0].contains("INR 0.36 + EUR 0.004554"),
        "one subtotal per currency: {}",
        week[0]
    );
    assert!(
        week[0].contains("USD 0.00945"),
        "USD sums every row: {}",
        week[0]
    );
    assert!(
        !st.out.contains("0.364554"),
        "never converted or added across currencies: {}",
        st.out
    );
}

// ------------------------------------------------------------------ review round 1

/// [`priced`]'s step in a plan whose company currency (EUR) has no rate:
/// the money of a run with no `rates:` entry.
fn unrated_plan(steps: Vec<StepPlan>) -> RunPlan {
    RunPlan {
        money: RunMoney {
            rate: am_domain::money::Rate::none("EUR"),
            ..eur_money()
        },
        ..plan(steps)
    }
}

/// Review round 1, M6 (owner Q6, row 14): the engine refuses, before any
/// row, a plan whose step makes a priced call when the plan's rate is
/// `None` (no `rates:` entry for the company currency); the words name the
/// step. A local step needs no rate (the control).
#[tokio::test]
async fn a_priced_step_with_no_rate_is_refused_before_any_row() {
    let p = project();
    let llm = texts(&["never"]);

    let refused = run_plan(
        &p.store,
        owner(),
        "manual",
        &unrated_plan(vec![priced("draft", llm.clone())]),
        CancellationToken::new(),
    )
    .await;

    let err = refused.expect_err("a priced call with no rate never runs");
    assert!(err.to_string().contains("draft"), "names the step: {err}");
    let j = journal(&p.db);
    assert!(j.runs.is_empty() && j.ledger.is_empty(), "{:#?}", j.runs);
    assert_eq!(llm.calls(), 0);

    let p = project();
    let outcome = run(&p, &unrated_plan(vec![step("draft", texts(&["done"]))])).await;
    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
}

/// A run of `draft` (this Mac), the gate `approve`, then `publish`, which
/// waits at its gate with the Review approved by a person: the run's id.
async fn approved_at_the_gate(p: &Project) -> String {
    let outcome = run(
        p,
        &plan(vec![
            step("draft", texts(&["a version"])),
            gate("approve", never()),
            step("publish", texts(&["published"])),
        ]),
    )
    .await;
    assert_eq!(outcome.status, "waiting_human", "{outcome:?}");
    let item = outcome.item.clone().expect("the Review item");
    p.store
        .writer()
        .resolve_inbox_item(
            am_domain::HumanActor::new("owner", am_domain::REVIEWER_HAT),
            &item,
            am_domain::Decision::Approve,
            None,
        )
        .unwrap();
    outcome.run_id
}

/// Review round 1, M9 (story 3.4's `continue_run`; AD-9, AD-16):
/// `continue_run` checks the plan's prices before it applies the person's
/// decision: a step still to run with no price, or a priced one with no
/// rate (M6), is refused and nothing is written (the gate attempt is not
/// finished, `publish` stays queued, no audit entry is added). The same
/// run then continues on its priced plan (the control).
#[tokio::test]
async fn continue_run_refuses_an_unpriced_or_unrated_step_before_applying_the_decision() {
    let p = project();
    let run_id = approved_at_the_gate(&p).await;
    let before = journal(&p.db);
    let unpriced = plan(vec![
        step("draft", never()),
        gate("approve", never()),
        StepPlan {
            price: None,
            ..step("publish", never())
        },
    ]);
    let unrated = unrated_plan(vec![
        step("draft", never()),
        gate("approve", never()),
        priced("publish", never()),
    ]);

    for (case, plan) in [("no price", &unpriced), ("no rate", &unrated)] {
        let refused =
            am_engine::run::continue_run(&p.store, &run_id, plan, CancellationToken::new()).await;
        assert!(refused.is_err(), "{case}: {refused:?}");
        let now = journal(&p.db);
        assert_eq!(now.shape(), before.shape(), "{case}: nothing applied");
        assert_eq!(
            now.audit.len(),
            before.audit.len(),
            "{case}: nothing audited"
        );
        assert_eq!(
            now.ledger.len(),
            before.ledger.len(),
            "{case}: nothing reserved"
        );
    }

    let outcome = am_engine::run::continue_run(
        &p.store,
        &run_id,
        &plan(vec![
            step("draft", never()),
            gate("approve", never()),
            step("publish", texts(&["published"])),
        ]),
        CancellationToken::new(),
    )
    .await
    .expect("the priced plan continues");
    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
}
