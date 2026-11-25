//! Story 2.14 (AD-15 *Change*, *Terminal*, *Open*, *Record*, *Key use*;
//! AD-13; AD-17): `agentic accounts change <id> [--provider P] [--base-url
//! U]`, the CLI's only way to confirm or re-confirm an Account. One test per
//! row of the story's I/O & Edge-Case Matrix, run in process through
//! `am_app::cli::accounts` with am-testkit's fakes (FakeTerminal,
//! FakeSecretStore behind a store that records every call and can refuse
//! `set`, FakeProjectIdentity) and a temporary HOME per test, on 2.12's
//! harness. `accounts test` after a change runs through a real
//! `am_net::NetEgress` on am-e2e's scripted resolver and transport, so each
//! hop and the key it carries are seen. Canary keys only; no network.
//!
//! Decisions these tests make (the plan leaves them to the build):
//!
//! - Prompts, in order: the sentence; the Account's id typed within 2
//!   minutes; then `yes` typed (any other answer writes nothing); then, on
//!   the new-key path only, the new key at the hidden prompt.
//! - Exit codes as `add`: 0 done, 1 refused (in words on `err`), 2 bad
//!   usage, 130 after Ctrl-C at a prompt.
//! - `account_changed` carries `old_provider`, `old_scheme`, `old_host`,
//!   `old_port` and `new_provider`, `new_scheme`, `new_host`, `new_port`
//!   (the key-use check already reads the `new_` ones), `key: stored` (the
//!   key state, unchanged) and `confirmation: typed_id_yes`.
//! - `account_removed` of a provider change or a first confirmation carries
//!   `reason: provider_change` (AD-15 *Record*, for both) and `key_for…`
//!   fields naming the new destination; the following `account_added`
//!   carries `replaces` (the old provider, scheme, host and port as
//!   `<provider> <scheme>://<host>:<port>`, or `none` when no live record
//!   existed) and `confirmation: typed_id_yes_new_key`.
//! - `list` says `unconfirmed` and `agentic accounts change <id>` on the
//!   line of an Account with no live confirmation record; `test` names
//!   `agentic accounts change <id>` when it refuses one.
//! - A crash after `account_removed` committed is the user writer's
//!   `BeforeRename` fail point (`am_app::runtime::testing::fail_writer_at`,
//!   keyed by the app data dir) on a provider change, whose file write comes
//!   after the record and the Keychain replacement.
//! - The Sentence row's change is a port change of a loopback Account: the
//!   matrix's `https://new.example/v1` is another host, which the pre-4.18
//!   row refuses while a reachable project names the Account.
//!
//! Written before the command exists.
#![forbid(unsafe_code)]
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use am_app::cli::{AccountsContext, accounts};
use am_app::runtime::{OpenRequest, Places, Prompt, open_project};
use am_domain::net::EgressDirectory;
use am_domain::{
    Actor, AuditNote, AuditSink, EgressClient, HistoryEntry, HistoryKind, SecretError, SecretStore,
    SecretString,
};
use am_store::user::{UserReader, UserStore};
use am_testkit::{FakeProjectIdentity, FakeSecretStore, FakeTerminal};

/// am-e2e's RULE-9 fakes: a scripted resolver and a scripted per-hop
/// transport for a real `am_net::NetEgress` (no network).
#[path = "../../am-e2e/tests/trust/support/egress.rs"]
mod net_rig;

const CANARY: &str = "sk-ant-canary-change-0a1b2c3d4e5f-WXYZ";
const ROUTER_CANARY: &str = "sk-or-canary-change-1a2b3c4d-RTRT";
const NEW_CANARY: &str = "sk-new-canary-change-5e5e5e5e-NEWK";
const USER_FILE: &str = "accounts.yaml";
const REMOTE_URL: &str = "https://openrouter.example/api/v1";
const NEW_REMOTE_URL: &str = "https://router2.example/api/v1";
const PROVIDER_URL: &str = "https://router.example/v1";
const LOOPBACK_URL: &str = "http://127.0.0.1:8080/v1";
const LOOPBACK_OTHER_PORT: &str = "http://127.0.0.1:9090/v1";
const YES: &str = "yes";

const COMPANY: &str = "schema_version: 2\nname: Example\ntimezone: UTC\ncurrency: EUR\nteams:\n  - id: core\n    name: Core team\n";

/// A `tools.yaml` enabling one `transport: http` MCP server `id` at `url`.
fn http_tools(id: &str, url: &str) -> String {
    format!(
        "schema_version: 2\nmcp_servers:\n  - id: {id}\n    transport: http\n    url: {url}\n    version: 1.0.0\n    network: none\n    tools:\n      - name: list_notes\n        class: read\n"
    )
}

/// A seat on the reserved local Account whose fallback names `account`.
fn seat(account: &str) -> String {
    format!(
        "---\nschema_version: 2\nname: Alpha\nrole: Analyst\nteam: core\nmodel:\n  account: local-lmstudio\n  model: example-local\nfallback:\n  - account: {account}\n    model: example-large\n---\nDo the work.\n"
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

    /// A project whose seat names `account`.
    fn naming(&self, name: &str, account: &str) -> PathBuf {
        self.project(
            name,
            &[
                ("company.yaml", COMPANY.to_string()),
                ("seats/alpha.md", seat(account)),
            ],
        )
    }

    /// A project this Mac knows whose folder is gone.
    fn unreachable(&self, name: &str, account: &str) -> String {
        let gone = self.naming(name, account);
        let text = gone.display().to_string();
        std::fs::remove_dir_all(&gone).unwrap();
        text
    }

    fn app_data(&self) -> PathBuf {
        self.places.user.dir().to_path_buf()
    }

    fn accounts_path(&self) -> PathBuf {
        self.places.user.accounts_file()
    }

    fn user_text(&self) -> Option<String> {
        std::fs::read_to_string(self.accounts_path()).ok()
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

    /// Everything a refused command must leave as it was.
    fn state(&self) -> (Option<String>, usize, String) {
        (self.user_text(), self.history().len(), self.audit())
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
/// `delete:<id>`, in order) and, when told, refuses `set` as a denied or
/// locked Keychain would.
struct Recording {
    inner: Arc<FakeSecretStore>,
    calls: Mutex<Vec<String>>,
    refuse_set: AtomicBool,
    refuse_delete: AtomicBool,
}

impl Recording {
    fn new(inner: &Arc<FakeSecretStore>) -> Arc<Recording> {
        Arc::new(Recording {
            inner: inner.clone(),
            calls: Mutex::new(Vec::new()),
            refuse_set: AtomicBool::new(false),
            refuse_delete: AtomicBool::new(false),
        })
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn gets(&self) -> usize {
        self.calls()
            .iter()
            .filter(|c| c.starts_with("get:"))
            .count()
    }
}

impl SecretStore for Recording {
    fn get(&self, account: &str) -> Result<Option<SecretString>, SecretError> {
        self.calls.lock().unwrap().push(format!("get:{account}"));
        self.inner.get(account)
    }
    fn set(&self, account: &str, secret: SecretString) -> Result<(), SecretError> {
        self.calls.lock().unwrap().push(format!("set:{account}"));
        if self.refuse_set.load(Ordering::SeqCst) {
            return Err(SecretError::Store("the keychain is locked".into()));
        }
        self.inner.set(account, secret)
    }
    fn delete(&self, account: &str) -> Result<(), SecretError> {
        self.calls.lock().unwrap().push(format!("delete:{account}"));
        if self.refuse_delete.load(Ordering::SeqCst) {
            return Err(SecretError::Store("the keychain is locked".into()));
        }
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

/// `agentic accounts add` as the owner runs it (typed id, then the key).
async fn add(
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
    assert_eq!(ran.code, 0, "setup add {id}: {}", ran.all());
}

/// `router` (openai-compatible at openrouter.example) on this Mac.
async fn router(env: &Env, keychain: &Arc<FakeSecretStore>) {
    add(
        env,
        keychain,
        "router",
        "openai-compatible",
        Some(REMOTE_URL),
        ROUTER_CANARY,
    )
    .await;
}

/// `cloud` (anthropic) on this Mac.
async fn cloud(env: &Env, keychain: &Arc<FakeSecretStore>) {
    add(env, keychain, "cloud", "anthropic", None, CANARY).await;
}

/// `id` written to the user file by the app as `accounts migrate` leaves an
/// Account it could not confirm (story 2.13): its History entry and an
/// `account_listed` record (`confirmation: none`), no confirmation record.
fn listed_unconfirmed(env: &Env, id: &str) {
    let note = AuditNote {
        action: "config.widened".into(),
        target: id.into(),
        args: [
            ("kind", "account_listed"),
            ("file", USER_FILE),
            ("account", id),
            ("provider", "anthropic"),
            ("scheme", "https"),
            ("host", "api.anthropic.com"),
            ("port", "443"),
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
                am_config::add_account_edit(
                    &now,
                    id,
                    "anthropic",
                    None,
                    None,
                    am_config::KeyState::Stored,
                ),
                built_on,
                vec![note],
            )
            .unwrap(),
        None => writer
            .create(
                owner(),
                am_config::ConfigCreate {
                    file: USER_FILE.into(),
                    bytes: am_config::new_accounts_file(
                        id,
                        "anthropic",
                        None,
                        None,
                        am_config::KeyState::Stored,
                    )
                    .unwrap(),
                    audit: vec![note],
                },
            )
            .unwrap(),
    };
}

// ------------------------------------------------------- test on the wire

/// What `accounts test` did through a real egress client: its exit code and
/// words, every hop the transport saw, and the Keychain reads.
struct Sent {
    code: i32,
    all: String,
    hops: Vec<net_rig::SeenHop>,
    gets: usize,
}

impl Sent {
    fn hosts(&self) -> Vec<String> {
        self.hops.iter().map(|h| h.url.clone()).collect()
    }

    /// True when some hop carried `key` in a header.
    fn carried(&self, key: &str) -> bool {
        self.hops
            .iter()
            .any(|h| h.headers.iter().any(|(_, v)| v.contains(key)))
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
/// client reading keys from `keychain`.
async fn test_on_wire(env: &Env, keychain: &Arc<FakeSecretStore>, id: &str, args: &[&str]) -> Sent {
    let store = Recording::new(keychain);
    let transport = net_rig::ScriptedTransport::new();
    transport.on(
        "https://api.anthropic.com/v1/messages",
        sse("anthropic/c1_count_stream.sse"),
    );
    for base in [
        REMOTE_URL,
        NEW_REMOTE_URL,
        PROVIDER_URL,
        "https://openrouter.example/v2",
    ] {
        transport.on(
            &format!("{base}/chat/completions"),
            sse("openai_compat/reasoning.sse"),
        );
    }
    let resolver = net_rig::ScriptedResolver::standard();
    resolver.answer("openrouter.example", &[net_rig::OTHER_IP]);
    resolver.answer("router2.example", &[net_rig::FILES_IP]);
    resolver.answer("router.example", &[net_rig::DOCS_IP]);
    let (client_store, client_transport) = (store.clone(), transport.clone());
    let factory: am_app::accounts::EgressFactory = Arc::new(
        move |directory: Arc<dyn EgressDirectory>, audit: Arc<dyn AuditSink>| {
            Arc::new(am_net::NetEgress::with_seams(
                directory,
                client_store.clone(),
                audit,
                am_net::Seams {
                    resolver: resolver.clone(),
                    transport: client_transport.clone(),
                    limits: net_rig::roomy(),
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
        all: format!(
            "{}\n{}",
            String::from_utf8_lossy(&out),
            String::from_utf8_lossy(&err)
        ),
        hops: transport.hops(),
        gets: store.gets(),
    }
}

/// `accounts list` from a pipe: the line naming `id`.
async fn list_line(env: &Env, keychain: &Arc<FakeSecretStore>, id: &str) -> String {
    let kit = Kit::new(FakeTerminal::piped(), keychain);
    let ran = kit.run(env, &["list"]).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    ran.out
        .lines()
        .filter(|l| l.split_whitespace().next() == Some(id))
        .collect::<Vec<_>>()
        .join("\n")
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

/// Review fix G-record (AD-15 *Record*): the confirmation `note` (an audit
/// line) was committed in the same user-store transaction as the History
/// entry `entry_id`: the head entry right before its run of notes is that
/// entry's History commit (`config.edited`, `config.created`, …, never a
/// `config.recorded` head with no History entry of its own), at the same
/// time.
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

/// No key: none of `keys` is in any output, any file under the temporary
/// HOME or the projects (stores included), any History entry or any audit
/// entry.
fn assert_no_key_leaked(env: &Env, runs: &[&Ran], keys: &[&str]) {
    for key in keys {
        for ran in runs {
            assert!(!ran.all().contains(key), "printed: {}", ran.all());
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

// =================================================================== rows

/// Sentence: `change router --base-url <another port>`; a reachable project
/// names `router` in a seat and another root is unreachable. Before the id
/// prompt the sentence shows the old and the new provider and address side
/// by side, the reachable project with its seat file, and the other project
/// as not checked; the id prompt has 2 minutes, then `yes` is asked.
#[tokio::test]
async fn the_sentence_shows_old_and_new_and_every_reference() {
    let env = env();
    let keychain = FakeSecretStore::new();
    let here = env.naming("here", "router");
    let gone = env.unreachable("gone", "router");
    add(
        &env,
        &keychain,
        "router",
        "openai-compatible",
        Some(LOOPBACK_URL),
        ROUTER_CANARY,
    )
    .await;

    let kit = Kit::typing(&keychain, &["router", YES]);
    let ran = kit
        .run(
            &env,
            &["change", "router", "--base-url", LOOPBACK_OTHER_PORT],
        )
        .await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    let shown = &ran.shown;
    for part in [
        "router",
        "openai-compatible",
        "127.0.0.1",
        "8080",
        "9090",
        "seats/alpha.md",
        "not checked",
    ] {
        assert!(shown.contains(part), "the sentence shows {part}: {shown}");
    }
    assert!(
        shown.contains(&here.display().to_string()),
        "the reachable project: {shown}"
    );
    assert!(shown.contains(&gone), "the unreachable project: {shown}");
    assert_eq!(kit.term.reads(), 2, "the id, then yes");
    assert_eq!(
        kit.term.timeouts()[0],
        Some(Duration::from_secs(120)),
        "the id within 2 minutes"
    );
    assert!(
        kit.calls().is_empty(),
        "an address change: no Keychain call"
    );
}

/// Wrong answer: a wrong id, a right id and `no`, or no id within 2 minutes
/// writes nothing and makes no Keychain call; the old record still governs,
/// so `test` still reaches the old endpoint.
#[tokio::test]
async fn a_wrong_answer_writes_nothing_and_the_old_record_governs() {
    let env = env();
    let keychain = FakeSecretStore::new();
    router(&env, &keychain).await;
    let before = env.state();

    for typed in [
        vec!["routre", YES],
        vec!["router", "no"],
        vec!["router", "y"],
    ] {
        let kit = Kit::typing(&keychain, &typed);
        let ran = kit
            .run(&env, &["change", "router", "--base-url", NEW_REMOTE_URL])
            .await;
        assert_eq!(ran.code, 1, "{typed:?}: {}", ran.all());
        assert!(
            ran.shown.contains("router2.example"),
            "{typed:?}: the sentence was shown first: {}",
            ran.all()
        );
        assert!(kit.calls().is_empty(), "{typed:?}: {:?}", kit.calls());
        assert_eq!(env.state(), before, "{typed:?}: nothing written");
    }
    let kit = Kit::new(FakeTerminal::new(), &keychain);
    kit.term.time_out();
    let ran = kit
        .run(&env, &["change", "router", "--base-url", NEW_REMOTE_URL])
        .await;
    assert_eq!(ran.code, 1, "{}", ran.all());
    assert!(ran.all().contains("2 minutes"), "{}", ran.all());
    assert_eq!(env.state(), before);

    let sent = test_on_wire(&env, &keychain, "router", &["--model", "m"]).await;
    assert_eq!(sent.code, 0, "{}", sent.all);
    assert_eq!(sent.hops.len(), 1, "{:?}", sent.hosts());
    assert!(sent.hops[0].url.starts_with("https://openrouter.example/"));
    assert!(sent.carried(ROUTER_CANARY));
}

/// Address change: same provider, a new host, no project naming it; typed
/// id and `yes`. No Keychain call at all; History plus `account_changed`
/// (old and new values, key state unchanged, `typed_id_yes`, no key); `test`
/// then sends the same key to the new endpoint only.
#[tokio::test]
async fn an_address_change_makes_no_keychain_call_and_moves_the_same_key() {
    let env = env();
    let keychain = FakeSecretStore::new();
    router(&env, &keychain).await;
    let history = env.history().len();

    let kit = Kit::typing(&keychain, &["router", YES]);
    let ran = kit
        .run(&env, &["change", "router", "--base-url", NEW_REMOTE_URL])
        .await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(
        kit.calls().is_empty(),
        "no Keychain call: {:?}",
        kit.calls()
    );
    assert_eq!(kit.term.echo_reads(), vec![true, true], "no key prompt");
    for part in ["openrouter.example", "router2.example"] {
        assert!(ran.shown.contains(part), "{part}: {}", ran.shown);
    }
    assert!(env.user_text().unwrap().contains(NEW_REMOTE_URL));
    let entries = env.history();
    assert_eq!(entries.len(), history + 1);
    assert_eq!(entries.last().unwrap().kind, HistoryKind::Edit);
    assert_eq!(entries.last().unwrap().actor, owner());
    let audit = env.audit();
    let changed = records_of(&audit, "account_changed", "router");
    assert_eq!(changed.len(), 1, "{audit}");
    for (key, value) in [
        ("old_provider", "openai-compatible"),
        ("old_scheme", "https"),
        ("old_host", "openrouter.example"),
        ("old_port", "443"),
        ("new_provider", "openai-compatible"),
        ("new_scheme", "https"),
        ("new_host", "router2.example"),
        ("new_port", "443"),
        ("key", "stored"),
        ("confirmation", "typed_id_yes"),
    ] {
        assert!(
            has(&changed[0], key, value),
            "{key}={value}: {}",
            changed[0]
        );
    }
    assert!(changed[0].contains("\"references\""), "{}", changed[0]);
    assert_eq!(records_of(&audit, "account_removed", "router").len(), 0);
    assert_eq!(records_of(&audit, "account_added", "router").len(), 1);
    assert_eq!(kit.key("router").as_deref(), Some(ROUTER_CANARY));

    let sent = test_on_wire(&env, &keychain, "router", &["--model", "m"]).await;
    assert_eq!(sent.code, 0, "{}", sent.all);
    assert_eq!(sent.hops.len(), 1, "{:?}", sent.hosts());
    assert!(
        sent.hops[0].url.starts_with("https://router2.example/"),
        "{:?}",
        sent.hosts()
    );
    assert!(sent.carried(ROUTER_CANARY), "the same key");
    assert_no_key_leaked(&env, &[&ran], &[ROUTER_CANARY]);
}

/// Provider change: `change cloud --provider openai-compatible --base-url
/// https://router.example/v1`; typed id, `yes`, a new key. `account_removed`
/// (`reason: provider_change`, naming the new destination) commits first;
/// the Keychain is `delete` then `set`, never `get`; then History plus
/// `account_added` (`typed_id_yes_new_key`, `replaces` the ended record).
/// `test` sends the new key to the new provider; the old canary reaches it
/// at no point and is in no file, row or output.
#[tokio::test]
async fn a_provider_change_removes_then_adds_with_a_new_key() {
    let env = env();
    let keychain = FakeSecretStore::new();
    cloud(&env, &keychain).await;
    let first = records_of(&env.audit(), "account_added", "cloud");
    assert_eq!(first.len(), 1);
    let history = env.history().len();

    let kit = Kit::typing(&keychain, &["cloud", YES, NEW_CANARY]);
    let ran = kit
        .run(
            &env,
            &[
                "change",
                "cloud",
                "--provider",
                "openai-compatible",
                "--base-url",
                PROVIDER_URL,
            ],
        )
        .await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    assert_eq!(
        kit.calls(),
        vec!["delete:cloud".to_string(), "set:cloud".to_string()],
        "delete then set, never get"
    );
    assert_eq!(
        kit.term.echo_reads(),
        vec![true, true, false],
        "a hidden key prompt"
    );
    assert!(kit.term.echo(), "echo is back");
    let shown = ran.shown.to_lowercase();
    for part in [
        "anthropic",
        "api.anthropic.com",
        "openai-compatible",
        "router.example",
        "new key",
        "delete",
    ] {
        assert!(
            shown.contains(part),
            "the sentence says {part}: {}",
            ran.shown
        );
    }
    assert_eq!(kit.key("cloud").as_deref(), Some(NEW_CANARY));

    let audit = env.audit();
    let replaces = "anthropic https://api.anthropic.com:443".to_string();
    let removed = records_of(&audit, "account_removed", "cloud");
    assert_eq!(removed.len(), 1, "{audit}");
    assert!(
        has(&removed[0], "reason", "provider_change"),
        "{}",
        removed[0]
    );
    assert!(removed[0].contains("key_for"), "{}", removed[0]);
    assert!(removed[0].contains("router.example"), "{}", removed[0]);
    let added = records_of(&audit, "account_added", "cloud");
    assert_eq!(added.len(), 2, "{audit}");
    let new = &added[1];
    for (key, value) in [
        ("provider", "openai-compatible"),
        ("scheme", "https"),
        ("host", "router.example"),
        ("port", "443"),
        ("key", "stored"),
        ("confirmation", "typed_id_yes_new_key"),
        ("replaces", replaces.as_str()),
    ] {
        assert!(has(new, key, value), "{key}={value}: {new}");
    }
    assert!(
        seq_of(&removed[0]) < seq_of(new),
        "the removal commits first"
    );
    let entries = env.history();
    assert_eq!(entries.len(), history + 1);
    assert_eq!(entries.last().unwrap().kind, HistoryKind::Edit);
    let text = env.user_text().unwrap();
    assert!(text.contains("provider: openai-compatible"), "{text}");
    assert!(text.contains(PROVIDER_URL), "{text}");

    let sent = test_on_wire(&env, &keychain, "cloud", &["--model", "m"]).await;
    assert_eq!(sent.code, 0, "{}", sent.all);
    assert_eq!(sent.hops.len(), 1, "{:?}", sent.hosts());
    assert!(sent.hops[0].url.starts_with("https://router.example/"));
    assert!(sent.carried(NEW_CANARY), "the new key");
    assert!(!sent.carried(CANARY), "never the old key");
    assert_no_key_leaked(&env, &[&ran], &[CANARY, NEW_CANARY]);
}

/// Crash after remove: the write of the user file fails after
/// `account_removed` committed. Every key use is refused (no Keychain read,
/// nothing sent) and `list` says unconfirmed, naming `change`; running
/// `change` again (a new key typed again) completes it and `test` sends.
#[tokio::test]
async fn a_crash_after_the_removal_leaves_the_account_refused_until_changed_again() {
    let env = env();
    let keychain = FakeSecretStore::new();
    cloud(&env, &keychain).await;
    let file = env.user_text();
    let args = [
        "change",
        "cloud",
        "--provider",
        "openai-compatible",
        "--base-url",
        PROVIDER_URL,
    ];
    let app_data = env.app_data();
    am_app::runtime::testing::fail_writer_at(&app_data, Some(am_config::FailPoint::BeforeRename));
    let kit = Kit::typing(&keychain, &["cloud", YES, NEW_CANARY]);
    let ran = kit.run(&env, &args).await;
    am_app::runtime::testing::fail_writer_at(&app_data, None);

    assert_eq!(ran.code, 1, "{}", ran.all());
    assert_eq!(env.user_text(), file, "the file was not written");
    let audit = env.audit();
    assert_eq!(
        records_of(&audit, "account_removed", "cloud").len(),
        1,
        "{audit}"
    );
    assert_eq!(
        records_of(&audit, "account_added", "cloud").len(),
        1,
        "{audit}"
    );

    let sent = test_on_wire(&env, &keychain, "cloud", &[]).await;
    assert_eq!(sent.code, 1, "{}", sent.all);
    assert!(
        sent.all.contains("agentic accounts change cloud"),
        "{}",
        sent.all
    );
    assert_eq!(
        (sent.gets, sent.hops.len()),
        (0, 0),
        "no key read, nothing sent"
    );
    let line = list_line(&env, &keychain, "cloud").await;
    assert!(line.contains("unconfirmed"), "{line}");
    assert!(line.contains("agentic accounts change cloud"), "{line}");

    let again = Kit::typing(&keychain, &["cloud", YES, NEW_CANARY]);
    let ran = again.run(&env, &args).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(again.calls().iter().all(|c| !c.starts_with("get:")));
    let sent = test_on_wire(&env, &keychain, "cloud", &["--model", "m"]).await;
    assert_eq!(sent.code, 0, "{}", sent.all);
    assert!(sent.hops[0].url.starts_with("https://router.example/"));
    assert!(sent.carried(NEW_CANARY) && !sent.carried(CANARY));
}

/// Keychain fails: `set` fails after `delete`. The command is refused in
/// words naming the Keychain; `account_removed` stands and no new
/// `account_added` is written, so the Account is refused (nothing sent)
/// until `change` is run again.
#[tokio::test]
async fn a_failing_keychain_leaves_the_account_refused_until_changed_again() {
    let env = env();
    let keychain = FakeSecretStore::new();
    cloud(&env, &keychain).await;
    let file = env.user_text();
    let args = [
        "change",
        "cloud",
        "--provider",
        "openai-compatible",
        "--base-url",
        PROVIDER_URL,
    ];

    let kit = Kit::typing(&keychain, &["cloud", YES, NEW_CANARY]);
    kit.store.refuse_set.store(true, Ordering::SeqCst);
    let ran = kit.run(&env, &args).await;

    assert_eq!(ran.code, 1, "{}", ran.all());
    assert!(ran.err.contains("Keychain"), "worded: {}", ran.all());
    assert_eq!(
        kit.calls(),
        vec!["delete:cloud".to_string(), "set:cloud".to_string()]
    );
    assert_eq!(env.user_text(), file);
    let audit = env.audit();
    assert_eq!(records_of(&audit, "account_removed", "cloud").len(), 1);
    assert_eq!(records_of(&audit, "account_added", "cloud").len(), 1);
    let sent = test_on_wire(&env, &keychain, "cloud", &[]).await;
    assert_eq!(sent.code, 1, "{}", sent.all);
    assert_eq!((sent.gets, sent.hops.len()), (0, 0));
    let line = list_line(&env, &keychain, "cloud").await;
    assert!(line.contains("unconfirmed"), "{line}");

    let again = Kit::typing(&keychain, &["cloud", YES, NEW_CANARY]);
    let ran = again.run(&env, &args).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    let sent = test_on_wire(&env, &keychain, "cloud", &["--model", "m"]).await;
    assert_eq!(sent.code, 0, "{}", sent.all);
    assert!(sent.carried(NEW_CANARY));
    assert_no_key_leaked(&env, &[&ran], &[CANARY, NEW_CANARY]);
}

/// First confirmation: an Account `migrate` listed unconfirmed; `change
/// spare` with no flags takes the new-key path whatever flags are given:
/// `account_removed` (`reason: provider_change`), `delete` then `set`,
/// `account_added` (`typed_id_yes_new_key`, the file's values); `test` then
/// sends the new key. Before it, `test` is refused and `list` says
/// unconfirmed.
#[tokio::test]
async fn a_first_confirmation_takes_a_new_key() {
    let env = env();
    let keychain = FakeSecretStore::with("spare", CANARY);
    listed_unconfirmed(&env, "spare");
    let sent = test_on_wire(&env, &keychain, "spare", &[]).await;
    assert_eq!(sent.code, 1, "{}", sent.all);
    assert_eq!((sent.gets, sent.hops.len()), (0, 0));
    let line = list_line(&env, &keychain, "spare").await;
    assert!(line.contains("unconfirmed"), "{line}");
    let history = env.history().len();

    let kit = Kit::typing(&keychain, &["spare", YES, NEW_CANARY]);
    let ran = kit.run(&env, &["change", "spare"]).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    assert_eq!(
        kit.calls(),
        vec!["delete:spare".to_string(), "set:spare".to_string()]
    );
    assert_eq!(kit.key("spare").as_deref(), Some(NEW_CANARY));
    assert!(ran.shown.contains("api.anthropic.com"), "{}", ran.shown);
    let audit = env.audit();
    let removed = records_of(&audit, "account_removed", "spare");
    assert_eq!(removed.len(), 1, "{audit}");
    assert!(
        has(&removed[0], "reason", "provider_change"),
        "{}",
        removed[0]
    );
    let added = records_of(&audit, "account_added", "spare");
    assert_eq!(added.len(), 1, "{audit}");
    for (key, value) in [
        ("provider", "anthropic"),
        ("host", "api.anthropic.com"),
        ("key", "stored"),
        ("confirmation", "typed_id_yes_new_key"),
    ] {
        assert!(has(&added[0], key, value), "{key}={value}: {}", added[0]);
    }
    assert!(
        !added[0].contains("\"replaces\":\"") || has(&added[0], "replaces", "none"),
        "no ended confirmation record to replace: {}",
        added[0]
    );
    // The file already held the values confirmed (review fix G-record,
    // 2026-11-23): the confirmation commits with a History entry whose
    // before equals its after, in the same transaction.
    let entries = env.history();
    assert_eq!(entries.len(), history + 1);
    let last = entries.last().unwrap();
    assert_eq!(last.before, last.after, "the file is unchanged");
    assert_committed_with(&audit, &added[0], &last.id);

    let sent = test_on_wire(&env, &keychain, "spare", &[]).await;
    assert_eq!(sent.code, 0, "{}", sent.all);
    assert!(sent.hops[0].url.starts_with("https://api.anthropic.com/"));
    assert!(sent.carried(NEW_CANARY) && !sent.carried(CANARY));
    assert_no_key_leaked(&env, &[&ran], &[CANARY, NEW_CANARY]);
}

/// A first confirmation also covers an Account whose record a `remove`
/// ended while it is in the file again (here: written back by a plain app
/// write with History and no record, as a reconciled intent leaves it):
/// `change` with no flags confirms the file's values with a new key.
#[tokio::test]
async fn an_account_whose_record_a_remove_ended_is_confirmed_by_change() {
    let env = env();
    let keychain = FakeSecretStore::new();
    let _p = env.naming("p", "local-ollama");
    cloud(&env, &keychain).await;
    let kit = Kit::new(FakeTerminal::new(), &keychain);
    let ran = kit.run(&env, &["remove", "cloud"]).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    {
        let writer = env.user_writer();
        let now = am_config::accounts_file(env.places.user.dir());
        writer
            .apply_with_audit(
                owner(),
                am_config::add_account_edit(
                    &now,
                    "cloud",
                    "anthropic",
                    None,
                    None,
                    am_config::KeyState::Stored,
                ),
                now.sha256.as_deref().unwrap(),
                Vec::new(),
            )
            .unwrap();
    }
    let sent = test_on_wire(&env, &keychain, "cloud", &[]).await;
    assert_eq!(sent.code, 1, "{}", sent.all);
    assert!(
        sent.all.contains("agentic accounts change cloud"),
        "{}",
        sent.all
    );

    let kit = Kit::typing(&keychain, &["cloud", YES, NEW_CANARY]);
    let ran = kit.run(&env, &["change", "cloud"]).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert_eq!(
        kit.calls(),
        vec!["delete:cloud".to_string(), "set:cloud".to_string()]
    );
    let removed = records_of(&env.audit(), "account_removed", "cloud");
    assert_eq!(removed.len(), 2);
    assert!(
        has(&removed[1], "reason", "provider_change"),
        "{}",
        removed[1]
    );
    let sent = test_on_wire(&env, &keychain, "cloud", &[]).await;
    assert_eq!(sent.code, 0, "{}", sent.all);
    assert!(sent.carried(NEW_CANARY) && !sent.carried(CANARY));
}

/// Empty key: an empty answer at the key prompt writes nothing and makes no
/// Keychain call.
#[tokio::test]
async fn an_empty_key_writes_nothing() {
    let env = env();
    let keychain = FakeSecretStore::new();
    cloud(&env, &keychain).await;
    let before = env.state();

    for empty in ["", "   "] {
        let kit = Kit::typing(&keychain, &["cloud", YES, empty]);
        let ran = kit
            .run(&env, &["change", "cloud", "--provider", "openai"])
            .await;
        assert_eq!(ran.code, 1, "{empty:?}: {}", ran.all());
        assert_eq!(kit.term.reads(), 3, "{empty:?}: the key prompt was reached");
        assert!(kit.calls().is_empty(), "{empty:?}: {:?}", kit.calls());
        assert_eq!(env.state(), before, "{empty:?}: nothing written");
        assert_eq!(kit.key("cloud").as_deref(), Some(CANARY));
    }
}

/// Echo: Ctrl-C at the hidden key prompt ends the command (130) with echo
/// restored; nothing is written and no Keychain call is made.
#[tokio::test]
async fn ctrl_c_at_the_key_prompt_restores_echo_and_writes_nothing() {
    let env = env();
    let keychain = FakeSecretStore::new();
    cloud(&env, &keychain).await;
    let before = env.state();

    let kit = Kit::typing(&keychain, &["cloud", YES]);
    kit.term.interrupt();
    let ran = kit
        .run(&env, &["change", "cloud", "--provider", "openai"])
        .await;

    assert_eq!(ran.code, 130, "{}", ran.all());
    assert_eq!(kit.term.echo_reads(), vec![true, true, false]);
    assert!(kit.term.echo(), "echo restored");
    assert!(kit.calls().is_empty(), "{:?}", kit.calls());
    assert_eq!(env.state(), before);
}

/// Loopback to remote while named: a loopback Account named by a reachable
/// project; every kind of change to a remote endpoint (an address change,
/// a provider change) is refused before epic 4 story 18, naming the
/// project; nothing written, no Keychain call.
#[tokio::test]
async fn loopback_to_remote_is_refused_while_a_reachable_project_names_it() {
    let env = env();
    let keychain = FakeSecretStore::new();
    let here = env.naming("here", "llama");
    add(
        &env,
        &keychain,
        "llama",
        "openai-compatible",
        Some(LOOPBACK_URL),
        ROUTER_CANARY,
    )
    .await;
    let before = env.state();

    for args in [
        vec!["change", "llama", "--base-url", "https://remote.example/v1"],
        vec!["change", "llama", "--provider", "openai"],
    ] {
        let kit = Kit::typing(&keychain, &["llama", YES, NEW_CANARY]);
        let ran = kit.run(&env, &args).await;
        assert_eq!(ran.code, 1, "{args:?}: {}", ran.all());
        assert!(
            ran.err.contains(&here.display().to_string()),
            "{args:?}: names the project: {}",
            ran.all()
        );
        assert!(kit.calls().is_empty(), "{args:?}: {:?}", kit.calls());
        assert_eq!(env.state(), before, "{args:?}: nothing written");
    }
}

/// Other host while named: a remote Account named by a reachable project;
/// a change to another host is refused naming the project; the same host
/// with another path is allowed.
#[tokio::test]
async fn another_host_is_refused_while_named_but_another_path_is_not() {
    let env = env();
    let keychain = FakeSecretStore::new();
    let here = env.naming("here", "router");
    router(&env, &keychain).await;
    let before = env.state();

    let kit = Kit::typing(&keychain, &["router", YES]);
    let ran = kit
        .run(&env, &["change", "router", "--base-url", NEW_REMOTE_URL])
        .await;
    assert_eq!(ran.code, 1, "{}", ran.all());
    assert!(
        ran.err.contains(&here.display().to_string()),
        "names the project: {}",
        ran.all()
    );
    assert_eq!(env.state(), before);

    let kit = Kit::typing(&keychain, &["router", YES]);
    let ran = kit
        .run(
            &env,
            &[
                "change",
                "router",
                "--base-url",
                "https://openrouter.example/v2",
            ],
        )
        .await;
    assert_eq!(ran.code, 0, "the same host: {}", ran.all());
    assert!(kit.calls().is_empty());
    assert_eq!(
        records_of(&env.audit(), "account_changed", "router").len(),
        1
    );
    assert!(
        env.user_text()
            .unwrap()
            .contains("https://openrouter.example/v2")
    );
    let sent = test_on_wire(&env, &keychain, "router", &["--model", "m"]).await;
    assert_eq!(sent.code, 0, "{}", sent.all);
    assert!(
        sent.hops[0]
            .url
            .starts_with("https://openrouter.example/v2/")
    );
}

/// Unreachable only: the only other project in the root index cannot be
/// reached; it is listed as not checked and the change proceeds (no
/// `--force` exists).
#[tokio::test]
async fn an_unreachable_project_is_listed_and_never_refuses() {
    let env = env();
    let keychain = FakeSecretStore::new();
    let gone = env.unreachable("gone", "router");
    router(&env, &keychain).await;

    let kit = Kit::typing(&keychain, &["router", YES]);
    let ran = kit
        .run(&env, &["change", "router", "--base-url", NEW_REMOTE_URL])
        .await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(ran.shown.contains(&gone), "{}", ran.shown);
    assert!(ran.shown.contains("not checked"), "{}", ran.shown);
    assert_eq!(
        records_of(&env.audit(), "account_changed", "router").len(),
        1
    );

    let kit = Kit::typing(&keychain, &["router", YES]);
    let ran = kit
        .run(
            &env,
            &["change", "router", "--base-url", REMOTE_URL, "--force"],
        )
        .await;
    assert_eq!(ran.code, 2, "there is no --force: {}", ran.all());
}

/// Hand edit: the provider or `base_url` edited by hand, no record: `test`
/// is refused before any Keychain read; `change` is refused as an outside
/// change naming the file, before any prompt and with no Keychain call.
#[tokio::test]
async fn a_hand_edit_is_refused_by_test_and_by_change() {
    let env = env();
    let keychain = FakeSecretStore::new();
    router(&env, &keychain).await;
    let written = env.user_text().unwrap();

    for edited in [
        written.replace(REMOTE_URL, NEW_REMOTE_URL),
        written
            .replace("provider: openai-compatible", "provider: openai")
            .replace(&format!("    base_url: {REMOTE_URL}\n"), ""),
    ] {
        assert_ne!(edited, written);
        std::fs::write(env.accounts_path(), &edited).unwrap();
        let history = env.history().len();

        let sent = test_on_wire(&env, &keychain, "router", &["--model", "m"]).await;
        assert_eq!(sent.code, 1, "{}", sent.all);
        assert_eq!((sent.gets, sent.hops.len()), (0, 0));

        let kit = Kit::typing(&keychain, &["router", YES, NEW_CANARY]);
        let ran = kit
            .run(&env, &["change", "router", "--base-url", NEW_REMOTE_URL])
            .await;
        assert_eq!(ran.code, 1, "{}", ran.all());
        assert!(ran.err.contains(USER_FILE), "names the file: {}", ran.all());
        assert_eq!(kit.term.reads(), 0, "refused before any prompt");
        assert!(kit.calls().is_empty());
        assert_eq!(env.history().len(), history);
        assert_eq!(env.user_text().unwrap(), edited);
        std::fs::write(env.accounts_path(), &written).unwrap();
    }
}

/// Undo: Undo of a provider change's History entry is refused by the
/// writer, naming `accounts change` (the old key is gone), and writes
/// nothing; Undo of a name edit still applies.
#[tokio::test]
async fn undo_of_a_provider_change_is_refused_and_of_a_name_edit_applies() {
    let env = env();
    let keychain = FakeSecretStore::new();
    cloud(&env, &keychain).await;
    let kit = Kit::typing(&keychain, &["cloud", YES, NEW_CANARY]);
    let ran = kit
        .run(
            &env,
            &[
                "change",
                "cloud",
                "--provider",
                "openai-compatible",
                "--base-url",
                PROVIDER_URL,
            ],
        )
        .await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    let change = env.history().pop().unwrap();
    let text = env.user_text();

    let err = env.user_writer().undo(owner(), &change.id).unwrap_err();
    assert!(err.to_string().contains("accounts change"), "{err}");
    assert_eq!(env.user_text(), text, "nothing written");
    let err = env
        .user_writer()
        .restore(owner(), &env.history()[0].id)
        .unwrap_err();
    assert!(err.to_string().contains("accounts change"), "{err}");
    assert_eq!(env.user_text(), text);

    let named = env
        .user_writer()
        .apply(
            owner(),
            am_config::ConfigEdit {
                file: USER_FILE.into(),
                ops: vec![am_config::EditOp::Set {
                    path: vec!["accounts".into(), "0".into(), "name".into()],
                    value: "Renamed".into(),
                }],
                body: None,
            },
        )
        .unwrap();
    let undone = env.user_writer().undo(owner(), &named.id).unwrap();
    assert_eq!(undone.kind, HistoryKind::Undo);
    assert_eq!(env.user_text(), text);
}

/// Changed since the sentence: while `change` waits at the id prompt,
/// another process edits the user file through the app. The fresh re-read
/// under `user.lock` refuses the command, to be run again; nothing of it is
/// written and no Keychain call is made.
#[tokio::test]
async fn a_file_changed_while_the_prompt_waits_refuses_the_change() {
    let env = env();
    let keychain = FakeSecretStore::new();
    cloud(&env, &keychain).await;
    let places = env.places.clone();
    let kit = Kit::new(FakeTerminal::new(), &keychain);
    kit.term.line_and("cloud", move || {
        let guard = UserStore::locked(&places.user, Duration::from_secs(10)).unwrap();
        let writer =
            am_config::ConfigWriter::for_user_accounts(places.user.dir(), Arc::new(guard)).unwrap();
        writer
            .apply(
                owner(),
                am_config::ConfigEdit {
                    file: USER_FILE.into(),
                    ops: vec![am_config::EditOp::Set {
                        path: vec!["accounts".into(), "0".into(), "name".into()],
                        value: "Meanwhile".into(),
                    }],
                    body: None,
                },
            )
            .unwrap();
    });
    kit.term.line(YES);
    kit.term.line(NEW_CANARY);
    let audit = env.audit();

    let ran = kit
        .run(&env, &["change", "cloud", "--provider", "openai"])
        .await;

    assert_eq!(ran.code, 1, "{}", ran.all());
    assert!(ran.err.contains("again"), "run it again: {}", ran.all());
    assert!(kit.calls().is_empty(), "{:?}", kit.calls());
    // The test's own edit at the prompt writes its History audit rows; the
    // command itself adds no confirmation record of any kind for `cloud`.
    let after = env.audit();
    for kind in ["account_removed", "account_added", "account_changed"] {
        assert_eq!(
            records_of(&after, kind, "cloud"),
            records_of(&audit, kind, "cloud"),
            "no new {kind} record"
        );
    }
    assert_eq!(kit.key("cloud").as_deref(), Some(CANARY));
}

/// Validation: the new provider and `base_url` pass 2.3's and 2.12's rules;
/// a refused value is refused in words (exit 1, not a usage error), never
/// echoed, with nothing written and no Keychain call.
#[tokio::test]
async fn a_refused_provider_or_address_is_never_echoed() {
    let env = env();
    let keychain = FakeSecretStore::new();
    router(&env, &keychain).await;
    let before = env.state();
    let pasted = "sk-or-pasted-canary-change-9q9q9q9q";
    let with_key = format!("https://user:{pasted}@router2.example/v1");

    for args in [
        vec![
            "change",
            "router",
            "--base-url",
            "http://router2.example/v1",
        ],
        vec!["change", "router", "--base-url", with_key.as_str()],
        vec!["change", "router", "--provider", "brave"],
        vec![
            "change",
            "router",
            "--provider",
            "anthropic",
            "--base-url",
            NEW_REMOTE_URL,
        ],
    ] {
        let kit = Kit::typing(&keychain, &["router", YES, NEW_CANARY]);
        let ran = kit.run(&env, &args).await;
        assert_eq!(ran.code, 1, "{args:?}: {}", ran.all());
        assert!(!ran.all().contains(pasted), "{args:?}: {}", ran.all());
        assert!(kit.calls().is_empty(), "{args:?}");
        assert_eq!(env.state(), before, "{args:?}");
    }
}

/// Usage: `change` is in the usage text, takes one id, and no `--force`.
#[tokio::test]
async fn change_is_in_the_usage_and_takes_no_force() {
    let env = env();
    let keychain = FakeSecretStore::new();
    let kit = Kit::new(FakeTerminal::new(), &keychain);
    let ran = kit.run(&env, &["nonsense"]).await;
    assert_eq!(ran.code, 2);
    assert!(
        ran.err.contains("agentic accounts change <id>"),
        "{}",
        ran.err
    );
    for args in [
        vec!["change"],
        vec!["change", "a", "b"],
        vec!["change", "a", "--force"],
        vec!["change", "a", "--project", "/tmp"],
    ] {
        let ran = kit.run(&env, &args).await;
        assert_eq!(ran.code, 2, "{args:?}: {}", ran.all());
    }
    assert!(kit.calls().is_empty());
}

/// Loopback-HTTP MCP: a reachable project enables a `transport: http` MCP
/// server on this Mac (`http://127.0.0.1:<port>`, `https://localhost`,
/// `https://[::1]`): `add`, `remove` and `change` are each refused before
/// any prompt, naming the project and the server, with nothing written and
/// no Keychain call; `test` still runs.
#[tokio::test]
async fn a_loopback_http_mcp_server_refuses_the_account_commands() {
    for url in [
        "http://127.0.0.1:3001/mcp",
        "https://localhost/mcp",
        "https://[::1]:8443/mcp",
    ] {
        let env = env();
        let keychain = FakeSecretStore::new();
        cloud(&env, &keychain).await;
        let child = env.project(
            "child",
            &[
                ("company.yaml", COMPANY.to_string()),
                ("tools.yaml", http_tools("loop-notes", url)),
            ],
        );
        let before = env.state();
        for args in [
            vec!["add", "cloud2", "--provider", "anthropic"],
            vec!["remove", "cloud"],
            vec!["change", "cloud", "--provider", "openai"],
        ] {
            let kit = Kit::typing(&keychain, &[args[1], YES, NEW_CANARY]);
            let ran = kit.run(&env, &args).await;
            assert_eq!(ran.code, 1, "{url} {args:?}: {}", ran.all());
            assert!(
                ran.err.contains(&child.display().to_string()),
                "{url} {args:?}: names the project: {}",
                ran.all()
            );
            assert!(
                ran.err.contains("loop-notes"),
                "{url} {args:?}: names the server: {}",
                ran.all()
            );
            assert_eq!(kit.term.reads(), 0, "{url} {args:?}");
            assert!(kit.calls().is_empty(), "{url} {args:?}");
            assert_eq!(env.state(), before, "{url} {args:?}");
        }
        let sent = test_on_wire(&env, &keychain, "cloud", &[]).await;
        assert_eq!(sent.code, 0, "{url}: test still runs: {}", sent.all);
    }
}

/// Remote HTTP MCP: a `transport: http` server at `https://mcp.example`
/// counts for none of the Account commands.
#[tokio::test]
async fn a_remote_http_mcp_server_refuses_no_account_command() {
    let env = env();
    let keychain = FakeSecretStore::new();
    let _remote = env.project(
        "remote",
        &[
            ("company.yaml", COMPANY.to_string()),
            (
                "tools.yaml",
                http_tools("far-notes", "https://mcp.example/mcp"),
            ),
        ],
    );
    cloud(&env, &keychain).await;
    add(&env, &keychain, "cloud2", "anthropic", None, ROUTER_CANARY).await;

    let kit = Kit::typing(&keychain, &["cloud", YES, NEW_CANARY]);
    let ran = kit
        .run(
            &env,
            &[
                "change",
                "cloud",
                "--provider",
                "openai-compatible",
                "--base-url",
                PROVIDER_URL,
            ],
        )
        .await;
    assert_eq!(ran.code, 0, "{}", ran.all());

    let kit = Kit::new(FakeTerminal::new(), &keychain);
    let ran = kit.run(&env, &["remove", "cloud2"]).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
}

/// Not interactive: a pipe on stdin is refused in words naming the
/// terminal rule before anything is read or written, with no Keychain
/// call.
#[tokio::test]
async fn change_from_a_pipe_is_refused_before_anything_is_read() {
    let env = env();
    let keychain = FakeSecretStore::new();
    cloud(&env, &keychain).await;
    let before = env.state();

    let kit = Kit::new(FakeTerminal::piped(), &keychain);
    kit.term.line("cloud");
    kit.term.line(YES);
    kit.term.line(NEW_CANARY);
    let ran = kit
        .run(&env, &["change", "cloud", "--provider", "openai"])
        .await;

    assert_eq!(ran.code, 1, "{}", ran.all());
    assert!(ran.err.contains("interactive terminal"), "{}", ran.all());
    assert!(ran.err.contains("accounts change"), "{}", ran.all());
    assert_eq!(kit.term.reads(), 0);
    assert!(kit.calls().is_empty());
    assert_eq!(env.state(), before);
}

/// A reconciled write whose record never committed: the file write of a
/// provider change completes but its History commit does not (the
/// `AfterRename` fail point); the next lock records the History entry, and
/// the Account has no live record. Running `change` again with the same
/// values stores the new key and writes only the records, no History
/// entry.
#[tokio::test]
async fn a_reconciled_write_whose_record_never_committed_gets_only_the_record() {
    let env = env();
    let keychain = FakeSecretStore::new();
    cloud(&env, &keychain).await;
    let args = [
        "change",
        "cloud",
        "--provider",
        "openai-compatible",
        "--base-url",
        PROVIDER_URL,
    ];
    let app_data = env.app_data();
    am_app::runtime::testing::fail_writer_at(&app_data, Some(am_config::FailPoint::AfterRename));
    let kit = Kit::typing(&keychain, &["cloud", YES, NEW_CANARY]);
    let ran = kit.run(&env, &args).await;
    am_app::runtime::testing::fail_writer_at(&app_data, None);
    assert_eq!(ran.code, 1, "{}", ran.all());
    assert!(
        env.user_text().unwrap().contains(PROVIDER_URL),
        "the file was written"
    );

    let history = env.history().len();

    let again = Kit::typing(&keychain, &["cloud", YES, NEW_CANARY]);
    let ran = again.run(&env, &args).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert_eq!(again.key("cloud").as_deref(), Some(NEW_CANARY));
    let added = records_of(&env.audit(), "account_added", "cloud");
    assert!(
        has(added.last().unwrap(), "host", "router.example"),
        "{added:?}"
    );
    // Review fix G-record (2026-11-23): the reconciled entry, then the
    // record-only confirmation's own History entry (before equals after),
    // committed with the `account_added` in one transaction.
    let entries = env.history();
    assert_eq!(
        entries.len(),
        history + 2,
        "the reconciled entry, then the confirmation's entry"
    );
    let last = entries.last().unwrap();
    assert_eq!(
        last.after,
        env.user_text().unwrap().as_bytes(),
        "the file the first run wrote"
    );
    assert_eq!(last.before, last.after);
    assert_committed_with(&env.audit(), added.last().unwrap(), &last.id);
    let sent = test_on_wire(&env, &keychain, "cloud", &["--model", "m"]).await;
    assert_eq!(sent.code, 0, "{}", sent.all);
    assert!(sent.carried(NEW_CANARY));
}

// ============================================ review fixes (2026-11-23)

const PROVIDER_FLAGS: &str = "--provider openai-compatible --base-url https://router.example/v1";

/// The provider change of `cloud` to openai-compatible at router.example.
const TO_ROUTER: [&str; 6] = [
    "change",
    "cloud",
    "--provider",
    "openai-compatible",
    "--base-url",
    PROVIDER_URL,
];

/// G-record (Quick 1, AD-15 *Record*): after a provider change the user
/// store's `account_added` for the id was committed in the same transaction
/// as the file's History entry: its head is that entry's History commit,
/// at the same time, and no `config.recorded` head comes before it.
#[tokio::test]
async fn a_provider_changes_confirmation_commits_with_its_history_entry() {
    let env = env();
    let keychain = FakeSecretStore::new();
    cloud(&env, &keychain).await;

    let kit = Kit::typing(&keychain, &["cloud", YES, NEW_CANARY]);
    let ran = kit.run(&env, &TO_ROUTER).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    let audit = env.audit();
    let added = records_of(&audit, "account_added", "cloud");
    assert_eq!(added.len(), 2, "{audit}");
    let last = env.history().pop().unwrap();
    assert_eq!(last.kind, HistoryKind::Edit);
    assert_committed_with(&audit, &added[1], &last.id);
}

/// A note of `kind` for `id` (anthropic at api.anthropic.com).
fn note_of(kind: &str, id: &str) -> AuditNote {
    AuditNote {
        action: if kind == "account_removed" {
            "config.narrowed".into()
        } else {
            "config.widened".into()
        },
        target: id.into(),
        args: [
            ("kind", kind),
            ("file", USER_FILE),
            ("account", id),
            ("provider", "anthropic"),
            ("scheme", "https"),
            ("host", "api.anthropic.com"),
            ("port", "443"),
            ("new_provider", "anthropic"),
            ("new_scheme", "https"),
            ("new_host", "api.anthropic.com"),
            ("new_port", "443"),
            ("references", ""),
            ("key", "stored"),
            ("confirmation", "typed_id_yes_new_key"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect(),
    }
}

/// G-record (Adversarial 2): `UserStore::record_config_notes` writes only
/// non-granting kinds. An `account_added` or `account_changed` note is
/// refused and nothing is written; `account_removed` and `account_key_set`
/// are accepted.
#[tokio::test]
async fn record_config_notes_refuses_the_granting_kinds() {
    let env = env();
    let keychain = FakeSecretStore::new();
    cloud(&env, &keychain).await;
    let after = env.history().pop().unwrap().after_sha256;
    let store = UserStore::locked(&env.places.user, Duration::from_secs(10)).unwrap();

    for kind in ["account_added", "account_changed"] {
        let before = env.audit();
        let result =
            store.record_config_notes(&owner(), USER_FILE, &after, &[note_of(kind, "cloud")]);
        assert!(result.is_err(), "{kind} is refused");
        assert_eq!(env.audit(), before, "{kind}: nothing written");
    }
    for kind in ["account_removed", "account_key_set"] {
        store
            .record_config_notes(&owner(), USER_FILE, &after, &[note_of(kind, "cloud")])
            .unwrap_or_else(|e| panic!("{kind} is accepted: {e}"));
        assert_eq!(records_of(&env.audit(), kind, "cloud").len(), 1, "{kind}");
    }
}

/// G-record (Quick 1): a `config.recorded` head carrying an
/// `account_added` is not read as a confirmation by the Key-use check:
/// `spare`, listed unconfirmed, stays refused before any Keychain read
/// whether or not the store took the forged record.
#[tokio::test]
async fn a_recorded_account_added_is_never_a_confirmation() {
    let env = env();
    let keychain = FakeSecretStore::with("spare", CANARY);
    listed_unconfirmed(&env, "spare");
    let after = env.history().pop().unwrap().after_sha256;
    {
        let store = UserStore::locked(&env.places.user, Duration::from_secs(10)).unwrap();
        let _ = store.record_config_notes(
            &owner(),
            USER_FILE,
            &after,
            &[note_of("account_added", "spare")],
        );
    }

    let sent = test_on_wire(&env, &keychain, "spare", &[]).await;
    assert_eq!(sent.code, 1, "{}", sent.all);
    assert_eq!(
        (sent.gets, sent.hops.len()),
        (0, 0),
        "no key read, nothing sent"
    );
}

/// Adversarial 1: a provider change cloud P (anthropic) to Q
/// (openai-compatible at router.example) replaces the Keychain entry, then
/// the file write fails before the rename. The refusal, `list` and `test`
/// name the exact flags (`--provider Q --base-url R`); `change cloud` with
/// no flags then defaults to Q at R, its sentence says the entry already
/// holds a key typed for Q, and it never confirms P.
#[tokio::test]
async fn an_interrupted_provider_change_finishes_at_its_new_destination() {
    let env = env();
    let keychain = FakeSecretStore::new();
    cloud(&env, &keychain).await;
    let app_data = env.app_data();
    am_app::runtime::testing::fail_writer_at(&app_data, Some(am_config::FailPoint::BeforeRename));
    let kit = Kit::typing(&keychain, &["cloud", YES, NEW_CANARY]);
    let ran = kit.run(&env, &TO_ROUTER).await;
    am_app::runtime::testing::fail_writer_at(&app_data, None);

    assert_eq!(ran.code, 1, "{}", ran.all());
    assert!(
        ran.err.contains(PROVIDER_FLAGS),
        "the exact flags: {}",
        ran.err
    );
    // Quick 6 (as far as it is reachable here): the key was replaced, and
    // the words say so.
    assert!(!ran.err.contains("no key was stored"), "{}", ran.err);
    assert_eq!(kit.key("cloud").as_deref(), Some(NEW_CANARY));
    let line = list_line(&env, &keychain, "cloud").await;
    assert!(line.contains(PROVIDER_FLAGS), "{line}");
    let sent = test_on_wire(&env, &keychain, "cloud", &[]).await;
    assert_eq!(sent.code, 1, "{}", sent.all);
    assert!(sent.all.contains(PROVIDER_FLAGS), "{}", sent.all);
    assert_eq!((sent.gets, sent.hops.len()), (0, 0));

    let again = Kit::typing(&keychain, &["cloud", YES, NEW_CANARY]);
    let ran = again.run(&env, &["change", "cloud"]).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    let shown = ran.shown.to_lowercase();
    assert!(
        shown.contains("router.example"),
        "defaults to R: {}",
        ran.shown
    );
    assert!(
        shown.contains("already holds a key") && shown.contains("openai-compatible"),
        "says the entry holds a key typed for Q: {}",
        ran.shown
    );
    assert!(again.calls().iter().all(|c| !c.starts_with("get:")));
    let added = records_of(&env.audit(), "account_added", "cloud");
    let latest = added.last().unwrap();
    assert!(has(latest, "provider", "openai-compatible"), "{latest}");
    assert!(has(latest, "host", "router.example"), "{latest}");
    assert_eq!(
        added
            .iter()
            .filter(|l| has(l, "provider", "anthropic"))
            .count(),
        1,
        "P is never confirmed again: {added:?}"
    );
    assert!(env.user_text().unwrap().contains(PROVIDER_URL));
    let sent = test_on_wire(&env, &keychain, "cloud", &["--model", "m"]).await;
    assert_eq!(sent.code, 0, "{}", sent.all);
    assert!(sent.hops[0].url.starts_with("https://router.example/"));
    assert!(sent.carried(NEW_CANARY) && !sent.carried(CANARY));
}

/// G-keyfor (Quick 2, Adversarial 4): the Keychain `delete` fails during a
/// provider change, so the entry still holds P's key. After a `remove`, an
/// `add` of the id says the held key was last confirmed for P (anthropic at
/// api.anthropic.com), not for Q (router.example).
#[tokio::test]
async fn a_failed_delete_leaves_the_held_key_named_for_its_old_destination() {
    let env = env();
    let keychain = FakeSecretStore::new();
    cloud(&env, &keychain).await;
    let kit = Kit::typing(&keychain, &["cloud", YES, NEW_CANARY]);
    kit.store.refuse_delete.store(true, Ordering::SeqCst);
    let ran = kit.run(&env, &TO_ROUTER).await;
    assert_eq!(ran.code, 1, "{}", ran.all());
    assert_eq!(
        kit.key("cloud").as_deref(),
        Some(CANARY),
        "the old key stays"
    );

    let kit = Kit::new(FakeTerminal::new(), &keychain);
    let ran = kit.run(&env, &["remove", "cloud"]).await;
    assert_eq!(ran.code, 0, "{}", ran.all());

    let kit = Kit::typing(&keychain, &["cloud", CANARY]);
    let ran = kit
        .run(&env, &["add", "cloud", "--provider", "anthropic"])
        .await;
    assert!(
        ran.shown.contains("last confirmed for provider anthropic"),
        "{}",
        ran.shown
    );
    assert!(!ran.shown.contains("router.example"), "{}", ran.shown);
}

/// Adversarial 5: an http MCP server reaching this Mac by any form the
/// loader accepts (unspecified addresses, IPv4-mapped, another 127/8
/// address, `localhost.` and `*.localhost`) makes `add` refuse, naming the
/// server, with nothing written.
#[tokio::test]
async fn every_form_of_a_loopback_http_mcp_server_refuses_add() {
    for url in [
        "https://0.0.0.0/mcp",
        "https://[::]/mcp",
        "https://[::ffff:127.0.0.1]/mcp",
        "https://127.0.0.2/mcp",
        "https://localhost./mcp",
        "https://x.localhost/mcp",
    ] {
        let env = env();
        let keychain = FakeSecretStore::new();
        let child = env.project(
            "child",
            &[
                ("company.yaml", COMPANY.to_string()),
                ("tools.yaml", http_tools("loop-notes", url)),
            ],
        );
        let kit = Kit::typing(&keychain, &["cloud", CANARY]);
        let ran = kit
            .run(&env, &["add", "cloud", "--provider", "anthropic"])
            .await;
        assert_eq!(ran.code, 1, "{url}: {}", ran.all());
        assert!(ran.err.contains("loop-notes"), "{url}: {}", ran.all());
        assert!(
            ran.err.contains(&child.display().to_string()),
            "{url}: {}",
            ran.all()
        );
        assert_eq!(kit.term.reads(), 0, "{url}");
        assert!(env.user_text().is_none(), "{url}: nothing written");
    }
}

/// Adversarial 6: an `agentic serve` record for the new loopback port
/// appears while the prompt waits; the change is refused under the lock,
/// naming the port, with nothing written and no Keychain call.
#[tokio::test]
async fn a_server_on_the_new_port_since_the_sentence_refuses_the_change() {
    let env = env();
    let keychain = FakeSecretStore::new();
    add(
        &env,
        &keychain,
        "router",
        "openai-compatible",
        Some(LOOPBACK_URL),
        ROUTER_CANARY,
    )
    .await;
    let before = env.state();
    let serving = env.places.user.serving_dir();
    let kit = Kit::new(FakeTerminal::new(), &keychain);
    kit.term.line_and("router", move || {
        std::fs::create_dir_all(&serving).unwrap();
        std::fs::write(serving.join(std::process::id().to_string()), "9090\n").unwrap();
    });
    kit.term.line(YES);

    let ran = kit
        .run(
            &env,
            &["change", "router", "--base-url", LOOPBACK_OTHER_PORT],
        )
        .await;

    assert_eq!(ran.code, 1, "{}", ran.all());
    assert!(ran.err.contains("9090"), "names the port: {}", ran.all());
    assert!(kit.calls().is_empty(), "{:?}", kit.calls());
    assert_eq!(env.state(), before, "nothing written");
}

// ======================================= follow-up review fixes (2026-11-23)

/// A key pasted into the path of a `--base-url` (a canary).
const PATH_CANARY: &str = "sk-ant-api03-pathcanary-0123456789abcdef";

/// Follow-up 1: `change cloud --provider openai-compatible --base-url
/// https://router.example/<key>/v1` with the id, `yes` and a new key is
/// refused before any record or Keychain call: no `account_removed`, no
/// `account_key_set`, the Keychain untouched. The pasted key is in no
/// output (the sentence included), no file, no audit entry, and not in
/// `list` or `test` afterwards; the old confirmation still governs.
#[tokio::test]
async fn a_key_pasted_into_the_base_url_is_refused_before_anything_is_recorded() {
    let env = env();
    let keychain = FakeSecretStore::new();
    cloud(&env, &keychain).await;
    let before = env.state();
    let url = format!("https://router.example/{PATH_CANARY}/v1");

    let kit = Kit::typing(&keychain, &["cloud", YES, NEW_CANARY]);
    let ran = kit
        .run(
            &env,
            &[
                "change",
                "cloud",
                "--provider",
                "openai-compatible",
                "--base-url",
                url.as_str(),
            ],
        )
        .await;

    assert_eq!(
        ran.code,
        1,
        "{}",
        ran.all().replace(PATH_CANARY, "<canary>")
    );
    assert!(
        kit.calls().is_empty(),
        "no Keychain call: {:?}",
        kit.calls()
    );
    assert_eq!(kit.key("cloud").as_deref(), Some(CANARY));
    let audit = env.audit();
    assert_eq!(records_of(&audit, "account_removed", "cloud").len(), 0);
    assert_eq!(records_of(&audit, "account_key_set", "cloud").len(), 0);
    assert_eq!(env.state(), before, "nothing written");

    let line = list_line(&env, &keychain, "cloud").await;
    assert!(!line.contains(PATH_CANARY), "list: {line}");
    let sent = test_on_wire(&env, &keychain, "cloud", &[]).await;
    assert!(!sent.all.contains(PATH_CANARY), "test: {}", sent.all);
    assert_eq!(sent.code, 0, "the old confirmation governs: {}", sent.all);
    assert_no_key_leaked(&env, &[&ran], &[PATH_CANARY, NEW_CANARY]);
}

/// Follow-up 2. The race the review names (a removal committed between the
/// two pre-prompt reads of the live record and the latest sequence) needs a
/// hook between those reads, and none exists outside `src/`. This test
/// covers the nearest case it can build: while the id prompt of an address
/// change waits, another process commits a provider-change
/// `account_removed` for the id (no `account_key_set`). Under the lock the
/// change is refused, and no `account_changed` is written; the Account
/// stays with no live record (no key used).
#[tokio::test]
async fn an_address_change_is_refused_when_a_removal_commits_while_it_waits() {
    let env = env();
    let keychain = FakeSecretStore::new();
    router(&env, &keychain).await;
    let places = env.places.clone();
    let kit = Kit::new(FakeTerminal::new(), &keychain);
    kit.term.line_and("router", move || {
        let after = UserReader::open(&places.user)
            .unwrap()
            .config_history(None)
            .unwrap()
            .pop()
            .unwrap()
            .after_sha256;
        let store = UserStore::locked(&places.user, Duration::from_secs(10)).unwrap();
        let removed = AuditNote {
            action: "config.narrowed".into(),
            target: "router".into(),
            args: [
                ("kind", "account_removed"),
                ("file", USER_FILE),
                ("account", "router"),
                ("reason", "provider_change"),
                ("key_for_provider", "openai"),
                ("key_for_scheme", "https"),
                ("key_for_host", "api.openai.com"),
                ("key_for_port", "443"),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        };
        store
            .record_config_notes(&owner(), USER_FILE, &after, &[removed])
            .unwrap();
    });
    kit.term.line(YES);

    let ran = kit
        .run(&env, &["change", "router", "--base-url", NEW_REMOTE_URL])
        .await;

    assert_eq!(ran.code, 1, "{}", ran.all());
    assert!(ran.err.contains("again"), "{}", ran.all());
    assert!(kit.calls().is_empty(), "{:?}", kit.calls());
    let audit = env.audit();
    assert!(
        records_of(&audit, "account_changed", "router").is_empty(),
        "{audit}"
    );
    assert!(
        env.user_text().unwrap().contains(REMOTE_URL),
        "the file is unchanged"
    );
    let sent = test_on_wire(&env, &keychain, "router", &["--model", "m"]).await;
    assert_eq!(sent.code, 1, "{}", sent.all);
    assert_eq!((sent.gets, sent.hops.len()), (0, 0));
}
