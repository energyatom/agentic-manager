# jsonschema 0.61.2 — allowed since the 2026-11-20 REPLAN, linked by epic 3 story 5

Pinned `=0.61.2` in the architecture's Stack (checked on crates.io 2026-11-20: the
latest release, MIT). Its graph `referencing 0.61.2` → `fluent-uri 0.4.1` →
`borrow-or-share 0.2.4` holds the only MIT-0 crate. The architect REPLAN of
2026-11-20 (owner decision) added **MIT-0** to the AD-3 allow-list and to
`deny.toml`, so `cargo deny check licenses` accepts it with no exception.
It is not in the workspace yet: epic 3 story 5 adds it when it validates step
outputs (AD-36), as AGENTS.md requires (no crate outside its story).

## History (story 1.4, 2026-11-20)

Before the REPLAN, MIT-0 was outside the allow-list, so story 1.4 did not link
jsonschema. `am-config` (`kinds::schema` and `schema_check`) checks each schema file
without a dependency: valid JSON (serde_json), a top-level object with
`schema_version: 1`, no secret-looking values, and a structural check of the
common keywords (`type` names, `required` as a list of names, `properties`,
`$defs`, `items`, `enum`, `additionalProperties` and the combinators holding
schemas, `$ref` local to the file). It is not a full meta-schema validation.

## When epic 3 story 5 adds it

- Use `default-features = false`: the defaults `resolve-http` (reqwest and
  rustls, a second HTTP client against AD-12) and `resolve-file` (file reads
  outside `WorkspacePath`) must stay off; a remote `$ref` then fails to
  compile, which is wanted.
- `jsonschema::validator_for(&Value)` checks against the draft meta-schema;
  the error's `instance_path()` is a JSON pointer that am-config maps to a
  line through the file's serde-saphyr node tree.
- Keep its types out of `am-domain` and port signatures.
