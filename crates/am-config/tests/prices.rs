//! Epic 3 story 7, "Cost settles per call and rolls up" (AD-9, AD-16;
//! owner Q6, Q10): `models/prices.yaml` gains `rates:` (company-currency
//! units per USD, dated by the file's `updated:`), the loaded prices carry
//! the file's sha256 (the run's rate-snapshot id), a file that prices a
//! model with no `updated:` gets a warning, and the two shipped price files
//! parse with their date.
//!
//! The API pinned here (tester, 2026-11-25): `am_config::parse_prices(text)
//! -> (Prices, Vec<Finding>)` (findings named `models/prices.yaml`);
//! `Prices { rates: Vec<CurrencyRate>, sha256: Option<String>, .. }`;
//! `CurrencyRate { currency, micros_per_usd }`.
//!
//! Prices and rates in the inline texts are fake test values; the shipped
//! files' numbers are the owner's to check at review, so no test reads them.
//! Written before `rates` exists (tests first, 2026-11-25).
#![forbid(unsafe_code)]

mod common;

use std::path::{Path, PathBuf};

use am_config::{CurrencyRate, Finding, LoadOptions, Prices, Severity, parse_prices};
use common::{copy_of, edit, findings, snapshot, write};
use sha2::{Digest, Sha256};

const FILE: &str = "models/prices.yaml";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn errors(found: &[Finding]) -> Vec<&Finding> {
    found
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .collect()
}

fn warnings(found: &[Finding]) -> Vec<&Finding> {
    found
        .iter()
        .filter(|f| f.severity == Severity::Warning)
        .collect()
}

/// The 1-based line of `text` holding `needle`.
fn line_of(text: &str, needle: &str) -> u32 {
    text.lines()
        .position(|l| l.contains(needle))
        .map(|i| i as u32 + 1)
        .unwrap_or_else(|| panic!("no line holds `{needle}` in:\n{text}"))
}

/// A price file with one fake model price and the given `rates:` block.
fn with_rates(rates: &str) -> String {
    format!(
        "schema_version: 2\nupdated: 2026-11-01\nmodels:\n  - provider: anthropic\n    model: example-large\n    input_usd_per_mtok: 3.0\n    output_usd_per_mtok: 15.0\nrates:\n{rates}"
    )
}

/// `am_config::parse_prices`, typed (findings named `models/prices.yaml`).
fn parsed(text: &str) -> (Prices, Vec<Finding>) {
    parse_prices(text)
}

fn rate(currency: &str, micros_per_usd: i64) -> CurrencyRate {
    CurrencyRate {
        currency: currency.into(),
        micros_per_usd,
    }
}

/// Row 27 (owner Q6): `rates:` is a list of `currency` (three capitals)
/// and `per_usd` (company units per USD, stored as micro-units, rounded to
/// the nearest one as every amount is); the costs fixture holds one.
#[test]
fn rates_parse_with_currency_and_per_usd() {
    let text = with_rates(
        "  - currency: EUR\n    per_usd: 0.92\n  - currency: INR\n    per_usd: 83.5\n  - currency: JPY\n    per_usd: 150\n  - currency: KWD\n    per_usd: 0.3071234\n",
    );
    let (prices, found) = parsed(&text);
    assert!(found.is_empty(), "{found:#?}");
    assert_eq!(
        prices.rates,
        vec![
            rate("EUR", 920_000),
            rate("INR", 83_500_000),
            rate("JPY", 150_000_000),
            rate("KWD", 307_123),
        ]
    );
    assert_eq!(prices.updated.as_deref(), Some("2026-11-01"));
    assert_eq!(prices.models.len(), 1);

    // A file with no `rates:` has none (a USD company needs none).
    let (bare, found) = parsed(
        "schema_version: 2\nupdated: 2026-11-01\nmodels:\n  - provider: anthropic\n    model: example-large\n    input_usd_per_mtok: 3.0\n    output_usd_per_mtok: 15.0\n",
    );
    assert!(found.is_empty(), "{found:#?}");
    assert!(bare.rates.is_empty());

    // The costs fixture (EUR): one rate, no finding.
    let loaded = findings(&common::fixture("costs"));
    assert!(loaded.is_empty(), "{loaded:#?}");
    let snap = snapshot(&common::fixture("costs"));
    assert_eq!(snap.prices().rates, vec![rate("EUR", 920_000)]);
    assert_eq!(snap.prices().updated.as_deref(), Some("2026-11-01"));
    assert_eq!(snap.company().currency, "EUR");
}

/// Row 27: each bad `rates` entry is an Error at its own line, with a fix,
/// and is not loaded: a code that is not three capitals, a `per_usd` that
/// is missing, zero, negative or not a number, a currency listed twice (at
/// the second), a USD entry (prices are USD already) and an unknown key.
#[test]
fn a_bad_rate_is_an_error_with_line_and_fix() {
    let cases: [(&str, &str, &str); 9] = [
        (
            "lower case",
            "  - currency: eur\n    per_usd: 0.92\n",
            "currency: eur",
        ),
        (
            "four letters",
            "  - currency: EURO\n    per_usd: 0.92\n",
            "currency: EURO",
        ),
        ("no per_usd", "  - currency: GBP\n", "currency: GBP"),
        ("zero", "  - currency: GBP\n    per_usd: 0\n", "per_usd: 0"),
        (
            "negative",
            "  - currency: GBP\n    per_usd: -0.79\n",
            "per_usd: -0.79",
        ),
        (
            "not a number",
            "  - currency: GBP\n    per_usd: lots\n",
            "per_usd: lots",
        ),
        (
            "twice",
            "  - currency: GBP\n    per_usd: 0.79\n  - currency: GBP # again\n    per_usd: 0.8\n",
            "currency: GBP # again",
        ),
        (
            "USD",
            "  - currency: USD\n    per_usd: 1\n",
            "currency: USD",
        ),
        (
            "unknown key",
            "  - currency: GBP\n    per_usd: 0.79\n    as_of: 2026-11-01\n",
            "as_of:",
        ),
    ];
    for (case, rates, needle) in cases {
        let text = with_rates(rates);
        let (prices, found) = parsed(&text);
        let line = line_of(&text, needle);
        let errs = errors(&found);
        assert!(
            errs.iter().any(|f| f.file == FILE && f.line == line),
            "{case}: an error at {FILE}:{line} ({needle}): {found:#?}"
        );
        for f in &errs {
            assert!(!f.message.trim().is_empty(), "{case}: {f:#?}");
            assert!(!f.fix.trim().is_empty(), "{case}: a fix: {f:#?}");
        }
        match case {
            // The first GBP stays; only the repeat is refused.
            "twice" => assert_eq!(prices.rates, vec![rate("GBP", 790_000)], "{case}"),
            // An unknown key is refused; the entry itself is well formed.
            "unknown key" => {}
            _ => assert!(prices.rates.is_empty(), "{case}: {:?}", prices.rates),
        }
    }

    // In a project, the error stops the snapshot (no run is built from it).
    let dir = copy_of("costs");
    edit(dir.path(), FILE, "per_usd: 0.92", "per_usd: 0");
    let loaded = am_config::load(dir.path()).unwrap();
    assert!(loaded.snapshot.is_none(), "{:#?}", loaded.findings);
    let text = std::fs::read_to_string(dir.path().join(FILE)).unwrap();
    assert!(
        errors(&loaded.findings)
            .iter()
            .any(|f| f.file == FILE && f.line == line_of(&text, "per_usd: 0")),
        "{:#?}",
        loaded.findings
    );
}

/// Review round 1, M4 (AD-9, AD-16): a (provider, model) priced twice is an
/// Error at the second entry (its `- provider:` or `model:` line), with a
/// fix; the first price is the one loaded, and in a project the error stops
/// the snapshot. The same model name under another provider is another
/// price (the control).
#[test]
fn a_model_priced_twice_is_an_error_at_the_second_entry() {
    let text = "schema_version: 2\nupdated: 2026-11-01\nmodels:\n  - provider: anthropic\n    model: example-large\n    input_usd_per_mtok: 3.0\n    output_usd_per_mtok: 15.0\n  - provider: openai\n    model: example-large\n    input_usd_per_mtok: 2.0\n    output_usd_per_mtok: 8.0\n  - provider: anthropic # again\n    model: example-large # again\n    input_usd_per_mtok: 0.5\n    output_usd_per_mtok: 1.0\n";
    let (prices, found) = parsed(text);
    let lines = [
        line_of(text, "provider: anthropic # again"),
        line_of(text, "model: example-large # again"),
    ];
    let errs = errors(&found);
    assert_eq!(errs.len(), 1, "one error, for the repeat: {found:#?}");
    assert_eq!(errs[0].file, FILE);
    assert!(
        lines.contains(&errs[0].line),
        "at the second entry ({lines:?}): {found:#?}"
    );
    assert!(
        errs[0].message.contains("example-large"),
        "names the model: {:#?}",
        errs[0]
    );
    assert!(!errs[0].fix.trim().is_empty(), "a fix: {:#?}", errs[0]);
    let anthropic: Vec<_> = prices
        .models
        .iter()
        .filter(|m| m.provider == "anthropic" && m.model == "example-large")
        .collect();
    assert_eq!(anthropic.len(), 1, "{:?}", prices.models);
    assert_eq!(
        anthropic[0].input_micros_per_mtok, 3_000_000,
        "the first is kept"
    );
    assert!(
        prices
            .models
            .iter()
            .any(|m| m.provider == "openai" && m.model == "example-large"),
        "another provider's price is its own: {:?}",
        prices.models
    );

    // In a project the error stops the snapshot.
    let dir = copy_of("costs");
    edit(
        dir.path(),
        FILE,
        "rates:",
        "  - provider: anthropic\n    model: example-large\n    input_usd_per_mtok: 0.5\n    output_usd_per_mtok: 1.0\nrates:",
    );
    let loaded = am_config::load(dir.path()).unwrap();
    assert!(loaded.snapshot.is_none(), "{:#?}", loaded.findings);
}

/// Row 27 (owner Q10): `updated:` stays optional, but a file that prices a
/// model without it gets a Warning naming `updated` (never an error: the
/// snapshot is still built); a file that prices no model needs none.
#[test]
fn a_price_file_without_a_date_warns() {
    let undated = "schema_version: 2\nmodels:\n  - provider: anthropic\n    model: example-large\n    input_usd_per_mtok: 3.0\n    output_usd_per_mtok: 15.0\n";
    let (prices, found) = parsed(undated);
    assert_eq!(prices.updated, None);
    assert!(errors(&found).is_empty(), "{found:#?}");
    let warned = warnings(&found);
    assert_eq!(warned.len(), 1, "{found:#?}");
    assert_eq!(warned[0].file, FILE);
    assert!(
        warned[0].message.contains("updated") || warned[0].fix.contains("updated"),
        "names `updated`: {:#?}",
        warned[0]
    );
    assert!(!warned[0].fix.trim().is_empty());

    // Dated: no warning.
    let (_, found) = parsed(&format!("updated: 2026-11-01\n{undated}"));
    assert!(found.is_empty(), "{found:#?}");

    // No model priced (a service only, or nothing): no warning.
    let (_, found) = parsed(
        "schema_version: 2\nservices:\n  - id: crawl-credits\n    unit: credit\n    usd_per_unit: 0.001\n",
    );
    assert!(found.is_empty(), "{found:#?}");
    let (_, found) = parsed("schema_version: 2\nmodels: []\n");
    assert!(found.is_empty(), "{found:#?}");

    // In a project: a warning, and the snapshot is still built.
    let dir = copy_of("costs");
    edit(dir.path(), FILE, "updated: 2026-11-01\n", "");
    let loaded = am_config::load(dir.path()).unwrap();
    assert!(loaded.snapshot.is_some(), "{:#?}", loaded.findings);
    let warned = warnings(&loaded.findings);
    assert!(
        warned
            .iter()
            .any(|f| f.file == FILE && (f.message.contains("updated") || f.fix.contains("updated"))),
        "{:#?}",
        loaded.findings
    );
}

/// Row 27, owner Q10: the shipped defaults `templates/prices.yaml` and each
/// sample company's `models/prices.yaml` exist, parse with no error and
/// carry their `updated:` date (a test fails the build while one is
/// missing or undated). `templates/prices.yaml` carries no rate (the
/// company currency is the owner's); a sample carries the rate of its own
/// company currency, and prices every model its seats name on a cloud
/// Account of `fixtures/accounts/sample.yaml`. The numbers are the owner's
/// to check at review; no test reads them.
#[test]
fn the_shipped_price_files_carry_their_date() {
    let dated = |path: &Path| -> Prices {
        assert!(path.is_file(), "{} is shipped", path.display());
        let text = std::fs::read_to_string(path).unwrap();
        let (prices, found) = parsed(&text);
        assert!(errors(&found).is_empty(), "{}: {found:#?}", path.display());
        assert!(
            !warnings(&found)
                .iter()
                .any(|f| f.message.contains("updated") || f.fix.contains("updated")),
            "{}: {found:#?}",
            path.display()
        );
        let updated = prices
            .updated
            .clone()
            .unwrap_or_else(|| panic!("{} has no `updated:` date", path.display()));
        assert!(
            updated.parse::<jiff::civil::Date>().is_ok(),
            "{}: {updated}",
            path.display()
        );
        assert!(
            !prices.models.is_empty(),
            "{} prices a model",
            path.display()
        );
        for m in &prices.models {
            assert!(
                m.input_micros_per_mtok > 0 && m.output_micros_per_mtok > 0,
                "{}: {m:?}",
                path.display()
            );
        }
        prices
    };

    let defaults = dated(&repo().join("templates/prices.yaml"));
    assert!(
        defaults.rates.is_empty(),
        "the shipped defaults carry no rate: {:?}",
        defaults.rates
    );

    let accounts = common::user_accounts("sample");
    let samples: Vec<PathBuf> = std::fs::read_dir(repo().join("templates/samples"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .collect();
    assert!(!samples.is_empty());
    for sample in samples {
        let prices = dated(&sample.join(FILE));
        let loaded = am_config::load_in(&sample, LoadOptions::template(&sample)).unwrap();
        let snap = loaded
            .snapshot
            .unwrap_or_else(|| panic!("{}: {:#?}", sample.display(), loaded.findings));
        let currency = snap.company().currency.clone();
        if currency != "USD" {
            assert!(
                prices.rates.iter().any(|r| r.currency == currency),
                "{} has a rate for its company currency {currency}: {:?}",
                sample.display(),
                prices.rates
            );
        }
        for seat in snap.seats().values() {
            for model in seat.model.iter().chain(seat.fallback.iter()) {
                let Some(account) = accounts.get(&model.account) else {
                    continue;
                };
                assert!(
                    prices
                        .models
                        .iter()
                        .any(|p| p.provider == account.provider && p.model == model.model),
                    "{}: seat {} names {} on the cloud Account {} ({}), which the file does not price",
                    sample.display(),
                    seat.id,
                    model.model,
                    model.account,
                    account.provider
                );
            }
        }
    }
}

/// Row 11 (owner Q6): the loaded prices carry the sha256 (lowercase hex) of
/// the file's bytes as read, the run's rate-snapshot id; an edit gives a
/// new one; a project with no price file has none.
#[test]
fn the_prices_carry_their_file_hash() {
    let dir = copy_of("costs");
    let bytes = std::fs::read(dir.path().join(FILE)).unwrap();
    let snap = snapshot(dir.path());
    let first = snap.prices().sha256.clone().expect("the file's hash");
    assert_eq!(first, sha256_hex(&bytes));
    assert_eq!(first.len(), 64);
    assert!(
        first
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
    );

    edit(
        dir.path(),
        FILE,
        "input_usd_per_mtok: 3.0",
        "input_usd_per_mtok: 3.5",
    );
    let edited = snapshot(dir.path());
    let second = edited.prices().sha256.clone().expect("the file's hash");
    assert_ne!(second, first, "an edited file gives a new snapshot id");
    assert_eq!(
        second,
        sha256_hex(&std::fs::read(dir.path().join(FILE)).unwrap())
    );

    // A text parsed on its own carries its own hash too.
    let text = with_rates("  - currency: EUR\n    per_usd: 0.92\n");
    assert_eq!(parsed(&text).0.sha256, Some(sha256_hex(text.as_bytes())));

    // No file, no hash (the run's snapshot is `none`).
    let bare = copy_of("costs");
    std::fs::remove_file(bare.path().join(FILE)).unwrap();
    write(
        bare.path(),
        "seats/writer.md",
        &std::fs::read_to_string(bare.path().join("seats/writer.md"))
            .unwrap()
            .replace("account: cloud", "account: local-lmstudio"),
    );
    let none = snapshot(bare.path());
    assert_eq!(none.prices().sha256, None);
    assert!(none.prices().rates.is_empty());
}
