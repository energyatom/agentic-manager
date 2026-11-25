//! The cost ledger's writes (epic 3 story 7; AD-8, AD-9, AD-25; owner Q3,
//! Q4, Q5, Q13; migration 0013).
//!
//! - A reservation journals its terms (the price, the rate and its
//!   snapshot, the counted prompt tokens, `max_output_tokens`, the week and
//!   the team); the writer computes both amounts from them with
//!   [`am_domain::money::cost`], so no caller hands the store an amount. A
//!   cost the function refuses (overflow, a cloud price with no rate)
//!   refuses the command with nothing written.
//! - A settlement prices the provider's usage with the terms the row
//!   journaled, in both currencies; missing usage, or a row from before
//!   0013 (no terms), settles at the reservation. It stamps the call's
//!   `settled_at`. Usage above the reservation settles as reported (no cap
//!   is checked: epic 4).
//! - `MarkLost` settles at the reservation in both currencies too, and
//!   audits what it settled at.
//! - Review M3: the words a reservation journals are checked: the currency
//!   is three capitals, the week a real `YYYY-MM-DD` date, the team a config
//!   id (empty only for the spike's rows, which run no seat).
//! - The audit holds amounts, counts, terms, ids, the week and the team,
//!   never text (AD-25).

use am_domain::Usage;
use am_domain::money::{CallPrice, Money, Rate, cost};
use rusqlite::{OptionalExtension, Transaction, params};
use serde_json::{Map, Value, json};

use crate::{StoreError, new_id};

/// What a reservation carries besides its ids: the command's fields.
pub(crate) struct Reservation<'a> {
    pub call_id: &'a str,
    pub run_id: &'a str,
    pub price: &'a CallPrice,
    pub rate: &'a Rate,
    pub counted_input_tokens: u64,
    pub max_output_tokens: u32,
    pub week: &'a str,
    pub team: &'a str,
}

/// Prices and journals the reservation's ledger row (the call row exists):
/// the audit arguments it adds to `model_call.reserved`.
pub(crate) fn reserve(
    tx: &Transaction,
    r: &Reservation,
    now: &str,
) -> Result<Map<String, Value>, StoreError> {
    let currency = &r.rate.currency;
    if currency.len() != 3 || !currency.chars().all(|c| c.is_ascii_uppercase()) {
        return Err(refused(
            r.call_id,
            &format!("its currency `{currency}` is not a three-letter ISO 4217 code in capitals"),
        ));
    }
    if !is_date(r.week) {
        return Err(refused(
            r.call_id,
            &format!("its budget week `{}` is not a date (YYYY-MM-DD)", r.week),
        ));
    }
    if !r.team.is_empty() && !am_domain::config_names::is_config_id(r.team) {
        return Err(refused(
            r.call_id,
            &format!("its team `{}` is not a config id", r.team),
        ));
    }
    let money = cost(
        r.price,
        r.rate,
        r.counted_input_tokens,
        u64::from(r.max_output_tokens),
    )
    .map_err(|e| refused(r.call_id, &e.to_string()))?;
    let counted = i64::try_from(r.counted_input_tokens)
        .map_err(|_| refused(r.call_id, "the counted prompt tokens do not fit"))?;
    let local = i64::from(r.price.local);
    tx.execute(
        "INSERT INTO ledger (id, model_call_id, run_id, kind, reserved_micro_usd, state, rate_snapshot, created_at,
             currency, reserved_micro_company, input_micro_usd_per_mtok, output_micro_usd_per_mtok,
             company_micro_per_usd, prices_updated, counted_input_tokens, max_output_tokens, local, week, team)
         VALUES (?1, ?2, ?3, 'model', ?4, 'reserved', ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        params![
            new_id(),
            r.call_id,
            r.run_id,
            money.usd_micro,
            r.rate.snapshot,
            now,
            r.rate.currency,
            money.company_micro,
            r.price.input_micro_usd_per_mtok,
            r.price.output_micro_usd_per_mtok,
            r.rate.company_micro_per_usd,
            r.rate.prices_updated,
            counted,
            i64::from(r.max_output_tokens),
            local,
            r.week,
            r.team,
        ],
    )?;
    let args = json!({
        "company_micro_per_usd": r.rate.company_micro_per_usd,
        "counted_input_tokens": counted,
        "currency": r.rate.currency,
        "input_micro_usd_per_mtok": r.price.input_micro_usd_per_mtok,
        "local": local,
        "max_output_tokens": r.max_output_tokens,
        "output_micro_usd_per_mtok": r.price.output_micro_usd_per_mtok,
        "prices_updated": r.rate.prices_updated,
        "rate_snapshot": r.rate.snapshot,
        "reserved_micro_company": money.company_micro,
        "reserved_micro_usd": money.usd_micro,
        "team": r.team,
        "week": r.week,
    });
    Ok(match args {
        Value::Object(map) => map,
        _ => Map::new(),
    })
}

/// A real calendar date written as `YYYY-MM-DD`.
fn is_date(text: &str) -> bool {
    text.parse::<jiff::civil::Date>()
        .is_ok_and(|d| d.to_string() == text)
}

fn refused(call_id: &str, why: &str) -> StoreError {
    StoreError::Invalid(format!(
        "refused: model call {call_id} cannot be reserved: {why}; nothing was written"
    ))
}

/// What a settlement wrote: the audit's amounts, currency, tokens and flags.
#[derive(Debug, Clone)]
pub(crate) struct Settled {
    usd: i64,
    company: Option<i64>,
    currency: Option<String>,
    usage: Option<Usage>,
    at_reservation: bool,
    over_reservation: bool,
}

impl Settled {
    /// `model_call.settled`'s arguments (row 26): exactly these seven.
    pub(crate) fn args(&self) -> Map<String, Value> {
        let mut map = Map::new();
        map.insert("at_reservation".into(), json!(self.at_reservation));
        map.insert("currency".into(), json!(self.currency));
        map.insert(
            "input_tokens".into(),
            json!(self.usage.map(|u| u.input_tokens)),
        );
        map.insert(
            "output_tokens".into(),
            json!(self.usage.map(|u| u.output_tokens)),
        );
        map.insert("over_reservation".into(), json!(self.over_reservation));
        map.insert("settled_micro_company".into(), json!(self.company));
        map.insert("settled_micro_usd".into(), json!(self.usd));
        map
    }
}

/// The terms one ledger row journaled at its reservation.
struct Terms {
    reserved_usd: i64,
    reserved_company: Option<i64>,
    currency: Option<String>,
    input_price: Option<i64>,
    output_price: Option<i64>,
    per_usd: Option<i64>,
    local: Option<i64>,
    snapshot: String,
}

/// Settles an open call: prices `usage` at the terms its reservation
/// journaled, in both currencies, or settles at the reservation when there
/// is no usage (or the row predates the terms); stamps `settled_at`. A call
/// that is not open is refused.
pub(crate) fn settle(
    tx: &Transaction,
    call_id: &str,
    usage: Option<Usage>,
    now: &str,
) -> Result<Settled, StoreError> {
    let tokens = |n: u64| {
        i64::try_from(n).map_err(|_| {
            StoreError::Invalid(format!("refused: call {call_id}'s usage does not fit"))
        })
    };
    let (input, output) = match usage {
        Some(u) => (
            Some(tokens(u.input_tokens)?),
            Some(tokens(u.output_tokens)?),
        ),
        None => (None, None),
    };
    let n = tx.execute(
        "UPDATE model_calls SET status = 'settled', settled = 1, input_tokens = ?2, output_tokens = ?3, settled_at = ?4 WHERE id = ?1 AND settled = 0",
        params![call_id, input, output, now],
    )?;
    if n != 1 {
        return Err(StoreError::Invalid(format!("call {call_id} is not open")));
    }
    let t = terms(tx, call_id)?;
    let priced = match (usage, t.input_price, t.output_price) {
        (Some(u), Some(input_price), Some(output_price)) => {
            let price = CallPrice {
                input_micro_usd_per_mtok: input_price,
                output_micro_usd_per_mtok: output_price,
                local: t.local == Some(1),
            };
            let rate = Rate {
                currency: t.currency.clone().unwrap_or_default(),
                company_micro_per_usd: t.per_usd,
                snapshot: t.snapshot.clone(),
                prices_updated: None,
            };
            let Money {
                usd_micro,
                company_micro,
            } = cost(&price, &rate, u.input_tokens, u.output_tokens).map_err(|e| {
                StoreError::Invalid(format!(
                    "refused: model call {call_id} cannot be settled: {e}; nothing was written"
                ))
            })?;
            Some((usd_micro, Some(company_micro)))
        }
        _ => None,
    };
    let at_reservation = priced.is_none();
    let (usd, company) = priced.unwrap_or((t.reserved_usd, t.reserved_company));
    tx.execute(
        "UPDATE ledger SET settled_micro_usd = ?2, settled_micro_company = ?3, state = 'settled' WHERE model_call_id = ?1",
        params![call_id, usd, company],
    )?;
    Ok(Settled {
        usd,
        company,
        currency: t.currency,
        usage,
        at_reservation,
        over_reservation: usd > t.reserved_usd,
    })
}

/// A call that never settled (a crash, a store error): marked `lost` and
/// settled at its reservation in both currencies, `settled_at` stamped. A
/// settled call keeps what it settled at. `model_call.lost`'s arguments
/// (review M8, AD-25): the amounts it settled at and the currency; each is
/// null when the call was not open, so nothing was settled.
pub(crate) fn mark_lost(
    tx: &Transaction,
    call_id: &str,
    now: &str,
) -> Result<Map<String, Value>, StoreError> {
    let n = tx.execute(
        "UPDATE model_calls SET status = 'lost', settled = 1, settled_at = ?2 WHERE id = ?1 AND settled = 0",
        params![call_id, now],
    )?;
    let (mut usd, mut company, mut currency) = (None, None, None);
    if n == 1 {
        let t = terms(tx, call_id)?;
        tx.execute(
            "UPDATE ledger SET settled_micro_usd = ?2, settled_micro_company = ?3, state = 'settled' WHERE model_call_id = ?1",
            params![call_id, t.reserved_usd, t.reserved_company],
        )?;
        (usd, company, currency) = (Some(t.reserved_usd), t.reserved_company, t.currency);
    }
    let mut args = Map::new();
    args.insert("currency".into(), json!(currency));
    args.insert("settled_micro_company".into(), json!(company));
    args.insert("settled_micro_usd".into(), json!(usd));
    Ok(args)
}

fn terms(tx: &Transaction, call_id: &str) -> Result<Terms, StoreError> {
    tx.query_row(
        "SELECT reserved_micro_usd, reserved_micro_company, currency, input_micro_usd_per_mtok,
                output_micro_usd_per_mtok, company_micro_per_usd, local, rate_snapshot
         FROM ledger WHERE model_call_id = ?1",
        params![call_id],
        |r| {
            Ok(Terms {
                reserved_usd: r.get(0)?,
                reserved_company: r.get(1)?,
                currency: r.get(2)?,
                input_price: r.get(3)?,
                output_price: r.get(4)?,
                per_usd: r.get(5)?,
                local: r.get(6)?,
                snapshot: r.get(7)?,
            })
        },
    )
    .optional()?
    .ok_or_else(|| StoreError::Invalid(format!("call {call_id} has no ledger entry")))
}
