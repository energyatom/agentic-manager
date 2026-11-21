//! Story 1.14 acceptance tests (AD-48, AD-7, AD-16): the first shipped
//! config migration, from schema_version 1 to 2, sees the whole project. It
//! copies each `tools.yaml` `egress_hosts` entry that `HostPattern` accepts
//! into the `allowed_hosts` of every seat that can fetch (Web search, Browse
//! & read pages or Crawl on, or owning a Source of any type) and of no other
//! seat, merges without duplicates, drops and lists every refused entry,
//! removes the key and bumps every file. Matrix row "Migration", on a temporary
//! copy of `fixtures/projects/hosts-v1/` (schema_version 1:
//! `egress_hosts: [docs.example.org, 10.1.2.3]`, the Web search seat
//! `searcher`, the Source owner `owner`, the seat `quiet` that fetches
//! nothing). Written before the step exists.
//!
//! What these tests expect besides story 1.10's API: `MigrationOffer` gains
//! `notes: Vec<String>` (words per file, e.g. a dropped entry), offers stay
//! one per file and all or nothing, and `check_offer` re-derives the whole
//! project from the current bytes.
#![forbid(unsafe_code)]

mod common;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use am_config::{
    ConfigEvent, ConfigSession, ConfigWriter, MigrationOffer, SessionError, WriteError,
    accept_migrations, canonical_bytes, config_files, load, migration_offers,
};
use am_domain::{Actor, ConfigHistory, HistoryKind};
use am_testkit::InMemoryConfigHistory;
use common::{copy_of, edit, errors, hosts_of, warnings, write};

const TOOLS: &str = "tools.yaml";
const SEARCHER: &str = "seats/searcher.md";
const OWNER: &str = "seats/owner.md";
const QUIET: &str = "seats/quiet.md";
const SOURCE: &str = "sources/notes.yaml";

fn reviewer() -> Actor {
    Actor::Human {
        person: "owner".into(),
        hat: "Reviewer".into(),
    }
}

fn read(root: &Path, rel: &str) -> Vec<u8> {
    std::fs::read(root.join(rel)).unwrap()
}

fn text(root: &Path, rel: &str) -> String {
    String::from_utf8(read(root, rel)).unwrap()
}

fn all_bytes(root: &Path) -> BTreeMap<String, Vec<u8>> {
    config_files(root)
        .into_iter()
        .map(|f| {
            let b = read(root, &f);
            (f, b)
        })
        .collect()
}

fn v1() -> tempfile::TempDir {
    copy_of("hosts-v1")
}

fn offers(root: &Path) -> Vec<MigrationOffer> {
    migration_offers(root).expect("the project folder exists")
}

fn offer<'a>(offers: &'a [MigrationOffer], file: &str) -> &'a MigrationOffer {
    offers
        .iter()
        .find(|o| o.file == file)
        .unwrap_or_else(|| panic!("no offer for {file} in {offers:#?}"))
}

fn after(offers: &[MigrationOffer], file: &str) -> String {
    String::from_utf8(offer(offers, file).after.clone()).unwrap()
}

/// The `allowed_hosts` entries of a seat file's frontmatter, as written.
fn listed_hosts(seat: &str) -> Vec<String> {
    let front = seat
        .strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---\n"))
        .map(|(front, _)| front)
        .expect("a seat file");
    let mut out = Vec::new();
    let mut inside = false;
    for line in front.lines() {
        if line.starts_with("allowed_hosts:") {
            inside = true;
            continue;
        }
        if inside {
            match line.strip_prefix("  - ") {
                Some(item) => out.push(item.trim_matches(['"', '\'']).to_string()),
                None => break,
            }
        }
    }
    out
}

fn writer(root: &Path) -> (ConfigWriter, Arc<InMemoryConfigHistory>) {
    let history = Arc::new(InMemoryConfigHistory::default());
    let w = ConfigWriter::open(root, history.clone()).unwrap();
    (w, history)
}

// ------------------------------------------------------------- the offer

/// Every file is offered from 1 to 2 with its diff; nothing is written; the
/// offer notes list the dropped `10.1.2.3`.
#[test]
fn migrate_hosts_offer_steps_every_file_and_writes_nothing() {
    let dir = v1();
    let root = dir.path();
    let before = all_bytes(root);
    assert!(text(root, TOOLS).contains("egress_hosts"), "a v1 fixture");

    let offers = offers(root);

    let files: Vec<&str> = offers.iter().map(|o| o.file.as_str()).collect();
    let all: Vec<String> = config_files(root);
    assert_eq!(files, all, "one offer per config file, sorted");
    for o in &offers {
        assert_eq!((o.from, o.to), (1, 2), "{o:?}");
        assert!(o.diff.contains("\n-schema_version: 1\n"), "{}", o.diff);
        assert!(o.diff.contains("\n+schema_version: 2\n"), "{}", o.diff);
        assert_eq!(
            o.after,
            canonical_bytes(&o.file, &o.after).unwrap(),
            "{}",
            o.file
        );
        assert!(
            !String::from_utf8_lossy(&o.after).contains("10.1.2.3"),
            "{}",
            o.file
        );
    }
    assert_eq!(all_bytes(root), before, "offering writes nothing");

    // tools.yaml loses the key, and its notes say what was dropped.
    let tools = offer(&offers, TOOLS);
    assert!(!after(&offers, TOOLS).contains("egress_hosts"));
    assert!(tools.diff.contains("\n-egress_hosts:\n"), "{}", tools.diff);
    assert!(
        tools.notes.iter().any(|n| n.contains("10.1.2.3")),
        "{:?}",
        tools.notes
    );

    // The two seats that can fetch gain the valid host; the other does not.
    for seat in [SEARCHER, OWNER] {
        assert_eq!(
            listed_hosts(&after(&offers, seat)),
            ["docs.example.org"],
            "{seat}"
        );
        assert!(
            offer(&offers, seat).diff.contains("\n+allowed_hosts:\n"),
            "{}",
            offer(&offers, seat).diff
        );
    }
    assert!(!after(&offers, QUIET).contains("allowed_hosts"));
    // Offering again gives the same offers.
    assert_eq!(migration_offers(root).unwrap(), offers);
}

/// A pending migration builds no snapshot: the session names the offers.
#[test]
fn migrate_hosts_session_open_returns_migration_pending() {
    let dir = v1();
    let root = dir.path();
    let before = all_bytes(root);
    let history = Arc::new(InMemoryConfigHistory::default());
    let on_event: Arc<dyn Fn(ConfigEvent) + Send + Sync> = Arc::new(|_| {});

    let err = match ConfigSession::open(root, history.clone(), on_event) {
        Ok(_) => panic!("a session opened with a migration pending"),
        Err(e) => e,
    };

    match err {
        SessionError::MigrationPending { offers, .. } => {
            let files: Vec<String> = offers.iter().map(|o| o.file.clone()).collect();
            assert_eq!(files, config_files(root));
        }
        other => panic!("expected MigrationPending, got {other:?}"),
    }
    assert!(load(root).unwrap().snapshot.is_none());
    assert_eq!(all_bytes(root), before);
    assert!(history.entries(None).unwrap().is_empty());
}

// ------------------------------------------------------------- the accept

/// After the owner's accept: the fetching seats list the host, the other
/// none; `tools.yaml` holds no `egress_hosts`; every file is at 2; the
/// project loads clean and a session opens.
#[test]
fn migrate_hosts_accept_moves_the_hosts_into_the_fetching_seats_only() {
    let dir = v1();
    let root = dir.path();
    let offers = offers(root);
    let (w, history) = writer(root);

    let entries = accept_migrations(&w, reviewer(), offers.clone()).expect("the accept applies");

    assert_eq!(entries.len(), offers.len());
    assert!(entries.iter().all(|e| e.kind == HistoryKind::Migration));
    for o in &offers {
        assert_eq!(read(root, &o.file), o.after, "{}", o.file);
        assert!(
            text(root, &o.file).contains("schema_version: 2\n"),
            "{}",
            o.file
        );
    }
    assert!(!text(root, TOOLS).contains("egress_hosts"));
    assert_eq!(listed_hosts(&text(root, SEARCHER)), ["docs.example.org"]);
    assert_eq!(listed_hosts(&text(root, OWNER)), ["docs.example.org"]);
    assert!(!text(root, QUIET).contains("allowed_hosts"));

    let loaded = load(root).unwrap();
    assert!(
        errors(&loaded.findings).is_empty(),
        "{:#?}",
        loaded.findings
    );
    assert!(
        warnings(&loaded.findings).is_empty(),
        "{:#?}",
        loaded.findings
    );
    let snap = loaded.snapshot.expect("the migrated project loads");
    assert_eq!(hosts_of(&snap, "searcher"), ["docs.example.org"]);
    assert_eq!(hosts_of(&snap, "owner"), ["docs.example.org"]);
    assert!(hosts_of(&snap, "quiet").is_empty());
    assert!(migration_offers(root).unwrap().is_empty());

    let on_event: Arc<dyn Fn(ConfigEvent) + Send + Sync> = Arc::new(|_| {});
    let session = ConfigSession::open(root, history, on_event).expect("the session opens");
    assert_eq!(session.snapshot().hash(), snap.hash());
}

/// Browse & read pages and Crawl count as fetching; a capability switched
/// off does not.
#[test]
fn migrate_hosts_browse_and_crawl_seats_fetch_and_an_off_capability_does_not() {
    let dir = v1();
    let root = dir.path();
    let seat = |name: &str, cap: &str, on: bool| {
        format!(
            "---\nschema_version: 1\nname: {name}\nrole: Researcher\nteam: core\ncapabilities:\n  {cap}:\n    on: {on}\n---\nRead.\n"
        )
    };
    write(root, "seats/browser.md", &seat("Browser", "browse", true));
    write(root, "seats/crawler.md", &seat("Crawler", "crawl", true));
    write(root, "seats/off.md", &seat("Off", "web_search", false));

    let offers = offers(root);

    for file in ["seats/browser.md", "seats/crawler.md"] {
        assert_eq!(
            listed_hosts(&after(&offers, file)),
            ["docs.example.org"],
            "{file}"
        );
    }
    assert!(!after(&offers, "seats/off.md").contains("allowed_hosts"));
}

/// The step merges with the patterns a seat already lists, without
/// duplicates.
#[test]
fn migrate_hosts_merges_with_a_seats_own_list_without_duplicates() {
    let dir = v1();
    let root = dir.path();
    edit(
        root,
        SEARCHER,
        "    on: true\n---\n",
        "    on: true\nallowed_hosts:\n  - other.example.net\n  - docs.example.org\n---\n",
    );

    let offers = offers(root);

    let hosts = listed_hosts(&after(&offers, SEARCHER));
    assert_eq!(
        hosts.iter().filter(|h| *h == "docs.example.org").count(),
        1,
        "{hosts:?}"
    );
    assert!(
        hosts.contains(&"other.example.net".to_string()),
        "{hosts:?}"
    );
    assert_eq!(hosts.len(), 2, "{hosts:?}");
}

/// Every entry `HostPattern` refuses is dropped and listed; the valid ones
/// move.
#[test]
fn migrate_hosts_drops_and_lists_every_refused_entry() {
    let dir = v1();
    let root = dir.path();
    edit(
        root,
        TOOLS,
        "  - 10.1.2.3\n",
        "  - 10.1.2.3\n  - example.org:8080\n  - localhost\n  - \"*.example.net\"\n",
    );

    let offers = offers(root);

    let notes = offer(&offers, TOOLS).notes.join("\n");
    for dropped in ["10.1.2.3", "example.org:8080", "localhost"] {
        assert!(notes.contains(dropped), "{dropped}: {notes}");
    }
    assert!(!notes.contains("*.example.net"), "{notes}");
    for seat in [SEARCHER, OWNER] {
        let hosts = listed_hosts(&after(&offers, seat));
        assert_eq!(hosts, ["docs.example.org", "*.example.net"], "{seat}");
    }
}

/// A v1 `tools.yaml` without `egress_hosts` gives every file only the bump.
#[test]
fn migrate_hosts_without_egress_hosts_only_bumps() {
    let dir = v1();
    let root = dir.path();
    edit(
        root,
        TOOLS,
        "egress_hosts:\n  - docs.example.org\n  - 10.1.2.3\n",
        "",
    );

    let offers = offers(root);

    assert_eq!(offers.len(), config_files(root).len());
    for o in &offers {
        assert!(
            !String::from_utf8_lossy(&o.after).contains("allowed_hosts"),
            "{}",
            o.file
        );
        assert!(o.notes.is_empty(), "{}: {:?}", o.file, o.notes);
    }
}

// ------------------------------------------------------------- re-derive

/// The accept re-derives the whole project from the current bytes: an offer
/// whose bytes differ from the app's migration (here a host given to the
/// seat that fetches nothing, canonical and error-free) writes nothing.
#[test]
fn migrate_hosts_accept_refuses_an_offer_the_project_does_not_derive() {
    let dir = v1();
    let root = dir.path();
    let before = all_bytes(root);
    let mut offers = offers(root);
    let quiet = offers.iter_mut().find(|o| o.file == QUIET).unwrap();
    let tampered = String::from_utf8(quiet.after.clone()).unwrap().replacen(
        "team: core\n",
        "team: core\nallowed_hosts:\n  - docs.example.org\n",
        1,
    );
    quiet.after = canonical_bytes(QUIET, tampered.as_bytes()).unwrap();
    let (w, history) = writer(root);

    let err = accept_migrations(&w, reviewer(), offers).unwrap_err();

    assert!(
        err.to_string()
            .contains("does not match the app's migration"),
        "{err}"
    );
    assert!(err.to_string().contains(QUIET), "{err}");
    assert_eq!(all_bytes(root), before, "nothing written");
    assert!(history.entries(None).unwrap().is_empty());
    assert!(history.pending_intents().unwrap().is_empty());
}

/// A change to another file after the offer (the Source moves to the seat
/// that fetches nothing, which changes what that seat's offer must be)
/// refuses the whole accept before anything is written.
#[test]
fn migrate_hosts_accept_refuses_after_another_file_changed() {
    let dir = v1();
    let root = dir.path();
    let offers = offers(root);
    edit(root, SOURCE, "owner_seat: owner", "owner_seat: quiet");
    let before = all_bytes(root);
    let (w, history) = writer(root);

    let err = accept_migrations(&w, reviewer(), offers).unwrap_err();

    // Refused as the changed file, or as the offer the project no longer
    // derives, whichever check meets it first.
    match &err {
        WriteError::ChangedSince { file, .. } => assert_eq!(file, SOURCE),
        other => {
            let words = other.to_string();
            assert!(
                words.contains("does not match the app's migration") && words.contains(QUIET),
                "expected ChangedSince or a refused offer, got {other:?}"
            );
        }
    }
    assert_eq!(all_bytes(root), before, "nothing written");
    assert!(history.entries(None).unwrap().is_empty());
    assert!(history.pending_intents().unwrap().is_empty());
}

// ------------------------------------------------------------- review fixes

/// A fetching seat whose `allowed_hosts` is not a list blocks its own
/// migration (a step refusal naming the seat, the hosts and why), so the
/// all-or-nothing accept is refused with nothing written until the owner
/// fixes the key; `tools.yaml` never loses the hosts meanwhile.
#[test]
fn migrate_hosts_a_seat_whose_allowed_hosts_is_not_a_list_blocks_the_migration() {
    let dir = v1();
    let root = dir.path();
    edit(
        root,
        SEARCHER,
        "    on: true\n---\n",
        "    on: true\nallowed_hosts: other.example.net\n---\n",
    );
    let before = all_bytes(root);

    let offers = offers(root);

    assert!(
        offers.iter().all(|o| o.file != SEARCHER),
        "the seat is blocked, not offered: {offers:#?}"
    );
    // The load names why, on the seat's version finding.
    let found = load(root).unwrap().findings;
    let seat = found
        .iter()
        .find(|f| f.file == SEARCHER && f.message.contains("schema_version 1"))
        .unwrap_or_else(|| panic!("no version finding on {SEARCHER}: {found:#?}"));
    for words in ["searcher", "docs.example.org", "not a list"] {
        assert!(seat.fix.contains(words), "{words}: {}", seat.fix);
    }
    // The accept of every other offer is refused, naming the seat and why.
    let (w, history) = writer(root);
    let err = accept_migrations(&w, reviewer(), offers).unwrap_err();
    let words = err.to_string();
    assert!(words.contains(SEARCHER), "{words}");
    assert!(words.contains("not a list"), "{words}");
    assert_eq!(all_bytes(root), before, "nothing written");
    assert!(text(root, TOOLS).contains("egress_hosts"));
    assert!(history.entries(None).unwrap().is_empty());
    assert!(history.pending_intents().unwrap().is_empty());
}

/// Review fix: the stepped bytes depend on files that are not offered, so
/// a schema_version 2 Source whose `owner_seat` changes after the offer
/// refuses the accept as changed, naming that file, with nothing written.
#[test]
fn migrate_hosts_accept_refuses_when_a_file_that_is_not_offered_changed() {
    let dir = v1();
    let root = dir.path();
    let extra = "sources/extra.yaml";
    write(
        root,
        extra,
        "schema_version: 2\ntype: feed\nowner_seat: searcher\nscope:\n  seeds:\n    - https://docs.example.org/more.xml\noutput:\n  records: true\nterms_checked: 2026-11-01\n",
    );
    let offers = offers(root);
    assert!(
        offers.iter().all(|o| o.file != extra),
        "a current file is not offered"
    );
    edit(root, extra, "owner_seat: searcher", "owner_seat: quiet");
    let before = all_bytes(root);
    let (w, history) = writer(root);

    let err = accept_migrations(&w, reviewer(), offers).unwrap_err();

    match &err {
        WriteError::ChangedSince { file, .. } => assert_eq!(file, extra),
        other => panic!("expected ChangedSince naming {extra}, got {other:?}"),
    }
    assert_eq!(all_bytes(root), before, "nothing written");
    assert!(history.entries(None).unwrap().is_empty());
    assert!(history.pending_intents().unwrap().is_empty());
}

/// Review fix: a schema_version 1 `tools.yaml` waiting for its migration
/// shows its version finding only, no `egress_hosts` error beside it.
#[test]
fn migrate_hosts_a_v1_tools_yaml_has_no_egress_hosts_error() {
    let dir = v1();
    let found = load(dir.path()).unwrap().findings;
    let tools: Vec<_> = found.iter().filter(|f| f.file == TOOLS).collect();
    assert!(
        tools.iter().any(|f| f.message.contains("schema_version 1")),
        "{tools:#?}"
    );
    assert!(
        tools.iter().all(|f| !f.message.contains("egress_hosts")),
        "{tools:#?}"
    );
}
