//! `agentic accounts add|change|remove|list|test|migrate` (story 2.1;
//! `migrate`, story 2.13, is in [`migrate`]; `change`, story 2.14, the only
//! way to confirm or re-confirm an Account, is in [`change`]): AD-15's interim CLI
//! Account edit, and the two reads beside it. Story 2.3 adds the providers
//! `openai` and `openai-compatible`; one function, [`endpoint_of`], derives
//! an Account's endpoint (scheme, host, port, base URL) for every command.
//!
//! Story 2.12 (AD-17): Accounts belong to this Mac. The commands take no
//! project and open none: they read and write the user Accounts file,
//! `<app data dir>/accounts.yaml`, and record in the user store, `user.db`
//! ([`crate::user`]). References are found in every project of the runtime
//! base's root records whose root is reachable, read only through
//! am-config; the projects that could not be reached are named as not
//! checked.
//!
//! `add` and `remove` are the only config edits the CLI makes. They write
//! only the user Accounts file, through am-config's writer, as the owner
//! (`Actor::Human`, the Owner hat). Their checks run in this order, holding
//! no lock, and the first that fails is what the owner reads; nothing is
//! read from the terminal, written or stored before all of them passed:
//!
//! 1. usage (exit 2);
//! 2. the terminal: stdin and stdout are both a real interactive terminal
//!    ([`crate::terminal::interactive`], through the `Terminal` port);
//! 3. the provider and its endpoint (`add`);
//! 4. the app data dir is owner-only and the user store is this Mac's;
//! 5. the user file has no outside change (its bytes differ from its latest
//!    History entry's), naming the file;
//! 6. the id: not reserved, not already in the list (`add`); in the list
//!    (`remove`);
//! 7. the scan: no scanned project enables a stdio MCP server or a Source
//!    with `render: true` (children that run as the owner, AD-13), naming
//!    the project; `remove` also refuses an id a reference still names, and
//!    while a recorded project could not be reached (`--force` lifts only
//!    that one).
//!
//! `add` then prints one sentence naming the Account, its provider, the
//! endpoint its calls go to and every reference found (and the projects not
//! checked), and requires the id typed within 2 minutes. An entry the
//! Keychain already holds is reused and never overwritten, and the sentence
//! says so; when the user store holds no confirmation of that id for this
//! provider and endpoint, the stored key must also be typed at the hidden
//! prompt and equal the entry. Otherwise the key is read with echo off.
//!
//! Then `user.lock` is taken (a bounded wait), which settles a write a crash
//! interrupted (reported), and everything is read again: anything that
//! changed since the sentence (the file's bytes other than by an app write
//! that left the id as it was, the id, the references, an outside change, a
//! child) refuses the command, to be run again. A Ctrl-C pressed since the
//! id was typed cancels (exit 130), before the key is stored and again
//! before the file is written. The key is stored only in the Keychain
//! (never over an entry that appeared meanwhile), then the file is written
//! with its History entry and the `account_added` record in one user-store
//! transaction; if the write fails, the Keychain entry is deleted only when
//! this command created it. `remove` writes `account_removed` with its
//! History entry and leaves the Keychain entry in place.
//!
//! `list` and `test` make no config edit, take no lock and run with any
//! stdin. `test` calls only an Account [`crate::key_use`] passes, reads its
//! key, compares the user store's stamp again before anything is sent (a
//! record that moved drops the key unsent and the check runs again), and
//! streams one short reply through an egress client over a directory that
//! pins the Account to its endpoint.
//!
//! Story 2.10 (AD-15 *Keyless loopback Account*): `add --no-key` and
//! `change --no-key` take an `openai-compatible` Account at
//! `http://127.0.0.1:<port>` only (refused in words before any prompt for
//! every other endpoint) and write no `secret_ref`; a keyless Account's
//! add, `test`, `list` line, `remove` and change make no Keychain call of
//! any kind, and `test` sends no credential. `add` also refuses, before any
//! prompt, an endpoint on this Mac at a reserved local Account's port or at
//! one of the app's own ports.

use std::collections::BTreeMap;
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use am_config::{ConfigCreate, KeyState, USER_ACCOUNTS_FILE};
use am_domain::{
    AuditEvent, AuditNote, AuditSink, AuthRef, AuthScheme, ChatRequest, ConfigHistory,
    EgressClient, EgressDirectory, EgressError, EventSink, Instruction, LlmProvider, Message, Part,
    ProjectIdentity, ProviderError, Role, SecretStore, StreamEvent, Terminal, TerminalError,
};
use am_engine::TokenSignal;
use am_providers::anthropic::ANTHROPIC_HOST;
use am_providers::openai_compat::{OPENAI_BASE_URL, OPENAI_HOST};
use am_providers::{AnthropicProvider, OpenAiCompatProvider};
use tokio_util::sync::CancellationToken;

mod change;
mod migrate;

use crate::cli::{EXIT_ERRORS, EXIT_OK, EXIT_USAGE};
use crate::key_use::{self, Refused};
use crate::runtime::{Places, owner};
use crate::user::{Locked, Scan, Standing};

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

const USAGE: &str = "usage: agentic accounts add <id> --provider anthropic|openai|openai-compatible [--base-url <url>] [--name <text>] [--no-key]\n       agentic accounts change <id> [--provider anthropic|openai|openai-compatible] [--base-url <url>] [--no-key]\n       agentic accounts remove <id> [--force]\n       agentic accounts list\n       agentic accounts test <id> [--model <model>]\n       agentic accounts migrate --project <path> [--as <old>=<new>]... [--dry-run | --rollback]\n       (Accounts belong to this Mac: no command but migrate takes a project;\n        migrate moves a project's old models/accounts.yaml to this Mac's Accounts, --as names a clashing id anew, --dry-run only shows what it would do, --rollback puts the file back from its backup;\n        --base-url is for openai-compatible only: https://<host>[/path], or http://127.0.0.1:<port>[/path] for a server on this Mac;\n        change confirms an Account again: a new address of the same provider keeps its key, a new provider (or an Account with no confirmation) asks for a new key and deletes the old one;\n        --no-key adds or confirms an openai-compatible Account at http://127.0.0.1:<port> with no key: its calls stay on this Mac and carry no credential;\n        --model is required for an openai or openai-compatible Account;\n        --force removes even while a project this Mac knows cannot be reached, never while a reference names the Account)";

/// Builds the egress client over a directory, reporting blocks to a sink.
pub type EgressFactory = Arc<
    dyn Fn(Arc<dyn EgressDirectory>, Arc<dyn AuditSink>) -> Arc<dyn EgressClient> + Send + Sync,
>;

/// What `agentic accounts` runs with. `main` builds it with the owner's real
/// terminal, the Keychain, the OS identity adapter and `am-net`'s client;
/// tests pass am-testkit's fakes.
pub struct AccountsContext {
    pub places: Places,
    /// The OS identity adapter: no command here opens a project since story
    /// 2.12; `accounts migrate` (story 13) resolves one with it.
    pub identity: Arc<dyn ProjectIdentity>,
    pub terminal: Box<dyn Terminal>,
    pub secrets: Arc<dyn SecretStore>,
    /// Builds the egress client over the directory `test` builds,
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
        /// Story 2.10: keyless, on this Mac only.
        no_key: bool,
    },
    Remove {
        id: String,
        force: bool,
    },
    /// Story 2.14: `change <id> [--provider P] [--base-url U]`; story 2.10
    /// adds `--no-key`.
    Change {
        id: String,
        provider: Option<String>,
        base_url: Option<String>,
        no_key: bool,
    },
    List,
    Test {
        id: String,
        model: Option<String>,
    },
    /// Story 2.13: `migrate --project <path> …`.
    Migrate(migrate::Migrate),
}

fn parse(args: &[String]) -> Result<Command, String> {
    let Some(verb) = args.first().map(String::as_str) else {
        return Err("accounts needs `add`, `change`, `remove`, `list`, `test` or `migrate`".into());
    };
    if verb == "migrate" {
        return migrate::parse(&args[1..]).map(Command::Migrate);
    }
    if !["add", "change", "remove", "list", "test"].contains(&verb) {
        return Err(format!("unknown accounts command `{verb}`"));
    }
    let mut positional: Vec<String> = Vec::new();
    let mut flags: BTreeMap<&str, String> = BTreeMap::new();
    let mut force = false;
    let mut no_key = false;
    let mut words = args[1..].iter();
    while let Some(word) = words.next() {
        let flag = match (word.as_str(), verb) {
            ("--force", "remove") => {
                if force {
                    return Err("--force is given twice".into());
                }
                force = true;
                continue;
            }
            ("--no-key", "add" | "change") => {
                if no_key {
                    return Err("--no-key is given twice".into());
                }
                no_key = true;
                continue;
            }
            ("--provider", "add") => "--provider",
            ("--name", "add") => "--name",
            ("--base-url", "add") => "--base-url",
            ("--provider", "change") => "--provider",
            ("--base-url", "change") => "--base-url",
            ("--model", "test") => "--model",
            ("--project", _) => {
                return Err(format!(
                    "`accounts {verb}` takes no --project: Accounts belong to this Mac, not to a project"
                ));
            }
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
    let one_id = |what: &str| match positional.as_slice() {
        [id] => Ok(id.clone()),
        [] => Err(format!("accounts {verb} needs {what}")),
        _ => Err(format!("accounts {verb} takes one Account id")),
    };
    Ok(match verb {
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
                no_key,
            }
        }
        "remove" => Command::Remove {
            id: one_id("the Account's id")?,
            force,
        },
        "change" => Command::Change {
            id: one_id("the Account's id")?,
            provider: flags.remove("--provider"),
            base_url: flags.remove("--base-url"),
            no_key,
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
    })
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
    let command = match parse(args) {
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
            no_key,
        } => add(
            ctx,
            &id,
            &provider,
            name.as_deref(),
            base_url.as_deref(),
            no_key,
            out,
        ),
        Command::Remove { id, force } => remove(ctx, &id, force, out),
        Command::Change {
            id,
            provider,
            base_url,
            no_key,
        } => change::change(
            ctx,
            &id,
            provider.as_deref(),
            base_url.as_deref(),
            no_key,
            out,
        ),
        Command::List => list(&ctx, out),
        Command::Test { id, model } => test(&ctx, &id, model, out).await,
        Command::Migrate(m) => migrate::run(ctx, m, out),
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

/// What a refused `add` did not do, while no key has been read yet. (Taking
/// the lock may have settled an interrupted write, so no refusal says that
/// nothing was written.)
const NOT_ADDED: &str = "No Account was added and no key was read or stored";
/// What a refused `add` did not do, once a key was typed.
const NOT_ADDED_NOR_STORED: &str = "No Account was added and no key was stored";
const NOT_REMOVED: &str = "No Account was removed";

/// Check 2: the terminal rule of AD-15. `what` is `add` or `remove`.
fn require_terminal(ctx: &AccountsContext, what: &str, did_not: &str) -> Result<(), Stop> {
    if ctx.terminal.interactive() {
        return Ok(());
    }
    refuse(format!(
        "`agentic accounts {what}` runs only in an interactive terminal: stdin and stdout must both be a terminal, not a pipe, a file or another program, because adding, changing or removing an Account changes what this Mac's projects may call and only you decide that. {did_not}; run the command yourself in a terminal"
    ))
}

/// Checks 4 and 5: the user store can be read and the file has no outside
/// change; the file is readable as Accounts.
fn stand(places: &Places, did_not: &str) -> Result<Standing, Stop> {
    let standing =
        Standing::read(places).map_err(|why| Stop(EXIT_ERRORS, format!("{why}. {did_not}")))?;
    if standing.outside_change() {
        return refuse(crate::user::outside_words(places, did_not));
    }
    if let Some(problem) = &standing.file.problem {
        return refuse(format!(
            "{USER_ACCOUNTS_FILE} ({}) cannot be changed because {problem}; fix the file first (`agentic validate` names the line). {did_not}",
            places.user.accounts_file().display()
        ));
    }
    Ok(standing)
}

/// Check 7's first half: no scanned project enables a child that runs as
/// the owner (AD-13, AD-15).
fn no_owner_child(scan: &Scan, what: &str, did_not: &str) -> Result<(), Stop> {
    let Some((root, why)) = scan.children.first() else {
        return Ok(());
    };
    refuse(format!(
        "`agentic accounts {what}` cannot be used from the command line while the project {} has {}: a program that runs as you could type here. {did_not}; remove or disable it first",
        crate::user::shown(&root.display().to_string()),
        crate::user::shown(why)
    ))
}

/// The user lock, taken for the fresh re-read and the write; what settling
/// interrupted writes did is printed at once.
fn lock(places: &Places, did_not: &str, out: &mut dyn Write) -> Result<Locked, Stop> {
    let locked =
        Locked::take(places).map_err(|why| Stop(EXIT_ERRORS, format!("{why}. {did_not}")))?;
    for line in &locked.lines {
        let _ = writeln!(out, "agentic: {line}");
    }
    Ok(locked)
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

/// After a write of this command failed: whether the file now holds the
/// bytes its intent meant to write, with that intent still pending (so the
/// next lock records the History entry). `None` when the store cannot be
/// read to tell.
fn written_pending(places: &Places, history: &dyn ConfigHistory) -> Option<bool> {
    let Some(on_disk) = am_config::accounts_file(places.user.dir()).sha256 else {
        return Some(false);
    };
    let pending = history.pending_intents().ok()?;
    Some(
        pending
            .iter()
            .any(|p| p.intent.file == USER_ACCOUNTS_FILE && p.intent.after_sha256 == on_disk),
    )
}

pub(crate) fn reserved(id: &str) -> bool {
    am_config::reserved_account_ids().iter().any(|r| r == id)
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

/// The projects not checked, in words (empty when every one was).
fn not_checked(scan: &Scan) -> String {
    if scan.unreachable.is_empty() {
        String::new()
    } else {
        format!(
            " These projects this Mac knows could not be reached and were not checked: {}.",
            scan.unreachable_words()
        )
    }
}

/// Everything the edit checked before its prompts, read again under the
/// lock: the owner may have taken minutes, and the edit, the sentence and
/// the record were built on the first read. Anything that changed since
/// refuses the edit (run it again). Returns the fresh read to build on.
/// What the owner was shown before the prompts: the read of the file and
/// its records, and the scan.
struct Shown<'a> {
    standing: &'a Standing,
    scan: &'a Scan,
}

fn nothing_changed_since(
    places: &Places,
    locked: &Locked,
    id: &str,
    present: bool,
    shown: Shown<'_>,
    what: &str,
    did_not: &str,
) -> Result<(Standing, Scan), Stop> {
    let (before, scan) = (shown.standing, shown.scan);
    let again = format!("{did_not}; run the command again");
    let now = locked
        .standing(places)
        .map_err(|why| Stop(EXIT_ERRORS, format!("{why}. {did_not}")))?;
    if now.outside_change() {
        return refuse(crate::user::outside_words(places, did_not));
    }
    if !before.still(&now, id, present) {
        return refuse(format!(
            "{USER_ACCOUNTS_FILE} changed while this command waited at its prompt, so the {what} was built on what the file no longer holds. {again}"
        ));
    }
    let scan_now = crate::user::scan(places, id);
    no_owner_child(&scan_now, what, did_not)?;
    if scan_now.uses != scan.uses {
        return refuse(format!(
            "what names the Account '{id}' changed while this command waited: it is now named by {}, and the sentence you saw said {}. {again}",
            scan_now.uses_words(),
            scan.uses_words()
        ));
    }
    Ok((now, scan_now))
}

/// Story 2.10 (AD-15 *Keyless loopback Account*, owner Q2): why `--no-key`
/// is refused for `endpoint`, or `None` for an endpoint on this Mac.
fn no_key_refusal(provider: &str, endpoint: &AccountEndpoint, did_not: &str) -> Option<String> {
    if endpoint.loopback {
        return None;
    }
    Some(if endpoint.own {
        format!(
            "`--no-key` is accepted only for an {OPENAI_COMPATIBLE} Account at http://127.0.0.1:<port> on this Mac: a remote endpoint always needs a key. {did_not}; leave out `--no-key`"
        )
    } else {
        format!(
            "`--no-key` is not accepted for an {provider} Account: its calls go only to {}, a remote endpoint, which always needs a key. {did_not}; leave out `--no-key`",
            endpoint.base_url
        )
    })
}

/// Story 2.10 (owner Q1; review K4): an endpoint on this Mac at a reserved
/// local Account's port, or at a port one of the app's own servers uses, is
/// refused by `add` and `change`, before any prompt and again under the
/// lock: the egress client refuses every call there (AD-12), so no Account
/// there could ever be called.
fn not_a_taken_port(
    places: &Places,
    endpoint: &AccountEndpoint,
    did_not: &str,
) -> Result<(), Stop> {
    if !endpoint.loopback {
        return Ok(());
    }
    let reserved = am_domain::net::RESERVED_LOCAL_ACCOUNTS.iter().find(|r| {
        am_domain::net::reserved_local_account(r).is_some_and(|e| e.port == endpoint.port)
    });
    if let Some(reserved) = reserved {
        return refuse(format!(
            "the endpoint is not accepted: port {} on this Mac belongs to the reserved local Account {reserved}, which is always available, and no other Account is called there. {did_not}",
            endpoint.port
        ));
    }
    let directory = crate::egress::ProjectDirectory::for_accounts(places);
    if directory.own_ports().contains(&endpoint.port) {
        return refuse(format!(
            "the endpoint is not accepted: {}, and an Account never reaches the app itself. {did_not}",
            crate::egress::own_server_words(endpoint.port)
        ));
    }
    Ok(())
}

fn add(
    mut ctx: AccountsContext,
    id: &str,
    provider: &str,
    name: Option<&str>,
    base_url: Option<&str>,
    no_key: bool,
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
    if no_key && let Some(why) = no_key_refusal(provider, &endpoint, NOT_ADDED) {
        return refuse(why);
    }
    let places = ctx.places.clone();
    not_a_taken_port(&places, &endpoint, NOT_ADDED)?;
    let standing = stand(&places, NOT_ADDED)?;
    if reserved(id) {
        return refuse(format!(
            "'{id}' is a reserved local Account: it is always available and is never added. {NOT_ADDED}"
        ));
    }
    if standing.file.has(id) {
        return refuse(format!(
            "this Mac already has an Account '{id}' (in {USER_ACCOUNTS_FILE}). {NOT_ADDED}; choose another id"
        ));
    }
    if !am_config::is_config_id(id) {
        return refuse(format!(
            "'{id}' is not an Account id: use lowercase letters, digits and single hyphens (kebab-case), for example `cloud-main`. {NOT_ADDED}"
        ));
    }
    let scan = crate::user::scan(&places, id);
    no_owner_child(&scan, "add", NOT_ADDED)?;
    let named = if scan.uses.is_empty() {
        "Nothing in the projects this Mac knows names it yet.".to_string()
    } else {
        format!(
            "Already naming it, and brought into force by this add: {}.",
            scan.uses_words()
        )
    };
    // Whether the user store confirmed this id for this provider and
    // endpoint with a key: only then is a held key reused without typing it.
    // A keyless add (story 2.10) reads no key, so it never asks.
    let key_confirmed = |did_not: &str| -> Result<bool, Stop> {
        if no_key {
            return Ok(false);
        }
        Ok(key_use::confirmed_destination(&places, id)
            .map_err(|why| Stop(EXIT_ERRORS, format!("{why}. {did_not}")))?
            .is_some_and(|d| d.names(provider, &endpoint) && d.key_state() == KeyState::Stored))
    };
    let confirmed = key_confirmed(NOT_ADDED)?;
    // Whether the Keychain already holds a key for this id (an earlier add,
    // the per-project stage): only that it is there, never its value. A
    // keyless add makes no Keychain call of any kind (story 2.10).
    let held = !no_key
        && match ctx.secrets.get(id) {
            Ok(key) => key.is_some(),
            Err(e) => {
                return refuse(format!("the Keychain cannot be read ({e}). {NOT_ADDED}"));
            }
        };
    let reused = if held {
        // Where that key was last confirmed, also when a removal followed.
        let last = match key_use::last_confirmed(&places, id) {
            Ok(Some(d)) => format!(" It was last confirmed for {}.", d.words()),
            Ok(None) => " No confirmation of it is recorded on this Mac.".to_string(),
            Err(why) => return refuse(format!("{why}. {NOT_ADDED}")),
        };
        let typed = if confirmed {
            String::new()
        } else {
            " This Mac has no confirmation of that key for this provider and endpoint, so you will type it once more, hidden, to show it is yours.".to_string()
        };
        format!(
            " This Mac's Keychain already holds a key for '{id}': that key is reused, and it will be sent to {}.{last}{typed}",
            endpoint.base_url
        )
    } else {
        String::new()
    };
    let carries = if no_key {
        " with no key: no key is read or stored, and its calls carry no credential.".to_string()
    } else {
        format!(", with a key kept only in this Mac's Keychain.{reused}")
    };
    ctx.terminal.show(&format!(
        "Adding the Account '{id}' (provider {provider}) on this Mac, for every project: its calls go to {}{carries} {named}{}\nTo confirm, type the Account's id ({id}) within 2 minutes: ",
        endpoint.goes_to(),
        not_checked(&scan)
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
    // The key: an entry the Keychain already holds is reused and never
    // overwritten; otherwise it is read with echo off. The Keychain is read
    // again now that the id is typed: an entry that appeared or went away
    // while the prompt waited refuses the add. A keyless add reads none.
    let held_now = if no_key {
        Ok(None)
    } else {
        ctx.secrets.get(id)
    };
    let new_key = match held_now {
        _ if no_key => None,
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
            if !confirmed {
                ctx.terminal.show(&format!(
                    "Type the key the Keychain holds for '{id}' (not shown as you type or paste it): "
                ));
                let typed = match ctx.terminal.read_secret() {
                    Ok(key) => key,
                    Err(TerminalError::Interrupted) => return Err(interrupted(NOT_ADDED)),
                    Err(e) => {
                        return refuse(format!("no key was read ({e}). {NOT_ADDED}"));
                    }
                };
                let same = match ctx.secrets.get(id) {
                    Err(e) => {
                        return refuse(format!("the Keychain cannot be read ({e}). {NOT_ADDED}"));
                    }
                    Ok(None) => {
                        return refuse(format!("{}. {NOT_ADDED}", entry_changed(id)));
                    }
                    Ok(Some(stored)) => stored.expose() == typed.expose().trim(),
                };
                drop(typed);
                if !same {
                    return refuse(format!(
                        "the key you typed does not match the key the Keychain holds for '{id}'. {NOT_ADDED}; the Keychain entry is left as it is"
                    ));
                }
            }
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
    // The last prompt has returned. Under `user.lock`: every check again on
    // a fresh read, and a Ctrl-C pressed since the id was typed.
    let before_unreachable = scan.unreachable.clone();
    let locked = lock(&places, NOT_ADDED_NOR_STORED, out)?;
    let (now, scan) = nothing_changed_since(
        &places,
        &locked,
        id,
        false,
        Shown {
            standing: &standing,
            scan: &scan,
        },
        "add",
        NOT_ADDED_NOR_STORED,
    )?;
    // What the sentence said and what decided the stored-key prompt must
    // still hold: the projects not checked, and the confirmation.
    if scan.unreachable != before_unreachable {
        return refuse(format!(
            "the projects this Mac could not reach changed while this command waited (now: {}). {NOT_ADDED_NOR_STORED}; run the command again",
            if scan.unreachable.is_empty() {
                "none".to_string()
            } else {
                scan.unreachable_words()
            }
        ));
    }
    // The ports again (review K4): an `agentic serve` may have started on
    // this one while the prompt waited.
    not_a_taken_port(&places, &endpoint, NOT_ADDED_NOR_STORED)?;
    if key_confirmed(NOT_ADDED_NOR_STORED)? != confirmed {
        return refuse(format!(
            "this Mac's confirmation of '{id}' changed while this command waited. {NOT_ADDED_NOR_STORED}; run the command again"
        ));
    }
    if ctx.terminal.interrupted() {
        return Err(interrupted(NOT_ADDED_NOR_STORED));
    }
    let stored_here = match new_key {
        None => false,
        // The key prompt has no time limit, so the Keychain is read again
        // right before the store: an entry another process stored
        // meanwhile is never overwritten, deleted or used.
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
        if no_key {
            return "it made no Keychain call (the Account has no key)".to_string();
        }
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
    // The write, with AD-15's confirmation record in its transaction. An
    // edit is applied only while the file holds the bytes it was built on.
    let references: Vec<String> = scan
        .uses
        .iter()
        .map(|(root, u)| format!("{}:{u}", root.display()))
        .collect();
    let key_state = if no_key {
        KeyState::None
    } else {
        KeyState::Stored
    };
    let note = AuditNote {
        action: "config.widened".into(),
        target: id.to_string(),
        args: BTreeMap::from([
            ("kind".to_string(), "account_added".to_string()),
            ("file".to_string(), USER_ACCOUNTS_FILE.to_string()),
            ("account".to_string(), id.to_string()),
            ("provider".to_string(), provider.to_string()),
            ("scheme".to_string(), endpoint.scheme.to_string()),
            ("host".to_string(), endpoint.host.clone()),
            ("port".to_string(), endpoint.port.to_string()),
            ("references".to_string(), references.join(", ")),
            ("key".to_string(), key_state.recorded().to_string()),
            ("confirmation".to_string(), "typed_id".to_string()),
        ]),
    };
    let writer = &locked.writer;
    let written = match (&now.file.sha256, now.file.exists) {
        (Some(built_on), true) => writer.apply_with_audit(
            owner(),
            am_config::add_account_edit(
                &now.file,
                id,
                provider,
                name,
                endpoint.written(),
                key_state,
            ),
            built_on,
            vec![note],
        ),
        _ => am_config::new_accounts_file(id, provider, name, endpoint.written(), key_state)
            .and_then(|bytes| {
                writer.create(
                    owner(),
                    ConfigCreate {
                        file: USER_ACCOUNTS_FILE.into(),
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
            return match written_pending(&places, locked.history.as_ref()) {
                // A failure after the file was written (the folder sync,
                // the History commit): the next lock records the entry,
                // and the key stays.
                Some(true) => refuse(format!(
                    "the Account '{id}' was written to {USER_ACCOUNTS_FILE}, but the write did not finish: {why}. Its History entry is recorded the next time an Account command takes this Mac's Accounts lock; {}",
                    if no_key {
                        "it has no key"
                    } else {
                        "its key is in the Keychain"
                    }
                )),
                Some(false) => {
                    let key = key_undone(ctx.secrets.as_ref());
                    refuse(format!(
                        "the Account '{id}' was not added: {why}. This command did not change {USER_ACCOUNTS_FILE}, and {key}"
                    ))
                }
                None => refuse(format!(
                    "the write of the Account '{id}' stopped ({why}), and this Mac's user store could not be read to tell whether {USER_ACCOUNTS_FILE} was written; `agentic accounts list` shows what it holds. {}",
                    if no_key {
                        "This command made no Keychain call"
                    } else {
                        "A key this command stored is still in the Keychain"
                    }
                )),
            };
        }
    };
    let key = if no_key {
        "it is keyless: no key was read or stored, and its calls carry no credential"
    } else if stored_here {
        "its key is in the Keychain"
    } else {
        "it reuses the key the Keychain already held"
    };
    let _ = writeln!(
        out,
        "agentic: added the Account '{id}' ({provider}, {}) to this Mac's Accounts, {} (History entry {}); {key}",
        endpoint.base_url,
        places.user.accounts_file().display(),
        entry.id
    );
    Ok(())
}

fn remove(
    mut ctx: AccountsContext,
    id: &str,
    force: bool,
    out: &mut dyn Write,
) -> Result<(), Stop> {
    require_terminal(&ctx, "remove", NOT_REMOVED)?;
    let places = ctx.places.clone();
    let standing = stand(&places, NOT_REMOVED)?;
    if reserved(id) {
        return refuse(format!(
            "'{id}' is a reserved local Account: it is always available and cannot be removed. {NOT_REMOVED}"
        ));
    }
    if !standing.file.has(id) {
        return refuse(format!(
            "there is no Account '{id}' on this Mac (`agentic accounts list` shows the Accounts). {NOT_REMOVED}"
        ));
    }
    let scan = crate::user::scan(&places, id);
    no_owner_child(&scan, "remove", NOT_REMOVED)?;
    let referenced = |scan: &Scan| -> Result<(), Stop> {
        if scan.uses.is_empty() {
            return Ok(());
        }
        refuse(format!(
            "the Account '{id}' is still named by {}. {NOT_REMOVED}; point each at another Account first, then remove it",
            scan.uses_words()
        ))
    };
    referenced(&scan)?;
    if !scan.unreachable.is_empty() && !force {
        return refuse(format!(
            "these projects this Mac knows could not be reached, and each might name the Account '{id}': {}. {NOT_REMOVED}; make them reachable and run the command again, or run `agentic accounts remove {id} --force` to remove it without checking them",
            scan.unreachable_words()
        ));
    }
    if ctx.terminal.interrupted() {
        return Err(interrupted(NOT_REMOVED));
    }
    let locked = lock(&places, NOT_REMOVED, out)?;
    let (now, scan_now) = nothing_changed_since(
        &places,
        &locked,
        id,
        true,
        Shown {
            standing: &standing,
            scan: &scan,
        },
        "remove",
        NOT_REMOVED,
    )?;
    referenced(&scan_now)?;
    if !scan_now.unreachable.is_empty() && !force {
        return refuse(format!(
            "these projects this Mac knows could not be reached: {}. {NOT_REMOVED}; run the command again",
            scan_now.unreachable_words()
        ));
    }
    let (Some(built_on), Some(edit)) = (
        now.file.sha256.as_deref(),
        am_config::remove_account_edit(&now.file, id),
    ) else {
        return refuse(format!(
            "there is no Account '{id}' on this Mac any more. {NOT_REMOVED}"
        ));
    };
    // Story 2.10 (review K5): what the words say of its key follows its
    // live record's key state (with none, its entry's).
    let record = key_use::live_record(&places, id).map(|r| r.map(|(d, _)| d.key_state()));
    let entry_keyless = now
        .file
        .accounts
        .iter()
        .find(|a| a.id == id)
        .is_some_and(|a| is_keyless(a, None));
    let note = AuditNote {
        action: "config.narrowed".into(),
        target: id.to_string(),
        args: BTreeMap::from([
            ("kind".to_string(), "account_removed".to_string()),
            ("file".to_string(), USER_ACCOUNTS_FILE.to_string()),
            ("account".to_string(), id.to_string()),
        ]),
    };
    // Applied only while the file holds the bytes the edit was built on.
    let entry = locked
        .writer
        .apply_with_audit(owner(), edit, built_on, vec![note])
        .map_err(|e| {
            let why = write_words(&e);
            let words = match written_pending(&places, locked.history.as_ref()) {
                // A failure after the file was replaced: the Account is
                // gone from the file, its History entry pending.
                Some(true) => format!(
                    "the Account '{id}' was removed from {USER_ACCOUNTS_FILE}, but the write did not finish: {why}. Its History entry is recorded the next time an Account command takes this Mac's Accounts lock"
                ),
                Some(false) => format!(
                    "the Account '{id}' was not removed: {why}. This command did not change {USER_ACCOUNTS_FILE}"
                ),
                None => format!(
                    "the removal of the Account '{id}' stopped ({why}), and this Mac's user store could not be read to tell whether {USER_ACCOUNTS_FILE} was written; `agentic accounts list` shows what it holds"
                ),
            };
            Stop(EXIT_ERRORS, words)
        })?;
    let skipped = if scan_now.unreachable.is_empty() {
        String::new()
    } else {
        format!(
            " With --force it did not check these projects, which could not be reached: {}.",
            scan_now.unreachable_words()
        )
    };
    let key = removed_key_words(id, record, entry_keyless);
    let _ = writeln!(
        out,
        "agentic: removed the Account '{id}' from this Mac's Accounts. The change is recorded in History (entry {}) and can be undone from the app's History when the UI ships. {key}{skipped}",
        entry.id
    );
    Ok(())
}

// ------------------------------------------------------------ list, test

/// An Account's `secret_ref` when it names another Keychain entry than the
/// Account's own id (`am_config::key_ref` gives none for it): never
/// followed, or a config file could have any `agentic-manager` Keychain
/// entry shown or sent to a provider.
fn other_entry(account: &am_config::Account) -> Option<&str> {
    match am_config::key_ref(account) {
        Some(_) => None,
        None => account.secret_ref.as_deref(),
    }
}

/// Story 2.10: a model Account at `http://127.0.0.1:<port>` whose entry has
/// no `secret_ref` and whose live record (`record`) is not one with a key:
/// no Keychain entry is read for it.
fn is_keyless(account: &am_config::Account, record: Option<KeyState>) -> bool {
    is_model_provider(&account.provider)
        && am_config::key_state(account) == KeyState::None
        && record != Some(KeyState::Stored)
        && endpoint_of(&account.provider, account.base_url.as_deref(), "base_url")
            .is_ok_and(|e| e.loopback)
}

/// What `remove` says of the removed Account's key (story 2.10, review K5):
/// its live record's key state decides (`record`); with no live record, its
/// entry's (`entry_keyless`, [`is_keyless`]). A record that could not be
/// read never reads as keyless. The command made no Keychain call either
/// way.
fn removed_key_words(
    id: &str,
    record: Result<Option<KeyState>, String>,
    entry_keyless: bool,
) -> String {
    let keyless = match record {
        Ok(Some(state)) => state == KeyState::None,
        Ok(None) => entry_keyless,
        Err(_) => false,
    };
    if keyless {
        "It had no key, so no Keychain entry was read or changed.".to_string()
    } else {
        format!(
            "Its key stays in the Keychain; delete the entry '{id}' of service agentic-manager in Keychain Access if nothing needs it."
        )
    }
}

/// How `list` shows a keyless Account's key.
const KEYLESS: &str = "keyless (no key; its calls carry no credential)";

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

fn list(ctx: &AccountsContext, out: &mut dyn Write) -> Result<(), Stop> {
    let places = &ctx.places;
    let standing = Standing::read(places).map_err(|why| Stop(EXIT_ERRORS, why))?;
    let file = &standing.file;
    if let Some(problem) = &file.problem {
        return refuse(format!(
            "{USER_ACCOUNTS_FILE} ({}) cannot be read as Accounts because {problem} (`agentic validate` names the line)",
            places.user.accounts_file().display()
        ));
    }
    if file.ids.is_empty() {
        let _ = writeln!(
            out,
            "agentic: no Accounts on this Mac yet; add one with `agentic accounts add <id>` and {PROVIDERS} (the local Accounts local-lmstudio and local-ollama are always available)"
        );
        return Ok(());
    }
    // An outside change: each Account's confirmed values are shown beside
    // the file's, so the owner can put the file back by hand.
    let outside = standing.outside_change();
    if outside {
        let _ = writeln!(
            out,
            "agentic: {USER_ACCOUNTS_FILE} ({}) changed on disk outside the app since the app last wrote it; no key is sent until it is put back. Each Account's last confirmed values are shown after `confirmed:`",
            places.user.accounts_file().display()
        );
    }
    for (n, id) in file.ids.iter().enumerate() {
        // An invalid entry is named by its line, never by what it holds.
        let Some(account) = id
            .as_ref()
            .and_then(|id| file.accounts.iter().find(|a| &a.id == id))
        else {
            let line = file
                .lines
                .get(n)
                .map_or_else(|| format!("number {}", n + 1), |l| format!("at line {l}"));
            let _ = writeln!(
                out,
                "the entry {line}  (not a valid Account entry; `agentic validate` names the line)"
            );
            continue;
        };
        // Story 2.10: the live record, read once. No Keychain read for a
        // model Account's entry with no `secret_ref`, or one confirmed with
        // no key.
        let model = is_model_provider(&account.provider);
        let live = if model {
            key_use::live_record(places, &account.id)
        } else {
            Ok(None)
        };
        let record = live
            .as_ref()
            .ok()
            .and_then(Option::as_ref)
            .map(|(d, _)| d.key_state());
        let key = if other_entry(account).is_some() {
            // No key is read for it.
            "secret_ref names another entry; not used".to_string()
        } else if is_keyless(account, record) {
            KEYLESS.to_string()
        } else if model && am_config::key_state(account) == KeyState::None {
            "no secret_ref, so no key is read".to_string()
        } else if record == Some(KeyState::None) {
            "no key is read (confirmed with no key)".to_string()
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
        let confirmed = if outside {
            match key_use::confirmed_destination(places, &account.id) {
                Ok(Some(d)) if d.key_state() == KeyState::None => {
                    format!("  confirmed: {} with no key", d.words())
                }
                Ok(Some(d)) => format!("  confirmed: {}", d.words()),
                Ok(None) => "  confirmed: nothing".to_string(),
                Err(why) => format!("  confirmed: unknown ({why})"),
            }
        } else if file.entry_errors(&account.id).is_empty() && is_model_provider(&account.provider)
        {
            // Story 2.14: an Account with no live confirmation record, or one
            // for another provider or endpoint than the file's, is not called.
            let endpoint =
                endpoint_of(&account.provider, account.base_url.as_deref(), "base_url").ok();
            match live {
                Ok(Some((d, _)))
                    if endpoint
                        .as_ref()
                        .is_some_and(|e| d.names(&account.provider, e)) =>
                {
                    // Story 2.10: the entry's key state is the record's.
                    match (d.key_state(), am_config::key_state(account)) {
                        (KeyState::Stored, KeyState::None) => format!(
                            "  unconfirmed: {}",
                            key_use::keyed_without_line_words(&account.id)
                        ),
                        (KeyState::None, KeyState::Stored) => format!(
                            "  unconfirmed: {}",
                            key_use::keyless_with_line_words(&account.id)
                        ),
                        _ => String::new(),
                    }
                }
                Ok(_) => format!(
                    "  unconfirmed: no key is used until `{}` confirms it",
                    key_use::change_command(places, &account.id)
                ),
                Err(why) => format!("  confirmed: unknown ({why})"),
            }
        } else {
            String::new()
        };
        let _ = writeln!(
            out,
            "{}  {}  {host}  {key}{confirmed}",
            account.id, account.provider
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

/// `test`'s refusal of an Account the key-use check refused.
fn refused_words(
    id: &str,
    refused: &Refused,
    provider: Option<&str>,
    own: bool,
    confirm: &str,
) -> String {
    match refused {
        Refused::Store(why) => format!("{why}; no key was read and nothing was sent"),
        Refused::NoAccount => format!(
            "there is no Account '{id}' on this Mac (`agentic accounts list` shows the Accounts; add it with `agentic accounts add {id}`); nothing was sent"
        ),
        Refused::Entry(errors) => format!(
            "the Account '{id}' is not called: its entry in {USER_ACCOUNTS_FILE} has an error ({}); `agentic validate` names the line and the fix. No key was read and nothing was sent",
            errors.join("; ")
        ),
        Refused::NotModel(provider) => format!(
            "the Account '{id}' is a {provider} Account, not a model Account (anthropic, openai or openai-compatible), the only kinds `agentic accounts test` calls; nothing was sent"
        ),
        Refused::Endpoint(why) => format!(
            "the Account '{id}' in {USER_ACCOUNTS_FILE} cannot be called: {why}; no key was read and nothing was sent"
        ),
        Refused::SecretRef(_) => format!(
            "the Account '{id}' has a `secret_ref` that names another Keychain entry; a key is only ever read from the entry named by the Account's own id, so no key was read and nothing was sent. Set `secret_ref: {id}` in {USER_ACCOUNTS_FILE}"
        ),
        Refused::KeyedWithoutLine => format!(
            "the Account '{id}' is not called: {}",
            key_use::keyed_without_line_words(id)
        ),
        Refused::KeylessWithLine => format!(
            "the Account '{id}' is not called: {}",
            key_use::keyless_with_line_words(id)
        ),
        Refused::Interrupted => format!(
            "the Account '{id}' is not called: {}. No key was read and nothing was sent",
            key_use::interrupted_words()
        ),
        Refused::Unconfirmed => format!(
            "the Account '{id}' is not called: {}. No key was read and nothing was sent",
            key_use::unconfirmed_words(id)
        ),
        Refused::NotConfirmed(why) => {
            let base_url = if own { " --base-url <url>" } else { "" };
            format!(
                "the Account '{id}' is not called: it was not confirmed by `agentic accounts add` on this Mac, or {USER_ACCOUNTS_FILE} changed since ({why}). No key was read or sent. If {USER_ACCOUNTS_FILE} was changed by hand, put it back as the app last wrote it; otherwise confirm it in a terminal with `{confirm}` (a new key is asked for when it has no confirmation), or remove the Account and add it again (`agentic accounts remove {id}`, then `agentic accounts add {id} --provider {}{base_url}`)",
                provider.unwrap_or("<provider>")
            )
        }
    }
}

/// The key-use check of `id` for `test`, in `test`'s words.
fn checked(places: &Places, id: &str) -> Result<key_use::Confirmed, Stop> {
    key_use::check(places, id).map_err(|refused| {
        // The file's provider and endpoint, for the advice.
        let file = am_config::accounts_file(places.user.dir());
        let account = file.accounts.iter().find(|a| a.id == id);
        let own = account.is_some_and(|a| a.base_url.is_some());
        Stop(
            EXIT_ERRORS,
            refused_words(
                id,
                &refused,
                account.map(|a| a.provider.as_str()),
                own,
                &key_use::change_command(places, id),
            ),
        )
    })
}

async fn test(
    ctx: &AccountsContext,
    id: &str,
    model: Option<String>,
    out: &mut dyn Write,
) -> Result<(), Stop> {
    let places = &ctx.places;
    if reserved(id) {
        return refuse(format!(
            "'{id}' is a local Account with no key; `agentic accounts test` calls the cloud Accounts on this Mac"
        ));
    }
    // AD-15 Key use: before any Keychain read or request.
    let mut confirmed = checked(places, id)?;
    // The app's own ports (every running `agentic serve`, and the one each
    // runtime folder records): an Account never reaches the app itself.
    let directory = crate::egress::ProjectDirectory::for_accounts(places);
    if confirmed.endpoint.loopback && directory.own_ports().contains(&confirmed.endpoint.port) {
        return refuse(format!(
            "the Account '{id}' is not called: {}, and an Account never reaches the app itself. No key was read and nothing was sent",
            crate::egress::own_server_words(confirmed.endpoint.port)
        ));
    }
    // No default model id is assumed for a provider whose model list this
    // app does not hold.
    let model = match (model, confirmed.account.provider.as_str()) {
        (Some(model), _) => model,
        (None, ANTHROPIC) => crate::DEFAULT_ANTHROPIC_MODEL.to_string(),
        (None, provider) => {
            return refuse(format!(
                "`agentic accounts test {id}` needs `--model <model>`: no default model is assumed for an {provider} Account; name one its endpoint ({}) serves. No key was read and nothing was sent",
                confirmed.endpoint.base_url
            ));
        }
    };
    // The key read, then the re-check before anything is sent (AD-15, Key
    // use): a record that moved meanwhile drops the key unsent and the
    // check runs again.
    for attempt in 0..2 {
        // Story 2.10: a keyless Account's call reads no key.
        if confirmed.key == KeyState::None {
            break;
        }
        match ctx.secrets.get(id) {
            Ok(Some(key)) => drop(key),
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
        if key_use::stamp(places, id) == Some(confirmed.stamp) {
            break;
        }
        // Moved: the key read is dropped unsent; check again.
        confirmed = checked(places, id)?;
        if attempt == 1 {
            return refuse(format!(
                "the Account '{id}' kept changing on this Mac while its key was read, so nothing was sent; run the command again"
            ));
        }
    }
    let account = confirmed.account.clone();
    let endpoint = confirmed.endpoint.clone();
    // Only the Account's pinned endpoint (AD-12, AD-48): the client asks a
    // directory that answers this Account only while it is confirmed. `test`
    // opens no store, so a block is collected and printed, not audited.
    let blocks = Arc::new(Blocks::default());
    let egress = (ctx.egress)(Arc::new(directory), blocks.clone());
    // The adapter by provider. OpenAI and compatible Accounts authenticate
    // with `Bearer` from the Keychain entry named by the Account's id.
    // A keyless Account's adapter has no credential reference (story 2.10).
    let keyed = confirmed.key == KeyState::Stored;
    let provider: Box<dyn LlmProvider> = if account.provider == ANTHROPIC {
        Box::new(AnthropicProvider::new(egress, id))
    } else if keyed {
        Box::new(
            OpenAiCompatProvider::new(egress, id, endpoint.call_base()).with_auth(AuthRef {
                scheme: AuthScheme::Bearer,
                account_id: id.to_string(),
            }),
        )
    } else {
        Box::new(OpenAiCompatProvider::new(egress, id, endpoint.call_base()))
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
        let _ = out.write_all(printable(&text).as_bytes());
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
            let keyless = if keyed {
                ""
            } else {
                "; it is keyless, so no credential was sent"
            };
            let _ = writeln!(
                out,
                "agentic: the Account '{id}' answered from {} (model {model}, {tokens}){keyless}",
                endpoint.base_url
            );
            Ok(())
        }
        // Owner Q1 (2026-11-25): a keyless Account's server that refuses
        // the connection, times out or sends nothing is named as such,
        // never as a key or a cloud problem.
        Err(ProviderError::Egress(e)) if !keyed && e.starts_with(&network_error()) => {
            let cause = printable(e.strip_prefix(network_error().as_str()).unwrap_or(&e));
            refuse(format!(
                "the local model server of the Account '{id}' at {}:{} is not answering: it refused the connection, timed out or sent nothing ({cause}). Start the server on this Mac, or check its port, then run `agentic accounts test {id}` again",
                endpoint.host, endpoint.port
            ))
        }
        Err(e) => refuse(format!(
            "the test call of the Account '{id}' (model {model}) failed: {}",
            printable(&format!("{e}{}", blocks.words()))
        )),
    }
}

/// Text from the server (a streamed reply, an error body or event) as
/// `test` prints it (story 2.10, review K2): every hidden character
/// (`am_domain::text`, as `agentic gate` shows model text) and a tab
/// written as `\u{XXXX}`, so no control character but `\n` reaches the
/// terminal to move the cursor, erase a line or write the clipboard.
fn printable(text: &str) -> String {
    am_domain::text::escape_hidden(text)
        .0
        .replace('\t', "\\u{0009}")
}

/// How the egress client's network failures begin (a refused connection, a
/// timeout, a reply cut short), as a provider adapter passes them on.
fn network_error() -> String {
    EgressError::Network(String::new()).to_string()
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

    /// Story 2.10 (review K5): `remove`'s words follow the live record's key
    /// state, the entry's only with no record, and a record that could not
    /// be read never reads as "It had no key", whatever the entry holds.
    #[test]
    fn removes_words_never_say_no_key_when_the_record_cannot_be_read() {
        let no_key = "It had no key";
        for entry_keyless in [true, false] {
            let words = removed_key_words("llama", Err("store busy".into()), entry_keyless);
            assert!(!words.contains(no_key), "{entry_keyless}: {words}");
            assert!(words.contains("stays in the Keychain"), "{words}");
        }
        for (record, entry_keyless, keyless) in [
            (Ok(Some(KeyState::None)), false, true),
            (Ok(Some(KeyState::Stored)), true, false),
            (Ok(None), true, true),
            (Ok(None), false, false),
        ] {
            let words = removed_key_words("llama", record.clone(), entry_keyless);
            assert_eq!(
                words.contains(no_key),
                keyless,
                "{record:?} {entry_keyless}: {words}"
            );
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
