# Agentic Manager — Stack spike criteria

The Rust-first stack (C2) is conditional on a two-week spike, the first story of the first epic, with these written pass/fail criteria (decisions D4). What the owner provides is listed in START-HERE, "What you provide for the spike".

**Counting** (restated 2026-11-19, owner answers): the REPLAN threshold is judged on criteria 1–7 and 9. Failing two or more of them sends the plan to replan: the architect proposes a hybrid and Sofia re-slices. Criterion 8 is **deferred** until the owner's Apple Developer account exists: it is not run as a pass/fail check in the spike, does not count towards the two-failure threshold, and becomes a gate before v0.9-rc (run at the start of epic 11 story 3). Criterion 5 may be recorded "pending owner input" only while Node.js or uv is not installed on the Mac; a pending criterion is not a fail.

| # | Criterion |
|---|---|
| 1 | Provider trait: Anthropic and OpenAI-compatible streaming with tool calls, including LM Studio, and cancellation. The frontier half proves a frontier cloud model end to end (streaming, a tool call, cancellation) on the owner's Anthropic key; an OpenAI key or any other OpenAI-compatible cloud key is an accepted alternative (owner, 2026-11-19), in which case that key's live run passes the frontier half and the Anthropic adapter passes on recorded fixtures, with its live run in epic 2 story 5 |
| 2 | kill -9 mid-run, then resume on SQLite WAL |
| 3 | Cron + timezone + catch-up after macOS sleep/wake |
| 4 | Files to models (rewritten 2026-11-19, owner: "What I want is folder reading access — a tool that I can link to the model … that LLM can be decided according to the type of files there"). The folder-access tool links the folder named by `AM_SPIKE_DOCS_DIR` (outside the repo; a few of the owner's files of mixed types, for example a PDF with a text layer, a scanned PDF, an image, a spreadsheet, a Word or PowerPoint file and a csv) read-only, lists it and reads every file under the confinement rules, then attaches each file through the provider trait's file parts to (a) a frontier model (the Anthropic key's model, or the OpenAI Responses API on an OpenAI key) and (b) a local vision-capable model served by LM Studio or Ollama. The report records, per file type and model, accepted natively or refused (with the worded refusal, made before any request), tokens and cost. Passes when every accepted type's answer names a known word in the file, every type the model's file chips exclude is refused with zero bytes sent, and the confinement cases pass (a symlink leaving the folder, a `../` path and a write attempt are refused). The test is skipped with "AM_SPIKE_DOCS_DIR not set" when the path is absent, the owner's recorded run being the evidence; CI runs it on small synthetic fixtures under `fixtures/` against recorded provider responses. No per-format parser, PDF engine comparison or OCR is part of the spike (arch AD-1, AD-30, AD-31) |
| 5 | MCP client (rmcp) over stdio and Streamable HTTP against 2 real, official reference servers, free and with no account, which Claude Code launches during the spike (owner, 2026-11-19; commands below). Passes when rmcp lists each server's tools, calls `read_text_file` on the stdio server and gets the fixture file's text, and calls `echo` on the HTTP server and gets the message back |
| 6 | Loopback server Host, Origin and token checks, including the WebSocket upgrade |
| 7 | macOS Keychain access (Windows and Linux stores are v1.x; amended 2026-11-19, owner Mac first) |
| 8 | **Deferred** (owner, 2026-11-19): a signed and notarised Tauri build running on a clean Mac. The spike records it "deferred: Apple Developer account" and keeps as stand-in evidence an ad-hoc-signed Tauri build that links the core in-process, launches and serves the UI; the signed and notarised clean-Mac demo is run once the owner provides the Developer ID certificate and notarisation credentials, as the gate at the start of epic 11 story 3, before v0.9-rc |
| 9 | Agent productivity: Claude Code agents complete a representative story with the test loop |

## Criterion 5: the two MCP servers

Owner prerequisites: **Node.js** (for `npx`) and **uv** (for `uvx`) installed on the Mac. Nothing else: no account, no key. Versions were checked on the npm registry and PyPI on 2026-11-19 and are pinned in the commands; the spike's MCP records pin the same versions (arch AD-26).

| Transport | Server | Command Claude Code runs | Notes |
|---|---|---|---|
| stdio | `@modelcontextprotocol/server-filesystem` 2.4.0 (official reference server) | `npx -y @modelcontextprotocol/server-filesystem@2.4.0 --read-only <throwaway test folder>` | The spike creates the throwaway folder under the system temp directory with one fixture text file and deletes it afterwards; `--read-only` registers only the reading tools, so nothing can be written. Never pointed at the project, the home folder or `AM_SPIKE_DOCS_DIR`. |
| stdio (alternative) | `mcp-server-fetch` 2026.11.18 (official reference server, MIT) | `uvx mcp-server-fetch@2026.11.18` | Use only if npx is unavailable; it fetches web pages, so the spike calls it on a local fixture page served on loopback. |
| Streamable HTTP | `@modelcontextprotocol/server-everything` 2.3.1 (official test server for MCP client builders) | `npx -y @modelcontextprotocol/server-everything@2.3.1 streamableHttp` | Endpoint `http://127.0.0.1:3001/mcp` (the `PORT` variable changes the port). It listens on loopback only by default and serves both the 2026-07-28 protocol (stateless) and 2025-era clients (sessions) from the same URL, so rmcp's Streamable HTTP client connects whichever era it speaks. |

## Shape

Shape the spike proves: one binary holding engine, local HTTP/WebSocket server, scheduler and SQLite; React + TypeScript UI embedded in it; Tauri shell wrapping it; browser mode first. Pre-1.0 crates (LLM clients, the PDF text layer) sit behind the project's own traits with no foreign types in the domain; thin own provider adapters are preferred. Rust is chosen for distribution, a single signed binary, reliability and maintenance, not raw performance.
