//! References between files, collected while each file is read and checked
//! once every file is loaded (Conventions: referential integrity).

use std::collections::{BTreeMap, BTreeSet};

use crate::finding::Report;

pub(crate) const RESERVED_ACCOUNTS: &[&str] = &["local-lmstudio", "local-ollama"];

#[derive(Debug, Clone)]
pub(crate) enum RefKind {
    Team,
    Seat,
    Account,
    Schema,
    Workflow,
    /// A step of the named workflow.
    Step(String),
    /// A seat's tool: a built-in id or `<server-id>/<tool-name>`.
    Tool,
    /// An `accepts` entry: `<team-id>/<workflow-id>` of that team.
    Accepts,
}

#[derive(Debug, Clone)]
pub(crate) struct PendingRef {
    pub file: String,
    pub line: u32,
    pub kind: RefKind,
    pub id: String,
}

/// Which tree a parent edge belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tree {
    /// A seat's `reports_to`.
    ReportsTo,
    /// A team's `parent`.
    TeamParent,
}

/// One `child → parent` edge and where it is written.
#[derive(Debug, Clone)]
pub(crate) struct Edge {
    pub tree: Tree,
    pub from: String,
    pub to: String,
    pub file: String,
    pub line: u32,
}

#[derive(Default)]
pub(crate) struct Refs {
    pub pending: Vec<PendingRef>,
    pub edges: Vec<Edge>,
}

impl Refs {
    pub fn push(&mut self, rep: &Report, line: u32, kind: RefKind, id: &str) {
        self.pending.push(PendingRef {
            file: rep.file.clone(),
            line,
            kind,
            id: id.to_string(),
        });
    }

    /// Records a parent edge (also checked as a reference).
    pub fn edge(&mut self, rep: &Report, tree: Tree, from: &str, to: &str, line: u32) {
        self.edges.push(Edge {
            tree,
            from: from.to_string(),
            to: to.to_string(),
            file: rep.file.clone(),
            line,
        });
    }
}

/// Every id a reference may name.
#[derive(Default)]
pub(crate) struct Known {
    pub teams: BTreeSet<String>,
    pub seats: BTreeSet<String>,
    pub accounts: BTreeSet<String>,
    pub schemas: BTreeSet<String>,
    /// Workflow id → (team, step ids), each known only when its file
    /// declared it readably.
    pub workflows: BTreeMap<String, (Option<String>, Option<BTreeSet<String>>)>,
    pub builtin_tools: Vec<String>,
    pub mcp_tools: BTreeSet<String>,
}

fn list<'a>(ids: impl Iterator<Item = &'a String>) -> String {
    let all: Vec<&str> = ids.map(String::as_str).collect();
    if all.is_empty() {
        "(none yet)".to_string()
    } else {
        all.join(", ")
    }
}

/// Checks every pending reference; findings go to the report of the file
/// that holds the reference.
pub(crate) fn resolve(refs: &Refs, known: &Known, reports: &mut BTreeMap<String, Report>) {
    for r in &refs.pending {
        let rep = reports
            .entry(r.file.clone())
            .or_insert_with(|| Report::new(&r.file));
        check(r, known, rep);
    }
    for tree in [Tree::ReportsTo, Tree::TeamParent] {
        let edges: Vec<&Edge> = refs.edges.iter().filter(|e| e.tree == tree).collect();
        for (cycle, closing) in cycles(&edges) {
            let rep = reports
                .entry(closing.file.clone())
                .or_insert_with(|| Report::new(&closing.file));
            let path = cycle.join(" → ");
            let (message, fix) = match tree {
                Tree::ReportsTo => (
                    format!("reports_to makes a loop: {path}"),
                    format!(
                        "point reports_to of '{}' at a seat outside the loop, or remove it",
                        closing.from
                    ),
                ),
                Tree::TeamParent => (
                    format!("parent makes a loop of teams: {path}"),
                    format!(
                        "give team '{}' a parent outside the loop, or remove its parent",
                        closing.from
                    ),
                ),
            };
            rep.error(closing.line, message, fix);
        }
    }
}

/// Every cycle in a forest of `child → parent` edges (each child has at
/// most one parent), as its path (first node repeated at the end) and the
/// edge that closes it. Walks start from ids in sorted order, so the result
/// is stable.
fn cycles<'e>(edges: &[&'e Edge]) -> Vec<(Vec<String>, &'e Edge)> {
    let parent: BTreeMap<&str, &Edge> = edges.iter().map(|e| (e.from.as_str(), *e)).collect();
    let mut state: BTreeMap<&str, u8> = BTreeMap::new(); // 1 on the walk, 2 done
    let mut found = Vec::new();
    for start in parent.keys() {
        let mut path: Vec<&str> = Vec::new();
        let mut node = *start;
        // Follow parents until a root (no parent) or an id already seen.
        let revisited = loop {
            if state.get(node).copied().unwrap_or(0) != 0 {
                break true;
            }
            state.insert(node, 1);
            path.push(node);
            match parent.get(node) {
                Some(e) => node = e.to.as_str(),
                None => break false,
            }
        };
        if revisited
            && state.get(node) == Some(&1)
            && let Some(at) = path.iter().position(|n| *n == node)
        {
            let last = path[path.len() - 1];
            let mut cycle: Vec<String> = path[at..].iter().map(|n| n.to_string()).collect();
            cycle.push(node.to_string());
            found.push((cycle, parent[last]));
        }
        for n in path {
            state.insert(n, 2);
        }
    }
    found
}

fn check(r: &PendingRef, known: &Known, rep: &mut Report) {
    let id = r.id.as_str();
    match &r.kind {
        RefKind::Team => {
            if !known.teams.contains(id) {
                rep.error(
                    r.line,
                    format!("unknown team '{id}'"),
                    format!(
                        "use a team declared under teams: in company.yaml: {}",
                        list(known.teams.iter())
                    ),
                );
            }
        }
        RefKind::Seat => {
            if !known.seats.contains(id) {
                rep.error(
                    r.line,
                    format!("unknown seat '{id}'"),
                    format!(
                        "name one of the known seats: {} (a seat is a file seats/<id>.md)",
                        list(known.seats.iter())
                    ),
                );
            }
        }
        RefKind::Account => {
            if !known.accounts.contains(id) && !RESERVED_ACCOUNTS.contains(&id) {
                let mut all: BTreeSet<String> = known.accounts.clone();
                all.extend(RESERVED_ACCOUNTS.iter().map(|s| s.to_string()));
                rep.error(
                    r.line,
                    format!("unknown account '{id}'"),
                    format!(
                        "use an Account from models/accounts.yaml or a local one: {}",
                        list(all.iter())
                    ),
                );
            }
        }
        RefKind::Schema => {
            if !known.schemas.contains(id) {
                rep.error(
                    r.line,
                    format!("unknown schema '{id}'"),
                    format!(
                        "add schemas/{id}.json or use one of: {}",
                        list(known.schemas.iter())
                    ),
                );
            }
        }
        RefKind::Workflow => {
            if !known.workflows.contains_key(id) {
                rep.error(
                    r.line,
                    format!("unknown workflow '{id}'"),
                    format!("use one of: {}", list(known.workflows.keys())),
                );
            }
        }
        RefKind::Step(workflow) => match known.workflows.get(workflow) {
            None => rep.error(
                r.line,
                format!("unknown workflow '{workflow}'"),
                format!("use one of: {}", list(known.workflows.keys())),
            ),
            Some((_, Some(steps))) if !steps.contains(id) => rep.error(
                r.line,
                format!("unknown step '{id}' in workflow '{workflow}'"),
                format!("use one of its steps: {}", list(steps.iter())),
            ),
            Some(_) => {}
        },
        RefKind::Tool => {
            if !known.builtin_tools.iter().any(|t| t == id) && !known.mcp_tools.contains(id) {
                rep.error(
                    r.line,
                    format!("unknown tool '{id}'"),
                    format!(
                        "use a built-in tool: {}; or an MCP tool declared in tools.yaml as <server-id>/<tool-name>: {}",
                        known.builtin_tools.join(", "),
                        list(known.mcp_tools.iter())
                    ),
                );
            }
        }
        RefKind::Accepts => {
            let Some((team, workflow)) = id.split_once('/') else {
                return; // the shape was reported when the file was read
            };
            if !known.teams.contains(team) {
                rep.error(
                    r.line,
                    format!("unknown team '{team}' in accepts entry '{id}'"),
                    format!("use a declared team: {}", list(known.teams.iter())),
                );
                return;
            }
            match known.workflows.get(workflow) {
                None => rep.error(
                    r.line,
                    format!("unknown workflow '{workflow}' in accepts entry '{id}'"),
                    format!("use one of: {}", list(known.workflows.keys())),
                ),
                Some((Some(owner), _)) if owner != team => rep.error(
                    r.line,
                    format!("workflow '{workflow}' belongs to team '{owner}', not '{team}'"),
                    format!("write the entry as '{owner}/{workflow}'"),
                ),
                Some(_) => {}
            }
        }
    }
}
