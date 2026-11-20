//! One reader per file kind of the project layout (Structural Seed). Each
//! reads a parsed node tree into its entity, reports problems at their line
//! and records the references it makes for the cross-file check.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use am_domain::capability::{
    self, AccountKind, BUILTIN_PROVIDER, Capability, FOLDER_WRITE_TOOL, LEGACY_FOLDER_ACCESS_KEY,
    SKILL_TOOL, capability_of_tool,
};
use am_domain::tool_manifest::{ActionClass, builtin_tool, builtin_tool_ids};
use am_domain::workspace::Access;

use crate::checks;
use crate::cron;
use crate::entities::*;
use crate::fields::{Fields, Text, fields, unique};
use crate::finding::Report;
use crate::node::{self, Kind, Node};
use crate::refs::{RESERVED_ACCOUNTS, RefKind, Refs, Tree};

const WEEKDAYS: &[&str] = &[
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
];
const MODEL_PROVIDERS: &[&str] = &[
    "anthropic",
    "openai",
    "openai-compatible",
    "lmstudio",
    "ollama",
];
const COLUMN_TYPES: &[&str] = &[
    "text", "number", "integer", "boolean", "date", "datetime", "url", "status", "json",
];

/// The top-level keys of `company.yaml`, in schema order: the loader's allow-list
/// and the writer's canonical order (story 1.8).
pub(crate) const COMPANY_KEYS: &[&str] = &[
    "schema_version",
    "name",
    "description",
    "timezone",
    "currency",
    "week_start",
    "max_chain_depth",
    "teams",
    "hats",
];

/// The top-level keys of `models/accounts.yaml`, in schema order: the loader's allow-list
/// and the writer's canonical order (story 1.8).
pub(crate) const ACCOUNTS_KEYS: &[&str] = &["schema_version", "accounts"];

/// The top-level keys of `models/prices.yaml`, in schema order: the loader's allow-list
/// and the writer's canonical order (story 1.8).
pub(crate) const PRICES_KEYS: &[&str] = &["schema_version", "updated", "models", "services"];

/// The top-level keys of `models/capabilities.yaml`, in schema order: the loader's allow-list
/// and the writer's canonical order (story 1.8).
pub(crate) const CAPABILITIES_KEYS: &[&str] = &["schema_version", "models"];

/// The top-level keys of `tools.yaml`, in schema order: the loader's allow-list
/// and the writer's canonical order (story 1.8).
pub(crate) const TOOLS_KEYS: &[&str] = &[
    "schema_version",
    "egress_hosts",
    "search",
    "builtin",
    "mcp_servers",
];

/// The top-level keys of a skill's `SKILL.md` frontmatter, in schema order: the loader's allow-list
/// and the writer's canonical order (story 1.8).
pub(crate) const SKILL_KEYS: &[&str] = &[
    "name",
    "description",
    "license",
    "metadata",
    "allowed-tools",
];

/// The top-level keys of a seat file's frontmatter, in schema order: the loader's allow-list
/// and the writer's canonical order (story 1.8).
pub(crate) const SEAT_KEYS: &[&str] = &[
    "schema_version",
    "id",
    "name",
    "role",
    "team",
    "description",
    "reports_to",
    "goal",
    "backstory",
    "avatar",
    "model",
    "fallback",
    "capabilities",
    "skills",
    "tools",
    "budget",
    "knowledge",
    "links",
    "folders",
    "text_fallback",
];

/// The top-level keys of `workflows/<id>.yaml`, in schema order: the loader's allow-list
/// and the writer's canonical order (story 1.8).
pub(crate) const WORKFLOW_KEYS: &[&str] = &[
    "schema_version",
    "id",
    "name",
    "team",
    "description",
    "triggers",
    "caps",
    "steps",
];

/// The top-level keys of `tables/<id>.yaml`, in schema order: the loader's allow-list
/// and the writer's canonical order (story 1.8).
pub(crate) const TABLE_KEYS: &[&str] = &[
    "schema_version",
    "id",
    "name",
    "team",
    "description",
    "key",
    "columns",
    "views",
    "grants",
];

/// The top-level keys of `dashboards/<id>.yaml`, in schema order: the loader's allow-list
/// and the writer's canonical order (story 1.8).
pub(crate) const DASHBOARD_KEYS: &[&str] = &[
    "schema_version",
    "id",
    "name",
    "team",
    "description",
    "cards",
];

/// The top-level keys of `sources/<id>.yaml`, in schema order: the loader's allow-list
/// and the writer's canonical order (story 1.8).
pub(crate) const SOURCE_KEYS: &[&str] = &[
    "schema_version",
    "id",
    "name",
    "description",
    "type",
    "owner_seat",
    "account",
    "scope",
    "trigger",
    "limits",
    "output",
    "render",
    "terms_checked",
];

fn value(t: Option<Text>) -> Option<String> {
    t.map(|t| t.value)
}

/// Parses a YAML file (or a seat's frontmatter, shifted by `offset`) and runs
/// the checks every file gets: comments, a mapping at the top,
/// schema_version and secret-looking values.
pub(crate) fn yaml(rep: &mut Report, text: &str, offset: u32) -> Option<Node> {
    let root = match node::parse(text, offset) {
        Ok(root) => root,
        Err(e) => {
            rep.error(
                e.line,
                format!("not valid YAML: {}", e.message),
                "fix the YAML syntax at this line (indentation, a missing ':' or an unclosed quote or bracket)",
            );
            return None;
        }
    };
    checks::comments(rep, &node::comment_lines(text, offset));
    top(rep, root)
}

/// Parses YAML that carries no `schema_version` (a SKILL.md frontmatter in
/// the open Agent Skills format) and runs the comment and secret checks.
pub(crate) fn yaml_plain(rep: &mut Report, text: &str, offset: u32) -> Option<Node> {
    let root = match node::parse(text, offset) {
        Ok(root) => root,
        Err(e) => {
            rep.error(
                e.line,
                format!("not valid YAML: {}", e.message),
                "fix the YAML syntax at this line (indentation, a missing ':' or an unclosed quote or bracket)",
            );
            return None;
        }
    };
    checks::comments(rep, &node::comment_lines(text, offset));
    if root.as_map().is_none() {
        rep.error(
            root.line.max(1 + offset),
            "the frontmatter must hold key: value pairs",
            "write `name: <id>` and `description: …` between the two `---` lines",
        );
        return None;
    }
    checks::secrets(rep, &root);
    Some(root)
}

/// The checks on a parsed document's top level.
fn top(rep: &mut Report, root: Node) -> Option<Node> {
    if root.kind == Kind::Null {
        rep.error(
            1,
            "missing schema_version (the file is empty)",
            "add `schema_version: 1` as the first line, then the file's fields",
        );
        return None;
    }
    let Some(entries) = root.as_map() else {
        rep.error(
            1 + root.line.saturating_sub(1),
            "the file must hold key: value pairs at the top level",
            "start the file with `schema_version: 1`, then one `key: value` per line",
        );
        return None;
    };
    checks::schema_version(rep, entries);
    checks::secrets(rep, &root);
    Some(root)
}

/// A per-entity file's optional `id:` must equal its file stem.
fn id_is_stem(rep: &mut Report, f: &Fields, stem: &str, ext: &str) {
    if let Some(id) = f.text(rep, "id")
        && id.value != stem
    {
        rep.error(
            id.line,
            format!("id '{}' does not match the file name '{stem}'", id.value),
            format!(
                "remove `id:` (the file name is the id) or rename the file to {}.{ext}",
                id.value
            ),
        );
    }
}

fn timezone(rep: &mut Report, t: &Text) -> bool {
    if jiff::tz::TimeZone::get(&t.value).is_ok() {
        return true;
    }
    rep.error(
        t.line,
        format!("unknown time zone '{}'", t.value),
        "use an IANA time zone name such as UTC or Area/City",
    );
    false
}

/// An https URL, or plain http only to this Mac (host exactly `127.0.0.1`
/// or `localhost`); never with a user name or password in it.
fn web_url(rep: &mut Report, t: &Text, name: &str) -> bool {
    let ok = url::Url::parse(&t.value).is_ok_and(|u| {
        let no_userinfo = u.username().is_empty() && u.password().is_none();
        let host = u.host_str().unwrap_or("");
        no_userinfo
            && match u.scheme() {
                "https" => !host.is_empty(),
                "http" => host == "127.0.0.1" || host == "localhost",
                _ => false,
            }
    });
    if !ok {
        rep.error(
            t.line,
            format!(
                "'{name}' must be an https URL, or an http URL on 127.0.0.1 or localhost, with no user name or password"
            ),
            format!(
                "set `{name}:` to https://… (or http://127.0.0.1:<port> or http://localhost:<port> for a server on this Mac)"
            ),
        );
    }
    ok
}

/// `company.yaml`.
pub(crate) fn company(rep: &mut Report, refs: &mut Refs, root: &Node) -> Option<Company> {
    let f = fields(rep, root, "company.yaml")?;
    f.allow(rep, COMPANY_KEYS);
    let name = f.req_text(rep, "name", "My company");
    let description = value(f.text(rep, "description"));
    let tz = f
        .req_text(rep, "timezone", "UTC")
        .filter(|t| timezone(rep, t));
    let currency = f.req_text(rep, "currency", "<ISO 4217 code>").filter(|t| {
        let ok = t.value.len() == 3 && t.value.chars().all(|c| c.is_ascii_uppercase());
        if !ok {
            rep.error(
                t.line,
                format!("currency '{}' is not a three-letter ISO 4217 code", t.value),
                "use the code in capitals, e.g. `currency: USD`",
            );
        }
        ok
    });
    let week_start =
        value(f.choice(rep, "week_start", WEEKDAYS)).unwrap_or_else(|| "monday".into());
    let max_chain_depth = f.int(rep, "max_chain_depth", 1, 5).unwrap_or(3) as u8;

    let mut teams = Vec::new();
    let mut team_ids = Vec::new();
    for item in f.list(rep, "teams") {
        let Some(t) = fields(rep, item, "team") else {
            continue;
        };
        t.allow(rep, &["id", "name", "description", "parent", "accepts"]);
        let id = t.req_id(rep, "id");
        let name = t.req_text(rep, "name", "Team name");
        let parent = t.id(rep, "parent");
        if let Some(p) = &parent {
            refs.push(rep, p.line, RefKind::Team, &p.value);
            if let Some(id) = &id {
                refs.edge(rep, Tree::TeamParent, &id.value, &p.value, p.line);
            }
        }
        let mut accepts = Vec::new();
        for a in t.texts(rep, "accepts") {
            let shaped = a
                .value
                .split_once('/')
                .is_some_and(|(team, wf)| checks::is_kebab(team) && checks::is_kebab(wf));
            if shaped {
                refs.push(rep, a.line, RefKind::Accepts, &a.value);
                accepts.push(a.value);
            } else {
                rep.error(
                    a.line,
                    format!("accepts entry '{}' is not <team-id>/<workflow-id>", a.value),
                    "name the sending team and its workflow, e.g. `- other-team/its-workflow`",
                );
            }
        }
        if let Some(id) = &id {
            team_ids.push(id.clone());
        }
        if let (Some(id), Some(name)) = (id, name) {
            teams.push(Team {
                id: id.value,
                name: name.value,
                description: value(t.text(rep, "description")),
                parent: value(parent),
                accepts,
            });
        }
    }
    unique(rep, "team", &team_ids);

    let mut hats = Vec::new();
    let mut hat_ids = Vec::new();
    for item in f.list(rep, "hats") {
        let Some(h) = fields(rep, item, "hat") else {
            continue;
        };
        h.allow(rep, &["id", "name", "description"]);
        if let (Some(id), Some(name)) = (h.req_id(rep, "id"), h.req_text(rep, "name", "Hat name")) {
            hat_ids.push(id.clone());
            hats.push(Hat {
                id: id.value,
                name: name.value,
                description: value(h.text(rep, "description")),
            });
        }
    }
    unique(rep, "hat", &hat_ids);

    Some(Company {
        name: name?.value,
        description,
        timezone: tz?.value,
        currency: currency?.value,
        week_start,
        max_chain_depth,
        teams,
        hats,
    })
}

/// `models/accounts.yaml`.
pub(crate) fn accounts(rep: &mut Report, root: &Node) -> Vec<Account> {
    let Some(f) = fields(rep, root, "models/accounts.yaml") else {
        return Vec::new();
    };
    f.allow(rep, ACCOUNTS_KEYS);
    let mut out = Vec::new();
    let mut ids = Vec::new();
    for item in f.list(rep, "accounts") {
        let Some(a) = fields(rep, item, "account") else {
            continue;
        };
        a.allow(
            rep,
            &[
                "id",
                "provider",
                "name",
                "description",
                "base_url",
                "secret_ref",
            ],
        );
        let id = a.req_id(rep, "id");
        if let Some(id) = &id
            && RESERVED_ACCOUNTS.contains(&id.value.as_str())
        {
            rep.error(
                id.line,
                format!(
                    "'{}' is a reserved local Account and is always available",
                    id.value
                ),
                "remove this entry; seats can name it without declaring it",
            );
        }
        let providers = capability::account_providers();
        let providers: Vec<&str> = providers.iter().map(String::as_str).collect();
        let provider = a.choice(rep, "provider", &providers);
        if a.absent("provider") {
            rep.error(
                a.line,
                "account is missing 'provider'",
                format!("add `provider:` with one of: {}", providers.join(", ")),
            );
        }
        let base_url = a
            .text(rep, "base_url")
            .filter(|t| web_url(rep, t, "base_url"));
        if let Some(p) = &provider
            && capability::provider_needs_base_url(&p.value)
            && base_url.is_none()
            && a.absent("base_url")
        {
            let article = if p.value.starts_with(['a', 'e', 'i', 'o', 'u']) {
                "an"
            } else {
                "a"
            };
            rep.error(
                a.line,
                format!("{article} {} account needs 'base_url'", p.value),
                "add `base_url: https://…` (or http://127.0.0.1:<port> or http://localhost:<port> for a server on this Mac)",
            );
        }
        if let Some(id) = &id {
            ids.push(id.clone());
        }
        if let (Some(id), Some(provider)) = (id, provider)
            && let Some(kind) = AccountKind::of_provider(&provider.value)
        {
            out.push(Account {
                id: id.value,
                provider: provider.value,
                kind,
                name: value(a.text(rep, "name")),
                base_url: value(base_url),
                secret_ref: value(a.text(rep, "secret_ref")),
            });
        }
    }
    unique(rep, "account", &ids);
    out
}

fn provider_and_model(rep: &mut Report, f: &Fields) -> Option<(String, String)> {
    let provider = f.choice(rep, "provider", MODEL_PROVIDERS);
    if f.absent("provider") {
        rep.error(
            f.line,
            format!("{} is missing 'provider'", f.what),
            format!(
                "add `provider:` with one of: {}",
                MODEL_PROVIDERS.join(", ")
            ),
        );
    }
    let model = f.req_text(rep, "model", "model-name");
    Some((provider?.value, model?.value))
}

/// `models/prices.yaml`.
pub(crate) fn prices(rep: &mut Report, root: &Node) -> Prices {
    let Some(f) = fields(rep, root, "models/prices.yaml") else {
        return Prices::default();
    };
    f.allow(rep, PRICES_KEYS);
    let updated = f.text(rep, "updated").filter(|t| {
        let ok = t.value.parse::<jiff::civil::Date>().is_ok();
        if !ok {
            rep.error(
                t.line,
                format!("'{}' is not a date", t.value),
                "write the date the prices were checked as YYYY-MM-DD",
            );
        }
        ok
    });
    let mut models = Vec::new();
    for item in f.list(rep, "models") {
        let Some(m) = fields(rep, item, "model price") else {
            continue;
        };
        m.allow(
            rep,
            &[
                "provider",
                "model",
                "input_usd_per_mtok",
                "output_usd_per_mtok",
            ],
        );
        let pm = provider_and_model(rep, &m);
        let input = m.micros(rep, "input_usd_per_mtok");
        let output = m.micros(rep, "output_usd_per_mtok");
        for (name, v) in [
            ("input_usd_per_mtok", &input),
            ("output_usd_per_mtok", &output),
        ] {
            if v.is_none() && m.absent(name) {
                rep.error(
                    m.line,
                    format!("model price is missing '{name}'"),
                    format!("add `{name}:` in USD per million tokens"),
                );
            }
        }
        if let (Some((provider, model)), Some(i), Some(o)) = (pm, input, output) {
            models.push(ModelPrice {
                provider,
                model,
                input_micros_per_mtok: i,
                output_micros_per_mtok: o,
            });
        }
    }
    let mut services = Vec::new();
    for item in f.list(rep, "services") {
        let Some(s) = fields(rep, item, "service price") else {
            continue;
        };
        s.allow(rep, &["id", "unit", "usd_per_unit"]);
        let id = s.req_id(rep, "id");
        let unit = s.req_text(rep, "unit", "credit");
        let price = s.micros(rep, "usd_per_unit");
        if price.is_none() && s.absent("usd_per_unit") {
            rep.error(
                s.line,
                "service price is missing 'usd_per_unit'",
                "add `usd_per_unit:` in USD",
            );
        }
        if let (Some(id), Some(unit), Some(p)) = (id, unit, price) {
            services.push(ServicePrice {
                id: id.value,
                unit: unit.value,
                micros_per_unit: p,
            });
        }
    }
    Prices {
        updated: value(updated),
        models,
        services,
    }
}

/// `models/capabilities.yaml`.
pub(crate) fn capabilities(rep: &mut Report, root: &Node) -> Capabilities {
    let Some(f) = fields(rep, root, "models/capabilities.yaml") else {
        return Capabilities::default();
    };
    f.allow(rep, CAPABILITIES_KEYS);
    let mut models = Vec::new();
    for item in f.list(rep, "models") {
        let Some(m) = fields(rep, item, "model capabilities") else {
            continue;
        };
        m.allow(
            rep,
            &[
                "provider",
                "model",
                "context_window",
                "max_output_tokens",
                "tools",
                "reads_pdf",
                "reads_images",
                "reads_office",
                "reads_audio",
            ],
        );
        let pm = provider_and_model(rep, &m);
        let cap = ModelCapability {
            provider: String::new(),
            model: String::new(),
            context_window: m
                .int(rep, "context_window", 1, 100_000_000)
                .map(|v| v as u64),
            max_output_tokens: m
                .int(rep, "max_output_tokens", 1, 100_000_000)
                .map(|v| v as u64),
            tools: m.bool(rep, "tools"),
            reads_pdf: m.bool(rep, "reads_pdf"),
            reads_images: m.bool(rep, "reads_images"),
            reads_office: m.bool(rep, "reads_office"),
            reads_audio: m.bool(rep, "reads_audio"),
        };
        if let Some((provider, model)) = pm {
            models.push(ModelCapability {
                provider,
                model,
                ..cap
            });
        }
    }
    Capabilities { models }
}

/// `tools.yaml`.
pub(crate) fn tools(rep: &mut Report, refs: &mut Refs, root: &Node) -> ToolsConfig {
    let Some(f) = fields(rep, root, "tools.yaml") else {
        return ToolsConfig::default();
    };
    f.allow(rep, TOOLS_KEYS);
    let mut egress_hosts = Vec::new();
    for h in f.texts(rep, "egress_hosts") {
        let ok = !h.value.is_empty()
            && h.value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
            && h.value.contains('.');
        if ok {
            egress_hosts.push(h.value);
        } else {
            rep.error(
                h.line,
                format!("'{}' is not a host name", h.value),
                "write the host only, without scheme or path, e.g. `- example.org`",
            );
        }
    }
    let mut search_provider = None;
    if let Some(node) = f.get("search")
        && let Some(s) = fields(rep, node, "search")
    {
        s.allow(rep, &["provider"]);
        if let Some(p) = s.text(rep, "provider") {
            if p.value != BUILTIN_PROVIDER {
                refs.push(
                    rep,
                    p.line,
                    RefKind::AccountOfKind {
                        want: AccountKind::Search,
                        user: "search".into(),
                        alternative: Some("set `provider: builtin` (Local search)".into()),
                    },
                    &p.value,
                );
            }
            search_provider = Some(p.value);
        }
    }
    let mut builtin = Vec::new();
    let mut builtin_ids = Vec::new();
    for item in f.list(rep, "builtin") {
        let Some(b) = fields(rep, item, "built-in tool setting") else {
            continue;
        };
        b.allow(rep, &["id", "max_uses_per_step"]);
        let Some(id) = b.req_text(rep, "id", "web_search") else {
            continue;
        };
        builtin_ids.push(id.clone());
        if builtin_tool(&id.value).is_none() {
            rep.error(
                id.line,
                format!("unknown tool '{}'", id.value),
                format!("use a built-in tool id: {}", builtin_tool_ids().join(", ")),
            );
            continue;
        }
        builtin.push(BuiltinToolSetting {
            id: id.value,
            max_uses_per_step: b.int(rep, "max_uses_per_step", 0, 10_000).map(|v| v as u32),
        });
    }
    unique(rep, "built-in tool setting", &builtin_ids);
    let mut mcp_servers = Vec::new();
    let mut server_ids = Vec::new();
    for item in f.list(rep, "mcp_servers") {
        if let Some(server) = mcp_server(rep, item) {
            server_ids.push(Text {
                value: server.id.clone(),
                line: item.line,
            });
            mcp_servers.push(server);
        }
    }
    unique(rep, "MCP server", &server_ids);
    ToolsConfig {
        egress_hosts,
        search_provider,
        builtin,
        mcp_servers,
    }
}

fn mcp_server(rep: &mut Report, item: &Node) -> Option<McpServer> {
    let s = fields(rep, item, "MCP server")?;
    s.allow(
        rep,
        &[
            "id",
            "description",
            "transport",
            "command",
            "args",
            "env",
            "url",
            "version",
            "network",
            "tools",
        ],
    );
    let id = s.req_id(rep, "id");
    let transport = s.choice(rep, "transport", &["stdio", "http"]);
    if s.absent("transport") {
        rep.error(
            s.line,
            "MCP server is missing 'transport'",
            "add `transport: stdio` or `transport: http`",
        );
    }
    let command = s.text(rep, "command");
    let url = s.text(rep, "url").filter(|t| web_url(rep, t, "url"));
    match transport.as_ref().map(|t| t.value.as_str()) {
        Some("stdio") if command.is_none() => rep.error(
            s.line,
            "a stdio MCP server needs 'command'",
            "add `command:` with the program to start",
        ),
        Some("http") if url.is_none() && s.absent("url") => rep.error(
            s.line,
            "an http MCP server needs 'url'",
            "add `url: https://…`",
        ),
        _ => {}
    }
    let env: Vec<String> = s
        .texts(rep, "env")
        .into_iter()
        .filter(|t| {
            let ok = t
                .value
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_uppercase() || c == '_')
                && t.value
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
            if !ok {
                rep.error(
                    t.line,
                    format!("'{}' is not an environment variable name", t.value),
                    "list variable names only (e.g. `- NOTES_ROOT`); values never go in this file",
                );
            }
            ok
        })
        .map(|t| t.value)
        .collect();
    let mut tools = Vec::new();
    let mut names = Vec::new();
    for t in s.list(rep, "tools") {
        let Some(tf) = fields(rep, t, "MCP tool") else {
            continue;
        };
        tf.allow(rep, &["name", "class", "description"]);
        let name = tf.req_text(rep, "name", "tool_name").filter(|n| {
            let ok = !n.value.contains('/') && !n.value.contains(char::is_whitespace);
            if !ok {
                rep.error(
                    n.line,
                    format!("MCP tool name '{}' may not contain '/' or spaces", n.value),
                    "use the name the server reports",
                );
            }
            ok
        });
        let class = tf
            .choice(rep, "class", &["read", "local-write", "external-effect"])
            .and_then(|c| ActionClass::parse(&c.value))
            .unwrap_or(ActionClass::ExternalEffect);
        if let Some(name) = name {
            names.push(name.clone());
            tools.push(McpTool {
                name: name.value,
                class,
            });
        }
    }
    unique(rep, "MCP tool", &names);
    Some(McpServer {
        id: id?.value,
        transport: transport?.value,
        command: value(command),
        args: s.texts(rep, "args").into_iter().map(|t| t.value).collect(),
        env,
        url: value(url),
        version: value(s.text(rep, "version")),
        network: value(s.text(rep, "network")),
        tools,
    })
}

fn model_ref(
    rep: &mut Report,
    refs: &mut Refs,
    node: &Node,
    what: &'static str,
) -> Option<ModelRef> {
    let m = fields(rep, node, what)?;
    m.allow(rep, &["account", "model"]);
    let account = m.req_id(rep, "account");
    if let Some(a) = &account {
        refs.push(
            rep,
            a.line,
            RefKind::AccountOfKind {
                want: AccountKind::Model,
                user: format!("a seat's {what}"),
                alternative: None,
            },
            &a.value,
        );
    }
    let model = m.req_text(rep, "model", "model-name");
    Some(ModelRef {
        account: account?.value,
        model: model?.value,
    })
}

/// `skills/<id>/SKILL.md` (AD-38): its frontmatter (already parsed) and
/// body. `allowed-tools` is kept as "asks for" only.
pub(crate) fn skill(
    rep: &mut Report,
    id: &str,
    root: &Node,
    body: &str,
    body_line: u32,
) -> Option<Skill> {
    let f = fields(rep, root, "SKILL.md")?;
    f.allow(rep, SKILL_KEYS);
    let name = f.req_text(rep, "name", id).filter(|n| {
        let ok = n.value == id;
        if !ok {
            rep.error(
                n.line,
                format!("skill name '{}' does not match its folder '{id}'", n.value),
                format!(
                    "set `name: {id}` (the folder name is the skill id), or rename the folder to skills/{}/",
                    n.value
                ),
            );
        }
        ok
    });
    let description = f.req_text(rep, "description", "What this skill helps with");
    let mut metadata = BTreeMap::new();
    if let Some(node) = f.get("metadata").filter(|n| n.kind != Kind::Null)
        && let Some(m) = fields(rep, node, "metadata")
    {
        for (key, _) in m.entries {
            if let Some(v) = m.text(rep, &key.name) {
                metadata.insert(key.name.clone(), v.value);
            }
        }
    }
    let asks_for: Vec<String> = match f.get("allowed-tools").map(|n| &n.kind) {
        None | Some(Kind::Null) => Vec::new(),
        Some(Kind::Str(s)) => s
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|w| !w.is_empty())
            .map(str::to_string)
            .collect(),
        Some(_) => f
            .texts(rep, "allowed-tools")
            .into_iter()
            .map(|t| t.value)
            .collect(),
    };
    if body.len() > SKILL_BODY_MAX_BYTES {
        rep.error(
            body_line,
            format!(
                "the SKILL.md body is {} bytes; the limit is 64 KB ({SKILL_BODY_MAX_BYTES} bytes)",
                body.len()
            ),
            "shorten the instructions, or move reference material into files under references/",
        );
        return None;
    }
    Some(Skill {
        id: id.to_string(),
        name: name?.value,
        description: description?.value,
        license: value(f.text(rep, "license")),
        metadata,
        asks_for,
        body: body.to_string(),
        references: BTreeMap::new(),
    })
}

const SKILL_BODY_MAX_BYTES: usize = 64 * 1024;

/// The six DESIGN avatar tints.
const TINTS: &[&str] = &["slate", "steel", "sage", "stone", "plum", "clay"];
const GOAL_MAX_CHARS: usize = 500;
const BACKSTORY_MAX_CHARS: usize = 2_000;
const FOLDER_FIX: &str = "link a folder inside the project (e.g. `path: notes`) or under your home folder (e.g. `path: ~/Documents/notes`); never /, ~, ~/Library or a .agentic folder";

/// Where a seat's relative paths resolve: the project folder and the
/// owner's home folder (when known). Resolution is lexical: nothing follows
/// a symlink at validate time.
#[derive(Default)]
pub(crate) struct Places {
    pub project: PathBuf,
    pub home: Option<PathBuf>,
}

/// Plain text of at most `max` characters (counted as chars).
fn max_chars(rep: &mut Report, t: &Text, name: &str, max: usize) -> bool {
    let n = t.value.chars().count();
    if n <= max {
        return true;
    }
    rep.error(
        t.line,
        format!("{name} is {n} characters; the limit is {max}"),
        format!("shorten {name} to at most {max} characters"),
    );
    false
}

/// Compiles a path glob (anchored, `*` stays within one path segment).
fn glob(rep: &mut Report, t: &Text) -> bool {
    match globset::GlobBuilder::new(&t.value)
        .literal_separator(true)
        .build()
    {
        Ok(_) => true,
        Err(e) => {
            rep.error(
                t.line,
                format!("glob '{}' does not compile: {}", t.value, e.kind()),
                "fix the pattern: `*` matches within a folder, `**` across folders, `[abc]` needs its closing `]`",
            );
            false
        }
    }
}

/// The globs listed under `name`, each checked.
fn globs(rep: &mut Report, f: &Fields, name: &str) -> Vec<String> {
    f.texts(rep, name)
        .into_iter()
        .filter(|t| glob(rep, t))
        .map(|t| t.value)
        .collect()
}

/// Lexically normalised components of `path` (split on `/`), and how many
/// `..` steps climbed above its start.
fn normalise(path: &str) -> (Vec<String>, usize) {
    let mut stack: Vec<String> = Vec::new();
    let mut ups = 0;
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if stack.pop().is_none() {
                    ups += 1;
                }
            }
            p => stack.push(p.to_string()),
        }
    }
    (stack, ups)
}

/// The components of an absolute path, without the root, normalised
/// lexically (`std::path::absolute` keeps `..` on Unix): `..` pops.
fn components(path: &Path) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in path.components() {
        match c {
            std::path::Component::Normal(s) => out.push(s.to_string_lossy().into_owned()),
            std::path::Component::ParentDir => {
                out.pop();
            }
            _ => {}
        }
    }
    out
}

/// True for a `.agentic` path component, ignoring case (APFS does).
fn is_agentic(component: &str) -> bool {
    component.eq_ignore_ascii_case(".agentic")
}

const PROJECT_REFUSED: &str = "it is the project folder itself, which holds .agentic/";
const ANCESTOR_REFUSED: &str = "it holds the project folder, which holds .agentic/";

/// `base` (absolute components) climbed by `ups` then extended by `rest`.
fn climb(base: &[String], ups: usize, rest: &[String]) -> Vec<String> {
    let keep = base.len().saturating_sub(ups);
    let mut out = base[..keep].to_vec();
    out.extend(rest.iter().cloned());
    out
}

/// Why an absolute location may not be linked, if it may not.
fn refused_location(
    abs: &[String],
    home: Option<&[String]>,
    project: &[String],
) -> Option<&'static str> {
    if abs.is_empty() {
        return Some("it is the filesystem root");
    }
    if abs.iter().any(|c| is_agentic(c)) {
        return Some("a .agentic folder holds the app's runtime files");
    }
    if let Some(home) = home {
        if abs == home {
            return Some("it is the home folder itself");
        }
        if abs.len() > home.len()
            && abs[..home.len()] == *home
            && abs[home.len()].eq_ignore_ascii_case("library")
        {
            return Some("~/Library holds the app data folder and system files");
        }
    }
    if !project.is_empty() && abs.len() <= project.len() && project[..abs.len()] == *abs {
        return Some(if abs.len() == project.len() {
            PROJECT_REFUSED
        } else {
            ANCESTOR_REFUSED
        });
    }
    None
}

/// Checks a linked folder's path (AD-30) and returns where it resolves to
/// on disk when that can be known, for the existence check. `None` (with an
/// error) when the path is refused.
fn folder_location(rep: &mut Report, places: &Places, t: &Text) -> Option<Option<PathBuf>> {
    let written = t.value.as_str();
    let refuse = |rep: &mut Report, why: &str| {
        rep.error(
            t.line,
            format!("folder path '{written}' is not allowed: {why}"),
            FOLDER_FIX,
        );
    };
    if written.split('/').any(is_agentic) {
        refuse(rep, "a .agentic folder holds the app's runtime files");
        return None;
    }
    if written.starts_with('/') {
        refuse(
            rep,
            "an absolute path; write it relative to the project or to ~/",
        );
        return None;
    }
    let home = places.home.as_deref().map(components);
    let project = std::path::absolute(&places.project)
        .map(|p| components(&p))
        .unwrap_or_default();
    let (home_relative, rest) = if written == "~" {
        (true, "")
    } else if let Some(rest) = written.strip_prefix("~/") {
        (true, rest)
    } else if written.starts_with('~') {
        refuse(rep, "write ~/ for your own home folder");
        return None;
    } else {
        (false, written)
    };
    let (parts, ups) = normalise(rest);
    if home_relative {
        if ups == 0 {
            if parts.is_empty() {
                refuse(rep, "it is the home folder itself");
                return None;
            }
            if parts[0].eq_ignore_ascii_case("library") {
                refuse(rep, "~/Library holds the app data folder and system files");
                return None;
            }
            let Some(home) = &home else {
                return Some(None);
            };
            let abs = climb(home, 0, &parts);
            if let Some(why) = refused_location(&abs, Some(home), &project) {
                refuse(rep, why);
                return None;
            }
            return Some(Some(PathBuf::from(format!("/{}", abs.join("/")))));
        }
        let Some(home) = &home else {
            refuse(rep, "it climbs above the home folder");
            return None;
        };
        let abs = climb(home, ups, &parts);
        if let Some(why) = refused_location(&abs, Some(home), &project) {
            refuse(rep, why);
            return None;
        }
        return Some(Some(PathBuf::from(format!("/{}", abs.join("/")))));
    }
    if ups == 0 {
        if parts.is_empty() {
            refuse(rep, PROJECT_REFUSED);
            return None;
        }
        return Some(Some(places.project.join(parts.join("/"))));
    }
    // It climbs out of the project: judge where it lands.
    let abs = climb(&project, ups, &parts);
    if let Some(why) = refused_location(&abs, home.as_deref(), &project) {
        refuse(rep, why);
        return None;
    }
    Some(Some(PathBuf::from(format!("/{}", abs.join("/")))))
}

/// A seat's `capabilities:` with the defaults filled in (AD-46).
fn seat_capabilities(
    rep: &mut Report,
    refs: &mut Refs,
    f: &Fields,
) -> BTreeMap<Capability, CapabilitySetting> {
    let mut out: BTreeMap<Capability, CapabilitySetting> = Capability::all()
        .into_iter()
        .map(|c| {
            (
                c,
                CapabilitySetting {
                    on: c.default_on(),
                    provider: BUILTIN_PROVIDER.to_string(),
                },
            )
        })
        .collect();
    let Some(node) = f.get("capabilities") else {
        return out;
    };
    if node.kind == Kind::Null {
        return out;
    }
    let Some(c) = fields(rep, node, "capabilities") else {
        return out;
    };
    let ids: Vec<&str> = Capability::all().iter().map(|c| c.id()).collect();
    let mut allowed = ids.clone();
    allowed.push(LEGACY_FOLDER_ACCESS_KEY);
    c.allow(rep, &allowed);
    let both = c.get("folder_access").is_some();
    for (key, value) in c.entries {
        let cap = if key.name == LEGACY_FOLDER_ACCESS_KEY {
            if both {
                rep.error(
                    key.line,
                    "both 'documents' and 'folder_access' are set; 'documents' is the old name of folder_access",
                    "remove `documents:` and keep `folder_access:`",
                );
                continue;
            }
            Capability::FolderAccess
        } else if let Some(cap) = Capability::parse(&key.name) {
            cap
        } else {
            continue; // reported by allow()
        };
        let Some(e) = fields(rep, value, "capability") else {
            continue;
        };
        e.allow(rep, &["on", "provider"]);
        let on = e.bool(rep, "on");
        if e.absent("on") {
            rep.error(
                key.line,
                format!("capability '{}' is missing 'on'", key.name),
                format!("add `on: true` or `on: false` under `{}:`", key.name),
            );
        }
        let mut provider = BUILTIN_PROVIDER.to_string();
        if let Some(p) = e.text(rep, "provider")
            && p.value != BUILTIN_PROVIDER
        {
            match cap.provider_kind() {
                None => rep.error(
                    p.line,
                    format!(
                        "{} offers the built-in provider only, not '{}'",
                        cap.id(),
                        p.value
                    ),
                    "set `provider: builtin` or remove the provider line",
                ),
                Some(kind) => {
                    refs.push(
                        rep,
                        p.line,
                        RefKind::AccountOfKind {
                            want: kind,
                            user: cap.id().to_string(),
                            alternative: Some("set `provider: builtin`".into()),
                        },
                        &p.value,
                    );
                    provider = p.value;
                }
            }
        }
        out.insert(
            cap,
            CapabilitySetting {
                on: on.unwrap_or(cap.default_on()),
                provider,
            },
        );
    }
    out
}

/// A seat's `folders:` (AD-30).
fn seat_folders(rep: &mut Report, places: &Places, f: &Fields) -> Vec<Folder> {
    let mut out = Vec::new();
    let mut ids = Vec::new();
    for item in f.list(rep, "folders") {
        let Some(fo) = fields(rep, item, "folder") else {
            continue;
        };
        fo.allow(rep, &["id", "path", "access", "include", "exclude"]);
        let id = fo.req_id(rep, "id");
        let path = fo.req_text(rep, "path", "notes");
        let access = match fo.choice(rep, "access", &["read", "read_write"]) {
            Some(a) if a.value == "read_write" => Access::ReadWrite,
            _ => Access::Read,
        };
        let include = globs(rep, &fo, "include");
        let exclude = globs(rep, &fo, "exclude");
        let mut path_ok = false;
        if let Some(p) = &path
            && let Some(location) = folder_location(rep, places, p)
        {
            path_ok = true;
            if let Some(location) = location {
                match std::fs::metadata(&location) {
                    Ok(m) if m.is_dir() => {}
                    Ok(_) => {
                        path_ok = false;
                        rep.error(
                            p.line,
                            format!("folder path '{}' is not a folder", p.value),
                            "link a folder, not a file",
                        );
                    }
                    Err(_) => rep.warning(
                        p.line,
                        format!("folder '{}' does not exist yet", p.value),
                        "create the folder or fix the path; the app checks it again when it links the folder",
                    ),
                }
            }
        }
        if let Some(id) = &id {
            ids.push(id.clone());
        }
        if let (Some(id), Some(path), true) = (id, path, path_ok) {
            out.push(Folder {
                id: id.value,
                path: path.value,
                access,
                include,
                exclude,
            });
        }
    }
    unique(rep, "folder", &ids);
    out
}

/// A seat's `knowledge:` entries; a missing file is a warning, since the
/// `.agentic/` folder does not travel with the project (AD-38).
fn seat_knowledge(
    rep: &mut Report,
    places: &Places,
    stem: &str,
    f: &Fields,
) -> Vec<KnowledgeEntry> {
    let mut out = Vec::new();
    let mut files = Vec::new();
    for item in f.list(rep, "knowledge") {
        let Some(k) = fields(rep, item, "knowledge entry") else {
            continue;
        };
        k.allow(rep, &["file", "note"]);
        let Some(file) = k.req_text(rep, "file", "reference.md") else {
            continue;
        };
        let bad = file.value.starts_with('/')
            || file.value.starts_with('~')
            || file.value.contains('\\')
            || file.value.split('/').any(|c| c == ".." || c.is_empty());
        if bad {
            rep.error(
                file.line,
                format!(
                    "reference file '{}' must be a name inside .agentic/knowledge/{stem}/",
                    file.value
                ),
                format!(
                    "write the file name relative to .agentic/knowledge/{stem}/, without `..`, `~` or a leading /"
                ),
            );
            continue;
        }
        let on_disk = places
            .project
            .join(".agentic/knowledge")
            .join(stem)
            .join(&file.value);
        if !on_disk.is_file() {
            rep.warning(
                file.line,
                format!(
                    "reference file '{}' is not in .agentic/knowledge/{stem}/",
                    file.value
                ),
                "copy the file there (that folder does not travel with the project), or remove the entry",
            );
        }
        files.push(file.clone());
        out.push(KnowledgeEntry {
            file: file.value,
            note: value(k.text(rep, "note")),
        });
    }
    unique(rep, "reference file", &files);
    out
}

/// The host of an http or https URL, or `None`.
fn http_host(text: &str) -> Option<String> {
    let u = url::Url::parse(text).ok()?;
    if !matches!(u.scheme(), "http" | "https") {
        return None;
    }
    u.host_str()
        .filter(|h| !h.is_empty())
        .map(|h| h.to_ascii_lowercase())
}

/// A seat's saved `links:`; a host off the allow-list is a warning.
fn seat_links(rep: &mut Report, refs: &mut Refs, stem: &str, f: &Fields) -> Vec<Link> {
    let mut out = Vec::new();
    for item in f.list(rep, "links") {
        let Some(l) = fields(rep, item, "link") else {
            continue;
        };
        l.allow(rep, &["title", "url", "group"]);
        let title = l.req_text(rep, "title", "Link title");
        let url = l.req_text(rep, "url", "https://…");
        let url = url.filter(|u| match http_host(&u.value) {
            Some(host) => {
                refs.host(
                    rep,
                    u.line,
                    &host,
                    stem,
                    "link",
                    format!(
                        "allow {host} for this seat (add it to egress_hosts in tools.yaml; the app confirms a new host), or remove the link"
                    ),
                );
                true
            }
            None => {
                rep.error(
                    u.line,
                    format!("link '{}' is not an http or https URL", u.value),
                    "use a web address starting with https:// (or http://)",
                );
                false
            }
        });
        if let (Some(title), Some(url)) = (title, url) {
            out.push(Link {
                title: title.value,
                url: url.value,
                group: value(l.text(rep, "group")),
            });
        }
    }
    out
}

/// A seat's frontmatter (already parsed) plus its body.
pub(crate) fn seat(
    rep: &mut Report,
    refs: &mut Refs,
    places: &Places,
    stem: &str,
    root: &Node,
    body: &str,
) -> Option<Seat> {
    let f = fields(rep, root, "seat")?;
    f.allow(rep, SEAT_KEYS);
    id_is_stem(rep, &f, stem, "md");
    let name = f.req_text(rep, "name", "Seat name");
    let role = f.req_text(rep, "role", "Analyst");
    let team = f.req_id(rep, "team");
    if let Some(t) = &team {
        refs.push(rep, t.line, RefKind::Team, &t.value);
    }
    let reports_to = f.id(rep, "reports_to");
    if let Some(r) = &reports_to {
        refs.push(rep, r.line, RefKind::Seat, &r.value);
        refs.edge(rep, Tree::ReportsTo, stem, &r.value, r.line);
    }
    let goal = f
        .text(rep, "goal")
        .filter(|t| max_chars(rep, t, "goal", GOAL_MAX_CHARS));
    let backstory = f
        .text(rep, "backstory")
        .filter(|t| max_chars(rep, t, "backstory", BACKSTORY_MAX_CHARS));
    let avatar = f.get("avatar").and_then(|n| {
        let a = fields(rep, n, "avatar")?;
        a.allow(rep, &["tint"]);
        if a.absent("tint") {
            rep.error(
                a.line,
                "avatar is missing 'tint'",
                format!("add `tint:` with one of: {}", TINTS.join(", ")),
            );
        }
        Some(Avatar {
            tint: a.choice(rep, "tint", TINTS)?.value,
        })
    });
    let model = f
        .get("model")
        .and_then(|n| model_ref(rep, refs, n, "model"));
    let fallback: Vec<ModelRef> = f
        .list(rep, "fallback")
        .into_iter()
        .filter_map(|n| model_ref(rep, refs, n, "fallback model"))
        .collect();
    let capabilities = seat_capabilities(rep, refs, &f);
    let folders = seat_folders(rep, places, &f);
    if !folders.is_empty()
        && !capabilities[&Capability::FolderAccess].on
        && let Some((key, _)) = f.entries.iter().find(|(k, _)| k.name == "folders")
    {
        rep.warning(
            key.line,
            "folders are linked but folder_access is off, so this seat cannot read them",
            "switch on folder_access, or remove the folders",
        );
    }
    let text_fallback = f.bool(rep, "text_fallback").unwrap_or(false);
    let skills = f.ids(rep, "skills");
    for s in &skills {
        refs.push(rep, s.line, RefKind::Skill, &s.value);
    }
    unique(rep, "skill", &skills);
    let knowledge = seat_knowledge(rep, places, stem, &f);
    let links = seat_links(rep, refs, stem, &f);

    // Grants: the capability expansion, read_skill_file with a skill, then
    // the listed tools that belong to no capability. Never a skill's
    // allowed-tools (AD-38).
    let on: Vec<Capability> = capabilities
        .iter()
        .filter(|(_, s)| s.on)
        .map(|(c, _)| *c)
        .collect();
    let read_write = folders.iter().any(|f| f.access == Access::ReadWrite);
    let mut grants: BTreeSet<String> = capability::expand(&on, read_write);
    if !skills.is_empty() {
        grants.insert(SKILL_TOOL.to_string());
    }
    let tools = f.texts(rep, "tools");
    for t in &tools {
        refs.push(rep, t.line, RefKind::Tool, &t.value);
        if builtin_tool(&t.value).is_none() {
            grants.insert(t.value.clone()); // an MCP tool (or unknown, reported)
            continue;
        }
        match capability_of_tool(&t.value) {
            None if t.value == SKILL_TOOL && skills.is_empty() => rep.error(
                t.line,
                "read_skill_file comes with an attached skill",
                "attach a skill under `skills:`, or remove read_skill_file from tools",
            ),
            None => {
                grants.insert(t.value.clone());
            }
            Some(cap) if !on.contains(&cap) => rep.error(
                t.line,
                format!(
                    "tool '{}' needs the {} capability, which is off",
                    t.value,
                    cap.id()
                ),
                format!(
                    "switch on {0} in capabilities (`{0}: {{ on: true }}`), or remove {1} from tools",
                    cap.id(),
                    t.value
                ),
            ),
            Some(_) if t.value == FOLDER_WRITE_TOOL && !read_write => rep.error(
                t.line,
                "tool 'write_file' needs a folder linked read_write",
                "link a folder with `access: read_write` under folders:, or remove write_file from tools",
            ),
            Some(_) => {}
        }
    }
    unique(rep, "tool", &tools);
    let budget = f.get("budget").and_then(|n| {
        let b = fields(rep, n, "budget")?;
        b.allow(rep, &["per_run", "per_week"]);
        Some(Budget {
            per_run: b.micros(rep, "per_run"),
            per_week: b.micros(rep, "per_week"),
        })
    });
    Some(Seat {
        id: stem.to_string(),
        name: name?.value,
        role: role?.value,
        team: team?.value,
        description: value(f.text(rep, "description")),
        reports_to: value(reports_to),
        model,
        fallback,
        tools: tools.into_iter().map(|t| t.value).collect(),
        budget,
        goal: value(goal),
        backstory: value(backstory),
        avatar,
        capabilities,
        folders,
        text_fallback,
        skills: skills.into_iter().map(|s| s.value).collect(),
        knowledge,
        links,
        grants,
        instructions: body.to_string(),
    })
}

/// True when a seat's frontmatter switches `crawl` on, read from the node
/// so that one local error does not cascade into Source errors.
pub(crate) fn declared_crawl_on(root: &Node) -> bool {
    root.as_map()
        .and_then(|m| m.iter().find(|(k, _)| k.name == "capabilities"))
        .and_then(|(_, caps)| caps.as_map())
        .and_then(|m| m.iter().find(|(k, _)| k.name == Capability::Crawl.id()))
        .and_then(|(_, c)| c.as_map())
        .and_then(|m| m.iter().find(|(k, _)| k.name == "on"))
        .is_some_and(|(_, v)| v.kind == Kind::Bool(true))
}

/// `workflows/<id>.yaml`.
pub(crate) fn workflow(
    rep: &mut Report,
    refs: &mut Refs,
    stem: &str,
    root: &Node,
) -> Option<Workflow> {
    let f = fields(rep, root, "workflow")?;
    f.allow(rep, WORKFLOW_KEYS);
    id_is_stem(rep, &f, stem, "yaml");
    let name = f.req_text(rep, "name", "Workflow name");
    let team = f.req_id(rep, "team");
    if let Some(t) = &team {
        refs.push(rep, t.line, RefKind::Team, &t.value);
    }
    let triggers: Vec<Trigger> = f
        .list(rep, "triggers")
        .into_iter()
        .filter_map(|n| trigger(rep, refs, n))
        .collect();
    let caps = f.get("caps").and_then(|n| {
        let c = fields(rep, n, "caps")?;
        c.allow(
            rep,
            &["per_run", "scheduled_per_run", "per_week", "per_month"],
        );
        Some(Caps {
            per_run: c.micros(rep, "per_run"),
            scheduled_per_run: c.micros(rep, "scheduled_per_run"),
            per_week: c.micros(rep, "per_week"),
            per_month: c.micros(rep, "per_month"),
        })
    });
    let items = f.list(rep, "steps");
    if items.is_empty()
        && f.get("steps")
            .is_none_or(|n| matches!(n.kind, Kind::Null | Kind::Seq(_)))
    {
        rep.error(
            f.line,
            "workflow has no steps",
            "add `steps:` with at least one `- id: …` step naming a seat and an instruction",
        );
    }
    let mut steps = Vec::new();
    let mut step_ids = Vec::new();
    for item in items {
        let Some(s) = fields(rep, item, "step") else {
            continue;
        };
        s.allow(
            rep,
            &[
                "id",
                "seat",
                "instruction",
                "description",
                "output_schema",
                "max_tokens",
                // Run-graph keys (AD-40..AD-42), declared here and checked
                // from epic 3 (stories 3.14-3.16) and 4.15.
                "seats",
                "join",
                "over_folder",
                "routes",
                "files",
                "handoff",
                "review",
                "max_parallel",
            ],
        );
        let id = s.req_id(rep, "id");
        // A fan-out step names its seats (or its folder routes) instead.
        let fan_out = !s.absent("seats") || !s.absent("over_folder");
        let seat = if fan_out {
            s.id(rep, "seat")
        } else {
            s.req_id(rep, "seat")
        };
        if let Some(seat) = &seat {
            refs.push(rep, seat.line, RefKind::Seat, &seat.value);
        }
        let instruction = s.req_text(rep, "instruction", "What this step does");
        let output_schema = s.id(rep, "output_schema");
        if let Some(o) = &output_schema {
            refs.push(rep, o.line, RefKind::Schema, &o.value);
        }
        let max_tokens = s.int(rep, "max_tokens", 1, 10_000_000).map(|v| v as u32);
        if let Some(id) = &id {
            step_ids.push(id.clone());
        }
        if let (Some(id), true, Some(instruction)) = (id, seat.is_some() || fan_out, instruction) {
            steps.push(Step {
                id: id.value,
                seat: value(seat),
                instruction: instruction.value,
                description: value(s.text(rep, "description")),
                output_schema: value(output_schema),
                max_tokens,
            });
        }
    }
    unique(rep, "step", &step_ids);
    Some(Workflow {
        id: stem.to_string(),
        name: name?.value,
        team: team?.value,
        description: value(f.text(rep, "description")),
        triggers,
        caps,
        steps,
    })
}

fn trigger(rep: &mut Report, refs: &mut Refs, node: &Node) -> Option<Trigger> {
    let t = fields(rep, node, "trigger")?;
    t.allow(rep, &["schedule", "event"]);
    match (t.get("schedule"), t.get("event")) {
        (Some(s), None) => {
            let s = fields(rep, s, "schedule")?;
            s.allow(rep, &["cron", "timezone", "catch_up"]);
            let cron = s.req_text(rep, "cron", "\"0 9 * * 1-5\"").filter(|c| {
                match cron::check(&c.value) {
                    Ok(()) => true,
                    Err(why) => {
                        rep.error(
                            c.line,
                            format!("invalid cron \"{}\": {why}", c.value),
                            cron::CRON_FIX,
                        );
                        false
                    }
                }
            });
            let tz = s.text(rep, "timezone").filter(|z| timezone(rep, z));
            let catch_up = value(s.choice(rep, "catch_up", &["once", "skip", "all"]))
                .unwrap_or_else(|| "once".into());
            Some(Trigger::Schedule {
                cron: cron?.value,
                timezone: value(tz),
                catch_up,
            })
        }
        (None, Some(e)) => {
            let e = fields(rep, e, "event")?;
            e.allow(rep, &["on", "only_if"]);
            let on = e.req_text(rep, "on", "workflow_completed:<workflow-id>")?;
            let only_if = value(e.choice(rep, "only_if", &["succeeded", "any"]))
                .unwrap_or_else(|| "succeeded".into());
            let ok = match on.value.split_once(':') {
                Some(("workflow_completed", wf)) if checks::is_kebab(wf) => {
                    refs.push(rep, on.line, RefKind::Workflow, wf);
                    true
                }
                Some(("step_completed", target)) => match target.split_once('/') {
                    Some((wf, step)) if checks::is_kebab(wf) && checks::is_kebab(step) => {
                        refs.push(rep, on.line, RefKind::Step(wf.to_string()), step);
                        true
                    }
                    _ => false,
                },
                Some(("document_updated", doc)) => checks::is_kebab(doc),
                _ => false,
            };
            if !ok {
                rep.error(
                    on.line,
                    format!("event '{}' is not a known event", on.value),
                    "use document_updated:<document-id>, step_completed:<workflow-id>/<step-id> or workflow_completed:<workflow-id>",
                );
                return None;
            }
            Some(Trigger::Event {
                on: on.value,
                only_if,
            })
        }
        _ => {
            rep.error(
                t.line,
                "a trigger is either `schedule:` or `event:`",
                "write `- schedule: { cron: \"0 9 * * 1-5\" }` or `- event: { on: workflow_completed:<id> }`",
            );
            None
        }
    }
}

fn is_column_id(id: &str) -> bool {
    id.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// `tables/<id>.yaml`.
pub(crate) fn table(rep: &mut Report, refs: &mut Refs, stem: &str, root: &Node) -> Option<Table> {
    let f = fields(rep, root, "table")?;
    f.allow(rep, TABLE_KEYS);
    id_is_stem(rep, &f, stem, "yaml");
    let name = f.req_text(rep, "name", "Table name");
    let team = f.req_id(rep, "team");
    if let Some(t) = &team {
        refs.push(rep, t.line, RefKind::Team, &t.value);
    }
    let mut columns = Vec::new();
    let mut column_ids = Vec::new();
    let items = f.list(rep, "columns");
    if items.is_empty()
        && f.get("columns")
            .is_none_or(|n| matches!(n.kind, Kind::Null | Kind::Seq(_)))
    {
        rep.error(
            f.line,
            "table has no columns",
            "add `columns:` with at least one `- id: …` and `type: …`",
        );
    }
    for item in items {
        let Some(c) = fields(rep, item, "column") else {
            continue;
        };
        c.allow(
            rep,
            &[
                "id",
                "type",
                "required",
                "description",
                "values",
                "transitions",
            ],
        );
        let id = c.req_text(rep, "id", "column_name").filter(|t| {
            let ok = is_column_id(&t.value);
            if !ok {
                rep.error(
                    t.line,
                    format!("column id '{}' must be snake_case", t.value),
                    "use lowercase letters, digits and underscores, starting with a letter",
                );
            }
            ok
        });
        let column_type = c.choice(rep, "type", COLUMN_TYPES);
        if c.absent("type") {
            rep.error(
                c.line,
                "column is missing 'type'",
                format!("add `type:` with one of: {}", COLUMN_TYPES.join(", ")),
            );
        }
        let values = c.texts(rep, "values");
        let is_status = column_type.as_ref().is_some_and(|t| t.value == "status");
        if is_status && values.is_empty() {
            rep.error(
                c.line,
                "a status column needs 'values'",
                "add `values:` listing each status",
            );
        }
        if !is_status && !values.is_empty() {
            rep.error(
                values[0].line,
                "only a status column takes 'values'",
                "set `type: status` or remove `values:`",
            );
        }
        unique(rep, "value", &values);
        let mut transitions = Vec::new();
        for t in c.list(rep, "transitions") {
            let Some(tf) = fields(rep, t, "transition") else {
                continue;
            };
            tf.allow(rep, &["from", "to"]);
            let from = tf.req_text(rep, "from", "a-status");
            let to = tf.req_text(rep, "to", "a-status");
            for end in [&from, &to].into_iter().flatten() {
                if !values.iter().any(|v| v.value == end.value) {
                    let known: Vec<&str> = values.iter().map(|v| v.value.as_str()).collect();
                    rep.error(
                        end.line,
                        format!("'{}' is not one of this column's values", end.value),
                        format!("use one of: {}", known.join(", ")),
                    );
                }
            }
            if let (Some(from), Some(to)) = (from, to) {
                transitions.push(Transition {
                    from: from.value,
                    to: to.value,
                });
            }
        }
        if let Some(id) = &id {
            column_ids.push(id.clone());
        }
        if let (Some(id), Some(column_type)) = (id, column_type) {
            columns.push(Column {
                id: id.value,
                column_type: column_type.value,
                required: c.bool(rep, "required").unwrap_or(false),
                description: value(c.text(rep, "description")),
                values: values.into_iter().map(|v| v.value).collect(),
                transitions,
            });
        }
    }
    unique(rep, "column", &column_ids);
    let known: Vec<&str> = column_ids.iter().map(|c| c.value.as_str()).collect();
    let column_ref = |rep: &mut Report, t: &Text| {
        if !known.contains(&t.value.as_str()) {
            rep.error(
                t.line,
                format!("unknown column '{}'", t.value),
                format!("use one of this table's columns: {}", known.join(", ")),
            );
        }
    };
    let key = f.text(rep, "key");
    if let Some(k) = &key {
        column_ref(rep, k);
    }
    let mut views = Vec::new();
    let mut view_ids = Vec::new();
    for item in f.list(rep, "views") {
        let Some(v) = fields(rep, item, "view") else {
            continue;
        };
        v.allow(rep, &["id", "name", "columns"]);
        let id = v.req_id(rep, "id");
        let name = v.req_text(rep, "name", "View name");
        let cols = v.texts(rep, "columns");
        for c in &cols {
            column_ref(rep, c);
        }
        unique(rep, "view column", &cols);
        if let Some(id) = &id {
            view_ids.push(id.clone());
        }
        if let (Some(id), Some(name)) = (id, name) {
            views.push(View {
                id: id.value,
                name: name.value,
                columns: cols.into_iter().map(|c| c.value).collect(),
            });
        }
    }
    unique(rep, "view", &view_ids);
    let grants = f.ids(rep, "grants");
    unique(rep, "grant", &grants);
    for g in &grants {
        if team.as_ref().is_some_and(|t| t.value == g.value) {
            rep.error(
                g.line,
                format!(
                    "team '{}' owns this table; a grant is for another team",
                    g.value
                ),
                "remove this grant",
            );
        } else {
            refs.push(rep, g.line, RefKind::Team, &g.value);
        }
    }
    Some(Table {
        id: stem.to_string(),
        name: name?.value,
        team: team?.value,
        description: value(f.text(rep, "description")),
        key: value(key),
        columns,
        views,
        grants: grants.into_iter().map(|g| g.value).collect(),
    })
}

/// `dashboards/<id>.yaml`.
pub(crate) fn dashboard(
    rep: &mut Report,
    refs: &mut Refs,
    stem: &str,
    root: &Node,
) -> Option<Dashboard> {
    let f = fields(rep, root, "dashboard")?;
    f.allow(rep, DASHBOARD_KEYS);
    id_is_stem(rep, &f, stem, "yaml");
    let name = f.req_text(rep, "name", "Dashboard name");
    let team = f.id(rep, "team");
    if let Some(t) = &team {
        refs.push(rep, t.line, RefKind::Team, &t.value);
    }
    let mut cards = Vec::new();
    let mut card_ids = Vec::new();
    for item in f.list(rep, "cards") {
        let Some(c) = fields(rep, item, "card") else {
            continue;
        };
        c.allow(rep, &["id", "title", "query", "layout"]);
        let id = c.req_id(rep, "id");
        let title = c.req_text(rep, "title", "Card title");
        let query = c
            .req_text(rep, "query", "SELECT … FROM v_<table>")
            .filter(|q| {
                let first = q
                    .value
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_ascii_uppercase();
                let ok = first == "SELECT" || first == "WITH";
                if !ok {
                    rep.error(
                        q.line,
                        "a card query must be one read-only SELECT",
                        "start the query with SELECT (or WITH … SELECT) over the v_* views",
                    );
                }
                ok
            });
        let layout = c.get("layout").and_then(|n| {
            let l = fields(rep, n, "layout")?;
            l.allow(rep, &["x", "y", "w", "h"]);
            Some(Layout {
                x: l.int(rep, "x", 0, 1000).unwrap_or(0) as u32,
                y: l.int(rep, "y", 0, 1000).unwrap_or(0) as u32,
                w: l.int(rep, "w", 1, 1000).unwrap_or(1) as u32,
                h: l.int(rep, "h", 1, 1000).unwrap_or(1) as u32,
            })
        });
        if let Some(id) = &id {
            card_ids.push(id.clone());
        }
        if let (Some(id), Some(title), Some(query)) = (id, title, query) {
            cards.push(Card {
                id: id.value,
                title: title.value,
                query: query.value,
                layout,
            });
        }
    }
    unique(rep, "card", &card_ids);
    Some(Dashboard {
        id: stem.to_string(),
        name: name?.value,
        team: value(team),
        description: value(f.text(rep, "description")),
        cards,
    })
}

const SOURCE_TYPES: &[&str] = &[
    "web-search",
    "http-reader",
    "crawler",
    "api",
    "feed",
    "local-repository",
    "knowledge",
    "database",
    "mcp",
    "action",
];
/// Source types that read websites: they carry `terms_checked`.
const WEBSITE_TYPES: &[&str] = &["http-reader", "crawler", "feed"];
/// Source types that may render pages built by scripts (AD-39).
const RENDER_TYPES: &[&str] = &["http-reader", "crawler"];

/// An http or https URL in a Source's scope; its host, when it is one.
fn scope_url(rep: &mut Report, t: &Text) -> Option<String> {
    let host = http_host(&t.value);
    if host.is_none() {
        rep.error(
            t.line,
            format!("'{}' is not an http or https URL", t.value),
            "use a web address starting with https:// (or http://)",
        );
    }
    host
}

/// Records a scope host for the owner seat's allow-list warning.
fn scope_host(rep: &mut Report, refs: &mut Refs, owner: Option<&Text>, line: u32, host: &str) {
    if let Some(owner) = owner {
        refs.host(
            rep,
            line,
            host,
            &owner.value,
            "scope",
            format!(
                "allow {host} for seat '{}' (add it to egress_hosts in tools.yaml; the app confirms a new host), or change the scope",
                owner.value
            ),
        );
    }
}

/// The line of `key` in `f`, or the mapping's line.
fn key_line(f: &Fields, key: &str) -> u32 {
    f.entries
        .iter()
        .find(|(k, _)| k.name == key)
        .map_or(f.line, |(k, _)| k.line)
}

fn source_scope(
    rep: &mut Report,
    refs: &mut Refs,
    owner: Option<&Text>,
    node: &Node,
) -> SourceScope {
    let Some(s) = fields(rep, node, "scope") else {
        return SourceScope::default();
    };
    s.allow(
        rep,
        &[
            "seeds",
            "url",
            "include",
            "exclude",
            "allowed_hosts",
            "max_depth",
        ],
    );
    let mut seeds = Vec::new();
    for t in s.texts(rep, "seeds") {
        if let Some(host) = scope_url(rep, &t) {
            scope_host(rep, refs, owner, t.line, &host);
            seeds.push(t.value);
        }
    }
    let url = s.text(rep, "url").filter(|t| match scope_url(rep, t) {
        Some(host) => {
            scope_host(rep, refs, owner, t.line, &host);
            true
        }
        None => false,
    });
    let include = globs(rep, &s, "include");
    let exclude = globs(rep, &s, "exclude");
    let mut allowed_hosts = Vec::new();
    for h in s.texts(rep, "allowed_hosts") {
        let ok = !h.value.is_empty()
            && h.value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
            && h.value.contains('.');
        if ok {
            scope_host(rep, refs, owner, h.line, &h.value.to_ascii_lowercase());
            allowed_hosts.push(h.value);
        } else {
            rep.error(
                h.line,
                format!("'{}' is not a host name", h.value),
                "write the host only, without scheme or path, e.g. `- example.org`",
            );
        }
    }
    SourceScope {
        seeds,
        url: value(url),
        include,
        exclude,
        allowed_hosts,
        max_depth: s.int(rep, "max_depth", 0, 10).map(|v| v as u32),
    }
}

fn source_limits(rep: &mut Report, node: &Node) -> SourceLimits {
    let Some(l) = fields(rep, node, "limits") else {
        return SourceLimits::default();
    };
    l.allow(
        rep,
        &[
            "max_items_per_load",
            "max_bytes_per_load",
            "max_loads_per_day",
            "max_spend_per_load",
            "rate_per_host",
        ],
    );
    let rate_per_host_ms = l.get("rate_per_host").and_then(|n| {
        let seconds =
            match n.kind {
                Kind::Int(i) => i as f64,
                Kind::Float(f) => f,
                Kind::Null => return None,
                _ => {
                    rep.error(
                    n.line,
                    format!("'rate_per_host' must be a number of seconds, not {}", n.type_name()),
                    "set `rate_per_host:` to the seconds between requests to one host, at least 1",
                );
                    return None;
                }
            };
        if !(1.0..=86_400.0).contains(&seconds) {
            rep.error(
                n.line,
                format!("'rate_per_host' is {seconds} seconds; it must be from 1 to 86400 seconds"),
                "set `rate_per_host:` to the seconds between requests to one host, from 1 to 86400",
            );
            return None;
        }
        Some((seconds * 1000.0).round() as u64)
    });
    SourceLimits {
        max_items_per_load: l
            .int(rep, "max_items_per_load", 1, 10_000)
            .map(|v| v as u32),
        max_bytes_per_load: l
            .int(rep, "max_bytes_per_load", 1, 1_000_000_000)
            .map(|v| v as u64),
        max_loads_per_day: l.int(rep, "max_loads_per_day", 1, 96).map(|v| v as u32),
        max_spend_per_load: l.micros(rep, "max_spend_per_load"),
        rate_per_host_ms,
    }
}

const OUTPUT_FIX: &str = "write exactly one of `table: <table-id>` (with `key:` and `mapping:`), `records: true` or `knowledge: seat` (or team)";

fn source_output(rep: &mut Report, refs: &mut Refs, f: &Fields) -> Option<SourceOutput> {
    if f.absent("output") {
        rep.error(f.line, "source is missing 'output'", OUTPUT_FIX);
        return None;
    }
    let line = key_line(f, "output");
    let o = fields(rep, f.get("output")?, "output")?;
    o.allow(rep, &["table", "key", "mapping", "records", "knowledge"]);
    let forms = ["table", "records", "knowledge"]
        .iter()
        .filter(|k| !o.absent(k))
        .count();
    if forms != 1 {
        rep.error(
            line,
            "output must have exactly one of table, records or knowledge",
            OUTPUT_FIX,
        );
        return None;
    }
    if o.absent("table") {
        for k in ["key", "mapping"] {
            if !o.absent(k) {
                rep.error(
                    key_line(&o, k),
                    format!("'{k}' goes with a table output"),
                    format!("remove `{k}:`, or write the output as `table: <table-id>`"),
                );
            }
        }
    }
    if !o.absent("records") {
        return match o.bool(rep, "records") {
            Some(true) => Some(SourceOutput::Records),
            Some(false) => {
                rep.error(
                    key_line(&o, "records"),
                    "'records: false' gives the Source nowhere to put its items",
                    OUTPUT_FIX,
                );
                None
            }
            None => None,
        };
    }
    if !o.absent("knowledge") {
        let scope = o.choice(rep, "knowledge", &["seat", "team"])?;
        return Some(SourceOutput::Knowledge { scope: scope.value });
    }
    let table = o.id(rep, "table");
    if let Some(t) = &table {
        refs.push(rep, t.line, RefKind::Table, &t.value);
    }
    let key = o.req_text(rep, "key", "column_id");
    if let (Some(t), Some(k)) = (&table, &key) {
        refs.push(rep, k.line, RefKind::TableColumn(t.value.clone()), &k.value);
    }
    let mut mapping = BTreeMap::new();
    match o.get("mapping") {
        None => rep.error(
            o.line,
            "a table output needs 'mapping'",
            "add `mapping:` with one `<column>: <source field>` line per column",
        ),
        Some(node) => {
            if let Some(m) = fields(rep, node, "mapping") {
                for (column, field) in m.entries {
                    if let Some(t) = &table {
                        refs.push(
                            rep,
                            column.line,
                            RefKind::TableColumn(t.value.clone()),
                            &column.name,
                        );
                    }
                    match &field.kind {
                        Kind::Str(s) if !s.trim().is_empty() => {
                            mapping.insert(column.name.clone(), s.clone());
                        }
                        _ => rep.error(
                            field.line,
                            format!(
                                "mapping for column '{}' must name a source field",
                                column.name
                            ),
                            format!("write `{}: <source field>`", column.name),
                        ),
                    }
                }
            }
        }
    }
    Some(SourceOutput::Table {
        table: table?.value,
        key: key?.value,
        mapping,
    })
}

/// `sources/<id>.yaml` (AD-39).
pub(crate) fn source(
    rep: &mut Report,
    refs: &mut Refs,
    stem: &str,
    root: &Node,
    text: &str,
) -> Option<Source> {
    let f = fields(rep, root, "source")?;
    f.allow(rep, SOURCE_KEYS);
    id_is_stem(rep, &f, stem, "yaml");
    let source_type = f.choice(rep, "type", SOURCE_TYPES);
    if f.absent("type") {
        rep.error(
            f.line,
            "source is missing 'type'",
            format!("add `type:` with one of: {}", SOURCE_TYPES.join(", ")),
        );
    }
    let kind = source_type
        .as_ref()
        .map(|t| t.value.clone())
        .unwrap_or_default();
    let owner = f.req_id(rep, "owner_seat");
    if let Some(o) = &owner {
        refs.push(rep, o.line, RefKind::Seat, &o.value);
        if kind == "crawler" {
            refs.push(rep, o.line, RefKind::CrawlOwner, &o.value);
        }
    }
    let account = f.id(rep, "account");
    if let Some(a) = &account {
        let want = match kind.as_str() {
            "crawler" | "http-reader" => Some(Some((
                AccountKind::CrawlService,
                "remove `account:` to use the built-in reader",
            ))),
            "web-search" => Some(Some((
                AccountKind::Search,
                "remove `account:` to use Local search",
            ))),
            "api" => Some(None),
            "" => Some(None),
            _ => None,
        };
        match want {
            Some(Some((want, alternative))) => refs.push(
                rep,
                a.line,
                RefKind::AccountOfKind {
                    want,
                    user: format!("a {kind} Source"),
                    alternative: Some(alternative.into()),
                },
                &a.value,
            ),
            Some(None) => refs.push(rep, a.line, RefKind::Account, &a.value),
            None => rep.error(
                a.line,
                format!("a {kind} Source takes no account"),
                "remove `account:`",
            ),
        }
    }
    let scope = f
        .get("scope")
        .filter(|n| n.kind != Kind::Null)
        .map(|n| source_scope(rep, refs, owner.as_ref(), n))
        .unwrap_or_default();
    let trigger = f
        .get("trigger")
        .filter(|n| n.kind != Kind::Null)
        .and_then(|n| trigger(rep, refs, n));
    let limits = f
        .get("limits")
        .filter(|n| n.kind != Kind::Null)
        .map(|n| source_limits(rep, n))
        .unwrap_or_default();
    let output = source_output(rep, refs, &f);
    let render = f.bool(rep, "render").unwrap_or(false);
    if render && !kind.is_empty() && !RENDER_TYPES.contains(&kind.as_str()) {
        rep.error(
            key_line(&f, "render"),
            format!("render: true is only for http-reader and crawler Sources, not {kind}"),
            "remove `render: true`; only http-reader and crawler Sources render pages built by scripts",
        );
    }
    let terms_fix =
        "check that the site's terms allow automated access, then set `terms_checked: YYYY-MM-DD`";
    let terms = f.text(rep, "terms_checked");
    let terms_checked = match terms {
        Some(t) if !t.value.trim().is_empty() => {
            if t.value.parse::<jiff::civil::Date>().is_ok() {
                Some(t.value)
            } else {
                rep.error(
                    t.line,
                    format!("'{}' is not a date", t.value),
                    "write the date you checked the site's terms as YYYY-MM-DD",
                );
                None
            }
        }
        Some(t) => {
            if WEBSITE_TYPES.contains(&kind.as_str()) {
                rep.warning(t.line, "terms_checked is empty", terms_fix);
            }
            None
        }
        None => {
            if WEBSITE_TYPES.contains(&kind.as_str()) {
                rep.warning(
                    1,
                    format!("a {kind} Source has no terms_checked date"),
                    terms_fix,
                );
            }
            None
        }
    };
    Some(Source {
        id: stem.to_string(),
        name: value(f.text(rep, "name")),
        description: value(f.text(rep, "description")),
        source_type: source_type?.value,
        owner_seat: owner?.value,
        account: value(account),
        scope,
        trigger,
        limits,
        output: output?,
        render,
        terms_checked,
        text: text.to_string(),
    })
}

/// `schemas/<id>.json`: valid JSON, a JSON Schema, `schema_version: 1`.
pub(crate) fn schema(rep: &mut Report, text: &str) -> Option<serde_json::Value> {
    let value: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            rep.error(
                u32::try_from(e.line()).unwrap_or(1),
                format!("not valid JSON: {e}"),
                "fix the JSON syntax at this line (a missing comma, quote or closing bracket)",
            );
            return None;
        }
    };
    // The same text through the YAML reader gives each key its line.
    let tree = node::parse(text, 0).ok();
    let Some(tree) = tree else {
        rep.error(
            1,
            "the file could not be read for line numbers",
            "keep the file plain JSON",
        );
        return None;
    };
    let tree = top(rep, tree)?;
    let before = rep.findings.len();
    crate::schema_check::check(rep, &tree, true);
    if rep.findings.len() > before {
        return None;
    }
    Some(value)
}

/// Text of `node[key]` when it is a plain string.
fn str_at(node: &Node, key: &str) -> Option<String> {
    node.as_map()?
        .iter()
        .find(|(k, _)| k.name == key)
        .and_then(|(_, v)| match &v.kind {
            Kind::Str(s) => Some(s.clone()),
            _ => None,
        })
}

/// The items of the list `node[key]`, or nothing.
fn items_at<'n>(node: &'n Node, key: &str) -> Vec<&'n Node> {
    let Some(entries) = node.as_map() else {
        return Vec::new();
    };
    match entries
        .iter()
        .find(|(k, _)| k.name == key)
        .map(|(_, v)| &v.kind)
    {
        Some(Kind::Seq(items)) => items.iter().collect(),
        _ => Vec::new(),
    }
}

/// The `id`s declared in the list `root[key]`, whether or not each entry
/// passed its other checks, so one local error does not turn every
/// reference to it into an "unknown" error elsewhere.
pub(crate) fn declared_ids(root: &Node, key: &str) -> Vec<String> {
    items_at(root, key)
        .into_iter()
        .filter_map(|item| str_at(item, "id"))
        .collect()
}

/// The `(id, provider)` of each Account declared in accounts.yaml.
pub(crate) fn declared_account_providers(root: &Node) -> Vec<(String, String)> {
    items_at(root, "accounts")
        .into_iter()
        .filter_map(|item| Some((str_at(item, "id")?, str_at(item, "provider")?)))
        .collect()
}

/// A table's declared column ids.
pub(crate) fn declared_columns(root: &Node) -> Vec<String> {
    declared_ids(root, "columns")
}

/// A workflow's declared team and step ids.
pub(crate) fn declared_workflow(root: &Node) -> (Option<String>, Vec<String>) {
    (str_at(root, "team"), declared_ids(root, "steps"))
}

/// The `<server-id>/<tool-name>` MCP tools declared in tools.yaml.
pub(crate) fn declared_mcp_tools(root: &Node) -> Vec<String> {
    let mut out = Vec::new();
    for server in items_at(root, "mcp_servers") {
        if let Some(id) = str_at(server, "id") {
            for tool in items_at(server, "tools") {
                if let Some(name) = str_at(tool, "name") {
                    out.push(format!("{id}/{name}"));
                }
            }
        }
    }
    out
}
