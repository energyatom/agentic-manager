//! `agentic accounts migrate --project <path> [--as <old>=<new>]...
//! [--dry-run | --rollback]` (story 2.13; AD-15 *Migrate*, *Record*,
//! *Terminal*; AD-16; AD-17 *Old project files*; AD-32): moves a project's
//! old `models/accounts.yaml` (stories 2.1 and 2.3) to this Mac's user list,
//! one entry at a time, and removes the file with one `ConfigMigration`.
//!
//! **Per entry** it decides one of four outcomes:
//!
//! - *present*: this Mac's list holds the id at the same provider, scheme,
//!   host and port; nothing is done;
//! - *clash*: it holds the id at another destination; the whole command is
//!   refused unless `--as <old>=<new>` renames it;
//! - *carried over*: the project store (read-only, its audit chain
//!   verifying) holds a latest `account_added` of the id, with no
//!   `account_removed` after it, naming the entry's provider, scheme, host
//!   and port, while the user store holds no record at all for the id and
//!   is not marked `recovered`: the user store gets an `account_added` with
//!   `confirmation: migrated`, the project id and the source entry's
//!   sequence number and hash;
//! - *unconfirmed*: otherwise; the user store gets an `account_listed`
//!   record, which is no confirmation, so the Key-use check refuses it
//!   until `agentic accounts change <id>` confirms it.
//!
//! **No key, ever:** no path here makes a `SecretStore` call.
//!
//! **Order:** the terminal rule; the project opened read-only by
//! `open_project_unchanged` (its store's lock held for the whole command);
//! the refusals (children that run as the owner in this or any reachable
//! project, an outside change of a project file or of the user file); the
//! old file read and the plan made; one sentence per entry and the id of
//! each new Account typed within 2 minutes; then `user.lock`, a fresh read
//! of everything the sentences were built on (any change refuses, to be
//! run again), and one user-store transaction per new Account. The lock
//! goes, the backup and its manifest are written to
//! `<app data dir>/backups/accounts-migrate/<project-id>/` and checked by
//! hash, and last one `ConfigMigration` (the `--as` renames, then the
//! removal) commits in the project store. A crash between leaves the file
//! to migrate again; the next run finds its entries present.
//!
//! **`--dry-run`** only reads: no lock, no terminal, nothing written.
//! **`--rollback`** re-creates the file byte for byte from the latest
//! backup (a `create` in the project's History) after the word `rollback`
//! is typed, and leaves the renames, this Mac's list, the user store and
//! the Keychain as they are.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use am_config::{
    AccountsFile, ConfigCreate, ConfigMigration, LEGACY_ACCOUNTS_FILE, LegacyAccounts,
    MigrationStep, RenamedFile, USER_ACCOUNTS_FILE, sha256_hex,
};
use am_domain::{Actor, AuditNote, ProjectId, TerminalError};
use am_store::ConfigNote;

use super::{
    AccountEndpoint, AccountsContext, CONFIRM_WITHIN, Stop, endpoint_of, interrupted,
    is_model_provider, not_checked, refuse, require_terminal, reserved, stand, write_words,
};
use crate::cli::EXIT_ERRORS;
use crate::runtime::{Places, owner};
use crate::user::{Scan, Standing, shown};

/// What a refused `migrate` did not do.
const NOT_MOVED: &str = "No Account was moved and nothing was written";
/// What a refused `--rollback` did not do.
const NOT_ROLLED_BACK: &str = "Nothing was restored and nothing was written";
/// The word `--rollback` asks for.
const ROLLBACK_WORD: &str = "rollback";

/// How `migrate` runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    Migrate,
    DryRun,
    Rollback,
}

/// The parsed `migrate` command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Migrate {
    pub project: PathBuf,
    /// `--as <old>=<new>`, in the order given.
    pub renames: Vec<(String, String)>,
    pub mode: Mode,
}

/// Parses the words after `migrate`.
pub(super) fn parse(words: &[String]) -> Result<Migrate, String> {
    let mut project: Option<PathBuf> = None;
    let mut renames: Vec<(String, String)> = Vec::new();
    let (mut dry, mut rollback) = (false, false);
    let mut words = words.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--project" => {
                let value = words
                    .next()
                    .filter(|v| !v.starts_with("--") && !v.is_empty())
                    .ok_or("--project needs a value: the project folder")?;
                if project.is_some() {
                    return Err("--project is given twice".into());
                }
                project = Some(PathBuf::from(value));
            }
            "--as" => {
                let value = words
                    .next()
                    .filter(|v| !v.starts_with("--") && !v.is_empty())
                    .ok_or("--as needs a value: <old>=<new>")?;
                let Some((old, new)) = value
                    .split_once('=')
                    .filter(|(o, n)| !o.is_empty() && !n.is_empty() && !n.contains('='))
                else {
                    return Err("--as takes <old>=<new>: the clashing id and its new id".into());
                };
                renames.push((old.to_string(), new.to_string()));
            }
            "--dry-run" => {
                if dry {
                    return Err("--dry-run is given twice".into());
                }
                dry = true;
            }
            "--rollback" => {
                if rollback {
                    return Err("--rollback is given twice".into());
                }
                rollback = true;
            }
            w if w.starts_with('-') => {
                return Err(format!("unknown option `{w}` for `accounts migrate`"));
            }
            w => {
                return Err(format!(
                    "`accounts migrate` takes no word `{w}`; name the project with --project <path>"
                ));
            }
        }
    }
    let project = project.ok_or("accounts migrate needs --project <path>")?;
    if dry && rollback {
        return Err("--dry-run and --rollback cannot be given together".into());
    }
    if !renames.is_empty() && (dry || rollback) {
        return Err("--as is given only to a migrate, not with --dry-run or --rollback".into());
    }
    Ok(Migrate {
        project,
        renames,
        mode: if dry {
            Mode::DryRun
        } else if rollback {
            Mode::Rollback
        } else {
            Mode::Migrate
        },
    })
}

/// Runs the parsed command.
pub(super) fn run(ctx: AccountsContext, m: Migrate, out: &mut dyn Write) -> Result<(), Stop> {
    match m.mode {
        Mode::DryRun => dry_run(&ctx, &m.project, out),
        Mode::Migrate => migrate(ctx, &m.project, &m.renames, out),
        Mode::Rollback => rollback(ctx, &m.project, out),
    }
}

// ------------------------------------------------------------------ the plan

/// What becomes of one entry of the old file.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Outcome {
    /// This Mac's list holds the id (or its new id) at the same destination.
    Present,
    /// This Mac's list holds the id at another destination (in words).
    Clash { theirs: String },
    /// The project's confirmation is carried over (its audit entry).
    Carried { seq: i64, hash: String },
    /// Added unconfirmed (why, in words).
    Unconfirmed { why: String },
}

/// One entry of the old file and its outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    /// The id in the old file.
    id: String,
    /// The id on this Mac's list: `id`, or the new id of `--as`.
    target: String,
    provider: String,
    name: Option<String>,
    endpoint: AccountEndpoint,
    outcome: Outcome,
}

impl Entry {
    fn renamed(&self) -> bool {
        self.id != self.target
    }

    fn is_new(&self) -> bool {
        matches!(
            self.outcome,
            Outcome::Carried { .. } | Outcome::Unconfirmed { .. }
        )
    }

    /// `provider p, https://host:port`.
    fn destination(&self) -> String {
        format!("provider {} at {}", self.provider, self.endpoint.goes_to())
    }
}

/// The notes a store holds for one Account id: `config_note_args` of the
/// user store's reader or guard.
type Notes<'a> = &'a dyn Fn(&str, &str) -> Result<Vec<ConfigNote>, String>;

/// What the plan is made from.
struct Inputs<'a> {
    legacy: &'a LegacyAccounts,
    user: &'a AccountsFile,
    renames: &'a [(String, String)],
    recovered: bool,
    user_notes: Notes<'a>,
    project: &'a ProjectRecords,
    /// The `--as` renames this project's latest backup manifest records
    /// (a re-run after a crash finds their new ids present).
    manifest_renames: &'a [(String, String)],
    /// Plan only the entries that may be shown (`--dry-run`).
    only_clean: bool,
}

/// The project store's records, read once (read-only).
struct ProjectRecords {
    /// The store's audit chain verifies and every read succeeded; when
    /// not, nothing is carried over (fails closed).
    usable: bool,
    /// The owner's `account_added` and `account_removed` notes on the old
    /// file, by Account id, oldest first.
    notes: BTreeMap<String, Vec<ConfigNote>>,
    /// Every History entry of the old file, oldest first.
    history: Vec<am_domain::HistoryEntry>,
}

impl ProjectRecords {
    /// Reads the records of `ids` from the project store at `db` (none when
    /// it cannot be read: nothing is then carried over).
    fn read(db: &Path, ids: &[String]) -> ProjectRecords {
        let mut out = ProjectRecords {
            usable: false,
            notes: BTreeMap::new(),
            history: Vec::new(),
        };
        if !db.is_file() {
            return out;
        }
        let Ok(reader) = am_store::Reader::open(db) else {
            return out;
        };
        let mut usable = matches!(reader.verify_audit_chain(), Ok(None));
        match reader.config_history(Some(LEGACY_ACCOUNTS_FILE)) {
            Ok(history) => out.history = history,
            Err(_) => usable = false,
        }
        for id in ids {
            let mut all = Vec::new();
            for action in ["config.widened", "config.narrowed"] {
                match reader.config_note_args(action, id) {
                    Ok(notes) => all.extend(notes),
                    Err(_) => usable = false,
                }
            }
            all.retain(|n| {
                matches!(n.actor, Actor::Human { .. })
                    && n.file == LEGACY_ACCOUNTS_FILE
                    && matches!(
                        n.args.get("kind").map(String::as_str),
                        Some("account_added" | "account_removed")
                    )
            });
            all.sort_by_key(|n| n.seq);
            out.notes.insert(id.clone(), all);
        }
        out.usable = usable;
        out
    }

    /// The live `account_added` of `id` that names exactly this provider
    /// and endpoint: its sequence number and hash. Only when every History
    /// entry of the old file from the record's own entry through the latest
    /// holds the id at that destination (a write that dropped it, a removal
    /// or a re-created file breaks it).
    fn matching(
        &self,
        id: &str,
        provider: &str,
        endpoint: &AccountEndpoint,
    ) -> Option<(i64, String)> {
        if !self.usable {
            return None;
        }
        let latest = self.notes.get(id)?.last()?;
        let arg = |k: &str| latest.args.get(k).map(String::as_str);
        let matches = arg("kind") == Some("account_added")
            && arg("provider") == Some(provider)
            && arg("scheme") == Some(endpoint.scheme)
            && arg("host") == Some(endpoint.host.as_str())
            && arg("port") == Some(endpoint.port.to_string().as_str())
            && matches!(arg("key"), None | Some("stored"));
        if !matches {
            return None;
        }
        let start = self.history.iter().position(|h| h.id == latest.entry_id)?;
        let held = self.history[start..].iter().all(|h| {
            let file = am_config::legacy_accounts(&h.after);
            file.accounts.get(id).is_some_and(|a| {
                a.provider == provider
                    && endpoint_of(&a.provider, a.base_url.as_deref(), "base_url")
                        .is_ok_and(|e| same_destination(&e, endpoint))
            })
        });
        held.then(|| (latest.seq, latest.entry_hash.clone()))
    }
}

/// The destination this Mac's list holds for `id`: `Some(Ok(endpoint))`, or
/// `Some(Err(words))` for an entry whose endpoint cannot be read.
fn user_destination(
    user: &AccountsFile,
    id: &str,
) -> Option<(String, Result<AccountEndpoint, String>)> {
    if !user.has(id) {
        return None;
    }
    let Some(account) = user.accounts.iter().find(|a| a.id == id) else {
        return Some((String::new(), Err("its entry has an error".into())));
    };
    if !user.entry_errors(id).is_empty() {
        return Some((
            account.provider.clone(),
            Err("its entry has an error".into()),
        ));
    }
    Some((
        account.provider.clone(),
        endpoint_of(&account.provider, account.base_url.as_deref(), "base_url"),
    ))
}

fn same_destination(a: &AccountEndpoint, b: &AccountEndpoint) -> bool {
    a.scheme == b.scheme && a.host == b.host && a.port == b.port
}

/// The plan: one entry per Account of the old file, in file order.
fn plan(inputs: &Inputs<'_>, did_not: &str) -> Result<Vec<Entry>, Stop> {
    let mut out = Vec::new();
    for account in inputs.legacy.accounts.accounts() {
        let id = account.id.clone();
        if inputs.only_clean && !inputs.legacy.is_clean(&id) {
            continue;
        }
        if !is_model_provider(&account.provider) {
            return refuse(format!(
                "the entry '{id}' of {LEGACY_ACCOUNTS_FILE} is a {} Account, and only model Accounts (anthropic, openai or openai-compatible) move from the command line. {did_not}",
                account.provider
            ));
        }
        let endpoint = match endpoint_of(&account.provider, account.base_url.as_deref(), "base_url")
        {
            // The dry run never repeats the refusal's words (they may quote
            // the entry's value): such an entry is named as not shown.
            Err(_) if inputs.only_clean => continue,
            other => other,
        }
        .map_err(|why| {
                Stop(
                    EXIT_ERRORS,
                    format!("the entry '{id}' of {LEGACY_ACCOUNTS_FILE} cannot be moved: {why}. {did_not}"),
                )
            })?;
        let rename = inputs
            .renames
            .iter()
            .find(|(old, _)| *old == id)
            .map(|(_, new)| new.clone());
        let same_on_list = |target: &str| {
            user_destination(inputs.user, target).is_some_and(|(provider, dest)| {
                provider == account.provider && dest.is_ok_and(|d| same_destination(&d, &endpoint))
            })
        };
        let mut entry = Entry {
            id: id.clone(),
            target: id.clone(),
            provider: account.provider.clone(),
            name: account.name.clone(),
            endpoint: endpoint.clone(),
            outcome: Outcome::Present,
        };
        if let Some((provider, dest)) = user_destination(inputs.user, &id) {
            if same_on_list(&id) {
                out.push(entry);
                continue;
            }
            match rename {
                Some(new) => {
                    entry.target = new.clone();
                    let recorded = inputs
                        .manifest_renames
                        .iter()
                        .any(|(o, n)| *o == id && *n == new);
                    entry.outcome = if same_on_list(&new) && recorded {
                        Outcome::Present
                    } else {
                        Outcome::Unconfirmed {
                            why: format!(
                                "it is added under the new id '{new}', and the Keychain entry under '{id}' belongs to the existing Account"
                            ),
                        }
                    };
                }
                None => {
                    let theirs = match dest {
                        Ok(d) => format!("provider {provider} at {}", d.goes_to()),
                        Err(why) => format!("provider {provider} ({why})"),
                    };
                    entry.outcome = Outcome::Clash { theirs };
                }
            }
            out.push(entry);
            continue;
        }
        let mut known = Vec::new();
        for action in ["config.widened", "config.narrowed"] {
            known.extend(
                (inputs.user_notes)(action, &id)
                    .map_err(|why| Stop(EXIT_ERRORS, format!("{why}. {did_not}")))?,
            );
        }
        let knows = known.iter().any(|n| {
            n.args
                .get("kind")
                .is_some_and(|k| k.starts_with("account_"))
        });
        entry.outcome = if inputs.recovered {
            Outcome::Unconfirmed {
                why: "this Mac's user store was created again after it was lost (marked recovered), so no project's confirmation is carried over".into(),
            }
        } else if knows {
            Outcome::Unconfirmed {
                why: format!(
                    "this Mac's user store already holds records of '{id}', so no project's confirmation is carried over"
                ),
            }
        } else if let Some((seq, hash)) = inputs.project.matching(&id, &account.provider, &endpoint)
        {
            Outcome::Carried { seq, hash }
        } else if !inputs.project.usable {
            Outcome::Unconfirmed {
                why: "this project's records cannot be read or its audit log does not verify, so no confirmation of it is carried over".into(),
            }
        } else {
            Outcome::Unconfirmed {
                why: "this project holds no confirmation of it for this provider and endpoint"
                    .into(),
            }
        };
        out.push(entry);
    }
    Ok(out)
}

/// `Notes` over a user-store reader that holds no store yet when `None`.
fn reader_notes(
    reader: &am_store::user::UserReader,
) -> impl Fn(&str, &str) -> Result<Vec<ConfigNote>, String> + '_ {
    move |action, target| {
        reader
            .config_note_args(action, target)
            .map_err(|e| e.to_string())
    }
}

/// The rule of `--as` (owner, 2026-11-22, AD-15 *Migrate*): only on a real
/// clash, a new id the owner typed that is an Account id, not reserved,
/// named nowhere in this project and not on this Mac's list, unless the
/// list already holds it at this entry's destination (a re-run after a
/// crash).
fn check_renames(
    root: &Path,
    base: &[Entry],
    legacy: &LegacyAccounts,
    user: &AccountsFile,
    renames: &[(String, String)],
    manifest_renames: &[(String, String)],
) -> Result<(), Stop> {
    let mut olds: Vec<&str> = Vec::new();
    let mut news: Vec<&str> = Vec::new();
    for (old, new) in renames {
        let pair = format!("--as {old}={new}");
        if olds.contains(&old.as_str()) {
            return refuse(format!("'{old}' is given to --as twice. {NOT_MOVED}"));
        }
        if news.contains(&new.as_str()) {
            return refuse(format!(
                "'{new}' is the new id of two --as renames. {NOT_MOVED}"
            ));
        }
        olds.push(old);
        news.push(new);
        let Some(entry) = base.iter().find(|e| e.id == *old) else {
            return refuse(format!(
                "`{pair}` is refused: '{old}' is not an entry of this project's {LEGACY_ACCOUNTS_FILE}; --as renames only an entry that clashes with this Mac's Accounts. {NOT_MOVED}"
            ));
        };
        if !matches!(entry.outcome, Outcome::Clash { .. }) {
            return refuse(format!(
                "`{pair}` is refused: '{old}' does not clash with this Mac's Accounts (it is {}), and --as is only for an id this Mac holds at another destination. {NOT_MOVED}",
                outcome_word(&entry.outcome)
            ));
        }
        // A re-run after a crash: the new id is there at this destination,
        // and this project's latest manifest, or the new id's own
        // `account_listed` record, records this very rename.
        let present = manifest_renames.iter().any(|(o, n)| o == old && n == new)
            && user_destination(user, new).is_some_and(|(provider, dest)| {
                provider == entry.provider
                    && dest.is_ok_and(|d| same_destination(&d, &entry.endpoint))
            });
        if present {
            continue;
        }
        if !am_config::is_config_id(new) {
            return refuse(format!(
                "`{pair}` is refused: '{new}' is not an Account id; use lowercase letters, digits and single hyphens (kebab-case), for example `{old}-old`. {NOT_MOVED}"
            ));
        }
        if reserved(new) {
            return refuse(format!(
                "`{pair}` is refused: '{new}' is a reserved local Account id. {NOT_MOVED}"
            ));
        }
        let uses = am_config::account_uses(root, new);
        if !uses.is_empty()
            || legacy
                .ids
                .iter()
                .any(|i| i.as_deref() == Some(new.as_str()))
        {
            let where_ = if uses.is_empty() {
                format!("an entry of {LEGACY_ACCOUNTS_FILE}")
            } else {
                uses.iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            return refuse(format!(
                "`{pair}` is refused: '{new}' is already named in this project ({}); choose an id named nowhere in it. {NOT_MOVED}",
                shown(&where_)
            ));
        }
        if user.has(new) {
            return refuse(format!(
                "`{pair}` is refused: this Mac's Accounts already hold '{new}' (and no earlier run of this migrate added it there); choose an id this Mac does not have. {NOT_MOVED}"
            ));
        }
    }
    Ok(())
}

fn outcome_word(outcome: &Outcome) -> &'static str {
    match outcome {
        Outcome::Present => "present",
        Outcome::Clash { .. } => "clash",
        Outcome::Carried { .. } => "carried over",
        Outcome::Unconfirmed { .. } => "unconfirmed",
    }
}

/// The refusal of a clash without `--as`, naming both destinations.
fn clash_refusal(entries: &[Entry]) -> Result<(), Stop> {
    let clashes: Vec<String> = entries
        .iter()
        .filter_map(|e| match &e.outcome {
            Outcome::Clash { theirs } => Some(format!(
                "'{}' in this project's {LEGACY_ACCOUNTS_FILE} goes to {}, and this Mac's Accounts hold '{}' at {theirs}; pass `--as {}=<new-id>` to add this project's one under a new id of your choosing (its references in this project are renamed), or rename one of them first",
                e.id,
                e.destination(),
                e.id,
                e.id
            )),
            _ => None,
        })
        .collect();
    if clashes.is_empty() {
        return Ok(());
    }
    refuse(format!(
        "{}. The whole command is refused. {NOT_MOVED}",
        clashes.join("; ")
    ))
}

// ------------------------------------------------------------ the sentences

/// The references of one id in the projects this Mac knows.
fn named(scan: &Scan) -> String {
    if scan.uses.is_empty() {
        "Nothing in the projects this Mac knows names it.".to_string()
    } else {
        format!("Named by {}.", scan.uses_words())
    }
}

/// One entry's sentence (one line, the id and its outcome first).
fn sentence(e: &Entry, scan: &Scan, renamed: &[RenamedFile], theirs: Option<&str>) -> String {
    let checked = not_checked(scan);
    match &e.outcome {
        Outcome::Present if e.renamed() => format!(
            "'{}' as '{}' (provider {}): present. This Mac's Accounts already hold '{}' at {}, added by an earlier run of this migrate (its manifest records the rename); nothing to add. Renamed in this project, each keeping its destination: {} (each now names '{}'). A template, another project or another Mac that names '{}' will reach the existing '{}' at its own destination, not this one, and this project's exports will carry '{}'. {}{checked}",
            e.id,
            e.target,
            e.provider,
            e.target,
            e.endpoint.goes_to(),
            renamed_lines(renamed),
            e.target,
            e.id,
            e.id,
            e.target,
            named(scan)
        ),
        Outcome::Present => format!(
            "'{}' ({}): present. This Mac's Accounts already hold '{}' at the same provider and endpoint; nothing to add.",
            e.id,
            e.destination(),
            e.target
        ),
        Outcome::Clash { theirs } => format!(
            "'{}' ({}): clash. This Mac's Accounts hold '{}' at {theirs}; `migrate` is refused unless `--as {}=<new-id>` adds this one under a new id.",
            e.id,
            e.destination(),
            e.id,
            e.id
        ),
        Outcome::Carried { seq, .. } => format!(
            "'{}' (provider {}): carried over. Moving the Account '{}' to this Mac's Accounts, for every project: its calls go to {}. This project's confirmation of it for that provider and endpoint (its audit entry {seq}) is carried over, so the key stored under '{}' in this Mac's Keychain, if one exists, will be used for that destination; no key is read, asked for or stored now. {}{checked}",
            e.id,
            e.provider,
            e.id,
            e.endpoint.goes_to(),
            e.id,
            named(scan)
        ),
        Outcome::Unconfirmed { why } if e.renamed() => {
            let lines = renamed_lines(renamed);
            format!(
                "'{}' as '{}' (provider {}): unconfirmed. This project's '{}' goes to {}, and this Mac's Accounts hold '{}' at {}; with --as it is added to this Mac's Accounts as '{}' at {}, unconfirmed ({why}): no key will be used for it until you confirm it with `agentic accounts change {}` (a new key typed then). Renamed in this project, each keeping its destination: {lines} (each now names '{}'). A template, another project or another Mac that names '{}' will reach the existing '{}' at its own destination, not this one, and this project's exports will carry '{}'. {}{checked}",
                e.id,
                e.target,
                e.provider,
                e.id,
                e.endpoint.goes_to(),
                e.id,
                theirs.unwrap_or("another destination"),
                e.target,
                e.endpoint.goes_to(),
                e.target,
                e.target,
                e.id,
                e.id,
                e.target,
                named(scan)
            )
        }
        Outcome::Unconfirmed { why } => format!(
            "'{}' (provider {}): unconfirmed. Moving the Account '{}' to this Mac's Accounts, for every project: its calls go to {}. It enters this Mac's list unconfirmed ({why}): no key will be used for it until you confirm it with `agentic accounts change {}` (a new key typed then). {}{checked}",
            e.id,
            e.provider,
            e.id,
            e.endpoint.goes_to(),
            e.target,
            named(scan)
        ),
    }
}

/// The lines `--as` renames in this project, in words.
fn renamed_lines(renamed: &[RenamedFile]) -> String {
    let lines: Vec<String> = renamed
        .iter()
        .map(|r| {
            let at: Vec<String> = r.lines.iter().map(u32::to_string).collect();
            format!("{} line {}", shown(&r.file), at.join(", "))
        })
        .collect();
    if lines.is_empty() {
        "nothing in this project names it".to_string()
    } else {
        lines.join("; ")
    }
}

/// The user file's audit note for a new Account (AD-15 *Record*).
fn note(e: &Entry, project: &ProjectId, references: &[String]) -> AuditNote {
    let mut args = BTreeMap::from([
        ("file".to_string(), USER_ACCOUNTS_FILE.to_string()),
        ("account".to_string(), e.target.clone()),
        ("provider".to_string(), e.provider.clone()),
        ("scheme".to_string(), e.endpoint.scheme.to_string()),
        ("host".to_string(), e.endpoint.host.clone()),
        ("port".to_string(), e.endpoint.port.to_string()),
        ("references".to_string(), references.join(", ")),
        ("project".to_string(), project.to_string()),
    ]);
    match &e.outcome {
        Outcome::Carried { seq, hash } => {
            args.insert("kind".into(), "account_added".into());
            args.insert("key".into(), "stored".into());
            args.insert("confirmation".into(), "migrated".into());
            args.insert("source_seq".into(), seq.to_string());
            args.insert("source_hash".into(), hash.clone());
        }
        _ => {
            args.insert("kind".into(), "account_listed".into());
            args.insert("confirmation".into(), "none".into());
        }
    }
    AuditNote {
        action: "config.widened".into(),
        target: e.target.clone(),
        args,
    }
}

/// The scans of every id the entries name on this Mac's list (references
/// and projects not checked), and the children that run as the owner.
fn scans(places: &Places, entries: &[Entry]) -> BTreeMap<String, Scan> {
    let mut out = BTreeMap::new();
    for e in entries {
        out.entry(e.target.clone())
            .or_insert_with(|| crate::user::scan(places, &e.target));
    }
    out.entry(String::new())
        .or_insert_with(|| crate::user::scan(places, ""));
    out
}

/// Refuses while this project or any reachable project enables a child
/// that runs as the owner (AD-13, AD-15).
fn no_children(root: &Path, scan: &Scan, what: &str, did_not: &str) -> Result<(), Stop> {
    let own = am_config::owner_children(root).account_words();
    let found = own
        .map(|why| (root.to_path_buf(), why))
        .or_else(|| scan.children.first().cloned());
    let Some((project, why)) = found else {
        return Ok(());
    };
    refuse(format!(
        "`agentic accounts {what}` cannot be used from the command line while the project {} has {}: a program that runs as you could type here. {did_not}; remove or disable it first",
        shown(&project.display().to_string()),
        shown(&why)
    ))
}

/// Refuses while a file of the project has an outside change not reloaded
/// (AD-16).
fn no_outside_change(opened: &crate::runtime::OpenedUnchanged, did_not: &str) -> Result<(), Stop> {
    let history = am_store::StoreConfigHistory::new(&opened.store)
        .map_err(|e| Stop(EXIT_ERRORS, format!("{e}. {did_not}")))?;
    let changed = am_config::outside_changes(&opened.root, &history)
        .map_err(|e| Stop(EXIT_ERRORS, format!("{e}. {did_not}")))?;
    if changed.is_empty() {
        return Ok(());
    }
    refuse(format!(
        "{} in the project {} changed on disk outside the app and the change is not reloaded, so this command writes nothing over it. {did_not}. Put each file back as the app last wrote it, or open the project with `agentic serve --project {}` and Reload, then run this command again",
        shown(&changed.join(", ")),
        shown(&opened.root.display().to_string()),
        shown(&opened.root.display().to_string())
    ))
}

/// The old file's errors as a refusal (no value from the file is shown).
fn legacy_problems(legacy: &LegacyAccounts, root: &Path, did_not: &str) -> Result<(), Stop> {
    let file = shown(&root.join(LEGACY_ACCOUNTS_FILE).display().to_string());
    if !legacy.secret_lines.is_empty() {
        let lines: Vec<String> = legacy.secret_lines.iter().map(u32::to_string).collect();
        return refuse(format!(
            "{file} holds a secret-looking value (line {}), and no key may reach this Mac's list, History or a backup; remove it from the file by hand (keys live in the Keychain), then run this command again. {did_not}",
            lines.join(", ")
        ));
    }
    if !legacy.errors.is_empty() {
        return refuse(format!(
            "{file} has errors ({}); fix them by hand, then run this command again. {did_not}",
            shown(&legacy.errors.join("; "))
        ));
    }
    Ok(())
}

/// One typed confirmation, echoed, within 2 minutes.
fn typed(ctx: &mut AccountsContext, expect: &str, what: &str, did_not: &str) -> Result<(), Stop> {
    match ctx.terminal.read_line(Some(CONFIRM_WITHIN)) {
        Ok(line) if line.trim() == expect => Ok(()),
        Ok(_) => refuse(format!("what was typed is not {what}. {did_not}")),
        Err(TerminalError::TimedOut) => {
            ctx.terminal.show("\n");
            refuse(format!("{what} was not typed within 2 minutes. {did_not}"))
        }
        Err(TerminalError::Interrupted) => {
            ctx.terminal.show("\n");
            Err(interrupted(did_not))
        }
        Err(e) => refuse(format!("{what} was not typed ({e}). {did_not}")),
    }
}

fn read_legacy(root: &Path, did_not: &str) -> Result<Option<Vec<u8>>, Stop> {
    match std::fs::read(root.join(LEGACY_ACCOUNTS_FILE)) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => refuse(format!(
            "{} cannot be read ({e}). {did_not}",
            shown(&root.join(LEGACY_ACCOUNTS_FILE).display().to_string())
        )),
    }
}

fn nothing_to_move(root: &Path, out: &mut dyn Write) {
    let _ = writeln!(
        out,
        "agentic: nothing to move: the project {} has no {LEGACY_ACCOUNTS_FILE}; its Accounts are on this Mac's list already (`agentic accounts list`)",
        shown(&root.display().to_string())
    );
}

// ----------------------------------------------------------------- dry run

/// `--dry-run`: lists each entry with its outcome and writes nothing; it
/// takes no lock and runs from any stdin.
fn dry_run(ctx: &AccountsContext, project: &Path, out: &mut dyn Write) -> Result<(), Stop> {
    const DRY: &str = "Nothing was written";
    let places = &ctx.places;
    let root = project.canonicalize().map_err(|_| {
        Stop(
            EXIT_ERRORS,
            format!(
                "the project folder {} does not exist",
                shown(&project.display().to_string())
            ),
        )
    })?;
    let layout = crate::runtime::find_runtime(places, &root)
        .map_err(|r| Stop(EXIT_ERRORS, format!("{r}. {DRY}")))?;
    let Some(bytes) = read_legacy(&root, DRY)? else {
        nothing_to_move(&root, out);
        return Ok(());
    };
    let legacy = am_config::legacy_accounts(&bytes);
    let standing =
        Standing::read(places).map_err(|why| Stop(EXIT_ERRORS, format!("{why}. {DRY}")))?;
    let recovered =
        crate::user::recovered(places).map_err(|why| Stop(EXIT_ERRORS, format!("{why}. {DRY}")))?;
    let user_reader =
        crate::user::reader(places).map_err(|why| Stop(EXIT_ERRORS, format!("{why}. {DRY}")))?;
    let notes = reader_notes(&user_reader);
    let ids = legacy.accounts.ids();
    let records = ProjectRecords::read(&layout.state_db(), &ids);
    let project_id = am_config::read_company(&root)
        .ok()
        .and_then(|h| h.project_id);
    let recorded = project_id
        .map(|id| recorded_renames(places, &id))
        .unwrap_or_default();
    // Only the entries with no error and no secret-looking value are
    // planned and shown; the others are named by their line alone.
    let entries = plan(
        &Inputs {
            legacy: &legacy,
            user: &standing.file,
            renames: &[],
            recovered,
            user_notes: &notes,
            project: &records,
            manifest_renames: &recorded,
            only_clean: true,
        },
        DRY,
    )?;
    let _ = writeln!(
        out,
        "agentic: dry run of `agentic accounts migrate --project {}` ({} on this Mac); nothing is written:",
        shown(&root.display().to_string()),
        shown(&places.user.accounts_file().display().to_string())
    );
    for e in &entries {
        let _ = writeln!(
            out,
            "  {}  {}  {}  {}",
            e.id,
            e.provider,
            e.endpoint.host_shown(),
            match &e.outcome {
                Outcome::Present =>
                    "present: already on this Mac's list at the same destination".to_string(),
                Outcome::Clash { theirs } => format!(
                    "clash: this Mac's list holds '{}' at {theirs}; migrate needs `--as {}=<new-id>`",
                    e.id, e.id
                ),
                Outcome::Carried { seq, .. } => format!(
                    "carried over: this project's confirmation (audit entry {seq}) moves with it"
                ),
                Outcome::Unconfirmed { why } => format!(
                    "unconfirmed ({why}): no key is used until `agentic accounts change {}`",
                    e.target
                ),
            }
        );
    }
    let planned_ids: Vec<&str> = entries.iter().map(|e| e.id.as_str()).collect();
    for e in legacy.entries.iter().filter(|e| {
        !e.clean
            || e.id
                .as_deref()
                .is_none_or(|id| !planned_ids.contains(&id) || !legacy.is_clean(id))
    }) {
        let which = match &e.id {
            Some(id) => format!("'{id}' (line {})", e.line),
            None => format!("at line {}", e.line),
        };
        let _ = writeln!(
            out,
            "  the entry {which}: its values are not shown (it has an error or a secret-looking value)"
        );
    }
    if !legacy.secret_lines.is_empty() || !legacy.errors.is_empty() {
        let mut lines: Vec<u32> = legacy
            .secret_lines
            .iter()
            .chain(&legacy.error_lines)
            .copied()
            .collect();
        lines.sort_unstable();
        lines.dedup();
        let lines: Vec<String> = lines.iter().map(u32::to_string).collect();
        let _ = writeln!(
            out,
            "agentic: migrate refuses {LEGACY_ACCOUNTS_FILE} until it is fixed by hand: it has an error or a secret-looking value (line {}); `agentic validate` names each error",
            lines.join(", ")
        );
    }
    if entries
        .iter()
        .all(|e| !e.is_new() && e.outcome == Outcome::Present)
    {
        let _ = writeln!(
            out,
            "agentic: every entry is present: migrate only removes {LEGACY_ACCOUNTS_FILE} (after a backup)"
        );
    }
    Ok(())
}

// ----------------------------------------------------------------- migrate

/// What the sentences were built on, read again under `user.lock`.
struct Shown {
    standing: Standing,
    entries: Vec<Entry>,
    scans: BTreeMap<String, Scan>,
    bytes: Vec<u8>,
}

fn migrate(
    mut ctx: AccountsContext,
    project: &Path,
    renames: &[(String, String)],
    out: &mut dyn Write,
) -> Result<(), Stop> {
    require_terminal(&ctx, "migrate", NOT_MOVED)?;
    let places = ctx.places.clone();
    let opened = crate::runtime::open_project_unchanged(&places, ctx.identity.as_ref(), project)
        .map_err(|r| Stop(EXIT_ERRORS, format!("{r}. {NOT_MOVED}")))?;
    for line in &opened.lines {
        let _ = writeln!(out, "agentic: {line}");
    }
    let root = opened.root.clone();
    let root_shown = shown(&root.display().to_string());
    no_links(&root, NOT_MOVED)?;
    let all = crate::user::scan(&places, "");
    no_children(&root, &all, "migrate", NOT_MOVED)?;
    no_outside_change(&opened, NOT_MOVED)?;
    let standing = stand(&places, NOT_MOVED)?;
    let Some(bytes) = read_legacy(&root, NOT_MOVED)? else {
        nothing_to_move(&root, out);
        return Ok(());
    };
    let legacy = am_config::legacy_accounts(&bytes);
    legacy_problems(&legacy, &root, NOT_MOVED)?;
    let recovered = crate::user::recovered(&places)
        .map_err(|why| Stop(EXIT_ERRORS, format!("{why}. {NOT_MOVED}")))?;
    let user_reader = crate::user::reader(&places)
        .map_err(|why| Stop(EXIT_ERRORS, format!("{why}. {NOT_MOVED}")))?;
    let notes = reader_notes(&user_reader);
    let ids = legacy.accounts.ids();
    let records = ProjectRecords::read(&opened.layout.state_db(), &ids);
    let mut recorded = recorded_renames(&places, &opened.project_id);
    recorded.extend(
        listed_renames(renames, &legacy, &notes, &opened.project_id)
            .map_err(|why| Stop(EXIT_ERRORS, format!("{why}. {NOT_MOVED}")))?,
    );
    let base = plan(
        &Inputs {
            legacy: &legacy,
            user: &standing.file,
            renames: &[],
            recovered,
            user_notes: &notes,
            project: &records,
            manifest_renames: &recorded,
            only_clean: false,
        },
        NOT_MOVED,
    )?;
    check_renames(&root, &base, &legacy, &standing.file, renames, &recorded)?;
    let entries = plan(
        &Inputs {
            legacy: &legacy,
            user: &standing.file,
            renames,
            recovered,
            user_notes: &notes,
            project: &records,
            manifest_renames: &recorded,
            only_clean: false,
        },
        NOT_MOVED,
    )?;
    clash_refusal(&entries)?;
    let scans = scans(&places, &entries);
    // The renames, as they would be written (named in the sentence and
    // recorded in the manifest).
    let pairs: Vec<(String, String)> = entries
        .iter()
        .filter(|e| e.renamed())
        .map(|e| (e.id.clone(), e.target.clone()))
        .collect();
    let mut renamed_by: BTreeMap<String, Vec<RenamedFile>> = BTreeMap::new();
    for (old, new) in &pairs {
        let files = am_config::account_renames(&root, &[(old.clone(), new.clone())])
            .map_err(|e| Stop(EXIT_ERRORS, format!("{}. {NOT_MOVED}", write_words(&e))))?;
        renamed_by.insert(old.clone(), files);
    }
    // The whole rename, planned in order as the writer applies it: its
    // before and after bytes are what the manifest records and what the
    // writer must still find.
    let planned = am_config::account_renames(&root, &pairs)
        .map_err(|e| Stop(EXIT_ERRORS, format!("{}. {NOT_MOVED}", write_words(&e))))?;
    // The sentences, and each new Account's id typed.
    ctx.terminal.show(&format!(
        "Moving the Accounts of {} to this Mac's Accounts ({}). Each new one is added only once you type its id; then {LEGACY_ACCOUNTS_FILE} is backed up to {} and removed from the project (`agentic accounts migrate --project {root_shown} --rollback` puts it back). No key is read, asked for or stored.\n",
        shown(&root.join(LEGACY_ACCOUNTS_FILE).display().to_string()),
        shown(&places.user.accounts_file().display().to_string()),
        shown(&places.user.accounts_migrate_backups(&opened.project_id).display().to_string()),
    ));
    for e in &entries {
        let scan = scans.get(&e.target).cloned().unwrap_or_default();
        let theirs = user_destination(&standing.file, &e.id).map(|(provider, dest)| match dest {
            Ok(d) => format!("provider {provider} at {}", d.goes_to()),
            Err(why) => format!("provider {provider} ({why})"),
        });
        let renamed = renamed_by.get(&e.id).cloned().unwrap_or_default();
        ctx.terminal.show(&format!(
            "{}\n",
            sentence(e, &scan, &renamed, theirs.as_deref())
        ));
        if e.is_new() || e.renamed() {
            ctx.terminal.show(&format!(
                "To confirm, type the Account's id ({}) within 2 minutes: ",
                e.target
            ));
            typed(
                &mut ctx,
                &e.target,
                &format!("the Account's id '{}'", e.target),
                NOT_MOVED,
            )?;
        }
    }
    if ctx.terminal.interrupted() {
        return Err(interrupted(NOT_MOVED));
    }
    let shown_then = Shown {
        standing,
        entries,
        scans,
        bytes,
    };
    // Under `user.lock`: everything again on a fresh read, then one
    // user-store transaction per new Account.
    let written = {
        let locked = super::lock(&places, NOT_MOVED, out)?;
        let (entries, scans) = still_as_shown(&places, &locked, &opened, renames, &shown_then)?;
        if ctx.terminal.interrupted() {
            return Err(interrupted(NOT_MOVED));
        }
        let mut written: Vec<String> = Vec::new();
        for e in entries.iter().filter(|e| e.is_new()) {
            let references: Vec<String> = scans
                .get(&e.target)
                .map(|s| {
                    s.uses
                        .iter()
                        .map(|(root, u)| format!("{}:{u}", root.display()))
                        .collect()
                })
                .unwrap_or_default();
            let note = note(e, &opened.project_id, &references);
            let file = am_config::accounts_file(places.user.dir());
            let result = match (&file.sha256, file.exists) {
                (Some(built_on), true) => locked.writer.apply_with_audit(
                    owner(),
                    am_config::add_account_edit(
                        &file,
                        &e.target,
                        &e.provider,
                        e.name.as_deref(),
                        e.endpoint.written(),
                    ),
                    built_on,
                    vec![note],
                ),
                _ => am_config::new_accounts_file(
                    &e.target,
                    &e.provider,
                    e.name.as_deref(),
                    e.endpoint.written(),
                )
                .and_then(|bytes| {
                    locked.writer.create(
                        owner(),
                        ConfigCreate {
                            file: USER_ACCOUNTS_FILE.into(),
                            bytes,
                            audit: vec![note],
                        },
                    )
                }),
            };
            if let Err(err) = result {
                let done = if written.is_empty() {
                    "No Account was added".to_string()
                } else {
                    format!(
                        "Already added to this Mac's Accounts: {} (a run again finds them present)",
                        written.join(", ")
                    )
                };
                return refuse(format!(
                    "the Account '{}' was not added: {}. {done}; {LEGACY_ACCOUNTS_FILE} was not removed. Run the command again",
                    e.target,
                    write_words(&err)
                ));
            }
            written.push(format!("{} ({})", e.target, outcome_word(&e.outcome)));
        }
        written
    };
    let entries = &shown_then.entries;
    let bytes = &shown_then.bytes;
    let added = if written.is_empty() {
        "no Account needed adding".to_string()
    } else {
        format!("added {}", written.join(", "))
    };
    // The backup, then its checks, then the removal.
    let backup = write_backup(
        &places,
        &opened.project_id,
        bytes,
        &pairs,
        &renamed_by,
        &planned,
    )
    .map_err(|why| {
        Stop(
            EXIT_ERRORS,
            format!(
                "this Mac's Accounts are updated ({added}), but the backup of {LEGACY_ACCOUNTS_FILE} could not be written ({why}), so the file was not removed; run the command again to finish (the Accounts count as present)"
            ),
        )
    })?;
    #[cfg(feature = "test-support")]
    crate::runtime::testing::run_before_backup_check(&root);
    let parsed = sha256_hex(bytes);
    let copy_ok = std::fs::read(&backup.0).is_ok_and(|b| sha256_hex(&b) == parsed);
    let file_ok =
        std::fs::read(root.join(LEGACY_ACCOUNTS_FILE)).is_ok_and(|b| sha256_hex(&b) == parsed);
    if !copy_ok || !file_ok {
        let which = if file_ok {
            "the backup copy"
        } else {
            "the project's file"
        };
        return refuse(format!(
            "{which} no longer holds the bytes this command read from {LEGACY_ACCOUNTS_FILE}, so nothing was removed. This Mac's Accounts are updated ({added}); run the command again to finish (the Accounts count as present)"
        ));
    }
    let mut steps: Vec<MigrationStep> = pairs
        .iter()
        .map(|(old, new)| MigrationStep::RenameAccount {
            old: old.clone(),
            new: new.clone(),
        })
        .collect();
    // The exact files the renames may write, as shown (empty included).
    steps.push(MigrationStep::Expects {
        files: planned
            .iter()
            .map(|r| (r.file.clone(), r.before_sha256()))
            .collect(),
    });
    steps.push(MigrationStep::Remove {
        file: LEGACY_ACCOUNTS_FILE.into(),
        before_sha256: parsed,
    });
    let history = opened
        .writer
        .apply_migration(owner(), ConfigMigration { steps })
        .map_err(|e| {
            Stop(
                EXIT_ERRORS,
                format!(
                    "this Mac's Accounts are updated ({added}) and {LEGACY_ACCOUNTS_FILE} is backed up to {}, but the project's migration did not finish: {}. Run `agentic accounts migrate --project {root_shown}` again to finish (the Accounts count as present)",
                    shown(&backup.0.display().to_string()),
                    write_words(&e)
                ),
            )
        })?;
    let removal = history
        .iter()
        .find(|h| h.file == LEGACY_ACCOUNTS_FILE)
        .map_or_else(String::new, |h| format!(" (History entry {})", h.id));
    let renamed: Vec<String> = history
        .iter()
        .filter(|h| h.file != LEGACY_ACCOUNTS_FILE)
        .map(|h| shown(&h.file))
        .collect();
    let renamed = if renamed.is_empty() {
        String::new()
    } else {
        format!(" Renamed in this project: {}.", renamed.join(", "))
    };
    let unconfirmed: Vec<&str> = entries
        .iter()
        .filter(|e| matches!(e.outcome, Outcome::Unconfirmed { .. }))
        .map(|e| e.target.as_str())
        .collect();
    let confirm = if unconfirmed.is_empty() {
        String::new()
    } else {
        format!(
            " Unconfirmed, so no key is used until each is confirmed with `agentic accounts change <id>`: {}.",
            unconfirmed.join(", ")
        )
    };
    let _ = writeln!(
        out,
        "agentic: moved the Accounts of {root_shown} to this Mac's Accounts ({}): {added}.{confirm}{renamed} Removed {LEGACY_ACCOUNTS_FILE}{removal}; backup: {} (`agentic accounts migrate --project {root_shown} --rollback` puts it back)",
        shown(&places.user.accounts_file().display().to_string()),
        shown(&backup.0.display().to_string())
    );
    Ok(())
}

/// Under `user.lock`: the user file, the ids and their records, the old
/// file's bytes, the references and the children read again; anything that
/// changed since the sentences refuses, to be run again. Returns the fresh
/// plan and scans (equal to what was shown).
fn still_as_shown(
    places: &Places,
    locked: &crate::user::Locked,
    opened: &crate::runtime::OpenedUnchanged,
    renames: &[(String, String)],
    then: &Shown,
) -> Result<(Vec<Entry>, BTreeMap<String, Scan>), Stop> {
    let root = opened.root.as_path();
    let again = format!("{NOT_MOVED}; run the command again");
    let changed = |what: &str| -> Result<(), Stop> {
        refuse(format!(
            "{what} changed while this command waited at its prompts, so what you confirmed is not what would be written. {again}"
        ))
    };
    let now = locked
        .standing(places)
        .map_err(|why| Stop(EXIT_ERRORS, format!("{why}. {NOT_MOVED}")))?;
    if now.outside_change() {
        return refuse(crate::user::outside_words(places, NOT_MOVED));
    }
    // The project as shown: no link, no outside change, the old file's
    // bytes.
    no_links(root, NOT_MOVED)?;
    no_outside_change(opened, NOT_MOVED)?;
    if read_legacy(root, NOT_MOVED)?.as_deref() != Some(then.bytes.as_slice()) {
        changed(&format!("the project's {LEGACY_ACCOUNTS_FILE}"))?;
    }
    for e in &then.entries {
        for id in [&e.id, &e.target] {
            if !then.standing.still(&now, id, then.standing.file.has(id)) {
                changed(&format!("this Mac's {USER_ACCOUNTS_FILE}"))?;
            }
        }
    }
    let legacy = am_config::legacy_accounts(&then.bytes);
    let guard = locked.history.clone();
    let notes = move |action: &str, target: &str| {
        guard
            .config_note_args(action, target)
            .map_err(|e| e.to_string())
    };
    let recovered = locked
        .history
        .is_recovered()
        .map_err(|e| Stop(EXIT_ERRORS, format!("{e}. {NOT_MOVED}")))?;
    let records = ProjectRecords::read(&opened.layout.state_db(), &legacy.accounts.ids());
    let mut recorded = recorded_renames(places, &opened.project_id);
    recorded.extend(
        listed_renames(renames, &legacy, &notes, &opened.project_id)
            .map_err(|why| Stop(EXIT_ERRORS, format!("{why}. {NOT_MOVED}")))?,
    );
    let mut inputs = Inputs {
        legacy: &legacy,
        user: &now.file,
        renames: &[],
        recovered,
        user_notes: &notes,
        project: &records,
        manifest_renames: &recorded,
        only_clean: false,
    };
    // `--as`'s rule again: the new id may have been named meanwhile.
    let base = plan(&inputs, NOT_MOVED)?;
    check_renames(root, &base, &legacy, &now.file, renames, &recorded)?;
    inputs.renames = renames;
    let entries = plan(&inputs, NOT_MOVED)?;
    if entries != then.entries {
        changed("this Mac's Accounts or their records (or this project's records)")?;
    }
    let scans = scans(places, &entries);
    let all = scans.get("").cloned().unwrap_or_default();
    no_children(root, &all, "migrate", NOT_MOVED)?;
    for (id, scan) in &scans {
        let before = then.scans.get(id);
        if before.map(|s| (&s.uses, &s.unreachable)) != Some((&scan.uses, &scan.unreachable)) {
            changed(&format!("what names the Account '{id}'"))?;
        }
    }
    Ok((entries, scans))
}

// ------------------------------------------------------------------ backup

/// Writes `<ts>.yaml` (the parsed bytes) and `<ts>.manifest.yaml` (0600)
/// in the project's backup folder (0700), each fsynced, and returns their
/// paths. A name already taken gets `~<n>`, which sorts after it.
fn write_backup(
    places: &Places,
    project: &ProjectId,
    bytes: &[u8],
    pairs: &[(String, String)],
    renamed_by: &BTreeMap<String, Vec<RenamedFile>>,
    planned: &[RenamedFile],
) -> Result<(PathBuf, PathBuf), String> {
    let dir = places.user.accounts_migrate_backups(project);
    for d in [
        places
            .user
            .accounts_migrate_backups_root()
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default(),
        places.user.accounts_migrate_backups_root(),
        dir.clone(),
    ] {
        crate::runtime::ensure_private_dir(&d).map_err(|r| r.0)?;
    }
    let stamp = jiff::Timestamp::now()
        .strftime("%Y%m%dT%H%M%S%.6fZ")
        .to_string();
    let backup_sha = sha256_hex(bytes);
    let mut n = 1;
    let (backup, manifest, name) = loop {
        let stem = if n == 1 {
            stamp.clone()
        } else {
            format!("{stamp}~{n}")
        };
        let backup = dir.join(format!("{stem}.yaml"));
        let manifest = dir.join(format!("{stem}.manifest.yaml"));
        if !backup.exists() && !manifest.exists() {
            break (backup, manifest, format!("{stem}.yaml"));
        }
        if n >= 100 {
            return Err(format!("no free backup name in {}", dir.display()));
        }
        n += 1;
    };
    write_new(&backup, bytes)?;
    let mut text = format!(
        "schema_version: 1\nproject_id: {project}\nfile: {LEGACY_ACCOUNTS_FILE}\nbackup: {name}\nbackup_sha256: {backup_sha}\ncreated_at: {}\n",
        am_store::ts(jiff::Timestamp::now())
    );
    if pairs.is_empty() {
        text.push_str("renames: []\n");
    } else {
        text.push_str("renames:\n");
        for (old, new) in pairs {
            text.push_str(&format!("  - old: {old}\n    new: {new}\n"));
            let files = renamed_by.get(old).cloned().unwrap_or_default();
            if files.is_empty() {
                text.push_str("    files: []\n");
                continue;
            }
            text.push_str("    files:\n");
            for f in files {
                // The hashes of the whole rename as written, not of this
                // pair's preview on its own.
                let f = planned
                    .iter()
                    .find(|p| p.file == f.file)
                    .map(|p| RenamedFile {
                        lines: f.lines.clone(),
                        ..p.clone()
                    })
                    .unwrap_or(f);
                let lines: Vec<String> = f.lines.iter().map(u32::to_string).collect();
                text.push_str(&format!(
                    "      - file: \"{}\"\n        lines: [{}]\n        before_sha256: {}\n        after_sha256: {}\n",
                    f.file.replace('\\', "\\\\").replace('"', "\\\""),
                    lines.join(", "),
                    f.before_sha256(),
                    f.after_sha256()
                ));
            }
        }
    }
    write_new(&manifest, text.as_bytes())?;
    std::fs::File::open(&dir)
        .and_then(|d| d.sync_all())
        .map_err(|e| format!("cannot sync {}: {e}", dir.display()))?;
    Ok((backup, manifest))
}

/// A new owner-only file holding `bytes`, fsynced; never over a file.
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut f = crate::runtime::private_options()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("cannot create {}: {e}", path.display()))?;
    f.write_all(bytes)
        .and_then(|()| f.sync_all())
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// The latest backup of a project and what its manifest records.
struct Backup {
    path: PathBuf,
    bytes: Vec<u8>,
    /// The ids the manifest lists as renamed by `--as` (`old`, `new`).
    renames: Vec<(String, String)>,
}

/// Reads the latest backup of `project` and checks it against its
/// manifest; refused in words when there is none, a half is missing, or
/// the hash differs.
fn latest_backup(places: &Places, project: &ProjectId, did_not: &str) -> Result<Backup, Stop> {
    let dir = places.user.accounts_migrate_backups(project);
    let none = || {
        refuse(format!(
            "there is no backup of this project's {LEGACY_ACCOUNTS_FILE} on this Mac (`agentic accounts migrate` writes one in {} before it removes the file), so there is nothing to roll back to. {did_not}",
            shown(&dir.display().to_string())
        ))
    };
    let Ok(read) = std::fs::read_dir(&dir) else {
        return none();
    };
    let mut stems: BTreeMap<String, (bool, bool)> = BTreeMap::new();
    for e in read.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if let Some(stem) = name.strip_suffix(".manifest.yaml") {
            stems.entry(stem.to_string()).or_default().1 = true;
        } else if let Some(stem) = name.strip_suffix(".yaml") {
            stems.entry(stem.to_string()).or_default().0 = true;
        }
    }
    let Some((stem, (has_backup, has_manifest))) = stems.into_iter().next_back() else {
        return none();
    };
    let path = dir.join(format!("{stem}.yaml"));
    let manifest_path = dir.join(format!("{stem}.manifest.yaml"));
    if !has_manifest {
        return refuse(format!(
            "the manifest of the latest backup ({}) is missing, so the backup cannot be checked. {did_not}",
            shown(&manifest_path.display().to_string())
        ));
    }
    if !has_backup {
        return refuse(format!(
            "the latest backup ({}) is missing beside its manifest. {did_not}",
            shown(&path.display().to_string())
        ));
    }
    let bytes = std::fs::read(&path).map_err(|e| {
        Stop(
            EXIT_ERRORS,
            format!(
                "the backup {} cannot be read ({e}). {did_not}",
                shown(&path.display().to_string())
            ),
        )
    })?;
    let manifest = std::fs::read_to_string(&manifest_path).map_err(|e| {
        Stop(
            EXIT_ERRORS,
            format!(
                "the manifest {} cannot be read ({e}). {did_not}",
                shown(&manifest_path.display().to_string())
            ),
        )
    })?;
    let field = |key: &str| {
        manifest.lines().find_map(|l| {
            l.strip_prefix(key)
                .and_then(|rest| rest.strip_prefix(": "))
                .map(str::trim)
        })
    };
    let fits = field("backup_sha256") == Some(sha256_hex(&bytes).as_str())
        && field("project_id") == Some(project.to_string().as_str())
        && field("backup") == Some(format!("{stem}.yaml").as_str());
    if !fits {
        return refuse(format!(
            "the latest backup {} does not match the hash its manifest records (it was changed or replaced), so it is not restored. {did_not}",
            shown(&path.display().to_string())
        ));
    }
    let renames = manifest_renames(&manifest);
    Ok(Backup {
        path,
        bytes,
        renames,
    })
}

/// The `--as` renames a manifest lists (`old`, `new`).
fn manifest_renames(manifest: &str) -> Vec<(String, String)> {
    let mut renames = Vec::new();
    let mut old: Option<String> = None;
    for line in manifest.lines() {
        if let Some(id) = line.strip_prefix("  - old: ") {
            old = Some(id.trim().to_string());
        } else if let Some(id) = line.strip_prefix("    new: ")
            && let Some(o) = old.take()
        {
            renames.push((o, id.trim().to_string()));
        }
    }
    renames
}

/// The renames this project's latest manifest records (none when there is
/// no manifest, or it names another project): a re-run after a crash finds
/// their new ids present (story 2.13, `--as` rule 2).
fn recorded_renames(places: &Places, project: &ProjectId) -> Vec<(String, String)> {
    let dir = places.user.accounts_migrate_backups(project);
    let Ok(read) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let latest = read
        .flatten()
        .filter_map(|e| {
            e.file_name()
                .to_string_lossy()
                .strip_suffix(".manifest.yaml")
                .map(str::to_string)
        })
        .max();
    let Some(stem) = latest else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(dir.join(format!("{stem}.manifest.yaml"))) else {
        return Vec::new();
    };
    let ours = text.lines().any(|l| {
        l.strip_prefix("project_id: ").map(str::trim) == Some(project.to_string().as_str())
    });
    if ours {
        manifest_renames(&text)
    } else {
        Vec::new()
    }
}

/// The `--as` renames whose new id an earlier run of this migrate already
/// listed (an owner `account_listed` record of `new` naming this project and
/// the old entry's provider, scheme, host and port): a run stopped after the
/// user records committed and before the manifest existed is completed by a
/// re-run (story 2.13, `--as` rule 2).
fn listed_renames(
    renames: &[(String, String)],
    legacy: &LegacyAccounts,
    notes: Notes<'_>,
    project: &ProjectId,
) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    for (old, new) in renames {
        let Some(account) = legacy.accounts.get(old) else {
            continue;
        };
        let Ok(endpoint) = endpoint_of(&account.provider, account.base_url.as_deref(), "base_url")
        else {
            continue;
        };
        let port = endpoint.port.to_string();
        let project = project.to_string();
        let listed = notes("config.widened", new)?.iter().any(|n| {
            let arg = |k: &str| n.args.get(k).map(String::as_str);
            matches!(n.actor, Actor::Human { .. })
                && n.file == USER_ACCOUNTS_FILE
                && arg("kind") == Some("account_listed")
                && arg("project") == Some(project.as_str())
                && arg("provider") == Some(account.provider.as_str())
                && arg("scheme") == Some(endpoint.scheme)
                && arg("host") == Some(endpoint.host.as_str())
                && arg("port") == Some(port.as_str())
        });
        if listed {
            out.push((old.clone(), new.clone()));
        }
    }
    Ok(out)
}

/// Refuses a project whose `models/` or `models/accounts.yaml` is a link
/// (Adversarial 8): the file could be read through it but never removed, so
/// nothing is asked or written.
fn no_links(root: &Path, did_not: &str) -> Result<(), Stop> {
    for path in [root.join("models"), root.join(LEGACY_ACCOUNTS_FILE)] {
        if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
            return refuse(format!(
                "{} is a link, and the app moves and removes only a project's own files, never through a link; replace the link with the file itself, then run this command again. {did_not}",
                shown(&path.display().to_string())
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- rollback

/// Refuses a restored id this Mac's list holds at another destination than
/// the backup's entry (ids `--as` renamed are exempt: the project names
/// their new ids).
fn restorable(backup: &Backup, user: &AccountsFile, did_not: &str) -> Result<(), Stop> {
    let legacy = am_config::legacy_accounts(&backup.bytes);
    for account in legacy.accounts.accounts() {
        if backup.renames.iter().any(|(old, _)| *old == account.id) {
            continue;
        }
        let Some((provider, dest)) = user_destination(user, &account.id) else {
            continue;
        };
        let ours = endpoint_of(&account.provider, account.base_url.as_deref(), "base_url");
        let same = provider == account.provider
            && matches!((&dest, &ours), (Ok(a), Ok(b)) if same_destination(a, b));
        if !same {
            return refuse(format!(
                "this Mac's Accounts hold '{}' at another destination than the backup's '{}' ({}), so restoring the file would name an Account this project never confirmed there. {did_not}",
                account.id,
                account.id,
                match dest {
                    Ok(d) => format!("provider {provider} at {}", d.goes_to()),
                    Err(why) => format!("provider {provider}: {why}"),
                }
            ));
        }
    }
    Ok(())
}

fn rollback(mut ctx: AccountsContext, project: &Path, out: &mut dyn Write) -> Result<(), Stop> {
    require_terminal(&ctx, "migrate --rollback", NOT_ROLLED_BACK)?;
    let places = ctx.places.clone();
    let opened = crate::runtime::open_project_unchanged(&places, ctx.identity.as_ref(), project)
        .map_err(|r| Stop(EXIT_ERRORS, format!("{r}. {NOT_ROLLED_BACK}")))?;
    for line in &opened.lines {
        let _ = writeln!(out, "agentic: {line}");
    }
    let root = opened.root.clone();
    let root_shown = shown(&root.display().to_string());
    let file = root.join(LEGACY_ACCOUNTS_FILE);
    let back_again = || {
        refuse(format!(
            "the project's {LEGACY_ACCOUNTS_FILE} ({}) exists again, and a rollback only re-creates it when it is absent. {NOT_ROLLED_BACK}",
            shown(&file.display().to_string())
        ))
    };
    if std::fs::symlink_metadata(&file).is_ok() {
        return back_again();
    }
    let all = crate::user::scan(&places, "");
    no_children(&root, &all, "migrate --rollback", NOT_ROLLED_BACK)?;
    no_outside_change(&opened, NOT_ROLLED_BACK)?;
    let standing = stand(&places, NOT_ROLLED_BACK)?;
    let backup = latest_backup(&places, &opened.project_id, NOT_ROLLED_BACK)?;
    let legacy = am_config::legacy_accounts(&backup.bytes);
    if !legacy.secret_lines.is_empty() {
        return refuse(format!(
            "the backup {} holds a secret-looking value, so it is not restored. {NOT_ROLLED_BACK}",
            shown(&backup.path.display().to_string())
        ));
    }
    restorable(&backup, &standing.file, NOT_ROLLED_BACK)?;
    let renames = if backup.renames.is_empty() {
        "The migration renamed nothing in this project.".to_string()
    } else {
        let all: Vec<String> = backup
            .renames
            .iter()
            .map(|(old, new)| format!("'{new}' (was '{old}')"))
            .collect();
        shown(&format!(
            "The renames --as made stay: this project's references keep {}, because each keeps its original destination, while '{}' on this Mac's Accounts is another Account at another destination; reverting them would move them there.",
            all.join(", "),
            backup
                .renames
                .iter()
                .map(|(old, _)| old.as_str())
                .collect::<Vec<_>>()
                .join("', '")
        ))
    };
    ctx.terminal.show(&format!(
        "Rolling back the Accounts move of {root_shown}: {LEGACY_ACCOUNTS_FILE} is re-created byte for byte from the backup {} (it is a project file again, read by nothing and an error in `agentic validate`). {renames} This Mac's Accounts, their records and the Keychain are left as they are, so the Accounts moved stay on this Mac.\nTo confirm, type `{ROLLBACK_WORD}` within 2 minutes: ",
        shown(&backup.path.display().to_string())
    ));
    typed(
        &mut ctx,
        ROLLBACK_WORD,
        &format!("the word `{ROLLBACK_WORD}`"),
        NOT_ROLLED_BACK,
    )?;
    if ctx.terminal.interrupted() {
        return Err(interrupted(NOT_ROLLED_BACK));
    }
    // Everything again, now that the word is typed.
    if std::fs::symlink_metadata(&file).is_ok() {
        return back_again();
    }
    let again = latest_backup(&places, &opened.project_id, NOT_ROLLED_BACK)?;
    if again.path != backup.path || again.bytes != backup.bytes {
        return refuse(format!(
            "the latest backup changed while this command waited. {NOT_ROLLED_BACK}; run the command again"
        ));
    }
    let now = stand(&places, NOT_ROLLED_BACK)?;
    restorable(&again, &now.file, NOT_ROLLED_BACK)?;
    let entry = opened
        .writer
        .restore_legacy_accounts(owner(), again.bytes)
        .map_err(|e| {
            Stop(
                EXIT_ERRORS,
                format!("{}. {NOT_ROLLED_BACK}", write_words(&e)),
            )
        })?;
    let _ = writeln!(
        out,
        "agentic: re-created {LEGACY_ACCOUNTS_FILE} in {root_shown} from {} (History entry {}). This Mac's Accounts are unchanged.",
        shown(&backup.path.display().to_string()),
        entry.id
    );
    Ok(())
}
