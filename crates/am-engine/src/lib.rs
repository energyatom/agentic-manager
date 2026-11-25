//! `am-engine`: the application core. Spike scope: the cancellation tree
//! (AD-20), the journaled run executor with resume (AD-8, AD-9), the
//! scheduler loop (AD-18) and ToolGate's per-step tool limits (AD-11).
//! Epic 3 story 1 adds [`run`]: the driver of a configured workflow's run,
//! on a plan of plain types; story 3 makes each step attempt a bounded loop
//! of model turns with its caps, and adds the `next`, `loop_back` and gate
//! hand-offs, Inbox items and taint carrying (AD-10, AD-11, AD-28, AD-36).
//! Story 4 adds [`gate`], the engine's apply of a decision a person made on
//! a gate's Review item, and [`run::continue_run`] (AD-8, AD-15). Story 6's
//! ToolGate slice adds [`row_writer`], the row-writer port over the store,
//! which only ToolGate calls (AD-21). Story 22 adds `row_inputs`, the
//! crate's own read site of a row's content: the rows a step reads at its
//! start, as fenced untrusted data (AD-10, AD-43). Story 7 adds [`count`],
//! the prompt-token count a call's reservation is priced from (AD-9).
#![forbid(unsafe_code)]

pub mod cancel;
pub mod count;
pub mod gate;
pub mod journal;
mod parts;
pub mod prompt;
pub(crate) mod row_inputs;
pub mod row_writer;
pub mod run;
pub mod scheduler;
pub mod schema;
pub mod toolgate;

pub use cancel::{CancelTree, TokenSignal};
