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
#![forbid(unsafe_code)]
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use am_app::cli::{AccountsContext, accounts};
use am_app::runtime::{OpenRequest, Places, Prompt, open_project};
use am_domain::runtime::{legacy_dir, open_marker};
use am_domain::{
    Actor, AuthRef, AuthScheme, ConfigHistory, EgressClient, HistoryEntry, HistoryKind,
    RuntimeLayout, SecretStore,
};
use am_net::{EgressPolicy, check_url};
use am_store::{SCHEMA_VERSION, StoreConfigHistory};
use am_testkit::{FakeEgress, FakeProjectIdentity, FakeSecretStore, FakeTerminal};

/// The key the owner types: a canary that must never leave the keychain.
const CANARY: &str = "sk-ant-canary-0a1b2c3d4e5f-WXYZ";
const ACCOUNTS_FILE: &str = "models/accounts.yaml";

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

/// `models/accounts.yaml` with one Anthropic Account per id.
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
    /// id, a runtime folder and a store) and closed again.
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

    /// Every History entry the commands under test made (the store opened
    /// after the command released it). The setup's own `open_project` mints
    /// `project_id`, which always leaves one `company.yaml` entry by
    /// `System(runtime)` (AD-32); that setup entry is left out.
    fn history(&self, root: &Path) -> Vec<HistoryEntry> {
        let (store, _) = am_store::open_runtime(&self.layout(root), "Test Mac".into()).unwrap();
        StoreConfigHistory::new(&store)
            .unwrap()
            .entries(None)
            .unwrap()
            .into_iter()
            .filter(|e| !matches!(&e.actor, Actor::System { component } if component == "runtime"))
            .collect()
    }

    /// The audit log as JSON Lines, read-only.
    fn audit(&self, root: &Path) -> String {
        let reader = am_store::Reader::open(&self.layout(root).state_db()).unwrap();
        let mut out = Vec::new();
        reader.export_audit_jsonl(&mut out).unwrap();
        String::from_utf8(out).unwrap()
    }
}

fn read(root: &Path, rel: &str) -> Option<String> {
    std::fs::read_to_string(root.join(rel)).ok()
}

/// The Accounts the project's config holds now: (id, provider, name,
/// secret_ref).
fn loaded_accounts(root: &Path) -> Vec<(String, String, Option<String>, Option<String>)> {
    let loaded = am_config::load(root).unwrap();
    let snap = loaded
        .snapshot
        .unwrap_or_else(|| panic!("{:#?}", loaded.findings));
    snap.accounts()
        .iter()
        .map(|a| {
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

/// The fakes one command runs with.
struct Kit {
    term: FakeTerminal,
    secrets: Arc<FakeSecretStore>,
    egress: Arc<FakeEgress>,
    /// Every policy the command asked the egress factory for.
    policies: Arc<Mutex<Vec<EgressPolicy>>>,
}

impl Kit {
    fn new(term: FakeTerminal) -> Kit {
        Kit {
            term,
            secrets: FakeSecretStore::new(),
            egress: FakeEgress::new(),
            policies: Arc::new(Mutex::new(Vec::new())),
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

    fn policies(&self) -> Vec<EgressPolicy> {
        self.policies.lock().unwrap().clone()
    }

    /// Runs `agentic accounts <args> --project <root>`.
    async fn run(&self, env: &Env, root: &Path, args: &[&str]) -> Ran {
        self.run_with(env, root, args, self.secrets.clone()).await
    }

    /// [`Kit::run`] with another secret store (one that counts its reads).
    async fn run_with(
        &self,
        env: &Env,
        root: &Path,
        args: &[&str],
        secrets: Arc<dyn SecretStore>,
    ) -> Ran {
        let policies = self.policies.clone();
        let egress = self.egress.clone();
        let factory: Arc<dyn Fn(EgressPolicy) -> Arc<dyn EgressClient> + Send + Sync> =
            Arc::new(move |policy: EgressPolicy| {
                policies.lock().unwrap().push(policy);
                egress.clone() as Arc<dyn EgressClient>
            });
        let ctx = AccountsContext {
            places: env.places.clone(),
            identity: Arc::new(FakeProjectIdentity::new("Test Mac")),
            terminal: Box::new(self.term.clone()),
            secrets,
            egress: factory,
        };
        let mut argv: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        argv.push("--project".into());
        argv.push(root.to_string_lossy().into_owned());
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

/// Nothing of the owner's key anywhere: output, config, History, audit.
fn assert_no_key_leaked(env: &Env, root: &Path, ran: &Ran) {
    assert!(
        !ran.all().contains(CANARY),
        "the key was printed: {}",
        ran.all()
    );
    if let Some(text) = read(root, ACCOUNTS_FILE) {
        assert!(!text.contains(CANARY), "the key reached the file");
    }
    for e in env.history(root) {
        for bytes in [e.before.as_slice(), e.after.as_slice(), e.diff.as_bytes()] {
            assert!(
                !String::from_utf8_lossy(bytes).contains(CANARY),
                "the key reached History"
            );
        }
    }
    assert!(
        !env.audit(root).contains(CANARY),
        "the key reached the audit log"
    );
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
/// missing `models/accounts.yaml` is created (`create` entry) holding
/// `secret_ref` and never the key; one `account_added` audit entry carries
/// the file, the Account, the pinned scheme, host and port, the references
/// and `confirmation: typed_id`. No marker is written.
#[tokio::test]
async fn add_creates_the_file_stores_the_key_and_records_history_and_audit() {
    let env = env();
    let root = env.usual(&[]);
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
        loaded_accounts(&root),
        vec![(
            "cloud2".to_string(),
            "anthropic".to_string(),
            None,
            Some("cloud2".to_string())
        )]
    );
    let history = env.history(&root);
    assert_eq!(history.len(), 1, "{history:#?}");
    assert_eq!(history[0].file, ACCOUNTS_FILE);
    assert_eq!(history[0].kind, HistoryKind::Create);
    assert_eq!(history[0].actor, owner());

    let audit = env.audit(&root);
    let added = audit_line_of_kind(&audit, "account_added");
    assert_eq!(added.len(), 1, "{audit}");
    let line = &added[0];
    assert!(has(line, "file", ACCOUNTS_FILE), "{line}");
    assert!(has(line, "account", "cloud2"), "{line}");
    assert!(has(line, "scheme", "https"), "{line}");
    assert!(has(line, "host", "api.anthropic.com"), "{line}");
    assert!(
        line.contains("\"port\":\"443\"") || line.contains("\"port\":443"),
        "{line}"
    );
    assert!(has(line, "confirmation", "typed_id"), "{line}");
    assert!(
        line.contains("alpha") && line.contains("beta"),
        "references: {line}"
    );
    assert!(audit.contains("config.created"), "{audit}");

    assert!(!open_marker(&root).exists(), "no marker is written");
    assert_no_key_leaked(&env, &root, &ran);
}

/// Add to an existing file: the Account is appended as an `edit` (with its
/// optional name); the other Accounts stay.
#[tokio::test]
async fn add_to_an_existing_file_appends_the_account_as_an_edit() {
    let env = env();
    let root = env.usual(&[(ACCOUNTS_FILE, accounts_yaml(&["cloud"]))]);
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
        loaded_accounts(&root),
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
    let history = env.history(&root);
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].kind, HistoryKind::Edit);
    assert_eq!(
        audit_line_of_kind(&env.audit(&root), "account_added").len(),
        1
    );
    assert_no_key_leaked(&env, &root, &ran);
}

/// Not a terminal (in process): stdin that is not a terminal is refused in
/// words naming the terminal rule, before anything is read, written or
/// stored; `remove` likewise.
#[tokio::test]
async fn add_and_remove_refuse_when_stdin_is_not_a_terminal() {
    let env = env();
    let root = env.usual(&[(ACCOUNTS_FILE, accounts_yaml(&["cloud2", "spare"]))]);
    let before = read(&root, ACCOUNTS_FILE);
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
    assert_eq!(read(&root, ACCOUNTS_FILE), before);
    assert!(env.history(&root).is_empty());
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
    assert!(!root.join(ACCOUNTS_FILE).exists());
    assert!(env.history(&root).is_empty());
    assert!(audit_line_of_kind(&env.audit(&root), "account_added").is_empty());
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
    assert!(!root.join(ACCOUNTS_FILE).exists());
    assert!(env.history(&root).is_empty());
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
    assert!(!root.join(ACCOUNTS_FILE).exists());
    assert!(env.history(&root).is_empty());
}

/// Reserved or duplicate: `local-lmstudio`, `local-ollama` and an id
/// already in the file are refused in words before anything is read;
/// nothing changes.
#[tokio::test]
async fn add_refuses_a_reserved_or_existing_id() {
    let env = env();
    let root = env.usual(&[(ACCOUNTS_FILE, accounts_yaml(&["cloud2"]))]);
    let before = read(&root, ACCOUNTS_FILE);
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
    assert_eq!(read(&root, ACCOUNTS_FILE), before);
    assert!(env.history(&root).is_empty());
}

/// Only Anthropic Accounts in this story (OpenAI is story 3).
#[tokio::test]
async fn add_refuses_a_provider_other_than_anthropic() {
    let env = env();
    let root = env.usual(&[]);
    let kit = Kit::new(FakeTerminal::new());
    kit.term.line("cloud2");
    kit.term.line(CANARY);

    let ran = kit
        .run(&env, &root, &["add", "cloud2", "--provider", "openai"])
        .await;

    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(ran.all().contains("anthropic"), "{}", ran.all());
    assert_eq!(kit.term.reads(), 0);
    assert!(!root.join(ACCOUNTS_FILE).exists());
}

/// Reuse key: a keychain entry already held for the id is reused, never
/// overwritten and the key is not asked; the add says it reuses it.
#[tokio::test]
async fn add_reuses_an_existing_keychain_entry_and_never_overwrites_it() {
    let env = env();
    let root = env.usual(&[]);
    let kit = Kit::new(FakeTerminal::new()).with_key("cloud2", "sk-ant-earlier-canary-1111");
    kit.term.line("cloud2");
    kit.term.line(CANARY);

    let ran = kit
        .run(&env, &root, &["add", "cloud2", "--provider", "anthropic"])
        .await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(ran.all().to_lowercase().contains("reus"), "{}", ran.all());
    assert_eq!(kit.term.reads(), 1, "only the id is read");
    assert_eq!(
        kit.key("cloud2").as_deref(),
        Some("sk-ant-earlier-canary-1111"),
        "never overwritten"
    );
    assert_eq!(loaded_accounts(&root).len(), 1);
    assert!(!ran.all().contains("sk-ant-earlier-canary-1111"));
}

/// Make `dir` read-only; false when the process can still write there (it
/// runs as root), so the test cannot make a write fail.
fn read_only(dir: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    let probe = dir.join(".probe");
    if std::fs::write(&probe, b"x").is_ok() {
        let _ = std::fs::remove_file(&probe);
        return false;
    }
    true
}

fn writable(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// Write fails: the writer refuses after this add stored the key, so the
/// keychain entry it created is deleted, the file is unchanged and the
/// writer's refusal is shown; an entry the add only reused stays.
#[tokio::test]
async fn a_failed_write_deletes_only_the_key_this_add_stored() {
    let env = env();
    let root = env.usual(&[(ACCOUNTS_FILE, accounts_yaml(&["cloud"]))]);
    let before = read(&root, ACCOUNTS_FILE);
    let models = root.join("models");
    if !read_only(&models) {
        writable(&models);
        eprintln!("skipped: running as a user who can write a read-only folder");
        return;
    }

    let kit = Kit::new(FakeTerminal::new());
    kit.term.line("cloud2");
    kit.term.line(CANARY);
    let ran = kit
        .run(&env, &root, &["add", "cloud2", "--provider", "anthropic"])
        .await;
    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(
        ran.all().contains("ermission denied"),
        "the writer's refusal: {}",
        ran.all()
    );
    assert_eq!(
        kit.key("cloud2"),
        None,
        "the key this add stored is deleted"
    );
    assert_eq!(read(&root, ACCOUNTS_FILE), before);

    let reused = Kit::new(FakeTerminal::new()).with_key("cloud2", "sk-ant-earlier-canary-1111");
    reused.term.line("cloud2");
    let ran = reused
        .run(&env, &root, &["add", "cloud2", "--provider", "anthropic"])
        .await;
    assert_ne!(ran.code, 0, "{}", ran.all());
    assert_eq!(
        reused.key("cloud2").as_deref(),
        Some("sk-ant-earlier-canary-1111"),
        "an entry the add did not create stays"
    );
    writable(&models);
    assert_eq!(read(&root, ACCOUNTS_FILE), before);
    assert!(env.history(&root).is_empty());
    assert!(audit_line_of_kind(&env.audit(&root), "account_added").is_empty());
}

// ------------------------------------------------------------ list, test

/// List: each Account's id, provider, host and key state, masked as dots
/// plus the last 4 characters, or "no key in Keychain"; any stdin; no edit.
#[tokio::test]
async fn list_masks_keys_and_names_missing_ones() {
    let env = env();
    let root = env.usual(&[(ACCOUNTS_FILE, accounts_yaml(&["cloud", "cloud2"]))]);
    let before = read(&root, ACCOUNTS_FILE);
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
    assert_eq!(read(&root, ACCOUNTS_FILE), before);
    assert!(env.history(&root).is_empty());
    assert_no_key_leaked(&env, &root, &ran);
}

/// Test: one short reply streamed through an egress client whose policy
/// admits only the Account's host (no loopback port); the request names the
/// Account in its `AuthRef`; a host outside the pin is refused by
/// `check_url`; any stdin; no edit.
#[tokio::test]
async fn test_streams_a_reply_through_the_accounts_pinned_host() {
    let env = env();
    let root = env.usual(&[(ACCOUNTS_FILE, accounts_yaml(&["cloud", "cloud2"]))]);
    let before = read(&root, ACCOUNTS_FILE);
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

    let policies = kit.policies();
    assert_eq!(policies.len(), 1, "{policies:?}");
    let pinned = &policies[0];
    assert_eq!(pinned.allowed_hosts, vec!["api.anthropic.com".to_string()]);
    assert!(pinned.loopback_ports.is_empty(), "no loopback port");
    assert!(check_url(pinned, "https://api.anthropic.com/v1/messages").is_ok());
    for outside in [
        "https://example.org/v1/messages",
        "https://api.anthropic.com.example.org/",
        "http://127.0.0.1:1234/v1/chat/completions",
        "http://127.0.0.1:11434/v1/chat/completions",
        "http://api.anthropic.com/v1/messages",
    ] {
        assert!(
            check_url(pinned, outside).is_err(),
            "{outside} is outside the pin"
        );
    }

    assert_eq!(read(&root, ACCOUNTS_FILE), before);
    assert!(env.history(&root).is_empty());
    assert!(!ran.all().contains(CANARY));
}

/// Test of an Account with no key, or of an unknown id: refused in words,
/// nothing sent.
#[tokio::test]
async fn test_without_a_key_or_of_an_unknown_id_sends_nothing() {
    let env = env();
    let root = env.usual(&[(ACCOUNTS_FILE, accounts_yaml(&["cloud2"]))]);
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
/// entry stays and the command says so.
#[tokio::test]
async fn remove_of_an_unreferenced_account_has_history_and_undo() {
    let env = env();
    let root = env.project(
        "p",
        &[
            ("company.yaml", COMPANY.to_string()),
            ("seats/alpha.md", alpha("cloud")),
            ("seats/beta.md", beta("cloud")),
            (ACCOUNTS_FILE, accounts_yaml(&["cloud", "spare"])),
        ],
    );
    let kit = Kit::new(FakeTerminal::new()).with_key("spare", CANARY);

    let ran = kit.run(&env, &root, &["remove", "spare"]).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    let ids: Vec<String> = loaded_accounts(&root).into_iter().map(|a| a.0).collect();
    assert_eq!(ids, vec!["cloud".to_string()]);
    assert_eq!(kit.key("spare").as_deref(), Some(CANARY), "the key stays");
    assert!(ran.all().contains("Keychain"), "it says so: {}", ran.all());
    // True today: the change is recorded in History; Undo comes with the UI.
    assert!(ran.all().contains("recorded in History"), "{}", ran.all());
    assert!(ran.all().contains("when the UI ships"), "{}", ran.all());
    let history = env.history(&root);
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].kind, HistoryKind::Edit);
    assert_eq!(history[0].file, ACCOUNTS_FILE);
    assert_eq!(history[0].actor, owner());

    // Undo through the writer brings the Account back.
    let (store, _) = am_store::open_runtime(&env.layout(&root), "Test Mac".into()).unwrap();
    let writer =
        am_config::ConfigWriter::open(&root, Arc::new(StoreConfigHistory::new(&store).unwrap()))
            .unwrap();
    writer.undo(owner(), &history[0].id).unwrap();
    drop(writer);
    drop(store);
    let ids: Vec<String> = loaded_accounts(&root).into_iter().map(|a| a.0).collect();
    assert_eq!(ids, vec!["cloud".to_string(), "spare".to_string()]);
    assert_no_key_leaked(&env, &root, &ran);
}

/// Remove, referenced: refused naming the seats that still name it; a
/// reserved or unknown id is refused too; nothing changes.
#[tokio::test]
async fn remove_refuses_a_referenced_reserved_or_unknown_id() {
    let env = env();
    let root = env.project(
        "p",
        &[
            ("company.yaml", COMPANY.to_string()),
            ("seats/alpha.md", alpha("cloud")),
            ("seats/beta.md", beta("cloud")),
            (ACCOUNTS_FILE, accounts_yaml(&["cloud", "spare"])),
        ],
    );
    let before = read(&root, ACCOUNTS_FILE);

    let kit = Kit::new(FakeTerminal::new());
    let ran = kit.run(&env, &root, &["remove", "cloud"]).await;
    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(
        ran.all().contains("alpha") && ran.all().contains("beta"),
        "names the seats: {}",
        ran.all()
    );

    for id in ["local-ollama", "local-lmstudio", "nope"] {
        let ran = kit.run(&env, &root, &["remove", id]).await;
        assert_ne!(ran.code, 0, "{id}: {}", ran.all());
        assert!(ran.all().contains(id), "{id}: {}", ran.all());
    }
    assert_eq!(read(&root, ACCOUNTS_FILE), before);
    assert!(env.history(&root).is_empty());
}

// ------------------------------------------------------------------ open

/// Busy: while another process holds the store's lock, an add is refused
/// naming the holder, before anything is read or stored.
#[tokio::test]
async fn add_while_the_store_is_held_is_refused_naming_the_holder() {
    let env = env();
    let root = env.usual(&[]);
    let (held, _) = am_store::open_runtime(&env.layout(&root), "Holder Mac".into()).unwrap();
    let kit = Kit::new(FakeTerminal::new());
    kit.term.line("cloud2");
    kit.term.line(CANARY);

    let ran = kit
        .run(&env, &root, &["add", "cloud2", "--provider", "anthropic"])
        .await;

    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(
        ran.all().contains("Holder Mac"),
        "names the holder: {}",
        ran.all()
    );
    assert_eq!(kit.term.reads(), 0);
    assert_eq!(kit.key("cloud2"), None);
    assert!(!root.join(ACCOUNTS_FILE).exists());
    drop(held);
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
        read(&root, ACCOUNTS_FILE).unwrap()
    );
    std::fs::write(root.join(ACCOUNTS_FILE), &changed).unwrap();

    for args in [
        vec!["add", "cloud3", "--provider", "anthropic"],
        vec!["remove", "spare"],
    ] {
        let kit = Kit::new(FakeTerminal::new());
        kit.term.line("cloud3");
        kit.term.line(CANARY);
        let ran = kit.run(&env, &root, &args).await;
        assert_ne!(ran.code, 0, "{args:?}: {}", ran.all());
        assert!(ran.all().contains(ACCOUNTS_FILE), "{args:?}: {}", ran.all());
        assert_eq!(kit.term.reads(), 0, "{args:?}");
        assert_eq!(kit.key("cloud3"), None);
    }
    assert_eq!(read(&root, ACCOUNTS_FILE).unwrap(), changed);
    assert_eq!(env.history(&root).len(), 1);
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
    assert_eq!(kit.term.reads(), 0);
    assert_eq!(kit.key("cloud2"), None);
    assert!(!root.join(ACCOUNTS_FILE).exists());
}

/// The project is resolved read-only (AD-32's checks): a project that would
/// need an id minted, a legacy folder moved or its store migrated is refused
/// in words, and none of those happens.
#[tokio::test]
async fn add_refuses_a_project_that_would_need_a_mint_a_move_or_a_migration() {
    let env = env();

    // Never opened: no project_id yet.
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
    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(ran.all().contains("project_id"), "{}", ran.all());
    assert_eq!(
        read(&fresh, "company.yaml").unwrap(),
        COMPANY,
        "no id minted"
    );
    assert!(!env.places.base.dir().exists(), "no runtime folder made");
    assert_eq!(kit.term.reads(), 0);

    // An unmoved legacy folder.
    let root = env.usual(&[]);
    std::fs::create_dir(legacy_dir(&root)).unwrap();
    let kit = Kit::new(FakeTerminal::new());
    kit.term.line("cloud2");
    kit.term.line(CANARY);
    let ran = kit
        .run(&env, &root, &["add", "cloud2", "--provider", "anthropic"])
        .await;
    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(ran.all().contains("old runtime folder"), "{}", ran.all());
    assert!(legacy_dir(&root).is_dir(), "not moved");
    assert_eq!(kit.term.reads(), 0);
    std::fs::remove_dir(legacy_dir(&root)).unwrap();

    // A store that would need a migration.
    let db = env.layout(&root).state_db();
    std::fs::remove_file(&db).unwrap();
    am_store::testing::database_at_version(&db, SCHEMA_VERSION - 1).unwrap();
    let db_before = std::fs::read(&db).unwrap();
    let kit = Kit::new(FakeTerminal::new());
    kit.term.line("cloud2");
    kit.term.line(CANARY);
    let ran = kit
        .run(&env, &root, &["add", "cloud2", "--provider", "anthropic"])
        .await;
    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(ran.all().contains("migrat"), "{}", ran.all());
    assert_eq!(std::fs::read(&db).unwrap(), db_before, "not migrated");
    assert_eq!(kit.term.reads(), 0);
    assert_eq!(kit.key("cloud2"), None);
    assert!(!root.join(ACCOUNTS_FILE).exists());
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
    let env = env();
    for start in [
        accounts_yaml(&["cloud"]),
        // `accounts:` absent: the add would set the whole list.
        "schema_version: 2\n".to_string(),
    ] {
        let root = env.project(
            &format!("p{}", start.len()),
            &[
                ("company.yaml", COMPANY.to_string()),
                ("seats/alpha.md", alpha("cloud")),
                (ACCOUNTS_FILE, start.clone()),
            ],
        );
        let appeared = accounts_yaml(&["cloud", "other"]);
        let kit = Kit::new(FakeTerminal::new());
        let file = root.join(ACCOUNTS_FILE);
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
            read(&root, ACCOUNTS_FILE).unwrap(),
            appeared,
            "the Account that appeared stays"
        );
        assert_eq!(kit.key("cloud2"), None, "no key is stored");
        assert!(env.history(&root).is_empty());
        assert!(audit_line_of_kind(&env.audit(&root), "account_added").is_empty());
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
    assert!(!root.join(ACCOUNTS_FILE).exists());
    assert!(env.history(&root).is_empty());
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
    let root = env.usual(&[(ACCOUNTS_FILE, yaml.to_string())]);
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

/// The open settles a config write a crash interrupted (reconcile at
/// open). Its report is printed even when the command is then refused, and
/// no refusal claims that nothing was written.
#[tokio::test]
async fn a_refused_add_still_reports_the_interrupted_write_it_completed() {
    let env = env();
    let root = env.usual(&[]);
    {
        // A create interrupted after the file was placed: its intent stays.
        let (store, _) = am_store::open_runtime(&env.layout(&root), "Test Mac".into()).unwrap();
        let writer = am_config::ConfigWriter::open(
            &root,
            Arc::new(StoreConfigHistory::new(&store).unwrap()),
        )
        .unwrap();
        writer.fail_at(Some(am_config::FailPoint::AfterRename));
        writer
            .create(
                owner(),
                am_config::ConfigCreate {
                    file: ACCOUNTS_FILE.into(),
                    bytes: accounts_yaml(&["cloud2"]).into_bytes(),
                    audit: Vec::new(),
                },
            )
            .unwrap_err();
    }
    assert!(env.history(&root).is_empty(), "the entry is still pending");

    let kit = Kit::new(FakeTerminal::new());
    let ran = kit
        .run(
            &env,
            &root,
            &["add", "local-ollama", "--provider", "anthropic"],
        )
        .await;

    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(
        ran.all().contains(&format!(
            "completed the interrupted write of {ACCOUNTS_FILE}"
        )),
        "the reconcile is reported before the refusal: {}",
        ran.all()
    );
    assert_eq!(env.history(&root).len(), 1, "the open recorded the entry");
    for claim in ["othing was written", "written or stored"] {
        assert!(
            !ran.all().contains(claim),
            "the refusal claims `{claim}`: {}",
            ran.all()
        );
    }
    assert!(ran.all().contains("No Account was added"), "{}", ran.all());
}

/// Ctrl-C after the id was typed, on the path that reads no key (the
/// Keychain already holds one): the add is cancelled with exit 130, and
/// nothing is written or stored.
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

    assert_eq!(ran.code, 130, "{}", ran.all());
    assert_eq!(kit.term.reads(), 1, "only the id was read");
    assert!(!root.join(ACCOUNTS_FILE).exists(), "nothing written");
    assert!(env.history(&root).is_empty());
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

/// The Keychain gains an entry for the id while the key prompt waits
/// (another process stored one): it is never overwritten. The typed key is
/// discarded, the add says the existing entry is kept and used, and a write
/// that then fails leaves that entry in place (this add did not create it).
#[tokio::test]
async fn a_keychain_entry_that_appears_during_the_key_prompt_is_kept() {
    const THEIRS: &str = "sk-ant-stored-by-another-process-7777";
    let env = env();
    let root = env.usual(&[]);
    let kit = Kit::new(FakeTerminal::new());
    kit.term.line("cloud2");
    let store = kit.secrets.clone();
    kit.term.line_and(CANARY, move || {
        store
            .set("cloud2", am_domain::SecretString::new(THEIRS.into()))
            .unwrap();
    });

    let ran = kit
        .run(&env, &root, &["add", "cloud2", "--provider", "anthropic"])
        .await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    assert_eq!(kit.term.reads(), 2, "the id, then the key");
    assert_eq!(
        kit.key("cloud2").as_deref(),
        Some(THEIRS),
        "the entry that appeared is not overwritten"
    );
    assert!(ran.all().contains("kept"), "it says so: {}", ran.all());
    assert_eq!(loaded_accounts(&root).len(), 1);
    assert!(!ran.all().contains(THEIRS));
    assert_no_key_leaked(&env, &root, &ran);

    // The same, with a write that fails: the entry stays.
    let other = env.project(
        "q",
        &[
            ("company.yaml", COMPANY.to_string()),
            (ACCOUNTS_FILE, accounts_yaml(&["cloud"])),
        ],
    );
    let models = other.join("models");
    if !read_only(&models) {
        writable(&models);
        eprintln!("skipped: running as a user who can write a read-only folder");
        return;
    }
    let kit = Kit::new(FakeTerminal::new());
    kit.term.line("cloud2");
    let store = kit.secrets.clone();
    kit.term.line_and(CANARY, move || {
        store
            .set("cloud2", am_domain::SecretString::new(THEIRS.into()))
            .unwrap();
    });
    let ran = kit
        .run(&env, &other, &["add", "cloud2", "--provider", "anthropic"])
        .await;
    writable(&models);
    assert_ne!(ran.code, 0, "{}", ran.all());
    assert_eq!(
        kit.key("cloud2").as_deref(),
        Some(THEIRS),
        "an entry this add did not create is never deleted"
    );
}

/// The file gains the same id from outside in the gap between the last
/// check and the write: the writer refuses (the file no longer holds the
/// bytes the edit was built on). That the id is now in the file does not
/// mean this command wrote it: nothing claims a pending History entry, and
/// the key this command stored is deleted.
#[tokio::test]
async fn the_same_id_added_from_outside_before_the_write_is_not_taken_for_this_write() {
    let env = env();
    let root = env.project(
        "p",
        &[
            ("company.yaml", COMPANY.to_string()),
            (ACCOUNTS_FILE, accounts_yaml(&["cloud"])),
        ],
    );
    let outside = accounts_yaml(&["cloud", "cloud2"]);
    let kit = Kit::new(FakeTerminal::new());
    kit.term.line("cloud2");
    kit.term.line(CANARY);
    let (file, bytes) = (root.join(ACCOUNTS_FILE), outside.clone());
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
    assert_eq!(read(&root, ACCOUNTS_FILE).unwrap(), outside);
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
    assert!(env.history(&root).is_empty());
    assert!(audit_line_of_kind(&env.audit(&root), "account_added").is_empty());
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
    assert!(!root.join(ACCOUNTS_FILE).exists(), "no Account written");
    assert!(env.history(&root).is_empty());
    assert_no_key_leaked(&env, &root, &ran);
}

/// A remove whose write fails after the file was replaced (the folder
/// sync, the History commit): the Account is already gone from the file,
/// so the command says it was removed and that its History entry is
/// recorded the next time the project opens, not that it was not removed.
#[tokio::test]
async fn a_remove_that_fails_after_the_file_was_written_says_it_was_removed() {
    let env = env();
    let root = env.project(
        "p",
        &[
            ("company.yaml", COMPANY.to_string()),
            (ACCOUNTS_FILE, accounts_yaml(&["cloud", "spare"])),
        ],
    );
    am_app::runtime::testing::fail_writer_at(&root, Some(am_config::FailPoint::AfterRename));
    let kit = Kit::new(FakeTerminal::new());

    let ran = kit.run(&env, &root, &["remove", "spare"]).await;

    am_app::runtime::testing::fail_writer_at(&root, None);
    assert_ne!(ran.code, 0, "{}", ran.all());
    let ids: Vec<String> = loaded_accounts(&root).into_iter().map(|a| a.0).collect();
    assert_eq!(ids, vec!["cloud".to_string()], "the file was written");
    assert!(
        !ran.all().contains("was not removed"),
        "it is gone from the file: {}",
        ran.all()
    );
    assert!(ran.all().contains("removed from"), "{}", ran.all());
    assert!(
        ran.all().contains("next time the project opens"),
        "{}",
        ran.all()
    );
    assert!(env.history(&root).is_empty(), "the entry is still pending");

    // The next command's open records it.
    let ran = kit.run(&env, &root, &["remove", "nope"]).await;
    assert!(
        ran.all().contains("completed the interrupted write"),
        "{}",
        ran.all()
    );
    assert_eq!(env.history(&root).len(), 1);
}
