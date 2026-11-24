//! The one assembler of a step's request (epic 3 story 5; AD-10, AD-27).
//!
//! A step's request is built here and nowhere else, from typed slots the
//! plan fills (am-app from a seat, a skill and the config; the engine from
//! the journal), in AD-27's order (as amended 2026-11-18):
//!
//! - system parts: role instructions, goal, backstory, each attached skill,
//!   CEO goals, unexpired guidance notes, memory notes, reference files,
//!   saved links;
//! - the user message: the step instruction, then the trusted inputs (a
//!   clean step's output, loop-back reasons, a gate note), then the fenced
//!   untrusted data.
//!
//! Slots fill in any order and come out in this one. Nothing is cut,
//! summarised or limited here (the overflow pause is story 3.10), and an
//! empty slot makes no part and no heading.
//!
//! **Types (AD-10).** Slots 1 to 10 are [`Instruction`]s that am-app built
//! from config: am-engine never calls `Instruction::authored`, so the
//! heading each headed part starts with comes from [`heading`] and is added
//! by the caller. Slot 11 is [`Part::Trusted`], built by `TrustedInput`'s
//! constructors from the journal; slot 12 is [`Part::Untrusted`].
//!
//! **Names, never content.** A reference file or a skill reference appears
//! by name and a saved link by title and URL ([`file_list`], [`link_list`]);
//! no file is opened for the prompt. Every name goes through
//! [`one_line`]: a hidden character is written as `\u{XXXX}`, a newline or
//! tab becomes a space, so an entry cannot span lines or forge a heading.
//!
//! **The journal records.** [`Assembled::parts`] is one record per request
//! part, in order, for `StartAttempt`: slot, kind, label, taint and the
//! text where it has no other home (config and plan-input text; an input
//! from the journal keeps its text on its producer's row, AD-43).

use am_domain::{Instruction, Part, Taint, UntrustedContent, UntrustedData};
use am_store::{NewPromptPart, PromptKind, PromptSlot};

/// The label of the role instructions (the seat file's body).
pub const ROLE: &str = "Role instructions";
/// The label of a seat's goal.
pub const GOAL: &str = "Goal";
/// The label of a seat's backstory.
pub const BACKSTORY: &str = "Backstory";
/// The label of the seat's reference-file list.
pub const REFERENCE_FILES: &str = "Reference files";
/// The label of the seat's saved-link list.
pub const SAVED_LINKS: &str = "Saved links";
/// The label of the step instruction.
pub const STEP: &str = "Step instruction";

/// The label of an attached skill: `skill:<id>`.
pub fn skill_label(id: &str) -> String {
    format!("skill:{id}")
}

/// The label of a CEO goal: `CEO goal:<id>`.
pub fn ceo_goal_label(id: &str) -> String {
    format!("CEO goal:{id}")
}

/// The label of a guidance note: `Guidance:<id>`.
pub fn guidance_label(id: &str) -> String {
    format!("Guidance:{id}")
}

/// The label of a memory note: `Memory:<id>`.
pub fn memory_label(id: &str) -> String {
    format!("Memory:{id}")
}

/// The text a headed system part starts with: its label on a line of its
/// own, then a blank line. The caller (am-app) appends the body and builds
/// the `Instruction`; am-engine authors none.
pub fn heading(label: &str) -> String {
    format!("## {}\n\n", one_line(label))
}

/// `text` on one line: hidden characters written as `\u{XXXX}`
/// ([`am_domain::text::escape_hidden`]), then each newline and tab a space.
pub fn one_line(text: &str) -> String {
    am_domain::text::escape_hidden(text)
        .0
        .replace(['\n', '\t'], " ")
}

/// Reference-file names, one `- ` entry per line, each through [`one_line`]
/// (a bullet, so a name such as `## Goal` never reads as a heading). No
/// heading, no note, no content.
pub fn file_list(names: &[String]) -> String {
    names
        .iter()
        .map(|n| format!("- {}", one_line(n)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Saved links as `- title \u{2014} url`, one per line, each part through
/// [`one_line`]. No group, no page content.
pub fn link_list(links: &[(String, String)]) -> String {
    links
        .iter()
        .map(|(title, url)| format!("- {} \u{2014} {}", one_line(title), one_line(url)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A CEO goal or a memory note: its id and its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdText {
    pub id: String,
    pub text: Instruction,
}

/// A guidance note: its id, its text and when it expires (`expires <= now`
/// drops it from the attempt that starts then).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Guidance {
    pub id: String,
    pub text: Instruction,
    pub expires: Option<jiff::Timestamp>,
}

/// Slots 2 to 9: what a seat's config (and, one day, the company's goals
/// and notes) adds to a step's system text. Each is built by am-app; the
/// order they were added in does not matter, except within a slot that
/// repeats (skills, goals, notes), which keeps its own.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StepPrompt {
    goal: Option<Instruction>,
    backstory: Option<Instruction>,
    skills: Vec<(String, Instruction)>,
    ceo_goals: Vec<IdText>,
    guidance: Vec<Guidance>,
    memory: Vec<IdText>,
    reference_files: Option<Instruction>,
    saved_links: Option<Instruction>,
}

impl StepPrompt {
    pub fn with_goal(mut self, goal: Instruction) -> Self {
        self.goal = Some(goal);
        self
    }

    pub fn with_backstory(mut self, backstory: Instruction) -> Self {
        self.backstory = Some(backstory);
        self
    }

    pub fn with_skill(mut self, id: impl Into<String>, body: Instruction) -> Self {
        self.skills.push((id.into(), body));
        self
    }

    pub fn with_ceo_goal(mut self, id: impl Into<String>, text: Instruction) -> Self {
        self.ceo_goals.push(IdText {
            id: id.into(),
            text,
        });
        self
    }

    pub fn with_guidance(
        mut self,
        id: impl Into<String>,
        text: Instruction,
        expires: Option<jiff::Timestamp>,
    ) -> Self {
        self.guidance.push(Guidance {
            id: id.into(),
            text,
            expires,
        });
        self
    }

    pub fn with_memory(mut self, id: impl Into<String>, text: Instruction) -> Self {
        self.memory.push(IdText {
            id: id.into(),
            text,
        });
        self
    }

    pub fn with_reference_files(mut self, files: Instruction) -> Self {
        self.reference_files = Some(files);
        self
    }

    pub fn with_saved_links(mut self, links: Instruction) -> Self {
        self.saved_links = Some(links);
        self
    }
}

/// Everything an attempt's request is built from.
#[derive(Debug, Clone)]
pub struct Slots {
    /// Slot 1: the role instructions (the seat file's body).
    pub role: Vec<Instruction>,
    /// Slots 2 to 9.
    pub prompt: StepPrompt,
    /// Slot 10.
    pub step: Instruction,
    /// Slot 11: the `Part::Trusted` inputs, in the order given.
    pub trusted: Vec<Part>,
    /// Slot 12: a tainted producer's output (`Part::Untrusted`, same label),
    /// in the order given.
    pub produced_untrusted: Vec<Part>,
    /// Slot 12: the plan's own untrusted inputs, after the produced ones.
    pub plan_inputs: Vec<UntrustedData>,
}

/// A step attempt's request parts and the journal's records of them.
#[derive(Debug, Clone, PartialEq)]
pub struct Assembled {
    /// The request's system parts (slots 1 to 9).
    pub system: Vec<Instruction>,
    /// The request's first user message (slots 10 to 12).
    pub user: Vec<Part>,
    /// One record per request part, in order, for `StartAttempt`.
    pub parts: Vec<NewPromptPart>,
}

fn instruction_record(slot: PromptSlot, label: &str, text: &Instruction) -> NewPromptPart {
    NewPromptPart {
        slot,
        kind: PromptKind::Instruction,
        label: label.to_string(),
        taint: Taint::Clean,
        text: Some(text.text().to_string()),
    }
}

/// Builds the request's parts in AD-27's order from `slots`, and the
/// journal's record of each. `now` is the attempt's start (the plan's
/// clock): a guidance note whose `expires` is at or before it is absent.
/// Nothing is cut; an instruction whose body is empty after trimming is no part.
pub fn assemble(slots: Slots, now: jiff::Timestamp) -> Assembled {
    let Slots {
        role,
        prompt,
        step,
        trusted,
        produced_untrusted,
        plan_inputs,
    } = slots;
    let mut system: Vec<Instruction> = Vec::new();
    let mut parts: Vec<NewPromptPart> = Vec::new();
    let mut put = |slot: PromptSlot, label: &str, text: Instruction| {
        // The body is what follows the part's heading (a bare part, the role
        // text, is all body); a part with nothing in it is no part.
        let body = text
            .text()
            .strip_prefix(heading(label).as_str())
            .unwrap_or(text.text());
        if body.trim().is_empty() {
            return;
        }
        parts.push(instruction_record(slot, label, &text));
        system.push(text);
    };
    for text in role {
        put(PromptSlot::Role, ROLE, text);
    }
    if let Some(goal) = prompt.goal {
        put(PromptSlot::Goal, GOAL, goal);
    }
    if let Some(backstory) = prompt.backstory {
        put(PromptSlot::Backstory, BACKSTORY, backstory);
    }
    for (id, body) in prompt.skills {
        put(PromptSlot::Skill, &skill_label(&id), body);
    }
    for goal in prompt.ceo_goals {
        put(PromptSlot::CeoGoal, &ceo_goal_label(&goal.id), goal.text);
    }
    for note in prompt.guidance {
        if note.expires.is_some_and(|expires| expires <= now) {
            continue;
        }
        put(PromptSlot::Guidance, &guidance_label(&note.id), note.text);
    }
    for note in prompt.memory {
        put(PromptSlot::Memory, &memory_label(&note.id), note.text);
    }
    if let Some(files) = prompt.reference_files {
        put(PromptSlot::ReferenceFiles, REFERENCE_FILES, files);
    }
    if let Some(links) = prompt.saved_links {
        put(PromptSlot::SavedLinks, SAVED_LINKS, links);
    }

    let mut user: Vec<Part> = Vec::new();
    parts.push(instruction_record(PromptSlot::Step, STEP, &step));
    user.push(Part::Instruction(step));
    for part in trusted {
        parts.push(input_record(&part, PromptSlot::Trusted, false));
        user.push(part);
    }
    for part in produced_untrusted {
        parts.push(input_record(&part, PromptSlot::Untrusted, false));
        user.push(part);
    }
    for data in plan_inputs {
        let part = Part::Untrusted(data);
        parts.push(input_record(&part, PromptSlot::Untrusted, true));
        user.push(part);
    }
    Assembled {
        system,
        user,
        parts,
    }
}

/// The record of an input part: a trusted or a fenced one. Its text is kept
/// only for a plan input's text (`keep_text`), which has no other home; a
/// producer's or a note's stays on its own row (AD-43). A native file part
/// is journaled by label alone here (file parts are epic 12's).
fn input_record(part: &Part, slot: PromptSlot, keep_text: bool) -> NewPromptPart {
    match part {
        Part::Trusted(t) => NewPromptPart {
            slot,
            kind: PromptKind::Trusted,
            label: t.label().to_string(),
            taint: Taint::Clean,
            text: None,
        },
        Part::Untrusted(d) => NewPromptPart {
            slot,
            kind: PromptKind::Untrusted,
            label: d.label().to_string(),
            taint: Taint::Tainted,
            text: match d.content() {
                UntrustedContent::Text(text) if keep_text => Some(text.clone()),
                _ => None,
            },
        },
        other => unreachable!("slots 11 and 12 hold only inputs, not {other:?}"),
    }
}
