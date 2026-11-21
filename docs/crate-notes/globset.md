# globset 0.4.21

Pinned `=0.4.21` with `default-features = false` (no `log`), by owner
decision (d) of story 1.12 (2026-11-20), and in the architecture's Stack
since the 2026-11-20 REPLAN as **the one glob matcher** (Consistency
Conventions, Globs). Since epic 1 story 14 it lives behind the `am-glob`
crate, the only crate that may name it: `cargo xtask check-deps` refuses a
`globset` edge (normal or build) from any other workspace crate but the
matrix-exempt test scaffolding (am-testkit, am-e2e, xtask), naming the rule,
and `xtask/tests/check_deps.rs` also checks every crate manifest's
`[dependencies]`: `literal_separator(true)`, pattern and subject NFC-normalised
(see unicode-normalization.md), linked-folder `include`/`exclude`
case-insensitive against the root-relative path, Source URL-path scopes
case-sensitive.

## Licence

globset is dual-licensed `Unlicense OR MIT`; this project takes it under MIT,
which is on the AD-3 allow-list. Its graph adds `bstr 1.14.0`
(`MIT OR Apache-2.0`) and reuses `aho-corasick`, `regex-automata`,
`regex-syntax` and `memchr`, already in the lock file. `cargo deny … check
licenses bans sources` passes on the macOS and Linux graphs with no new
exception.

## Where it is used

- `am-glob` only, behind its own `Glob` type (`case_insensitive`,
  `case_sensitive`, `is_match`, `regex`) and `GlobError`. `am-config`
  compiles every `include`/`exclude` glob of a linked folder (AD-30) and of
  a Source's scope (AD-39) through it and reports one that does not compile
  at its line; the folder tools, fan-out routes and the crawler match
  through the same type later (am-config, am-engine, am-tools and
  am-connectors may depend on am-glob, no other crate).

## What we use

- `GlobBuilder::new(pattern).literal_separator(true).case_insensitive(b).build()`:
  `*` and `?` stay inside one path segment and `**` crosses segments, so
  globs are anchored path patterns. The error's `kind()` gives the message
  without repeating the pattern.
- `Glob::compile_matcher()` for matching and `Glob::regex()` for the regex
  form (it starts with `(?i)` when case-insensitive).

## Do not

- Do not depend on `globset` outside `am-glob`, and do not let its types out
  of that crate; keep the pattern as a `String` and compile it with
  `am_glob::Glob` where it is matched.
- Do not enable the `serde` feature; config files are read by am-config's own
  node reader.
