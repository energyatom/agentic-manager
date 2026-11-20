//! Cron syntax check for schedule triggers. croner's types stay inside this
//! module and never reach a public API (docs/crate-notes/croner.md).

use std::str::FromStr;

pub(crate) const CRON_FIX: &str = "use 5 fields: minute hour day-of-month month day-of-week, e.g. \"0 9 * * 1-5\" (09:00 on weekdays)";

/// Ok for a valid five-field pattern; otherwise why it is not.
pub(crate) fn check(expr: &str) -> Result<(), String> {
    let n = expr.split_whitespace().count();
    if n != 5 {
        return Err(format!("it has {n} fields, not 5"));
    }
    croner::Cron::from_str(expr)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::check;

    #[test]
    fn five_field_patterns() {
        for ok in ["0 9 * * 1-5", "*/5 * * * *", "30 2 1 * *", "0 0 * * MON"] {
            assert!(check(ok).is_ok(), "{ok}: {:?}", check(ok));
        }
        for bad in [
            "61 * * * *",
            "* 25 * * *",
            "0 0 9 * * 1-5",
            "* * *",
            "",
            "@daily",
            "x * * * *",
        ] {
            assert!(check(bad).is_err(), "{bad}");
        }
    }
}
