# globset 0.4.21

Pinned `=0.4.21` with `default-features = false` (no `log`), by owner
decision (d) of story 1.12 (2026-11-20), and in the architecture's Stack
since the 2026-11-20 REPLAN as **the one glob matcher** (Consistency
Conventions, Globs). It moves behind the `am-glob` crate (epic 1 story 14):
`literal_separator(true)`, pattern and subject NFC-normalised, linked-folder
`include`/`exclude` case-insensitive against the root-relative path, Source
URL-path scopes case-sensitive. Until then `am-config` compiles it directly.

## Licence

globset is dual-licensed `Unlicense OR MIT`; this project takes it under MIT,
which is on the AD-3 allow-list. Its graph adds `bstr 1.14.0`
(`MIT OR Apache-2.0`) and reuses `aho-corasick`, `regex-automata`,
`regex-syntax` and `memchr`, already in the lock file. `cargo deny … check
licenses bans sources` passes on the macOS and Linux graphs with no new
exception.

## Where it is used

- `am-config` only: `agentic validate` compiles every `include`/`exclude`
  glob of a linked folder (AD-30) and of a Source's scope (AD-39), and
  reports one that does not compile at its line. Nothing matches paths yet;
  the folder tools and the crawler use the same builder later.

## What we use

- `GlobBuilder::new(pattern).literal_separator(true).build()`: `*` and `?`
  stay inside one path segment and `**` crosses segments, so globs are
  anchored path patterns. The error's `kind()` gives the message without
  repeating the pattern.

## Do not

- Do not let `globset` types into `am-domain` or a port signature; keep the
  pattern as a `String` and compile it where it is matched.
- Do not enable the `serde` feature; config files are read by am-config's own
  node reader.
