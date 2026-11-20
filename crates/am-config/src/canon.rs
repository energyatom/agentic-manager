//! Canonical config files (AD-16, story 1.8; owner decision 2026-11-21 on
//! key order). The writer edits a file's parsed tree and emits it here:
//!
//! - Top-level keys follow the file kind's schema order (the loader's
//!   allow-list, `schema_version` first); keys the schema does not know come
//!   after them, sorted.
//! - Keys in nested mappings are sorted alphabetically; sequences keep their
//!   order.
//! - YAML is emitted by serde-saphyr (two-space indent, list items indented
//!   under their key, YAML 1.2 scalars, long text folded, multi-line text
//!   as a block); a
//!   seat's or skill's Markdown body is kept byte for byte; a step output
//!   schema stays JSON (pretty, two-space indent).
//! - An untouched scalar keeps its original text when that differs from its
//!   typed value's rendering (`1.10`, `007`: the loader reads text fields
//!   from that text); a value an edit sets is written from the new value.
//!   YAML comments are not kept (the loader warns about them).

use std::cell::RefCell;

use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};
use serde_json::Value;

use crate::finding::Report;
use crate::kinds;
use crate::node::{self, Key, Kind, Node};

/// The kinds of config file the writer edits, by project-relative path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileKind {
    Company,
    Tools,
    Accounts,
    Prices,
    Capabilities,
    Seat,
    Skill,
    Workflow,
    Table,
    Dashboard,
    Source,
    Schema,
}

impl FileKind {
    fn order(self) -> &'static [&'static str] {
        match self {
            FileKind::Company => kinds::COMPANY_KEYS,
            FileKind::Tools => kinds::TOOLS_KEYS,
            FileKind::Accounts => kinds::ACCOUNTS_KEYS,
            FileKind::Prices => kinds::PRICES_KEYS,
            FileKind::Capabilities => kinds::CAPABILITIES_KEYS,
            FileKind::Seat => kinds::SEAT_KEYS,
            FileKind::Skill => kinds::SKILL_KEYS,
            FileKind::Workflow => kinds::WORKFLOW_KEYS,
            FileKind::Table => kinds::TABLE_KEYS,
            FileKind::Dashboard => kinds::DASHBOARD_KEYS,
            FileKind::Source => kinds::SOURCE_KEYS,
            FileKind::Schema => &["schema_version"],
        }
    }

    pub(crate) fn markdown(self) -> bool {
        matches!(self, FileKind::Seat | FileKind::Skill)
    }
}

/// One path component a writer may name: not empty, not `.` or `..`, not
/// hidden, no backslash or NUL.
fn plain(component: &str) -> bool {
    !component.is_empty() && !component.starts_with('.') && !component.contains(['\\', '\0'])
}

/// The kind of the config file at `rel` (forward slashes, relative), or
/// `None` when it is not one the writer edits.
pub(crate) fn kind_of(rel: &str) -> Option<FileKind> {
    let parts: Vec<&str> = rel.split('/').collect();
    if !parts.iter().all(|p| plain(p)) {
        return None;
    }
    let stem_of = |name: &str, ext: &str| name.strip_suffix(ext).is_some_and(plain);
    Some(match parts.as_slice() {
        ["company.yaml"] => FileKind::Company,
        ["tools.yaml"] => FileKind::Tools,
        ["models", "accounts.yaml"] => FileKind::Accounts,
        ["models", "prices.yaml"] => FileKind::Prices,
        ["models", "capabilities.yaml"] => FileKind::Capabilities,
        ["seats", n] if stem_of(n, ".md") => FileKind::Seat,
        ["skills", _, "SKILL.md"] => FileKind::Skill,
        ["workflows", n] if stem_of(n, ".yaml") => FileKind::Workflow,
        ["tables", n] if stem_of(n, ".yaml") => FileKind::Table,
        ["dashboards", n] if stem_of(n, ".yaml") => FileKind::Dashboard,
        ["sources", n] if stem_of(n, ".yaml") => FileKind::Source,
        ["schemas", n] if stem_of(n, ".json") => FileKind::Schema,
        _ => return None,
    })
}

/// A config file read for editing: its tree and, for Markdown, its body.
pub(crate) struct Doc {
    pub kind: FileKind,
    pub tree: Node,
    pub body: Option<String>,
}

/// Reads `text` as a file of `kind`; the error says why it cannot be edited.
pub(crate) fn parse(kind: FileKind, text: &str) -> Result<Doc, String> {
    let (front, body) = if kind.markdown() {
        let mut rep = Report::new("");
        match crate::frontmatter(&mut rep, text, "") {
            Some((front, _, body)) => (front, Some(body.to_string())),
            None => {
                let why = rep
                    .findings
                    .first()
                    .map(|f| f.message.clone())
                    .unwrap_or_else(|| "it has no frontmatter".into());
                return Err(why);
            }
        }
    } else {
        (text, None)
    };
    let tree = node::parse(front, 0)
        .map_err(|e| format!("it is not valid YAML at line {}: {}", e.line, e.message))?;
    let tree = match tree.kind {
        Kind::Null => Node {
            line: 0,
            kind: Kind::Map(Vec::new()),
            raw: None,
        },
        Kind::Map(_) => tree,
        _ => return Err("its top level is not key: value pairs".into()),
    };
    Ok(Doc { kind, tree, body })
}

/// The canonical text of `doc`.
pub(crate) fn emit(doc: &Doc) -> Result<String, String> {
    if doc.kind == FileKind::Schema {
        let canon = Canon {
            node: &doc.tree,
            order: Some(doc.kind.order()),
            raws: None,
        };
        let mut text =
            serde_json::to_string_pretty(&canon).map_err(|e| format!("cannot emit JSON: {e}"))?;
        text.push('\n');
        return Ok(text);
    }
    let raws = RefCell::new(Vec::new());
    let canon = Canon {
        node: &doc.tree,
        order: Some(doc.kind.order()),
        raws: Some(&raws),
    };
    let mut yaml = serde_saphyr::to_string_with_options(&canon, yaml_options())
        .map_err(|e| format!("cannot emit YAML: {e}"))?;
    // Each kept spelling stands in the output as a reserved plain token;
    // every token must appear exactly once (a file whose text holds the
    // reserved word is refused rather than rewritten).
    let raws = raws.into_inner();
    if yaml.matches(RAW_TOKEN).count() != raws.len() {
        return Err(format!(
            "it holds the text '{RAW_TOKEN}', which the writer reserves; remove it by hand"
        ));
    }
    for (i, raw) in raws.iter().enumerate() {
        let token = raw_token(i);
        if yaml.matches(&token).count() != 1 {
            return Err(format!(
                "it holds the text '{RAW_TOKEN}', which the writer reserves; remove it by hand"
            ));
        }
        yaml = yaml.replacen(&token, raw, 1);
    }
    if !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    Ok(match &doc.body {
        Some(body) if doc.kind.markdown() => format!("---\n{yaml}---\n{body}"),
        _ => yaml,
    })
}

fn yaml_options() -> serde_saphyr::SerializerOptions {
    let mut options = serde_saphyr::SerializerOptions::default();
    options.compact_list_indent = false;
    // The loader reads YAML 1.2 (`on`, `yes` and `no` are text), so they are
    // written plain; no `%YAML` directive or document marker.
    options.yaml_12 = true;
    options.no_lang_directive = true;
    options
}

/// The reserved word of the tokens that stand for kept scalar spellings.
const RAW_TOKEN: &str = "am-raw-scalar-";

fn raw_token(i: usize) -> String {
    format!("{RAW_TOKEN}{i:06}-end")
}

/// The scalar's original text when it differs from how its typed value is
/// written (`1.10` read as 1.1, `007` as 7); the loader reads text fields
/// from that text, so an untouched scalar keeps it. A value an edit sets has
/// no original text.
fn kept_spelling(node: &Node) -> Option<&str> {
    let raw = node.raw.as_deref()?;
    let typed = match &node.kind {
        Kind::Bool(b) => serde_saphyr::to_string_with_options(b, yaml_options()),
        Kind::Int(i) => serde_saphyr::to_string_with_options(i, yaml_options()),
        Kind::Float(f) => serde_saphyr::to_string_with_options(f, yaml_options()),
        _ => return None,
    };
    match typed {
        Ok(t) if t.trim() == raw => None,
        _ => Some(raw),
    }
}

/// A node serialised in canonical order; `order` is set at the top level
/// only. With `raws` (YAML), a kept spelling is written as a token and
/// recorded there, to be put back after emission.
struct Canon<'n> {
    node: &'n Node,
    order: Option<&'static [&'static str]>,
    raws: Option<&'n RefCell<Vec<String>>>,
}

impl Serialize for Canon<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if let (Some(raws), Some(raw)) = (self.raws, kept_spelling(self.node)) {
            let mut raws = raws.borrow_mut();
            let token = raw_token(raws.len());
            raws.push(raw.to_string());
            return s.serialize_str(&token);
        }
        match &self.node.kind {
            Kind::Null => s.serialize_unit(),
            Kind::Bool(b) => s.serialize_bool(*b),
            Kind::Int(i) => s.serialize_i64(*i),
            Kind::Float(f) => s.serialize_f64(*f),
            Kind::Str(t) => s.serialize_str(t),
            Kind::Seq(items) => {
                let mut seq = s.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(&Canon {
                        node: item,
                        order: None,
                        raws: self.raws,
                    })?;
                }
                seq.end()
            }
            Kind::Map(entries) => {
                let mut sorted: Vec<&(Key, Node)> = entries.iter().collect();
                let order = self.order.unwrap_or(&[]);
                let rank = |k: &str| order.iter().position(|o| *o == k).unwrap_or(usize::MAX);
                sorted.sort_by(|(a, _), (b, _)| {
                    (rank(&a.name), &a.name).cmp(&(rank(&b.name), &b.name))
                });
                let mut map = s.serialize_map(Some(sorted.len()))?;
                for (k, v) in sorted {
                    map.serialize_entry(
                        &k.name,
                        &Canon {
                            node: v,
                            order: None,
                            raws: self.raws,
                        },
                    )?;
                }
                map.end()
            }
        }
    }
}

// ------------------------------------------------------------------ edits

fn node_of(v: &Value) -> Result<Node, String> {
    let kind = match v {
        Value::Null => Kind::Null,
        Value::Bool(b) => Kind::Bool(*b),
        Value::Number(n) => match (n.as_i64(), n.as_f64()) {
            (Some(i), _) => Kind::Int(i),
            (None, Some(f)) if n.is_f64() => Kind::Float(f),
            _ => return Err(format!("the number {n} is too large")),
        },
        Value::String(t) => Kind::Str(t.clone()),
        Value::Array(items) => Kind::Seq(items.iter().map(node_of).collect::<Result<_, _>>()?),
        Value::Object(m) => Kind::Map(
            m.iter()
                .map(|(k, v)| {
                    Ok((
                        Key {
                            name: k.clone(),
                            line: 0,
                        },
                        node_of(v)?,
                    ))
                })
                .collect::<Result<_, String>>()?,
        ),
    };
    Ok(Node {
        line: 0,
        kind,
        raw: None,
    })
}

fn empty_map() -> Node {
    Node {
        line: 0,
        kind: Kind::Map(Vec::new()),
        raw: None,
    }
}

/// The child `seg` of `node`, created as an empty mapping when it is missing
/// and `create` is set (a null parent becomes a mapping).
fn child<'n>(
    node: &'n mut Node,
    seg: &str,
    create: bool,
    at: &str,
) -> Result<&'n mut Node, String> {
    if create && node.kind == Kind::Null {
        node.kind = Kind::Map(Vec::new());
    }
    let type_name = node.type_name();
    match &mut node.kind {
        Kind::Map(entries) => {
            let pos = match entries.iter().position(|(k, _)| k.name == seg) {
                Some(p) => p,
                None if create => {
                    entries.push((
                        Key {
                            name: seg.to_string(),
                            line: 0,
                        },
                        empty_map(),
                    ));
                    entries.len() - 1
                }
                None => return Err(format!("there is no `{at}`")),
            };
            Ok(&mut entries[pos].1)
        }
        Kind::Seq(items) => {
            let len = items.len();
            seg.parse::<usize>()
                .ok()
                .and_then(|i| items.get_mut(i))
                .ok_or_else(|| format!("`{at}` is not an item of a list of {len}"))
        }
        _ => Err(format!(
            "`{at}` is inside {type_name}, not key: value pairs or a list"
        )),
    }
}

/// Applies one Set (with `value`) or Remove (`value` None) at `path`.
pub(crate) fn apply(root: &mut Node, path: &[String], value: Option<&Value>) -> Result<(), String> {
    let Some((last, parents)) = path.split_last() else {
        return Err("an edit names at least one key".into());
    };
    let mut node = root;
    let mut at = String::new();
    for seg in parents {
        if !at.is_empty() {
            at.push('.');
        }
        at.push_str(seg);
        node = child(node, seg, value.is_some(), &at)?;
    }
    let full = path.join(".");
    let new = value.map(node_of).transpose()?;
    if new.is_some() && node.kind == Kind::Null {
        node.kind = Kind::Map(Vec::new());
    }
    let type_name = node.type_name();
    match (&mut node.kind, new) {
        (Kind::Map(entries), Some(new)) => {
            match entries.iter_mut().find(|(k, _)| k.name == *last) {
                Some((_, v)) => *v = new,
                None => entries.push((
                    Key {
                        name: last.clone(),
                        line: 0,
                    },
                    new,
                )),
            }
            Ok(())
        }
        (Kind::Map(entries), None) => {
            let before = entries.len();
            entries.retain(|(k, _)| k.name != *last);
            if entries.len() == before {
                return Err(format!("there is no `{full}` to remove"));
            }
            Ok(())
        }
        (Kind::Seq(items), new) => {
            let len = items.len();
            let i = last
                .parse::<usize>()
                .map_err(|_| format!("`{full}`: a list item is named by its number"))?;
            match new {
                Some(new) if i < len => items[i] = new,
                Some(new) if i == len => items.push(new),
                None if i < len => {
                    items.remove(i);
                }
                _ => return Err(format!("`{full}` is not an item of a list of {len}")),
            }
            Ok(())
        }
        _ => Err(format!(
            "`{full}` is inside {type_name}, not key: value pairs or a list"
        )),
    }
}
