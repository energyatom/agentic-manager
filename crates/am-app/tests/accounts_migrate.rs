//! Story 2.13 (AD-15 *Migrate*, *Record*, *Terminal*; AD-16; AD-17 *Old
//! project files*; AD-32): `agentic accounts migrate --project <path>
//! [--as old=new]... [--dry-run | --rollback]` moves a project's old
//! `models/accounts.yaml` to this Mac's user list. One test per row of the
//! story's I/O & Edge-Case Matrix, run in process through
//! `am_app::cli::accounts` with am-testkit's fakes (FakeTerminal,
//! FakeSecretStore behind a store that counts every call, FakeEgress,
//! FakeProjectIdentity) and a temporary HOME per test; the piped-stdin row
//! and `agentic validate` also run the real binary. Canary keys only.
//!
//! The project is a copy of `fixtures/projects/legacy-accounts/`, whose old
//! file holds `cloud` (anthropic), `spare` (anthropic), `same`
//! (openai-compatible at `same.example`) and `clash` (openai-compatible at
//! `old-host.example`); its seats name `cloud` (alpha's fallback), `clash`
//! (beta's model, gamma's fallback), `spare` (beta's fallback) and `same`
//! (gamma's model). Before the project is opened, the usual setup adds
//! `same` at the same destination and `clash` at `new-host.example` to this
//! Mac's list through `agentic accounts add`; the project store's records
//! are seeded by the tests as stories 2.1 and 2.3 wrote them (a `create`
//! History entry of `models/accounts.yaml` whose `config.widened` notes are
//! `account_added` records), by default a matching one for `cloud` only.
//!
//! Decisions these tests make (the plan leaves them to the build; recorded
//! in the plan's Implementation Notes):
//!
//! - Outcome words, per entry, in the sentences and the dry run: `carried
//!   over`, `unconfirmed`, `present` and `clash`; a run with nothing left:
//!   `nothing to move`.
//! - The carried `account_added` record names its source with the args
//!   `project` (the project id), `source_seq` (the project audit entry's
//!   `seq`) and `source_hash` (its `entry_hash`); `account_listed` uses the
//!   same `account`, `provider`, `scheme`, `host`, `port`, `references`
//!   keys as `account_added`, with `confirmation: none`.
//! - New Accounts are asked in the old file's order, the renamed one by its
//!   new id; the typed id is echoed (no hidden prompt is ever shown).
//! - `--rollback`'s typed confirmation is the word `rollback`, within 2
//!   minutes (it adds no Account, so there is no Account id to type).
//! - The backup's manifest records the backup's own sha256 (so a changed
//!   backup fails its hash) and, per `--as` rename, the old id, the new id
//!   and each file rewritten with its before and after sha256.
//! - `am_app::runtime::testing::before_backup_check(root, meanwhile)`
//!   (test-support only, like `fail_writer_at`) runs `meanwhile` once, after
//!   the user store's records committed and before the backup's hash
//!   checks, for the project at `root`.
//!
//! Written before the command exists.
#![forbid(unsafe_code)]
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use am_app::cli::{AccountsContext, accounts};
use am_app::runtime::{OpenRequest, Places, Prompt, open_project};
use am_domain::net::EgressDirectory;
use am_domain::runtime::ProjectId;
use am_domain::{
    Actor, AuditNote, AuditSink, ConfigHistory, EgressClient, HistoryEntry, HistoryKind, NewIntent,
    RuntimeLayout, SecretStore,
};
use am_store::user::{UserReader, UserStore};
use am_testkit::{FakeEgress, FakeProjectIdentity, FakeSecretStore, FakeTerminal};
use sha2::{Digest, Sha256};

/// The key in this Mac's Keychain: it must never be read, shown or stored
/// by `migrate`, `--dry-run` or `--rollback`.
const CANARY: &str = "sk-ant-canary-migrate-0a1b2c3d4e5f-WXYZ";
/// A key pasted into the old file by hand: never echoed.
const PASTED: &str = "sk-ant-api03-pasted-migrate-7c7c7c7c7c7c-PSTD";
const USER_FILE: &str = "accounts.yaml";
const LEGACY_FILE: &str = "models/accounts.yaml";
const SAME_URL: &str = "https://same.example/v1";
const NEW_HOST_URL: &str = "https://new-host.example/v1";
const ROLLBACK_WORD: &str = "rollback";

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

fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The OS app-data folder the binary resolves for `home` (and `xdg` off
/// macOS).
fn support(home: &Path, xdg: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support")
    } else {
        xdg.to_path_buf()
    }
}

/// `text` without the entry of `id` (its lines from `  - id: <id>` to the
/// next entry or the end).
fn without_entry(text: &str, id: &str) -> String {
    let mut out = String::new();
    let mut skipping = false;
    for line in text.split_inclusive('\n') {
        if line.starts_with("  - id: ") {
            skipping = line.trim_end() == format!("  - id: {id}");
        }
        if !skipping {
            out.push_str(line);
        }
    }
    assert_ne!(out, text, "the file has an entry {id}");
    out
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

// ------------------------------------------------------------- the places

/// A temporary HOME (and XDG_DATA_HOME) with its places, as the binary
/// resolves them, and a folder for projects.
struct Env {
    dir: tempfile::TempDir,
    places: Places,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let places = Places::under(&support(&home, &dir.path().join("xdg")), &home);
    Env { dir, places }
}

impl Env {
    fn home(&self) -> PathBuf {
        self.dir.path().join("home")
    }

    fn xdg(&self) -> PathBuf {
        self.dir.path().join("xdg")
    }

    /// Opens the folder at `root` once on this Mac (an id, a runtime folder,
    /// a store and a root record) and closes it again.
    fn open(&self, root: &Path) -> PathBuf {
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

    /// A project folder `name` holding `files`, opened once.
    fn project(&self, name: &str, files: &[(&str, String)]) -> PathBuf {
        let root = self.folder(name, files);
        self.open(&root)
    }

    /// A project whose seat's fallback names `account`, opened once.
    fn naming(&self, name: &str, account: &str) -> PathBuf {
        self.project(
            name,
            &[
                ("company.yaml", COMPANY.to_string()),
                ("seats/alpha.md", seat(account)),
            ],
        )
    }

    /// A copy of `fixtures/projects/legacy-accounts/` named `name`, its old
    /// file changed by `edit` (given its text), opened once.
    fn legacy(&self, name: &str, edit: impl FnOnce(&str) -> String) -> PathBuf {
        let root = self.dir.path().join("projects").join(name);
        std::fs::create_dir_all(&root).unwrap();
        am_testkit::copy_tree(&repo().join("fixtures/projects/legacy-accounts"), &root);
        let text = std::fs::read_to_string(root.join(LEGACY_FILE)).unwrap();
        std::fs::write(root.join(LEGACY_FILE), edit(&text)).unwrap();
        self.open(&root)
    }

    fn project_id(&self, root: &Path) -> ProjectId {
        am_config::read_company(root).unwrap().project_id.unwrap()
    }

    fn layout(&self, root: &Path) -> RuntimeLayout {
        self.places.base.project(&self.project_id(root))
    }

    /// This project's `accounts migrate` backups folder.
    fn backups(&self, root: &Path) -> PathBuf {
        self.places
            .user
            .accounts_migrate_backups(&self.project_id(root))
    }

    /// The backup files of this project (name, bytes), sorted by name.
    fn backup_files(&self, root: &Path) -> Vec<(String, Vec<u8>)> {
        let Ok(read) = std::fs::read_dir(self.backups(root)) else {
            return Vec::new();
        };
        let mut out: Vec<(String, Vec<u8>)> = read
            .flatten()
            .map(|e| {
                (
                    e.file_name().to_string_lossy().into_owned(),
                    std::fs::read(e.path()).unwrap(),
                )
            })
            .collect();
        out.sort();
        out
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

    fn recovered(&self) -> bool {
        UserReader::open(&self.places.user)
            .unwrap()
            .is_recovered()
            .unwrap()
    }

    /// Every History entry of the project's own store.
    fn project_history(&self, root: &Path) -> Vec<HistoryEntry> {
        am_store::Reader::open(&self.layout(root).state_db())
            .unwrap()
            .config_history(None)
            .unwrap()
    }

    fn project_audit(&self, root: &Path) -> String {
        let reader = am_store::Reader::open(&self.layout(root).state_db()).unwrap();
        let mut out = Vec::new();
        reader.export_audit_jsonl(&mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    /// A writer of the user Accounts file holding `user.lock`, as another
    /// process holds it.
    fn user_writer(&self) -> am_config::ConfigWriter {
        let guard = UserStore::locked(&self.places.user, Duration::from_secs(10)).unwrap();
        am_config::ConfigWriter::for_user_accounts(self.places.user.dir(), Arc::new(guard)).unwrap()
    }

    /// Seeds the project store as stories 2.1 and 2.3 left it: History
    /// entries of `models/accounts.yaml` (a `create`, then an `edit` per
    /// further step, the last holding the file's bytes now), each carrying
    /// its notes.
    fn seed(&self, root: &Path, steps: Vec<Vec<AuditNote>>) {
        let now = std::fs::read(root.join(LEGACY_FILE)).unwrap();
        let (store, _) = am_store::open_runtime(&self.layout(root), "Test Mac".into()).unwrap();
        let history = am_store::StoreConfigHistory::new(&store).unwrap();
        let count = steps.len();
        let mut before: Vec<u8> = Vec::new();
        for (i, notes) in steps.into_iter().enumerate() {
            let after = if i + 1 == count {
                now.clone()
            } else {
                let mut b = now.clone();
                b.extend_from_slice(format!("# earlier {i}\n").as_bytes());
                b
            };
            let intent = NewIntent {
                file: LEGACY_FILE.into(),
                kind: if before.is_empty() {
                    HistoryKind::Create
                } else {
                    HistoryKind::Edit
                },
                ref_entry: None,
                before_sha256: sha(&before),
                after_sha256: sha(&after),
                before: before.clone(),
                after: after.clone(),
                diff: format!("step {i}\n"),
                audit: notes,
            };
            let id = history.begin_intent(owner(), intent).unwrap();
            history.commit_entry(owner(), id).unwrap();
            before = after;
        }
    }
}

/// A project-store record as stories 2.1 and 2.3 wrote it: kind
/// `account_added` (or `account_removed`) for `id`, with `provider` only
/// when given, at `https://<host>:<port>`.
fn project_note(kind: &str, id: &str, provider: Option<&str>, host: &str, port: &str) -> AuditNote {
    let mut args: Vec<(&str, String)> = vec![
        ("kind", kind.into()),
        ("file", LEGACY_FILE.into()),
        ("account", id.into()),
    ];
    if kind == "account_added" {
        if let Some(p) = provider {
            args.push(("provider", p.into()));
        }
        args.extend([
            ("scheme", "https".to_string()),
            ("host", host.to_string()),
            ("port", port.to_string()),
            ("references", String::new()),
            ("key", "stored".to_string()),
            ("confirmation", "typed_id".to_string()),
        ]);
    }
    AuditNote {
        action: if kind == "account_removed" {
            "config.narrowed".into()
        } else {
            "config.widened".into()
        },
        target: id.into(),
        args: args.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
    }
}

/// The record matching `cloud`'s entry in the old file.
fn cloud_record() -> AuditNote {
    project_note(
        "account_added",
        "cloud",
        Some("anthropic"),
        "api.anthropic.com",
        "443",
    )
}

// ----------------------------------------------------------------- the kit

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

/// A Keychain that counts every call (reads apart).
struct Recording {
    inner: Arc<FakeSecretStore>,
    calls: Arc<AtomicUsize>,
    gets: Arc<AtomicUsize>,
}

impl SecretStore for Recording {
    fn get(
        &self,
        account: &str,
    ) -> Result<Option<am_domain::SecretString>, am_domain::SecretError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.gets.fetch_add(1, Ordering::SeqCst);
        self.inner.get(account)
    }
    fn set(
        &self,
        account: &str,
        secret: am_domain::SecretString,
    ) -> Result<(), am_domain::SecretError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.set(account, secret)
    }
    fn delete(&self, account: &str) -> Result<(), am_domain::SecretError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.delete(account)
    }
}

/// The fakes one command runs with, on this Mac's Keychain `secrets`.
struct Kit {
    term: FakeTerminal,
    secrets: Arc<FakeSecretStore>,
    egress: Arc<FakeEgress>,
    calls: Arc<AtomicUsize>,
    gets: Arc<AtomicUsize>,
}

impl Kit {
    fn new(term: FakeTerminal, secrets: &Arc<FakeSecretStore>) -> Kit {
        Kit {
            term,
            secrets: secrets.clone(),
            egress: FakeEgress::new(),
            calls: Arc::new(AtomicUsize::new(0)),
            gets: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Every Keychain call the commands of this kit made.
    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn gets(&self) -> usize {
        self.gets.load(Ordering::SeqCst)
    }

    async fn run(&self, env: &Env, args: &[&str]) -> Ran {
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
            secrets: Arc::new(Recording {
                inner: self.secrets.clone(),
                calls: self.calls.clone(),
                gets: self.gets.clone(),
            }),
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
) {
    let mut args = vec!["add", id, "--provider", provider];
    if let Some(url) = base_url {
        args.extend(["--base-url", url]);
    }
    let kit = Kit::new(FakeTerminal::new(), keychain);
    kit.term.line(id);
    kit.term.line(CANARY);
    let ran = kit.run(env, &args).await;
    assert_eq!(ran.code, 0, "setup add {id}: {}", ran.all());
}

/// `migrate --project <root> <extra>` with `typed` lines ready at the
/// terminal; returns what it printed and its kit.
async fn migrate(
    env: &Env,
    keychain: &Arc<FakeSecretStore>,
    root: &Path,
    typed: &[&str],
    extra: &[&str],
) -> (Ran, Kit) {
    let kit = Kit::new(FakeTerminal::new(), keychain);
    for line in typed {
        kit.term.line(line);
    }
    let root_text = root.to_str().unwrap().to_string();
    let mut args = vec!["migrate", "--project", root_text.as_str()];
    args.extend(extra);
    let ran = kit.run(env, &args).await;
    (ran, kit)
}

/// `accounts test <id>` from a pipe, with the Keychain holding the canary
/// for `id`: what it printed, its Keychain reads and its requests.
async fn test_account(env: &Env, id: &str, args: &[&str]) -> (Ran, usize, usize) {
    let keychain = FakeSecretStore::with(id, CANARY);
    let kit = Kit::new(FakeTerminal::piped(), &keychain);
    kit.egress
        .replay("api.anthropic.com", "anthropic/c1_count_stream.sse");
    kit.egress
        .replay("old-host.example", "openai_compat/reasoning.sse");
    let mut argv = vec!["test", id];
    argv.extend(args);
    let ran = kit.run(env, &argv).await;
    (ran, kit.gets(), kit.egress.requests().len())
}

// -------------------------------------------------------------- the setup

/// The usual Mac and project: `same` (same destination) and, when
/// `clash_on_list`, `clash` (at `new-host.example`) added to this Mac's list
/// first; then the legacy project, its old file without `clash` unless
/// `clash_in_file`, with the project store holding `records` (by default
/// the matching record of `cloud`).
struct Setup {
    env: Env,
    root: PathBuf,
    keychain: Arc<FakeSecretStore>,
    /// The old file's bytes as the project holds them.
    legacy: Vec<u8>,
}

async fn setup_with(
    clash_on_list: bool,
    clash_in_file: bool,
    records: Vec<Vec<AuditNote>>,
) -> Setup {
    let env = env();
    let keychain = FakeSecretStore::new();
    add(&env, &keychain, "same", "openai-compatible", Some(SAME_URL)).await;
    if clash_on_list {
        add(
            &env,
            &keychain,
            "clash",
            "openai-compatible",
            Some(NEW_HOST_URL),
        )
        .await;
    }
    let root = env.legacy("p", |text| {
        if clash_in_file {
            text.to_string()
        } else {
            without_entry(text, "clash")
        }
    });
    env.seed(&root, records);
    let legacy = std::fs::read(root.join(LEGACY_FILE)).unwrap();
    Setup {
        env,
        root,
        keychain,
        legacy,
    }
}

async fn setup(clash_on_list: bool, clash_in_file: bool) -> Setup {
    setup_with(clash_on_list, clash_in_file, vec![vec![cloud_record()]]).await
}

/// Everything a refused command must leave as it was: the user file, both
/// stores' History and audit, the old file and the backups.
#[derive(Debug, PartialEq)]
struct State {
    user_text: Option<String>,
    user_history: usize,
    user_audit: String,
    project_history: usize,
    project_audit: String,
    legacy: Option<Vec<u8>>,
    backups: Vec<(String, Vec<u8>)>,
}

impl State {
    fn of(env: &Env, root: &Path) -> State {
        State {
            user_text: env.user_text(),
            user_history: env.history().len(),
            user_audit: env.audit(),
            project_history: env.project_history(root).len(),
            project_audit: env.project_audit(root),
            legacy: std::fs::read(root.join(LEGACY_FILE)).ok(),
            backups: env.backup_files(root),
        }
    }
}

// ---------------------------------------------------------- the readings

fn audit_lines(audit: &str, kind: &str) -> Vec<String> {
    audit
        .lines()
        .filter(|l| l.contains(&format!("\"kind\":\"{kind}\"")))
        .map(str::to_string)
        .collect()
}

/// The audit lines of `kind` for the Account `id`.
fn records_of(audit: &str, kind: &str, id: &str) -> Vec<String> {
    audit_lines(audit, kind)
        .into_iter()
        .filter(|l| has(l, "account", id))
        .collect()
}

fn has(line: &str, key: &str, value: &str) -> bool {
    line.contains(&format!("\"{key}\":\"{value}\""))
}

fn text_field(line: &str, key: &str) -> Option<String> {
    let start = line.find(&format!("\"{key}\":\""))? + key.len() + 4;
    let end = line[start..].find('"')?;
    Some(line[start..start + end].to_string())
}

fn num_field(line: &str, key: &str) -> Option<i64> {
    let start = line.find(&format!("\"{key}\":"))? + key.len() + 3;
    let digits: String = line[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

/// The lines of `text` that name `id` as a word (not inside another id).
fn lines_naming(text: &str, id: &str) -> String {
    let word = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
    text.lines()
        .filter(|line| {
            line.match_indices(id).any(|(at, _)| {
                let before = line[..at].chars().next_back();
                let after = line[at + id.len()..].chars().next();
                !before.is_some_and(word) && !after.is_some_and(word)
            })
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every file under `dir` with its bytes, leaving out SQLite's `-wal` and
/// `-shm` side files (a read-only reader may create or touch them).
fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if e.file_type().unwrap().is_dir() {
                stack.push(p);
            } else if !name.ends_with("-wal") && !name.ends_with("-shm") {
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

/// The canary is in no output, no file under the temporary HOME or the
/// projects (stores included), no History entry and no audit entry.
fn assert_no_key_leaked(env: &Env, runs: &[&Ran]) {
    for ran in runs {
        assert!(!ran.all().contains(CANARY), "printed: {}", ran.all());
    }
    for (path, bytes) in snapshot(env.dir.path()) {
        assert!(
            !contains(&bytes, CANARY),
            "the key reached {}",
            path.display()
        );
    }
    for e in env.history() {
        assert!(!format!("{e:?}").contains(CANARY));
    }
    assert!(!env.audit().contains(CANARY));
}

// =================================================================== rows

/// Dry run: each entry with its outcome (`cloud` carried over, `spare`
/// unconfirmed, `same` present, `clash` a clash naming both hosts), from a
/// pipe; every project file, the user file and both stores unchanged; no
/// Keychain call.
#[tokio::test]
async fn dry_run_lists_every_outcome_and_writes_nothing() {
    let s = setup(true, true).await;
    let before = snapshot(s.env.dir.path());
    let state = State::of(&s.env, &s.root);
    let kit = Kit::new(FakeTerminal::piped(), &s.keychain);

    let root = s.root.to_str().unwrap();
    let ran = kit
        .run(&s.env, &["migrate", "--project", root, "--dry-run"])
        .await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    let all = ran.all();
    for (id, outcome) in [
        ("cloud", "carried over"),
        ("spare", "unconfirmed"),
        ("same", "present"),
        ("clash", "new-host.example"),
    ] {
        let lines = lines_naming(&all, id).to_lowercase();
        assert!(lines.contains(outcome), "{id}: {outcome}: {all}");
    }
    assert!(all.contains("old-host.example"), "{all}");
    assert!(all.contains("new-host.example"), "{all}");
    assert!(all.contains("api.anthropic.com"), "{all}");
    assert_eq!(kit.term.reads(), 0, "nothing is asked");
    assert_eq!(kit.calls(), 0, "no Keychain call");
    assert_eq!(State::of(&s.env, &s.root), state);
    assert_eq!(snapshot(s.env.dir.path()), before, "no file changed");
}

/// Dry run takes no lock: it runs while `agentic serve` holds the
/// project's store and while another process holds `user.lock`.
#[tokio::test]
async fn dry_run_takes_no_lock() {
    let s = setup(true, true).await;
    let (held, _) = am_store::open_runtime(&s.env.layout(&s.root), "Holder Mac".into()).unwrap();
    let user_guard = UserStore::locked(&s.env.places.user, Duration::from_secs(10)).unwrap();
    let kit = Kit::new(FakeTerminal::piped(), &s.keychain);

    let root = s.root.to_str().unwrap();
    let ran = kit
        .run(&s.env, &["migrate", "--project", root, "--dry-run"])
        .await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(
        lines_naming(&ran.all(), "cloud")
            .to_lowercase()
            .contains("carried over"),
        "{}",
        ran.all()
    );
    assert!(am_store::lock_is_held(&s.env.layout(&s.root)));
    drop(user_guard);
    drop(held);
}

/// The migrate of the usual project without `clash`: typed `cloud` and
/// `spare` (`same` is present and asks nothing).
async fn migrated_without_clash() -> (Setup, Ran, Kit) {
    let s = setup(false, false).await;
    let (ran, kit) = migrate(&s.env, &s.keychain, &s.root, &["cloud", "spare"], &[]).await;
    (s, ran, kit)
}

/// Carry over: `cloud`'s matching project record becomes a user-store
/// `account_added` with `confirmation: migrated`, the project id and the
/// source entry's seq and hash, `key: stored`; the ids are typed (echoed,
/// within 2 minutes) for the two new Accounts only, no key prompt; one
/// History entry per Account; the old file is removed by a `removal` entry
/// in the project store; `accounts test cloud` then sends to its endpoint.
#[tokio::test]
async fn migrate_carries_over_a_matching_confirmation() {
    let (s, ran, kit) = migrated_without_clash().await;
    let env = &s.env;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(
        lines_naming(&ran.all(), "cloud")
            .to_lowercase()
            .contains("carried over"),
        "{}",
        ran.all()
    );
    assert_eq!(kit.term.echo_reads(), vec![true, true], "two ids, echoed");
    assert_eq!(
        kit.term.timeouts().first().copied().flatten(),
        Some(Duration::from_secs(120))
    );
    assert_eq!(kit.calls(), 0, "no Keychain call");

    // The source record in the project store.
    let project_audit = env.project_audit(&s.root);
    let source = records_of(&project_audit, "account_added", "cloud");
    assert_eq!(source.len(), 1, "{project_audit}");
    let seq = num_field(&source[0], "seq").unwrap().to_string();
    let hash = text_field(&source[0], "entry_hash").unwrap();

    let pid = env.project_id(&s.root).to_string();
    let audit = env.audit();
    let carried: Vec<String> = records_of(&audit, "account_added", "cloud");
    assert_eq!(carried.len(), 1, "{audit}");
    let line = &carried[0];
    for (key, value) in [
        ("file", USER_FILE),
        ("provider", "anthropic"),
        ("scheme", "https"),
        ("host", "api.anthropic.com"),
        ("port", "443"),
        ("key", "stored"),
        ("confirmation", "migrated"),
        ("project", pid.as_str()),
        ("source_seq", seq.as_str()),
        ("source_hash", hash.as_str()),
    ] {
        assert!(has(line, key, value), "{key}={value}: {line}");
    }
    assert!(records_of(&audit, "account_listed", "cloud").is_empty());

    let text = env.user_text().unwrap();
    for id in ["cloud", "spare", "same"] {
        assert!(text.contains(&format!("id: {id}\n")), "{text}");
    }
    assert!(text.contains("Cloud account"), "the entry's name: {text}");
    assert!(text.contains("secret_ref: cloud"), "{text}");
    assert_eq!(
        env.history().len(),
        1 + 2,
        "the add of same, then one entry per new Account"
    );

    assert!(!s.root.join(LEGACY_FILE).exists(), "removed");
    let last = env
        .project_history(&s.root)
        .into_iter()
        .rfind(|e| e.file == LEGACY_FILE)
        .unwrap();
    assert_eq!(last.kind, HistoryKind::Removal);
    assert_eq!(last.before, s.legacy);
    assert!(last.after.is_empty());
    assert!(
        project_audit.contains("\"action\":\"config.migrated\""),
        "{project_audit}"
    );
    assert!(!env.recovered(), "a store created normally");

    let (ran, gets, sent) = test_account(env, "cloud", &[]).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert_eq!(gets, 1);
    assert_eq!(sent, 1, "sent to its endpoint");
    assert_no_key_leaked(env, &[]);
}

/// Unconfirmed: `spare` (no project record) is added with History and an
/// `account_listed` entry (`confirmation: none`), no confirmation record;
/// its sentence names `agentic accounts change spare`; `accounts test
/// spare` is refused before any Keychain read, naming the same command.
#[tokio::test]
async fn an_unconfirmed_account_is_listed_and_refused_until_changed() {
    let (s, ran, _kit) = migrated_without_clash().await;
    let env = &s.env;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(
        ran.all().contains("agentic accounts change spare"),
        "{}",
        ran.all()
    );
    let audit = env.audit();
    let listed = records_of(&audit, "account_listed", "spare");
    assert_eq!(listed.len(), 1, "{audit}");
    for (key, value) in [
        ("provider", "anthropic"),
        ("scheme", "https"),
        ("host", "api.anthropic.com"),
        ("port", "443"),
        ("confirmation", "none"),
    ] {
        assert!(has(&listed[0], key, value), "{key}={value}: {}", listed[0]);
    }
    assert!(records_of(&audit, "account_added", "spare").is_empty());
    assert!(env.user_text().unwrap().contains("id: spare\n"));

    let (ran, gets, sent) = test_account(env, "spare", &[]).await;
    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(
        ran.all().contains("agentic accounts change spare"),
        "{}",
        ran.all()
    );
    assert_eq!((gets, sent), (0, 0), "refused before any Keychain read");
}

/// No-provider, other-host, other-provider or removed record: only an
/// exact, live match carries over; each of these adds the Account
/// unconfirmed.
#[tokio::test]
async fn a_record_that_does_not_match_exactly_carries_nothing() {
    let cases: Vec<(&str, Vec<Vec<AuditNote>>)> = vec![
        (
            "no provider; another host",
            vec![vec![
                project_note("account_added", "cloud", None, "api.anthropic.com", "443"),
                project_note(
                    "account_added",
                    "spare",
                    Some("anthropic"),
                    "elsewhere.example",
                    "443",
                ),
            ]],
        ),
        (
            "another provider; another port",
            vec![vec![
                project_note(
                    "account_added",
                    "cloud",
                    Some("openai"),
                    "api.anthropic.com",
                    "443",
                ),
                project_note(
                    "account_added",
                    "spare",
                    Some("anthropic"),
                    "api.anthropic.com",
                    "8443",
                ),
            ]],
        ),
        (
            "removed after it was added",
            vec![
                vec![cloud_record()],
                vec![project_note("account_removed", "cloud", None, "", "")],
            ],
        ),
    ];
    for (why, records) in cases {
        let s = setup_with(false, false, records).await;
        let (ran, kit) = migrate(&s.env, &s.keychain, &s.root, &["cloud", "spare"], &[]).await;
        assert_eq!(ran.code, 0, "{why}: {}", ran.all());
        assert_eq!(kit.calls(), 0, "{why}");
        let audit = s.env.audit();
        for id in ["cloud", "spare"] {
            assert!(
                records_of(&audit, "account_added", id).is_empty(),
                "{why}: {id} is not carried over: {audit}"
            );
            assert_eq!(
                records_of(&audit, "account_listed", id).len(),
                1,
                "{why}: {id} is listed unconfirmed: {audit}"
            );
            assert!(
                lines_naming(&ran.all(), id)
                    .to_lowercase()
                    .contains("unconfirmed"),
                "{why}: {id}: {}",
                ran.all()
            );
        }
        assert!(!s.root.join(LEGACY_FILE).exists(), "{why}");
    }
}

/// User store already knows the id: `cloud` was added on this Mac and
/// removed before the project came (an `account_added` then an
/// `account_removed` in the user store), so the project's matching record
/// is never carried: `cloud` enters unconfirmed.
#[tokio::test]
async fn an_id_the_user_store_already_knows_is_never_carried() {
    let env = env();
    let keychain = FakeSecretStore::new();
    add(&env, &keychain, "cloud", "anthropic", None).await;
    let kit = Kit::new(FakeTerminal::new(), &keychain);
    let ran = kit.run(&env, &["remove", "cloud"]).await;
    assert_eq!(ran.code, 0, "setup remove: {}", ran.all());
    add(&env, &keychain, "same", "openai-compatible", Some(SAME_URL)).await;
    let root = env.legacy("p", |text| without_entry(text, "clash"));
    env.seed(&root, vec![vec![cloud_record()]]);

    let (ran, kit) = migrate(&env, &keychain, &root, &["cloud", "spare"], &[]).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    assert_eq!(kit.calls(), 0);
    let audit = env.audit();
    let added = records_of(&audit, "account_added", "cloud");
    assert_eq!(added.len(), 1, "only the first add's record: {audit}");
    assert!(has(&added[0], "confirmation", "typed_id"), "{}", added[0]);
    assert_eq!(records_of(&audit, "account_listed", "cloud").len(), 1);
    assert!(
        lines_naming(&ran.all(), "cloud")
            .to_lowercase()
            .contains("unconfirmed"),
        "{}",
        ran.all()
    );
}

/// Recovered store: `user.db` is lost while the user Accounts file
/// remains, and `migrate` creates it: the store is marked `recovered` and
/// nothing is carried over. A store `migrate` creates on a fresh Mac (no
/// user file, no backups) is not marked and carries over; one it creates
/// while `backups/accounts-migrate/` exists is marked.
#[tokio::test]
async fn a_store_created_while_accounts_remain_is_marked_recovered() {
    // The user file remains.
    let s = setup(false, false).await;
    assert!(!s.env.recovered());
    for suffix in ["", "-wal", "-shm"] {
        let path = PathBuf::from(format!("{}{suffix}", s.env.places.user.user_db().display()));
        let _ = std::fs::remove_file(path);
    }
    let (ran, kit) = migrate(&s.env, &s.keychain, &s.root, &["cloud", "spare"], &[]).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert_eq!(kit.calls(), 0);
    assert!(s.env.recovered(), "marked when created over a user file");
    let audit = s.env.audit();
    assert!(
        records_of(&audit, "account_added", "cloud").is_empty(),
        "{audit}"
    );
    assert_eq!(records_of(&audit, "account_listed", "cloud").len(), 1);

    // A fresh Mac: no user file, no user store, no backups.
    let env = env();
    let keychain = FakeSecretStore::new();
    let root = env.legacy("fresh", |text| without_entry(text, "clash"));
    env.seed(&root, vec![vec![cloud_record()]]);
    let (ran, _) = migrate(&env, &keychain, &root, &["cloud", "spare", "same"], &[]).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(!env.recovered(), "created normally");
    let audit = env.audit();
    assert_eq!(records_of(&audit, "account_added", "cloud").len(), 1);
    assert!(has(
        &records_of(&audit, "account_added", "cloud")[0],
        "confirmation",
        "migrated"
    ));

    // The user file and the store are lost; the backups remain.
    for suffix in ["", "-wal", "-shm"] {
        let path = PathBuf::from(format!("{}{suffix}", env.places.user.user_db().display()));
        let _ = std::fs::remove_file(path);
    }
    std::fs::remove_file(env.accounts_path()).unwrap();
    assert!(
        env.places
            .user
            .dir()
            .join("backups")
            .join("accounts-migrate")
            .is_dir()
    );
    let other = env.legacy("other", |text| without_entry(text, "clash"));
    env.seed(&other, vec![vec![cloud_record()]]);
    let (ran, _) = migrate(&env, &keychain, &other, &["cloud", "spare", "same"], &[]).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(env.recovered(), "marked when created beside the backups");
    let audit = env.audit();
    assert!(
        records_of(&audit, "account_added", "cloud").is_empty(),
        "{audit}"
    );
}

/// Backup: `<project-id>/<ts>.yaml` holds the exact bytes the run parsed,
/// beside its `<ts>.manifest.yaml`; both 0600, in 0700 folders.
#[tokio::test]
async fn the_backup_holds_the_parsed_bytes_owner_only() {
    let (s, ran, _kit) = migrated_without_clash().await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    let dir = s.env.backups(&s.root);
    assert_eq!(
        dir.parent().unwrap(),
        s.env
            .places
            .user
            .dir()
            .join("backups")
            .join("accounts-migrate")
    );
    let files = s.env.backup_files(&s.root);
    assert_eq!(files.len(), 2, "{files:?}");
    let manifest = files
        .iter()
        .find(|(n, _)| n.ends_with(".manifest.yaml"))
        .expect("a manifest");
    let backup = files
        .iter()
        .find(|(n, _)| !n.ends_with(".manifest.yaml") && n.ends_with(".yaml"))
        .expect("a backup");
    assert_eq!(backup.1, s.legacy, "the exact bytes parsed");
    assert_eq!(
        manifest.0.trim_end_matches(".manifest.yaml"),
        backup.0.trim_end_matches(".yaml"),
        "one timestamp"
    );
    let manifest_text = String::from_utf8(manifest.1.clone()).unwrap();
    assert!(
        manifest_text.contains(&sha(&s.legacy)),
        "the backup's hash: {manifest_text}"
    );
    for (name, _) in &files {
        assert_eq!(mode(&dir.join(name)), 0o600, "{name}");
    }
    assert_eq!(mode(&dir), 0o700);
    assert_eq!(mode(dir.parent().unwrap()), 0o700);
}

/// Backup mismatch: the project file changes after the user store's
/// records committed and before the removal: its hash no longer matches
/// what was parsed, so nothing is removed and the command is refused, to
/// be run again.
#[tokio::test]
async fn a_file_that_changes_before_the_removal_is_not_removed() {
    let s = setup(false, false).await;
    let file = s.root.join(LEGACY_FILE);
    let changed = {
        let mut b = s.legacy.clone();
        b.extend_from_slice(b"# changed meanwhile\n");
        b
    };
    let meanwhile = changed.clone();
    let target = file.clone();
    am_app::runtime::testing::before_backup_check(&s.root, move || {
        std::fs::write(&target, &meanwhile).unwrap();
    });

    let (ran, kit) = migrate(&s.env, &s.keychain, &s.root, &["cloud", "spare"], &[]).await;

    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(ran.all().contains("again"), "{}", ran.all());
    assert_eq!(std::fs::read(&file).unwrap(), changed, "not removed");
    assert!(
        !s.env
            .project_history(&s.root)
            .iter()
            .any(|e| e.kind == HistoryKind::Removal),
        "no removal recorded"
    );
    assert_eq!(kit.calls(), 0);
}

/// Order on crash: the project's removal fails after the user store's
/// records committed: the project file is still there; the next run finds
/// every entry present, asks nothing and only removes.
#[tokio::test]
async fn a_crash_after_the_user_store_commits_leaves_the_file_to_migrate_again() {
    let s = setup(false, false).await;
    am_app::runtime::testing::fail_writer_at(&s.root, Some(am_config::FailPoint::BeforeRename));
    let (ran, _) = migrate(&s.env, &s.keychain, &s.root, &["cloud", "spare"], &[]).await;
    am_app::runtime::testing::fail_writer_at(&s.root, None);

    assert_ne!(ran.code, 0, "{}", ran.all());
    assert_eq!(
        std::fs::read(s.root.join(LEGACY_FILE)).unwrap(),
        s.legacy,
        "still there"
    );
    let audit = s.env.audit();
    assert_eq!(records_of(&audit, "account_added", "cloud").len(), 1);
    assert_eq!(records_of(&audit, "account_listed", "spare").len(), 1);
    let user_history = s.env.history().len();

    let (ran, kit) = migrate(&s.env, &s.keychain, &s.root, &[], &[]).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert_eq!(kit.term.reads(), 0, "nothing to type");
    for id in ["cloud", "spare", "same"] {
        assert!(
            lines_naming(&ran.all(), id)
                .to_lowercase()
                .contains("present"),
            "{id}: {}",
            ran.all()
        );
    }
    assert!(!s.root.join(LEGACY_FILE).exists(), "removed now");
    assert_eq!(
        s.env.history().len(),
        user_history,
        "the user side as it was"
    );
    assert_eq!(s.env.audit(), audit);
    assert!(
        s.env
            .project_history(&s.root)
            .iter()
            .any(|e| e.kind == HistoryKind::Removal && e.file == LEGACY_FILE)
    );
}

/// Second run: after a migrate, `migrate` again finds nothing to move and
/// writes nothing (no backup either).
#[tokio::test]
async fn a_second_run_finds_nothing_to_move() {
    let (s, ran, _) = migrated_without_clash().await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    let state = State::of(&s.env, &s.root);

    let (ran, kit) = migrate(&s.env, &s.keychain, &s.root, &[], &[]).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(
        ran.all().to_lowercase().contains("nothing to move"),
        "{}",
        ran.all()
    );
    assert_eq!(kit.term.reads(), 0);
    assert_eq!(State::of(&s.env, &s.root), state);
}

/// Clash: `clash` is on this Mac at another host and no `--as` is given:
/// the whole command is refused naming both destinations and the way out,
/// before any id is asked; nothing is written.
#[tokio::test]
async fn a_clash_refuses_the_whole_command_naming_both_destinations() {
    let s = setup(true, true).await;
    let state = State::of(&s.env, &s.root);

    let (ran, kit) = migrate(
        &s.env,
        &s.keychain,
        &s.root,
        &["cloud", "spare", "clash"],
        &[],
    )
    .await;

    assert_ne!(ran.code, 0, "{}", ran.all());
    let all = ran.all();
    assert!(all.contains("old-host.example"), "{all}");
    assert!(all.contains("new-host.example"), "{all}");
    assert!(all.contains("--as clash="), "names the way out: {all}");
    assert_eq!(kit.term.reads(), 0);
    assert_eq!(kit.calls(), 0);
    assert_eq!(State::of(&s.env, &s.root), state);
}

/// `--as clash=clash-old-host`: the new id is added unconfirmed at the old
/// file's destination; every reference to `clash` in this project (beta's
/// model, gamma's fallback) names `clash-old-host`, each with a
/// `migration` History entry in the same `ConfigMigration` as the removal;
/// the manifest lists the rename and each file with its hashes; another
/// project naming `clash` is untouched; the sentence names the renamed
/// lines and says templates, other projects and other Macs keep reaching
/// the existing `clash`, and this project's exports carry the new id.
#[tokio::test]
async fn as_renames_the_clashing_id_in_this_project_only() {
    let s = setup(true, true).await;
    let other = s.env.naming("other", "clash");
    let other_before = snapshot(&other);
    let beta_before = std::fs::read(s.root.join("seats/beta.md")).unwrap();
    let gamma_before = std::fs::read(s.root.join("seats/gamma.md")).unwrap();
    let clash_added = records_of(&s.env.audit(), "account_added", "clash").len();

    let (ran, kit) = migrate(
        &s.env,
        &s.keychain,
        &s.root,
        &["cloud", "spare", "clash-old-host"],
        &["--as", "clash=clash-old-host"],
    )
    .await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    assert_eq!(kit.calls(), 0);
    assert_eq!(kit.term.reads(), 3);
    let all = ran.all();
    for words in ["seats/beta.md", "seats/gamma.md", "clash-old-host"] {
        assert!(all.contains(words), "names {words}: {all}");
    }
    let lower = all.to_lowercase();
    for words in ["template", "another mac", "export"] {
        assert!(lower.contains(words), "says {words}: {all}");
    }

    // This Mac's list.
    let user = s.env.user_text().unwrap();
    assert!(user.contains("id: clash-old-host\n"), "{user}");
    assert!(user.contains("old-host.example"), "{user}");
    assert!(user.contains("new-host.example"), "clash stays: {user}");
    let audit = s.env.audit();
    assert_eq!(
        records_of(&audit, "account_listed", "clash-old-host").len(),
        1
    );
    assert!(records_of(&audit, "account_added", "clash-old-host").is_empty());
    assert!(has(
        &records_of(&audit, "account_listed", "clash-old-host")[0],
        "host",
        "old-host.example"
    ));
    assert_eq!(
        records_of(&audit, "account_added", "clash").len(),
        clash_added,
        "the existing clash is untouched"
    );

    // This project.
    let beta = std::fs::read_to_string(s.root.join("seats/beta.md")).unwrap();
    let gamma = std::fs::read_to_string(s.root.join("seats/gamma.md")).unwrap();
    for (name, text) in [("beta", &beta), ("gamma", &gamma)] {
        assert!(text.contains("account: clash-old-host\n"), "{name}: {text}");
        assert!(!text.contains("account: clash\n"), "{name}: {text}");
    }
    assert!(!s.root.join(LEGACY_FILE).exists());
    let history = s.env.project_history(&s.root);
    for file in ["seats/beta.md", "seats/gamma.md"] {
        assert!(
            history
                .iter()
                .any(|e| e.file == file && e.kind == HistoryKind::Migration),
            "{file}: {history:#?}"
        );
    }
    assert!(
        history
            .iter()
            .any(|e| e.file == LEGACY_FILE && e.kind == HistoryKind::Removal)
    );
    assert!(
        s.env
            .project_audit(&s.root)
            .matches("config.migrated")
            .count()
            >= 3,
        "one per file"
    );

    // The manifest.
    let files = s.env.backup_files(&s.root);
    let manifest = files
        .iter()
        .find(|(n, _)| n.ends_with(".manifest.yaml"))
        .map(|(_, b)| String::from_utf8(b.clone()).unwrap())
        .expect("a manifest");
    for words in [
        "clash".to_string(),
        "clash-old-host".to_string(),
        "seats/beta.md".to_string(),
        "seats/gamma.md".to_string(),
        sha(&beta_before),
        sha(beta.as_bytes()),
        sha(&gamma_before),
        sha(gamma.as_bytes()),
    ] {
        assert!(manifest.contains(&words), "{words}: {manifest}");
    }

    // Another project is untouched.
    assert_eq!(snapshot(&other), other_before);

    // The new Account starts unconfirmed.
    let (ran, gets, sent) = test_account(&s.env, "clash-old-host", &["--model", "m"]).await;
    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(
        ran.all().contains("agentic accounts change clash-old-host"),
        "{}",
        ran.all()
    );
    assert_eq!((gets, sent), (0, 0));
}

/// `--as` refused: for an id that does not clash, an old id the file does
/// not hold, and a new id that is reserved, not kebab-case, already named
/// in this project or on this Mac's list at another destination; each in
/// words naming the rule, before any id is asked; nothing is written.
#[tokio::test]
async fn as_is_refused_outside_its_rule() {
    let s = setup(true, true).await;
    add(
        &s.env,
        &s.keychain,
        "elsewhere",
        "openai-compatible",
        Some("https://elsewhere.example/v1"),
    )
    .await;
    let state = State::of(&s.env, &s.root);
    for (pair, words) in [
        ("spare=spare-two", "spare"),
        ("nosuch=fresh-id", "nosuch"),
        ("clash=local-ollama", "reserved"),
        ("clash=Clash_Old", "kebab"),
        ("clash=spare", "spare"),
        ("clash=elsewhere", "elsewhere"),
    ] {
        let (ran, kit) = migrate(
            &s.env,
            &s.keychain,
            &s.root,
            &["cloud", "spare", "x"],
            &["--as", pair],
        )
        .await;
        assert_ne!(ran.code, 0, "{pair}: {}", ran.all());
        assert!(ran.all().contains(words), "{pair}: {words}: {}", ran.all());
        assert_eq!(kit.term.reads(), 0, "{pair}");
        assert_eq!(State::of(&s.env, &s.root), state, "{pair}");
    }
}

/// Usage: `migrate` needs `--project`; `--dry-run` with `--rollback`, `--as`
/// with either, an `--as` without `=`, a word or an option it does not take
/// are usage errors (exit 2) naming `agentic accounts migrate`.
#[tokio::test]
async fn migrate_usage_errors_exit_two() {
    let s = setup(true, true).await;
    let root = s.root.to_str().unwrap();
    for args in [
        vec!["migrate"],
        vec!["migrate", "--project"],
        vec!["migrate", root],
        vec!["migrate", "--project", root, "--dry-run", "--rollback"],
        vec![
            "migrate",
            "--project",
            root,
            "--as",
            "clash=clash-old-host",
            "--dry-run",
        ],
        vec![
            "migrate",
            "--project",
            root,
            "--as",
            "clash=clash-old-host",
            "--rollback",
        ],
        vec!["migrate", "--project", root, "--as", "clash"],
        vec!["migrate", "--project", root, "--as"],
        vec!["migrate", "--project", root, "extra"],
        vec!["migrate", "--project", root, "--force"],
        vec!["migrate", "--project", root, "--project", root],
    ] {
        let kit = Kit::new(FakeTerminal::new(), &s.keychain);
        let ran = kit.run(&s.env, &args).await;
        assert_eq!(ran.code, 2, "{args:?}: {}", ran.all());
        assert!(
            ran.err.contains("usage: agentic accounts"),
            "{args:?}: {}",
            ran.err
        );
        assert!(
            ran.err.contains("agentic accounts migrate --project"),
            "{args:?}: the usage names migrate: {}",
            ran.err
        );
        assert_eq!(kit.term.reads(), 0);
    }
}

/// Rollback after the `--as` run: the old file is re-created byte for byte
/// from the latest backup (a `create` History entry in the project), the
/// renames are kept (the seats still name `clash-old-host`, at the same
/// destination), and this Mac's list, the user store and the Keychain are
/// unchanged; the sentence says the renames stay.
#[tokio::test]
async fn rollback_restores_the_file_byte_for_byte_and_keeps_the_renames() {
    let s = setup(true, true).await;
    let (ran, _) = migrate(
        &s.env,
        &s.keychain,
        &s.root,
        &["cloud", "spare", "clash-old-host"],
        &["--as", "clash=clash-old-host"],
    )
    .await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    let seats = (
        std::fs::read(s.root.join("seats/beta.md")).unwrap(),
        std::fs::read(s.root.join("seats/gamma.md")).unwrap(),
    );
    let user = (s.env.user_text(), s.env.history().len(), s.env.audit());

    let (ran, kit) = migrate(
        &s.env,
        &s.keychain,
        &s.root,
        &[ROLLBACK_WORD],
        &["--rollback"],
    )
    .await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    assert_eq!(kit.calls(), 0, "no Keychain call");
    assert_eq!(kit.term.reads(), 1);
    assert_eq!(
        kit.term.timeouts().first().copied().flatten(),
        Some(Duration::from_secs(120))
    );
    assert!(ran.all().contains("clash-old-host"), "{}", ran.all());
    assert_eq!(
        std::fs::read(s.root.join(LEGACY_FILE)).unwrap(),
        s.legacy,
        "byte for byte"
    );
    assert_eq!(
        (
            std::fs::read(s.root.join("seats/beta.md")).unwrap(),
            std::fs::read(s.root.join("seats/gamma.md")).unwrap(),
        ),
        seats,
        "the renames stay"
    );
    assert_eq!(
        (s.env.user_text(), s.env.history().len(), s.env.audit()),
        user,
        "this Mac's list and records unchanged"
    );
    let last = s
        .env
        .project_history(&s.root)
        .into_iter()
        .rfind(|e| e.file == LEGACY_FILE)
        .unwrap();
    assert_eq!(last.kind, HistoryKind::Create);
    assert_eq!(last.after, s.legacy);
    assert_no_key_leaked(&s.env, &[&ran]);
}

/// Backups are never pruned: migrate, roll back and migrate again leaves
/// two backups; the second migrate finds every entry present.
#[tokio::test]
async fn backups_are_never_pruned() {
    let (s, ran, _) = migrated_without_clash().await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    let (ran, _) = migrate(
        &s.env,
        &s.keychain,
        &s.root,
        &[ROLLBACK_WORD],
        &["--rollback"],
    )
    .await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(s.root.join(LEGACY_FILE).exists());
    // Two runs in one millisecond would share a name.
    std::thread::sleep(Duration::from_millis(5));
    let (ran, kit) = migrate(&s.env, &s.keychain, &s.root, &[], &[]).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert_eq!(kit.term.reads(), 0, "every entry present");
    assert!(!s.root.join(LEGACY_FILE).exists());
    let backups: Vec<String> = s
        .env
        .backup_files(&s.root)
        .into_iter()
        .map(|(n, _)| n)
        .filter(|n| !n.ends_with(".manifest.yaml"))
        .collect();
    assert_eq!(backups.len(), 2, "{backups:?}");
}

/// Rollback refused, writing nothing: no backup; a backup whose bytes no
/// longer match its manifest; a missing manifest; the old file back on
/// disk; an id of the restored file on this Mac's list at another
/// destination (not a renamed one).
#[tokio::test]
async fn rollback_is_refused_when_it_cannot_restore_safely() {
    // No backup: a project that was never migrated.
    let env = env();
    let keychain = FakeSecretStore::new();
    let plain = env.project("plain", &[("company.yaml", COMPANY.to_string())]);
    let state = State::of(&env, &plain);
    let (ran, kit) = migrate(&env, &keychain, &plain, &[ROLLBACK_WORD], &["--rollback"]).await;
    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(ran.all().contains("backup"), "{}", ran.all());
    assert_eq!(kit.term.reads(), 0);
    assert_eq!(State::of(&env, &plain), state);

    type Spoil = fn(&Env, &Path);
    let cases: [(&str, Spoil, &str); 4] = [
        (
            "a changed backup",
            |env, root| {
                let dir = env.backups(root);
                let (name, mut bytes) = env
                    .backup_files(root)
                    .into_iter()
                    .find(|(n, _)| !n.ends_with(".manifest.yaml"))
                    .unwrap();
                bytes.extend_from_slice(b"# changed\n");
                std::fs::write(dir.join(name), bytes).unwrap();
            },
            "backup",
        ),
        (
            "a missing manifest",
            |env, root| {
                let dir = env.backups(root);
                let (name, _) = env
                    .backup_files(root)
                    .into_iter()
                    .find(|(n, _)| n.ends_with(".manifest.yaml"))
                    .unwrap();
                std::fs::remove_file(dir.join(name)).unwrap();
            },
            "manifest",
        ),
        (
            "the file back on disk",
            |_env, root| {
                std::fs::write(root.join(LEGACY_FILE), "schema_version: 2\naccounts: []\n")
                    .unwrap();
            },
            LEGACY_FILE,
        ),
        (
            "spare elsewhere on this Mac",
            |env, _root| {
                let dir = env.places.user.dir().to_path_buf();
                let file = am_config::accounts_file(&dir);
                let at = file
                    .ids
                    .iter()
                    .position(|i| i.as_deref() == Some("spare"))
                    .unwrap()
                    .to_string();
                let writer = env.user_writer();
                writer
                    .apply(
                        owner(),
                        am_config::ConfigEdit {
                            file: USER_FILE.into(),
                            ops: vec![
                                am_config::EditOp::Set {
                                    path: vec!["accounts".into(), at.clone(), "provider".into()],
                                    value: "openai-compatible".into(),
                                },
                                am_config::EditOp::Set {
                                    path: vec!["accounts".into(), at, "base_url".into()],
                                    value: "https://elsewhere.example/v1".into(),
                                },
                            ],
                            body: None,
                        },
                    )
                    .unwrap();
            },
            "spare",
        ),
    ];
    for (why, spoil, words) in cases {
        let (s, ran, _) = migrated_without_clash().await;
        assert_eq!(ran.code, 0, "{why}: {}", ran.all());
        spoil(&s.env, &s.root);
        let state = State::of(&s.env, &s.root);
        let (ran, kit) = migrate(
            &s.env,
            &s.keychain,
            &s.root,
            &[ROLLBACK_WORD],
            &["--rollback"],
        )
        .await;
        assert_ne!(ran.code, 0, "{why}: {}", ran.all());
        assert!(ran.all().contains(words), "{why}: {words}: {}", ran.all());
        assert_eq!(kit.calls(), 0, "{why}");
        assert_eq!(State::of(&s.env, &s.root), state, "{why}: nothing written");
    }
}

/// Not interactive (in process): `migrate` and `--rollback` with stdin that
/// is not a terminal are refused in words before anything is read or
/// written.
#[tokio::test]
async fn migrate_and_rollback_need_a_terminal() {
    let s = setup(false, false).await;
    let state = State::of(&s.env, &s.root);
    let root = s.root.to_str().unwrap();
    for args in [
        vec!["migrate", "--project", root],
        vec!["migrate", "--project", root, "--rollback"],
    ] {
        let kit = Kit::new(FakeTerminal::piped(), &s.keychain);
        kit.term.line("cloud");
        kit.term.line("spare");
        let ran = kit.run(&s.env, &args).await;
        assert_ne!(ran.code, 0, "{args:?}: {}", ran.all());
        assert!(ran.all().contains("terminal"), "{args:?}: {}", ran.all());
        assert_eq!(kit.term.reads(), 0, "{args:?}");
        assert_eq!(kit.calls(), 0, "{args:?}");
        assert_eq!(State::of(&s.env, &s.root), state, "{args:?}");
    }
}

/// Not interactive (the real binary): `migrate` and `--rollback` with piped
/// stdin exit non-zero naming the terminal rule, and nothing is written;
/// `--dry-run` from the same pipe lists the entries and exits 0.
#[test]
fn piped_stdin_is_refused_by_the_binary_but_a_dry_run_runs() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let xdg = dir.path().join("xdg");
    std::fs::create_dir(&home).unwrap();
    let places = Places::under(&support(&home, &xdg), &home);
    let root = dir.path().join("project");
    std::fs::create_dir(&root).unwrap();
    am_testkit::copy_tree(&repo().join("fixtures/projects/legacy-accounts"), &root);
    let root = root.canonicalize().unwrap();
    let identity = am_app::identity::system();
    let req = OpenRequest {
        root: root.clone(),
        confirm_move: false,
        take_over: false,
    };
    drop(open_project(&places, identity.as_ref(), &req, &mut NoPrompt).unwrap());
    let legacy = std::fs::read(root.join(LEGACY_FILE)).unwrap();
    let run = |extra: &[&str]| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_agentic"))
            .args(["accounts", "migrate", "--project", root.to_str().unwrap()])
            .args(extra)
            .env("HOME", &home)
            .env("XDG_DATA_HOME", &xdg)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // The command may refuse before reading: a closed pipe is fine.
        use std::io::Write;
        let _ = child
            .stdin
            .take()
            .unwrap()
            .write_all(b"cloud\nspare\nsame\nrollback\n");
        let out = child.wait_with_output().unwrap();
        (
            out.status.code().unwrap_or(-1),
            format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ),
        )
    };

    let refused: [&[&str]; 2] = [&[], &["--rollback"]];
    for extra in refused {
        let (code, out) = run(extra);
        assert_ne!(code, 0, "{extra:?}: {out}");
        assert!(out.contains("terminal"), "{extra:?}: {out}");
        assert_eq!(std::fs::read(root.join(LEGACY_FILE)).unwrap(), legacy);
        assert!(!places.user.accounts_file().exists(), "{extra:?}");
    }
    let (code, out) = run(&["--dry-run"]);
    assert_eq!(code, 0, "{out}");
    for id in ["cloud", "spare", "same", "clash"] {
        assert!(!lines_naming(&out, id).is_empty(), "{id}: {out}");
    }
    assert_eq!(std::fs::read(root.join(LEGACY_FILE)).unwrap(), legacy);
    assert!(!places.user.accounts_file().exists());
}

/// Wrong typed id: a mistyped id at the first or the second prompt, or no
/// answer in time, ends the command with nothing written; so does a
/// mistyped confirmation of `--rollback`.
#[tokio::test]
async fn a_wrong_or_late_typed_id_writes_nothing() {
    let s = setup(false, false).await;
    let state = State::of(&s.env, &s.root);

    let (ran, kit) = migrate(&s.env, &s.keychain, &s.root, &["clouds"], &[]).await;
    assert_ne!(ran.code, 0, "{}", ran.all());
    assert_eq!(kit.term.reads(), 1, "ends at the first");
    assert_eq!(State::of(&s.env, &s.root), state);

    let (ran, kit) = migrate(&s.env, &s.keychain, &s.root, &["cloud", "sparse"], &[]).await;
    assert_ne!(ran.code, 0, "{}", ran.all());
    assert_eq!(kit.term.reads(), 2);
    assert_eq!(State::of(&s.env, &s.root), state, "the first is not kept");

    let kit = Kit::new(FakeTerminal::new(), &s.keychain);
    kit.term.time_out();
    let root = s.root.to_str().unwrap();
    let ran = kit.run(&s.env, &["migrate", "--project", root]).await;
    assert_ne!(ran.code, 0, "{}", ran.all());
    assert_eq!(State::of(&s.env, &s.root), state);

    // Rollback: a mistyped word after a migrate restores nothing.
    let (ran, _) = migrate(&s.env, &s.keychain, &s.root, &["cloud", "spare"], &[]).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    let state = State::of(&s.env, &s.root);
    let (ran, kit) = migrate(&s.env, &s.keychain, &s.root, &["rollbak"], &["--rollback"]).await;
    assert_ne!(ran.code, 0, "{}", ran.all());
    assert_eq!(kit.term.reads(), 1);
    assert_eq!(kit.calls(), 0);
    assert_eq!(State::of(&s.env, &s.root), state);
    assert!(!s.root.join(LEGACY_FILE).exists());
}

/// Project held or risky: refused in words before anything is asked, and
/// nothing is written, while `agentic serve` holds the project (naming the
/// holder); while the project enables a stdio MCP server, or another
/// reachable project does (naming it); while a project file or the user
/// file has an outside change not reloaded (naming it); and for a folder
/// never opened on this Mac (no id is minted).
#[tokio::test]
async fn a_held_or_risky_project_is_refused_naming_why() {
    // Held by `agentic serve`.
    let s = setup(false, false).await;
    let state = State::of(&s.env, &s.root);
    let held = am_store::open_runtime(&s.env.layout(&s.root), "Holder Mac".into()).unwrap();
    let (ran, kit) = migrate(&s.env, &s.keychain, &s.root, &["cloud", "spare"], &[]).await;
    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(ran.all().contains("Holder Mac"), "{}", ran.all());
    assert_eq!(kit.term.reads(), 0);
    drop(held);
    assert_eq!(State::of(&s.env, &s.root), state);

    // The project enables a stdio MCP server.
    let s = setup(false, false).await;
    std::fs::write(s.root.join("tools.yaml"), STDIO_TOOLS).unwrap();
    let state = State::of(&s.env, &s.root);
    let (ran, kit) = migrate(&s.env, &s.keychain, &s.root, &["cloud", "spare"], &[]).await;
    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(ran.all().contains("stdio MCP server"), "{}", ran.all());
    assert!(
        ran.all().contains(&s.root.display().to_string()),
        "{}",
        ran.all()
    );
    assert_eq!(kit.term.reads(), 0);
    assert_eq!(State::of(&s.env, &s.root), state);

    // Another reachable project does.
    let s = setup(false, false).await;
    let child = s.env.project(
        "child",
        &[
            ("company.yaml", COMPANY.to_string()),
            ("tools.yaml", STDIO_TOOLS.to_string()),
        ],
    );
    let state = State::of(&s.env, &s.root);
    let (ran, kit) = migrate(&s.env, &s.keychain, &s.root, &["cloud", "spare"], &[]).await;
    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(
        ran.all().contains(&child.display().to_string()),
        "{}",
        ran.all()
    );
    assert_eq!(kit.term.reads(), 0);
    assert_eq!(State::of(&s.env, &s.root), state);

    // An outside change of a project file not reloaded.
    let s = setup(false, false).await;
    let company = std::fs::read_to_string(s.root.join("company.yaml")).unwrap();
    std::fs::write(s.root.join("company.yaml"), format!("{company}# by hand\n")).unwrap();
    let state = State::of(&s.env, &s.root);
    let (ran, kit) = migrate(&s.env, &s.keychain, &s.root, &["cloud", "spare"], &[]).await;
    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(ran.all().contains("company.yaml"), "{}", ran.all());
    assert_eq!(kit.term.reads(), 0);
    assert_eq!(State::of(&s.env, &s.root), state);

    // An outside change of the user file.
    let s = setup(false, false).await;
    let user = s.env.user_text().unwrap();
    std::fs::write(s.env.accounts_path(), format!("{user}\n")).unwrap();
    let state = State::of(&s.env, &s.root);
    let (ran, kit) = migrate(&s.env, &s.keychain, &s.root, &["cloud", "spare"], &[]).await;
    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(ran.all().contains(USER_FILE), "{}", ran.all());
    assert_eq!(kit.term.reads(), 0);
    assert_eq!(State::of(&s.env, &s.root), state);

    // A folder never opened on this Mac: no id is minted.
    let env = env();
    let keychain = FakeSecretStore::new();
    let fresh = env.folder("fresh", &[("company.yaml", COMPANY.to_string())]);
    std::fs::create_dir_all(fresh.join("models")).unwrap();
    std::fs::write(
        fresh.join(LEGACY_FILE),
        "schema_version: 2\naccounts:\n  - id: cloud\n    provider: anthropic\n    secret_ref: cloud\n",
    )
    .unwrap();
    let (ran, kit) = migrate(&env, &keychain, &fresh, &["cloud"], &[]).await;
    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(ran.all().contains("agentic serve"), "{}", ran.all());
    assert_eq!(kit.term.reads(), 0);
    assert_eq!(
        std::fs::read_to_string(fresh.join("company.yaml")).unwrap(),
        COMPANY
    );
    assert!(fresh.join(LEGACY_FILE).exists());
    assert!(!env.accounts_path().exists());
}

/// Changed since the sentence: while the command waits at the first id
/// prompt, another process adds `spare` to this Mac's list (an app write
/// with History). The re-read under `user.lock` sees it, so the command is
/// refused, to be run again, and nothing of it is written.
#[tokio::test]
async fn a_change_on_the_user_list_since_the_sentence_refuses() {
    let s = setup(false, false).await;
    let state = State::of(&s.env, &s.root);
    let places = s.env.places.clone();
    let kit = Kit::new(FakeTerminal::new(), &s.keychain);
    kit.term.line_and("cloud", move || {
        let guard = UserStore::locked(&places.user, Duration::from_secs(10)).unwrap();
        let writer =
            am_config::ConfigWriter::for_user_accounts(places.user.dir(), Arc::new(guard)).unwrap();
        let file = am_config::accounts_file(places.user.dir());
        let edit = am_config::add_account_edit(
            &file,
            "spare",
            "anthropic",
            Some("Spare account"),
            None,
            am_config::KeyState::Stored,
        );
        writer.apply(owner(), edit).unwrap();
    });
    kit.term.line("spare");

    let root = s.root.to_str().unwrap();
    let ran = kit.run(&s.env, &["migrate", "--project", root]).await;

    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(ran.all().contains("again"), "run it again: {}", ran.all());
    assert_eq!(
        s.env.history().len(),
        state.user_history + 1,
        "only the other process's write"
    );
    let audit = s.env.audit();
    assert!(audit_lines(&audit, "account_listed").is_empty(), "{audit}");
    assert!(records_of(&audit, "account_added", "cloud").is_empty());
    assert_eq!(
        std::fs::read(s.root.join(LEGACY_FILE)).unwrap(),
        s.legacy,
        "not removed"
    );
    assert!(s.env.backup_files(&s.root).is_empty(), "no backup");
    assert_eq!(s.env.project_history(&s.root).len(), state.project_history);
}

/// No key anywhere: with the canary in this Mac's Keychain for `cloud`,
/// `same` and `clash`, a dry run, the `--as` migrate and a rollback make no
/// Keychain call, and the canary is in no file, row, History entry, audit
/// entry or output.
#[tokio::test]
async fn no_key_is_read_stored_or_shown_by_migrate() {
    let s = setup(true, true).await;
    s.keychain
        .set("cloud", am_domain::SecretString::new(CANARY.to_string()))
        .unwrap();
    let root = s.root.to_str().unwrap();

    let kit = Kit::new(FakeTerminal::piped(), &s.keychain);
    let dry = kit
        .run(&s.env, &["migrate", "--project", root, "--dry-run"])
        .await;
    assert_eq!(dry.code, 0, "{}", dry.all());
    assert_eq!(kit.calls(), 0);

    let (moved, kit) = migrate(
        &s.env,
        &s.keychain,
        &s.root,
        &["cloud", "spare", "clash-old-host"],
        &["--as", "clash=clash-old-host"],
    )
    .await;
    assert_eq!(moved.code, 0, "{}", moved.all());
    assert_eq!(kit.calls(), 0);
    assert!(
        !kit.term.echo_reads().contains(&false),
        "no hidden prompt is ever shown"
    );

    let (back, kit) = migrate(
        &s.env,
        &s.keychain,
        &s.root,
        &[ROLLBACK_WORD],
        &["--rollback"],
    )
    .await;
    assert_eq!(back.code, 0, "{}", back.all());
    assert_eq!(kit.calls(), 0);

    for id in ["cloud", "same", "clash"] {
        assert_eq!(
            s.keychain.get(id).unwrap().map(|k| k.expose().to_string()),
            Some(CANARY.to_string()),
            "{id}: the entry is as it was"
        );
    }
    assert_eq!(s.keychain.get("clash-old-host").unwrap().map(|_| ()), None);
    assert_no_key_leaked(&s.env, &[&dry, &moved, &back]);
}

/// A key pasted into the old file (as its `secret_ref`) is never echoed by
/// the dry run or the migrate, and never reaches this Mac's list or the
/// user store.
#[tokio::test]
async fn a_key_looking_value_in_the_old_file_is_never_echoed() {
    let env = env();
    let keychain = FakeSecretStore::new();
    add(&env, &keychain, "same", "openai-compatible", Some(SAME_URL)).await;
    let root = env.legacy("p", |text| {
        without_entry(text, "clash").replace("secret_ref: spare", &format!("secret_ref: {PASTED}"))
    });
    env.seed(&root, vec![vec![cloud_record()]]);
    let root_text = root.to_str().unwrap();

    let kit = Kit::new(FakeTerminal::piped(), &keychain);
    let dry = kit
        .run(&env, &["migrate", "--project", root_text, "--dry-run"])
        .await;
    assert_eq!(dry.code, 0, "{}", dry.all());
    assert!(
        !lines_naming(&dry.all(), "spare").is_empty(),
        "{}",
        dry.all()
    );
    let (moved, _) = migrate(&env, &keychain, &root, &["cloud", "spare"], &[]).await;

    for ran in [&dry, &moved] {
        assert!(!ran.all().contains(PASTED), "echoed: {}", ran.all());
        assert!(!ran.all().contains("pasted-migrate"), "{}", ran.all());
    }
    if let Some(text) = env.user_text() {
        assert!(!text.contains(PASTED), "{text}");
    }
    for e in env.history() {
        assert!(!format!("{e:?}").contains(PASTED));
    }
    assert!(!env.audit().contains(PASTED));
}

/// Validate after: `agentic validate` (the real binary, this temporary
/// HOME) on the migrated project has no old-file error; the one id this
/// Mac still lacks (`clash`, named by two seats) is only a warning.
#[tokio::test]
async fn validate_after_migrate_has_no_legacy_error() {
    let (s, ran, _) = migrated_without_clash().await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    let out = Command::new(env!("CARGO_BIN_EXE_agentic"))
        .arg("validate")
        .arg(&s.root)
        .env("HOME", s.env.home())
        .env("XDG_DATA_HOME", s.env.xdg())
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(!text.contains(LEGACY_FILE), "{text}");
    assert!(text.contains("0 errors"), "{text}");
    assert!(text.contains("seats/beta.md"), "{text}");
    assert!(text.contains("agentic accounts add clash"), "{text}");
    for id in ["cloud", "spare", "same"] {
        assert!(
            !text.contains(&format!("agentic accounts add {id}")),
            "{id} is on this Mac: {text}"
        );
    }
}

// ============================================================================
// Review fixes (2026-11-22 review pass, plan § Review Triage Log): one test
// per `[patch]` row that can be driven from outside `src/`. Written before
// the fixes, to fail first.
// ============================================================================

/// Seeds the project store with History entries of `models/accounts.yaml`
/// of the given kinds and bytes (each step's `before` is the previous
/// step's `after`; a `removal` leaves no bytes), each with its notes. The
/// last step must leave the bytes the file holds now (or none).
fn seed_steps(env: &Env, root: &Path, steps: Vec<(HistoryKind, Vec<u8>, Vec<AuditNote>)>) {
    let (store, _) = am_store::open_runtime(&env.layout(root), "Test Mac".into()).unwrap();
    let history = am_store::StoreConfigHistory::new(&store).unwrap();
    let mut before: Vec<u8> = Vec::new();
    for (i, (kind, after, notes)) in steps.into_iter().enumerate() {
        let intent = NewIntent {
            file: LEGACY_FILE.into(),
            kind,
            ref_entry: None,
            before_sha256: sha(&before),
            after_sha256: sha(&after),
            before: before.clone(),
            after: after.clone(),
            diff: format!("step {i}\n"),
            audit: notes,
        };
        let id = history.begin_intent(owner(), intent).unwrap();
        history.commit_entry(owner(), id).unwrap();
        before = after;
    }
}

/// Gives the project file `rel` a History entry holding its bytes now, as
/// an app write would leave it (so a later change of it is an outside
/// change).
fn seed_file(env: &Env, root: &Path, rel: &str) {
    let now = std::fs::read(root.join(rel)).unwrap();
    let (store, _) = am_store::open_runtime(&env.layout(root), "Test Mac".into()).unwrap();
    let history = am_store::StoreConfigHistory::new(&store).unwrap();
    let id = history
        .begin_intent(
            owner(),
            NewIntent {
                file: rel.into(),
                kind: HistoryKind::Create,
                ref_entry: None,
                before_sha256: sha(b""),
                after_sha256: sha(&now),
                before: Vec::new(),
                after: now,
                diff: format!("+{rel}\n"),
                audit: Vec::new(),
            },
        )
        .unwrap();
    history.commit_entry(owner(), id).unwrap();
}

/// Opens a new project at `<dir>/projects/<name>` whose seat names
/// `account`, from inside a prompt (another process does it meanwhile).
fn open_naming(places: &Places, dir: &Path, name: &str, account: &str) {
    let root = dir.join("projects").join(name);
    std::fs::create_dir_all(root.join("seats")).unwrap();
    std::fs::write(root.join("company.yaml"), COMPANY).unwrap();
    std::fs::write(root.join("seats/alpha.md"), seat(account)).unwrap();
    let req = OpenRequest {
        root: root.canonicalize().unwrap(),
        confirm_move: false,
        take_over: false,
    };
    drop(
        open_project(
            places,
            &FakeProjectIdentity::new("Test Mac"),
            &req,
            &mut NoPrompt,
        )
        .unwrap(),
    );
}

const AS_CLASH: [&str; 2] = ["--as", "clash=clash-old-host"];
const OLD_HOST_URL: &str = "https://old-host.example/v1";

/// Quick 1: a renamed entry is a new id like any other: its sentence names
/// the projects that already name `new` and the projects not checked, and
/// its `account_listed` records the references to `new`, never those to
/// `old` (another project naming `old` reaches the existing Account).
#[tokio::test]
async fn a_renamed_entry_shows_and_records_the_references_to_its_new_id() {
    let s = setup(true, true).await;
    let names_old = s.env.naming("names-old", "clash");
    let names_new = s.env.naming("names-new", "clash-old-host");
    let gone = s.env.naming("gone", "clash-old-host");
    let gone_text = gone.display().to_string();
    std::fs::remove_dir_all(&gone).unwrap();

    let (ran, _) = migrate(
        &s.env,
        &s.keychain,
        &s.root,
        &["cloud", "spare", "clash-old-host"],
        &AS_CLASH,
    )
    .await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    let sentence = lines_naming(&ran.shown, "clash-old-host");
    assert!(
        sentence.contains(&names_new.display().to_string()),
        "names the project that names the new id: {sentence}"
    );
    assert!(
        sentence.contains(&gone_text) && sentence.contains("not checked"),
        "names the project not checked: {sentence}"
    );
    let listed = records_of(&s.env.audit(), "account_listed", "clash-old-host");
    assert_eq!(listed.len(), 1);
    let references = text_field(&listed[0], "references").unwrap_or_default();
    assert!(
        references.contains(&names_new.display().to_string()),
        "{references}"
    );
    assert!(
        !references.contains(&names_old.display().to_string()),
        "the references to 'clash' are not this Account's: {references}"
    );
}

/// Quick 1: under the lock the references to `new` are checked again: a
/// project that starts naming `clash-old-host` while the command waits at
/// its prompts refuses it, to be run again, with nothing written.
#[tokio::test]
async fn a_project_naming_the_new_id_meanwhile_refuses_the_migrate() {
    let s = setup(true, true).await;
    let state = State::of(&s.env, &s.root);
    let places = s.env.places.clone();
    let dir = s.env.dir.path().to_path_buf();
    let kit = Kit::new(FakeTerminal::new(), &s.keychain);
    kit.term.line_and("cloud", move || {
        open_naming(&places, &dir, "late", "clash-old-host");
    });
    kit.term.line("spare");
    kit.term.line("clash-old-host");
    let root = s.root.to_str().unwrap();

    let ran = kit
        .run(
            &s.env,
            &["migrate", "--project", root, AS_CLASH[0], AS_CLASH[1]],
        )
        .await;

    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(ran.all().contains("again"), "{}", ran.all());
    assert_eq!(State::of(&s.env, &s.root), state, "nothing written");
}

/// Quick 2: a re-run after a crash that left `clash-old-host` on this Mac's
/// list (its manifest records the rename) still shows the renamed lines and
/// asks for the new id, then renames and removes.
#[tokio::test]
async fn a_crash_re_run_of_a_rename_shows_the_renamed_lines_and_asks_the_id() {
    let s = setup(true, true).await;
    am_app::runtime::testing::fail_writer_at(&s.root, Some(am_config::FailPoint::BeforeRename));
    let (ran, _) = migrate(
        &s.env,
        &s.keychain,
        &s.root,
        &["cloud", "spare", "clash-old-host"],
        &AS_CLASH,
    )
    .await;
    am_app::runtime::testing::fail_writer_at(&s.root, None);
    assert_ne!(ran.code, 0, "the first run stops: {}", ran.all());
    assert!(s.env.user_text().unwrap().contains("id: clash-old-host\n"));
    assert!(s.root.join(LEGACY_FILE).exists());

    let (ran, kit) = migrate(&s.env, &s.keychain, &s.root, &["clash-old-host"], &AS_CLASH).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    assert_eq!(kit.term.reads(), 1, "the new id is typed again");
    for file in ["seats/beta.md", "seats/gamma.md"] {
        assert!(ran.shown.contains(file), "names {file}: {}", ran.all());
    }
    assert!(
        std::fs::read_to_string(s.root.join("seats/beta.md"))
            .unwrap()
            .contains("account: clash-old-host\n")
    );
    assert!(!s.root.join(LEGACY_FILE).exists());
}

/// Quick 2: on a first run, an unrelated `clash-old-host` already on this
/// Mac's list at the same destination (no manifest of this project records
/// the rename) is not taken as present: the `--as` is refused naming it,
/// and nothing is written.
#[tokio::test]
async fn an_unrelated_new_id_at_the_same_destination_is_refused() {
    let s = setup(true, true).await;
    add(
        &s.env,
        &s.keychain,
        "clash-old-host",
        "openai-compatible",
        Some(OLD_HOST_URL),
    )
    .await;
    let state = State::of(&s.env, &s.root);
    let beta = std::fs::read(s.root.join("seats/beta.md")).unwrap();

    let (ran, kit) = migrate(
        &s.env,
        &s.keychain,
        &s.root,
        &["cloud", "spare", "clash-old-host"],
        &AS_CLASH,
    )
    .await;

    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(ran.all().contains("clash-old-host"), "{}", ran.all());
    assert_eq!(kit.term.reads(), 0);
    assert_eq!(State::of(&s.env, &s.root), state);
    assert_eq!(std::fs::read(s.root.join("seats/beta.md")).unwrap(), beta);
}

/// Quick 3: two renames in one seat file (beta names `clash` and `spare`):
/// the manifest's hashes are those of what was written: the file's bytes
/// before the migration and its bytes now.
#[tokio::test]
async fn the_manifest_hashes_match_two_renames_in_one_file() {
    let s = setup(true, true).await;
    add(
        &s.env,
        &s.keychain,
        "spare",
        "openai-compatible",
        Some("https://elsewhere.example/v1"),
    )
    .await;
    let beta_before = std::fs::read(s.root.join("seats/beta.md")).unwrap();

    let (ran, _) = migrate(
        &s.env,
        &s.keychain,
        &s.root,
        &["cloud", "spare-old", "clash-old-host"],
        &["--as", "clash=clash-old-host", "--as", "spare=spare-old"],
    )
    .await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    let beta_now = std::fs::read(s.root.join("seats/beta.md")).unwrap();
    let text = String::from_utf8(beta_now.clone()).unwrap();
    assert!(text.contains("account: clash-old-host\n"), "{text}");
    assert!(text.contains("account: spare-old\n"), "{text}");
    let entry = s
        .env
        .project_history(&s.root)
        .into_iter()
        .rfind(|e| e.file == "seats/beta.md")
        .unwrap();
    assert_eq!(entry.after, beta_now);
    let manifest = s
        .env
        .backup_files(&s.root)
        .into_iter()
        .find(|(n, _)| n.ends_with(".manifest.yaml"))
        .map(|(_, b)| String::from_utf8(b).unwrap())
        .expect("a manifest");
    assert!(manifest.contains(&sha(&beta_before)), "{manifest}");
    assert!(
        manifest.contains(&sha(&beta_now)),
        "the bytes written: {manifest}"
    );
    // Every hash the manifest records for a renamed file is one of the
    // bytes that file held: before the migration, or as written.
    let gamma_before = std::fs::read(s.root.join("seats/gamma.md")).ok();
    let known: Vec<String> = s
        .env
        .project_history(&s.root)
        .iter()
        .filter(|e| e.file.starts_with("seats/"))
        .flat_map(|e| [e.before_sha256.clone(), e.after_sha256.clone()])
        .chain(gamma_before.iter().map(|b| sha(b)))
        .collect();
    for line in manifest.lines() {
        for key in ["before_sha256: ", "after_sha256: "] {
            if let Some(hash) = line.trim().strip_prefix(key) {
                assert!(known.contains(&hash.to_string()), "{line}: {manifest}");
            }
        }
    }
}

/// G-outside (Quick 4, Adversarial 4): a seat file that names `clash`
/// edited by hand while the command waits at its prompts is never written
/// as the app's own migration: the command is refused, nothing is written,
/// and the edit is still an outside change the next run names.
#[tokio::test]
async fn a_seat_edited_during_the_prompts_is_never_taken_in() {
    let s = setup(true, true).await;
    seed_file(&s.env, &s.root, "seats/beta.md");
    let state = State::of(&s.env, &s.root);
    let beta = s.root.join("seats/beta.md");
    let edited = std::fs::read_to_string(&beta).unwrap().replace(
        "Writes the longer drafts.",
        "Writes the longer drafts, by hand.",
    );
    let (target, bytes) = (beta.clone(), edited.clone());
    let kit = Kit::new(FakeTerminal::new(), &s.keychain);
    kit.term.line_and("cloud", move || {
        std::fs::write(&target, &bytes).unwrap();
    });
    kit.term.line("spare");
    kit.term.line("clash-old-host");
    let root = s.root.to_str().unwrap();

    let ran = kit
        .run(
            &s.env,
            &["migrate", "--project", root, AS_CLASH[0], AS_CLASH[1]],
        )
        .await;

    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(ran.all().contains("seats/beta.md"), "{}", ran.all());
    assert_eq!(
        std::fs::read_to_string(&beta).unwrap(),
        edited,
        "kept as edited"
    );
    assert_eq!(State::of(&s.env, &s.root), state, "nothing written");

    let (ran, kit) = migrate(
        &s.env,
        &s.keychain,
        &s.root,
        &["cloud", "spare", "clash-old-host"],
        &AS_CLASH,
    )
    .await;
    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(
        ran.all().contains("seats/beta.md"),
        "still an outside change: {}",
        ran.all()
    );
    assert_eq!(kit.term.reads(), 0);
}

/// G-outside (Adversarial 4): `--as`'s "named nowhere in this project" is
/// checked again under the locks: a seat naming `clash-old-host` written
/// during the prompts refuses the command, with nothing written.
#[tokio::test]
async fn the_new_id_named_in_the_project_during_the_prompts_refuses() {
    let s = setup(true, true).await;
    let state = State::of(&s.env, &s.root);
    let delta = s.root.join("seats/delta.md");
    let text = "---\nschema_version: 2\nname: Delta\nrole: Writer\nteam: core\nmodel:\n  account: clash-old-host\n  model: example-large\n---\nWrite.\n";
    let target = delta.clone();
    let kit = Kit::new(FakeTerminal::new(), &s.keychain);
    kit.term.line_and("cloud", move || {
        std::fs::write(&target, text).unwrap();
    });
    kit.term.line("spare");
    kit.term.line("clash-old-host");
    let root = s.root.to_str().unwrap();

    let ran = kit
        .run(
            &s.env,
            &["migrate", "--project", root, AS_CLASH[0], AS_CLASH[1]],
        )
        .await;

    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(ran.all().contains("clash-old-host"), "{}", ran.all());
    assert_eq!(State::of(&s.env, &s.root), state, "nothing written");
    assert!(
        std::fs::read_to_string(s.root.join("seats/beta.md"))
            .unwrap()
            .contains("account: clash\n"),
        "no rename"
    );
}

/// G-dry (Quick 5, Adversarial 9): `--dry-run` never prints a key-looking
/// id or host label, nor any value of an entry with an error.
#[tokio::test]
async fn a_dry_run_never_prints_a_key_looking_or_erroneous_value() {
    let cases = [
        (
            "a key-looking id",
            "schema_version: 2\naccounts:\n  - id: sk-ant-api03-dryidcanary-0123456789abcdef\n    provider: anthropic\n    secret_ref: cloud\n  - id: cloud\n    provider: anthropic\n    secret_ref: cloud\n",
            "dryidcanary",
        ),
        (
            "a key-looking host label",
            "schema_version: 2\naccounts:\n  - id: hosty\n    provider: openai-compatible\n    base_url: https://sk-ant-api03-dryhostcanary-0123456789abcdef.example/v1\n    secret_ref: hosty\n  - id: cloud\n    provider: anthropic\n    secret_ref: cloud\n",
            "dryhostcanary",
        ),
        (
            "an entry with an error",
            "schema_version: 2\naccounts:\n  - id: broken\n    provider: openai-compatible\n    base_url: http://valuecanary-host.example/v1\n    secret_ref: broken\n  - id: cloud\n    provider: anthropic\n    secret_ref: cloud\n",
            "valuecanary",
        ),
    ];
    for (why, file, marker) in cases {
        let env = env();
        let keychain = FakeSecretStore::new();
        let root = env.legacy("p", |_| file.to_string());
        let kit = Kit::new(FakeTerminal::piped(), &keychain);
        let root_text = root.to_str().unwrap();
        let ran = kit
            .run(&env, &["migrate", "--project", root_text, "--dry-run"])
            .await;
        assert!(
            !ran.all().contains(marker),
            "{why}: the value was printed: {}",
            ran.all()
        );
        assert!(
            !lines_naming(&ran.all(), "cloud").is_empty(),
            "{why}: the other entries are still listed: {}",
            ran.all()
        );
    }
}

/// G-removed (Quick 6): continuity of the project's History. The project
/// confirmed `cloud` at its destination, a later write of
/// `models/accounts.yaml` dropped `cloud`, and a still later one put it
/// back at the same destination with no new confirmation: `cloud` is
/// unconfirmed, never carried.
#[tokio::test]
async fn a_record_broken_by_a_later_write_without_the_id_carries_nothing() {
    let env = env();
    let keychain = FakeSecretStore::new();
    add(&env, &keychain, "same", "openai-compatible", Some(SAME_URL)).await;
    let root = env.legacy("p", |text| without_entry(text, "clash"));
    let now = std::fs::read(root.join(LEGACY_FILE)).unwrap();
    let dropped = without_entry(&String::from_utf8(now.clone()).unwrap(), "cloud").into_bytes();
    seed_steps(
        &env,
        &root,
        vec![
            (HistoryKind::Create, now.clone(), vec![cloud_record()]),
            (HistoryKind::Edit, dropped, Vec::new()),
            (HistoryKind::Edit, now, Vec::new()),
        ],
    );

    let (ran, kit) = migrate(&env, &keychain, &root, &["cloud", "spare"], &[]).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    assert_eq!(kit.calls(), 0);
    let audit = env.audit();
    assert!(
        records_of(&audit, "account_added", "cloud").is_empty(),
        "not carried: {audit}"
    );
    assert_eq!(records_of(&audit, "account_listed", "cloud").len(), 1);
    assert!(
        lines_naming(&ran.shown, "cloud")
            .to_lowercase()
            .contains("unconfirmed"),
        "{}",
        ran.all()
    );
}

/// G-removed (Adversarial 3): an earlier migrate removed the file (a
/// `removal` entry) and it came back later (a `create`), as after the app
/// data was wiped: the confirmation before the removal is not carried.
#[tokio::test]
async fn a_record_before_an_earlier_removal_carries_nothing() {
    let env = env();
    let keychain = FakeSecretStore::new();
    add(&env, &keychain, "same", "openai-compatible", Some(SAME_URL)).await;
    let root = env.legacy("p", |text| without_entry(text, "clash"));
    let now = std::fs::read(root.join(LEGACY_FILE)).unwrap();
    seed_steps(
        &env,
        &root,
        vec![
            (HistoryKind::Create, now.clone(), vec![cloud_record()]),
            (HistoryKind::Removal, Vec::new(), Vec::new()),
            (HistoryKind::Create, now, Vec::new()),
        ],
    );

    let (ran, _) = migrate(&env, &keychain, &root, &["cloud", "spare"], &[]).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    let audit = env.audit();
    assert!(
        records_of(&audit, "account_added", "cloud").is_empty(),
        "not carried: {audit}"
    );
    assert_eq!(records_of(&audit, "account_listed", "cloud").len(), 1);
}

/// Adversarial 7/8: a linked `models/accounts.yaml`, or a linked `models/`
/// folder, is refused in words before anything is asked or written: this
/// Mac's list and the user store stay as they were (no run is stranded
/// with Accounts added and a file it can never remove).
#[tokio::test]
async fn a_linked_old_file_or_folder_is_refused_before_any_user_write() {
    for linked in ["file", "folder"] {
        let s = setup(false, false).await;
        let outside = s.env.dir.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        match linked {
            "file" => {
                let target = outside.join("accounts.yaml");
                std::fs::rename(s.root.join(LEGACY_FILE), &target).unwrap();
                std::os::unix::fs::symlink(&target, s.root.join(LEGACY_FILE)).unwrap();
            }
            _ => {
                let target = outside.join("models");
                std::fs::rename(s.root.join("models"), &target).unwrap();
                std::os::unix::fs::symlink(&target, s.root.join("models")).unwrap();
            }
        }
        let user = (s.env.user_text(), s.env.history().len(), s.env.audit());

        let (ran, kit) = migrate(&s.env, &s.keychain, &s.root, &["cloud", "spare"], &[]).await;

        assert_ne!(ran.code, 0, "{linked}: {}", ran.all());
        assert!(ran.all().contains("link"), "{linked}: {}", ran.all());
        assert_eq!(kit.term.reads(), 0, "{linked}: refused before any prompt");
        assert_eq!(
            (s.env.user_text(), s.env.history().len(), s.env.audit()),
            user,
            "{linked}: no user write"
        );
        assert!(
            std::fs::symlink_metadata(s.root.join(LEGACY_FILE)).is_ok()
                || std::fs::symlink_metadata(s.root.join("models")).is_ok()
        );
        assert!(
            s.env.backup_files(&s.root).is_empty(),
            "{linked}: no backup"
        );
    }
}

/// Adversarial 10: the rollback sentence shows the manifest's renames
/// through the terminal escaping: a control character written into the
/// manifest reaches the terminal only escaped.
#[tokio::test]
async fn the_rollback_sentence_escapes_the_manifests_renames() {
    let s = setup(true, true).await;
    let (ran, _) = migrate(
        &s.env,
        &s.keychain,
        &s.root,
        &["cloud", "spare", "clash-old-host"],
        &AS_CLASH,
    )
    .await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    let (name, bytes) = s
        .env
        .backup_files(&s.root)
        .into_iter()
        .find(|(n, _)| n.ends_with(".manifest.yaml"))
        .unwrap();
    let text = String::from_utf8(bytes).unwrap().replace(
        "    new: clash-old-host",
        "    new: clash-old-host\u{1b}[2Kerased",
    );
    std::fs::write(s.env.backups(&s.root).join(name), text).unwrap();

    let (ran, _) = migrate(&s.env, &s.keychain, &s.root, &["no"], &["--rollback"]).await;

    assert!(
        !ran.all().contains('\u{1b}'),
        "a raw control character reached the terminal: {:?}",
        ran.all()
    );
    assert!(
        ran.all().contains("\\u{1b}"),
        "shown escaped: {}",
        ran.all()
    );
}

// ============================================================================
// Follow-up fixes (2026-11-22 follow-up review pass, plan § Review Triage
// Log). Written before the fixes, to fail first.
// ============================================================================

/// The usual project whose seats name `clash` nowhere (beta's model is
/// `spare`, gamma's fallback `same`; seat files with no History, as the
/// fixture's).
async fn setup_without_clash_references() -> Setup {
    let s = setup(true, true).await;
    for (file, from, to) in [
        ("seats/beta.md", "  account: clash\n", "  account: spare\n"),
        (
            "seats/gamma.md",
            "  - account: clash\n",
            "  - account: same\n",
        ),
    ] {
        let path = s.root.join(file);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains(from), "{file}");
        std::fs::write(&path, text.replacen(from, to, 1)).unwrap();
    }
    s
}

/// Follow-up 1 (during a prompt): `--as clash=…` while nothing in the
/// project names `clash`; a seat file with no History gains `account:
/// clash` from outside while the command waits: refused, nothing written,
/// the file left exactly as the outside program wrote it.
#[tokio::test]
async fn a_reference_to_old_added_from_outside_during_a_prompt_is_never_taken_in() {
    let s = setup_without_clash_references().await;
    let state = State::of(&s.env, &s.root);
    let beta = s.root.join("seats/beta.md");
    let outside = std::fs::read_to_string(&beta).unwrap().replacen(
        "  account: spare\n",
        "  account: clash\n",
        1,
    );
    let (target, bytes) = (beta.clone(), outside.clone());
    let kit = Kit::new(FakeTerminal::new(), &s.keychain);
    kit.term.line_and("cloud", move || {
        std::fs::write(&target, &bytes).unwrap();
    });
    kit.term.line("spare");
    kit.term.line("clash-old-host");
    let root = s.root.to_str().unwrap();

    let ran = kit
        .run(
            &s.env,
            &["migrate", "--project", root, AS_CLASH[0], AS_CLASH[1]],
        )
        .await;

    assert_ne!(ran.code, 0, "{}", ran.all());
    assert_eq!(
        std::fs::read_to_string(&beta).unwrap(),
        outside,
        "untouched"
    );
    assert_eq!(State::of(&s.env, &s.root), state, "nothing written");
}

/// Follow-up 1 (after the lock's re-read, before the apply; the
/// `before_backup_check` hook): with nothing naming `clash` when the
/// command planned its renames, a seat file given `account: clash` from
/// outside is never written as the app's own migration: the project's
/// migration is refused (no rename, no removal), the file stays as the
/// outside program wrote it, and the refusal names it.
#[tokio::test]
async fn a_reference_to_old_added_from_outside_before_the_apply_is_never_taken_in() {
    let s = setup_without_clash_references().await;
    let beta = s.root.join("seats/beta.md");
    let outside = std::fs::read_to_string(&beta).unwrap().replacen(
        "  account: spare\n",
        "  account: clash\n",
        1,
    );
    let (target, bytes) = (beta.clone(), outside.clone());
    am_app::runtime::testing::before_backup_check(&s.root, move || {
        std::fs::write(&target, &bytes).unwrap();
    });
    let project_history = s.env.project_history(&s.root).len();

    let (ran, _) = migrate(
        &s.env,
        &s.keychain,
        &s.root,
        &["cloud", "spare", "clash-old-host"],
        &AS_CLASH,
    )
    .await;

    assert_ne!(ran.code, 0, "{}", ran.all());
    assert!(ran.all().contains("seats/beta.md"), "{}", ran.all());
    assert_eq!(
        std::fs::read_to_string(&beta).unwrap(),
        outside,
        "untouched"
    );
    assert_eq!(
        s.env.project_history(&s.root).len(),
        project_history,
        "no project write"
    );
    assert_eq!(
        std::fs::read(s.root.join(LEGACY_FILE)).unwrap(),
        s.legacy,
        "not removed"
    );
}

/// Follow-up 2: the user records commit and then the backup (and its
/// manifest) cannot be written (here: a file stands where this project's
/// backup folder goes; no test hook stops a run between the user commits
/// and the manifest). A re-run with the same `--as` counts `clash-old-host`
/// as present by its own `account_listed` record (this project, this
/// destination) and completes: renamed, removed, nothing added twice.
#[tokio::test]
async fn a_re_run_after_a_failure_before_the_manifest_completes_the_rename() {
    let s = setup(true, true).await;
    let folder = s.env.backups(&s.root);
    std::fs::create_dir_all(folder.parent().unwrap()).unwrap();
    std::fs::write(&folder, "not a folder").unwrap();

    let (ran, _) = migrate(
        &s.env,
        &s.keychain,
        &s.root,
        &["cloud", "spare", "clash-old-host"],
        &AS_CLASH,
    )
    .await;
    assert_ne!(ran.code, 0, "the backup cannot be written: {}", ran.all());
    assert!(s.env.user_text().unwrap().contains("id: clash-old-host\n"));
    assert_eq!(
        records_of(&s.env.audit(), "account_listed", "clash-old-host").len(),
        1
    );
    assert!(s.root.join(LEGACY_FILE).exists());
    std::fs::remove_file(&folder).unwrap();

    let (ran, _) = migrate(&s.env, &s.keychain, &s.root, &["clash-old-host"], &AS_CLASH).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    assert!(!s.root.join(LEGACY_FILE).exists());
    for file in ["seats/beta.md", "seats/gamma.md"] {
        let text = std::fs::read_to_string(s.root.join(file)).unwrap();
        assert!(text.contains("account: clash-old-host\n"), "{file}: {text}");
    }
    let audit = s.env.audit();
    assert_eq!(
        records_of(&audit, "account_listed", "clash-old-host").len(),
        1,
        "nothing added twice"
    );
}

/// Follow-up 3: `--dry-run` decides per entry whether it is clean: with
/// `id: cloud` twice and the second copy unclean (a key-looking label in
/// its `base_url`, or a plain `base_url` the endpoint rules refuse), no
/// value of either `cloud` entry is printed.
#[tokio::test]
async fn a_dry_run_shows_no_value_of_a_duplicated_id_with_an_unclean_copy() {
    let cases = [
        (
            "a key-looking base_url",
            "https://sk-ant-api03-dupcanary-0123456789abcdef.example/v1",
            "dupcanary",
        ),
        (
            "a refused plain base_url",
            "http://plainvalue-host.example/v1",
            "plainvalue",
        ),
    ];
    for (why, url, marker) in cases {
        let file = format!(
            "schema_version: 2\naccounts:\n  - id: cloud\n    provider: anthropic\n    secret_ref: cloud\n  - id: cloud\n    provider: openai-compatible\n    base_url: {url}\n    secret_ref: cloud\n  - id: other\n    provider: openai-compatible\n    base_url: https://clean.example/v1\n    secret_ref: other\n"
        );
        let env = env();
        let keychain = FakeSecretStore::new();
        let root = env.legacy("p", |_| file.clone());
        let kit = Kit::new(FakeTerminal::piped(), &keychain);
        let root_text = root.to_str().unwrap();
        let ran = kit
            .run(&env, &["migrate", "--project", root_text, "--dry-run"])
            .await;
        let all = ran.all();
        assert!(!all.contains(marker), "{why}: printed: {all}");
        let cloud = lines_naming(&all, "cloud");
        assert!(
            !cloud.contains("api.anthropic.com") && !cloud.contains("anthropic  "),
            "{why}: a value of the first cloud entry: {all}"
        );
    }
}

// ============================================================ story 2.14

/// A `tools.yaml` enabling one `transport: http` MCP server `id` at `url`.
fn http_tools(id: &str, url: &str) -> String {
    format!(
        "schema_version: 2\nmcp_servers:\n  - id: {id}\n    transport: http\n    url: {url}\n    version: 1.0.0\n    network: none\n    tools:\n      - name: list_notes\n        class: read\n"
    )
}

/// Story 2.14 (AD-15 *Open*, owner 2026-11-22): an enabled `transport:
/// http` MCP server on this Mac (`http://127.0.0.1:<port>`,
/// `https://localhost`) counts exactly as a stdio one for `migrate`: in the
/// project itself, or in another reachable project, it refuses the command
/// before anything is asked, naming the project and the server, and nothing
/// is written. A remote `https` server does not.
#[tokio::test]
async fn a_loopback_http_mcp_server_refuses_migrate() {
    for url in ["http://127.0.0.1:3001/mcp", "https://localhost/mcp"] {
        // In the project itself.
        let s = setup(false, false).await;
        std::fs::write(s.root.join("tools.yaml"), http_tools("loop-notes", url)).unwrap();
        let state = State::of(&s.env, &s.root);
        let (ran, kit) = migrate(&s.env, &s.keychain, &s.root, &["cloud", "spare"], &[]).await;
        assert_ne!(ran.code, 0, "{url}: {}", ran.all());
        assert!(ran.all().contains("loop-notes"), "{url}: {}", ran.all());
        assert!(
            ran.all().contains(&s.root.display().to_string()),
            "{url}: {}",
            ran.all()
        );
        assert_eq!(kit.term.reads(), 0, "{url}");
        assert_eq!(kit.calls(), 0, "{url}");
        assert_eq!(State::of(&s.env, &s.root), state, "{url}");

        // In another reachable project.
        let s = setup(false, false).await;
        let child = s.env.project(
            "child",
            &[
                ("company.yaml", COMPANY.to_string()),
                ("tools.yaml", http_tools("loop-notes", url)),
            ],
        );
        let state = State::of(&s.env, &s.root);
        let (ran, kit) = migrate(&s.env, &s.keychain, &s.root, &["cloud", "spare"], &[]).await;
        assert_ne!(ran.code, 0, "{url}: {}", ran.all());
        assert!(
            ran.all().contains(&child.display().to_string()),
            "{url}: {}",
            ran.all()
        );
        assert!(ran.all().contains("loop-notes"), "{url}: {}", ran.all());
        assert_eq!(kit.term.reads(), 0, "{url}");
        assert_eq!(State::of(&s.env, &s.root), state, "{url}");
    }

    // A remote https server counts for nothing.
    let s = setup(false, false).await;
    std::fs::write(
        s.root.join("tools.yaml"),
        http_tools("far-notes", "https://mcp.example/mcp"),
    )
    .unwrap();
    let (ran, _kit) = migrate(&s.env, &s.keychain, &s.root, &["cloud", "spare"], &[]).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
}

/// Story 2.14, First confirmation: `spare`, which `migrate` moved to this
/// Mac unconfirmed, is confirmed with `agentic accounts change spare` and
/// no flags: typed id, `yes`, a new key. The Keychain gets `delete` then
/// `set` (no read), the user store `account_removed` (`reason:
/// provider_change`) then `account_added` (`typed_id_yes_new_key`), and
/// `accounts test spare` then sends.
#[tokio::test]
async fn an_account_migrate_left_unconfirmed_is_confirmed_by_change() {
    let (s, ran, _kit) = migrated_without_clash().await;
    let env = &s.env;
    assert_eq!(ran.code, 0, "{}", ran.all());
    let new_key = "sk-ant-canary-migrate-new-5e5e5e5e-NEWK";

    let kit = Kit::new(FakeTerminal::new(), &s.keychain);
    kit.term.line("spare");
    kit.term.line("yes");
    kit.term.line(new_key);
    let ran = kit.run(env, &["change", "spare"]).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    assert_eq!(kit.gets(), 0, "the old key is never read");
    assert_eq!(kit.calls(), 2, "delete, then set");
    assert_eq!(
        s.keychain
            .get("spare")
            .unwrap()
            .map(|k| k.expose().to_string())
            .as_deref(),
        Some(new_key)
    );
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
    assert!(
        has(&added[0], "confirmation", "typed_id_yes_new_key"),
        "{}",
        added[0]
    );
    assert!(has(&added[0], "host", "api.anthropic.com"), "{}", added[0]);

    let tester = Kit::new(FakeTerminal::piped(), &s.keychain);
    tester
        .egress
        .replay("api.anthropic.com", "anthropic/c1_count_stream.sse");
    let ran = tester.run(env, &["test", "spare"]).await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    assert_eq!(tester.egress.requests().len(), 1);
    assert!(!ran.all().contains(new_key));
    assert!(!env.audit().contains(new_key));
}
