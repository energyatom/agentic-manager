//! `agentic accounts change <id> [--provider P] [--base-url U]` (story
//! 2.14; AD-15 *Change*, *Terminal*, *Open*, *Record*, *Key use*): the
//! CLI's only way to confirm or re-confirm an Account on this Mac.
//!
//! Before any prompt, holding no lock, in this order (the first that fails
//! is what the owner reads; nothing is read from the terminal, written or
//! stored before all passed): the terminal; the provider (a model provider
//! only); the user store and an outside change of the user file, naming
//! it; the id (not reserved, in the file, its entry without error, its key
//! under its own id); the new endpoint (2.3's rules, never the app's own
//! port); the scan (no reachable project enables a child that runs as the
//! owner, loopback-HTTP MCP servers included); and, before epic 4 story 18,
//! no move from a loopback to a remote endpoint or to another host while a
//! reachable project names the Account.
//!
//! The path follows from the Account's live confirmation record (its latest
//! `account_added` or `account_changed`, with no `account_removed` or
//! `account_listed` after it):
//!
//! - **Address change** (a live record, the same provider): the key stored
//!   under the id goes to the new endpoint. No Keychain call. The file is
//!   written with its History entry and `account_changed` in one user-store
//!   transaction (an entry whose before equals its after when the file
//!   already holds the values).
//! - **Provider change or first confirmation** (another provider, or no live
//!   record): a remove and an add of the same id. After the typed id and
//!   `yes`, a new key is read at the hidden prompt; then, under `user.lock`,
//!   (1) `account_removed` (`reason: provider_change`, `key_for_…` the new
//!   destination) commits on its own, (2) the Keychain entry is deleted and
//!   the new key set (never read, never updated in place), and
//!   `account_key_set` (what the key was typed for; it grants nothing) is
//!   recorded, (3) the file is written with its History entry and
//!   `account_added` (`replaces`, `typed_id_yes_new_key`) in one
//!   transaction. A failure after (1) leaves the Account with no live
//!   record, so no key is used until `change` runs again; after (2), the
//!   words name the exact flags, and `change <id>` with no flags finishes at
//!   that destination.
//!
//! The sentence shows the old values (the live record's; for a first
//! confirmation, the file's) beside the new, and every reference found,
//! with the projects not checked; then the id typed within 2 minutes and
//! `yes`. Under `user.lock` the file's bytes, the Account's latest record
//! and the scan are read again: anything changed since refuses the command,
//! to be run again.

use std::collections::BTreeMap;
use std::io::Write;

use am_config::{AccountsFile, USER_ACCOUNTS_FILE};
use am_domain::{AuditNote, EgressDirectory, SecretString, TerminalError};

use super::{
    AccountEndpoint, AccountsContext, CONFIRM_WITHIN, PROVIDERS, Stop, endpoint_of, interrupted,
    is_model_provider, lock, no_owner_child, not_checked, refuse, reserved, stand, write_words,
    written_pending,
};
use crate::cli::EXIT_ERRORS;
use crate::key_use::{self, Destination};
use crate::runtime::{Places, owner};
use crate::user::{Scan, shown};

/// What a refused `change` did not do, while no key has been read yet.
const NOT_CHANGED: &str = "The Account was not changed and no key was read or stored";
/// What a refused `change` did not do, once a new key was typed.
const NOT_CHANGED_NOR_STORED: &str = "The Account was not changed and no key was stored";

/// Where the Account's calls go before the change: provider, scheme, host
/// and port, and its key state.
struct Old {
    provider: String,
    scheme: String,
    host: String,
    port: String,
    key: String,
}

impl Old {
    fn of(d: &Destination) -> Old {
        Old {
            provider: d.provider.clone(),
            scheme: d.scheme.clone(),
            host: d.host.clone(),
            port: d.port.clone(),
            key: d.key.clone(),
        }
    }

    fn words(&self) -> String {
        format!(
            "provider {} at {}://{}:{}",
            self.provider, self.scheme, self.host, self.port
        )
    }

    fn loopback(&self) -> bool {
        self.scheme == "http"
    }
}

fn new_words(provider: &str, endpoint: &AccountEndpoint) -> String {
    format!(
        "provider {provider} at {}://{}:{} ({})",
        endpoint.scheme, endpoint.host, endpoint.port, endpoint.base_url
    )
}

fn stop(did_not: &str) -> impl Fn(String) -> Stop + '_ {
    move |why| Stop(EXIT_ERRORS, format!("{why}. {did_not}"))
}

fn note(action: &str, id: &str, args: Vec<(&str, String)>) -> AuditNote {
    let mut all = BTreeMap::from([
        ("file".to_string(), USER_ACCOUNTS_FILE.to_string()),
        ("account".to_string(), id.to_string()),
    ]);
    all.extend(args.into_iter().map(|(k, v)| (k.to_string(), v)));
    AuditNote {
        action: action.into(),
        target: id.to_string(),
        args: all,
    }
}

/// True when the file's entry for `id` already holds `provider` and the
/// `base_url` the endpoint writes.
fn holds(file: &AccountsFile, id: &str, provider: &str, endpoint: &AccountEndpoint) -> bool {
    file.accounts
        .iter()
        .find(|a| a.id == id)
        .is_some_and(|a| a.provider == provider && a.base_url.as_deref() == endpoint.written())
}

pub(super) fn change(
    mut ctx: AccountsContext,
    id: &str,
    provider_flag: Option<&str>,
    base_url_flag: Option<&str>,
    out: &mut dyn Write,
) -> Result<(), Stop> {
    super::require_terminal(&ctx, "change", NOT_CHANGED)?;
    // A refused provider is not repeated: it is never echoed.
    if provider_flag.is_some_and(|p| !is_model_provider(p)) {
        return refuse(format!(
            "the `--provider` given is not a model provider; an Account is changed from the command line to {PROVIDERS} only. {NOT_CHANGED}"
        ));
    }
    let places = ctx.places.clone();
    let standing = stand(&places, NOT_CHANGED)?;
    let id_shown = shown(id);
    if reserved(id) {
        return refuse(format!(
            "'{id_shown}' is a reserved local Account: it is always available and is never changed. {NOT_CHANGED}"
        ));
    }
    if !standing.file.has(id) {
        return refuse(format!(
            "there is no Account '{id_shown}' on this Mac (`agentic accounts list` shows the Accounts; add one with `agentic accounts add`). {NOT_CHANGED}"
        ));
    }
    let errors = standing.file.entry_errors(id);
    let current = standing.file.accounts.iter().find(|a| a.id == id).cloned();
    let Some(current) = current.filter(|_| errors.is_empty()) else {
        return refuse(format!(
            "the Account '{id_shown}' cannot be changed: its entry in {USER_ACCOUNTS_FILE} has an error ({}); `agentic validate` names the line and the fix. {NOT_CHANGED}",
            errors.join("; ")
        ));
    };
    if !is_model_provider(&current.provider) {
        return refuse(format!(
            "the Account '{id_shown}' is a {} Account, not a model Account; only model Accounts are changed from the command line. {NOT_CHANGED}",
            shown(&current.provider)
        ));
    }
    if am_config::key_ref(&current) != Some(id) {
        return refuse(format!(
            "the Account '{id_shown}' has a `secret_ref` that names another Keychain entry; a key is kept only under the Account's own id. Set `secret_ref: {id_shown}` (or remove the line) in {USER_ACCOUNTS_FILE} first. {NOT_CHANGED}"
        ));
    }
    if standing.last.is_none() {
        return refuse(format!(
            "the app has never written {USER_ACCOUNTS_FILE} on this Mac, so it has nothing to confirm against; add the Account with `agentic accounts add`. {NOT_CHANGED}"
        ));
    }
    // A key pasted into the address is never shown, recorded or written
    // (review follow-up): refused before anything else reads it.
    if base_url_flag.is_some_and(am_config::looks_like_a_secret) {
        return refuse(format!(
            "the `--base-url` given holds a secret-looking value (a key prefix such as `sk-`); a key is never part of an address, it is typed at the hidden key prompt. {NOT_CHANGED}"
        ));
    }
    // An interrupted provider change (review fix Adversarial 1): with no
    // flags, `change` finishes it at the destination its key was typed for.
    let pending = if provider_flag.is_none() && base_url_flag.is_none() {
        key_use::pending_change(&places, id).map_err(stop(NOT_CHANGED))?
    } else {
        None
    };
    // The new values: a flag, or the file's for what the flags leave out
    // (no `base_url` once the provider changes).
    let new_provider = provider_flag
        .or(pending.as_ref().map(|p| p.dest.provider.as_str()))
        .unwrap_or(&current.provider)
        .to_string();
    let (new_base, flag) = match (base_url_flag, &pending) {
        (Some(url), _) => (Some(url), "--base-url"),
        (None, Some(p)) => (p.base_url.as_deref(), "base_url"),
        (None, None) if new_provider == current.provider => {
            (current.base_url.as_deref(), "base_url")
        }
        (None, None) => (None, "--base-url"),
    };
    let endpoint = endpoint_of(&new_provider, new_base, flag).map_err(stop(NOT_CHANGED))?;
    not_the_app(&places, &endpoint, NOT_CHANGED)?;
    // The live confirmation record decides the path, and the old values.
    // Both from one read of the notes (review follow-up).
    let (live, record_seq) = key_use::record_state(&places, id).map_err(stop(NOT_CHANGED))?;
    let old = match &live {
        Some((d, _)) => Old::of(d),
        None => {
            let e = endpoint_of(&current.provider, current.base_url.as_deref(), "base_url")
                .map_err(stop(NOT_CHANGED))?;
            Old {
                provider: current.provider.clone(),
                scheme: e.scheme.to_string(),
                host: e.host.clone(),
                port: e.port.to_string(),
                key: "stored".into(),
            }
        }
    };
    let address = live
        .as_ref()
        .is_some_and(|(d, _)| d.provider == new_provider);
    // Keyless Accounts arrive with story 2.10: none may become remote.
    if old.key == "none" && !endpoint.loopback {
        return refuse(format!(
            "the Account '{id_shown}' has no key, and an Account with no key stays on this Mac; a remote endpoint needs an Account with a key. {NOT_CHANGED}"
        ));
    }
    let file_holds = holds(&standing.file, id, &new_provider, &endpoint);
    if address
        && file_holds
        && live
            .as_ref()
            .is_some_and(|(d, _)| d.names(&new_provider, &endpoint))
    {
        return refuse(format!(
            "the Account '{id_shown}' is already confirmed for {}, as {USER_ACCOUNTS_FILE} holds it; there is nothing to change. Give `--provider` or `--base-url` for a new destination. {NOT_CHANGED}",
            new_words(&new_provider, &endpoint)
        ));
    }
    let scan = crate::user::scan(&places, id);
    no_owner_child(&scan, "change", NOT_CHANGED)?;
    before_story_18(&scan, id, &old, &new_provider, &endpoint, NOT_CHANGED)?;

    // The sentence.
    let how = if address {
        format!(
            "The provider stays {new_provider}: the key stored under '{id_shown}' in this Mac's Keychain will now be sent to the new endpoint, {}. No key is asked for, and the Keychain entry is not read or changed.",
            endpoint.base_url
        )
    } else {
        let why = if live.is_some() {
            "The provider changes".to_string()
        } else {
            format!(
                "'{id_shown}' has no confirmation on this Mac (it was never confirmed, was moved here unconfirmed, was removed, or an earlier change did not finish), so this confirms it"
            )
        };
        let held = match &pending {
            Some(p) => format!(
                " This Mac's Keychain entry for '{id_shown}' already holds a key typed for {} by an earlier change that did not finish; it is replaced by the key you type now.",
                p.dest.words()
            ),
            None => String::new(),
        };
        format!(
            "{why}: the key stored under '{id_shown}' will not be sent to the new provider. After you confirm, you type a new key now, and the old key is deleted from this Mac's Keychain and not kept.{held}"
        )
    };
    let from = if live.is_some() {
        "confirmed now"
    } else {
        "in the file, not confirmed"
    };
    let named = if scan.uses.is_empty() {
        "Nothing in the projects this Mac knows names it.".to_string()
    } else {
        format!("Named by, and moved by this change: {}.", scan.uses_words())
    };
    ctx.terminal.show(&format!(
        "Changing the Account '{id_shown}' on this Mac, for every project.\n  from: {} ({from})\n  to:   {}\n{how} {named}{}\nTo confirm, type the Account's id ({id_shown}) within 2 minutes: ",
        old.words(),
        new_words(&new_provider, &endpoint),
        not_checked(&scan)
    ));
    let typed = read_answer(&mut ctx, "the Account's id")?;
    if typed.trim() != id {
        return refuse(format!(
            "what was typed is not the Account's id '{id_shown}'. {NOT_CHANGED}"
        ));
    }
    ctx.terminal
        .show("Type yes to make this change (any other answer changes nothing): ");
    let answer = read_answer(&mut ctx, "yes")?;
    if answer.trim() != "yes" {
        return refuse(format!(
            "the change was not confirmed (`yes` was not typed). {NOT_CHANGED}"
        ));
    }
    let new_key = if address {
        None
    } else {
        ctx.terminal.show(&format!(
            "New key for '{id_shown}' (not shown as you type or paste it): "
        ));
        let key = match ctx.terminal.read_secret() {
            Ok(key) => key,
            Err(TerminalError::Interrupted) => return Err(interrupted(NOT_CHANGED_NOR_STORED)),
            Err(e) => {
                return refuse(format!("no key was read ({e}). {NOT_CHANGED_NOR_STORED}"));
            }
        };
        if key.expose().trim().is_empty() {
            return refuse(format!("no key was typed. {NOT_CHANGED_NOR_STORED}"));
        }
        Some(SecretString::new(key.expose().trim().to_string()))
    };

    // Under `user.lock`: the file, the Account's records and the scan again.
    let did_not = if address {
        NOT_CHANGED
    } else {
        NOT_CHANGED_NOR_STORED
    };
    let locked = lock(&places, did_not, out)?;
    let again = format!("{did_not}; run the command again");
    let now = locked.standing(&places).map_err(stop(did_not))?;
    if now.outside_change() {
        return refuse(crate::user::outside_words(&places, did_not));
    }
    if now.file.problem.is_some() || now.file.sha256 != standing.file.sha256 {
        return refuse(format!(
            "{USER_ACCOUNTS_FILE} changed while this command waited at its prompt, so the change was built on what the file no longer holds. {again}"
        ));
    }
    // A record written since is a change, unless it is a note of the write
    // this lock just settled (its bytes are what the sentence was built on).
    let since = key_use::notes_after(&places, id, record_seq).map_err(stop(did_not))?;
    if since.iter().any(|e| !locked.settled.contains(e)) {
        return refuse(format!(
            "this Mac's confirmation of '{id_shown}' changed while this command waited. {again}"
        ));
    }
    // Review fix Adversarial 6: the app's own ports, again.
    not_the_app(&places, &endpoint, did_not)?;
    // The address path stands on the live record the sentence showed: the
    // same record (sequence and destination) under the lock (review
    // follow-up).
    let (live_now, _) = key_use::record_state(&places, id).map_err(stop(did_not))?;
    let same = |a: &Option<(Destination, i64)>, b: &Option<(Destination, i64)>| match (a, b) {
        (Some((x, i)), Some((y, j))) => i == j && x.replaces() == y.replaces() && x.key == y.key,
        (None, None) => true,
        _ => false,
    };
    if address && !same(&live, &live_now) {
        return refuse(format!(
            "this Mac's confirmation of '{id_shown}' changed while this command waited. {again}"
        ));
    }
    let scan_now = crate::user::scan(&places, id);
    no_owner_child(&scan_now, "change", did_not)?;
    if scan_now.uses != scan.uses || scan_now.unreachable != scan.unreachable {
        return refuse(format!(
            "what names the Account '{id_shown}', or which projects this Mac could not reach, changed while this command waited (now named by {}). {again}",
            scan_now.uses_words()
        ));
    }
    if ctx.terminal.interrupted() {
        return Err(interrupted(did_not));
    }
    let Some(built_on) = now.file.sha256.clone() else {
        return refuse(format!("{USER_ACCOUNTS_FILE} is gone. {again}"));
    };
    let references: Vec<String> = scan_now
        .uses
        .iter()
        .map(|(root, u)| format!("{}:{u}", root.display()))
        .collect();
    let references = references.join(", ");
    // Built before any Keychain call, so no refusal after it is about the
    // edit.
    let edit = if file_holds {
        None
    } else {
        Some(
            am_config::change_account_edit(&now.file, id, &new_provider, endpoint.written())
                .ok_or_else(|| {
                    Stop(
                        EXIT_ERRORS,
                        format!("there is no Account '{id_shown}' on this Mac any more. {again}"),
                    )
                })?,
        )
    };
    // Every check of the write (a secret-looking value, an error it adds)
    // runs now, before anything is recorded or the Keychain is touched
    // (review follow-up).
    if let Some(edit) = &edit
        && let Err(e) = locked.writer.check_edit(edit.clone(), &built_on)
    {
        return refuse(format!(
            "the change of '{id_shown}' cannot be written to {USER_ACCOUNTS_FILE}: {}. {did_not}",
            write_words(&e)
        ));
    }

    let Some(key) = new_key else {
        // Address change: no Keychain call; the record with the write.
        let args = vec![
            ("kind", "account_changed".to_string()),
            ("old_provider", old.provider.clone()),
            ("old_scheme", old.scheme.clone()),
            ("old_host", old.host.clone()),
            ("old_port", old.port.clone()),
            ("new_provider", new_provider.clone()),
            ("new_scheme", endpoint.scheme.to_string()),
            ("new_host", endpoint.host.clone()),
            ("new_port", endpoint.port.to_string()),
            ("references", references),
            ("key", old.key.clone()),
            ("confirmation", "typed_id_yes".to_string()),
        ];
        let changed = note("config.widened", id, args);
        // The confirmation commits with its History entry (review fix
        // G-record): an entry whose before equals its after when the file
        // already holds the values.
        let written = match edit {
            None => locked
                .writer
                .apply_unchanged_with_audit(owner(), &built_on, vec![changed]),
            Some(edit) => locked
                .writer
                .apply_with_audit(owner(), edit, &built_on, vec![changed]),
        };
        let entry = {
            match written {
                Ok(entry) => format!("History entry {}", entry.id),
                Err(e) => {
                    let why = write_words(&e);
                    return refuse(match written_pending(&places, locked.history.as_ref()) {
                        Some(true) => format!(
                            "the change of '{id_shown}' was written to {USER_ACCOUNTS_FILE}, but the write did not finish: {why}. Its History entry and confirmation are recorded the next time an Account command takes this Mac's Accounts lock"
                        ),
                        Some(false) => format!(
                            "the Account '{id_shown}' was not changed: {why}. This command did not change {USER_ACCOUNTS_FILE} and made no Keychain call"
                        ),
                        None => format!(
                            "the change of '{id_shown}' stopped ({why}), and this Mac's user store could not be read to tell whether {USER_ACCOUNTS_FILE} was written; `agentic accounts list` shows what it holds"
                        ),
                    });
                }
            }
        };
        let _ = writeln!(
            out,
            "agentic: changed the Account '{id_shown}' to {} ({entry}); the key the Keychain holds for it now goes there",
            new_words(&new_provider, &endpoint)
        );
        return Ok(());
    };

    // Provider change or first confirmation. `replaces` is the live record
    // now (a write this lock settled may have committed one).
    let replaces = live_now.map_or_else(|| "none".to_string(), |(d, _)| d.replaces());
    let finish = match endpoint.written() {
        Some(url) => {
            format!("agentic accounts change {id_shown} --provider {new_provider} --base-url {url}")
        }
        None => format!("agentic accounts change {id_shown} --provider {new_provider}"),
    };
    let destination = [
        ("key_for_provider", new_provider.clone()),
        ("key_for_scheme", endpoint.scheme.to_string()),
        ("key_for_host", endpoint.host.clone()),
        ("key_for_port", endpoint.port.to_string()),
    ];
    // (1) The removal on its own.
    let mut args = vec![
        ("kind", "account_removed".to_string()),
        ("reason", "provider_change".to_string()),
    ];
    args.extend(destination.iter().cloned());
    let removed = note("config.narrowed", id, args);
    if let Err(e) =
        locked
            .history
            .record_config_notes(&owner(), USER_ACCOUNTS_FILE, &built_on, &[removed])
    {
        drop(key);
        return refuse(format!(
            "the old confirmation of '{id_shown}' could not be ended ({e}). {again}"
        ));
    }
    let ended = format!(
        "Its old confirmation has ended, so no key is used for '{id_shown}' until you finish with `{finish}` (typing a new key again)"
    );
    // (2) The Keychain entry replaced: deleted, then set; never read.
    if let Err(e) = ctx.secrets.delete(id) {
        drop(key);
        return refuse(format!(
            "the old key of '{id_shown}' could not be deleted from the Keychain ({e}), so the Keychain entry was not replaced and the new key was not stored. {ended}"
        ));
    }
    if let Err(e) = ctx.secrets.set(id, key) {
        return refuse(format!(
            "the new key of '{id_shown}' could not be stored in the Keychain ({e}); the old key was deleted. {ended}"
        ));
    }
    // From here the Keychain entry holds the new key: every refusal says so.
    let replaced = format!(
        "The Keychain entry for '{id_shown}' was replaced: it holds the new key you typed (the old key was deleted). {ended}"
    );
    // The key now held, and for where (review fix G-keyfor): grants nothing.
    let mut args = vec![("kind", "account_key_set".to_string())];
    args.extend(destination.iter().cloned());
    if let Some(url) = endpoint
        .written()
        .filter(|u| !am_config::looks_like_a_secret(u))
    {
        args.push(("key_for_base_url", url.to_string()));
    }
    let key_set = note("config.narrowed", id, args);
    if let Err(e) =
        locked
            .history
            .record_config_notes(&owner(), USER_ACCOUNTS_FILE, &built_on, &[key_set])
    {
        return refuse(format!(
            "what the new key of '{id_shown}' is for could not be recorded ({e}). {replaced}"
        ));
    }
    // (3) The file with its History entry and, in its transaction, the new
    // confirmation (review fix G-record; an entry whose before equals its
    // after when the file already holds the values).
    let args = vec![
        ("kind", "account_added".to_string()),
        ("provider", new_provider.clone()),
        ("scheme", endpoint.scheme.to_string()),
        ("host", endpoint.host.clone()),
        ("port", endpoint.port.to_string()),
        ("references", references),
        ("key", "stored".to_string()),
        ("replaces", replaces),
        ("confirmation", "typed_id_yes_new_key".to_string()),
    ];
    let added = note("config.widened", id, args);
    let written = match edit {
        None => locked
            .writer
            .apply_unchanged_with_audit(owner(), &built_on, vec![added]),
        Some(edit) => locked
            .writer
            .apply_with_audit(owner(), edit, &built_on, vec![added]),
    };
    if let Err(e) = written {
        let why = write_words(&e);
        return refuse(match written_pending(&places, locked.history.as_ref()) {
            Some(true) => format!(
                "the new values of '{id_shown}' were written to {USER_ACCOUNTS_FILE}, but the write did not finish: {why}. Its History entry and confirmation are recorded the next time an Account command takes this Mac's Accounts lock. The Keychain entry for '{id_shown}' was replaced: it holds the new key you typed (the old key was deleted)"
            ),
            Some(false) => format!(
                "the new values of '{id_shown}' were not written: {why}. This command did not change {USER_ACCOUNTS_FILE}. {replaced}"
            ),
            None => format!(
                "the write of '{id_shown}' stopped ({why}), and this Mac's user store could not be read to tell whether {USER_ACCOUNTS_FILE} was written. {replaced}"
            ),
        });
    }
    let _ = writeln!(
        out,
        "agentic: confirmed the Account '{id_shown}' for {} with a new key in the Keychain; the old key was deleted ({})",
        new_words(&new_provider, &endpoint),
        places.user.accounts_file().display()
    );
    Ok(())
}

/// The app's own ports (every running `agentic serve`, and the one each
/// runtime folder records): an Account never reaches the app itself.
fn not_the_app(places: &Places, endpoint: &AccountEndpoint, did_not: &str) -> Result<(), Stop> {
    let directory = crate::egress::ProjectDirectory::for_accounts(places);
    if endpoint.loopback && directory.own_ports().contains(&endpoint.port) {
        return refuse(format!(
            "the new endpoint is not accepted: {}, and an Account never reaches the app itself. {did_not}",
            crate::egress::own_server_words(endpoint.port)
        ));
    }
    Ok(())
}

/// One line typed within 2 minutes: the id or `yes`.
fn read_answer(ctx: &mut AccountsContext, what: &str) -> Result<String, Stop> {
    match ctx.terminal.read_line(Some(CONFIRM_WITHIN)) {
        Ok(line) => Ok(line),
        Err(TerminalError::TimedOut) => {
            ctx.terminal.show("\n");
            refuse(format!(
                "{what} was not typed within 2 minutes. {NOT_CHANGED}"
            ))
        }
        Err(TerminalError::Interrupted) => {
            ctx.terminal.show("\n");
            Err(interrupted(NOT_CHANGED))
        }
        Err(e) => refuse(format!("{what} was not typed ({e}). {NOT_CHANGED}")),
    }
}

/// Before epic 4 story 18 (AD-15 *Change*): while a reachable project names
/// the Account, no change moves it from a loopback to a remote endpoint or
/// to another host (the same host with another port, path or scheme is
/// allowed). An unreachable project is only listed.
fn before_story_18(
    scan: &Scan,
    id: &str,
    old: &Old,
    provider: &str,
    endpoint: &AccountEndpoint,
    did_not: &str,
) -> Result<(), Stop> {
    if scan.uses.is_empty() {
        return Ok(());
    }
    let to_remote = old.loopback() && !endpoint.loopback;
    if !to_remote && old.host == endpoint.host {
        return Ok(());
    }
    refuse(format!(
        "the Account '{}' is named by {}, so it cannot move from {} to {} yet: a move from this Mac to a remote endpoint, or to another host, waits for the record of each project's Account destinations that a later version adds. {did_not}; point those projects at another Account first, or keep the host",
        shown(id),
        scan.uses_words(),
        old.words(),
        new_words(provider, endpoint)
    ))
}
