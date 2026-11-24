//! Step output schemas (epic 3 story 5; AD-3, AD-12, AD-36): the only file
//! that names the `jsonschema` crate. Its types never leave this module:
//! [`OutputSchema`] holds a plain `serde_json::Value`, [`Compiled`] hides
//! the validator.
//!
//! - **Compiled before any row is written.** [`compile`] builds the
//!   validator from the schema value: no retriever (the crate is built with
//!   its defaults off, so no `resolve-http` or `resolve-file`, and the
//!   options say `offline`), so a `$ref` that is not into the schema's own
//!   document, and a relative `$ref` to another document, fail the build.
//!   Nothing connects and no file is read. The pattern engine is the
//!   linear-time `regex` one with size limits (a model's text is the
//!   subject of a `pattern`, and a backtracking engine can be made to take
//!   exponential time on it): look-around and back-references fail the
//!   build, in words that name the schema's id.
//! - **Words come from the schema, never the message.** [`check`] finds a
//!   rule and a place for each failure (the keyword, and the property names
//!   the schema itself holds, read from the evaluation path); it never uses
//!   the error's `Display` (it quotes the instance), a value, or the name
//!   of a key the message carried. An extra field is "a field the schema
//!   does not allow", unnamed. [`words`] shows at most five findings of at
//!   most 200 characters each, hidden characters escaped, and counts the
//!   rest.
//! - **Nothing is repaired here.** The message is trimmed and parsed; a
//!   code fence or a stray quote is a finding, not something the engine
//!   fixes (no silent conversion).

use am_domain::Instruction;
use jsonschema::PatternOptions;
use jsonschema::error::ValidationErrorKind;
use serde_json::Value;

/// How many findings [`words`] shows.
const SHOWN: usize = 5;
/// The longest a finding's text may be, in characters.
const FINDING_CHARS: usize = 200;
/// The most validation errors read from one message (a hostile message can
/// make a validator report an error per item).
const READ_AT_MOST: usize = 1000;
/// Limits on a compiled `pattern` (bytes): the linear-time engine's own.
const PATTERN_SIZE_LIMIT: usize = 1 << 20;

/// A step's output schema, as am-app builds it from `schemas/<id>.json`:
/// its id, its value, and the instruction a repair call carries (config
/// text; am-engine never authors an `Instruction`).
#[derive(Debug, Clone)]
pub struct OutputSchema {
    pub id: String,
    pub schema: Value,
    pub repair: Instruction,
}

impl OutputSchema {
    pub fn new(id: impl Into<String>, schema: Value, repair: Instruction) -> OutputSchema {
        OutputSchema {
            id: id.into(),
            schema,
            repair,
        }
    }
}

/// A schema that cannot be used, in words that name its id.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct SchemaError(String);

/// A schema built into a validator.
#[derive(Debug)]
pub struct Compiled {
    validator: jsonschema::Validator,
}

/// One reason a message does not match: a rule and a place, as words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding(String);

/// Builds `schema` (the file `schemas/<schema_id>.json`'s value) into a
/// validator with no way to fetch or read anything.
pub fn compile(schema_id: &str, schema: &Value) -> Result<Compiled, SchemaError> {
    let patterns = PatternOptions::regex()
        .size_limit(PATTERN_SIZE_LIMIT)
        .dfa_size_limit(PATTERN_SIZE_LIMIT);
    jsonschema::options()
        .offline()
        .with_pattern_options(patterns)
        .build(schema)
        .map(|validator| Compiled { validator })
        .map_err(|e| {
            SchemaError(format!(
                "the output schema `{schema_id}` cannot be used: {}",
                one_line(&e.to_string())
            ))
        })
}

/// Checks a final `message` against the schema: trimmed, parsed as JSON,
/// validated. `Err` holds each failure as a [`Finding`].
pub fn check(compiled: &Compiled, message: &str) -> Result<(), Vec<Finding>> {
    let Ok(value) = serde_json::from_str::<Value>(message.trim()) else {
        return Err(vec![Finding("its final message is not JSON".into())]);
    };
    let findings: Vec<Finding> = compiled
        .validator
        .iter_errors(&value)
        .take(READ_AT_MOST)
        .map(|e| Finding(finding_of(&e)))
        .collect();
    if findings.is_empty() {
        Ok(())
    } else {
        Err(findings)
    }
}

/// The findings in words: at most five, each cut at 200 characters with
/// hidden characters escaped, the rest counted.
pub fn words(findings: &[Finding]) -> String {
    let mut parts: Vec<String> = findings
        .iter()
        .take(SHOWN)
        .map(|f| one_line(&cut(&f.0)))
        .collect();
    if findings.len() > SHOWN {
        let more = findings.len() - SHOWN;
        // At the read limit the validator may have had more to say.
        let bound = if findings.len() >= READ_AT_MOST {
            "at least "
        } else {
            ""
        };
        parts.push(format!(
            "and {bound}{more} more finding{}",
            if more == 1 { "" } else { "s" }
        ));
    }
    parts.join("; ")
}

/// One finding for a validation error: its place and its rule, from the
/// schema alone.
fn finding_of(error: &jsonschema::ValidationError<'_>) -> String {
    let place = place_of(&error.evaluation_path().to_string());
    let rule = match error.kind() {
        ValidationErrorKind::Required { property } => {
            let name = property.as_str().unwrap_or("a required field");
            format!("field `{name}` is required and missing")
        }
        ValidationErrorKind::Type { kind } => format!("must be of type {}", type_words(kind)),
        ValidationErrorKind::AdditionalProperties { .. }
        | ValidationErrorKind::UnevaluatedProperties { .. } => {
            "has a field the schema does not allow".to_string()
        }
        ValidationErrorKind::AdditionalItems { .. }
        | ValidationErrorKind::UnevaluatedItems { .. } => {
            "has an item the schema does not allow".to_string()
        }
        ValidationErrorKind::Enum { .. } => "must be one of the values the schema lists".into(),
        ValidationErrorKind::Constant { .. } => "must be the one value the schema fixes".into(),
        ValidationErrorKind::Pattern { .. } => "must match the pattern the schema gives".into(),
        ValidationErrorKind::MinLength { limit } => {
            format!("must be at least {limit} characters long")
        }
        ValidationErrorKind::MaxLength { limit } => {
            format!("must be at most {limit} characters long")
        }
        ValidationErrorKind::MinItems { limit } => format!("must have at least {limit} items"),
        ValidationErrorKind::MaxItems { limit } => format!("must have at most {limit} items"),
        ValidationErrorKind::MinProperties { limit } => {
            format!("must have at least {limit} fields")
        }
        ValidationErrorKind::MaxProperties { limit } => {
            format!("must have at most {limit} fields")
        }
        ValidationErrorKind::Minimum { limit } => format!("must be at least {limit}"),
        ValidationErrorKind::Maximum { limit } => format!("must be at most {limit}"),
        ValidationErrorKind::ExclusiveMinimum { limit } => {
            format!("must be more than {limit}")
        }
        ValidationErrorKind::ExclusiveMaximum { limit } => {
            format!("must be less than {limit}")
        }
        ValidationErrorKind::UniqueItems => "must have no repeated items".into(),
        ValidationErrorKind::FalseSchema => "is not allowed by the schema".into(),
        other => format!("breaks the schema's `{}` rule", other.keyword()),
    };
    format!("{place}: {rule}")
}

/// The expected type(s) of a `type` rule, as the schema names them.
fn type_words(kind: &jsonschema::error::TypeKind) -> String {
    match kind {
        jsonschema::error::TypeKind::Single(t) => t.to_string(),
        jsonschema::error::TypeKind::Multiple(set) => {
            let names: Vec<String> = (*set).into_iter().map(|t| t.to_string()).collect();
            names.join(" or ")
        }
    }
}

/// Where in the message a rule applies, from the rule's path in the schema
/// (`/properties/line/type`): the schema's own property names, `each item`
/// for an array's items, `the message` for the root. A `$ref`, a
/// combinator's index and a `$defs` name say nothing about the place.
fn place_of(evaluation_path: &str) -> String {
    let segments: Vec<String> = evaluation_path
        .split('/')
        .skip(1)
        .map(|s| s.replace("~1", "/").replace("~0", "~"))
        .collect();
    let mut place: Vec<String> = Vec::new();
    let mut i = 0;
    while i + 1 < segments.len() {
        match segments[i].as_str() {
            "properties" => {
                if i + 2 < segments.len() {
                    place.push(format!("field `{}`", segments[i + 1]));
                    i += 2;
                    continue;
                }
                break;
            }
            "patternProperties" => {
                place.push("a field the schema names by pattern".into());
                i += 2;
                continue;
            }
            "items" | "additionalItems" | "unevaluatedItems" => place.push("each item".into()),
            "prefixItems" => {
                place.push(format!("item {}", segments.get(i + 1).map_or("?", |s| s)));
                i += 2;
                continue;
            }
            "additionalProperties" | "unevaluatedProperties" => {
                place.push("any other field".into());
            }
            "$defs" | "definitions" | "allOf" | "anyOf" | "oneOf" | "dependentSchemas" => {
                i += 2;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    if place.is_empty() {
        "the message".into()
    } else {
        place.join(", in ")
    }
}

/// `text` on one line, hidden characters written as `\u{XXXX}`.
fn one_line(text: &str) -> String {
    am_domain::text::escape_hidden(text)
        .0
        .replace(['\n', '\t'], " ")
}

/// At most [`FINDING_CHARS`] characters, cut on a character boundary and
/// before any escaping (so a cut never splits a `\u{XXXX}`).
fn cut(text: &str) -> String {
    if text.chars().count() <= FINDING_CHARS {
        text.to_string()
    } else {
        let mut cut: String = text.chars().take(FINDING_CHARS).collect();
        cut.push('\u{2026}');
        cut
    }
}
