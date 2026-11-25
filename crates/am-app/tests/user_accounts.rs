//! Story 2.12 (AD-6, AD-13, AD-15, AD-17): Accounts belong to this Mac. The
//! rows of the story's I/O matrix that `accounts_cli.rs` (the rewritten
//! story 2.1 and 2.3 tests) does not already hold: sharing across
//! projects, where the Account lives, the first add on a Mac, owner-only
//! permissions, the wrong database, a record that disagrees with the file,
//! the remove record, unreachable projects and `--force`, children that run
//! as the owner, the lock (changed since the sentence, two adds at once, a
//! holder past the deadline), a check in flight, and `agentic validate` with
//! this Mac's user list (the real binary, with HOME in a temporary folder).
//!
//! The commands run in process through `am_app::cli::accounts`, with
//! am-testkit's fakes (FakeTerminal, FakeSecretStore with canary keys,
//! FakeEgress); every test has its own temporary HOME, never the real app
//! data dir or Keychain. Written before the change.
#![forbid(unsafe_code)]
#![cfg(unix)]

use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, mpsc};
use std::time::{Duration, Instant};

use am_app::cli::{AccountsContext, accounts};
use am_app::runtime::{OpenRequest, Places, Prompt, open_project};
use am_domain::net::{EgressDirectory, Endpoint};
use am_domain::runtime::RUNTIME_BASE_NAME;
use am_domain::{
    Actor, AuditSink, EgressClient, HistoryEntry, HistoryKind, RuntimeBase, SecretStore,
};
use am_store::user::{UserReader, UserStore};
use am_testkit::{FakeEgress, FakeProjectIdentity, FakeSecretStore, FakeTerminal};

const CANARY: &str = "sk-ant-canary-user-0a1b2c3d4e5f-WXYZ";
const OA_CANARY: &str = "sk-proj-canary-user-9f8e7d6c-OAOA";
const ROUTER_CANARY: &str = "sk-or-canary-user-1a2b3c4d-RTRT";
const USER_FILE: &str = "accounts.yaml";
const LEGACY_FILE: &str = "models/accounts.yaml";
const REMOTE_URL: &str = "https://openrouter.example/api/v1";

const COMPANY: &str = "schema_version: 2\nname: Example\ntimezone: UTC\ncurrency: EUR\nteams:\n  - id: core\n    name: Core team\n";

const STDIO_TOOLS: &str = "schema_version: 2\nmcp_servers:\n  - id: notes\n    transport: stdio\n    command: example-notes-server\n    version: 1.0.0\n    network: none\n    tools:\n      - name: list_notes\n        class: read\n";

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

fn set_mode(path: &Path, mode: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
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

    /// Writes the user Accounts file by hand, owner-only, with no History.
    fn write_user_file(&self, text: &str) {
        let dir = self.places.user.dir();
        std::fs::create_dir_all(dir).unwrap();
        set_mode(dir, 0o700);
        std::fs::write(self.accounts_path(), text).unwrap();
        set_mode(&self.accounts_path(), 0o600);
    }

    fn user_writer(&self) -> am_config::ConfigWriter {
        let guard = UserStore::locked(&self.places.user, Duration::from_secs(10)).unwrap();
        am_config::ConfigWriter::for_user_accounts(self.places.user.dir(), Arc::new(guard)).unwrap()
    }
}

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

/// The fakes one command runs with; the Keychain is shared by every run of
/// one kit (one Mac).
struct Kit {
    term: FakeTerminal,
    secrets: Arc<FakeSecretStore>,
    egress: Arc<FakeEgress>,
}

impl Kit {
    fn new(term: FakeTerminal) -> Kit {
        Kit {
            term,
            secrets: FakeSecretStore::new(),
            egress: FakeEgress::new(),
        }
    }

    fn with_secrets(term: FakeTerminal, secrets: Arc<FakeSecretStore>) -> Kit {
        Kit {
            term,
            secrets,
            egress: FakeEgress::new(),
        }
    }

    fn key(&self, account: &str) -> Option<String> {
        self.secrets
            .get(account)
            .unwrap()
            .map(|s| s.expose().to_string())
    }

    async fn run(&self, env: &Env, args: &[&str]) -> Ran {
        self.run_with(env, args, self.secrets.clone()).await
    }

    async fn run_with(&self, env: &Env, args: &[&str], secrets: Arc<dyn SecretStore>) -> Ran {
        let egress = self.egress.clone();
        let factory = Arc::new(
            move |_directory: Arc<dyn EgressDirectory>, _audit: Arc<dyn AuditSink>| {
                egress.clone() as Arc<dyn EgressClient>
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

/// Adds `id` through the command as the owner does, on `secrets` (the
/// Mac's Keychain).
async fn add(
    env: &Env,
    secrets: &Arc<FakeSecretStore>,
    id: &str,
    provider: &str,
    base_url: Option<&str>,
    key: &str,
) -> Ran {
    let mut args = vec!["add", id, "--provider", provider];
    if let Some(url) = base_url {
        args.extend(["--base-url", url]);
    }
    let kit = Kit::with_secrets(FakeTerminal::new(), secrets.clone());
    kit.term.line(id);
    kit.term.line(key);
    kit.run(env, &args).await
}

/// A `test` with a counting Keychain holding `key` for `id`; returns what
/// was printed and how many Keychain reads it made.
async fn test_counting(env: &Env, id: &str, key: &str, args: &[&str]) -> (Ran, usize, usize) {
    let kit = Kit::new(FakeTerminal::piped());
    kit.secrets
        .set(id, am_domain::SecretString::new(key.to_string()))
        .unwrap();
    kit.egress
        .replay("api.anthropic.com", "anthropic/c1_count_stream.sse");
    kit.egress
        .replay("openrouter.example", "openai_compat/reasoning.sse");
    let counting = Arc::new(CountingStore {
        inner: kit.secrets.clone(),
        gets: AtomicUsize::new(0),
    });
    let mut argv = vec!["test", id];
    argv.extend(args);
    let ran = kit.run_with(env, &argv, counting.clone()).await;
    let gets = counting.gets.load(Ordering::SeqCst);
    (ran, gets, kit.egress.requests().len())
}

fn audit_lines(audit: &str, kind: &str) -> Vec<String> {
    audit
        .lines()
        .filter(|l| l.contains(&format!("\"kind\":\"{kind}\"")))
        .map(str::to_string)
        .collect()
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

// ------------------------------------------------------- shared Account

/// Shared Account: `add anthropic` once (typed id, canary key); `list` and
/// `test` then answer it the same way whichever project the owner works in
/// (P1 and P2 both name it). There is one Keychain entry, and a second add
/// of the id, from P2, is refused as already on this Mac.
#[tokio::test]
async fn one_account_serves_every_project_on_this_mac() {
    let env = env();
    let _p1 = env.naming("p1", "anthropic");
    let _p2 = env.naming("p2", "anthropic");
    let keychain = FakeSecretStore::new();

    let ran = add(&env, &keychain, "anthropic", "anthropic", None, CANARY).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    for project in ["p1", "p2"] {
        assert!(
            ran.all().contains(project),
            "the sentence names the references in {project}: {}",
            ran.all()
        );
    }

    for _from in ["p1", "p2"] {
        let kit = Kit::with_secrets(FakeTerminal::piped(), keychain.clone());
        let ran = kit.run(&env, &["list"]).await;
        assert_eq!(ran.code, 0, "{}", ran.all());
        assert!(ran.out.contains("anthropic"), "{}", ran.out);
        assert!(ran.out.contains("api.anthropic.com"), "{}", ran.out);

        let kit = Kit::with_secrets(FakeTerminal::piped(), keychain.clone());
        kit.egress
            .replay("api.anthropic.com", "anthropic/c1_count_stream.sse");
        let ran = kit.run(&env, &["test", "anthropic"]).await;
        assert_eq!(ran.code, 0, "{}", ran.all());
        assert_eq!(kit.egress.requests().len(), 1);
    }

    let again = Kit::with_secrets(FakeTerminal::new(), keychain.clone());
    again.term.line("anthropic");
    again.term.line(CANARY);
    let ran = again
        .run(&env, &["add", "anthropic", "--provider", "anthropic"])
        .await;
    assert_ne!(ran.code, 0, "no second add: {}", ran.all());
    assert!(ran.all().contains("anthropic"), "{}", ran.all());
    assert_eq!(again.term.reads(), 0);
    assert_eq!(
        keychain
            .get("anthropic")
            .unwrap()
            .map(|s| s.expose().to_string()),
        Some(CANARY.to_string()),
        "one entry, unchanged"
    );
    assert_eq!(env.history().len(), 1);
}

/// Where it lives: after the add, the user file and `user.db` hold the
/// Account; no file under P1 or P2 changed; the canary is in no file under
/// the temporary HOME or the projects, no History row, no audit row and no
/// output.
#[tokio::test]
async fn the_account_lives_in_the_app_data_dir_and_nowhere_else() {
    let env = env();
    let p1 = env.naming("p1", "anthropic");
    let p2 = env.naming("p2", "anthropic");
    let (before1, before2) = (snapshot(&p1), snapshot(&p2));
    let keychain = FakeSecretStore::new();

    let ran = add(&env, &keychain, "anthropic", "anthropic", None, CANARY).await;
    assert_eq!(ran.code, 0, "{}", ran.all());

    assert!(env.user_text().unwrap().contains("id: anthropic"));
    let history = env.history();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].file, USER_FILE);
    assert_eq!(audit_lines(&env.audit(), "account_added").len(), 1);
    assert_eq!(snapshot(&p1), before1, "P1 unchanged");
    assert_eq!(snapshot(&p2), before2, "P2 unchanged");
    for p in [&p1, &p2] {
        assert!(!p.join(LEGACY_FILE).exists());
    }

    assert!(!ran.all().contains(CANARY));
    for (path, bytes) in snapshot(env.dir.path()) {
        assert!(
            !contains(&bytes, CANARY),
            "the key reached {}",
            path.display()
        );
    }
    for e in &history {
        assert!(!format!("{e:?}").contains(CANARY));
    }
    assert!(!env.audit().contains(CANARY));
}

/// First add on a Mac: no app data dir at all (no project was ever
/// opened). The add creates the dir 0700, the file by `ConfigCreate`, and
/// `user.db` migrated; the file, `user.db` and `user.lock` are 0600.
#[tokio::test]
async fn the_first_add_on_a_mac_creates_the_app_data_dir_owner_only() {
    let env = env();
    assert!(!env.places.user.dir().exists());
    let keychain = FakeSecretStore::new();

    let ran = add(&env, &keychain, "cloud", "anthropic", None, CANARY).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    assert_eq!(mode(env.places.user.dir()), 0o700, "the app data dir");
    let files: [PathBuf; 3] = [
        env.accounts_path(),
        env.places.user.user_db(),
        env.places.user.user_lock(),
    ];
    for path in files {
        assert_eq!(mode(&path), 0o600, "{}", path.display());
    }
    let history = env.history();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].kind, HistoryKind::Create);
    assert_eq!(history[0].actor, owner());
    assert!(env.audit().contains("config.created"));
}

// ------------------------------------------------------------ permissions

/// Loose permissions: an app data dir that others can enter, or a `user.db`
/// that others can read, is refused before the store opens, naming the path
/// and the `chmod` that fixes it; nothing is read from the terminal or the
/// Keychain, and nothing is written.
#[tokio::test]
async fn loose_permissions_are_refused_naming_the_path_and_the_chmod() {
    let env = env();
    let keychain = FakeSecretStore::new();
    let ran = add(&env, &keychain, "cloud", "anthropic", None, CANARY).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    let dir: PathBuf = env.places.user.dir().to_path_buf();
    let db: PathBuf = env.places.user.user_db();

    for (path, loose, fix) in [
        (dir.clone(), 0o755, format!("chmod 700 {}", dir.display())),
        (db.clone(), 0o644, format!("chmod 600 {}", db.display())),
    ] {
        set_mode(&path, loose);
        let history = env.history().len();

        let (ran, gets, sent) = test_counting(&env, "cloud", CANARY, &[]).await;
        assert_ne!(ran.code, 0, "{}: {}", path.display(), ran.all());
        assert!(ran.all().contains(&fix), "{fix}: {}", ran.all());
        assert_eq!((gets, sent), (0, 0), "no key read, nothing sent");

        let kit = Kit::new(FakeTerminal::new());
        kit.term.line("cloud2");
        kit.term.line(CANARY);
        let ran = kit
            .run(&env, &["add", "cloud2", "--provider", "anthropic"])
            .await;
        assert_ne!(ran.code, 0, "{}: {}", path.display(), ran.all());
        assert!(ran.all().contains(&fix), "{fix}: {}", ran.all());
        assert_eq!(kit.term.reads(), 0, "refused before any prompt");
        assert_eq!(kit.key("cloud2"), None);

        set_mode(&path, if path == dir { 0o700 } else { 0o600 });
        assert_eq!(env.history().len(), history, "nothing written");
    }
    let (ran, _, sent) = test_counting(&env, "cloud", CANARY, &[]).await;
    assert_eq!(ran.code, 0, "owner-only again: {}", ran.all());
    assert_eq!(sent, 1);
}

/// Wrong database: a project's store where `user.db` should be is refused
/// in words naming it, before any Keychain read; it is not migrated.
#[tokio::test]
async fn a_project_store_at_user_db_is_refused() {
    let env = env();
    env.write_user_file(
        "schema_version: 2\naccounts:\n  - id: cloud\n    provider: anthropic\n    secret_ref: cloud\n",
    );
    drop(am_store::Store::open(&env.places.user.user_db()).unwrap());
    set_mode(&env.places.user.user_db(), 0o600);
    let before = std::fs::read(env.places.user.user_db()).unwrap();

    let (ran, gets, sent) = test_counting(&env, "cloud", CANARY, &[]).await;
    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(
        ran.all().contains("project"),
        "names what it is: {}",
        ran.all()
    );
    assert_eq!((gets, sent), (0, 0));

    let kit = Kit::new(FakeTerminal::new());
    kit.term.line("cloud2");
    kit.term.line(CANARY);
    let ran = kit
        .run(&env, &["add", "cloud2", "--provider", "anthropic"])
        .await;
    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(ran.all().contains("project"), "{}", ran.all());
    assert_eq!(kit.key("cloud2"), None);
    assert_eq!(std::fs::read(env.places.user.user_db()).unwrap(), before);
}

// ------------------------------------------------- hand edit and records

/// Hand edit: change any byte of the user file (here a name, which no
/// check of an endpoint would notice): `test`, the egress directory, `add`
/// and `remove` are refused before any Keychain read, naming the file;
/// putting the bytes back restores them.
#[tokio::test]
async fn any_hand_edit_of_the_user_file_refuses_every_key_path() {
    let env = env();
    let project = env.naming("p", "local-ollama");
    let keychain = FakeSecretStore::new();
    let ran = add(&env, &keychain, "cloud", "anthropic", None, CANARY).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    let written = env.user_text().unwrap();
    let edited = format!("{written}\n");
    std::fs::write(env.accounts_path(), &edited).unwrap();

    let (ran, gets, sent) = test_counting(&env, "cloud", CANARY, &[]).await;
    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(
        ran.all().contains(USER_FILE),
        "names the file: {}",
        ran.all()
    );
    assert_eq!((gets, sent), (0, 0));

    let directory = am_app::egress::ProjectDirectory::open(&project, &env.places).unwrap();
    assert_eq!(directory.account_endpoint("cloud"), None);

    for args in [
        vec!["add", "cloud2", "--provider", "anthropic"],
        vec!["remove", "cloud"],
    ] {
        let kit = Kit::new(FakeTerminal::new());
        kit.term.line("cloud2");
        kit.term.line(CANARY);
        let ran = kit.run(&env, &args).await;
        assert_ne!(ran.code, 0, "{args:?}: {}", ran.all());
        assert!(ran.all().contains(USER_FILE), "{args:?}: {}", ran.all());
        assert_eq!(kit.term.reads(), 0, "{args:?}");
    }

    std::fs::write(env.accounts_path(), &written).unwrap();
    let (ran, _, sent) = test_counting(&env, "cloud", CANARY, &[]).await;
    assert_eq!(ran.code, 0, "the bytes are back: {}", ran.all());
    assert_eq!(sent, 1);
    let directory = am_app::egress::ProjectDirectory::open(&project, &env.places).unwrap();
    assert_eq!(
        directory.account_endpoint("cloud"),
        Some(Endpoint {
            scheme: "https".into(),
            host: "api.anthropic.com".into(),
            port: 443
        })
    );
}

/// Record mismatch: the app writes the file with another host for an
/// Account (a History entry with no confirmation record, as a non-widening
/// accepted edit would), so the file's hash matches History but the latest
/// `account_added` names the old host: `test` is refused before any
/// Keychain read, naming the confirmed host and the file's.
#[tokio::test]
async fn a_file_that_disagrees_with_the_confirmation_record_is_refused() {
    let env = env();
    let keychain = FakeSecretStore::new();
    let ran = add(
        &env,
        &keychain,
        "router",
        "openai-compatible",
        Some(REMOTE_URL),
        ROUTER_CANARY,
    )
    .await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    {
        let writer = env.user_writer();
        writer
            .apply(
                owner(),
                am_config::ConfigEdit {
                    file: USER_FILE.into(),
                    ops: vec![am_config::EditOp::Set {
                        path: vec!["accounts".into(), "0".into(), "base_url".into()],
                        value: "https://elsewhere.example/v1".into(),
                    }],
                    body: None,
                },
            )
            .unwrap();
    }
    assert_eq!(env.history().len(), 2);

    let (ran, gets, sent) = test_counting(&env, "router", ROUTER_CANARY, &["--model", "m"]).await;
    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(
        ran.all().contains("openrouter.example"),
        "the confirmed host: {}",
        ran.all()
    );
    assert!(
        ran.all().contains("elsewhere.example"),
        "the file's host: {}",
        ran.all()
    );
    assert_eq!((gets, sent), (0, 0));
}

// ----------------------------------------------------------------- remove

/// Remove: `remove openai` with no references writes `account_removed` and
/// its History entry in the user store, leaves the Keychain entry, and a
/// later `test openai` is refused before any Keychain read.
#[tokio::test]
async fn a_removed_account_is_recorded_and_then_refused() {
    let env = env();
    let _p = env.naming("p", "local-ollama");
    let keychain = FakeSecretStore::new();
    let ran = add(&env, &keychain, "openai", "openai", None, OA_CANARY).await;
    assert_eq!(ran.code, 0, "{}", ran.all());

    let kit = Kit::with_secrets(FakeTerminal::new(), keychain.clone());
    let ran = kit.run(&env, &["remove", "openai"]).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(ran.all().contains("Keychain"), "{}", ran.all());
    assert_eq!(
        kit.key("openai").as_deref(),
        Some(OA_CANARY),
        "left in place"
    );
    let history = env.history();
    assert_eq!(history.len(), 2);
    assert_eq!(history[1].kind, HistoryKind::Edit);
    let removed = audit_lines(&env.audit(), "account_removed");
    assert_eq!(removed.len(), 1, "{}", env.audit());
    assert!(
        removed[0].contains("\"account\":\"openai\""),
        "{}",
        removed[0]
    );
    assert!(!env.audit().contains(OA_CANARY));

    let (ran, gets, sent) = test_counting(&env, "openai", OA_CANARY, &["--model", "m"]).await;
    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(ran.all().contains("openai"), "{}", ran.all());
    assert_eq!((gets, sent), (0, 0));
}

/// Unreachable project: a root record whose folder is gone. `add` says it
/// was not checked, naming it; `remove` is refused naming it until
/// `--force`, which removes and names the project it skipped; `--force`
/// never overrides a reference found in a reachable project.
#[tokio::test]
async fn an_unreachable_project_is_named_and_only_force_lifts_its_refusal() {
    let env = env();
    let gone = env.naming("gone", "spare");
    let gone_text = gone.display().to_string();
    std::fs::remove_dir_all(&gone).unwrap();
    let keychain = FakeSecretStore::new();

    let ran = add(&env, &keychain, "spare", "anthropic", None, CANARY).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(ran.all().contains(&gone_text), "names it: {}", ran.all());
    assert!(ran.all().contains("not checked"), "{}", ran.all());

    let kit = Kit::with_secrets(FakeTerminal::new(), keychain.clone());
    let ran = kit.run(&env, &["remove", "spare"]).await;
    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(ran.all().contains(&gone_text), "{}", ran.all());
    assert!(ran.all().contains("--force"), "says how: {}", ran.all());
    assert_eq!(env.history().len(), 1, "nothing removed");

    let ran = kit.run(&env, &["remove", "spare", "--force"]).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(
        ran.all().contains(&gone_text),
        "names what it skipped: {}",
        ran.all()
    );
    assert!(!env.user_text().unwrap().contains("spare"));
    assert_eq!(audit_lines(&env.audit(), "account_removed").len(), 1);

    // A reference found in a reachable project is never forced.
    let _here = env.naming("here", "cloud");
    let ran = add(&env, &keychain, "cloud", "anthropic", None, CANARY).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    let ran = kit.run(&env, &["remove", "cloud", "--force"]).await;
    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(ran.all().contains("seats/alpha.md"), "{}", ran.all());
    assert!(env.user_text().unwrap().contains("id: cloud"));
}

/// Child of the owner: a reachable project enables a stdio MCP server, so
/// `add` and `remove` are refused naming that project; `test` still runs.
#[tokio::test]
async fn a_project_with_an_owner_child_refuses_add_and_remove_but_not_test() {
    let env = env();
    let keychain = FakeSecretStore::new();
    let ran = add(&env, &keychain, "cloud", "anthropic", None, CANARY).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    let child = env.project(
        "child",
        &[
            ("company.yaml", COMPANY.to_string()),
            ("tools.yaml", STDIO_TOOLS.to_string()),
        ],
    );

    for args in [
        vec!["add", "cloud2", "--provider", "anthropic"],
        vec!["remove", "cloud"],
    ] {
        let kit = Kit::with_secrets(FakeTerminal::new(), keychain.clone());
        kit.term.line("cloud2");
        kit.term.line(CANARY);
        let ran = kit.run(&env, &args).await;
        assert_ne!(ran.code, 0, "{args:?}: {}", ran.all());
        assert!(
            ran.all().contains("stdio MCP server"),
            "{args:?}: {}",
            ran.all()
        );
        assert!(
            ran.all().contains(&child.display().to_string()),
            "{args:?}: names the project: {}",
            ran.all()
        );
        assert_eq!(kit.term.reads(), 0, "{args:?}");
    }
    assert_eq!(env.history().len(), 1);

    let (ran, _, sent) = test_counting(&env, "cloud", CANARY, &[]).await;
    assert_eq!(ran.code, 0, "test still runs: {}", ran.all());
    assert_eq!(sent, 1);
}

// ---------------------------------------------------------------- the lock

/// Changed since the sentence: while the add waits at the id prompt,
/// another process adds the same id (an app write, with its History). The
/// fresh re-read under `user.lock` sees it, so the add is refused, to be run
/// again; nothing of this add is written or stored.
#[tokio::test]
async fn the_same_id_added_by_another_process_meanwhile_refuses_the_add() {
    let env = env();
    let places = env.places.clone();
    let kit = Kit::new(FakeTerminal::new());
    kit.term.line_and("cloud", move || {
        let guard = UserStore::locked(&places.user, Duration::from_secs(10)).unwrap();
        let writer =
            am_config::ConfigWriter::for_user_accounts(places.user.dir(), Arc::new(guard)).unwrap();
        writer
            .create(
                owner(),
                am_config::ConfigCreate {
                    file: USER_FILE.into(),
                    bytes: am_config::new_accounts_file(
                        "cloud",
                        "anthropic",
                        None,
                        None,
                        am_config::KeyState::Stored,
                    )
                    .unwrap(),
                    audit: Vec::new(),
                },
            )
            .unwrap();
    });
    kit.term.line(CANARY);

    let ran = kit
        .run(&env, &["add", "cloud", "--provider", "anthropic"])
        .await;

    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(ran.all().contains("again"), "run it again: {}", ran.all());
    assert_eq!(kit.key("cloud"), None, "no key stored");
    assert_eq!(env.history().len(), 1, "only the other process's write");
    assert!(audit_lines(&env.audit(), "account_added").is_empty());
}

/// Concurrency: two adds of different ids at once, each on its own thread
/// (its own runtime and its own `user.lock` file description, as two
/// processes have): both succeed, one after the other, and the user store's
/// audit chain verifies.
#[test]
fn two_adds_at_once_take_turns_and_the_chain_verifies() {
    let env = env();
    let keychain = FakeSecretStore::new();
    let barrier = Arc::new(Barrier::new(2));
    let env = Arc::new(env);
    let handles: Vec<_> = ["first", "second"]
        .into_iter()
        .map(|id| {
            let (env, keychain, barrier) = (env.clone(), keychain.clone(), barrier.clone());
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                barrier.wait();
                rt.block_on(add(&env, &keychain, id, "anthropic", None, CANARY))
            })
        })
        .collect();
    for h in handles {
        let ran = h.join().unwrap();
        assert_eq!(ran.code, 0, "{}", ran.all());
    }
    let loaded = am_config::user_accounts::load(&env.accounts_path());
    let mut ids = loaded.accounts.ids();
    ids.sort();
    assert_eq!(ids, ["first", "second"]);
    let reader = UserReader::open(&env.places.user).unwrap();
    assert_eq!(reader.verify_audit_chain().unwrap(), None);
    assert_eq!(audit_lines(&env.audit(), "account_added").len(), 2);
}

/// The child half of the lock-held row: run as `<this test binary> --exact
/// lock_holder_child --ignored` with `AM_TEST_HOLD_USER_LOCK` naming a
/// `user.lock`, it takes that lock (an advisory `flock`, as std's
/// `File::lock` does) and holds it for `AM_TEST_HOLD_SECS` seconds.
#[test]
#[ignore = "a helper the lock test runs as a child process"]
fn lock_holder_child() {
    let Some(path) = std::env::var_os("AM_TEST_HOLD_USER_LOCK") else {
        return;
    };
    let secs: u64 = std::env::var("AM_TEST_HOLD_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(20);
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.lock().unwrap();
    println!("holding");
    std::thread::sleep(Duration::from_secs(secs));
    drop(file);
}

/// Lock held: a child process holds `user.lock` past the deadline (10 s,
/// polled); the add is refused in words after the bounded wait, naming the
/// lock, and nothing is written or stored.
#[tokio::test]
async fn an_add_waits_its_bounded_time_for_a_held_lock_then_refuses() {
    let env = env();
    let keychain = FakeSecretStore::new();
    let ran = add(&env, &keychain, "cloud", "anthropic", None, CANARY).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    let before = env.user_text();

    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "lock_holder_child", "--ignored", "--nocapture"])
        .env("AM_TEST_HOLD_USER_LOCK", env.places.user.user_lock())
        .env("AM_TEST_HOLD_SECS", "25")
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    // Wait until the child says it holds the lock.
    let started = Instant::now();
    while UserStore::locked(&env.places.user, Duration::from_millis(50)).is_ok() {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the child never took the lock"
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    let kit = Kit::with_secrets(FakeTerminal::new(), keychain.clone());
    kit.term.line("cloud2");
    kit.term.line(CANARY);
    let started = Instant::now();
    let ran = kit
        .run(&env, &["add", "cloud2", "--provider", "anthropic"])
        .await;
    let waited = started.elapsed();
    let _ = child.kill();
    let _ = child.wait();

    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(
        waited >= Duration::from_secs(9),
        "it waits its bounded time: {waited:?}"
    );
    assert!(
        waited < Duration::from_secs(20),
        "and no longer: {waited:?}"
    );
    assert!(
        ran.all().contains("user.lock"),
        "names the lock: {}",
        ran.all()
    );
    assert_eq!(kit.key("cloud2"), None, "no key stored");
    assert_eq!(env.user_text(), before, "nothing written");
    assert_eq!(env.history().len(), 1);
}

/// In flight: `test` while a write holds `user.lock` with the new file
/// already renamed (its intent pending, its History not yet committed):
/// the bytes match the pending intent, so the check waits for the lock and
/// then passes against the new History entry.
#[tokio::test]
async fn a_test_during_a_write_waits_for_it_and_passes() {
    let env = env();
    let keychain = FakeSecretStore::new();
    let ran = add(&env, &keychain, "cloud", "anthropic", None, CANARY).await;
    assert_eq!(ran.code, 0, "{}", ran.all());

    let (renamed_tx, renamed_rx) = mpsc::channel();
    let places = env.places.clone();
    let writing = std::thread::spawn(move || {
        let guard = UserStore::locked(&places.user, Duration::from_secs(10)).unwrap();
        let writer =
            am_config::ConfigWriter::for_user_accounts(places.user.dir(), Arc::new(guard)).unwrap();
        writer.fail_at(Some(am_config::FailPoint::AfterRename));
        let now = am_config::accounts_file(places.user.dir());
        writer
            .apply_with_audit(
                owner(),
                am_config::add_account_edit(
                    &now,
                    "oa",
                    "openai",
                    None,
                    None,
                    am_config::KeyState::Stored,
                ),
                now.sha256.as_deref().unwrap(),
                Vec::new(),
            )
            .unwrap_err();
        renamed_tx.send(()).unwrap();
        std::thread::sleep(Duration::from_millis(800));
        writer.fail_at(None);
        writer.reconcile(owner()).unwrap();
        drop(writer);
    });
    renamed_rx.recv().unwrap();

    let (ran, gets, sent) = test_counting(&env, "cloud", CANARY, &[]).await;
    writing.join().unwrap();
    assert_eq!(ran.code, 0, "waits, then passes: {}", ran.all());
    assert_eq!(sent, 1);
    assert!(gets >= 1);
    let history = env.history();
    assert_eq!(history.len(), 2, "the write's entry committed");
    assert_eq!(
        history[1].after,
        std::fs::read(env.accounts_path()).unwrap()
    );
}

// ---------------------------------------------- validate with the user list

/// The OS app-data folder the binary resolves for `home` (and `xdg` off
/// macOS), and the places in it.
fn support(home: &Path, xdg: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support")
    } else {
        xdg.to_path_buf()
    }
}

/// A temporary HOME whose app data dir holds `accounts` (when given) as the
/// user Accounts file, owner-only.
struct Home {
    dir: tempfile::TempDir,
}

impl Home {
    fn new(accounts: Option<&Path>) -> Home {
        let dir = tempfile::tempdir().unwrap();
        let h = Home { dir };
        std::fs::create_dir_all(h.home()).unwrap();
        if let Some(file) = accounts {
            let places = Places::under(&support(&h.home(), &h.xdg()), &h.home());
            std::fs::create_dir_all(places.user.dir()).unwrap();
            set_mode(places.user.dir(), 0o700);
            std::fs::write(places.user.accounts_file(), std::fs::read(file).unwrap()).unwrap();
            set_mode(&places.user.accounts_file(), 0o600);
        }
        h
    }
    fn home(&self) -> PathBuf {
        self.dir.path().join("home")
    }
    fn xdg(&self) -> PathBuf {
        self.dir.path().join("xdg")
    }
    fn validate(&self, args: &[&str]) -> (i32, String) {
        let out = Command::new(env!("CARGO_BIN_EXE_agentic"))
            .arg("validate")
            .args(args)
            .env("HOME", self.home())
            .env("XDG_DATA_HOME", self.xdg())
            .output()
            .unwrap();
        (
            out.status.code().unwrap_or(-1),
            format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ),
        )
    }
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(name: &str) -> PathBuf {
    repo().join("fixtures/projects").join(name)
}

fn accounts_fixture(variant: &str) -> PathBuf {
    repo()
        .join("fixtures/accounts")
        .join(format!("{variant}.yaml"))
}

/// A copy of a fixture project in a temporary folder.
fn copy_of(name: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    am_testkit::copy_tree(&fixture(name), dir.path());
    dir
}

/// Missing id: a seat names `anthropic` and this Mac's user list lacks it
/// (no Accounts file at all): `agentic validate` warns at the seat line with
/// the add fix and exits 0; with the Account on this Mac it is silent; and
/// a key path (the project's egress directory) refuses it in words.
#[tokio::test]
async fn a_missing_id_is_a_validate_warning_and_a_key_path_refusal() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("p");
    std::fs::create_dir_all(root.join("seats")).unwrap();
    std::fs::write(root.join("company.yaml"), COMPANY).unwrap();
    std::fs::write(root.join("seats/alpha.md"), seat("anthropic")).unwrap();

    let empty = Home::new(None);
    let (code, out) = empty.validate(&[root.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("seats/alpha.md:10: warning:"), "{out}");
    assert!(out.contains("agentic accounts add anthropic"), "{out}");

    let with = Home::new(Some(&accounts_fixture("sample")));
    let (code, out) = with.validate(&[root.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("agentic accounts add"), "{out}");

    let env = env();
    let project = env.naming("p", "anthropic");
    let directory = am_app::egress::ProjectDirectory::open(&project, &env.places).unwrap();
    assert_eq!(directory.account_endpoint("anthropic"), None);
    let why = directory.account_refusal("anthropic").expect("a reason");
    assert!(why.contains("agentic accounts add anthropic"), "{why}");
}

/// Old project file: `agentic validate` on a project holding
/// `models/accounts.yaml` exits 1 with an error at that file with the
/// migrate fix; `--template` refuses it the same way.
#[test]
fn an_old_project_file_fails_validate_with_the_migrate_fix() {
    let home = Home::new(Some(&accounts_fixture("valid")));
    let legacy = copy_of("legacy-accounts");
    for args in [
        vec![legacy.path().to_str().unwrap()],
        vec!["--template", legacy.path().to_str().unwrap()],
    ] {
        let (code, out) = home.validate(&args);
        assert_eq!(code, 1, "{args:?}: {out}");
        assert!(out.contains("models/accounts.yaml:1:"), "{args:?}: {out}");
        assert!(
            out.contains("agentic accounts migrate --project"),
            "{args:?}: {out}"
        );
    }
}

/// The user file's own errors are `agentic validate` errors (a
/// `secret_ref` naming another entry), naming the file and line.
#[test]
fn validate_reports_the_user_files_own_errors() {
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("bad.yaml");
    std::fs::write(
        &bad,
        "schema_version: 2\naccounts:\n  - id: cloud\n    provider: anthropic\n    secret_ref: other\n",
    )
    .unwrap();
    let home = Home::new(Some(&bad));
    let project = copy_of("one-step");
    let (code, out) = home.validate(&[project.path().to_str().unwrap()]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("accounts.yaml:5:"), "{out}");
    assert!(out.contains("secret_ref"), "{out}");
}

/// Templates: `validate --template` on the sample company needs no
/// Accounts file (references are checked for form only); each fixture
/// project validated as a project, with its `fixtures/accounts` variant as
/// this Mac's list, has no error and no missing-id warning; and the broken
/// anatomy fixture's wrong-kind Account is an error once the kind is known.
#[test]
fn templates_and_fixtures_validate_with_their_accounts_variant() {
    let nobody = Home::new(None);
    let samples = repo().join("templates/samples");
    for e in std::fs::read_dir(&samples).unwrap() {
        let sample = e.unwrap().path();
        let (code, out) = nobody.validate(&["--template", sample.to_str().unwrap()]);
        assert_eq!(code, 0, "{out}");
        // Story 3.6's ToolGate slice (matrix row 40, owner 2026-11-24): the
        // sample's one warning is that its chained flow's fan-out step
        // cannot write a table until story 3.14 (crates/am-e2e/tests/
        // cli_validate.rs pins it); no Account warning.
        assert!(out.contains("0 errors, 1 warning"), "{out}");
        assert!(out.contains("3.14"), "{out}");
    }

    for (project, variant) in [
        ("valid", "valid"),
        ("anatomy", "anatomy"),
        ("sources-ok", "sources"),
    ] {
        let home = Home::new(Some(&accounts_fixture(variant)));
        let copy = copy_of(project);
        let (code, out) = home.validate(&[copy.path().to_str().unwrap()]);
        assert_eq!(code, 0, "{project}: {out}");
        assert!(!out.contains("agentic accounts add"), "{project}: {out}");
    }

    let home = Home::new(Some(&accounts_fixture("anatomy")));
    let copy = copy_of("broken-anatomy");
    let (code, out) = home.validate(&[copy.path().to_str().unwrap()]);
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains("seats/searcher.md:9: account 'crawly' is a crawl-service Account"),
        "{out}"
    );
}

/// Moved from `crates/am-e2e/tests/cli_validate.rs` (it read the real
/// HOME): a project with no runtime folder on this Mac skips the reference
/// file warning, and validate never writes a `project_id`.
#[test]
fn a_project_without_a_runtime_folder_skips_the_reference_file_warning_and_gets_no_id() {
    let home = Home::new(Some(&accounts_fixture("anatomy")));
    let copy = copy_of("anatomy");
    let company = copy.path().join("company.yaml");
    let before = std::fs::read(&company).unwrap();
    let (code, out) = home.validate(&[copy.path().to_str().unwrap()]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("absent.md"), "{out}");
    assert!(out.contains("0 errors, 2 warnings"), "{out}");
    assert_eq!(std::fs::read(&company).unwrap(), before, "no id is written");
}

/// Moved from `crates/am-e2e/tests/cli_validate.rs` (the project half of
/// `template_mode_refuses_a_project_id_with_file_line_and_fix`): as a
/// project, a folder carrying a `project_id` is fine, and validate writes
/// nothing.
#[test]
fn as_a_project_a_project_id_is_fine_and_validate_writes_nothing() {
    let home = Home::new(None);
    let dir = tempfile::tempdir().unwrap();
    am_testkit::copy_tree(&repo().join("templates/departments/blank"), dir.path());
    let company = dir.path().join("company.yaml");
    let text = std::fs::read_to_string(&company).unwrap();
    let version = text
        .lines()
        .find(|l| l.starts_with("schema_version:"))
        .unwrap()
        .to_string();
    let id = am_domain::ProjectId::mint();
    std::fs::write(
        &company,
        text.replacen(
            &format!("{version}\n"),
            &format!("{version}\nproject_id: {id}\n"),
            1,
        ),
    )
    .unwrap();
    let before = std::fs::read(&company).unwrap();
    let (code, out) = home.validate(&[dir.path().to_str().unwrap()]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(std::fs::read(&company).unwrap(), before);
}

/// The runtime base and the app data dir stay apart: neither holds the
/// other (AD-32), whatever HOME is.
#[test]
fn the_app_data_dir_is_not_inside_the_runtime_base() {
    let home = Home::new(None);
    let places = Places::under(&support(&home.home(), &home.xdg()), &home.home());
    let base = RuntimeBase::new(support(&home.home(), &home.xdg()).join(RUNTIME_BASE_NAME));
    assert_eq!(places.base, base);
    assert!(!places.user.dir().starts_with(base.dir()));
    assert!(!base.dir().starts_with(places.user.dir()));
    assert_eq!(places.user.dir(), places.app_data.as_path());
}

// ------------------------------------------- re-check after the key read

/// A Keychain whose first read commits a change of the Account (another
/// process re-pointing it, as story 14's `change` will) before it answers.
struct ChangingStore {
    inner: Arc<FakeSecretStore>,
    places: Places,
    gets: AtomicUsize,
}

impl SecretStore for ChangingStore {
    fn get(
        &self,
        account: &str,
    ) -> Result<Option<am_domain::SecretString>, am_domain::SecretError> {
        if self.gets.fetch_add(1, Ordering::SeqCst) == 0 {
            let guard = UserStore::locked(&self.places.user, Duration::from_secs(10)).unwrap();
            let writer =
                am_config::ConfigWriter::for_user_accounts(self.places.user.dir(), Arc::new(guard))
                    .unwrap();
            writer
                .apply(
                    owner(),
                    am_config::ConfigEdit {
                        file: USER_FILE.into(),
                        ops: vec![am_config::EditOp::Set {
                            path: vec!["accounts".into(), "0".into(), "base_url".into()],
                            value: "https://elsewhere.example/v1".into(),
                        }],
                        body: None,
                    },
                )
                .unwrap();
        }
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

/// Key use (AD-15, tightened 2026-11-22 after the reviewer gate): after the
/// key is read and before it is sent, the caller reads the id's latest
/// user-store sequence again; a record that moved meanwhile drops the key
/// unsent and the check runs again, which now refuses.
#[tokio::test]
async fn a_record_that_moves_while_the_key_is_read_drops_the_key_unsent() {
    let env = env();
    let keychain = FakeSecretStore::new();
    let ran = add(
        &env,
        &keychain,
        "router",
        "openai-compatible",
        Some(REMOTE_URL),
        ROUTER_CANARY,
    )
    .await;
    assert_eq!(ran.code, 0, "{}", ran.all());

    let kit = Kit::with_secrets(FakeTerminal::piped(), keychain.clone());
    kit.egress
        .replay("openrouter.example", "openai_compat/reasoning.sse");
    kit.egress
        .replay("elsewhere.example", "openai_compat/reasoning.sse");
    let changing = Arc::new(ChangingStore {
        inner: keychain.clone(),
        places: env.places.clone(),
        gets: AtomicUsize::new(0),
    });
    let ran = kit
        .run_with(&env, &["test", "router", "--model", "m"], changing.clone())
        .await;
    assert_ne!(ran.code, 0, "refused after the re-check: {}", ran.all());
    assert_eq!(kit.egress.requests().len(), 0, "the key read is never sent");
    assert!(
        ran.all().contains("elsewhere.example"),
        "the refusal names the file's new host: {}",
        ran.all()
    );
    assert!(!ran.all().contains(ROUTER_CANARY));
}

// ============================================================================
// Review fixes (2026-11-22 review pass; plan `## Review Triage Log`), written
// first, before any `src/` change. Each test names its finding.
// ============================================================================

/// am-e2e's RULE-9 fakes: a scripted resolver and a scripted per-hop
/// transport for a real `am_net::NetEgress` (no network).
#[path = "../../am-e2e/tests/trust/support/egress.rs"]
mod net_rig;

use am_domain::{AuthScheme, EgressError, HttpRequest};

const ANTHROPIC_MESSAGES: &str = "https://api.anthropic.com/v1/messages";
const ROUTER_COMPLETIONS: &str = "https://openrouter.example/api/v1/chat/completions";

/// A real egress client (am-net's `NetEgress` on the scripted resolver and
/// transport) over `directory`, reading keys from `secrets`.
struct Wire {
    egress: am_net::NetEgress,
    transport: Arc<net_rig::ScriptedTransport>,
}

fn resolver() -> Arc<net_rig::ScriptedResolver> {
    let resolver = net_rig::ScriptedResolver::standard();
    resolver.answer("openrouter.example", &[net_rig::OTHER_IP]);
    resolver.answer("elsewhere.example", &[net_rig::FILES_IP]);
    resolver
}

fn wire(
    directory: Arc<dyn EgressDirectory>,
    secrets: Arc<dyn SecretStore>,
    audit: Arc<dyn AuditSink>,
) -> Wire {
    let transport = net_rig::ScriptedTransport::new();
    let egress = am_net::NetEgress::with_seams(
        directory,
        secrets,
        audit,
        am_net::Seams {
            resolver: resolver(),
            transport: transport.clone(),
            limits: net_rig::roomy(),
        },
    );
    Wire { egress, transport }
}

fn counting(secrets: &Arc<FakeSecretStore>) -> Arc<CountingStore> {
    Arc::new(CountingStore {
        inner: secrets.clone(),
        gets: AtomicUsize::new(0),
    })
}

/// A keyed call of the Account `id`: a POST to `url` carrying a reference
/// to its key, which the client reads from the Keychain.
fn keyed_call(id: &str, url: &str, scheme: AuthScheme) -> HttpRequest {
    net_rig::keyed(net_rig::post(net_rig::account(id), url), scheme, id)
}

/// The refusal's words; panics unless the client refused (`Blocked`).
fn refused_call(result: Result<am_domain::HttpResponse, EgressError>, what: &str) -> String {
    match result {
        Err(EgressError::Blocked(words)) => words,
        Err(other) => panic!("{what}: expected a refusal (Blocked), got {other:?}"),
        Ok(resp) => panic!(
            "{what}: expected a refusal, the call was sent (HTTP {})",
            resp.status
        ),
    }
}

/// The `account_added` record `agentic accounts add` writes for an
/// Anthropic Account (AD-15 *Record*).
fn added_note(id: &str) -> am_domain::AuditNote {
    am_domain::AuditNote {
        action: "config.widened".into(),
        target: id.into(),
        args: [
            ("kind", "account_added"),
            ("file", USER_FILE),
            ("account", id),
            ("provider", "anthropic"),
            ("scheme", "https"),
            ("host", "api.anthropic.com"),
            ("port", "443"),
            ("references", ""),
            ("key", "stored"),
            ("confirmation", "typed_id"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect(),
    }
}

/// The raw control characters in `text` (a line feed and a tab aside).
fn raw_controls(text: &str) -> Vec<char> {
    text.chars()
        .filter(|c| c.is_control() && *c != '\n' && *c != '\t')
        .collect()
}

// ------------------------------------------------- G1: send-time key use

/// G1 (Quick 1, Adversarial 1): the client of a project is built while its
/// Account is confirmed and sends with it; then the Account is removed
/// (`account_removed`). The next call through the same client is refused:
/// nothing reaches the transport and the Keychain is not read again. The
/// directory checks every Account answer at the call, never from a map
/// filled when it was built.
#[tokio::test]
async fn an_account_removed_after_the_client_was_built_is_refused_unsent() {
    let env = env();
    let project = env.naming("p", "local-ollama");
    let keychain = FakeSecretStore::new();
    let ran = add(&env, &keychain, "cloud", "anthropic", None, CANARY).await;
    assert_eq!(ran.code, 0, "{}", ran.all());

    let directory = am_app::egress::ProjectDirectory::open(&project, &env.places).unwrap();
    let secrets = counting(&keychain);
    let wire = wire(
        Arc::new(directory),
        secrets.clone(),
        am_testkit::CollectAudit::new(),
    );
    let sent = wire
        .egress
        .send(keyed_call("cloud", ANTHROPIC_MESSAGES, AuthScheme::XApiKey))
        .await;
    assert!(
        sent.is_ok(),
        "confirmed: the call is sent: {:?}",
        sent.err()
    );
    assert_eq!(wire.transport.hops().len(), 1);
    assert_eq!(secrets.gets.load(Ordering::SeqCst), 1);

    let kit = Kit::with_secrets(FakeTerminal::new(), keychain.clone());
    let ran = kit.run(&env, &["remove", "cloud"]).await;
    assert_eq!(ran.code, 0, "{}", ran.all());

    let result = wire
        .egress
        .send(keyed_call("cloud", ANTHROPIC_MESSAGES, AuthScheme::XApiKey))
        .await;
    assert_eq!(
        wire.transport.hops().len(),
        1,
        "nothing more reaches the transport after the removal"
    );
    assert_eq!(
        secrets.gets.load(Ordering::SeqCst),
        1,
        "the Keychain is not read for a removed Account"
    );
    refused_call(result, "an Account removed after the client was built");
}

/// G1 (Quick 1, Adversarial 1): as above, with a hand edit of the user file
/// (one byte) after the client was built: the next call is refused, nothing
/// is sent and the Keychain is not read.
#[tokio::test]
async fn a_hand_edit_after_the_client_was_built_is_refused_unsent() {
    let env = env();
    let project = env.naming("p", "local-ollama");
    let keychain = FakeSecretStore::new();
    let ran = add(&env, &keychain, "cloud", "anthropic", None, CANARY).await;
    assert_eq!(ran.code, 0, "{}", ran.all());

    let directory = am_app::egress::ProjectDirectory::open(&project, &env.places).unwrap();
    let secrets = counting(&keychain);
    let wire = wire(
        Arc::new(directory),
        secrets.clone(),
        am_testkit::CollectAudit::new(),
    );
    let written = env.user_text().unwrap();
    std::fs::write(env.accounts_path(), format!("{written}\n")).unwrap();

    let result = wire
        .egress
        .send(keyed_call("cloud", ANTHROPIC_MESSAGES, AuthScheme::XApiKey))
        .await;
    assert_eq!(wire.transport.hops().len(), 0, "nothing is sent");
    assert_eq!(secrets.gets.load(Ordering::SeqCst), 0, "no key is read");
    refused_call(result, "a hand edit after the client was built");
}

/// G1 (Quick 1, Adversarial 2; AD-15 *Key use*, re-check after the read):
/// the Keychain read the client itself makes commits a change of the
/// Account (another process re-points it) before it answers. The client
/// reads the stamp again after its key read, so the key is dropped unsent:
/// the transport receives nothing, and the result is a refusal naming the
/// change.
#[tokio::test]
async fn a_change_committed_while_the_client_reads_the_key_is_never_sent() {
    let env = env();
    let project = env.naming("p", "local-ollama");
    let keychain = FakeSecretStore::new();
    let ran = add(
        &env,
        &keychain,
        "router",
        "openai-compatible",
        Some(REMOTE_URL),
        ROUTER_CANARY,
    )
    .await;
    assert_eq!(ran.code, 0, "{}", ran.all());

    let directory = am_app::egress::ProjectDirectory::open(&project, &env.places).unwrap();
    let changing = Arc::new(ChangingStore {
        inner: keychain.clone(),
        places: env.places.clone(),
        gets: AtomicUsize::new(0),
    });
    let wire = wire(
        Arc::new(directory),
        changing.clone(),
        am_testkit::CollectAudit::new(),
    );

    let result = wire
        .egress
        .send(keyed_call("router", ROUTER_COMPLETIONS, AuthScheme::Bearer))
        .await;
    assert_eq!(
        wire.transport.hops().len(),
        0,
        "the key read while the Account moved is never sent"
    );
    let words = refused_call(result, "a change committed during the key read");
    assert!(
        words.contains("elsewhere.example") || words.contains("changed"),
        "the refusal names the change: {words}"
    );
    assert!(!words.contains(ROUTER_CANARY));
}

/// G1 (Quick 1, Adversarial 2): the key `agentic accounts test` sends is the
/// one the egress client reads itself, between its own two stamp checks (a
/// real `NetEgress` built by the command's factory on the scripted
/// transport). The command keeps one earlier read, which only checks that a
/// key is present and is never sent (coordinator decision after the review,
/// logged in the plan): two reads in all, never more.
#[tokio::test]
async fn accounts_test_reads_the_key_once_inside_the_egress_client() {
    let env = env();
    let keychain = FakeSecretStore::new();
    let ran = add(&env, &keychain, "cloud", "anthropic", None, CANARY).await;
    assert_eq!(ran.code, 0, "{}", ran.all());

    let secrets = counting(&keychain);
    let transport = net_rig::ScriptedTransport::new();
    transport.on(
        ANTHROPIC_MESSAGES,
        net_rig::Reply {
            status: 200,
            headers: vec![("content-type".into(), "text/event-stream".into())],
            chunks: vec![am_testkit::recorded("anthropic/c1_count_stream.sse")],
            head_delay: Duration::ZERO,
            stalls: false,
        },
    );
    let (client_secrets, client_transport) = (secrets.clone(), transport.clone());
    let factory: am_app::accounts::EgressFactory = Arc::new(
        move |directory: Arc<dyn EgressDirectory>, audit: Arc<dyn AuditSink>| {
            Arc::new(am_net::NetEgress::with_seams(
                directory,
                client_secrets.clone(),
                audit,
                am_net::Seams {
                    resolver: resolver(),
                    transport: client_transport.clone(),
                    limits: net_rig::roomy(),
                },
            )) as Arc<dyn EgressClient>
        },
    );
    let term = FakeTerminal::piped();
    let ctx = AccountsContext {
        places: env.places.clone(),
        identity: Arc::new(FakeProjectIdentity::new("Test Mac")),
        terminal: Box::new(term.clone()),
        secrets: secrets.clone(),
        egress: factory,
    };
    let argv: Vec<String> = ["test", "cloud"].iter().map(|s| s.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = accounts(&argv, ctx, &mut out, &mut err).await;
    let all = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out),
        String::from_utf8_lossy(&err)
    );

    assert_eq!(code, 0, "{all}");
    assert_eq!(transport.hops().len(), 1, "one call: {all}");
    assert_eq!(
        secrets.gets.load(Ordering::SeqCst),
        2,
        "the command's presence check and the client's own guarded read: {all}"
    );
    assert!(!all.contains(CANARY));
}

// ------------------------------------------------------------- Quick 2

/// Quick 2 (AD-15 *Add*): the Keychain already holds a key for `router`,
/// and the user store's latest `account_added` of `router` (with an
/// `account_removed` after it) names provider openai-compatible at
/// openrouter.example. A new add of `router` says where that key was last
/// confirmed: the provider and the host.
#[tokio::test]
async fn add_names_where_a_reused_key_was_last_confirmed_even_after_a_removal() {
    let env = env();
    let keychain = FakeSecretStore::new();
    let ran = add(
        &env,
        &keychain,
        "router",
        "openai-compatible",
        Some(REMOTE_URL),
        ROUTER_CANARY,
    )
    .await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    let kit = Kit::with_secrets(FakeTerminal::new(), keychain.clone());
    let ran = kit.run(&env, &["remove", "router"]).await;
    assert_eq!(ran.code, 0, "{}", ran.all());

    let kit = Kit::with_secrets(FakeTerminal::new(), keychain.clone());
    kit.term.line("router");
    kit.term.line(ROUTER_CANARY);
    let ran = kit
        .run(&env, &["add", "router", "--provider", "anthropic"])
        .await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(
        ran.shown.contains("openai-compatible"),
        "the sentence names the provider the key was last confirmed for: {}",
        ran.shown
    );
    assert!(
        ran.shown.contains("openrouter.example"),
        "and the host: {}",
        ran.shown
    );
    assert!(!ran.all().contains(ROUTER_CANARY));
}

// ------------------------------------------------------------------ G5

/// G5 (Quick 5, Adversarial 8a): on a fresh HOME, `agentic serve`'s
/// serving record is the first thing to create the app data dir; it leaves
/// it 0700 (not the 0755 a default `create_dir_all` gives under a normal
/// umask), and an Accounts command afterwards is not refused for
/// permissions.
#[tokio::test]
async fn a_serving_record_on_a_fresh_mac_leaves_the_app_data_dir_owner_only() {
    let env = env();
    let app_data = env.places.app_data.clone();
    assert!(!app_data.exists());
    let recorded = am_app::runtime::record_serving(&app_data, 47_001);
    let mode_after = std::fs::metadata(&app_data)
        .map(|m| m.permissions().mode() & 0o777)
        .ok();
    let keychain = FakeSecretStore::new();
    let ran = add(&env, &keychain, "cloud", "anthropic", None, CANARY).await;
    am_app::runtime::clear_serving(&app_data);

    assert!(recorded.is_ok(), "{recorded:?}");
    assert_eq!(mode_after, Some(0o700), "the app data dir is owner-only");
    assert_eq!(ran.code, 0, "not refused for permissions: {}", ran.all());
    assert!(!ran.all().contains("chmod"), "{}", ran.all());
}

// ------------------------------------------------------------- Quick 7

/// Quick 7: a write that stopped after its rename (the file holds a pending
/// intent's new bytes) and whose process is gone (no lock holder). `test`
/// is refused before any Keychain read, in words that say a write was
/// interrupted and that the next `agentic accounts add` or `remove` settles
/// it, not that the file was changed outside the app.
#[tokio::test]
async fn an_interrupted_write_is_named_as_one_and_not_as_an_outside_edit() {
    let env = env();
    let keychain = FakeSecretStore::new();
    let ran = add(&env, &keychain, "cloud", "anthropic", None, CANARY).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    {
        let writer = env.user_writer();
        writer.fail_at(Some(am_config::FailPoint::AfterRename));
        let now = am_config::accounts_file(env.places.user.dir());
        writer
            .apply_with_audit(
                owner(),
                am_config::add_account_edit(
                    &now,
                    "oa",
                    "openai",
                    None,
                    None,
                    am_config::KeyState::Stored,
                ),
                now.sha256.as_deref().unwrap(),
                Vec::new(),
            )
            .unwrap_err();
    }
    assert!(
        !am_store::user::user_lock_is_held(&env.places.user),
        "no lock holder"
    );

    let (ran, gets, sent) = test_counting(&env, "cloud", CANARY, &[]).await;
    assert_ne!(ran.code, 0, "{}", ran.all());
    assert_eq!((gets, sent), (0, 0));
    let words = ran.all();
    assert!(words.contains("interrupted"), "{words}");
    assert!(words.contains("agentic accounts add"), "{words}");
    assert!(words.contains("remove"), "{words}");
    assert!(
        !words.contains("outside the app"),
        "not an outside edit: {words}"
    );
    assert!(!words.contains("by hand"), "not an outside edit: {words}");
}

// -------------------------------------------------------- Adversarial 3

/// A key pasted as `secret_ref:` in the user file (Adversarial 3).
const PASTED: &str = "sk-ant-pasted-canary-7c7c7c7c7c7c-PSTD";

fn pasted_file() -> String {
    format!(
        "schema_version: 2\naccounts:\n  - id: cloud\n    provider: anthropic\n    secret_ref: {PASTED}\n"
    )
}

/// Adversarial 3: a key pasted as `secret_ref:` in the user file. `agentic
/// accounts test` refuses the Account without printing the value.
#[tokio::test]
async fn accounts_test_never_prints_a_key_pasted_as_secret_ref() {
    let env = env();
    env.write_user_file(&pasted_file());

    let (ran, gets, sent) = test_counting(&env, "cloud", CANARY, &[]).await;
    assert_ne!(ran.code, 0, "{}", ran.all());
    assert_eq!((gets, sent), (0, 0));
    assert!(
        !ran.all().contains(PASTED),
        "`test` prints the pasted key: {}",
        ran.all()
    );
}

/// Adversarial 3: an egress refusal of that Account through a project's
/// directory neither returns the pasted key nor writes it into the
/// project's audit chain (the block records a fixed reason, not loader
/// words).
#[tokio::test]
async fn an_egress_refusal_never_returns_or_audits_a_key_pasted_as_secret_ref() {
    let env = env();
    let project = env.naming("p", "local-ollama");
    env.write_user_file(&pasted_file());

    let tmp = tempfile::tempdir().unwrap();
    let (store, _) = am_store::open_runtime(
        &am_domain::RuntimeLayout::at(tmp.path().join("rt")),
        "test-mac".into(),
    )
    .unwrap();
    let sink: Arc<dyn AuditSink> = Arc::new(am_app::egress::StoreAuditSink::new(store.writer()));
    let directory = am_app::egress::ProjectDirectory::open(&project, &env.places).unwrap();
    let secrets = counting(&FakeSecretStore::new());
    let wire = wire(Arc::new(directory), secrets.clone(), sink);
    let result = wire
        .egress
        .send(keyed_call("cloud", ANTHROPIC_MESSAGES, AuthScheme::XApiKey))
        .await;
    let words = refused_call(result, "a pasted secret_ref");
    assert_eq!(secrets.gets.load(Ordering::SeqCst), 0);
    let mut export = Vec::new();
    store
        .reader()
        .unwrap()
        .export_audit_jsonl(&mut export)
        .unwrap();
    let export = String::from_utf8(export).unwrap();
    assert!(
        export.contains("egress.blocked"),
        "the block is audited: {export}"
    );
    assert!(
        !export.contains(PASTED),
        "the project's audit chain holds the pasted key: {export}"
    );
    assert!(!words.contains(PASTED), "the refusal returns it: {words}");
}

/// Adversarial 3 (validate): the same pasted key never appears in `agentic
/// validate`'s output, though the error is reported at the user file.
#[test]
fn validate_never_prints_a_key_pasted_as_secret_ref() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("pasted.yaml");
    std::fs::write(&file, pasted_file()).unwrap();
    let home = Home::new(Some(&file));
    let project = copy_of("one-step");
    let (code, out) = home.validate(&[project.path().to_str().unwrap()]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("accounts.yaml:5:"), "{out}");
    assert!(
        !out.contains(PASTED),
        "validate prints the pasted key: {out}"
    );
}

// -------------------------------------------------------- Adversarial 5

/// Adversarial 5 (a): between the add's sentence and the lock, a project
/// that was reachable (and so checked) becomes unreachable. The sentence's
/// not-checked list no longer holds, so the add is refused, to be run
/// again; nothing is written or stored.
#[tokio::test]
async fn a_project_unreachable_since_the_sentence_refuses_the_add() {
    let env = env();
    let project = env.naming("p", "local-ollama");
    let kit = Kit::new(FakeTerminal::new());
    let gone = project.clone();
    kit.term
        .line_and("cloud", move || std::fs::remove_dir_all(&gone).unwrap());
    kit.term.line(CANARY);

    let ran = kit
        .run(&env, &["add", "cloud", "--provider", "anthropic"])
        .await;

    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(ran.all().contains("again"), "run it again: {}", ran.all());
    assert_eq!(kit.key("cloud"), None, "no key stored");
    assert_eq!(env.user_text(), None, "nothing written");
}

/// Adversarial 5 (b): the Keychain holds a key for `cloud` and the user
/// store has no confirmation of it, so the sentence asks for the stored
/// key. While the add waits at its prompt, another process records a
/// confirmation of `cloud` for this provider and endpoint (an add, then an
/// app write that drops the entry with no record), so the state that
/// decided the stored-key prompt changed: the add is refused, to be run
/// again, and writes nothing.
#[tokio::test]
async fn a_confirmation_recorded_since_the_sentence_refuses_the_add() {
    const STORED: &str = "sk-ant-stored-canary-3c3c3c3c-STRD";
    let env = env();
    let keychain = FakeSecretStore::new();
    keychain
        .set("cloud", am_domain::SecretString::new(STORED.to_string()))
        .unwrap();
    let places = env.places.clone();
    let kit = Kit::with_secrets(FakeTerminal::new(), keychain.clone());
    kit.term.line_and("cloud", move || {
        let guard = UserStore::locked(&places.user, Duration::from_secs(10)).unwrap();
        let writer =
            am_config::ConfigWriter::for_user_accounts(places.user.dir(), Arc::new(guard)).unwrap();
        writer
            .create(
                owner(),
                am_config::ConfigCreate {
                    file: USER_FILE.into(),
                    bytes: am_config::new_accounts_file(
                        "cloud",
                        "anthropic",
                        None,
                        None,
                        am_config::KeyState::Stored,
                    )
                    .unwrap(),
                    audit: vec![added_note("cloud")],
                },
            )
            .unwrap();
        let now = am_config::accounts_file(places.user.dir());
        writer
            .apply_with_audit(
                owner(),
                am_config::remove_account_edit(&now, "cloud").unwrap(),
                now.sha256.as_deref().unwrap(),
                Vec::new(),
            )
            .unwrap();
    });
    kit.term.line(STORED);

    let ran = kit
        .run(&env, &["add", "cloud", "--provider", "anthropic"])
        .await;

    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(ran.all().contains("again"), "run it again: {}", ran.all());
    assert_eq!(env.history().len(), 2, "only the other process's writes");
    assert_eq!(
        audit_lines(&env.audit(), "account_added").len(),
        1,
        "only the other process's record"
    );
    assert!(!env.user_text().unwrap().contains("id: cloud"));
    assert!(!ran.all().contains(STORED));
}

// ------------------------------------------------------- Adversarial 8c

/// Adversarial 8c: building a project's egress directory checks no Account
/// (each Account is checked when it is asked for), so it does not wait on
/// a user-store write in flight (`user.lock` held, the file not what the
/// app last recorded) once per Account: with two Accounts on this Mac it
/// returns well under one bounded wait (10 s).
#[tokio::test]
async fn building_the_egress_directory_waits_on_no_account() {
    let env = env();
    let project = env.naming("p", "local-ollama");
    let keychain = FakeSecretStore::new();
    for id in ["cloud", "spare"] {
        let ran = add(&env, &keychain, id, "anthropic", None, CANARY).await;
        assert_eq!(ran.code, 0, "{}", ran.all());
    }
    let guard = UserStore::locked(&env.places.user, Duration::from_secs(10)).unwrap();
    let written = env.user_text().unwrap();
    std::fs::write(env.accounts_path(), format!("{written}\n")).unwrap();

    let started = Instant::now();
    let directory = am_app::egress::ProjectDirectory::open(&project, &env.places);
    let took = started.elapsed();
    drop(guard);

    assert!(directory.is_ok());
    assert!(
        took < Duration::from_secs(3),
        "the directory waited on the Accounts it was not asked for: {took:?}"
    );
}

// ------------------------------------------------------- Adversarial 8d

/// Adversarial 8d: a project whose folder name holds control characters
/// (an ESC erase-line sequence and a carriage return, which could hide
/// where a key goes) names the Account. The add sentence and the remove
/// refusal name it with the control characters escaped: no raw control
/// character reaches the terminal.
#[tokio::test]
async fn control_characters_in_a_project_path_never_reach_the_terminal() {
    let env = env();
    let _p = env.naming("p\u{1b}[2Khidden\rshown", "cloud");
    let keychain = FakeSecretStore::new();

    let ran = add(&env, &keychain, "cloud", "anthropic", None, CANARY).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(
        ran.all().contains("hidden"),
        "the sentence names the project: {:?}",
        ran.all()
    );
    assert_eq!(
        raw_controls(&ran.all()),
        Vec::<char>::new(),
        "the add sentence: {:?}",
        ran.all()
    );

    let kit = Kit::with_secrets(FakeTerminal::new(), keychain.clone());
    let ran = kit.run(&env, &["remove", "cloud"]).await;
    assert_ne!(ran.code, 0, "still referenced: {}", ran.all());
    assert!(ran.all().contains("hidden"), "{:?}", ran.all());
    assert_eq!(
        raw_controls(&ran.all()),
        Vec::<char>::new(),
        "the remove refusal: {:?}",
        ran.all()
    );
}

// ============================================================================
// Follow-up review fixes (2026-11-22 follow-up review pass; plan `## Review
// Triage Log`), written first, before any `src/` change.
// ============================================================================

/// A resolver that runs a hook once, on the first lookup of `host`, before
/// it answers: the window after the client took an Account's endpoint and
/// before it reads the key (another process writing meanwhile).
struct HookResolver {
    inner: Arc<net_rig::ScriptedResolver>,
    host: String,
    hook: std::sync::Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

impl am_net::Resolver for HookResolver {
    fn lookup(
        &self,
        host: &str,
        port: u16,
    ) -> am_domain::BoxFuture<Result<Vec<std::net::IpAddr>, EgressError>> {
        if host == self.host
            && let Some(hook) = self.hook.lock().unwrap().take()
        {
            hook();
        }
        self.inner.lookup(host, port)
    }
}

/// The client over `directory`, whose resolver runs `hook` on the first
/// lookup of `host`.
fn hooked_wire(
    directory: Arc<dyn EgressDirectory>,
    secrets: Arc<dyn SecretStore>,
    host: &str,
    hook: impl FnOnce() + Send + 'static,
) -> Wire {
    let transport = net_rig::ScriptedTransport::new();
    let egress = am_net::NetEgress::with_seams(
        directory,
        secrets,
        am_testkit::CollectAudit::new(),
        am_net::Seams {
            resolver: Arc::new(HookResolver {
                inner: resolver(),
                host: host.into(),
                hook: std::sync::Mutex::new(Some(Box::new(hook))),
            }),
            transport: transport.clone(),
            limits: net_rig::roomy(),
        },
    );
    Wire { egress, transport }
}

/// The `account_added` record of an openai-compatible Account at `host`.
fn added_compatible_note(id: &str, host: &str) -> am_domain::AuditNote {
    let mut note = added_note(id);
    for (k, v) in [("provider", "openai-compatible"), ("host", host)] {
        note.args.insert(k.into(), v.into());
    }
    note
}

/// `router` (openai-compatible at openrouter.example) added on this Mac,
/// with a project and the project's egress directory.
async fn router_and_directory(
    env: &Env,
) -> (Arc<FakeSecretStore>, am_app::egress::ProjectDirectory) {
    let project = env.naming("p", "local-ollama");
    let keychain = FakeSecretStore::new();
    let ran = add(
        env,
        &keychain,
        "router",
        "openai-compatible",
        Some(REMOTE_URL),
        ROUTER_CANARY,
    )
    .await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    let directory = am_app::egress::ProjectDirectory::open(&project, &env.places).unwrap();
    (keychain, directory)
}

/// Follow-up finding 1 (endpoint vs stamp): the client took `router`'s endpoint
/// (openrouter.example) from one check; while it resolves that host,
/// another process removes `router` and adds it again at elsewhere.example
/// (reusing the Keychain entry, with its `account_added` naming the new
/// host). The stamps the client reads around its key read are then equal,
/// but they confirm another endpoint than the one it is about to send to:
/// nothing reaches the transport for the old host, and the call is refused.
#[tokio::test]
async fn an_account_re_pointed_while_its_host_resolves_is_never_sent_to_the_old_host() {
    let env = env();
    let (keychain, directory) = router_and_directory(&env).await;
    let places = env.places.clone();
    let wire = hooked_wire(
        Arc::new(directory),
        counting(&keychain),
        "openrouter.example",
        move || {
            let guard = UserStore::locked(&places.user, Duration::from_secs(10)).unwrap();
            let writer =
                am_config::ConfigWriter::for_user_accounts(places.user.dir(), Arc::new(guard))
                    .unwrap();
            let now = am_config::accounts_file(places.user.dir());
            writer
                .apply_with_audit(
                    owner(),
                    am_config::remove_account_edit(&now, "router").unwrap(),
                    now.sha256.as_deref().unwrap(),
                    vec![am_domain::AuditNote {
                        action: "config.narrowed".into(),
                        target: "router".into(),
                        args: [
                            ("kind", "account_removed"),
                            ("file", USER_FILE),
                            ("account", "router"),
                        ]
                        .into_iter()
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect(),
                    }],
                )
                .unwrap();
            let now = am_config::accounts_file(places.user.dir());
            writer
                .apply_with_audit(
                    owner(),
                    am_config::add_account_edit(
                        &now,
                        "router",
                        "openai-compatible",
                        None,
                        Some("https://elsewhere.example/v1"),
                        am_config::KeyState::Stored,
                    ),
                    now.sha256.as_deref().unwrap(),
                    vec![added_compatible_note("router", "elsewhere.example")],
                )
                .unwrap();
        },
    );

    let result = wire
        .egress
        .send(keyed_call("router", ROUTER_COMPLETIONS, AuthScheme::Bearer))
        .await;
    let hops = wire.transport.hops();
    assert!(
        hops.iter().all(|h| !h.url.contains("openrouter.example")),
        "the key went to the old host: {:?}",
        hops.iter().map(|h| h.url.clone()).collect::<Vec<_>>()
    );
    assert_eq!(hops.len(), 0, "nothing is sent");
    refused_call(result, "re-pointed while its host resolved");
}

/// Follow-up finding 1 (no stamp): the user file is hand-edited while the client resolves
/// the Account's host. The stamps around the key read are both absent
/// (`None == None`), which must never pass: nothing is sent, refused.
#[tokio::test]
async fn a_hand_edit_while_the_host_resolves_is_never_sent() {
    let env = env();
    let (keychain, directory) = router_and_directory(&env).await;
    let file = env.accounts_path();
    let wire = hooked_wire(
        Arc::new(directory),
        counting(&keychain),
        "openrouter.example",
        move || {
            let text = std::fs::read_to_string(&file).unwrap();
            std::fs::write(&file, format!("{text}\n")).unwrap();
        },
    );

    let result = wire
        .egress
        .send(keyed_call("router", ROUTER_COMPLETIONS, AuthScheme::Bearer))
        .await;
    assert_eq!(wire.transport.hops().len(), 0, "nothing is sent");
    refused_call(result, "a hand edit while the host resolved");
}

/// Follow-up finding 1 (no stamp): as above, with the Account removed (its
/// `account_removed` recorded) while the client resolves its host.
#[tokio::test]
async fn an_account_removed_while_its_host_resolves_is_never_sent() {
    let env = env();
    let (keychain, directory) = router_and_directory(&env).await;
    let places = env.places.clone();
    let wire = hooked_wire(
        Arc::new(directory),
        counting(&keychain),
        "openrouter.example",
        move || {
            let guard = UserStore::locked(&places.user, Duration::from_secs(10)).unwrap();
            let writer =
                am_config::ConfigWriter::for_user_accounts(places.user.dir(), Arc::new(guard))
                    .unwrap();
            let now = am_config::accounts_file(places.user.dir());
            writer
                .apply_with_audit(
                    owner(),
                    am_config::remove_account_edit(&now, "router").unwrap(),
                    now.sha256.as_deref().unwrap(),
                    vec![am_domain::AuditNote {
                        action: "config.narrowed".into(),
                        target: "router".into(),
                        args: [
                            ("kind", "account_removed"),
                            ("file", USER_FILE),
                            ("account", "router"),
                        ]
                        .into_iter()
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect(),
                    }],
                )
                .unwrap();
        },
    );

    let result = wire
        .egress
        .send(keyed_call("router", ROUTER_COMPLETIONS, AuthScheme::Bearer))
        .await;
    assert_eq!(wire.transport.hops().len(), 0, "nothing is sent");
    refused_call(result, "removed while the host resolved");
}

// ------------------------------------------------------------------ F5

/// F5 canaries: a provider key as the user file might hold it.
const F5_KEY: &str = "sk-ant-api03-f5canary-0123456789abcdef";
/// A key inside a `base_url` host, mixed case and with an underscore (so
/// the host is not a DNS name and an error that quotes the lowercased host
/// would print it lowercased).
const F5_HOST_KEY: &str = "sk-ant-api03-F5HostCanary_0a0a0a0a";
/// A 64-character lowercase-hex key.
const F5_HEX_KEY: &str = "f5c0ffeef5c0ffeef5c0ffeef5c0ffeef5c0ffeef5c0ffeef5c0ffeef5c0ffee";

/// Each placement: what it is, the user file, whether its entry is `cloud`
/// (so `test` and an egress call can name it), and the canary to look for.
fn f5_placements() -> Vec<(&'static str, String, bool, String)> {
    let head = "schema_version: 2\naccounts:\n";
    vec![
        (
            "provider quoted",
            format!("{head}  - id: cloud\n    provider: \"{F5_KEY}\"\n    secret_ref: cloud\n"),
            true,
            F5_KEY.to_string(),
        ),
        (
            "id quoted",
            format!("{head}  - id: '{F5_KEY}'\n    provider: anthropic\n"),
            false,
            F5_KEY.to_string(),
        ),
        (
            "value with a trailing comment",
            format!("{head}  - id: cloud\n    provider: {F5_KEY} # mine\n    secret_ref: cloud\n"),
            true,
            F5_KEY.to_string(),
        ),
        (
            "key in the base_url host",
            format!(
                "{head}  - id: cloud\n    provider: openai-compatible\n    base_url: https://{F5_HOST_KEY}.example/v1\n    secret_ref: cloud\n"
            ),
            true,
            F5_HOST_KEY.to_ascii_lowercase(),
        ),
        (
            "lowercase-hex key as secret_ref",
            format!("{head}  - id: cloud\n    provider: anthropic\n    secret_ref: {F5_HEX_KEY}\n"),
            true,
            F5_HEX_KEY.to_string(),
        ),
    ]
}

/// True when `text` holds `canary`, whatever its case.
fn holds(text: &str, canary: &str) -> bool {
    text.to_ascii_lowercase()
        .contains(&canary.to_ascii_lowercase())
}

/// F5: `agentic validate` never prints a key the user file holds, however
/// it is placed (quoted, as an id, before a comment, in a `base_url` host,
/// as a lowercase-hex `secret_ref`); each file still fails validate.
#[test]
fn validate_never_prints_a_key_in_the_user_file_however_placed() {
    let mut leaks = Vec::new();
    for (what, text, _, canary) in f5_placements() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("accounts.yaml");
        std::fs::write(&file, &text).unwrap();
        let home = Home::new(Some(&file));
        let project = copy_of("one-step");
        let (code, out) = home.validate(&[project.path().to_str().unwrap()]);
        assert_eq!(code, 1, "{what}: {out}");
        if holds(&out, &canary) {
            leaks.push(format!("{what}: {out}"));
        }
    }
    assert!(
        leaks.is_empty(),
        "validate printed a key:\n{}",
        leaks.join("\n")
    );
}

/// F5: `agentic accounts test cloud` never prints a key the user file
/// holds in `cloud`'s entry, however it is placed.
#[tokio::test]
async fn accounts_test_never_prints_a_key_in_the_user_file_however_placed() {
    let mut leaks = Vec::new();
    for (what, text, names_cloud, canary) in f5_placements() {
        if !names_cloud {
            continue;
        }
        let env = env();
        env.write_user_file(&text);
        let (ran, gets, sent) = test_counting(&env, "cloud", CANARY, &["--model", "m"]).await;
        assert_ne!(ran.code, 0, "{what}: {}", ran.all());
        assert_eq!((gets, sent), (0, 0), "{what}");
        if holds(&ran.all(), &canary) {
            leaks.push(format!("{what}: {}", ran.all()));
        }
    }
    assert!(
        leaks.is_empty(),
        "`test` printed a key:\n{}",
        leaks.join("\n")
    );
}

/// F5: an egress refusal of `cloud` through a project's directory neither
/// returns nor audits (in the project's chain) a key the user file holds in
/// `cloud`'s entry, however it is placed.
#[tokio::test]
async fn an_egress_refusal_never_returns_or_audits_a_key_in_the_user_file_however_placed() {
    let mut leaks = Vec::new();
    for (what, text, names_cloud, canary) in f5_placements() {
        if !names_cloud {
            continue;
        }
        let env = env();
        let project = env.naming("p", "local-ollama");
        env.write_user_file(&text);
        let tmp = tempfile::tempdir().unwrap();
        let (store, _) = am_store::open_runtime(
            &am_domain::RuntimeLayout::at(tmp.path().join("rt")),
            "test-mac".into(),
        )
        .unwrap();
        let sink: Arc<dyn AuditSink> =
            Arc::new(am_app::egress::StoreAuditSink::new(store.writer()));
        let directory = am_app::egress::ProjectDirectory::open(&project, &env.places).unwrap();
        let wire = wire(Arc::new(directory), counting(&FakeSecretStore::new()), sink);
        let result = wire
            .egress
            .send(keyed_call("cloud", ANTHROPIC_MESSAGES, AuthScheme::XApiKey))
            .await;
        let words = refused_call(result, what);
        let mut export = Vec::new();
        store
            .reader()
            .unwrap()
            .export_audit_jsonl(&mut export)
            .unwrap();
        let export = String::from_utf8(export).unwrap();
        assert!(export.contains("egress.blocked"), "{what}: {export}");
        if holds(&export, &canary) {
            leaks.push(format!("{what} (audit): {export}"));
        }
        if holds(&words, &canary) {
            leaks.push(format!("{what} (refusal): {words}"));
        }
    }
    assert!(leaks.is_empty(), "a key leaked:\n{}", leaks.join("\n"));
}

// ------------------------------------------------------------------ 8d

/// The characters that can hide or reorder text on a terminal: controls
/// (a line feed and a tab aside), bidi embeddings, overrides and isolates,
/// zero-width characters, the line and paragraph separators and the BOM.
fn hiding_chars(text: &str) -> Vec<char> {
    text.chars()
        .filter(|&c| {
            (c.is_control() && c != '\n' && c != '\t')
                || ('\u{200B}'..='\u{200F}').contains(&c)
                || ('\u{202A}'..='\u{202E}').contains(&c)
                || ('\u{2066}'..='\u{2069}').contains(&c)
                || c == '\u{2028}'
                || c == '\u{2029}'
                || c == '\u{FEFF}'
        })
        .collect()
}

/// 8d (follow-up): a project path holding a right-to-left override
/// (U+202E), a left-to-right isolate (U+2066), a zero-width space (U+200B)
/// and a line separator (U+2028) is shown escaped in the add sentence and
/// the remove refusal.
#[tokio::test]
async fn bidi_and_invisible_characters_in_a_project_path_are_escaped() {
    let env = env();
    let _p = env.naming("p\u{202E}rtl\u{2066}iso\u{200B}zw\u{2028}ls", "cloud");
    let keychain = FakeSecretStore::new();

    let ran = add(&env, &keychain, "cloud", "anthropic", None, CANARY).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(
        ran.all().contains("rtl"),
        "names the project: {:?}",
        ran.all()
    );
    assert_eq!(
        hiding_chars(&ran.all()),
        Vec::<char>::new(),
        "the add sentence: {:?}",
        ran.all()
    );

    let kit = Kit::with_secrets(FakeTerminal::new(), keychain.clone());
    let ran = kit.run(&env, &["remove", "cloud"]).await;
    assert_ne!(ran.code, 0, "still referenced: {}", ran.all());
    assert!(ran.all().contains("rtl"), "{:?}", ran.all());
    assert_eq!(
        hiding_chars(&ran.all()),
        Vec::<char>::new(),
        "the remove refusal: {:?}",
        ran.all()
    );
}

/// 8d (follow-up): `accounts list` shows an invalid entry's raw id with an
/// ESC sequence or a right-to-left override escaped.
#[tokio::test]
async fn accounts_list_escapes_an_invalid_entrys_raw_id() {
    let env = env();
    env.write_user_file(
        "schema_version: 2\naccounts:\n  - id: \"esc\\e[2Kid\"\n    provider: anthropic\n  - id: \"rtl\\u202Eid\"\n    provider: anthropic\n",
    );
    let kit = Kit::new(FakeTerminal::piped());
    let ran = kit.run(&env, &["list"]).await;
    // Named by line, never by id (FU6 (e), coordinator decision).
    assert!(
        ran.all().contains("the entry at line 3"),
        "lists the entry: {:?}",
        ran.all()
    );
    assert!(
        ran.all().contains("the entry at line 5"),
        "lists the entry: {:?}",
        ran.all()
    );
    assert_eq!(
        hiding_chars(&ran.all()),
        Vec::<char>::new(),
        "{:?}",
        ran.all()
    );
}

// ============================================================================
// Second follow-up review fixes (FU6, 2026-11-22; plan `## Review Triage
// Log`), written first, before any `src/` change: hiding by membership.
// ============================================================================

/// A provider-prefixed key.
const FU6_KEY: &str = "sk-ant-api03-fu6canary-0123456789abcdef";
/// A key with no known prefix, mixed case with underscores, 40 characters.
const FU6_PLAIN: &str = "Zx81_qR7tLm2Np9Kv4Ws6Yb3Hd8Jf1Gc5Xe0UaTy";
/// Another such key.
const FU6_PLAIN_D: &str = "Qm4_Tz9Lp2Rw7Ks1Vn8Bx3Hy6Jd0Fc5GaEhLo2Wq";

/// One FU6 placement: what it is, the user file, the canary, and whether
/// the file has an entry `cloud` that `test` and an egress call can name.
struct Fu6 {
    what: &'static str,
    text: String,
    canary: &'static str,
    names_cloud: bool,
}

fn fu6_placements() -> Vec<Fu6> {
    let head = "schema_version: 2\naccounts:\n";
    vec![
        Fu6 {
            what: "(a) the key twice as a mapping key (duplicate-key parse error)",
            text: format!(
                "{head}  - id: cloud\n    provider: anthropic\n    secret_ref: cloud\n    {FU6_KEY}: 1\n    {FU6_KEY}: 1\n"
            ),
            canary: FU6_KEY,
            names_cloud: true,
        },
        Fu6 {
            what: "(b) provider with embedded single quotes",
            text: format!(
                "{head}  - id: cloud\n    provider: \"openai' {FU6_KEY} '\"\n    secret_ref: cloud\n"
            ),
            canary: FU6_KEY,
            names_cloud: true,
        },
        Fu6 {
            what: "(c) an unprefixed, non-kebab key as id",
            text: format!("{head}  - id: {FU6_PLAIN}\n    provider: anthropic\n"),
            canary: FU6_PLAIN,
            names_cloud: false,
        },
        Fu6 {
            what: "(d) a block scalar line `id: <key>` and an error quoting that key",
            text: format!(
                "{head}  - id: cloud\n    provider: {FU6_PLAIN_D}\n    description: |\n      id: {FU6_PLAIN_D}\n    secret_ref: cloud\n"
            ),
            canary: FU6_PLAIN_D,
            names_cloud: true,
        },
    ]
}

/// FU6: `agentic validate` never prints a key placed as (a)–(d).
#[test]
fn fu6_validate_never_prints_a_key_in_the_user_file() {
    let mut leaks = Vec::new();
    for p in fu6_placements() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("accounts.yaml");
        std::fs::write(&file, &p.text).unwrap();
        let home = Home::new(Some(&file));
        let project = copy_of("one-step");
        let (code, out) = home.validate(&[project.path().to_str().unwrap()]);
        assert_eq!(code, 1, "{}: {out}", p.what);
        if holds(&out, p.canary) {
            leaks.push(format!("{}: {out}", p.what));
        }
    }
    assert!(
        leaks.is_empty(),
        "validate printed a key:\n{}",
        leaks.join("\n")
    );
}

/// FU6: `agentic accounts test cloud` never prints a key placed as (a),
/// (b) or (d) (the placements whose file names `cloud`).
#[tokio::test]
async fn fu6_accounts_test_never_prints_a_key_in_the_user_file() {
    let mut leaks = Vec::new();
    for p in fu6_placements().into_iter().filter(|p| p.names_cloud) {
        let env = env();
        env.write_user_file(&p.text);
        let (ran, gets, sent) = test_counting(&env, "cloud", CANARY, &["--model", "m"]).await;
        assert_ne!(ran.code, 0, "{}: {}", p.what, ran.all());
        assert_eq!((gets, sent), (0, 0), "{}", p.what);
        if holds(&ran.all(), p.canary) {
            leaks.push(format!("{}: {}", p.what, ran.all()));
        }
    }
    assert!(
        leaks.is_empty(),
        "`test` printed a key:\n{}",
        leaks.join("\n")
    );
}

/// FU6: `agentic accounts list` never prints a key placed as (a)–(d).
#[tokio::test]
async fn fu6_accounts_list_never_prints_a_key_in_the_user_file() {
    let mut leaks = Vec::new();
    for p in fu6_placements() {
        let env = env();
        env.write_user_file(&p.text);
        let kit = Kit::new(FakeTerminal::piped());
        let ran = kit.run(&env, &["list"]).await;
        if holds(&ran.all(), p.canary) {
            leaks.push(format!("{}: {}", p.what, ran.all()));
        }
    }
    assert!(
        leaks.is_empty(),
        "`list` printed a key:\n{}",
        leaks.join("\n")
    );
}

/// FU6: an egress refusal of `cloud` through a project's directory neither
/// returns nor audits a key placed as (a), (b) or (d).
#[tokio::test]
async fn fu6_an_egress_refusal_never_returns_or_audits_a_key_in_the_user_file() {
    let mut leaks = Vec::new();
    for p in fu6_placements().into_iter().filter(|p| p.names_cloud) {
        let env = env();
        let project = env.naming("p", "local-ollama");
        env.write_user_file(&p.text);
        let tmp = tempfile::tempdir().unwrap();
        let (store, _) = am_store::open_runtime(
            &am_domain::RuntimeLayout::at(tmp.path().join("rt")),
            "test-mac".into(),
        )
        .unwrap();
        let sink: Arc<dyn AuditSink> =
            Arc::new(am_app::egress::StoreAuditSink::new(store.writer()));
        let directory = am_app::egress::ProjectDirectory::open(&project, &env.places).unwrap();
        let wire = wire(Arc::new(directory), counting(&FakeSecretStore::new()), sink);
        let result = wire
            .egress
            .send(keyed_call("cloud", ANTHROPIC_MESSAGES, AuthScheme::XApiKey))
            .await;
        let words = refused_call(result, p.what);
        let mut export = Vec::new();
        store
            .reader()
            .unwrap()
            .export_audit_jsonl(&mut export)
            .unwrap();
        let export = String::from_utf8(export).unwrap();
        assert!(export.contains("egress.blocked"), "{}: {export}", p.what);
        if holds(&export, p.canary) {
            leaks.push(format!("{} (audit): {export}", p.what));
        }
        if holds(&words, p.canary) {
            leaks.push(format!("{} (refusal): {words}", p.what));
        }
    }
    assert!(leaks.is_empty(), "a key leaked:\n{}", leaks.join("\n"));
}

/// FU6 (e): `accounts list` names an invalid entry whose id is a key by its
/// line in the file, never by its id.
#[tokio::test]
async fn fu6_list_names_an_invalid_entry_by_its_line_not_its_id() {
    let env = env();
    env.write_user_file(&format!(
        "schema_version: 2\naccounts:\n  - id: cloud\n    provider: anthropic\n    secret_ref: cloud\n  - id: {FU6_KEY}\n"
    ));
    let kit = Kit::new(FakeTerminal::piped());
    let ran = kit.run(&env, &["list"]).await;
    assert!(
        !holds(&ran.all(), FU6_KEY),
        "`list` named the entry by its id: {}",
        ran.all()
    );
    assert!(
        ran.all().contains("line 6"),
        "names the invalid entry by its line: {}",
        ran.all()
    );
}
