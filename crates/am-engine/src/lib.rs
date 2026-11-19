//! `am-engine`: the application core. Spike scope: the cancellation tree
//! (AD-20), the journaled run executor with resume (AD-8, AD-9) and the
//! scheduler loop (AD-18). ToolGate (AD-11) lands with criterion 9.
#![forbid(unsafe_code)]

pub mod cancel;
pub mod journal;
pub mod scheduler;

pub use cancel::{CancelTree, TokenSignal};
