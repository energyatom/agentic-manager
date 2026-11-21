//! Story 1.4 acceptance tests: the loader and validator over the fixture
//! projects (one test per row of the plan's I/O matrix) plus small projects
//! built in a temporary folder for the variants of each row.
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use am_config::{Finding, LoadError, Severity, load};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/projects")
        .join(name)
}

fn copy_dir(from: &Path, to: &Path) {
    // A stream copy (story 1.13: never `std::fs::copy`).
    am_testkit::copy_tree(from, to);
}

/// A copy of the valid fixture in a temporary folder, to add or change files in.
fn valid_copy() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&fixture("valid"), dir.path());
    dir
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn findings(root: &Path) -> Vec<Finding> {
    load(root).expect("the project folder exists").findings
}

fn errors(found: &[Finding]) -> Vec<&Finding> {
    found
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .collect()
}

/// The one finding at `file:line` whose message contains `message`.
fn at<'a>(found: &'a [Finding], file: &str, line: u32, message: &str) -> &'a Finding {
    found
        .iter()
        .find(|f| f.file == file && f.line == line && f.message.contains(message))
        .unwrap_or_else(|| panic!("no finding {file}:{line}: {message} in {found:#?}"))
}

// Row: valid project.

#[test]
fn valid_project_has_no_findings_and_every_entity() {
    let loaded = load(&fixture("valid")).unwrap();
    assert!(loaded.findings.is_empty(), "{:#?}", loaded.findings);
    let snap = loaded.snapshot.expect("a valid project gives a snapshot");
    assert_eq!(snap.company().name, "Example Project");
    assert_eq!(snap.company().teams.len(), 2);
    assert_eq!(snap.accounts().len(), 1);
    assert_eq!(snap.prices().models.len(), 1);
    assert_eq!(snap.capabilities().models.len(), 1);
    assert_eq!(snap.tools().mcp_servers.len(), 1);
    let seats: Vec<&String> = snap.seats().keys().collect();
    assert_eq!(seats, ["alpha", "beta"]);
    assert_eq!(snap.seats()["alpha"].tools.len(), 3);
    assert!(
        snap.seats()["alpha"]
            .instructions
            .contains("Read the items")
    );
    assert_eq!(snap.workflows().len(), 1);
    assert_eq!(snap.workflows()["digest"].steps.len(), 2);
    assert_eq!(snap.workflows()["digest"].triggers.len(), 1);
    assert_eq!(snap.tables().len(), 1);
    assert_eq!(snap.tables()["items"].columns.len(), 4);
    assert_eq!(snap.schemas().len(), 1);
    assert!(snap.schemas().contains_key("summary"));
    assert_eq!(snap.dashboards().len(), 1);
    assert_eq!(snap.sources().len(), 1);
    assert!(snap.sources().contains_key("feed"));
    let hash = snap.hash();
    assert_eq!(hash.len(), 64, "{hash}");
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn the_same_files_give_the_same_hash() {
    let first = load(&fixture("valid")).unwrap().snapshot.unwrap();
    let again = load(&fixture("valid")).unwrap().snapshot.unwrap();
    assert_eq!(first.hash(), again.hash());
    // The hash covers project-relative paths and bytes, not where the folder is.
    let copy = valid_copy();
    let moved = load(copy.path()).unwrap().snapshot.unwrap();
    assert_eq!(first.hash(), moved.hash());
    // One changed byte changes it.
    let body = std::fs::read_to_string(copy.path().join("seats/alpha.md")).unwrap();
    write(copy.path(), "seats/alpha.md", &format!("{body}More.\n"));
    let changed = load(copy.path()).unwrap().snapshot.unwrap();
    assert_ne!(first.hash(), changed.hash());
}

#[test]
fn a_missing_folder_is_a_load_error() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("nope");
    assert!(matches!(load(&missing), Err(LoadError::NotFound(_))));
}

// Row: unknown seat id.

#[test]
fn unknown_seat_id_names_the_known_seats() {
    let found = findings(&fixture("broken"));
    let f = at(
        &found,
        "workflows/ghost.yaml",
        6,
        "unknown seat 'ghost-seat'",
    );
    assert_eq!(f.severity, Severity::Error);
    assert!(
        f.fix.contains("alpha") && f.fix.contains("teleporter"),
        "{}",
        f.fix
    );
}

// Row: bad cron.

#[test]
fn bad_cron_shows_the_five_field_format() {
    let found = findings(&fixture("broken"));
    let f = at(&found, "workflows/nightly.yaml", 6, "invalid cron");
    assert_eq!(f.severity, Severity::Error);
    assert!(f.fix.contains("5 fields"), "{}", f.fix);
    assert!(
        f.fix.contains("minute hour day-of-month month day-of-week"),
        "{}",
        f.fix
    );
}

#[test]
fn a_six_field_cron_is_refused() {
    let copy = valid_copy();
    let text = std::fs::read_to_string(copy.path().join("workflows/digest.yaml")).unwrap();
    write(
        copy.path(),
        "workflows/digest.yaml",
        &text.replace("\"0 9 * * 1-5\"", "\"0 0 9 * * 1-5\""),
    );
    let found = findings(copy.path());
    at(&found, "workflows/digest.yaml", 7, "invalid cron");
}

// Row: secret-looking value.

#[test]
fn secret_looking_value_in_the_broken_fixture() {
    let found = findings(&fixture("broken"));
    let f = at(&found, "models/accounts.yaml", 6, "secret-looking value");
    assert_eq!(f.severity, Severity::Error);
    assert!(f.fix.contains("Keychain"), "{}", f.fix);
    assert!(f.fix.contains("Account"), "{}", f.fix);
    assert!(
        !f.message.contains("abcdef0123456789"),
        "a finding never echoes the value"
    );
}

#[test]
fn every_secret_pattern_is_caught_in_any_file() {
    let copy = valid_copy();
    // Canary values: shaped like keys, never real.
    write(
        copy.path(),
        "sources/leaky.yaml",
        "schema_version: 1\n\
         type: feed\n\
         note: sk-ant-canary0canary0canary\n\
         other: hf_canary0canary0canary\n\
         api_key: abc123canary\n\
         openai_api_key: abc123canary\n\
         github_token: abc123canary\n\
         Password: abc123canary\n\
         gh: ghp_canary0canary0canary0canary\n\
         aws: AKIACANARY0CANARY000\n\
         goog: AIzaCANARY0CANARY0CANARY0CANARY0CANARY0\n\
         slack: xoxb-canary0canary0canary\n\
         generic: sk-canary0canary0canary0canary\n\
         pat: github_pat_canary0canary0canary\n\
         pem: \"-----BEGIN RSA PRIVATE KEY-----\"\n",
    );
    let found = findings(copy.path());
    for line in 3..=15 {
        let f = at(&found, "sources/leaky.yaml", line, "secret-looking value");
        assert!(
            !f.message.contains("canary"),
            "never echo the value: {}",
            f.message
        );
        assert!(f.fix.contains("Keychain"));
    }
    // Instructions in a seat body are scanned too.
    let body = std::fs::read_to_string(copy.path().join("seats/beta.md")).unwrap();
    write(
        copy.path(),
        "seats/beta.md",
        &format!("{body}Use the key sk-ant-canary0canary0canary when asked.\n"),
    );
    let found = findings(copy.path());
    // beta.md gained its capabilities block (3 lines) in story 1.12.
    at(&found, "seats/beta.md", 21, "secret-looking value");
}

// Row: token-count key.

#[test]
fn token_count_and_reference_keys_are_not_secrets() {
    // The valid fixture holds max_tokens, max_output_tokens and secret_ref.
    let found = findings(&fixture("valid"));
    assert!(
        found.iter().all(|f| !f.message.contains("secret")),
        "{found:#?}"
    );
    let copy = valid_copy();
    write(
        copy.path(),
        "sources/budgets.yaml",
        "schema_version: 1\n\
         type: feed\n\
         max_tokens: 4096\n\
         max_output_tokens: 8192\n\
         token_budget: 100\n\
         secret_ref: cloud\n\
         tokens: 3\n\
         api_key: \"\"\n\
         task: sk-short\n\
         word: mask-ant-thing\n",
    );
    // Source files have a schema from story 1.12, so these keys are unknown
    // keys there; none of them is a secret-looking value.
    let found = findings(copy.path());
    assert!(
        found.iter().all(|f| !f.message.contains("secret-looking")),
        "{found:#?}"
    );
}

// Row: missing schema_version.

#[test]
fn missing_schema_version_is_reported_on_line_one() {
    let found = findings(&fixture("broken"));
    let f = at(&found, "tables/bare.yaml", 1, "missing schema_version");
    assert_eq!(f.severity, Severity::Error);
    assert!(f.fix.contains("schema_version: 1"), "{}", f.fix);
}

#[test]
fn an_unknown_schema_version_is_an_error() {
    let copy = valid_copy();
    let text = std::fs::read_to_string(copy.path().join("tables/items.yaml")).unwrap();
    write(
        copy.path(),
        "tables/items.yaml",
        &text.replace("schema_version: 1", "schema_version: 2"),
    );
    let found = findings(copy.path());
    at(&found, "tables/items.yaml", 1, "schema_version 2");
}

// Row: unknown tool id.

#[test]
fn unknown_tool_lists_the_built_in_ids() {
    let found = findings(&fixture("broken"));
    let f = at(&found, "seats/teleporter.md", 7, "unknown tool 'teleport'");
    assert_eq!(f.severity, Severity::Error);
    for id in ["web_search", "read_file", "table_write", "ask_seat"] {
        assert!(f.fix.contains(id), "{id} missing from {}", f.fix);
    }
}

#[test]
fn built_in_tool_table_lives_in_the_domain() {
    let tools = am_domain::tool_manifest::builtin_tools();
    assert_eq!(tools.len(), 21);
    let search = am_domain::tool_manifest::builtin_tool("web_search").unwrap();
    assert_eq!(search.class, am_domain::tool_manifest::ActionClass::Read);
    let write = am_domain::tool_manifest::builtin_tool("ask_seat").unwrap();
    assert_eq!(
        write.class,
        am_domain::tool_manifest::ActionClass::LocalWrite
    );
    assert!(am_domain::tool_manifest::builtin_tool("teleport").is_none());
}

// Row: commented YAML.

#[test]
fn a_comment_is_a_warning_at_its_line() {
    let found = findings(&fixture("broken"));
    let f = at(
        &found,
        "dashboards/overview.yaml",
        4,
        "comments are not kept when the app saves this file",
    );
    assert_eq!(f.severity, Severity::Warning);
}

#[test]
fn comments_alone_leave_a_project_valid() {
    let copy = valid_copy();
    let text = std::fs::read_to_string(copy.path().join("company.yaml")).unwrap();
    write(
        copy.path(),
        "company.yaml",
        &format!(
            "# top note\n{}",
            text.replace("currency: EUR", "currency: EUR # money")
        ),
    );
    let seat = std::fs::read_to_string(copy.path().join("seats/alpha.md")).unwrap();
    write(
        copy.path(),
        "seats/alpha.md",
        &seat.replacen("name: Alpha", "# a frontmatter note\nname: Alpha", 1),
    );
    let loaded = load(copy.path()).unwrap();
    assert!(
        errors(&loaded.findings).is_empty(),
        "{:#?}",
        loaded.findings
    );
    assert!(loaded.snapshot.is_some());
    at(&loaded.findings, "company.yaml", 1, "comments are not kept");
    at(&loaded.findings, "company.yaml", 6, "comments are not kept");
    at(
        &loaded.findings,
        "seats/alpha.md",
        3,
        "comments are not kept",
    );
    // A '#' inside a value or a block scalar is not a comment.
    assert_eq!(loaded.findings.len(), 3, "{:#?}", loaded.findings);
}

// Row: many problems.

#[test]
fn the_broken_fixture_reports_everything_sorted() {
    let loaded = load(&fixture("broken")).unwrap();
    assert!(loaded.snapshot.is_none(), "errors give no snapshot");
    let found = loaded.findings;
    assert!(errors(&found).len() >= 5, "{found:#?}");
    for (file, line, message, severity) in [
        (
            "workflows/ghost.yaml",
            6,
            "unknown seat 'ghost-seat'",
            Severity::Error,
        ),
        ("workflows/nightly.yaml", 6, "invalid cron", Severity::Error),
        (
            "models/accounts.yaml",
            6,
            "secret-looking value",
            Severity::Error,
        ),
        (
            "tables/bare.yaml",
            1,
            "missing schema_version",
            Severity::Error,
        ),
        (
            "seats/teleporter.md",
            7,
            "unknown tool 'teleport'",
            Severity::Error,
        ),
        (
            "dashboards/overview.yaml",
            4,
            "comments are not kept when the app saves this file",
            Severity::Warning,
        ),
    ] {
        assert_eq!(
            at(&found, file, line, message).severity,
            severity,
            "{file}:{line}"
        );
    }
    let keys: Vec<(String, u32)> = found.iter().map(|f| (f.file.clone(), f.line)).collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
    for f in &found {
        assert!(f.line >= 1, "{f:?}");
        assert!(!f.fix.is_empty(), "{f:?}");
    }
}

// Decisions and Boundaries: unknown keys, ids and references.

#[test]
fn unknown_key_suggests_the_nearest_field() {
    let copy = valid_copy();
    let text = std::fs::read_to_string(copy.path().join("company.yaml")).unwrap();
    write(
        copy.path(),
        "company.yaml",
        &text.replace("timezone: UTC", "timezon: UTC"),
    );
    let found = findings(copy.path());
    let f = at(&found, "company.yaml", 4, "unknown key 'timezon'");
    assert!(f.fix.contains("did you mean 'timezone'"), "{}", f.fix);
}

#[test]
fn ids_are_kebab_case_and_equal_the_file_stem() {
    let copy = valid_copy();
    let seat = std::fs::read_to_string(copy.path().join("seats/beta.md")).unwrap();
    write(copy.path(), "seats/Gamma_Seat.md", &seat);
    let table = std::fs::read_to_string(copy.path().join("tables/items.yaml")).unwrap();
    write(
        copy.path(),
        "tables/items.yaml",
        &table.replacen("schema_version: 1\n", "schema_version: 1\nid: things\n", 1),
    );
    let found = findings(copy.path());
    at(&found, "seats/Gamma_Seat.md", 1, "kebab-case");
    at(&found, "tables/items.yaml", 2, "'things'");
}

#[test]
fn references_are_checked_across_files() {
    let copy = valid_copy();
    let seat = std::fs::read_to_string(copy.path().join("seats/beta.md")).unwrap();
    write(
        copy.path(),
        "seats/beta.md",
        &seat
            .replace("team: support", "team: nowhere")
            .replace("account: cloud", "account: missing"),
    );
    let wf = std::fs::read_to_string(copy.path().join("workflows/digest.yaml")).unwrap();
    write(
        copy.path(),
        "workflows/digest.yaml",
        &wf.replace("output_schema: summary", "output_schema: absent"),
    );
    let found = findings(copy.path());
    at(&found, "seats/beta.md", 5, "unknown team 'nowhere'");
    at(&found, "seats/beta.md", 11, "unknown account 'missing'");
    at(
        &found,
        "workflows/digest.yaml",
        23,
        "unknown schema 'absent'",
    );
}

#[test]
fn a_seat_without_frontmatter_is_an_error() {
    let copy = valid_copy();
    write(copy.path(), "seats/plain.md", "Just instructions.\n");
    let found = findings(copy.path());
    at(&found, "seats/plain.md", 1, "frontmatter");
}

#[test]
fn a_schema_that_is_not_json_schema_is_an_error() {
    let copy = valid_copy();
    write(
        copy.path(),
        "schemas/odd.json",
        "{\n  \"schema_version\": 1,\n  \"type\": \"thing\"\n}\n",
    );
    write(
        copy.path(),
        "schemas/torn.json",
        "{\n  \"schema_version\": 1,\n",
    );
    let found = findings(copy.path());
    at(&found, "schemas/odd.json", 3, "JSON Schema");
    assert!(
        found
            .iter()
            .any(|f| f.file == "schemas/torn.json" && f.severity == Severity::Error),
        "{found:#?}"
    );
}

#[test]
fn invalid_yaml_is_reported_with_its_line() {
    let copy = valid_copy();
    write(
        copy.path(),
        "dashboards/torn.yaml",
        "schema_version: 1\nname: [Torn\nteam: core\n",
    );
    let found = findings(copy.path());
    let f = found
        .iter()
        .find(|f| f.file == "dashboards/torn.yaml")
        .expect("a parse finding");
    assert_eq!(f.severity, Severity::Error);
    assert!(f.line >= 2, "{f:?}");
}

// Review fixes.

fn edit(root: &Path, rel: &str, from: &str, to: &str) {
    let text = std::fs::read_to_string(root.join(rel)).unwrap();
    assert!(text.contains(from), "{rel} has no `{from}`");
    write(root, rel, &text.replacen(from, to, 1));
}

#[test]
fn an_empty_required_value_is_reported_like_a_missing_one() {
    let copy = valid_copy();
    edit(
        copy.path(),
        "models/accounts.yaml",
        "provider: anthropic",
        "provider:",
    );
    edit(
        copy.path(),
        "tables/items.yaml",
        "  - id: title\n    type: text",
        "  - id: title\n    type:",
    );
    let found = findings(copy.path());
    at(&found, "models/accounts.yaml", 3, "missing 'provider'");
    at(&found, "tables/items.yaml", 9, "missing 'type'");
    // The account is still declared, so the seat that names it is fine.
    assert!(
        found.iter().all(|f| f.file != "seats/beta.md"),
        "{found:#?}"
    );
}

#[test]
fn one_local_error_does_not_cascade_into_unknown_references() {
    let copy = valid_copy();
    edit(
        copy.path(),
        "company.yaml",
        "timezone: UTC",
        "timezone: Mars/Base",
    );
    let found = findings(copy.path());
    let errs = errors(&found);
    assert_eq!(errs.len(), 1, "{found:#?}");
    assert_eq!((errs[0].file.as_str(), errs[0].line), ("company.yaml", 4));
    assert!(errs[0].message.contains("Mars/Base"));
}

#[test]
fn plain_scalars_keep_their_spelling() {
    let copy = valid_copy();
    edit(
        copy.path(),
        "company.yaml",
        "name: Example Project",
        "name: No",
    );
    edit(copy.path(), "tools.yaml", "version: 1.0.0", "version: 1.10");
    let loaded = load(copy.path()).unwrap();
    assert!(loaded.findings.is_empty(), "{:#?}", loaded.findings);
    let snap = loaded.snapshot.unwrap();
    assert_eq!(snap.company().name, "No");
    assert_eq!(snap.tools().mcp_servers[0].version.as_deref(), Some("1.10"));
}

#[test]
fn a_secret_in_a_block_scalar_is_reported_at_its_own_line() {
    let copy = valid_copy();
    edit(
        copy.path(),
        "workflows/digest.yaml",
        "    instruction: Write the summary.\n",
        "    instruction: |\n      Use sk-ant-canary0canary0canary here.\n      Then write the summary.\n",
    );
    let found = findings(copy.path());
    at(&found, "workflows/digest.yaml", 23, "secret-looking value");
    let secret_lines: Vec<u32> = found
        .iter()
        .filter(|f| f.message.contains("secret"))
        .map(|f| f.line)
        .collect();
    assert_eq!(secret_lines, vec![23], "{found:#?}");
}

#[test]
fn plain_http_is_only_for_this_mac_and_never_with_userinfo() {
    let copy = valid_copy();
    let text = std::fs::read_to_string(copy.path().join("models/accounts.yaml")).unwrap();
    write(
        copy.path(),
        "models/accounts.yaml",
        &format!(
            "{text}  - id: tricky\n    provider: openai-compatible\n    base_url: http://127.0.0.1:80@remote.example/\n\
             \x20 - id: lan\n    provider: openai-compatible\n    base_url: http://192.168.1.5:8080/v1\n\
             \x20 - id: creds\n    provider: openai-compatible\n    base_url: https://user:pw@example.org/v1\n\
             \x20 - id: box\n    provider: openai-compatible\n    base_url: http://localhost:1234/v1\n\
             \x20 - id: loop\n    provider: openai-compatible\n    base_url: http://127.0.0.1:1234/v1\n"
        ),
    );
    let found = findings(copy.path());
    for line in [9, 12, 15] {
        let f = at(&found, "models/accounts.yaml", line, "https URL");
        assert!(
            f.message.contains("localhost") && f.fix.contains("localhost"),
            "{f:?}"
        );
    }
    assert_eq!(errors(&found).len(), 3, "{found:#?}");
}

#[test]
fn reporting_and_team_loops_are_errors() {
    // A seat reporting to itself.
    let copy = valid_copy();
    edit(
        copy.path(),
        "seats/beta.md",
        "reports_to: alpha",
        "reports_to: beta",
    );
    let found = findings(copy.path());
    at(&found, "seats/beta.md", 6, "loop");
    // Two seats reporting to each other: one error, at the closing reference.
    let copy = valid_copy();
    edit(
        copy.path(),
        "seats/alpha.md",
        "team: core\n",
        "team: core\nreports_to: beta\n",
    );
    let found = findings(copy.path());
    let f = at(&found, "seats/beta.md", 6, "alpha → beta → alpha");
    assert!(f.fix.contains("reports_to"), "{}", f.fix);
    assert_eq!(errors(&found).len(), 1, "{found:#?}");
    // A team that is its own parent.
    let copy = valid_copy();
    edit(
        copy.path(),
        "company.yaml",
        "    name: Core team\n",
        "    name: Core team\n    parent: core\n",
    );
    let found = findings(copy.path());
    at(&found, "company.yaml", 11, "loop of teams");
    assert_eq!(errors(&found).len(), 1, "{found:#?}");
}

#[test]
fn repeated_grants_tool_settings_and_view_columns_are_errors() {
    let copy = valid_copy();
    edit(
        copy.path(),
        "tables/items.yaml",
        "grants:\n  - support\n",
        "grants:\n  - support\n  - support\n",
    );
    edit(
        copy.path(),
        "tables/items.yaml",
        "      - ref\n",
        "      - ref\n      - ref\n",
    );
    edit(
        copy.path(),
        "tools.yaml",
        "    max_uses_per_step: 5\n",
        "    max_uses_per_step: 5\n  - id: web_search\n",
    );
    let found = findings(copy.path());
    at(
        &found,
        "tables/items.yaml",
        26,
        "view column 'ref' appears twice",
    );
    at(
        &found,
        "tables/items.yaml",
        31,
        "grant 'support' appears twice",
    );
    at(
        &found,
        "tools.yaml",
        9,
        "built-in tool setting 'web_search' appears twice",
    );
}
