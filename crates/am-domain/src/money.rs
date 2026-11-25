//! Money (AD-9; story 3.7, owner Q5, Q6, Q8): the one function that prices a
//! model call, in USD and in the company currency.
//!
//! Amounts are i64 micro-units. The arithmetic runs in i128, each amount is
//! rounded up once from the exact value (the company amount from the exact
//! USD value times the rate, never from the rounded USD amount), and an
//! amount that does not fit an i64 is refused, never wrapped or saturated.
//! A local call costs nothing whatever its terms.
//!
//! ```
//! use am_domain::money::{CallPrice, Money, Rate, cost};
//! let price = CallPrice { input_micro_usd_per_mtok: 3_000_000, output_micro_usd_per_mtok: 15_000_000, local: false };
//! let rate = Rate { currency: "EUR".into(), company_micro_per_usd: Some(920_000), snapshot: "none".into(), prices_updated: None };
//! assert_eq!(cost(&price, &rate, 1_200, 90).unwrap(), Money { usd_micro: 4_950, company_micro: 4_554 });
//! assert_eq!(cost(&CallPrice::local(), &Rate::none("EUR"), 1_200, 90).unwrap(), Money { usd_micro: 0, company_micro: 0 });
//! ```

/// One million: tokens per price unit, and micro-units per unit.
const MILLION: i128 = 1_000_000;

/// The rate snapshot id of a run with no price file.
pub const NO_SNAPSHOT: &str = "none";

/// The price terms of one model call, journaled on its reservation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallPrice {
    /// Micro-USD per million input tokens.
    pub input_micro_usd_per_mtok: i64,
    /// Micro-USD per million output tokens.
    pub output_micro_usd_per_mtok: i64,
    /// A call on this Mac: it costs nothing whatever its terms (owner Q8).
    pub local: bool,
}

impl CallPrice {
    /// The terms of a local call: 0 and 0, local.
    pub fn local() -> Self {
        Self {
            input_micro_usd_per_mtok: 0,
            output_micro_usd_per_mtok: 0,
            local: true,
        }
    }
}

/// The company currency and its rate from the run's pinned price file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rate {
    /// The company currency's ISO code (from `company.yaml`).
    pub currency: String,
    /// Company micro-units per USD; `None` when the price file has none for
    /// the currency. A USD company carries 1,000,000.
    pub company_micro_per_usd: Option<i64>,
    /// The rate snapshot id: the price file's sha256 hex, or `none`.
    pub snapshot: String,
    /// The price file's `updated:` date, when it gives one.
    pub prices_updated: Option<String>,
}

impl Rate {
    /// The rate of a run with no price file: no rate, snapshot `none`, no
    /// date.
    pub fn none(currency: &str) -> Self {
        Self {
            currency: currency.to_owned(),
            company_micro_per_usd: None,
            snapshot: NO_SNAPSHOT.to_owned(),
            prices_updated: None,
        }
    }
}

/// An amount in USD and in the company currency, in micro-units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Money {
    /// Micro-USD.
    pub usd_micro: i64,
    /// Company micro-units.
    pub company_micro: i64,
}

/// Why a call could not be priced.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MoneyError {
    /// The amount does not fit a 64-bit micro-unit count.
    #[error("the cost is too large to record: it does not fit a 64-bit micro-unit amount")]
    Overflow,
    /// A priced call with no rate for the company currency.
    #[error("the call cannot be priced in the company currency: the price file has no rate for it")]
    NoRate,
    /// Story 3.7, review M3: a price term below 0, or a rate of 0 or below;
    /// `term` names which.
    #[error(
        "the call cannot be priced: its {term}, and a price is never below 0 nor a rate 0 or below"
    )]
    InvalidTerms { term: &'static str },
}

/// Prices a call's tokens at `price`, in USD and in `rate`'s currency.
///
/// A local price costs 0 and 0. Otherwise a negative price term or a rate of
/// 0 or below is refused up front ([`MoneyError::InvalidTerms`]), and no
/// rate is [`MoneyError::NoRate`]. Then N = input × input price + output ×
/// output price (micro-USD per million tokens), USD = ⌈N / 10⁶⌉ and company
/// = ⌈N × rate / 10¹²⌉, each from N.
pub fn cost(
    price: &CallPrice,
    rate: &Rate,
    input_tokens: u64,
    output_tokens: u64,
) -> Result<Money, MoneyError> {
    if price.local {
        return Ok(Money {
            usd_micro: 0,
            company_micro: 0,
        });
    }
    let invalid = |term| Err(MoneyError::InvalidTerms { term });
    if price.input_micro_usd_per_mtok < 0 {
        return invalid("input price is below 0");
    }
    if price.output_micro_usd_per_mtok < 0 {
        return invalid("output price is below 0");
    }
    let per_usd = rate.company_micro_per_usd.ok_or(MoneyError::NoRate)?;
    if per_usd <= 0 {
        return invalid("rate is 0 or below");
    }
    let input = i128::from(input_tokens)
        .checked_mul(i128::from(price.input_micro_usd_per_mtok))
        .ok_or(MoneyError::Overflow)?;
    let output = i128::from(output_tokens)
        .checked_mul(i128::from(price.output_micro_usd_per_mtok))
        .ok_or(MoneyError::Overflow)?;
    let exact = input.checked_add(output).ok_or(MoneyError::Overflow)?;
    let usd = ceil_div(exact, MILLION)?;
    let company = ceil_div(
        exact
            .checked_mul(i128::from(per_usd))
            .ok_or(MoneyError::Overflow)?,
        MILLION * MILLION,
    )?;
    Ok(Money {
        usd_micro: usd,
        company_micro: company,
    })
}

/// ⌈n / d⌉ for n ≥ 0 and d > 0, as an i64.
fn ceil_div(n: i128, d: i128) -> Result<i64, MoneyError> {
    let q = n / d + i128::from(n % d != 0);
    i64::try_from(q).map_err(|_| MoneyError::Overflow)
}
