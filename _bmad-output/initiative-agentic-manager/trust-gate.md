# Agentic Manager — Trust gate

Release and process rules that gate v0.9-rc and v1.0. From decisions D11 (release and process rules) and D10, and PRD "Stakeholders, risks and rollout". The product-side safety rules are the initiative's Constraints (RULE-8 to RULE-17); each ships with a failing-first CI test.

## Signing and updates

- macOS Apple Silicon: Developer ID signed, notarised and stapled, hardened runtime, including every bundled executable (Local search; bundled OCR retired 2026-11-19). Entitlements are checked per binary (added 2026-11-19, final review 2 security M5 and H5): the main app has no `disable-library-validation`, `allow-unsigned-executable-memory` or `allow-dyld-environment-variables`; the App Sandbox helpers (`agentic-extract`, the bundled Python) carry exactly their arch AD-13 sets, and the sandbox self-test passes on a clean Mac. This is the only platform v0.9-rc and v1.0 ship (owner, 2026-11-19, Mac first; arch AD-47).
- Deferred spike criterion 8 (owner, 2026-11-19): the signed and notarised Tauri build launching on a clean Mac and serving the UI is a gate before v0.9-rc, run at the start of epic 11 story 3 once the owner provides the Apple Developer ID certificate and notarisation credentials; it was deferred from the stack spike and not counted in the spike's REPLAN threshold.
- A separate update-signing key with a rotation plan; the updater verifies against the pinned public key, never downgrades, never installs without consent.

## Supply chain and CI

- cargo-deny, cargo-audit and npm audit; locked dependencies.
- CI runs the full checks on macOS arm64 and the portability job (core build and unit tests) on Linux; GitHub Actions pinned by SHA; least-privilege CI tokens; branch protection with required review.
- SBOM (CycloneDX or SPDX) and build provenance per release, listing the Rust and web dependencies and every bundled component: the CPython build, SearXNG at its commit with its locked Python packages, Granian (2026-11-19; Tesseract, Leptonica, the OCR model and pdfium left with bundled OCR the same day); OpenSSF Scorecard badge.
- A CI test asserts no telemetry hosts.

## Security test suites (release gates)

- Every suite below runs on the signed macOS build. The gate selects every file under `crates/am-e2e/tests/trust/` and `crates/am-e2e/tests/engine/` by directory (rule_*, connectors_*, local_search_*, capabilities_*, folder_access_*, taint_*, widening_*, sandbox_* and the cross-department suites), never a hand-kept list (2026-11-19, final review 2 security M5).
- DNS rebinding, CSRF and WebSocket Origin suite against the local server, including cookie-only requests and upgrades, pairing lockout, no session secret in any file, and a same-user child process that cannot approve, apply or widen anything (Decision 2026-11-18).
- Path traversal and symlink escape, linked folders included (2026-11-19): a `../` path, an absolute path or a symlink leaving a linked folder, a write to a read-only folder and a read past a cap make zero file reads or writes; a file type the seat's model cannot read sends zero bytes; linking a folder, switching write on and linking one to a cloud-model seat need the two-step confirmation.
- Prompt-injection regression pack (zero outbound calls, zero approvable proposals (a Proposal from a tainted step is never approvable), taint carried through AI-written notes, Ask a seat questions and local writes at rest (2026-11-19), zero requests carrying a table or document canary through read tools or search queries, in plain, URL-encoded or base64 form; Decision 2026-11-18).
- Budget and kill-switch tests (settled spend never exceeds a cap, per fan-out branch included; Stop everything halts all, every branch included).
- Cross-department tests (added 2026-11-18): taint carried across a hand-off still blocks external effects and tainted reads in the receiving department; a hand-off or review gate the receiving department did not accept is refused; a trigger chain past its maximum depth or on a cycle is refused and audited.
- Parser fuzzing (the PDF text layer, xlsx, CSV, text decoding, archives, HTML, feeds and sitemaps, linked-folder media-type sniffing, link lists and the CDP pipe framing; amended 2026-11-19: docx, pptx, OCR input and Local OCR service responses left with their readers).
- Privilege-widening suite (2026-11-19): every widening kind is flagged on every config path, an outside edit or startup load that widens starts nothing before the two-step confirmation, and a human review gate cannot be replaced by an AI reviewer without it.
- Child isolation suite (2026-11-19): the App Sandbox helpers cannot open the app data dir, a project or (the parser) a socket; the rendering browser makes zero loopback connections and opens no debugging port.
- Secret-leak scan of logs, traces, exports and config (zero keys), including MCP environment values echoed by a server and backup keys.

## Disclosure and documentation

- SECURITY.md with a disclosure channel and response target.
- A published threat model.
- A plain page on what the app protects you from and what it does not (Settings › About, linked from Doctor), stating at least: read-tool exfiltration mitigated, not eliminated; stdio MCP network declared, not enforced; the parser and Local search run in the macOS App Sandbox while the rendering browser and stdio MCP servers run as the owner, so every decision moves to the app window while either is enabled; files a seat reads from a linked folder are sent to that seat's model, so with a cloud model they leave this Mac for the provider named on the seat (added 2026-11-19, replacing the Local OCR service sentence); Local search's engine hosts proxied by configuration, not enforced by the OS; other processes of the same OS user outside the full threat model; backups encrypted, the live database relying on disk encryption.
- A community review window before v1.0.

## v1.0 gate

Four consecutive weeks on a frozen build with no manual repair (a repair resets the clock), the owner running the owner's own company's departments with at least one cross-department flow, one-click install, docs, and every item above green.

## Deferred to v1.x (owner, 2026-11-19, Mac first)

These rules gate the release of each platform when it is taken up, not v0.9-rc or v1.0:

- Windows 11 x64: code-signed installer and bundled executables through the route chosen then (OV or EV certificate, or a cloud signing service); updater `installMode` set; every security suite above on the signed Windows build; Local search on Windows decided (native SearXNG, a loopback listener with its residual risk on the About page, or a stated RULE-20 exception).
- Linux x64: AppImage and .deb with a checksums file signed through Sigstore attestations; every security suite above on the Linux build.
