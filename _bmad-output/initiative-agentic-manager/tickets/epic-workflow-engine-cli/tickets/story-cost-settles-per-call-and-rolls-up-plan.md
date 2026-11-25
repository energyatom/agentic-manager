---
title: 'Cost settles per call and rolls up'
type: feature
ticket: 7
created: '2026-11-25'
status: approved
baseline_revision: '461c14c'
after: [21]
route: full
route_source: auto
risk: high
review: quick
review_source: pinned
lenses_ran: []
review_loop_iteration: 0
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

### API the tests fix (tester, after the owner's answers)

*Left for the tester. The planner's proposal, pending Q1–Q16:*
- `am_domain::money::{CallPrice, Rate, Money, MoneyError, cost}`.
- `am_domain::week::{BudgetWeek, budget_week}`.
- `WriteCommand::ReserveCall { step_run_id, call_id, call_seq, price: CallPrice, rate: Rate, counted_input_tokens: u64, max_output_tokens: u32, week: String, team: String, taint, purpose, prompt }`.
- The settle commands and `CallSettlement` without `cost_micro_usd`.
- `LedgerRow` and `CallRow` gain the new columns.
- `Reader::spend()`.
- `StepPlan.price: Option<CallPrice>`; `RunPlan.money`.
- `am_config::Prices { rates, sha256, .. }`.

### Red run

*Tester: commands, failing output and test lines.*

### Review triage

*Manager.*
