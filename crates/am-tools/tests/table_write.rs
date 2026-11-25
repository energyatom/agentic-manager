//! Epic 3 story 6 (AD-10, AD-21; owner Q1; matrix rows 1, 14 and 22): the
//! built-in `table_write` lives in am-tools as a `RowTool`. It builds the
//! tool's spec from the step's declared table (config ids and types only)
//! and checks a call's arguments into a prepared row; it writes nothing and
//! never calls the row-writer port (ToolGate does, `cargo xtask
//! check-deps`).
//!
//! - The spec names the table and every column with its type.
//! - A bad row is refused in words built from the table's schema (column
//!   ids, types, a status column's values), never from the model's keys or
//!   values: not an object, an undeclared column, a wrong type, a missing
//!   required column on insert, a required column set to `null`, a status
//!   value outside its values, data over 64 KiB, and a `row_id` that is not
//!   a UUID.
//!
//! No IO; tables are built by hand. Written before `table_write` exists.
#![forbid(unsafe_code)]

use am_domain::rows::{ColumnKind, ColumnSpec, DeclaredTable, PreparedRow, RowTool};
use am_tools::table_write::TableWrite;
use serde_json::{Value, json};

const ROW_ID: &str = "0192f0c4-9a3e-7c2b-8d4e-1f2a3b4c5d6e";

fn column(id: &str, kind: ColumnKind, required: bool, values: &[&str]) -> ColumnSpec {
    ColumnSpec {
        id: id.into(),
        kind,
        required,
        values: values.iter().map(|v| v.to_string()).collect(),
    }
}

/// `entries` of team `core`, one column of every type.
fn entries() -> DeclaredTable {
    DeclaredTable {
        id: "entries".into(),
        team: "core".into(),
        columns: vec![
            column("ref", ColumnKind::Text, true, &[]),
            column("title", ColumnKind::Text, false, &[]),
            column("amount", ColumnKind::Number, false, &[]),
            column("count", ColumnKind::Integer, false, &[]),
            column("done", ColumnKind::Boolean, false, &[]),
            column("due", ColumnKind::Date, false, &[]),
            column("at", ColumnKind::Datetime, false, &[]),
            column("link", ColumnKind::Url, false, &[]),
            column("state", ColumnKind::Status, false, &["new", "done"]),
            column("extra", ColumnKind::Json, false, &[]),
        ],
    }
}

/// `others` of team `side`: one required text column.
fn others() -> DeclaredTable {
    DeclaredTable {
        id: "others".into(),
        team: "side".into(),
        columns: vec![column("ref", ColumnKind::Text, true, &[])],
    }
}

fn prepare(args: Value) -> Result<PreparedRow, String> {
    TableWrite::new().prepare(&args.to_string(), &entries())
}

fn insert(data: Value) -> Value {
    json!({ "table": "entries", "data": data })
}

fn update(data: Value) -> Value {
    json!({ "table": "entries", "row_id": ROW_ID, "expected_version": 1, "data": data })
}

fn parsed(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|e| panic!("JSON ({e}): {text}"))
}

/// Row 1: the spec is the built-in `table_write`; its input schema is a
/// JSON object schema whose `table` property names the declared table,
/// whose `data` property names every column with its JSON type (a status
/// column by its values, a `json` column with no type), and which takes a
/// `row_id` and an `expected_version` (an update). Another table gives its
/// own ids, never the first's.
#[test]
fn the_spec_names_the_declared_table_and_columns() {
    let spec = TableWrite::new().spec(&entries());

    assert_eq!(spec.name, "table_write");
    let schema = parsed(&spec.input_schema_json);
    assert_eq!(schema["type"], "object", "{schema:#}");
    let table = schema["properties"]["table"].to_string();
    assert!(table.contains("\"entries\""), "names the table: {table}");
    let data = &schema["properties"]["data"]["properties"];
    for (id, ty) in [
        ("ref", "string"),
        ("title", "string"),
        ("amount", "number"),
        ("count", "integer"),
        ("done", "boolean"),
        ("due", "string"),
        ("at", "string"),
        ("link", "string"),
    ] {
        assert_eq!(data[id]["type"], ty, "{id}: {schema:#}");
    }
    assert_eq!(data["state"]["enum"], json!(["new", "done"]), "{schema:#}");
    assert!(data["extra"].is_object(), "the json column: {schema:#}");
    assert!(
        data["extra"].get("type").is_none(),
        "a json column takes any value: {schema:#}"
    );
    assert_eq!(
        data.as_object().map(|o| o.len()),
        Some(10),
        "every column, no other: {schema:#}"
    );
    assert!(schema["properties"]["row_id"].is_object(), "{schema:#}");
    assert!(
        schema["properties"]["expected_version"].is_object(),
        "{schema:#}"
    );

    let other = TableWrite::new().spec(&others());
    assert_eq!(other.name, "table_write");
    let text = format!("{} {}", other.description, other.input_schema_json);
    assert!(text.contains("others"), "{text}");
    assert!(
        !text.contains("entries"),
        "built from its own table: {text}"
    );
    assert!(!text.contains("amount"), "only its own columns: {text}");
}

/// Row 14: the seven bad rows are refused, each in words from the table's
/// schema; a good insert and a good update (a column cleared with `null`
/// included) are prepared as given, and an unknown top-level argument is
/// ignored.
#[test]
fn bad_rows_are_refused_in_words_from_the_columns() {
    // Controls.
    let full = json!({
        "ref": "r-1", "title": "first", "amount": 2.5, "count": 3, "done": true,
        "due": "2026-11-24", "at": "2026-11-24T10:00:00Z", "link": "https://example.org/x",
        "state": "new", "extra": { "any": [1, 2] }
    });
    match prepare(insert(full.clone())) {
        Ok(PreparedRow::Insert { data_json }) => assert_eq!(parsed(&data_json), full),
        other => panic!("a good insert is prepared: {other:?}"),
    }
    let mut with_note = insert(json!({ "ref": "r-2" }));
    with_note["note"] = json!("an argument the tool does not take");
    assert!(
        matches!(prepare(with_note), Ok(PreparedRow::Insert { .. })),
        "an unknown top-level argument is ignored"
    );
    match prepare(update(json!({ "title": "second", "amount": null }))) {
        Ok(PreparedRow::Update {
            row_id,
            expected_version,
            patch_json,
        }) => {
            assert_eq!(row_id.to_string(), ROW_ID);
            assert_eq!(expected_version, 1);
            assert_eq!(
                parsed(&patch_json),
                json!({ "title": "second", "amount": null })
            );
        }
        other => panic!("a good update is prepared: {other:?}"),
    }

    let refused = |what: &str, args: Value, words: &[&str]| {
        let why = prepare(args).expect_err(what);
        for w in words {
            assert!(why.contains(w), "{what}: names `{w}`: {why}");
        }
    };
    // 1. Not an object.
    refused("data is text", insert(json!("just text")), &["object"]);
    refused("data is a list", insert(json!([1, 2])), &["object"]);
    // 2. A column the table does not declare: the words list the declared ones.
    refused(
        "an undeclared column",
        insert(json!({ "ref": "r-1", "colour": "x" })),
        &["ref", "title", "state"],
    );
    // 3. A wrong type: the column and its type.
    refused(
        "text for a number",
        insert(json!({ "ref": "r-1", "amount": "lots" })),
        &["amount", "number"],
    );
    refused(
        "a fraction for an integer",
        insert(json!({ "ref": "r-1", "count": 2.5 })),
        &["count", "integer"],
    );
    refused(
        "text for a boolean",
        insert(json!({ "ref": "r-1", "done": "yes" })),
        &["done", "boolean"],
    );
    refused(
        "a number for text",
        update(json!({ "title": 5 })),
        &["title", "text"],
    );
    // 4. A required column missing on insert.
    refused(
        "no ref on insert",
        insert(json!({ "title": "x" })),
        &["ref", "required"],
    );
    // 5. A required column set to null, on insert or update.
    refused(
        "ref null on insert",
        insert(json!({ "ref": null })),
        &["ref", "required"],
    );
    refused(
        "ref null on update",
        update(json!({ "ref": null })),
        &["ref", "required"],
    );
    // 6. A status value outside its values.
    refused(
        "a status outside its values",
        insert(json!({ "ref": "r-1", "state": "archived" })),
        &["state", "new", "done"],
    );
    // 7. Data over 64 KiB.
    refused(
        "data over 64 KiB",
        insert(json!({ "ref": "r-1", "title": "x".repeat(64 * 1024 + 1) })),
        &["64 KiB"],
    );
    // And a row id that is not a UUID.
    refused(
        "a row id that is not a UUID",
        json!({ "table": "entries", "row_id": "17", "expected_version": 1, "data": { "title": "x" } }),
        &["a row this table does not hold"],
    );
}

/// Rows 14 and 22 (no model words in refusals): every refusal above, made
/// with canary keys and values, holds none of them; a `row_id` canary is
/// not echoed; and the words of an oversized row stay short.
#[test]
fn refusals_never_echo_the_models_keys_or_values() {
    const CANARIES: [&str; 8] = [
        "canary_key_k1",
        "canary-value-v2",
        "canary-state-v3",
        "canary-text-v4",
        "canary-title-v5",
        "canary-row-v6",
        "canary_arg_k7",
        "canary-big-v8",
    ];
    let cases = [
        insert(json!({ "ref": "r-1", "canary_key_k1": "x" })),
        insert(json!({ "ref": "r-1", "amount": "canary-value-v2" })),
        insert(json!({ "ref": "r-1", "state": "canary-state-v3" })),
        insert(json!("canary-text-v4")),
        insert(json!({ "title": "canary-title-v5" })),
        json!({ "table": "entries", "row_id": "canary-row-v6", "expected_version": 1, "data": { "title": "x" } }),
        json!({ "table": "entries", "canary_arg_k7": "x", "data": { "count": "canary-value-v2" } }),
        insert(json!({ "ref": "r-1", "title": format!("canary-big-v8{}", "y".repeat(70 * 1024)) })),
        update(json!({ "ref": null, "canary_key_k1": "canary-value-v2" })),
    ];
    for args in cases {
        let why = prepare(args.clone()).expect_err(&format!("refused: {args}"));
        for canary in CANARIES {
            assert!(
                !why.contains(canary),
                "the words never echo `{canary}`: {why}"
            );
        }
        assert!(
            why.len() < 1000,
            "the words are the tool's, not the row: {} bytes",
            why.len()
        );
    }
    let why = prepare(
        json!({ "table": "entries", "row_id": "canary-row-v6", "expected_version": 1, "data": {} }),
    )
    .expect_err("not a UUID");
    assert!(why.contains("a row this table does not hold"), "{why}");
}

/// Owner Q1: the tool never names the row-writer port's method or the
/// store's row command (the xtask rule is the real guard; this reads the
/// module's own text).
#[test]
fn table_write_names_no_row_writer() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/table_write.rs");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} must exist ({e})", path.display()));
    for name in ["write_row", "apply_row_write"] {
        assert!(
            !text.contains(name),
            "am-tools' table_write names `{name}`; only ToolGate calls the port (AD-21)"
        );
    }
}

// ------------------------------------------------- review round 1 (3.23)
//
// The ToolGate slice's review triage round 1 (manager, 2026-11-25): R1 (a
// key repeated at any depth of `data` is the tool's refusal, in words, so
// the model reads it as an untrusted error result and the step goes on),
// R6 (arguments ToolGate no longer refuses at admission reach `prepare`
// and are refused here in words) and R7 (`null` means absent for
// `row_id`). Written before the patches.

/// The prepared row for the raw arguments text `text` (a key may repeat in
/// it, which `serde_json::Value` cannot hold).
fn prepare_text(text: &str) -> Result<PreparedRow, String> {
    TableWrite::new().prepare(text, &entries())
}

/// R1 (a): a key repeated anywhere inside `data`, in a `json` column's
/// object or deeper inside an array, is refused in words that say a key
/// appears once and never name the key or its values. The same key in two
/// different objects is no repeat, and is prepared.
#[test]
fn a_key_repeated_at_any_depth_of_data_is_refused_in_words() {
    for (what, text) in [
        (
            "inside a json column's object",
            r#"{"table":"entries","data":{"ref":"r-1","extra":{"canary_dup_k9":"canary-v1","canary_dup_k9":"canary-v2"}}}"#,
        ),
        (
            "inside an object in an array",
            r#"{"table":"entries","data":{"ref":"r-1","extra":[1,{"a":{"canary_dup_k9":1,"canary_dup_k9":2}}]}}"#,
        ),
        (
            "inside an update's patch",
            r#"{"table":"entries","row_id":"0192f0c4-9a3e-7c2b-8d4e-1f2a3b4c5d6e","expected_version":1,"data":{"extra":{"canary_dup_k9":true,"canary_dup_k9":false}}}"#,
        ),
    ] {
        let why = prepare_text(text).expect_err(what);
        assert!(
            why.contains("once"),
            "{what}: says a key appears once: {why}"
        );
        for canary in ["canary_dup_k9", "canary-v1", "canary-v2"] {
            assert!(!why.contains(canary), "{what}: echoes `{canary}`: {why}");
        }
    }
    let same_key_twice_apart = r#"{"table":"entries","data":{"ref":"r-1","extra":{"a":1,"b":{"a":2},"c":[{"a":3},{"a":4}]}}}"#;
    match prepare_text(same_key_twice_apart) {
        Ok(PreparedRow::Insert { data_json }) => assert_eq!(
            parsed(&data_json),
            json!({"ref": "r-1", "extra": {"a": 1, "b": {"a": 2}, "c": [{"a": 3}, {"a": 4}]}})
        ),
        other => panic!("one key in different objects is no repeat: {other:?}"),
    }
}

/// R6: ToolGate admits a `table_write` whose `table` is not a string naming
/// another table, so `prepare` words each malformed call for the model: no
/// `table`, a `table` that is not a string, arguments that are not JSON,
/// and arguments that are JSON but no object. Each refusal names the
/// declared table and echoes nothing the model wrote.
#[test]
fn malformed_arguments_are_refused_in_words() {
    for (what, text) in [
        ("no table", r#"{"data":{"ref":"canary-mal-1"}}"#.to_string()),
        (
            "a table that is not a string",
            r#"{"table":7,"data":{"ref":"canary-mal-1"}}"#.to_string(),
        ),
        ("not JSON", "canary-mal-1 is not JSON {".to_string()),
        ("a JSON array", r#"["canary-mal-1"]"#.to_string()),
        ("a JSON string", r#""canary-mal-1""#.to_string()),
    ] {
        let why = prepare_text(&text).expect_err(what);
        assert!(why.contains("entries"), "{what}: names the table: {why}");
        assert!(!why.contains("canary-mal-1"), "{what}: echoes: {why}");
    }
}

/// R7: `null` means absent for `row_id`: a call with `"row_id": null` and
/// no `expected_version` is an insert, prepared like the same call without
/// the key.
#[test]
fn a_null_row_id_is_an_insert() {
    let with_null = json!({ "table": "entries", "row_id": null, "data": { "ref": "r-1" } });
    match prepare(with_null) {
        Ok(PreparedRow::Insert { data_json }) => {
            assert_eq!(parsed(&data_json), json!({ "ref": "r-1" }))
        }
        other => panic!("`row_id: null` is an insert: {other:?}"),
    }
}
