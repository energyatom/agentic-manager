//! The definitions a project's files hold (AD-35), as read into a
//! [`crate::ConfigSnapshot`]. Money is i64 micro-units: prices in USD, budgets
//! and caps in the company currency. Ids are the kebab-case file stem or the
//! `id:` of a list item.

use std::collections::{BTreeMap, BTreeSet};

use am_domain::capability::{AccountKind, Capability};
use am_domain::workspace::Access;

/// `company.yaml`.
#[derive(Debug, Clone, PartialEq)]
pub struct Company {
    /// The project's id (UUIDv7, AD-32), written by the app on the first
    /// open; `None` until then, and always in a template.
    pub project_id: Option<String>,
    pub name: String,
    pub description: Option<String>,
    /// IANA time zone name.
    pub timezone: String,
    /// ISO 4217 code, e.g. `EUR`.
    pub currency: String,
    /// `monday` … `sunday`; the budget week starts here (default `monday`).
    pub week_start: String,
    /// Event-trigger chains stop past this depth (AD-44; default 3, at most 5).
    pub max_chain_depth: u8,
    pub teams: Vec<Team>,
    pub hats: Vec<Hat>,
}

/// A team (department) in `company.yaml`.
#[derive(Debug, Clone, PartialEq)]
pub struct Team {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub parent: Option<String>,
    /// Hand-offs this team accepts, as `<team-id>/<workflow-id>` (AD-41).
    pub accepts: Vec<String>,
}

/// A hat a human wears when acting (AD-24).
#[derive(Debug, Clone, PartialEq)]
pub struct Hat {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

/// An Account of this Mac's user Accounts file (AD-17, AD-31): never a
/// key, only its Keychain reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub id: String,
    /// `anthropic`, `openai`, `openai-compatible` (model); `brave`, `tavily`,
    /// `exa`, `serper`, `searxng-remote` (search); `firecrawl` (crawl service).
    pub provider: String,
    /// Follows from the provider.
    pub kind: AccountKind,
    pub name: Option<String>,
    pub base_url: Option<String>,
    /// The Keychain entry that holds the key.
    pub secret_ref: Option<String>,
}

/// `models/prices.yaml`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Prices {
    /// The date the prices were last checked (`YYYY-MM-DD`).
    pub updated: Option<String>,
    pub models: Vec<ModelPrice>,
    pub services: Vec<ServicePrice>,
    /// Company-currency units per USD (story 3.7, owner Q6), dated by
    /// `updated`.
    pub rates: Vec<CurrencyRate>,
    /// The lowercase sha256 hex of the file's bytes as loaded: the run's
    /// rate-snapshot id. `None` with no file.
    pub sha256: Option<String>,
}

/// One `rates:` entry of `models/prices.yaml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrencyRate {
    /// The ISO 4217 code, never `USD`.
    pub currency: String,
    /// Company micro-units per USD (0.92 is 920,000).
    pub micros_per_usd: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelPrice {
    pub provider: String,
    pub model: String,
    /// USD micro-units per million input tokens.
    pub input_micros_per_mtok: i64,
    /// USD micro-units per million output tokens.
    pub output_micros_per_mtok: i64,
}

/// A paid service's price (AD-9 service spend).
#[derive(Debug, Clone, PartialEq)]
pub struct ServicePrice {
    pub id: String,
    pub unit: String,
    /// USD micro-units per unit.
    pub micros_per_unit: i64,
}

/// `models/capabilities.yaml`: the owner's overrides of shipped capabilities.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Capabilities {
    pub models: Vec<ModelCapability>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ModelCapability {
    pub provider: String,
    pub model: String,
    pub context_window: Option<u64>,
    pub max_output_tokens: Option<u64>,
    pub tools: Option<bool>,
    pub reads_pdf: Option<bool>,
    pub reads_images: Option<bool>,
    pub reads_office: Option<bool>,
    pub reads_audio: Option<bool>,
}

/// `tools.yaml`: built-in tool settings, search and MCP servers. (Its
/// `egress_hosts` list was retired by the schema_version 2 migration, story
/// 1.14: each seat carries its own `allowed_hosts`, AD-48.)
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ToolsConfig {
    /// `builtin` (Local search) or a search Account id.
    pub search_provider: Option<String>,
    pub builtin: Vec<BuiltinToolSetting>,
    pub mcp_servers: Vec<McpServer>,
}

/// A project's setting for one built-in tool.
#[derive(Debug, Clone, PartialEq)]
pub struct BuiltinToolSetting {
    pub id: String,
    pub max_uses_per_step: Option<u32>,
}

/// An MCP server record (AD-26). Its tools are referenced from a seat as
/// `<server-id>/<tool-name>`.
#[derive(Debug, Clone, PartialEq)]
pub struct McpServer {
    pub id: String,
    /// `stdio` or `http`.
    pub transport: String,
    pub command: Option<String>,
    pub args: Vec<String>,
    /// Names of the environment variables the server gets (never values).
    pub env: Vec<String>,
    pub url: Option<String>,
    pub version: Option<String>,
    /// Declared network use, e.g. `none` or a host list in words.
    pub network: Option<String>,
    pub tools: Vec<McpTool>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct McpTool {
    pub name: String,
    /// The action class; MCP tools default to `external-effect` (AD-11).
    pub class: am_domain::tool_manifest::ActionClass,
}

/// `seats/<id>.md`: frontmatter plus the Markdown body, kept byte for byte.
#[derive(Debug, Clone, PartialEq)]
pub struct Seat {
    pub id: String,
    pub name: String,
    pub role: String,
    pub team: String,
    pub description: Option<String>,
    pub reports_to: Option<String>,
    pub model: Option<ModelRef>,
    pub fallback: Vec<ModelRef>,
    /// Built-in tool ids and `<server-id>/<tool-name>` MCP tools, as listed.
    pub tools: Vec<String>,
    pub budget: Option<Budget>,
    /// At most 500 characters (AD-38).
    pub goal: Option<String>,
    /// At most 2,000 characters (AD-38).
    pub backstory: Option<String>,
    pub avatar: Option<Avatar>,
    /// Every capability, with the defaults filled in for those the file does
    /// not mention (AD-46).
    pub capabilities: BTreeMap<Capability, CapabilitySetting>,
    /// Linked folders (AD-30).
    pub folders: Vec<Folder>,
    /// Serve text-readable files as text to a model that cannot read them
    /// natively (AD-30; off by default).
    pub text_fallback: bool,
    /// Attached skill ids (`skills/<id>/`, AD-38).
    pub skills: Vec<String>,
    /// Reference files in the runtime folder's `knowledge/<seat-id>/`.
    pub knowledge: Vec<KnowledgeEntry>,
    /// Saved links.
    pub links: Vec<Link>,
    /// The hosts this seat's tools, readers, link lists and Sources may
    /// reach (AD-48), normalised; empty means none. Never an Account's
    /// endpoint.
    pub allowed_hosts: Vec<am_domain::HostPattern>,
    /// The seat's tool grants: the capability expansion (plus
    /// `read_skill_file` with a skill attached), the listed built-in tools of
    /// no capability and the listed MCP tools. Never read from a skill.
    pub grants: BTreeSet<String>,
    /// The Markdown body after the frontmatter.
    pub instructions: String,
}

/// A seat avatar: white initials on one DESIGN avatar tint.
#[derive(Debug, Clone, PartialEq)]
pub struct Avatar {
    /// `slate`, `steel`, `sage`, `stone`, `plum` or `clay`.
    pub tint: String,
}

/// One capability of a seat.
#[derive(Debug, Clone, PartialEq)]
pub struct CapabilitySetting {
    pub on: bool,
    /// `builtin` or an Account id of the kind the capability takes.
    pub provider: String,
}

/// A linked folder (AD-30): a grant root.
#[derive(Debug, Clone, PartialEq)]
pub struct Folder {
    pub id: String,
    /// As written: project-relative or `~/`-relative.
    pub path: String,
    /// `read` (default) or `read_write`.
    pub access: Access,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
}

/// A reference file of a seat, relative to the runtime folder's
/// `knowledge/<seat-id>/`.
#[derive(Debug, Clone, PartialEq)]
pub struct KnowledgeEntry {
    pub file: String,
    pub note: Option<String>,
}

/// A saved link of a seat.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub title: String,
    /// http or https.
    pub url: String,
    pub group: Option<String>,
}

/// `skills/<id>/`: an Agent Skills folder (AD-38). Instructions only: no
/// field of a skill ever reaches a seat's grants.
#[derive(Debug, Clone, PartialEq)]
pub struct Skill {
    pub id: String,
    /// Equal to `id`.
    pub name: String,
    pub description: String,
    pub license: Option<String>,
    pub metadata: BTreeMap<String, String>,
    /// The SKILL.md `allowed-tools`, kept as "asks for" information only.
    pub asks_for: Vec<String>,
    /// The SKILL.md body (at most 64 KB).
    pub body: String,
    /// `references/<file>` → its text.
    pub references: BTreeMap<String, String>,
}

/// Account › model.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelRef {
    pub account: String,
    pub model: String,
}

/// A seat's budget, in company-currency micro-units.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Budget {
    pub per_run: Option<i64>,
    pub per_week: Option<i64>,
}

/// `workflows/<id>.yaml`.
#[derive(Debug, Clone, PartialEq)]
pub struct Workflow {
    pub id: String,
    pub name: String,
    pub team: String,
    pub description: Option<String>,
    pub triggers: Vec<Trigger>,
    pub caps: Option<Caps>,
    pub steps: Vec<Step>,
}

/// What starts a workflow (AD-18, AD-44).
#[derive(Debug, Clone, PartialEq)]
pub enum Trigger {
    Schedule {
        /// Five-field cron pattern.
        cron: String,
        /// IANA zone; the company's when absent.
        timezone: Option<String>,
        /// `once` (default), `skip` or `all`.
        catch_up: String,
    },
    Event {
        /// `document_updated:<id>`, `step_completed:<workflow>/<step>` or
        /// `workflow_completed:<workflow>`.
        on: String,
        /// `succeeded` (default).
        only_if: String,
    },
}

/// A workflow's caps, in company-currency micro-units (AD-9).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Caps {
    pub per_run: Option<i64>,
    pub scheduled_per_run: Option<i64>,
    pub per_week: Option<i64>,
    pub per_month: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub id: String,
    /// The step's seat; absent on a fan-out step written with `seats:` or
    /// `over_folder:` (AD-40, checked from E3).
    pub seat: Option<String>,
    pub instruction: String,
    pub description: Option<String>,
    /// A `schemas/<id>.json` the step's output must match.
    pub output_schema: Option<String>,
    /// Output tokens per model call in this step.
    pub max_tokens: Option<u32>,
    /// The run-graph keys the step declares (AD-40..AD-42: `seats`, `join`,
    /// `over_folder`, `routes`, `files`, `handoff`, `review`,
    /// `max_parallel`), names only, sorted. Their values are checked from
    /// epic 3 on; a runner that cannot honour one refuses the step by this
    /// list instead of running it as a plain turn.
    pub run_graph: Vec<String>,
    /// Story 3.3 (AD-36): `loop_back: { to, reasons, max }`, a hand-off back
    /// to an earlier step when the step's output lists reasons.
    pub loop_back: Option<LoopBack>,
    /// Story 3.3 (AD-36): `gate: review`, a human gate that makes no model
    /// call. Removing it, or moving its step to a seat with a model, is
    /// privilege-widening (AD-15, AD-42; classified by epic 4 story 17).
    pub gate: Option<Gate>,
    /// Story 3.4 (AD-36; Decision 1): `max_returns` beside `gate: review`,
    /// how many times the owner may return the gate's Review before the
    /// gate fails with a Failure item, 0 to 5; 2 when the gate omits it (and
    /// on a step with no gate, where it is unused and refused if written).
    /// Raising it raises a cap, so it is privilege-widening (epic 4 story
    /// 17 classifies it).
    pub max_returns: u32,
    /// Story 3.3 (AD-11, RULE-11): the step attempt's caps under `limits:`,
    /// with the defaults where a key is absent. Raising one is
    /// privilege-widening (epic 4 story 17).
    pub limits: StepLimits,
    /// Story 3.6 (AD-21, owner Q3): `table:`, the one table the step writes,
    /// of its seat's team; it binds `table_write`, never grants it (only the
    /// Tables capability does, AD-46). Kept on a fan-out step too (which
    /// cannot write a table until story 3.14). Adding or changing it takes
    /// effect at Reload or startup without the two-step confirmation until
    /// epic 4 story 17 classifies it as widening (owner Q3, accepted).
    pub table: Option<String>,
    /// Story 3.21 (AD-29, AD-35, owner Q3 and Q5): `document:`, the one
    /// document the step versions: its final message (Markdown) is stored
    /// as the next version when it succeeds, with a Draft item. A kebab-case
    /// id that belongs to one team (its declaring seats'); not with
    /// `output_schema`, `loop_back` or `gate`, and not on a fan-out step.
    /// Adding or changing it takes effect at Reload or startup without the
    /// two-step confirmation until epic 4 story 17 classifies it as widening
    /// (owner Q3, accepted).
    pub document: Option<String>,
    /// Whether the step fans out (AD-40): it declares `seats:` or
    /// `over_folder:` with a value. Decided once, here, for `agentic
    /// validate` and `agentic run` alike (the ToolGate slice's review round
    /// 1, R8).
    pub fan_out: bool,
    /// A fan-out step's `seats:` (AD-40), the seat ids as written; empty on
    /// a step with one `seat:`. Story 3.6 reads it only to warn that a
    /// fan-out step whose seats hold `table_write` cannot write a table
    /// until story 3.14; 3.14 checks and runs it.
    pub seats: Vec<String>,
}

/// A step's loop-back (AD-36): when its output's `reasons` field lists
/// reasons, the run goes back to the earlier step `to`, at most `max`
/// times.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopBack {
    /// An earlier step of the same workflow.
    pub to: String,
    /// The field of the step's JSON output that holds the reasons
    /// (default `reasons`).
    pub reasons: String,
    /// How many loop-backs this hand-off allows, 0 to 5 (default 2).
    pub max: u32,
}

/// A human gate's kind (AD-36): `review`, the one kind in this version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    Review,
}

/// A step attempt's caps (AD-11, RULE-11). The defaults (8 model turns,
/// 20 tool calls, 10 minutes) are the owner's placeholders (story 3.3,
/// Question 7); a step sets its own under `limits:`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepLimits {
    /// Model turns per attempt, 1 to 50.
    pub iterations: u32,
    /// Tool calls per attempt, 0 to 200.
    pub tool_calls: u32,
    /// Wall-clock time per attempt, 10 s to 2 h.
    pub wall_clock: std::time::Duration,
}

impl Default for StepLimits {
    fn default() -> Self {
        StepLimits {
            iterations: 8,
            tool_calls: 20,
            wall_clock: std::time::Duration::from_secs(600),
        }
    }
}

/// `tables/<id>.yaml` (AD-21, AD-22).
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub id: String,
    pub name: String,
    /// The one owning team.
    pub team: String,
    pub description: Option<String>,
    /// The column whose value identifies a row.
    pub key: Option<String>,
    pub columns: Vec<Column>,
    pub views: Vec<View>,
    /// Teams granted read-only access.
    pub grants: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Column {
    pub id: String,
    /// `text`, `number`, `integer`, `boolean`, `date`, `datetime`, `url`,
    /// `status` or `json`.
    pub column_type: String,
    pub required: bool,
    pub description: Option<String>,
    /// The allowed values of a `status` column.
    pub values: Vec<String>,
    pub transitions: Vec<Transition>,
}

/// An allowed status change (AD-21).
#[derive(Debug, Clone, PartialEq)]
pub struct Transition {
    pub from: String,
    pub to: String,
}

/// A saved view.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub id: String,
    pub name: String,
    pub columns: Vec<String>,
}

/// `dashboards/<id>.yaml`: stored queries and their layout (AD-34).
#[derive(Debug, Clone, PartialEq)]
pub struct Dashboard {
    pub id: String,
    pub name: String,
    pub team: Option<String>,
    pub description: Option<String>,
    pub cards: Vec<Card>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub id: String,
    pub title: String,
    /// One read-only `SELECT` over the `v_*` views.
    pub query: String,
    pub layout: Option<Layout>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// `sources/<id>.yaml`: a configured Connector instance (AD-39).
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    pub id: String,
    pub name: Option<String>,
    pub description: Option<String>,
    /// The connector type: `web-search`, `http-reader`, `crawler`, `api`,
    /// `feed`, `local-repository`, `knowledge`, `database`, `mcp`, `action`.
    pub source_type: String,
    /// Whose allow-list, budget scope and Agent card the Source uses.
    pub owner_seat: String,
    pub account: Option<String>,
    pub scope: SourceScope,
    pub trigger: Option<Trigger>,
    pub limits: SourceLimits,
    pub output: SourceOutput,
    /// Render pages built by scripts (http-reader and crawler only).
    pub render: bool,
    /// The date the owner checked the site's terms (`YYYY-MM-DD`).
    pub terms_checked: Option<String>,
    /// The file's text, byte for byte.
    pub text: String,
}

/// What a Source reads.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SourceScope {
    pub seeds: Vec<String>,
    pub url: Option<String>,
    /// Anchored path globs.
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    /// Scope host patterns (AD-48), normalised through
    /// `am_domain::HostPattern`; each should be covered by the owner seat's
    /// `allowed_hosts`.
    pub allowed_hosts: Vec<String>,
    /// 0 to 10.
    pub max_depth: Option<u32>,
}

/// A Source's limits (AD-39), each within its cap.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SourceLimits {
    /// 1 to 10,000.
    pub max_items_per_load: Option<u32>,
    /// 1 byte to 1 GB (10^9 bytes).
    pub max_bytes_per_load: Option<u64>,
    /// 1 to 96.
    pub max_loads_per_day: Option<u32>,
    /// Company-currency micro-units, from 0.
    pub max_spend_per_load: Option<i64>,
    /// Milliseconds between requests to one host; at least 1,000.
    pub rate_per_host_ms: Option<u64>,
}

/// Where a Source's items go: exactly one form.
#[derive(Debug, Clone, PartialEq)]
pub enum SourceOutput {
    /// Rows of a table, keyed by `key`; `mapping` is column → source path.
    Table {
        table: String,
        key: String,
        mapping: BTreeMap<String, String>,
    },
    /// Untrusted source records.
    Records,
    /// The owner seat's Knowledge (`seat`) or its team's (`team`).
    Knowledge { scope: String },
}
