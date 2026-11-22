//! Story 1.12 acceptance tests: the Source file schema (AD-39) and the
//! crawl-service Account's base_url.
#![forbid(unsafe_code)]

mod common;

use am_config::{Severity, SourceOutput, Trigger};
use common::*;

// The valid Source set.

#[test]
fn a_valid_source_set_reads_into_typed_sources() {
    let loaded = am_config::load(&fixture("sources-ok")).unwrap();
    assert!(
        errors(&loaded.findings).is_empty(),
        "{:#?}",
        loaded.findings
    );
    let snap = loaded.snapshot.unwrap();
    let docs = &snap.sources()["docs"];
    assert_eq!(docs.id, "docs");
    assert_eq!(docs.name.as_deref(), Some("Documentation pages"));
    assert_eq!(docs.source_type, "crawler");
    assert_eq!(docs.owner_seat, "owner");
    assert_eq!(docs.account.as_deref(), Some("crawly"));
    assert_eq!(docs.scope.seeds, ["https://example.org/docs/"]);
    assert_eq!(docs.scope.include, ["/docs/**"]);
    assert_eq!(docs.scope.exclude, ["/docs/old/**"]);
    assert_eq!(docs.scope.allowed_hosts, ["example.org"]);
    assert_eq!(docs.scope.max_depth, Some(2));
    assert!(matches!(docs.trigger, Some(Trigger::Schedule { .. })));
    assert_eq!(docs.limits.max_items_per_load, Some(100));
    assert_eq!(docs.limits.max_bytes_per_load, Some(10_000_000));
    assert_eq!(docs.limits.max_loads_per_day, Some(4));
    assert_eq!(docs.limits.max_spend_per_load, Some(500_000));
    assert_eq!(docs.limits.rate_per_host_ms, Some(2_000));
    match &docs.output {
        SourceOutput::Table {
            table,
            key,
            mapping,
        } => {
            assert_eq!(table, "pages");
            assert_eq!(key, "ref");
            assert_eq!(mapping.get("title").map(String::as_str), Some("title"));
            assert_eq!(mapping.len(), 3);
        }
        other => panic!("{other:?}"),
    }
    assert!(docs.render);
    assert_eq!(docs.terms_checked.as_deref(), Some("2026-11-01"));
    assert!(matches!(
        snap.sources()["site"].output,
        SourceOutput::Knowledge { .. }
    ));
    assert_eq!(snap.sources()["search"].output, SourceOutput::Records);
    assert!(docs.text.contains("type: crawler"), "the raw text is kept");
}

// Row: website Source.

#[test]
fn an_empty_terms_checked_and_an_unlisted_host_are_two_warnings() {
    let loaded = am_config::load(&fixture("sources-ok")).unwrap();
    assert_eq!(
        warnings(&loaded.findings).len(),
        2,
        "{:#?}",
        loaded.findings
    );
    let f = at(&loaded.findings, "sources/site.yaml", 8, "terms_checked");
    assert_eq!(f.severity, Severity::Warning);
    let f = at(
        &loaded.findings,
        "sources/site.yaml",
        5,
        "elsewhere.example.net",
    );
    assert_eq!(f.severity, Severity::Warning);
}

#[test]
fn a_missing_terms_checked_on_a_website_source_is_a_warning() {
    let copy = copy_of("sources-ok");
    edit(
        copy.path(),
        "sources/docs.yaml",
        "terms_checked: 2026-11-01\n",
        "",
    );
    let found = findings(copy.path());
    let f = at(&found, "sources/docs.yaml", 1, "terms_checked");
    assert_eq!(f.severity, Severity::Warning);
    // A date that is not a date is an error.
    let copy = copy_of("sources-ok");
    edit(
        copy.path(),
        "sources/docs.yaml",
        "terms_checked: 2026-11-01",
        "terms_checked: soon",
    );
    let f = at(&findings(copy.path()), "sources/docs.yaml", 33, "soon").severity;
    assert_eq!(f, Severity::Error);
}

// Row: broken crawler Source.

#[test]
fn a_broken_crawler_source_gives_three_errors() {
    let found = findings(&fixture("sources-broken"));
    for (line, message) in [
        (6, "ftp://example.org/files/"),
        (8, "/docs/[a"),
        (10, "unknown table 'ghost'"),
    ] {
        let f = at(&found, "sources/crawl.yaml", line, message);
        assert_eq!(f.severity, Severity::Error, "{line}");
        assert!(!f.fix.is_empty());
    }
    assert_eq!(
        errors(
            &in_file(&found, "sources/crawl.yaml")
                .into_iter()
                .cloned()
                .collect::<Vec<_>>()
        )
        .len(),
        3,
        "{found:#?}"
    );
}

// Row: render and base URL.

#[test]
fn render_on_a_feed_and_a_remote_plain_http_base_url_are_errors() {
    let found = findings(&fixture("sources-broken"));
    let f = at(&found, "sources/news.yaml", 9, "render");
    assert_eq!(f.severity, Severity::Error);
    assert!(f.fix.contains("http-reader"), "{}", f.fix);
    // Story 2.12: the crawl Account's plain-http `base_url` moved with the
    // Accounts file to `fixtures/accounts/broken.yaml` (user_accounts.rs).
    assert_eq!(errors(&found).len(), 4, "{found:#?}");
}

// Variants.

#[test]
fn type_owner_and_account_are_checked() {
    let copy = copy_of("sources-ok");
    edit(
        copy.path(),
        "sources/site.yaml",
        "type: http-reader",
        "type: scraper",
    );
    edit(
        copy.path(),
        "sources/search.yaml",
        "owner_seat: owner",
        "owner_seat: nobody",
    );
    edit(
        copy.path(),
        "sources/docs.yaml",
        "account: crawly",
        "account: finder",
    );
    // Story 2.12: the Account's kind is known only from the user list.
    let found = findings_with(copy.path(), "sources");
    let f = at(&found, "sources/site.yaml", 2, "scraper");
    assert!(f.fix.contains("crawler"), "{}", f.fix);
    at(&found, "sources/search.yaml", 3, "unknown seat 'nobody'");
    let f = at(&found, "sources/docs.yaml", 5, "finder");
    assert!(f.message.contains("search"), "{}", f.message);
    assert!(f.fix.contains("crawl-service"), "{}", f.fix);
}

#[test]
fn a_feed_takes_no_account() {
    let copy = copy_of("sources-ok");
    write(
        copy.path(),
        "sources/news.yaml",
        "schema_version: 1\ntype: feed\nowner_seat: owner\naccount: crawly\noutput:\n  records: true\nterms_checked: 2026-11-01\n",
    );
    let found = findings(copy.path());
    at(&found, "sources/news.yaml", 4, "account");
}

#[test]
fn a_crawler_needs_its_owner_to_have_crawl_on() {
    let copy = copy_of("sources-ok");
    edit(
        copy.path(),
        "seats/owner.md",
        "  crawl:\n    on: true\n",
        "",
    );
    let found = findings(copy.path());
    let f = at(&found, "sources/docs.yaml", 4, "crawl");
    assert!(f.fix.contains("switch on crawl"), "{}", f.fix);
}

#[test]
fn limits_are_within_their_caps() {
    let copy = copy_of("sources-ok");
    edit(
        copy.path(),
        "sources/docs.yaml",
        "  max_items_per_load: 100\n  max_bytes_per_load: 10000000\n  max_loads_per_day: 4\n  max_spend_per_load: 0.5\n  rate_per_host: 2\n",
        "  max_items_per_load: 10001\n  max_bytes_per_load: 1000000001\n  max_loads_per_day: 97\n  max_spend_per_load: -1\n  rate_per_host: 0.5\n",
    );
    edit(
        copy.path(),
        "sources/docs.yaml",
        "max_depth: 2",
        "max_depth: 11",
    );
    let found = findings(copy.path());
    for (line, key) in [
        (15, "max_depth"),
        (20, "max_items_per_load"),
        (21, "max_bytes_per_load"),
        (22, "max_loads_per_day"),
        (23, "max_spend_per_load"),
        (24, "rate_per_host"),
    ] {
        at(&found, "sources/docs.yaml", line, key);
    }
}

#[test]
fn output_has_exactly_one_form_and_its_columns_exist() {
    let copy = copy_of("sources-ok");
    edit(
        copy.path(),
        "sources/search.yaml",
        "  records: true\n",
        "  records: true\n  knowledge: team\n",
    );
    edit(
        copy.path(),
        "sources/docs.yaml",
        "    title: title\n",
        "    headline: title\n",
    );
    edit(
        copy.path(),
        "sources/docs.yaml",
        "  key: ref\n",
        "  key: nope\n",
    );
    edit(
        copy.path(),
        "sources/site.yaml",
        "knowledge: seat",
        "knowledge: everyone",
    );
    let found = findings(copy.path());
    at(&found, "sources/search.yaml", 5, "exactly one");
    at(&found, "sources/docs.yaml", 27, "unknown column 'nope'");
    at(&found, "sources/docs.yaml", 30, "unknown column 'headline'");
    at(&found, "sources/site.yaml", 7, "everyone");
}

#[test]
fn a_source_trigger_uses_the_workflow_trigger_reader() {
    let copy = copy_of("sources-ok");
    edit(
        copy.path(),
        "sources/docs.yaml",
        "cron: \"0 6 * * 1\"",
        "cron: \"61 * * * *\"",
    );
    let found = findings(copy.path());
    at(&found, "sources/docs.yaml", 18, "invalid cron");
}

#[test]
fn render_is_allowed_on_http_reader_and_crawler() {
    let copy = copy_of("sources-ok");
    edit(
        copy.path(),
        "sources/site.yaml",
        "terms_checked: \"\"\n",
        "render: true\nterms_checked: 2026-11-01\n",
    );
    assert!(errors(&findings(copy.path())).is_empty());
}

// Story 2.12: `searxng_remote_on_this_mac_is_a_valid_search_account` moved to
// user_accounts.rs with the Accounts file (AD-17).
