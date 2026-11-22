//! References between files, collected while each file is read and checked
//! once every file is loaded (Conventions: referential integrity).

use std::collections::{BTreeMap, BTreeSet};

use am_domain::capability::AccountKind;

use crate::finding::Report;

/// The reserved local Accounts: am-domain's one definition (AD-31).
pub(crate) const RESERVED_ACCOUNTS: &[&str] = &am_domain::net::RESERVED_LOCAL_ACCOUNTS;

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
    /// An Account that must be of the `want` kind (a capability provider, a
    /// seat model, a Source account); `user` names what takes it, for the
    /// message, and `alternative` is the other choice offered in the fix.
    AccountOfKind {
        want: AccountKind,
        user: String,
        alternative: Option<String>,
    },
    /// A skill folder `skills/<id>/`.
    Skill,
    /// A table `tables/<id>.yaml`.
    Table,
    /// A column of the named table.
    TableColumn(String),
    /// The owner seat of a crawler Source, which must have `crawl` on.
    CrawlOwner,
}

/// What a host check asks of a seat's allow-list (AD-48).
#[derive(Debug, Clone)]
pub(crate) enum HostTarget {
    /// A URL's destination: some pattern must admit it.
    Url(crate::kinds::UrlTarget),
    /// A host pattern: some pattern must cover it.
    Pattern(am_domain::HostPattern),
}

/// A host that should be on a seat's allow-list; a warning when it is not.
#[derive(Debug, Clone)]
pub(crate) struct HostCheck {
    pub file: String,
    pub line: u32,
    pub host: HostTarget,
    /// The seat whose allow-list applies.
    pub seat: String,
    /// What names the host, for the message: "scope".
    pub what: &'static str,
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
    pub hosts: Vec<HostCheck>,
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

    /// Records a host to check against `seat`'s allow-list.
    pub fn host(
        &mut self,
        rep: &Report,
        line: u32,
        host: HostTarget,
        seat: &str,
        what: &'static str,
    ) {
        self.hosts.push(HostCheck {
            file: rep.file.clone(),
            line,
            host,
            seat: seat.to_string(),
            what,
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
    /// Account id → provider as written.
    pub account_providers: BTreeMap<String, String>,
    /// `accounts` and `account_providers` hold this Mac's user list (story
    /// 2.12); otherwise an Account reference is checked for form only.
    pub user_list: bool,
    pub skills: BTreeSet<String>,
    /// Table id → its column ids, when its file declared them readably.
    pub tables: BTreeMap<String, Option<BTreeSet<String>>>,
    /// Seats whose file switches `crawl` on.
    pub crawl_seats: BTreeSet<String>,
    /// Each declared seat's `allowed_hosts` (the entries that parse).
    pub seat_hosts: BTreeMap<String, Vec<am_domain::HostPattern>>,
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
    for h in &refs.hosts {
        // An unknown seat is reported as a reference; its list is not read.
        let Some(allowed) = known.seat_hosts.get(&h.seat) else {
            continue;
        };
        let (ok, message, host) = match &h.host {
            HostTarget::Url(target) => (
                crate::kinds::admitted(allowed, target),
                format!(
                    "{} host '{}' is not on the allow-list of seat '{}'",
                    h.what, target.host, h.seat
                ),
                target.host.clone(),
            ),
            HostTarget::Pattern(p) => (
                allowed.iter().any(|a| a.covers(p)),
                format!(
                    "{} host pattern '{p}' is not covered by the allow-list of seat '{}'",
                    h.what, h.seat
                ),
                p.to_string(),
            ),
        };
        if !ok {
            let rep = reports
                .entry(h.file.clone())
                .or_insert_with(|| Report::new(&h.file));
            rep.warning(
                h.line,
                message,
                crate::kinds::allow_host_fix(&h.seat, &host, &format!("change the {}", h.what)),
            );
        }
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

/// An Account id this Mac's user list lacks (story 2.12, AD-17): a
/// Warning, since the project is valid on a Mac that has it; a run or call
/// that needs it is refused in words.
fn missing_account(rep: &mut Report, line: u32, id: &str) {
    rep.warning(
        line,
        format!("the Account '{id}' is not on this Mac"),
        format!("add that Account on this Mac: `agentic accounts add {id}`"),
    );
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
            if known.user_list && !known.accounts.contains(id) && !RESERVED_ACCOUNTS.contains(&id) {
                missing_account(rep, r.line, id);
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
        RefKind::AccountOfKind {
            want,
            user,
            alternative,
        } => {
            let declared = known.accounts.contains(id);
            let reserved = RESERVED_ACCOUNTS.contains(&id);
            if !declared && !reserved {
                if known.user_list {
                    missing_account(rep, r.line, id);
                }
                return;
            }
            let kind = if reserved {
                Some(AccountKind::Model)
            } else {
                known
                    .account_providers
                    .get(id)
                    .and_then(|p| AccountKind::of_provider(p))
            };
            if let Some(kind) = kind
                && kind != *want
            {
                let providers = want.providers().join(", ");
                let choice = match alternative {
                    Some(alt) => format!("{alt}, or name"),
                    None => "name".to_string(),
                };
                rep.error(
                    r.line,
                    format!(
                        "account '{id}' is a {} Account; {user} takes a {} Account",
                        kind.as_str(),
                        want.as_str()
                    ),
                    format!(
                        "{choice} a {} Account (provider {providers}) from this Mac's Accounts (`agentic accounts list`)",
                        want.as_str()
                    ),
                );
            }
        }
        RefKind::Skill => {
            if !known.skills.contains(id) {
                rep.error(
                    r.line,
                    format!("unknown skill '{id}'"),
                    format!(
                        "add the folder skills/{id}/ with a SKILL.md, or use one of: {}",
                        list(known.skills.iter())
                    ),
                );
            }
        }
        RefKind::Table => {
            if !known.tables.contains_key(id) {
                rep.error(
                    r.line,
                    format!("unknown table '{id}'"),
                    format!(
                        "add tables/{id}.yaml or use one of: {}",
                        list(known.tables.keys())
                    ),
                );
            }
        }
        RefKind::TableColumn(table) => {
            // An unknown table is reported once, at the table reference.
            if let Some(Some(columns)) = known.tables.get(table)
                && !columns.contains(id)
            {
                rep.error(
                    r.line,
                    format!("unknown column '{id}' in table '{table}'"),
                    format!("use one of its columns: {}", list(columns.iter())),
                );
            }
        }
        RefKind::CrawlOwner => {
            if known.seats.contains(id) && !known.crawl_seats.contains(id) {
                rep.error(
                    r.line,
                    format!(
                        "seat '{id}' has the crawl capability off; a crawler Source needs its owner seat to have it on"
                    ),
                    format!(
                        "switch on crawl in seats/{id}.md (`capabilities: {{ crawl: {{ on: true }} }}`), or choose another owner_seat"
                    ),
                );
            }
        }
    }
}
