//! Epic 3 story 7, "Cost settles per call and rolls up" (AD-9; owner Q5,
//! Q8, Q11): the one money function and the budget week, through
//! am-domain's public API.
//!
//! - `am_domain::money::cost` prices a call's tokens in USD and in the
//!   company currency, in i128, rounding each amount up once from the exact
//!   value (never the company amount from the rounded USD one), refusing
//!   overflow and a cloud price with no rate in words; a local price costs
//!   nothing whatever its terms.
//! - `am_domain::week::budget_week` is the company-local week an instant
//!   falls in: the date it starts (the ledger's key) and, for a Monday
//!   start, its ISO week label from jiff.
//!
//! Prices and rates here are fake test values (the matrix's terms: input
//! 3.00 and output 15.00 USD per million tokens, 0.92 EUR per USD).
//! Written before the modules exist (tests first, 2026-11-25).
#![forbid(unsafe_code)]

use am_domain::money::{CallPrice, Money, MoneyError, Rate, cost};
use am_domain::week::{BudgetWeek, budget_week};
use jiff::Timestamp;
use jiff::civil::{Date, Weekday, date};
use jiff::tz::TimeZone;

/// A rate snapshot id: a price file's sha256 hex (owner Q6).
const SNAPSHOT: &str = "3f9c0a1b2c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7";

/// The matrix's cloud terms: 3.00 in, 15.00 out, USD per million tokens.
fn cloud() -> CallPrice {
    CallPrice {
        input_micro_usd_per_mtok: 3_000_000,
        output_micro_usd_per_mtok: 15_000_000,
        local: false,
    }
}

/// `per_usd` company micro-units per USD in `currency` (a fake rate).
fn rate(currency: &str, per_usd: i64) -> Rate {
    Rate {
        currency: currency.into(),
        company_micro_per_usd: Some(per_usd),
        snapshot: SNAPSHOT.into(),
        prices_updated: Some("2026-11-01".into()),
    }
}

/// 0.92 EUR per USD (fake).
fn eur() -> Rate {
    rate("EUR", 920_000)
}

/// A company currency with no rate in the price file.
fn no_rate(currency: &str) -> Rate {
    Rate {
        company_micro_per_usd: None,
        ..rate(currency, 1)
    }
}

fn money(usd_micro: i64, company_micro: i64) -> Money {
    Money {
        usd_micro,
        company_micro,
    }
}

// ------------------------------------------------------------------ cost

/// Rows 1, 2 and 15 (owner Q5): each amount is the exact value rounded up
/// once, to the micro-unit.
#[test]
fn a_cost_rounds_up_once_per_currency() {
    // Row 1: (1,000 x 3 + 4,096 x 15) / 10^6 USD = 0.06444 USD exactly;
    // x 0.92 = 0.0592848 EUR, rounded up to 59,285 micro-EUR.
    assert_eq!(
        cost(&cloud(), &eur(), 1_000, 4_096).unwrap(),
        money(64_440, 59_285)
    );
    // Row 2: usage 1,200/90 is 4,950 micro-USD and 4,554 micro-EUR, both
    // exact, so nothing is rounded.
    assert_eq!(
        cost(&cloud(), &eur(), 1_200, 90).unwrap(),
        money(4_950, 4_554)
    );
    // Row 15: one token at 0.075 USD per million is 0.075 micro-USD, which
    // rounds up to 1 in each currency (0.069 micro-EUR exact).
    let cheap = CallPrice {
        input_micro_usd_per_mtok: 75_000,
        output_micro_usd_per_mtok: 0,
        local: false,
    };
    assert_eq!(cost(&cheap, &eur(), 1, 0).unwrap(), money(1, 1));
    // One output token at 15 USD per million: 15 micro-USD exactly, 13.8
    // micro-EUR rounded up to 14.
    assert_eq!(cost(&cloud(), &eur(), 0, 1).unwrap(), money(15, 14));
    // Nothing used costs nothing.
    assert_eq!(cost(&cloud(), &eur(), 0, 0).unwrap(), money(0, 0));
    // A free allowance priced at 0.0 is 0 too, and not local (owner Q8).
    let free = CallPrice {
        input_micro_usd_per_mtok: 0,
        output_micro_usd_per_mtok: 0,
        local: false,
    };
    assert_eq!(cost(&free, &eur(), 5_000, 500).unwrap(), money(0, 0));
}

/// Row 15 (owner Q5): the company amount is computed from the exact USD
/// value times the rate, never from the rounded USD amount. Row 12: a USD
/// company's amounts equal its USD amounts at rate 1.
#[test]
fn the_company_amount_comes_from_the_exact_usd_value() {
    let cheap = CallPrice {
        input_micro_usd_per_mtok: 75_000,
        output_micro_usd_per_mtok: 0,
        local: false,
    };
    // 0.075 micro-USD x 83.5 = 6.2625 micro-INR exact: 7. From the rounded
    // USD amount (1 micro-USD x 83.5 = 83.5) it would be 84.
    let inr = rate("INR", 83_500_000);
    assert_eq!(cost(&cheap, &inr, 1, 0).unwrap(), money(1, 7));
    // Three such tokens: 0.225 micro-USD (1) and 18.7875 micro-INR (19),
    // not 3 x 7 or 1 x 83.5.
    assert_eq!(cost(&cheap, &inr, 3, 0).unwrap(), money(1, 19));
    // Row 12: at 1 USD = 1 USD the two amounts are equal.
    let usd = rate("USD", 1_000_000);
    for (input, output) in [(1_000, 4_096), (1_200, 90), (1, 0), (0, 1), (7, 3)] {
        let m = cost(&cloud(), &usd, input, output).unwrap();
        assert_eq!(m.usd_micro, m.company_micro, "{input}/{output}");
    }
    assert_eq!(cost(&cheap, &usd, 1, 0).unwrap(), money(1, 1));
}

/// Row 15: an amount that does not fit is refused in words, never wrapped
/// or saturated: the i128 product, the USD amount and the company amount
/// each on its own.
#[test]
fn an_overflowing_cost_is_refused() {
    let dearest = CallPrice {
        input_micro_usd_per_mtok: i64::MAX,
        output_micro_usd_per_mtok: i64::MAX,
        local: false,
    };
    let both = cost(&dearest, &eur(), u64::MAX, u64::MAX);
    assert!(matches!(both, Err(MoneyError::Overflow)), "{both:?}");
    let words = both.unwrap_err().to_string();
    assert!(!words.trim().is_empty(), "refused in words");

    // 10^15 tokens at 10^13 micro-USD per million: 10^22 micro-USD, past
    // i64.
    let dear = CallPrice {
        input_micro_usd_per_mtok: 10_000_000_000_000,
        output_micro_usd_per_mtok: 0,
        local: false,
    };
    let usd = cost(&dear, &eur(), 1_000_000_000_000_000, 0);
    assert!(matches!(usd, Err(MoneyError::Overflow)), "{usd:?}");

    // 10^12 tokens at 9 x 10^12: 9 x 10^18 micro-USD fits i64; at 2.0 per
    // USD the company amount (1.8 x 10^19) does not.
    let big = CallPrice {
        input_micro_usd_per_mtok: 9_000_000_000_000,
        output_micro_usd_per_mtok: 0,
        local: false,
    };
    assert_eq!(
        cost(&big, &rate("USD", 1_000_000), 1_000_000_000_000, 0)
            .unwrap()
            .usd_micro,
        9_000_000_000_000_000_000
    );
    let company = cost(&big, &rate("CHF", 2_000_000), 1_000_000_000_000, 0);
    assert!(matches!(company, Err(MoneyError::Overflow)), "{company:?}");
}

/// Row 5 (Conventions, Money; owner Q8): a local call is 0 in both
/// currencies whatever the price file says, with or without a rate; a
/// cloud price with no rate is refused (owner Q6).
#[test]
fn a_local_price_costs_nothing() {
    let local = CallPrice::local();
    assert_eq!(
        local,
        CallPrice {
            input_micro_usd_per_mtok: 0,
            output_micro_usd_per_mtok: 0,
            local: true,
        }
    );
    assert_eq!(cost(&local, &eur(), 1_200, 90).unwrap(), money(0, 0));
    assert_eq!(
        cost(&local, &eur(), u64::MAX, u64::MAX).unwrap(),
        money(0, 0),
        "no product is formed for a local call"
    );
    // A local Account priced in the file by mistake still costs nothing.
    let priced_local = CallPrice {
        local: true,
        ..cloud()
    };
    assert_eq!(cost(&priced_local, &eur(), 1_200, 90).unwrap(), money(0, 0));
    // A local-only run needs no rate. `Rate::none` is the rate of a run
    // with no price file: no rate, the snapshot `none`, no date (row 6).
    assert_eq!(
        Rate::none("EUR"),
        Rate {
            currency: "EUR".into(),
            company_micro_per_usd: None,
            snapshot: "none".into(),
            prices_updated: None,
        }
    );
    assert_eq!(
        cost(&local, &Rate::none("EUR"), 1_200, 90).unwrap(),
        money(0, 0)
    );
    assert_eq!(
        cost(&local, &no_rate("EUR"), 1_200, 90).unwrap(),
        money(0, 0)
    );
    // A cloud call cannot be priced in the company currency without one.
    let refused = cost(&cloud(), &no_rate("EUR"), 1_200, 90);
    assert!(matches!(refused, Err(MoneyError::NoRate)), "{refused:?}");
    let words = refused.unwrap_err().to_string();
    assert!(!words.trim().is_empty(), "refused in words");
}

/// Review round 1, M3 (AD-9; tester's variant name, 2026-11-25:
/// `MoneyError::InvalidTerms`): a negative input or output price, or a rate
/// of 0 or below, is refused up front with its own words, never as
/// `Overflow` and never priced, also when the bad term meets zero tokens.
/// Free terms (0) and the smallest rate (1 micro-unit) are fine.
#[test]
fn a_negative_term_or_a_rate_of_zero_is_refused_in_words() {
    let terms = |input: i64, output: i64| CallPrice {
        input_micro_usd_per_mtok: input,
        output_micro_usd_per_mtok: output,
        local: false,
    };
    let overflow_words = MoneyError::Overflow.to_string();
    for (case, price, per_usd, input, output) in [
        (
            "a negative input price",
            terms(-3_000_000, 15_000_000),
            920_000,
            1_000,
            90,
        ),
        (
            "a negative output price",
            terms(3_000_000, -15_000_000),
            920_000,
            1_000,
            90,
        ),
        (
            "a negative input price on no input",
            terms(-3_000_000, 15_000_000),
            920_000,
            0,
            90,
        ),
        (
            "a negative output price on no output",
            terms(3_000_000, -1),
            920_000,
            1_000,
            0,
        ),
        ("a rate of 0", cloud(), 0, 1_000, 90),
        ("a negative rate", cloud(), -920_000, 1_000, 90),
        ("a rate of 0 on no tokens", cloud(), 0, 0, 0),
    ] {
        let refused = cost(&price, &rate("EUR", per_usd), input, output);
        assert!(
            matches!(refused, Err(MoneyError::InvalidTerms { .. })),
            "{case}: {refused:?}"
        );
        let words = refused.unwrap_err().to_string();
        assert!(!words.trim().is_empty(), "{case}: in words");
        assert_ne!(words, overflow_words, "{case}: not the overflow's words");
    }
    assert_eq!(
        cost(&terms(0, 0), &rate("EUR", 920_000), 1_000, 90).unwrap(),
        money(0, 0)
    );
    assert_eq!(
        cost(&cloud(), &rate("EUR", 1), 1_000, 4_096).unwrap(),
        money(64_440, 1)
    );
}

// ------------------------------------------------------------------ week

fn at(instant: &str) -> Timestamp {
    instant.parse().unwrap()
}

fn tz(name: &str) -> TimeZone {
    TimeZone::get(name).unwrap()
}

fn starting(d: Date) -> BudgetWeek {
    BudgetWeek { start: d }
}

/// Row 17 (owner Q11): the week turns at Monday 00:00 company time, on
/// either side of a DST change (New York: EST after 2026-11-01, EDT after
/// 2026-03-08).
#[test]
fn the_week_turns_at_monday_midnight_across_dst() {
    let ny = tz("America/New_York");
    let week = |t: &str| budget_week(at(t), &ny, Weekday::Monday);

    // Sunday 1 November, 23:30 EST.
    let sunday = week("2026-11-02T04:30:00Z");
    assert_eq!(sunday, starting(date(2026, 10, 26)));
    assert_eq!(sunday.key(), "2026-10-26");
    assert_eq!(sunday.iso_label().as_deref(), Some("2026-W44"));
    // Monday 2 November, 00:00 EST.
    let monday = week("2026-11-02T05:00:00Z");
    assert_eq!(monday, starting(date(2026, 11, 2)));
    assert_eq!(monday.key(), "2026-11-02");
    assert_eq!(monday.iso_label().as_deref(), Some("2026-W45"));

    // Sunday 8 March, 23:30 EDT.
    let before = week("2026-03-09T03:30:00Z");
    assert_eq!(before, starting(date(2026, 3, 2)));
    assert_eq!(before.iso_label().as_deref(), Some("2026-W10"));
    // Monday 9 March, 00:30 EDT.
    let after = week("2026-03-09T04:30:00Z");
    assert_eq!(after, starting(date(2026, 3, 9)));
    assert_eq!(after.key(), "2026-03-09");
    assert_eq!(after.iso_label().as_deref(), Some("2026-W11"));

    // The same instants in UTC fall in other weeks: the zone decides.
    let utc = TimeZone::UTC;
    assert_eq!(
        budget_week(at("2026-11-02T04:30:00Z"), &utc, Weekday::Monday),
        starting(date(2026, 11, 2))
    );
}

/// Row 18 (owner Q11): the ISO week-year, not the calendar year, at the
/// year boundary, in company time.
#[test]
fn the_iso_week_year_holds_at_the_year_boundary() {
    let kolkata = tz("Asia/Kolkata");
    // Friday 1 January 2027, 00:30 IST: still 2026's week 53.
    let new_year = budget_week(at("2026-12-31T19:00:00Z"), &kolkata, Weekday::Monday);
    assert_eq!(new_year, starting(date(2026, 12, 28)));
    assert_eq!(new_year.key(), "2026-12-28");
    assert_eq!(new_year.iso_label().as_deref(), Some("2026-W53"));
    // Monday 4 January 2027, 00:00 IST, while UTC is still Sunday.
    let first = budget_week(at("2027-01-03T18:30:00Z"), &kolkata, Weekday::Monday);
    assert_eq!(first, starting(date(2027, 1, 4)));
    assert_eq!(first.iso_label().as_deref(), Some("2027-W01"));
    // Monday 29 December 2025 in a UTC company: week 1 of ISO year 2026.
    let utc = budget_week(at("2025-12-29T00:00:00Z"), &TimeZone::UTC, Weekday::Monday);
    assert_eq!(utc, starting(date(2025, 12, 29)));
    assert_eq!(utc.key(), "2025-12-29");
    assert_eq!(utc.iso_label().as_deref(), Some("2026-W01"));
}

/// Row 19 (owner Q11): with `week_start: sunday` the key is the Sunday the
/// week starts on, and there is no ISO label (an ISO week starts on a
/// Monday).
#[test]
fn a_sunday_week_has_no_iso_label() {
    let ny = tz("America/New_York");
    let week = |t: &str| budget_week(at(t), &ny, Weekday::Sunday);
    // Sunday 22 November 2026, 10:00 EST.
    let sunday = week("2026-11-22T15:00:00Z");
    assert_eq!(sunday, starting(date(2026, 11, 22)));
    assert_eq!(sunday.key(), "2026-11-22");
    assert_eq!(sunday.iso_label(), None);
    // Monday 23 November, 10:00 EST: the same week.
    assert_eq!(week("2026-11-23T15:00:00Z"), starting(date(2026, 11, 22)));
    // Saturday 21 November, 23:59 EST: the week before.
    assert_eq!(week("2026-11-22T04:59:00Z"), starting(date(2026, 11, 15)));
    // Sunday 22 November, 00:00 EST: the new week.
    assert_eq!(week("2026-11-22T05:00:00Z"), starting(date(2026, 11, 22)));
}
