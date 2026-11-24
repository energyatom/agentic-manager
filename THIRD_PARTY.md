# Third-party components

Agentic Manager is MIT-licensed. Rust and npm dependencies are linked under
their own permissive licences (MIT, MIT-0, Apache-2.0, BSD-2/3-Clause, ISC,
Zlib, Unicode-3.0, MPL-2.0; architecture AD-3); story 1.2 enforces the allow-list
with cargo-deny. One per-crate exception: `notify` 8.2.2 (the config watcher)
is CC0-1.0, allowed for that crate at that version only by a
`[[licenses.exceptions]]` entry in `deny.toml` (AD-3, owner 2026-11-21).

## Bundled native binaries

None. (Native libraries compiled into the binary are listed below.)

## Native libraries compiled into the binary

- SQLite 3.54.1: compiled from source into the binary by `libsqlite3-sys`
  0.40.0 through `rusqlite`'s `bundled` feature; SQLite is in the public
  domain.
- AWS-LC 5.12.0 (AWS Libcrypto, a fork of BoringSSL): built from source and
  statically linked by the `aws-lc-sys` 0.47.0 crate, through `aws-lc-rs`
  1.19.0 (`ISC AND (Apache-2.0 OR ISC)`), which rustls uses for TLS (reqwest
  in `am-net`) and, from epic 9 story 10, `am-store`'s audit-hash key module
  (architecture AD-3, AD-32). From that story `.cargo/config.toml` sets
  `AWS_LC_SYS_USE_SYSTEM=0`, so an AWS-LC installed on the build machine is
  never linked instead. Licence, as `aws-lc-sys` declares it:
  `ISC AND (Apache-2.0 OR ISC) AND Apache-2.0 AND MIT AND BSD-3-Clause AND
  (Apache-2.0 OR ISC OR MIT) AND (Apache-2.0 OR ISC OR MIT-0)`. Its `LICENSE`
  file carries the AWS-LC, BoringSSL and OpenSSL notices and those of the code
  compiled into the library: Fiat Cryptography (MIT), s2n-bignum
  (Apache-2.0 OR ISC OR MIT-0), mlkem-native and mldsa-native
  (Apache-2.0 OR ISC OR MIT), the Jitter Entropy RNG (BSD-3-Clause, which
  Amazon elects over GPL-2.0) and public-domain (CC0) Keccak and AES reference
  code.

The desktop app (`am-desktop`, epic 11) also compiles one small native file:
`objc2-exception-helper` 0.1.1's Objective-C `try_catch.m`, through Tauri
(`Zlib OR Apache-2.0 OR MIT`).

## Not bundled, launched only in development and tests

- `@modelcontextprotocol/server-filesystem` 2.4.0 and
  `@modelcontextprotocol/server-everything` 2.3.1 (MIT): MCP reference
  servers the spike's criterion 5 test starts with `npx`. Never shipped.
