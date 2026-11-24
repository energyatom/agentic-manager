# jsonschema 0.61.2 — linked by epic 3 story 5, built in story 3.5

Pinned `=0.61.2` in the architecture's Stack (checked on crates.io 2026-11-20: the
latest release, MIT). Its graph `referencing 0.61.2` → `fluent-uri 0.4.1` →
`borrow-or-share 0.2.4` holds the only MIT-0 crate. The architect REPLAN of
2026-11-20 (owner decision) added **MIT-0** to the AD-3 allow-list and to
`deny.toml`, so `cargo deny check licenses` accepts it with no exception.

## As built (story 3.5, AD-3, AD-12, AD-36)

- **One user.** The workspace dependency is
  `jsonschema = { version = "=0.61.2", default-features = false }`, and only
  `am-engine` names it, only in `src/schema.rs`. `Value` in, words out: no
  `jsonschema` type is in `StepPlan`, `RunPlan`, `am-domain` or a port.
- **Features off.** Without the defaults there is no `resolve-http` (reqwest and
  rustls, a second HTTP client against AD-12), no `resolve-file` (file reads
  outside `WorkspacePath`), no `tls-aws-lc-rs` and no `idna`. `cargo tree -p
  am-engine -e normal -i reqwest` finds nothing. The options also say
  `.offline()`, so the retriever refuses everything that is not already in the
  schema's own document.
- **`compile(schema_id, &Value)`** builds the validator with `jsonschema::options()`,
  `.offline()` and `PatternOptions::regex()` with a 1 MiB size and DFA limit.
  Draft detection follows the schema's `$schema`; the file's own
  `schema_version` key is an unknown keyword and is ignored. Formats are not
  asserted (the draft default). `run_on` and `continue_run` compile every step's
  schema before any row is written; a failure is `RunError::Schema { step, why }`
  and `why` names the schema's id.
- **`$ref`.** `am-config` already refuses a `$ref` that does not start with `#`.
  Past that check, a remote `$ref` (`http:`, `https:`), a `file:` URL, a path
  and a relative name for another document all fail the build, because the
  offline retriever has nothing to return. A `$ref` into the schema's own
  document (`#/$defs/x`) compiles. Nothing is fetched and no file is read (the
  tests bind a loopback listener and plant a canary file).
- **Pattern engine.** The subject of `pattern` is the model's text, which may be
  hostile, so the linear-time `regex` engine is used, not `fancy-regex`
  (which can backtrack exponentially). Look-around and back-references fail the
  build, at compile time, in words that name the id. No schema in the repo uses
  either.
- **Words.** `check` trims the message, parses it as JSON (no fence stripping, no
  quote fixing) and collects up to 1,000 validation errors. Each becomes a
  finding from the error's keyword and its *evaluation path* in the schema
  (`/properties/count/type` → "field `count`: must be of type integer"), with
  the property names the schema itself holds. It never uses the error's
  `Display` (it quotes the instance), the instance path, a value or the name of
  a key the message carried; an extra field is "a field the schema does not
  allow". `words` shows five findings of at most 200 characters, hidden
  characters escaped (`am_domain::text::escape_hidden`), and counts the rest.
- **Repair.** AD-36's one repair is the engine's (`run.rs`); this module only
  says whether a message matches and why not.

## History (story 1.4, 2026-11-20)

Before the REPLAN, MIT-0 was outside the allow-list, so story 1.4 did not link
jsonschema. `am-config` (`kinds::schema` and `schema_check`) checks each schema file
without a dependency: valid JSON (serde_json), a top-level object with
`schema_version: 1`, no secret-looking values, and a structural check of the
common keywords (`type` names, `required` as a list of names, `properties`,
`$defs`, `items`, `enum`, `additionalProperties` and the combinators holding
schemas, `$ref` local to the file). It is not a full meta-schema validation.
