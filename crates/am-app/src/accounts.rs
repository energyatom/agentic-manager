//! `agentic accounts add|remove|list|test` (story 2.1): AD-15's interim CLI
//! Account edit, and the two reads beside it. Story 2.3 adds the providers
//! `openai` and `openai-compatible` to `add`, `list` and `test`: the add
//! path is unchanged, and only the provider and the endpoint differ. One
//! function, [`endpoint_of`], derives an Account's endpoint (scheme, host,
//! port, base URL) for all three commands.
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
//! 3. the provider and its endpoint (`add`: `anthropic`, `openai`, or
//!    `openai-compatible` with a `--base-url` in one of the two accepted
//!    forms);
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
//! overwritten, and the sentence says so, and that this key will be sent to
//! the endpoint, before the id is typed (story 2.3). An entry that appears
//! or goes away while a prompt waits refuses the add. Once the last prompt has returned, checks 4, 6 and 7 run
//! again on a fresh read (the file's bytes, the id, what names it, outside
//! changes): the owner may have taken minutes, and anything that changed
//! since the sentence was shown refuses the add, to be run again. A Ctrl-C
//! pressed since the id was typed cancels (exit 130), before the key is
//! stored and again before the file is written. Then the key is stored,
//! only in the Keychain through `SecretStore` (the entry is read again
//! right before: one stored by another process while the key prompt waited
//! is never overwritten, deleted or used, and the add is refused), and the
//! file is written last
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
//! stdin. `test` calls only an Account the owner confirmed: before any
//! Keychain read or request it checks, through a read-only store reader,
//! that `models/accounts.yaml` holds exactly the bytes the app last wrote
//! and that the latest `account_added` audit note of the Account names the
//! same provider, scheme, host and port ([`crate::egress::confirmed`], the
//! check the project's egress directory makes for every caller since story
//! 2.2); an entry of the file that has an error is not called either. A
//! key is read only from the Keychain entry named by the Account's own id
//! (AD-17): a `secret_ref` naming another entry is reported by `list` and
//! refused by `test`, and never followed. `test` streams one short reply
//! through an egress client over the project's egress directory
//! ([`crate::egress::ProjectDirectory`]), which pins the Account to its
//! endpoint (scheme, host and port); a block the client reports is printed
//! and said not to be in the audit log, since `test` opens no store.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use am_config::{ACCOUNTS_FILE, AccountUse, ConfigCreate};
use am_domain::{
    AuditEvent, AuditNote, AuditSink, AuthRef, AuthScheme, ChatRequest, ConfigHistory,
    EgressClient, EgressDirectory, EventSink, Instruction, LlmProvider, Message, Part,
    ProjectIdentity, Role, SecretStore, StreamEvent, Terminal, TerminalError,
};
use am_engine::TokenSignal;
use am_providers::anthropic::ANTHROPIC_HOST;
use am_providers::openai_compat::{OPENAI_BASE_URL, OPENAI_HOST};
use am_providers::{AnthropicProvider, OpenAiCompatProvider};
use tokio_util::sync::CancellationToken;

use crate::cli::{EXIT_ERRORS, EXIT_OK, EXIT_USAGE};
use crate::runtime::{Places, owner};

/// The exit code after Ctrl-C at a prompt (128 + SIGINT, as shells report it).
pub const EXIT_INTERRUPTED: i32 = 130;

/// How long the owner has to type the Account's id (AD-15).
pub const CONFIRM_WITHIN: Duration = Duration::from_secs(120);

/// The model providers an Account can be added for and tested from the
/// command line.
const ANTHROPIC: &str = "anthropic";
const OPENAI: &str = "openai";
const OPENAI_COMPATIBLE: &str = "openai-compatible";

/// The three providers as a refusal or a hint names them.
const PROVIDERS: &str =
    "`--provider anthropic`, `--provider openai` or `--provider openai-compatible`";

const USAGE: &str = "usage: agentic accounts add <id> --provider anthropic|openai|openai-compatible [--base-url <url>] [--name <text>] [--project <folder>]\n       agentic accounts remove <id> [--project <folder>]\n       agentic accounts list [--project <folder>]\n       agentic accounts test <id> [--model <model>] [--project <folder>]\n       (--base-url is for openai-compatible only: https://<host>[/path], or http://127.0.0.1:<port>[/path] for a server on this Mac;\n        --model is required for an openai or openai-compatible Account; --project defaults to the current folder)";

/// Builds the egress client over a directory, reporting blocks to a sink.
pub type EgressFactory = Arc<
    dyn Fn(Arc<dyn EgressDirectory>, Arc<dyn AuditSink>) -> Arc<dyn EgressClient> + Send + Sync,
>;

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
    /// Builds the egress client over the project's egress directory,
    /// reporting blocks to the command's audit sink (story 2.2).
    pub egress: EgressFactory,
}

/// The parsed command line.
enum Command {
    Add {
        id: String,
        provider: String,
        name: Option<String>,
        base_url: Option<String>,
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
            ("--base-url", "add") => "--base-url",
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
                .ok_or("accounts add needs --provider (anthropic, openai or openai-compatible)")?;
            Command::Add {
                id,
                provider,
                name: flags.remove("--name"),
                base_url: flags.remove("--base-url"),
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
        Command::Add {
            id,
            provider,
            name,
            base_url,
        } => add(
            ctx,
            &project,
            &id,
            &provider,
            name.as_deref(),
            base_url.as_deref(),
            out,
        ),
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

// --------------------------------------------------------- the endpoint

pub(crate) fn is_model_provider(provider: &str) -> bool {
    [ANTHROPIC, OPENAI, OPENAI_COMPATIBLE].contains(&provider)
}

/// Where an Account's calls go (AD-12, AD-48): the one endpoint the egress
/// directory pins it to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AccountEndpoint {
    /// `https`, or `http` for an endpoint on this Mac.
    pub(crate) scheme: &'static str,
    /// The DNS name in lowercase, or `127.0.0.1`.
    pub(crate) host: String,
    /// 443 for https; the explicit port of an endpoint on this Mac.
    pub(crate) port: u16,
    /// The base URL: the provider's fixed one, or a compatible Account's
    /// `base_url` as typed.
    pub(crate) base_url: String,
    /// True for `http://127.0.0.1:<port>`: the calls stay on this Mac.
    pub(crate) loopback: bool,
    /// True when the base URL comes from the Account (`base_url` in the
    /// file), not from its provider.
    pub(crate) own: bool,
}

impl AccountEndpoint {
    fn fixed(host: &str, base_url: &str) -> AccountEndpoint {
        AccountEndpoint {
            scheme: "https",
            host: host.to_string(),
            port: 443,
            base_url: base_url.to_string(),
            loopback: false,
            own: false,
        }
    }

    /// The `base_url` the Accounts file holds for it: none for a provider
    /// with one fixed endpoint.
    fn written(&self) -> Option<&str> {
        self.own.then_some(self.base_url.as_str())
    }

    /// The endpoint as the add's sentence names it.
    fn goes_to(&self) -> String {
        if self.loopback {
            format!(
                "{} and stay on this Mac, on port {}",
                self.base_url, self.port
            )
        } else {
            self.base_url.clone()
        }
    }

    /// The host as `list` shows it, with the port for an endpoint on this Mac.
    fn host_shown(&self) -> String {
        if self.loopback {
            format!("{}:{}", self.host, self.port)
        } else {
            self.host.clone()
        }
    }

    /// The base URL the adapter appends its path to.
    fn call_base(&self) -> &str {
        self.base_url.trim_end_matches('/')
    }

    /// The endpoint the egress client pins this Account to (AD-12, AD-48):
    /// its scheme, host and port, compared exactly at every call.
    pub(crate) fn to_domain(&self) -> am_domain::Endpoint {
        am_domain::Endpoint {
            scheme: self.scheme.to_string(),
            host: self.host.clone(),
            port: self.port,
        }
    }
}

/// The endpoint of an Account of `provider` with the optional `base_url`:
/// the one function `add`, `list` and `test` derive it with (AD-12, AD-48).
///
/// - `anthropic`: `https://api.anthropic.com`, port 443; no base URL.
/// - `openai`: `https://api.openai.com/v1`, port 443; no base URL.
/// - `openai-compatible`: the base URL is required, in one of two forms:
///   `https://<DNS name>[/path]` on port 443, or
///   `http://127.0.0.1:<port>[/path]` with an explicit port, for a server on
///   this Mac.
///
/// Everything else is refused in words; `flag` is how the refusal names the
/// base URL (`--base-url` on the command line, `base_url` in the file).
pub(crate) fn endpoint_of(
    provider: &str,
    base_url: Option<&str>,
    flag: &str,
) -> Result<AccountEndpoint, String> {
    let fixed = |host: &str, url: &str| match base_url {
        None => Ok(AccountEndpoint::fixed(host, url)),
        Some(_) => Err(format!(
            "`{flag}` is not accepted for an {provider} Account: its calls go only to {url}. For another endpoint use an {OPENAI_COMPATIBLE} Account (`--provider {OPENAI_COMPATIBLE} --base-url <url>`)"
        )),
    };
    match provider {
        ANTHROPIC => fixed(ANTHROPIC_HOST, &format!("https://{ANTHROPIC_HOST}")),
        OPENAI => fixed(OPENAI_HOST, OPENAI_BASE_URL),
        OPENAI_COMPATIBLE => {
            let Some(typed) = base_url else {
                return Err(format!(
                    "an {OPENAI_COMPATIBLE} Account needs `{flag}`, the endpoint its calls go to: give {ACCEPTED_FORMS}"
                ));
            };
            // What a refusal calls the URL. It is repeated only when it is
            // plain text with no user name, password or query in it: a key
            // someone put in a URL is never printed, and neither is a
            // control character from a hand-written file.
            let plain = typed
                .chars()
                .all(|c| c.is_ascii_graphic() && !['@', '?', '#', '\\', '`'].contains(&c));
            let said = if plain {
                format!("`{flag} {typed}`")
            } else {
                format!("the `{flag}` given")
            };
            let endpoint = compatible_endpoint(typed)
                .map_err(|why| format!("{said} is not accepted: {why}"))?;
            // The egress client must read the URL of a call as the same
            // endpoint at call time, or it is not one.
            let call = format!("{}/chat/completions", endpoint.call_base());
            match am_net::url_target(&call) {
                Ok(read)
                    if read.endpoint() == Some(endpoint.to_domain())
                        && !read.userinfo
                        && (read.ip.is_some() == endpoint.loopback) =>
                {
                    Ok(endpoint)
                }
                Ok(_) => Err(format!(
                    "{said} is not accepted: the app's network check reads another host or port in it than the one written; give {ACCEPTED_FORMS}"
                )),
                Err(e) => Err(format!(
                    "{said} is not accepted: the app's network check refuses it ({e}); give {ACCEPTED_FORMS}"
                )),
            }
        }
        other => Err(format!("'{other}' is not a model provider ({PROVIDERS})")),
    }
}

/// The two forms a compatible Account's base URL takes.
const ACCEPTED_FORMS: &str = am_config::COMPATIBLE_BASE_URL_FORMS;

/// A compatible Account's base URL, read by the one rule the loader applies
/// too ([`am_config::compatible_base_url`]): only the two accepted forms
/// pass, and the reason is given for everything else.
fn compatible_endpoint(typed: &str) -> Result<AccountEndpoint, String> {
    let accepted = am_config::compatible_base_url(typed)?;
    Ok(AccountEndpoint {
        scheme: if accepted.loopback { "http" } else { "https" },
        host: accepted.host,
        port: accepted.port,
        base_url: typed.to_string(),
        loopback: accepted.loopback,
        own: true,
    })
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

pub(crate) fn reserved(id: &str) -> bool {
    am_config::reserved_account_ids().iter().any(|r| r == id)
}

fn uses_words(uses: &[AccountUse]) -> String {
    if uses.is_empty() {
        return "nothing".to_string();
    }
    let all: Vec<String> = uses.iter().map(AccountUse::to_string).collect();
    all.join(", ")
}

/// What a refused `add` says when the Keychain entry of the id is not as
/// the sentence said (held, or not held) once a prompt has returned.
fn entry_changed(id: &str) -> String {
    format!(
        "the Keychain entry for '{id}' changed while you typed; run the command again (the entry is left as it is now: this command neither stored nor deleted it)"
    )
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
    base_url: Option<&str>,
    out: &mut dyn Write,
) -> Result<(), Stop> {
    require_terminal(&ctx, "add", NOT_ADDED)?;
    if !is_model_provider(provider) {
        return refuse(format!(
            "an Account can be added from the command line for a model provider only ({PROVIDERS}), not '{provider}'. {NOT_ADDED}"
        ));
    }
    let endpoint = endpoint_of(provider, base_url, "--base-url")
        .map_err(|why| Stop(EXIT_ERRORS, format!("{why}. {NOT_ADDED}")))?;
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
    let named = if uses.is_empty() {
        "Nothing in the project names it yet.".to_string()
    } else {
        format!(
            "Already naming it, and brought into force by this add: {}.",
            uses_words(&uses)
        )
    };
    // Whether the Keychain already holds a key for this id (an earlier add,
    // another project): only that it is there, never its value. That key
    // would be reused, so the sentence says where it will go before the id
    // is typed.
    let held = match ctx.secrets.get(id) {
        Ok(key) => key.is_some(),
        Err(e) => {
            return refuse(format!("the Keychain cannot be read ({e}). {NOT_ADDED}"));
        }
    };
    let reused = if held {
        format!(
            " This Mac's Keychain already holds a key for '{id}': that key is reused, and it will be sent to {}.",
            endpoint.base_url
        )
    } else {
        String::new()
    };
    ctx.terminal.show(&format!(
        "Adding the Account '{id}' (provider {provider}): its calls go to {}, with a key kept only in this Mac's Keychain.{reused} {named}\nTo confirm, type the Account's id ({id}) within 2 minutes: ",
        endpoint.goes_to()
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
    // and never overwritten; otherwise it is read with echo off. The
    // sentence said which of the two it is, so the Keychain is read again
    // now that the id is typed: an entry that appeared or went away while
    // the prompt waited refuses the add (the owner confirmed a sentence
    // that no longer says which key goes to the endpoint).
    let new_key = match ctx.secrets.get(id) {
        Err(e) => {
            return refuse(format!("the Keychain cannot be read ({e}). {NOT_ADDED}"));
        }
        Ok(now) if now.is_some() != held => {
            return refuse(format!("{}. {NOT_ADDED}", entry_changed(id)));
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
        // meanwhile is never overwritten, never deleted (this command did
        // not create it) and never used, because the sentence said no key
        // was held. The typed key is discarded and the add is refused.
        Some(key) => match ctx.secrets.get(id) {
            Err(e) => {
                return refuse(format!(
                    "the Keychain cannot be read ({e}). {NOT_ADDED_NOR_STORED}"
                ));
            }
            Ok(Some(_)) => {
                drop(key);
                return refuse(format!(
                    "{}; the key you typed was discarded. {NOT_ADDED_NOR_STORED}",
                    entry_changed(id)
                ));
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
            ("scheme".to_string(), endpoint.scheme.to_string()),
            ("host".to_string(), endpoint.host.clone()),
            ("port".to_string(), endpoint.port.to_string()),
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
            am_config::add_account_edit(&file, id, provider, name, endpoint.written()),
            built_on,
            vec![note],
        ),
        None => {
            am_config::new_accounts_file(id, provider, name, endpoint.written()).and_then(|bytes| {
                writer.create(
                    owner(),
                    ConfigCreate {
                        file: ACCOUNTS_FILE.into(),
                        bytes,
                        audit: vec![note],
                    },
                )
            })
        }
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
        "agentic: added the Account '{id}' ({provider}, {}) to {ACCOUNTS_FILE} (History entry {}); {key}",
        endpoint.base_url, entry.id
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

/// The host an Account's calls go to, as `list` shows it: the endpoint's
/// host, with its port for an endpoint on this Mac. A model Account whose
/// `base_url` is not an endpoint this version calls says so, and names no
/// host. For another kind of Account (search, crawl) it is the host of its
/// `base_url`, when it has one.
fn host_of(account: &am_config::Account) -> String {
    if is_model_provider(&account.provider) {
        return match endpoint_of(&account.provider, account.base_url.as_deref(), "base_url") {
            Ok(endpoint) => endpoint.host_shown(),
            Err(_) => "(base_url not accepted; `agentic accounts test` says why)".to_string(),
        };
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
            "agentic: no Accounts in {ACCOUNTS_FILE} yet; add one with `agentic accounts add <id>` and {PROVIDERS} (the local Accounts local-lmstudio and local-ollama are always available)"
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
        // An entry with an error is shown with no host: what the loader
        // could read of it is not what would be called.
        let host = if file.entry_errors(&account.id).is_empty() {
            host_of(account)
        } else {
            "(its entry has an error; `agentic validate` names it)".to_string()
        };
        let _ = writeln!(out, "{}  {}  {host}  {key}", account.id, account.provider);
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
    // An entry with an error is never called: the loader returns what it
    // could read of it (a `base_url` it refuses reads as none).
    let errors = file.entry_errors(id);
    if !errors.is_empty() {
        return refuse(format!(
            "the Account '{id}' is not called: its entry in {ACCOUNTS_FILE} has an error ({}); `agentic validate` names the line and the fix. No key was read and nothing was sent",
            errors.join("; ")
        ));
    }
    if !is_model_provider(&account.provider) {
        return refuse(format!(
            "the Account '{id}' is a {} Account, not a model Account (anthropic, openai or openai-compatible), the only kinds `agentic accounts test` calls; nothing was sent",
            account.provider
        ));
    }
    // The endpoint is derived by the rules `add` applies, whatever a
    // hand-written file says: a `base_url` outside them is never called.
    let endpoint = endpoint_of(&account.provider, account.base_url.as_deref(), "base_url")
        .map_err(|why| {
            Stop(
                EXIT_ERRORS,
                format!(
                    "the Account '{id}' in {ACCOUNTS_FILE} cannot be called: {why}; no key was read and nothing was sent"
                ),
            )
        })?;
    if let Some(entry) = other_entry(account) {
        return refuse(format!(
            "the Account '{id}' has `secret_ref: {entry}`, which names another Keychain entry; a key is only ever read from the entry named by the Account's own id, so no key was read and nothing was sent. Set `secret_ref: {id}` (or remove the line) in {ACCOUNTS_FILE}"
        ));
    }
    // A key goes only to what the owner confirmed (AD-15, AD-16, AD-48):
    // before any Keychain read, the file must be as the app last wrote it
    // and the latest `account_added` note of this Account must name its
    // provider and endpoint.
    // The project's egress directory, read now: the client is built over
    // it, and the app's own port it records (while `agentic serve` runs)
    // is refused before any Keychain read (AD-12).
    let directory = crate::egress::ProjectDirectory::open(project, &ctx.places)
        .map_err(|why| Stop(EXIT_ERRORS, format!("{why}; nothing was sent")))?;
    if endpoint.loopback && directory.own_ports().contains(&endpoint.port) {
        return refuse(format!(
            "the Account '{id}' is not called: {}, and an Account never reaches the app itself. No key was read and nothing was sent",
            crate::egress::own_server_words(endpoint.port)
        ));
    }
    if let Err(why) = crate::egress::confirmed(&ctx.places, project, &file, account, &endpoint) {
        let base_url = if endpoint.own {
            " --base-url <url>"
        } else {
            ""
        };
        return refuse(format!(
            "the Account '{id}' is not called: it was not confirmed by `agentic accounts add` on this Mac, or {ACCOUNTS_FILE} changed since ({why}). No key was read or sent. If {ACCOUNTS_FILE} was changed by hand, put it back as the app last wrote it; otherwise remove the Account and add it again in a terminal (`agentic accounts remove {id}`, then `agentic accounts add {id} --provider {}{base_url}`)",
            account.provider
        ));
    }
    // No default model id is assumed for a provider whose model list this
    // app does not hold.
    let model = match (model, account.provider.as_str()) {
        (Some(model), _) => model,
        (None, ANTHROPIC) => crate::DEFAULT_ANTHROPIC_MODEL.to_string(),
        (None, provider) => {
            return refuse(format!(
                "`agentic accounts test {id}` needs `--model <model>`: no default model is assumed for an {provider} Account; name one its endpoint ({}) serves. No key was read and nothing was sent",
                endpoint.base_url
            ));
        }
    };
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
    // Only the Account's pinned endpoint (AD-12, AD-48): the client asks
    // the project's egress directory, which answers this Account only while
    // it is confirmed. `test` opens no store (it works while `serve` holds
    // the project), so a block is collected and printed, not audited
    // (owner decision 3, 2026-11-22).
    let blocks = Arc::new(Blocks::default());
    let egress = (ctx.egress)(Arc::new(directory), blocks.clone());
    // The adapter by provider. OpenAI and compatible Accounts authenticate
    // with `Bearer` from the Keychain entry named by the Account's id.
    let provider: Box<dyn LlmProvider> = if account.provider == ANTHROPIC {
        Box::new(AnthropicProvider::new(egress, id))
    } else {
        Box::new(
            OpenAiCompatProvider::new(egress, id, endpoint.call_base()).with_auth(AuthRef {
                scheme: AuthScheme::Bearer,
                account_id: id.to_string(),
            }),
        )
    };
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
                "agentic: the Account '{id}' answered from {} (model {model}, {tokens})",
                endpoint.base_url
            );
            Ok(())
        }
        Err(e) => refuse(format!(
            "the test call of the Account '{id}' (model {model}) failed: {e}{}",
            blocks.words()
        )),
    }
}

/// The blocks the egress client reported during `test`: printed, because
/// the command opens no store to audit them in.
#[derive(Default)]
struct Blocks(std::sync::Mutex<Vec<AuditEvent>>);

impl AuditSink for Blocks {
    fn record(&self, event: AuditEvent) {
        if let Ok(mut all) = self.0.lock() {
            all.push(event);
        }
    }
}

impl Blocks {
    /// Each block's destination and reason, and that it is not in the
    /// audit log; empty when there was none.
    fn words(&self) -> String {
        let all = self.0.lock().map(|a| a.clone()).unwrap_or_default();
        let mut words = String::new();
        for event in all.iter().filter(|e| e.action == "egress.blocked") {
            let arg = |k: &str| event.args.get(k).map(String::as_str).unwrap_or("?");
            words.push_str(&format!(
                ". The app's network check blocked {}://{}:{} ({}); this block is not in the audit log, because `agentic accounts test` opens no store, so that it works while `agentic serve` holds the project",
                arg("scheme"),
                arg("host"),
                arg("port"),
                arg("reason"),
            ));
        }
        words
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compatible(url: &str) -> Result<AccountEndpoint, String> {
        endpoint_of(OPENAI_COMPATIBLE, Some(url), "--base-url")
    }

    /// The two accepted forms, and what each pins.
    #[test]
    fn the_two_accepted_forms_give_their_scheme_host_and_port() {
        let remote = compatible("https://Relay.Example/api/v1/").unwrap();
        assert_eq!(
            (remote.scheme, remote.host.as_str(), remote.port),
            ("https", "relay.example", 443)
        );
        assert_eq!(remote.base_url, "https://Relay.Example/api/v1/", "as typed");
        assert_eq!(remote.call_base(), "https://Relay.Example/api/v1");
        assert_eq!(
            remote.to_domain(),
            am_domain::Endpoint {
                scheme: "https".into(),
                host: "relay.example".into(),
                port: 443
            }
        );
        assert_eq!(compatible("https://relay.example:443").unwrap().port, 443);

        let local = compatible("http://127.0.0.1:80").unwrap();
        assert_eq!(
            (
                local.scheme,
                local.host.as_str(),
                local.port,
                local.loopback
            ),
            ("http", "127.0.0.1", 80, true)
        );
        assert_eq!(
            local.to_domain(),
            am_domain::Endpoint {
                scheme: "http".into(),
                host: "127.0.0.1".into(),
                port: 80
            }
        );
        assert_eq!(local.host_shown(), "127.0.0.1:80");
    }

    /// No spelling of an address passes as a DNS name, and nothing that the
    /// egress check would read as another host or port is accepted.
    #[test]
    fn an_address_in_any_spelling_and_an_odd_url_are_refused() {
        for url in [
            "https://2130706433/v1",
            "https://0x7f.0.0.1/v1",
            "https://127.1/v1",
            "https://[fd00::1]/v1",
            "https://[::ffff:10.0.0.5]/v1",
            "https://localhost/v1",
            "https://api.localhost/v1",
            "http://127.0.0.2:8080/v1",
            "http://0.0.0.0:8080/v1",
            "http://127.0.0.1:0/v1",
            "http://127.0.0.1:99999/v1",
            "http://127.0.0.1:+80/v1",
            "https://relay.example:/v1",
            "https://relay.example.:443/v1",
            "https://relay.example\\@other.example/v1",
            "https://relay.example /v1",
            "https://réseau.example/v1",
            "HTTPS://relay.example/v1",
            "ftp://relay.example/v1",
            "relay.example/v1",
            "https:///v1",
            "",
        ] {
            assert!(compatible(url).is_err(), "{url} was accepted");
        }
    }

    /// A refusal never repeats a URL that holds a user name, a password or a
    /// query: a key someone put in a URL is not printed.
    #[test]
    fn a_refusal_never_repeats_a_url_that_could_hold_a_key() {
        for url in [
            "https://someone:sk-canary-in-a-url@relay.example/v1",
            "https://relay.example/v1?key=sk-canary-in-a-url",
            "http://192.168.1.4:8080/v1#sk-canary-in-a-url",
        ] {
            let why = compatible(url).unwrap_err();
            assert!(!why.contains("sk-canary-in-a-url"), "{why}");
            assert!(why.contains("--base-url"), "{why}");
        }
    }

    /// The fixed providers take no base URL; another provider has no endpoint.
    #[test]
    fn a_fixed_provider_has_one_endpoint_and_takes_no_base_url() {
        let anthropic = endpoint_of(ANTHROPIC, None, "base_url").unwrap();
        assert_eq!(anthropic.base_url, "https://api.anthropic.com");
        assert_eq!(anthropic.written(), None);
        let openai = endpoint_of(OPENAI, None, "base_url").unwrap();
        assert_eq!(
            (openai.scheme, openai.host.as_str(), openai.port),
            ("https", "api.openai.com", 443)
        );
        assert_eq!(openai.base_url, "https://api.openai.com/v1");
        assert_eq!(openai.written(), None);
        for provider in [ANTHROPIC, OPENAI] {
            assert!(endpoint_of(provider, Some("https://relay.example/v1"), "base_url").is_err());
        }
        assert!(endpoint_of(OPENAI_COMPATIBLE, None, "base_url").is_err());
        assert!(endpoint_of("brave", None, "base_url").is_err());
    }
}
