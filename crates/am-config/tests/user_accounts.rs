//! Story 2.12 (am-config side; AD-15, AD-16, AD-17): Accounts belong to this
//! Mac. am-config owns the user Accounts file, `<app data dir>/accounts.yaml`:
//!
//! - `am_config::user_accounts::load(path)` is its only reader: findings
//!   (each with the file's path and line) and the `UserAccounts` list.
//!   Entries are `id`, `provider`, `name`, `base_url` and `secret_ref`,
//!   never a key; ids are kebab-case and unique, `local-lmstudio` and
//!   `local-ollama` are refused, and `secret_ref`, when present, must equal
//!   the id through the one predicate `am_config::key_ref` every key reader
//!   uses. Story 2.3's provider and `base_url` rules apply unchanged (the
//!   tests of those rules moved here from `validate.rs` and `anatomy.rs`).
//! - `ConfigWriter::for_user_accounts(<app data dir>, history)` writes only
//!   that one file, with the same atomic write, intent, History and
//!   `ConfigCreate` as a project file.
//! - A project names Accounts by id only. `LoadOptions::user_accounts`
//!   passes the user list: `None` checks an Account reference for form only;
//!   with a list, an id the list lacks is a Warning at each line naming it
//!   with the fix "add that Account on this Mac: `agentic accounts add
//!   <id>`"; a known id of the wrong kind stays an Error; the reserved ids
//!   always resolve.
//! - An old `models/accounts.yaml` in a project is an Error at that file,
//!   line 1, with the fix `agentic accounts migrate --project <path>`; it is
//!   not parsed as Accounts, in project and template mode alike.
//! - Fixtures carry their Accounts in `fixtures/accounts/<variant>.yaml`;
//!   `fixtures/projects/legacy-accounts/` is the one old-format project.
//!
//! Tests read fixtures and temporary folders only, never the real app data
//! dir. Written before the module exists.
#![forbid(unsafe_code)]

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use am_config::{
    ConfigCreate, ConfigWriter, EditOp, Finding, LEGACY_ACCOUNTS_FILE, LoadOptions, Severity,
    USER_ACCOUNTS_FILE, UserAccounts, WriteError, key_ref, load_in, user_accounts,
};
use am_domain::capability::AccountKind;
use am_domain::{Actor, ConfigHistory, HistoryKind};
use am_testkit::InMemoryConfigHistory;
use common::{accounts_fixture, copy_of, edit, errors, findings, findings_with, fixture, write};

fn owner() -> Actor {
    Actor::Human {
        person: "owner".into(),
        hat: "Owner".into(),
    }
}

/// The findings of an Accounts file holding `text`, written to a temporary
/// app data dir.
fn user_findings(text: &str) -> Vec<Finding> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(USER_ACCOUNTS_FILE);
    std::fs::write(&path, text).unwrap();
    user_accounts::load(&path).findings
}

/// The one finding at `line` whose message contains `message`; its file is
/// the Accounts file loaded (whatever path it was loaded from).
fn at_line<'a>(found: &'a [Finding], file: &str, line: u32, message: &str) -> &'a Finding {
    found
        .iter()
        .find(|f| f.file.ends_with(file) && f.line == line && f.message.contains(message))
        .unwrap_or_else(|| panic!("no finding {file}:{line}: {message} in {found:#?}"))
}

fn user_errors(found: &[Finding]) -> Vec<&Finding> {
    found
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .collect()
}

fn ids(list: &UserAccounts) -> Vec<String> {
    list.ids()
}

const VALID: &str = "schema_version: 2\naccounts:\n  - id: cloud\n    provider: anthropic\n    name: Cloud account\n    secret_ref: cloud\n";

// ---------------------------------------------------------- the user file

/// Templates (Accounts side): every Accounts fixture a valid fixture or the
/// sample template needs loads with no finding, and each Account's kind
/// follows its provider (moved from `anatomy.rs`).
#[test]
fn every_accounts_fixture_loads_clean_and_kinds_follow_the_provider() {
    for (variant, expect) in [
        ("sample", vec!["anthropic", "openai"]),
        ("valid", vec!["cloud"]),
        ("anatomy", vec!["finder", "crawly"]),
        ("sources", vec!["crawly", "finder", "local"]),
    ] {
        let loaded = user_accounts::load(&accounts_fixture(variant));
        assert!(
            loaded.findings.is_empty(),
            "{variant}: {:#?}",
            loaded.findings
        );
        assert_eq!(ids(&loaded.accounts), expect, "{variant}");
    }
    let anatomy = user_accounts::load(&accounts_fixture("anatomy")).accounts;
    assert_eq!(anatomy.get("finder").unwrap().kind, AccountKind::Search);
    assert_eq!(
        anatomy.get("crawly").unwrap().kind,
        AccountKind::CrawlService
    );
    let valid = user_accounts::load(&accounts_fixture("valid")).accounts;
    assert_eq!(valid.get("cloud").unwrap().kind, AccountKind::Model);
    assert!(valid.get("nope").is_none());
    let sources = user_accounts::load(&accounts_fixture("sources")).accounts;
    assert_eq!(
        sources.get("local").unwrap().base_url.as_deref(),
        Some("http://127.0.0.1:8888"),
        "searxng-remote on this Mac is a valid search Account (moved from sources.rs)"
    );
}

/// The broken fixtures' Account findings, gathered in
/// `fixtures/accounts/broken.yaml`: a secret-looking value (never echoed),
/// an unknown key, a remote plain-http `base_url`, a `secret_ref` naming
/// another entry, a reserved id, a missing provider and a duplicate id.
#[test]
fn the_broken_accounts_fixture_reports_each_error_at_its_line() {
    let loaded = user_accounts::load(&accounts_fixture("broken"));
    let found = &loaded.findings;
    let f = at_line(found, "broken.yaml", 6, "secret-looking value");
    assert_eq!(f.severity, Severity::Error);
    assert!(f.fix.contains("Keychain"), "{}", f.fix);
    assert!(f.fix.contains("Account"), "{}", f.fix);
    assert!(
        !format!("{found:?}").contains("abcdef0123456789"),
        "a finding never echoes the value"
    );
    // An unknown key is named by its line; its text is hidden (FU6).
    let f = at_line(found, "broken.yaml", 6, "unknown key");
    assert!(!f.message.contains("api_key"), "{}", f.message);
    let f = at_line(found, "broken.yaml", 9, "'base_url' must be an https URL");
    assert_eq!(f.severity, Severity::Error);
    let f = at_line(found, "broken.yaml", 13, "secret_ref");
    assert_eq!(f.severity, Severity::Error);
    // `secret_ref` is never quoted (follow-up review F5; coordinator
    // decision 2026-11-22): a key pasted there would be printed.
    assert!(
        !f.message.contains("someone-else") && !f.fix.contains("someone-else"),
        "{f:?}"
    );
    assert!(f.fix.contains("secret_ref: other"), "{f:?}");
    at_line(found, "broken.yaml", 14, "reserved");
    at_line(found, "broken.yaml", 16, "missing 'provider'");
    at_line(found, "broken.yaml", 17, "cloud");
    assert!(user_errors(found).len() >= 7, "{found:#?}");
    // Findings come sorted by line.
    let lines: Vec<u32> = found.iter().map(|f| f.line).collect();
    let mut sorted = lines.clone();
    sorted.sort_unstable();
    assert_eq!(lines, sorted);
}

/// `secret_ref` ≠ id: a validate error at the line, and `key_ref` (the one
/// predicate every key reader uses) gives no Keychain entry for it; with
/// `secret_ref` equal to the id it names the id's own entry.
#[test]
fn a_secret_ref_must_equal_the_id() {
    let found = user_findings(
        "schema_version: 2\naccounts:\n  - id: cloud\n    provider: anthropic\n    secret_ref: other\n",
    );
    let f = at_line(&found, USER_ACCOUNTS_FILE, 5, "secret_ref");
    assert_eq!(f.severity, Severity::Error);
    assert!(f.fix.contains("secret_ref: cloud"), "{f:?}");

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(USER_ACCOUNTS_FILE);
    std::fs::write(
        &path,
        "schema_version: 2\naccounts:\n  - id: cloud\n    provider: anthropic\n    secret_ref: other\n  - id: fine\n    provider: anthropic\n    secret_ref: fine\n",
    )
    .unwrap();
    let list = user_accounts::load(&path).accounts;
    let bad = list.get("cloud").expect("still listed, with its error");
    assert_eq!(key_ref(bad), None, "never another entry's key");
    assert_eq!(key_ref(list.get("fine").unwrap()), Some("fine"));
}

/// A missing file is an empty list with no finding: the first add on a Mac.
#[test]
fn a_missing_accounts_file_is_an_empty_list() {
    let dir = tempfile::tempdir().unwrap();
    let loaded = user_accounts::load(&dir.path().join(USER_ACCOUNTS_FILE));
    assert!(loaded.findings.is_empty(), "{:#?}", loaded.findings);
    assert!(ids(&loaded.accounts).is_empty());
    assert_eq!(UserAccounts::default().ids(), Vec::<String>::new());
}

/// A file newer than this binary is refused in plain words (AD-7), and ids
/// are kebab-case.
#[test]
fn a_newer_file_and_a_bad_id_are_errors() {
    let found = user_findings("schema_version: 99\naccounts: []\n");
    let f = at_line(&found, USER_ACCOUNTS_FILE, 1, "99");
    assert_eq!(f.severity, Severity::Error);
    let found = user_findings(
        "schema_version: 2\naccounts:\n  - id: Bad_Id\n    provider: anthropic\n    secret_ref: Bad_Id\n",
    );
    // The id is named by its line and field, its value hidden (FU6: a value
    // in the user file is never repeated unless it is a valid id).
    let f = at_line(&found, USER_ACCOUNTS_FILE, 3, "'id'");
    assert_eq!(f.severity, Severity::Error);
    assert!(!format!("{f:?}").contains("Bad_Id"), "{f:?}");
}

/// Moved from `validate.rs`: an empty required value is reported like a
/// missing one.
#[test]
fn an_empty_provider_is_reported_like_a_missing_one() {
    let found = user_findings(&VALID.replace("provider: anthropic", "provider:"));
    at_line(&found, USER_ACCOUNTS_FILE, 3, "missing 'provider'");
}

/// Moved from `validate.rs`: the general `base_url` rule (every provider
/// but `openai-compatible`): https, or plain http only to this Mac, never
/// with a user name or password.
#[test]
fn plain_http_is_only_for_this_mac_and_never_with_userinfo() {
    let found = user_findings(&format!(
        "{VALID}  - id: tricky\n    provider: firecrawl\n    base_url: http://127.0.0.1:80@remote.example/\n\
         \x20 - id: lan\n    provider: firecrawl\n    base_url: http://192.168.1.5:8080/v1\n\
         \x20 - id: creds\n    provider: firecrawl\n    base_url: https://user:pw@example.org/v1\n\
         \x20 - id: box\n    provider: firecrawl\n    base_url: http://localhost:1234/v1\n\
         \x20 - id: loop\n    provider: firecrawl\n    base_url: http://127.0.0.1:1234/v1\n"
    ));
    for line in [9, 12, 15] {
        let f = at_line(&found, USER_ACCOUNTS_FILE, line, "https URL");
        assert!(
            f.message.contains("localhost") && f.fix.contains("localhost"),
            "{f:?}"
        );
    }
    assert_eq!(user_errors(&found).len(), 3, "{found:#?}");
}

/// Moved from `validate.rs`: one rule for an `openai-compatible` Account's
/// `base_url`, the one `agentic accounts add` applies.
#[test]
fn a_compatible_base_url_takes_only_the_two_accepted_forms() {
    let refused = [
        "http://localhost:1234/v1",
        "https://10.0.0.5/v1",
        "https://relay.example:8443/v1",
        "http://127.0.0.1/v1",
        "https://127.0.0.1:8080/v1",
        "http://192.168.1.5:8080/v1",
        "https://someone:hunter2canary@example.org/v1",
        "https://relay.example/v1?tier=a",
        "http://example.org/v1",
        "https://2130706433/v1",
    ];
    let accepted = ["https://relay.example/api/v1", "http://127.0.0.1:1234/v1"];
    let mut text = VALID.to_string();
    let first = text.lines().count() as u32 + 3;
    for (n, url) in refused.iter().chain(&accepted).enumerate() {
        text.push_str(&format!(
            "  - id: box{n}\n    provider: openai-compatible\n    base_url: {url}\n"
        ));
    }
    let found = user_findings(&text);
    for (n, url) in refused.iter().enumerate() {
        let f = at_line(&found, USER_ACCOUNTS_FILE, first + 3 * n as u32, "base_url");
        assert_eq!(f.severity, Severity::Error, "{url}: {f:?}");
        assert!(
            f.fix.contains("https://<host>") && f.fix.contains("http://127.0.0.1:<port>"),
            "{url}: the fix gives the two forms: {f:?}"
        );
        assert!(!f.fix.contains("localhost"), "{url}: {f:?}");
        assert!(
            !format!("{f:?}").contains("hunter2canary"),
            "{url}: a password is never repeated: {f:?}"
        );
    }
    assert_eq!(
        user_errors(&found).len(),
        refused.len(),
        "the two accepted forms are no error: {found:#?}"
    );
}

/// Moved from `validate.rs`: a compatible Account with no `base_url` is
/// told the two forms.
#[test]
fn a_compatible_account_without_a_base_url_is_told_the_two_forms() {
    let line = VALID.lines().count() as u32 + 1;
    let found = user_findings(&format!(
        "{VALID}  - id: box\n    provider: openai-compatible\n"
    ));
    let f = at_line(&found, USER_ACCOUNTS_FILE, line, "needs 'base_url'");
    assert!(
        f.fix.contains("https://<host>") && f.fix.contains("http://127.0.0.1:<port>"),
        "{f:?}"
    );
    assert!(!f.fix.contains("localhost"), "{f:?}");
    assert_eq!(user_errors(&found).len(), 1, "{found:#?}");
}

/// Moved from `anatomy.rs`: firecrawl and searxng-remote need a `base_url`.
#[test]
fn firecrawl_and_searxng_remote_need_a_base_url() {
    let text = std::fs::read_to_string(accounts_fixture("anatomy"))
        .unwrap()
        .replacen("    base_url: https://crawl.example.org\n", "", 1)
        .replacen("provider: tavily", "provider: searxng-remote", 1);
    let found = user_findings(&text);
    at_line(&found, USER_ACCOUNTS_FILE, 3, "base_url");
    at_line(&found, USER_ACCOUNTS_FILE, 7, "base_url");
}

// ---------------------------------------------------------------- writer

/// A temporary app data dir for the writer.
fn app_data() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

/// First add on a Mac (am-config side): the writer for the user file
/// creates it by `ConfigCreate` (a `create` History entry for
/// `accounts.yaml`); a later add is an `edit` of it; the file never holds a
/// key and loads clean.
#[test]
fn the_user_writer_creates_then_edits_the_accounts_file() {
    let dir = app_data();
    let history = Arc::new(InMemoryConfigHistory::default());
    let w = ConfigWriter::for_user_accounts(dir.path(), history.clone()).unwrap();

    let bytes = am_config::new_accounts_file("cloud", "anthropic", None, None).unwrap();
    let created = w
        .create(
            owner(),
            ConfigCreate {
                file: USER_ACCOUNTS_FILE.into(),
                bytes: bytes.clone(),
                audit: Vec::new(),
            },
        )
        .unwrap();
    assert_eq!(created.kind, HistoryKind::Create);
    assert_eq!(created.file, USER_ACCOUNTS_FILE);
    let path = dir.path().join(USER_ACCOUNTS_FILE);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);

    let now = am_config::accounts_file(dir.path());
    let edited = w
        .apply_with_audit(
            owner(),
            am_config::add_account_edit(&now, "oa", "openai", None, None),
            now.sha256.as_deref().unwrap(),
            Vec::new(),
        )
        .unwrap();
    assert_eq!(edited.kind, HistoryKind::Edit);
    let loaded = user_accounts::load(&path);
    assert!(loaded.findings.is_empty(), "{:#?}", loaded.findings);
    assert_eq!(ids(&loaded.accounts), ["cloud", "oa"]);
    assert_eq!(history.entries(None).unwrap().len(), 2);
    assert!(w.is_own_write(USER_ACCOUNTS_FILE, &edited.after_sha256));
}

/// The user writer writes only that one file: any other name (a project
/// file, the legacy file, a path outside) is refused and nothing is written
/// or recorded.
#[test]
fn the_user_writer_writes_only_the_accounts_file() {
    let dir = app_data();
    let history = Arc::new(InMemoryConfigHistory::default());
    let w = ConfigWriter::for_user_accounts(dir.path(), history.clone()).unwrap();
    for file in [
        "company.yaml",
        LEGACY_ACCOUNTS_FILE,
        "user.db",
        "../accounts.yaml",
        "/tmp/accounts.yaml",
        "sub/accounts.yaml",
    ] {
        let err = w
            .create(
                owner(),
                ConfigCreate {
                    file: file.into(),
                    bytes: VALID.as_bytes().to_vec(),
                    audit: Vec::new(),
                },
            )
            .unwrap_err();
        assert!(matches!(err, WriteError::BadPath { .. }), "{file}: {err:?}");
    }
    let err = w
        .apply(
            owner(),
            am_config::ConfigEdit {
                file: "company.yaml".into(),
                ops: vec![EditOp::Set {
                    path: vec!["name".into()],
                    value: serde_json::json!("x"),
                }],
                body: None,
            },
        )
        .unwrap_err();
    assert!(matches!(err, WriteError::BadPath { .. }), "{err:?}");
    assert!(history.entries(None).unwrap().is_empty());
    assert!(history.pending_intents().unwrap().is_empty());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

/// The user writer refuses a result with an error (a `secret_ref` naming
/// another entry, a reserved id) and a secret-looking value, writing
/// nothing.
#[test]
fn the_user_writer_refuses_an_accounts_file_with_an_error() {
    let dir = app_data();
    let history = Arc::new(InMemoryConfigHistory::default());
    let w = ConfigWriter::for_user_accounts(dir.path(), history.clone()).unwrap();
    for (text, words) in [
        (
            "schema_version: 2\naccounts:\n  - id: cloud\n    provider: anthropic\n    secret_ref: other\n",
            "secret_ref",
        ),
        (
            "schema_version: 2\naccounts:\n  - id: local-ollama\n    provider: anthropic\n",
            "reserved",
        ),
        (
            "schema_version: 2\naccounts:\n  - id: cloud\n    provider: anthropic\n    description: sk-ant-canary0canary0canary\n",
            "secret-looking",
        ),
    ] {
        let err = w
            .create(
                owner(),
                ConfigCreate {
                    file: USER_ACCOUNTS_FILE.into(),
                    bytes: text.as_bytes().to_vec(),
                    audit: Vec::new(),
                },
            )
            .unwrap_err();
        assert!(err.to_string().contains(words), "{words}: {err}");
        assert!(!err.to_string().contains("sk-ant-canary"), "{err}");
    }
    assert!(!dir.path().join(USER_ACCOUNTS_FILE).exists());
    assert!(history.entries(None).unwrap().is_empty());
}

/// Outside change (am-config side): `outside_changes` over the user file
/// names it when its bytes differ from its latest History entry's.
#[test]
fn outside_changes_names_the_hand_edited_accounts_file() {
    let dir = app_data();
    let history = Arc::new(InMemoryConfigHistory::default());
    let w = ConfigWriter::for_user_accounts(dir.path(), history.clone()).unwrap();
    w.create(
        owner(),
        ConfigCreate {
            file: USER_ACCOUNTS_FILE.into(),
            bytes: VALID.as_bytes().to_vec(),
            audit: Vec::new(),
        },
    )
    .unwrap();
    assert!(
        am_config::outside_changes(dir.path(), history.as_ref())
            .unwrap()
            .is_empty()
    );
    let path = dir.path().join(USER_ACCOUNTS_FILE);
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, text.replace("Cloud account", "Cloud")).unwrap();
    assert_eq!(
        am_config::outside_changes(dir.path(), history.as_ref()).unwrap(),
        [USER_ACCOUNTS_FILE.to_string()]
    );
}

// ------------------------------------------------------ project references

fn seat(model: &str, fallback: &str) -> String {
    format!(
        "---\nschema_version: 2\nname: Writer\nrole: Writer\nteam: core\nmodel:\n  account: {model}\n  model: example-large\nfallback:\n  - account: {fallback}\n    model: example-large\n---\nWrite.\n"
    )
}

fn with_list(list: Option<UserAccounts>) -> LoadOptions {
    LoadOptions {
        user_accounts: list,
        ..LoadOptions::default()
    }
}

fn found_in(root: &Path, list: Option<UserAccounts>) -> Vec<Finding> {
    load_in(root, with_list(list)).unwrap().findings
}

/// Missing id: a seat names `anthropic` and the user list lacks it, so the
/// seat line gets a Warning with the add fix; the reserved ids resolve; an
/// id the list holds gives nothing; with no list (`None`, as `validate
/// --template` and the library tests pass) the reference is checked for
/// form only and gives no finding.
#[test]
fn an_id_missing_on_this_mac_is_a_warning_at_each_line_naming_it() {
    let dir = common::project_with(&[("seats/writer.md", &seat("local-lmstudio", "anthropic"))]);
    let root = dir.path();

    let found = found_in(root, Some(UserAccounts::default()));
    let f = common::at(&found, "seats/writer.md", 10, "anthropic");
    assert_eq!(f.severity, Severity::Warning, "{f:?}");
    assert!(f.fix.contains("add that Account on this Mac"), "{f:?}");
    assert!(f.fix.contains("agentic accounts add anthropic"), "{f:?}");
    assert!(errors(&found).is_empty(), "{found:#?}");
    assert!(
        load_in(root, with_list(Some(UserAccounts::default())))
            .unwrap()
            .snapshot
            .is_some(),
        "a warning keeps the snapshot"
    );

    let found = found_in(root, Some(common::user_accounts("sample")));
    assert!(found.is_empty(), "{found:#?}");

    let found = found_in(root, None);
    assert!(found.is_empty(), "form only: {found:#?}");

    // Both lines naming the id are warned.
    write(root, "seats/writer.md", &seat("anthropic", "anthropic"));
    let found = found_in(root, Some(UserAccounts::default()));
    for line in [7, 10] {
        let f = common::at(&found, "seats/writer.md", line, "anthropic");
        assert_eq!(f.severity, Severity::Warning);
    }
}

/// A known id of the wrong kind stays an Error (moved from `anatomy.rs`,
/// now with the user list passed): a crawl-service Account as a
/// `web_search` provider.
#[test]
fn a_provider_account_of_the_wrong_kind_is_an_error() {
    let found = findings_with(&fixture("broken-anatomy"), "anatomy");
    let f = common::at(&found, "seats/searcher.md", 9, "crawly");
    assert_eq!(f.severity, Severity::Error);
    assert!(f.message.contains("crawl-service"), "{}", f.message);
    assert!(f.fix.contains("search"), "{}", f.fix);
}

/// A Source and a capability provider naming an id the list lacks are
/// warned too.
#[test]
fn a_source_and_a_capability_naming_a_missing_id_are_warned() {
    let found = findings_with(&fixture("sources-ok"), "valid");
    for f in found.iter().filter(|f| f.message.contains("crawly")) {
        assert_eq!(f.severity, Severity::Warning, "{f:?}");
        assert!(f.fix.contains("agentic accounts add crawly"), "{f:?}");
    }
    assert!(
        found
            .iter()
            .any(|f| f.file.starts_with("sources/") && f.message.contains("crawly")),
        "{found:#?}"
    );
    assert!(errors(&found).is_empty(), "{found:#?}");
}

// ----------------------------------------------------- old project files

fn legacy() -> PathBuf {
    fixture("legacy-accounts")
}

/// Old project file: a project still holding `models/accounts.yaml` gets an
/// Error at that file, line 1, with the migrate fix; in project and
/// template mode alike.
#[test]
fn an_old_project_accounts_file_is_an_error_with_the_migrate_fix() {
    for (mode, opts) in [
        ("project", LoadOptions::default()),
        ("template", LoadOptions::template(&legacy())),
        (
            "project with a list",
            with_list(Some(common::user_accounts("valid"))),
        ),
    ] {
        let loaded = load_in(&legacy(), opts).unwrap();
        let f = common::at(&loaded.findings, LEGACY_ACCOUNTS_FILE, 1, "");
        assert_eq!(f.severity, Severity::Error, "{mode}: {f:?}");
        assert!(
            f.fix.contains("agentic accounts migrate --project"),
            "{mode}: {f:?}"
        );
        assert!(
            loaded.snapshot.is_none(),
            "{mode}: an error gives no snapshot"
        );
        assert_eq!(
            errors(&loaded.findings).len(),
            1,
            "{mode}: the one error: {:#?}",
            loaded.findings
        );
    }
}

/// The old file is not parsed as Accounts: whatever it holds, the project
/// gets only the one error at line 1, and nothing in it declares an Account
/// for the project (its id is still missing on this Mac).
#[test]
fn an_old_project_accounts_file_is_never_parsed_as_accounts() {
    let copy = copy_of("legacy-accounts");
    write(
        copy.path(),
        LEGACY_ACCOUNTS_FILE,
        "schema_version: 2\naccounts: 5\nsurprise: [\n",
    );
    let found = findings(copy.path());
    let errs = errors(&found);
    assert_eq!(errs.len(), 1, "{found:#?}");
    assert_eq!(
        (errs[0].file.as_str(), errs[0].line),
        (LEGACY_ACCOUNTS_FILE, 1)
    );

    // The id the old file declares is not declared for the project: with an
    // empty user list the seat naming it is warned.
    let found = found_in(&legacy(), Some(UserAccounts::default()));
    let f = common::at(&found, "seats/alpha.md", 11, "cloud");
    assert_eq!(f.severity, Severity::Warning);
}

/// Templates: the sample template validates with its Accounts fixture, and
/// so does each valid fixture with its own variant; the legacy fixture
/// fails as above.
#[test]
fn templates_and_fixtures_validate_with_their_accounts_variant() {
    let sample = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../templates/samples");
    let sample = std::fs::read_dir(&sample)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.is_dir())
        .expect("the sample company");
    for (root, variant) in [
        (sample, "sample"),
        (fixture("valid"), "valid"),
        (fixture("anatomy"), "anatomy"),
        (fixture("sources-ok"), "sources"),
    ] {
        let opts = LoadOptions {
            user_accounts: Some(common::user_accounts(variant)),
            ..LoadOptions::template(&root)
        };
        let loaded = load_in(&root, opts).unwrap();
        assert!(
            errors(&loaded.findings).is_empty(),
            "{}: {:#?}",
            root.display(),
            loaded.findings
        );
        assert!(
            !loaded
                .findings
                .iter()
                .any(|f| f.fix.contains("agentic accounts add")),
            "{}: every id it names is in its variant: {:#?}",
            root.display(),
            loaded.findings
        );
        assert!(
            !root.join(LEGACY_ACCOUNTS_FILE).exists(),
            "{}: no old-format file",
            root.display()
        );
    }
}

/// Only `fixtures/projects/legacy-accounts/` holds an old-format file among
/// the fixture projects and templates.
#[test]
fn only_the_legacy_fixture_holds_an_old_accounts_file() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut found = Vec::new();
    for group in [
        "fixtures/projects",
        "templates/departments",
        "templates/roles",
        "templates/samples",
    ] {
        for e in std::fs::read_dir(repo.join(group)).unwrap() {
            let dir = e.unwrap().path();
            if dir.join(LEGACY_ACCOUNTS_FILE).exists() {
                found.push(dir.file_name().unwrap().to_string_lossy().into_owned());
            }
        }
    }
    assert_eq!(found, ["legacy-accounts"]);
}

/// The valid fixture's seat naming `cloud` loads (form only) after its old
/// file left, and with its variant passed the project has no finding.
#[test]
fn the_valid_fixture_names_its_account_by_id_only() {
    assert!(findings(&fixture("valid")).is_empty());
    assert!(findings_with(&fixture("valid"), "valid").is_empty());
    let copy = copy_of("valid");
    edit(
        copy.path(),
        "seats/beta.md",
        "account: cloud",
        "account: missing",
    );
    let found = findings_with(copy.path(), "valid");
    let f = common::at(&found, "seats/beta.md", 11, "missing");
    assert_eq!(f.severity, Severity::Warning);
}

// ======================================================================
// Review fixes (2026-11-22 review pass; plan `## Review Triage Log`),
// written first, before any `src/` change.
// ======================================================================

/// Quick 8: a secret-looking value, in a project file or in the user
/// Accounts file, gets a fix that sends the owner to this Mac's Accounts
/// (`agentic accounts add`), never to a project's old
/// `models/accounts.yaml`.
#[test]
fn the_secret_value_fix_names_this_macs_accounts_not_the_old_project_file() {
    const KEY: &str = "sk-ant-api03-fixcanary-9d9d9d9d9d9d-FIXX";
    let dir = common::project_with(&[(
        "seats/writer.md",
        &format!(
            "{}Use the key {KEY} for every call.\n",
            seat("local-lmstudio", "local-ollama")
        ),
    )]);
    let project = found_in(dir.path(), None);
    let user = user_findings(&format!(
        "schema_version: 2\naccounts:\n  - id: cloud\n    provider: anthropic\n    name: {KEY}\n    secret_ref: cloud\n"
    ));
    for (what, found) in [("a project file", project), ("the user file", user)] {
        let f = found
            .iter()
            .find(|f| f.message.contains("secret-looking"))
            .unwrap_or_else(|| panic!("{what}: no secret finding in {found:#?}"));
        assert!(
            !f.fix.contains(LEGACY_ACCOUNTS_FILE),
            "{what}: the fix names the old project file: {f:?}"
        );
        assert!(
            f.fix.contains("agentic accounts add"),
            "{what}: the fix names how to add an Account on this Mac: {f:?}"
        );
        assert!(
            !f.message.contains(KEY) && !f.fix.contains(KEY),
            "{what}: {f:?}"
        );
    }
}
