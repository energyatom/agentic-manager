//! The built-in `table_write` (epic 3 story 6, the ToolGate slice; AD-10,
//! AD-21 as amended 2026-11-24, owner Q1): a [`RowTool`] that builds the
//! tool's spec from the step's declared table and checks a call's arguments
//! into a [`PreparedRow`].
//!
//! It writes nothing and never reaches the store: ToolGate prepares a call
//! here, then writes it through the row-writer port (AD-21), the one path a
//! row takes. Everything here is built from config ids, types and fixed
//! words: the spec names the table and its columns with their types, and
//! every refusal is worded from the table's schema (column ids, types, a
//! status column's values), never from a key or value the model chose.
//!
//! The data is checked by hand against the declared columns (no cleaning,
//! no normalising, no status transitions): `data` is a JSON object; every
//! key a declared column; a value of the column's type (`text`, `url`,
//! `date` and `datetime` are strings, `number` a number, `integer` a whole
//! number, `boolean` a boolean, `status` one of its values, `json` any);
//! `null` only for a column that is not required; on an insert, every
//! required column present; each key named once in each object, at any
//! depth; at most 64 KiB. `null` means absent: for `row_id`, and for a
//! column on an insert (dropped) as on an update (cleared). The prepared
//! data is the model's own `data` text, as written: nothing is
//! re-serialised.

use am_domain::ToolSpec;
use am_domain::rows::{ColumnKind, ColumnSpec, DeclaredTable, PreparedRow, RowTool};
use serde_json::{Map, Value, json};

/// The tool's id (AD-11's built-in table).
pub const NAME: &str = "table_write";

/// The most bytes a call's `data` may hold: 64 KiB, the store's cap on a
/// row.
const MAX_DATA_BYTES: usize = 64 * 1024;

/// The words of a `row_id` that names no row the table holds.
const NO_SUCH_ROW: &str = "a row this table does not hold";

/// am-tools' `table_write`.
#[derive(Debug, Clone, Copy, Default)]
pub struct TableWrite;

impl TableWrite {
    pub fn new() -> TableWrite {
        TableWrite
    }
}

impl RowTool for TableWrite {
    fn spec(&self, table: &DeclaredTable) -> ToolSpec {
        let columns: Map<String, Value> = table
            .columns
            .iter()
            .map(|c| (c.id.clone(), column_schema(c)))
            .collect();
        let required: Vec<&str> = table
            .columns
            .iter()
            .filter(|c| c.required)
            .map(|c| c.id.as_str())
            .collect();
        let schema = json!({
            "type": "object",
            "properties": {
                "table": {
                    "type": "string",
                    "enum": [table.id],
                    "description": "The table this step writes.",
                },
                "data": {
                    "type": "object",
                    "properties": columns,
                    "additionalProperties": false,
                    "description": "The row's columns. On an insert, every required column; on an update, only the columns to change (null clears a column that is not required).",
                },
                "row_id": {
                    "type": "string",
                    "description": "Only for an update: the id of the row, as an earlier result of this tool gave it.",
                },
                "expected_version": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Only for an update: the row's version, as an earlier result of this tool gave it.",
                },
            },
            "required": ["table", "data"],
        });
        let required = if required.is_empty() {
            "none".to_string()
        } else {
            required.join(", ")
        };
        ToolSpec {
            name: NAME.to_string(),
            description: format!(
                "Writes one row of the table `{}`: an insert with `table` and `data`, or an update of a row with `table`, `row_id`, `expected_version` and the columns to change in `data`. Required columns on an insert: {required}. The result gives the row's id and its new version.",
                table.id
            ),
            input_schema_json: schema.to_string(),
        }
    }

    fn prepare(&self, arguments_json: &str, table: &DeclaredTable) -> Result<PreparedRow, String> {
        let args: Value = serde_json::from_str(arguments_json).map_err(|_| {
            format!(
                "the arguments are not JSON; write a JSON object with `table` (\"{}\") and `data`",
                table.id
            )
        })?;
        let Some(args) = args.as_object() else {
            return Err(format!(
                "the arguments are not a JSON object; write one with `table` (\"{}\") and `data`",
                table.id
            ));
        };
        if args.get("table").and_then(Value::as_str) != Some(table.id.as_str()) {
            return Err(format!(
                "this step writes only the table `{}`, and the call names another table or none",
                table.id
            ));
        }
        let Some(data) = args.get("data") else {
            return Err(format!(
                "the call has no `data`; `data` is a JSON object of the table `{}`'s columns",
                table.id
            ));
        };
        let Some(columns) = data.as_object() else {
            return Err(format!(
                "`data` is not a JSON object; it is an object of the table `{}`'s columns ({})",
                table.id,
                column_ids(table)
            ));
        };
        // The model's own text of `data`, as written.
        let raw = top_level_value(arguments_json, "data").ok_or_else(|| {
            "the arguments' `data` could not be read as written; write it again".to_string()
        })?;
        if raw.len() > MAX_DATA_BYTES {
            return Err(format!(
                "`data` holds {} bytes, over the 64 KiB a row may hold; write fewer or shorter values",
                raw.len()
            ));
        }
        // A key written twice in one object, at any depth, would be checked
        // at one value and stored with both: refused.
        if repeats_a_key(raw).is_none_or(|repeated| repeated) {
            return Err(format!(
                "`data` names a key more than once in one object (a column of the table `{}`, or a key inside a value); each key appears once in an object",
                table.id
            ));
        }
        check_columns(columns, table)?;
        // `null` means absent (review round 1, R7): for `row_id` and
        // `expected_version`, and for a column on an insert.
        let given = |key: &str| args.get(key).is_some_and(|v| !v.is_null());
        let update = given("row_id") || given("expected_version");
        if !update {
            for column in table.columns.iter().filter(|c| c.required) {
                if !columns.contains_key(&column.id) {
                    return Err(format!(
                        "the column `{}` is required on an insert, and `data` does not hold it",
                        column.id
                    ));
                }
            }
            return Ok(PreparedRow::Insert {
                data_json: without_nulls(raw).ok_or_else(|| {
                    "the arguments' `data` could not be read as written; write it again".to_string()
                })?,
            });
        }
        let row_id = args
            .get("row_id")
            .and_then(Value::as_str)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| {
                format!(
                    "`row_id` is not the id of a row of the table `{}` (an earlier result of this tool gives it), so it names {NO_SUCH_ROW}",
                    table.id
                )
            })?;
        let expected_version = args
            .get("expected_version")
            .and_then(Value::as_i64)
            .filter(|v| *v >= 1)
            .ok_or_else(|| {
                "an update needs `expected_version`, the row's version as an earlier result of this tool gave it (a whole number from 1)".to_string()
            })?;
        Ok(PreparedRow::Update {
            row_id,
            expected_version,
            patch_json: raw.to_string(),
        })
    }
}

/// A column's JSON schema: its type, a status column's values, nothing for
/// a `json` column (any value).
fn column_schema(column: &ColumnSpec) -> Value {
    match column.kind {
        ColumnKind::Text | ColumnKind::Url | ColumnKind::Date | ColumnKind::Datetime => {
            json!({ "type": "string" })
        }
        ColumnKind::Number => json!({ "type": "number" }),
        ColumnKind::Integer => json!({ "type": "integer" }),
        ColumnKind::Boolean => json!({ "type": "boolean" }),
        ColumnKind::Status => json!({ "enum": column.values }),
        ColumnKind::Json => json!({}),
    }
}

/// The table's column ids, for words.
fn column_ids(table: &DeclaredTable) -> String {
    table
        .columns
        .iter()
        .map(|c| c.id.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Every key a declared column and every value of its column's type; a
/// required column never `null`. Words from the table's schema only.
fn check_columns(columns: &Map<String, Value>, table: &DeclaredTable) -> Result<(), String> {
    for (key, value) in columns {
        let Some(column) = table.columns.iter().find(|c| &c.id == key) else {
            return Err(format!(
                "`data` names a column the table `{}` does not declare; its columns are: {}",
                table.id,
                column_ids(table)
            ));
        };
        if value.is_null() {
            if column.required {
                return Err(format!(
                    "the column `{}` is required, so it cannot be null",
                    column.id
                ));
            }
            continue;
        }
        let fits = match column.kind {
            ColumnKind::Text | ColumnKind::Url | ColumnKind::Date | ColumnKind::Datetime => {
                value.is_string()
            }
            ColumnKind::Number => value.is_number(),
            ColumnKind::Integer => value.is_i64() || value.is_u64(),
            ColumnKind::Boolean => value.is_boolean(),
            ColumnKind::Status => value
                .as_str()
                .is_some_and(|v| column.values.iter().any(|allowed| allowed == v)),
            ColumnKind::Json => true,
        };
        if !fits {
            return Err(match column.kind {
                ColumnKind::Status => format!(
                    "the column `{}` is a status column, and the value given is not one of its values: {}",
                    column.id,
                    column.values.join(", ")
                ),
                kind => format!(
                    "the column `{}` holds {} values ({}), and the value given is not one",
                    column.id,
                    kind.as_str(),
                    json_type(kind)
                ),
            });
        }
    }
    Ok(())
}

/// How a column's values are written in JSON, for words.
fn json_type(kind: ColumnKind) -> &'static str {
    match kind {
        ColumnKind::Text | ColumnKind::Url | ColumnKind::Date | ColumnKind::Datetime => {
            "a JSON string"
        }
        ColumnKind::Number => "a JSON number",
        ColumnKind::Integer => "a JSON whole number",
        ColumnKind::Boolean => "true or false",
        ColumnKind::Status => "a JSON string",
        ColumnKind::Json => "any JSON value",
    }
}

/// The text of the value of `key` in `text`, a JSON object serde_json has
/// already read (so its syntax is known to be valid): the last member of
/// that name, as JSON keeps it. `None` when there is none.
fn top_level_value<'t>(text: &'t str, key: &str) -> Option<&'t str> {
    members(text)?
        .into_iter()
        .rev()
        .find(|m| m.name == key)
        .map(|m| m.value)
}

/// `data`, an object, as written but without its `null` members (on an
/// insert, `null` means absent).
fn without_nulls(data: &str) -> Option<String> {
    let kept: Vec<String> = members(data)?
        .into_iter()
        .filter(|m| m.value != "null")
        .map(|m| format!("{}:{}", m.key, m.value))
        .collect();
    Some(format!("{{{}}}", kept.join(",")))
}

/// One member of a JSON object: its decoded name, and its key's and its
/// value's text as written.
struct Member<'t> {
    name: String,
    key: &'t str,
    value: &'t str,
}

/// The members of the JSON object `text` (already read by serde_json), in
/// order, duplicates kept. `None` when `text` is no object.
fn members(text: &str) -> Option<Vec<Member<'_>>> {
    let bytes = text.as_bytes();
    let mut i = skip_space(bytes, 0);
    if bytes.get(i) != Some(&b'{') {
        return None;
    }
    i += 1;
    let mut found = Vec::new();
    loop {
        i = skip_space(bytes, i);
        match bytes.get(i)? {
            b'}' => return Some(found),
            b',' => {
                i += 1;
                continue;
            }
            b'"' => {}
            _ => return None,
        }
        let key_end = string_end(bytes, i)?;
        let key = &text[i..key_end];
        let name: String = serde_json::from_str(key).ok()?;
        i = skip_space(bytes, key_end);
        if bytes.get(i) != Some(&b':') {
            return None;
        }
        i = skip_space(bytes, i + 1);
        let value_end = value_end(bytes, i)?;
        found.push(Member {
            name,
            key,
            value: &text[i..value_end],
        });
        i = value_end;
    }
}

/// Whether any object in the JSON value `text` (already read by
/// serde_json), at any depth, names a key twice. `None` when it cannot be
/// read.
fn repeats_a_key(text: &str) -> Option<bool> {
    walk(text, skip_space(text.as_bytes(), 0)).map(|(_, repeated)| repeated)
}

/// The end (exclusive) of the JSON value at `i` of `text`, and whether an
/// object in it names a key twice. serde_json bounds the nesting it reads
/// (the text was read by it first), so the recursion is bounded too.
fn walk(text: &str, i: usize) -> Option<(usize, bool)> {
    let bytes = text.as_bytes();
    let (open, close) = match bytes.get(i)? {
        b'{' => (b'{', b'}'),
        b'[' => (b'[', b']'),
        _ => return value_end(bytes, i).map(|end| (end, false)),
    };
    let mut names = std::collections::BTreeSet::new();
    let mut repeated = false;
    let mut j = i + 1;
    loop {
        j = skip_space(bytes, j);
        match *bytes.get(j)? {
            b if b == close => return Some((j + 1, repeated)),
            b',' => {
                j += 1;
                continue;
            }
            _ => {}
        }
        if open == b'{' {
            if bytes.get(j) != Some(&b'"') {
                return None;
            }
            let key_end = string_end(bytes, j)?;
            let name: String = serde_json::from_str(&text[j..key_end]).ok()?;
            repeated |= !names.insert(name);
            j = skip_space(bytes, key_end);
            if bytes.get(j) != Some(&b':') {
                return None;
            }
            j = skip_space(bytes, j + 1);
        }
        let (end, inner) = walk(text, j)?;
        repeated |= inner;
        j = end;
    }
}

fn skip_space(bytes: &[u8], mut i: usize) -> usize {
    while bytes.get(i).is_some_and(|b| b.is_ascii_whitespace()) {
        i += 1;
    }
    i
}

/// The end (exclusive) of the string that starts at `i` (its quote).
fn string_end(bytes: &[u8], i: usize) -> Option<usize> {
    let mut j = i + 1;
    while let Some(b) = bytes.get(j) {
        match b {
            b'\\' => j += 2,
            b'"' => return Some(j + 1),
            _ => j += 1,
        }
    }
    None
}

/// The end (exclusive) of the JSON value that starts at `i`.
fn value_end(bytes: &[u8], i: usize) -> Option<usize> {
    match bytes.get(i)? {
        b'"' => string_end(bytes, i),
        b'{' | b'[' => {
            let mut depth = 0usize;
            let mut j = i;
            while let Some(b) = bytes.get(j) {
                match b {
                    b'"' => {
                        j = string_end(bytes, j)?;
                        continue;
                    }
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(j + 1);
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            None
        }
        _ => {
            let mut j = i;
            while bytes
                .get(j)
                .is_some_and(|b| !matches!(b, b',' | b'}' | b']') && !b.is_ascii_whitespace())
            {
                j += 1;
            }
            (j > i).then_some(j)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_raw_value_is_the_last_member_of_its_name_as_written() {
        let text = r#" { "data" : {"a": 2.50, "b": [1, {"c": "}"}]}, "x": "\"data\"", "data":{"z" :1e2} } "#;
        assert_eq!(top_level_value(text, "data"), Some(r#"{"z" :1e2}"#));
        assert_eq!(top_level_value(text, "x"), Some(r#""\"data\"""#));
        assert_eq!(top_level_value(text, "none"), None);
        let escaped = r#"{"d\u0061ta": {"k": true}}"#;
        assert_eq!(top_level_value(escaped, "data"), Some(r#"{"k": true}"#));
        assert_eq!(members(r#"{"a":1,"a":2}"#).map(|m| m.len()), Some(2));
        assert_eq!(repeats_a_key(r#"{"a":[{"b":1,"b":2}]}"#), Some(true));
        assert_eq!(
            repeats_a_key(r#"{"a":{"a":1},"b":[{"a":1},{"a":2}]}"#),
            Some(false)
        );
        assert_eq!(
            without_nulls(r#"{"a" : null, "b":2.50,"c":"null"}"#).as_deref(),
            Some(r#"{"b":2.50,"c":"null"}"#)
        );
    }
}
