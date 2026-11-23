---
review: versions and reality check
spine: architecture-agentic-manager.md
scope: 2026-11-23 owner amendment adding `TrustedInput` to AD-10, with wording in AD-27 and AD-36
date: 2026-11-23
---

# Review: versions and facts in the 2026-11-23 `TrustedInput` amendment

## Verdict

**Pass with changes.** The amendment names no new library, crate, framework or version, so nothing needs checking against the web or the Stack. Checked against the built code, the named crates exist and play the roles claimed. The domain types and both provider adapters have a place for the new part: a typed, labelled text block in the user role. The dependency matrix allows `am-domain` to hold the type with no new edge. Two committed guarantees are not yet backed by anything in the code or the rest of the spine:

- "untainted when it wrote it": no store field records a step run's own taint.
- "`am-engine` builds it only from…": Rust cannot enforce this through a type that lives in `am-domain`.

One sentence in AD-27 also reads wider than AD-10's (a).

## Method

- **Diff:** `git diff` of the spine against HEAD. The change covers the `updated:` date, AD-10's Binds and Rule (the three-type split and the **Trusted input** paragraph), AD-27's order and hand-off sentence, and AD-36's loop-back sentence.
- **Spine:** read the Design Paradigm table (lines 27–41) and AD-4 (line 129). Read AD-8 (journal and state machine), AD-41 (hand-off payload kinds), AD-42 (review-gate notes) and AD-43 (`step_inputs`).
- **Code** (read-only):
  - `crates/am-domain/Cargo.toml`
  - `crates/am-domain/src/content.rs` (`Instruction`, `UntrustedData`, `UntrustedContent`)
  - `crates/am-domain/src/llm.rs` (`Part`, `Message`, `ChatRequest.system: Vec<Instruction>`, `file_parts`)
  - `crates/am-domain/src/lib.rs` (re-exports)
  - `crates/am-providers/src/anthropic.rs` (`request_body`, lines 40–126)
  - `crates/am-providers/src/openai_compat.rs` (`request_body`, lines 94–185)
  - `crates/am-providers/src/common.rs` (`render_untrusted_text`, `neutralise`, `file_label`)
  - `crates/am-providers/Cargo.toml`, `crates/am-engine/Cargo.toml`
  - `crates/am-engine/src/run.rs` (`StepPlan`, `request_of`)
  - `crates/am-engine/src/journal.rs`
  - `crates/am-store/migrations/0001_initial.sql` and `0006_run_pin_and_times.sql`
  - Every `Part::` use in `am-app`, `am-testkit` and `am-e2e`
- **Tickets:** read `epic-workflow-engine-cli/tickets/tickets.toml`, the stories that mention "trusted input" (the loop-back/gate story, the resolve command, labelled prompt assembly and review gates).

## What checks out

| Claim in the amendment | Evidence | Result |
|---|---|---|
| `am-domain` holds the content types | `content.rs` defines `Instruction`, `UntrustedData`; `lib.rs:48` re-exports them | Fits. A `TrustedInput { label, text }` of two `String`s needs only `std` (plus `serde` if wanted), which is inside the Design Paradigm row "std, serde, thiserror, jiff, uuid only". `Cargo.toml` already has exactly those four dependencies. |
| `am-engine` does prompt assembly and would construct it | `run.rs::request_of` builds the `ChatRequest`: seat instructions in `system`, the step instruction as the one user `Part::Instruction` | Fits. `am-engine` → `am-domain` and `am-engine` → `am-store` are both allowed edges (Design Paradigm line 37), so reading the journal to build one adds no edge. |
| `am-providers` renders each part | Both adapters match on `Part` and own the wire format | Fits. `am-providers` → `am-domain` is an existing edge. |
| "never in the system or instruction position" | `ChatRequest.system` is `Vec<Instruction>`, and `Part::Instruction` wraps `Instruction` | Holds by type: a distinct `TrustedInput` cannot be put in `system` or in `Part::Instruction` without a conversion, and the amendment forbids one. A new `Part` variant is needed, as "its own part" implies. |
| "own labelled part in the user role" | Anthropic: user `content` is an array of `{"type":"text"}` blocks (`anthropic.rs:47–52`). OpenAI-compatible: the user message `content` is an array of `{"type":"text"}` parts (`openai_compat.rs:112–118`) | Both adapters have a user-role text block that is neither the `system` field nor the untrusted fence. A labelled, unfenced text part fits the same way `Part::Instruction` does today. |
| "after the step instruction" (AD-27 order) | `request_of` puts one user message with the instruction part first | Fits: the trusted parts are appended to the same user message, before any `Part::Untrusted`. |
| AI reviewer note trusted only when the reviewer is untainted | AD-42 "return loops back … with the note as a trusted input" and "a return or reject note written by a tainted reviewer is `UntrustedData`" | Consistent. |
| Loop-back reasons (AD-36) | Tickets: the loop-back story's verify line says "reasons that appear as a trusted input in the journal" | Consistent; the amendment gives that phrase a type. |

## Findings

### F1 — Medium: no stored record says a step was "untainted when it wrote it"

**Claim:** AD-10 says `am-engine` builds a `TrustedInput` only from "the journaled output of a step that was untainted when it wrote it".

**Reality:**

- `step_runs` (`0001_initial.sql:10–19`, `0006`) has `output` but no taint column.
- AD-8's journal rule lists no taint field on step runs.
- AD-43's `step_inputs` records the taint of each *input* edge. It does not record taint a step picks up mid-step from a tool result: `Part::ToolResult` content is `UntrustedData` (`llm.rs:40–44`), for example a web fetch or an MCP call.

So after a restart (AD-8 resume), or for a later step that reads an earlier output, the engine cannot decide (a) from what the spine says is stored. Recomputing it from `step_inputs` alone would miss tool-result taint, and that would launder taint through a loop-back reason or a hand-off.

**Suggested fix:** one clause in AD-10 or AD-8 saying:

- each step run carries a sticky `tainted` flag, set in the same transaction that journals the first `UntrustedData` entering its context;
- the step's output, reasons and notes are written with the flag's value at that moment;
- `TrustedInput` construction reads that flag and never recomputes taint from `step_inputs`.

The migration itself belongs in the loop-back story's ticket.

### F2 — Medium: "only `am-engine` builds it" is not enforceable by an `am-domain` type alone

**Claim:** "`am-engine` builds it only from (a) … or (b) … Nothing else yields one", and "`am-domain` gives it no `From` or `TryFrom`".

**Reality:** Rust visibility cannot limit construction to one downstream crate. If `TrustedInput` has public fields, as `TrustedInput { label, text }` suggests, or a public constructor, then `am-tools`, `am-mcp`, `am-connectors` and `am-providers` can all mint one. All of them depend on `am-domain`.

The existing pattern shows the risk:

- `Instruction::authored(impl Into<String>)` (`content.rs:15`) is a public constructor any crate can call.
- `UntrustedContent::Text(String)` is a public variant.
- So `Instruction::authored(<untrusted text>)` compiles today.

"No conversion" is enforced only by the absence of `From` impls and by review. With a third trusted-ish type, that gap is wider. If the type derives `Deserialize` (as `Instruction` does), any JSON read could also mint one.

**Suggested fix:** in the spine, state how the rule is enforced, in one line:

- private fields and one named constructor (for example `TrustedInput::from_clean_step(label, text)`, documented as engine-only);
- no `Deserialize` derive (the journal stores label and text, and the engine rebuilds the value after checking the F1 flag);
- a check that fails CI when that constructor appears outside `crates/am-engine/src` (a test, an `xtask check-deps`-style scan, or a `tests/trust/taint_trusted_input_*.rs` file).

The same check could cover `Instruction::authored` outside `am-config`, `am-engine` and the CLI. That part is optional and outside this amendment.

### F3 — Medium: AD-27's hand-off sentence reads wider than AD-10 (a)

**AD-10 (a):** limits the source to "the journaled output of a step", which includes "a hand-off payload".

**AD-41:** a hand-off payload is `output | document:<id> | rows:<table-id>`.

**AD-27 (amended):** "each such part [branch or hand-off] is a `TrustedInput` only when its producer was untainted". Read alone, this admits a `rows:` or `document:` payload from an untainted producer as a `TrustedInput`. AD-10 says free text in table rows is always `UntrustedData`, and (c) treats document reads by their at-rest taint.

**Suggested fix:** qualify both places as "a hand-off payload of kind `output`". Add that `rows:` payloads are always `UntrustedData`, and `document:` payloads follow (c).

### F4 — Low: adapter wildcards turn a missing render arm into a runtime "wrong role" error

**Claim:** "`am-providers` renders it so."

**Reality:** both adapters end their `Part` match with a catch-all:

- `anthropic.rs:84` `_ => Err(Protocol("a part was placed in the wrong role"))`
- `openai_compat.rs:147` (user branch) and `:169` (assistant branch)

A new `Part::Trusted` variant therefore compiles with no arm and fails at runtime with a misleading message. Nothing forces the rendering the amendment commits to. `ChatRequest::file_parts` (`llm.rs:69`) uses `_ => continue`, which is fine there.

**Suggested fix (ticket, not spine):**

- replace the wildcards with explicit wrong-role arms so the compiler lists every variant;
- add a recorded-fixture test per adapter. It asserts that the trusted part:
  - appears in a user message after the instruction text;
  - is absent from Anthropic's `system` and from the OpenAI-compatible `role: system` message;
  - is not wrapped in `<untrusted-data>`.

### F5 — Low: the label's rendered form is unspecified, and the trusted text is not neutralised

**Claim:** a `TrustedInput` renders "with its label".

**Reality:**

- Untrusted text has a defined fence (`common.rs:10`, `<untrusted-data source="…">`), and `neutralise` escapes any closing tag inside the data.
- For `TrustedInput`, the amendment does not say how the label appears.
- A clean step's output can still contain the literal string `<untrusted-data …>` or `</untrusted-data>`, for example when it quotes or summarises.
- Rendered unfenced just before the real fence, that text can make the model misread where the real fence starts or ends.

This is not a trust breach, because the text came from a clean step, but it weakens the one boundary the model sees.

**Suggested fix:** render the label as a plain header line (for example `[loop-back reasons: <step>]`). Pass the trusted text through `neutralise` as well, so the only fence tags in a request are the adapter's own. Ticket-level; one phrase in AD-10 is optional.

### F6 — Low: `Instruction` and `TrustedInput` overlap for "owner-authored"

**Claim:** `Instruction` is "config- or owner-authored", and (b) makes "an owner-authored note (for example a Human return note at a gate)" a `TrustedInput`.

**Reality:** the code agrees with the old wording. `content.rs:9–10` says to build an `Instruction` "from config files or the owner's own words", and the module doc (`content.rs:3–5`) says content is "either `Instruction` or `UntrustedData`". After the amendment, a Human gate note could be typed either way.

**Suggested fix:**

- Spine: say that `Instruction` is config and owner-authored configuration (role, goal, backstory, skills, guidance, memory). Owner notes attached to a run (gate returns) are `TrustedInput`.
- Code: the story updates the two doc comments in `content.rs`.

### F7 — Nit: Binds omits the test kit and drill-down

AD-10's Binds now names `am-providers`. A new `Part` variant also touches:

- `am-app` (`spike.rs:404–405` and `tests/run_cli.rs:448` match on part shapes);
- the run drill-down, which AD-27 says shows each part's label.

Neither needs a spine change. List them in the story's touch list.

## Not checked

- No web research: the amendment names no third-party technology or version.
- No `cargo` build, run or test: the review is read-only.
