//! Story 2.10 (AD-15 *Keyless loopback Account*, *Terminal*, *Open*,
//! *Record*, *Key use*, *Change*; AD-12; AD-17): keyless Accounts on this
//! Mac. One test per row of the story's I/O & Edge-Case Matrix that runs
//! through the `agentic accounts` commands (rows 1-11 and 13-23, with row
//! 27's leak scan in each keyless test), and the owner's additions of
//! 2026-11-25:
//!
//! - Q1: when a keyless Account's server refuses the connection, times out
//!   or answers nothing, `test` says the local model server is not
//!   answering, naming the Account and the address, and never reads as a
//!   key problem or a cloud error;
//! - Q2: `--no-key` takes `http://127.0.0.1:<port>` only;
//! - Q5: a keyless `change` runs only on a terminal, with the typed id and
//!   `yes`;
//! - Q6: an Undo or a Restore that flips an Account's key state (either
//!   way) is refused by the writer, writing nothing;
//! - no Keychain call: a store that records every call sees no `get`,
//!   `set` or `delete` on any keyless path (add, change, call, `test`,
//!   `list` of that Account, `remove`).
//!
//! The commands run in process through `am_app::cli::accounts` on 2.14's
//! harness: am-testkit's FakeTerminal, FakeSecretStore behind a store that
//! records every call, FakeProjectIdentity, and a temporary HOME per test.
//! `accounts test` runs through a real `am_net::NetEgress` on am-e2e's
//! scripted resolver and transport, so each hop, its address and its
//! headers are seen. Canary values only, never a key; no network.
//!
//! Decisions these tests make (tester, 2026-11-25; the plan leaves words
//! and test structure to the build):
//!
//! - `--no-key` is a flag with no value, on `add` and `change` only. Every
//!   refusal of it is exit 1 in words naming `--no-key` (or 2.3's endpoint
//!   words for another spelling of this Mac), before anything is shown.
//! - The keyless add's sentence names the Account, `openai-compatible` and
//!   the base URL, says the calls "stay on this Mac, on port <port>" and
//!   carry "no credential", and says no key is read or stored; it never
//!   names a key the Keychain already holds under the id. Then only the id
//!   is read (no hidden prompt). The output line says "keyless".
//! - `list` shows a keyless Account as `<id>  openai-compatible
//!   127.0.0.1:<port>  keyless …` with "no credential" on the line.
//! - `test` of a keyless Account names its base URL and says "no
//!   credential" was sent. When its server is not answering the words say
//!   "local model server" and "not answering", name the Account and
//!   `127.0.0.1:<port>`, and hold none of the words key, keys, Keychain,
//!   cloud, credential or credentials.
//! - `remove` of a keyless Account says "no key"; never "stays in the
//!   Keychain" or "Keychain Access".
//! - A keyless address change's sentence says "no credential", never "will
//!   now be sent"; `account_changed` records `key: none`.
//! - `change <id> --no-key` (a first confirmation, or a provider change, to
//!   `http://127.0.0.1:<port>`): no key prompt; `account_removed` with
//!   `reason: provider_change` and no `key_for…` field, then
//!   `account_added` with `key: none`, `replaces` and `confirmation:
//!   typed_id_yes_new_key` (AD-15 *Record*, literally), both committed with
//!   the History entry of the file write that removes `secret_ref`.
//! - Row 14 (a keyed record, an entry without `secret_ref`): `test`'s words
//!   hold `secret_ref: <id>`. Row 16 (a keyless record, an entry naming a
//!   `secret_ref`): they hold `agentic accounts change <id>`. In both the
//!   directory answers no stamp and its refusal names `secret_ref`, and
//!   `list` says `unconfirmed`.
//! - The writer's refusal of a key-state flip names `agentic accounts
//!   change`, as 2.14's refusal of a move does.
//!
//! Written before the change.
#![forbid(unsafe_code)]
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use am_app::cli::{AccountsContext, accounts};
use am_app::egress::ProjectDirectory;
use am_app::runtime::{OpenRequest, Places, Prompt, open_project};
use am_config::{ConfigEdit, EditOp, KeyState};
use am_domain::net::{EgressDirectory, Endpoint};
use am_domain::{
    Actor, AuditNote, AuditSink, AuthScheme, BoxFuture, EgressClient, EgressError, HistoryEntry,
    HistoryKind, HttpResponse, SecretError, SecretStore, SecretString,
};
use am_store::user::{UserReader, UserStore};
use am_testkit::{CollectAudit, FakeProjectIdentity, FakeSecretStore, FakeTerminal};

/// am-e2e's RULE-9 fakes: a scripted resolver and a scripted per-hop
/// transport for a real `am_net::NetEgress` (no network).
#[path = "../../am-e2e/tests/trust/support/egress.rs"]
mod net_rig;

/// The keyed Account beside the keyless one (row 27).
const ROUTER_CANARY: &str = "canary-keyless-router-5c1e-RTRT";
/// What a removed keyed Account of the same id left in the Keychain (Q3:
/// never read, used, deleted or named for a keyless Account).
const LEFTOVER_CANARY: &str = "canary-keyless-leftover-9a7d-LEFT";
/// A key typed for a keyed loopback Account (rows 9, 10).
const LOCAL_CANARY: &str = "canary-keyless-local-2e8b-LOCL";
/// A new key typed at a keyed confirmation (row 23).
const NEW_CANARY: &str = "canary-keyless-new-3b2f-NEWK";
const USER_FILE: &str = "accounts.yaml";
/// `LOOP` in the matrix.
const LOOP: &str = "http://127.0.0.1:8080/v1";
const LOOP_9090: &str = "http://127.0.0.1:9090/v1";
const LOOP_COMPLETIONS: &str = "http://127.0.0.1:8080/v1/chat/completions";
const LOOP_9090_COMPLETIONS: &str = "http://127.0.0.1:9090/v1/chat/completions";
const REMOTE_URL: &str = "https://openrouter.example/api/v1";
const NEW_REMOTE_URL: &str = "https://router2.example/api/v1";
const ANTHROPIC_MESSAGES: &str = "https://api.anthropic.com/v1/messages";
const YES: &str = "yes";

const COMPANY: &str = "schema_version: 2\nname: Example\ntimezone: UTC\ncurrency: EUR\nteams:\n  - id: core\n    name: Core team\n";

const STDIO_TOOLS: &str = "schema_version: 2\nmcp_servers:\n  - id: notes\n    transport: stdio\n    command: example-notes-server\n    version: 1.0.0\n    network: none\n    tools:\n      - name: list_notes\n        class: read\n";

/// A `tools.yaml` enabling one `transport: http` MCP server `id` at `url`.
fn http_tools(id: &str, url: &str) -> String {
    format!(
        "schema_version: 2\nmcp_servers:\n  - id: {id}\n    transport: http\n    url: {url}\n    version: 1.0.0\n    network: none\n    tools:\n      - name: list_notes\n        class: read\n"
    )
}

fn owner() -> Actor {
    Actor::Human {
        person: "owner".into(),
        hat: "Owner".into(),
    }
}

struct NoPrompt;

impl Prompt for NoPrompt {
    fn interactive(&self) -> bool {
        false
    }
    fn ask(&mut self, _question: &str) -> bool {
        false
    }
}

fn loopback(port: u16) -> Endpoint {
    Endpoint {
        scheme: "http".into(),
        host: "127.0.0.1".into(),
        port,
    }
}

// ------------------------------------------------------------------- env

/// A temporary HOME with its places, and a folder for projects.
struct Env {
    dir: tempfile::TempDir,
    places: Places,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let places = Places::under(&home.join("Library/Application Support"), &home);
    Env { dir, places }
}

impl Env {
    /// A project folder `name` holding `files`, opened once on this Mac (so
    /// it is in the runtime base's root records) and closed again.
    fn project(&self, name: &str, files: &[(&str, String)]) -> PathBuf {
        let root = self.dir.path().join("projects").join(name);
        std::fs::create_dir_all(&root).unwrap();
        for (rel, text) in files {
            let path = root.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let root = root.canonicalize().unwrap();
        let req = OpenRequest {
            root: root.clone(),
            confirm_move: false,
            take_over: false,
        };
        drop(
            open_project(
                &self.places,
                &FakeProjectIdentity::new("Test Mac"),
                &req,
                &mut NoPrompt,
            )
            .unwrap(),
        );
        root
    }

    fn accounts_path(&self) -> PathBuf {
        self.places.user.accounts_file()
    }

    fn user_text(&self) -> Option<String> {
        std::fs::read_to_string(self.accounts_path()).ok()
    }

    /// The Account `id` as the user file holds it now.
    fn entry(&self, id: &str) -> am_config::Account {
        am_config::accounts_file(self.places.user.dir())
            .accounts
            .into_iter()
            .find(|a| a.id == id)
            .unwrap_or_else(|| panic!("{id} is in the user file: {:?}", self.user_text()))
    }

    fn history(&self) -> Vec<HistoryEntry> {
        UserReader::open(&self.places.user)
            .unwrap()
            .config_history(None)
            .unwrap()
    }

    fn audit(&self) -> String {
        let reader = UserReader::open(&self.places.user).unwrap();
        let mut out = Vec::new();
        reader.export_audit_jsonl(&mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    fn user_writer(&self) -> am_config::ConfigWriter {
        let guard = UserStore::locked(&self.places.user, Duration::from_secs(10)).unwrap();
        am_config::ConfigWriter::for_user_accounts(self.places.user.dir(), Arc::new(guard)).unwrap()
    }

    /// Everything a refused command must leave as it was: the user file,
    /// History and the user store's audit.
    fn state(&self) -> (Option<String>, usize, String) {
        (self.user_text(), self.history().len(), self.audit())
    }

    /// An app write of the user file (a History entry with no confirmation
    /// record, so the file's hash check passes) that gives `id` the
    /// `secret_ref: <id>` line (`set`) or removes it.
    fn app_write_secret_ref(&self, id: &str, set: bool) -> HistoryEntry {
        let file = am_config::accounts_file(self.places.user.dir());
        let index = file
            .ids
            .iter()
            .position(|i| i.as_deref() == Some(id))
            .unwrap();
        let path = vec!["accounts".into(), index.to_string(), "secret_ref".into()];
        let op = if set {
            EditOp::Set {
                path,
                value: id.into(),
            }
        } else {
            EditOp::Remove { path }
        };
        self.user_writer()
            .apply(
                owner(),
                ConfigEdit {
                    file: USER_FILE.into(),
                    ops: vec![op],
                    body: None,
                },
            )
            .unwrap()
    }
}

// ------------------------------------------------------------------- kit

/// What one command printed.
struct Ran {
    code: i32,
    out: String,
    err: String,
    shown: String,
}

impl Ran {
    fn all(&self) -> String {
        format!("{}\n{}\n{}", self.out, self.err, self.shown)
    }
}

/// A Keychain that records every call (`get:<id>`, `set:<id>`,
/// `delete:<id>`, in order).
struct Recording {
    inner: Arc<FakeSecretStore>,
    calls: Mutex<Vec<String>>,
}

impl Recording {
    fn new(inner: &Arc<FakeSecretStore>) -> Arc<Recording> {
        Arc::new(Recording {
            inner: inner.clone(),
            calls: Mutex::new(Vec::new()),
        })
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl SecretStore for Recording {
    fn get(&self, account: &str) -> Result<Option<SecretString>, SecretError> {
        self.calls.lock().unwrap().push(format!("get:{account}"));
        self.inner.get(account)
    }
    fn set(&self, account: &str, secret: SecretString) -> Result<(), SecretError> {
        self.calls.lock().unwrap().push(format!("set:{account}"));
        self.inner.set(account, secret)
    }
    fn delete(&self, account: &str) -> Result<(), SecretError> {
        self.calls.lock().unwrap().push(format!("delete:{account}"));
        self.inner.delete(account)
    }
}

/// The fakes one command runs with, on this Mac's Keychain.
struct Kit {
    term: FakeTerminal,
    keychain: Arc<FakeSecretStore>,
    store: Arc<Recording>,
}

impl Kit {
    fn new(term: FakeTerminal, keychain: &Arc<FakeSecretStore>) -> Kit {
        Kit {
            term,
            keychain: keychain.clone(),
            store: Recording::new(keychain),
        }
    }

    /// An interactive terminal with `typed` ready at its prompts.
    fn typing(keychain: &Arc<FakeSecretStore>, typed: &[&str]) -> Kit {
        let kit = Kit::new(FakeTerminal::new(), keychain);
        for line in typed {
            kit.term.line(line);
        }
        kit
    }

    /// A pipe on stdin with `typed` ready (never read: refused first).
    fn piped(keychain: &Arc<FakeSecretStore>, typed: &[&str]) -> Kit {
        let kit = Kit::new(FakeTerminal::piped(), keychain);
        for line in typed {
            kit.term.line(line);
        }
        kit
    }

    fn calls(&self) -> Vec<String> {
        self.store.calls()
    }

    fn key(&self, account: &str) -> Option<String> {
        self.keychain
            .get(account)
            .unwrap()
            .map(|s| s.expose().to_string())
    }

    async fn run(&self, env: &Env, args: &[&str]) -> Ran {
        let factory = Arc::new(
            move |_directory: Arc<dyn EgressDirectory>, _audit: Arc<dyn AuditSink>| {
                am_testkit::FakeEgress::new() as Arc<dyn EgressClient>
            },
        );
        let ctx = AccountsContext {
            places: env.places.clone(),
            identity: Arc::new(FakeProjectIdentity::new("Test Mac")),
            terminal: Box::new(self.term.clone()),
            secrets: self.store.clone(),
            egress: factory,
        };
        let argv: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = accounts(&argv, ctx, &mut out, &mut err).await;
        Ran {
            code,
            out: String::from_utf8(out).unwrap(),
            err: String::from_utf8(err).unwrap(),
            shown: self.term.shown(),
        }
    }
}

/// `agentic accounts add <id> --provider openai-compatible --base-url
/// <url> --no-key` as the owner runs it: only the id is typed. Asserts it
/// was added with no Keychain call.
async fn add_keyless(env: &Env, keychain: &Arc<FakeSecretStore>, id: &str, url: &str) -> Ran {
    let kit = Kit::typing(keychain, &[id]);
    let ran = kit
        .run(
            env,
            &[
                "add",
                id,
                "--provider",
                "openai-compatible",
                "--base-url",
                url,
                "--no-key",
            ],
        )
        .await;
    assert_eq!(ran.code, 0, "setup: keyless add of {id}: {}", ran.all());
    assert!(
        kit.calls().is_empty(),
        "setup: keyless add of {id}: no Keychain call: {:?}",
        kit.calls()
    );
    ran
}

/// A keyed `agentic accounts add` (typed id, then the key).
async fn add_keyed(
    env: &Env,
    keychain: &Arc<FakeSecretStore>,
    id: &str,
    provider: &str,
    base_url: Option<&str>,
    key: &str,
) {
    let mut args = vec!["add", id, "--provider", provider];
    if let Some(url) = base_url {
        args.extend(["--base-url", url]);
    }
    let kit = Kit::typing(keychain, &[id, key]);
    let ran = kit.run(env, &args).await;
    assert_eq!(ran.code, 0, "setup: keyed add of {id}: {}", ran.all());
}

/// `router`: a keyed openai-compatible Account at openrouter.example.
async fn router(env: &Env, keychain: &Arc<FakeSecretStore>) {
    add_keyed(
        env,
        keychain,
        "router",
        "openai-compatible",
        Some(REMOTE_URL),
        ROUTER_CANARY,
    )
    .await;
}

/// `id` written to the user file by the app as `accounts migrate` leaves an
/// Account it could not confirm (story 2.13): its History entry and an
/// `account_listed` record, no confirmation record. `key` is the entry's
/// key state: `Stored` writes `secret_ref: <id>` (as migrate writes it),
/// `None` writes none.
fn listed_unconfirmed(
    env: &Env,
    id: &str,
    provider: &str,
    base_url: Option<&str>,
    (scheme, host, port): (&str, &str, &str),
    key: KeyState,
) {
    let note = AuditNote {
        action: "config.widened".into(),
        target: id.into(),
        args: [
            ("kind", "account_listed"),
            ("file", USER_FILE),
            ("account", id),
            ("provider", provider),
            ("scheme", scheme),
            ("host", host),
            ("port", port),
            ("references", ""),
            ("confirmation", "none"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect(),
    };
    let writer = env.user_writer();
    let now = am_config::accounts_file(env.places.user.dir());
    match now.sha256.as_deref() {
        Some(built_on) => writer
            .apply_with_audit(
                owner(),
                am_config::add_account_edit(&now, id, provider, None, base_url, key),
                built_on,
                vec![note],
            )
            .unwrap(),
        None => writer
            .create(
                owner(),
                am_config::ConfigCreate {
                    file: USER_FILE.into(),
                    bytes: am_config::new_accounts_file(id, provider, None, base_url, key).unwrap(),
                    audit: vec![note],
                },
            )
            .unwrap(),
    };
}

/// An openai-compatible Account `id` at `LOOP`, listed unconfirmed with
/// `secret_ref: <id>` (a 2.3-era Account `migrate` moved here).
fn listed_loopback(env: &Env, id: &str) {
    listed_unconfirmed(
        env,
        id,
        "openai-compatible",
        Some(LOOP),
        ("http", "127.0.0.1", "8080"),
        KeyState::Stored,
    );
}

/// An Anthropic Account `id`, listed unconfirmed, whose entry holds the
/// given key state.
fn listed_anthropic(env: &Env, id: &str, key: KeyState) {
    listed_unconfirmed(
        env,
        id,
        "anthropic",
        None,
        ("https", "api.anthropic.com", "443"),
        key,
    );
}

// ------------------------------------------------------- test on the wire

/// What `accounts test` did through a real egress client: its exit code
/// and words, the Keychain calls of the command and the client together,
/// and the names the resolver was asked for.
struct Sent {
    code: i32,
    out: String,
    err: String,
    calls: Vec<String>,
    lookups: Vec<String>,
}

impl Sent {
    fn all(&self) -> String {
        format!("{}\n{}", self.out, self.err)
    }
}

/// What one scripted `accounts test` saw on the wire: [`Sent`] and every
/// hop the transport was asked to make.
struct OnWire {
    sent: Sent,
    hops: Vec<net_rig::SeenHop>,
}

impl OnWire {
    fn urls(&self) -> Vec<String> {
        self.hops.iter().map(|h| h.url.clone()).collect()
    }

    /// True when some hop carried `value` in a header.
    fn carried(&self, value: &str) -> bool {
        self.hops
            .iter()
            .any(|h| h.headers.iter().any(|(_, v)| v.contains(value)))
    }
}

fn sse(fixture: &str) -> net_rig::Reply {
    net_rig::Reply {
        status: 200,
        headers: vec![("content-type".into(), "text/event-stream".into())],
        chunks: vec![am_testkit::recorded(fixture)],
        head_delay: Duration::ZERO,
        stalls: false,
    }
}

/// `agentic accounts test <id> <args>` from a pipe, with a real egress
/// client on `transport` and `limits`, reading keys from `keychain`
/// through one recording store shared by the command and the client.
async fn test_through(
    env: &Env,
    keychain: &Arc<FakeSecretStore>,
    id: &str,
    args: &[&str],
    transport: Arc<dyn am_net::Transport>,
    limits: am_net::Limits,
) -> Sent {
    let store = Recording::new(keychain);
    let resolver = net_rig::ScriptedResolver::standard();
    resolver.answer("openrouter.example", &[net_rig::OTHER_IP]);
    resolver.answer("router2.example", &[net_rig::FILES_IP]);
    let (client_store, client_resolver) = (store.clone(), resolver.clone());
    let factory: am_app::accounts::EgressFactory = Arc::new(
        move |directory: Arc<dyn EgressDirectory>, audit: Arc<dyn AuditSink>| {
            Arc::new(am_net::NetEgress::with_seams(
                directory,
                client_store.clone(),
                audit,
                am_net::Seams {
                    resolver: client_resolver.clone(),
                    transport: transport.clone(),
                    limits,
                },
            )) as Arc<dyn EgressClient>
        },
    );
    let ctx = AccountsContext {
        places: env.places.clone(),
        identity: Arc::new(FakeProjectIdentity::new("Test Mac")),
        terminal: Box::new(FakeTerminal::piped()),
        secrets: store.clone(),
        egress: factory,
    };
    let mut argv: Vec<String> = vec!["test".into(), id.into()];
    argv.extend(args.iter().map(|s| s.to_string()));
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = accounts(&argv, ctx, &mut out, &mut err).await;
    Sent {
        code,
        out: String::from_utf8_lossy(&out).to_string(),
        err: String::from_utf8_lossy(&err).to_string(),
        calls: store.calls(),
        lookups: resolver.all_lookups(),
    }
}

/// `accounts test` on the scripted transport, which answers every model
/// endpoint of these tests with a recorded stream.
async fn test_on_wire(
    env: &Env,
    keychain: &Arc<FakeSecretStore>,
    id: &str,
    args: &[&str],
) -> OnWire {
    let transport = net_rig::ScriptedTransport::new();
    transport.on(ANTHROPIC_MESSAGES, sse("anthropic/c1_count_stream.sse"));
    for url in [
        LOOP_COMPLETIONS.to_string(),
        LOOP_9090_COMPLETIONS.to_string(),
        format!("{REMOTE_URL}/chat/completions"),
        format!("{NEW_REMOTE_URL}/chat/completions"),
    ] {
        transport.on(&url, sse("openai_compat/reasoning.sse"));
    }
    let sent = test_through(env, keychain, id, args, transport.clone(), net_rig::roomy()).await;
    OnWire {
        sent,
        hops: transport.hops(),
    }
}

/// `accounts list` from a pipe: the line naming `id`, and the Keychain
/// calls it made.
async fn list_line(env: &Env, keychain: &Arc<FakeSecretStore>, id: &str) -> (String, Vec<String>) {
    let kit = Kit::new(FakeTerminal::piped(), keychain);
    let ran = kit.run(env, &["list"]).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    let line = ran
        .out
        .lines()
        .filter(|l| l.split_whitespace().next() == Some(id))
        .collect::<Vec<_>>()
        .join("\n");
    (line, kit.calls())
}

/// A transport whose every hop fails as reqwest words a local server that
/// is not answering, recording each hop's URL.
struct Failing {
    words: String,
    urls: Mutex<Vec<String>>,
}

impl Failing {
    fn new(words: &str) -> Arc<Failing> {
        Arc::new(Failing {
            words: words.to_string(),
            urls: Mutex::new(Vec::new()),
        })
    }
}

impl am_net::Transport for Failing {
    fn send(&self, hop: am_net::Hop) -> BoxFuture<Result<HttpResponse, EgressError>> {
        self.urls.lock().unwrap().push(hop.url.clone());
        let words = self.words.clone();
        Box::pin(async move { Err(EgressError::Network(words)) })
    }
}

// ------------------------------------------------------------ the readings

fn audit_lines(audit: &str, kind: &str) -> Vec<String> {
    audit
        .lines()
        .filter(|l| l.contains(&format!("\"kind\":\"{kind}\"")))
        .map(str::to_string)
        .collect()
}

/// The audit lines of `kind` for the Account `id`, oldest first.
fn records_of(audit: &str, kind: &str, id: &str) -> Vec<String> {
    audit_lines(audit, kind)
        .into_iter()
        .filter(|l| has(l, "account", id))
        .collect()
}

fn has(line: &str, key: &str, value: &str) -> bool {
    line.contains(&format!("\"{key}\":\"{value}\""))
}

fn seq_of(line: &str) -> i64 {
    let start = line.find("\"seq\":").unwrap() + 6;
    line[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap()
}

/// The text value of the first `"key":"…"` in `line`.
fn text_field(line: &str, key: &str) -> Option<String> {
    let start = line.find(&format!("\"{key}\":\""))? + key.len() + 4;
    let end = line[start..].find('"')?;
    Some(line[start..start + end].to_string())
}

/// AD-15 *Record*: the confirmation `note` (an audit line) was committed in
/// the same user-store transaction as the History entry `entry_id`: the
/// head before its run of notes is that entry's History commit, at the
/// same time.
fn assert_committed_with(audit: &str, note: &str, entry_id: &str) {
    let lines: Vec<&str> = audit.lines().collect();
    let at = lines
        .iter()
        .position(|l| *l == note)
        .unwrap_or_else(|| panic!("the note is in the audit log: {note}"));
    let head = lines[..at]
        .iter()
        .rev()
        .find(|l| {
            let action = text_field(l, "action").unwrap_or_default();
            action != "config.widened" && action != "config.narrowed"
        })
        .unwrap_or_else(|| panic!("a head before {note}"));
    let action = text_field(head, "action").unwrap_or_default();
    assert_ne!(
        action, "config.recorded",
        "a confirmation never rides on a record-only head: {head}"
    );
    assert!(action.starts_with("config."), "{head}");
    assert!(
        has(head, "entry_id", entry_id),
        "the head is the History entry {entry_id}: {head}"
    );
    assert_eq!(
        text_field(head, "at"),
        text_field(note, "at"),
        "one transaction: {head} / {note}"
    );
}

/// Every file under `dir` with its bytes.
fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if e.file_type().unwrap().is_dir() {
                stack.push(p);
            } else {
                out.push((p.clone(), std::fs::read(&p).unwrap()));
            }
        }
    }
    out.sort();
    out
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|w| w == needle.as_bytes())
}

/// Row 27: none of `keys` is in any output, any file under the temporary
/// HOME or the projects (stores included), any History entry or any audit
/// entry.
fn assert_no_key_leaked(env: &Env, outputs: &[String], keys: &[&str]) {
    for key in keys {
        for output in outputs {
            assert!(!output.contains(key), "printed: {output}");
        }
        for (path, bytes) in snapshot(env.dir.path()) {
            assert!(!contains(&bytes, key), "the key reached {}", path.display());
        }
        for e in env.history() {
            assert!(!format!("{e:?}").contains(key));
        }
        assert!(!env.audit().contains(key));
    }
}

/// The words of `text`, lowercased: its runs of letters and digits.
fn words_of(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// Q1: words that read as a key problem or a cloud error.
const NOT_A_LOCAL_SERVERS_WORDS: [&str; 6] = [
    "key",
    "keys",
    "keychain",
    "cloud",
    "credential",
    "credentials",
];

// =================================================================== rows

/// Row 1 (and 27): on a terminal, `add llama --provider openai-compatible
/// --base-url LOOP --no-key` with `llama` typed, beside a keyed Account and
/// with a key a removed Account of the same id left in the Keychain. The
/// sentence names the Account, the provider and LOOP, says the calls stay
/// on this Mac on port 8080 and carry no credential, and says no key is
/// read or stored; only the id is read. The entry has no `secret_ref`; one
/// History entry; `config.widened` `account_added` (`http`, `127.0.0.1`,
/// `8080`, `key: none`, `typed_id`) committed with it. Zero Keychain calls
/// of any kind; the output says keyless; no key anywhere.
#[tokio::test]
async fn a_keyless_add_on_this_mac_writes_no_secret_ref_and_records_key_none() {
    let env = env();
    let keychain = FakeSecretStore::with("llama", LEFTOVER_CANARY);
    router(&env, &keychain).await;
    let history = env.history().len();

    let kit = Kit::typing(&keychain, &["llama"]);
    let ran = kit
        .run(
            &env,
            &[
                "add",
                "llama",
                "--provider",
                "openai-compatible",
                "--base-url",
                LOOP,
                "--no-key",
            ],
        )
        .await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(
        kit.calls().is_empty(),
        "no Keychain call of any kind: {:?}",
        kit.calls()
    );
    assert_eq!(
        kit.term.echo_reads(),
        vec![true],
        "only the id; no key prompt"
    );
    assert_eq!(kit.term.timeouts()[0], Some(Duration::from_secs(120)));
    let shown = &ran.shown;
    for part in [
        "llama",
        "openai-compatible",
        LOOP,
        "stay on this Mac, on port 8080",
        "no credential",
    ] {
        assert!(shown.contains(part), "the sentence says {part}: {shown}");
    }
    assert!(
        shown.to_lowercase().contains("no key"),
        "the sentence says no key is read or stored: {shown}"
    );
    assert!(
        !shown.contains("already holds"),
        "a key the Keychain holds under the id is never named: {shown}"
    );
    assert!(ran.out.contains("keyless"), "{}", ran.all());
    assert_eq!(
        kit.key("llama").as_deref(),
        Some(LEFTOVER_CANARY),
        "untouched"
    );

    let entry = env.entry("llama");
    assert_eq!(entry.provider, "openai-compatible");
    assert_eq!(entry.base_url.as_deref(), Some(LOOP));
    assert_eq!(
        entry.secret_ref,
        None,
        "no secret_ref: {:?}",
        env.user_text()
    );
    assert_eq!(
        env.entry("router").secret_ref.as_deref(),
        Some("router"),
        "the keyed Account keeps its line"
    );
    let entries = env.history();
    assert_eq!(entries.len(), history + 1, "one History entry");
    let last = entries.last().unwrap();
    assert_eq!(last.kind, HistoryKind::Edit);
    assert_eq!(last.actor, owner());

    let audit = env.audit();
    let added = records_of(&audit, "account_added", "llama");
    assert_eq!(added.len(), 1, "{audit}");
    assert_eq!(
        text_field(&added[0], "action").as_deref(),
        Some("config.widened")
    );
    for (key, value) in [
        ("provider", "openai-compatible"),
        ("scheme", "http"),
        ("host", "127.0.0.1"),
        ("port", "8080"),
        ("key", "none"),
        ("confirmation", "typed_id"),
    ] {
        assert!(has(&added[0], key, value), "{key}={value}: {}", added[0]);
    }
    assert!(added[0].contains("\"references\""), "{}", added[0]);
    assert_committed_with(&audit, &added[0], &last.id);
    assert_no_key_leaked(&env, &[ran.all()], &[ROUTER_CANARY, LEFTOVER_CANARY]);
}

/// Rows 2 and 3 (Q2): `--no-key` with a remote endpoint, or with a provider
/// whose endpoint is fixed, is refused in words before any prompt: nothing
/// is shown, read, written or called. A fixed provider's refusal names its
/// endpoint.
#[tokio::test]
async fn a_keyless_add_is_refused_for_a_remote_endpoint_before_any_prompt() {
    let env = env();
    let keychain = FakeSecretStore::new();
    router(&env, &keychain).await;
    let before = env.state();

    for (args, names) in [
        (
            vec![
                "add",
                "remote",
                "--provider",
                "openai-compatible",
                "--base-url",
                "https://models.example/v1",
                "--no-key",
            ],
            None,
        ),
        (
            vec!["add", "remote", "--provider", "anthropic", "--no-key"],
            Some("https://api.anthropic.com"),
        ),
        (
            vec!["add", "remote", "--provider", "openai", "--no-key"],
            Some("https://api.openai.com/v1"),
        ),
    ] {
        let kit = Kit::typing(&keychain, &["remote", ROUTER_CANARY]);
        let ran = kit.run(&env, &args).await;
        assert_eq!(ran.code, 1, "{args:?}: a worded refusal: {}", ran.all());
        assert!(ran.err.contains("--no-key"), "{args:?}: {}", ran.all());
        if let Some(endpoint) = names {
            assert!(ran.err.contains(endpoint), "{args:?}: {}", ran.all());
        }
        assert_eq!(ran.shown, "", "{args:?}: nothing shown on the terminal");
        assert_eq!(kit.term.reads(), 0, "{args:?}");
        assert!(kit.calls().is_empty(), "{args:?}: {:?}", kit.calls());
        assert_eq!(env.state(), before, "{args:?}: nothing written");
    }
}

/// Row 4 (Q2): every other spelling of this Mac (`localhost`, `[::1]`,
/// another 127/8 address, `127.1`, no port, https to this Mac) is refused
/// by 2.3's endpoint rule, in its words, before any prompt.
#[tokio::test]
async fn a_keyless_add_refuses_every_other_spelling_of_this_mac() {
    let env = env();
    let keychain = FakeSecretStore::new();
    for url in [
        "http://localhost:8080",
        "http://[::1]:8080",
        "http://127.0.0.2:8080",
        "http://127.1:8080",
        "http://127.0.0.1/v1",
        "https://127.0.0.1:8080",
    ] {
        let kit = Kit::typing(&keychain, &["llama"]);
        let ran = kit
            .run(
                &env,
                &[
                    "add",
                    "llama",
                    "--provider",
                    "openai-compatible",
                    "--base-url",
                    url,
                    "--no-key",
                ],
            )
            .await;
        assert_eq!(ran.code, 1, "{url}: {}", ran.all());
        assert!(
            ran.err.contains("is not accepted"),
            "{url}: 2.3's endpoint words: {}",
            ran.all()
        );
        assert_eq!(ran.shown, "", "{url}");
        assert_eq!(kit.term.reads(), 0, "{url}");
        assert!(kit.calls().is_empty(), "{url}: {:?}", kit.calls());
        assert!(env.user_text().is_none(), "{url}: nothing written");
    }
}

/// Row 5 (Q1): an add at a reserved local Account's port (1234, 11434) or
/// at the port an `agentic serve` records is refused before any prompt,
/// with or without `--no-key`, naming the reserved Account or the app's
/// own server.
#[tokio::test]
async fn an_add_on_a_reserved_local_port_or_the_apps_own_port_is_refused() {
    let env = env();
    let keychain = FakeSecretStore::new();
    am_app::runtime::record_serving(&env.places.app_data, 9090).unwrap();

    for (url, names) in [
        ("http://127.0.0.1:1234/v1", "local-lmstudio"),
        ("http://127.0.0.1:11434/v1", "local-ollama"),
        (LOOP_9090, "the app's own server is listening on port 9090"),
    ] {
        for keyless in [false, true] {
            let mut args = vec![
                "add",
                "llama",
                "--provider",
                "openai-compatible",
                "--base-url",
                url,
            ];
            if keyless {
                args.push("--no-key");
            }
            let kit = Kit::typing(&keychain, &["llama", LOCAL_CANARY]);
            let ran = kit.run(&env, &args).await;
            assert_eq!(ran.code, 1, "{args:?}: {}", ran.all());
            assert!(
                ran.err.contains(names),
                "{args:?}: names {names}: {}",
                ran.all()
            );
            assert_eq!(kit.term.reads(), 0, "{args:?}: before any prompt");
            assert!(kit.calls().is_empty(), "{args:?}: {:?}", kit.calls());
            assert!(env.user_text().is_none(), "{args:?}: nothing written");
            assert_eq!(kit.key("llama"), None, "{args:?}");
        }
    }
    am_app::runtime::clear_serving(&env.places.app_data);
}

/// Row 6: a keyless add with a wrong id, no id within 2 minutes, or Ctrl-C
/// at the id writes nothing and makes no Keychain call (exit 1, or 130).
#[tokio::test]
async fn a_keyless_add_cancelled_or_mistyped_writes_nothing() {
    let env = env();
    let keychain = FakeSecretStore::new();
    router(&env, &keychain).await;
    let before = env.state();
    let args = [
        "add",
        "llama",
        "--provider",
        "openai-compatible",
        "--base-url",
        LOOP,
        "--no-key",
    ];

    let wrong = Kit::typing(&keychain, &["lama"]);
    let late = Kit::new(FakeTerminal::new(), &keychain);
    late.term.time_out();
    let stopped = Kit::new(FakeTerminal::new(), &keychain);
    stopped.term.interrupt();
    for (kit, code, what) in [
        (wrong, 1, "a wrong id"),
        (late, 1, "no id within 2 minutes"),
        (stopped, 130, "Ctrl-C at the id"),
    ] {
        let ran = kit.run(&env, &args).await;
        assert_eq!(ran.code, code, "{what}: {}", ran.all());
        assert!(ran.shown.contains(LOOP), "{what}: the sentence was shown");
        assert!(kit.term.reads() <= 1, "{what}: the id prompt only");
        assert!(
            kit.term.echo_reads().iter().all(|echo| *echo),
            "{what}: no hidden key prompt"
        );
        assert!(kit.calls().is_empty(), "{what}: {:?}", kit.calls());
        assert_eq!(env.state(), before, "{what}: nothing written");
    }
}

/// Rows 7 and 8: a keyless add from a pipe is refused by the terminal rule;
/// on a terminal, a reserved or existing id, an outside change of the user
/// file, and a reachable project enabling a stdio or a loopback-HTTP MCP
/// server are each refused as for a keyed add. All before any prompt, with
/// nothing written and no Keychain call.
#[tokio::test]
async fn a_keyless_add_is_refused_from_a_pipe_and_while_an_owner_child_is_enabled() {
    let keyless = |id: &'static str| {
        vec![
            "add",
            id,
            "--provider",
            "openai-compatible",
            "--base-url",
            LOOP,
            "--no-key",
        ]
    };

    // Row 7: a pipe.
    let env = env();
    let keychain = FakeSecretStore::new();
    router(&env, &keychain).await;
    let before = env.state();
    let kit = Kit::piped(&keychain, &["llama"]);
    let ran = kit.run(&env, &keyless("llama")).await;
    assert_eq!(ran.code, 1, "{}", ran.all());
    assert!(ran.err.contains("interactive terminal"), "{}", ran.all());
    assert!(ran.err.contains("accounts add"), "{}", ran.all());
    assert_eq!(kit.term.reads(), 0);
    assert!(kit.calls().is_empty(), "{:?}", kit.calls());
    assert_eq!(env.state(), before);

    // Row 8: a reserved id, an existing id.
    for (id, words) in [
        ("local-ollama", "reserved"),
        ("router", "already has an Account 'router'"),
    ] {
        let kit = Kit::typing(&keychain, &[id]);
        let ran = kit.run(&env, &keyless(id)).await;
        assert_eq!(ran.code, 1, "{id}: {}", ran.all());
        assert!(ran.err.contains(words), "{id}: {}", ran.all());
        assert_eq!(kit.term.reads(), 0, "{id}");
        assert!(kit.calls().is_empty(), "{id}: {:?}", kit.calls());
        assert_eq!(env.state(), before, "{id}");
    }

    // Row 8: the user file changed outside the app.
    let written = env.user_text().unwrap();
    std::fs::write(env.accounts_path(), format!("{written}\n")).unwrap();
    let kit = Kit::typing(&keychain, &["llama"]);
    let ran = kit.run(&env, &keyless("llama")).await;
    assert_eq!(ran.code, 1, "{}", ran.all());
    assert!(ran.err.contains(USER_FILE), "names the file: {}", ran.all());
    assert!(ran.err.contains("outside the app"), "{}", ran.all());
    assert_eq!(kit.term.reads(), 0);
    assert!(kit.calls().is_empty(), "{:?}", kit.calls());
    std::fs::write(env.accounts_path(), &written).unwrap();

    // Row 8: a reachable project enabling a child that runs as the owner.
    for (name, tools, server) in [
        ("stdio", STDIO_TOOLS.to_string(), "notes"),
        (
            "loopback-http",
            http_tools("loop-notes", "http://127.0.0.1:3001/mcp"),
            "loop-notes",
        ),
    ] {
        let env = self::env();
        let keychain = FakeSecretStore::new();
        let child = env.project(
            name,
            &[("company.yaml", COMPANY.to_string()), ("tools.yaml", tools)],
        );
        let kit = Kit::typing(&keychain, &["llama"]);
        let ran = kit.run(&env, &keyless("llama")).await;
        assert_eq!(ran.code, 1, "{name}: {}", ran.all());
        assert!(
            ran.err.contains(&child.display().to_string()),
            "{name}: names the project: {}",
            ran.all()
        );
        assert!(ran.err.contains(server), "{name}: {}", ran.all());
        assert_eq!(kit.term.reads(), 0, "{name}");
        assert!(kit.calls().is_empty(), "{name}: {:?}", kit.calls());
        assert!(env.user_text().is_none(), "{name}: nothing written");
    }
}

/// Rows 9 and 10 (guards, AD-15: "never by an empty answer at the key
/// prompt"): a loopback add without `--no-key` and an empty line at the key
/// prompt is refused; the Account is never added keyless. With a key it is
/// added as 2.3 built it: the key stored, `secret_ref`, `key: stored`.
#[tokio::test]
async fn an_empty_key_never_makes_an_account_keyless() {
    let env = env();
    let keychain = FakeSecretStore::new();
    let args = [
        "add",
        "llama",
        "--provider",
        "openai-compatible",
        "--base-url",
        LOOP,
    ];

    for empty in ["", "   "] {
        let kit = Kit::typing(&keychain, &["llama", empty]);
        let ran = kit.run(&env, &args).await;
        assert_eq!(ran.code, 1, "{empty:?}: {}", ran.all());
        assert!(
            ran.err.contains("no key was typed"),
            "{empty:?}: {}",
            ran.all()
        );
        assert_eq!(kit.term.echo_reads(), vec![true, false], "{empty:?}");
        assert!(
            kit.calls().iter().all(|c| c.starts_with("get:")),
            "{empty:?}: nothing stored or deleted: {:?}",
            kit.calls()
        );
        assert!(env.user_text().is_none(), "{empty:?}: no Account");
        assert_eq!(kit.key("llama"), None);
    }

    let kit = Kit::typing(&keychain, &["llama", LOCAL_CANARY]);
    let ran = kit.run(&env, &args).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert_eq!(kit.key("llama").as_deref(), Some(LOCAL_CANARY));
    assert_eq!(env.entry("llama").secret_ref.as_deref(), Some("llama"));
    let added = records_of(&env.audit(), "account_added", "llama");
    assert_eq!(added.len(), 1);
    assert!(has(&added[0], "key", "stored"), "{}", added[0]);
    assert!(has(&added[0], "port", "8080"), "{}", added[0]);
    assert_no_key_leaked(&env, &[ran.all()], &[LOCAL_CANARY]);
}

/// Row 11 (and 27): `test llama --model m` from a pipe, after row 1. One
/// POST to `LOOP/chat/completions` at 127.0.0.1:8080, with no credential
/// header and no name looked up; zero Keychain calls (the command's and
/// the client's); the output names the endpoint and says no credential
/// was sent. The keyed Account's and the leftover key are on no hop.
#[tokio::test]
async fn test_of_a_keyless_account_sends_no_credential_and_reads_no_key() {
    let env = env();
    let keychain = FakeSecretStore::with("llama", LEFTOVER_CANARY);
    router(&env, &keychain).await;
    add_keyless(&env, &keychain, "llama", LOOP).await;

    let wire = test_on_wire(&env, &keychain, "llama", &["--model", "m"]).await;

    assert_eq!(wire.sent.code, 0, "{}", wire.sent.all());
    assert_eq!(wire.urls(), vec![LOOP_COMPLETIONS.to_string()]);
    let hop = &wire.hops[0];
    assert_eq!(hop.method, am_domain::HttpMethod::Post);
    assert_eq!(hop.addr, "127.0.0.1:8080".parse().unwrap());
    assert!(
        !hop.carries_a_credential(),
        "no credential header: {:?}",
        hop.headers
    );
    assert!(!wire.carried(LEFTOVER_CANARY) && !wire.carried(ROUTER_CANARY));
    assert!(
        wire.sent.calls.is_empty(),
        "no Keychain call: {:?}",
        wire.sent.calls
    );
    assert!(wire.sent.lookups.is_empty(), "{:?}", wire.sent.lookups);
    assert!(wire.sent.out.contains(LOOP), "{}", wire.sent.all());
    assert!(
        wire.sent.out.contains("no credential"),
        "{}",
        wire.sent.all()
    );
    assert_no_key_leaked(&env, &[wire.sent.all()], &[ROUTER_CANARY, LEFTOVER_CANARY]);
}

/// Owner addition Q1: when a keyless Account's server refuses the
/// connection, times out (no reply head, or a reply that stops) or answers
/// nothing, `test` says the local model server is not answering, naming
/// the Account and the address; the words never read as a key problem or
/// a cloud error, and no Keychain call is made. A keyed remote Account's
/// failure is never called a local model server.
#[tokio::test]
async fn a_keyless_server_that_is_not_answering_is_named_and_never_a_key_or_cloud_problem() {
    let env = env();
    let keychain = FakeSecretStore::with("llama", LEFTOVER_CANARY);
    router(&env, &keychain).await;
    add_keyless(&env, &keychain, "llama", LOOP).await;

    let short = Duration::from_millis(50);
    let head = am_net::Limits {
        account: am_net::Timeouts {
            head: Some(short),
            idle: None,
        },
        ..net_rig::roomy()
    };
    let idle = am_net::Limits {
        account: am_net::Timeouts {
            head: None,
            idle: Some(short),
        },
        ..net_rig::roomy()
    };
    let late = net_rig::ScriptedTransport::new();
    late.on(
        LOOP_COMPLETIONS,
        net_rig::Reply::late(Duration::from_secs(5)),
    );
    let stalls = net_rig::ScriptedTransport::new();
    stalls.on(LOOP_COMPLETIONS, net_rig::Reply::stalling(&[]));
    let cases: Vec<(&str, Arc<dyn am_net::Transport>, am_net::Limits)> = vec![
        (
            "the connection is refused",
            Failing::new(
                "error sending request: client error (Connect): tcp connect error: Connection refused (os error 61)",
            ),
            net_rig::roomy(),
        ),
        (
            "the connection times out",
            Failing::new(
                "error sending request: client error (Connect): tcp connect error: operation timed out",
            ),
            net_rig::roomy(),
        ),
        (
            "it answers nothing",
            Failing::new(
                "error sending request: client error (SendRequest): connection closed before message completed",
            ),
            net_rig::roomy(),
        ),
        ("no reply head in time", late, head),
        ("a reply that stops", stalls, idle),
    ];
    for (what, transport, limits) in cases {
        let sent = test_through(
            &env,
            &keychain,
            "llama",
            &["--model", "m"],
            transport,
            limits,
        )
        .await;
        assert_eq!(sent.code, 1, "{what}: {}", sent.all());
        for part in [
            "local model server",
            "not answering",
            "llama",
            "127.0.0.1:8080",
        ] {
            assert!(
                sent.err.contains(part),
                "{what}: says {part}: {}",
                sent.all()
            );
        }
        let words = words_of(&sent.all());
        for word in NOT_A_LOCAL_SERVERS_WORDS {
            assert!(
                !words.iter().any(|w| w == word),
                "{what}: never reads as a key or cloud problem ({word}): {}",
                sent.all()
            );
        }
        assert!(sent.calls.is_empty(), "{what}: {:?}", sent.calls);
    }

    let refused = Failing::new(
        "error sending request: client error (Connect): tcp connect error: Connection refused (os error 61)",
    );
    let sent = test_through(
        &env,
        &keychain,
        "router",
        &["--model", "m"],
        refused,
        net_rig::roomy(),
    )
    .await;
    assert_eq!(sent.code, 1, "{}", sent.all());
    assert!(
        !sent.all().contains("local model server"),
        "a remote Account is not a local model server: {}",
        sent.all()
    );
}

/// Row 13 (am-app side; Q4): the key-use check passes a keyless Account
/// with no key (`Confirmed::key`), and the directory answers its stamp as
/// keyless (`AccountStamp::keyed`), pinned to `http 127.0.0.1 8080`; a
/// keyed Account beside it is answered keyed. Through a real egress client
/// on that directory, a request for it with no credential reference
/// reaches 127.0.0.1:8080 with no credential, and one carrying a
/// reference to its id is refused and audited before any Keychain call:
/// the key a removed Account left under the id is on no hop.
#[tokio::test]
async fn the_directory_answers_a_keyless_account_with_no_key() {
    let env = env();
    let keychain = FakeSecretStore::with("llama", LEFTOVER_CANARY);
    router(&env, &keychain).await;
    add_keyless(&env, &keychain, "llama", LOOP).await;

    let llama = am_app::key_use::check(&env.places, "llama").expect("a keyless Account passes");
    assert_eq!(llama.key, KeyState::None);
    assert_eq!(llama.endpoint(), loopback(8080));
    let keyed = am_app::key_use::check(&env.places, "router").expect("the keyed Account passes");
    assert_eq!(keyed.key, KeyState::Stored);

    let directory = ProjectDirectory::for_accounts(&env.places);
    let stamp = directory.account_stamp("llama").expect("answered");
    assert!(!stamp.keyed, "answered as keyless: {stamp:?}");
    assert_eq!(stamp.endpoint, loopback(8080));
    assert_eq!(directory.account_endpoint("llama"), Some(loopback(8080)));
    let keyed_stamp = directory.account_stamp("router").expect("answered");
    assert!(keyed_stamp.keyed, "{keyed_stamp:?}");

    let store = Recording::new(&keychain);
    let transport = net_rig::ScriptedTransport::new();
    let audit = CollectAudit::new();
    let egress = am_net::NetEgress::with_seams(
        Arc::new(directory),
        store.clone(),
        audit.clone(),
        am_net::Seams {
            resolver: net_rig::ScriptedResolver::standard(),
            transport: transport.clone(),
            limits: net_rig::roomy(),
        },
    );

    let sent = egress
        .send(net_rig::post(net_rig::account("llama"), LOOP_COMPLETIONS))
        .await;
    assert!(
        sent.is_ok(),
        "no credential reference: sent: {:?}",
        sent.err()
    );
    let hops = transport.hops();
    assert_eq!(hops.len(), 1);
    assert_eq!(hops[0].addr, "127.0.0.1:8080".parse().unwrap());
    assert!(!hops[0].carries_a_credential(), "{:?}", hops[0].headers);

    for scheme in [AuthScheme::Bearer, AuthScheme::XApiKey] {
        let refused = egress
            .send(net_rig::keyed(
                net_rig::post(net_rig::account("llama"), LOOP_COMPLETIONS),
                scheme,
                "llama",
            ))
            .await;
        match refused {
            Err(EgressError::Blocked(words)) => {
                assert!(!words.contains(LEFTOVER_CANARY), "{words}");
            }
            Err(other) => panic!("{scheme:?}: expected a refusal, got {other:?}"),
            Ok(resp) => panic!(
                "{scheme:?}: expected a refusal, sent (HTTP {})",
                resp.status
            ),
        }
    }
    assert!(
        store.calls().is_empty(),
        "no Keychain call of any kind: {:?}",
        store.calls()
    );
    assert_eq!(transport.hops().len(), 1, "nothing more was sent");
    assert!(
        transport
            .hops()
            .iter()
            .all(|h| h.headers.iter().all(|(_, v)| !v.contains(LEFTOVER_CANARY)))
    );
    let blocks: Vec<_> = audit
        .events()
        .into_iter()
        .filter(|e| e.action == "egress.blocked")
        .collect();
    assert_eq!(blocks.len(), 2, "each refusal is audited: {blocks:?}");
    for block in &blocks {
        assert_eq!(
            block.args.get("account").map(String::as_str),
            Some("llama"),
            "{block:?}"
        );
    }
}

/// Row 14: a confirmed keyed Account whose `secret_ref` an app write
/// removed (a History entry, so the hash check passes) is refused before
/// any Keychain read: `test` sends nothing and names the line to put back;
/// the directory answers no stamp and says why; `list` says unconfirmed.
#[tokio::test]
async fn a_keyed_account_whose_secret_ref_an_app_write_removed_is_refused() {
    let env = env();
    let keychain = FakeSecretStore::new();
    router(&env, &keychain).await;
    env.app_write_secret_ref("router", false);
    assert_eq!(env.entry("router").secret_ref, None);

    let wire = test_on_wire(&env, &keychain, "router", &["--model", "m"]).await;
    assert_eq!(wire.sent.code, 1, "{}", wire.sent.all());
    assert!(
        wire.sent.calls.is_empty(),
        "no Keychain read: {:?}",
        wire.sent.calls
    );
    assert!(wire.hops.is_empty(), "nothing sent: {:?}", wire.urls());
    assert!(
        wire.sent.err.contains("secret_ref: router"),
        "names the line to put back: {}",
        wire.sent.all()
    );
    assert!(!wire.sent.all().contains(ROUTER_CANARY));

    let directory = ProjectDirectory::for_accounts(&env.places);
    assert!(directory.account_stamp("router").is_none());
    let why = directory.account_refusal("router").expect("worded");
    assert!(why.contains("secret_ref"), "{why}");

    let (line, _) = list_line(&env, &keychain, "router").await;
    assert!(line.contains("unconfirmed"), "{line}");
    assert!(line.contains("secret_ref"), "{line}");
}

/// Row 15 (guard: the ticket's verify): a hand edit that removes
/// `secret_ref` from a keyed remote Account is an outside change of the
/// user file, refused before any Keychain read.
#[tokio::test]
async fn a_hand_edit_removing_secret_ref_from_a_keyed_remote_account_is_refused() {
    let env = env();
    let keychain = FakeSecretStore::new();
    router(&env, &keychain).await;
    let written = env.user_text().unwrap();
    let edited: String = written
        .lines()
        .filter(|l| l.trim() != "secret_ref: router")
        .map(|l| format!("{l}\n"))
        .collect();
    assert_ne!(edited, written, "the line was there");
    std::fs::write(env.accounts_path(), &edited).unwrap();

    let wire = test_on_wire(&env, &keychain, "router", &["--model", "m"]).await;
    assert_eq!(wire.sent.code, 1, "{}", wire.sent.all());
    assert!(wire.sent.calls.is_empty(), "{:?}", wire.sent.calls);
    assert!(wire.hops.is_empty(), "{:?}", wire.urls());
    assert!(
        wire.sent.err.contains("outside the app"),
        "{}",
        wire.sent.all()
    );
    assert!(
        ProjectDirectory::for_accounts(&env.places)
            .account_stamp("router")
            .is_none()
    );
}

/// Row 16: a keyless Account that an app write gave `secret_ref: llama`
/// (with a key a removed Account left under the id) is refused before any
/// Keychain read: `test` names `agentic accounts change llama`; the
/// directory answers no stamp; `list` says unconfirmed.
#[tokio::test]
async fn a_keyless_account_given_a_secret_ref_by_an_app_write_is_refused() {
    let env = env();
    let keychain = FakeSecretStore::with("llama", LEFTOVER_CANARY);
    add_keyless(&env, &keychain, "llama", LOOP).await;
    env.app_write_secret_ref("llama", true);
    assert_eq!(env.entry("llama").secret_ref.as_deref(), Some("llama"));

    let wire = test_on_wire(&env, &keychain, "llama", &["--model", "m"]).await;
    assert_eq!(wire.sent.code, 1, "{}", wire.sent.all());
    assert!(
        wire.sent.calls.is_empty(),
        "no Keychain read: {:?}",
        wire.sent.calls
    );
    assert!(wire.hops.is_empty(), "nothing sent: {:?}", wire.urls());
    assert!(
        wire.sent.err.contains("agentic accounts change llama"),
        "{}",
        wire.sent.all()
    );
    assert!(!wire.sent.all().contains(LEFTOVER_CANARY));

    let directory = ProjectDirectory::for_accounts(&env.places);
    assert!(directory.account_stamp("llama").is_none());
    let why = directory.account_refusal("llama").expect("worded");
    assert!(why.contains("secret_ref"), "{why}");

    let (line, calls) = list_line(&env, &keychain, "llama").await;
    assert!(line.contains("unconfirmed"), "{line}");
    assert!(line.contains("agentic accounts change llama"), "{line}");
    assert!(
        !line.contains(LEFTOVER_CANARY) && !line.contains("LEFT"),
        "{line}"
    );
    assert!(!calls.contains(&"get:llama".to_string()), "{calls:?}");
}

/// Row 17: `list` shows the keyless Account as keyless (no key, no
/// credential), reads no Keychain entry for it, and masks the keyed one as
/// today.
#[tokio::test]
async fn list_shows_a_keyless_account_as_keyless_and_reads_no_key_for_it() {
    let env = env();
    let keychain = FakeSecretStore::with("llama", LEFTOVER_CANARY);
    router(&env, &keychain).await;
    add_keyless(&env, &keychain, "llama", LOOP).await;

    let kit = Kit::new(FakeTerminal::piped(), &keychain);
    let ran = kit.run(&env, &["list"]).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    let line = |id: &str| {
        ran.out
            .lines()
            .find(|l| l.split_whitespace().next() == Some(id))
            .unwrap_or_else(|| panic!("{id} is listed: {}", ran.out))
            .to_string()
    };
    let llama = line("llama");
    assert!(
        llama.starts_with("llama  openai-compatible  127.0.0.1:8080  keyless"),
        "{llama}"
    );
    assert!(llama.contains("no credential"), "{llama}");
    assert!(!llama.contains("unconfirmed"), "{llama}");
    assert!(!llama.contains("LEFT") && !llama.contains("••"), "{llama}");
    let router = line("router");
    assert!(router.contains("••••••••RTRT"), "masked as today: {router}");
    assert!(!router.contains("keyless"), "{router}");
    assert!(
        !kit.calls().contains(&"get:llama".to_string()),
        "no Keychain read for the keyless Account: {:?}",
        kit.calls()
    );
}

/// Row 18 (and 27): `remove llama` of a keyless Account: `account_removed`
/// and a History entry as today; the words say it had no key and never
/// send the owner to the Keychain; zero Keychain calls, and the key a
/// removed Account left under the id is untouched.
#[tokio::test]
async fn removing_a_keyless_account_makes_no_keychain_call_and_says_so() {
    let env = env();
    let keychain = FakeSecretStore::with("llama", LEFTOVER_CANARY);
    router(&env, &keychain).await;
    add_keyless(&env, &keychain, "llama", LOOP).await;
    let history = env.history().len();

    let kit = Kit::new(FakeTerminal::new(), &keychain);
    let ran = kit.run(&env, &["remove", "llama"]).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(kit.calls().is_empty(), "{:?}", kit.calls());
    assert!(ran.out.to_lowercase().contains("no key"), "{}", ran.all());
    assert!(!ran.out.contains("stays in the Keychain"), "{}", ran.all());
    assert!(!ran.out.contains("Keychain Access"), "{}", ran.all());
    assert_eq!(kit.key("llama").as_deref(), Some(LEFTOVER_CANARY));
    assert_eq!(
        records_of(&env.audit(), "account_removed", "llama").len(),
        1
    );
    assert_eq!(env.history().len(), history + 1);
    assert!(!env.user_text().unwrap().contains("llama"));
    assert_no_key_leaked(&env, &[ran.all()], &[ROUTER_CANARY, LEFTOVER_CANARY]);
}

/// Row 19 (Q5, and 27): `change llama --base-url http://127.0.0.1:9090/v1`
/// with the id and `yes` typed: the sentence says the calls carry no
/// credential (never that a key will now be sent); `account_changed` with
/// `key: none` and `typed_id_yes`; zero Keychain calls; the entry stays
/// without `secret_ref`. `test` then reaches :9090 only, with no
/// credential.
#[tokio::test]
async fn a_keyless_address_change_stays_keyless_and_makes_no_keychain_call() {
    let env = env();
    let keychain = FakeSecretStore::with("llama", LEFTOVER_CANARY);
    router(&env, &keychain).await;
    add_keyless(&env, &keychain, "llama", LOOP).await;
    let history = env.history().len();

    let kit = Kit::typing(&keychain, &["llama", YES]);
    let ran = kit
        .run(&env, &["change", "llama", "--base-url", LOOP_9090])
        .await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(kit.calls().is_empty(), "{:?}", kit.calls());
    assert_eq!(kit.term.echo_reads(), vec![true, true], "the id, then yes");
    for part in ["8080", "9090", "no credential"] {
        assert!(ran.shown.contains(part), "{part}: {}", ran.shown);
    }
    assert!(!ran.shown.contains("will now be sent"), "{}", ran.shown);
    assert!(
        !ran.all().contains("the key the Keychain holds"),
        "{}",
        ran.all()
    );
    let entry = env.entry("llama");
    assert_eq!(entry.base_url.as_deref(), Some(LOOP_9090));
    assert_eq!(entry.secret_ref, None);
    assert_eq!(env.history().len(), history + 1);
    let changed = records_of(&env.audit(), "account_changed", "llama");
    assert_eq!(changed.len(), 1);
    for (key, value) in [
        ("old_port", "8080"),
        ("new_scheme", "http"),
        ("new_host", "127.0.0.1"),
        ("new_port", "9090"),
        ("key", "none"),
        ("confirmation", "typed_id_yes"),
    ] {
        assert!(
            has(&changed[0], key, value),
            "{key}={value}: {}",
            changed[0]
        );
    }

    let wire = test_on_wire(&env, &keychain, "llama", &["--model", "m"]).await;
    assert_eq!(wire.sent.code, 0, "{}", wire.sent.all());
    assert_eq!(wire.urls(), vec![LOOP_9090_COMPLETIONS.to_string()]);
    assert_eq!(wire.hops[0].addr, "127.0.0.1:9090".parse().unwrap());
    assert!(!wire.hops[0].carries_a_credential());
    assert!(wire.sent.calls.is_empty(), "{:?}", wire.sent.calls);
    assert_no_key_leaked(
        &env,
        &[ran.all(), wire.sent.all()],
        &[ROUTER_CANARY, LEFTOVER_CANARY],
    );
}

/// Owner addition Q5: a keyless change runs only on a terminal, with the
/// typed name. From a pipe, an address change and a `--no-key`
/// confirmation are refused before anything is read; on a terminal a wrong
/// id, a missing `yes`, no answer in 2 minutes and Ctrl-C at the id each
/// change nothing. No Keychain call in any of them.
#[tokio::test]
async fn a_keyless_change_runs_only_on_a_terminal_with_the_typed_name() {
    let env = env();
    let keychain = FakeSecretStore::with("spare", LEFTOVER_CANARY);
    add_keyless(&env, &keychain, "llama", LOOP).await;
    listed_loopback(&env, "spare");
    let before = env.state();

    for args in [
        vec!["change", "llama", "--base-url", LOOP_9090],
        vec!["change", "spare", "--no-key"],
    ] {
        let kit = Kit::piped(&keychain, &[args[1], YES]);
        let ran = kit.run(&env, &args).await;
        assert_eq!(ran.code, 1, "{args:?}: {}", ran.all());
        assert!(
            ran.err.contains("interactive terminal"),
            "{args:?}: {}",
            ran.all()
        );
        assert!(
            ran.err.contains("accounts change"),
            "{args:?}: {}",
            ran.all()
        );
        assert_eq!(kit.term.reads(), 0, "{args:?}");
        assert!(kit.calls().is_empty(), "{args:?}: {:?}", kit.calls());
        assert_eq!(env.state(), before, "{args:?}");
    }

    for args in [
        vec!["change", "llama", "--base-url", LOOP_9090],
        vec!["change", "spare", "--no-key"],
    ] {
        let id = args[1];
        let wrong = Kit::typing(&keychain, &["sparrow", YES]);
        let no = Kit::typing(&keychain, &[id, "no"]);
        let late = Kit::new(FakeTerminal::new(), &keychain);
        late.term.time_out();
        let stopped = Kit::new(FakeTerminal::new(), &keychain);
        stopped.term.interrupt();
        for (kit, code, what) in [
            (wrong, 1, "a wrong id"),
            (no, 1, "no `yes`"),
            (late, 1, "no id within 2 minutes"),
            (stopped, 130, "Ctrl-C at the id"),
        ] {
            let ran = kit.run(&env, &args).await;
            assert_eq!(ran.code, code, "{args:?} {what}: {}", ran.all());
            assert!(
                ran.shown.contains(id),
                "{args:?} {what}: the sentence was shown"
            );
            assert!(kit.calls().is_empty(), "{args:?} {what}: {:?}", kit.calls());
            assert_eq!(env.state(), before, "{args:?} {what}: nothing written");
        }
    }
}

/// Row 20 (guard on a real keyless Account): a keyless Account never moves
/// to a remote endpoint, by an address or a provider change: refused before
/// any prompt, as 2.14 built it, with nothing written and no Keychain call.
#[tokio::test]
async fn a_keyless_account_never_moves_to_a_remote_endpoint() {
    let env = env();
    let keychain = FakeSecretStore::new();
    add_keyless(&env, &keychain, "llama", LOOP).await;
    let before = env.state();

    for args in [
        vec!["change", "llama", "--base-url", "https://models.example/v1"],
        vec!["change", "llama", "--provider", "openai"],
    ] {
        let kit = Kit::typing(&keychain, &["llama", YES, NEW_CANARY]);
        let ran = kit.run(&env, &args).await;
        assert_eq!(ran.code, 1, "{args:?}: {}", ran.all());
        assert!(
            ran.err.contains("stays on this Mac"),
            "{args:?}: {}",
            ran.all()
        );
        assert_eq!(kit.term.reads(), 0, "{args:?}: before any prompt");
        assert!(kit.calls().is_empty(), "{args:?}: {:?}", kit.calls());
        assert_eq!(env.state(), before, "{args:?}");
    }
}

/// Row 21 (Q5, and 27): `change <id> --no-key` confirms an Account at
/// `http://127.0.0.1:<port>` keyless, with no key prompt and no Keychain
/// call (the key under the id is not read, deleted or named), for a first
/// confirmation (an Account `migrate` listed unconfirmed) and for a
/// provider change. `account_removed` (`provider_change`, no `key_for…`)
/// and `account_added` (`key: none`, `replaces`, `typed_id_yes_new_key`)
/// commit with the History entry of the file write, which removes
/// `secret_ref`. `test` then sends with no credential.
#[tokio::test]
async fn a_first_confirmation_with_no_key_confirms_a_loopback_account_keyless() {
    let env = env();
    let keychain = FakeSecretStore::with("spare", LEFTOVER_CANARY);
    add_keyed(&env, &keychain, "cloud", "anthropic", None, ROUTER_CANARY).await;
    listed_loopback(&env, "spare");
    assert_eq!(env.entry("spare").secret_ref.as_deref(), Some("spare"));

    for (id, args, replaces) in [
        ("spare", vec!["change", "spare", "--no-key"], None),
        (
            "cloud",
            vec![
                "change",
                "cloud",
                "--provider",
                "openai-compatible",
                "--base-url",
                LOOP,
                "--no-key",
            ],
            Some("anthropic https://api.anthropic.com:443"),
        ),
    ] {
        let history = env.history().len();
        let kit = Kit::typing(&keychain, &[id, YES]);
        let ran = kit.run(&env, &args).await;

        assert_eq!(ran.code, 0, "{id}: {}", ran.all());
        assert!(
            kit.calls().is_empty(),
            "{id}: no Keychain call: {:?}",
            kit.calls()
        );
        assert_eq!(
            kit.term.echo_reads(),
            vec![true, true],
            "{id}: the id and yes; no key prompt"
        );
        assert!(ran.shown.contains("no credential"), "{id}: {}", ran.shown);
        assert!(!ran.shown.contains("type a new key"), "{id}: {}", ran.shown);
        assert!(!ran.shown.contains("already holds"), "{id}: {}", ran.shown);

        let entry = env.entry(id);
        assert_eq!(entry.provider, "openai-compatible", "{id}");
        assert_eq!(entry.base_url.as_deref(), Some(LOOP), "{id}");
        assert_eq!(entry.secret_ref, None, "{id}: {:?}", env.user_text());
        let entries = env.history();
        assert_eq!(entries.len(), history + 1, "{id}");
        let last = entries.last().unwrap();
        assert_eq!(last.kind, HistoryKind::Edit, "{id}: secret_ref removed");

        let audit = env.audit();
        assert!(records_of(&audit, "account_key_set", id).is_empty(), "{id}");
        let removed = records_of(&audit, "account_removed", id);
        let removed = removed.last().expect("account_removed");
        assert!(has(removed, "reason", "provider_change"), "{id}: {removed}");
        assert!(!removed.contains("key_for"), "{id}: {removed}");
        let added = records_of(&audit, "account_added", id);
        let added = added.last().expect("account_added");
        for (key, value) in [
            ("provider", "openai-compatible"),
            ("scheme", "http"),
            ("host", "127.0.0.1"),
            ("port", "8080"),
            ("key", "none"),
            ("confirmation", "typed_id_yes_new_key"),
        ] {
            assert!(has(added, key, value), "{id}: {key}={value}: {added}");
        }
        match replaces {
            Some(old) => assert!(has(added, "replaces", old), "{id}: {added}"),
            None => assert!(
                !added.contains("\"replaces\":\"") || has(added, "replaces", "none"),
                "{id}: no ended record to replace: {added}"
            ),
        }
        assert!(seq_of(removed) < seq_of(added), "{id}");
        assert_committed_with(&audit, removed, &last.id);
        assert_committed_with(&audit, added, &last.id);

        let wire = test_on_wire(&env, &keychain, id, &["--model", "m"]).await;
        assert_eq!(wire.sent.code, 0, "{id}: {}", wire.sent.all());
        assert_eq!(wire.urls(), vec![LOOP_COMPLETIONS.to_string()], "{id}");
        assert!(!wire.hops[0].carries_a_credential(), "{id}");
        assert!(!wire.carried(LEFTOVER_CANARY) && !wire.carried(ROUTER_CANARY));
        assert!(wire.sent.calls.is_empty(), "{id}: {:?}", wire.sent.calls);
        assert_no_key_leaked(
            &env,
            &[ran.all(), wire.sent.all()],
            &[ROUTER_CANARY, LEFTOVER_CANARY],
        );
    }
    for (id, left) in [("spare", LEFTOVER_CANARY), ("cloud", ROUTER_CANARY)] {
        assert_eq!(
            keychain
                .get(id)
                .unwrap()
                .map(|k| k.expose().to_string())
                .as_deref(),
            Some(left),
            "{id}: the key under the id is not read, deleted or replaced"
        );
    }
}

/// Row 22: `--no-key` is refused before any prompt on an address change of
/// a keyed Account (remote or on this Mac), and for a remote endpoint (a
/// provider change, a first confirmation): an address change keeps the key
/// state, and a key is dropped only by remove, then add with `--no-key`.
/// On a keyless Account's address change it is accepted and changes
/// nothing about the key state.
#[tokio::test]
async fn no_key_is_refused_on_a_keyed_address_change_and_for_a_remote_endpoint() {
    let env = env();
    let keychain = FakeSecretStore::new();
    router(&env, &keychain).await;
    add_keyed(
        &env,
        &keychain,
        "keyed-local",
        "openai-compatible",
        Some(LOOP),
        LOCAL_CANARY,
    )
    .await;
    listed_anthropic(&env, "spare", KeyState::Stored);
    add_keyless(&env, &keychain, "llama", LOOP_9090).await;
    let before = env.state();

    for args in [
        vec!["change", "router", "--base-url", NEW_REMOTE_URL, "--no-key"],
        vec!["change", "keyed-local", "--base-url", LOOP_9090, "--no-key"],
        vec!["change", "router", "--provider", "openai", "--no-key"],
        vec!["change", "spare", "--no-key"],
    ] {
        let kit = Kit::typing(&keychain, &[args[1], YES, NEW_CANARY]);
        let ran = kit.run(&env, &args).await;
        assert_eq!(ran.code, 1, "{args:?}: {}", ran.all());
        assert!(ran.err.contains("--no-key"), "{args:?}: {}", ran.all());
        assert_eq!(kit.term.reads(), 0, "{args:?}: before any prompt");
        assert!(kit.calls().is_empty(), "{args:?}: {:?}", kit.calls());
        assert_eq!(env.state(), before, "{args:?}: nothing written");
    }

    let kit = Kit::typing(&keychain, &["llama", YES]);
    let ran = kit
        .run(&env, &["change", "llama", "--base-url", LOOP, "--no-key"])
        .await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(kit.calls().is_empty(), "{:?}", kit.calls());
    assert_eq!(env.entry("llama").secret_ref, None);
    let changed = records_of(&env.audit(), "account_changed", "llama");
    assert_eq!(changed.len(), 1);
    assert!(has(&changed[0], "key", "none"), "{}", changed[0]);
}

/// Row 23: a keyed first confirmation of an Account whose entry has no
/// `secret_ref` stores the new key (delete, then set) and then writes
/// `secret_ref: <id>` with `account_added key: stored`, so file and record
/// agree; `test` then sends the new key.
#[tokio::test]
async fn a_keyed_first_confirmation_writes_the_secret_ref_the_file_lacked() {
    let env = env();
    let keychain = FakeSecretStore::new();
    listed_anthropic(&env, "spare", KeyState::None);
    assert_eq!(env.entry("spare").secret_ref, None);
    let history = env.history().len();

    let kit = Kit::typing(&keychain, &["spare", YES, NEW_CANARY]);
    let ran = kit.run(&env, &["change", "spare"]).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    assert_eq!(
        kit.calls(),
        vec!["delete:spare".to_string(), "set:spare".to_string()]
    );
    assert_eq!(kit.key("spare").as_deref(), Some(NEW_CANARY));
    assert_eq!(
        env.entry("spare").secret_ref.as_deref(),
        Some("spare"),
        "file and record agree: {:?}",
        env.user_text()
    );
    let entries = env.history();
    assert_eq!(entries.len(), history + 1);
    assert_eq!(entries.last().unwrap().kind, HistoryKind::Edit);
    let added = records_of(&env.audit(), "account_added", "spare");
    assert_eq!(added.len(), 1);
    assert!(has(&added[0], "key", "stored"), "{}", added[0]);

    let wire = test_on_wire(&env, &keychain, "spare", &[]).await;
    assert_eq!(wire.sent.code, 0, "{}", wire.sent.all());
    assert_eq!(wire.urls(), vec![ANTHROPIC_MESSAGES.to_string()]);
    assert!(wire.carried(NEW_CANARY));
    assert_no_key_leaked(&env, &[ran.all(), wire.sent.all()], &[NEW_CANARY]);
}

/// Row 24 and owner addition Q6, through the commands: the History entry of
/// a keyless confirmation (which removed `secret_ref`) and of a keyed one
/// (which added it) can be neither undone nor restored past: the writer
/// refuses, naming `agentic accounts change`, and writes nothing. Both
/// directions of a key-state change; each Undo or Restore differs from the
/// file only in that one Account's `secret_ref` line.
#[tokio::test]
async fn undo_or_restore_that_flips_a_key_state_is_refused() {
    type Act<'a> =
        Box<dyn Fn(&am_config::ConfigWriter) -> Result<HistoryEntry, am_config::WriteError> + 'a>;
    /// Each act is refused naming `agentic accounts change`, and writes
    /// nothing; one writer (one hold of the user lock) per act.
    fn refused(env: &Env, acts: Vec<(&str, Act<'_>)>) {
        for (what, act) in acts {
            let text = env.user_text();
            let entries = env.history().len();
            let result = act(&env.user_writer());
            let err = result.expect_err(what);
            assert!(
                err.to_string().contains("agentic accounts change"),
                "{what}: {err}"
            );
            assert_eq!(env.user_text(), text, "{what}: nothing written");
            assert_eq!(env.history().len(), entries, "{what}");
        }
    }
    let env = env();
    let keychain = FakeSecretStore::new();

    // Keyed to keyless: `spare` listed with `secret_ref`, confirmed keyless.
    listed_loopback(&env, "spare");
    let listed_keyed = env.history().pop().unwrap();
    let kit = Kit::typing(&keychain, &["spare", YES]);
    let ran = kit.run(&env, &["change", "spare", "--no-key"]).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    let to_keyless = env.history().pop().unwrap();
    refused(
        &env,
        vec![
            (
                "Undo of keyed to keyless",
                Box::new(|w| w.undo(owner(), &to_keyless.id)),
            ),
            (
                "Restore of the keyed entry",
                Box::new(|w| w.restore(owner(), &listed_keyed.id)),
            ),
        ],
    );
    assert_eq!(env.entry("spare").secret_ref, None);

    // Keyless to keyed: `plain` listed without it, confirmed with a key.
    listed_anthropic(&env, "plain", KeyState::None);
    let listed_bare = env.history().pop().unwrap();
    let kit = Kit::typing(&keychain, &["plain", YES, NEW_CANARY]);
    let ran = kit.run(&env, &["change", "plain"]).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    let to_keyed = env.history().pop().unwrap();
    refused(
        &env,
        vec![
            (
                "Undo of keyless to keyed",
                Box::new(|w| w.undo(owner(), &to_keyed.id)),
            ),
            (
                "Restore of the entry without secret_ref",
                Box::new(|w| w.restore(owner(), &listed_bare.id)),
            ),
        ],
    );
    assert_eq!(env.entry("plain").secret_ref.as_deref(), Some("plain"));
    assert_eq!(env.entry("spare").secret_ref, None);
}

// ======================================== review round 2 (2026-11-25)
//
// The manager's triage of review round 1 (plan, "Review triage, round 1"),
// items K1-K5, written before the fixes (tester, 2026-11-25).

/// K1: repair words that send the owner to a hand edit, which is refused as
/// an outside change of the user file. None of these may appear.
const HAND_EDIT_ADVICE: [&str; 7] = [
    "remove the `secret_ref` line",
    "remove the line",
    "back in its entry",
    "edit the file",
    "edit its entry",
    "edit accounts.yaml",
    "by hand",
];

/// K1: `words` give no hand-edit advice (no phrase of [`HAND_EDIT_ADVICE`],
/// and never the word "put").
fn assert_no_hand_edit_advice(words: &str, what: &str) {
    let lower = words.to_lowercase();
    for phrase in HAND_EDIT_ADVICE {
        assert!(
            !lower.contains(phrase),
            "{what}: hand-edit advice ({phrase}): {words}"
        );
    }
    assert!(
        !words_of(words).iter().any(|w| w == "put"),
        "{what}: hand-edit advice (put): {words}"
    );
}

/// The three forms of a key-state refusal of `id`: `test`'s words, the
/// `list` line and the directory's refusal (the words every other caller
/// shows).
async fn refusal_forms(
    env: &Env,
    keychain: &Arc<FakeSecretStore>,
    id: &str,
) -> Vec<(&'static str, String)> {
    let wire = test_on_wire(env, keychain, id, &["--model", "m"]).await;
    assert_eq!(wire.sent.code, 1, "{}", wire.sent.all());
    assert!(wire.sent.calls.is_empty(), "{:?}", wire.sent.calls);
    assert!(wire.hops.is_empty(), "{:?}", wire.urls());
    let (line, _) = list_line(env, keychain, id).await;
    let directory = ProjectDirectory::for_accounts(&env.places)
        .account_refusal(id)
        .expect("worded");
    vec![
        ("test", wire.sent.err),
        ("list", line),
        ("directory", directory),
    ]
}

/// K1 (C1, S1; matrix row 14): a keyed Account whose `secret_ref` an app
/// write removed. Every form of the refusal names `agentic accounts change
/// router` first (before the line it restores), still holds `secret_ref:
/// router`, and gives no hand-edit advice.
#[tokio::test]
async fn row_14s_words_name_change_first_and_give_no_hand_edit_advice() {
    let env = env();
    let keychain = FakeSecretStore::new();
    router(&env, &keychain).await;
    env.app_write_secret_ref("router", false);

    for (what, words) in refusal_forms(&env, &keychain, "router").await {
        let change = words.find("agentic accounts change router");
        let line = words.find("secret_ref: router");
        assert!(change.is_some(), "{what}: names change: {words}");
        assert!(
            line.is_some(),
            "{what}: holds `secret_ref: router`: {words}"
        );
        assert!(change < line, "{what}: change comes first: {words}");
        assert_no_hand_edit_advice(&words, what);
    }
}

/// K1 (C2, S1; matrix row 16): a keyless Account an app write gave
/// `secret_ref: llama`. Every form of the refusal says that `agentic
/// accounts change llama` confirms it keyless again (removing the line),
/// and that giving it a key means removing it and adding it again without
/// `--no-key`; it no longer says that `change` confirms it with a key, and
/// gives no hand-edit advice.
#[tokio::test]
async fn row_16s_words_say_change_confirms_it_keyless_and_how_to_give_it_a_key() {
    let env = env();
    let keychain = FakeSecretStore::with("llama", LEFTOVER_CANARY);
    add_keyless(&env, &keychain, "llama", LOOP).await;
    env.app_write_secret_ref("llama", true);

    for (what, words) in refusal_forms(&env, &keychain, "llama").await {
        for part in [
            "agentic accounts change llama",
            "keyless",
            "agentic accounts remove llama",
            "without `--no-key`",
        ] {
            assert!(words.contains(part), "{what}: says {part}: {words}");
        }
        assert!(
            !words.contains("with a key: `agentic accounts change"),
            "{what}: change never gives it a key: {words}"
        );
        assert_no_hand_edit_advice(&words, what);
        assert!(!words.contains(LEFTOVER_CANARY), "{what}: {words}");
    }
}

/// K1 (C2): the row-16 words are true. In the row-16 state, `agentic
/// accounts change llama` (the id and `yes`) confirms it keyless again: no
/// key prompt and no Keychain call, the `secret_ref` line removed,
/// `account_changed` with `key: none`; `test` then sends with no
/// credential. The row-14 twin: `agentic accounts change router` puts
/// `secret_ref: router` back, with no Keychain call, and `test` sends the
/// key again.
#[tokio::test]
async fn change_repairs_both_key_state_mismatches_as_the_words_say() {
    let env = env();
    let keychain = FakeSecretStore::with("llama", LEFTOVER_CANARY);
    router(&env, &keychain).await;
    add_keyless(&env, &keychain, "llama", LOOP).await;

    // Row 16.
    env.app_write_secret_ref("llama", true);
    let kit = Kit::typing(&keychain, &["llama", YES]);
    let ran = kit.run(&env, &["change", "llama"]).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(kit.calls().is_empty(), "{:?}", kit.calls());
    assert_eq!(kit.term.echo_reads(), vec![true, true], "no key prompt");
    assert_eq!(env.entry("llama").secret_ref, None, "{:?}", env.user_text());
    let changed = records_of(&env.audit(), "account_changed", "llama");
    assert_eq!(changed.len(), 1);
    assert!(has(&changed[0], "key", "none"), "{}", changed[0]);
    let wire = test_on_wire(&env, &keychain, "llama", &["--model", "m"]).await;
    assert_eq!(wire.sent.code, 0, "{}", wire.sent.all());
    assert!(!wire.hops[0].carries_a_credential());
    assert!(wire.sent.calls.is_empty(), "{:?}", wire.sent.calls);
    assert!(!wire.carried(LEFTOVER_CANARY));

    // Row 14.
    env.app_write_secret_ref("router", false);
    let kit = Kit::typing(&keychain, &["router", YES]);
    let ran = kit.run(&env, &["change", "router"]).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(kit.calls().is_empty(), "{:?}", kit.calls());
    assert_eq!(env.entry("router").secret_ref.as_deref(), Some("router"));
    let changed = records_of(&env.audit(), "account_changed", "router");
    assert_eq!(changed.len(), 1);
    assert!(has(&changed[0], "key", "stored"), "{}", changed[0]);
    let wire = test_on_wire(&env, &keychain, "router", &["--model", "m"]).await;
    assert_eq!(wire.sent.code, 0, "{}", wire.sent.all());
    assert!(wire.carried(ROUTER_CANARY));
}

/// K2 (S2): `test` never prints a raw control character from the server.
/// A keyless Account's server (a process squatting on its port, DR-187)
/// streams a reply carrying OSC 52 (a clipboard write), cursor moves, an
/// erase-line, a carriage return and a C1 CSI, and answers with an error
/// body and an error event carrying the same. What `test` prints keeps
/// the visible text, and no control character but `\n` survives.
#[tokio::test]
async fn test_prints_a_reply_and_an_error_body_with_control_characters_neutralised() {
    let env = env();
    let keychain = FakeSecretStore::new();
    add_keyless(&env, &keychain, "llama", LOOP).await;
    let raw = |status: u16, kind: &str, body: &str| net_rig::Reply {
        status,
        headers: vec![("content-type".into(), kind.into())],
        chunks: vec![body.as_bytes().to_vec()],
        head_delay: Duration::ZERO,
        stalls: false,
    };
    let stream = concat!(
        r#"data: {"choices":[{"index":0,"delta":{"content":"one\u001b]52;c;Y2FuYXJ5\u0007\n"},"finish_reason":null}]}"#,
        "\n\n",
        r#"data: {"choices":[{"index":0,"delta":{"content":"two\u001b[1A\u001b[2K\rthree\u009b2J\u0008\n"},"finish_reason":null}]}"#,
        "\n\n",
        r#"data: {"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#,
        "\n\n",
        "data: [DONE]\n\n",
    );
    let event = concat!(
        r#"data: {"error":{"message":"denied\u001b]52;c;Y2FuYXJ5\u0007\u001b[2K\rgranted"}}"#,
        "\n\n",
    );
    let cases = [
        (
            "a streamed reply",
            raw(200, "text/event-stream", stream),
            0,
            vec!["one", "two", "three"],
        ),
        (
            "an error body",
            raw(
                400,
                "application/json",
                "bad\u{1b}]52;c;Y2FuYXJ5\u{7}\u{1b}[1A\u{1b}[2K\rrequest\u{9b}2J",
            ),
            1,
            vec!["bad", "request"],
        ),
        (
            "an error event",
            raw(200, "text/event-stream", event),
            1,
            vec!["denied", "granted"],
        ),
    ];
    for (what, reply, code, visible) in cases {
        let transport = net_rig::ScriptedTransport::new();
        transport.on(LOOP_COMPLETIONS, reply);
        let sent = test_through(
            &env,
            &keychain,
            "llama",
            &["--model", "m"],
            transport,
            net_rig::roomy(),
        )
        .await;
        assert_eq!(sent.code, code, "{what}: {:?}", sent.all());
        let printed = sent.all();
        let raw_controls: Vec<char> = printed
            .chars()
            .filter(|c| c.is_control() && *c != '\n')
            .collect();
        assert!(
            raw_controls.is_empty(),
            "{what}: raw control characters printed {raw_controls:?}: {printed:?}"
        );
        for part in visible {
            assert!(printed.contains(part), "{what}: keeps {part}: {printed:?}");
        }
        assert!(sent.calls.is_empty(), "{what}: {:?}", sent.calls);
    }
}

/// K3 (C3, S3): `add`'s sentence, when the Keychain holds a key under the
/// id, never names a keyless confirmation (a `key: none` record) as where
/// that key was "last confirmed": the key was never confirmed there. With
/// an earlier keyed confirmation it names that one; with none, it says no
/// confirmation of the key is recorded.
#[tokio::test]
async fn add_never_names_a_keyless_record_as_where_a_held_key_was_last_confirmed() {
    let keyless_record = "last confirmed for provider openai-compatible at http://127.0.0.1";

    // An earlier keyed confirmation at openrouter.example, then a keyless
    // one at this Mac; the key from the first stays in the Keychain.
    let env = env();
    let keychain = FakeSecretStore::new();
    add_keyed(
        &env,
        &keychain,
        "llama",
        "openai-compatible",
        Some(REMOTE_URL),
        ROUTER_CANARY,
    )
    .await;
    for step in [&["remove", "llama"][..]] {
        let ran = Kit::new(FakeTerminal::new(), &keychain)
            .run(&env, step)
            .await;
        assert_eq!(ran.code, 0, "{}", ran.all());
    }
    add_keyless(&env, &keychain, "llama", LOOP).await;
    let ran = Kit::new(FakeTerminal::new(), &keychain)
        .run(&env, &["remove", "llama"])
        .await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    let kit = Kit::typing(&keychain, &["not-the-id"]);
    let ran = kit
        .run(&env, &["add", "llama", "--provider", "anthropic"])
        .await;
    assert_eq!(ran.code, 1, "stopped at the wrong id: {}", ran.all());
    assert!(ran.shown.contains("already holds a key"), "{}", ran.shown);
    assert!(!ran.shown.contains(keyless_record), "{}", ran.shown);
    assert!(
        ran.shown.contains(
            "last confirmed for provider openai-compatible at https://openrouter.example:443"
        ),
        "names the keyed confirmation: {}",
        ran.shown
    );

    // Only a keyless confirmation, beside a key left under the id.
    let env = self::env();
    let keychain = FakeSecretStore::with("llama", LEFTOVER_CANARY);
    add_keyless(&env, &keychain, "llama", LOOP).await;
    let ran = Kit::new(FakeTerminal::new(), &keychain)
        .run(&env, &["remove", "llama"])
        .await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    let kit = Kit::typing(&keychain, &["not-the-id"]);
    let ran = kit
        .run(&env, &["add", "llama", "--provider", "anthropic"])
        .await;
    assert_eq!(ran.code, 1, "{}", ran.all());
    assert!(ran.shown.contains("already holds a key"), "{}", ran.shown);
    assert!(!ran.shown.contains(keyless_record), "{}", ran.shown);
    assert!(
        ran.shown
            .contains("No confirmation of it is recorded on this Mac"),
        "{}",
        ran.shown
    );
    assert!(!ran.all().contains(LEFTOVER_CANARY));
}

/// K4 (C4, S5): `change` refuses a reserved local Account's port (1234,
/// 11434) before any prompt, as `add` does, for an address change (keyless
/// or keyed) and a provider change (with a key or `--no-key`), naming the
/// reserved Account; nothing is written and no Keychain call is made.
#[tokio::test]
async fn change_to_a_reserved_local_port_is_refused_before_any_prompt() {
    let env = env();
    let keychain = FakeSecretStore::new();
    add_keyed(&env, &keychain, "cloud", "anthropic", None, ROUTER_CANARY).await;
    add_keyed(
        &env,
        &keychain,
        "keyed-local",
        "openai-compatible",
        Some(LOOP_9090),
        LOCAL_CANARY,
    )
    .await;
    add_keyless(&env, &keychain, "llama", LOOP).await;
    let before = env.state();
    let lmstudio = "http://127.0.0.1:1234/v1";
    let ollama = "http://127.0.0.1:11434/v1";

    for (args, names) in [
        (
            vec!["change", "llama", "--base-url", lmstudio],
            "local-lmstudio",
        ),
        (
            vec!["change", "llama", "--base-url", ollama],
            "local-ollama",
        ),
        (
            vec!["change", "keyed-local", "--base-url", lmstudio],
            "local-lmstudio",
        ),
        (
            vec![
                "change",
                "cloud",
                "--provider",
                "openai-compatible",
                "--base-url",
                ollama,
            ],
            "local-ollama",
        ),
        (
            vec![
                "change",
                "cloud",
                "--provider",
                "openai-compatible",
                "--base-url",
                lmstudio,
                "--no-key",
            ],
            "local-lmstudio",
        ),
    ] {
        let kit = Kit::typing(&keychain, &[args[1], YES, NEW_CANARY]);
        let ran = kit.run(&env, &args).await;
        assert_eq!(ran.code, 1, "{args:?}: {}", ran.all());
        assert!(
            ran.err.contains(names),
            "{args:?}: names {names}: {}",
            ran.all()
        );
        assert_eq!(kit.term.reads(), 0, "{args:?}: before any prompt");
        assert!(kit.calls().is_empty(), "{args:?}: {:?}", kit.calls());
        assert_eq!(env.state(), before, "{args:?}: nothing written");
    }
}

/// K4 (C4, S5): `add`'s port check holds under the lock too. While the id
/// prompt of an add at port 9090 waits, an `agentic serve` records that
/// port; the add is refused under the lock, naming the app's own server,
/// keyless or keyed, with nothing written and no key stored.
#[tokio::test]
async fn an_add_whose_port_the_app_takes_while_it_waits_is_refused_under_the_lock() {
    let env = env();
    let keychain = FakeSecretStore::new();
    router(&env, &keychain).await;
    let before = env.state();

    for keyless in [true, false] {
        let app_data = env.places.app_data.clone();
        let kit = Kit::new(FakeTerminal::new(), &keychain);
        kit.term.line_and("llama", move || {
            am_app::runtime::record_serving(&app_data, 9090).unwrap();
        });
        let mut args = vec![
            "add",
            "llama",
            "--provider",
            "openai-compatible",
            "--base-url",
            LOOP_9090,
        ];
        if keyless {
            args.push("--no-key");
        } else {
            kit.term.line(LOCAL_CANARY);
        }
        let ran = kit.run(&env, &args).await;
        am_app::runtime::clear_serving(&env.places.app_data);

        assert_eq!(ran.code, 1, "keyless {keyless}: {}", ran.all());
        assert!(
            ran.err
                .contains("the app's own server is listening on port 9090"),
            "keyless {keyless}: {}",
            ran.all()
        );
        assert!(
            kit.calls()
                .iter()
                .all(|c| !c.starts_with("set:") && !c.starts_with("delete:")),
            "keyless {keyless}: no key stored: {:?}",
            kit.calls()
        );
        assert_eq!(kit.key("llama"), None, "keyless {keyless}");
        assert_eq!(env.state(), before, "keyless {keyless}: nothing written");
    }
}

/// K5 (C5): `remove`'s words follow the record's key state. A row-16 entry
/// (a keyless record, an entry an app write gave `secret_ref`) is described
/// as keyless, and the owner is not sent to the Keychain; a row-14 entry on
/// this Mac (a keyed record, an entry without the line) keeps the keyed
/// words. Neither makes a Keychain call.
#[tokio::test]
async fn remove_describes_an_entry_by_its_records_key_state() {
    let env = env();
    let keychain = FakeSecretStore::with("llama", LEFTOVER_CANARY);
    add_keyless(&env, &keychain, "llama", LOOP).await;
    add_keyed(
        &env,
        &keychain,
        "keyed-local",
        "openai-compatible",
        Some(LOOP_9090),
        LOCAL_CANARY,
    )
    .await;
    env.app_write_secret_ref("llama", true);
    env.app_write_secret_ref("keyed-local", false);

    let kit = Kit::new(FakeTerminal::new(), &keychain);
    let ran = kit.run(&env, &["remove", "llama"]).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(kit.calls().is_empty(), "{:?}", kit.calls());
    assert!(
        ran.out.to_lowercase().contains("no key"),
        "keyless: {}",
        ran.all()
    );
    assert!(!ran.out.contains("stays in the Keychain"), "{}", ran.all());
    assert!(!ran.out.contains("Keychain Access"), "{}", ran.all());
    assert_eq!(kit.key("llama").as_deref(), Some(LEFTOVER_CANARY));

    let kit = Kit::new(FakeTerminal::new(), &keychain);
    let ran = kit.run(&env, &["remove", "keyed-local"]).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(kit.calls().is_empty(), "{:?}", kit.calls());
    assert!(
        ran.out.contains("stays in the Keychain"),
        "keyed: {}",
        ran.all()
    );
    assert!(!ran.out.contains("It had no key"), "{}", ran.all());
    assert_eq!(kit.key("keyed-local").as_deref(), Some(LOCAL_CANARY));
}
