//! A structural check of `schemas/<id>.json` without a JSON Schema crate:
//! the common keywords must hold the shapes JSON Schema gives them, and a
//! `$ref` must point inside the file. `jsonschema` is not in the workspace
//! (licence conflict, docs/crate-notes/jsonschema.md); this is not a full
//! meta-schema validation.

use crate::finding::Report;
use crate::node::{Kind, Node};

const TYPES: &[&str] = &[
    "null", "boolean", "object", "array", "number", "string", "integer",
];
/// Keywords whose value is one schema.
const ONE_SCHEMA: &[&str] = &[
    "additionalProperties",
    "additionalItems",
    "not",
    "contains",
    "propertyNames",
    "if",
    "then",
    "else",
    "unevaluatedProperties",
    "unevaluatedItems",
];
/// Keywords whose value is a list of schemas.
const SCHEMA_LISTS: &[&str] = &["allOf", "anyOf", "oneOf", "prefixItems"];
/// Keywords whose value maps names to schemas.
const SCHEMA_MAPS: &[&str] = &[
    "properties",
    "patternProperties",
    "$defs",
    "definitions",
    "dependentSchemas",
];
const NUMBERS: &[&str] = &["minimum", "maximum", "multipleOf"];
const COUNTS: &[&str] = &[
    "minLength",
    "maxLength",
    "minItems",
    "maxItems",
    "minProperties",
    "maxProperties",
];
const FIX: &str = "follow JSON Schema draft 2020-12 (https://json-schema.org) for this keyword";

fn bad(rep: &mut Report, line: u32, message: String) {
    rep.error(line, format!("not a valid JSON Schema: {message}"), FIX);
}

/// Checks one schema node; `root` skips the file's own `schema_version`.
pub(crate) fn check(rep: &mut Report, node: &Node, root: bool) {
    let entries = match &node.kind {
        Kind::Map(entries) => entries,
        Kind::Bool(_) => return,
        _ => {
            bad(
                rep,
                node.line,
                format!(
                    "a schema must be an object or true/false, not {}",
                    node.type_name()
                ),
            );
            return;
        }
    };
    for (key, value) in entries {
        let k = key.name.as_str();
        let line = key.line;
        match k {
            "schema_version" if root => {}
            "type" => match &value.kind {
                Kind::Str(t) => type_name(rep, value.line, t),
                Kind::Seq(items) if !items.is_empty() => {
                    for item in items {
                        match &item.kind {
                            Kind::Str(t) => type_name(rep, item.line, t),
                            _ => bad(rep, item.line, "each 'type' entry must be a type name".into()),
                        }
                    }
                }
                _ => bad(rep, line, "'type' must be a type name or a list of them".into()),
            },
            "required" => match &value.kind {
                Kind::Seq(items) if items.iter().all(|i| matches!(i.kind, Kind::Str(_))) => {}
                _ => bad(rep, line, "'required' must be a list of property names".into()),
            },
            "enum" => match &value.kind {
                Kind::Seq(items) if !items.is_empty() => {}
                _ => bad(rep, line, "'enum' must be a non-empty list".into()),
            },
            "$ref" => match &value.kind {
                Kind::Str(r) if r.starts_with('#') => {}
                _ => bad(
                    rep,
                    line,
                    "'$ref' must point inside this file (start with '#'); remote and file references are not read".into(),
                ),
            },
            "pattern" | "$schema" | "$id" | "title" | "description" | "format" | "$comment"
                if !matches!(value.kind, Kind::Str(_)) =>
            {
                bad(rep, line, format!("'{k}' must be text"));
            }
            "items" => match &value.kind {
                Kind::Seq(items) => items.iter().for_each(|i| check(rep, i, false)),
                _ => check(rep, value, false),
            },
            "exclusiveMinimum" | "exclusiveMaximum"
                if !matches!(value.kind, Kind::Int(_) | Kind::Float(_) | Kind::Bool(_)) =>
            {
                bad(rep, line, format!("'{k}' must be a number"));
            }
            _ if ONE_SCHEMA.contains(&k) => check(rep, value, false),
            _ if SCHEMA_LISTS.contains(&k) => match &value.kind {
                Kind::Seq(items) if !items.is_empty() => {
                    items.iter().for_each(|i| check(rep, i, false))
                }
                _ => bad(rep, line, format!("'{k}' must be a non-empty list of schemas")),
            },
            _ if SCHEMA_MAPS.contains(&k) => match &value.kind {
                Kind::Map(props) => props.iter().for_each(|(_, v)| check(rep, v, false)),
                _ => bad(rep, line, format!("'{k}' must map names to schemas")),
            },
            _ if NUMBERS.contains(&k) && !matches!(value.kind, Kind::Int(_) | Kind::Float(_)) => {
                bad(rep, line, format!("'{k}' must be a number"));
            }
            _ if COUNTS.contains(&k) && !matches!(value.kind, Kind::Int(n) if n >= 0) => {
                bad(rep, line, format!("'{k}' must be a whole number from 0 up"));
            }
            // Other keywords and annotations are allowed as JSON Schema allows them.
            _ => {}
        }
    }
}

fn type_name(rep: &mut Report, line: u32, t: &str) {
    if !TYPES.contains(&t) {
        rep.error(
            line,
            format!("not a valid JSON Schema: '{t}' is not a JSON type"),
            format!("use one of: {}", TYPES.join(", ")),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::parse;

    fn problems(json: &str) -> Vec<(u32, String)> {
        let mut rep = Report::new("schemas/x.json");
        check(&mut rep, &parse(json, 0).ok().unwrap(), true);
        rep.findings
            .into_iter()
            .map(|f| (f.line, f.message))
            .collect()
    }

    #[test]
    fn a_plain_object_schema_passes() {
        let ok = r##"{"schema_version": 1, "type": "object", "required": ["a"],
            "properties": {"a": {"type": ["string", "null"]}, "b": {"$ref": "#/$defs/n"}},
            "$defs": {"n": {"type": "integer", "minimum": 0}},
            "items": true, "anyOf": [{"type": "string"}, {"enum": [1, 2]}]}"##;
        assert!(problems(ok).is_empty(), "{:?}", problems(ok));
    }

    #[test]
    fn shapes_are_checked_where_they_occur() {
        let p = problems(
            "{\n\"type\": \"thing\",\n\"required\": \"a\",\n\"properties\": {\"x\": 3},\n\"$ref\": \"https://example.org/s.json\",\n\"minLength\": -1\n}",
        );
        let lines: Vec<u32> = p.iter().map(|(l, _)| *l).collect();
        assert_eq!(lines, vec![2, 3, 4, 5, 6], "{p:?}");
        assert!(p[0].1.contains("'thing' is not a JSON type"));
    }
}
