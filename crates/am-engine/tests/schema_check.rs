//! Epic 3 story 5 (AD-3, AD-12, AD-36): `am_engine::schema`, the one file
//! that names the `jsonschema` crate. A step's schema is compiled before
//! any run row is written (`compile`); a final message is checked against
//! it (`check`); a failure is put in words (`words`).
//!
//! - **Words never echo the model's text.** Each finding is a rule and a
//!   place built from the schema (the failing keyword and the schema's own
//!   property names), never the message's values or its own key names. At
//!   most 5 findings, each at most 200 characters with hidden characters
//!   escaped; the rest are counted.
//! - **Not JSON** is its own finding ("its final message is not JSON"); the
//!   engine strips no code fence and fixes no quote.
//! - **No fetch, no read.** A `$ref` that is not local, a pattern the
//!   linear-time engine refuses (look-around, a back-reference) and an
//!   unknown `type` fail `compile`, in words that name the schema's id;
//!   nothing is fetched and no file is read.
//! - The file's own `schema_version` key is an unknown keyword and is
//!   ignored.
//!
//! No network and no real model; the one loopback listener below only
//! proves nothing connects to it. Written before `am_engine::schema`
//! exists.
#![forbid(unsafe_code)]

use std::io::ErrorKind;
use std::net::TcpListener;

use am_engine::schema::{check, compile, words};
use serde_json::{Value, json};

fn line_schema() -> Value {
    json!({
        "schema_version": 2,
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "required": ["line"],
        "additionalProperties": false,
        "properties": { "line": { "type": "string" } }
    })
}

/// The words `check` gives for `message` against `schema`, or a panic when
/// it passes.
fn worded(schema: &Value, message: &str) -> String {
    let compiled = compile("line", schema).unwrap_or_else(|e| panic!("compiles: {e}"));
    match check(&compiled, message) {
        Ok(()) => panic!("{message} should not match {schema}"),
        Err(findings) => {
            assert!(!findings.is_empty(), "a failure has a finding");
            words(&findings)
        }
    }
}

/// An object schema that requires one string field called `name`.
fn requiring(name: &str) -> Value {
    let mut properties = serde_json::Map::new();
    properties.insert(name.to_string(), json!({ "type": "string" }));
    json!({ "type": "object", "required": [name], "properties": properties })
}

fn holds_number(text: &str, n: u64) -> bool {
    let n = n.to_string();
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .any(|w| w == n)
}

/// A message that matches passes, with surrounding whitespace and a
/// newline (the engine trims, then parses).
#[test]
fn a_matching_message_passes() {
    let compiled = compile("line", &line_schema()).unwrap();

    assert!(check(&compiled, r#"{"line":"hello"}"#).is_ok());
    assert!(check(&compiled, "  {\"line\": \"hello\"}\n\n").is_ok());
}

/// Row 21: a type clash, an unexpected key and a value outside an enum are
/// worded from the schema: the schema's own property names and the rule,
/// and none of the canaries the message carried (a string value, an
/// unexpected key's name, an enum-breaking value).
#[test]
fn a_mismatch_is_worded_from_the_schema_not_the_message() {
    let schema = json!({
        "type": "object",
        "required": ["count", "name"],
        "additionalProperties": false,
        "properties": {
            "count": { "type": "integer" },
            "name": { "type": "string" },
            "kind": { "enum": ["a", "b"] }
        }
    });

    let clash = worded(
        &schema,
        r#"{"count":"canary-value-71c0","name":"ok","kind":"canary-enum-92d4"}"#,
    );
    assert!(
        clash.contains("count") && clash.contains("integer"),
        "{clash}"
    );
    assert!(clash.contains("kind"), "{clash}");

    let extra = worded(&schema, r#"{"count":1,"name":"ok","canary-key-5e18":true}"#);
    assert!(
        extra.contains("not allow"),
        "an extra field, unnamed: {extra}"
    );

    let missing = worded(&schema, r#"{"name":"ok"}"#);
    assert!(missing.contains("count"), "{missing}");

    let wrong_shape = worded(&schema, r#"["canary-item-3a9f"]"#);
    assert!(wrong_shape.contains("object"), "{wrong_shape}");

    for text in [&clash, &extra, &missing, &wrong_shape] {
        assert!(!text.contains("canary"), "never the message's text: {text}");
    }
}

/// Row 20: prose, a fenced code block and nothing are not JSON, in one
/// phrase; the engine does not strip the fence.
#[test]
fn not_json_is_worded() {
    let compiled = compile("line", &line_schema()).unwrap();
    for message in [
        "Here is the answer: it works",
        "```json\n{\"line\":\"hello\"}\n```",
        "{\"line\": \"hello\"",
        "",
    ] {
        let findings = check(&compiled, message).expect_err(message);
        let text = words(&findings);
        assert!(
            text.contains("its final message is not JSON"),
            "{message:?}: {text}"
        );
        assert!(!text.contains("hello"), "{text}");
        assert!(!text.contains("Here is"), "{text}");
    }
}

/// At most 5 findings are worded; the rest are counted. Eight missing
/// fields name five of them and the number three.
#[test]
fn findings_are_capped_at_five() {
    let names = [
        "field_a", "field_b", "field_c", "field_d", "field_e", "field_f", "field_g", "field_h",
    ];
    let properties: serde_json::Map<String, Value> = names
        .iter()
        .map(|n| (n.to_string(), json!({ "type": "string" })))
        .collect();
    let schema = json!({ "type": "object", "required": names, "properties": properties });

    let text = worded(&schema, "{}");

    let named = names.iter().filter(|n| text.contains(**n)).count();
    assert_eq!(named, 5, "five findings are worded: {text}");
    assert!(
        holds_number(&text, 3),
        "the other three are counted: {text}"
    );
}

/// Each finding is cut at 200 characters and its hidden characters are
/// written as `\u{XXXX}`: a property name of 300 characters, or one with a
/// bidi override, gives a short, plain line.
#[test]
fn findings_are_cut_and_hidden_characters_escaped() {
    let long = "x".repeat(300);
    let schema = requiring(&long);
    let text = worded(&schema, "{}");
    assert!(
        text.chars().count() < 260,
        "{} characters: {text}",
        text.chars().count()
    );

    let hidden = "na\u{202e}me";
    let schema = requiring(hidden);
    let text = worded(&schema, "{}");
    assert!(!text.contains('\u{202e}'), "{text:?}");
    assert!(text.contains("\\u{202E}"), "{text:?}");
}

/// Row 26: a `$ref` to a remote document makes the build fail, in words
/// naming the schema's id, and nothing connects (a loopback listener of
/// the test sees no connection). A reference to another document by a
/// relative name fails too.
#[test]
fn a_remote_ref_is_refused_and_nothing_is_fetched() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    for target in [
        format!("http://127.0.0.1:{port}/line.json"),
        format!("https://127.0.0.1:{port}/line.json"),
        "other.json".to_string(),
    ] {
        let schema = json!({
            "type": "object",
            "properties": { "line": { "$ref": target } }
        });

        let refused = compile("remote-line", &schema).expect_err(&target);

        let words = refused.to_string();
        assert!(words.contains("remote-line"), "names the schema: {words}");
    }
    assert_eq!(
        listener
            .accept()
            .expect_err("no connection was made")
            .kind(),
        ErrorKind::WouldBlock
    );
}

/// Row 26: a `$ref` to a file, as a path or a `file:` URL, makes the build
/// fail naming the schema's id; the file's text (a canary) is not read
/// into the error.
#[test]
fn a_file_ref_is_refused_and_no_file_is_read() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("line.json");
    std::fs::write(
        &file,
        r#"{"type":"integer","description":"canary-file-ref-b7e3"}"#,
    )
    .unwrap();
    for target in [
        file.display().to_string(),
        format!("file://{}", file.display()),
    ] {
        let schema = json!({
            "type": "object",
            "properties": { "line": { "$ref": target } }
        });

        let refused = compile("file-line", &schema).expect_err(&target);

        let words = refused.to_string();
        assert!(words.contains("file-line"), "names the schema: {words}");
        assert!(!words.contains("canary-file-ref"), "{words}");
    }
}

/// A reference into the schema's own document is fine.
#[test]
fn a_local_ref_compiles_and_checks() {
    let schema = json!({
        "type": "object",
        "required": ["line"],
        "properties": { "line": { "$ref": "#/$defs/text" } },
        "$defs": { "text": { "type": "string" } }
    });
    let compiled = compile("local", &schema).unwrap();

    assert!(check(&compiled, r#"{"line":"x"}"#).is_ok());
    assert!(check(&compiled, r#"{"line":1}"#).is_err());
}

/// Row 27: a pattern the linear-time engine cannot run (look-around, a
/// back-reference) and an unknown `type` fail the build in words naming
/// the id; an ordinary pattern works.
#[test]
fn a_lookaround_pattern_is_refused() {
    for (what, schema) in [
        (
            "look-ahead",
            json!({ "type": "string", "pattern": "(?=a)b" }),
        ),
        (
            "look-behind",
            json!({ "type": "string", "pattern": "(?<=a)b" }),
        ),
        (
            "back-reference",
            json!({ "type": "string", "pattern": "(a)\\1" }),
        ),
        ("an unknown type", json!({ "type": "strng" })),
    ] {
        let refused = compile("patterned", &schema).expect_err(what);

        let words = refused.to_string();
        assert!(words.contains("patterned"), "{what}: {words}");
    }

    let compiled = compile(
        "patterned",
        &json!({ "type": "string", "pattern": "^[a-z]+$" }),
    )
    .unwrap();
    assert!(check(&compiled, "\"abc\"").is_ok());
    assert!(check(&compiled, "\"ABC\"").is_err());
}

/// The file's own `schema_version` key is an unknown keyword to the
/// validator and is ignored (the schema above holds it and compiles).
#[test]
fn the_file_schema_version_key_is_ignored() {
    let with = line_schema();
    let mut without = line_schema();
    without.as_object_mut().unwrap().remove("schema_version");

    for schema in [&with, &without] {
        let compiled = compile("line", schema).unwrap();
        assert!(check(&compiled, r#"{"line":"x"}"#).is_ok());
        assert!(check(&compiled, r#"{"line":2}"#).is_err());
    }
}

// ------------------------------------------------------- review round 1

/// R13 (review round 1): a finding is cut before its hidden characters are
/// escaped, on a character boundary, so a cut never splits a `\u{XXXX}`:
/// with a bidi override at each position around the 200-character mark,
/// every `\u{` in the words is a whole `\u{202E}`.
#[test]
fn a_cut_never_splits_an_escape() {
    for at in 120..230 {
        let name = format!("{}\u{202e}{}", "x".repeat(at), "y".repeat(40));

        let text = worded(&requiring(&name), "{}");

        for (i, _) in text.match_indices("\\u{") {
            assert!(
                text[i..].starts_with("\\u{202E}"),
                "at {at}: a split escape: {text:?}"
            );
        }
        assert!(!text.contains('\u{202e}'), "at {at}: {text:?}");
    }
}

/// R13: with more findings than the validator is read for, the count is
/// said as a lower bound ("at least"); with a few more than five it stays
/// exact (see `findings_are_capped_at_five`).
#[test]
fn a_very_long_list_of_findings_is_counted_as_at_least() {
    let names: Vec<String> = (0..1500).map(|i| format!("f{i}")).collect();
    let mut properties = serde_json::Map::new();
    for n in &names {
        properties.insert(n.clone(), json!({ "type": "string" }));
    }
    let schema = json!({ "type": "object", "required": names, "properties": properties });

    let text = worded(&schema, "{}");

    assert!(text.contains("at least"), "{text}");
    assert!(
        text.split(|c: char| !c.is_ascii_alphanumeric())
            .any(|w| w.parse::<u64>().is_ok_and(|n| n >= 900)),
        "a count of the rest: {text}"
    );
}
