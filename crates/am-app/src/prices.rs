//! A run's prices (epic 3 story 7; AD-9, AD-16; owner Q6, Q8).
//!
//! `agentic run` (and `agentic gate`'s continue) prices each step of the
//! plan once, when the plan is built from the snapshot the run pins:
//!
//! - **Local or cloud (Q8; review M5, decided by endpoint):** a call on
//!   this Mac costs 0 in both currencies whatever the price file says: a
//!   reserved local Account (`local-lmstudio`, `local-ollama`), or an
//!   Account of this Mac's Accounts file whose `base_url` is a server on
//!   this Mac (`http://127.0.0.1:<port>`, the one loopback form
//!   [`am_config::compatible_base_url`] accepts). Where the server is
//!   decides, never the provider's name. Every other call is priced by the
//!   snapshot's `models/prices.yaml` under (the Account's provider, the
//!   model); one it does not price refuses the run before any row (row 13).
//!   An Accounts file with an error (an id listed twice, say) decides
//!   nothing: a seat on an Account that is not reserved is refused before
//!   any row.
//! - **The rate (Q6):** the company currency from `company.yaml` and its
//!   `rates:` entry (a USD company needs none: rate 1); the snapshot id is
//!   the price file's sha256, or `none` without one, and its `updated:`
//!   date goes with it. A run with a cloud call and no rate is refused
//!   before any row (row 14); a run of calls on this Mac needs none.
//! - **The week:** the company's timezone and `week_start`.
//!
//! No currency, timezone or price is written here: each comes from config.

use am_config::user_accounts::LoadedAccounts;
use am_config::{ConfigSnapshot, ModelRef, Severity, UserAccounts};
use am_domain::money::{CallPrice, NO_SNAPSHOT, Rate};
use am_engine::run::RunMoney;
use jiff::civil::Weekday;

use crate::runtime::Places;

/// The base currency of every price (`models/prices.yaml` is in USD).
const USD: &str = "USD";

/// One USD in micro-USD: a USD company's rate.
const ONE_USD: i64 = 1_000_000;

const PRICES_FILE: &str = "models/prices.yaml";

/// This Mac's Accounts (AD-17) with their findings, as the run reads them
/// for each seat's provider and endpoint; an absent file gives none, so a
/// seat on an Account it does not hold is refused by [`step_price`] in
/// words.
pub(crate) fn this_macs_accounts(places: &Places) -> LoadedAccounts {
    am_config::user_accounts::load(&places.user.accounts_file())
}

/// Whether calls on `account` stay on this Mac (owner Q8; review M5): a
/// reserved local Account, or one whose `base_url` is a server on this Mac;
/// never by the provider's name. `None` when the Account is neither
/// reserved nor one of this Mac's.
fn on_this_mac(account: &str, accounts: &UserAccounts) -> Option<bool> {
    if am_domain::reserved_local_account(account).is_some() {
        return Some(true);
    }
    let a = accounts.get(account)?;
    Some(
        a.base_url
            .as_deref()
            .and_then(|url| am_config::compatible_base_url(url).ok())
            .is_some_and(|url| url.loopback),
    )
}

/// The price terms of `model`, the model of the seat `seat` (owner Q8): 0
/// on this Mac, else the snapshot's price for (its Account's provider, the
/// model), or a refusal naming the seat, the Account, the model and the fix.
pub(crate) fn step_price(
    snapshot: &ConfigSnapshot,
    loaded: &LoadedAccounts,
    seat: &str,
    model: &ModelRef,
    not_done: &str,
) -> Result<CallPrice, String> {
    let account = &model.account;
    let accounts = &loaded.accounts;
    // Review M5: a file with an error decides nothing for an Account that
    // is not reserved (an id listed twice is never decided by first match).
    if am_domain::reserved_local_account(account).is_none()
        && let Some(first) = loaded
            .findings
            .iter()
            .find(|f| f.severity == Severity::Error)
    {
        return Err(format!(
            "the seat `{seat}` runs on the Account `{account}`, and this Mac's Accounts file has an error ({}:{}: {}), so whether its calls cost money cannot be decided (AD-9); fix the file (`agentic accounts list` shows its problems). {not_done}",
            first.file, first.line, first.message
        ));
    }
    match on_this_mac(account, accounts) {
        Some(true) => return Ok(CallPrice::local()),
        Some(false) => {}
        None => {
            return Err(format!(
                "the seat `{seat}` runs on the Account `{account}`, which is not one of this Mac's Accounts, so whether its calls cost money cannot be known and they cannot be reserved (AD-9); add the Account with `agentic accounts add`, or choose another Account in seats/{seat}.md. {not_done}"
            ));
        }
    }
    let provider = accounts
        .get(account)
        .map(|a| a.provider.as_str())
        .unwrap_or_default();
    snapshot
        .prices()
        .models
        .iter()
        .find(|p| p.provider == provider && p.model == model.model)
        .map(|p| CallPrice {
            input_micro_usd_per_mtok: p.input_micros_per_mtok,
            output_micro_usd_per_mtok: p.output_micros_per_mtok,
            local: false,
        })
        .ok_or_else(|| {
            format!(
                "the seat `{seat}` runs the model `{}` on the cloud Account `{account}` (provider `{provider}`), and {PRICES_FILE} has no price for it, so its calls cannot be reserved (AD-9); add it to {PRICES_FILE} (`provider: {provider}`, `model: {}`, `input_usd_per_mtok` and `output_usd_per_mtok`, from the provider's price page). {not_done}",
                model.model, model.model
            )
        })
}

/// The run's money from `snapshot` (owner Q6, Q11): the company currency's
/// rate, the price file's snapshot id and date, and the company's timezone
/// and week start. `cloud_seat` is a seat whose calls cost money, if any:
/// then a company currency with no rate refuses the run.
pub(crate) fn run_money(
    snapshot: &ConfigSnapshot,
    cloud_seat: Option<&str>,
    not_done: &str,
) -> Result<RunMoney, String> {
    let company = snapshot.company();
    let prices = snapshot.prices();
    let currency = company.currency.clone();
    let per_usd = if currency == USD {
        Some(ONE_USD)
    } else {
        prices
            .rates
            .iter()
            .find(|r| r.currency == currency)
            .map(|r| r.micros_per_usd)
    };
    if let (None, Some(seat)) = (per_usd, cloud_seat) {
        return Err(format!(
            "the company's currency is {currency}, and {PRICES_FILE} has no `rates` entry for it, so the cloud calls of the seat `{seat}` cannot be priced in {currency} (AD-9); add `rates:` with `- currency: {currency}` and `per_usd:` (how many {currency} one USD buys, and its date in `updated:`) to {PRICES_FILE}. {not_done}"
        ));
    }
    let timezone = jiff::tz::TimeZone::get(&company.timezone).map_err(|e| {
        format!(
            "the company's timezone `{}` cannot be used: {e}. {not_done}",
            company.timezone
        )
    })?;
    let week_start = weekday(&company.week_start).ok_or_else(|| {
        format!(
            "the company's `week_start` `{}` is not a day of the week. {not_done}",
            company.week_start
        )
    })?;
    Ok(RunMoney {
        rate: Rate {
            currency,
            company_micro_per_usd: per_usd,
            snapshot: prices
                .sha256
                .clone()
                .unwrap_or_else(|| NO_SNAPSHOT.to_string()),
            prices_updated: prices.updated.clone(),
        },
        timezone,
        week_start,
    })
}

/// `company.yaml`'s `week_start` word as a weekday.
fn weekday(word: &str) -> Option<Weekday> {
    Some(match word {
        "monday" => Weekday::Monday,
        "tuesday" => Weekday::Tuesday,
        "wednesday" => Weekday::Wednesday,
        "thursday" => Weekday::Thursday,
        "friday" => Weekday::Friday,
        "saturday" => Weekday::Saturday,
        "sunday" => Weekday::Sunday,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use am_config::Account;
    use am_domain::capability::AccountKind;

    fn account(id: &str, provider: &str, base_url: Option<&str>) -> Account {
        Account {
            id: id.into(),
            provider: provider.into(),
            kind: AccountKind::Model,
            name: None,
            base_url: base_url.map(str::to_string),
            secret_ref: None,
        }
    }

    /// Review M5 (owner Q8): where the server is decides, never the
    /// provider's name.
    #[test]
    fn a_call_is_local_only_on_a_reserved_account_or_a_loopback_endpoint() {
        let accounts = UserAccounts::new(vec![
            account("box", "openai-compatible", Some("http://127.0.0.1:8080/v1")),
            account(
                "hosted",
                "openai-compatible",
                Some("https://models.example/v1"),
            ),
            account("named-local", "ollama", Some("https://ollama.example")),
            account("bare", "ollama", None),
            account("cloud", "anthropic", None),
            account(
                "spoof",
                "openai-compatible",
                Some("http://localhost.example:8080"),
            ),
        ]);
        for reserved in am_domain::net::RESERVED_LOCAL_ACCOUNTS {
            assert_eq!(on_this_mac(reserved, &accounts), Some(true), "{reserved}");
        }
        assert_eq!(on_this_mac("box", &accounts), Some(true));
        for cloud in ["hosted", "named-local", "bare", "cloud", "spoof"] {
            assert_eq!(on_this_mac(cloud, &accounts), Some(false), "{cloud}");
        }
        assert_eq!(on_this_mac("unknown", &accounts), None);
    }
}
