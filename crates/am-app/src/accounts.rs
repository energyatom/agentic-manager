//! `agentic accounts add|remove|list|test` (story 2.1): AD-15's interim CLI
//! Account edit, and the two reads beside it.
//!
//! `add` and `remove` are the only config edits the CLI makes, and they
//! write only `models/accounts.yaml`, through am-config's writer, as the
//! owner (`Actor::Human`, the Owner hat). Their checks run in this order,
//! and the first that fails is what the owner reads; nothing is read from
//! the terminal, written or stored before all of them passed:
//!
//! 1. usage (exit 2);
//! 2. the terminal: stdin and stdout are both a real interactive terminal
//!    ([`crate::terminal::interactive`], through the `Terminal` port);
//! 3. the provider (`add`: Anthropic only in this story);
//! 4. no stdio MCP server and no Source with `render: true` is enabled
//!    (children that run as the owner, AD-13);
//! 5. the project, resolved read-only by AD-32's checks
//!    ([`crate::runtime::open_project_unchanged`]: no id minted, no legacy
//!    folder moved, no store migrated, no marker written), with the store's
//!    lock held from here to the end of the command;
//! 6. no outside change waits for Reload (AD-16);
//! 7. the id: not reserved, not already in the file (`add`); in the file
//!    and named by nothing (`remove`).
//!
//! `add` then prints one sentence naming the Account, its provider, the
//! endpoint its calls go to and everything that already names the id, and
//! requires the id typed within 2 minutes. Only then is the key read, with
//! echo off; an entry the Keychain already holds is reused and never
//! overwritten. Once the last prompt has returned, checks 4, 6 and 7 run
//! again on a fresh read (the file's bytes, the id, what names it, outside
//! changes): the owner may have taken minutes, and anything that changed
//! since the sentence was shown refuses the add, to be run again. A Ctrl-C
//! pressed since the id was typed cancels (exit 130), before the key is
//! stored and again before the file is written. Then the key is stored,
//! only in the Keychain through `SecretStore` (the entry is read again
//! right before: one stored by another process while the key prompt waited
//! is kept and used, never overwritten), and the file is written last
//! (a `ConfigCreate` when it is missing; otherwise an edit the writer
//! applies only while the file holds the bytes it was built on); its
//! History entry and the `account_added` audit entry commit in one store
//! transaction. If the write fails, the Keychain entry is deleted when this
//! command created it. A write counts as landed with its History entry
//! pending only when the file holds the bytes of this command's pending
//! intent (its sha256), never because the id is, or is not, in the file.
//! The key is never an argument, never read from the environment and never
//! printed.
//!
//! The open (5) settles a config write a crash interrupted, as every open
//! of the writer does, and prints what it did before any refusal; so a
//! refusal says what this command did not do (no Account added or removed,
//! no key read or stored), never that nothing was written.
//!
//! `list` and `test` make no config edit, take no lock and run with any
//! stdin. A key is read only from the Keychain entry named by the Account's
//! own id (AD-17): a `secret_ref` naming another entry is reported by
//! `list` and refused by `test`, and never followed. `test` streams one
//! short reply through an egress client whose policy admits only the
//! Account's pinned host and no loopback port.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use am_config::{ACCOUNTS_FILE, AccountUse, ConfigCreate};
use am_domain::{
    AuditNote, ChatRequest, ConfigHistory, EgressClient, EventSink, Instruction, LlmProvider,
    Message, Part, ProjectIdentity, Role, SecretStore, StreamEvent, Terminal, TerminalError,
};
use am_engine::TokenSignal;
use am_net::EgressPolicy;
use am_providers::AnthropicProvider;
use am_providers::anthropic::ANTHROPIC_HOST;
use tokio_util::sync::CancellationToken;

use crate::cli::{EXIT_ERRORS, EXIT_OK, EXIT_USAGE};
use crate::runtime::{Places, owner};

/// The exit code after Ctrl-C at a prompt (128 + SIGINT, as shells report it).
pub const EXIT_INTERRUPTED: i32 = 130;

/// How long the owner has to type the Account's id (AD-15).
pub const CONFIRM_WITHIN: Duration = Duration::from_secs(120);

/// The one provider an Account can be added for in this story.
const ANTHROPIC: &str = "anthropic";

const USAGE: &str = "usage: agentic accounts add <id> --provider anthropic [--name <text>] [--project <folder>]\n       agentic accounts remove <id> [--project <folder>]\n       agentic accounts list [--project <folder>]\n       agentic accounts test <id> [--model <model>] [--project <folder>]\n       (--project defaults to the current folder)";

/// What `agentic accounts` runs with. `main` builds it with the owner's real
/// terminal, the Keychain, the OS identity adapter and `am-net`'s client;
/// tests pass am-testkit's fakes.
pub struct AccountsContext {
    pub places: Places,
    /// Supplies the checks of the read-only resolve and the host recorded
    /// in the store's lock (AD-32).
    pub identity: Arc<dyn ProjectIdentity>,
    pub terminal: Box<dyn Terminal>,
    pub secrets: Arc<dyn SecretStore>,
    /// Builds the egress client for a policy (`test`: the Account's host only).
    pub egress: Arc<dyn Fn(EgressPolicy) -> Arc<dyn EgressClient> + Send + Sync>,
}

/// The parsed command line.
enum Command {
    Add {
        id: String,
        provider: String,
        name: Option<String>,
    },
    Remove {
        id: String,
    },
    List,
    Test {
        id: String,
        model: Option<String>,
    },
}

fn parse(args: &[String]) -> Result<(Command, PathBuf), String> {
    let Some(verb) = args.first().map(String::as_str) else {
        return Err("accounts needs `add`, `remove`, `list` or `test`".into());
    };
    if !["add", "remove", "list", "test"].contains(&verb) {
        return Err(format!("unknown accounts command `{verb}`"));
    }
    let mut positional: Vec<String> = Vec::new();
    let mut flags: BTreeMap<&str, String> = BTreeMap::new();
    let mut words = args[1..].iter();
    while let Some(word) = words.next() {
        let flag = match (word.as_str(), verb) {
            ("--project", _) => "--project",
            ("--provider", "add") => "--provider",
            ("--name", "add") => "--name",
            ("--model", "test") => "--model",
            (w, _) if w.starts_with('-') => {
                return Err(format!("unknown option `{w}` for `accounts {verb}`"));
            }
            _ => {
                positional.push(word.clone());
                continue;
            }
        };
        match words.next() {
            Some(value) if !value.starts_with("--") && !value.is_empty() => {
                if flags.insert(flag, value.clone()).is_some() {
                    return Err(format!("{flag} is given twice"));
                }
            }
            _ => return Err(format!("{flag} needs a value")),
        }
    }
    let project = PathBuf::from(flags.remove("--project").unwrap_or_else(|| ".".into()));
    let one_id = |what: &str| match positional.as_slice() {
        [id] => Ok(id.clone()),
        [] => Err(format!("accounts {verb} needs {what}")),
        _ => Err(format!("accounts {verb} takes one Account id")),
    };
    let command = match verb {
        "add" => {
            let id = one_id("the new Account's id")?;
            let provider = flags
                .remove("--provider")
                .ok_or("accounts add needs --provider anthropic")?;
            Command::Add {
                id,
                provider,
                name: flags.remove("--name"),
            }
        }
        "remove" => Command::Remove {
            id: one_id("the Account's id")?,
        },
        "test" => Command::Test {
            id: one_id("the Account's id")?,
            model: flags.remove("--model"),
        },
        _ => {
            if !positional.is_empty() {
                return Err("accounts list takes no Account id".into());
            }
            Command::List
        }
    };
    Ok((command, project))
}

/// A refusal: its exit code and its words.
struct Stop(i32, String);

fn refuse<T>(words: impl Into<String>) -> Result<T, Stop> {
    Err(Stop(EXIT_ERRORS, words.into()))
}

/// `agentic accounts …`: `args` are the words after `accounts`. Returns the
/// exit code: 0 done, 2 bad usage (with `usage: agentic accounts` on
/// `err`), 130 after Ctrl-C at a prompt, 1 for every other refusal or
/// failure, each in words on `err`.
pub async fn accounts(
    args: &[String],
    ctx: AccountsContext,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let (command, project) = match parse(args) {
        Ok(parsed) => parsed,
        Err(why) => {
            let _ = writeln!(err, "{USAGE}");
            let _ = writeln!(err, "agentic: {why}");
            return EXIT_USAGE;
        }
    };
    let done = match command {
        Command::Add { id, provider, name } => {
            add(ctx, &project, &id, &provider, name.as_deref(), out)
        }
        Command::Remove { id } => remove(ctx, &project, &id, out),
        Command::List => list(&ctx, &project, out),
        Command::Test { id, model } => test(&ctx, &project, &id, model, out).await,
    };
    match done {
        Ok(()) => EXIT_OK,
        Err(Stop(code, words)) => {
            let _ = writeln!(err, "agentic: {words}");
            code
        }
    }
}

// ------------------------------------------------------------- the edit

/// What a refused `add` did not do, while no key has been read yet. (The
/// open may have settled an interrupted write, so no refusal says that
/// nothing was written.)
const NOT_ADDED: &str = "No Account was added and no key was read or stored";
/// What a refused `add` did not do, once the key was typed.
const NOT_ADDED_NOR_STORED: &str = "No Account was added and no key was stored";
const NOT_REMOVED: &str = "No Account was removed";

/// Check 2: the terminal rule of AD-15. `what` is `add` or `remove`.
fn require_terminal(ctx: &AccountsContext, what: &str, did_not: &str) -> Result<(), Stop> {
    if ctx.terminal.interactive() {
        return Ok(());
    }
    refuse(format!(
        "`agentic accounts {what}` runs only in an interactive terminal: stdin and stdout must both be a terminal, not a pipe, a file or another program, because adding or removing an Account changes what this project may call and only you decide that. {did_not}; run the command yourself in a terminal"
    ))
}

/// Check 4: no child that runs as the owner is enabled (AD-13, AD-15).
fn no_owner_child(project: &Path, what: &str, did_not: &str) -> Result<(), Stop> {
    match am_config::decisions_locked(project) {
        None => Ok(()),
        Some(why) => refuse(format!(
            "`agentic accounts {what}` cannot be used from the command line while {why}: a program that runs as you could type here. {did_not}; remove or disable it first"
        )),
    }
}

/// A project open for the edit: the store under its lock, the writer, and
/// the History adapter the outside-change check reads.
struct Editing {
    opened: crate::runtime::OpenedUnchanged,
    history: am_store::StoreConfigHistory,
}

impl Editing {
    /// Check 6: no tracked file differs from what its last History entry
    /// wrote (an outside change that no Reload has taken in yet, AD-16).
    fn no_outside_change(&self, did_not: &str) -> Result<(), Stop> {
        let changed = am_config::outside_changes(&self.opened.root, &self.history)
            .map_err(|e| Stop(EXIT_ERRORS, format!("{e}. {did_not}")))?;
        if changed.is_empty() {
            return Ok(());
        }
        refuse(format!(
            "{} changed on disk outside the app since the app last wrote it, so nothing is written over that change. {did_not}. Put the file back as the app last wrote it (undo your change), then run this command again; Reload, which takes such a change in, arrives with the app's UI",
            changed.join(", ")
        ))
    }

    /// After a write of this command failed: whether the file now holds the
    /// bytes its intent meant to write, with that intent still pending (so
    /// the next open records the History entry). The file's sha256 is
    /// compared with the pending intent's; that the Account's id is, or is
    /// not, in the file would not tell, because an outside edit can do the
    /// same. `None` when the store cannot be read to tell.
    fn written_pending(&self) -> Option<bool> {
        let Some(on_disk) = am_config::accounts_file(&self.opened.root).sha256 else {
            return Some(false);
        };
        let pending = self.history.pending_intents().ok()?;
        Some(
            pending
                .iter()
                .any(|p| p.intent.file == ACCOUNTS_FILE && p.intent.after_sha256 == on_disk),
        )
    }
}

/// A writer's refusal in this command's words. The writer words a file
/// that changed under an edit for the app's Reload; here the command is
/// simply run again.
fn write_words(e: &am_config::WriteError) -> String {
    match e {
        am_config::WriteError::ChangedOnDisk { files } => format!(
            "{} changed on disk after this command last checked it, so the edit built on what it held was not applied; run the command again",
            files.join(", ")
        ),
        other => other.to_string(),
    }
}

/// Checks 4 to 6 of the module's list: no child that runs as the owner is
/// enabled, the project opens read-only under the store's lock, and no
/// outside change waits for Reload. `what` is `add` or `remove`. The open
/// settles config writes a crash interrupted; what it did is printed here,
/// before any later refusal.
fn open_for_edit(
    ctx: &AccountsContext,
    project: &Path,
    what: &str,
    did_not: &str,
    out: &mut dyn Write,
) -> Result<Editing, Stop> {
    if !project.is_dir() {
        return refuse(format!(
            "the project folder {} does not exist. {did_not}",
            project.display()
        ));
    }
    no_owner_child(project, what, did_not)?;
    let opened =
        crate::runtime::open_project_unchanged(&ctx.places, ctx.identity.as_ref(), project)
            .map_err(|e| {
                Stop(
                    EXIT_ERRORS,
                    format!(
                        "cannot open the project {}: {e}. {did_not}",
                        project.display()
                    ),
                )
            })?;
    for line in &opened.lines {
        let _ = writeln!(out, "agentic: {line}");
    }
    let history = am_store::StoreConfigHistory::new(&opened.store)
        .map_err(|e| Stop(EXIT_ERRORS, format!("{e}. {did_not}")))?;
    let editing = Editing { opened, history };
    editing.no_outside_change(did_not)?;
    Ok(editing)
}

/// The Accounts file, when it can be patched.
fn patchable(root: &Path, did_not: &str) -> Result<am_config::AccountsFile, Stop> {
    let file = am_config::accounts_file(root);
    match &file.problem {
        Some(problem) => refuse(format!(
            "{ACCOUNTS_FILE} cannot be changed because {problem}; fix the file first (`agentic validate` names the line). {did_not}"
        )),
        None => Ok(file),
    }
}

fn reserved(id: &str) -> bool {
    am_config::reserved_account_ids().iter().any(|r| r == id)
}

fn uses_words(uses: &[AccountUse]) -> String {
    if uses.is_empty() {
        return "nothing".to_string();
    }
    let all: Vec<String> = uses.iter().map(AccountUse::to_string).collect();
    all.join(", ")
}

fn interrupted(did_not: &str) -> Stop {
    Stop(EXIT_INTERRUPTED, format!("interrupted (Ctrl-C). {did_not}"))
}

/// Everything the add checked before its prompts, checked again on a fresh
/// read once the last prompt returned: the owner may have taken minutes,
/// and the edit, the sentence and the audit entry were all built on the
/// first read. Anything that changed since refuses the add (run it again).
fn nothing_changed_since(
    editing: &Editing,
    id: &str,
    file: &am_config::AccountsFile,
    uses: &[AccountUse],
) -> Result<(), Stop> {
    let root = &editing.opened.root;
    let again = format!("{NOT_ADDED_NOR_STORED}; run the command again");
    no_owner_child(root, "add", NOT_ADDED_NOR_STORED)?;
    editing.no_outside_change(NOT_ADDED_NOR_STORED)?;
    let now = am_config::accounts_file(root);
    if now.problem.is_some()
        || now.exists != file.exists
        || now.sha256 != file.sha256
        || now.has(id)
    {
        return refuse(format!(
            "{ACCOUNTS_FILE} changed while this command waited at its prompt, so the add was built on what the file no longer holds. {again}"
        ));
    }
    let uses_now = am_config::account_uses(root, id);
    if uses_now != uses {
        return refuse(format!(
            "what names the Account '{id}' changed while this command waited at its prompt: it is now named by {}, and the sentence you confirmed said {}. {again}",
            uses_words(&uses_now),
            uses_words(uses)
        ));
    }
    Ok(())
}

fn add(
    mut ctx: AccountsContext,
    project: &Path,
    id: &str,
    provider: &str,
    name: Option<&str>,
    out: &mut dyn Write,
) -> Result<(), Stop> {
    require_terminal(&ctx, "add", NOT_ADDED)?;
    if provider != ANTHROPIC {
        return refuse(format!(
            "only an Anthropic Account can be added from the command line in this version (`--provider {ANTHROPIC}`), not '{provider}'. {NOT_ADDED}"
        ));
    }
    let editing = open_for_edit(&ctx, project, "add", NOT_ADDED, out)?;
    let root = editing.opened.root.clone();
    if reserved(id) {
        return refuse(format!(
            "'{id}' is a reserved local Account: it is always available and is never added. {NOT_ADDED}"
        ));
    }
    let file = patchable(&root, NOT_ADDED)?;
    if file.has(id) {
        return refuse(format!(
            "{ACCOUNTS_FILE} already has an Account '{id}'. {NOT_ADDED}; choose another id"
        ));
    }
    if !am_config::is_config_id(id) {
        return refuse(format!(
            "'{id}' is not an Account id: use lowercase letters, digits and single hyphens (kebab-case), for example `cloud-main`. {NOT_ADDED}"
        ));
    }
    let uses = am_config::account_uses(&root, id);
    let endpoint = format!("https://{ANTHROPIC_HOST}");
    let named = if uses.is_empty() {
        "Nothing in the project names it yet.".to_string()
    } else {
        format!(
            "Already naming it, and brought into force by this add: {}.",
            uses_words(&uses)
        )
    };
    ctx.terminal.show(&format!(
        "Adding the Account '{id}' (provider {provider}): its calls go to {endpoint}, with a key kept only in this Mac's Keychain. {named}\nTo confirm, type the Account's id ({id}) within 2 minutes: "
    ));
    let typed = match ctx.terminal.read_line(Some(CONFIRM_WITHIN)) {
        Ok(line) => line,
        Err(TerminalError::TimedOut) => {
            ctx.terminal.show("\n");
            return refuse(format!(
                "the Account's id was not typed within 2 minutes. {NOT_ADDED}"
            ));
        }
        Err(TerminalError::Interrupted) => {
            ctx.terminal.show("\n");
            return Err(interrupted(NOT_ADDED));
        }
        Err(e) => {
            return refuse(format!("the Account's id was not typed ({e}). {NOT_ADDED}"));
        }
    };
    if typed.trim() != id {
        return refuse(format!(
            "what was typed is not the Account's id '{id}'. {NOT_ADDED}"
        ));
    }
    // The key: an entry the Keychain already holds for this id is reused
    // and never overwritten; otherwise it is read with echo off.
    let new_key = match ctx.secrets.get(id) {
        Err(e) => {
            return refuse(format!("the Keychain cannot be read ({e}). {NOT_ADDED}"));
        }
        Ok(Some(_)) => {
            ctx.terminal.show(&format!(
                "The Keychain already holds a key for '{id}': reusing it (it is never overwritten).\n"
            ));
            None
        }
        Ok(None) => {
            ctx.terminal.show(&format!(
                "Key for '{id}' (not shown as you type or paste it): "
            ));
            let key = match ctx.terminal.read_secret() {
                Ok(key) => key,
                Err(TerminalError::Interrupted) => return Err(interrupted(NOT_ADDED_NOR_STORED)),
                Err(e) => {
                    return refuse(format!("no key was read ({e}). {NOT_ADDED_NOR_STORED}"));
                }
            };
            if key.expose().trim().is_empty() {
                return refuse(format!("no key was typed. {NOT_ADDED_NOR_STORED}"));
            }
            Some(am_domain::SecretString::new(
                key.expose().trim().to_string(),
            ))
        }
    };
    // The last prompt has returned. Before anything is stored: every check
    // again on a fresh read, and a Ctrl-C pressed since the id was typed.
    nothing_changed_since(&editing, id, &file, &uses)?;
    if ctx.terminal.interrupted() {
        return Err(interrupted(NOT_ADDED_NOR_STORED));
    }
    let stored_here = match new_key {
        None => false,
        // The key prompt has no time limit, so the Keychain is read again
        // right before the store: an entry another process stored
        // meanwhile is never overwritten (and so never deleted below). The
        // typed key is discarded and the add goes on as a reuse.
        Some(key) => match ctx.secrets.get(id) {
            Err(e) => {
                return refuse(format!(
                    "the Keychain cannot be read ({e}). {NOT_ADDED_NOR_STORED}"
                ));
            }
            Ok(Some(_)) => {
                drop(key);
                ctx.terminal.show(&format!(
                    "The Keychain now holds a key for '{id}', stored while this command waited: that entry is kept and used (it is never overwritten), and the key you typed was discarded.\n"
                ));
                false
            }
            Ok(None) => {
                if let Err(e) = ctx.secrets.set(id, key) {
                    return refuse(format!(
                        "the key could not be stored in the Keychain ({e}). No Account was added"
                    ));
                }
                true
            }
        },
    };
    // What became of the Keychain entry when the add stops after this point.
    let key_undone = |secrets: &dyn SecretStore| {
        if !stored_here {
            return "the Keychain entry it would have reused is unchanged".to_string();
        }
        match secrets.delete(id) {
            Ok(()) => "the key this command stored was deleted from the Keychain".to_string(),
            Err(d) => format!(
                "the key this command stored could not be deleted from the Keychain ({d}); delete the entry '{id}' of service agentic-manager by hand"
            ),
        }
    };
    if ctx.terminal.interrupted() {
        let key = key_undone(ctx.secrets.as_ref());
        return Err(interrupted(&format!("No Account was added, and {key}")));
    }
    // The write, with AD-15's widening record in its transaction. An edit
    // is applied only while the file holds the bytes it was built on.
    let note = AuditNote {
        action: "config.widened".into(),
        target: id.to_string(),
        args: BTreeMap::from([
            ("kind".to_string(), "account_added".to_string()),
            ("file".to_string(), ACCOUNTS_FILE.to_string()),
            ("account".to_string(), id.to_string()),
            ("provider".to_string(), provider.to_string()),
            ("scheme".to_string(), "https".to_string()),
            ("host".to_string(), ANTHROPIC_HOST.to_string()),
            ("port".to_string(), "443".to_string()),
            (
                "references".to_string(),
                if uses.is_empty() {
                    String::new()
                } else {
                    uses_words(&uses)
                },
            ),
            ("confirmation".to_string(), "typed_id".to_string()),
        ]),
    };
    let writer = &editing.opened.writer;
    let written = match &file.sha256 {
        Some(built_on) => writer.apply_with_audit(
            owner(),
            am_config::add_account_edit(&file, id, provider, name),
            built_on,
            vec![note],
        ),
        None => am_config::new_accounts_file(id, provider, name).and_then(|bytes| {
            writer.create(
                owner(),
                ConfigCreate {
                    file: ACCOUNTS_FILE.into(),
                    bytes,
                    audit: vec![note],
                },
            )
        }),
    };
    let entry = match written {
        Ok(entry) => entry,
        Err(e) => {
            let why = write_words(&e);
            return match editing.written_pending() {
                // A failure after the file was written (the folder sync,
                // the History commit): the next start records the entry,
                // and the key stays.
                Some(true) => refuse(format!(
                    "the Account '{id}' was written to {ACCOUNTS_FILE}, but the write did not finish: {why}. Its History entry is recorded the next time the project opens; its key is in the Keychain"
                )),
                // That the id is in the file would not show it was this
                // command's write: an outside edit can add the same id.
                Some(false) => {
                    let key = key_undone(ctx.secrets.as_ref());
                    refuse(format!(
                        "the Account '{id}' was not added: {why}. This command did not change {ACCOUNTS_FILE}, and {key}"
                    ))
                }
                None => refuse(format!(
                    "the write of the Account '{id}' stopped ({why}), and the project's store could not be read to tell whether {ACCOUNTS_FILE} was written; `agentic accounts list` shows what it holds. A key this command stored is still in the Keychain"
                )),
            };
        }
    };
    let key = if stored_here {
        "its key is in the Keychain"
    } else {
        "it reuses the key the Keychain already held"
    };
    let _ = writeln!(
        out,
        "agentic: added the Account '{id}' ({provider}, {endpoint}) to {ACCOUNTS_FILE} (History entry {}); {key}",
        entry.id
    );
    Ok(())
}

fn remove(
    mut ctx: AccountsContext,
    project: &Path,
    id: &str,
    out: &mut dyn Write,
) -> Result<(), Stop> {
    require_terminal(&ctx, "remove", NOT_REMOVED)?;
    let editing = open_for_edit(&ctx, project, "remove", NOT_REMOVED, out)?;
    let root = editing.opened.root.clone();
    if reserved(id) {
        return refuse(format!(
            "'{id}' is a reserved local Account: it is always available and cannot be removed. {NOT_REMOVED}"
        ));
    }
    let file = patchable(&root, NOT_REMOVED)?;
    let unknown = || {
        Stop(
            EXIT_ERRORS,
            format!(
                "there is no Account '{id}' in {ACCOUNTS_FILE} (`agentic accounts list` shows the Accounts). {NOT_REMOVED}"
            ),
        )
    };
    let Some(built_on) = file.sha256.as_deref().filter(|_| file.has(id)) else {
        return Err(unknown());
    };
    let uses = am_config::account_uses(&root, id);
    if !uses.is_empty() {
        return refuse(format!(
            "the Account '{id}' is still named by {}. {NOT_REMOVED}; point each at another Account first, then remove it",
            uses_words(&uses)
        ));
    }
    let Some(edit) = am_config::remove_account_edit(&file, id) else {
        return Err(unknown());
    };
    if ctx.terminal.interrupted() {
        return Err(interrupted(NOT_REMOVED));
    }
    // Applied only while the file holds the bytes the edit was built on.
    let entry = editing
        .opened
        .writer
        .apply_with_audit(owner(), edit, built_on, Vec::new())
        .map_err(|e| {
            let why = write_words(&e);
            let words = match editing.written_pending() {
                // A failure after the file was replaced: the Account is
                // gone from the file, its History entry pending.
                Some(true) => format!(
                    "the Account '{id}' was removed from {ACCOUNTS_FILE}, but the write did not finish: {why}. Its History entry is recorded the next time the project opens"
                ),
                Some(false) => format!(
                    "the Account '{id}' was not removed: {why}. This command did not change {ACCOUNTS_FILE}"
                ),
                None => format!(
                    "the removal of the Account '{id}' stopped ({why}), and the project's store could not be read to tell whether {ACCOUNTS_FILE} was written; `agentic accounts list` shows what it holds"
                ),
            };
            Stop(EXIT_ERRORS, words)
        })?;
    let _ = writeln!(
        out,
        "agentic: removed the Account '{id}' from {ACCOUNTS_FILE}. The change is recorded in History (entry {}) and can be undone from the app's History when the UI ships. Its key stays in the Keychain, because another project may use the same id; delete the entry '{id}' of service agentic-manager in Keychain Access if no project needs it",
        entry.id
    );
    Ok(())
}

// ------------------------------------------------------------ list, test

/// An Account's `secret_ref` when it names another Keychain entry than the
/// Account's own id. AD-17 names a key's entry by the Account id, and that
/// is the only entry `list` and `test` ever read: a `secret_ref` written by
/// hand is never followed, or a config file could have any `agentic-manager`
/// Keychain entry shown or sent to a provider.
fn other_entry(account: &am_config::Account) -> Option<&str> {
    account
        .secret_ref
        .as_deref()
        .filter(|entry| *entry != account.id)
}

/// A key as `list` shows it: dots, then its last 4 characters; dots only
/// for a key too short to show any of.
fn masked(key: &str) -> String {
    const DOTS: &str = "••••••••";
    let chars: Vec<char> = key.chars().collect();
    if chars.len() < 12 {
        return DOTS.to_string();
    }
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{DOTS}{tail}")
}

/// The host an Account's calls go to, when this version knows it.
fn host_of(account: &am_config::Account) -> String {
    if account.provider == ANTHROPIC && account.base_url.is_none() {
        return ANTHROPIC_HOST.to_string();
    }
    account
        .base_url
        .as_deref()
        .and_then(|url| url.split_once("://"))
        .map(|(_, rest)| {
            rest.split(['/', '?', '#'])
                .next()
                .unwrap_or(rest)
                .to_string()
        })
        .filter(|host| !host.is_empty())
        .unwrap_or_else(|| "(its provider's host)".to_string())
}

fn readable(project: &Path) -> Result<am_config::AccountsFile, Stop> {
    if !project.is_dir() {
        return refuse(format!(
            "the project folder {} does not exist",
            project.display()
        ));
    }
    let file = am_config::accounts_file(project);
    match &file.problem {
        Some(problem) => refuse(format!(
            "{ACCOUNTS_FILE} cannot be read as Accounts because {problem} (`agentic validate` names the line)"
        )),
        None => Ok(file),
    }
}

fn list(ctx: &AccountsContext, project: &Path, out: &mut dyn Write) -> Result<(), Stop> {
    let file = readable(project)?;
    if file.ids.is_empty() {
        let _ = writeln!(
            out,
            "agentic: no Accounts in {ACCOUNTS_FILE} yet; add one with `agentic accounts add <id> --provider {ANTHROPIC}` (the local Accounts local-lmstudio and local-ollama are always available)"
        );
        return Ok(());
    }
    for id in file.ids.iter().flatten() {
        let Some(account) = file.accounts.iter().find(|a| &a.id == id) else {
            let _ = writeln!(
                out,
                "{id}  (not a valid Account entry; `agentic validate` names the line)"
            );
            continue;
        };
        let key = if other_entry(account).is_some() {
            // No key is read for it.
            "secret_ref names another entry; not used".to_string()
        } else {
            match ctx.secrets.get(&account.id) {
                Ok(Some(key)) => masked(key.expose()),
                Ok(None) => "no key in Keychain".to_string(),
                Err(e) => format!("the Keychain cannot be read: {e}"),
            }
        };
        let _ = writeln!(
            out,
            "{}  {}  {}  {key}",
            account.id,
            account.provider,
            host_of(account)
        );
    }
    Ok(())
}

/// Sends each piece of streamed text to the command as it arrives.
struct Forward(tokio::sync::mpsc::UnboundedSender<String>);

impl EventSink for Forward {
    fn on_event(&self, event: StreamEvent) {
        if let StreamEvent::TextDelta(text) = event {
            let _ = self.0.send(text);
        }
    }
}

const TEST_PROMPT: &str = "Count from one to three in words, one word per line, and nothing else.";

async fn test(
    ctx: &AccountsContext,
    project: &Path,
    id: &str,
    model: Option<String>,
    out: &mut dyn Write,
) -> Result<(), Stop> {
    let file = readable(project)?;
    if reserved(id) {
        return refuse(format!(
            "'{id}' is a local Account with no key; `agentic accounts test` calls the cloud Accounts in {ACCOUNTS_FILE}"
        ));
    }
    let Some(account) = file.accounts.iter().find(|a| a.id == id) else {
        return refuse(format!(
            "there is no Account '{id}' in {ACCOUNTS_FILE} (`agentic accounts list` shows the Accounts); nothing was sent"
        ));
    };
    if account.provider != ANTHROPIC || account.base_url.is_some() {
        return refuse(format!(
            "the Account '{id}' is not a plain Anthropic Account, the only kind this version can test; nothing was sent"
        ));
    }
    if let Some(entry) = other_entry(account) {
        return refuse(format!(
            "the Account '{id}' has `secret_ref: {entry}`, which names another Keychain entry; a key is only ever read from the entry named by the Account's own id, so no key was read and nothing was sent. Set `secret_ref: {id}` (or remove the line) in {ACCOUNTS_FILE}"
        ));
    }
    match ctx.secrets.get(id) {
        Ok(Some(_)) => {}
        Ok(None) => {
            return refuse(format!(
                "the Account '{id}' has no key in Keychain on this Mac, so nothing was sent. From the command line a key is stored only when an Account is added, and adding '{id}' again needs it removed first, which needs it unreferenced (no seat, fallback, capability or Source naming it); the app's Account card will set a key when it ships"
            ));
        }
        Err(e) => {
            return refuse(format!(
                "the Keychain cannot be read ({e}); nothing was sent"
            ));
        }
    }
    // Only the Account's pinned endpoint: its host over https, no loopback
    // port (AD-12, AD-48).
    let policy = EgressPolicy {
        allowed_hosts: vec![ANTHROPIC_HOST.to_string()],
        loopback_ports: Vec::new(),
        ..EgressPolicy::default()
    };
    let egress = (ctx.egress)(policy);
    let provider = AnthropicProvider::new(egress, id);
    let model = model.unwrap_or_else(|| crate::DEFAULT_ANTHROPIC_MODEL.to_string());
    let request = ChatRequest {
        model: model.clone(),
        system: Vec::new(),
        messages: vec![Message {
            role: Role::User,
            parts: vec![Part::Instruction(Instruction::authored(TEST_PROMPT))],
        }],
        tools: Vec::new(),
        max_output_tokens: 64,
    };
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut call = provider.stream(
        request,
        TokenSignal::signal(CancellationToken::new()),
        Arc::new(Forward(tx)),
    );
    let mut streamed = String::new();
    let mut show = |text: String, out: &mut dyn Write| {
        let _ = out.write_all(text.as_bytes());
        let _ = out.flush();
        streamed.push_str(&text);
    };
    let outcome = loop {
        tokio::select! {
            biased;
            Some(text) = rx.recv() => show(text, out),
            outcome = &mut call => break outcome,
        }
    };
    while let Ok(text) = rx.try_recv() {
        show(text, out);
    }
    if !streamed.is_empty() && !streamed.ends_with('\n') {
        let _ = writeln!(out);
    }
    match outcome {
        Ok(reply) => {
            let tokens = match reply.usage {
                Some(u) => format!(
                    "{} input and {} output tokens",
                    u.input_tokens, u.output_tokens
                ),
                None => "no usage reported".to_string(),
            };
            let _ = writeln!(
                out,
                "agentic: the Account '{id}' answered from https://{ANTHROPIC_HOST} (model {model}, {tokens})"
            );
            Ok(())
        }
        Err(e) => refuse(format!(
            "the test call of the Account '{id}' (model {model}) failed: {e}"
        )),
    }
}
