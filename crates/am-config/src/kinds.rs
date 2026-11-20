//! One reader per file kind of the project layout (Structural Seed). Each
//! reads a parsed node tree into its entity, reports problems at their line
//! and records the references it makes for the cross-file check.

use am_domain::tool_manifest::{ActionClass, builtin_tool, builtin_tool_ids};

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
const ACCOUNT_PROVIDERS: &[&str] = &["anthropic", "openai", "openai-compatible"];
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
    f.allow(
        rep,
        &[
            "schema_version",
            "name",
            "description",
            "timezone",
            "currency",
            "week_start",
            "max_chain_depth",
            "teams",
            "hats",
        ],
    );
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
    f.allow(rep, &["schema_version", "accounts"]);
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
        let provider = a.choice(rep, "provider", ACCOUNT_PROVIDERS);
        if a.absent("provider") {
            rep.error(
                a.line,
                "account is missing 'provider'",
                format!(
                    "add `provider:` with one of: {}",
                    ACCOUNT_PROVIDERS.join(", ")
                ),
            );
        }
        let base_url = a
            .text(rep, "base_url")
            .filter(|t| web_url(rep, t, "base_url"));
        if provider
            .as_ref()
            .is_some_and(|p| p.value == "openai-compatible")
            && base_url.is_none()
            && a.absent("base_url")
        {
            rep.error(
                a.line,
                "an openai-compatible account needs 'base_url'",
                "add `base_url: https://…` (or http://127.0.0.1:<port> or http://localhost:<port> for a server on this Mac)",
            );
        }
        if let Some(id) = &id {
            ids.push(id.clone());
        }
        if let (Some(id), Some(provider)) = (id, provider) {
            out.push(Account {
                id: id.value,
                provider: provider.value,
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
    f.allow(rep, &["schema_version", "updated", "models", "services"]);
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
    f.allow(rep, &["schema_version", "models"]);
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
    f.allow(
        rep,
        &[
            "schema_version",
            "egress_hosts",
            "search",
            "builtin",
            "mcp_servers",
        ],
    );
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
            if p.value != "builtin" {
                refs.push(rep, p.line, RefKind::Account, &p.value);
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
        refs.push(rep, a.line, RefKind::Account, &a.value);
    }
    let model = m.req_text(rep, "model", "model-name");
    Some(ModelRef {
        account: account?.value,
        model: model?.value,
    })
}

/// A seat's frontmatter (already parsed) plus its body.
pub(crate) fn seat(
    rep: &mut Report,
    refs: &mut Refs,
    stem: &str,
    root: &Node,
    body: &str,
) -> Option<Seat> {
    let f = fields(rep, root, "seat")?;
    f.allow(
        rep,
        &[
            "schema_version",
            "id",
            "name",
            "role",
            "team",
            "description",
            "reports_to",
            "model",
            "fallback",
            "tools",
            "budget",
        ],
    );
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
    let model = f
        .get("model")
        .and_then(|n| model_ref(rep, refs, n, "model"));
    let fallback: Vec<ModelRef> = f
        .list(rep, "fallback")
        .into_iter()
        .filter_map(|n| model_ref(rep, refs, n, "fallback model"))
        .collect();
    let tools = f.texts(rep, "tools");
    for t in &tools {
        refs.push(rep, t.line, RefKind::Tool, &t.value);
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
        instructions: body.to_string(),
    })
}

/// `workflows/<id>.yaml`.
pub(crate) fn workflow(
    rep: &mut Report,
    refs: &mut Refs,
    stem: &str,
    root: &Node,
) -> Option<Workflow> {
    let f = fields(rep, root, "workflow")?;
    f.allow(
        rep,
        &[
            "schema_version",
            "id",
            "name",
            "team",
            "description",
            "triggers",
            "caps",
            "steps",
        ],
    );
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
            ],
        );
        let id = s.req_id(rep, "id");
        let seat = s.req_id(rep, "seat");
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
        if let (Some(id), Some(seat), Some(instruction)) = (id, seat, instruction) {
            steps.push(Step {
                id: id.value,
                seat: seat.value,
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
    f.allow(
        rep,
        &[
            "schema_version",
            "id",
            "name",
            "team",
            "description",
            "key",
            "columns",
            "views",
            "grants",
        ],
    );
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
    f.allow(
        rep,
        &[
            "schema_version",
            "id",
            "name",
            "team",
            "description",
            "cards",
        ],
    );
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

/// `sources/<id>.yaml`: the checks every file gets and `id` = stem; its
/// schema is validated from E1 story 12.
pub(crate) fn source(rep: &mut Report, stem: &str, root: &Node) {
    if let Some(f) = fields(rep, root, "source") {
        id_is_stem(rep, &f, stem, "yaml");
    }
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
