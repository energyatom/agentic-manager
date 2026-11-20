# jsonschema 0.61.2 — not in the workspace (licence conflict)

The Stack pins `jsonschema = "0.61.2"`, and story 1.4's plan adds it to check
`schemas/<id>.json`. It is **not added**: with or without default features,
`jsonschema` depends on `referencing 0.61.2` → `fluent-uri 0.4.1` →
`borrow-or-share 0.2.4`, whose licence is **MIT-0**. MIT-0 is not on the AD-3
allow-list (MIT, Apache-2.0, BSD-2/3-Clause, ISC, Zlib, Unicode-3.0,
MPL-2.0), `cargo deny check licenses` rejects it, and `deny.toml` forbids
adding a licence or an exception. Found 2026-11-20 (story 1.4).

## What story 1.4 does instead

`am-config` (`kinds::schema` and `schema_check`) checks each schema file
without a dependency: valid JSON (serde_json), a top-level object with
`schema_version: 1`, no secret-looking values, and a structural check of the
common keywords (`type` names, `required` as a list of names, `properties`,
`$defs`, `items`, `enum`, `additionalProperties` and the combinators holding
schemas, `$ref` local to the file). It is not a full meta-schema validation.

## Open for the owner and the architect (REPLAN)

The engine needs a JSON Schema validator for step outputs (AD-36). Choices:
allow MIT-0 in AD-3 (it is a public-domain-equivalent MIT without the
attribution clause), pick another validator whose graph passes the policy,
or keep a project-owned validator for the subset step schemas use. Until
that is decided, do not add `jsonschema` and do not add MIT-0 to `deny.toml`.

## If it is added later

- Use `default-features = false`: the defaults `resolve-http` (reqwest and
  rustls, a second HTTP client against AD-12) and `resolve-file` (file reads
  outside `WorkspacePath`) must stay off; a remote `$ref` then fails to
  compile, which is wanted.
- `jsonschema::validator_for(&Value)` checks against the draft meta-schema;
  the error's `instance_path()` is a JSON pointer that am-config maps to a
  line through the file's serde-saphyr node tree.
- Keep its types out of `am-domain` and port signatures.
