//! A YAML document as a tree of nodes that remember their line, read through
//! serde-saphyr's `Spanned`. Comments come from the same parser's event
//! stream (granit-parser, re-exported by serde-saphyr), so a `#` inside a
//! quoted value or a block scalar is never taken for a comment.

use std::fmt;

use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_saphyr::Spanned;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Kind {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Seq(Vec<Node>),
    Map(Vec<(Key, Node)>),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Node {
    pub line: u32,
    pub kind: Kind,
    /// A boolean or number exactly as written (`1.10`, `true`), so a field
    /// read as text keeps the owner's spelling.
    pub raw: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Key {
    pub name: String,
    pub line: u32,
}

impl Node {
    pub fn as_map(&self) -> Option<&[(Key, Node)]> {
        match &self.kind {
            Kind::Map(m) => Some(m),
            _ => None,
        }
    }

    /// A short name for the node's type, for messages.
    pub fn type_name(&self) -> &'static str {
        match self.kind {
            Kind::Null => "nothing",
            Kind::Bool(_) => "true or false",
            Kind::Int(_) => "a whole number",
            Kind::Float(_) => "a number",
            Kind::Str(_) => "text",
            Kind::Seq(_) => "a list",
            Kind::Map(_) => "key: value pairs",
        }
    }

    /// True when the value holds something: not null, not empty text, not an
    /// empty list or mapping.
    pub fn is_non_empty(&self) -> bool {
        match &self.kind {
            Kind::Null => false,
            Kind::Str(s) => !s.trim().is_empty(),
            Kind::Seq(v) => !v.is_empty(),
            Kind::Map(m) => !m.is_empty(),
            _ => true,
        }
    }
}

/// The value as serde sees it, before lines are attached.
enum Raw {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Seq(Vec<Spanned<Raw>>),
    Map(Vec<(Spanned<String>, Spanned<Raw>)>),
}

impl<'de> Deserialize<'de> for Raw {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Raw;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a YAML value")
            }
            fn visit_unit<E>(self) -> Result<Raw, E> {
                Ok(Raw::Null)
            }
            fn visit_none<E>(self) -> Result<Raw, E> {
                Ok(Raw::Null)
            }
            fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<Raw, D::Error> {
                Raw::deserialize(d)
            }
            fn visit_bool<E>(self, v: bool) -> Result<Raw, E> {
                Ok(Raw::Bool(v))
            }
            fn visit_i64<E>(self, v: i64) -> Result<Raw, E> {
                Ok(Raw::Int(v))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Raw, E> {
                i64::try_from(v)
                    .map(Raw::Int)
                    .map_err(|_| E::custom("number too large"))
            }
            fn visit_f64<E>(self, v: f64) -> Result<Raw, E> {
                Ok(Raw::Float(v))
            }
            fn visit_str<E>(self, v: &str) -> Result<Raw, E> {
                Ok(Raw::Str(v.to_string()))
            }
            fn visit_string<E>(self, v: String) -> Result<Raw, E> {
                Ok(Raw::Str(v))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Raw, A::Error> {
                let mut out = Vec::new();
                while let Some(x) = a.next_element()? {
                    out.push(x);
                }
                Ok(Raw::Seq(out))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Raw, A::Error> {
                let mut out = Vec::new();
                while let Some(entry) = a.next_entry()? {
                    out.push(entry);
                }
                Ok(Raw::Map(out))
            }
        }
        d.deserialize_any(V)
    }
}

fn line_of(loc: &serde_saphyr::Location) -> u32 {
    u32::try_from(loc.line()).unwrap_or(u32::MAX)
}

/// The source text a location covers, when the parser knows its bytes.
fn slice(src: &str, loc: &serde_saphyr::Location) -> Option<String> {
    let span = loc.span();
    let start = usize::try_from(span.byte_offset()?).ok()?;
    let len = usize::try_from(span.byte_len()?).ok()?;
    src.get(start..start + len).map(str::to_string)
}

fn convert(src: &str, raw: Spanned<Raw>, offset: u32) -> Node {
    // serde-saphyr locates a `|` or `>` block scalar at its first content
    // line, not at the indicator (checked by a test below).
    let line = line_of(&raw.referenced) + offset;
    let mut text = None;
    let kind = match raw.value {
        Raw::Null => Kind::Null,
        Raw::Bool(b) => {
            text = slice(src, &raw.referenced);
            Kind::Bool(b)
        }
        Raw::Int(i) => {
            text = slice(src, &raw.referenced);
            Kind::Int(i)
        }
        Raw::Float(f) => {
            text = slice(src, &raw.referenced);
            Kind::Float(f)
        }
        Raw::Str(s) => Kind::Str(s),
        Raw::Seq(v) => Kind::Seq(v.into_iter().map(|x| convert(src, x, offset)).collect()),
        Raw::Map(m) => Kind::Map(
            m.into_iter()
                .map(|(k, v)| {
                    let key = Key {
                        line: line_of(&k.referenced) + offset,
                        name: k.value,
                    };
                    (key, convert(src, v, offset))
                })
                .collect(),
        ),
    };
    Node {
        line,
        kind,
        raw: text.map(|t| t.trim().to_string()),
    }
}

/// A parse failure: its 1-based line and the parser's message.
pub(crate) struct ParseError {
    pub line: u32,
    pub message: String,
}

/// Parses `text` (YAML, or JSON, which this parser also reads); every line is
/// shifted by `offset` (the frontmatter's position in a seat file).
pub(crate) fn parse(text: &str, offset: u32) -> Result<Node, ParseError> {
    // YAML 1.2 booleans only: `yes`, `no`, `on`, `off` stay text.
    let mut options = serde_saphyr::Options::default();
    options.strict_booleans = true;
    match serde_saphyr::from_str_with_options::<Spanned<Raw>>(text, options) {
        Ok(raw) => Ok(convert(text, raw, offset)),
        Err(e) => {
            let line = e.location().map(|l| line_of(&l)).unwrap_or(1).max(1) + offset;
            Err(ParseError {
                line,
                message: plain_message(&e.to_string()),
            })
        }
    }
}

/// The parser's message without its snippet or position prefix.
fn plain_message(rendered: &str) -> String {
    let first = rendered.lines().next().unwrap_or("").trim();
    let first = first.strip_prefix("error:").unwrap_or(first).trim();
    // "line 2 column 2: illegal placement…" → "illegal placement…"
    let rest = match first.strip_prefix("line ") {
        Some(after) => after.split_once(": ").map(|(_, msg)| msg).unwrap_or(first),
        None => first,
    };
    if rest.is_empty() {
        "the file is not valid YAML".to_string()
    } else {
        rest.to_string()
    }
}

/// The 1-based lines (plus `offset`) that hold a YAML comment.
pub(crate) fn comment_lines(text: &str, offset: u32) -> Vec<u32> {
    use serde_saphyr::granit_parser::{Event, Parser};
    let mut lines = Vec::new();
    for next in Parser::new_from_str(text) {
        let Ok((event, span)) = next else {
            break; // the parse error itself is reported elsewhere
        };
        if let Event::Comment(..) = event {
            let line = u32::try_from(span.start.line()).unwrap_or(u32::MAX) + offset;
            if !lines.contains(&line) {
                lines.push(line);
            }
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_follow_keys_and_values() {
        let n = parse("a: 1\nb:\n  - x\n  - y: z\n", 0).ok().unwrap();
        let m = n.as_map().unwrap();
        assert_eq!(m[0].0.line, 1);
        assert_eq!(m[1].0.line, 2);
        let Kind::Seq(items) = &m[1].1.kind else {
            panic!()
        };
        assert_eq!(items[0].line, 3);
        assert_eq!(items[1].as_map().unwrap()[0].0.line, 4);
    }

    #[test]
    fn scalars_keep_their_spelling_and_block_lines() {
        let n = parse("a: No\nb: 1.10\nc: true\nd: |\n  x\n  y\ne: >-\n  z\n", 0)
            .ok()
            .unwrap();
        let m = n.as_map().unwrap();
        assert_eq!(m[0].1.kind, Kind::Str("No".into()));
        assert_eq!(m[1].1.raw.as_deref(), Some("1.10"));
        assert_eq!(m[2].1.kind, Kind::Bool(true));
        assert_eq!(m[3].1.line, 5, "block content starts after the indicator");
        assert_eq!(m[4].1.line, 8);
    }

    #[test]
    fn offset_shifts_every_line() {
        let n = parse("a: 1\n", 1).ok().unwrap();
        assert_eq!(n.as_map().unwrap()[0].0.line, 2);
    }

    #[test]
    fn only_real_comments_are_found() {
        let text = "a: \"x # y\" # real\nb: |\n  # text\nc: 'q#'\n# real too\n";
        assert_eq!(comment_lines(text, 0), vec![1, 5]);
    }

    #[test]
    fn parse_errors_carry_a_line_and_a_short_message() {
        let e = parse("a: [1\nb: 2\n", 0).err().unwrap();
        assert_eq!(e.line, 2);
        assert!(!e.message.contains('\n'));
        assert!(!e.message.starts_with("line"), "{}", e.message);
        let dup = parse("a: 1\na: 2\n", 0).err().unwrap();
        assert_eq!(dup.line, 2);
        assert!(dup.message.contains("duplicate"), "{}", dup.message);
    }
}
