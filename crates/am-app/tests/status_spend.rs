//! Epic 3 story 7, "Cost settles per call and rolls up" (AD-9, AD-16;
//! owner Q6, Q8, Q10, Q12): `agentic run` prices each step from the
//! snapshot it pins (an Account's provider decides local or a price from
//! `models/prices.yaml`, and the rate comes from its `rates:`), refusing an
//! unpriced cloud model or a missing rate before any row; `agentic status`
//! shows the spend of each run and attempt, then a spend section by week,
//! team and seat, headed by the price file's date and snapshot.
//!
//! The format pinned here is owner Q12's (tester, 2026-11-25): each amount
//! is its ISO code and the amount in units with up to six decimals,
//! trailing zeros trimmed (`0` when zero), the company currency first, then
//! USD, joined by ` · ` with the tokens (`in/out tokens`, grouped with
//! commas), the call count on an attempt line, the time on a spend line,
//! and `local` for calls on this Mac. No currency symbol and no locale
//! grouping of amounts.
//!
//! On stream copies of `fixtures/projects/costs` (EUR, America/New_York,
//! teams `core` and `side`; seat `writer` of `core` on the cloud Account
//! `cloud`, seat `checker` of `side` on `local-lmstudio`; workflow `pair`:
//! `draft` then `check`; fake prices 3.00/15.00 USD per million and 0.92
//! EUR per USD, dated 2026-11-01), with `fixtures/accounts/costs.yaml` as
//! this Mac's Accounts. FakeLlm and FakeClock only; no network. Written
//! before the API exists (tests first, 2026-11-25).
//!
//! Row 23's test (`status_shows_old_rows_and_mixed_currencies_honestly`)
//! is in am-e2e's `tests/engine/cost_ledger.rs`: it writes pre-0013 rows
//! into a version-12 database by hand, and am-app's tests have no SQLite
//! library (tester, 2026-11-25).
#![forbid(unsafe_code)]
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use am_app::cli::{Providers, RunContext, run, status};
use am_app::runtime::Places;
use am_config::ModelRef;
use am_domain::money::{CallPrice, Rate};
use am_domain::{Actor, LlmProvider, RuntimeLayout, Taint, Usage};
use am_store::{AttemptWrites, CallPurpose, LedgerRow, Reader, Store, WriteCommand, new_id};
use am_testkit::{FakeLlm, FakeProjectIdentity, FakeTerminal, ScriptedTurn};
use sha2::{Digest, Sha256};

/// The week from Monday 23 November 2026 in New York (Tuesday 10:00 EST).
const IN_W48: &str = "2026-11-24T15:00:00Z";
/// The week from Monday 30 November 2026 (Tuesday 10:00 EST).
const IN_W49: &str = "2026-12-01T15:00:00Z";
const PRICES: &str = "models/prices.yaml";
/// What a refused `run` did not do (am-app's words).
const NOT_STARTED: &str = "No run was started";

struct Env {
    dir: tempfile::TempDir,
    places: Places,
}

struct Ran {
    code: i32,
    out: String,
    err: String,
}

impl Ran {
    fn all(&self) -> String {
        format!("{}\n{}", self.out, self.err)
    }

    /// The id of the `run <id> <status>` line.
    fn run_id(&self) -> String {
        let line = self
            .out
            .lines()
            .find(|l| l.starts_with("run "))
            .unwrap_or_else(|| panic!("no `run <id> <status>` line: {}", self.all()));
        line.split_whitespace().nth(1).unwrap().to_string()
    }
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let places = Places::under(&home.join("Library/Application Support"), &home);
    let env = Env { dir, places };
    env.with_accounts();
    env
}

fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

impl Env {
    /// `fixtures/accounts/costs.yaml` as this Mac's Accounts file.
    fn with_accounts(&self) {
        let dir = self.places.user.dir().to_path_buf();
        std::fs::create_dir_all(&dir).unwrap();
        set_mode(&dir, 0o700);
        let path = self.places.user.accounts_file();
        std::fs::write(
            &path,
            std::fs::read(am_testkit::fixture("accounts/costs.yaml")).unwrap(),
        )
        .unwrap();
        set_mode(&path, 0o600);
    }

    /// A stream copy of `fixtures/projects/costs`, never opened.
    fn fixture(&self) -> PathBuf {
        let root = self.dir.path().join("projects").join("costs");
        am_testkit::copy_tree(&am_testkit::fixture("projects/costs"), &root);
        root.canonicalize().unwrap()
    }

    fn layout(&self, root: &Path) -> Option<RuntimeLayout> {
        let id = am_config::read_company(root).ok()?.project_id?;
        Some(self.places.base.project(&id))
    }

    fn db(&self, root: &Path) -> PathBuf {
        self.layout(root).expect("opened once").state_db()
    }

    fn reader(&self, root: &Path) -> Reader {
        Reader::open(&self.db(root)).unwrap()
    }

    /// `agentic run pair` at the clock `at`, the cloud seat's model on
    /// `cloud` and this Mac's on `local`.
    async fn run_pair(
        &self,
        root: &Path,
        at: &str,
        cloud: Arc<FakeLlm>,
        local: Arc<FakeLlm>,
    ) -> Ran {
        let providers: Providers = Arc::new(move |m: &ModelRef| match m.account.as_str() {
            "cloud" => Ok(cloud.clone() as Arc<dyn LlmProvider>),
            "local-lmstudio" => Ok(local.clone() as Arc<dyn LlmProvider>),
            other => Err(format!("the test has no provider for Account {other}")),
        });
        let ctx = RunContext {
            places: self.places.clone(),
            identity: Arc::new(FakeProjectIdentity::new("Test Mac")),
            terminal: Box::new(FakeTerminal::new()),
            providers,
            clock: am_testkit::FakeClock::at(at),
        };
        let argv = vec![
            "pair".to_string(),
            "--project".to_string(),
            root.to_string_lossy().into_owned(),
        ];
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run(&argv, ctx, &mut out, &mut err).await;
        Ran {
            code,
            out: String::from_utf8(out).unwrap(),
            err: String::from_utf8(err).unwrap(),
        }
    }

    /// `run_pair` with the matrix's usage: 1,200/90 on the cloud seat,
    /// 900/70 on this Mac's.
    async fn pair(&self, root: &Path, at: &str) -> Ran {
        self.run_pair(root, at, model(1_200, 90, 0), model(900, 70, 0))
            .await
    }

    fn status(&self, root: &Path) -> Ran {
        let argv = vec!["--project".to_string(), root.to_string_lossy().into_owned()];
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = status(&argv, &self.places, &mut out, &mut err);
        Ran {
            code,
            out: String::from_utf8(out).unwrap(),
            err: String::from_utf8(err).unwrap(),
        }
    }
}

/// A model that answers with `input`/`output` tokens of usage after
/// `delay_ms`.
fn model(input: u64, output: u64, delay_ms: u64) -> Arc<FakeLlm> {
    Arc::new(FakeLlm::new(vec![
        ScriptedTurn::text("A short line.")
            .usage(input, output)
            .delay(Duration::from_millis(delay_ms)),
    ]))
}

fn edit(root: &Path, rel: &str, from: &str, to: &str) {
    let path = root.join(rel);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains(from), "{rel} has no `{from}`");
    std::fs::write(&path, text.replacen(from, to, 1)).unwrap();
}

/// The one line of `out` that starts (after its indent) with `head` and a
/// space.
fn line<'a>(out: &'a str, head: &str) -> &'a str {
    let found: Vec<&str> = out
        .lines()
        .filter(|l| {
            let l = l.trim_start();
            l.strip_prefix(head)
                .is_some_and(|rest| rest.starts_with(' ') || rest.is_empty())
        })
        .collect();
    assert_eq!(found.len(), 1, "one line starting `{head}` in:\n{out}");
    found[0]
}

/// The line of the attempt `key` (`<key>  attempt <n>`) under the run `id`.
fn attempt_line<'a>(out: &'a str, id: &str, key: &str) -> &'a str {
    let lines: Vec<&str> = out.lines().collect();
    let start = lines
        .iter()
        .position(|l| l.starts_with(id))
        .unwrap_or_else(|| panic!("run {id} is not listed:\n{out}"));
    lines[start + 1..]
        .iter()
        .take_while(|l| l.starts_with(' '))
        .find(|l| {
            let words: Vec<&str> = l.split_whitespace().collect();
            words.first() == Some(&key) && words.get(1) == Some(&"attempt")
        })
        .unwrap_or_else(|| panic!("no attempt line for {key} under {id}:\n{out}"))
}

/// The run line of `id`.
fn run_line<'a>(out: &'a str, id: &str) -> &'a str {
    out.lines()
        .find(|l| l.starts_with(id))
        .unwrap_or_else(|| panic!("run {id} is not listed:\n{out}"))
}

/// The time a spend line shows after its tokens (`… tokens · 1.042s …`),
/// in milliseconds.
fn time_after_tokens(line: &str) -> i64 {
    let rest = line
        .split(" tokens · ")
        .nth(1)
        .unwrap_or_else(|| panic!("no time after the tokens: {line}"));
    let word = rest.split(" · ").next().unwrap().trim();
    let seconds = word
        .strip_suffix('s')
        .unwrap_or_else(|| panic!("a time in seconds: {line}"));
    let (whole, frac) = seconds.split_once('.').unwrap_or((seconds, "0"));
    let frac = format!("{frac:0<3}");
    whole.parse::<i64>().unwrap() * 1_000 + frac[..3].parse::<i64>().unwrap()
}

/// Row 20: each attempt line shows its calls' settled amounts in EUR and
/// USD, tokens and the call count (`local` for this Mac's), after its
/// state and time; the run line sums them.
#[tokio::test]
async fn status_rolls_up_by_step_and_run() {
    let env = env();
    let root = env.fixture();
    let ran = env.pair(&root, IN_W48).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    let id = ran.run_id();

    let st = env.status(&root);

    assert_eq!(st.code, 0, "{}", st.all());
    let head = run_line(&st.out, &id);
    assert!(
        head.contains("EUR 0.004554 · USD 0.00495 · 2,100/160 tokens"),
        "the run sums its calls: {head}"
    );
    assert!(
        head.contains("succeeded") && head.contains("snapshot "),
        "{head}"
    );
    let draft = attempt_line(&st.out, &id, "draft");
    assert!(
        draft.contains("EUR 0.004554 · USD 0.00495 · 1,200/90 tokens · 1 call"),
        "{draft}"
    );
    assert!(!draft.contains("local"), "{draft}");
    assert!(draft.contains("succeeded"), "{draft}");
    let check = attempt_line(&st.out, &id, "check");
    assert!(
        check.contains("EUR 0 · USD 0 · 900/70 tokens · 1 call · local"),
        "{check}"
    );
    for text in [&st.out, &st.err] {
        for symbol in ["€", "$", "₹"] {
            assert!(!text.contains(symbol), "no currency symbol: {text}");
        }
    }
}

/// Row 21: the spend section sums settled spend by week (`2026-W48 (from
/// 2026-11-23)`), by team and by seat, each with both currencies, tokens
/// and time, over two runs in two weeks; its head names the price file's
/// `updated` date, the rate snapshot (its first 12 hex digits) and the
/// rate.
#[tokio::test]
async fn status_rolls_up_by_seat_team_and_week_with_the_price_date() {
    let env = env();
    let root = env.fixture();
    let snapshot = sha256_hex(&std::fs::read(root.join(PRICES)).unwrap());
    for at in [IN_W48, IN_W49] {
        let ran = env.pair(&root, at).await;
        assert_eq!(ran.code, 0, "{}", ran.all());
    }

    let st = env.status(&root);

    assert_eq!(st.code, 0, "{}", st.all());
    let head = line(&st.out, "spend, settled");
    for words in [
        "prices updated 2026-11-01",
        &format!("rate snapshot {}", &snapshot[..12]),
        "1 USD = 0.92 EUR",
    ] {
        assert!(head.contains(words), "the head names {words}: {head}");
    }
    for week in [
        "week 2026-W48 (from 2026-11-23)",
        "week 2026-W49 (from 2026-11-30)",
    ] {
        let l = line(&st.out, week);
        assert!(
            l.contains("EUR 0.004554 · USD 0.00495 · 2,100/160 tokens · "),
            "{l}"
        );
        time_after_tokens(l);
    }
    for who in ["team core", "seat writer"] {
        let l = line(&st.out, who);
        assert!(
            l.contains("EUR 0.009108 · USD 0.0099 · 2,400/180 tokens · "),
            "{who}: {l}"
        );
        assert!(!l.contains("local"), "{l}");
        time_after_tokens(l);
    }
    for who in ["team side", "seat checker"] {
        let l = line(&st.out, who);
        assert!(
            l.contains("EUR 0 · USD 0 · 1,800/140 tokens · "),
            "{who}: {l}"
        );
        assert!(l.trim_end().ends_with("· local"), "{who}: {l}");
    }
}

/// Row 5: a call on this Mac shows `EUR 0 · USD 0`, its tokens, its time
/// (the call's `settled_at` minus `created_at`: at least the model's delay)
/// and `local`.
#[tokio::test]
async fn status_shows_a_local_call_at_zero_with_tokens_and_time() {
    let env = env();
    let root = env.fixture();
    let ran = env
        .run_pair(&root, IN_W48, model(1_200, 90, 0), model(900, 70, 80))
        .await;
    assert_eq!(ran.code, 0, "{}", ran.all());

    let st = env.status(&root);

    let l = line(&st.out, "seat checker");
    assert!(l.contains("EUR 0 · USD 0 · 900/70 tokens · "), "{l}");
    assert!(l.trim_end().ends_with("· local"), "{l}");
    assert!(time_after_tokens(l) >= 80, "the call's time: {l}");
    let reader = env.reader(&root);
    let run = reader.runs().unwrap().remove(0);
    let local: Vec<LedgerRow> = reader
        .ledger_for_run(&run.id)
        .unwrap()
        .into_iter()
        .filter(|l| l.local == Some(true))
        .collect();
    assert_eq!(local.len(), 1);
    assert_eq!(
        (
            local[0].reserved_micro_usd,
            local[0].reserved_micro_company,
            local[0].settled_micro_usd,
            local[0].settled_micro_company
        ),
        (0, Some(0), Some(0), Some(0))
    );
}

/// The cloud terms of the costs fixture (fake).
fn cloud() -> CallPrice {
    CallPrice {
        input_micro_usd_per_mtok: 3_000_000,
        output_micro_usd_per_mtok: 15_000_000,
        local: false,
    }
}

/// A run of `pair` written through the store's own commands (no CLI): the
/// draft attempt of seat `writer` reserves one cloud call at `rate` in
/// the week from 2026-11-23, settled with `usage` unless it is `None`.
fn written_run(db: &Path, rate: Rate, usage: Option<Option<Usage>>) -> String {
    let store = Store::open(db).unwrap();
    let w = store.writer();
    let engine = Actor::System {
        component: "engine".into(),
    };
    let run_id = new_id();
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
            taint: Taint::Clean,
            prompt: vec![],
            writes: AttemptWrites {
                seat: "writer".into(),
                table: None,
                document: None,
            },
        },
    )
    .unwrap();
    let call = new_id();
    w.exec(
        engine.clone(),
        WriteCommand::ReserveCall {
            step_run_id: draft.id.clone(),
            call_id: call.clone(),
            call_seq: 1,
            price: cloud(),
            rate,
            counted_input_tokens: 1_000,
            max_output_tokens: 4_096,
            week: "2026-11-23".into(),
            team: "core".into(),
            taint: Taint::Clean,
            purpose: CallPurpose::Turn,
            prompt: vec![],
        },
    )
    .unwrap();
    if let Some(usage) = usage {
        w.exec(
            engine,
            WriteCommand::SettleCall {
                call_id: call,
                usage,
            },
        )
        .unwrap();
    }
    run_id
}

/// 0.92 EUR per USD from the costs fixture's price file.
fn eur_rate(root: &Path) -> Rate {
    Rate {
        currency: "EUR".into(),
        company_micro_per_usd: Some(920_000),
        snapshot: sha256_hex(&std::fs::read(root.join(PRICES)).unwrap()),
        prices_updated: Some("2026-11-01".into()),
    }
}

/// Row 22: an open reservation (a run in flight, or DR-109's case) is
/// listed as `reserved, not settled`, with both amounts and its call
/// count, apart from settled spend and never folded into it.
#[tokio::test]
async fn status_shows_open_reservations_apart() {
    let env = env();
    let root = env.fixture();
    let ran = env.pair(&root, IN_W48).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    let open = written_run(&env.db(&root), eur_rate(&root), None);

    let st = env.status(&root);

    assert_eq!(st.code, 0, "{}", st.all());
    let l = line(&st.out, "reserved, not settled:");
    assert!(
        l.contains("EUR 0.059285 · USD 0.06444 (1 call)"),
        "the open reservation: {l}"
    );
    let week = line(&st.out, "week 2026-W48 (from 2026-11-23)");
    assert!(
        week.contains("EUR 0.004554 · USD 0.00495 · 2,100/160 tokens"),
        "settled spend leaves the reservation out: {week}"
    );
    assert!(run_line(&st.out, &open).contains("running"), "{}", st.out);
}

/// Row 6 (owner Q12): a project with no `models/prices.yaml` and only
/// seats on this Mac runs; every entry is 0 and `local`, with the snapshot
/// `none` and no rate; status says the price file is absent.
#[tokio::test]
async fn a_project_without_prices_runs_local_seats() {
    let env = env();
    let root = env.fixture();
    std::fs::remove_file(root.join(PRICES)).unwrap();
    edit(
        &root,
        "seats/writer.md",
        "account: cloud",
        "account: local-lmstudio",
    );

    let ran = env
        .run_pair(&root, IN_W48, model(1_200, 90, 0), model(900, 70, 0))
        .await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    let reader = env.reader(&root);
    let run = reader.runs().unwrap().remove(0);
    let ledger = reader.ledger_for_run(&run.id).unwrap();
    assert_eq!(ledger.len(), 2);
    for l in &ledger {
        assert_eq!(l.local, Some(true), "{l:?}");
        assert_eq!(l.rate_snapshot, "none", "{l:?}");
        assert_eq!(l.company_micro_per_usd, None, "{l:?}");
        assert_eq!(l.currency.as_deref(), Some("EUR"), "{l:?}");
        assert_eq!(
            (
                l.reserved_micro_usd,
                l.settled_micro_usd,
                l.settled_micro_company
            ),
            (0, Some(0), Some(0)),
            "{l:?}"
        );
    }
    let st = env.status(&root);
    assert!(
        st.out.contains("no price file: local calls only"),
        "{}",
        st.out
    );
}

/// Row 12 (owner Q6): a company in USD needs no `rates:` entry: its
/// entries carry `USD` at rate 1 (1,000,000 micro per USD), and its company
/// amounts equal its USD amounts.
#[tokio::test]
async fn a_usd_company_needs_no_rate() {
    let env = env();
    let root = env.fixture();
    edit(&root, "company.yaml", "currency: EUR", "currency: USD");
    edit(
        &root,
        PRICES,
        "rates:\n  - currency: EUR\n    per_usd: 0.92\n",
        "",
    );

    let ran = env.pair(&root, IN_W48).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    let reader = env.reader(&root);
    let run = reader.runs().unwrap().remove(0);
    let cloud = reader
        .ledger_for_run(&run.id)
        .unwrap()
        .into_iter()
        .find(|l| l.local == Some(false))
        .expect("the cloud entry");
    assert_eq!(cloud.currency.as_deref(), Some("USD"));
    assert_eq!(cloud.company_micro_per_usd, Some(1_000_000));
    assert_eq!(cloud.reserved_micro_company, Some(cloud.reserved_micro_usd));
    assert_eq!(cloud.settled_micro_usd, Some(4_950));
    assert_eq!(cloud.settled_micro_company, Some(4_950));
}

/// Row 13 (owner Q8): a seat on a cloud Account whose (provider, model) the
/// price file does not price refuses the run before any run row, naming
/// the seat, the Account, the model and the fix; no model is called. The
/// price is looked up by the Account's provider, so the same model name
/// under another provider does not price it.
#[tokio::test]
async fn an_unpriced_cloud_model_is_refused_before_any_row() {
    for (case, rel, from, to, model_name) in [
        (
            "an unpriced model",
            "seats/writer.md",
            "model: example-large",
            "model: example-unpriced",
            "example-unpriced",
        ),
        (
            "another provider's price",
            PRICES,
            "provider: anthropic",
            "provider: openai",
            "example-large",
        ),
    ] {
        let env = env();
        let root = env.fixture();
        edit(&root, rel, from, to);
        let (cloud, local) = (model(1_200, 90, 0), model(900, 70, 0));

        let ran = env
            .run_pair(&root, IN_W48, cloud.clone(), local.clone())
            .await;

        assert_eq!(ran.code, 1, "{case}: {}", ran.all());
        for words in ["writer", "cloud", model_name, PRICES, NOT_STARTED] {
            assert!(
                ran.err.contains(words),
                "{case}: names {words}: {}",
                ran.all()
            );
        }
        assert!(
            !ran.out.lines().any(|l| l.starts_with("run ")),
            "{case}: no run line: {}",
            ran.all()
        );
        assert_eq!((cloud.calls(), local.calls()), (0, 0), "{case}");
        if let Some(layout) = env.layout(&root)
            && layout.state_db().exists()
        {
            assert!(
                Reader::open(&layout.state_db())
                    .unwrap()
                    .runs()
                    .unwrap()
                    .is_empty(),
                "{case}: no run row"
            );
        }
    }
}

/// Row 14 (owner Q6): a cloud call in a company currency other than USD
/// with no `rates:` entry for it refuses the run before any row, naming the
/// currency and the fix; the same project with only seats on this Mac runs
/// (they need no rate).
#[tokio::test]
async fn a_missing_rate_is_refused_before_any_row() {
    let env = env();
    let root = env.fixture();
    edit(
        &root,
        PRICES,
        "rates:\n  - currency: EUR\n    per_usd: 0.92\n",
        "",
    );
    let (cloud, local) = (model(1_200, 90, 0), model(900, 70, 0));

    let ran = env
        .run_pair(&root, IN_W48, cloud.clone(), local.clone())
        .await;

    assert_eq!(ran.code, 1, "{}", ran.all());
    for words in ["EUR", "rates", PRICES, NOT_STARTED] {
        assert!(ran.err.contains(words), "names {words}: {}", ran.all());
    }
    assert_eq!((cloud.calls(), local.calls()), (0, 0));
    if let Some(layout) = env.layout(&root)
        && layout.state_db().exists()
    {
        assert!(
            Reader::open(&layout.state_db())
                .unwrap()
                .runs()
                .unwrap()
                .is_empty(),
            "no run row"
        );
    }

    edit(
        &root,
        "seats/writer.md",
        "account: cloud",
        "account: local-lmstudio",
    );
    let ran = env
        .run_pair(&root, IN_W48, model(1_200, 90, 0), model(900, 70, 0))
        .await;
    assert_eq!(ran.code, 0, "local seats need no rate: {}", ran.all());
}

// ------------------------------------------------------------------ review round 1

impl Env {
    /// This Mac's Accounts file: `fixtures/accounts/costs.yaml` and then
    /// `extra` (more entries of its `accounts:` list).
    fn with_accounts_and(&self, extra: &str) {
        let path = self.places.user.accounts_file();
        let base = std::fs::read_to_string(am_testkit::fixture("accounts/costs.yaml")).unwrap();
        std::fs::write(&path, format!("{base}{extra}")).unwrap();
        set_mode(&path, 0o600);
    }

    /// `agentic run <workflow>` at the clock `at` with `providers`.
    async fn run_workflow(
        &self,
        root: &Path,
        workflow: &str,
        at: &str,
        providers: Providers,
    ) -> Ran {
        let ctx = RunContext {
            places: self.places.clone(),
            identity: Arc::new(FakeProjectIdentity::new("Test Mac")),
            terminal: Box::new(FakeTerminal::new()),
            providers,
            clock: am_testkit::FakeClock::at(at),
        };
        let argv = vec![
            workflow.to_string(),
            "--project".to_string(),
            root.to_string_lossy().into_owned(),
        ];
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run(&argv, ctx, &mut out, &mut err).await;
        Ran {
            code,
            out: String::from_utf8(out).unwrap(),
            err: String::from_utf8(err).unwrap(),
        }
    }

    /// `agentic gate <item> --project <root>`, deciding `a` (approve) on
    /// the terminal, with `providers`.
    async fn approve(&self, root: &Path, item: &str, providers: Providers) -> Ran {
        let term = FakeTerminal::new();
        term.line("a");
        let ctx = am_app::cli::GateContext {
            places: self.places.clone(),
            identity: Arc::new(FakeProjectIdentity::new("Test Mac")),
            terminal: Box::new(term),
            providers,
            clock: am_testkit::FakeClock::at(IN_W48),
        };
        let argv = vec![
            item.to_string(),
            "--project".to_string(),
            root.to_string_lossy().into_owned(),
        ];
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = am_app::cli::gate(&argv, ctx, &mut out, &mut err).await;
        Ran {
            code,
            out: String::from_utf8(out).unwrap(),
            err: String::from_utf8(err).unwrap(),
        }
    }
}

/// Every model served by `llm`, whatever its Account.
fn every(llm: Arc<FakeLlm>) -> Providers {
    Arc::new(move |_m: &ModelRef| Ok(llm.clone() as Arc<dyn LlmProvider>))
}

/// A seat `boxer` of `core` on the Account `box`, model `example-large`.
fn add_boxer(root: &Path) {
    std::fs::write(
        root.join("seats/boxer.md"),
        "---\nschema_version: 2\nname: Boxer\nrole: Writer\nteam: core\ndescription: Writes on the Account box.\nmodel:\n  account: box\n  model: example-large\n---\nWrite one line.\n",
    )
    .unwrap();
}

/// An `openai-compatible` Account `box` at `base_url`.
fn compatible_box(base_url: &str) -> String {
    format!(
        "  - id: box\n    provider: openai-compatible\n    name: A model server\n    base_url: {base_url}\n    secret_ref: box\n"
    )
}

/// Review round 1, M5 (owner Q8, "on this Mac"): whether an Account's calls
/// cost money follows where its server is, never its provider's name. An
/// `openai-compatible` Account at `https://…` is a cloud call: with no
/// price for its model the run is refused before any row, as an unpriced
/// cloud model; at `http://127.0.0.1:<port>` it is local and costs 0. An
/// Account whose `provider` is `ollama` at `https://…` is refused before
/// any row too, and no model is called. And the local decision ignores no
/// finding of this Mac's Accounts file: `box` listed twice (first at
/// `127.0.0.1`, then at `https://…`) is not taken as local; the run is
/// refused before any row (tester's reading of "ignores no Accounts-file
/// findings", 2026-11-25).
#[tokio::test]
async fn an_account_off_this_mac_is_priced_whatever_its_provider() {
    let refused = |ran: &Ran, env: &Env, root: &Path, llm: &FakeLlm, case: &str| {
        assert_eq!(ran.code, 1, "{case}: {}", ran.all());
        assert!(ran.err.contains(NOT_STARTED), "{case}: {}", ran.all());
        assert_eq!(llm.calls(), 0, "{case}: no model called");
        if let Some(layout) = env.layout(root)
            && layout.state_db().exists()
        {
            assert!(
                Reader::open(&layout.state_db())
                    .unwrap()
                    .runs()
                    .unwrap()
                    .is_empty(),
                "{case}: no run row"
            );
        }
    };
    let on_box = |root: &Path| edit(root, "seats/writer.md", "account: cloud", "account: box");

    // openai-compatible at https: a cloud call, unpriced.
    let env = crate::env();
    env.with_accounts_and(&compatible_box("https://models.example/v1"));
    let root = env.fixture();
    on_box(&root);
    let llm = model(1_200, 90, 0);
    let ran = env
        .run_workflow(&root, "pair", IN_W48, every(llm.clone()))
        .await;
    refused(&ran, &env, &root, &llm, "compatible at https");
    for words in ["writer", "box", "example-large", PRICES] {
        assert!(ran.err.contains(words), "names {words}: {}", ran.all());
    }

    // openai-compatible at 127.0.0.1: on this Mac, 0.
    let env = crate::env();
    env.with_accounts_and(&compatible_box("http://127.0.0.1:8080/v1"));
    let root = env.fixture();
    on_box(&root);
    let ran = env
        .run_workflow(&root, "pair", IN_W48, every(model(1_200, 90, 0)))
        .await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    let reader = env.reader(&root);
    let run = reader.runs().unwrap().remove(0);
    for l in reader.ledger_for_run(&run.id).unwrap() {
        assert_eq!(l.local, Some(true), "{l:?}");
        assert_eq!((l.reserved_micro_usd, l.settled_micro_usd), (0, Some(0)));
    }

    // provider `ollama` at https: never priced at 0.
    let env = crate::env();
    env.with_accounts_and(
        "  - id: box\n    provider: ollama\n    name: A hosted model server\n    base_url: https://ollama.example\n    secret_ref: box\n",
    );
    let root = env.fixture();
    on_box(&root);
    let llm = model(1_200, 90, 0);
    let ran = env
        .run_workflow(&root, "pair", IN_W48, every(llm.clone()))
        .await;
    refused(&ran, &env, &root, &llm, "ollama at https");

    // `box` twice in this Mac's Accounts: a finding, so not taken as local.
    let env = crate::env();
    env.with_accounts_and(&format!(
        "{}{}",
        compatible_box("http://127.0.0.1:8080/v1"),
        compatible_box("https://models.example/v1")
    ));
    let root = env.fixture();
    on_box(&root);
    let llm = model(1_200, 90, 0);
    let ran = env
        .run_workflow(&root, "pair", IN_W48, every(llm.clone()))
        .await;
    refused(&ran, &env, &root, &llm, "box listed twice");
}

/// Adds the workflow `gated` to the costs copy: `draft` on `checker` (this
/// Mac), the gate `approve` on the Human seat `boss`, then `publish` on
/// `boxer` (Account `box`).
fn add_gated(root: &Path) {
    add_boxer(root);
    std::fs::write(
        root.join("seats/boss.md"),
        "---\nschema_version: 2\nname: Boss\nrole: Human seat\nteam: core\ndescription: Approves the work in the Inbox. It has no model.\n---\nA person fills this seat.\n",
    )
    .unwrap();
    std::fs::write(
        root.join("workflows/gated.yaml"),
        "schema_version: 2\nname: Gated\nteam: core\ndescription: A step on this Mac, a human gate, then a step on the Account box.\nsteps:\n  - id: draft\n    seat: checker\n    instruction: Draft one line.\n  - id: approve\n    seat: boss\n    instruction: Approve or return the line.\n    gate: review\n  - id: publish\n    seat: boxer\n    instruction: Publish the line.\n",
    )
    .unwrap();
}

/// Review round 1, M9 (rows 13 and 14; owner Q8, AD-16): `agentic gate`'s
/// continue path prices the steps still to run before it records the
/// decision. The run started with `box` on this Mac (`127.0.0.1`), so it
/// needed no price and no rate, and waits at its gate; then this Mac's
/// Accounts move `box` to `https://…` (Accounts are not in the pinned
/// snapshot). Approving is refused, exit 1, naming the seat, the Account,
/// the model and `models/prices.yaml` (row 13) or the currency, `rates`
/// and `models/prices.yaml` (row 14); the item stays open with no
/// decision, and no model is called after the gate.
#[tokio::test]
async fn the_gate_refuses_an_unpriced_or_unrated_step_before_recording_the_decision() {
    for (case, priced, words) in [
        (
            "row 13: no price",
            false,
            vec!["boxer", "box", "example-large", PRICES],
        ),
        ("row 14: no rate", true, vec!["EUR", "rates", PRICES]),
    ] {
        let env = crate::env();
        env.with_accounts_and(&compatible_box("http://127.0.0.1:8080/v1"));
        let root = env.fixture();
        add_gated(&root);
        if priced {
            edit(
                &root,
                PRICES,
                "rates:\n  - currency: EUR\n    per_usd: 0.92\n",
                "  - provider: openai-compatible\n    model: example-large\n    input_usd_per_mtok: 1.0\n    output_usd_per_mtok: 2.0\n",
            );
        }
        let llm = model(1_200, 90, 0);
        let ran = env
            .run_workflow(&root, "gated", IN_W48, every(llm.clone()))
            .await;
        assert_eq!(ran.code, 0, "{case}: {}", ran.all());
        assert!(ran.out.contains("waiting_human"), "{case}: {}", ran.all());
        let reader = env.reader(&root);
        let run = reader.runs().unwrap().remove(0);
        let item = reader
            .inbox_items(&run.id)
            .unwrap()
            .into_iter()
            .find(|i| i.status == "open")
            .expect("the Review item")
            .id;
        drop(reader);
        let calls = llm.calls();
        env.with_accounts_and(&compatible_box("https://models.example/v1"));

        let ran = env.approve(&root, &item, every(llm.clone())).await;

        assert_eq!(ran.code, 1, "{case}: {}", ran.all());
        for w in &words {
            assert!(ran.err.contains(w), "{case}: names {w}: {}", ran.all());
        }
        let row = env.reader(&root).inbox_item(&item).unwrap().unwrap();
        assert_eq!(row.status, "open", "{case}: {row:?}");
        assert_eq!((row.decision, row.resolved_by), (None, None), "{case}");
        assert_eq!(llm.calls(), calls, "{case}: no model called");
        let publish = env
            .reader(&root)
            .steps(&run.id)
            .unwrap()
            .into_iter()
            .find(|s| s.step_key == "publish")
            .unwrap();
        assert_eq!(publish.status, "queued", "{case}");
    }
}

/// Review round 1, M11 (owner Q12): a company in USD shows each amount
/// once, `USD x`, never `USD x · USD x`, on the run and attempt lines and
/// in the spend section, whose head names no rate.
#[tokio::test]
async fn a_usd_company_shows_usd_once() {
    let env = crate::env();
    let root = env.fixture();
    edit(&root, "company.yaml", "currency: EUR", "currency: USD");
    edit(
        &root,
        PRICES,
        "rates:\n  - currency: EUR\n    per_usd: 0.92\n",
        "",
    );
    let ran = env.pair(&root, IN_W48).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    let id = ran.run_id();

    let st = env.status(&root);

    assert_eq!(st.code, 0, "{}", st.all());
    let usd_count = |l: &str| l.matches("USD ").count();
    let head = run_line(&st.out, &id);
    assert!(head.contains("USD 0.00495 · 2,100/160 tokens"), "{head}");
    assert_eq!(usd_count(head), 1, "{head}");
    let draft = attempt_line(&st.out, &id, "draft");
    assert!(
        draft.contains("USD 0.00495 · 1,200/90 tokens · 1 call"),
        "{draft}"
    );
    assert_eq!(usd_count(draft), 1, "{draft}");
    let check = attempt_line(&st.out, &id, "check");
    assert!(
        check.contains("USD 0 · 900/70 tokens · 1 call · local"),
        "{check}"
    );
    assert_eq!(usd_count(check), 1, "{check}");
    let week = line(&st.out, "week 2026-W48 (from 2026-11-23)");
    assert!(week.contains("USD 0.00495 · 2,100/160 tokens · "), "{week}");
    assert_eq!(usd_count(week), 1, "{week}");
    let spend = line(&st.out, "spend, settled");
    assert!(
        !spend.contains("= 1 USD"),
        "no rate for a USD company: {spend}"
    );
}
