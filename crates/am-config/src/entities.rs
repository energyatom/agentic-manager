//! The definitions a project's files hold (AD-35), as read into a
//! [`crate::ConfigSnapshot`]. Money is i64 micro-units: prices in USD, budgets
//! and caps in the company currency. Ids are the kebab-case file stem or the
//! `id:` of a list item.

/// `company.yaml`.
#[derive(Debug, Clone, PartialEq)]
pub struct Company {
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

/// An Account in `models/accounts.yaml` (AD-31): never a key, only its
/// Keychain reference.
#[derive(Debug, Clone, PartialEq)]
pub struct Account {
    pub id: String,
    /// `anthropic`, `openai` or `openai-compatible`.
    pub provider: String,
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

/// `tools.yaml`: built-in tool settings, search, MCP servers, egress hosts.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ToolsConfig {
    pub egress_hosts: Vec<String>,
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
    /// Built-in tool ids and `<server-id>/<tool-name>` MCP tools.
    pub tools: Vec<String>,
    pub budget: Option<Budget>,
    /// The Markdown body after the frontmatter.
    pub instructions: String,
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
    pub seat: String,
    pub instruction: String,
    pub description: Option<String>,
    /// A `schemas/<id>.json` the step's output must match.
    pub output_schema: Option<String>,
    /// Output tokens per model call in this step.
    pub max_tokens: Option<u32>,
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

/// `sources/<id>.yaml`: loaded and hashed only in this release; its schema
/// is validated from E1 story 12.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceFile {
    pub id: String,
    pub text: String,
}
