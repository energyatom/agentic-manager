//! The one Inbox item entity's kinds (AD-28) and the step states of the
//! run journal (AD-8), so the store and the engine share one spelling.

/// What an Inbox item asks of the owner (AD-28). Only the engine creates
/// items; only `Actor::Human` resolves them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InboxKind {
    Review,
    Approval,
    Draft,
    Question,
    BudgetAsk,
    Failure,
    Proposal,
}

impl InboxKind {
    /// AD-28's seven kinds, in its order.
    pub const ALL: [InboxKind; 7] = [
        InboxKind::Review,
        InboxKind::Approval,
        InboxKind::Draft,
        InboxKind::Question,
        InboxKind::BudgetAsk,
        InboxKind::Failure,
        InboxKind::Proposal,
    ];

    /// The kind as the journal spells it.
    pub fn as_str(self) -> &'static str {
        match self {
            InboxKind::Review => "review",
            InboxKind::Approval => "approval",
            InboxKind::Draft => "draft",
            InboxKind::Question => "question",
            InboxKind::BudgetAsk => "budget_ask",
            InboxKind::Failure => "failure",
            InboxKind::Proposal => "proposal",
        }
    }

    /// The kind spelled `s`, if it is one of the seven.
    pub fn parse(s: &str) -> Option<InboxKind> {
        InboxKind::ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// The kind's name in an item's words: `Review`, `Budget ask`, …
    pub fn title(self) -> &'static str {
        match self {
            InboxKind::Review => "Review",
            InboxKind::Approval => "Approval",
            InboxKind::Draft => "Draft",
            InboxKind::Question => "Question",
            InboxKind::BudgetAsk => "Budget ask",
            InboxKind::Failure => "Failure",
            InboxKind::Proposal => "Proposal",
        }
    }
}

/// A step attempt's state (AD-8). Stories add `waiting_branches` (3.14)
/// and `waiting_review` (3.16).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StepStatus {
    Queued,
    Running,
    WaitingHuman,
    LoopedBack,
    Succeeded,
    Failed,
    Cancelled,
}

impl StepStatus {
    pub const ALL: [StepStatus; 7] = [
        StepStatus::Queued,
        StepStatus::Running,
        StepStatus::WaitingHuman,
        StepStatus::LoopedBack,
        StepStatus::Succeeded,
        StepStatus::Failed,
        StepStatus::Cancelled,
    ];

    /// The state as the journal spells it.
    pub fn as_str(self) -> &'static str {
        match self {
            StepStatus::Queued => "queued",
            StepStatus::Running => "running",
            StepStatus::WaitingHuman => "waiting_human",
            StepStatus::LoopedBack => "looped_back",
            StepStatus::Succeeded => "succeeded",
            StepStatus::Failed => "failed",
            StepStatus::Cancelled => "cancelled",
        }
    }

    /// The state spelled `s`, if it is one.
    pub fn parse(s: &str) -> Option<StepStatus> {
        StepStatus::ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// A final state: nothing moves the attempt on (a loop-back's re-run is
    /// a new attempt).
    pub fn is_final(self) -> bool {
        matches!(
            self,
            StepStatus::LoopedBack
                | StepStatus::Succeeded
                | StepStatus::Failed
                | StepStatus::Cancelled
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spellings_round_trip() {
        for k in InboxKind::ALL {
            assert_eq!(InboxKind::parse(k.as_str()), Some(k));
        }
        for s in StepStatus::ALL {
            assert_eq!(StepStatus::parse(s.as_str()), Some(s));
        }
        assert_eq!(InboxKind::parse("Review"), None);
        assert_eq!(StepStatus::parse("skipped"), None);
    }
}
