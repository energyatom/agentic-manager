# Rubric review: AD-10 `TrustedInput` amendment (2026-11-23)

Reviewer: rubric walker (good-spine checklist). Scope: the working-tree diff of `architecture-agentic-manager.md` on 2026-11-23 (AD-10 Binds and Rule, AD-27 Rule, AD-36 Rule), checked against the rest of the spine and the built code (`crates/am-domain/src/content.rs`, `crates/am-domain/src/llm.rs`, `crates/am-providers/src/{common,anthropic,openai_compat}.rs`, `crates/am-engine/src/{run,journal}.rs`). The owner decision itself (third type, its two sources, no conversions, rendering position, taint behaviour, AD-27/AD-36 wording only) is taken as final; nothing below re-opens it.

## Verdict

**Accept with changes.** No critical finding. The amendment fixes the real divergence point for story 3.3 (what type an untainted upstream output, loop-back reason or Human return note takes, and where it renders) and stays inside the owner decision. But the rule as written is not yet enforceable against the most likely slip (wrapping text in `Instruction::authored`, which the spike executor already does), it leaves three other ADs using "trusted input" in ways that now disagree with the closed source list, and the new "three types" partition has no type for some clean local reads. Findings 1 and 4–7 are wording fixes inside the decision; findings 2 and 3 need an Open Questions entry for the owner (they do not block story 3.3).

## Checklist walk

| Check | Result |
|---|---|
| Fixes the real divergence points (story 3.3 prompt assembly, loop-back, gate notes) | Yes for type and position; partly for rendering form and order (F5, F9) |
| Each Rule enforceable and actually prevents its divergence | Partly: "built only by am-engine" and "no `From`/`TryFrom`" do not stop a string round-trip (F1) |
| Consistent with other ADs that mention trusted input, Instruction, UntrustedData, fences, labels, prompt order | No: AD-38 `read_skill_file`, AD-42 return note, AD-11 tainted-read rule, AD-46 `ask_seat` answer and AD-10 (c) clean-at-rest reads (F2, F3, F4, F8) |
| Ratifies existing code rather than contradicting it | `content.rs` and `llm.rs` need one new type and one new `Part` variant, which the spine does not name (F5); the spike executor `am-engine/src/journal.rs` contradicts the new rule and is not called out (F1) |
| Stays within the owner decision | Yes; suggested fixes below either restate existing ADs or go to the owner as an Open Question |

## Findings

### F1 — high — The "no conversion" rule is not enforceable as worded, and the spike executor already breaks it

**Where:** AD-10 Rule, new **Trusted input** sentences ("`am-engine` builds it only from …"; "`am-domain` gives it no `From` or `TryFrom` with either type").

**Problem:** Banning `From`/`TryFrom` does not prevent laundering, because every content type has a public constructor taking a `String`: `Instruction::authored(text)` (`content.rs:15`) and `UntrustedData::text(label, text)` already exist, and a `TrustedInput { label, text }` with a public constructor would add a third. Any crate can write `Instruction::authored(trusted.text())` or `TrustedInput::new(l, untrusted_text)`. "Only am-engine builds it" is not expressible in Rust visibility from `am-domain`. The live proof is the spike executor: `crates/am-engine/src/journal.rs:130-155` formats `previous_outputs` (journaled outputs of earlier steps, i.e. case (a)) into a string and sends it as `Part::Instruction(Instruction::authored(prompt))` — exactly the TrustedInput→Instruction conversion the amendment forbids, done by string, not by `From`. A builder reading the amended rule will believe the type system guards this; it does not.

**Suggested fix (spine wording, append to the Trusted input sentences):**
> "No conversion" means no path, including through `String`: `TrustedInput` has no public field and exactly two constructors in `am-domain`, `TrustedInput::from_step_output(label, text, producer_taint: Taint) -> StepInput` (an enum of `Trusted(TrustedInput)` or `Untrusted(UntrustedData)`, so a tainted producer can only yield `UntrustedData`) and `TrustedInput::owner_note(label, text, actor: &Actor) -> Option<TrustedInput>` (`None` unless `Actor::Human`). `cargo xtask check-deps` (AD-4) fails the build if either constructor, or `Instruction::authored`, is called outside its allowed crates (`TrustedInput`: `am-engine` only; `Instruction::authored`: `am-config` and `am-engine`'s config-to-prompt path, plus tests). The spike executor's use of `Instruction::authored` for earlier step outputs (`am-engine` `journal.rs`, epic 1) is superseded: epic 3 story 3 replaces it with `TrustedInput` parts. A trust test, `crates/am-e2e/tests/trust/taint_trusted_input_*.rs`, proves a canary written by an upstream step never appears in an `Instruction` part or the system prompt, appears as a `TrustedInput` part when the producer was clean and inside the fence when it was tainted.

(The xtask clause is the enforcement; the constructor shape keeps the taint decision in one place. If the owner prefers no xtask scan, the trust test alone is the minimum.)

### F2 — high — AD-38 `read_skill_file` returns "a trusted input": a third source outside the closed list

**Where:** AD-38 Rule (line 368): "`read_skill_file` … returns the file as a trusted input labelled `skill:<id>/<file>`".

**Problem:** The amendment says `TrustedInput` comes only from (a) a clean step's journaled output or (b) an owner-authored note; "Nothing else yields one." A skill file is neither. Read literally, AD-38 now names a third source the owner did not decide; read loosely, a builder of epic 9 story 16 will mint a `TrustedInput` from a file read. In addition, the built `Part::ToolResult { content: UntrustedData }` (`llm.rs:44-47`) can only carry `UntrustedData`, so neither reading is buildable without a type decision. The natural type is `Instruction`: AD-10 already lists "attached skills" as `Instruction`, and AD-38 already says the SKILL.md body "is `Instruction`".

**Suggested fix:** in AD-38 replace "returns the file as a trusted input labelled `skill:<id>/<file>`" with "returns the file as config-authored text (`Instruction`, AD-10: a skill file is config once applied) labelled `skill:<id>/<file>`; how `am-domain` carries it in a tool result (today `Part::ToolResult` holds only `UntrustedData`) is decided before epic 9 story 16 (Open Questions)". Add the Open Questions line: "Open 2026-11-23, for the owner (AD-10, AD-38): `read_skill_file` returns config text through a tool result; either `Part::ToolResult` gains an `Instruction` content case for this one tool, or the file is rendered as an `Instruction` part after the tool result. It is never a `TrustedInput`." This does not reopen the owner decision; it keeps the source list closed.

### F3 — high — The three-type partition leaves clean local reads and `ask_seat` answers without a type

**Where:** AD-10 (c) ("a later `workspace_read`, `read_file` or document input of a tainted-at-rest item returns `UntrustedData`"), AD-46 Hand-off & ask ("the answer `UntrustedData` when the answering step read untrusted content"), and AD-46 Workspace ("a read of a tainted file returns `UntrustedData`").

**Problem:** These sentences imply that reading a *clean*-at-rest workspace file or document version, or an answer from a clean `ask_seat` run, does *not* return `UntrustedData` and does not taint. The amended AD-10 now says prompt content is exactly one of three types. Such a read is not `Instruction` (not config), not `TrustedInput` (not a step's journaled output nor an owner note; "Nothing else yields one"), and cannot be `UntrustedData` without tainting the reader, contradicting (c)'s clear implication. Each epic that builds these tools (Folder access, workspace, documents, `ask_seat`) will pick a different answer. Story 3.3 does not build them, so this need not block it.

**Suggested fix:** add to Open Questions: "Open 2026-11-23, for the owner (AD-10 TrustedInput, AD-46): what type a clean-at-rest read takes (a workspace file or document version written by an untainted step, an `ask_seat` answer from an untainted run). Either (i) it is `UntrustedData` and taints the reader, and AD-10 (c) and AD-46 are reworded to say every such read returns `UntrustedData`, tainted at rest or not; or (ii) source (a) is widened to 'the journaled output or the journaled local write of a step untainted when it wrote it'. Until answered, builders use (i) (the safe reading)." Option (i) needs no owner widening and is the default the spine should state now.

### F4 — medium — AD-42 still passes every return note "as a trusted input", unconditionally

**Where:** AD-42 Rule (line 412): "return loops back to the producing step with the note as a trusted input".

**Problem:** AD-36 was reworded to name the type and its condition; AD-42 was not. Its main sentence is unconditional (a later sentence restricts the tainted-reviewer case), and it does not say a Human note is source (b) while an AI note is source (a). The two ADs now word the same hand-off differently.

**Suggested fix (wording only, same as AD-36):** "return loops back to the producing step with the note as a `TrustedInput` (AD-10: a Human reviewer's note is an owner-authored note, an AI reviewer's note is the journaled output of its review step and is a `TrustedInput` only when that step was untainted, otherwise it goes inside the untrusted fence, AD-10 (a))".

### F5 — medium — The rendering rule does not name the `Part` variant or the rendered form, so it does not ratify the provider code

**Where:** AD-10 "A `TrustedInput` renders as its own part, with its label, in the user role … `am-providers` renders it so."

**Problem:** The built `Part` enum (`llm.rs:34-48`) has `Instruction`, `Untrusted`, `AssistantText`, `ToolCall`, `ToolResult`. The amendment does not say a new variant is added, so a builder may encode a `TrustedInput` as `Part::Instruction` (losing the type at the provider boundary, which is the F1 slip again). Both providers end their `match` with a `_ =>` arm that returns "a part was placed in the wrong role" (`anthropic.rs:84`, `openai_compat.rs:146`), so a new variant compiles and fails only at run time. The label's rendered form is unspecified, while `UntrustedData` has one (`render_untrusted_text` in `common.rs`); two providers will render the label two ways and fixture tests will differ.

**Suggested fix:** "`am-domain::llm::Part` gains `Trusted(TrustedInput)`, user role only. `am-providers` renders it with one function in `common` (a text block that opens with its label, for example `[input <label>]`, and is never wrapped in the untrusted fence), used by every provider; each provider matches every `Part` variant by name (the wrong-role arm lists the variants it refuses rather than `_`), and a fixture test per provider shows a `TrustedInput` part as a separate user-role text block after the step instruction and before the first fence."

### F6 — medium — Source (b) "an owner-authored note" overlaps `Instruction` ("config- or owner-authored")

**Where:** AD-10 first sentence (`Instruction` = "config- or owner-authored") and the new (b).

**Problem:** Guidance notes and memory notes are owner-authored and are `Instruction` (AD-11 lists guidance notes among `Instruction` parts; AD-27 places them before the step instruction). "An owner-authored note" in (b) also matches them, so a builder can type a guidance note as `TrustedInput` or a Human gate note as `Instruction`. Both are user role today, but the drill-down label, the AD-11 tainted-query word list (which reads only `Instruction` parts) and prompt order differ.

**Suggested fix:** narrow (b): "(b) an owner-authored note attached to one step's input by a Human resolution (a Human reviewer's return note at a gate, AD-36, AD-42, written under `Actor::Human`); standing owner text kept in config (guidance notes, memory notes, CEO goals) stays `Instruction`."

### F7 — medium — "Reading a tainted row never yields one" implies a clean row may

**Where:** AD-10 "reading a tainted row (or any item tainted at rest, (c) below) never yields one".

**Problem:** AD-10's own `UntrustedData` list already makes *all* free text in table rows `UntrustedData` ("generalised 2026-11-18"), and "documents" too. The new sentence's contrapositive invites a builder to give a clean row's or clean document's text as `TrustedInput`, which source (a) does not allow (a row is not a journaled step output).

**Suggested fix:** "No store read yields one: a table row, document, workspace file, linked-folder file or tool result is never a `TrustedInput`, tainted or not (free text in rows is always `UntrustedData`, above); an item tainted at rest is in addition a taint source ((c) below). A `TrustedInput` comes only from the journal." (Consistent with the owner's sentence, which it strengthens; see F3 for the open part.)

### F8 — low — AD-11's tainted-read rule says "the step's trusted inputs" in lower case

**Where:** AD-11 Rule (line 198): "a URL … present in the step's trusted inputs".

**Problem:** With a named type, "trusted inputs" now reads either as `TrustedInput` parts only or as `Instruction` and `TrustedInput` parts. The same sentence later says "`Instruction` parts" for queries, so a builder may exclude URLs in the step instruction.

**Suggested fix (wording only; owner or Winston to confirm the intended set):** "present in the step's `Instruction` or `TrustedInput` parts (AD-10)".

### F9 — low — Order among several `TrustedInput` parts is not fixed

**Where:** AD-27 order "step instruction, `TrustedInput` parts, then fenced `UntrustedData`".

**Problem:** A step can receive a next input, a hand-off payload, loop-back reasons and a gate return note together. AD-27 fixes branch order only. Journal snapshots and the story 3.3 verify ("parts in order") will differ between builders.

**Suggested fix:** "`TrustedInput` parts in this order: declared step inputs in declaration order (fan-in branches in the fan-out's declared order), then a hand-off payload, then loop-back reasons and return notes, oldest first."

### F10 — low — "Untainted when it wrote it" is not tied to a journal record

**Where:** AD-10 source (a).

**Problem:** It is not said where the producer's taint is read from. On resume (AD-8) an engine that recomputes taint could disagree with the original run.

**Suggested fix:** "(a) … untainted when it wrote it, as recorded in the journal with that output (taint only grows within a step, so a final output carries the step's final taint); resume and lineage read that record and never recompute it."

### F11 — low — Leftover two-type wording

**Where:** AD-10 **Prevents** line; `content.rs` module doc ("Prompt content is either an `Instruction` … or `UntrustedData`"); the AGENTS.md policy line on untrusted content; UI rule "The UI shows `UntrustedData` as plain text in an untrusted well" (no rule for `TrustedInput`).

**Suggested fix:** AD-10 Prevents: add "and a clean upstream step's model-written text being rendered as config-authored instructions". AD-10 Rule: "The run drill-down shows a `TrustedInput` with its label as model output through the sanitising Markdown renderer (AD-29), never in the untrusted well." Story 3.3 updates the `content.rs` doc; the AGENTS.md managed block is refreshed through `bmad-project-context`, not by hand.

## Scope check against the owner decision

- Third type in `am-domain`, built by `am-engine` from (a) or (b): stated. ✔
- No UntrustedData→TrustedInput, none TrustedInput↔Instruction: stated (enforceability F1). ✔
- Tainted producer output, reasons and notes always `UntrustedData`: stated, by pointer to (a). ✔
- Own labelled user-role part outside the fence, never system or instruction: stated (form F5). ✔
- Never taints, never clears taint; reading a tainted row never yields one: stated (F7 tightens). ✔
- AD-27 and AD-36 wording only; binds am-domain, am-engine, am-providers: stated; AD-42 missed (F4). ✔
- Nothing in the amendment goes beyond the decision. The suggested fixes stay inside it, except F2 and F3, which are routed to the owner as Open Questions with a safe interim reading.
