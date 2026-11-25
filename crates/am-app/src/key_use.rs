//! AD-15 *Key use* (story 2.12): the one check before any key of a
//! user-level Account is read. `agentic accounts test` and the egress
//! directory ([`crate::egress::ProjectDirectory`]) both call it, so every
//! path that sends a key gets it.
//!
//! The check is read-only (a user-store reader, no lock) and runs over the
//! exact bytes the endpoint is taken from:
//!
//! 1. the app data dir is owner-only and the user store is this Mac's (not a
//!    project's, not a newer app's);
//! 2. the Account is in the user Accounts file, its entry has no error, it
//!    is a model Account with an endpoint `agentic accounts add` accepts,
//!    and its key is read only from the entry named by its own id
//!    (`am_config::key_ref`; story 2.10: an entry with no `secret_ref` is
//!    keyless, `am_config::key_state`);
//! 3. those bytes hash to the after-hash of the file's latest History entry
//!    in the user store;
//! 4. the Account's latest confirmation record (`account_added`, or
//!    `account_changed` from story 14), with no `account_removed` after it,
//!    written by the owner with an intent of the user file, names the same
//!    provider, scheme, host, port and key state (story 2.10: a record with
//!    `key: stored` needs `secret_ref: <id>` in the entry, one with `key:
//!    none` needs no `secret_ref`, at `http://127.0.0.1:<port>` only; any
//!    other pairing refuses). An Account whose latest
//!    record is `account_listed` (story 2.13: listed unconfirmed by
//!    `accounts migrate`) is refused naming `agentic accounts change <id>`.
//!
//! A hash that fails while another process holds `user.lock`, or that
//! equals a pending intent's new hash, is a write in flight: the check waits
//! for the lock (bounded) and runs once more. After the key is read and
//! before it is sent, the caller compares [`stamp`] with the one the check
//! gave; a record that moved meanwhile drops the key unsent and the check
//! runs again. Any failure reads no key and says in words what to do.

use std::time::Instant;

use am_config::{Account, KeyState, USER_ACCOUNTS_FILE};
use am_domain::Actor;
use am_store::ConfigNote;

use crate::accounts::{self, AccountEndpoint};
use crate::runtime::Places;
use crate::user::{LOCK_WAIT, Standing};

/// Why a key may not be read for an Account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// The app data dir, the user store or the file cannot be used (loose
    /// permissions, a project's store at `user.db`, a newer store, a file
    /// that is not YAML).
    Store(String),
    /// The user list holds no Account with that id.
    NoAccount,
    /// Its entry in the file has an error (each `line N: message`).
    Entry(Vec<String>),
    /// Not a model Account (its provider).
    NotModel(String),
    /// Its endpoint is not one `add` accepts (why).
    Endpoint(String),
    /// `secret_ref` names another Keychain entry.
    SecretRef(String),
    /// Not confirmed by the owner for what the file holds (why).
    NotConfirmed(String),
    /// The file holds the bytes of a write that was interrupted (its
    /// History entry not recorded yet) and no process is finishing it.
    Interrupted,
    /// Listed unconfirmed by `agentic accounts migrate` (story 2.13, an
    /// `account_listed` record with no confirmation after it): no key is
    /// used until `agentic accounts change <id>` confirms it.
    Unconfirmed,
    /// Story 2.10: confirmed with a key, but its entry has no `secret_ref`.
    KeyedWithoutLine,
    /// Story 2.10: confirmed with no key, but its entry has a `secret_ref`.
    KeylessWithLine,
}

impl Refused {
    /// The refusal in words, for a caller with no wording of its own.
    pub fn words(&self, id: &str) -> String {
        match self {
            Refused::Store(why) => why.clone(),
            Refused::NoAccount => no_account_words(id),
            Refused::Entry(errors) => format!(
                "its entry in {USER_ACCOUNTS_FILE} has an error ({}); `agentic validate` names the line and the fix",
                errors.join("; ")
            ),
            Refused::NotModel(provider) => format!(
                "a {provider} Account is not a model Account, and only model Accounts are called in this version"
            ),
            Refused::Endpoint(why) => why.clone(),
            Refused::SecretRef(_) => "its `secret_ref` names another Keychain entry; a key is only ever read from the entry named by the Account's own id".to_string(),
            Refused::NotConfirmed(why) => {
                format!(
                    "it is not confirmed on this Mac ({why}); confirm it with `agentic accounts change {id}` in a terminal"
                )
            }
            Refused::Interrupted => interrupted_words(),
            Refused::Unconfirmed => unconfirmed_words(id),
            Refused::KeyedWithoutLine => keyed_without_line_words(id),
            Refused::KeylessWithLine => keyless_with_line_words(id),
        }
    }
}

/// Story 2.10 (matrix row 14; review K1): an Account confirmed with a key
/// whose entry lost its `secret_ref`. `change` repairs it (an address
/// change keeps the confirmed key state); a hand edit would be refused as
/// an outside change, so none is offered.
pub fn keyed_without_line_words(id: &str) -> String {
    format!(
        "it was confirmed with a key, but its entry in {USER_ACCOUNTS_FILE} has no `secret_ref`, so no key is read for it and nothing is sent; run `agentic accounts change {id}` in a terminal: it confirms it again with its key and writes `secret_ref: {id}` to its entry"
    )
}

/// Story 2.10 (matrix row 16; review K1): an Account confirmed with no key
/// whose entry names a Keychain entry. `change` confirms it keyless again;
/// a key is given only by removing it and adding it again.
pub fn keyless_with_line_words(id: &str) -> String {
    format!(
        "it was confirmed with no key, but its entry in {USER_ACCOUNTS_FILE} has a `secret_ref`, so nothing is read or sent for it; run `agentic accounts change {id}` in a terminal: it confirms it keyless again and removes its `secret_ref` line. To give it a key instead, remove it with `agentic accounts remove {id}` and add it again without `--no-key`"
    )
}

/// The words for a write of the user file that was interrupted.
pub fn interrupted_words() -> String {
    format!(
        "a write of this Mac's {USER_ACCOUNTS_FILE} was interrupted before its History entry was recorded, so no key is read until it is settled; the next `agentic accounts add`, `change` or `remove` settles it, then run this command again"
    )
}

/// The words for an Account `accounts migrate` listed unconfirmed.
pub fn unconfirmed_words(id: &str) -> String {
    format!(
        "'{id}' was moved to this Mac's Accounts unconfirmed (by `agentic accounts migrate`), so no key is used for it until you confirm it with `agentic accounts change {id}`, which asks for a new key"
    )
}

/// The words for an id this Mac's list lacks.
pub fn no_account_words(id: &str) -> String {
    format!(
        "the Account '{id}' is not on this Mac; add that Account on this Mac: `agentic accounts add {id}`"
    )
}

/// Where things stood for one Account when the check passed: the user
/// file's hash (as its latest History entry wrote it) and the sequence of
/// the Account's latest confirmation record. With the confirmed endpoint it
/// is the directory's `AccountStamp`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stamp {
    file: String,
    record: i64,
}

impl std::fmt::Display for Stamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.file, self.record)
    }
}

/// An Account the check passed, with its endpoint, its stamp and its key
/// state (story 2.10: `None` for a keyless Account, whose calls carry no
/// credential and read no Keychain entry).
#[derive(Debug, Clone)]
pub struct Confirmed {
    pub account: Account,
    pub(crate) endpoint: AccountEndpoint,
    pub stamp: Stamp,
    pub key: KeyState,
}

impl Confirmed {
    /// The endpoint the Account is pinned to (scheme, host, port).
    pub fn endpoint(&self) -> am_domain::Endpoint {
        self.endpoint.to_domain()
    }
}

/// One outcome of a single check: in flight, or settled.
enum Once {
    InFlight(String),
    Done(Box<Result<Confirmed, Refused>>),
}

/// AD-15's check for the Account `id`, waiting once (bounded) for a write
/// in flight.
pub fn check(places: &Places, id: &str) -> Result<Confirmed, Refused> {
    match once(places, id) {
        Once::Done(done) => *done,
        Once::InFlight(_) => {
            wait_for_lock(places);
            match once(places, id) {
                Once::Done(done) => *done,
                Once::InFlight(why) => Err(Refused::NotConfirmed(why)),
            }
        }
    }
}

/// Waits while another process holds `user.lock`, at most [`LOCK_WAIT`].
fn wait_for_lock(places: &Places) {
    let started = Instant::now();
    while am_store::user::user_lock_is_held(&places.user) && started.elapsed() < LOCK_WAIT {
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

/// The owner's confirmation records of `id` on the user file, oldest first:
/// `account_added`, `account_changed` and `account_removed`.
fn records(reader: &am_store::user::UserReader, id: &str) -> Result<Vec<ConfigNote>, String> {
    let mut all = Vec::new();
    for action in ["config.widened", "config.narrowed"] {
        all.extend(
            reader
                .config_note_args(action, id)
                .map_err(|e| e.to_string())?,
        );
    }
    all.retain(|n| {
        matches!(n.actor, Actor::Human { .. })
            && n.file == USER_ACCOUNTS_FILE
            && matches!(
                n.args.get("kind").map(String::as_str),
                Some("account_added" | "account_changed" | "account_removed")
            )
    });
    all.sort_by_key(|n| n.seq);
    Ok(all)
}

/// True when the owner's `account_listed` record of `id` on the user file
/// (story 2.13) comes after the record at `seq`.
fn listed_after(reader: &am_store::user::UserReader, id: &str, seq: i64) -> Result<bool, String> {
    Ok(reader
        .config_note_args("config.widened", id)
        .map_err(|e| e.to_string())?
        .iter()
        .any(|n| {
            matches!(n.actor, Actor::Human { .. })
                && n.file == USER_ACCOUNTS_FILE
                && n.args.get("kind").map(String::as_str) == Some("account_listed")
                && n.seq > seq
        }))
}

/// What the latest record of an Account confirms: provider, scheme, host,
/// port and key state; `None` when there is none or it was removed.
pub(crate) struct Destination {
    pub provider: String,
    pub scheme: String,
    pub host: String,
    pub port: String,
    pub key: String,
}

impl Destination {
    fn of(note: &ConfigNote) -> Option<Destination> {
        let kind = note.args.get("kind").map(String::as_str)?;
        if kind == "account_removed" {
            return None;
        }
        // A change records the old and the new values; the new ones govern.
        let get = |key: &str| {
            let new = format!("new_{key}");
            note.args
                .get(&new)
                .or_else(|| note.args.get(key))
                .cloned()
                .unwrap_or_default()
        };
        Some(Destination {
            provider: get("provider"),
            scheme: get("scheme"),
            host: get("host"),
            port: get("port"),
            key: note
                .args
                .get("key")
                .cloned()
                .unwrap_or_else(|| "stored".into()),
        })
    }

    /// The destination an `account_key_set` names in its `key_for_…`
    /// fields (story 2.14, review fix G-keyfor: written only after the
    /// Keychain `set` succeeded); `None` for any other record.
    fn key_set(note: &ConfigNote) -> Option<Destination> {
        if note.args.get("kind").map(String::as_str) != Some("account_key_set") {
            return None;
        }
        let get = |key: &str| note.args.get(&format!("key_for_{key}")).cloned();
        Some(Destination {
            provider: get("provider")?,
            scheme: get("scheme")?,
            host: get("host")?,
            port: get("port")?,
            key: "stored".into(),
        })
    }

    /// The destination as `account_added`'s `replaces` names it (story
    /// 2.14): `<provider> <scheme>://<host>:<port>`.
    pub fn replaces(&self) -> String {
        format!(
            "{} {}://{}:{}",
            self.provider, self.scheme, self.host, self.port
        )
    }

    /// True when this names `provider` at `endpoint` (whatever its key
    /// state; story 2.10 compares that apart, [`Destination::key_state`]).
    pub fn names(&self, provider: &str, endpoint: &AccountEndpoint) -> bool {
        self.provider == provider
            && self.scheme == endpoint.scheme
            && self.host == endpoint.host
            && self.port == endpoint.port.to_string()
    }

    /// The key state the record confirms: `none` is keyless, anything else
    /// (a record from before story 2.10 has no `key`) a stored key.
    pub fn key_state(&self) -> KeyState {
        if self.key == KeyState::None.recorded() {
            KeyState::None
        } else {
            KeyState::Stored
        }
    }

    pub fn words(&self) -> String {
        format!(
            "provider {} at {}://{}:{}",
            self.provider, self.scheme, self.host, self.port
        )
    }
}

/// The latest confirmed destination of `id`, read-only: what the owner
/// confirmed last, with no removal after it.
pub(crate) fn confirmed_destination(
    places: &Places,
    id: &str,
) -> Result<Option<Destination>, String> {
    let reader = crate::user::reader(places)?;
    Ok(records(&reader, id)?.last().and_then(Destination::of))
}

/// Every note of `id` on the user file by the owner that the change
/// command compares or reads: `account_added`, `account_changed`,
/// `account_removed`, `account_listed` and `account_key_set`, oldest first.
fn all_notes(reader: &am_store::user::UserReader, id: &str) -> Result<Vec<ConfigNote>, String> {
    let mut all = Vec::new();
    for action in ["config.widened", "config.narrowed"] {
        all.extend(
            reader
                .config_note_args(action, id)
                .map_err(|e| e.to_string())?,
        );
    }
    all.retain(|n| {
        matches!(n.actor, Actor::Human { .. })
            && n.file == USER_ACCOUNTS_FILE
            && matches!(
                n.args.get("kind").map(String::as_str),
                Some(
                    "account_added"
                        | "account_changed"
                        | "account_removed"
                        | "account_listed"
                        | "account_key_set"
                )
            )
    });
    all.sort_by_key(|n| n.seq);
    Ok(all)
}

/// Where the key the Keychain holds for `id` was last confirmed or typed
/// for: the latest `account_added` or `account_changed` record, also when
/// an `account_removed` follows it, or a later `account_key_set` (story
/// 2.14: the entry was replaced for that destination). An
/// `account_removed`'s `key_for_…` never names it (the Keychain may have
/// refused the replacement), and neither does a keyless record (story
/// 2.10, review K3: `key: none` confirmed no key).
pub(crate) fn last_confirmed(places: &Places, id: &str) -> Result<Option<Destination>, String> {
    let reader = crate::user::reader(places)?;
    Ok(all_notes(&reader, id)?.iter().rev().find_map(|n| {
        match n.args.get("kind").map(String::as_str) {
            Some("account_added" | "account_changed") => {
                Destination::of(n).filter(|d| d.key_state() == KeyState::Stored)
            }
            Some("account_key_set") => Destination::key_set(n),
            _ => None,
        }
    }))
}

/// The Account's live confirmation record (story 2.14, AD-15 *Change*): its
/// latest `account_added` or `account_changed`, with no `account_removed`
/// and no `account_listed` after it, and that record's sequence number.
/// `None` when it has none (never confirmed, listed unconfirmed by
/// `accounts migrate`, removed, or a change cut short).
pub(crate) fn live_record(places: &Places, id: &str) -> Result<Option<(Destination, i64)>, String> {
    Ok(record_state(places, id)?.0)
}

/// The live record ([`live_record`]) and the sequence number of the latest
/// note of `id` ([`all_notes`]; 0 with none), both from one read of the
/// notes (review follow-up: never two snapshots).
pub(crate) fn record_state(
    places: &Places,
    id: &str,
) -> Result<(Option<(Destination, i64)>, i64), String> {
    let reader = crate::user::reader(places)?;
    let all = all_notes(&reader, id)?;
    let latest = all.last().map_or(0, |n| n.seq);
    let kind = |n: &ConfigNote| n.args.get("kind").cloned().unwrap_or_default();
    let Some(record) = all.iter().rev().find(|n| {
        matches!(
            kind(n).as_str(),
            "account_added" | "account_changed" | "account_removed"
        )
    }) else {
        return Ok((None, latest));
    };
    let listed = all
        .iter()
        .any(|n| n.seq > record.seq && kind(n) == "account_listed");
    if listed {
        return Ok((None, latest));
    }
    Ok((Destination::of(record).map(|d| (d, record.seq)), latest))
}

/// The History entry ids of the notes of `id` after `seq` ([`all_notes`]).
pub(crate) fn notes_after(places: &Places, id: &str, seq: i64) -> Result<Vec<String>, String> {
    let reader = crate::user::reader(places)?;
    Ok(all_notes(&reader, id)?
        .into_iter()
        .filter(|n| n.seq > seq)
        .map(|n| n.entry_id)
        .collect())
}

/// Where an interrupted provider change of `id` was going (story 2.14,
/// review fix Adversarial 1): its latest records are a `provider_change`
/// `account_removed` and then an `account_key_set`, so the Keychain entry
/// holds a key typed for that destination.
pub(crate) struct Pending {
    pub dest: Destination,
    /// The `base_url` typed for it (none for a provider with one endpoint).
    pub base_url: Option<String>,
}

impl Pending {
    /// The flags that finish the change: `--provider P[ --base-url U]`.
    pub fn flags(&self) -> String {
        match &self.base_url {
            Some(url) => format!("--provider {} --base-url {url}", self.dest.provider),
            None => format!("--provider {}", self.dest.provider),
        }
    }
}

/// The interrupted provider change of `id`, if any (see [`Pending`]).
pub(crate) fn pending_change(places: &Places, id: &str) -> Result<Option<Pending>, String> {
    let reader = crate::user::reader(places)?;
    let all = all_notes(&reader, id)?;
    let kind = |n: &ConfigNote| n.args.get("kind").cloned().unwrap_or_default();
    let mut rev = all.iter().rev();
    let (Some(set), Some(removed)) = (rev.next(), rev.next()) else {
        return Ok(None);
    };
    if kind(set) != "account_key_set"
        || kind(removed) != "account_removed"
        || removed.args.get("reason").map(String::as_str) != Some("provider_change")
    {
        return Ok(None);
    }
    let base_url = set.args.get("key_for_base_url").cloned();
    // Never a URL the secret check flags (review follow-up): no flags are
    // offered for it.
    if base_url
        .as_deref()
        .is_some_and(am_config::looks_like_a_secret)
    {
        return Ok(None);
    }
    Ok(Destination::key_set(set).map(|dest| Pending { dest, base_url }))
}

/// The command that confirms `id`: `agentic accounts change <id>`, with the
/// exact flags of an interrupted provider change when there is one.
pub(crate) fn change_command(places: &Places, id: &str) -> String {
    match pending_change(places, id) {
        Ok(Some(p)) => format!("agentic accounts change {id} {}", p.flags()),
        _ => format!("agentic accounts change {id}"),
    }
}

fn done(result: Result<Confirmed, Refused>) -> Once {
    Once::Done(Box::new(result))
}

fn once(places: &Places, id: &str) -> Once {
    let reader = match crate::user::reader(places) {
        Ok(r) => r,
        Err(why) => return done(Err(Refused::Store(why))),
    };
    let standing = match Standing::read(places) {
        Ok(s) => s,
        Err(why) => return done(Err(Refused::Store(why))),
    };
    let file = &standing.file;
    if let Some(problem) = &file.problem {
        return done(Err(Refused::Store(format!(
            "{USER_ACCOUNTS_FILE} ({}) cannot be read as Accounts because {problem} (`agentic validate` names the line)",
            places.user.accounts_file().display()
        ))));
    }
    let Some(account) = file.accounts.iter().find(|a| a.id == id).cloned() else {
        return done(Err(Refused::NoAccount));
    };
    let errors = file.entry_errors(id);
    if !errors.is_empty() {
        return done(Err(Refused::Entry(errors)));
    }
    if !accounts::is_model_provider(&account.provider) {
        return done(Err(Refused::NotModel(account.provider)));
    }
    let endpoint =
        match accounts::endpoint_of(&account.provider, account.base_url.as_deref(), "base_url") {
            Ok(e) => e,
            Err(why) => return done(Err(Refused::Endpoint(why))),
        };
    if let Some(entry) = account.secret_ref.clone().filter(|e| e != id) {
        return done(Err(Refused::SecretRef(entry)));
    }
    // The bytes the endpoint was taken from are the bytes the app wrote.
    let Some(last) = &standing.last else {
        return done(Err(Refused::NotConfirmed(format!(
            "the app has never written {USER_ACCOUNTS_FILE} on this Mac"
        ))));
    };
    if file.sha256.as_deref() != Some(last.after_sha256.as_str()) {
        let why = format!(
            "{USER_ACCOUNTS_FILE} was changed on disk outside the app since the app last wrote it"
        );
        if am_store::user::user_lock_is_held(&places.user) {
            return Once::InFlight(why);
        }
        if standing.pending_holds() {
            return done(Err(Refused::Interrupted));
        }
        return done(Err(Refused::NotConfirmed(why)));
    }
    let records = match records(&reader, id) {
        Ok(r) => r,
        Err(why) => return done(Err(Refused::Store(why))),
    };
    // Listed unconfirmed by `accounts migrate`, with no confirmation since.
    let listed = match listed_after(&reader, id, records.last().map_or(0, |n| n.seq)) {
        Ok(l) => l,
        Err(why) => return done(Err(Refused::Store(why))),
    };
    if listed {
        return done(Err(Refused::Unconfirmed));
    }
    let Some(latest) = records.last() else {
        return done(Err(Refused::NotConfirmed(format!(
            "no `agentic accounts add` of '{id}' is recorded on this Mac"
        ))));
    };
    let Some(confirmed) = Destination::of(latest) else {
        let why = if latest.args.get("reason").map(String::as_str) == Some("provider_change") {
            format!(
                "a confirmation of '{id}' with a new key ended its last confirmation and did not finish; finish it with `{}`",
                change_command(places, id)
            )
        } else {
            format!("'{id}' was removed with `agentic accounts remove` after it was last added")
        };
        return done(Err(Refused::NotConfirmed(why)));
    };
    if !confirmed.names(&account.provider, &endpoint) {
        return done(Err(Refused::NotConfirmed(format!(
            "the last confirmation of '{id}' recorded {}, and {USER_ACCOUNTS_FILE} now holds provider {} at {}://{}:{}",
            confirmed.words(),
            account.provider,
            endpoint.scheme,
            endpoint.host,
            endpoint.port
        ))));
    }
    // Story 2.10: the file's key state is the record's, and a keyless
    // Account is called only on this Mac.
    let key = am_config::key_state(&account);
    match (confirmed.key_state(), key) {
        (KeyState::Stored, KeyState::None) => return done(Err(Refused::KeyedWithoutLine)),
        (KeyState::None, KeyState::Stored) => return done(Err(Refused::KeylessWithLine)),
        (KeyState::None, KeyState::None) if !endpoint.loopback => {
            return done(Err(Refused::NotConfirmed(format!(
                "'{id}' has no key, and an Account with no key is called only at http://127.0.0.1:<port>"
            ))));
        }
        _ => {}
    }
    done(Ok(Confirmed {
        account,
        endpoint,
        stamp: Stamp {
            file: last.after_sha256.clone(),
            record: latest.seq,
        },
        key,
    }))
}

/// Where the user store stands for `id` now (read-only): compared with a
/// check's stamp after the key is read and before it is sent. `None` when
/// it cannot be read (which the caller treats as moved).
pub fn stamp(places: &Places, id: &str) -> Option<Stamp> {
    let reader = crate::user::reader(places).ok()?;
    let standing = Standing::read(places).ok()?;
    // The file itself must still hold what its latest entry wrote.
    let file = standing.last.as_ref()?.after_sha256.clone();
    if Some(file.as_str()) != standing.file.sha256.as_deref() {
        return None;
    }
    let record = records(&reader, id).ok()?.last().map_or(0, |n| n.seq);
    Some(Stamp { file, record })
}
