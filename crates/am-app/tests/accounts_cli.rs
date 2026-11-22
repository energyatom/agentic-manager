//! Story 2.1: `agentic accounts add|list|test|remove`, the interim CLI
//! Account edit (AD-15), run in process through `am_app::cli::accounts`
//! with am-testkit's `FakeTerminal` (scripted lines, an interrupt, a
//! timeout, echo-state recording), `FakeSecretStore`, `FakeEgress` and
//! `FakeProjectIdentity`. One test per row of the story's I/O matrix that
//! runs in process; the rows that need the real binary (piped stdin, `^C`
//! on a pty) are in `accounts_tty.rs`, the store's side in
//! `crates/am-store/tests/config_history.rs`, `ConfigCreate` in
//! `crates/am-config/tests/create.rs` and the dependency guard in
//! `xtask/tests/release_deps.rs`. Canary keys only; no network, no Keychain.
//! Written before the command exists.
//!
//! Story 2.3 (the OpenAI section): the same commands for the providers
//! `openai` and `openai-compatible`. The add path is story 2.1's, unchanged;
//! what differs is the provider and the endpoint (`--base-url`, in the two
//! accepted forms), which the sentence, the audit entry, `list` and `test`'s
//! egress policy all name. Written before the providers were accepted.
//!
//! Story 2.12 (AD-17, AD-15 *Interim CLI Account edit*): Accounts belong to
//! this Mac. Every test here was rewritten for it before the change:
//!
//! - The commands take no `--project` and open no project: they read and
//!   write the user Accounts file, `<app data dir>/accounts.yaml`
//!   (`Places::user`, an `AppDataLayout`), and record in the user store,
//!   `user.db`, under `user.lock`. The folder a command runs from changes
//!   nothing (`Kit::run`'s folder is not passed).
//! - History entries name the file `accounts.yaml`; `account_added` (with
//!   `key: stored`) and `account_removed` are the user store's audit
//!   entries. Project stores are never written by these commands.
//! - References are found in every project of the runtime base's root
//!   records (each test's projects are opened once on this Mac, so they are
//!   recorded).
//! - An Account seeded "by hand" is written straight into the user file,
//!   owner-only, with no History (as a hand edit would leave it).
//! - When the Keychain already holds an entry for the id and the user store
//!   has no confirmation of it for this destination, the add also reads the
//!   stored key at the hidden prompt and it must equal the entry.
//!
//! The new rows of story 2.12 that need several processes, permissions or
//! the lock are in `user_accounts.rs`.
#![forbid(unsafe_code)]
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use am_app::cli::{AccountsContext, accounts};
use am_app::runtime::{OpenRequest, Places, Prompt, open_project};
use am_domain::net::{Caller, EgressDirectory, Endpoint, reserved_local_account};
use am_domain::runtime::{legacy_dir, open_marker};
use am_domain::{
    Actor, AuthRef, AuthScheme, ConfigHistory, EgressClient, HistoryEntry, HistoryKind,
    RuntimeLayout, SecretStore,
};
use am_domain::{AuditEvent, AuditSink, BoxFuture, EgressError, HttpRequest, HttpResponse};
use am_store::SCHEMA_VERSION;
use am_store::user::{UserReader, UserStore};
use am_testkit::{FakeEgress, FakeProjectIdentity, FakeSecretStore, FakeTerminal};

/// The key the owner types: a canary that must never leave the keychain.
const CANARY: &str = "sk-ant-canary-0a1b2c3d4e5f-WXYZ";
/// The user Accounts file as History and the audit name it (AD-17).
const USER_FILE: &str = "accounts.yaml";
/// The old per-project file: never written or read for a key since 2.12.
const LEGACY_FILE: &str = "models/accounts.yaml";

const COMPANY: &str = "schema_version: 2\nname: Example\ntimezone: UTC\ncurrency: EUR\nteams:\n  - id: core\n    name: Core team\n";

/// A seat whose fallback names `account`.
fn alpha(account: &str) -> String {
    format!(
        "---\nschema_version: 2\nname: Alpha\nrole: Analyst\nteam: core\nmodel:\n  account: local-lmstudio\n  model: example-local\nfallback:\n  - account: {account}\n    model: example-large\n---\nDo the work.\n"
    )
}

/// A seat whose model is on `account`.
fn beta(account: &str) -> String {
    format!(
        "---\nschema_version: 2\nname: Beta\nrole: Writer\nteam: core\nmodel:\n  account: {account}\n  model: example-large\n---\nWrite it up.\n"
    )
}

/// An Accounts file with one Anthropic Account per id.
fn accounts_yaml(ids: &[&str]) -> String {
    let mut s = String::from("schema_version: 2\naccounts:\n");
    for id in ids {
        s.push_str(&format!(
            "  - id: {id}\n    provider: anthropic\n    secret_ref: {id}\n"
        ));
    }
    s
}

const STDIO_TOOLS: &str = "schema_version: 2\nmcp_servers:\n  - id: notes\n    transport: stdio\n    command: example-notes-server\n    version: 1.0.0\n    network: none\n    tools:\n      - name: list_notes\n        class: read\n";

fn owner() -> Actor {
    Actor::Human {
        person: "owner".into(),
        hat: "Owner".into(),
    }
}

/// The open-time prompt of the setup: never asked.
struct NoPrompt;

impl Prompt for NoPrompt {
    fn interactive(&self) -> bool {
        false
    }
    fn ask(&mut self, _question: &str) -> bool {
        false
    }
}

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

fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

impl Env {
    /// A project folder `name` holding `files`, not yet opened.
    fn folder(&self, name: &str, files: &[(&str, String)]) -> PathBuf {
        let root = self.dir.path().join("projects").join(name);
        std::fs::create_dir_all(&root).unwrap();
        for (rel, text) in files {
            let path = root.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        root.canonicalize().unwrap()
    }

    /// A project folder `name` holding `files`, opened once on this Mac (an
    /// id, a runtime folder, a store and a root record) and closed again.
    fn project(&self, name: &str, files: &[(&str, String)]) -> PathBuf {
        let root = self.folder(name, files);
        let req = OpenRequest {
            root: root.clone(),
            confirm_move: false,
            take_over: false,
        };
        let opened = open_project(
            &self.places,
            &FakeProjectIdentity::new("Test Mac"),
            &req,
            &mut NoPrompt,
        )
        .unwrap();
        drop(opened);
        root
    }

    /// The usual project: company, a seat whose fallback names `cloud2` and
    /// a seat on `cloud2`, with `extra` files.
    fn usual(&self, extra: &[(&str, String)]) -> PathBuf {
        let mut files = vec![
            ("company.yaml", COMPANY.to_string()),
            ("seats/alpha.md", alpha("cloud2")),
            ("seats/beta.md", beta("cloud2")),
        ];
        files.extend(extra.iter().cloned());
        self.project("p", &files)
    }

    fn layout(&self, root: &Path) -> RuntimeLayout {
        let id = am_config::read_company(root).unwrap().project_id.unwrap();
        self.places.base.project(&id)
    }

    /// The user Accounts file.
    fn accounts_path(&self) -> PathBuf {
        self.places.user.accounts_file()
    }

    /// Its text now, if it exists.
    fn user_text(&self) -> Option<String> {
        std::fs::read_to_string(self.accounts_path()).ok()
    }

    /// Writes the user Accounts file by hand (no History), owner-only in an
    /// owner-only app data dir, as the app keeps them.
    fn write_user_file(&self, text: &str) {
        let dir = self.places.user.dir();
        std::fs::create_dir_all(dir).unwrap();
        set_mode(dir, 0o700);
        std::fs::write(self.accounts_path(), text).unwrap();
        set_mode(&self.accounts_path(), 0o600);
    }

    /// Every History entry of the user store, read-only.
    fn history(&self) -> Vec<HistoryEntry> {
        UserReader::open(&self.places.user)
            .unwrap()
            .config_history(None)
            .unwrap()
    }

    /// The user store's audit log as JSON Lines, read-only.
    fn audit(&self) -> String {
        let reader = UserReader::open(&self.places.user).unwrap();
        let mut out = Vec::new();
        reader.export_audit_jsonl(&mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    /// The History entries the project's own store holds beyond the setup's
    /// `project_id` mint: the commands never write a project store.
    fn project_history(&self, root: &Path) -> Vec<HistoryEntry> {
        am_store::Reader::open(&self.layout(root).state_db())
            .unwrap()
            .config_history(None)
            .unwrap()
            .into_iter()
            .filter(|e| !matches!(&e.actor, Actor::System { component } if component == "runtime"))
            .collect()
    }

    /// A writer of the user Accounts file holding the user store's lock, as
    /// a command holds it (released when the writer drops).
    fn user_writer(&self) -> am_config::ConfigWriter {
        let guard = UserStore::locked(&self.places.user, Duration::from_secs(10)).unwrap();
        am_config::ConfigWriter::for_user_accounts(self.places.user.dir(), Arc::new(guard)).unwrap()
    }
}

fn read(root: &Path, rel: &str) -> Option<String> {
    std::fs::read_to_string(root.join(rel)).ok()
}

/// The Accounts the user file holds now: (id, provider, name, secret_ref).
/// It must load without an error: what the commands wrote is valid.
fn loaded_accounts(env: &Env) -> Vec<(String, String, Option<String>, Option<String>)> {
    let loaded = am_config::user_accounts::load(&env.accounts_path());
    assert!(
        loaded
            .findings
            .iter()
            .all(|f| f.severity != am_config::Severity::Error),
        "{:#?}",
        loaded.findings
    );
    loaded
        .accounts
        .ids()
        .iter()
        .map(|id| {
            let a = loaded.accounts.get(id).unwrap();
            (
                a.id.clone(),
                a.provider.clone(),
                a.name.clone(),
                a.secret_ref.clone(),
            )
        })
        .collect()
}

/// What one command printed.
struct Ran {
    code: i32,
    out: String,
    err: String,
    /// What it wrote to the terminal itself (prompts).
    shown: String,
}

impl Ran {
    fn all(&self) -> String {
        format!("{}\n{}\n{}", self.out, self.err, self.shown)
    }
}

/// What `AccountsContext::egress` is (story 2.2): a client built over the
/// project's egress directory, reporting blocks to the command's sink.
type EgressFactory = Arc<
    dyn Fn(Arc<dyn EgressDirectory>, Arc<dyn AuditSink>) -> Arc<dyn EgressClient> + Send + Sync,
>;

/// The fakes one command runs with.
struct Kit {
    term: FakeTerminal,
    secrets: Arc<FakeSecretStore>,
    egress: Arc<FakeEgress>,
    /// Every egress directory the command built a client for (story 2.2:
    /// the client decides from the directory alone).
    directories: Arc<Mutex<Vec<Arc<dyn EgressDirectory>>>>,
    /// When set, the client the factory builds refuses every request,
    /// reporting the block to the audit sink the command handed it.
    blocking: bool,
}

impl Kit {
    fn new(term: FakeTerminal) -> Kit {
        Kit {
            term,
            secrets: FakeSecretStore::new(),
            egress: FakeEgress::new(),
            directories: Arc::new(Mutex::new(Vec::new())),
            blocking: false,
        }
    }

    fn with_key(mut self, account: &str, key: &str) -> Kit {
        self.secrets = FakeSecretStore::with(account, key);
        self
    }

    fn key(&self, account: &str) -> Option<String> {
        self.secrets
            .get(account)
            .unwrap()
            .map(|s| s.expose().to_string())
    }

    fn directories(&self) -> Vec<Arc<dyn EgressDirectory>> {
        self.directories.lock().unwrap().clone()
    }

    /// The client refuses every request (and audits it) instead of
    /// answering.
    fn blocking(mut self) -> Kit {
        self.blocking = true;
        self
    }

    /// Runs `agentic accounts <args>` as the owner does from the folder
    /// `_cwd`. Since story 2.12 the commands take no project and open none,
    /// so the folder is not passed: it changes nothing.
    async fn run(&self, env: &Env, _cwd: &Path, args: &[&str]) -> Ran {
        self.run_with(env, _cwd, args, self.secrets.clone()).await
    }

    /// [`Kit::run`] with another secret store (one that counts its reads).
    async fn run_with(
        &self,
        env: &Env,
        _cwd: &Path,
        args: &[&str],
        secrets: Arc<dyn SecretStore>,
    ) -> Ran {
        let directories = self.directories.clone();
        let egress = self.egress.clone();
        let blocking = self.blocking;
        let factory: EgressFactory = Arc::new(
            move |directory: Arc<dyn EgressDirectory>, audit: Arc<dyn AuditSink>| {
                directories.lock().unwrap().push(directory);
                if blocking {
                    Arc::new(BlockingEgress { audit }) as Arc<dyn EgressClient>
                } else {
                    egress.clone() as Arc<dyn EgressClient>
                }
            },
        );
        let ctx = AccountsContext {
            places: env.places.clone(),
            identity: Arc::new(FakeProjectIdentity::new("Test Mac")),
            terminal: Box::new(self.term.clone()),
            secrets,
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

fn endpoint(scheme: &str, host: &str, port: u16) -> Endpoint {
    Endpoint {
        scheme: scheme.into(),
        host: host.into(),
        port,
    }
}

/// The project's directory knows no id this Mac's user list does not hold,
/// and answers the reserved local Accounts from am-domain's one
/// definition (story 2.2).
fn assert_directory_refuses_unknown_ids(directory: &dyn EgressDirectory) {
    assert_eq!(directory.account_endpoint("no-such-account"), None);
    assert_eq!(directory.seat_hosts("no-such-seat"), None);
    assert_eq!(directory.mcp_endpoint("no-such-server"), None);
    for reserved in ["local-lmstudio", "local-ollama"] {
        assert_eq!(
            directory.account_endpoint(reserved),
            reserved_local_account(reserved),
            "{reserved}"
        );
    }
}

/// An egress client that refuses every request as the real one refuses an
/// unlisted destination: it reports an `egress.blocked` entry to the sink
/// the command handed its factory, and returns `Blocked`.
struct BlockingEgress {
    audit: Arc<dyn AuditSink>,
}

impl EgressClient for BlockingEgress {
    fn send(&self, request: HttpRequest) -> BoxFuture<Result<HttpResponse, EgressError>> {
        let host = request
            .url
            .split("://")
            .nth(1)
            .and_then(|rest| rest.split(['/', ':']).next())
            .unwrap_or("")
            .to_string();
        let account = match &request.caller {
            Some(Caller::Account { account, .. }) => account.clone(),
            _ => String::new(),
        };
        self.audit.record(AuditEvent {
            actor: Actor::System {
                component: "am-net".into(),
            },
            action: "egress.blocked".into(),
            target: format!("https://{host}:443"),
            outcome: "blocked".into(),
            hash: None,
            args: [
                ("scheme", "https".to_string()),
                ("host", host.clone()),
                ("port", "443".to_string()),
                (
                    "reason",
                    "the destination is not the Account's endpoint".to_string(),
                ),
                ("account", account),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
        });
        Box::pin(async move {
            Err(EgressError::Blocked(format!(
                "{host} is not this Account's endpoint"
            )))
        })
    }
}

/// Every file under `dir`, recursively.
fn files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for e in read.flatten() {
        let path = e.path();
        match e.file_type() {
            Ok(t) if t.is_dir() => files_under(&path, out),
            Ok(t) if t.is_file() => out.push(path),
            _ => {}
        }
    }
}

/// Nothing of the owner's key anywhere: output, the user file, History,
/// the audit log, and no byte of any file in the temporary HOME or the
/// projects (`user.db`, its WAL, every project store, every config file).
fn assert_no_key_leaked(env: &Env, _root: &Path, ran: &Ran) {
    assert!(
        !ran.all().contains(CANARY),
        "the key was printed: {}",
        ran.all()
    );
    if let Some(text) = env.user_text() {
        assert!(!text.contains(CANARY), "the key reached the file");
    }
    if env.places.user.user_db().exists() {
        for e in env.history() {
            for bytes in [e.before.as_slice(), e.after.as_slice(), e.diff.as_bytes()] {
                assert!(
                    !String::from_utf8_lossy(bytes).contains(CANARY),
                    "the key reached History"
                );
            }
        }
        assert!(
            !env.audit().contains(CANARY),
            "the key reached the audit log"
        );
    }
    let mut all = Vec::new();
    files_under(env.dir.path(), &mut all);
    for file in all {
        let bytes = std::fs::read(&file).unwrap_or_default();
        assert!(
            !bytes.windows(CANARY.len()).any(|w| w == CANARY.as_bytes()),
            "the key reached {}",
            file.display()
        );
    }
}

fn audit_line_of_kind(audit: &str, kind: &str) -> Vec<String> {
    audit
        .lines()
        .filter(|l| l.contains(&format!("\"kind\":\"{kind}\"")))
        .map(str::to_string)
        .collect()
}

fn has(line: &str, key: &str, value: &str) -> bool {
    line.contains(&format!("\"{key}\":\"{value}\""))
}

// ------------------------------------------------------------------- add

/// Add (and Missing file): on a terminal, the sentence names the Account,
/// its provider, its endpoint and every seat that already names it; the
/// typed id is read with echo on and within 2 minutes, then the key with
/// echo off; the key goes only to the keychain under the Account's id; the
/// missing user Accounts file is created (`create` entry in the user
/// store) holding `secret_ref` and never the key; one `account_added` audit
/// entry carries the file, the Account, its provider, the pinned scheme,
/// host and port, the references, `key: stored` and `confirmation:
/// typed_id`. No marker is written, and the project (its files and its
/// store) is not touched.
#[tokio::test]
async fn add_creates_the_file_stores_the_key_and_records_history_and_audit() {
    let env = env();
    let root = env.usual(&[]);
    let seats = (read(&root, "seats/alpha.md"), read(&root, "seats/beta.md"));
    let kit = Kit::new(FakeTerminal::new());
    kit.term.line("cloud2");
    kit.term.line(CANARY);

    let ran = kit
        .run(&env, &root, &["add", "cloud2", "--provider", "anthropic"])
        .await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    let all = ran.all();
    for word in [
        "cloud2",
        "anthropic",
        "https://api.anthropic.com",
        "alpha",
        "beta",
    ] {
        assert!(all.contains(word), "the sentence names {word}: {all}");
    }
    assert_eq!(
        kit.term.echo_reads(),
        vec![true, false],
        "id echoed, key not"
    );
    assert!(kit.term.echo(), "echo is on again");
    assert_eq!(
        kit.term.timeouts().first().copied().flatten(),
        Some(Duration::from_secs(120)),
        "the id is asked within 2 minutes"
    );
    assert_eq!(kit.key("cloud2").as_deref(), Some(CANARY));

    assert_eq!(
        loaded_accounts(&env),
        vec![(
            "cloud2".to_string(),
            "anthropic".to_string(),
            None,
            Some("cloud2".to_string())
        )]
    );
    let history = env.history();
    assert_eq!(history.len(), 1, "{history:#?}");
    assert_eq!(history[0].file, USER_FILE);
    assert_eq!(history[0].kind, HistoryKind::Create);
    assert_eq!(history[0].actor, owner());

    let audit = env.audit();
    let added = audit_line_of_kind(&audit, "account_added");
    assert_eq!(added.len(), 1, "{audit}");
    let line = &added[0];
    assert!(has(line, "file", USER_FILE), "{line}");
    assert!(has(line, "account", "cloud2"), "{line}");
    assert!(has(line, "provider", "anthropic"), "{line}");
    assert!(has(line, "scheme", "https"), "{line}");
    assert!(has(line, "host", "api.anthropic.com"), "{line}");
    assert!(
        line.contains("\"port\":\"443\"") || line.contains("\"port\":443"),
        "{line}"
    );
    assert!(has(line, "key", "stored"), "{line}");
    assert!(has(line, "confirmation", "typed_id"), "{line}");
    assert!(
        line.contains("alpha") && line.contains("beta"),
        "references: {line}"
    );
    assert!(audit.contains("config.created"), "{audit}");

    assert!(!open_marker(&root).exists(), "no marker is written");
    assert!(!root.join(LEGACY_FILE).exists(), "no project Accounts file");
    assert_eq!(
        (read(&root, "seats/alpha.md"), read(&root, "seats/beta.md")),
        seats,
        "no project file changes"
    );
    assert!(
        env.project_history(&root).is_empty(),
        "the project store is not written"
    );
    assert_no_key_leaked(&env, &root, &ran);
}

/// Add to an existing file: the Account is appended as an `edit` (with its
/// optional name); the other Accounts stay.
#[tokio::test]
async fn add_to_an_existing_file_appends_the_account_as_an_edit() {
    let env = env();
    let root = env.usual(&[]);
    env.write_user_file(&accounts_yaml(&["cloud"]));
    let kit = Kit::new(FakeTerminal::new());
    kit.term.line("cloud2");
    kit.term.line(CANARY);

    let ran = kit
        .run(
            &env,
            &root,
            &[
                "add",
                "cloud2",
                "--provider",
                "anthropic",
                "--name",
                "Second cloud",
            ],
        )
        .await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    assert_eq!(
        loaded_accounts(&env),
        vec![
            (
                "cloud".to_string(),
                "anthropic".to_string(),
                None,
                Some("cloud".to_string())
            ),
            (
                "cloud2".to_string(),
                "anthropic".to_string(),
                Some("Second cloud".to_string()),
                Some("cloud2".to_string())
            ),
        ]
    );
    let history = env.history();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].kind, HistoryKind::Edit);
    assert_eq!(audit_line_of_kind(&env.audit(), "account_added").len(), 1);
    assert_no_key_leaked(&env, &root, &ran);
}

/// Not a terminal (in process): stdin that is not a terminal is refused in
/// words naming the terminal rule, before anything is read, written or
/// stored; `remove` likewise.
#[tokio::test]
async fn add_and_remove_refuse_when_stdin_is_not_a_terminal() {
    let env = env();
    let root = env.usual(&[]);
    env.write_user_file(&accounts_yaml(&["cloud2", "spare"]));
    let before = env.user_text();
    for args in [
        vec!["add", "cloud3", "--provider", "anthropic"],
        vec!["remove", "spare"],
    ] {
        let kit = Kit::new(FakeTerminal::piped());
        kit.term.line("cloud3");
        kit.term.line(CANARY);
        let ran = kit.run(&env, &root, &args).await;
        assert_ne!(ran.code, 0, "{args:?}: {}", ran.all());
        assert!(ran.all().contains("terminal"), "{args:?}: {}", ran.all());
        assert_eq!(kit.term.reads(), 0, "{args:?}: nothing is read");
        assert_eq!(kit.key("cloud3"), None);
    }
    assert_eq!(env.user_text(), before);
    assert!(env.history().is_empty());
}

/// Wrong name: a typed id that differs writes and stores nothing, and the
/// key is never asked (echo never turned off).
#[tokio::test]
async fn add_with_a_wrong_name_writes_and_stores_nothing() {
    let env = env();
    let root = env.usual(&[]);
    let kit = Kit::new(FakeTerminal::new());
    kit.term.line("cloud3");
    kit.term.line(CANARY);

    let ran = kit
        .run(&env, &root, &["add", "cloud2", "--provider", "anthropic"])
        .await;

    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(
        !ran.err.is_empty() || !ran.out.is_empty(),
        "a worded refusal"
    );
    assert_eq!(kit.term.echo_reads(), vec![true], "the key is never read");
    assert!(kit.term.echo());
    assert_eq!(kit.key("cloud2"), None);
    assert!(!env.accounts_path().exists());
    assert!(env.history().is_empty());
    assert!(audit_line_of_kind(&env.audit(), "account_added").is_empty());
}

/// The id not typed within 2 minutes: nothing written or stored.
#[tokio::test]
async fn add_that_times_out_writes_and_stores_nothing() {
    let env = env();
    let root = env.usual(&[]);
    let kit = Kit::new(FakeTerminal::new());
    kit.term.time_out();
    kit.term.line(CANARY);

    let ran = kit
        .run(&env, &root, &["add", "cloud2", "--provider", "anthropic"])
        .await;

    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(ran.all().contains("2 minutes"), "{}", ran.all());
    assert_eq!(
        kit.term.timeouts().first().copied().flatten(),
        Some(Duration::from_secs(120))
    );
    assert_eq!(kit.term.reads(), 1);
    assert!(kit.term.echo());
    assert_eq!(kit.key("cloud2"), None);
    assert!(!env.accounts_path().exists());
    assert!(env.history().is_empty());
}

/// Interrupt (in process): `^C` at the key prompt ends the command non-zero
/// with echo on again, nothing written or stored.
#[tokio::test]
async fn interrupt_at_the_key_prompt_restores_echo_and_stores_nothing() {
    let env = env();
    let root = env.usual(&[]);
    let kit = Kit::new(FakeTerminal::new());
    kit.term.line("cloud2");
    kit.term.interrupt();

    let ran = kit
        .run(&env, &root, &["add", "cloud2", "--provider", "anthropic"])
        .await;

    assert_ne!(ran.code, 0, "{}", ran.all());
    assert_eq!(kit.term.echo_reads(), vec![true, false]);
    assert!(kit.term.echo(), "echo is restored after ^C");
    assert_eq!(kit.key("cloud2"), None);
    assert!(!env.accounts_path().exists());
    assert!(env.history().is_empty());
}

/// Reserved or duplicate: `local-lmstudio`, `local-ollama` and an id
/// already in the file are refused in words before anything is read;
/// nothing changes.
#[tokio::test]
async fn add_refuses_a_reserved_or_existing_id() {
    let env = env();
    let root = env.usual(&[]);
    env.write_user_file(&accounts_yaml(&["cloud2"]));
    let before = env.user_text();
    for id in ["local-ollama", "local-lmstudio", "cloud2"] {
        let kit = Kit::new(FakeTerminal::new());
        kit.term.line(id);
        kit.term.line(CANARY);
        let ran = kit
            .run(&env, &root, &["add", id, "--provider", "anthropic"])
            .await;
        assert_ne!(ran.code, 0, "{id}: {}", ran.all());
        assert!(ran.all().contains(id), "{id}: {}", ran.all());
        assert_eq!(kit.term.reads(), 0, "{id}: nothing is read");
        assert_eq!(kit.key(id), None);
    }
    assert_eq!(env.user_text(), before);
    assert!(env.history().is_empty());
}

/// A provider that is not a model provider (a search provider, or a word
/// that is no provider at all) is refused naming the three an Account can be
/// added for, before anything is read. (Story 2.1 refused every provider but
/// Anthropic; story 2.3 adds OpenAI and OpenAI-compatible Accounts.)
#[tokio::test]
async fn add_refuses_a_provider_that_is_not_a_model_provider() {
    let env = env();
    let root = env.usual(&[]);
    for provider in ["brave", "nope"] {
        let kit = Kit::new(FakeTerminal::new());
        kit.term.line("cloud2");
        kit.term.line(CANARY);

        let ran = kit
            .run(&env, &root, &["add", "cloud2", "--provider", provider])
            .await;

        assert_ne!(ran.code, 0, "{provider}: {}", ran.all());
        let said = ran.all().to_lowercase();
        let named: Vec<&str> = said
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
            .collect();
        for model_provider in ["anthropic", "openai", "openai-compatible"] {
            assert!(
                named.contains(&model_provider),
                "{provider}: the refusal names {model_provider}: {}",
                ran.all()
            );
        }
        assert!(ran.all().contains(provider), "{provider}: {}", ran.all());
        assert_eq!(kit.term.reads(), 0, "{provider}: nothing is read");
        assert!(ran.shown.is_empty(), "{provider}: no prompt: {}", ran.shown);
        assert_eq!(kit.key("cloud2"), None);
    }
    assert!(!env.accounts_path().exists());
    assert!(env.history().is_empty());
}

/// Reuse key (story 2.12, Existing Keychain entry): a keychain entry
/// already held for the id is reused and never overwritten, and the add
/// says so; since the user store holds no confirmation of that id for this
/// provider and endpoint, the owner types the stored key at the hidden
/// prompt (echo off) and it must equal the entry. It is never echoed,
/// printed or stored again.
#[tokio::test]
async fn add_reuses_an_existing_keychain_entry_and_never_overwrites_it() {
    const EARLIER: &str = "sk-ant-earlier-canary-1111";
    let env = env();
    let root = env.usual(&[]);
    let kit = Kit::new(FakeTerminal::new()).with_key("cloud2", EARLIER);
    kit.term.line("cloud2");
    kit.term.line(EARLIER);

    let ran = kit
        .run(&env, &root, &["add", "cloud2", "--provider", "anthropic"])
        .await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(ran.all().to_lowercase().contains("reus"), "{}", ran.all());
    assert_eq!(
        kit.term.echo_reads(),
        vec![true, false],
        "the id echoed, then the stored key at the hidden prompt"
    );
    assert!(kit.term.echo(), "echo is on again");
    assert_eq!(
        kit.key("cloud2").as_deref(),
        Some(EARLIER),
        "never overwritten"
    );
    assert_eq!(loaded_accounts(&env).len(), 1);
    assert!(!ran.all().contains(EARLIER));
    assert!(!ran.all().contains(&EARLIER[..12]));
    let audit = env.audit();
    assert!(!audit.contains(EARLIER), "the stored key is never recorded");
    assert_eq!(audit_line_of_kind(&audit, "account_added").len(), 1);
}

/// Existing Keychain entry, mismatch: the key typed at the hidden prompt is
/// not the stored one, so nothing is written or recorded, the entry stays
/// as it was, and neither key is printed.
#[tokio::test]
async fn add_reusing_an_entry_whose_stored_key_is_mistyped_writes_nothing() {
    const EARLIER: &str = "sk-ant-earlier-canary-1111";
    let env = env();
    let root = env.usual(&[]);
    let kit = Kit::new(FakeTerminal::new()).with_key("cloud2", EARLIER);
    kit.term.line("cloud2");
    kit.term.line(CANARY);

    let ran = kit
        .run(&env, &root, &["add", "cloud2", "--provider", "anthropic"])
        .await;

    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(
        ran.all().contains("does not match"),
        "says the typed key is not the stored one: {}",
        ran.all()
    );
    assert_eq!(kit.term.echo_reads(), vec![true, false]);
    assert_eq!(kit.key("cloud2").as_deref(), Some(EARLIER), "unchanged");
    assert!(!env.accounts_path().exists(), "nothing written");
    assert!(env.history().is_empty());
    assert!(audit_line_of_kind(&env.audit(), "account_added").is_empty());
    for key in [EARLIER, CANARY] {
        assert!(!ran.all().contains(key), "{}", ran.all());
    }
}

/// Existing Keychain entry with a confirmation for another endpoint: the
/// user store confirmed the id for another host, so a re-add at a new
/// endpoint asks for the stored key too.
#[tokio::test]
async fn add_reusing_an_entry_confirmed_for_another_host_asks_for_the_stored_key() {
    const STORED: &str = "sk-or-stored-canary-2222-RTRT";
    let env = env();
    let root = plain(&env);
    add_account(
        &env,
        &root,
        "router",
        "openai-compatible",
        Some("https://first.example/v1"),
        STORED,
    )
    .await;
    remove_account(&env, &root, "router").await;

    let kit = Kit::new(FakeTerminal::new()).with_key("router", STORED);
    kit.term.line("router");
    kit.term.line(STORED);
    let ran = kit
        .run(
            &env,
            &root,
            &[
                "add",
                "router",
                "--provider",
                "openai-compatible",
                "--base-url",
                "https://second.example/v1",
            ],
        )
        .await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert_eq!(
        kit.term.echo_reads(),
        vec![true, false],
        "the stored key is asked"
    );
    assert_eq!(kit.key("router").as_deref(), Some(STORED));
    assert!(!ran.all().contains(STORED));
}

/// Write fails: the user file's writer stops before the rename (as a full
/// disk or a crash would; `am_app::runtime::testing::fail_writer_at` keyed
/// by the app data dir, the user writer's root) after this add stored the
/// key, so the keychain entry it created is deleted, the file is unchanged
/// and the failure is shown; an entry the add only reused stays.
#[tokio::test]
async fn a_failed_write_deletes_only_the_key_this_add_stored() {
    let env = env();
    let root = env.usual(&[]);
    env.write_user_file(&accounts_yaml(&["cloud"]));
    let before = env.user_text();
    let app_data = env.places.user.dir().to_path_buf();
    am_app::runtime::testing::fail_writer_at(&app_data, Some(am_config::FailPoint::BeforeRename));

    let kit = Kit::new(FakeTerminal::new());
    kit.term.line("cloud2");
    kit.term.line(CANARY);
    let ran = kit
        .run(&env, &root, &["add", "cloud2", "--provider", "anthropic"])
        .await;
    assert_ne!(ran.code, 0, "{}", ran.all());
    assert_eq!(
        kit.key("cloud2"),
        None,
        "the key this add stored is deleted"
    );
    assert_eq!(env.user_text(), before);

    let reused = Kit::new(FakeTerminal::new()).with_key("cloud2", "sk-ant-earlier-canary-1111");
    reused.term.line("cloud2");
    reused.term.line("sk-ant-earlier-canary-1111");
    let ran = reused
        .run(&env, &root, &["add", "cloud2", "--provider", "anthropic"])
        .await;
    am_app::runtime::testing::fail_writer_at(&app_data, None);
    assert_ne!(ran.code, 0, "{}", ran.all());
    assert_eq!(
        reused.key("cloud2").as_deref(),
        Some("sk-ant-earlier-canary-1111"),
        "an entry the add did not create stays"
    );
    assert_eq!(env.user_text(), before);
    assert!(env.history().is_empty());
    assert!(audit_line_of_kind(&env.audit(), "account_added").is_empty());
}

// ------------------------------------------------------------ list, test

/// List: each Account's id, provider, host and key state, masked as dots
/// plus the last 4 characters, or "no key in Keychain"; any stdin; no edit.
#[tokio::test]
async fn list_masks_keys_and_names_missing_ones() {
    let env = env();
    let root = env.usual(&[]);
    env.write_user_file(&accounts_yaml(&["cloud", "cloud2"]));
    let before = env.user_text();
    let kit = Kit::new(FakeTerminal::piped()).with_key("cloud", CANARY);

    let ran = kit.run(&env, &root, &["list"]).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    let line_of = |id: &str| {
        ran.out
            .lines()
            .find(|l| {
                l.split_whitespace()
                    .any(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '-') == id)
            })
            .unwrap_or_else(|| panic!("no line for {id}: {}", ran.out))
            .to_string()
    };
    let cloud = line_of("cloud");
    assert!(cloud.contains("anthropic"), "{cloud}");
    assert!(cloud.contains("api.anthropic.com"), "{cloud}");
    assert!(
        cloud.contains('•') && cloud.contains("WXYZ"),
        "masked: {cloud}"
    );
    assert!(!cloud.contains("sk-ant"), "{cloud}");
    let cloud2 = line_of("cloud2");
    assert!(cloud2.contains("no key in Keychain"), "{cloud2}");
    assert_eq!(kit.term.reads(), 0);
    assert_eq!(env.user_text(), before);
    assert!(env.history().is_empty());
    assert_no_key_leaked(&env, &root, &ran);
}

/// Test: one short reply streamed through an egress client whose policy
/// admits only the Account's host (no loopback port); the request names the
/// Account in its `AuthRef`; the client's directory pins the Account to
/// its endpoint (story 2.2); any stdin; no edit. (Rewritten in story 2.3's review: the
/// Account is added through the command first, because `test` calls only an
/// Account that `agentic accounts add` confirmed on this Mac.)
#[tokio::test]
async fn test_streams_a_reply_through_the_accounts_pinned_host() {
    let env = env();
    let root = plain(&env);
    add_account(&env, &root, "cloud", "anthropic", None, CANARY).await;
    let before = env.user_text();
    let recorded = env.history().len();
    let kit = Kit::new(FakeTerminal::piped()).with_key("cloud", CANARY);
    kit.egress
        .replay("api.anthropic.com", "anthropic/c1_count_stream.sse");

    let ran = kit.run(&env, &root, &["test", "cloud"]).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    for word in ["one", "two", "three"] {
        assert!(ran.out.contains(word), "streamed text: {}", ran.out);
    }
    let requests = kit.egress.requests();
    assert_eq!(requests.len(), 1, "one short reply");
    assert!(
        requests[0].url.starts_with("https://api.anthropic.com/"),
        "{}",
        requests[0].url
    );
    assert_eq!(
        requests[0].auth,
        Some(AuthRef {
            scheme: AuthScheme::XApiKey,
            account_id: "cloud".into()
        })
    );
    assert!(!String::from_utf8_lossy(&requests[0].body).contains(CANARY));

    // Story 2.2: the client is built over the project's egress directory,
    // which pins the Account to its endpoint (scheme, host and port; the
    // client compares them exactly, rule_09_callers), knows no other id
    // than the project's, and the request names the Account as its caller.
    let directories = kit.directories();
    assert_eq!(directories.len(), 1, "one egress client is built");
    let directory = directories[0].as_ref();
    assert_eq!(
        directory.account_endpoint("cloud"),
        Some(endpoint("https", "api.anthropic.com", 443))
    );
    assert_directory_refuses_unknown_ids(directory);
    assert_eq!(
        requests[0].caller,
        Some(Caller::Account {
            account: "cloud".into(),
            seat: None
        })
    );

    assert_eq!(env.user_text(), before);
    assert_eq!(env.history().len(), recorded, "no edit");
    assert!(!ran.all().contains(CANARY));
}

/// Story 2.2, decision 3 (owner, 2026-11-22): `agentic accounts test`
/// opens no store, so a block the egress client reports cannot reach the
/// audit log. The command prints the block (its host and reason) and says
/// it is not in the audit log; it fails, and the key is never printed.
#[tokio::test]
async fn test_prints_a_block_and_says_it_is_not_in_the_audit_log() {
    let env = env();
    let root = plain(&env);
    add_account(&env, &root, "cloud3", "anthropic", None, CANARY).await;
    let kit = Kit::new(FakeTerminal::piped())
        .with_key("cloud3", CANARY)
        .blocking();

    let ran = kit.run(&env, &root, &["test", "cloud3"]).await;

    assert_ne!(ran.code, 0, "{}", ran.all());
    assert_eq!(kit.directories().len(), 1, "one egress client was built");
    let said = ran.all();
    assert!(said.contains("api.anthropic.com"), "names the host: {said}");
    assert!(
        said.contains("the destination is not the Account's endpoint"),
        "prints the block's reason: {said}"
    );
    assert!(
        said.to_ascii_lowercase().contains("not in the audit log"),
        "says the block is not in the audit log: {said}"
    );
    assert!(!said.contains(CANARY), "{said}");
}

/// Test of an Account with no key, or of an unknown id: refused in words,
/// nothing sent. (Rewritten in story 2.3's review: the Account is added
/// through the command first; the `test` then runs with a Keychain that
/// holds no key for it.)
#[tokio::test]
async fn test_without_a_key_or_of_an_unknown_id_sends_nothing() {
    let env = env();
    let root = env.usual(&[]);
    add_account(&env, &root, "cloud2", "anthropic", None, CANARY).await;
    let kit = Kit::new(FakeTerminal::piped());
    kit.egress
        .replay("api.anthropic.com", "anthropic/c1_count_stream.sse");

    let ran = kit.run(&env, &root, &["test", "cloud2"]).await;
    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(ran.all().contains("no key in Keychain"), "{}", ran.all());
    // The advice is one the owner can follow: `cloud2` is named by seats,
    // so "remove it, then add it" is not offered as if it would work.
    assert!(
        !ran.all().contains("remove cloud2` then"),
        "advice that cannot be followed: {}",
        ran.all()
    );
    assert!(ran.all().contains("unreferenced"), "{}", ran.all());

    let ran = kit.run(&env, &root, &["test", "nope"]).await;
    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(ran.all().contains("nope"), "{}", ran.all());
    assert!(kit.egress.requests().is_empty(), "nothing sent");
}

// ---------------------------------------------------------------- remove

/// Remove, unreferenced: a plain edit with History and Undo; the keychain
/// entry stays and the command says so. Story 2.12: the remove writes
/// `account_removed` (the id) in the user store with its History entry, and
/// a later `test` of the id is refused.
#[tokio::test]
async fn remove_of_an_unreferenced_account_has_history_and_undo() {
    let env = env();
    let root = env.project(
        "p",
        &[
            ("company.yaml", COMPANY.to_string()),
            ("seats/alpha.md", alpha("cloud")),
            ("seats/beta.md", beta("cloud")),
        ],
    );
    env.write_user_file(&accounts_yaml(&["cloud", "spare"]));
    let kit = Kit::new(FakeTerminal::new()).with_key("spare", CANARY);

    let ran = kit.run(&env, &root, &["remove", "spare"]).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    let ids: Vec<String> = loaded_accounts(&env).into_iter().map(|a| a.0).collect();
    assert_eq!(ids, vec!["cloud".to_string()]);
    assert_eq!(kit.key("spare").as_deref(), Some(CANARY), "the key stays");
    assert!(ran.all().contains("Keychain"), "it says so: {}", ran.all());
    // True today: the change is recorded in History; Undo comes with the UI.
    assert!(ran.all().contains("recorded in History"), "{}", ran.all());
    assert!(ran.all().contains("when the UI ships"), "{}", ran.all());
    let history = env.history();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].kind, HistoryKind::Edit);
    assert_eq!(history[0].file, USER_FILE);
    assert_eq!(history[0].actor, owner());
    let audit = env.audit();
    let removed = audit_line_of_kind(&audit, "account_removed");
    assert_eq!(removed.len(), 1, "{audit}");
    assert!(has(&removed[0], "account", "spare"), "{}", removed[0]);
    assert!(
        env.project_history(&root).is_empty(),
        "the project store is not written"
    );

    // Story 2.14 (AD-15: "Undoing a remove re-adds an Account, which is
    // widening"; coordinator decision 2026-11-23): an Undo of the remove
    // through the user file's writer is refused, naming `agentic accounts
    // add`, and writes nothing; the Account is added again only by `add`.
    let text = env.user_text();
    let writer = env.user_writer();
    let err = writer.undo(owner(), &history[0].id).unwrap_err();
    drop(writer);
    assert!(err.to_string().contains("agentic accounts add"), "{err}");
    assert_eq!(env.user_text(), text, "nothing written");
    assert_eq!(env.history().len(), 1);
    let ids: Vec<String> = loaded_accounts(&env).into_iter().map(|a| a.0).collect();
    assert_eq!(ids, vec!["cloud".to_string()]);
    assert_no_key_leaked(&env, &root, &ran);
}

/// Remove, referenced (Remove in use): refused naming the project and the
/// seat lines that still name it; a reserved or unknown id is refused too;
/// nothing changes.
#[tokio::test]
async fn remove_refuses_a_referenced_reserved_or_unknown_id() {
    let env = env();
    let root = env.project(
        "p",
        &[
            ("company.yaml", COMPANY.to_string()),
            ("seats/alpha.md", alpha("cloud")),
            ("seats/beta.md", beta("cloud")),
        ],
    );
    env.write_user_file(&accounts_yaml(&["cloud", "spare"]));
    let before = env.user_text();

    let kit = Kit::new(FakeTerminal::new());
    let ran = kit.run(&env, &root, &["remove", "cloud"]).await;
    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(
        ran.all().contains("alpha") && ran.all().contains("beta"),
        "names the seats: {}",
        ran.all()
    );
    assert!(
        ran.all().contains(&root.display().to_string()),
        "names the project: {}",
        ran.all()
    );
    assert!(
        ran.all().contains("seats/alpha.md") && ran.all().contains("seats/beta.md"),
        "names the seat files: {}",
        ran.all()
    );

    for id in ["local-ollama", "local-lmstudio", "nope"] {
        let ran = kit.run(&env, &root, &["remove", id]).await;
        assert_ne!(ran.code, 0, "{id}: {}", ran.all());
        assert!(ran.all().contains(id), "{id}: {}", ran.all());
    }
    assert_eq!(env.user_text(), before);
    assert!(env.history().is_empty());
}

// ------------------------------------------------------------------ open

/// Story 2.12: the commands open no project. While another process holds
/// a project's store (the app is serving it), an add and a remove still
/// run, and the holder keeps its lock: the user store has its own lock,
/// `user.lock`, taken only for the write.
#[tokio::test]
async fn add_and_remove_run_while_a_project_store_is_held() {
    let env = env();
    let root = env.usual(&[]);
    let (held, _) = am_store::open_runtime(&env.layout(&root), "Holder Mac".into()).unwrap();
    let kit = Kit::new(FakeTerminal::new());
    kit.term.line("cloud2");
    kit.term.line(CANARY);

    let ran = kit
        .run(&env, &root, &["add", "cloud2", "--provider", "anthropic"])
        .await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(!ran.all().contains("Holder Mac"), "{}", ran.all());
    assert_eq!(kit.key("cloud2").as_deref(), Some(CANARY));

    // A second Account nobody names is removed the same way.
    let other = Kit::new(FakeTerminal::new());
    other.term.line("spare");
    other.term.line(CANARY);
    let ran = other
        .run(&env, &root, &["add", "spare", "--provider", "anthropic"])
        .await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    let ran = Kit::new(FakeTerminal::new())
        .run(&env, &root, &["remove", "spare"])
        .await;
    assert_eq!(ran.code, 0, "{}", ran.all());

    assert!(
        am_store::lock_is_held(&env.layout(&root)),
        "the holder keeps it"
    );
    assert_eq!(env.history().len(), 3);
    drop(held);
    assert!(env.project_history(&root).is_empty());
}

/// Busy: an outside change not yet reloaded (a tracked file that differs
/// from its last History bytes) refuses the next add or remove, naming the
/// file, before anything is read or stored.
#[tokio::test]
async fn an_outside_change_waiting_for_reload_refuses_naming_the_file() {
    let env = env();
    let root = env.usual(&[]);
    let first = Kit::new(FakeTerminal::new());
    first.term.line("cloud2");
    first.term.line(CANARY);
    let ran = first
        .run(&env, &root, &["add", "cloud2", "--provider", "anthropic"])
        .await;
    assert_eq!(ran.code, 0, "{}", ran.all());

    // Changed by hand afterwards.
    let changed = format!(
        "{}  - id: spare\n    provider: anthropic\n    secret_ref: spare\n",
        env.user_text().unwrap()
    );
    std::fs::write(env.accounts_path(), &changed).unwrap();

    for args in [
        vec!["add", "cloud3", "--provider", "anthropic"],
        vec!["remove", "spare"],
    ] {
        let kit = Kit::new(FakeTerminal::new());
        kit.term.line("cloud3");
        kit.term.line(CANARY);
        let ran = kit.run(&env, &root, &args).await;
        assert_ne!(ran.code, 0, "{args:?}: {}", ran.all());
        assert!(ran.all().contains(USER_FILE), "{args:?}: {}", ran.all());
        assert_eq!(kit.term.reads(), 0, "{args:?}");
        assert_eq!(kit.key("cloud3"), None);
    }
    assert_eq!(env.user_text().unwrap(), changed);
    assert_eq!(env.history().len(), 1);
}

/// While config enables a stdio MCP server (a child that runs as the owner)
/// the add is refused in words, before anything is read.
#[tokio::test]
async fn add_is_refused_while_a_stdio_mcp_server_is_enabled() {
    let env = env();
    let root = env.usual(&[("tools.yaml", STDIO_TOOLS.to_string())]);
    let kit = Kit::new(FakeTerminal::new());
    kit.term.line("cloud2");
    kit.term.line(CANARY);

    let ran = kit
        .run(&env, &root, &["add", "cloud2", "--provider", "anthropic"])
        .await;

    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(ran.all().contains("stdio MCP server"), "{}", ran.all());
    assert!(
        ran.all().contains(&root.display().to_string()),
        "names the project: {}",
        ran.all()
    );
    assert_eq!(kit.term.reads(), 0);
    assert_eq!(kit.key("cloud2"), None);
    assert!(!env.accounts_path().exists());
}

/// Story 2.12: the commands open no project, so none is resolved: a folder
/// never opened on this Mac (no `project_id`), a project with an unmoved
/// legacy folder, and a project whose store an older binary wrote are all
/// left exactly as they were by an add run from inside them: no id minted,
/// no folder moved, no store migrated, no marker written.
#[tokio::test]
async fn add_resolves_no_project_and_changes_none() {
    let env = env();

    // Never opened: no project_id yet; the add is run from inside it.
    let fresh = env.folder(
        "fresh",
        &[
            ("company.yaml", COMPANY.to_string()),
            ("seats/alpha.md", alpha("cloud2")),
        ],
    );
    let kit = Kit::new(FakeTerminal::new());
    kit.term.line("cloud2");
    kit.term.line(CANARY);
    let ran = kit
        .run(&env, &fresh, &["add", "cloud2", "--provider", "anthropic"])
        .await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert_eq!(
        read(&fresh, "company.yaml").unwrap(),
        COMPANY,
        "no id minted"
    );
    assert!(!open_marker(&fresh).exists(), "no marker is written");

    // An unmoved legacy folder, and a store at an older version.
    let root = env.usual(&[]);
    std::fs::create_dir(legacy_dir(&root)).unwrap();
    let db = env.layout(&root).state_db();
    std::fs::remove_file(&db).unwrap();
    am_store::testing::database_at_version(&db, SCHEMA_VERSION - 1).unwrap();
    let db_before = std::fs::read(&db).unwrap();
    let kit = Kit::new(FakeTerminal::new());
    kit.term.line("cloud3");
    kit.term.line(CANARY);
    let ran = kit
        .run(&env, &root, &["add", "cloud3", "--provider", "anthropic"])
        .await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(legacy_dir(&root).is_dir(), "not moved");
    assert_eq!(std::fs::read(&db).unwrap(), db_before, "not migrated");
    assert!(!open_marker(&root).exists(), "no marker is written");
    let ids: Vec<String> = loaded_accounts(&env).into_iter().map(|a| a.0).collect();
    assert_eq!(ids, ["cloud2", "cloud3"]);
}

#[tokio::test]
async fn bad_usage_exits_two() {
    let env = env();
    let root = env.usual(&[]);
    for args in [
        vec![],
        vec!["frobnicate"],
        vec!["add"],
        vec!["add", "cloud2", "--provider"],
        vec!["remove"],
        vec!["test"],
        vec!["list", "extra"],
        // Story 2.12: no command takes a project.
        vec!["list", "--project", "/tmp"],
        vec![
            "add",
            "cloud2",
            "--provider",
            "anthropic",
            "--project",
            "/tmp",
        ],
        vec!["remove", "cloud2", "--project", "/tmp"],
        vec!["test", "cloud2", "--project", "/tmp"],
    ] {
        let kit = Kit::new(FakeTerminal::new());
        let ran = kit.run(&env, &root, &args).await;
        assert_eq!(ran.code, 2, "{args:?}: {}", ran.all());
        assert!(
            ran.err.contains("usage: agentic accounts"),
            "{args:?}: {}",
            ran.err
        );
        assert_eq!(kit.term.reads(), 0);
    }
}

// ------------------------------------------------- review fixes (story 2.1)

/// A secret store that counts its reads.
struct CountingStore {
    inner: Arc<FakeSecretStore>,
    gets: AtomicUsize,
}

impl SecretStore for CountingStore {
    fn get(
        &self,
        account: &str,
    ) -> Result<Option<am_domain::SecretString>, am_domain::SecretError> {
        self.gets.fetch_add(1, Ordering::SeqCst);
        self.inner.get(account)
    }
    fn set(
        &self,
        account: &str,
        secret: am_domain::SecretString,
    ) -> Result<(), am_domain::SecretError> {
        self.inner.set(account, secret)
    }
    fn delete(&self, account: &str) -> Result<(), am_domain::SecretError> {
        self.inner.delete(account)
    }
}

/// The file changes while the add waits at its prompt (another program
/// adds an Account): the edit was built on the earlier bytes, so the add is
/// refused, saying to run it again; the Account that appeared stays, the
/// key is not stored and nothing is recorded.
#[tokio::test]
async fn an_account_added_to_the_file_while_add_waits_is_never_overwritten() {
    for start in [
        accounts_yaml(&["cloud"]),
        // `accounts:` absent: the add would set the whole list.
        "schema_version: 2\n".to_string(),
    ] {
        let env = env();
        let root = env.project(
            "p",
            &[
                ("company.yaml", COMPANY.to_string()),
                ("seats/alpha.md", alpha("cloud")),
            ],
        );
        env.write_user_file(&start);
        let appeared = accounts_yaml(&["cloud", "other"]);
        let kit = Kit::new(FakeTerminal::new());
        let file = env.accounts_path();
        let bytes = appeared.clone();
        kit.term.line_and("cloud2", move || {
            std::fs::write(&file, &bytes).unwrap();
        });
        kit.term.line(CANARY);

        let ran = kit
            .run(&env, &root, &["add", "cloud2", "--provider", "anthropic"])
            .await;

        assert_ne!(ran.code, 0, "{}", ran.all());
        assert!(ran.all().contains("again"), "run it again: {}", ran.all());
        assert_eq!(
            env.user_text().unwrap(),
            appeared,
            "the Account that appeared stays"
        );
        assert_eq!(kit.key("cloud2"), None, "no key is stored");
        assert!(env.history().is_empty());
        assert!(audit_line_of_kind(&env.audit(), "account_added").is_empty());
        assert_no_key_leaked(&env, &root, &ran);
    }
}

/// A seat starts naming the id while the add waits: the sentence the owner
/// confirmed no longer says what the add brings into force, so it is
/// refused, saying to run it again, with nothing stored or written.
#[tokio::test]
async fn a_seat_that_starts_naming_the_account_while_add_waits_refuses_the_add() {
    let env = env();
    let root = env.project(
        "p",
        &[
            ("company.yaml", COMPANY.to_string()),
            ("seats/alpha.md", alpha("cloud2")),
        ],
    );
    let kit = Kit::new(FakeTerminal::new());
    let seat = root.join("seats/beta.md");
    kit.term.line_and("cloud2", move || {
        std::fs::write(&seat, beta("cloud2")).unwrap();
    });
    kit.term.line(CANARY);

    let ran = kit
        .run(&env, &root, &["add", "cloud2", "--provider", "anthropic"])
        .await;

    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(ran.all().contains("again"), "run it again: {}", ran.all());
    assert!(
        ran.all().contains("beta"),
        "names what changed: {}",
        ran.all()
    );
    assert_eq!(kit.key("cloud2"), None);
    assert!(!env.accounts_path().exists());
    assert!(env.history().is_empty());
}

/// AD-17 names a key's Keychain entry by the Account id. A hand-written
/// `secret_ref` that names another entry is never followed: `list` says so
/// and reads no key for that Account, and `test` refuses before any
/// Keychain read or request, so no other entry's key is shown or sent.
#[tokio::test]
async fn a_secret_ref_that_names_another_entry_is_never_read_or_sent() {
    const OTHER: &str = "sk-other-entry-canary-9z8y7x-QRST";
    let env = env();
    let yaml = "schema_version: 2\naccounts:\n  - id: cloud\n    provider: anthropic\n    secret_ref: other-entry\n";
    let root = env.usual(&[]);
    env.write_user_file(yaml);
    let kit = Kit::new(FakeTerminal::piped()).with_key("other-entry", OTHER);
    kit.egress
        .replay("api.anthropic.com", "anthropic/c1_count_stream.sse");
    let counting = Arc::new(CountingStore {
        inner: kit.secrets.clone(),
        gets: AtomicUsize::new(0),
    });

    let ran = kit.run_with(&env, &root, &["list"], counting.clone()).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(
        ran.out.contains("secret_ref names another entry; not used"),
        "{}",
        ran.out
    );
    assert!(
        !ran.all().contains("QRST"),
        "another entry's key: {}",
        ran.all()
    );
    assert!(!ran.all().contains('•'), "no key is shown: {}", ran.all());
    assert_eq!(counting.gets.load(Ordering::SeqCst), 0, "no key is read");

    let ran = kit
        .run_with(&env, &root, &["test", "cloud"], counting.clone())
        .await;
    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(ran.all().contains("secret_ref"), "{}", ran.all());
    assert_eq!(
        counting.gets.load(Ordering::SeqCst),
        0,
        "refused before any Keychain read"
    );
    assert!(kit.egress.requests().is_empty(), "nothing sent");
    assert!(!ran.all().contains("QRST"));
}

/// Story 2.12: taking `user.lock` settles a write of the user file a crash
/// interrupted (a pending intent whose file holds its new bytes is
/// completed as reconciled), and the command reports it; the add then
/// proceeds on the settled file, and no refusal or report claims that
/// nothing was written.
#[tokio::test]
async fn an_add_settles_and_reports_an_interrupted_write_of_the_user_file() {
    let env = env();
    let root = env.usual(&[]);
    {
        // A create interrupted after the file was placed: its intent stays.
        let writer = env.user_writer();
        writer.fail_at(Some(am_config::FailPoint::AfterRename));
        writer
            .create(
                owner(),
                am_config::ConfigCreate {
                    file: USER_FILE.into(),
                    bytes: accounts_yaml(&["cloud"]).into_bytes(),
                    audit: Vec::new(),
                },
            )
            .unwrap_err();
    }
    assert!(env.history().is_empty(), "the entry is still pending");
    assert!(env.accounts_path().exists(), "the file was placed");

    let kit = Kit::new(FakeTerminal::new());
    kit.term.line("cloud2");
    kit.term.line(CANARY);
    let ran = kit
        .run(&env, &root, &["add", "cloud2", "--provider", "anthropic"])
        .await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(
        ran.all()
            .contains(&format!("completed the interrupted write of {USER_FILE}")),
        "the reconcile is reported: {}",
        ran.all()
    );
    let history = env.history();
    assert_eq!(history.len(), 2, "{history:#?}");
    assert_eq!(history[0].kind, HistoryKind::Create);
    assert_eq!(history[1].kind, HistoryKind::Edit);
    let ids: Vec<String> = loaded_accounts(&env).into_iter().map(|a| a.0).collect();
    assert_eq!(ids, ["cloud", "cloud2"]);
}

/// Ctrl-C after the id was typed, on the path that reuses the Keychain's
/// entry (story 2.12: the stored key's hidden prompt comes next): the add
/// is cancelled, echo is on again, and nothing is written or stored.
#[tokio::test]
async fn ctrl_c_after_the_id_on_the_reuse_path_adds_nothing() {
    let env = env();
    let root = env.usual(&[]);
    let kit = Kit::new(FakeTerminal::new()).with_key("cloud2", "sk-ant-earlier-canary-1111");
    kit.term.line("cloud2");
    kit.term.interrupt();

    let ran = kit
        .run(&env, &root, &["add", "cloud2", "--provider", "anthropic"])
        .await;

    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(kit.term.echo(), "echo is on again");
    assert!(!env.accounts_path().exists(), "nothing written");
    assert!(env.history().is_empty());
    assert_eq!(
        kit.key("cloud2").as_deref(),
        Some("sk-ant-earlier-canary-1111"),
        "the entry that was there stays"
    );
}

// ------------------------------------------- second review (story 2.1)

/// A secret store that runs something once, right before a `set` reaches
/// the store (another program acting in that gap).
struct BeforeSet {
    inner: Arc<FakeSecretStore>,
    meanwhile: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

impl SecretStore for BeforeSet {
    fn get(
        &self,
        account: &str,
    ) -> Result<Option<am_domain::SecretString>, am_domain::SecretError> {
        self.inner.get(account)
    }
    fn set(
        &self,
        account: &str,
        secret: am_domain::SecretString,
    ) -> Result<(), am_domain::SecretError> {
        if let Some(meanwhile) = self.meanwhile.lock().unwrap().take() {
            meanwhile();
        }
        self.inner.set(account, secret)
    }
    fn delete(&self, account: &str) -> Result<(), am_domain::SecretError> {
        self.inner.delete(account)
    }
}

/// The Keychain entry of the id changes while a prompt waits (another
/// process stores one during the id prompt or the key prompt, or deletes
/// the one the sentence said would be reused): the sentence the owner
/// confirmed no longer says which key goes to the endpoint, so the add is
/// refused, to be run again. Nothing is written or recorded, the typed key
/// is not stored, and an entry this command did not create is never
/// overwritten or deleted. (Rewritten in story 2.3's second review: story
/// 2.1 kept and used an entry that appeared during the key prompt.)
#[tokio::test]
async fn a_keychain_entry_that_changes_while_a_prompt_waits_refuses_the_add() {
    const THEIRS: &str = "sk-ant-stored-by-another-process-7777";
    let refused = |ran: &Ran, when: &str| {
        assert_ne!(ran.code, 0, "{when}: {}", ran.all());
        for words in [
            "the Keychain entry for 'cloud2' changed while you typed",
            "run the command again",
        ] {
            assert!(
                ran.all().contains(words),
                "{when}: it says \"{words}\": {}",
                ran.all()
            );
        }
        for key in [THEIRS, CANARY] {
            assert!(!ran.all().contains(key), "{when}: a key was printed");
        }
    };
    let nothing_written = |env: &Env, _root: &Path, when: &str| {
        assert!(!env.accounts_path().exists(), "{when}: no file");
        assert!(env.history().is_empty(), "{when}: no History entry");
        assert!(
            audit_line_of_kind(&env.audit(), "account_added").is_empty(),
            "{when}: no audit note"
        );
    };
    let args = ["add", "cloud2", "--provider", "anthropic"];

    // An entry appears while the id prompt waits: the sentence said no key
    // was held, so the entry is not reused and no key is asked for.
    let env = env();
    let root = env.usual(&[]);
    let kit = Kit::new(FakeTerminal::new());
    let store = kit.secrets.clone();
    kit.term.line_and("cloud2", move || {
        store
            .set("cloud2", am_domain::SecretString::new(THEIRS.into()))
            .unwrap();
    });
    kit.term.line(CANARY);
    let ran = kit.run(&env, &root, &args).await;
    refused(&ran, "during the id prompt");
    assert!(
        !ran.shown.contains("already holds"),
        "the sentence said no key was held: {}",
        ran.shown
    );
    assert_eq!(kit.term.reads(), 1, "the key is not asked for");
    assert_eq!(
        kit.key("cloud2").as_deref(),
        Some(THEIRS),
        "the entry that appeared is neither overwritten nor deleted"
    );
    nothing_written(&env, &root, "during the id prompt");

    // An entry appears while the key prompt waits: the typed key is
    // discarded and the entry stays as it is.
    let env = self::env();
    let root = env.usual(&[]);
    let kit = Kit::new(FakeTerminal::new());
    kit.term.line("cloud2");
    let store = kit.secrets.clone();
    kit.term.line_and(CANARY, move || {
        store
            .set("cloud2", am_domain::SecretString::new(THEIRS.into()))
            .unwrap();
    });
    let ran = kit.run(&env, &root, &args).await;
    refused(&ran, "during the key prompt");
    assert_eq!(kit.term.reads(), 2, "the id, then the key");
    assert_eq!(
        kit.key("cloud2").as_deref(),
        Some(THEIRS),
        "the entry that appeared is neither overwritten nor deleted"
    );
    nothing_written(&env, &root, "during the key prompt");

    // The entry the sentence said would be reused is deleted while the id
    // prompt waits: no new key is stored in its place (story 2.12: the
    // stored key's hidden prompt may come first; what is typed there is
    // never stored).
    let env = self::env();
    let root = env.usual(&[]);
    let kit = Kit::new(FakeTerminal::new()).with_key("cloud2", THEIRS);
    let store = kit.secrets.clone();
    kit.term.line_and("cloud2", move || {
        store.delete("cloud2").unwrap();
    });
    kit.term.line(CANARY);
    let ran = kit.run(&env, &root, &args).await;
    refused(&ran, "deleted during the id prompt");
    assert!(ran.shown.contains("already holds"), "{}", ran.shown);
    assert_eq!(kit.key("cloud2"), None, "nothing is stored");
    nothing_written(&env, &root, "deleted during the id prompt");
}

/// The file gains the same id from outside in the gap between the last
/// check and the write: the writer refuses (the file no longer holds the
/// bytes the edit was built on). That the id is now in the file does not
/// mean this command wrote it: nothing claims a pending History entry, and
/// the key this command stored is deleted.
#[tokio::test]
async fn the_same_id_added_from_outside_before_the_write_is_not_taken_for_this_write() {
    let env = env();
    let root = env.project("p", &[("company.yaml", COMPANY.to_string())]);
    env.write_user_file(&accounts_yaml(&["cloud"]));
    let outside = accounts_yaml(&["cloud", "cloud2"]);
    let kit = Kit::new(FakeTerminal::new());
    kit.term.line("cloud2");
    kit.term.line(CANARY);
    let (file, bytes) = (env.accounts_path(), outside.clone());
    let secrets = Arc::new(BeforeSet {
        inner: kit.secrets.clone(),
        meanwhile: Mutex::new(Some(Box::new(move || {
            std::fs::write(&file, &bytes).unwrap();
        }))),
    });

    let ran = kit
        .run_with(
            &env,
            &root,
            &["add", "cloud2", "--provider", "anthropic"],
            secrets,
        )
        .await;

    assert_ne!(ran.code, 0, "{}", ran.all());
    assert_eq!(env.user_text().unwrap(), outside);
    assert!(
        !ran.all().contains("History entry is recorded"),
        "no pending History entry is claimed: {}",
        ran.all()
    );
    assert_eq!(
        kit.key("cloud2"),
        None,
        "the key this command stored is deleted"
    );
    assert!(env.history().is_empty());
    assert!(audit_line_of_kind(&env.audit(), "account_added").is_empty());
}

/// Ctrl-C between the store and the write (the second check): the key this
/// add just stored is deleted and no Account is written; exit 130.
#[tokio::test]
async fn ctrl_c_between_the_store_and_the_write_leaves_no_key_and_no_account() {
    let env = env();
    let root = env.usual(&[]);
    let kit = Kit::new(FakeTerminal::new());
    kit.term.line("cloud2");
    kit.term.line(CANARY);
    kit.term.interrupt_on_check(2);

    let ran = kit
        .run(&env, &root, &["add", "cloud2", "--provider", "anthropic"])
        .await;

    assert_eq!(ran.code, 130, "{}", ran.all());
    assert_eq!(kit.term.reads(), 2, "the id and the key were read");
    assert_eq!(kit.key("cloud2"), None, "the key it stored is deleted");
    assert!(!env.accounts_path().exists(), "no Account written");
    assert!(env.history().is_empty());
    assert_no_key_leaked(&env, &root, &ran);
}

/// A remove whose write fails after the file was replaced (the folder
/// sync, the History commit): the Account is already gone from the file,
/// so the command says it was removed and that its History entry is
/// recorded the next time the project opens, not that it was not removed.
#[tokio::test]
async fn a_remove_that_fails_after_the_file_was_written_says_it_was_removed() {
    let env = env();
    let root = env.project("p", &[("company.yaml", COMPANY.to_string())]);
    env.write_user_file(&accounts_yaml(&["cloud", "spare"]));
    // Story 2.12: the user file's writer has the app data dir as its root.
    let app_data = env.places.user.dir().to_path_buf();
    am_app::runtime::testing::fail_writer_at(&app_data, Some(am_config::FailPoint::AfterRename));
    let kit = Kit::new(FakeTerminal::new());

    let ran = kit.run(&env, &root, &["remove", "spare"]).await;

    am_app::runtime::testing::fail_writer_at(&app_data, None);
    assert_ne!(ran.code, 0, "{}", ran.all());
    let ids: Vec<String> = loaded_accounts(&env).into_iter().map(|a| a.0).collect();
    assert_eq!(ids, vec!["cloud".to_string()], "the file was written");
    assert!(
        !ran.all().contains("was not removed"),
        "it is gone from the file: {}",
        ran.all()
    );
    assert!(ran.all().contains("removed from"), "{}", ran.all());
    assert!(
        ran.all().contains("History"),
        "says its History entry is recorded later: {}",
        ran.all()
    );
    assert!(env.history().is_empty(), "the entry is still pending");

    // The next write's lock settles it (an add of another Account).
    let next = Kit::new(FakeTerminal::new());
    next.term.line("cloud3");
    next.term.line(CANARY);
    let ran = next
        .run(&env, &root, &["add", "cloud3", "--provider", "anthropic"])
        .await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(
        ran.all().contains("completed the interrupted write"),
        "{}",
        ran.all()
    );
    assert_eq!(env.history().len(), 2);
}

// ------------------------------- OpenAI and compatible Accounts (story 2.3)

/// The key typed for an OpenAI Account, and the keys of the compatible ones.
const OA_CANARY: &str = "sk-proj-canary-9f8e7d6c5b4a-OAOA";
const ROUTER_CANARY: &str = "sk-or-canary-1a2b3c4d5e6f-RTRT";
const LLAMA_CANARY: &str = "any-text-canary-for-a-local-server-LLMA";

const OPENAI_URL: &str = "https://api.openai.com/v1";
const REMOTE_URL: &str = "https://openrouter.example/api/v1";
const LOOPBACK_URL: &str = "http://127.0.0.1:8080/v1";

/// `models/accounts.yaml` with one Account of each kind: Anthropic, OpenAI,
/// a compatible endpoint that is remote and one on this Mac.
fn four_kinds() -> String {
    format!(
        "{}  - id: oa\n    provider: openai\n    secret_ref: oa\n  - id: router\n    provider: openai-compatible\n    base_url: {REMOTE_URL}\n    secret_ref: router\n  - id: llama\n    provider: openai-compatible\n    base_url: {LOOPBACK_URL}\n    secret_ref: llama\n",
        accounts_yaml(&["cloud"])
    )
}

/// Adds an Account through the command, as the owner does: on an
/// interactive terminal, the id typed to confirm, then the key. `test`
/// calls only an Account added this way (its endpoint and provider are in
/// the `account_added` audit note, and the file is as the app wrote it).
async fn add_account(
    env: &Env,
    root: &Path,
    id: &str,
    provider: &str,
    base_url: Option<&str>,
    key: &str,
) {
    let mut args = vec!["add", id, "--provider", provider];
    if let Some(url) = base_url {
        args.extend(["--base-url", url]);
    }
    let kit = Kit::new(FakeTerminal::new());
    kit.term.line(id);
    kit.term.line(key);
    let ran = kit.run(env, root, &args).await;
    assert_eq!(ran.code, 0, "add {id}: {}", ran.all());
}

/// A valid project that names no cloud Account yet (its one seat runs on
/// the reserved local Accounts), opened once on this Mac: any Account can be
/// added to it, and removed again.
fn plain(env: &Env) -> PathBuf {
    env.project(
        "p",
        &[
            ("company.yaml", COMPANY.to_string()),
            ("seats/alpha.md", alpha("local-ollama")),
        ],
    )
}

/// The plain project with one Account of each kind, each added through the
/// command: `cloud` (Anthropic), `oa` (OpenAI), `router` (a compatible
/// endpoint that is remote) and `llama` (one on this Mac).
async fn four_kinds_added(env: &Env) -> PathBuf {
    let root = plain(env);
    add_account(env, &root, "cloud", "anthropic", None, CANARY).await;
    add_account(env, &root, "oa", "openai", None, OA_CANARY).await;
    add_account(
        env,
        &root,
        "router",
        "openai-compatible",
        Some(REMOTE_URL),
        ROUTER_CANARY,
    )
    .await;
    add_account(
        env,
        &root,
        "llama",
        "openai-compatible",
        Some(LOOPBACK_URL),
        LLAMA_CANARY,
    )
    .await;
    root
}

/// A `test` that must be refused before anything is read or sent: run with
/// a Keychain that holds a key for `id` and counts its reads, and an egress
/// that would answer any host. Returns what was printed, after checking
/// that the command failed, read no key, asked for no egress policy and
/// sent nothing.
async fn refused_test(env: &Env, root: &Path, id: &str) -> Ran {
    let kit = Kit::new(FakeTerminal::piped()).with_key(id, ROUTER_CANARY);
    kit.egress.replay("", "openai_compat/reasoning.sse");
    let counting = Arc::new(CountingStore {
        inner: kit.secrets.clone(),
        gets: AtomicUsize::new(0),
    });
    let ran = kit
        .run_with(env, root, &["test", id, "--model", "m"], counting.clone())
        .await;
    assert_ne!(ran.code, 0, "{id}: {}", ran.all());
    assert_eq!(
        counting.gets.load(Ordering::SeqCst),
        0,
        "{id}: refused before any Keychain read: {}",
        ran.all()
    );
    assert!(
        kit.egress.requests().is_empty(),
        "{id}: nothing sent: {}",
        ran.all()
    );
    assert!(
        kit.directories().is_empty(),
        "{id}: no egress client is built"
    );
    assert!(!ran.all().contains(ROUTER_CANARY), "{id}: {}", ran.all());
    ran
}

/// What the refusal of an Account that `agentic accounts add` did not
/// confirm says: that, that no key was read or sent, and what to do.
fn assert_says_not_confirmed(ran: &Ran, id: &str) {
    let said = ran.all();
    for words in [
        "not confirmed by `agentic accounts add` on this Mac",
        "No key was read or sent",
        "agentic accounts remove",
    ] {
        assert!(said.contains(words), "{id}: it says \"{words}\": {said}");
    }
    assert!(said.contains(id), "{id}: {said}");
}

/// One Account as the user Accounts file holds it: (id, provider, name,
/// base_url, secret_ref).
type Held = (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn held(id: &str, provider: &str, name: Option<&str>, base_url: Option<&str>) -> Held {
    (
        id.to_string(),
        provider.to_string(),
        name.map(str::to_string),
        base_url.map(str::to_string),
        Some(id.to_string()),
    )
}

/// The Accounts of the user file now. It must load without an error: what
/// the add wrote is valid config.
fn held_accounts(env: &Env) -> Vec<Held> {
    let loaded = am_config::user_accounts::load(&env.accounts_path());
    assert!(
        loaded
            .findings
            .iter()
            .all(|f| f.severity != am_config::Severity::Error),
        "{:#?}",
        loaded.findings
    );
    loaded
        .accounts
        .ids()
        .iter()
        .map(|id| {
            let a = loaded.accounts.get(id).unwrap();
            (
                a.id.clone(),
                a.provider.clone(),
                a.name.clone(),
                a.base_url.clone(),
                a.secret_ref.clone(),
            )
        })
        .collect()
}

/// The one `account_added` audit line, checked for what every add records
/// (AD-15) and for the endpoint's scheme, host and port.
fn assert_added_audit(
    env: &Env,
    _root: &Path,
    account: &str,
    provider: &str,
    scheme: &str,
    host: &str,
    port: &str,
) {
    let audit = env.audit();
    let added = audit_line_of_kind(&audit, "account_added");
    assert_eq!(added.len(), 1, "{audit}");
    let line = &added[0];
    assert!(has(line, "file", USER_FILE), "{line}");
    assert!(has(line, "account", account), "{line}");
    assert!(has(line, "provider", provider), "{line}");
    assert!(has(line, "scheme", scheme), "{line}");
    assert!(has(line, "host", host), "{line}");
    assert!(
        has(line, "port", port) || line.contains(&format!("\"port\":{port}")),
        "port {port}: {line}"
    );
    assert!(has(line, "key", "stored"), "{line}");
    assert!(has(line, "confirmation", "typed_id"), "{line}");
}

/// Add OpenAI: the sentence names the Account, its provider and
/// `https://api.openai.com/v1`; the id is typed within 2 minutes and the key
/// is read with echo off, as for every add; the file gains the Account with
/// `secret_ref` and no `base_url`; the audit entry carries `https`,
/// `api.openai.com` and 443; the key is only in the (fake) Keychain; no
/// seat file changes.
#[tokio::test]
async fn add_an_openai_account_names_its_endpoint_and_records_it() {
    let env = env();
    let root = env.project(
        "p",
        &[
            ("company.yaml", COMPANY.to_string()),
            ("seats/alpha.md", alpha("oa")),
        ],
    );
    let seat = read(&root, "seats/alpha.md");
    let kit = Kit::new(FakeTerminal::new());
    kit.term.line("oa");
    kit.term.line(OA_CANARY);

    let ran = kit
        .run(&env, &root, &["add", "oa", "--provider", "openai"])
        .await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    for word in ["'oa'", "openai", OPENAI_URL, "alpha", "Keychain"] {
        assert!(
            ran.shown.contains(word),
            "the sentence names {word}: {}",
            ran.shown
        );
    }
    assert!(
        !ran.shown.contains("stay on this Mac"),
        "a cloud endpoint is not on this Mac: {}",
        ran.shown
    );
    assert_eq!(
        kit.term.echo_reads(),
        vec![true, false],
        "id echoed, key not"
    );
    assert!(kit.term.echo(), "echo is on again");
    assert_eq!(
        kit.term.timeouts().first().copied().flatten(),
        Some(Duration::from_secs(120)),
        "the id is asked within 2 minutes"
    );
    assert_eq!(kit.key("oa").as_deref(), Some(OA_CANARY));

    assert_eq!(held_accounts(&env), vec![held("oa", "openai", None, None)]);
    let text = env.user_text().unwrap();
    assert!(!text.contains("base_url"), "{text}");
    let history = env.history();
    assert_eq!(history.len(), 1, "{history:#?}");
    assert_eq!(history[0].file, USER_FILE);
    assert_eq!(history[0].kind, HistoryKind::Create);
    assert_eq!(history[0].actor, owner());
    assert_added_audit(
        &env,
        &root,
        "oa",
        "openai",
        "https",
        "api.openai.com",
        "443",
    );

    assert_eq!(read(&root, "seats/alpha.md"), seat, "no seat is changed");
    assert!(!open_marker(&root).exists(), "no marker is written");
    for leak in [&ran.all(), &text, &env.audit()] {
        assert!(!leak.contains(OA_CANARY), "the key left the Keychain");
    }
    for e in env.history() {
        assert!(
            !format!("{e:?}").contains(OA_CANARY),
            "the key reached History"
        );
        assert!(!String::from_utf8_lossy(&e.after).contains(OA_CANARY));
    }
}

/// Add compatible, remote: `--base-url https://<host>/<path>` is named in
/// the sentence as typed; the Account is appended to the existing file with
/// its `base_url` and `secret_ref` (an `edit`); the audit entry carries
/// `https`, the host and 443.
#[tokio::test]
async fn add_a_compatible_account_at_a_remote_url_pins_that_host() {
    let env = env();
    let root = env.project(
        "p",
        &[
            ("company.yaml", COMPANY.to_string()),
            ("seats/alpha.md", alpha("router")),
            ("seats/beta.md", beta("cloud")),
        ],
    );
    env.write_user_file(&accounts_yaml(&["cloud"]));
    let seats = (read(&root, "seats/alpha.md"), read(&root, "seats/beta.md"));
    let kit = Kit::new(FakeTerminal::new());
    kit.term.line("router");
    kit.term.line(ROUTER_CANARY);

    let ran = kit
        .run(
            &env,
            &root,
            &[
                "add",
                "router",
                "--provider",
                "openai-compatible",
                "--base-url",
                REMOTE_URL,
                "--name",
                "Router",
            ],
        )
        .await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    for word in ["'router'", "openai-compatible", REMOTE_URL, "alpha"] {
        assert!(
            ran.shown.contains(word),
            "the sentence names {word}: {}",
            ran.shown
        );
    }
    assert!(
        !ran.shown.contains("stay on this Mac"),
        "a remote endpoint is not on this Mac: {}",
        ran.shown
    );
    assert_eq!(kit.term.echo_reads(), vec![true, false]);
    assert_eq!(kit.key("router").as_deref(), Some(ROUTER_CANARY));

    assert_eq!(
        held_accounts(&env),
        vec![
            held("cloud", "anthropic", None, None),
            held(
                "router",
                "openai-compatible",
                Some("Router"),
                Some(REMOTE_URL)
            ),
        ]
    );
    let history = env.history();
    assert_eq!(history.len(), 1, "{history:#?}");
    assert_eq!(history[0].kind, HistoryKind::Edit);
    assert_eq!(history[0].actor, owner());
    assert_added_audit(
        &env,
        &root,
        "router",
        "openai-compatible",
        "https",
        "openrouter.example",
        "443",
    );

    assert_eq!(
        (read(&root, "seats/alpha.md"), read(&root, "seats/beta.md")),
        seats,
        "no seat is changed"
    );
    let text = env.user_text().unwrap();
    for leak in [&ran.all(), &text, &env.audit()] {
        assert!(!leak.contains(ROUTER_CANARY), "the key left the Keychain");
    }
}

/// Add compatible, loopback: `--base-url http://127.0.0.1:<port>/<path>` is
/// accepted for a server on this Mac; the sentence says the calls stay on
/// this Mac, on that port; the new file holds the `base_url`; the audit
/// entry carries `http`, `127.0.0.1` and the port. A key is read and stored
/// as for every Account (there is no keyless Account).
#[tokio::test]
async fn add_a_compatible_account_on_this_mac_says_its_calls_stay_here() {
    let env = env();
    let root = env.project(
        "p",
        &[
            ("company.yaml", COMPANY.to_string()),
            ("seats/alpha.md", alpha("llama")),
        ],
    );
    let kit = Kit::new(FakeTerminal::new());
    kit.term.line("llama");
    kit.term.line(LLAMA_CANARY);

    let ran = kit
        .run(
            &env,
            &root,
            &[
                "add",
                "llama",
                "--provider",
                "openai-compatible",
                "--base-url",
                LOOPBACK_URL,
            ],
        )
        .await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    for word in [
        "'llama'",
        "openai-compatible",
        LOOPBACK_URL,
        "stay on this Mac",
        "port 8080",
    ] {
        assert!(
            ran.shown.contains(word),
            "the sentence says {word}: {}",
            ran.shown
        );
    }
    assert_eq!(
        kit.term.echo_reads(),
        vec![true, false],
        "the key is read, echo off"
    );
    assert_eq!(kit.key("llama").as_deref(), Some(LLAMA_CANARY));

    assert_eq!(
        held_accounts(&env),
        vec![held("llama", "openai-compatible", None, Some(LOOPBACK_URL))]
    );
    let history = env.history();
    assert_eq!(history.len(), 1, "{history:#?}");
    assert_eq!(history[0].kind, HistoryKind::Create);
    assert_added_audit(
        &env,
        &root,
        "llama",
        "openai-compatible",
        "http",
        "127.0.0.1",
        "8080",
    );
    let text = env.user_text().unwrap();
    for leak in [&ran.all(), &text, &env.audit()] {
        assert!(!leak.contains(LLAMA_CANARY), "the key left the Keychain");
    }
}

/// Refused URLs: a compatible Account without `--base-url`, a `--base-url`
/// outside the two accepted forms (`https://<DNS name>[/path]` on port 443,
/// `http://127.0.0.1:<port>[/path]`), and a `--base-url` for a provider
/// with one fixed endpoint. Each is refused in words that name `--base-url`
/// and the reason, before any prompt: nothing is shown on the terminal,
/// read, written or stored.
#[tokio::test]
async fn add_refuses_a_base_url_outside_the_two_accepted_forms() {
    const COMPATIBLE: &str = "openai-compatible";
    // (provider, --base-url, words of which the refusal holds at least one)
    let cases: [(&str, Option<&str>, &[&str]); 16] = [
        (COMPATIBLE, None, &["openai-compatible"]),
        // Plain http to anything but this Mac.
        (COMPATIBLE, Some("http://example.org"), &["https"]),
        // An IP literal: private, LAN, link-local (metadata) or public.
        (COMPATIBLE, Some("https://10.0.0.5/v1"), &["address"]),
        (
            COMPATIBLE,
            Some("http://192.168.1.4:8080"),
            &["address", "127.0.0.1"],
        ),
        (COMPATIBLE, Some("https://169.254.169.254/v1"), &["address"]),
        (COMPATIBLE, Some("https://203.0.113.9/v1"), &["address"]),
        // This Mac, but not as `http://127.0.0.1:<port>`.
        (COMPATIBLE, Some("http://localhost:8080"), &["127.0.0.1"]),
        (COMPATIBLE, Some("http://127.0.0.1/v1"), &["port"]),
        (
            COMPATIBLE,
            Some("https://127.0.0.1:8080/v1"),
            &["http://127.0.0.1", "this mac", "address"],
        ),
        (
            COMPATIBLE,
            Some("http://[::1]:8080/v1"),
            &["127.0.0.1", "address"],
        ),
        // A remote port other than 443, userinfo, a query, a fragment.
        (COMPATIBLE, Some("https://x.org:8443"), &["port"]),
        (
            COMPATIBLE,
            Some("https://someone:something@relay.example/v1"),
            &[
                "user name",
                "username",
                "password",
                "userinfo",
                "credential",
            ],
        ),
        (
            COMPATIBLE,
            Some("https://relay.example/v1?tier=a"),
            &["query"],
        ),
        (
            COMPATIBLE,
            Some("https://relay.example/v1#part"),
            &["fragment"],
        ),
        // One fixed endpoint: no `--base-url`.
        (
            "openai",
            Some(REMOTE_URL),
            &["api.openai.com", "openai-compatible"],
        ),
        (
            "anthropic",
            Some(REMOTE_URL),
            &["api.anthropic.com", "openai-compatible"],
        ),
    ];
    let env = env();
    let root = env.usual(&[]);
    for (provider, base_url, reasons) in cases {
        let mut args = vec!["add", "cloud2", "--provider", provider];
        if let Some(url) = base_url {
            args.extend(["--base-url", url]);
        }
        let kit = Kit::new(FakeTerminal::new());
        kit.term.line("cloud2");
        kit.term.line(CANARY);

        let ran = kit.run(&env, &root, &args).await;

        let case = format!("{provider} {base_url:?}");
        assert_ne!(ran.code, 0, "{case}: {}", ran.all());
        assert!(
            !ran.all().contains("unknown option"),
            "{case}: `--base-url` is an option of `accounts add`: {}",
            ran.all()
        );
        assert!(
            ran.all().contains("--base-url"),
            "{case}: the refusal names --base-url: {}",
            ran.all()
        );
        let said = ran.all().to_lowercase();
        assert!(
            reasons.iter().any(|word| said.contains(word)),
            "{case}: the refusal gives its reason (one of {reasons:?}): {}",
            ran.all()
        );
        assert!(
            ran.shown.is_empty(),
            "{case}: refused before any prompt: {}",
            ran.shown
        );
        assert_eq!(kit.term.reads(), 0, "{case}: nothing is read");
        assert_eq!(kit.key("cloud2"), None, "{case}: nothing is stored");
        assert!(!env.accounts_path().exists(), "{case}: nothing is written");
    }
    assert!(env.history().is_empty());
    assert!(audit_line_of_kind(&env.audit(), "account_added").is_empty());
}

/// The 2.1 terminal rule holds for the new providers: with a stdin that is
/// not a terminal an OpenAI or compatible add is refused naming the rule,
/// and nothing is read, written or stored.
#[tokio::test]
async fn add_of_an_openai_or_compatible_account_needs_a_terminal() {
    let env = env();
    let root = env.usual(&[]);
    for args in [
        vec!["add", "cloud2", "--provider", "openai"],
        vec![
            "add",
            "cloud2",
            "--provider",
            "openai-compatible",
            "--base-url",
            LOOPBACK_URL,
        ],
    ] {
        let kit = Kit::new(FakeTerminal::piped());
        kit.term.line("cloud2");
        kit.term.line(CANARY);
        let ran = kit.run(&env, &root, &args).await;
        assert_ne!(ran.code, 0, "{args:?}: {}", ran.all());
        assert!(ran.all().contains("terminal"), "{args:?}: {}", ran.all());
        assert_eq!(kit.term.reads(), 0, "{args:?}: nothing is read");
        assert_eq!(kit.key("cloud2"), None);
    }
    assert!(!env.accounts_path().exists());
    assert!(env.history().is_empty());
}

/// The usage text names the three providers and `--base-url`; a
/// `--base-url` with no value is bad usage (exit 2), not an unknown option.
#[tokio::test]
async fn usage_names_the_three_providers_and_the_base_url() {
    let env = env();
    let root = env.usual(&[]);
    let kit = Kit::new(FakeTerminal::new());

    let ran = kit.run(&env, &root, &["add"]).await;
    assert_eq!(ran.code, 2, "{}", ran.all());
    for word in ["anthropic", "openai", "openai-compatible", "--base-url"] {
        assert!(ran.err.contains(word), "usage names {word}: {}", ran.err);
    }

    let ran = kit
        .run(
            &env,
            &root,
            &[
                "add",
                "cloud2",
                "--provider",
                "openai-compatible",
                "--base-url",
            ],
        )
        .await;
    assert_eq!(ran.code, 2, "{}", ran.all());
    assert!(ran.err.contains("usage: agentic accounts"), "{}", ran.err);
    assert!(
        !ran.err.contains("unknown option"),
        "`--base-url` is an option of `accounts add`: {}",
        ran.err
    );
    assert_eq!(kit.term.reads(), 0);
}

/// The line of `accounts list` for the Account `id`, as whitespace-separated
/// columns.
fn list_columns(out: &str, id: &str) -> Vec<String> {
    out.lines()
        .map(|l| l.split_whitespace().map(str::to_string).collect::<Vec<_>>())
        .find(|columns| columns.first().is_some_and(|c| c == id))
        .unwrap_or_else(|| panic!("no line for {id}: {out}"))
}

/// List: one Account of each kind shows its provider and its endpoint's
/// host (`api.anthropic.com`, `api.openai.com`, the compatible endpoint's
/// host, and `127.0.0.1:<port>` for one on this Mac), with each key masked
/// as before; any stdin; no edit.
#[tokio::test]
async fn list_shows_the_endpoint_host_of_each_kind_of_account() {
    let env = env();
    let root = env.usual(&[]);
    env.write_user_file(&four_kinds());
    let before = env.user_text();
    let kit = Kit::new(FakeTerminal::piped()).with_key("cloud", CANARY);
    for (id, key) in [
        ("oa", OA_CANARY),
        ("router", ROUTER_CANARY),
        ("llama", LLAMA_CANARY),
    ] {
        kit.secrets
            .set(id, am_domain::SecretString::new(key.to_string()))
            .unwrap();
    }

    let ran = kit.run(&env, &root, &["list"]).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    for (id, provider, host, tail) in [
        ("cloud", "anthropic", "api.anthropic.com", "WXYZ"),
        ("oa", "openai", "api.openai.com", "OAOA"),
        ("router", "openai-compatible", "openrouter.example", "RTRT"),
        ("llama", "openai-compatible", "127.0.0.1:8080", "LLMA"),
    ] {
        let columns = list_columns(&ran.out, id);
        assert!(
            columns.iter().any(|c| c == provider),
            "{id}: its provider {provider}: {columns:?}"
        );
        assert!(
            columns.iter().any(|c| c == host),
            "{id}: its endpoint host {host}: {columns:?}"
        );
        assert!(
            columns
                .iter()
                .any(|c| c.starts_with('•') && c.ends_with(tail)),
            "{id}: its key masked: {columns:?}"
        );
    }
    for key in [CANARY, OA_CANARY, ROUTER_CANARY, LLAMA_CANARY] {
        assert!(!ran.all().contains(key), "a key was printed: {}", ran.all());
        assert!(
            !ran.all().contains(&key[..12]),
            "the head of a key was printed: {}",
            ran.all()
        );
    }
    assert_eq!(kit.term.reads(), 0);
    assert_eq!(env.user_text(), before);
    assert!(env.history().is_empty());
    assert!(kit.egress.requests().is_empty(), "list calls nothing");
}

/// Test OpenAI: with `--model`, one short reply is streamed from the
/// recorded OpenAI stream; the one request goes to
/// `https://api.openai.com/v1/chat/completions` with a `Bearer` reference
/// naming the Account; the egress policy admits only that host and no
/// loopback port; any stdin; no edit.
#[tokio::test]
async fn test_of_an_openai_account_streams_through_its_pinned_host() {
    let env = env();
    let root = four_kinds_added(&env).await;
    let before = env.user_text();
    let recorded = env.history().len();
    let kit = Kit::new(FakeTerminal::piped()).with_key("oa", OA_CANARY);
    kit.egress.replay("api.openai.com", "openai/text.sse");

    let ran = kit.run(&env, &root, &["test", "oa", "--model", "m"]).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    for word in ["one", "two", "three"] {
        assert!(ran.out.contains(word), "streamed text: {}", ran.out);
    }
    assert!(
        ran.out.contains("api.openai.com"),
        "the result names the endpoint: {}",
        ran.out
    );
    let requests = kit.egress.requests();
    assert_eq!(requests.len(), 1, "one short reply");
    assert_eq!(
        requests[0].url,
        format!("{OPENAI_URL}/chat/completions"),
        "the request goes to the Account's endpoint"
    );
    assert_eq!(
        requests[0].auth,
        Some(AuthRef {
            scheme: AuthScheme::Bearer,
            account_id: "oa".into()
        })
    );
    let body = String::from_utf8_lossy(&requests[0].body).to_string();
    assert!(!body.contains(OA_CANARY));
    assert!(
        body.contains("\"model\":\"m\""),
        "the model asked for: {body}"
    );

    // Story 2.2: the client is built over the project's egress directory,
    // which pins the Account to its endpoint (scheme, host and port; the
    // client compares them exactly, rule_09_callers), knows no other id
    // than the project's, and the request names the Account as its caller.
    let directories = kit.directories();
    assert_eq!(directories.len(), 1, "one egress client is built");
    let directory = directories[0].as_ref();
    assert_eq!(
        directory.account_endpoint("oa"),
        Some(endpoint("https", "api.openai.com", 443))
    );
    assert_directory_refuses_unknown_ids(directory);
    assert_eq!(
        requests[0].caller,
        Some(Caller::Account {
            account: "oa".into(),
            seat: None
        })
    );

    assert_eq!(env.user_text(), before);
    assert_eq!(env.history().len(), recorded, "no edit");
    assert!(!ran.all().contains(OA_CANARY));
}

/// `--model` is required for an OpenAI or compatible Account (no default
/// model id is assumed): without it the test is refused naming `--model`,
/// no key is read and nothing is sent.
#[tokio::test]
async fn test_of_an_openai_or_compatible_account_needs_a_model() {
    let env = env();
    let root = four_kinds_added(&env).await;
    for (id, key, host, fixture) in [
        ("oa", OA_CANARY, "api.openai.com", "openai/text.sse"),
        (
            "router",
            ROUTER_CANARY,
            "openrouter.example",
            "openai_compat/reasoning.sse",
        ),
        (
            "llama",
            LLAMA_CANARY,
            "127.0.0.1:8080",
            "openai_compat/reasoning.sse",
        ),
    ] {
        let kit = Kit::new(FakeTerminal::piped()).with_key(id, key);
        kit.egress.replay(host, fixture);
        let counting = Arc::new(CountingStore {
            inner: kit.secrets.clone(),
            gets: AtomicUsize::new(0),
        });

        let ran = kit
            .run_with(&env, &root, &["test", id], counting.clone())
            .await;

        assert_ne!(ran.code, 0, "{id}: {}", ran.all());
        assert!(
            ran.all().contains("--model"),
            "{id}: the refusal names --model: {}",
            ran.all()
        );
        assert_eq!(
            counting.gets.load(Ordering::SeqCst),
            0,
            "{id}: refused before any Keychain read"
        );
        assert!(kit.egress.requests().is_empty(), "{id}: nothing sent");
        assert!(!ran.all().contains(key));
    }
}

/// Test compatible, remote and on this Mac, each added through the command
/// first (its endpoint confirmed): the one request goes to
/// `{base_url}/chat/completions` with a `Bearer` reference naming the
/// Account, and only the content is printed (a reasoning model's thinking is
/// not). The remote policy admits only that host; the loopback policy admits
/// only that port and no host (story 2.2: the directory pins each to its
/// endpoint and the client compares scheme, host and port exactly).
#[tokio::test]
async fn test_of_a_compatible_account_calls_only_its_base_url() {
    let env = env();
    let root = four_kinds_added(&env).await;
    let before = env.user_text();
    let recorded = env.history().len();

    // Remote.
    let kit = Kit::new(FakeTerminal::piped()).with_key("router", ROUTER_CANARY);
    kit.egress
        .replay("openrouter.example", "openai_compat/reasoning.sse");
    let ran = kit
        .run(&env, &root, &["test", "router", "--model", "m"])
        .await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(ran.out.contains("The answer is 4."), "{}", ran.out);
    assert!(!ran.all().contains("The question asks"), "{}", ran.all());
    assert!(ran.out.contains("openrouter.example"), "{}", ran.out);
    let requests = kit.egress.requests();
    assert_eq!(requests.len(), 1, "one short reply");
    assert_eq!(requests[0].url, format!("{REMOTE_URL}/chat/completions"));
    assert_eq!(
        requests[0].auth,
        Some(AuthRef {
            scheme: AuthScheme::Bearer,
            account_id: "router".into()
        })
    );
    assert!(!String::from_utf8_lossy(&requests[0].body).contains(ROUTER_CANARY));
    // Story 2.2: the client is built over the project's egress directory,
    // which pins the Account to its endpoint (scheme, host and port; the
    // client compares them exactly, rule_09_callers), knows no other id
    // than the project's, and the request names the Account as its caller.
    let directories = kit.directories();
    assert_eq!(directories.len(), 1, "one egress client is built");
    let directory = directories[0].as_ref();
    assert_eq!(
        directory.account_endpoint("router"),
        Some(endpoint("https", "openrouter.example", 443))
    );
    assert_directory_refuses_unknown_ids(directory);
    assert_eq!(
        requests[0].caller,
        Some(Caller::Account {
            account: "router".into(),
            seat: None
        })
    );
    assert!(!ran.all().contains(ROUTER_CANARY));

    // On this Mac.
    let kit = Kit::new(FakeTerminal::piped()).with_key("llama", LLAMA_CANARY);
    kit.egress
        .replay("127.0.0.1:8080", "openai_compat/reasoning.sse");
    let ran = kit
        .run(&env, &root, &["test", "llama", "--model", "m"])
        .await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(ran.out.contains("The answer is 4."), "{}", ran.out);
    assert!(ran.out.contains("127.0.0.1:8080"), "{}", ran.out);
    let requests = kit.egress.requests();
    assert_eq!(requests.len(), 1, "one short reply");
    assert_eq!(requests[0].url, format!("{LOOPBACK_URL}/chat/completions"));
    assert_eq!(
        requests[0].auth,
        Some(AuthRef {
            scheme: AuthScheme::Bearer,
            account_id: "llama".into()
        })
    );
    assert!(!String::from_utf8_lossy(&requests[0].body).contains(LLAMA_CANARY));
    // Story 2.2: the client is built over the project's egress directory,
    // which pins the Account to its endpoint (scheme, host and port; the
    // client compares them exactly, rule_09_callers), knows no other id
    // than the project's, and the request names the Account as its caller.
    let directories = kit.directories();
    assert_eq!(directories.len(), 1, "one egress client is built");
    let directory = directories[0].as_ref();
    assert_eq!(
        directory.account_endpoint("llama"),
        Some(endpoint("http", "127.0.0.1", 8080))
    );
    assert_directory_refuses_unknown_ids(directory);
    assert_eq!(
        requests[0].caller,
        Some(Caller::Account {
            account: "llama".into(),
            seat: None
        })
    );
    assert!(!ran.all().contains(LLAMA_CANARY));

    assert_eq!(env.user_text(), before);
    assert_eq!(env.history().len(), recorded, "no edit");
}

/// There is no keyless Account: a compatible Account (added through the
/// command) whose Keychain entry is missing on this run is refused as an
/// Anthropic one is, and nothing is sent.
#[tokio::test]
async fn test_of_a_compatible_account_without_a_key_sends_nothing() {
    let env = env();
    let root = four_kinds_added(&env).await;
    for (id, host) in [
        ("oa", "api.openai.com"),
        ("router", "openrouter.example"),
        ("llama", "127.0.0.1:8080"),
    ] {
        let kit = Kit::new(FakeTerminal::piped());
        kit.egress.replay(host, "openai_compat/reasoning.sse");

        let ran = kit.run(&env, &root, &["test", id, "--model", "m"]).await;

        assert_ne!(ran.code, 0, "{id}: {}", ran.all());
        assert!(
            ran.all().contains("no key in Keychain"),
            "{id}: {}",
            ran.all()
        );
        assert!(kit.egress.requests().is_empty(), "{id}: nothing sent");
    }
}

/// An Account's calls go only to its own endpoint (AD-48), whatever is
/// written into the file by hand. Each Account is added through the command
/// first, then its `base_url` is edited in the file: an OpenAI Account given
/// a `base_url` is refused for that `base_url` (its calls go only to
/// `api.openai.com`), and a compatible Account whose `base_url` becomes a
/// private or LAN address (which `add` refuses) is refused for that
/// `base_url` too. The endpoint rules come before the confirmed check, so
/// these refusals name the `base_url`; no key is read, no egress policy is
/// asked for and nothing is sent.
#[tokio::test]
async fn test_never_follows_a_hand_written_base_url_outside_the_endpoint_rules() {
    let env = env();
    let root = four_kinds_added(&env).await;
    let written = env.user_text().unwrap();

    // An OpenAI Account with a `base_url` line added under its provider.
    let provider_line = written
        .lines()
        .find(|l| l.trim() == "provider: openai")
        .expect("the OpenAI Account's provider line");
    let indent = &provider_line[..provider_line.len() - provider_line.trim_start().len()];
    let edited = written.replacen(
        &format!("{provider_line}\n"),
        &format!("{provider_line}\n{indent}base_url: https://elsewhere.example/v1\n"),
        1,
    );
    assert_ne!(edited, written);
    std::fs::write(env.accounts_path(), &edited).unwrap();
    assert!(
        file_accounts(&env)
            .iter()
            .any(|a| a.0 == "oa" && a.3.as_deref() == Some("https://elsewhere.example/v1")),
        "the loader reads the hand-written base_url: {edited}"
    );
    let ran = refused_test(&env, &root, "oa").await;
    for words in ["`base_url`", "api.openai.com"] {
        assert!(
            ran.all().contains(words),
            "oa: the refusal gives the base_url reason ({words}): {}",
            ran.all()
        );
    }
    assert!(
        !ran.all().contains("elsewhere.example"),
        "oa: the other host is not named as an endpoint: {}",
        ran.all()
    );

    // A compatible Account whose `base_url` becomes a private or LAN
    // address.
    for (id, from, to) in [
        ("router", REMOTE_URL, "https://10.0.0.5/v1"),
        ("llama", LOOPBACK_URL, "http://192.168.1.4:8080/v1"),
    ] {
        let edited = written.replace(from, to);
        assert_ne!(edited, written, "{id}");
        std::fs::write(env.accounts_path(), &edited).unwrap();
        let ran = refused_test(&env, &root, id).await;
        assert!(
            ran.all().contains("base_url"),
            "{id}: the refusal gives the base_url reason: {}",
            ran.all()
        );
        assert!(
            ran.all().contains("address"),
            "{id}: it says an address is not called: {}",
            ran.all()
        );
    }
}

// ----------------------------------- review of story 2.3: confirmed only
//
// A key goes only to what the owner confirmed (AD-15, AD-16, AD-48). Before
// any Keychain read or request, `accounts test` checks, read-only and
// without the project's lock, that `models/accounts.yaml` holds exactly the
// bytes the app last wrote and that the latest `account_added` audit note
// of the Account names the same provider, scheme, host and port. It still
// runs from any stdin.

/// The Accounts the user file holds now, as its reader reads them (entries
/// with an error included).
fn file_accounts(env: &Env) -> Vec<Held> {
    let file = am_config::accounts_file(env.places.user.dir());
    assert_eq!(file.problem, None);
    file.accounts
        .into_iter()
        .map(|a| (a.id, a.provider, a.name, a.base_url, a.secret_ref))
        .collect()
}

/// Writes an older entry's bytes back through the app's writer of the user
/// file (History's Restore), under `user.lock`: a write the app made, which
/// carries no `account_added` note.
/// Writes the Account's earlier provider and endpoint back through an app
/// edit with no confirmation record (story 2.14: a History Restore that
/// moves an Account is refused, naming `accounts change`; this sets up the
/// same file state the old Restore did).
fn app_edit_back(env: &Env, id: &str, provider: &str, base_url: Option<&str>) {
    let writer = env.user_writer();
    let file = am_config::accounts_file(env.places.user.dir());
    let edit = am_config::change_account_edit(&file, id, provider, base_url).unwrap();
    writer.apply(owner(), edit).unwrap();
}

async fn remove_account(env: &Env, root: &Path, id: &str) {
    let ran = Kit::new(FakeTerminal::new())
        .run(env, root, &["remove", id])
        .await;
    assert_eq!(ran.code, 0, "remove {id}: {}", ran.all());
}

/// A `base_url` edited by hand after a confirmed add (well-formed, another
/// host; then another port on this Mac): the file no longer holds what the
/// app wrote, so no Account in it is called. No key is read, nothing is
/// sent, and the refusal says the endpoint was not confirmed and what to
/// do. With the file put back byte for byte, the Account answers again.
#[tokio::test]
async fn test_refuses_a_base_url_edited_by_hand_after_the_add() {
    let env = env();
    let root = four_kinds_added(&env).await;
    let written = env.user_text().unwrap();

    for (id, from, to) in [
        ("router", "openrouter.example", "elsewhere.example"),
        ("llama", "127.0.0.1:8080", "127.0.0.1:9090"),
    ] {
        let edited = written.replace(from, to);
        assert_ne!(edited, written, "{id}: the edit changes the file");
        std::fs::write(env.accounts_path(), &edited).unwrap();
        assert!(
            file_accounts(&env)
                .iter()
                .any(|a| a.0 == id && a.3.as_deref().is_some_and(|u| u.contains(to))),
            "{id}: the edited file is valid config naming {to}"
        );

        let ran = refused_test(&env, &root, id).await;
        assert_says_not_confirmed(&ran, id);
        assert!(
            ran.all().contains(USER_FILE),
            "{id}: it names the file: {}",
            ran.all()
        );
        // Every Account of the edited file is refused, not only the one
        // whose line changed.
        for other in ["cloud", "oa", "router", "llama"] {
            refused_test(&env, &root, other).await;
        }
    }

    std::fs::write(env.accounts_path(), &written).unwrap();
    let kit = Kit::new(FakeTerminal::piped()).with_key("router", ROUTER_CANARY);
    kit.egress
        .replay("openrouter.example", "openai_compat/reasoning.sse");
    let ran = kit
        .run(&env, &root, &["test", "router", "--model", "m"])
        .await;
    assert_eq!(
        ran.code,
        0,
        "the file is as the app wrote it: {}",
        ran.all()
    );
    assert_eq!(kit.egress.requests().len(), 1);
}

/// A provider edited by hand after a confirmed add (Anthropic to OpenAI,
/// and OpenAI to Anthropic): the Account's key would go to a provider the
/// owner never confirmed for it, so `test` refuses, reads no key and sends
/// nothing.
#[tokio::test]
async fn test_refuses_a_provider_edited_by_hand_after_the_add() {
    let env = env();
    let root = four_kinds_added(&env).await;
    let written = env.user_text().unwrap();

    for (id, from, to) in [
        ("cloud", "provider: anthropic", "provider: openai"),
        ("oa", "provider: openai\n", "provider: anthropic\n"),
    ] {
        let edited = written.replacen(from, to, 1);
        assert_ne!(edited, written, "{id}: the edit changes the file");
        std::fs::write(env.accounts_path(), &edited).unwrap();
        let provider = to.trim().trim_start_matches("provider: ");
        assert!(
            file_accounts(&env)
                .iter()
                .any(|a| a.0 == id && a.1 == provider),
            "{id}: the edited file is valid config with provider {provider}"
        );

        let ran = refused_test(&env, &root, id).await;
        assert_says_not_confirmed(&ran, id);
    }
}

/// An Account written into the file by hand and never added (Anthropic,
/// OpenAI, compatible): refused the same way, in a project the app has
/// opened, in one it never opened on this Mac, and in one whose database
/// this version has not migrated.
#[tokio::test]
async fn test_refuses_an_account_that_was_never_added() {
    // Opened on this Mac; the file was never written by the app.
    let env = env();
    let root = env.usual(&[]);
    env.write_user_file(&four_kinds());
    for id in ["cloud", "oa", "router", "llama"] {
        let ran = refused_test(&env, &root, id).await;
        assert_says_not_confirmed(&ran, id);
    }
    assert!(env.history().is_empty(), "nothing was written");

    // Story 2.12: an old project file is never read to send a key. A
    // project still holding `models/accounts.yaml` (never migrated), on a
    // Mac whose user list lacks its ids: `test` of such an id is refused
    // before any Keychain read, from inside that project.
    let legacy = self::env();
    let folder = legacy.project(
        "p",
        &[
            ("company.yaml", COMPANY.to_string()),
            (LEGACY_FILE, four_kinds()),
        ],
    );
    for id in ["cloud", "oa", "router", "llama"] {
        let ran = refused_test(&legacy, &folder, id).await;
        assert!(ran.all().contains(id), "{id}: {}", ran.all());
    }
    assert!(!legacy.accounts_path().exists(), "no user file is made");

    // Added, then the user store is lost (deleted): every Account is to be
    // re-confirmed; the keys stay in the Keychain.
    let lost = self::env();
    let root = four_kinds_added(&lost).await;
    for side in ["user.db", "user.db-wal", "user.db-shm"] {
        let _ = std::fs::remove_file(lost.places.user.dir().join(side));
    }
    for id in ["cloud", "router"] {
        let ran = refused_test(&lost, &root, id).await;
        assert_says_not_confirmed(&ran, id);
    }
}

/// An Account added, then removed, then put back into the file by hand
/// (the very bytes the add wrote): the app's last write was the removal, so
/// the file holds an outside change and the Account is not called.
#[tokio::test]
async fn test_refuses_an_account_removed_and_put_back_by_hand() {
    let env = env();
    let root = four_kinds_added(&env).await;
    let with_router = env.user_text().unwrap();
    remove_account(&env, &root, "router").await;
    assert!(!env.user_text().unwrap().contains("router"));
    std::fs::write(env.accounts_path(), &with_router).unwrap();

    let ran = refused_test(&env, &root, "router").await;
    assert_says_not_confirmed(&ran, "router");
    refused_test(&env, &root, "cloud").await;
}

/// The file is as the app last wrote it, but that write was not the add
/// the latest `account_added` note records (an older History entry was
/// restored): the endpoint in the file, or its provider alone, is not the
/// one confirmed last, so the Account is not called.
#[tokio::test]
async fn test_refuses_what_the_latest_add_of_the_account_did_not_record() {
    // Another host: added at one URL, removed, added at another, and the
    // first add's bytes restored.
    let env = env();
    let root = plain(&env);
    add_account(
        &env,
        &root,
        "router",
        "openai-compatible",
        Some(REMOTE_URL),
        ROUTER_CANARY,
    )
    .await;
    remove_account(&env, &root, "router").await;
    add_account(
        &env,
        &root,
        "router",
        "openai-compatible",
        Some("https://second.example/v1"),
        ROUTER_CANARY,
    )
    .await;
    app_edit_back(&env, "router", "openai-compatible", Some(REMOTE_URL));
    assert_eq!(
        file_accounts(&env),
        vec![held("router", "openai-compatible", None, Some(REMOTE_URL))],
        "the app wrote the first endpoint back"
    );
    let ran = refused_test(&env, &root, "router").await;
    assert_says_not_confirmed(&ran, "router");

    // The provider alone: the same scheme, host and port, confirmed last
    // for an OpenAI Account, held in the file as a compatible one.
    let env = self::env();
    let root = plain(&env);
    add_account(
        &env,
        &root,
        "oa",
        "openai-compatible",
        Some(OPENAI_URL),
        OA_CANARY,
    )
    .await;
    remove_account(&env, &root, "oa").await;
    add_account(&env, &root, "oa", "openai", None, OA_CANARY).await;
    app_edit_back(&env, "oa", "openai-compatible", Some(OPENAI_URL));
    assert_eq!(
        file_accounts(&env),
        vec![held("oa", "openai-compatible", None, Some(OPENAI_URL))]
    );
    let ran = refused_test(&env, &root, "oa").await;
    assert_says_not_confirmed(&ran, "oa");
}

// --------------------------------------- review of story 2.3: reused key

/// The Keychain already holds a key for the id (an earlier add, another
/// project): the sentence says so, and says that key will be sent to the
/// endpoint, before the id is typed. The key itself is never shown, and it
/// is reused and never overwritten, as before. With no such entry the
/// sentence says nothing of the kind.
#[tokio::test]
async fn add_says_before_the_id_is_typed_that_a_held_key_will_go_to_the_endpoint() {
    const EARLIER: &str = "sk-earlier-canary-5e6f7a8b-HELD";
    let cases: [(&str, &str, Option<&str>, &str); 3] = [
        ("router", "openai-compatible", Some(REMOTE_URL), REMOTE_URL),
        (
            "llama",
            "openai-compatible",
            Some(LOOPBACK_URL),
            LOOPBACK_URL,
        ),
        ("cloud2", "anthropic", None, "https://api.anthropic.com"),
    ];
    for (id, provider, base_url, endpoint) in cases {
        let mut args = vec!["add", id, "--provider", provider];
        if let Some(url) = base_url {
            args.extend(["--base-url", url]);
        }

        // Held: said in the sentence, before the prompt for the id.
        let env = env();
        let root = plain(&env);
        let kit = Kit::new(FakeTerminal::new()).with_key(id, EARLIER);
        kit.term.line(id);
        // Story 2.12: no confirmation of this id for this endpoint yet, so
        // the stored key is typed at the hidden prompt.
        kit.term.line(EARLIER);
        let ran = kit.run(&env, &root, &args).await;
        assert_eq!(ran.code, 0, "{id}: {}", ran.all());
        let prompt = ran
            .shown
            .find("To confirm")
            .unwrap_or_else(|| panic!("{id}: no prompt: {}", ran.shown));
        let sentence = &ran.shown[..prompt];
        for words in [
            format!("already holds a key for '{id}'"),
            format!("will be sent to {endpoint}"),
        ] {
            assert!(
                sentence.contains(&words),
                "{id}: before the id is typed, the sentence says \"{words}\": {sentence}"
            );
        }
        assert_eq!(
            kit.term.echo_reads(),
            vec![true, false],
            "{id}: the id, then the stored key with echo off"
        );
        assert_eq!(kit.key(id).as_deref(), Some(EARLIER), "never overwritten");
        assert!(!ran.all().contains(EARLIER), "{id}: the key is never shown");
        assert!(!ran.all().contains(&EARLIER[..12]), "{id}: nor its head");

        // Not held: the sentence is the one of every add.
        let env = self::env();
        let root = plain(&env);
        let kit = Kit::new(FakeTerminal::new());
        kit.term.line(id);
        kit.term.line(CANARY);
        let ran = kit.run(&env, &root, &args).await;
        assert_eq!(ran.code, 0, "{id}: {}", ran.all());
        for words in ["already holds", "will be sent"] {
            assert!(
                !ran.shown.contains(words),
                "{id}: no key was held: {}",
                ran.shown
            );
        }
    }
}

// ------------------------------- review of story 2.3: an entry with an error

/// An Account whose entry in the file has an error (`agentic validate`
/// names it) is never called and never shown with a host: a `base_url` the
/// loader refuses reads as no `base_url`, which must not turn an OpenAI
/// Account into one called at the provider's host, nor a compatible one
/// into one that "needs `base_url`". `test` refuses before any Keychain
/// read, pointing at `agentic validate`; `list` says the entry has an
/// error.
#[tokio::test]
async fn test_and_list_of_an_entry_with_an_error_point_at_validate() {
    let env = env();
    let yaml = "schema_version: 2\naccounts:\n  - id: cloud\n    provider: anthropic\n    secret_ref: cloud\n  - id: oa\n    provider: openai\n    base_url: http://example.org/v1\n    secret_ref: oa\n  - id: box\n    provider: openai-compatible\n    base_url: http://localhost:8080/v1\n    secret_ref: box\n  - id: odd\n    provider: anthropic\n    surprise: true\n    secret_ref: odd\n";
    let root = env.usual(&[]);
    env.write_user_file(yaml);

    for id in ["oa", "box", "odd"] {
        let ran = refused_test(&env, &root, id).await;
        assert!(
            ran.all().contains("agentic validate"),
            "{id}: it points at validate: {}",
            ran.all()
        );
        assert!(
            ran.all().contains("error"),
            "{id}: it says the entry has an error: {}",
            ran.all()
        );
        assert!(
            !ran.all().contains("needs `base_url`"),
            "{id}: the entry has a base_url: {}",
            ran.all()
        );
    }

    let kit = Kit::new(FakeTerminal::piped()).with_key("cloud", CANARY);
    let ran = kit.run(&env, &root, &["list"]).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    let line_of = |id: &str| {
        ran.out
            .lines()
            .find(|l| l.split_whitespace().next() == Some(id))
            .unwrap_or_else(|| panic!("no line for {id}: {}", ran.out))
            .to_string()
    };
    for id in ["oa", "box", "odd"] {
        let line = line_of(id);
        assert!(line.contains("error"), "{id}: {line}");
        assert!(line.contains("agentic validate"), "{id}: {line}");
        for host in [
            "api.openai.com",
            "api.anthropic.com",
            "example.org",
            "localhost",
        ] {
            assert!(!line.contains(host), "{id}: no host is shown: {line}");
        }
    }
    let cloud = line_of("cloud");
    assert!(cloud.contains("api.anthropic.com"), "{cloud}");
    assert!(!cloud.contains("error"), "{cloud}");
    assert!(kit.egress.requests().is_empty(), "list calls nothing");
}

// ------------------------------- second review of story 2.3: no lock taken

/// `accounts test` takes no lock: while another process holds the project's
/// store (the app is serving it) and another holds `user.lock` (story
/// 2.12: a write in progress that has not touched the file), a confirmed
/// Account is still read from the user store read-only and answers, on any
/// stdin. The holders keep their locks, and nothing is written.
#[tokio::test]
async fn test_answers_while_another_process_holds_the_store() {
    let env = env();
    let root = four_kinds_added(&env).await;
    let before = env.user_text();
    let (held, _) = am_store::open_runtime(&env.layout(&root), "Holder Mac".into()).unwrap();
    let user_held = UserStore::locked(&env.places.user, Duration::from_secs(10)).unwrap();

    let kit = Kit::new(FakeTerminal::piped()).with_key("cloud", CANARY);
    kit.egress
        .replay("api.anthropic.com", "anthropic/c1_count_stream.sse");
    let ran = kit.run(&env, &root, &["test", "cloud"]).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    for word in ["one", "two", "three"] {
        assert!(ran.out.contains(word), "streamed text: {}", ran.out);
    }
    assert_eq!(kit.egress.requests().len(), 1, "one short reply");

    let kit = Kit::new(FakeTerminal::piped()).with_key("llama", LLAMA_CANARY);
    kit.egress
        .replay("127.0.0.1:8080", "openai_compat/reasoning.sse");
    let ran = kit
        .run(&env, &root, &["test", "llama", "--model", "m"])
        .await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(ran.out.contains("The answer is 4."), "{}", ran.out);
    assert_eq!(kit.egress.requests().len(), 1, "one short reply");

    assert!(
        am_store::lock_is_held(&env.layout(&root)),
        "the holder still has the lock"
    );
    assert_eq!(env.user_text(), before);
    assert!(
        UserStore::locked(&env.places.user, Duration::from_millis(100)).is_err(),
        "the user lock's holder still has it"
    );
    drop(user_held);
    drop(held);
}

// ------------------------------------------- story 2.2: the project directory

// Decision 8 (owner, 2026-11-22): AD-15's Key use check moves out of
// `accounts test` into the egress directory, so every path that sends a
// key gets it. Story 2.12: `am_app::egress::ProjectDirectory::open(root,
// places)` resolves an Account id from this Mac's user list through
// `am_app::key_use`, read-only (no lock, no edit), and answers for an
// Account only while it is confirmed: the user Accounts file
// holds exactly the bytes of its latest History entry, and the latest
// `account_added` note names the same provider, scheme, host and port. The
// reserved local Accounts skip the check (AD-31). Written first.

/// The project's egress directory, opened read-only now.
fn project_directory(env: &Env, root: &Path) -> am_app::egress::ProjectDirectory {
    am_app::egress::ProjectDirectory::open(root, &env.places)
        .unwrap_or_else(|e| panic!("the directory opens: {e}"))
}

/// What a directory answers for every Account of the four-kinds project.
fn every_account_refused(directory: &dyn EgressDirectory, why: &str) {
    for id in ["cloud", "oa", "router", "llama"] {
        assert_eq!(directory.account_endpoint(id), None, "{why}: {id} refused");
    }
    for reserved in ["local-lmstudio", "local-ollama"] {
        assert_eq!(
            directory.account_endpoint(reserved),
            reserved_local_account(reserved),
            "{why}: the reserved {reserved} skips the check"
        );
    }
}

/// Added through `agentic accounts add` (the interactive fake): the
/// directory answers each Account's endpoint, and nothing for an id or a
/// seat the project does not configure. Opening it writes nothing.
#[tokio::test]
async fn the_project_directory_answers_an_account_added_through_the_command() {
    let env = env();
    let root = four_kinds_added(&env).await;
    let before = env.user_text();
    let recorded = env.history().len();

    let directory = project_directory(&env, &root);
    for (id, at) in [
        ("cloud", endpoint("https", "api.anthropic.com", 443)),
        ("oa", endpoint("https", "api.openai.com", 443)),
        ("router", endpoint("https", "openrouter.example", 443)),
        ("llama", endpoint("http", "127.0.0.1", 8080)),
    ] {
        assert_eq!(directory.account_endpoint(id), Some(at), "{id}");
    }
    assert_directory_refuses_unknown_ids(&directory);
    assert_eq!(
        directory.seat_hosts("alpha"),
        Some(Vec::new()),
        "a configured seat with no allowed_hosts reaches nothing"
    );

    assert_eq!(env.user_text(), before, "no edit");
    assert_eq!(env.history().len(), recorded, "nothing recorded");
}

/// A hand edit of `models/accounts.yaml` after the adds (a `base_url`, or a
/// provider): the file no longer holds what the app wrote, so the directory
/// refuses that Account and every other Account in the file. Put back byte
/// for byte, the file is confirmed again.
#[tokio::test]
async fn the_project_directory_refuses_every_account_of_a_hand_edited_file() {
    let env = env();
    let root = four_kinds_added(&env).await;
    let written = env.user_text().unwrap();

    for (what, from, to) in [
        ("base_url host", "openrouter.example", "elsewhere.example"),
        ("base_url port", "127.0.0.1:8080", "127.0.0.1:9090"),
        ("provider", "provider: anthropic", "provider: openai"),
    ] {
        let edited = written.replacen(from, to, 1);
        assert_ne!(edited, written, "{what}: the edit changes the file");
        std::fs::write(env.accounts_path(), &edited).unwrap();
        let directory = project_directory(&env, &root);
        every_account_refused(&directory, what);
    }

    std::fs::write(env.accounts_path(), &written).unwrap();
    let directory = project_directory(&env, &root);
    assert_eq!(
        directory.account_endpoint("router"),
        Some(endpoint("https", "openrouter.example", 443)),
        "the file is as the app wrote it"
    );
}

/// An Account written into the file by hand and never added: refused.
#[tokio::test]
async fn the_project_directory_refuses_an_account_never_added() {
    let env = env();
    let root = env.usual(&[]);
    env.write_user_file(&four_kinds());
    let directory = project_directory(&env, &root);
    every_account_refused(&directory, "never added");
}

/// An Account added, removed, then put back into the file by hand (the
/// very bytes the add wrote): the app's last write was the removal, so the
/// directory refuses it and every other Account of the file.
#[tokio::test]
async fn the_project_directory_refuses_an_account_removed_and_put_back_by_hand() {
    let env = env();
    let root = four_kinds_added(&env).await;
    let with_router = env.user_text().unwrap();
    remove_account(&env, &root, "router").await;
    std::fs::write(env.accounts_path(), &with_router).unwrap();
    let directory = project_directory(&env, &root);
    every_account_refused(&directory, "removed and put back");
}

/// End to end through the real egress client over that directory: the
/// unconfirmed Account's request is refused before any Keychain read (the
/// counting store sees zero) and before any lookup or connection, and the
/// refusal is audited naming the Account.
#[tokio::test]
async fn an_unconfirmed_accounts_request_reads_no_key_and_is_audited() {
    let env = env();
    let root = four_kinds_added(&env).await;
    let written = env.user_text().unwrap();
    std::fs::write(
        env.accounts_path(),
        written.replacen("openrouter.example", "elsewhere.example", 1),
    )
    .unwrap();

    let counting = Arc::new(CountingStore {
        inner: FakeSecretStore::with("router", ROUTER_CANARY),
        gets: AtomicUsize::new(0),
    });
    let audit = am_testkit::CollectAudit::new();
    let egress = am_net::NetEgress::new(
        Arc::new(project_directory(&env, &root)),
        counting.clone(),
        audit.clone(),
    );
    for (id, scheme, url) in [
        (
            "router",
            AuthScheme::Bearer,
            "https://elsewhere.example/api/v1/chat/completions",
        ),
        (
            "router",
            AuthScheme::Bearer,
            "https://openrouter.example/api/v1/chat/completions",
        ),
        (
            "cloud",
            AuthScheme::XApiKey,
            "https://api.anthropic.com/v1/messages",
        ),
    ] {
        let request = HttpRequest {
            caller: Some(Caller::Account {
                account: id.into(),
                seat: None,
            }),
            auth: Some(AuthRef {
                scheme,
                account_id: id.into(),
            }),
            ..HttpRequest::post_json(url, b"{}".to_vec())
        };
        match egress.send(request).await {
            Err(EgressError::Blocked(_)) => {}
            Err(other) => panic!("{id} {url}: expected Blocked, got {other:?}"),
            Ok(resp) => panic!("{id} {url}: expected Blocked, got HTTP {}", resp.status),
        }
        let last = audit
            .events()
            .into_iter()
            .rfind(|e| e.action == "egress.blocked")
            .unwrap_or_else(|| panic!("{id} {url}: the refusal is audited"));
        assert_eq!(
            last.args.get("account").map(String::as_str),
            Some(id),
            "{last:?}"
        );
        assert!(
            !format!("{last:?}").contains(ROUTER_CANARY),
            "no key in the entry"
        );
    }
    assert_eq!(
        counting.gets.load(Ordering::SeqCst),
        0,
        "no Keychain read for an unconfirmed Account"
    );
}

/// Story 2.2 review (medium): `agentic serve` records its bound port in
/// the project's runtime folder, and every client am-app builds for the
/// project reads it through the project's egress directory. With the port
/// file naming 8080, `accounts test` of the confirmed compatible Account at
/// `http://127.0.0.1:8080` is refused before any Keychain read, and no
/// egress client is built; the directory answers no Account there and says
/// why.
#[tokio::test]
async fn test_of_an_account_on_the_apps_own_port_is_refused_with_no_key_read() {
    let env = env();
    let root = four_kinds_added(&env).await;
    let layout = am_app::runtime::find_runtime(&env.places, &root).unwrap();
    std::fs::write(layout.serve_port(), "8080\n").unwrap();

    let directory = project_directory(&env, &root);
    assert_eq!(directory.own_ports(), vec![8080]);
    assert_eq!(directory.account_endpoint("llama"), None);
    let why = directory.account_refusal("llama").expect("a reason");
    assert!(
        why.contains("the app's own server is listening on port 8080"),
        "{why}"
    );
    assert!(!why.contains("this project"), "{why}");
    assert_eq!(
        directory.account_endpoint("router"),
        Some(endpoint("https", "openrouter.example", 443)),
        "an Account elsewhere is still answered"
    );

    let counting = Arc::new(CountingStore {
        inner: FakeSecretStore::with("llama", LLAMA_CANARY),
        gets: AtomicUsize::new(0),
    });
    let kit = Kit::new(FakeTerminal::piped());
    let ran = kit
        .run_with(
            &env,
            &root,
            &["test", "llama", "--model", "m"],
            counting.clone(),
        )
        .await;
    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(
        ran.all()
            .contains("the app's own server is listening on port 8080"),
        "{}",
        ran.all()
    );
    assert_eq!(counting.gets.load(Ordering::SeqCst), 0, "no Keychain read");
    assert!(kit.directories().is_empty(), "no egress client is built");
    assert!(kit.egress.requests().is_empty(), "nothing sent");
}

/// Story 2.2 review (2): an `account_added` note confirms an Account only
/// when the owner confirmed it (`Actor::Human`) on an intent of the user
/// Accounts file (story 2.12: in the user store). The same note written by a System actor, or
/// carried by another file's History entry (`company.yaml`), does not; the
/// control, the owner on the Accounts file's own intent, does.
#[tokio::test]
async fn a_note_by_a_system_actor_or_with_another_files_entry_does_not_confirm() {
    use am_domain::{AuditNote, NewIntent};
    use sha2::Digest;
    let system = || Actor::System {
        component: "engine".into(),
    };
    let sha = |b: &[u8]| hex::encode(sha2::Sha256::digest(b));
    let note = AuditNote {
        action: "config.widened".into(),
        target: "router".into(),
        args: [
            ("kind", "account_added"),
            ("file", USER_FILE),
            ("account", "router"),
            ("provider", "openai-compatible"),
            ("scheme", "https"),
            ("host", "openrouter.example"),
            ("port", "443"),
            ("key", "stored"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect(),
    };
    let created = |file: &str, after: &[u8], audit: Vec<AuditNote>| NewIntent {
        file: file.into(),
        kind: HistoryKind::Create,
        ref_entry: None,
        before_sha256: sha(b""),
        after_sha256: sha(after),
        before: Vec::new(),
        after: after.to_vec(),
        diff: String::new(),
        audit,
    };
    for (case, confirms) in [
        ("the owner, on the Accounts file's intent", true),
        ("a System actor, on the Accounts file's intent", false),
        ("the owner, on company.yaml's intent", false),
    ] {
        let env = env();
        let root = env.usual(&[]);
        env.write_user_file(&four_kinds());
        let bytes = std::fs::read(env.accounts_path()).unwrap();
        {
            // Story 2.12: the records live in the user store.
            let history = UserStore::locked(&env.places.user, Duration::from_secs(10)).unwrap();
            let write = |actor: Actor, intent: NewIntent| {
                let id = history.begin_intent(actor.clone(), intent).unwrap();
                history.commit_entry(actor, id).unwrap();
            };
            match case {
                c if c.starts_with("the owner, on the Accounts") => {
                    write(owner(), created(USER_FILE, &bytes, vec![note.clone()]))
                }
                c if c.starts_with("a System actor") => {
                    write(system(), created(USER_FILE, &bytes, vec![note.clone()]))
                }
                _ => {
                    write(owner(), created(USER_FILE, &bytes, Vec::new()));
                    write(
                        owner(),
                        created("company.yaml", b"x: 1\n", vec![note.clone()]),
                    );
                }
            }
        }
        let directory = project_directory(&env, &root);
        let answered = directory.account_endpoint("router");
        if confirms {
            assert_eq!(
                answered,
                Some(endpoint("https", "openrouter.example", 443)),
                "{case}: {:?}",
                directory.account_refusal("router")
            );
        } else {
            assert_eq!(answered, None, "{case}");
        }
    }
}

/// Story 2.2 review (3): every running `agentic serve` records its port in
/// one user-level folder under the app data dir (one owner-only file per
/// process, named by its id), with or without a project, so a server for
/// another project, or for none, protects this project's Accounts too. The
/// directory refuses every recorded port to Accounts; a file whose process
/// is not running is ignored and removed.
#[tokio::test]
async fn every_running_servers_port_is_refused_and_a_stale_record_removed() {
    let env = env();
    let root = four_kinds_added(&env).await;
    let running = env.places.user.serving_dir();
    std::fs::create_dir_all(&running).unwrap();
    std::fs::write(running.join(std::process::id().to_string()), "8080\n").unwrap();
    std::fs::write(
        running.join(std::os::unix::process::parent_id().to_string()),
        "3001\n",
    )
    .unwrap();
    let stale = running.join("99999999");
    std::fs::write(&stale, "9090\n").unwrap();

    let directory = project_directory(&env, &root);
    let mut ports = directory.own_ports();
    ports.sort_unstable();
    assert_eq!(ports, vec![3001, 8080], "the stale record is ignored");
    assert!(!stale.exists(), "and removed");
    assert_eq!(directory.account_endpoint("llama"), None);
    let why = directory.account_refusal("llama").expect("a reason");
    assert!(
        why.contains("the app's own server is listening on port 8080"),
        "{why}"
    );
    assert_eq!(
        directory.account_endpoint("router"),
        Some(endpoint("https", "openrouter.example", 443))
    );
}
