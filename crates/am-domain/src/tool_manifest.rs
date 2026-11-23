//! The built-in tool table (AD-11, AD-46): every built-in tool's id, action
//! class, network needs and default `max_uses_per_step`, as data. The config
//! validator resolves a seat's `tools:` against it; ToolGate adopts it later.
//! MCP tools are not here: they come from a project's `tools.yaml` (AD-26).
//! Story 3.3 adds the [`ToolRunner`] port, which no shipped code implements.

use std::sync::Arc;

use crate::content::UntrustedData;
use crate::llm::ToolCall;
use crate::{BoxFuture, CancelSignal};

/// The port that runs a tool call (AD-11; story 3.3, owner condition 2).
/// am-engine calls it only after ToolGate allowed the call, and only when a
/// runner was handed to it: the shipped binary hands none and registers no
/// tool (am-testkit's `FixtureTool` is the one runner, in tests). A result
/// is always `UntrustedData` (AD-10), so it taints the step that reads it;
/// an `Err` is the tool's failure in words.
pub trait ToolRunner: Send + Sync {
    /// The manifests of the tools this runner runs.
    fn manifests(&self) -> Vec<ToolManifest>;
    /// Runs one call; `cancel` ends it (a cap, Stop).
    fn run(
        &self,
        call: ToolCall,
        cancel: Arc<dyn CancelSignal>,
    ) -> BoxFuture<Result<UntrustedData, String>>;
}

/// The action class a tool's manifest declares (AD-11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionClass {
    Read,
    LocalWrite,
    ExternalEffect,
}

impl ActionClass {
    /// The class as written in config files: `read`, `local-write`,
    /// `external-effect`.
    pub fn as_str(self) -> &'static str {
        match self {
            ActionClass::Read => "read",
            ActionClass::LocalWrite => "local-write",
            ActionClass::ExternalEffect => "external-effect",
        }
    }

    /// Parses the config spelling.
    pub fn parse(s: &str) -> Option<ActionClass> {
        match s {
            "read" => Some(ActionClass::Read),
            "local-write" => Some(ActionClass::LocalWrite),
            "external-effect" => Some(ActionClass::ExternalEffect),
            _ => None,
        }
    }
}

/// What a built-in tool needs from the network (AD-12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkNeed {
    /// Local only: no request leaves the machine.
    None,
    /// The seat's search provider: Local search or a chosen search Account.
    SearchProvider,
    /// Hosts on the seat's allow-list, through `Fetcher`.
    AllowListedHosts,
}

/// One built-in tool's manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolManifest {
    pub id: String,
    pub class: ActionClass,
    pub network: NetworkNeed,
    /// How many times one step may call it; overridable per seat (AD-11).
    pub max_uses_per_step: u32,
}

use ActionClass::{LocalWrite, Read};
use NetworkNeed::{AllowListedHosts, None as Local, SearchProvider};

/// The table, in AD-11 order: read tools, then local-write tools. v1 has no
/// external-effect built-in (no send tools).
const BUILTINS: &[(&str, ActionClass, NetworkNeed, u32)] = &[
    ("web_search", Read, SearchProvider, 10),
    ("read_url", Read, AllowListedHosts, 20),
    ("read_website", Read, AllowListedHosts, 20),
    ("read_file", Read, Local, 50),
    ("arxiv", Read, AllowListedHosts, 10),
    ("read_skill_file", Read, Local, 20),
    ("search_knowledge", Read, Local, 20),
    ("table_read", Read, Local, 20),
    ("list_files", Read, Local, 20),
    ("workspace_read", Read, Local, 50),
    ("calculate", Read, Local, 50),
    ("stats", Read, Local, 50),
    ("query_table", Read, Local, 20),
    ("table_write", LocalWrite, Local, 100),
    ("write_document", LocalWrite, Local, 5),
    ("draft_message", LocalWrite, Local, 5),
    ("propose_change", LocalWrite, Local, 5),
    ("write_file", LocalWrite, Local, 20),
    ("workspace_write", LocalWrite, Local, 50),
    ("remember", LocalWrite, Local, 5),
    ("ask_seat", LocalWrite, Local, 3),
];

fn manifest(row: &(&str, ActionClass, NetworkNeed, u32)) -> ToolManifest {
    ToolManifest {
        id: row.0.to_string(),
        class: row.1,
        network: row.2,
        max_uses_per_step: row.3,
    }
}

/// Every built-in tool, in table order.
pub fn builtin_tools() -> Vec<ToolManifest> {
    BUILTINS.iter().map(manifest).collect()
}

/// The built-in tool with this id, if there is one.
pub fn builtin_tool(id: &str) -> Option<ToolManifest> {
    BUILTINS.iter().find(|row| row.0 == id).map(manifest)
}

/// Every built-in tool id, in table order.
pub fn builtin_tool_ids() -> Vec<String> {
    BUILTINS.iter().map(|row| row.0.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_snake_case() {
        let ids = builtin_tool_ids();
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len());
        assert!(
            ids.iter()
                .all(|id| id.chars().all(|c| c.is_ascii_lowercase() || c == '_'))
        );
    }

    #[test]
    fn classes_follow_ad_11() {
        for id in ["web_search", "read_file", "query_table", "list_files"] {
            assert_eq!(builtin_tool(id).unwrap().class, ActionClass::Read, "{id}");
        }
        for id in ["table_write", "write_file", "remember", "ask_seat"] {
            assert_eq!(
                builtin_tool(id).unwrap().class,
                ActionClass::LocalWrite,
                "{id}"
            );
        }
        assert!(
            builtin_tools()
                .iter()
                .all(|t| t.class != ActionClass::ExternalEffect)
        );
        assert_eq!(
            builtin_tool("web_search").unwrap().network,
            NetworkNeed::SearchProvider
        );
        assert_eq!(
            builtin_tool("calculate").unwrap().network,
            NetworkNeed::None
        );
    }

    #[test]
    fn class_spelling_round_trips() {
        for c in [
            ActionClass::Read,
            ActionClass::LocalWrite,
            ActionClass::ExternalEffect,
        ] {
            assert_eq!(ActionClass::parse(c.as_str()), Some(c));
        }
        assert_eq!(ActionClass::parse("write"), None);
    }
}
