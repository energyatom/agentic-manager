//! Typed reading of a mapping node: unknown keys (with "did you mean"),
//! required and optional fields, each problem reported at its line.

use crate::checks::{did_you_mean, is_kebab, kebab_of};
use crate::finding::Report;
use crate::node::{Key, Kind, Node};

/// A text value and the line it came from.
#[derive(Debug, Clone)]
pub(crate) struct Text {
    pub value: String,
    pub line: u32,
}

/// One mapping being read.
pub(crate) struct Fields<'n> {
    pub entries: &'n [(Key, Node)],
    pub line: u32,
    /// What this mapping is, for messages: "account", "step", …
    pub what: &'static str,
}

/// The node as a mapping, or an error at its line.
pub(crate) fn fields<'n>(
    rep: &mut Report,
    node: &'n Node,
    what: &'static str,
) -> Option<Fields<'n>> {
    match node.as_map() {
        Some(entries) => Some(Fields {
            entries,
            line: node.line,
            what,
        }),
        None => {
            rep.error(
                node.line,
                format!("{what} must be key: value pairs, not {}", node.type_name()),
                format!("write the {what} as `key: value` lines"),
            );
            None
        }
    }
}

impl<'n> Fields<'n> {
    /// Reports every key not in `allowed`, suggesting the nearest one.
    pub fn allow(&self, rep: &mut Report, allowed: &[&str]) {
        self.allow_except(rep, allowed, &[]);
    }

    /// [`Fields::allow`], leaving out the keys in `reported` (the caller
    /// reports those with its own words).
    pub fn allow_except(&self, rep: &mut Report, allowed: &[&str], reported: &[&str]) {
        for (key, _) in self.entries {
            if allowed.contains(&key.name.as_str()) || reported.contains(&key.name.as_str()) {
                continue;
            }
            let fix = match did_you_mean(&key.name, allowed) {
                Some(near) => format!(
                    "did you mean '{near}'? Keys allowed here: {}",
                    allowed.join(", ")
                ),
                None => format!("remove it; keys allowed here: {}", allowed.join(", ")),
            };
            rep.error(
                key.line,
                format!("unknown key '{}' in {}", key.name, self.what),
                fix,
            );
        }
    }

    /// True when the key is missing or holds nothing (`key:`); a required
    /// field is reported the same way in both cases.
    pub fn absent(&self, name: &str) -> bool {
        self.get(name).is_none_or(|n| n.kind == Kind::Null)
    }

    pub fn get(&self, name: &str) -> Option<&'n Node> {
        self.entries
            .iter()
            .find(|(k, _)| k.name == name)
            .map(|(_, v)| v)
    }

    fn wrong(&self, rep: &mut Report, name: &str, node: &Node, want: &str) {
        rep.error(
            node.line,
            format!(
                "'{name}' in {} must be {want}, not {}",
                self.what,
                node.type_name()
            ),
            format!("set `{name}:` to {want}"),
        );
    }

    fn missing(&self, rep: &mut Report, name: &str, example: &str) {
        rep.error(
            self.line,
            format!("{} is missing '{name}'", self.what),
            format!("add `{name}: {example}`"),
        );
    }

    /// Optional text; numbers and booleans are read as their text too (a
    /// version such as `1.0` or a name such as `2026`).
    pub fn text(&self, rep: &mut Report, name: &str) -> Option<Text> {
        let node = self.get(name)?;
        let value = match &node.kind {
            Kind::Str(s) => s.clone(),
            Kind::Int(i) => node.raw.clone().unwrap_or_else(|| i.to_string()),
            Kind::Float(f) => node.raw.clone().unwrap_or_else(|| f.to_string()),
            Kind::Bool(b) => node.raw.clone().unwrap_or_else(|| b.to_string()),
            Kind::Null => return None,
            _ => {
                self.wrong(rep, name, node, "text");
                return None;
            }
        };
        Some(Text {
            value,
            line: node.line,
        })
    }

    /// Required, non-empty text.
    pub fn req_text(&self, rep: &mut Report, name: &str, example: &str) -> Option<Text> {
        match self.text(rep, name) {
            Some(t) if !t.value.trim().is_empty() => Some(t),
            Some(t) => {
                rep.error(
                    t.line,
                    format!("'{name}' in {} is empty", self.what),
                    format!("set `{name}: {example}`"),
                );
                None
            }
            None => {
                if self.absent(name) {
                    self.missing(rep, name, example);
                }
                None
            }
        }
    }

    /// Optional text that must be one of `choices`.
    pub fn choice(&self, rep: &mut Report, name: &str, choices: &[&str]) -> Option<Text> {
        let t = self.text(rep, name)?;
        if choices.contains(&t.value.as_str()) {
            return Some(t);
        }
        let fix = match did_you_mean(&t.value, choices) {
            Some(near) => format!("did you mean '{near}'? Use one of: {}", choices.join(", ")),
            None => format!("use one of: {}", choices.join(", ")),
        };
        rep.error(t.line, format!("'{}' is not a valid {name}", t.value), fix);
        None
    }

    /// Optional kebab-case id text (a reference or a declared id).
    pub fn id(&self, rep: &mut Report, name: &str) -> Option<Text> {
        let t = self.text(rep, name)?;
        if is_kebab(&t.value) {
            Some(t)
        } else {
            rep.error(
                t.line,
                format!("'{}' in '{name}' is not a kebab-case id", t.value),
                format!(
                    "use lowercase words joined by hyphens, e.g. '{}'",
                    kebab_of(&t.value)
                ),
            );
            None
        }
    }

    /// Required kebab-case id.
    pub fn req_id(&self, rep: &mut Report, name: &str) -> Option<Text> {
        if self.absent(name) {
            self.missing(rep, name, "an-id");
            return None;
        }
        self.id(rep, name)
    }

    pub fn bool(&self, rep: &mut Report, name: &str) -> Option<bool> {
        let node = self.get(name)?;
        match node.kind {
            Kind::Bool(b) => Some(b),
            Kind::Null => None,
            _ => {
                self.wrong(rep, name, node, "true or false");
                None
            }
        }
    }

    /// Optional whole number within `min..=max`.
    pub fn int(&self, rep: &mut Report, name: &str, min: i64, max: i64) -> Option<i64> {
        let node = self.get(name)?;
        match node.kind {
            Kind::Int(i) if (min..=max).contains(&i) => Some(i),
            Kind::Int(i) => {
                rep.error(
                    node.line,
                    format!("'{name}' is {i}; it must be between {min} and {max}"),
                    format!("set `{name}:` to a whole number from {min} to {max}"),
                );
                None
            }
            Kind::Null => None,
            _ => {
                self.wrong(rep, name, node, "a whole number");
                None
            }
        }
    }

    /// Optional non-negative amount of money or price, as i64 micro-units.
    pub fn micros(&self, rep: &mut Report, name: &str) -> Option<i64> {
        let node = self.get(name)?;
        let value = match node.kind {
            Kind::Int(i) => i as f64,
            Kind::Float(f) => f,
            Kind::Null => return None,
            _ => {
                self.wrong(rep, name, node, "a number");
                return None;
            }
        };
        if !value.is_finite() || value < 0.0 || value > 1e12 {
            rep.error(
                node.line,
                format!("'{name}' must be a number from 0 up"),
                format!("set `{name}:` to an amount such as 1.50"),
            );
            return None;
        }
        Some((value * 1_000_000.0).round() as i64)
    }

    /// Optional list; each item is handed back for the caller to read.
    pub fn list(&self, rep: &mut Report, name: &str) -> Vec<&'n Node> {
        let Some(node) = self.get(name) else {
            return Vec::new();
        };
        match &node.kind {
            Kind::Seq(items) => items.iter().collect(),
            Kind::Null => Vec::new(),
            _ => {
                self.wrong(rep, name, node, "a list");
                Vec::new()
            }
        }
    }

    /// Optional list of text items.
    pub fn texts(&self, rep: &mut Report, name: &str) -> Vec<Text> {
        let mut out = Vec::new();
        for item in self.list(rep, name) {
            match &item.kind {
                Kind::Str(s) => out.push(Text {
                    value: s.clone(),
                    line: item.line,
                }),
                Kind::Int(i) => out.push(Text {
                    value: item.raw.clone().unwrap_or_else(|| i.to_string()),
                    line: item.line,
                }),
                _ => rep.error(
                    item.line,
                    format!("items of '{name}' must be text, not {}", item.type_name()),
                    format!("write each item of `{name}:` as `- text`"),
                ),
            }
        }
        out
    }

    /// Optional list of kebab-case ids.
    pub fn ids(&self, rep: &mut Report, name: &str) -> Vec<Text> {
        self.texts(rep, name)
            .into_iter()
            .filter(|t| {
                let ok = is_kebab(&t.value);
                if !ok {
                    rep.error(
                        t.line,
                        format!("'{}' in '{name}' is not a kebab-case id", t.value),
                        format!(
                            "use lowercase words joined by hyphens, e.g. '{}'",
                            kebab_of(&t.value)
                        ),
                    );
                }
                ok
            })
            .collect()
    }
}

/// Reports a second use of the same id inside one list.
pub(crate) fn unique(rep: &mut Report, what: &str, ids: &[Text]) {
    for (i, t) in ids.iter().enumerate() {
        if ids[..i].iter().any(|p| p.value == t.value) {
            rep.error(
                t.line,
                format!("{what} '{}' appears twice", t.value),
                format!("remove the second '{}' or give it its own id", t.value),
            );
        }
    }
}
