---
type: epic
title: "Desktop, release and trust gate (v0.9-rc)"
parent: initiative-agentic-manager
covers: [R54, R55, R57, R79, R80]
sources:
  - _bmad-output/initiative-agentic-manager/initiative-agentic-manager.md
  - _bmad-output/initiative-agentic-manager/architecture-agentic-manager/architecture-agentic-manager.md
  - _bmad-output/initiative-agentic-manager/trust-gate.md
  - _bmad-output/initiative-agentic-manager/prd-agentic-manager/prd-agentic-manager.md
  - _bmad-output/initiative-agentic-manager/party-decisions/decisions.md
  - _bmad-output/initiative-agentic-manager/ux-agentic-manager/EXPERIENCE.md
companions:
  - _bmad-output/initiative-agentic-manager/architecture-agentic-manager/architecture-agentic-manager.md
  - _bmad-output/initiative-agentic-manager/trust-gate.md
  - _bmad-output/initiative-agentic-manager/ux-agentic-manager/EXPERIENCE.md
  - _bmad-output/initiative-agentic-manager/ux-agentic-manager/mockups/key-first-run.html
  - _bmad-output/initiative-agentic-manager/ux-agentic-manager/mockups/key-models-doctor-history.html
after: []
risk: high
---

# Desktop, release and trust gate (v0.9-rc)

## Description

The Tauri 2 shell linking the core in-process with minimal capabilities, strict CSP, narrow IPC and session bootstrap; the signed, notarised, hardened-runtime macOS Apple Silicon build (Windows and Linux builds are v1.x, owner 2026-11-19); the updater with a pinned minisign key, newer-only and consent-gated; the release pipeline with locked and audited dependencies, SHA-pinned least-privilege Actions, required review, SBOM, provenance, Scorecard and parser fuzzing; every security suite as a release gate on signed builds; SECURITY.md, the threat model and the protections page checked against the tests; the docs (README, config schema, CrewAI and Paperclip how-to, contributor docs, examples); the community review window; the v0.9-rc tag; and the v1.0 gate that tags only after the clean dogfood weeks. Every trust-gate rule is one checkable story here.

## Outcome

A stranger installs the app in one click and can verify it is safe, and the owner tags v1.0 once the dogfood weeks are clean and the trust gate is green.

## Requirements

- R1 (parent R54; arch AD-32, AD-13, AD-14, AD-29; prd RULE-17): The Tauri 2 app links the core in-process with no sidecar, serves the same embedded UI, receives its session through bootstrap IPC, and limits IPC to session bootstrap, window control and the updater, with minimal capabilities, a strict CSP whose `connect-src` is the loopback API origin, and no remote content. Threshold: the RULE-17 suite passes.
- R2 (parent R54; trust gate Signing; prd SM-5): The macOS Apple Silicon installer is Developer ID signed, notarised and hardened-runtime; on a clean Mac it reaches first run in under 5 minutes with no runtime to install, and the first successful run completes within 15 minutes of install.
- R3 (retired 2026-11-19, owner Mac first): the code-signed Windows 11 x64 installer and the Linux x64 packages with signed checksums move to v1.x (parent R54 restated; trust gate Deferred to v1.x); never reused.
- R4 (parent R55; arch AD-32; trust gate Signing and updates): The updater verifies every update against a pinned minisign public key separate from the code-signing keys, installs only a newer version and only after the owner consents ("Install on next launch?"), and refuses a downgrade or a bad signature with the reason in words; a key rotation plan is published.
- R5 (trust gate Supply chain and CI; arch AD-3, AD-12): Every release build runs cargo-deny (licences, advisories, bans), cargo-audit and npm audit on locked dependencies; every Action is pinned by SHA with least-privilege tokens; main requires review and green CI; a CI test asserts no telemetry host. Each rule fails the release when broken.
- R6 (trust gate Supply chain and CI): Each release publishes a CycloneDX or SPDX SBOM, build provenance attestations and checksums for every artefact; OpenSSF Scorecard runs in CI and its badge is on the README.
- R7 (trust gate Security test suites; rhea B7; restated 2026-11-19, owner folder-access direction: docx and pptx readers retired): Parsers (the PDF text layer, xlsx, CSV, text decoding, archives) and linked-folder media-type sniffing are fuzzed nightly, and a release needs a minimum fuzzing time per target with no open crash.
- R8 (trust gate Security test suites; parent Done when; arch AD-33): The release workflow publishes nothing unless, on the signed macOS Apple Silicon build (restated 2026-11-19, owner Mac first), the RULE-8 to RULE-17 suites (including rebinding, CSRF and WebSocket, path traversal and symlink escape, budget and kill-switch, secret-leak scan) pass and the golden run and injection pack pass on the fake provider and one real local model; the keyed-audit-hash suite (`audit_hash_*`, written by epic 9 story 10) passes too, so a release build holds no bare hash of free text (restated 2026-11-24, owner keyed audit hashes; arch AD-32, AD-33).
- R9 (parent R57; trust gate Disclosure and documentation; rhea B6, B9): SECURITY.md names a private disclosure channel and a response target; a threat model covering the assets and attacker stories is published; the Settings › About protections page maps every "protects" claim to a named test and states every "does not", including that prompt injection is mitigated by architecture, not solved, and that stdio MCP network use is declared, not enforced.
- R10 (parent R57; prd Developer experience; D9): The README leads a fresh reader to first run; the config schema is documented with every `agentic validate` error's file, line and fix; a how-to explains moving CrewAI and Paperclip setups by hand; contributor docs and example projects ship.
- R11 (parent R54; prd OQ-1, D12; arch Deferred product name): The public product name, once chosen, is applied to binaries, bundle id, installers, strings and the keychain service, with a one-time migration of stored keys from the working name.
- R12 (trust gate v1.0 gate; parent constraint Release shape; Decisions 2026-11-18): v0.9-rc is tagged when R1–R11 hold; a community review window opens with its findings published; v1.0 is tagged under MIT only after four consecutive clean dogfood weeks with no manual repair, counted from the frozen post-epic-9 build, of which at least the final two run on the v0.9-rc build, with every trust-gate item green.
- R13 (parent R79, R80; parent constraints Open by default, Local search service; arch AD-45, AD-46; added 2026-11-18; restated 2026-11-19, owner Mac first): The macOS installer carries the bundled Local search (pinned Python, SearXNG, Granian) read-only (local OCR retired 2026-11-19), signed and notarised with the app and matching the hashes compiled into the binary; THIRD_PARTY.md and the About page state their licences (SearXNG AGPL-3.0-or-later, unmodified, separate process, its source attached to each release); a clean machine with no keys answers a web search through Local search.

## Constraints

- Desktop (parent; prd RULE-17; arch AD-32): owned here; `rule_17` tests land in `crates/am-e2e/tests/trust/`, written to fail first.
- Trust gate (parent; trust-gate.md): owned here; every rule in the companion is a requirement above and a story below.
- Licence (parent; C1; arch AD-3): MIT; cargo-deny and the npm licence check allow only the AD-3 list.
- Release shape (parent; Decisions 2026-11-18): the clock starts on the frozen post-epic-9 build; a manual repair resets it; the final two clean weeks run on the v0.9-rc build.
- Platforms (parent; restated 2026-11-19, owner Mac first; arch AD-47): v1 releases macOS Apple Silicon only; Windows 11 and Linux x64 release jobs, signing and installers are v1.x.
- Local and private (parent; prd RULE-5): the updater contacts only the release host; no telemetry.
- Secrets (parent; prd RULE-12): signing keys live in CI secrets, never on a laptop or in the repo; the update key is separate from the code-signing keys.

## Done when

- On a clean Apple Silicon Mac the signed, notarised installer reaches first run in under 5 minutes and the first run completes within 15 minutes.
- The updater installs a newer signed version only after consent and refuses a downgrade or a bad signature.
- Every trust-gate item is green on the signed macOS Apple Silicon build, including the RULE-8 to RULE-17 suites, fuzzing, SBOM, provenance and Scorecard.
- A fresh reader reaches first run from the published README; SECURITY.md, the threat model and the rest of the R57 docs are published; the protections page's claims each name a passing test.
- v0.9-rc is tagged and the community review window has opened; v1.0 is tagged under MIT only after four clean weeks, the final two on v0.9-rc, with findings published.

## Boundaries

- Out: Windows 11 and Linux x64 builds, installers, code signing, signed checksums and their release and test jobs (v1.x, owner 2026-11-19; trust gate Deferred to v1.x).
- Out: LAN mode (v1.x); app-store distribution; updates without consent (never); OS-level sandboxing of children (v1.x); CrewAI and Paperclip import tools (a how-to only, D9).
- Out: the trust suites themselves (each built in its owning epic: RULE-15 in epic-workflow-engine-cli, RULE-8 to RULE-12 and RULE-14 in epic-triggers-budgets-safety-floor and epic-accounts-model-routing, RULE-13 in epic-tools-mcp-search-doctor, RULE-16 in epic-foundations-rust-spike, RULE-18 and RULE-19 in epic-polish-dry-run-learning, the keyed-audit-hash suite in epic-tools-mcp-search-doctor story 10); this epic gates releases on them; one exception, story 17 (owner, 2026-11-23): the pre-release extension of RULE-15's owner-only start check to macOS ACLs and the folder owner is built here.
- Out: writing the protections page (epic-tools-mcp-search-doctor); this epic reviews it against the threat model and tests.
- Touch point: Apple notary service, GitHub Releases, OpenSSF Scorecard and attestations — signing and distribution; external.
- Must not change: the CI baseline from epic-foundations-rust-spike story 2 is extended, never loosened.

## References

- parent — _bmad-output/initiative-agentic-manager/initiative-agentic-manager.md, Requirements R54, R55, R57; Constraints (Desktop, Trust gate, Licence, Release shape, Platforms); Done when; Notes (Decisions 2026-11-18)
- architecture — _bmad-output/initiative-agentic-manager/architecture-agentic-manager/architecture-agentic-manager.md, AD-3, AD-13 (desktop bootstrap), AD-14, AD-17, AD-29 (CSP), AD-32 (keyed audit hashes), AD-33, Structural Seed (deployment, environments and release), Deferred (product name, backup encryption)
- trust gate — _bmad-output/initiative-agentic-manager/trust-gate.md (every section)
- party — _bmad-output/initiative-agentic-manager/party-decisions/decisions.md, D11 (trust and safety package, release and process rules)
- design — _bmad-output/initiative-agentic-manager/ux-agentic-manager/EXPERIENCE.md, State Patterns (Update available), Safety Surfaces (Updates, What the app protects you from), Responsive & Platform
- mockups — _bmad-output/initiative-agentic-manager/ux-agentic-manager/mockups/key-first-run.html (first run on the installed app); key-models-doctor-history.html (Doctor link to About)

## Notes

- Waits on epic-polish-dry-run-learning because: a feature-complete, QA-passed v1 build.
- Decision: the official dogfood clock starts after epic 9 on a frozen build, not at v0.9-rc (owner delegated, 2026-11-18; parent Notes).
- Decision: Clean weeks (owner delegated, 2026-11-18; initiative Notes): v1.0 additionally requires at least the final two of the four clean weeks to run on the v0.9-rc build; moving to v0.9-rc does not reset the clock, only a manual repair does; the initiative's open question is closed and Release shape restated.
- Decision: Refined (owner delegated, 2026-11-18): every trust-gate rule became one requirement and at least one story; no spike, since epic 1 story 1 criterion 8 already proved a signed, notarised Tauri build on a clean Mac.
- Decision: Tracer bullet is story 1, the desktop app opening the project on macOS through the in-process core, bootstrap IPC and the strict CSP (2026-11-18).
- Decision: Sequencing (2026-11-18): the updater comes second as the least certain part (no-downgrade and consent; on macOS only since 2026-11-19); platform builds, then the supply-chain, SBOM, fuzzing and suite gates in the release workflow lane; disclosure and docs; the product-name story waits on its unknown; the refactor sweep precedes the two release stories, because a release candidate is cut from swept code and the release stories are gates, not features.
- Assumption: 15 entries stay in one epic; a split into a desktop-and-installers epic and a release-gate-and-docs epic was considered and not taken because v0.9-rc needs both under one Done when; the owner may split.
- Assumption (retired 2026-11-19, owner Mac first): Linux packaging and Sigstore-signed checksums moved to v1.x with Linux (arch Deferred).
- Assumption: The minimum fuzzing time before a release is one cumulative hour per target with no open crash; nightly runs keep the corpus.
- Assumption: The community review window lasts at least two weeks, overlaps the final two dogfood weeks on v0.9-rc, and its findings are published in the repo before v1.0.
- Assumption: The v1.0 gate is a `just release-gate` check that reads docs/dogfood-log.md (started by epic-tools-mcp-search-doctor story 13) and the trust-gate results.
- Open question: see initiative Notes, prd OQ-1 public product name (owner, before story 12; its entry carries it as `unknown`).
- Open question: see initiative Notes, prd OQ-7 data-protection duties and backup encryption (owner, before v0.9-rc; may add a backup-encryption story here if epic 9 story 10 shipped unencrypted).
- Decision: the Windows code-signing route question is closed as deferred to v1.x (owner, 2026-11-19; initiative Notes).
- Decision: Fuzzing gate (owner delegated, 2026-11-18): this epic makes epic 4's nightly fuzzing (story 4.6) a release gate; story 7 adds a CSV target and the release-time minimum.
- Decision: REPLAN consistency pass (2026-11-18): owner open questions moved to the initiative Notes and referenced here; no entry change.
- Decision: Final review F7d, F11, F12 and F1 residual (owner delegated, 2026-11-18): story 1 registers the updater IPC disabled until story 2 verifies signatures; story 9's review requires the About page and threat model to state read-tool exfiltration as mitigated not eliminated, same-user processes outside the full threat model, the client-side real-click check and the live database's reliance on disk encryption; story 12 migrates backup keys with the other keychain entries. Requirements unchanged.
- Decision: Bundling (owner addition, 2026-11-18; initiative Notes): new story 16 "Bundle Local search and OCR in every installer with their licence notices" builds after the updater (2) and before the macOS installer (3), which now waits on it; R13 added; story 3's clean-Mac first run uses a local model and no keys. Its `unknown` holds the Windows question.
- Decision: Mac first (owner, 2026-11-19: "Priority is the Mac build. If it's OK, the entire Windows and Linux build can wait for later."; initiative Notes): R3 retired; R2 stands; R4, R8, R13, Platforms, Done when and Boundaries restated for the macOS build only.
- Dropped: story 4 "Signed Windows and Linux builds" (2026-11-19, owner Mac first): Windows and Linux builds, installers, signing and their release jobs move to v1.x. It is not reduced to a portability check because that check belongs on every push, so epic 1 story 2's CI baseline runs it (a Linux job building the core and running its unit tests). Story 5 now waits on story 3 (the release workflow's macOS job), and the sweep no longer waits on story 4; id 4 is never reused.
- Decision: Story 16 is rescoped to the macOS installer only and loses its Windows `unknown`; stories 2, 8 and 14 drop their Windows and Linux wording (2026-11-19).
- Decision: Final review 2 (owner delegated, 2026-11-19; initiative Notes): story 4's description is marked DROPPED (title kept for its plan file); story 6's SBOM lists the bundled Python, SearXNG, Granian, Tesseract, Leptonica, model and pdfium; story 7 adds HTML, feed, sitemap, image, link-list and CDP-framing fuzz targets; story 8 runs every trust and engine suite by directory and checks per-binary entitlements; story 9's protections page states the App Sandbox scope and the browser and MCP residual risk; story 16 signs each helper with its own App Sandbox entitlements.
- Decision: Owner answers (owner, 2026-11-19; initiative Notes): the stack spike's criterion 8 (a signed and notarised Tauri build on a clean Mac) is deferred until the owner's Apple Developer account exists and becomes the gate that opens story 3, before v0.9-rc; story 3 needs the owner's Developer ID certificate and notarisation credentials and records criterion 8's verdict in the spike report. pptx joins the parser list (R7) and story 7's fuzz targets build on epic 4 story 6's pptx target. No entry added, dropped or reordered.
- Decision: Folder access (owner, 2026-11-19; initiative Notes): story 16 is retitled "Bundle Local search in the macOS installer with its licence notice" and no longer bundles Tesseract, Leptonica or the OCR model (it no longer waits on the dropped 12.20); story 6's SBOM drops Tesseract, Leptonica, the model and pdfium; story 7 drops the docx, pptx and OCR-input targets and adds linked-folder media-type sniffing; story 8 runs the folder_access_* suites and checks only the agentic-extract and Python helper entitlements; story 9's protections page says files attached to a cloud model leave the Mac. R7 and R13 restated. No entry added or dropped.
- Decision: Pre-release hardening (owner, 2026-11-23, after epic 3 story 2 merged as PR #22; its review finding A3, accepted there as a residual): add an ACL check. The owner-only start check (arch AD-13: the app data dir and the project's runtime folder 0700 with 0600 files, built in 3.2) looks only at mode bits; macOS ACLs that grant other users access, and the folder's owner, are not checked. No separate pre-release hardening list exists (trust-gate.md is a companion Sofia does not edit, and the About residuals in 9.11 state limits, not work), so the item is tracked here, beside the release trust gate, as story 17 "Owner-only start check covers macOS ACLs and the folder owner": table order between 7 and 8, `after = [1]` (story 1 also changes how am-app starts the server, in-process in the desktop app, so the two do not collide; through story 1 it is pinned to this epic's gate, 10.11, and 3.2, which built the check, is reached through that, so epic 3 is not added to this epic's `after`), covers R8 because its refusal is a RULE-15 trust case that story 8's gate runs; story 8 (so the release gate is first proven with the ACL case present, not because it needs 17's output) and the sweep (13) now wait on it. Boundaries name it as the one exception to "the trust suites are built in their owning epic". No Requirement or AD changed.
- Assumption: Story 17 realises AD-13's owner-only intent and needs no AD amendment; if the architect judges otherwise at its plan, or reading ACLs or the owner needs a crate or a nix feature the Stack does not allow today, the plan stops for a Sofia REPLAN with the architect before any code. Known facts for that plan (checked 2026-11-23): nix 0.31.4 has no ACL API, `getuid` needs nix's `user` feature (not compiled in), and `acl_get_file` through libc needs unsafe code; a std-only route exists (ACLs read with `/bin/ls -le`, the running user's id taken from the owner of a file the process itself creates), so the REPLAN clause need not fire.
- Decision: Keyed audit hashes (owner, 2026-11-24, final; verbatim in epic 9's Notes; arch AD-32, AD-33), condition (5), verbatim: "add a check to the release trust-gate that a release build has no bare hash of free text". Story 8, the release gate over `tests/trust/` by directory, names `audit_hash_*` and how it checks: on the signed release build, every audit action the store's classifier marks as text is written with a canary text, with a key and without one, and no bare sha256 of the canary may appear in the runtime folder or the audit export, each keyed value recomputing independently; a seeded bare hash of a canary note stops publishing. The suite itself is written by epic 9 story 10, which this epic already follows (through epic 10), so no `after` changes. Story 12's keychain move runs before any store opens, so a writer open never creates a new backup key while the old service still holds one (arch AD-17, AD-32). R8 restated, Boundaries name the suite's owner; no entry added, dropped or reordered.
- Open question: (2026-11-24, owner, before story 8; arch AD-3 and Open Questions (4)) confirm that AWS-LC's public-domain (CC0) reference code and its Jitter Entropy RNG (BSD-3-Clause, as Amazon elects) fall under AD-3's native-library rule, recorded in `THIRD_PARTY.md`, with no crate exception.
