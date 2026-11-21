//! Story 1.10 acceptance tests (R4, AD-7, AD-16): config files at an older
//! `schema_version` are offered as a diff and written only on the owner's
//! accept, through the writer, as one `migration` History entry each. Written
//! before the migration registry exists.
//!
//! Story 1.14 moves `SCHEMA_VERSION` to 2 and ships the first step (1→2,
//! the whole-project `egress_hosts` move, tested in `migrate_hosts.rs`);
//! this crate's tests build with `test-support`, which also registers a step
//! from 0 to 1 that renames a seat's v0-only `group:` key to `team:` and
//! bumps the version (files without that key get only the bump), so a v0
//! file is offered from 0 to 2. Every v0 project is a temporary copy of
//! `fixtures/projects/valid` with files edited to `schema_version: 0`.
//!
//! The API these tests expect (plan Code Map):
//! - `MigrationOffer { file, from, to, before_sha256, diff, after }`, public
//!   fields, `Debug + Clone`;
//! - `migration_offers(&Path) -> Result<Vec<MigrationOffer>, LoadError>`,
//!   one offer per older file, sorted by file;
//! - `accept_migrations(&ConfigWriter, Actor, Vec<MigrationOffer>) ->
//!   Result<Vec<HistoryEntry>, WriteError>`, one entry per offer, in order;
//! - `WriteError::NothingToMigrate`, `SessionError::MigrationPending { offers, .. }`
//!   and `HistoryKind::Migration`.
#![forbid(unsafe_code)]

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};

use am_config::{
    CompanyRefusal, ConfigEvent, ConfigSession, ConfigWriter, FailPoint, MigrationOffer,
    SessionError, Severity, WriteError, accept_migrations, canonical_bytes, config_files, load,
    migration_offers, read_company,
};
use am_domain::{
    Actor, ConfigHistory, HistoryEntry, HistoryError, HistoryKind, NewIntent, PendingIntent,
};
use am_testkit::InMemoryConfigHistory;
use common::{copy_of, edit, errors, in_file};
use sha2::{Digest, Sha256};

const SEAT: &str = "seats/alpha.md";
const WORKFLOW: &str = "workflows/digest.yaml";
/// This app's `schema_version` (story 1.14).
const CURRENT: i64 = 2;
/// A body whose bytes (trailing spaces, a tab, a `---` line, no final
/// newline) must survive the migration.
const BODY: &str =
    "Read the items added since the last run.  \n\tKeep the list short.\n---\nno final newline";

fn reviewer() -> Actor {
    Actor::Human {
        person: "owner".into(),
        hat: "Reviewer".into(),
    }
}

fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn read(root: &Path, rel: &str) -> Vec<u8> {
    std::fs::read(root.join(rel)).unwrap()
}

/// Every config file's bytes, to prove that nothing was written.
fn all_bytes(root: &Path) -> BTreeMap<String, Vec<u8>> {
    config_files(root)
        .into_iter()
        .map(|f| {
            let b = read(root, &f);
            (f, b)
        })
        .collect()
}

/// The first `schema_version:` line of `text`.
fn version_line(text: &str) -> String {
    text.lines()
        .find(|l| l.trim_start().starts_with("schema_version:"))
        .expect("a schema_version line")
        .to_string()
}

/// Sets `rel`'s `schema_version` line, whatever it holds now, to `version`.
fn at_version(root: &Path, rel: &str, version: i64) {
    let text = String::from_utf8(read(root, rel)).unwrap();
    edit(
        root,
        rel,
        &version_line(&text),
        &format!("schema_version: {version}"),
    );
}

/// A copy of the valid fixture with the seat and the workflow at v0.
fn v0_project() -> tempfile::TempDir {
    let dir = copy_of("valid");
    at_version(dir.path(), SEAT, 0);
    at_version(dir.path(), WORKFLOW, 0);
    dir
}

fn writer(root: &Path) -> (ConfigWriter, Arc<InMemoryConfigHistory>) {
    let history = Arc::new(InMemoryConfigHistory::default());
    let w = ConfigWriter::open(root, history.clone()).unwrap();
    (w, history)
}

fn offers(root: &Path) -> Vec<MigrationOffer> {
    migration_offers(root).expect("the project folder exists")
}

fn open_session(
    root: &Path,
    history: Arc<InMemoryConfigHistory>,
) -> Result<ConfigSession, SessionError> {
    let on_event: Arc<dyn Fn(ConfigEvent) + Send + Sync> = Arc::new(|_| {});
    ConfigSession::open(root, history, on_event)
}

/// The bytes the canonical emitter gives `rel` with its version set to 2:
/// what the 0→1 test step and the 1→2 step must produce for a file with no
/// v0-only `group:` key (the first renames only that key, which these files
/// do not hold; the second moves `tools.yaml` `egress_hosts`, which the
/// current `tools.yaml` no longer holds).
fn stepped(root: &Path, rel: &str) -> Vec<u8> {
    let text = String::from_utf8(read(root, rel)).unwrap();
    assert!(text.contains("schema_version: 0"), "{rel} is not at v0");
    let bumped = text.replacen(
        "schema_version: 0",
        &format!("schema_version: {CURRENT}"),
        1,
    );
    canonical_bytes(rel, bumped.as_bytes()).unwrap()
}

/// The part of a seat file after its frontmatter's closing `---` line.
fn body_of(text: &str) -> &str {
    let rest = text.strip_prefix("---\n").expect("a seat file");
    let end = rest.find("\n---\n").expect("a closing `---` line");
    &rest[end + "\n---\n".len()..]
}

// ------------------------------------------------------------- matrix rows

/// Row "Offer": both older files are offered from 0 to 2 with a unified diff
/// and the canonical stepped bytes; nothing on disk changes.
#[test]
fn migrate_offer_lists_every_older_file_with_its_diff_and_writes_nothing() {
    let dir = v0_project();
    let root = dir.path();
    let before = all_bytes(root);

    let offers = offers(root);

    let files: Vec<&str> = offers.iter().map(|o| o.file.as_str()).collect();
    assert_eq!(files, [SEAT, WORKFLOW], "one offer per older file, sorted");
    for o in &offers {
        assert_eq!((o.from, o.to), (0, CURRENT), "{o:?}");
        assert_eq!(o.before_sha256, sha(&before[&o.file]), "{o:?}");
        assert_eq!(o.after, stepped(root, &o.file), "{}", o.file);
        assert!(
            o.diff
                .starts_with(&format!("--- a/{}\n+++ b/{}\n", o.file, o.file)),
            "{}",
            o.diff
        );
        assert!(o.diff.contains("\n-schema_version: 0\n"), "{}", o.diff);
        assert!(o.diff.contains("\n+schema_version: 2\n"), "{}", o.diff);
    }
    assert_eq!(all_bytes(root), before, "offering writes nothing");
    // Offering again gives the same offers.
    let again: Vec<(String, String)> = migration_offers(root)
        .unwrap()
        .into_iter()
        .map(|o| (o.file, o.before_sha256))
        .collect();
    let first: Vec<(String, String)> = offers
        .into_iter()
        .map(|o| (o.file, o.before_sha256))
        .collect();
    assert_eq!(again, first);
}

/// Row "Session refuses": a pending migration builds no snapshot; the
/// session names the offers instead of `Invalid`.
#[test]
fn migrate_session_open_refuses_with_migration_pending() {
    let dir = v0_project();
    let root = dir.path();
    let before = all_bytes(root);
    let history = Arc::new(InMemoryConfigHistory::default());

    let err = match open_session(root, history.clone()) {
        Ok(_) => panic!("a session opened with a migration pending"),
        Err(e) => e,
    };

    match err {
        SessionError::MigrationPending { offers, .. } => {
            let files: Vec<&str> = offers.iter().map(|o| o.file.as_str()).collect();
            assert_eq!(files, [SEAT, WORKFLOW]);
            assert!(offers.iter().all(|o| (o.from, o.to) == (0, CURRENT)));
        }
        other => panic!("expected MigrationPending, got {other:?}"),
    }
    assert!(load(root).unwrap().snapshot.is_none(), "no snapshot");
    assert_eq!(all_bytes(root), before);
    assert!(history.entries(None).unwrap().is_empty());
    assert!(history.pending_intents().unwrap().is_empty());
}

/// Row "Accept": each file is written at schema_version 2 as one `migration`
/// entry; the project then loads with no errors and a session opens.
#[test]
fn migrate_accept_writes_every_file_with_one_migration_entry_each() {
    let dir = v0_project();
    let root = dir.path();
    let old: BTreeMap<String, Vec<u8>> = [SEAT, WORKFLOW]
        .iter()
        .map(|f| (f.to_string(), read(root, f)))
        .collect();
    let offers = offers(root);
    let (w, history) = writer(root);

    let entries = accept_migrations(&w, reviewer(), offers.clone()).expect("the accept applies");

    assert_eq!(entries.len(), 2);
    for (entry, offer) in entries.iter().zip(&offers) {
        assert_eq!(entry.file, offer.file);
        assert_eq!(entry.kind, HistoryKind::Migration);
        assert_eq!(entry.ref_entry, None);
        assert_eq!(entry.who(), "You (Reviewer)");
        assert_eq!(entry.before, old[&offer.file]);
        assert_eq!(entry.before_sha256, offer.before_sha256);
        assert_eq!(entry.after, offer.after);
        assert_eq!(entry.after_sha256, sha(&offer.after));
        assert_eq!(entry.diff, offer.diff);
        assert_eq!(read(root, &offer.file), offer.after, "{}", offer.file);
        let text = String::from_utf8(read(root, &offer.file)).unwrap();
        assert!(text.contains("schema_version: 2\n"), "{text}");
        assert!(!text.contains("schema_version: 0"), "{text}");
    }
    assert_eq!(history.entries(None).unwrap(), entries);
    assert!(history.pending_intents().unwrap().is_empty());

    let loaded = load(root).unwrap();
    assert!(
        errors(&loaded.findings).is_empty(),
        "{:#?}",
        loaded.findings
    );
    assert!(loaded.snapshot.is_some());
    assert!(
        migration_offers(root).unwrap().is_empty(),
        "nothing is left to migrate"
    );
    let session = open_session(root, history.clone()).expect("the session opens after accept");
    assert_eq!(
        session.snapshot().hash(),
        loaded.snapshot.unwrap().hash(),
        "the session's snapshot is the migrated project"
    );
}

/// Row "Body kept": a seat file's Markdown body is byte-identical after the
/// migration.
#[test]
fn migrate_keeps_a_seat_body_byte_for_byte() {
    let dir = copy_of("valid");
    let root = dir.path();
    let text = String::from_utf8(read(root, SEAT)).unwrap();
    let head = &text[..text.len() - body_of(&text).len()];
    let seat = format!(
        "{}{BODY}",
        head.replacen(&version_line(head), "schema_version: 0", 1)
    );
    std::fs::write(root.join(SEAT), &seat).unwrap();
    let offers = offers(root);
    assert_eq!(offers.len(), 1, "{offers:?}");
    let (w, _history) = writer(root);

    accept_migrations(&w, reviewer(), offers).expect("the accept applies");

    let after = String::from_utf8(read(root, SEAT)).unwrap();
    assert_eq!(body_of(&after), BODY, "the body is kept byte for byte");
    assert!(after.ends_with("no final newline"), "{after:?}");
    assert!(after.contains("schema_version: 2\n"), "{after}");
}

/// Row "Invalid after migrate": the `test-support` step renames a v0 seat's
/// v0-only `group:` key to `team:`, the seat's team reference. A v0 seat
/// whose `group:` names a team that does not exist gets, once stepped, an
/// unknown-team error the v0 project does not have; the offer is not
/// tampered with. Accept re-validates the migrated project (only added
/// errors refuse, as for an edit): nothing is written, nothing reaches
/// History.
#[test]
fn migrate_accept_that_adds_an_error_writes_nothing() {
    let dir = v0_project();
    let root = dir.path();
    edit(root, SEAT, "team: core", "group: no-such-team");
    let before = all_bytes(root);
    let offers = offers(root);
    let seat = offers
        .iter()
        .find(|o| o.file == SEAT)
        .expect("the seat is offered");
    let text = String::from_utf8(seat.after.clone()).unwrap();
    assert!(text.contains("team: no-such-team"), "{text}");
    assert!(!text.contains("group:"), "{text}");
    let (w, history) = writer(root);

    let err = accept_migrations(&w, reviewer(), offers).unwrap_err();

    match &err {
        WriteError::Invalid { findings, .. } => {
            assert!(
                findings
                    .iter()
                    .any(|f| f.file == SEAT && f.message.contains("no-such-team")),
                "{findings:#?}"
            );
            assert!(findings.iter().all(|f| f.severity == Severity::Error));
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
    assert!(err.to_string().contains("no-such-team"), "{err}");
    assert_eq!(all_bytes(root), before, "nothing written");
    assert!(history.entries(None).unwrap().is_empty());
    assert!(history.pending_intents().unwrap().is_empty());
}

/// Review fix: accept writes only the app's own migration of each file. An
/// offer whose `after` was changed (here a canonical, error-free rename of
/// the seat) is refused before anything is written.
#[test]
fn migrate_accept_refuses_a_tampered_offer_and_writes_nothing() {
    let dir = v0_project();
    let root = dir.path();
    let before = all_bytes(root);
    let mut offers = offers(root);
    let seat = offers.iter_mut().find(|o| o.file == SEAT).unwrap();
    let text = String::from_utf8(seat.after.clone()).unwrap();
    assert!(text.contains("name: Alpha\n"), "{text}");
    seat.after = text
        .replacen("name: Alpha\n", "name: Alpha two\n", 1)
        .into_bytes();
    assert_eq!(
        canonical_bytes(SEAT, &seat.after).unwrap(),
        seat.after,
        "the tampered bytes are canonical"
    );
    let (w, history) = writer(root);

    let err = accept_migrations(&w, reviewer(), offers).unwrap_err();

    assert!(
        err.to_string()
            .contains("does not match the app's migration"),
        "{err}"
    );
    assert!(err.to_string().contains(SEAT), "{err}");
    assert_eq!(all_bytes(root), before, "nothing written");
    assert!(history.entries(None).unwrap().is_empty());
    assert!(history.pending_intents().unwrap().is_empty());
}

/// Follow-up review: an offer whose `diff` alone was altered (so the owner
/// would review a change that is not the one written) is refused too.
#[test]
fn migrate_accept_refuses_an_offer_whose_diff_was_altered() {
    let dir = v0_project();
    let root = dir.path();
    let before = all_bytes(root);
    let mut offers = offers(root);
    let seat = offers.iter_mut().find(|o| o.file == SEAT).unwrap();
    assert!(seat.diff.contains("+schema_version: 2\n"), "{}", seat.diff);
    seat.diff = seat.diff.replacen(
        "+schema_version: 2\n",
        "+schema_version: 2\n+note: none\n",
        1,
    );
    let (w, history) = writer(root);

    let err = accept_migrations(&w, reviewer(), offers).unwrap_err();

    assert!(
        err.to_string()
            .contains("does not match the app's migration"),
        "{err}"
    );
    assert!(err.to_string().contains(SEAT), "{err}");
    assert_eq!(all_bytes(root), before, "nothing written");
    assert!(history.entries(None).unwrap().is_empty());
    assert!(history.pending_intents().unwrap().is_empty());
}

/// Row "No path": a file older than the first registered step is not
/// offered and stays a load error whose fix says the app cannot migrate it.
#[test]
fn migrate_file_with_no_path_is_not_offered_and_says_it_cannot_be_migrated() {
    let dir = copy_of("valid");
    let root = dir.path();
    at_version(root, SEAT, -1);

    assert!(offers(root).is_empty(), "no step from -1");

    let found = load(root).unwrap().findings;
    let seat = errors(&found)
        .into_iter()
        .find(|f| f.file == SEAT && f.message.contains("schema_version -1"))
        .unwrap_or_else(|| panic!("no schema_version error in {found:#?}"));
    assert!(seat.fix.contains("cannot migrate"), "{seat:#?}");
}

/// The older file with a path is still a load error until accepted, worded
/// as offered (not "cannot migrate").
#[test]
fn migrate_older_file_with_a_path_is_a_load_error_worded_as_offered() {
    let dir = v0_project();
    let root = dir.path();

    let found = load(root).unwrap().findings;
    for file in [SEAT, WORKFLOW] {
        let e = in_file(&found, file)
            .into_iter()
            .find(|f| f.severity == Severity::Error && f.message.contains("schema_version 0"))
            .unwrap_or_else(|| panic!("no schema_version error for {file} in {found:#?}"));
        assert!(e.fix.contains("migrat"), "{e:#?}");
        assert!(!e.fix.contains("cannot migrate"), "{e:#?}");
    }
}

/// Row "Newer": a file at `SCHEMA_VERSION + 1` (3 since story 1.14) is not
/// offered and is refused as today.
#[test]
fn migrate_newer_file_is_not_offered_and_still_refused() {
    let dir = copy_of("valid");
    let root = dir.path();
    at_version(root, SEAT, CURRENT + 1);

    assert!(offers(root).is_empty(), "a newer file is never offered");
    let found = load(root).unwrap().findings;
    let seat = errors(&found)
        .into_iter()
        .find(|f| f.file == SEAT && f.message.contains("schema_version 3"))
        .unwrap_or_else(|| panic!("no schema_version error in {found:#?}"));
    assert!(
        format!("{} {}", seat.message, seat.fix).contains("update the app"),
        "{seat:#?}"
    );
    assert!(!seat.fix.contains("cannot migrate"), "{seat:#?}");

    // A newer company.yaml is refused before the project opens.
    let company = copy_of("valid");
    at_version(company.path(), "company.yaml", CURRENT + 1);
    assert!(
        matches!(
            read_company(company.path()),
            Err(CompanyRefusal::TooNew { found: 3, known: 2 })
        ),
        "{:?}",
        read_company(company.path())
    );
    let offered = migration_offers(company.path()).unwrap_or_default();
    assert!(offered.is_empty(), "{offered:?}");
}

/// Row "Changed since offer": a file edited on disk after the offer refuses
/// the whole accept before any file is written (the changed file is the
/// second one, so a per-file write would already have written the first).
#[test]
fn migrate_accept_refuses_a_file_changed_since_the_offer_and_writes_nothing() {
    let dir = v0_project();
    let root = dir.path();
    let offers = offers(root);
    edit(
        root,
        WORKFLOW,
        "name: Daily digest",
        "name: Daily digest two",
    );
    let before = all_bytes(root);
    let (w, history) = writer(root);

    let err = accept_migrations(&w, reviewer(), offers).unwrap_err();

    match &err {
        WriteError::ChangedSince { file, .. } => assert_eq!(file, WORKFLOW),
        other => panic!("expected ChangedSince, got {other:?}"),
    }
    assert_eq!(all_bytes(root), before, "nothing written");
    assert!(history.entries(None).unwrap().is_empty());
    assert!(history.pending_intents().unwrap().is_empty());
}

/// Row "Nothing to migrate": a current project has no offers and an accept
/// is refused.
#[test]
fn migrate_current_project_offers_nothing_and_accept_refuses() {
    let dir = copy_of("valid");
    let root = dir.path();
    let before = all_bytes(root);

    assert!(offers(root).is_empty());
    let (w, history) = writer(root);
    let err = accept_migrations(&w, reviewer(), Vec::new()).unwrap_err();

    assert!(
        matches!(err, WriteError::NothingToMigrate { .. }),
        "{err:?}"
    );
    assert_eq!(all_bytes(root), before);
    assert!(history.entries(None).unwrap().is_empty());
}

// ------------------------------------------------------------- review fixes

/// Review fix (AD-7): a migration is written only on the owner's accept, so
/// any Actor but a Human is refused before anything is checked or written.
#[test]
fn migrate_accept_by_an_actor_other_than_the_owner_is_refused_and_writes_nothing() {
    let dir = v0_project();
    let root = dir.path();
    let before = all_bytes(root);
    let offers = offers(root);
    assert_eq!(offers.len(), 2);
    let (w, history) = writer(root);

    for actor in [
        Actor::System {
            component: "config-writer".into(),
        },
        Actor::Import {
            source: "a backup".into(),
        },
    ] {
        let err = accept_migrations(&w, actor.clone(), offers.clone())
            .expect_err("only the owner accepts a migration");
        assert!(err.to_string().contains("only the owner"), "{err}");
        // Refused before anything is checked: even an empty accept.
        let err = accept_migrations(&w, actor, Vec::new()).unwrap_err();
        assert!(err.to_string().contains("only the owner"), "{err}");
    }

    assert_eq!(all_bytes(root), before, "nothing written");
    assert!(history.entries(None).unwrap().is_empty());
    assert!(history.pending_intents().unwrap().is_empty());
}

/// Review fix: a file whose bytes hold a secret-looking value is not offered
/// (the value would reach the diff and the stepped bytes); its load finding
/// says to remove the value by hand first, and then it is offered.
#[test]
fn migrate_file_holding_a_secret_is_not_offered_and_the_value_is_never_shown() {
    const KEY: &str = "sk-ant-canary0canary0canary";
    let dir = v0_project();
    let root = dir.path();
    let seat = String::from_utf8(read(root, SEAT)).unwrap();
    std::fs::write(root.join(SEAT), format!("{seat}\n{KEY}\n")).unwrap();

    let offered = offers(root);
    let files: Vec<&str> = offered.iter().map(|o| o.file.as_str()).collect();
    assert_eq!(files, [WORKFLOW], "the seat holding a key is not offered");
    assert!(!format!("{offered:?}").contains(KEY), "{offered:?}");

    let history = Arc::new(InMemoryConfigHistory::default());
    let err = match open_session(root, history) {
        Ok(_) => panic!("a session opened with a migration pending"),
        Err(e) => e,
    };
    assert!(!format!("{err:?}").contains(KEY), "{err:?}");
    assert!(!err.to_string().contains(KEY), "{err}");
    match err {
        SessionError::MigrationPending { offers, .. } => {
            let files: Vec<&str> = offers.iter().map(|o| o.file.as_str()).collect();
            assert_eq!(files, [WORKFLOW]);
        }
        other => panic!("expected MigrationPending, got {other:?}"),
    }

    let found = load(root).unwrap().findings;
    let e = in_file(&found, SEAT)
        .into_iter()
        .find(|f| f.severity == Severity::Error && f.message.contains("schema_version 0"))
        .unwrap_or_else(|| panic!("no schema_version error for {SEAT} in {found:#?}"));
    assert!(e.fix.contains("by hand"), "{e:#?}");
    assert!(e.fix.contains("secret-looking"), "{e:#?}");
    assert!(!e.fix.contains(KEY), "{e:#?}");

    // Once the value is removed by hand, the migration is offered.
    std::fs::write(root.join(SEAT), &seat).unwrap();
    let files: Vec<String> = offers(root).into_iter().map(|o| o.file).collect();
    assert_eq!(files, [SEAT, WORKFLOW]);
}

// ------------------------------------------------------- follow-up review

/// Follow-up review (high): an older file that has a step path but is not
/// offered (here a seat holding a secret-looking value) keeps the accept of
/// the other offers from writing anything: the project is never half
/// migrated. The refusal names the file and why.
#[test]
fn migrate_accept_refuses_when_an_older_file_is_not_offered_and_writes_nothing() {
    const KEY: &str = "sk-ant-canary0canary0canary";
    let dir = v0_project();
    let root = dir.path();
    let seat = String::from_utf8(read(root, SEAT)).unwrap();
    std::fs::write(root.join(SEAT), format!("{seat}\n{KEY}\n")).unwrap();
    let before = all_bytes(root);
    let offers = offers(root);
    let files: Vec<&str> = offers.iter().map(|o| o.file.as_str()).collect();
    assert_eq!(files, [WORKFLOW]);
    let (w, history) = writer(root);

    let err = accept_migrations(&w, reviewer(), offers).unwrap_err();

    let words = err.to_string();
    assert!(words.contains(SEAT), "{words}");
    assert!(words.contains("secret-looking"), "{words}");
    assert!(words.contains("ask for the offers again"), "{words}");
    assert!(!words.contains(KEY), "{words}");
    assert_eq!(all_bytes(root), before, "nothing written");
    assert!(history.entries(None).unwrap().is_empty());
    assert!(history.pending_intents().unwrap().is_empty());
}

/// Follow-up review: a file whose step fails (a seat with both `group:` and
/// `team:`) is not offered; its load finding gives the step's reason and
/// says to fix it by hand (no "offers"), and an empty accept is refused
/// naming it rather than claiming nothing is older.
#[test]
fn migrate_file_whose_step_fails_states_the_reason_and_an_empty_accept_names_it() {
    let dir = v0_project();
    let root = dir.path();
    edit(root, SEAT, "team: core", "team: core\ngroup: core");
    let before = all_bytes(root);

    let files: Vec<String> = offers(root).into_iter().map(|o| o.file).collect();
    assert_eq!(files, [WORKFLOW], "the seat is not offered");

    let found = load(root).unwrap().findings;
    let e = in_file(&found, SEAT)
        .into_iter()
        .find(|f| f.severity == Severity::Error && f.message.contains("schema_version 0"))
        .unwrap_or_else(|| panic!("no schema_version error for {SEAT} in {found:#?}"));
    assert!(e.fix.contains("both `group` and `team`"), "{e:#?}");
    assert!(e.fix.contains("by hand"), "{e:#?}");
    assert!(!e.fix.contains("offers"), "{e:#?}");

    let (w, history) = writer(root);
    let err = accept_migrations(&w, reviewer(), Vec::new()).unwrap_err();
    assert!(
        !matches!(err, WriteError::NothingToMigrate { .. }),
        "{err:?}"
    );
    let words = err.to_string();
    assert!(words.contains(SEAT), "{words}");
    assert!(words.contains("both `group` and `team`"), "{words}");
    assert_eq!(all_bytes(root), before, "nothing written");
    assert!(history.entries(None).unwrap().is_empty());
}

/// A `ConfigHistory` that edits a file on disk right after the History
/// entry of another file is committed: an owner saving in an editor while a
/// multi-file accept runs, at a fixed point.
struct EditAfterCommit {
    inner: Arc<InMemoryConfigHistory>,
    root: PathBuf,
    after: &'static str,
    file: &'static str,
    from: &'static str,
    to: &'static str,
}

impl ConfigHistory for EditAfterCommit {
    fn begin_intent(&self, actor: Actor, intent: NewIntent) -> Result<String, HistoryError> {
        self.inner.begin_intent(actor, intent)
    }
    fn commit_entry(&self, actor: Actor, intent_id: String) -> Result<HistoryEntry, HistoryError> {
        let entry = self.inner.commit_entry(actor, intent_id)?;
        if entry.file == self.after {
            edit(&self.root, self.file, self.from, self.to);
        }
        Ok(entry)
    }
    fn drop_intent(&self, actor: Actor, intent_id: String) -> Result<(), HistoryError> {
        self.inner.drop_intent(actor, intent_id)
    }
    fn reconcile_intent(
        &self,
        actor: Actor,
        intent_id: String,
    ) -> Result<HistoryEntry, HistoryError> {
        self.inner.reconcile_intent(actor, intent_id)
    }
    fn pending_intents(&self) -> Result<Vec<PendingIntent>, HistoryError> {
        self.inner.pending_intents()
    }
    fn entries(&self, file: Option<String>) -> Result<Vec<HistoryEntry>, HistoryError> {
        self.inner.entries(file)
    }
    fn entry(&self, id: String) -> Result<Option<HistoryEntry>, HistoryError> {
        self.inner.entry(id)
    }
}

/// Follow-up review: a file edited on disk while the accept runs (after
/// the first file is written, before the second) is not overwritten: the
/// accept stops mid-way at it (its cause is `ChangedSince`), the refusal
/// names the file already migrated, the edit stays, the first file keeps
/// its entry and no intent is left.
#[test]
fn migrate_accept_stops_mid_way_at_a_file_edited_during_the_accept() {
    let dir = v0_project();
    let root = dir.path();
    let offers = offers(root);
    let inner = Arc::new(InMemoryConfigHistory::default());
    let history = Arc::new(EditAfterCommit {
        inner: inner.clone(),
        root: root.to_path_buf(),
        after: SEAT,
        file: WORKFLOW,
        from: "name: Daily digest",
        to: "name: Daily digest edited",
    });
    let w = ConfigWriter::open(root, history).unwrap();

    let err = accept_migrations(&w, reviewer(), offers.clone()).unwrap_err();

    let debug = format!("{err:?}");
    assert!(
        debug.contains(&format!("ChangedSince {{ file: \"{WORKFLOW}\"")),
        "{debug}"
    );
    let words = err.to_string();
    assert!(words.contains(&format!("stopped at {WORKFLOW}")), "{words}");
    assert!(
        words.contains(&format!("already migrated: {SEAT}")),
        "{words}"
    );
    let text = String::from_utf8(read(root, WORKFLOW)).unwrap();
    assert!(text.contains("name: Daily digest edited"), "{text}");
    assert!(text.contains("schema_version: 0"), "{text}");
    let seat = offers.iter().find(|o| o.file == SEAT).unwrap();
    assert_eq!(read(root, SEAT), seat.after, "the first file stays written");
    let entries = inner.entries(None).unwrap();
    let files: Vec<&str> = entries.iter().map(|e| e.file.as_str()).collect();
    assert_eq!(files, [SEAT]);
    assert!(inner.pending_intents().unwrap().is_empty());
}

/// Follow-up review: the happy path of the test step's rename. A v0 seat
/// holding only `group: core` is accepted and written with `team: core`.
#[test]
fn migrate_accept_renames_a_v0_group_to_team() {
    let dir = v0_project();
    let root = dir.path();
    edit(root, SEAT, "team: core", "group: core");
    let offers = offers(root);
    assert_eq!(offers.len(), 2);
    let (w, _history) = writer(root);

    accept_migrations(&w, reviewer(), offers).expect("the accept applies");

    let text = String::from_utf8(read(root, SEAT)).unwrap();
    assert!(text.contains("\nteam: core\n"), "{text}");
    assert!(!text.contains("group:"), "{text}");
    assert!(text.contains("schema_version: 2\n"), "{text}");
    let loaded = load(root).unwrap();
    assert!(
        errors(&loaded.findings).is_empty(),
        "{:#?}",
        loaded.findings
    );
}

// --------------------------------------------------- narrow review (links)

/// Creates `root/rel` as a link to a file outside the project holding
/// `bytes`; returns the folder that holds the target.
#[cfg(unix)]
fn link_outside(root: &Path, rel: &str, bytes: &[u8]) -> (tempfile::TempDir, PathBuf) {
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join(Path::new(rel).file_name().unwrap());
    std::fs::write(&target, bytes).unwrap();
    let _ = std::fs::remove_file(root.join(rel));
    std::os::unix::fs::symlink(&target, root.join(rel)).unwrap();
    (outside, target)
}

/// Narrow review (high): a link inside `seats/` is skipped by the loader,
/// so it is not a config file for migration either: it is not offered, and
/// the accept of the offers agrees and applies (never a `BadPath`), writing
/// nothing through the link.
#[cfg(unix)]
#[test]
fn migrate_linked_seat_in_a_folder_is_not_offered_and_the_accept_agrees() {
    let dir = v0_project();
    let root = dir.path();
    let seat_v0 = read(root, SEAT);
    let (_outside, target) = link_outside(root, "seats/gamma.md", &seat_v0);

    let offers = offers(root);
    let files: Vec<&str> = offers.iter().map(|o| o.file.as_str()).collect();
    assert_eq!(files, [SEAT, WORKFLOW], "the linked seat is not offered");
    let (w, _history) = writer(root);
    let entries = accept_migrations(&w, reviewer(), offers).expect("the accept applies");

    assert_eq!(entries.len(), 2);
    let meta = std::fs::symlink_metadata(root.join("seats/gamma.md")).unwrap();
    assert!(meta.file_type().is_symlink(), "the link stays a link");
    assert_eq!(
        std::fs::read(&target).unwrap(),
        seat_v0,
        "nothing written through it"
    );
}

/// Narrow review (high): a linked top-level file the loader reads (here
/// `company.yaml` at v0) is not offered; its finding gives the link reason,
/// and an accept is refused naming it, in words (never a `BadPath`).
#[cfg(unix)]
#[test]
fn migrate_linked_company_yaml_is_not_offered_and_an_accept_names_it() {
    let dir = copy_of("valid");
    let root = dir.path();
    at_version(root, WORKFLOW, 0);
    let company = String::from_utf8(read(root, "company.yaml")).unwrap();
    let company_v0 = company.replacen(&version_line(&company), "schema_version: 0", 1);
    let (_outside, target) = link_outside(root, "company.yaml", company_v0.as_bytes());
    let before = all_bytes(root);

    let offers = offers(root);
    let files: Vec<&str> = offers.iter().map(|o| o.file.as_str()).collect();
    assert_eq!(files, [WORKFLOW], "the linked company.yaml is not offered");

    let found = load(root).unwrap().findings;
    let e = in_file(&found, "company.yaml")
        .into_iter()
        .find(|f| f.severity == Severity::Error && f.message.contains("schema_version 0"))
        .unwrap_or_else(|| panic!("no schema_version error in {found:#?}"));
    assert!(e.fix.contains("is a link"), "{e:#?}");
    assert!(!e.fix.contains("accept the migration"), "{e:#?}");

    let (w, history) = writer(root);
    for offered in [offers.clone(), Vec::new()] {
        let err = accept_migrations(&w, reviewer(), offered).unwrap_err();
        assert!(!matches!(err, WriteError::BadPath { .. }), "{err:?}");
        let words = err.to_string();
        assert!(words.contains("company.yaml is a link"), "{words}");
    }
    assert_eq!(all_bytes(root), before, "nothing written");
    assert_eq!(std::fs::read(&target).unwrap(), company_v0.as_bytes());
    assert!(history.entries(None).unwrap().is_empty());
    assert!(history.pending_intents().unwrap().is_empty());
}

/// Narrow review: an edit made after the offer and before the accept
/// (here to the first offered file) is refused before any write, and the
/// refusal says nothing about files already migrated.
#[test]
fn migrate_accept_refuses_an_edit_made_after_the_offer_and_migrates_nothing() {
    let dir = v0_project();
    let root = dir.path();
    let offers = offers(root);
    edit(root, SEAT, "name: Alpha", "name: Alpha two");
    let before = all_bytes(root);
    let (w, history) = writer(root);

    let err = accept_migrations(&w, reviewer(), offers).unwrap_err();

    match &err {
        WriteError::ChangedSince { file, .. } => assert_eq!(file, SEAT),
        other => panic!("expected ChangedSince, got {other:?}"),
    }
    let words = err.to_string();
    assert!(!words.contains("already migrated"), "{words}");
    assert!(!words.contains("History entry"), "{words}");
    assert_eq!(all_bytes(root), before, "nothing written");
    assert!(history.entries(None).unwrap().is_empty());
    assert!(history.pending_intents().unwrap().is_empty());
}

/// Narrow review: while an older file is blocked (here a seat holding a
/// secret-looking value), the session's refusal says what blocks the
/// migration instead of asking the owner to accept it, and never shows the
/// value.
#[test]
fn migrate_session_names_a_blocked_file_instead_of_asking_to_accept() {
    const KEY: &str = "sk-ant-canary0canary0canary";
    let dir = v0_project();
    let root = dir.path();
    let seat = String::from_utf8(read(root, SEAT)).unwrap();
    std::fs::write(root.join(SEAT), format!("{seat}\n{KEY}\n")).unwrap();

    let history = Arc::new(InMemoryConfigHistory::default());
    let err = match open_session(root, history) {
        Ok(_) => panic!("a session opened with a migration pending"),
        Err(e) => e,
    };

    let words = err.to_string();
    assert!(!words.contains("accept the migration"), "{words}");
    assert!(words.contains(SEAT), "{words}");
    assert!(words.contains("secret-looking"), "{words}");
    assert!(!words.contains(KEY), "{words}");
    assert!(!format!("{err:?}").contains(KEY), "{err:?}");
}

/// Narrow review: an accept given only one of two offers is refused before
/// any write, naming the file left out. (Passes with the guard as built.)
#[test]
fn migrate_accept_of_only_some_offers_is_refused_naming_the_rest() {
    let dir = v0_project();
    let root = dir.path();
    let before = all_bytes(root);
    let only_seat: Vec<MigrationOffer> = offers(root)
        .into_iter()
        .filter(|o| o.file == SEAT)
        .collect();
    assert_eq!(only_seat.len(), 1);
    let (w, history) = writer(root);

    let err = accept_migrations(&w, reviewer(), only_seat).unwrap_err();

    let words = err.to_string();
    assert!(words.contains(WORKFLOW), "{words}");
    assert!(words.contains("not in this accept"), "{words}");
    assert_eq!(all_bytes(root), before, "nothing written");
    assert!(history.entries(None).unwrap().is_empty());
    assert!(history.pending_intents().unwrap().is_empty());
}

/// Narrow review: the same offer passed twice is refused before any write.
/// (Passes with the guard as built.)
#[test]
fn migrate_accept_of_the_same_offer_twice_is_refused() {
    let dir = v0_project();
    let root = dir.path();
    let before = all_bytes(root);
    let mut twice = offers(root);
    let seat = twice.iter().find(|o| o.file == SEAT).unwrap().clone();
    twice.push(seat);
    let (w, history) = writer(root);

    let err = accept_migrations(&w, reviewer(), twice).unwrap_err();

    let words = err.to_string();
    assert!(
        words.contains(&format!("{SEAT} is offered twice")),
        "{words}"
    );
    assert_eq!(all_bytes(root), before, "nothing written");
    assert!(history.entries(None).unwrap().is_empty());
    assert!(history.pending_intents().unwrap().is_empty());
}

// ------------------------------------------------------ last review round

/// A `ConfigHistory` that sets the writer's after-rename failpoint once the
/// History entry of `after` is committed, so the next file's write stops
/// after its rename (a crash before its entry is recorded).
struct FailAfterCommit {
    inner: Arc<InMemoryConfigHistory>,
    after: &'static str,
    writer: Mutex<Weak<ConfigWriter>>,
}

impl ConfigHistory for FailAfterCommit {
    fn begin_intent(&self, actor: Actor, intent: NewIntent) -> Result<String, HistoryError> {
        self.inner.begin_intent(actor, intent)
    }
    fn commit_entry(&self, actor: Actor, intent_id: String) -> Result<HistoryEntry, HistoryError> {
        let entry = self.inner.commit_entry(actor, intent_id)?;
        if entry.file == self.after
            && let Some(w) = self.writer.lock().unwrap().upgrade()
        {
            w.fail_at(Some(FailPoint::AfterRename));
        }
        Ok(entry)
    }
    fn drop_intent(&self, actor: Actor, intent_id: String) -> Result<(), HistoryError> {
        self.inner.drop_intent(actor, intent_id)
    }
    fn reconcile_intent(
        &self,
        actor: Actor,
        intent_id: String,
    ) -> Result<HistoryEntry, HistoryError> {
        self.inner.reconcile_intent(actor, intent_id)
    }
    fn pending_intents(&self) -> Result<Vec<PendingIntent>, HistoryError> {
        self.inner.pending_intents()
    }
    fn entries(&self, file: Option<String>) -> Result<Vec<HistoryEntry>, HistoryError> {
        self.inner.entries(file)
    }
    fn entry(&self, id: String) -> Result<Option<HistoryEntry>, HistoryError> {
        self.inner.entry(id)
    }
}

fn system() -> Actor {
    Actor::System {
        component: "config-writer".into(),
    }
}

/// Last round (medium): a failure after the first file's rename counts that
/// file as migrated, its History entry recorded at the next start; the
/// refusal says so, and `reconcile` then records the `migration` entry.
#[test]
fn migrate_failure_after_the_first_rename_counts_that_file_as_migrated() {
    let dir = v0_project();
    let root = dir.path();
    let offers = offers(root);
    let (w, history) = writer(root);
    w.fail_at(Some(FailPoint::AfterRename));

    let err = accept_migrations(&w, reviewer(), offers.clone()).unwrap_err();

    let words = err.to_string();
    assert!(words.contains(&format!("migrated: {SEAT}")), "{words}");
    assert!(words.contains("recorded at the next start"), "{words}");
    let seat = offers.iter().find(|o| o.file == SEAT).unwrap();
    assert_eq!(
        read(root, SEAT),
        seat.after,
        "the seat holds its migrated bytes"
    );
    assert!(
        String::from_utf8(read(root, WORKFLOW))
            .unwrap()
            .contains("schema_version: 0")
    );
    assert!(history.entries(None).unwrap().is_empty());
    let pending: Vec<String> = history
        .pending_intents()
        .unwrap()
        .into_iter()
        .map(|p| p.intent.file)
        .collect();
    assert_eq!(pending, [SEAT]);

    w.fail_at(None);
    let report = w.reconcile(system()).unwrap();
    assert_eq!(report.completed.len(), 1);
    assert_eq!(report.completed[0].file, SEAT);
    assert_eq!(report.completed[0].kind, HistoryKind::Migration);
}

/// Last round (medium): a failure after the second file's rename counts
/// both files as migrated (the first with its entry, the second with its
/// entry recorded at the next start).
#[test]
fn migrate_failure_after_the_second_rename_counts_both_files_as_migrated() {
    let dir = v0_project();
    let root = dir.path();
    let offers = offers(root);
    let inner = Arc::new(InMemoryConfigHistory::default());
    let history = Arc::new(FailAfterCommit {
        inner: inner.clone(),
        after: SEAT,
        writer: Mutex::new(Weak::new()),
    });
    let w = Arc::new(ConfigWriter::open(root, history.clone()).unwrap());
    *history.writer.lock().unwrap() = Arc::downgrade(&w);

    let err = accept_migrations(&w, reviewer(), offers.clone()).unwrap_err();

    let words = err.to_string();
    assert!(
        words.contains(&format!("migrated: {SEAT}, {WORKFLOW}")),
        "{words}"
    );
    assert!(words.contains("recorded at the next start"), "{words}");
    for o in &offers {
        assert_eq!(read(root, &o.file), o.after, "{}", o.file);
    }
    let entries: Vec<String> = inner
        .entries(None)
        .unwrap()
        .into_iter()
        .map(|e| e.file)
        .collect();
    assert_eq!(entries, [SEAT]);
    let pending: Vec<String> = inner
        .pending_intents()
        .unwrap()
        .into_iter()
        .map(|p| p.intent.file)
        .collect();
    assert_eq!(pending, [WORKFLOW]);

    w.fail_at(None);
    let report = w.reconcile(system()).unwrap();
    assert_eq!(report.completed.len(), 1);
    assert_eq!(report.completed[0].file, WORKFLOW);
    assert_eq!(report.completed[0].kind, HistoryKind::Migration);
}

/// Last round (low): when a parent folder is the link (a linked `seats/`
/// holding a v0 seat), the finding and the refusal name that folder and say
/// to replace the link with the folder itself.
#[cfg(unix)]
#[test]
fn migrate_seat_under_a_linked_folder_names_the_folder() {
    let dir = v0_project();
    let root = dir.path();
    let outside = tempfile::tempdir().unwrap();
    let real = outside.path().join("seats");
    std::fs::rename(root.join("seats"), &real).unwrap();
    std::os::unix::fs::symlink(&real, root.join("seats")).unwrap();
    let before = all_bytes(root);

    let offers = offers(root);
    let files: Vec<&str> = offers.iter().map(|o| o.file.as_str()).collect();
    assert_eq!(
        files,
        [WORKFLOW],
        "the seat under the linked folder is not offered"
    );

    let found = load(root).unwrap().findings;
    let e = in_file(&found, SEAT)
        .into_iter()
        .find(|f| f.severity == Severity::Error && f.message.contains("schema_version 0"))
        .unwrap_or_else(|| panic!("no schema_version error for {SEAT} in {found:#?}"));
    assert!(e.fix.contains("seats/ is a link"), "{e:#?}");
    assert!(e.fix.contains("folder itself"), "{e:#?}");

    let (w, history) = writer(root);
    let err = accept_migrations(&w, reviewer(), offers).unwrap_err();
    let words = err.to_string();
    assert!(words.contains("seats/ is a link"), "{words}");
    assert!(words.contains("folder itself"), "{words}");
    assert!(!words.contains(&format!("{SEAT} is a link")), "{words}");
    assert_eq!(all_bytes(root), before, "nothing written");
    assert!(history.entries(None).unwrap().is_empty());
}

/// Last round (low): the session also names older files the app cannot
/// migrate (no step path), which stay errors after the accept.
#[test]
fn migrate_session_names_files_that_cannot_be_migrated() {
    let dir = v0_project();
    let root = dir.path();
    at_version(root, "tables/items.yaml", -1);

    let history = Arc::new(InMemoryConfigHistory::default());
    let err = match open_session(root, history) {
        Ok(_) => panic!("a session opened with a migration pending"),
        Err(e) => e,
    };

    match &err {
        SessionError::MigrationPending { offers, .. } => {
            let files: Vec<&str> = offers.iter().map(|o| o.file.as_str()).collect();
            assert_eq!(files, [SEAT, WORKFLOW]);
        }
        other => panic!("expected MigrationPending, got {other:?}"),
    }
    let words = err.to_string();
    assert!(words.contains("cannot be migrated"), "{words}");
    assert!(
        words.contains("tables/items.yaml (schema_version -1)"),
        "{words}"
    );
}

// ------------------------------------------------------------- acceptance

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The folders of a parent folder, sorted.
fn folders(parent: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(parent)
        .unwrap_or_else(|e| panic!("{}: {e}", parent.display()))
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .collect();
    out.sort();
    out
}

/// Every file under `dir`, recursively.
fn files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let path = e.unwrap().path();
        if path.is_dir() {
            files_under(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// The fixture kept at schema_version 1 for the 1→2 step's tests
/// (`migrate_hosts.rs`); every other shipped project is current.
const OLD_FIXTURE: &str = "hosts-v1";

/// Acceptance 2 (story 1.14: version 2): the shipped templates and fixtures
/// offer nothing, and every `schema_version` in them is 2, except the one
/// fixture kept at version 1 on purpose.
#[test]
fn migrate_shipped_templates_and_fixtures_offer_nothing() {
    let old = repo().join("fixtures/projects").join(OLD_FIXTURE);
    assert!(old.is_dir(), "{}", old.display());
    let mut roots: Vec<PathBuf> = folders(&repo().join("fixtures/projects"))
        .into_iter()
        .filter(|p| *p != old)
        .collect();
    for group in folders(&repo().join("templates")) {
        roots.extend(folders(&group));
    }
    assert!(roots.len() > 3, "{roots:?}");
    for root in &roots {
        let offered = migration_offers(root).unwrap();
        assert!(offered.is_empty(), "{}: {offered:?}", root.display());
    }

    let mut files = Vec::new();
    files_under(&repo().join("templates"), &mut files);
    files_under(&repo().join("fixtures/projects"), &mut files);
    for path in files.into_iter().filter(|p| !p.starts_with(&old)) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for line in text.lines() {
            let t = line.trim().trim_start_matches('"');
            if let Some(rest) = t.strip_prefix("schema_version") {
                let value = rest
                    .trim_start_matches('"')
                    .trim_start_matches([':', ' '])
                    .trim_end_matches(',');
                assert_eq!(value.trim(), "2", "{}: {line}", path.display());
            }
        }
    }
}
