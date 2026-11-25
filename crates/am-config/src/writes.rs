//! What a workflow step writes (epic 3 story 6, the ToolGate slice; AD-21,
//! AD-40; the owner's fan-out decision of 2026-11-24).
//!
//! A fan-out step (`seats:` or `over_folder:`) cannot write a table until
//! story 3.14 allows `table:` on fan-out steps. One that declares `table:`,
//! or lists a seat whose grants hold `table_write`, is a validate warning
//! (never an error) at the step, and `agentic run` refuses it before any
//! run or provider, in the same words: [`fan_out_table_words`] builds them
//! for both.

use crate::entities::Step;
use crate::snapshot::ConfigSnapshot;

/// The built-in tool that writes a table row (AD-11, AD-46).
pub(crate) const TABLE_WRITE: &str = "table_write";

/// The warning's message and fix for the fan-out step `step`: the one
/// wording of `agentic validate`'s warning and `agentic run`'s refusal.
pub fn fan_out_table_words(step: &str) -> (String, String) {
    (
        format!(
            "step '{step}' is a fan-out step that would write a table; a fan-out step cannot write a table until story 3.14, so `agentic run` refuses it"
        ),
        "keep it until story 3.14 allows `table:` on fan-out steps, or move the writes to a step with one `seat:` and a `table:`".to_string(),
    )
}

/// Whether a fan-out step would write a table: it declares one, or one of
/// its `seats` holds `table_write`.
pub(crate) fn fan_out_writes_table(
    declares_table: bool,
    seats: &[String],
    holds_table_write: impl Fn(&str) -> bool,
) -> bool {
    declares_table || seats.iter().any(|s| holds_table_write(s))
}

/// `agentic run`'s check (story 3.6, row 40): `Some((message, fix))`, in
/// the words of `agentic validate`'s warning, when `step` is a fan-out step
/// that would write a table; `None` otherwise.
pub fn fan_out_table_refusal(snapshot: &ConfigSnapshot, step: &Step) -> Option<(String, String)> {
    let holds = |seat: &str| {
        snapshot
            .seats()
            .get(seat)
            .is_some_and(|s| s.grants.contains(TABLE_WRITE))
    };
    (step.fan_out && fan_out_writes_table(step.table.is_some(), &step.seats, holds))
        .then(|| fan_out_table_words(&step.id))
}
