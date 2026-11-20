# Third-party components

Agentic Manager is MIT-licensed. Rust and npm dependencies are linked under
their own permissive licences (MIT, MIT-0, Apache-2.0, BSD-2/3-Clause, ISC,
Zlib, Unicode-3.0, MPL-2.0; architecture AD-3); story 1.2 enforces the allow-list
with cargo-deny.

## Bundled native binaries

None. (SQLite is compiled from source into the binary by `rusqlite`'s
`bundled` feature; SQLite is in the public domain.)

## Not bundled, launched only in development and tests

- `@modelcontextprotocol/server-filesystem` 2.4.0 and
  `@modelcontextprotocol/server-everything` 2.3.1 (MIT): MCP reference
  servers the spike's criterion 5 test starts with `npx`. Never shipped.
