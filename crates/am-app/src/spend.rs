//! `agentic status`'s spend (epic 3 story 7; AD-9; owner Q12, Q13, Q15).
//!
//! Read from the ledger alone (no config): each run line shows its calls'
//! settled spend, each attempt line its own with its call count, and a
//! spend section after the runs sums settled spend by week, team and seat,
//! headed by the price file's date, its rate snapshot and the rate. Open
//! reservations are listed apart, never folded into settled spend.
//!
//! Format (Q12): each amount is its ISO code and the amount in units with
//! up to six decimals, trailing zeros trimmed, `0` when zero; the company
//! currency first, then USD, joined by ` · ` with the tokens (`in/out
//! tokens`, grouped with commas). No currency symbol and no locale
//! grouping of amounts. A row from before the company currency was kept
//! shows `-` for its company amount, with a footnote; amounts in two
//! company currencies are summed per currency (`EUR … + INR …`), never
//! converted; USD sums every row. Calls on this Mac are marked `local`.

use std::collections::BTreeMap;

use am_store::SpendRow;

/// The base currency of every amount.
const USD: &str = "USD";

/// What the footnote says of a `-` company amount.
const UNKNOWN_COMPANY: &str = "recorded before company-currency settlement";

/// The sums of a group of calls.
struct Sum {
    usd: i128,
    company: BTreeMap<String, i128>,
    /// A row with no company amount (from before migration 0013).
    unknown: bool,
    input: i128,
    output: i128,
    /// The calls' time, from the rows that hold both stamps.
    millis: Option<i128>,
    calls: usize,
    all_local: bool,
}

impl Sum {
    fn new() -> Sum {
        Sum {
            usd: 0,
            company: BTreeMap::new(),
            unknown: false,
            input: 0,
            output: 0,
            millis: None,
            calls: 0,
            all_local: true,
        }
    }

    /// Adds a settled row's amounts, tokens and time.
    fn settled(&mut self, row: &SpendRow) {
        self.add(
            row,
            row.settled_micro_usd.unwrap_or(0),
            row.settled_micro_company,
        );
        self.input += i128::from(row.input_tokens.unwrap_or(0));
        self.output += i128::from(row.output_tokens.unwrap_or(0));
        if let Some(took) = row
            .settled_at
            .as_deref()
            .and_then(|to| millis_between(&row.created_at, to))
        {
            self.millis = Some(self.millis.unwrap_or(0) + took);
        }
    }

    /// Adds an open row's reservation.
    fn reserved(&mut self, row: &SpendRow) {
        self.add(row, row.reserved_micro_usd, row.reserved_micro_company);
    }

    fn add(&mut self, row: &SpendRow, usd: i64, company: Option<i64>) {
        self.usd += i128::from(usd);
        match (&row.currency, company) {
            (Some(currency), Some(amount)) => {
                *self.company.entry(currency.clone()).or_insert(0) += i128::from(amount);
            }
            _ => self.unknown = true,
        }
        self.calls += 1;
        self.all_local &= row.local == Some(true);
    }

    /// `EUR 0.004554 · USD 0.00495`: the company amounts, then USD.
    fn money(&self) -> String {
        let usd = format!("{USD} {}", amount(self.usd));
        // A USD company's amounts are its USD amounts: said once.
        if !self.unknown && self.company.len() == 1 && self.company.get(USD) == Some(&self.usd) {
            return usd;
        }
        let mut parts: Vec<String> = self
            .company
            .iter()
            .map(|(currency, sum)| format!("{currency} {}", amount(*sum)))
            .collect();
        if self.unknown || parts.is_empty() {
            parts.push("-".into());
        }
        format!("{} · {usd}", parts.join(" + "))
    }

    /// `2,100/160 tokens`.
    fn tokens(&self) -> String {
        format!("{}/{} tokens", grouped(self.input), grouped(self.output))
    }

    fn time(&self) -> String {
        self.millis.map_or_else(|| "-".into(), seconds)
    }

    /// ` · local` when every call of the group ran on this Mac.
    fn local(&self) -> &'static str {
        if self.calls > 0 && self.all_local {
            " · local"
        } else {
            ""
        }
    }
}

/// An amount of micro-units in units: up to six decimals, trailing zeros
/// trimmed, `0` when zero; no grouping.
fn amount(micro: i128) -> String {
    let sign = if micro < 0 { "-" } else { "" };
    let micro = micro.abs();
    let (whole, frac) = (micro / 1_000_000, micro % 1_000_000);
    if frac == 0 {
        format!("{sign}{whole}")
    } else {
        let digits = format!("{frac:06}");
        format!("{sign}{whole}.{}", digits.trim_end_matches('0'))
    }
}

/// A token count grouped by thousands with commas.
fn grouped(n: i128) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) && c.is_ascii_digit() {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn seconds(millis: i128) -> String {
    format!("{}.{:03}s", millis / 1000, millis % 1000)
}

fn millis_between(from: &str, to: &str) -> Option<i128> {
    let from: jiff::Timestamp = from.parse().ok()?;
    let to: jiff::Timestamp = to.parse().ok()?;
    Some(i128::from(
        (to.as_millisecond() - from.as_millisecond()).max(0),
    ))
}

fn calls(n: usize) -> String {
    if n == 1 {
        "1 call".into()
    } else {
        format!("{n} calls")
    }
}

/// The settled spend of `rows` (a run's), after its run line: `  EUR … ·
/// USD … · in/out tokens`; empty when none of them settled.
pub(crate) fn run_suffix<'a>(rows: impl Iterator<Item = &'a SpendRow>) -> String {
    let mut sum = Sum::new();
    for row in rows.filter(|r| r.state == "settled") {
        sum.settled(row);
    }
    if sum.calls == 0 {
        return String::new();
    }
    format!("  {} · {}", sum.money(), sum.tokens())
}

/// An attempt's spend after its line: its settled amounts and tokens, its
/// call count, the calls not settled yet and `local`; empty with no call.
pub(crate) fn attempt_suffix<'a>(rows: impl Iterator<Item = &'a SpendRow>) -> String {
    let rows: Vec<&SpendRow> = rows.collect();
    if rows.is_empty() {
        return String::new();
    }
    let mut sum = Sum::new();
    let mut open = 0;
    let mut all_local = true;
    for row in &rows {
        all_local &= row.local == Some(true);
        if row.state == "settled" {
            sum.settled(row);
        } else {
            open += 1;
        }
    }
    let mut out = if sum.calls > 0 {
        format!(
            "  {} · {} · {}",
            sum.money(),
            sum.tokens(),
            calls(rows.len())
        )
    } else {
        format!("  {}", calls(rows.len()))
    };
    if open > 0 {
        out.push_str(&format!(" · {open} reserved, not settled"));
    }
    if all_local {
        out.push_str(" · local");
    }
    out
}

/// The spend section: its head (the price files' dates, snapshots and
/// rates), the settled sums by week, team and seat, the open reservations
/// apart and the footnote of a `-` amount. Empty with no call.
pub(crate) fn section(rows: &[SpendRow]) -> Vec<String> {
    if rows.is_empty() {
        return Vec::new();
    }
    let mut out = vec![head(rows)];
    let settled: Vec<&SpendRow> = rows.iter().filter(|r| r.state == "settled").collect();
    let mut weeks: BTreeMap<(bool, String), Sum> = BTreeMap::new();
    let mut teams: BTreeMap<(bool, String), Sum> = BTreeMap::new();
    let mut seats: BTreeMap<(bool, String), Sum> = BTreeMap::new();
    // Known keys first, in order; the unknown (`-`) last.
    let key = |v: &Option<String>| (v.is_none(), v.clone().unwrap_or_else(|| "-".into()));
    for row in &settled {
        weeks
            .entry(key(&row.week))
            .or_insert_with(Sum::new)
            .settled(row);
        teams
            .entry(key(&row.team))
            .or_insert_with(Sum::new)
            .settled(row);
        seats
            .entry(key(&row.seat))
            .or_insert_with(Sum::new)
            .settled(row);
    }
    let line = |label: String, sum: &Sum| {
        format!(
            "  {label}  {} · {} · {}{}",
            sum.money(),
            sum.tokens(),
            sum.time(),
            sum.local()
        )
    };
    for ((_, week), sum) in &weeks {
        out.push(line(week_label(week), sum));
    }
    for ((_, team), sum) in &teams {
        out.push(line(format!("team {team}"), sum));
    }
    for ((_, seat), sum) in &seats {
        out.push(line(format!("seat {seat}"), sum));
    }
    let mut open = Sum::new();
    for row in rows.iter().filter(|r| r.state != "settled") {
        open.reserved(row);
    }
    if open.calls > 0 {
        out.push(format!(
            "  reserved, not settled: {} ({})",
            open.money(),
            calls(open.calls)
        ));
    }
    if rows.iter().any(|r| r.currency.is_none()) {
        out.push(format!(
            "  -: {UNKNOWN_COMPANY}, so its amount in the company currency is not known"
        ));
    }
    out
}

/// `week 2026-W48 (from 2026-11-23)` for a week from a Monday, `week from
/// <date>` otherwise, `week -` when the row holds none.
fn week_label(key: &str) -> String {
    let Ok(start) = key.parse::<jiff::civil::Date>() else {
        return format!("week {key}");
    };
    match (am_domain::week::BudgetWeek { start }).iso_label() {
        Some(iso) => format!("week {iso} (from {key})"),
        None => format!("week from {key}"),
    }
}

/// `spend, settled (prices updated <date>, rate snapshot <12 hex>: 1 USD =
/// 0.92 EUR; …)`: one description per price file the rows were priced
/// with, in the order first seen.
fn head(rows: &[SpendRow]) -> String {
    let mut seen: Vec<String> = Vec::new();
    for row in rows {
        let Some(currency) = &row.currency else {
            continue;
        };
        let words = if row.rate_snapshot == am_domain::money::NO_SNAPSHOT {
            "no price file: local calls only".to_string()
        } else {
            let date = match &row.prices_updated {
                Some(date) => format!("prices updated {date}"),
                None => "prices date not given".to_string(),
            };
            let short: String = row.rate_snapshot.chars().take(12).collect();
            let rate = match row.company_micro_per_usd {
                Some(per_usd) if currency != USD => {
                    format!(": 1 {USD} = {} {currency}", amount(i128::from(per_usd)))
                }
                _ => String::new(),
            };
            format!("{date}, rate snapshot {short}{rate}")
        };
        if !seen.contains(&words) {
            seen.push(words);
        }
    }
    if seen.is_empty() {
        "spend, settled".into()
    } else {
        format!("spend, settled ({})", seen.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts_and_tokens_are_written_plainly() {
        assert_eq!(amount(0), "0");
        assert_eq!(amount(4_950), "0.00495");
        assert_eq!(amount(4_554), "0.004554");
        assert_eq!(amount(360_000), "0.36");
        assert_eq!(amount(1_234_500_000), "1234.5");
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(900), "900");
        assert_eq!(grouped(2_100), "2,100");
        assert_eq!(grouped(1_234_567), "1,234,567");
        assert_eq!(week_label("2026-11-23"), "week 2026-W48 (from 2026-11-23)");
        assert_eq!(week_label("2026-11-22"), "week from 2026-11-22");
        assert_eq!(week_label("-"), "week -");
    }
}
