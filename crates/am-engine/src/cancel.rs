//! One cancellation tree (AD-20): global, then run, then step, then call.

use std::sync::Arc;

use am_domain::{BoxFuture, CancelSignal};
use tokio_util::sync::CancellationToken;

/// The `CancelSignal` port over a tokio-util token.
#[derive(Clone)]
pub struct TokenSignal(CancellationToken);

impl TokenSignal {
    pub fn signal(token: CancellationToken) -> Arc<dyn CancelSignal> {
        Arc::new(TokenSignal(token))
    }
}

impl CancelSignal for TokenSignal {
    fn is_cancelled(&self) -> bool {
        self.0.is_cancelled()
    }
    fn cancelled(&self) -> BoxFuture<()> {
        Box::pin(self.0.clone().cancelled_owned())
    }
}

/// The root of the tree ("Stop everything"). Children are cancelled with it.
#[derive(Clone, Default)]
pub struct CancelTree {
    global: CancellationToken,
}

impl CancelTree {
    pub fn new() -> Self {
        CancelTree::default()
    }
    pub fn run_token(&self) -> CancellationToken {
        self.global.child_token()
    }
    pub fn stop_everything(&self) {
        self.global.cancel();
    }
}
