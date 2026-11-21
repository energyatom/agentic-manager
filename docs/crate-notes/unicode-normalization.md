# unicode-normalization 0.1.26

Pinned `=0.1.26` in the workspace `Cargo.toml` (architecture Stack, amended
2026-11-20: "NFC for `am-glob`; already in the graph through `url`"). Epic 1
story 14 makes it a direct dependency of `am-glob` only.

## Licence

`MIT OR Apache-2.0`, both on the AD-3 allow-list. It was already in the lock
file through `url` (idna), so it adds no crate to the graph; `cargo deny …
check licenses bans sources` passes on the macOS and Linux graphs.

## Where it is used

- `am-glob` only: both a glob's pattern and every subject matched against it
  are put in Normalization Form C before globset sees them, so a file name
  that macOS reports decomposed (`e` + U+0301) matches a pattern written
  precomposed (`é`), and the reverse.

## What we use

- `use unicode_normalization::UnicodeNormalization;` then
  `text.nfc().collect::<String>()`. Nothing else.

## Do not

- Do not use it to rewrite names on disk or in config; it only normalises the
  two strings being compared.
- Do not add it to `am-domain` (its externals are serde, thiserror, jiff and
  uuid); the config-name policy there compares ASCII names ignoring case.
