---
title: 'Cost settles per call and rolls up'
type: feature
ticket: 7
created: '2026-11-25'
status: built
baseline_revision: 'b234d74'
after: [21]
route: full
route_source: auto
risk: high
review: quick
review_source: pinned
lenses_ran: [correctness, adversarial-security]
review_loop_iteration: 2
followup_review_recommended: false
context:
  - '{project-root}/docs/owner-standing-rules.md'
  - '{project-root}/docs/deferred-register.md'
  - '{project-root}/_bmad-output/initiative-agentic-manager/architecture-agentic-manager/architecture-agentic-manager.md'
  - '{project-root}/_bmad-output/initiative-agentic-manager/ux-agentic-manager/EXPERIENCE.md'
  - '{project-root}/_bmad-output/initiative-agentic-manager/tickets/epic-workflow-engine-cli/tickets/story-document-versions-and-draft-items-plan.md'
  - '{project-root}/_bmad-output/initiative-agentic-manager/tickets/epic-workflow-engine-cli/tickets/story-golden-run-so-far-on-every-pr-plan.md'
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Story:** 3.7, epic 3 entry 7, "Cost settles per call and rolls up" (`covers = ["R2"]`, `after = [21]`). It builds after 3.6b (entry 21, PR #46); its baseline is set when 3.6b merges. **Money is an owner stop:** this plan is a draft, and no test or code is written until the owner answers Q1–Q16 (Design Notes, Owner questions (money)). The Approach below is the planner's recommendation, pending those answers.

**Problem (the code as it stands on `story/3-21`, 226c4fc):**
- **Every call is reserved and settled at zero.**
  - `crates/am-engine/src/run.rs` L130-L132: `NO_RATE_SNAPSHOT = "none"`, "amounts arrive with the cost ledger, story 7".
  - L1400-L1412: every `ReserveCall` sends `reserved_micro_usd: 0`.
  - Every settlement passes `cost_micro_usd: None` (L1452, L1466, L1493, L1515, L1571, L1627, L1639, L1726, L1927, L1944, L1972, L2011, L2050, L2078, L2106), so each entry settles at its zero reservation.
  - The golden copy shows it: `fixtures/golden/so-far/snapshot.txt` L47, L67, L88, `ledger call 1 reserved 0 settled 0 settled`.
- **The ledger holds USD only.** `crates/am-store/migrations/0001_initial.sql` L31-L41: `reserved_micro_usd`, `settled_micro_usd`, `state`, `rate_snapshot TEXT`. It has no company-currency amount, no price terms, no team and no week. `model_calls` (L20-L30) holds tokens but no settle time.
- **The writer takes amounts the caller makes.**
  - `crates/am-store/src/lib.rs` L329-L338: `ReserveCall { reserved_micro_usd, rate_snapshot, .. }`.
  - L1680-L1709 `settle()`: the caller's `cost_micro_usd`, or the reservation when `None`.
  - L1358-L1371 `MarkLost`: settles USD only.
  - L3336-L3341 `LedgerRow`: four fields.
- **The price file has no rate and no hash.**
  - `crates/am-config/src/kinds.rs` L64 `PRICES_KEYS = [schema_version, updated, models, services]`; L612-L697 `prices()`.
  - `updated` is optional (L618).
  - `Prices` (`crates/am-config/src/entities.rs` L66-L73) has no USD-to-company rate and no file hash.
  - No AD says where the rate comes from.
- **No shipped price file exists.**
  - The Structural Seed names `templates/prices.yaml` ("shipped defaults"); there is none.
  - `templates/samples/company-a/` has no `models/` folder.
  - The only price file is `fixtures/projects/valid/models/prices.yaml` (`updated: 2026-11-01`).
- **The company currency is config, not locale.**
  - `company.yaml` requires `currency` (`kinds.rs` L387-L397, three capital letters) and `timezone` (L385); `week_start` defaults to `monday` (L398-L399).
  - Nothing reads the system locale; epic 5 story 16's wizard pre-fills it.
- **`agentic status` shows no money.**
  - `crates/am-app/src/run.rs` L963-L1004 `status`, L854 `run_line`, L871 `step_line`.
  - It reads only the database, without config.
  - `plan_of` (L412-L548) hands the engine no price.
- **The spike's arithmetic floors and guesses.** `crates/am-engine/src/journal.rs` L29-L43 `Price::cost` truncates; L177-L178 estimates input at characters / 4.
- **Providers report two counts.**
  - `Usage { input_tokens, output_tokens }` only (`crates/am-domain/src/llm.rs` L108-L112).
  - A cut-off stream reports `None` (`crates/am-providers/src/anthropic.rs` L399-L406).
  - OpenAI-compatible asks `include_usage` (`openai_compat.rs` L204) and reads `prompt_tokens`/`completion_tokens` (L305-L311); reasoning text is never read (L6-L7).
- **The writer stamps time with the system clock** (`now_ts`, `lib.rs` L251-L253), never the run's `Clock`.

**Approach (recommended; each point names its owner question):**
- **Price terms per step (Q1, Q8):**
  - am-app maps each non-gate step's `ModelRef` to its Account's provider and to a price from the run's pinned snapshot (AD-16): local zero, or `models/prices.yaml`'s USD per million tokens.
  - An unpriced cloud model, or a missing rate, refuses the run before any row (Q6, Q8).
- **Reservation (Q1, Q2):** the engine counts the request's prompt tokens by the approved rule. `ReserveCall` carries the counted tokens, `max_output_tokens`, the price terms, the rate, the snapshot id, the week and the team. The writer computes both amounts, journals them with the terms and commits before the call (AD-8, AD-9).
- **Settlement (Q3, Q4):** the settle commands carry usage only. The writer prices that usage with the terms journaled at reserve, in both currencies, and stamps `settled_at`. Missing usage settles at the reservation. `cost_micro_usd` leaves every settle command.
- **Money arithmetic (Q5):** one pure function in am-domain, in i128, rounding up once per amount from the exact value, checked for overflow.
- **Rate (Q6):** a `rates:` list in `models/prices.yaml`: company-currency units per USD, dated by the file's `updated:`. The snapshot id is the file's sha256; a USD company needs no rate.
- **Week (Q11):** the engine stamps each reservation with its budget week, computed from the run's `Clock` in the pinned snapshot's timezone and `week_start`.
- **Ledger shape (Q13):** migration 0013 adds the columns and integrity triggers; old rows are not backfilled.
- **`agentic status` (Q12):** spend on each run and attempt line, then a spend section by seat, team and week with the price file's date. Each amount shows as the ISO code and its amount (no symbol, no locale grouping), with tokens and time; local calls are marked `local`.
- **Shipped price files (Q10):** `templates/prices.yaml` and the sample's `models/prices.yaml`, each with an `updated:` date.
- **No cap is checked** (epic 4 story 2).

## Boundaries & Constraints

**Always:**
- **Money (Consistency Conventions, AD-9):** i64 micro-units in USD and in the company currency, with the rate-snapshot id, on every ledger entry this story writes. The arithmetic runs in one am-domain function only (Q5).
- **Reserve, then call (AD-8, AD-9):** the reservation and its audit commit before the provider is called. The settlement commits with the step's transition, as today (`SettleAndFinishStep`, `SettleAndFinishDocumentStep`, `SettleAndLoopBack`, `FailWithItem`).
- **Missing usage settles at the reservation (AD-9),** in both currencies; `MarkLost` too (AD-8).
- **Prices and rate from the pinned snapshot (AD-16):** read once, when the plan is built from the snapshot the run pins. A later edit to the price file changes no reservation or settlement of that run.
- **Local calls are 0** in both currencies, with tokens and time (Conventions, Money), whatever the price file says.
- **The company currency comes from `company.yaml`**: no currency or timezone literal in `crates/` (AGENTS.md).
- **Writer commands with an explicit Actor (AD-6, AD-24):** the engine's reserve and settle commands stay `Actor::System` (engine).
- **Audit (AD-25):** amounts, counts, price terms, ids, the week and the team. Never prompt or reply text, and no new `*_sha256` of free text.
- **Golden extension rule (3.24 plan):** the golden copy's ledger lines in `fixtures/golden/so-far/snapshot.txt` move in this story's commit, hand-checked against this matrix.
- **Tests first:** every test in the Tests-first list is written and seen red, with its reason logged, before any `src/` change. FakeLlm and FakeClock only.
- **One squash commit;** stop and report if the code passes about 4,000 lines (owner, 2026-11-24).

**Never:**
- **No cap is checked (epic 4 story 2).** A reservation is never refused for its size, and no Budget ask is raised.
- **No network in tests, and none added to the app for rates or prices (AD-12):** no rate fetch, no token-count endpoint.
- **No new crate.** ISO weeks come from jiff 1.0.0 (Stack), 128-bit arithmetic is native, and no tokenizer or locale crate is added (Q2, Q7).
- **No system-locale read** (Q7; epic 5 story 16's wizard pre-fills the currency).
- **No layering of the shipped price file under a project's** (Q10).
- **No UI, meter or estimator** (epic 5 stories 6 and 19), **no service spend** (epic 12), **no Assistant spend scope** (epic 4 story 2, epic 8).
- **No retry or fallback pricing** (3.8): this story makes one call per turn, as today.
- **No currency symbol or locale formatting in the CLI** (Conventions, Money: "formatted only in the UI").
- **No department vocabulary in `crates/`;** fixtures and templates only.

## I/O & Edge-Case Matrix

Amounts in the examples use the fixture's terms: input 3.00 and output 15.00 USD per million tokens; 0.92 EUR per USD; `max_output_tokens` 4,096.

| # | Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|----------|--------------|---------------------------|----------------|
| 1 | Reserve (cloud) | A step on a priced cloud model; its request counts 1,000 prompt tokens (Q2) | Before the call: one ledger row `reserved`, 64,440 micro-USD and 59,285 micro-EUR ((1,000 × 3 + 4,096 × 15) / 10⁶ USD, × 0.92, rounded up), with the terms, counted tokens, `max_output_tokens`, rate, snapshot id, `prices_updated`, week and team. `model_call.reserved` is audited before the provider is called. | — |
| 2 | Settle from usage | FakeLlm reports usage 1,200/90 | Settled 4,950 micro-USD and 4,554 micro-EUR at the reserved terms; tokens 1,200/90 on the call; `settled_at` stamped; state `settled`; audit `at_reservation: false` | — |
| 3 | Settle at the reservation | FakeLlm reports no usage | Settled = reserved in both currencies; audit `at_reservation: true` | — |
| 4 | Usage above the estimate | Usage input above the counted estimate | Settled at the reported usage, above the reservation; audit `over_reservation: true`; no cap check (Q3) | — |
| 5 | Local call | A seat on `local-lmstudio`; FakeLlm usage 1,200/90 after a delay | 0 and 0 reserved and settled; `local = 1`; tokens 1,200/90; time = `settled_at − created_at`; status shows `EUR 0 · USD 0 · 1,200/90 tokens · <time> · local` | — |
| 6 | Local only, no price file | A project with no `models/prices.yaml` and only local seats | The run goes ahead; amounts 0; rate snapshot `none`; status says the price file is absent (Q12) | — |
| 7 | Failed call | A provider error, a reply cut off at `max_tokens`, a call cancelled at the wall clock | Settles from usage if reported, else at the reservation (AD-9 literal, Q3); the step fails as today | — |
| 8 | Repair turn (3.5) | A final message fails its schema; the repair call runs | Its own reservation, counted from its own request (the invalid reply and the repair instruction included), its own settlement; rolls up under the same attempt | — |
| 9 | Retry or fallback | — | None in this story (3.8). Each retry is its own entry through the same path; a fallback settles at the serving model's price (3.8, new register row) | — |
| 10 | Lost call | `MarkLost` on an open call (resume, 3.9) | Settled = reserved in both currencies, `settled_at` stamped; a settled call is unchanged (as today) | — |
| 11 | Rate snapshot | Two runs with the price file edited between them | Each entry keeps the snapshot id (the file's sha256), terms, rate and `updated` it was reserved with; status lists both dates | — |
| 12 | Company currency | The costs fixture (`currency: EUR`); the sample (`INR`); a fixture with `currency: USD` | Entries carry `EUR`, `INR` and `USD`. A USD company uses rate 1 with no `rates` entry, so its company amounts equal its USD amounts. No code reads the locale (Q7). | — |
| 13 | No price for a cloud model | A seat on a cloud Account whose (provider, model) has no price | `agentic run` refuses before any run row, naming the seat, Account, model and the fix (add it to `models/prices.yaml`) | Worded refusal |
| 14 | No rate | Company currency not USD, a cloud call, and no `rates` entry for that currency | Refused before any run row, naming the currency and the fix | Worded refusal |
| 15 | Rounding (Q5) | 1 token at 0.075 USD per million; an overflowing product | The cost rounds up to 1 micro-unit. Each currency's amount is computed once from the exact value, never from the other rounded amount. Overflow is refused in words and nothing is written. | Refused |
| 16 | Counting (Q2) | Two requests that differ by one 3,000-byte part | Counted tokens and the reservation grow by the rule's amount. The count covers the system parts, every message part and each offered tool's definition. | — |
| 17 | Week across DST (Q11) | Company `America/New_York`, `week_start: monday`; FakeClock at 2026-11-02T04:30Z and 05:00Z; 2026-03-09T03:30Z and 04:30Z | 2026-W44 and 2026-W45 (Sunday 23:30 EST, then Monday 00:00 EST); 2026-W10 and 2026-W11 (Sunday 23:30 EDT, then Monday 00:30 EDT) | — |
| 18 | Week at the year boundary | Company `Asia/Kolkata`; FakeClock at 2026-12-31T19:00Z; 2027-01-03T18:30Z; 2025-12-29T00:00Z (UTC company) | 2026-W53 (1 Jan 00:30 IST); 2027-W01 (Monday 00:00 IST while UTC is Sunday); 2026-W01 | — |
| 19 | Week start not Monday | `week_start: sunday`; an instant on Sunday 10:00 local | Week key = that Sunday's date; label `week from <date>`, no ISO label (Q11) | — |
| 20 | Roll-up by step and run | A run of two steps, one cloud and one local | Each attempt line shows its calls' settled amounts, tokens and time; the run line sums them | — |
| 21 | Roll-up by seat, team, week | Runs over two seats in two teams and two weeks | The spend section sums by seat, by team and by week, each with both currencies, tokens and time. Its head names the price file's `updated` date(s) and snapshot(s). | — |
| 22 | Reserved, never settled | An open reservation (DR-109's case, or a run in flight) | Shown as `reserved`, apart from settled spend, and named `not settled` | — |
| 23 | Old rows and mixed currencies | Pre-0013 ledger rows; rows in EUR and in INR (currency changed between runs) | Old rows: company amount `-` and a footnote. Mixed currencies: one subtotal per currency, never converted. USD always sums. | — |
| 24 | Ledger integrity (Q13) | `UPDATE` of reserve-time columns, a second settle, `DELETE`, a negative amount, a local row with a non-zero amount | Aborted by trigger or refused by the writer, with nothing written | Trigger / refused |
| 25 | Migration 0013 | A version-12 database with runs, attempts, calls, ledger rows, rows, history, versions, Inbox items, prompt parts | Pre-migration copy; every row kept value for value; new columns NULL on old rows; report 12 → 13; `store.migrated` audited; reopening applies nothing; a failing 0013 names the backup and leaves version 12 | Migration error |
| 26 | Audit | Rows 1–5 and 8 | `model_call.reserved` and `model_call.settled` hold amounts, currency, tokens, terms, snapshot, week, team and the flags of rows 3–4. A canary in the prompt and in the reply appears in no audit entry. The chain verifies. | — |
| 27 | Price files | `templates/prices.yaml`; the sample's `models/prices.yaml`; a `rates` entry that is malformed, duplicated, zero or for USD | The shipped files parse and carry `updated:`. Each bad entry is a validate error with file, line and fix. A file pricing a model with no `updated:` gets a warning (Q10). | Validate error / warning |
| 28 | Golden so-far | The copy's three local calls | Ledger lines show `local`, 0/0 in USD and INR, and the snapshot id, moved by hand (3.24's extension rule) | — |
| 29 | No cap, no network | Any run here | No reservation is refused for its size; no egress action is audited; FakeLlm only | — |

</intent-contract>

## Code Map

- **`crates/am-domain`:**
  - `src/money.rs` (new): `CallPrice` (input and output micro-USD per million tokens, `local`), `Rate` (currency code, company micro-units per USD, snapshot id, `prices_updated`), `Money { usd_micro, company_micro }`. `cost(price, rate, input_tokens, output_tokens) -> Result<Money, MoneyError>` in i128, rounding up once per amount (Q5).
  - `src/week.rs` (new): `budget_week(at: Timestamp, tz: &TimeZone, week_start: Weekday) -> BudgetWeek { start: civil::Date }` and its ISO label for a Monday start (jiff's `ISOWeekDate`).
  - `src/lib.rs`: re-exports. `Usage` is unchanged.
- **`crates/am-config`:**
  - `src/kinds.rs` L64: `PRICES_KEYS` gains `rates`.
  - `prices()` (L612-L697) reads `rates: [{ currency, per_usd }]`: three capital letters, unique, `per_usd > 0`, no `USD` entry. It warns when a file prices a model and has no `updated:` (Q10).
  - `src/entities.rs` L66-L73: `Prices` gains `rates: Vec<CurrencyRate>` and `sha256: Option<String>` (the file's bytes as loaded, `src/lib.rs` L530).
  - A public parse of one prices text for the shipped-file test.
- **`crates/am-store`:**
  - `migrations/0013_cost_ledger.sql` (new, additive; Q13). `ledger` gains:
    - `currency`, `reserved_micro_company`, `settled_micro_company`;
    - `input_micro_usd_per_mtok`, `output_micro_usd_per_mtok`, `company_micro_per_usd`, `prices_updated`;
    - `counted_input_tokens`, `max_output_tokens`, `local`, `week`, `team`.
  - `model_calls` gains `settled_at`. Integrity triggers (row 24). `src/migrations.rs` `MIGRATIONS`; `SCHEMA_VERSION` 13.
  - `src/lib.rs`:
    - `ReserveCall` (L329-L338): `reserved_micro_usd` and `rate_snapshot` are replaced by `price: CallPrice`, `rate: Rate`, `counted_input_tokens`, `max_output_tokens`, `week`, `team`.
    - Its handler (L1313-L1352) computes both amounts with `am_domain::money::cost`.
    - `settle()` (L1680-L1709) prices the usage from the row's terms and stamps `settled_at`.
    - `MarkLost` (L1358-L1371) settles both currencies.
    - `cost_micro_usd` leaves `SettleCall`, `SettleAndFinishStep`, `SettleAndFinishDocumentStep`, `SettleAndLoopBack` and `CallSettlement` (L340-L408, L786-L790).
    - The audit args (L554-L660) follow row 26.
  - `src/documents.rs`: the settle call loses `cost_micro_usd`.
  - Readers: `LedgerRow` (L3336) gains the new columns; `CallRow` (L3164) gains `settled_at`; a new `Reader::spend()` returns one row per call, joined to its step key, attempt, run, seat, tokens and times.
- **`crates/am-engine`:**
  - `src/count.rs` (new): the prompt-token count of a `ChatRequest` (Q2).
  - `src/run.rs`:
    - `NO_RATE_SNAPSHOT` (L130-L132) goes.
    - `RunPlan` gains `money: RunMoney` (rate, timezone, week start).
    - `StepPlan` gains `price: Option<CallPrice>` (`None` only on a gate). `run_on` refuses a non-gate step without one, before any row.
    - The reserve (L1400-L1412) counts, prices and stamps the week from `plan.clock` and the team from `seat_team`.
    - Every `cost_micro_usd: None` goes (Intent's list).
    - The module docs (L101-L103) are updated.
  - `src/journal.rs` (the spike, L29-L43, L177-L190): its `Price` becomes `CallPrice` with a USD `Rate`, so `__spike-run` and the c2 tests keep working.
- **`crates/am-app`:**
  - `src/run.rs` `plan_of` (L412-L548): Account → provider → local or a price (Q8); the rate (Q6); the refusals of rows 13 and 14 in `NOT_STARTED` words.
  - `status` (L963-L1004, `run_line` L854, `step_line` L871): spend on run and attempt lines, then the spend section (Q12).
  - `src/spike.rs`: follows the spike's `Price` change.
- **Templates (Q10):** `templates/prices.yaml` (new; shipped defaults) and `templates/samples/company-a/models/prices.yaml` (new; the sample's cloud fallback models and an INR rate), each with `updated:`.
- **Fixtures:**
  - `fixtures/projects/costs/` (new): `currency: EUR`, timezone `America/New_York`, two teams, a cloud seat and a local seat, two steps, `models/prices.yaml` with `updated:` and an EUR rate.
  - `fixtures/accounts/costs.yaml` (new): one cloud Account, no key.
  - The `justfile` `validate` list gains the fixture if no glob reaches it.
  - `fixtures/golden/so-far/snapshot.txt` L47, L67, L88, and `crates/am-e2e/tests/golden/support/mod.rs` L2069-L2077 (the ledger line's fields).
- **Docs:** `docs/config-schema.md` (`rates`, `updated`).
- **Existing tests that must change (supersession, mechanical):**
  - About 38 `WriteCommand::ReserveCall` literals: am-store tests (`run_journal`, `inbox`, `inbox_resolve`, `prompt_journal`, `rows`, `row_inputs`, `documents`, `audit`), `am-app/tests/gate_cli.rs`, the am-e2e trust and engine tests, the `am-server/src/inbox.rs` test.
  - About 85 `cost_micro_usd` fields, which go.
  - `ledger_for_run` users; the spike tests that read `Price`.

## Tasks & Acceptance

**Execution:**
- [ ] **Before the build:** the owner answers Q1–Q16; 3.6b merged; this plan's baseline set to main after 3.6b; the answers recorded under Design Notes; the plan committed on its own branch.
- [ ] **Tests first (no `src/` change):** the Tests-first list, the fixtures, the shipped price files, and the superseded literals. Log the red run with each test's failure.
- [ ] **am-domain:** `money.rs`, `week.rs`.
- [ ] **am-config:** `rates`, the file hash, the `updated` warning.
- [ ] **am-store:** migration 0013, `ReserveCall`, `settle`, `MarkLost`, the readers, the audit args.
- [ ] **am-engine:** the prompt count, the plan's money, the reserve, the settle commands; the spike's `Price`.
- [ ] **am-app:** `plan_of`'s price and refusals; `status`'s spend.
- [ ] **Golden:** the snapshot's ledger lines, by hand.
- [ ] **Docs:** `docs/config-schema.md`.
- [ ] **Register:** DR-109 closed or retargeted (Q14); the new rows from Design Notes (Deferred) added at triage.
- [ ] **Size:** one squash commit; if the code nears about 4,000 lines, stop and report.

**Acceptance Criteria:**
- Given each matrix row, when its test runs, then ledger rows, calls, audit entries and CLI output match the row.
- Given the ticket's verify, when the costs fixture runs on FakeLlm with set usage:
  - each call's entry is reserved, then settled in both currencies with the rate snapshot id;
  - a call with no usage settles at its reservation;
  - `agentic status` shows the roll-up by step, run, seat, team and week, with the price file's date;
  - a local Account call shows zero cost with tokens and time.
- Given the workspace, when `just check` runs, then it is green, `golden_so_far` included. Merge only with both CI jobs green after review triage.

## Plan Change Log

- 2026-11-25, planner: draft for the owner's money stop. No tests or code until Q1–Q16 are answered. Risk set `high` against the ticket's `medium`: the ledger's shape, a migration with triggers, and every settle command change. The ticket's `risk` field is not edited.
- 2026-11-25, owner: plan approved with the recommended answers, except Q6 (lowering prices or rates is widening, in 4.17) and Q10 (published prices and today's INR rate, dated, checked at review). Baseline `461c14c`.
- 2026-11-25, manager: rebased onto `b234d74` (documents-only replan #47), and the baseline set to it.

## Design Notes

### Owner answers (2026-11-25)

**Owner, verbatim:** "Approve the 3.7 plan with the recommended answers, except Q6: in 4.17, changing a price or rate so costs look lower counts as widening. Q10: use published prices and today's INR rate with a "checked on" date for me to check at review. Sofia and Sally fix the ₹ wording. New standing rules: reports are a 5-line summary plus the question; Opus only for plans and builds on money or security stories, Sonnet for everything else."

**How the plan takes them:**
- **Q1–Q5, Q7–Q9, Q11–Q16:** the recommended answers, as written below.
- **Q6:** the recommended answer for this story: a `rates:` list in `models/prices.yaml`, the file's sha256 as the snapshot id, and a run refused when there is no rate. Changed by the owner: in 4.17, editing a price or a rate so that costs look lower is widening. Sofia records this in 4.17's `is_widening` list through a replan. This story does not build `is_widening` (4.17 does).
- **Q10:**
  - The build fills both shipped price files (`templates/prices.yaml` and `templates/samples/company-a/models/prices.yaml`) with the providers' published prices and today's INR rate.
  - Each file gets a "checked on" date (`updated: 2026-11-25`) and a comment naming the source pages.
  - The owner checks the numbers at review. The manager gathers them, so no subagent browses.
- **₹ wording (Q12, Q16):** Sofia (R2, the epic's Done-when) and Sally (EXPERIENCE.md L84) fix it in a replan. This story's build follows the architecture's no-hard-coded-currency rule either way.
- **Model:** this is a money story, so its plan, tests, build and reviews run on Opus (owner rule, 2026-11-25).

### Owner questions (money)

Each question gives the options, the planner's pick and why. Anything that changes an AD, the ticket or adds a dependency is marked **owner decision**; the planner decided none of them.

**Q1. The reservation formula, and when the price is read.**
- **The formula.** AD-9 says "(counted prompt tokens + `max_output_tokens`) × price"; the price file has separate input and output prices.
  - (a) counted × input price + `max_output_tokens` × output price;
  - (b) (counted + `max_output_tokens`) × the higher price;
  - (c) (a) plus a safety margin.
- **When the price is read.**
  - (i) once, at plan build, from the snapshot the run pins (AD-16), journaled on each reservation;
  - (ii) at each call, from the current files.
- **Pick: (a) and (i).** (a) is AD-9's formula with each token class at its own price. (b) over-reserves output-heavy calls up to five times, which matters once epic 4 checks caps. (ii) breaks AD-16: a run would change prices mid-run, and resume (3.9) could price the same call twice differently.

**Q2. How prompt tokens are "counted".** The Stack has no tokenizer, and AD-9 says "counted".
- (a) characters / 4 (the spike; under-counts code, JSON and non-Latin scripts);
- (b) UTF-8 bytes / 3, rounded up, over every system part, message part and offered tool definition (a file part by its bytes until epic 12's per-type estimate, AD-31);
- (c) bytes / 2 (safer, but reaches caps sooner in epic 4);
- (d) a tokenizer crate (**owner decision**: a new dependency, and none fits every provider);
- (e) the provider's count endpoint (an extra request per call, Anthropic only).
- **Pick: (b).** No dependency and no extra request. It over-reserves English (about 4 characters a token) and is close for Devanagari and CJK, so it seldom under-reserves. A wrong guess moves only the reservation, never the settled spend. **Owner decision:** AD-9's word "counted" reads as "estimated by a fixed rule". Winston may want the AD to say so.

**Q3. Settling when usage is missing, partial, above the reservation, or the call failed.**
- **Missing usage** (no `usage` in the outcome: a provider error, a cancelled stream, a cut-off call with no final usage): the reservation, in both currencies (AD-9 literal).
- **Partial usage** seen only as stream events: ignored. The adapters already return `None` for a stream without final usage (`anthropic.rs` L399-L406), so the call settles at the reservation.
- **Usage above the reservation:**
  - (a) settle at the reported usage and mark the audit `over_reservation`;
  - (b) cap the settlement at the reservation.
  Pick (a): the ledger records what the provider says was spent. (b) under-reports. Epic 4's caps own the consequence; AD-9's "Prevents: settled spend exceeding a cap" is theirs to enforce.
- **A call refused before any byte was sent** (`ProviderError::Unsupported`, which `llm.rs` documents as raised "before any byte is sent"):
  - (a) the reservation, AD-9 literal;
  - (b) zero.
  Pick (a). It over-counts, but it needs no AD change; (b) is an **owner decision** that amends AD-9.

**Q4. Who computes the amounts.**
- (a) The engine computes and passes them (today's `cost_micro_usd` field).
- (b) The reservation journals the terms; the writer computes both the reservation and the settlement from them.
- **Pick: (b).**
  - One computation.
  - The store cannot hold a settlement its own terms contradict.
  - Resume (3.9) and `MarkLost` need no price.
  - The audit can be checked against the terms.
- **Cost:** about 85 mechanical test edits removing `cost_micro_usd: None`.

**Q5. Rounding: which direction, and where.**
- (a) up (ceiling), per call, per currency, from the exact value;
- (b) half to even;
- (c) down (the spike).
- **Pick: (a).** It never under-states a reservation or a settlement. Across a run it over-states by under one micro-unit per call ($0.000001).
- **Where:** in `am_domain::money::cost` only, in i128. The company amount comes from the exact USD value × rate, not from the rounded USD. Overflow is an error, never a wrap or saturation.
- The rate is stored as company micro-units per USD: 0.92 is stored as 920,000, rounded to the nearest micro-unit when the file is read, as `micros()` already does (`fields.rs` L263-L283).

**Q6. The USD-to-company-currency rate: where it comes from, how its snapshot is named, and what happens without one.** No AD names the source.
- **Source:**
  - (a) a `rates:` list in `models/prices.yaml` (`- currency: EUR` / `per_usd: 0.92`), dated by the file's `updated:` and edited like a price;
  - (b) a `usd_rate:` key in `company.yaml`;
  - (c) fetched online (refused: no host for it in AD-12, and none may be added);
  - (d) a table compiled into the app (stale, not editable).
  Pick (a): AD-9 and the Conventions both say "the price file's rate snapshot".
- **Snapshot id:**
  - (a) the price file's sha256, in full hex, or `none` without a file;
  - (b) the run's config hash;
  - (c) a `rate_snapshots` table with UUIDv7 ids.
  Pick (a). It changes only when prices or rates change, needs no table, and each ledger row journals the terms and rate anyway, so the row reads alone.
- **No rate:**
  - (a) refuse, before any run row, a run with a cloud call when the company currency is not USD and the file has no rate for it; local-only runs go ahead at 0;
  - (b) record the company amount as NULL and show "rate unknown".
  Pick (a). Epic 4's caps are in the company currency (seat budgets are company micro-units, `entities.rs` L270-L275), so a NULL would leave them blind.
- **Owner decision:** `rates` is a new config field. AGENTS.md says every new config field is classified in `is_widening`. That is 4.17's, which does not exist yet. Sofia adds `rates` (and price edits) to 4.17's list. The planner's view: neither is widening, since spend accounting is not a privilege.

**Q7. The locale default for the company currency, and how it is stored.**
- The ticket says the company currency "defaults from the system locale".
- The code requires `currency:` in `company.yaml` (`kinds.rs` L387).
- Epic 5 story 16's wizard pre-fills it from the locale ("timezone and currency pre-filled from the Mac's system locale for the owner to confirm").
- Options:
  - (a) this story reads `company.yaml`'s `currency` and stores it on every ledger row; the locale default stays the wizard's (5.16);
  - (b) add a locale port now, with a macOS adapter, and default a missing `currency` from it.
- **Pick: (a).** (b) would need either a new crate or a `defaults read -g AppleLocale` child plus a region-to-currency table. It would make the config snapshot differ between Macs (AD-16) and would skip the wizard's confirmation.
- **Owner decision:** whether the ticket's parenthesis is met by 5.16. If so, Sofia may reword it. The planner does not edit it.

**Q8. Local and free providers.**
- **Which calls are local** (zero in both currencies, `local = 1`, tokens and time shown):
  - a reserved local Account (`local-lmstudio`, `local-ollama`);
  - an Account whose provider is `lmstudio` or `ollama`;
  - an `openai-compatible` Account whose `base_url` host is `127.0.0.1` or `localhost` (EXPERIENCE.md: "On this Mac · 127.0.0.1:8080").
- **Local calls are 0 even if the price file prices them** (Conventions, Money).
- **A free cloud allowance** is priced at whatever the owner writes, `0.0` included. It is not marked `local`.
- **A cloud model with no price:**
  - (a) refuse the run before any row (row 13);
  - (b) price it at 0;
  - (c) a default price.
  Pick (a). (b) hides spend from caps, and (c) is an invented number.
- **Lookup key:** the existing schema's (provider, model). Two `openai-compatible` Accounts therefore share a price for the same model name. Keying by Account id instead is an **owner decision** (a schema change).

**Q9. Whether reasoning or cached tokens are priced.**
- (a) Price what the adapters report:
  - reasoning tokens inside output (Anthropic's `output_tokens` and OpenAI's `completion_tokens` include them);
  - cached prompt tokens at the full input price (OpenAI's `prompt_tokens` includes them; the cache discount is not applied).
- (b) Add reasoning and cache fields to `Usage` and new price fields.
- **Pick: (a).** v1 sends no cache markers (prompt caching is v1.x, architecture Deferred). OpenAI's automatic cache only lowers the true cost, so (a) over-states and never under-states. (b) changes `Usage`, both adapters and the price schema for little gain now.

**Q10. The "updated date" on the shipped price files.**
- **Which files:** `templates/prices.yaml` (the Structural Seed's shipped defaults) and `templates/samples/company-a/models/prices.yaml`.
  - The sample's file names its cloud fallback models: from the sample's seats, provider `anthropic` with models `claude-sonnet-5-5` and `claude-haiku-5-5`, and `openai` with `gpt-5-mini`.
  - It also needs an INR rate.
- **The numbers and the date** are owner input:
  - (a) the owner supplies the prices, the INR rate and the date they were checked;
  - (b) the build checks the providers' published pages on build day, writes that date, and the owner confirms in review.
  Pick (a). The planner cannot verify prices.
- **How the shipped file is used:**
  - (a) a starting copy, which the wizard (5.16, 5.12) copies into a new project;
  - (b) layered under every project's file at load.
  Pick (a). (b) puts bytes the snapshot hash does not cover into the run's prices (AD-16), so a new app release would change a pinned run's prices.
- **`updated:` in owner projects:**
  - (a) stays optional, with a validate warning when a file prices any model without it, and status says "date not given";
  - (b) required (breaks existing projects).
  Pick (a). A test fails the build if a shipped file lacks it.

**Q11. How the ISO week is computed in company time.**
- **The instant:** the reservation's, read from the run's `Clock` (FakeClock in tests). The writer's own stamp is the system clock (`lib.rs` L251) and cannot be faked.
- **Where the week is fixed:**
  - (a) journaled on the ledger row at reserve, computed by the engine in the pinned snapshot's `timezone` and `week_start`;
  - (b) computed when status reads, in the current config's timezone.
  Pick (a):
  - status needs no config;
  - epic 4's "company week" cap can sum one indexed key in the writer;
  - a later timezone edit does not move past spend.
- **The key:** the company-local date the week starts, for example `2026-11-23`. Status labels it `2026-W48` (ISO week-year from jiff) when that date is a Monday, and `week from <date>` otherwise.
- **Owner decision:** the Conventions allow a `week_start` other than Monday, so the R2 and ticket phrase "ISO week" is exact only for Monday. Pick: the label rule above.

**Q12. What `agentic status` shows, including an unknown currency or rate.** Proposed, for owner approval:
```
<run-id>  pair  succeeded  snapshot 1a2b3c4d5e6f  2.104s  EUR 0.004554 · USD 0.00495 · 2,100/160 tokens
    draft  attempt 1  succeeded  1.042s  EUR 0.004554 · USD 0.00495 · 1,200/90 tokens · 1 call
    check  attempt 1  succeeded  0.812s  EUR 0 · USD 0 · 900/70 tokens · 1 call · local
spend, settled (prices updated 2026-11-01, rate snapshot 3f9c0a1b2c4d: 1 USD = 0.92 EUR)
  week 2026-W48 (from 2026-11-23)  EUR 0.004554 · USD 0.00495 · 2,100/160 tokens · 1.854s
  team core   EUR 0.004554 · USD 0.00495 · 1,200/90 tokens · 1.042s
  team side   EUR 0 · USD 0 · 900/70 tokens · 0.812s · local
  seat writer EUR 0.004554 · …
  reserved, not settled: EUR 0.059285 · USD 0.06444 (1 call)
```
- **Format:**
  - the ISO code, then the amount with up to six decimals, trailing zeros trimmed, `0` when zero;
  - no symbol and no locale grouping (Conventions: symbols only in the UI);
  - token counts grouped with commas.
- **Price file absent:** "no price file: local calls only".
- **No `updated:` date:** "prices date not given".
- **Pre-0013 rows** (unknown company amount): `EUR -`, with a footnote "recorded before company-currency settlement".
- **Rows in two currencies:** one subtotal per currency (`EUR 1.20 + INR 30`), never converted.
- **Open reservations:** listed apart from settled spend, never folded into it.
- **The rate is never unknown** for a row this story writes (Q6 refuses the run).
- **Owner decision:** EXPERIENCE.md L84 ("INR by default, with ₹") and R2 and the epic's Done-when ("₹0") predate the 2026-11-19 amendment. That amendment says the currency comes from the locale and no ₹ is hard-coded. The planner follows the architecture and the initiative and flags this wording for Sofia and Sally. Nothing is edited.

**Q13. The ledger's shape (migration 0013).**
- **Columns:** additive, as in the Code Map, so the row holds its formula's inputs and the seat's team. AD-43 forbids storing lineage twice; the team is not lineage, and no other table holds it.
  - **Alternative:** store `seat` on the ledger too. Pick: no. `step_runs.seat`, set at start since 3.6, already holds it.
- **Old rows:**
  - (a) left NULL in the new columns (shown as `-`, Q12);
  - (b) back-filled: company amount 0 where USD is 0.
  Pick (a). It writes no user data, and every shipped pre-3.7 row is 0 anyway.
- **Integrity triggers** (row 24):
  - no `DELETE`;
  - reserve-time columns never change;
  - the settled columns are set once, together with `state` going from `reserved` to `settled`;
  - amounts are never negative;
  - a `local` row's amounts are 0.
  Pick: add them. Money rows are as append-only as 3.5's and 3.6's journals.

**Q14. DR-109 (unassigned, money): a store error leaves the open call's reservation unsettled.**
- (a) Fold in: the engine's cleanup after a store error (`run.rs` L938-L966) also sends `MarkLost` for the open call, once, like its `FinishStep`. About 20 lines and one test.
- (b) Leave it unassigned. Status still shows it as "reserved, not settled" (row 22).
- **Pick: (a).** Otherwise a crash-free run keeps a permanent reservation, which epic 4's meter would count as spend.

**Q15. Team attribution.** The ledger journals the step's `seat_team` at reserve; the seat comes through `step_runs.seat`. A seat moved to another team later keeps its past spend under the old team. Pick: yes, it is what happened. The alternative, the current team from config at read, needs config in status.

**Q16. The ticket wording, for Sofia (no edit here).**
- The locale clause (Q7).
- "ISO week" with a non-Monday `week_start` (Q11).
- "₹0" in R2 and the Done-when (Q12).
- "the shipped prices.yaml and models/prices.yaml": neither exists, and this story creates both (Q10).
- The owner decides whether Sofia rewords any of them. The build follows the answers above either way.

### Design decisions within the ADs (planner, under the standing rules)

- **Test structure:**
  - the money and week functions are tested through am-domain's public API in `tests/`;
  - "reserved before the call" is shown by a test provider in am-e2e that reads the ledger inside `stream`, plus the audit order;
  - roll-ups are tested through `agentic status`.
- **The prompt count** lives in am-engine (`count.rs`), because the request is assembled there. The money arithmetic lives in am-domain (Structural Seed: "Money, policies").
- **No `schema_version` bump** for `rates`: an optional additive key, as 1.10's rules allow. If they do not, the build follows them and says so.
- **No validate warning for unpriced cloud models:** validate would need the user Accounts' providers in every caller. The run refuses instead (row 13). Recorded as a register row.

### Deferred (to add to the register at triage)

- **3.8:** the fallback wrapper (epic 2 story 6) may switch models inside one provider call. 3.8 must reserve and settle at the serving model's price, with each retry its own entry. Target 3.8; must close yes.
- **Epic 12 (Folder access):** a file part is counted by its bytes until the capabilities file's per-type token estimate is used (AD-31). Target: epic 12's file-parts story.
- **`agentic validate` warns of an unpriced cloud model or a missing rate.** Target: unassigned; must close no.
- **Epic 4 story 2:** the Assistant spend scope needs `ledger.model_call_id` and `run_id` to allow NULL, or a sibling table; this story keeps them `NOT NULL`. Target 4.2.

### Register entries folded in

- **DR-109** (unassigned, money): folded in if Q14 is (a); otherwise row 22 shows it.
- **No entry targets 3.7.** No other unassigned entry touches money. DR-023 (`MarkLost` on a settled call audits `ok`) stays 3.18's.

### Size (owner: code and test lines separately; stop at about 4,000 lines of code)

- **Code: about 1,250 to 1,650 lines.** Production is about 1,050 to 1,350; in-crate `#[cfg(test)]` is about 200 to 300.
  - am-domain `money.rs` and `week.rs`: about 220.
  - am-config `rates`, the hash and the warning: about 120.
  - am-store (migration 0013 with triggers about 90; the reserve, settle, `MarkLost`, readers and audit args about 330): about 420.
  - am-engine (count, plan money, reserve, settle edits, spike): about 230.
  - am-app (`plan_of` prices and refusals about 150; status spend about 250): about 400.
  - Templates, fixtures, docs and golden lines: about 120.
- **Tests: about 1,700 to 2,200 lines.**
  - New: `money.rs` about 220; `prices.rs` about 140; `ledger.rs` about 550, with its version-12 migration fixture; `cost_ledger.rs` about 400; `status_spend.rs` about 380.
  - Churn: about 200 to 300 lines across about 40 files (the `ReserveCall` literals and the removed `cost_micro_usd`).
- **Well under the stop.** No split is proposed. If the owner takes Q2 (d), Q9 (b) or Q8's Account-keyed prices, add about 150 to 300 lines of code each.

### Tests-first list

All on FakeLlm and FakeClock; no network; no real model. Names are the planner's; the tester may adjust them under the standing rules.

**am-domain (`crates/am-domain/tests/money.rs`, new). Red for all: no `am_domain::money` or `am_domain::week` module.**
- `a_cost_rounds_up_once_per_currency` (rows 1, 2, 15).
- `the_company_amount_comes_from_the_exact_usd_value` (row 15).
- `an_overflowing_cost_is_refused` (row 15).
- `a_local_price_costs_nothing` (row 5).
- `the_week_turns_at_monday_midnight_across_dst` (row 17).
- `the_iso_week_year_holds_at_the_year_boundary` (row 18).
- `a_sunday_week_has_no_iso_label` (row 19).

**am-config (`crates/am-config/tests/prices.rs`, new).**
- `rates_parse_with_currency_and_per_usd` (row 27). Red: unknown key `rates`.
- `a_bad_rate_is_an_error_with_line_and_fix` (row 27). Red: unknown key `rates`.
- `a_price_file_without_a_date_warns` (row 27). Red: no warning today.
- `the_shipped_price_files_carry_their_date` (rows 27, Q10). Red: neither file exists.
- `the_prices_carry_their_file_hash` (row 11). Red: `Prices` has no `sha256` field.

**am-store (`crates/am-store/tests/ledger.rs`, new).**
- `a_reservation_journals_both_amounts_and_its_terms` (row 1). Red: `ReserveCall` takes no `price`.
- `usage_settles_both_currencies_at_the_reserved_terms` (row 2). Red: as above.
- `missing_usage_settles_at_the_reservation_in_both_currencies` (row 3). Red: as above.
- `usage_above_the_estimate_settles_as_reported` (row 4). Red: as above.
- `a_local_reservation_is_zero_with_tokens_and_time` (row 5). Red: no `local` column and no `settled_at`.
- `a_lost_call_settles_both_currencies_at_its_reservation` (row 10). Red: no company columns.
- `the_ledger_refuses_changes_to_what_was_reserved` (row 24). Red: no triggers.
- `reserve_and_settle_audit_amounts_never_text` (row 26). Red: no new args.
- The four migration tests in 3.6b's pattern (row 25). Red: `SCHEMA_VERSION` is 12.
  - `migration_0013_keeps_every_row_and_the_pre_migration_copy`
  - `migration_0013_adds_the_cost_columns_null_on_old_rows`
  - `reopening_a_version_13_database_applies_nothing`
  - `a_failing_0013_names_the_backup_and_leaves_version_12`

**am-e2e engine (`crates/am-e2e/tests/engine/cost_ledger.rs`, new `[[test]] cost_ledger`). Red for all: every reservation is 0 with snapshot `none`.**
- `each_call_is_reserved_before_it_is_made_then_settled` (rows 1, 2, 26: a test provider reads the ledger inside `stream`).
- `a_call_with_no_usage_settles_at_its_reservation` (row 3).
- `a_local_call_costs_nothing_and_keeps_tokens_and_time` (row 5).
- `a_failed_call_settles_at_its_reservation` (row 7).
- `a_repair_call_is_reserved_from_its_own_request` (row 8).
- `the_week_is_the_run_clocks_in_company_time` (rows 17, 18).
- `an_edited_price_file_gives_the_next_run_a_new_snapshot` (row 11).
- `a_prompt_twice_as_long_reserves_more` (row 16).
- `the_store_error_cleanup_settles_the_open_call` (Q14, if (a)).

**am-app (`crates/am-app/tests/status_spend.rs`, new). Red for the status tests: `status` prints no spend. Red for the refusals: `plan_of` prices nothing.**
- `status_rolls_up_by_step_and_run` (row 20).
- `status_rolls_up_by_seat_team_and_week_with_the_price_date` (row 21).
- `status_shows_a_local_call_at_zero_with_tokens_and_time` (row 5).
- `status_shows_open_reservations_apart` (row 22).
- `status_shows_old_rows_and_mixed_currencies_honestly` (row 23).
- `a_project_without_prices_runs_local_seats` (row 6).
- `a_usd_company_needs_no_rate` (row 12).
- `an_unpriced_cloud_model_is_refused_before_any_row` (row 13).
- `a_missing_rate_is_refused_before_any_row` (row 14).

**Golden (`crates/am-e2e/tests/golden/golden_so_far.rs`, unchanged code):**
- The snapshot's three ledger lines and the support's ledger line format (row 28).
- Red: the expected lines name `local`, INR and the snapshot id, which the run does not write yet.

**Superseded literals:** the `ReserveCall` and `cost_micro_usd` sites in the Code Map. They change in the test commit, so the red run shows only this story's reasons.

## Verification

- `cargo nextest run`, filters first:
  - am-domain `money`;
  - am-config `prices`, `validate`;
  - am-store `ledger`, `run_journal`, `documents`, `audit`;
  - am-engine `run`, `journal`;
  - am-e2e `cost_ledger`, `golden_so_far`, `rule_16_audit_log`, `c2_crash_resume`;
  - am-app `status_spend`, `run_cli`, `gate_cli`.
- `cargo xtask check-deps` and `cargo xtask denylist`: unchanged rules and green. The new fixtures and templates hold no sample id in `crates/`.
- `agentic validate` over `templates/` and `fixtures/`: green, the new price files and fixture included.
- `just check` at the end.
- No `cargo clean`, and no build of a workspace copy against the shared target (AGENTS.md, owner 2026-11-23). No `am-desktop` build.
- Merge only with both CI jobs green after review triage.

## Implementation Notes

### Published prices for the shipped files (manager, 2026-11-25; owner Q10)

The manager read these on 2026-11-25. The owner checks them at review. The build writes them into `templates/prices.yaml` and `templates/samples/company-a/models/prices.yaml` with `updated: 2026-11-25` and a comment naming each source page.

| Provider | Model | Input USD per 1M tokens | Output USD per 1M tokens | Source |
|---|---|---|---|---|
| anthropic | claude-opus-5-5 | 4.00 | 20.00 | platform.claude.com/docs/en/about-claude/pricing |
| anthropic | claude-sonnet-5-5 | 2.00 | 10.00 | same |
| anthropic | claude-haiku-5-5 | 0.10 | 0.50 | same (prompts up to 100,000 tokens) |
| openai | gpt-5-mini | 0.25 | 2.00 | developers.openai.com/api/docs/pricing (Standard) |

- **Rate:** 1 USD = 92.292247 INR. Mid-market rate from x-rates.com at 12:34 UTC on 25 Nov 2026. Written as `rates: [{ currency: INR, per_usd: 92.292247 }]` in the sample's file only. `templates/prices.yaml` carries no rate, because the company currency is the owner's.
  - **Owner, at review (2026-11-25):** "Model prices are approved. Change the sample's INR rate to 95.0 (deliberately rounded up, so INR spend is never understated), and note that in the file's source comment in templates/prices.yaml." The sample's file now carries `per_usd: 95.0`. The golden snapshot's rate-snapshot id moved from `6433b3384aa4` to `1323f85eb009`, the sha256 of the changed file.
- **Models per file:**
  - `templates/prices.yaml`: all four models.
  - The sample's file: the three models its seats name (`claude-sonnet-5-5`, `claude-haiku-5-5` and `gpt-5-mini`), plus the INR rate.
- **Haiku 5.5's long-prompt tier, for the owner at review:** a prompt over 100,000 tokens costs $0.50 input and $2.50 output. The price schema has one price per model, so the base price under-states such a call. The other Q9 choices over-state rather than under-state. The build writes the base price with a comment naming the tier and records a register row for a tiered price. The owner may choose the higher tier instead.
- **Not priced:** cache reads and writes, batch, fast mode, priority, and US-only inference. Q9: v1 sends none of these.

### M5 decided by endpoint (implementer, 2026-11-25)

*Implementer, 2026-11-25: M5 decided by endpoint (the manager's decision in review round 1).*
- A call is local (0 in both currencies) only on a reserved local Account (`local-lmstudio`, `local-ollama`), or on an Account of this Mac's Accounts file whose `base_url` is a server on this Mac: the loopback form `http://127.0.0.1:<port>` that `am_config::compatible_base_url` accepts.
- The provider's name never decides. The `"lmstudio" | "ollama"` arm is gone: neither is an Account provider, so it was unreachable from config.
- An Accounts file with an error decides nothing for an Account that is not reserved. A seat on such an Account is refused before any row, naming the file's first error; an id listed twice is never decided by first match.
- The unit test is `prices::tests::a_call_is_local_only_on_a_reserved_account_or_a_loopback_endpoint` (am-app, in-crate).
- The owner is told the rule was tightened from Q8's wording to its intent ("on this Mac").

### API the tests fix (tester, 2026-11-25)

*Tester, on Opus, after the owner's answers. The planner's proposal stands; every name it left open is chosen here and marked "tester, 2026-11-25". Prices and rates in tests and fixtures are fake (3.00/15.00 USD per million, 0.92 EUR per USD, 80 or 83.5 INR per USD), never the published ones.*

**am-domain** (`crates/am-domain/tests/money.rs`):
- `am_domain::money`:
  - `CallPrice { input_micro_usd_per_mtok: i64, output_micro_usd_per_mtok: i64, local: bool }` (Debug, Clone, PartialEq, Eq). `CallPrice::local()` is 0, 0, `true` (tester, 2026-11-25).
  - `Rate { currency: String, company_micro_per_usd: Option<i64>, snapshot: String, prices_updated: Option<String> }` (Debug, Clone, PartialEq, Eq). *Tester, 2026-11-25:* the rate is an `Option`, `None` when the price file has no rate for the company currency, so a local-only run in EUR with no rate (rows 6, 14) is expressible. A USD company carries `Some(1_000_000)`. `Rate::none(currency)` is the rate of a run with no price file: no rate, snapshot `none`, no date.
  - `Money { usd_micro: i64, company_micro: i64 }` (Debug, Clone, Copy, PartialEq, Eq).
  - `MoneyError::{Overflow, NoRate}`, with Display words (tester, 2026-11-25).
  - `cost(price: &CallPrice, rate: &Rate, input_tokens: u64, output_tokens: u64) -> Result<Money, MoneyError>`:
    - a `local` price is 0 and 0, whatever its terms and the rate;
    - otherwise N = in × input price + out × output price, in i128; USD = ⌈N / 10⁶⌉; company = ⌈N × rate / 10¹²⌉, from N, never from the rounded USD;
    - `Overflow` for any product or amount past i64; `NoRate` for a priced call with no rate.
- `am_domain::week`:
  - `BudgetWeek { start: jiff::civil::Date }` (Debug, Clone, Copy, PartialEq, Eq);
  - `key() -> String` (`YYYY-MM-DD`, the ledger's `week`);
  - `iso_label() -> Option<String>` (`2026-W48`, only when `start` is a Monday);
  - `budget_week(at: jiff::Timestamp, tz: &jiff::tz::TimeZone, week_start: jiff::civil::Weekday) -> BudgetWeek`.
  - The seven tests were checked once against a throw-away reference implementation (a temporary test file, deleted): all seven pass, so the DST, ISO week-year and rounding numbers are consistent.

**am-config** (`crates/am-config/tests/prices.rs`):
- `models/prices.yaml` gains `rates:`, a list of `{ currency, per_usd }`. *Tester, 2026-11-25:* stored as `CurrencyRate { currency: String, micros_per_usd: i64 }`, named like `ModelPrice`'s `*_micros_*` fields.
- **Errors**, each at its own line with a fix, the bad entry not loaded:
  - a code that is not three capitals;
  - `per_usd` missing, zero, negative or not a number;
  - a currency listed twice (the error is at the second entry; the first is kept);
  - a `USD` entry;
  - an unknown key in an entry.
- **Warning** naming `updated`: a file that prices at least one model and has no `updated:`. A file that prices no model gets none.
- `Prices { rates: Vec<CurrencyRate>, sha256: Option<String>, .. }`. `sha256` is the lowercase hex of the file's bytes as loaded, and `None` with no file.
- `am_config::parse_prices(text: &str) -> (Prices, Vec<Finding>)`: findings name `models/prices.yaml`; `sha256` is the text's (tester, 2026-11-25).
- **Shipped files** (the implementer writes them):
  - `templates/prices.yaml`: carries no `rates`.
  - Each `templates/samples/*/models/prices.yaml`: carries the rate of its company currency, and prices every model its seats name on a cloud Account of `fixtures/accounts/sample.yaml`.
  - Both parse with no error, carry a valid `updated:` date, and price each model above 0.
  - A source comment is allowed: its "comments are not kept" warning is not checked.

**am-store** (`crates/am-store/tests/ledger.rs`, and the churn below):
- **`WriteCommand::ReserveCall`** is `{ step_run_id, call_id, call_seq, price: CallPrice, rate: Rate, counted_input_tokens: u64, max_output_tokens: u32, week: String, team: String, taint, purpose, prompt }`.
  - `reserved_micro_usd` and `rate_snapshot` are gone; the snapshot is `rate.snapshot`.
  - The writer computes both amounts with `am_domain::money::cost`.
  - A cost the function refuses (overflow, no rate) refuses the command, with nothing written.
- **The settle commands carry no `cost_micro_usd`:**
  - `SettleCall { call_id, usage }`;
  - `SettleAndFinishStep { call_id, usage, step_run_id, status, output }`;
  - `SettleAndFinishDocumentStep { call_id, usage, step_run_id, output, version_id, document_id, expected_version, item }`;
  - `SettleAndLoopBack { call_id, usage, step_run_id, output, to }`;
  - `CallSettlement { call_id, usage }`.
  - Usage settles at the terms journaled at reserve. No usage settles at the reservation in both currencies. Each stamps `settled_at`; `MarkLost` does too.
- **Migration 0013** (`SCHEMA_VERSION` 13; reported "from version 12 to 13"):
  - `ledger` gains `currency`, `reserved_micro_company`, `settled_micro_company`, `input_micro_usd_per_mtok`, `output_micro_usd_per_mtok`, `company_micro_per_usd`, `prices_updated`, `counted_input_tokens`, `max_output_tokens`, `local` (0/1), `week`, `team`.
  - `model_calls` gains `settled_at`.
  - All of them are NULL on old rows.
- **Triggers:**
  - A reserve-time column never changes: `id`, `model_call_id`, `run_id`, `kind`, `reserved_micro_usd`, `rate_snapshot`, `created_at` and the twelve new ones.
  - The settled columns are set once, with `state` going `reserved` → `settled`. A row whose reservation has a company amount settles both amounts together.
  - A row is never un-settled.
  - No amount is negative, and a `local` row's amounts are 0.
  - No `DELETE` and no `INSERT OR REPLACE`.
  - An old row (company NULL) still settles in USD; resume's `MarkLost` needs this.
- **Readers:**
  - `LedgerRow` gains `rate_snapshot: String`, `currency`, `prices_updated`, `week`, `team` (`Option<String>`), `reserved_micro_company`, `settled_micro_company`, `input_micro_usd_per_mtok`, `output_micro_usd_per_mtok`, `company_micro_per_usd`, `counted_input_tokens`, `max_output_tokens` (`Option<i64>`), `local: Option<bool>` and `created_at: String`.
  - `CallRow` gains `created_at: String` and `settled_at: Option<String>`.
  - `Reader::spend() -> Result<Vec<SpendRow>, StoreError>` returns one row per call of every run: oldest run first, then step `seq`, attempt and `call_seq` (tester, 2026-11-25). `SpendRow` holds:
    - the call and where it ran: `model_call_id`, `run_id`, `workflow`, `step_run_id`, `step_key`, `attempt: i64`, and `seat: Option<String>` (from `step_runs.seat`);
    - the ledger row: `team`, `week`, `local`, `state`, `currency`, `reserved_micro_usd: i64`, `settled_micro_usd`, `reserved_micro_company`, `settled_micro_company`, `rate_snapshot: String`, `prices_updated` and `company_micro_per_usd`;
    - the call's tokens and times: `input_tokens`, `output_tokens`, `created_at: String` and `settled_at: Option<String>`.
- **Audit (row 26):**
  - `model_call.reserved` holds exactly `call_seq`, `company_micro_per_usd`, `counted_input_tokens`, `currency`, `input_micro_usd_per_mtok`, `local`, `max_output_tokens`, `output_micro_usd_per_mtok`, `prices_updated`, `purpose`, `rate_snapshot`, `reserved_micro_company`, `reserved_micro_usd`, `step_run_id`, `taint`, `team` and `week`. `local` is 0 or 1, like `taint` (tester, 2026-11-25).
  - `model_call.settled` holds exactly `at_reservation`, `currency`, `input_tokens`, `output_tokens`, `over_reservation`, `settled_micro_company` and `settled_micro_usd`. `at_reservation` and `over_reservation` are JSON booleans (tester, 2026-11-25); `over_reservation` means the settled USD is above the reserved USD.
  - The combined `+step.finished` and `+step.looped_back` entries carry the same keys besides their own.
  - No entry has `cost_micro_usd`, and no new `*_sha256` key appears.
  - `model_call.lost`'s arguments are not pinned.

**am-engine** (`crates/am-e2e/tests/engine/cost_ledger.rs`, and the churn below):
- **`StepPlan.price: Option<CallPrice>`:** `run_plan` refuses a non-gate step with `None` before any row, naming the step. The error variant is the implementer's. A gate may have `None`.
- **`RunPlan.money: am_engine::run::RunMoney { rate: Rate, timezone: jiff::tz::TimeZone, week_start: jiff::civil::Weekday }`** (tester, 2026-11-25).
- **The count (Q2)** is pinned by differences, with a floor of ⌈text bytes / 3⌉ (`count.rs`'s signature is not pinned):
  - 3,000 more bytes in the instruction or the system part count exactly 1,000 more tokens (1,000 three-byte `€` too) and reserve 3,000 micro-USD more;
  - a 3,000-byte plan input counts at least 1,000 more;
  - an offered tool counts more.
- **The week** is the run clock's instant at each reservation, in `money.timezone` and `money.week_start`. A provider that moves the clock between two calls puts them in two weeks.
- **The team** is the `StepPlan.seat_team`.
- **Q14:** after a store error, the cleanup sends `MarkLost` for the open call, exactly once.
- **The spike:** `LinearWorkflow { price: CallPrice, rate: Rate, .. }`, and `am_engine::journal::Price` goes. `am_app::spike::spike_workflow` and `anthropic_price` follow; that is the implementer's `src/`.

**am-app** (`crates/am-app/tests/status_spend.rs`; row 23 is in `cost_ledger.rs`):
- `agentic run` reads this Mac's Accounts (`places.user.accounts_file()`) for each seat's Account provider: local, or the (provider, model) price in the pinned snapshot.
- **Refusals** exit 1 before any run row, with "No run was started" and no model called:
  - row 13 names the seat, the Account, the model and `models/prices.yaml`;
  - row 14 names the currency, `rates` and `models/prices.yaml`.
- **The `agentic status` format** follows Q12:
  - Each amount is its ISO code and the amount in units with at most six decimals, trailing zeros trimmed, `0` when zero.
  - The company currency comes first, then USD, joined by ` · ` with the tokens (`in/out tokens`, grouped with commas). No currency symbol.
  - The run line holds the sum of its calls. The attempt line adds `· N call` and, for this Mac, `· local`.
  - The spend section opens with `spend, settled (prices updated <date>, rate snapshot <first 12 hex>: 1 USD = 0.92 EUR …)`, or with `no price file: local calls only`.
  - Then come the lines `week 2026-W48 (from 2026-11-23)`, `team <id>` and `seat <id>`. Each holds both amounts, the tokens and ` · <time>s`, and ends with `· local` for calls on this Mac.
  - The open reservations follow: `reserved, not settled: EUR 0.059285 · USD 0.06444 (1 call)`.
  - A pre-0013 row shows `- · USD …` and the footnote `recorded before company-currency settlement`.
  - Two company currencies give `EUR … + INR …`, never converted; USD sums every row.
- **Fixtures:**
  - `fixtures/projects/costs/` (EUR, America/New_York, teams `core` and `side`; seat `writer` of `core` on the Account `cloud`, model `example-large`; seat `checker` of `side` on `local-lmstudio`; workflow `pair` with `draft` then `check`; `models/prices.yaml` dated 2026-11-01 with the fake price and an EUR rate of 0.92).
  - `fixtures/accounts/costs.yaml` (one Account, `cloud`, provider `anthropic`, no key).
  - `just validate`'s list gains `fixtures/projects/costs/`.

**Golden (row 28):** the support's ledger line is now `ledger call <n> <local|priced> reserved <cur> <company> USD <usd> settled <cur> <company> USD <usd> <state>, rate snapshot <first 12 hex>`. `fixtures/golden/so-far/snapshot.txt` is not edited here. Under 3.24's rule, the implementer moves L47, L67 and L88 by hand, each to `ledger call 1 local reserved INR 0 USD 0 settled INR 0 USD 0 settled, rate snapshot <first 12 hex of the sha256 of templates/samples/company-a/models/prices.yaml>`. No other golden line changes.

**Test structure changes (tester, 2026-11-25):**
- `a_prompt_twice_as_long_reserves_more` is renamed `a_longer_prompt_reserves_more_by_the_rule`: it tests row 16's 3,000-byte difference.
- Row 23's `status_shows_old_rows_and_mixed_currencies_honestly` is in am-e2e `cost_ledger.rs`, not am-app's `status_spend.rs`. It writes pre-0013 rows into a version-12 database by hand, and am-app's tests have no SQLite library. Adding `rusqlite` as an am-app dev-dependency was not taken.
- **Added beyond the list:**
  - am-store: `spend_reads_one_row_per_call_with_its_step_seat_team_and_week`, which pins the reader;
  - am-e2e: `a_step_without_a_price_is_refused_before_any_row` (the Code Map's `run_on` refusal) and `a_large_reservation_is_never_refused` (row 29).

**Churn, done in the tests (mechanical; they fail to compile for this story's reason):**
- **24 `ReserveCall` literals** with `0`/`"none"` now carry `price: CallPrice::local()`, `rate: Rate::none("EUR")`, `counted_input_tokens: 0`, `max_output_tokens: 0`, `week: "2026-11-23"` and `team: "core"`. The files: am-store `run_journal`, `inbox`, `inbox_resolve`, `prompt_journal`, `rows`, `row_inputs` and `documents`; am-app `gate_cli`; am-e2e `rule_15_inbox`, `rule_16_audit_log`, `taint_at_rest` and `handoff_gate_resolve`.
- **2 priced literals** (am-store `audit`, am-e2e `rule_16_audit_log`) carry the fake cloud terms and EUR rate. `audit.rs`'s exact `model_call.settled` arguments now read 138 micro-USD and 127 micro-EUR, and its reservation reads 64,440 and 59,285.
- **34 `cost_micro_usd` fields are removed** from the same files and am-engine `run.rs`.
- **The plans:** `StepPlan.price` and `RunPlan.money` are added in am-e2e `tests/engine/support/mod.rs` (`step()` and `plan()`, with a new `support::money()`: EUR, no rate, UTC, Monday) and in am-engine `tests/run.rs`, `loop_caps.rs` and `toolgate.rs`.
- **The spike:** am-engine `tests/journal.rs` and am-e2e `spike/c2_crash_resume.rs`.

**Left to the implementer, because they are in `src/`:**
- am-store `src/lib.rs`'s `#[cfg(test)]` module: the `ReserveCall` literals at L3677, L3786, L3850 and L4051, and `cost_micro_usd` at L3698, L3707, L3730, L3803, L3866, L3877 and L4068. Three of them settle `Some(40)`, so those tests must be re-expressed through usage and terms.
- am-server `src/inbox.rs`'s test: L560 and L577.
- am-engine `src/run.rs`'s `#[cfg(test)]` module: the plans at L2392, L2432 and L2565, `ReserveCall` at L2609, and `cost_micro_usd` at L2630.
- am-app `src/run.rs`'s `plan_of_runs_on_the_contexts_clock`, if `plan_of`'s signature changes.
- The shipped `src/` sites in the Code Map.

### Red run

*Tester, 2026-11-25, on Opus, in the real working tree with the shared cache, on `story/3-7` at 43a60bc. No `cargo clean`, no copy, no git change, no `am-desktop` build, no migration file. No `src/` change, not even a doc comment.*

**Commands:**
```
cargo nextest run --no-fail-fast            (baseline, before any edit: 1,680 passed, 8 skipped)
cargo fmt --all
cargo test -p am-domain --test money --no-run; cargo test -p am-config --test prices --no-run
cargo build -p am-store --tests --keep-going
cargo build -p am-engine -p am-app --tests --keep-going
cargo build -p am-e2e --tests --keep-going
cargo nextest run --workspace --exclude am-desktop --exclude am-domain --exclude am-config --exclude am-store --exclude am-engine --exclude am-app --exclude am-e2e --no-fail-fast
cargo nextest run -p <am-domain | am-config | am-store | am-engine | am-app | am-e2e> --lib [--bins] --test <every target that compiles> --no-fail-fast
cargo test --doc -p am-store -p am-domain
cargo xtask check-deps; cargo xtask denylist; cargo fmt --all -- --check
agentic validate --template fixtures/projects/costs
```

**Every compile error is this story's.** The build logs were filtered for any error that names something other than the pinned API: `am_domain::money`, `am_domain::week`, `RunMoney`, the new fields of `ReserveCall`, `StepPlan`, `RunPlan`, `LinearWorkflow`, `LedgerRow` and `CallRow`, the missing `cost_micro_usd`, `Reader::spend`, and the `SCHEMA_VERSION > 12` assertion. None was found. Before that, the seven am-domain tests passed against a throw-away reference implementation in a temporary test file, which was then deleted.

**The new targets do not compile (45 tests):**

| Target | Tests | Red because |
|--------|-------|-------------|
| am-domain `money` | 7 | no `am_domain::money` or `am_domain::week` (E0432) |
| am-config `prices` | 5 | no `am_config::CurrencyRate` or `parse_prices` (E0432); no field `rates` or `sha256` on `Prices` (E0609) |
| am-store `ledger` | 13 | no `am_domain::money` (E0432, E0433); `ReserveCall` has no field `price`, `rate`, `counted_input_tokens`, `max_output_tokens`, `week` or `team` (E0559); the settle commands still need `cost_micro_usd` (E0063); no new field on `LedgerRow` or `CallRow` (E0609, E0560); no `Reader::spend` (E0599); `const { assert!(SCHEMA_VERSION > 12) }` fails at build (E0080) |
| am-e2e `cost_ledger` | 12 | through the support: no `am_domain::money` or `am_engine::run::RunMoney` (E0432), no field `price` on `StepPlan` or `money` on `RunPlan` (E0560); its own: E0609 on `LedgerRow` and `CallRow`, and E0559 and E0063 on the commands |
| am-app `status_spend` | 8 | no `am_domain::money` (E0432); `ReserveCall` fields (E0559); `cost_micro_usd` (E0063); `LedgerRow` fields (E0609) |

**Existing targets that do not compile, only because of the churn (469 tests, all green in the baseline):**

| Package | Targets (baseline passes) | Red because |
|---------|---------------------------|-------------|
| am-store | `audit` (23), `documents` (16), `inbox` (17), `inbox_resolve` (19), `prompt_journal` (20), `row_inputs` (15), `rows` (58), `run_journal` (14) | no `am_domain::money` (E0433); `ReserveCall` fields (E0559); `cost_micro_usd` still required (E0063) |
| am-engine | `journal` (3), `loop_caps` (23), `run` (7), `toolgate` (14) | no field `price` on `StepPlan`, `money` on `RunPlan` or `rate` on `LinearWorkflow` (E0560); no `money` or `RunMoney` (E0433, E0422); `run` has E0063 too |
| am-app | `gate_cli` (36) | E0433, E0559, E0063 |
| am-e2e | 34 targets (204) | each includes `tests/engine/support/mod.rs`, whose `step()` and `plan()` now set `price` and `money` (E0432, E0560). Their own churn: `rule_15_inbox`, `rule_16_audit_log`, `taint_at_rest`, `handoff_gate_resolve` (E0559, E0063), `spike` (`c2_crash_resume`: no field `rate` on `LinearWorkflow`) and `golden_so_far` (the support's ledger line reads the new `LedgerRow` fields, E0609) |

The 34 am-e2e targets are `capabilities_tables_grant`, `documents_versions`, `engine_result_journal`, `golden_so_far`, `handoff_gate`, `handoff_gate_resolve`, `handoff_loop_back`, `handoff_next`, `inbox_resolve_human_only`, `prompt_assembly_journal`, `rows_idempotent`, `rows_table_write`, `rule_15_inbox`, `rule_16_audit_log`, `rule_16_row_provenance`, `schema_repair`, `spike`, `taint_at_rest`, `taint_documents_at_rest`, `taint_documents_untrusted_markdown`, `taint_handoff_chain`, `taint_prompt_fence`, `taint_prompt_names_only`, `taint_prompt_repair`, `taint_prompt_type_wall`, `taint_rows_at_rest`, `taint_rows_declared_table`, `taint_rows_no_read_path`, `taint_rows_reader`, `taint_rows_reader_fence`, `taint_rows_reader_start`, `taint_tool_result_engine_values`, `taint_trusted_input_gate_note` and `taint_trusted_input_sources`.

**Red at run time, for this story's reason:**
- `agentic validate --template fixtures/projects/costs`, which `just validate` now runs, exits 1 with "models/prices.yaml:8: unknown key 'rates' in models/prices.yaml". The fixture is otherwise clean: without `rates:` it validates with 0 errors and 0 warnings.

**Everything else is green: 1,211 run, 1,211 passed, 3 skipped.** With the 469 baseline passes in the targets above, that makes the baseline's 1,680.
- Every default member except the six below (am-glob, am-net, am-providers, am-tools, am-connectors, am-render, am-services, am-mcp, am-extract, am-secrets, am-api, am-server, am-testkit, xtask): 233 passed, 2 skipped. That includes `xtask::denylist current_repo_passes`. Its first run failed on this round's own variable names `d1` and `f1` in `ledger.rs`, now `doc_call` and `fail_call`.
- am-domain without `money`: 37 passed.
- am-config without `prices`: 322 passed.
- am-store without the nine targets: 76 passed.
- am-engine without the four targets: 44 passed.
- am-app without `gate_cli` and `status_spend`: 352 passed, 1 skipped.
- am-e2e, its 21 compiling targets: 147 passed.
- **Doctests** (`-p am-store -p am-domain`): all pass (8, 14, 3, 7).
- **`cargo xtask check-deps`** and **`cargo xtask denylist`** pass, and fmt is clean.
- **Clippy** was not run on the changed targets: none of them compiles until the API exists.

**Golden:** `golden_so_far` is red by compile now. Once the build lands it stays red until the implementer moves the snapshot's three ledger lines by hand (see "API the tests fix", Golden).

**Test lines (against 43a60bc):**
- **New, 4,135 lines** in five files:
  - `money.rs` 317;
  - `prices.rs` 388;
  - `ledger.rs` 1,585, of which about 560 are the migration fixture, its helpers and the four migration tests;
  - `cost_ledger.rs` 1,109, row 23 included;
  - `status_spend.rs` 736.
  - Also: the golden support's ledger line, +13 −1; the am-e2e manifest, +7.
- **Churn, about 262 added and 94 removed** in 18 test files.
- **Fixtures:** 60 lines (`fixtures/projects/costs/`, `fixtures/accounts/costs.yaml`).
- **justfile:** +3 −2.
- **`src/`:** 0.

### Review triage

*Manager.*

#### Round 1 (manager, 2026-11-25)

Two Opus reviews ran on the built diff: correctness (C) and adversarial money and security (S). Neither found a blocker. Both checked these and found them holding:
- The money core:
  - rounding is up, once, from the exact value, in i128;
  - the company amount comes from the exact value;
  - the writer computes amounts from journaled terms;
  - settle at the reservation, `over_reservation`, settle once, and `MarkLost` all behave.
- The 0013 freeze triggers. Counting before sending. The week across DST, the year boundary and a non-Monday start.
- Refusals before any run row, in `plan_of`, `run_on`, `continue_run` and the gate continue.
- Pinning (AD-16), and an audit with no free text.
- Loopback spoofing of `openai-compatible` is refused.
- No keychain access, no network, no new crate.
- 3.4, 3.6, `neutralise` and the check-deps rules are untouched. The golden hash matches.

Fixes for round 2, tests first. The tester writes each test and logs it red, then the implementer fixes it. Both run on Opus.
- **M1 (S1, important):** an adapter turns missing or invalid usage fields into a real 0, so a cloud call can settle at USD 0 instead of at its reservation. This breaks AD-9 and Q3; the plan wrongly said adapters already return `None`.
  - Fix in am-providers (`anthropic.rs`, `openai_compat.rs`): `usage` is `Some` only when both counts are present as valid u64, including Anthropic's `message_start` input count. Otherwise it is `None`, and the call settles at the reservation.
  - Test: each adapter, with an empty, partial, negative or string usage, reports `None`.
  - This crosses into am-providers because 3.7 is the first story that settles on usage.
- **M2 (S2, important):** `UPDATE OR REPLACE ... SET rowid` on a reserved row deletes a settled row when an outside tool runs with `recursive_triggers` off.
  - Fix: `ledger_reservation_fixed` also refuses `NEW.rowid IS NOT OLD.rowid`, following the 0010 precedent. The insert trigger requires `state = 'reserved'` and NULL settled columns.
  - Tests: both, with recursive triggers off.
- **M3 (S3, important):** `cost()` accepts negative price terms and a zero rate from Rust callers. A negative sum is reported as `Overflow`.
  - Fix: a new `MoneyError` variant, worded, refuses `input < 0`, `output < 0` or `per_usd <= 0` up front.
  - 0013's insert trigger checks the terms: prices `>= 0`, and `company_micro_per_usd > 0` when not NULL.
  - The writer checks the format of `currency` (three capitals), `week` (an ISO date) and `team` (`is_config_id`, or empty only for the spike's rows).
  - Tests for each.
- **M4 (S4, important):** a duplicate `(provider, model)` price is accepted, and the first entry wins.
  - Fix: a validate error at the second entry, with file, line and fix, as `rates` already does.
  - Test: the duplicate case.
- **M5 (C1, S nit):** an `ollama` or `lmstudio` Account is local by its provider alone, so `https://ollama.com` would be priced at 0.
  - Fix (Q8's intent, "on this Mac"): such an Account is local only when its `base_url` is absent or a loopback `http://127.0.0.1` / `http://localhost` URL. Otherwise it is a cloud call and needs a price.
  - The local decision ignores no Accounts-file findings.
  - Test: an https `ollama` Account with no price is refused before any row. The owner is told that the rule was tightened to match Q8's intent.
- **M6 (C2):** the engine's `check_prices` also refuses a non-local priced step when `money.rate.company_micro_per_usd` is `None`, before any row.
  - Test through `run_plan`.
- **M7 (C3):** count call ids too: `ToolCall.id`, `ToolResult.call_id` and `EngineResult.call_id`.
  - Test by the byte-difference rule.
- **M8 (C4, S nit):** `model_call.lost` audits the settled USD amount, the company amount and the currency (AD-25).
  - Test the args exactly.
- **M9 (C5):** tests for `continue_run`'s `check_prices`, and for rows 13 and 14 through `agentic gate`'s continue path.
- **M10 (S nit):** `model_calls` gains set-once triggers: the token and `settled_at` columns never change once set. There is no DELETE trigger, because AD-25 run deletion must delete (as DR-162).
  - Test.
- **M11 (C7):** a test pins the USD-company status line (`USD x`, printed once).

Deferred:
- **D1 (C6) → DR-184:** the spike's literals: `journal.rs` takes the week from UTC `now()`, and `spike.rs` hard-codes a USD company. This conflicts with AGENTS.md's no-literal rule, in spike-only code. Goes to the 3.12 sweep.
- **D2 (C7):** a status group that mixes pre- and post-0013 rows sums time only over stamped rows, without marking the total partial. Accepted: every pre-0013 row is a zero-cost spike row. No register row.

### Red run of review round 2 (tester, 2026-11-25)

*Tester, on Opus, in the real working tree over the implementer's build, on `story/3-7` rebased onto `b234d74` (plan commit `a01ed8b`). No `src/` change, no git operation, no `cargo clean`, no copy of the workspace, no `am-desktop` build. FakeLlm, FakeClock and `FakeEgress` only; prices and rates are fake.*

**API pinned this round:**
- **M3:** `MoneyError::InvalidTerms` (tester, 2026-11-25). The tests match it as `InvalidTerms { .. }`, so the implementer may give it fields.
  - `cost` returns it up front, before any product and also on zero tokens, for a negative input or output term or a rate of 0 or below.
  - Its words are its own, not `Overflow`'s.
  - `NoRate` stays the error for `None`.
- **M8:** `model_call.lost`'s arguments are exactly `currency`, `settled_micro_company` and `settled_micro_usd`.
- **M3, the writer's words:**
  - `currency` is three capitals;
  - `week` is a real `YYYY-MM-DD` date (`2026-W48`, `2026-13-01` and empty are refused);
  - `team` is a config id (`Core Team` and `core/side` are refused).
  - An empty team is not tested: the triage allows it for the spike's rows.

**Commands:**
```
cargo nextest run -p am-providers --test usage --no-fail-fast
cargo nextest run -p am-domain --test money; cargo nextest run -p am-config --test prices
cargo nextest run -p am-store --test ledger --no-fail-fast
cargo nextest run -p am-engine --test count
cargo nextest run -p am-e2e --test cost_ledger --no-fail-fast
cargo nextest run -p am-app --test status_spend --no-fail-fast
cargo nextest run --workspace --exclude am-desktop --exclude am-domain --no-fail-fast
cargo nextest run -p am-domain --lib --test <every target but money>
cargo test --doc -p am-store -p am-domain; cargo xtask check-deps; cargo xtask denylist; cargo fmt --all -- --check
```
Two throw-away probes, both deleted, read reasons that an earlier assertion hides:
- a copy of `money.rs` that matches "neither `Ok` nor `Overflow`" in place of the new variant;
- a copy of `ledger.rs` with one extra test that sends only the writer's bad words.

**One line per test:**

| Item | Test (file) | Result | Reason |
|------|-------------|--------|--------|
| M1 | `anthropic_reports_no_usage_unless_both_counts_are_valid` (am-providers `tests/usage.rs`, new) | red | The well-formed control passes. `message_start` with no `input_tokens` reports `Some(Usage { input_tokens: 0, output_tokens: 9 })`, not `None`. The later cases are an empty usage, no usage, partial, negative, string and fractional counts. |
| M1 | `openai_compat_reports_no_usage_unless_both_counts_are_valid` (same file) | red | The control passes at both base URLs. `"usage":{}` reports `Some(Usage { input_tokens: 0, output_tokens: 0 })`. The later cases are partial, negative, string and a non-object usage. |
| M2 | `a_ledger_row_is_never_replaced_through_its_rowid_or_inserted_settled` (am-store `ledger.rs`) | red | With `recursive_triggers` off, `UPDATE OR REPLACE ledger SET rowid = <the settled row's rowid> WHERE model_call_id = <the open call>` is not aborted. A plain rowid change comes next, then the inserts that are `settled` or carry a settled amount, then the control. |
| M3 | `a_negative_term_or_a_rate_of_zero_is_refused_in_words` (am-domain `money.rs`) | does not compile | no variant `MoneyError::InvalidTerms` (E0599). The target's other 7 tests therefore do not run either; they passed before this round. In the probe, the first case (a negative input price) is refused as `Overflow`, and a negative term on zero tokens and a rate of 0 are priced. |
| M3 | `the_ledger_refuses_bad_terms_and_the_writer_bad_words` (am-store `ledger.rs`) | red | A raw insert with `input_micro_usd_per_mtok = -1` is accepted. In the probe, the writer accepts all eight bad words: `eur`, `EURO` and an empty currency; `2026-W48`, `2026-13-01` and an empty week; `Core Team` and `core/side`. |
| M4 | `a_model_priced_twice_is_an_error_at_the_second_entry` (am-config `prices.rs`) | red | The repeated `(anthropic, example-large)` gives no error (0, expected 1). The file's other 5 tests pass. |
| M5 | `an_account_off_this_mac_is_priced_whatever_its_provider` (am-app `status_spend.rs`) | red | The last case fails. With `box` listed twice in this Mac's Accounts (first at `127.0.0.1`, then at `https://…`), the run succeeds at 0, so a finding is ignored. The first three cases are green on arrival (see the M5 note below). |
| M6 | `a_priced_step_with_no_rate_is_refused_before_any_row` (am-e2e `cost_ledger.rs`) | red | A run row is written and the run stops at the reservation ("the call cannot be priced in the company currency: the price file has no rate for it"). `run_on`'s `check_prices` lets the step through. |
| M7 | `call_ids_count_by_their_bytes` (am-engine `tests/count.rs`, new) | red | A `ToolCall.id` 300 bytes longer counts 54 tokens, not 154. `ToolResult.call_id` and `EngineResult.call_id` come next. |
| M8 | `a_lost_call_audits_what_it_settled_at` (am-store `ledger.rs`) | red | `model_call.lost`'s arguments are `{}`. |
| M9 | `continue_run_refuses_an_unpriced_or_unrated_step_before_applying_the_decision` (am-e2e `cost_ledger.rs`) | red | The no-price case passes: `check_prices` already runs first. The no-rate case (M6) fails: `continue_run` applied the approval (the gate ended `succeeded`), and `publish` failed at its reservation. |
| M9 | `the_gate_refuses_an_unpriced_or_unrated_step_before_recording_the_decision` (am-app `status_spend.rs`) | **green on arrival** | `Gate::plan_for` runs `plan_of` before `record`. This Mac's Accounts move `box` from `127.0.0.1` to `https://…` after the run waits. Approving is then refused with row 13's words (no price) or row 14's (no rate). The item stays open with no decision, `publish` stays queued, and no model is called. |
| M10 | `a_calls_tokens_and_settled_at_never_change_once_set` (am-store `ledger.rs`) | red | With `recursive_triggers` off, `UPDATE model_calls SET input_tokens = 1` on a settled call is not aborted. The later updates touch `output_tokens`, NULLs, and `settled_at` on the settled call and on a lost one. |
| M11 | `a_usd_company_shows_usd_once` (am-app `status_spend.rs`) | **green on arrival** | `spend.rs` already prints a USD company's amounts once: the run, attempt and week lines hold one `USD …` each, and the head names no rate. |

**M5: what a test cannot reach.** The coordinator's two local cases cannot be reached from config today: an `ollama` Account with no `base_url`, or one at `127.0.0.1`, "stays local at 0".
- `ollama` and `lmstudio` are not Account providers. The Accounts schema (`am_domain::capability` PROVIDERS) offers `anthropic`, `openai` and `openai-compatible` for models.
- The Accounts loader therefore rejects such an entry with an error and drops it. Every run on it is refused as "not one of this Mac's Accounts".
- So `prices.rs`'s `"lmstudio" | "ollama"` arm is unreachable from config. That is also why the https-`ollama` case is green on arrival: it is refused, but not for M5's reason.
- The local cases need one of two things:
  - an in-crate unit test of `on_this_mac`, in `src/`, which is the implementer's;
  - making `ollama` and `lmstudio` Account providers, a config-schema decision outside 3.7.
- The test asserts only what holds either way for `ollama` at https: refused before any row, and no model called. **Question for the manager:** which of the two routes is wanted.
- The duplicate-id case is the tester's reading of "the local decision ignores no Accounts-file findings".

**Everything else is green.**
- **Default members except `am-desktop` and `am-domain`:** 1,695 run, 1,684 passed, 11 failed, 8 skipped. The 11 failures are exactly this round's reds above.
- **am-domain without `money`:** 37 passed.
- **Against the implementer's 1,726:** that run's 7 `money` tests do not run now, and this round's 13 runnable tests are added (1,726 − 7 + 13 = 1,732 = 1,684 + 11 + 37). No other test changed state.
- **Doctests** (`-p am-store -p am-domain`) pass (10, 14, 3, 7). `cargo xtask check-deps` and `cargo xtask denylist` pass, and fmt is clean.
- **Clippy `-D warnings`** is clean on every changed target that compiles: `usage`, `count`, `prices`, `ledger`, `cost_ledger` and `status_spend`.

**Test lines this round: about 1,221 added, none removed.**
- New files: am-providers `usage.rs` 256 and am-engine `count.rs` 83.
- `ledger.rs` +318, `status_spend.rs` +315, `cost_ledger.rs` +127, `money.rs` +65 and `prices.rs` +57.
- No fixture or `src/` change.

#### Round 2 (manager, 2026-11-25)

A focused Opus re-review found round 2 sound: no blocker, no important finding, and no new hole. Every item from M1 to M11 holds. 3.4, 3.6, `neutralise` and the check-deps and seam-guard rules are unchanged.

Nits, deferred because none leaks money before providers are wired:
- **N1 and N3 → DR-185 (2.6, must close):**
  - Decide locality from `accounts::endpoint_of`.
  - Build each provider on the endpoint that was priced.
  - Anthropic keeps the larger input count.
- **N2 → DR-186 (3.12):** an empty `team` is accepted from any caller, not only the spike.

## Auto Run Result

**Status:** built, 2026-11-25. Money story: planned, tested, built and reviewed on Opus (owner rule, 2026-11-25). The plan is owner-approved; the owner changed Q6 and Q10.

**Summary.** Every model call reserves and settles a ledger entry. The writer computes both from the terms journaled at reserve. Amounts are i64 micro-units in USD and the company currency, each rounded up once from the exact value in i128.

- **Reservation:**
  - counted input (UTF-8 bytes ÷ 3, rounded up) at the input price, plus `max_output_tokens` at the output price;
  - prices and the rate are pinned in the run's snapshot;
  - the week is set from the run clock in the company's timezone and week start;
  - the seat's team is recorded.
- **Settlement:**
  - settles from usage when the provider reports both counts as valid;
  - otherwise settles at the reservation;
  - usage above the reservation is flagged;
  - `MarkLost` settles in both currencies, and is sent once after a store error (DR-109, Q14).
- **Local calls:** cost 0 when they use a reserved local Account or a loopback endpoint.
- **Refusals before any run row:** an unpriced cloud model, a missing rate, or an error in this Mac's Accounts file.
- **Config:** `rates:` in `models/prices.yaml`; the price file's sha256 is the rate snapshot id.
- **`agentic status`:** shows spend by step, run, seat, team and week.
- **Migration 0013:**
  - additive columns;
  - triggers fixing reserve-time terms and the rowid;
  - a row settles once and whole;
  - no negative amounts or terms;
  - local rows stay 0;
  - `model_calls` tokens and `settled_at` are set once.
- **Shipped price files** (`templates/prices.yaml` and the sample's `models/prices.yaml`) carry the published prices and the INR rate, `updated: 2026-11-25`, for the owner to check.

**Size (code and test lines separately):**
- Production code: 1,969 lines added, 291 removed (net 1,678).
- In-crate tests: 150 added, 77 removed.
- Docs, templates and fixtures: 185 added, 6 removed. This includes the golden snapshot's three ledger lines, edited under 3.24's rule.
- Tests: 5,632 added, 102 removed.

**Tests.** Written first, with red runs logged for the build and for review round 2. `just check`: 1,741 passed, 8 skipped.

**Review (Opus).**
- Round 1, correctness and adversarial money/security: no blocker. M1–M11 fixed; D1 → DR-184, D2 accepted.
- Round 2, a focused re-review: sound. N1–N3 → DR-185 and DR-186.

**For the owner at review:**
- Check the prices and the INR rate in the two shipped price files.
- DR-183: Haiku 5.5's long-prompt tier is under-stated at the base price.
- M5 tightened Q8 to its intent ("on this Mac"): locality is decided by endpoint, never by provider name.
- An error in this Mac's Accounts file refuses runs on non-reserved Accounts.
