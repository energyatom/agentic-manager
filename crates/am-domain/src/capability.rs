//! The capability table (AD-46): the nine capabilities a seat switches on,
//! the built-in tools each one grants, which provider each one takes, and
//! the Account kind each Account provider belongs to. It is data, read by the
//! config loader to expand a seat's capabilities into grants, and later by
//! ToolGate and the UI. A skill never appears here (AD-38).

use std::collections::BTreeSet;

/// One of the nine capabilities of a seat (AD-46).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Capability {
    WebSearch,
    Browse,
    Crawl,
    FolderAccess,
    Files,
    Tables,
    Memory,
    HandoffAsk,
    Calculate,
}

/// The kind of an Account, which follows from its provider (owner decision
/// 2026-11-20).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AccountKind {
    /// A model provider: `anthropic`, `openai`, `openai-compatible`.
    Model,
    /// A search provider: `brave`, `tavily`, `exa`, `serper`, `searxng-remote`.
    Search,
    /// A Firecrawl-compatible crawl service: `firecrawl`.
    CrawlService,
}

/// The config key that `folder_access` had before 2026-11-19; the loader
/// reads it as `folder_access`.
pub const LEGACY_FOLDER_ACCESS_KEY: &str = "documents";
/// The provider value meaning "the built-in, free provider".
pub const BUILTIN_PROVIDER: &str = "builtin";
/// Granted whenever a seat has at least one skill attached (AD-38).
pub const SKILL_TOOL: &str = "read_skill_file";
/// Granted by folder access only while a linked folder is `read_write` (AD-30).
pub const FOLDER_WRITE_TOOL: &str = "write_file";

use Capability::*;

/// One row of the capability table.
struct Row {
    capability: Capability,
    /// The config id.
    id: &'static str,
    /// The tools it grants when on (`write_file` is added separately, see
    /// [`expand`]).
    grants: &'static [&'static str],
    default_on: bool,
    /// The Account kind its provider may name; `None`: built-in only.
    provider: Option<AccountKind>,
}

/// The table, in AD-46 order.
const TABLE: &[Row] = &[
    Row {
        capability: WebSearch,
        id: "web_search",
        grants: &["web_search"],
        default_on: false,
        provider: Some(AccountKind::Search),
    },
    Row {
        capability: Browse,
        id: "browse",
        grants: &["read_website", "read_url", "arxiv"],
        default_on: false,
        provider: Some(AccountKind::CrawlService),
    },
    Row {
        capability: Crawl,
        id: "crawl",
        grants: &[],
        default_on: false,
        provider: Some(AccountKind::CrawlService),
    },
    Row {
        capability: FolderAccess,
        id: "folder_access",
        grants: &["list_files", "read_file", "search_knowledge"],
        default_on: true,
        provider: None,
    },
    Row {
        capability: Files,
        id: "files",
        grants: &["workspace_read", "workspace_write"],
        default_on: false,
        provider: None,
    },
    Row {
        capability: Tables,
        id: "tables",
        grants: &["table_read", "table_write"],
        default_on: false,
        provider: None,
    },
    Row {
        capability: Memory,
        id: "memory",
        grants: &["remember"],
        default_on: false,
        provider: None,
    },
    Row {
        capability: HandoffAsk,
        id: "handoff_ask",
        grants: &["ask_seat"],
        default_on: false,
        provider: None,
    },
    Row {
        capability: Calculate,
        id: "calculate",
        grants: &["calculate", "stats", "query_table"],
        default_on: false,
        provider: None,
    },
];

/// Account providers and their kinds.
const PROVIDERS: &[(&str, AccountKind)] = &[
    ("anthropic", AccountKind::Model),
    ("openai", AccountKind::Model),
    ("openai-compatible", AccountKind::Model),
    ("brave", AccountKind::Search),
    ("tavily", AccountKind::Search),
    ("exa", AccountKind::Search),
    ("serper", AccountKind::Search),
    ("searxng-remote", AccountKind::Search),
    ("firecrawl", AccountKind::CrawlService),
];

/// Providers whose Account must give a `base_url`.
const NEEDS_BASE_URL: &[&str] = &["openai-compatible", "searxng-remote", "firecrawl"];

fn row(c: Capability) -> &'static Row {
    TABLE
        .iter()
        .find(|r| r.capability == c)
        .unwrap_or_else(|| unreachable!("every capability has a row"))
}

impl Capability {
    /// Every capability, in table order.
    pub fn all() -> Vec<Capability> {
        TABLE.iter().map(|r| r.capability).collect()
    }

    /// The config id, e.g. `web_search`.
    pub fn id(self) -> &'static str {
        row(self).id
    }

    /// The capability with this config id (the legacy `documents` key is
    /// not accepted here; the loader maps it).
    pub fn parse(id: &str) -> Option<Capability> {
        TABLE.iter().find(|r| r.id == id).map(|r| r.capability)
    }

    /// On when a seat file does not mention it: `folder_access` only.
    pub fn default_on(self) -> bool {
        row(self).default_on
    }

    /// The built-in tools this capability grants when on (folder access's
    /// `write_file` is extra, see [`expand`]).
    pub fn grants(self) -> Vec<String> {
        row(self).grants.iter().map(|s| s.to_string()).collect()
    }

    /// The Account kind its `provider` may name; `None` when it offers the
    /// built-in provider only.
    pub fn provider_kind(self) -> Option<AccountKind> {
        row(self).provider
    }
}

impl AccountKind {
    /// The kind an Account provider belongs to; `None` for an unknown one.
    pub fn of_provider(provider: &str) -> Option<AccountKind> {
        PROVIDERS
            .iter()
            .find(|(p, _)| *p == provider)
            .map(|(_, k)| *k)
    }

    /// As written in messages: `model`, `search`, `crawl-service`.
    pub fn as_str(self) -> &'static str {
        match self {
            AccountKind::Model => "model",
            AccountKind::Search => "search",
            AccountKind::CrawlService => "crawl-service",
        }
    }

    /// The providers of this kind, in table order.
    pub fn providers(self) -> Vec<String> {
        PROVIDERS
            .iter()
            .filter(|(_, k)| *k == self)
            .map(|(p, _)| p.to_string())
            .collect()
    }
}

/// Every Account provider, in table order.
pub fn account_providers() -> Vec<String> {
    PROVIDERS.iter().map(|(p, _)| p.to_string()).collect()
}

/// True when an Account of this provider must give a `base_url`.
pub fn provider_needs_base_url(provider: &str) -> bool {
    NEEDS_BASE_URL.contains(&provider)
}

/// The capability that grants this built-in tool, if any (`write_file`
/// belongs to folder access; `read_skill_file` and the tools of no
/// capability return `None`).
pub fn capability_of_tool(tool: &str) -> Option<Capability> {
    if tool == FOLDER_WRITE_TOOL {
        return Some(FolderAccess);
    }
    TABLE
        .iter()
        .find(|r| r.grants.contains(&tool))
        .map(|r| r.capability)
}

/// The built-in tools granted by the capabilities that are on: each one's
/// tools, plus `write_file` when folder access is on and a linked folder is
/// `read_write`. A skill's `read_skill_file` and the tools listed by name are
/// added by the caller.
pub fn expand(on: &[Capability], read_write_folder: bool) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for c in on {
        out.extend(c.grants());
        if *c == FolderAccess && read_write_folder {
            out.insert(FOLDER_WRITE_TOOL.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_manifest::builtin_tool;

    #[test]
    fn nine_capabilities_with_unique_ids() {
        let ids: BTreeSet<&str> = Capability::all().iter().map(|c| c.id()).collect();
        assert_eq!(ids.len(), 9);
        for c in Capability::all() {
            assert_eq!(Capability::parse(c.id()), Some(c));
        }
        assert_eq!(Capability::parse(LEGACY_FOLDER_ACCESS_KEY), None);
    }

    #[test]
    fn only_folder_access_is_on_by_default() {
        let on: Vec<Capability> = Capability::all()
            .into_iter()
            .filter(|c| c.default_on())
            .collect();
        assert_eq!(on, [FolderAccess]);
    }

    #[test]
    fn every_granted_tool_is_a_built_in_tool() {
        for c in Capability::all() {
            for t in c.grants() {
                assert!(builtin_tool(&t).is_some(), "{t}");
                assert_eq!(capability_of_tool(&t), Some(c), "{t}");
            }
        }
        assert!(builtin_tool(FOLDER_WRITE_TOOL).is_some());
        assert!(builtin_tool(SKILL_TOOL).is_some());
        assert_eq!(capability_of_tool(SKILL_TOOL), None);
        assert_eq!(capability_of_tool("write_document"), None);
    }

    #[test]
    fn expansion_adds_write_file_only_for_a_read_write_folder() {
        let read = expand(&[FolderAccess], false);
        assert!(!read.contains(FOLDER_WRITE_TOOL));
        let write = expand(&[FolderAccess], true);
        assert!(write.contains(FOLDER_WRITE_TOOL));
        assert!(expand(&[Tables], true).len() == 2);
        assert!(expand(&[Crawl], false).is_empty());
    }

    #[test]
    fn providers_have_kinds() {
        assert_eq!(
            AccountKind::of_provider("tavily"),
            Some(AccountKind::Search)
        );
        assert_eq!(
            AccountKind::of_provider("firecrawl"),
            Some(AccountKind::CrawlService)
        );
        assert_eq!(AccountKind::of_provider("openai"), Some(AccountKind::Model));
        assert_eq!(AccountKind::of_provider("ftp"), None);
        assert_eq!(WebSearch.provider_kind(), Some(AccountKind::Search));
        assert_eq!(Browse.provider_kind(), Some(AccountKind::CrawlService));
        assert_eq!(Crawl.provider_kind(), Some(AccountKind::CrawlService));
        assert_eq!(Tables.provider_kind(), None);
        assert!(provider_needs_base_url("firecrawl"));
        assert!(provider_needs_base_url("searxng-remote"));
        assert!(!provider_needs_base_url("brave"));
    }
}
