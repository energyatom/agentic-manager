---
type: epic
title: "Accounts and model routing"
parent: initiative-agentic-manager
covers: [R6, R7, R8, R9]
sources:
  - _bmad-output/initiative-agentic-manager/initiative-agentic-manager.md
  - _bmad-output/initiative-agentic-manager/architecture-agentic-manager/architecture-agentic-manager.md
  - _bmad-output/initiative-agentic-manager/ux-agentic-manager/EXPERIENCE.md
  - _bmad-output/initiative-agentic-manager/prd-agentic-manager/prd-agentic-manager.md
  - _bmad-output/initiative-agentic-manager/party-decisions/decisions.md
companions:
  - _bmad-output/initiative-agentic-manager/architecture-agentic-manager/architecture-agentic-manager.md
  - _bmad-output/initiative-agentic-manager/ux-agentic-manager/EXPERIENCE.md
  - _bmad-output/initiative-agentic-manager/ux-agentic-manager/mockups/key-models-doctor-history.html
after: []
risk: high
---

# Accounts and model routing

## Description

Accounts in `models/accounts.yaml` with keychain references; SecretStore over the OS keychain with redaction and the canary leak scan; am-net's EgressClient as the only HTTP client, with pinned Account hosts and SSRF blocks; Anthropic and OpenAI-compatible adapters behind LlmProvider; loopback discovery of LM Studio and Ollama (reserved ids, health only, loaded context window read); seat model and fallback resolution with Recommended models and worded capability warnings; Test this model probes. Delivered through the core commands and the `agentic accounts` CLI; epic-org-seats-ui renders the Accounts card, grouped picker and chips over the same commands.

## Outcome

The owner adds a key once and any seat can use any cloud or local model with an honest picture of what it can do, without the key ever leaving the keychain.

## Requirements

- R1 (parent R6; arch AD-17, AD-31): The owner adds an Anthropic, OpenAI or OpenAI-compatible Account once, through the `agentic accounts` CLI until epic-org-seats-ui renders the Account card; the key goes only to the OS keychain under the Account's id, `models/accounts.yaml` holds only its reference, every read shows it masked, and the Account's host is pinned.
- R2 (parent R7; arch AD-31): `local-lmstudio` (127.0.0.1:1234) and `local-ollama` (127.0.0.1:11434) are always resolvable Accounts badged On this Mac; discovery only toggles their health and lists their models with each loaded context window; no other address is probed, and config validates with both servers off.
- R3 (parent R8; arch AD-19, AD-31; D3): Resolving a seat returns Account › Model and its fallback from one list grouped by Account with local Accounts first, pre-filled with the company default model, with the role's Recommended model from existing Accounts and worded warnings for missing capabilities that never refuse; an error, timeout or refusal switches the call to the fallback and the switch is audited.
- R4 (parent R9; arch AD-31): Capability chips (Tools, Structured output, Context, Cost) come from the shipped capabilities file overridden by `models/capabilities.yaml` and are marked known; Test this model runs one tool call and one schema output and marks the chips tested for that Account and model only after the probe passes, storing the result.
- R5 (parent constraint Egress; prd RULE-9; arch AD-12; parent Decision 2026-11-18): am-net EgressClient is the only HTTP client; each request carries its seat and Account and goes only to that seat's allow-listed hosts or the Account's pinned host; private, link-local, loopback and metadata addresses are blocked except the two local Accounts at their ports; DNS is pinned; at most 3 redirects, each re-checked; bodies capped; auth stripped on a host change; every blocked call is audited; no telemetry host exists. Threshold: zero calls outside configured endpoints, measured on this client.
- R7 (parent R9, and the file-input part of init R83, which epic-connectors-sources owns; arch AD-5, AD-31; added 2026-11-19, owner folder-access direction): LlmProvider carries native file parts: the Anthropic adapter sends PDFs as document blocks and JPEG, PNG, GIF and WebP as image blocks, the OpenAI adapter sends PDF, text and office files through the Responses API with image and audio inputs, the OpenAI-compatible adapter sends base64 images to vision models only, and any part whose type the model's file chips exclude is refused before a byte is sent, with no conversion; the chips "reads PDF · images · office files · audio" are known from the shipped capabilities file and tested only after Test this model's per-type fixture probe names its known word.
- R6 (parent constraint Secrets; prd RULE-12; arch AD-17): Keys live only in the OS keychain behind SecretStore; SecretString cannot be printed or serialised and is materialised only inside am-net request building, never handed to a tool; a redaction layer masks registered secrets and known key patterns in logs and audit. Threshold: the canary leak scan over logs, journal, audit and config finds zero keys.

## Constraints

- Stack (parent Stack; arch AD-5): own thin Anthropic Messages and OpenAI-chat-compatible adapters over reqwest SSE through am-net; genai, rig-core and async-openai are not dependencies; no foreign types in LlmProvider or SecretStore.
- Local and private (parent; prd RULE-5): no telemetry; no LAN scan; network only to configured endpoints.
- Failing-first tests (parent; D11): the RULE-9 and RULE-12 trust tests land here as `crates/am-e2e/tests/trust/rule_09_*.rs` and `rule_12_*.rs`, written to fail first; provider tests run on recorded fixtures, never a live key, in CI.

## Done when

- Adding an Anthropic, OpenAI or compatible Account from the CLI stores the key only in the keychain and every read shows it masked; the canary leak scan finds zero keys in logs, audit and config.
- With LM Studio or Ollama running on loopback, their Accounts report healthy with discovered models and the loaded context window; with them off, config still validates; no non-loopback scan occurs.
- Resolving a seat's model returns Account › Model, its fallback, the role's Recommended model and worded warnings for missing capabilities; a forced provider error switches to the fallback and the switch is in the audit log.
- Test this model runs one tool call and one schema output, and the chip reads tested only for that Account and model after a pass.
- Merged with `just check` and the RULE-9 egress and RULE-12 secrets suites green: calls to unlisted hosts, private, link-local or metadata addresses and unchecked redirects are blocked and audited, with the threshold measured on the app's own egress client.

## Boundaries

- Out: cost roll-up and the context-overflow pause (epic-workflow-engine-cli: R11, R10); caps (epic-triggers-budgets-safety-floor); the Accounts, picker and chips UI and their API routes (epic-org-seats-ui); per-seat allow-lists for search, fetch and Source connectors (epic-connectors-sources) and the Doctor list of blocked calls (epic-tools-mcp-search-doctor); stdio MCP network use (declared, not measured here; epic-tools-mcp-search-doctor).
- Touch point: LM Studio and Ollama on loopback — model listing, loaded context window, calls; external, not changed.
- Touch point: Anthropic, OpenAI and compatible APIs — calls and usage fields; external.
- Touch point: the macOS Keychain — the only home for keys (Credential Manager and Secret Service are v1.x, restated 2026-11-19, owner Mac first); external.
- Must not change: no LAN scan.

## References

- parent — _bmad-output/initiative-agentic-manager/initiative-agentic-manager.md, Requirements R6, R7, R8, R9; Constraints (Secrets, Egress); Notes (Decision: MCP network use, 2026-11-18)
- architecture — _bmad-output/initiative-agentic-manager/architecture-agentic-manager/architecture-agentic-manager.md, AD-5, AD-12, AD-17, AD-19 (fallback switch), AD-31, Deferred (OpenAI Chat Completions or Responses), Open Questions (Ollama field)
- design — _bmad-output/initiative-agentic-manager/ux-agentic-manager/EXPERIENCE.md, Account card, Account › Model picker, Capability chip, Test this model, Voice and Tone (warning wording)
- design — _bmad-output/initiative-agentic-manager/ux-agentic-manager/mockups/key-models-doctor-history.html §1

## Notes

- Waits on epic-foundations-rust-spike because: the spike verdict, the ports, the store writer with Actor and audit, and the config loader and writer for `models/`.
- Decision: Refined (owner delegated, 2026-11-18): settled by the assumptions below; the OpenAI API choice and Ollama's loaded-context field are answered by spike criterion 1 (story 1.1); probe prompts and chip wording are left to the stories within EXPERIENCE.md; no new spike.
- Decision: Tracer bullet is story 1, adding an Anthropic Account from the CLI and calling it: CLI, config writer, keychain, egress client and provider adapter in one thin path (2026-11-18).
- Decision: Sequencing (2026-11-18): egress hardening second because RULE-9 is the least certain part and every later network story builds on it; the OpenAI and compatible adapter next because local discovery reuses it; local discovery, then resolution and probes, which need every Account kind; secrets hardening follows egress hardening because both touch am-net request building, and runs beside stories 3 and 4; refactor sweep closes; no closing end-to-end story (epic 3's golden run exercises these paths).
- Decision: Approved CLI group (owner delegated, 2026-11-18): until epic-org-seats-ui ships the Account card, Accounts are added, listed, tested, probed and removed through the `agentic accounts` CLI group, which reads keys from the terminal without echo, never from arguments or the environment; the API routes for the same commands come with epic 5.
- Assumption: One OpenAI-chat-compatible adapter serves OpenAI cloud, compatible endpoints and both local servers; the Responses API is used only if the spike's criterion 1 shows Chat Completions lacks streamed tool calls (arch Deferred).
- Assumption: The fallback switch is a wrapper around LlmProvider that epic 3's agent loop calls; its run-log line arrives with epic 3's run log, its audit entry here.
- Assumption: Test this model is offered as `agentic accounts probe` until the seat sheet renders it (epic 5); its price shown up front uses epic 3's price file once it exists.
- Open question: see initiative Notes, prd OQ-6 recommended local model per role (owner with the architect, before story 5.4); story 6 builds the resolution, not the values.
- Decision: REPLAN consistency pass (2026-11-18): epic 5 now waits on this epic directly (5.4 on 2.6 and 2.7, 5.5 on 2.4 and 2.5); no change to this epic's entries.
- Decision: Final review F7a and F10 (owner delegated, 2026-11-18): no real key is used before the redaction layer and leak scan exist, so story 1 runs on recorded fixtures and a canary key (hitl off) and the owner's real-key step moves to story 5, which now ends with a leak scan over the owner's own logs and journal; story 2's verify names the no-telemetry test so every dogfood build carries it; story 3 accepts an OpenAI-compatible Account pinned to a loopback port other than the app's own (arch AD-12, amended 2026-11-18). Requirements unchanged.
- Decision: Mac first (owner, 2026-11-19; initiative Notes): story 5 hardens SecretStore over the macOS Keychain only; its contract tests also run against FakeSecretStore in the Linux portability job; Windows and Linux stores are v1.x (arch AD-17, AD-47). No requirement or entry change beyond that rescope.
- Decision: Folder access (owner, 2026-11-19; initiative Notes): new story 9 "Models take files: native file parts, file chips and Test this model probes them" builds after story 7 and before the sweep (8), which now also waits on it; R7 added. Provider facts were checked on the Anthropic, OpenAI, LM Studio and Ollama documentation the same day (arch AD-31): only the OpenAI Responses API takes office files, local OpenAI-compatible servers take images only.
