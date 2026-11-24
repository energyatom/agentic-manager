//! The engine's side of the Owner gate (epic 3 story 4; AD-8, AD-15,
//! AD-28, AD-36): applying a decision a person committed on a gate's
//! Review item. The person resolved the item (the store's Human-only
//! command, which this crate never names: AD-15, `cargo xtask
//! check-deps`); the engine applies what they decided, as
//! `System(engine)`, in its own transaction, decided from committed rows:
//!
//! - approve: the gate attempt succeeds, and the step after it receives the
//!   Review's payload ([`crate::run`]);
//! - return: within the Review item's `max_returns`, the gate loops back to
//!   the producing step, whose new attempt receives the note; past it, the
//!   gate fails with one Failure item (the engine's words: the step,
//!   `max_returns` and its value, the Review item; never the note) and the
//!   run fails;
//! - reject: the gate and the run fail, with no Failure item; the reason
//!   names who rejected it.
//!
//! The return count is the journal's (AD-36): this run's Review items on
//! attempts of the gate's step resolved `return`, this one included; the
//! writer counts it again inside its transaction and refuses an apply that
//! disagrees.

use am_domain::{Actor, Decision, InboxKind};
use am_store::{NewInboxItem, Reader, StoreError, WriteCommand, Writer, new_id};

/// What an apply did, for the caller's report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    /// The run of the item.
    pub run_id: String,
    /// The decision that was applied.
    pub decision: Decision,
    /// Whether the apply finished the run `failed` (a reject, a return past
    /// the maximum).
    pub run_failed: bool,
    /// Why the run failed, in words, when it did.
    pub reason: Option<String>,
    /// The Failure item a return past the maximum raised.
    pub item: Option<String>,
}

fn engine() -> Actor {
    Actor::System {
        component: "engine".into(),
    }
}

/// Applies the decision a person committed on the Review item `item_id`
/// (story 3.4): reads the item, its gate attempt and the return count from
/// `reader`, then sends the one `WriteCommand::ApplyResolution` through
/// `writer` as `System(engine)`, with the Failure item when a return is
/// past the item's `max_returns`. Refused (and nothing changes) on an open
/// item, an item that is not a Review, and a decision applied already.
pub fn apply_resolution(
    writer: &Writer,
    reader: &Reader,
    item_id: &str,
) -> Result<Applied, StoreError> {
    let item = reader.inbox_item(item_id)?.ok_or_else(|| {
        StoreError::Invalid(format!(
            "refused: there is no Inbox item {item_id}; nothing was changed"
        ))
    })?;
    let Some(decision) = item.decision.filter(|_| item.status == "resolved") else {
        return Err(StoreError::Invalid(format!(
            "refused: Inbox item {item_id} is open: no person has decided on it, so there is nothing to apply (AD-15); nothing was changed"
        )));
    };
    if item.kind != InboxKind::Review {
        return Err(StoreError::Invalid(format!(
            "refused: the decision on {} item {item_id} has nothing for the engine to apply in this version; nothing was changed",
            item.kind.title()
        )));
    }
    let gate = reader
        .steps(&item.run_id)?
        .into_iter()
        .find(|s| s.id == item.step_run_id)
        .ok_or_else(|| {
            StoreError::Invalid(format!(
                "refused: the step run {} of Inbox item {item_id} is not in its run; nothing was changed",
                item.step_run_id
            ))
        })?;
    let who = item
        .resolver()
        .map(|by| by.describe())
        .unwrap_or_else(|| "an unknown resolver".into());
    let mut applied = Applied {
        run_id: item.run_id.clone(),
        decision,
        run_failed: false,
        reason: None,
        item: None,
    };
    let mut past_max = None;
    match decision {
        Decision::Reject => {
            applied.run_failed = true;
            applied.reason = Some(format!(
                "rejected at step `{}` by {who} (Review item {item_id})",
                gate.step_key
            ));
        }
        Decision::Return => {
            let max = item.max_returns.unwrap_or(0);
            let returns = reader.returns_at(&item.run_id, &gate.step_key)?;
            if returns > max {
                let failure = NewInboxItem {
                    id: new_id(),
                    kind: InboxKind::Failure,
                    payload: Some(gate.id.clone()),
                    taint: item.taint,
                    title: format!("Step {} was returned too often", gate.step_key),
                    body: format!(
                        "Step `{}` was returned {returns} times, past its max_returns of {max}, so the step failed and the run failed. The last return was on Review item {item_id}.",
                        gate.step_key
                    ),
                    max_returns: None,
                };
                applied.run_failed = true;
                applied.reason = Some(format!(
                    "step `{}` failed: it was returned {returns} times, past its max_returns of {max}",
                    gate.step_key
                ));
                applied.item = Some(failure.id.clone());
                past_max = Some(failure);
            }
        }
        Decision::Approve | Decision::Answer | Decision::Dismiss => {}
    }
    writer.exec(
        engine(),
        WriteCommand::ApplyResolution {
            item_id: item_id.to_string(),
            past_max,
        },
    )?;
    Ok(applied)
}
