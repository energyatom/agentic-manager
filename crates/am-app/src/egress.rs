//! The egress client's wiring (story 2.2, AD-12, AD-15, AD-25, AD-48).
//!
//! - [`ProjectDirectory`]: the `EgressDirectory` of a project, read from its
//!   config and, read-only, its store. It answers what a seat, an Account
//!   or an MCP server may reach, and nothing for an id the project does not
//!   configure. An Account is answered only while it is confirmed (AD-15
//!   Key use, owner decision 8): `models/accounts.yaml` holds exactly the
//!   bytes of its latest History entry, and the latest `account_added`
//!   note of the Account (written by its config intent) names the same
//!   provider, scheme, host and port ([`confirmed`]). So every path that
//!   sends a key gets the check.
//!   The reserved local Accounts are always answered (AD-31). A project
//!   never opened on this Mac (no project id, no runtime folder, no
//!   database) has no confirmation record: every Account of it is refused,
//!   with the reason, while seats and the reserved Accounts still answer.
//! - [`StoreAuditSink`]: writes each `egress.blocked` entry (and nothing
//!   else) through the store's writer.
//! - The confirmation reads only notes a committed config intent wrote
//!   (`Reader::config_note_args`), and only the owner's (`Actor::Human`)
//!   on an intent of `models/accounts.yaml`. The directory also reads the
//!   ports every running `agentic serve` records (and the one the project's
//!   runtime folder records), and answers no Account on them (the client
//!   refuses them to every Account and MCP server).
//! - [`LocalAccountsOnly`]: a directory answering the reserved local
//!   Accounts only (the spike's `local-context` command).

use std::collections::BTreeMap;
use std::path::Path;

use am_config::ACCOUNTS_FILE;
use am_domain::net::{EgressDirectory, Endpoint, reserved_local_account};
use am_domain::{AuditEvent, AuditSink, HostPattern};
use am_store::{WriteCommand, Writer};

use crate::accounts::{self, AccountEndpoint};
use crate::runtime::Places;

/// A project's egress directory, built once from its config and store.
pub struct ProjectDirectory {
    seats: BTreeMap<String, Vec<HostPattern>>,
    mcp: BTreeMap<String, Endpoint>,
    /// Every Account of the file: its endpoint while confirmed, or why not.
    accounts: BTreeMap<String, Result<Endpoint, String>>,
    /// The ports the app's own servers listen on.
    own_ports: Vec<u16>,
}

impl ProjectDirectory {
    /// Reads the project at `root`: its config, and its store read-only
    /// (no lock, no edit, no migration). `Err` only when there is no
    /// project folder; anything else that stops an answer refuses that
    /// answer instead, in words.
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
        let file = am_config::accounts_file(root);
        let mut answers = BTreeMap::new();
        match &file.problem {
            Some(problem) => {
                for id in file.ids.iter().flatten() {
                    answers.insert(
                        id.clone(),
                        Err(format!(
                            "{ACCOUNTS_FILE} cannot be read as Accounts because {problem}"
                        )),
                    );
                }
            }
            None => {
                let records = Records::open(places, root, &file);
                for account in &file.accounts {
                    if accounts::reserved(&account.id) {
                        continue;
                    }
                    let answer = account_answer(&file, account, &records);
                    answers.insert(account.id.clone(), answer);
                }
            }
        }
        // The app's own ports: every running `agentic serve`'s (the
        // user-level records), and the one this project's runtime folder
        // records. No Account on any of them.
        let mut own_ports = crate::runtime::serving_ports(&places.app_data);
        if let Some(port) = crate::runtime::find_runtime(places, root)
            .ok()
            .and_then(|layout| std::fs::read_to_string(layout.serve_port()).ok())
            .and_then(|text| text.trim().parse::<u16>().ok())
            && !own_ports.contains(&port)
        {
            own_ports.push(port);
        }
        for answer in answers.values_mut() {
            if let Ok(endpoint) = answer
                && endpoint.host == "127.0.0.1"
                && own_ports.contains(&endpoint.port)
            {
                *answer = Err(own_server_words(endpoint.port));
            }
        }
        Ok(ProjectDirectory {
            seats,
            mcp,
            accounts: answers,
            own_ports,
        })
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
        self.accounts.get(account)?.as_ref().ok().cloned()
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
        match self.accounts.get(account) {
            Some(Ok(_)) => None,
            Some(Err(why)) => Some(why.clone()),
            None => Some(format!(
                "there is no Account '{account}' in {ACCOUNTS_FILE}"
            )),
        }
    }
}

/// Why an Account at `127.0.0.1:<port>` is not called while one of the
/// app's own servers listens there.
pub(crate) fn own_server_words(port: u16) -> String {
    format!("the app's own server is listening on port {port}")
}

/// One Account's answer: its endpoint while confirmed, or why not.
fn account_answer(
    file: &am_config::AccountsFile,
    account: &am_config::Account,
    records: &Result<Records, String>,
) -> Result<Endpoint, String> {
    let id = &account.id;
    let errors = file.entry_errors(id);
    if !errors.is_empty() {
        return Err(format!(
            "its entry in {ACCOUNTS_FILE} has an error ({})",
            errors.join("; ")
        ));
    }
    if !accounts::is_model_provider(&account.provider) {
        return Err(format!(
            "a {} Account is not a model Account, and only model Accounts are called in this version",
            account.provider
        ));
    }
    let endpoint =
        accounts::endpoint_of(&account.provider, account.base_url.as_deref(), "base_url")?;
    let records = records.as_ref().map_err(Clone::clone)?;
    records.check(account, &endpoint)?;
    Ok(endpoint.to_domain())
}

/// The confirmation records of a project: its store, read-only, once the
/// Accounts file was found to hold exactly the bytes the app last wrote.
struct Records {
    reader: am_store::Reader,
}

impl Records {
    /// Read-only and without the project's lock, as `agentic status`
    /// reads: the runtime folder is found, never made, and the database is
    /// never migrated. No runtime folder, no database, an older database
    /// and no History entry of the file each mean "not confirmed".
    fn open(
        places: &Places,
        project: &Path,
        file: &am_config::AccountsFile,
    ) -> Result<Records, String> {
        let layout = crate::runtime::find_runtime(places, project).map_err(|e| e.to_string())?;
        let db = layout.state_db();
        if !db.is_file() {
            return Err("this project has no database on this Mac yet".into());
        }
        am_store::check_database(&db).map_err(|e| e.to_string())?;
        let reader = am_store::Reader::open(&db).map_err(|e| e.to_string())?;
        let found = reader.schema_version().map_err(|e| e.to_string())?;
        if found < am_store::SCHEMA_VERSION {
            return Err(format!(
                "the project's database is at version {found}, older than this app's version {}, and it is never migrated here",
                am_store::SCHEMA_VERSION
            ));
        }
        let history = reader
            .config_history(Some(ACCOUNTS_FILE))
            .map_err(|e| e.to_string())?;
        let Some(last) = history.last() else {
            return Err(format!(
                "the app has never written {ACCOUNTS_FILE} in this project"
            ));
        };
        if file.sha256.as_deref() != Some(last.after_sha256.as_str()) {
            return Err(format!(
                "{ACCOUNTS_FILE} was changed on disk outside the app since the app last wrote it"
            ));
        }
        Ok(Records { reader })
    }

    /// The latest `account_added` note of `account` names its provider,
    /// scheme, host and port.
    fn check(
        &self,
        account: &am_config::Account,
        endpoint: &AccountEndpoint,
    ) -> Result<(), String> {
        let notes = self
            .reader
            .config_note_args("config.widened", &account.id)
            .map_err(|e| e.to_string())?;
        // Only the owner's note on an intent of the Accounts file confirms.
        let Some(note) = notes
            .iter()
            .filter(|n| matches!(n.actor, am_domain::Actor::Human { .. }))
            .filter(|n| n.file == ACCOUNTS_FILE)
            .map(|n| &n.args)
            .rfind(|args| args.get("kind").map(String::as_str) == Some("account_added"))
        else {
            return Err(format!(
                "no `agentic accounts add` of '{}' is recorded for this project",
                account.id
            ));
        };
        let recorded = |key: &str| note.get(key).map(String::as_str);
        let port = endpoint.port.to_string();
        if recorded("provider") != Some(account.provider.as_str())
            || recorded("scheme") != Some(endpoint.scheme)
            || recorded("host") != Some(endpoint.host.as_str())
            || recorded("port") != Some(port.as_str())
        {
            return Err(format!(
                "the last `agentic accounts add` of '{}' recorded provider {} at {}://{}:{}, and {ACCOUNTS_FILE} now holds provider {} at {}://{}:{port}",
                account.id,
                recorded("provider").unwrap_or("(none)"),
                recorded("scheme").unwrap_or("?"),
                recorded("host").unwrap_or("?"),
                recorded("port").unwrap_or("?"),
                account.provider,
                endpoint.scheme,
                endpoint.host,
            ));
        }
        Ok(())
    }
}

/// Whether `account` is one the owner confirmed with `agentic accounts
/// add` on this Mac, and nothing changed since (AD-15, AD-16, AD-48): the
/// check [`ProjectDirectory`] makes, for `agentic accounts test`'s own
/// wording before any Keychain read. `file` is the read the endpoint was
/// derived from, so the bytes checked are the bytes used.
pub(crate) fn confirmed(
    places: &Places,
    project: &Path,
    file: &am_config::AccountsFile,
    account: &am_config::Account,
    endpoint: &AccountEndpoint,
) -> Result<(), String> {
    Records::open(places, project, file)?.check(account, endpoint)
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
