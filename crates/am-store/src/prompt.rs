//! Story 3.5 (AD-8, AD-10, AD-25, AD-27, AD-35, AD-36, AD-43): the journal
//! of what a model was sent.
//!
//! An attempt's assembled prompt is journaled, part by part, in the
//! transaction that starts it ([`crate::WriteCommand::StartAttempt`]); a
//! schema repair call journals its own two parts when it is reserved
//! ([`crate::WriteCommand::ReserveCall`]). The writer checks AD-27's order,
//! that a slot holds only its kind, and that taint matches kind (AD-10): an
//! instruction or trusted part is clean, an untrusted part is tainted and
//! raises the attempt's taint. The text of a part that has another home (a
//! step output, a loop-back's reasons, a gate note) is not copied here
//! (AD-43).

use am_domain::Taint;

/// Why a model call was made (AD-36): a step's own turn, or its one repair
/// after a final message that did not match the step's output schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallPurpose {
    Turn,
    Repair,
}

impl CallPurpose {
    /// The word the journal column and the audit argument hold.
    pub fn as_str(self) -> &'static str {
        match self {
            CallPurpose::Turn => "turn",
            CallPurpose::Repair => "repair",
        }
    }

    pub(crate) fn parse(word: &str) -> Option<CallPurpose> {
        match word {
            "turn" => Some(CallPurpose::Turn),
            "repair" => Some(CallPurpose::Repair),
            _ => None,
        }
    }
}

/// A part's place in the prompt. The first twelve are AD-27's order;
/// `Reply` and `Repair` are used only by a repair call's own parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PromptSlot {
    Role,
    Goal,
    Backstory,
    Skill,
    CeoGoal,
    Guidance,
    Memory,
    ReferenceFiles,
    SavedLinks,
    Step,
    Trusted,
    Untrusted,
    Reply,
    Repair,
}

impl PromptSlot {
    /// The word the journal column holds (its CHECK list).
    pub fn as_str(self) -> &'static str {
        match self {
            PromptSlot::Role => "role",
            PromptSlot::Goal => "goal",
            PromptSlot::Backstory => "backstory",
            PromptSlot::Skill => "skill",
            PromptSlot::CeoGoal => "ceo_goal",
            PromptSlot::Guidance => "guidance",
            PromptSlot::Memory => "memory",
            PromptSlot::ReferenceFiles => "reference_files",
            PromptSlot::SavedLinks => "saved_links",
            PromptSlot::Step => "step",
            PromptSlot::Trusted => "trusted",
            PromptSlot::Untrusted => "untrusted",
            PromptSlot::Reply => "reply",
            PromptSlot::Repair => "repair",
        }
    }

    pub(crate) fn parse(word: &str) -> Option<PromptSlot> {
        Some(match word {
            "role" => PromptSlot::Role,
            "goal" => PromptSlot::Goal,
            "backstory" => PromptSlot::Backstory,
            "skill" => PromptSlot::Skill,
            "ceo_goal" => PromptSlot::CeoGoal,
            "guidance" => PromptSlot::Guidance,
            "memory" => PromptSlot::Memory,
            "reference_files" => PromptSlot::ReferenceFiles,
            "saved_links" => PromptSlot::SavedLinks,
            "step" => PromptSlot::Step,
            "trusted" => PromptSlot::Trusted,
            "untrusted" => PromptSlot::Untrusted,
            "reply" => PromptSlot::Reply,
            "repair" => PromptSlot::Repair,
            _ => return None,
        })
    }

    /// The only kind a part in this slot may be.
    pub(crate) fn kind(self) -> PromptKind {
        match self {
            PromptSlot::Trusted => PromptKind::Trusted,
            PromptSlot::Untrusted => PromptKind::Untrusted,
            PromptSlot::Reply => PromptKind::Assistant,
            _ => PromptKind::Instruction,
        }
    }

    /// Whether a prompt may hold the slot more than once (a role text is a
    /// list; skills, goals, notes and inputs are one part each).
    pub(crate) fn repeats(self) -> bool {
        matches!(
            self,
            PromptSlot::Role
                | PromptSlot::Skill
                | PromptSlot::CeoGoal
                | PromptSlot::Guidance
                | PromptSlot::Memory
                | PromptSlot::Trusted
                | PromptSlot::Untrusted
        )
    }
}

/// A part's type (AD-10): config or owner text, an input from the journal
/// that is not tainted, data from outside, or the model's own earlier reply
/// in a repair call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    Instruction,
    Trusted,
    Untrusted,
    Assistant,
}

impl PromptKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PromptKind::Instruction => "instruction",
            PromptKind::Trusted => "trusted",
            PromptKind::Untrusted => "untrusted",
            PromptKind::Assistant => "assistant",
        }
    }

    pub(crate) fn parse(word: &str) -> Option<PromptKind> {
        match word {
            "instruction" => Some(PromptKind::Instruction),
            "trusted" => Some(PromptKind::Trusted),
            "untrusted" => Some(PromptKind::Untrusted),
            "assistant" => Some(PromptKind::Assistant),
            _ => None,
        }
    }
}

/// One part to journal, in request order. `text` is `None` for a part whose
/// text stays on its producer's row or its Inbox item (AD-43).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPromptPart {
    pub slot: PromptSlot,
    pub kind: PromptKind,
    pub label: String,
    pub taint: Taint,
    pub text: Option<String>,
}

/// One journaled part, as the reader returns it. `call_seq` 0 is the
/// attempt's assembled prompt; a repair call's parts carry its own
/// `call_seq`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptPartRow {
    pub call_seq: i64,
    pub ord: i64,
    pub slot: PromptSlot,
    pub kind: PromptKind,
    pub label: String,
    pub taint: Taint,
    pub text: Option<String>,
    /// The text's length in bytes (0 when there is none stored).
    pub bytes: i64,
    /// A native file part's hash (epic 12); `None` for every other part.
    pub sha256: Option<String>,
}
