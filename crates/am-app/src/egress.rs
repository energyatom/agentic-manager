//! The egress client's wiring (story 2.2, AD-12, AD-15, AD-25, AD-48).
//!
//! - [`ProjectDirectory`]: the `EgressDirectory` of a project, read from its
//!   config and this Mac's user Accounts (story 2.12, AD-17). It answers what
//!   a seat, an Account or an MCP server may reach, and nothing for an id
//!   neither configures. An Account is resolved from the user list and
//!   answered only while [`crate::key_use`] passes it (AD-15 Key use), so
//!   every path that sends a key gets the check; an id missing on this Mac
//!   is refused like an unknown id, saying how to add it. The reserved local
//!   Accounts are always answered (AD-31). [`ProjectDirectory::for_accounts`]
//!   is the same directory with no project (`agentic accounts test`).
//! - [`StoreAuditSink`]: writes each `egress.blocked` entry (and nothing
//!   else) through the store's writer.
//! - The directory also reads the ports every running `agentic serve`
//!   records (and the ones the runtime folders record), and answers no
//!   Account on them (the client refuses them to every Account and MCP
//!   server).
//! - [`LocalAccountsOnly`]: a directory answering the reserved local
//!   Accounts only (the spike's `local-context` command).

use std::collections::BTreeMap;
use std::path::Path;

use am_domain::net::{AccountStamp, EgressDirectory, Endpoint, reserved_local_account};
use am_domain::{AuditEvent, AuditSink, HostPattern};
use am_store::{WriteCommand, Writer};

use crate::accounts;
use crate::runtime::Places;

/// A project's egress directory, built once from its config and this Mac's
/// user Accounts.
pub struct ProjectDirectory {
    seats: BTreeMap<String, Vec<HostPattern>>,
    mcp: BTreeMap<String, Endpoint>,
    /// Where this Mac's Accounts are: each Account is checked when it is
    /// asked for, on every answer (AD-15 *Key use*), never from a map.
    places: Places,
    /// The ports the app's own servers listen on.
    own_ports: Vec<u16>,
}

impl ProjectDirectory {
    /// Reads the project at `root`: its config, and this Mac's user
    /// Accounts and user store read-only (no lock, no edit, no migration).
    /// `Err` only when there is no project folder; anything else that stops
    /// an answer refuses that answer instead, in words.
    pub fn open(root: &Path, places: &Places) -> Result<ProjectDirectory, String> {
        if !root.is_dir() {
            return Err(format!(
                "the project folder {} does not exist",
                root.display()
            ));
        }
        let mut seats = BTreeMap::new();
        let mut mcp = BTreeMap::new();
        if let Ok(am_config::Loaded {
            snapshot: Some(snapshot),
            ..
        }) = am_config::load(root)
        {
            for id in snapshot.seats().keys() {
                seats.insert(id.clone(), snapshot.allowed_hosts(id).to_vec());
            }
            for server in &snapshot.tools().mcp_servers {
                if server.transport != "http" {
                    continue;
                }
                if let Some(endpoint) = server
                    .url
                    .as_deref()
                    .and_then(|url| am_net::url_endpoint(url).ok())
                {
                    mcp.insert(server.id.clone(), endpoint);
                }
            }
        }
        let mut directory = ProjectDirectory::for_accounts(places);
        // The port this project's runtime folder records, when it has one.
        if let Some(port) = crate::runtime::find_runtime(places, root)
            .ok()
            .and_then(|layout| std::fs::read_to_string(layout.serve_port()).ok())
            .and_then(|text| text.trim().parse::<u16>().ok())
            && !directory.own_ports.contains(&port)
        {
            directory.own_ports.push(port);
        }
        directory.seats = seats;
        directory.mcp = mcp;
        Ok(directory)
    }

    /// The directory of this Mac's Accounts alone: no seat and no MCP server
    /// (`agentic accounts test`). Its own ports are every running `agentic
    /// serve`'s and every runtime folder's recorded port.
    pub fn for_accounts(places: &Places) -> ProjectDirectory {
        let mut own_ports = crate::runtime::serving_ports(&places.app_data);
        for record in crate::runtime::records(&places.base) {
            if let Some(port) =
                std::fs::read_to_string(places.base.project(&record.project_id).serve_port())
                    .ok()
                    .and_then(|text| text.trim().parse::<u16>().ok())
                && !own_ports.contains(&port)
            {
                own_ports.push(port);
            }
        }
        ProjectDirectory {
            seats: BTreeMap::new(),
            mcp: BTreeMap::new(),
            places: places.clone(),
            own_ports,
        }
    }

    /// The Account's answer now: its endpoint, stamp and whether it has a
    /// key (story 2.10) while the key-use check passes and it is not on one
    /// of the app's own ports, or why not.
    fn answer(&self, account: &str) -> Result<(Endpoint, String, bool), String> {
        if accounts::reserved(account) {
            return Err(format!("'{account}' is a reserved local Account"));
        }
        let confirmed = crate::key_use::check(&self.places, account)
            .map_err(|refused| refused.words(account))?;
        let endpoint = confirmed.endpoint();
        if endpoint.host == "127.0.0.1" && self.own_ports.contains(&endpoint.port) {
            return Err(own_server_words(endpoint.port));
        }
        let keyed = confirmed.key == am_config::KeyState::Stored;
        Ok((endpoint, confirmed.stamp.to_string(), keyed))
    }
}

impl EgressDirectory for ProjectDirectory {
    fn seat_hosts(&self, seat: &str) -> Option<Vec<HostPattern>> {
        self.seats.get(seat).cloned()
    }

    fn account_endpoint(&self, account: &str) -> Option<Endpoint> {
        if let Some(endpoint) = reserved_local_account(account) {
            return Some(endpoint);
        }
        self.answer(account).ok().map(|(endpoint, _, _)| endpoint)
    }

    fn account_stamp(&self, account: &str) -> Option<AccountStamp> {
        if let Some(endpoint) = reserved_local_account(account) {
            // Unchanged by story 2.10 (owner Q8): the reserved local
            // Accounts are answered as before.
            return Some(AccountStamp {
                endpoint,
                version: String::new(),
                keyed: true,
            });
        }
        self.answer(account)
            .ok()
            .map(|(endpoint, version, keyed)| AccountStamp {
                endpoint,
                version,
                keyed,
            })
    }

    fn mcp_endpoint(&self, server: &str) -> Option<Endpoint> {
        self.mcp.get(server).cloned()
    }

    fn own_ports(&self) -> Vec<u16> {
        self.own_ports.clone()
    }

    fn account_refusal(&self, account: &str) -> Option<String> {
        if reserved_local_account(account).is_some() {
            return None;
        }
        self.answer(account).err()
    }
}

/// Why an Account at `127.0.0.1:<port>` is not called while one of the
/// app's own servers listens there.
pub(crate) fn own_server_words(port: u16) -> String {
    format!("the app's own server is listening on port {port}")
}

/// Writes each audit event through the store's writer (AD-25): the
/// production sink of the egress client.
pub struct StoreAuditSink {
    writer: Writer,
}

impl StoreAuditSink {
    pub fn new(writer: Writer) -> StoreAuditSink {
        StoreAuditSink { writer }
    }
}

impl AuditSink for StoreAuditSink {
    /// Writes an `egress.blocked` entry; any other event is not written (a
    /// holder of this sink can never write, say, a `config.*` note).
    fn record(&self, event: AuditEvent) {
        if event.action != "egress.blocked" {
            tracing::warn!(
                "the egress audit sink writes only egress.blocked, not {}",
                event.action
            );
            return;
        }
        let AuditEvent {
            actor,
            action,
            target,
            outcome,
            hash,
            args,
        } = event;
        if let Err(e) = self.writer.exec(
            actor,
            WriteCommand::Audit {
                action: action.clone(),
                target,
                outcome,
                hash,
                args,
            },
        ) {
            tracing::warn!("the audit entry {action} could not be written: {e}");
        }
    }
}

/// A directory that answers the reserved local Accounts only: no seat, no
/// other Account, no MCP server.
pub struct LocalAccountsOnly;

impl EgressDirectory for LocalAccountsOnly {
    fn seat_hosts(&self, _seat: &str) -> Option<Vec<HostPattern>> {
        None
    }
    fn account_endpoint(&self, account: &str) -> Option<Endpoint> {
        reserved_local_account(account)
    }
    fn mcp_endpoint(&self, _server: &str) -> Option<Endpoint> {
        None
    }
}
