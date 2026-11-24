//! `agentic gate [<item id>] [--project <folder>]` (epic 3 story 4; AD-8,
//! AD-10, AD-13, AD-15, AD-16, AD-28, AD-36): the owner decides the
//! project's open Inbox items on an interactive terminal, and the runs the
//! decisions free go on in the foreground.
//!
//! A decision is a CLI-class act (AD-15), so the checks run in this order,
//! and the first that fails is what the owner reads:
//!
//! 1. usage (exit 2): no flag decides anything (`--yes`, `--approve` and
//!    every other option are usage errors; AD-15: no auto-approve);
//! 2. stdin and stdout are both an interactive terminal (the `Terminal`
//!    port; [`crate::terminal::interactive`] in the binary): else refused,
//!    nothing read and the project not opened;
//! 3. no stdio MCP server and no Source with `render: true` is enabled in
//!    the project's files (a program that runs as the owner could type
//!    here, AD-13); a file with an error fails closed. The decisions then
//!    belong in the app's Inbox;
//! 4. the project opens as `agentic run` opens it (its lock, migrations,
//!    the marker), refused in words while `agentic serve` holds it
//!    (Question 6);
//! 5. for approve and return only, before anything is written: `agentic
//!    run`'s checks on the snapshot the run pinned (no outside change, no
//!    pending config migration, no config error, no owner child in it, its
//!    hash equal to the run's pin, a plan and providers for the steps still
//!    to run; Question 2), read as the server's pin check reads them
//!    ([`crate::serving::current_snapshot`]); and the run could go on
//!    (`am_engine::run::ready_to_go_on`: no other step running, no call
//!    unsettled or tool call in flight; review R2). Reject and dismiss
//!    need none of them.
//!
//! Each open item (Review, then Question, then Failure; oldest first) is
//! shown on the terminal with its id, run, workflow, step and attempt, the
//! engine's words and, for a Review, the returns so far and the output it
//! carries (model text): every line behind the prefix `  | `, every hidden
//! character (a control character but newline and tab, a format,
//! separator or tag character) shown as `\u{XXXX}` with a count line, and
//! runs of blank lines collapsed (review R7). A tainted Review says, above that output and again at the
//! choice, that it came from untrusted content, and approving it needs the
//! item's id typed back (owner condition 2; AD-15's v0.1 clause lets the CLI
//! resolve it until the Inbox UI arrives, story 6.7, which turns
//! [`cli_may_resolve`] into a refusal).
//!
//! Every answer is typed, read with a 10-minute limit, after whatever was
//! typed ahead of the prompt is dropped (review R9); only a line ended by
//! Return answers (R11): `a` approve, `r`
//! return (past `max_returns` a warning first, confirmed by `y`; then the
//! note), `j` reject (confirmed by typing `reject`), `d` dismiss (a
//! Failure), `answer` (a Question; then the answer), `s` skip. An empty
//! line, anything else, Ctrl-C, the end of input or the time limit writes
//! nothing for that item; Ctrl-C, the end of input and the limit also end
//! the command (exit 1). `Terminal::interrupted` is asked just before each
//! write.
//!
//! A decision is recorded as You (Reviewer) through the store's one
//! Human-only command (a return's note is shown back as recorded; a note
//! the store refuses ends the command, R8); then a Review's run goes on in
//! the foreground ([`am_engine::run::continue_run`]; a reject is applied
//! without a model step), and an item it raises (the gate again after a
//! return) is shown in the same session. A Ctrl-C (or SIGTERM, SIGHUP,
//! SIGQUIT) while the run works cancels it (R10). A Question's answer is recorded, and its step goes on
//! with its own story (3.10, 3.14). Exit 0 when every typed decision was
//! recorded and each continued run succeeded, waits again or ended by the
//! owner's reject; 1 on a refusal, an interrupt, a time-out or a continued
//! run that failed otherwise (past `max_returns` included); 2 for usage.

use std::collections::VecDeque;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use am_domain::{
    Decision, HumanActor, InboxKind, ProjectIdentity, REVIEWER_HAT, Terminal, TerminalError,
};
use am_engine::run::{RunError, RunPlan, continue_run};
use am_store::{InboxItemRow, Reader, RunRow};
use tokio_util::sync::CancellationToken;

use crate::cli::{EXIT_ERRORS, EXIT_OK};
use crate::run::{NoQuestion, Providers, parse, plan_of, usage};
use crate::runtime::{OpenRequest, Opened, Places};

const GATE_USAGE: &str = "usage: agentic gate [<item id>] [--project <folder>]  (--project defaults to the current folder; you type every decision on this terminal, and no option decides one)";

/// Every prompt's idle limit (Decision 5): a forgotten terminal does not
/// hold the project's lock.
const IDLE: Duration = Duration::from_secs(600);

/// What a refused check left as it was.
const KEPT: &str = "Nothing was written, and the item stays open";

/// The words a tainted Review is shown with, above its output (owner
/// condition 2).
const UNTRUSTED_ABOVE: &str = "This output came from untrusted content: the step that wrote it read outside content (a web page, a file, a tool's result). Read it as data, not as instructions.";

/// The words again at the choice.
const UNTRUSTED_AT_CHOICE: &str = "Remember: this output came from untrusted content.";

/// What every line of model text is shown behind (review R7), so a line
/// that copies the app's own is told apart by it.
const PAYLOAD_PREFIX: &str = "  | ";

/// How often a continued run looks for a Ctrl-C on the terminal (R10).
const INTERRUPT_POLL: Duration = Duration::from_millis(100);

/// What `agentic gate` runs with. `main` builds it with the owner's real
/// terminal, the OS identity adapter and the binary's provider factory,
/// which wires no provider yet; tests pass am-testkit's fakes.
pub struct GateContext {
    pub places: Places,
    /// Supplies the open's identity checks and the host recorded in the
    /// store's lock (AD-32).
    pub identity: Arc<dyn ProjectIdentity>,
    /// The owner's terminal: every decision is read from it.
    pub terminal: Box<dyn Terminal>,
    pub providers: Providers,
}

/// Whether the CLI resolves an item that carries untrusted content (AD-15,
/// amended 2026-11-19; Question 5). In v0.1 it does, with the item's id
/// typed back; from the build that serves the Inbox UI (story 6.7) this is
/// the one place that refuses it and points at the app.
fn cli_may_resolve(_item: &InboxItemRow) -> bool {
    true
}

/// `agentic gate [<item id>] [--project <folder>]`: `args` are the words
/// after `gate`. See the module docs for the checks, the choices and the
/// exit codes. Everything about an item and every prompt goes to the
/// terminal; `run <id> <status>` of a continued run to `out`; refusals to
/// `err`.
pub async fn gate(
    args: &[String],
    mut ctx: GateContext,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let (only, project) = match parse(args) {
        Ok((positional, project)) => match positional.as_slice() {
            [] => (None, project),
            [id] => (Some(id.clone()), project),
            _ => return usage(err, GATE_USAGE, "gate takes at most one item id"),
        },
        Err(why) => return usage(err, GATE_USAGE, &why),
    };
    match session(only.as_deref(), &project, &mut ctx, out, err).await {
        Ok(code) => code,
        Err(words) => {
            let _ = writeln!(err, "agentic: {words}");
            EXIT_ERRORS
        }
    }
}

/// Checks 2 to 4, then the items. The open project (its marker, then the
/// store's lock) is dropped when this returns.
async fn session(
    only: Option<&str>,
    project: &Path,
    ctx: &mut GateContext,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32, String> {
    if !ctx.terminal.interactive() {
        return Err(
            "`agentic gate` runs only in an interactive terminal: stdin and stdout must both be a terminal, not a pipe, a file or another program, because every decision is yours and only you type it. Nothing was read and the project was not opened; run the command yourself in a terminal".into(),
        );
    }
    if !project.is_dir() {
        return Err(format!(
            "the project folder {} does not exist. Nothing was read",
            project.display()
        ));
    }
    no_owner_child(project)?;
    let held = || {
        crate::runtime::find_runtime(&ctx.places, project)
            .ok()
            .is_some_and(|layout| am_store::lock_is_held(&layout))
    };
    let serve_words = || {
        format!(
            "the project {} is open in another process, as `agentic serve` holds it while it runs; stop `agentic serve` (or that process), then run `agentic gate` again. Nothing was read or written",
            project.display()
        )
    };
    if held() {
        return Err(serve_words());
    }
    let req = OpenRequest {
        root: project.to_path_buf(),
        confirm_move: false,
        take_over: false,
    };
    let opened = match crate::runtime::open_project(
        &ctx.places,
        ctx.identity.as_ref(),
        &req,
        &mut NoQuestion,
    ) {
        Ok(opened) => opened,
        Err(_) if held() => return Err(serve_words()),
        Err(e) => {
            return Err(format!(
                "cannot open the project {}: {e}. Nothing was read or written",
                project.display()
            ));
        }
    };
    for line in &opened.lines {
        let _ = writeln!(out, "agentic: project {}: {line}", project.display());
    }
    let root = project
        .canonicalize()
        .unwrap_or_else(|_| project.to_path_buf());
    let mut gate = Gate {
        ctx,
        opened: &opened,
        root: &root,
        project,
        out,
        err,
        code: EXIT_OK,
    };
    gate.decide_all(only).await
}

/// Check 3: no child that runs as the owner is enabled in the project's
/// files (AD-13, AD-15); a file with an error fails closed.
fn no_owner_child(project: &Path) -> Result<(), String> {
    let found = am_config::owner_children(project);
    if !found.enabled.is_empty() {
        return Err(format!(
            "`agentic gate` decides nothing from the command line while {}: a program that runs as you could type here. Decide the items in the app's Inbox instead (`agentic open --item <id>` arrives with the app's UI), or remove or disable it first. Nothing was read or written",
            found.enabled.join("; ")
        ));
    }
    if found.broken.is_empty() {
        return Ok(());
    }
    Err(format!(
        "{} {}, so `agentic gate` cannot tell whether the project enables a program that runs as you, and decides nothing from the command line. Nothing was read or written; run `agentic validate {}` to see each error with its fix",
        found.broken.join(", "),
        if found.broken.len() == 1 {
            "has an error"
        } else {
            "have errors"
        },
        project.display()
    ))
}

/// Text from a model (or typed) as the terminal shows it in one line: every
/// hidden character (a control character but newline and tab, a format,
/// separator or tag character; `am_domain::text`) written as `\u{XXXX}`,
/// so it never drives, hides or reorders what the owner reads (Decision
/// 5, review R7).
fn neutralised(text: &str) -> String {
    am_domain::text::escape_hidden(text).0
}

/// How many columns a payload segment holds after its prefix (review S3).
const SEGMENT_COLUMNS: usize = 72;

/// What one tab is shown as (review S3): exactly four spaces.
const TAB: &str = "    ";

/// The columns `c` takes: 2 in the common wide ranges (CJK, Hangul,
/// fullwidth forms, the wide emoji blocks), 1 otherwise (review S3). A
/// terminal narrower than the prefix plus 72 columns can still wrap a
/// segment (a residual, recorded in the plan).
fn columns(c: char) -> usize {
    match u32::from(c) {
        0x1100..=0x115F
        | 0x2E80..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE30..=0xFE4F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1F64F
        | 0x1F900..=0x1F9FF
        | 0x20000..=0x3FFFD => 2,
        _ => 1,
    }
}

/// Text as the terminal shows it, line by line (reviews R7, S3): every
/// hidden character escaped as `\u{XXXX}`, each tab as four spaces, a run
/// of blank lines (empty after `trim`) shown as one, and each line cut
/// into segments of at most [`SEGMENT_COLUMNS`] columns, every segment
/// behind [`PAYLOAD_PREFIX`]; an escape is never cut between two segments.
/// The lines, and how many characters were escaped.
fn segmented(text: &str) -> (String, usize) {
    let mut out = String::new();
    let mut hidden = 0;
    let mut after_blank = false;
    for line in text.split('\n') {
        let blank = line.trim().is_empty();
        if blank && after_blank {
            continue;
        }
        after_blank = blank;
        // The pieces a segment is made of: an escape is one piece.
        let mut pieces: Vec<(String, usize)> = Vec::new();
        for c in line.chars() {
            if c == '\t' {
                pieces.extend(TAB.chars().map(|space| (space.to_string(), 1)));
            } else if am_domain::text::is_hidden(c) {
                hidden += 1;
                let escape = format!("\\u{{{:04X}}}", u32::from(c));
                let width = escape.len();
                pieces.push((escape, width));
            } else {
                pieces.push((c.to_string(), columns(c)));
            }
        }
        let mut segment = String::new();
        let mut used = 0;
        for (piece, width) in pieces {
            if used + width > SEGMENT_COLUMNS && used > 0 {
                out.push_str(PAYLOAD_PREFIX);
                out.push_str(&segment);
                out.push('\n');
                segment.clear();
                used = 0;
            }
            segment.push_str(&piece);
            used += width;
        }
        out.push_str(PAYLOAD_PREFIX);
        out.push_str(&segment);
        out.push('\n');
    }
    (out, hidden)
}

/// Model text as the terminal shows it ([`segmented`]), then the end of
/// the output and, when any character was escaped, one line saying how
/// many.
fn payload_block(text: &str) -> String {
    let (mut out, hidden) = segmented(text);
    out.push_str("  --- end of the output ---\n");
    if hidden > 0 {
        out.push_str(&format!(
            "  {hidden} hidden or control characters in this output are shown as \\u{{XXXX}}\n"
        ));
    }
    out
}

/// One line of words read from a row (an item's title or body; review
/// S7): hidden characters escaped as in a payload, and a newline or a tab
/// too, so it stays one line.
fn one_line(text: &str) -> String {
    neutralised(text)
        .replace('\n', "\\u{000A}")
        .replace('\t', "\\u{0009}")
}

/// The order items are shown in: Review, Question, Failure, then the kinds
/// this version does not resolve; oldest first within each.
fn urgency(kind: InboxKind) -> u8 {
    match kind {
        InboxKind::Review => 0,
        InboxKind::Question => 1,
        InboxKind::Failure => 2,
        _ => 3,
    }
}

/// The story a kind's decisions arrive with (Question 3).
fn later_story(kind: InboxKind) -> Option<&'static str> {
    match kind {
        InboxKind::Approval => Some("4.8"),
        InboxKind::Draft => Some("3.6"),
        InboxKind::BudgetAsk => Some("4.3"),
        InboxKind::Proposal => Some("4.10"),
        InboxKind::Review | InboxKind::Question | InboxKind::Failure => None,
    }
}

/// How one item ended.
enum Next {
    /// Go on with the next item.
    Go,
    /// End the command with this code (an interrupt, a time-out, the end of
    /// input, a refusal).
    Stop(i32),
}

/// One `agentic gate` session on an open project.
struct Gate<'a> {
    ctx: &'a mut GateContext,
    opened: &'a Opened,
    root: &'a Path,
    project: &'a Path,
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
    /// The exit code so far.
    code: i32,
}

impl Gate<'_> {
    fn show(&mut self, text: &str) {
        self.ctx.terminal.show(text);
    }

    /// One typed line, within the idle limit; `Err` is the code the command
    /// ends with, after saying why. What was typed ahead of the prompt is
    /// dropped first (review R9), and only a line ended by Return answers
    /// (R11: a partial line then the end of input is `Closed`).
    fn ask(&mut self, prompt: &str) -> Result<String, i32> {
        self.ctx.terminal.discard_typed_ahead();
        self.show(prompt);
        match self.ctx.terminal.read_line(Some(IDLE)) {
            Ok(line) => Ok(line),
            Err(e) => {
                let why = match e {
                    TerminalError::Interrupted => "interrupted (Ctrl-C)".to_string(),
                    TerminalError::TimedOut => "nothing was typed within 10 minutes".to_string(),
                    TerminalError::Closed => "the terminal's input ended".to_string(),
                    TerminalError::Io(why) => format!("the terminal cannot be read: {why}"),
                };
                self.show("\n");
                let _ = writeln!(
                    self.err,
                    "agentic: gate: {why}; nothing was written for this item"
                );
                Err(EXIT_ERRORS)
            }
        }
    }

    /// Asked just before a write: a Ctrl-C that no read reported ends the
    /// command with nothing written.
    fn interrupted(&mut self) -> bool {
        if self.ctx.terminal.interrupted() {
            let _ = writeln!(
                self.err,
                "agentic: gate: interrupted (Ctrl-C); nothing was written for this item"
            );
            return true;
        }
        false
    }

    fn reader(&self) -> Result<Reader, String> {
        self.opened
            .store
            .reader()
            .map_err(|e| format!("cannot read the project's store: {e}"))
    }

    /// The open items, in the order they are shown (one, when named).
    fn open_items(&self, only: Option<&str>) -> Result<VecDeque<String>, String> {
        let reader = self.reader()?;
        if let Some(id) = only {
            return match reader.inbox_item(id).map_err(|e| e.to_string())? {
                Some(item) if item.status == "open" => Ok(VecDeque::from([item.id])),
                Some(item) => Err(format!(
                    "Inbox item {id} is resolved already ({} by {}); nothing was written",
                    item.decision.map(Decision::as_str).unwrap_or("?"),
                    item.resolver()
                        .map(|by| by.describe())
                        .unwrap_or_else(|| "an unknown resolver".into())
                )),
                None => Err(format!(
                    "this project has no Inbox item {id}; nothing was written"
                )),
            };
        }
        let mut items = reader.open_items().map_err(|e| e.to_string())?;
        items.sort_by(|a, b| {
            (urgency(a.kind), &a.created_at).cmp(&(urgency(b.kind), &b.created_at))
        });
        Ok(items.into_iter().map(|i| i.id).collect())
    }

    async fn decide_all(&mut self, only: Option<&str>) -> Result<i32, String> {
        let mut queue = self.open_items(only)?;
        if queue.is_empty() {
            self.show("No Inbox item is open.\n");
            return Ok(EXIT_OK);
        }
        while let Some(id) = queue.pop_front() {
            let reader = self.reader()?;
            let Some(item) = reader.inbox_item(&id).map_err(|e| e.to_string())? else {
                continue;
            };
            if item.status != "open" {
                continue;
            }
            match self.decide(&reader, &item, &mut queue).await? {
                Next::Go => {}
                Next::Stop(code) => return Ok(code),
            }
        }
        Ok(self.code)
    }

    /// Shows `item` and reads one decision on it.
    async fn decide(
        &mut self,
        reader: &Reader,
        item: &InboxItemRow,
        queue: &mut VecDeque<String>,
    ) -> Result<Next, String> {
        let run = reader
            .run(&item.run_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Inbox item {} names no run", item.id))?;
        let steps = reader.steps(&item.run_id).map_err(|e| e.to_string())?;
        let (key, attempt) = steps
            .iter()
            .find(|s| s.id == item.step_run_id)
            .map(|s| (s.step_key.clone(), s.attempt))
            .unwrap_or_else(|| ("?".into(), 0));
        let mut text = format!(
            "\n{} item {}\n  run {}  workflow {}  step `{key}` attempt {attempt}\n  {}\n  {}\n",
            item.kind.title(),
            item.id,
            run.id,
            run.workflow,
            one_line(&item.title),
            one_line(&item.body)
        );
        let tainted = item.taint.is_tainted();
        if item.kind == InboxKind::Review {
            let returns = reader
                .returns_at(&item.run_id, &key)
                .map_err(|e| e.to_string())?;
            text.push_str(&format!(
                "  returns so far: {returns} of {}\n",
                item.max_returns.unwrap_or(0)
            ));
            if tainted {
                text.push_str(&format!("  {UNTRUSTED_ABOVE}\n"));
            }
            if let Some(payload) = item
                .payload
                .as_ref()
                .and_then(|id| steps.iter().find(|s| &s.id == id))
            {
                text.push_str(&format!(
                    "  --- the output of step `{}` attempt {} ---\n{}",
                    payload.step_key,
                    payload.attempt,
                    payload_block(payload.output.as_deref().unwrap_or(""))
                ));
            }
        }
        self.show(&text);
        if let Some(story) = later_story(item.kind) {
            self.show(&format!(
                "A {} item is decided from story {story} on; it is skipped.\n",
                item.kind.title()
            ));
            return Ok(Next::Go);
        }
        if !cli_may_resolve(item) {
            self.show("Decide this item in the app's Inbox; it is skipped.\n");
            return Ok(Next::Go);
        }
        let prompt = match item.kind {
            InboxKind::Review => {
                let mut p = String::new();
                if tainted {
                    p.push_str(UNTRUSTED_AT_CHOICE);
                    p.push('\n');
                }
                p.push_str(
                    "Type a to approve, r to return it with a note, j to reject, or s to skip: ",
                );
                p
            }
            InboxKind::Question => "Type answer to answer it, or s to skip: ".to_string(),
            _ => "Type d to dismiss it, or s to skip: ".to_string(),
        };
        let choice = match self.ask(&prompt) {
            Ok(line) => line.trim().to_string(),
            Err(code) => return Ok(Next::Stop(code)),
        };
        match (item.kind, choice.as_str()) {
            (_, "s") => {
                self.show("Skipped; nothing was written.\n");
                Ok(Next::Go)
            }
            (InboxKind::Review, "a") => self.approve(item, &run, queue).await,
            (InboxKind::Review, "r") => self.return_it(reader, item, &run, &key, queue).await,
            (InboxKind::Review, "j") => self.reject(reader, item, &key),
            (InboxKind::Question, "answer") => self.answer(item, &key),
            (InboxKind::Failure, "d") => self.dismiss(item),
            (_, "") => {
                self.show("An empty line decides nothing; nothing was written.\n");
                Ok(Next::Go)
            }
            (_, other) => {
                self.show(&format!(
                    "`{}` is not one of the choices; nothing was written.\n",
                    neutralised(other)
                ));
                Ok(Next::Go)
            }
        }
    }

    /// Check 5 (Question 2): the run goes on only on the snapshot it
    /// pinned (read as the server's pin check reads it,
    /// [`crate::serving::current_snapshot`]); its plan, with a provider for
    /// every step still to run; and the run could go on (review R2: what
    /// `continue_run` would refuse is refused here, before anything is
    /// written).
    fn plan_for(&self, run: &RunRow) -> Result<RunPlan, String> {
        let history = am_store::StoreConfigHistory::new(&self.opened.store)
            .map_err(|e| format!("{e}. {KEPT}"))?;
        let snapshot = crate::serving::current_snapshot(
            &self.ctx.places,
            self.root,
            self.project,
            &history,
            KEPT,
        )?;
        let enabled = snapshot.owner_children();
        if !enabled.is_empty() {
            return Err(format!(
                "the project's config enables {}: a program that runs as you could type here. Decide the item in the app's Inbox instead. {KEPT}",
                enabled.join("; ")
            ));
        }
        let now = snapshot.hash().to_string();
        if run.config_hash.as_deref() != Some(now.as_str()) {
            return Err(format!(
                "the project's config is not the snapshot run {} pinned: the run pinned {}, and the project's files now give {now}; a run goes on only on the config it started with (AD-16). {KEPT}; put the files back as they were when the run started, or reject the item (reject and dismiss still work)",
                run.id,
                run.config_hash.as_deref().unwrap_or("no snapshot")
            ));
        }
        let plan = plan_of(&snapshot, &run.workflow, &self.ctx.providers, KEPT)?;
        let reader = self.reader()?;
        match am_engine::run::ready_to_go_on(&reader, &run.id) {
            Ok(_) => Ok(plan),
            Err(e) => Err(format!("{e}. {KEPT}")),
        }
    }

    /// Records the person's decision as You (Reviewer), the one Human-only
    /// command (AD-15).
    fn record(
        &mut self,
        item: &InboxItemRow,
        decision: Decision,
        note: Option<String>,
    ) -> Result<(), String> {
        self.opened
            .store
            .writer()
            .resolve_inbox_item(
                HumanActor::new("owner", REVIEWER_HAT),
                &item.id,
                decision,
                note,
            )
            .map_err(|e| format!("the decision was not recorded: {e}"))
    }

    /// `a`: approve, then the run goes on in the foreground.
    async fn approve(
        &mut self,
        item: &InboxItemRow,
        run: &RunRow,
        queue: &mut VecDeque<String>,
    ) -> Result<Next, String> {
        let plan = self.plan_for(run)?;
        if item.taint.is_tainted() {
            // Owner condition 2: only the exact id (spaces around it
            // trimmed) approves output from untrusted content.
            let typed = match self.ask(&format!(
                "Type the item id {} to approve output from untrusted content: ",
                item.id
            )) {
                Ok(line) => line,
                Err(code) => return Ok(Next::Stop(code)),
            };
            if typed.trim() != item.id {
                self.show("That is not this item's id; nothing was written.\n");
                return Ok(Next::Go);
            }
        }
        if self.interrupted() {
            return Ok(Next::Stop(EXIT_ERRORS));
        }
        self.record(item, Decision::Approve, None)?;
        self.show("Approved by You (Reviewer).\n");
        self.go_on(run, &plan, queue).await
    }

    /// `r`: return with a note (past the maximum, after a warning).
    async fn return_it(
        &mut self,
        reader: &Reader,
        item: &InboxItemRow,
        run: &RunRow,
        key: &str,
        queue: &mut VecDeque<String>,
    ) -> Result<Next, String> {
        let plan = self.plan_for(run)?;
        let max = item.max_returns.unwrap_or(0);
        let returns = reader
            .returns_at(&item.run_id, key)
            .map_err(|e| e.to_string())?;
        if returns + 1 > max {
            // Row 6: the last return ends the run.
            let typed = match self.ask(&format!(
                "This would be return {} at step `{key}`, past its max_returns of {max}: the run will stop with a Failure item. Type y to return it anyway: ",
                returns + 1
            )) {
                Ok(line) => line,
                Err(code) => return Ok(Next::Stop(code)),
            };
            if typed.trim() != "y" {
                self.show("Not returned; nothing was written.\n");
                return Ok(Next::Go);
            }
        }
        let note = match self.ask(&format!(
            "Type the note for the step that wrote it (one line, at most {} characters): ",
            am_store::NOTE_MAX_CHARS
        )) {
            Ok(line) => line,
            Err(code) => return Ok(Next::Stop(code)),
        };
        if self.interrupted() {
            return Ok(Next::Stop(EXIT_ERRORS));
        }
        if let Err(why) = self.record(item, Decision::Return, Some(note.clone())) {
            // R8: a note the store refuses ends the command.
            let _ = writeln!(self.err, "agentic: gate: {why}");
            return Ok(Next::Stop(EXIT_ERRORS));
        }
        // R9: the note as recorded, so a cut-off paste shows.
        self.show(&format!(
            "Returned by You (Reviewer), with the note:\n{}",
            segmented(&note).0
        ));
        self.go_on(run, &plan, queue).await
    }

    /// `j`, then `reject`: the gate and the run fail (Question 7); no model
    /// step, so no config check.
    fn reject(&mut self, reader: &Reader, item: &InboxItemRow, key: &str) -> Result<Next, String> {
        let typed = match self.ask(&format!(
            "Type reject to reject it: step `{key}` fails and the run stops: "
        )) {
            Ok(line) => line,
            Err(code) => return Ok(Next::Stop(code)),
        };
        if typed.trim() != "reject" {
            self.show("Not rejected; nothing was written.\n");
            return Ok(Next::Go);
        }
        if self.interrupted() {
            return Ok(Next::Stop(EXIT_ERRORS));
        }
        self.record(item, Decision::Reject, None)?;
        let applied =
            am_engine::gate::apply_resolution(&self.opened.store.writer(), reader, &item.id)
                .map_err(|e| format!("the reject was recorded, and applying it failed: {e}"))?;
        let _ = writeln!(self.out, "run {} failed", applied.run_id);
        let _ = writeln!(
            self.out,
            "agentic: run {}: {}",
            applied.run_id,
            applied.reason.unwrap_or_else(|| "rejected".into())
        );
        Ok(Next::Go)
    }

    /// `d`: a Failure is dismissed; nothing else changes (Question 3).
    fn dismiss(&mut self, item: &InboxItemRow) -> Result<Next, String> {
        if self.interrupted() {
            return Ok(Next::Stop(EXIT_ERRORS));
        }
        self.record(item, Decision::Dismiss, None)?;
        self.show("Dismissed by You (Reviewer).\n");
        Ok(Next::Go)
    }

    /// `answer`: a Question's answer is stored once; its step goes on with
    /// its own story, not here (Question 3).
    fn answer(&mut self, item: &InboxItemRow, key: &str) -> Result<Next, String> {
        let text = match self.ask(&format!(
            "Type the answer (one line, at most {} characters): ",
            am_store::NOTE_MAX_CHARS
        )) {
            Ok(line) => line,
            Err(code) => return Ok(Next::Stop(code)),
        };
        if self.interrupted() {
            return Ok(Next::Stop(EXIT_ERRORS));
        }
        if let Err(why) = self.record(item, Decision::Answer, Some(text)) {
            // R8: an answer the store refuses ends the command.
            let _ = writeln!(self.err, "agentic: gate: {why}");
            return Ok(Next::Stop(EXIT_ERRORS));
        }
        self.show(&format!(
            "Answered by You (Reviewer). Step `{key}` goes on with the stories that ask questions (3.10, 3.14); `agentic gate` does not run it.\n"
        ));
        Ok(Next::Go)
    }

    /// The run of a Review the owner approved or returned goes on in the
    /// foreground; an item it raises is shown next.
    async fn go_on(
        &mut self,
        run: &RunRow,
        plan: &RunPlan,
        queue: &mut VecDeque<String>,
    ) -> Result<Next, String> {
        // R10: a Ctrl-C (or SIGTERM, SIGHUP, SIGQUIT) reported by the
        // terminal while the run works cancels it.
        let cancel = CancellationToken::new();
        let opened = self.opened;
        let terminal = &mut self.ctx.terminal;
        let going = continue_run(&opened.store, &run.id, plan, cancel.clone());
        tokio::pin!(going);
        let mut poll = tokio::time::interval(INTERRUPT_POLL);
        let mut interrupted = false;
        let outcome = loop {
            tokio::select! {
                done = &mut going => break done,
                _ = poll.tick(), if !interrupted => {
                    if terminal.interrupted() {
                        interrupted = true;
                        cancel.cancel();
                    }
                }
            }
        };
        if interrupted {
            let _ = writeln!(
                self.err,
                "agentic: gate: interrupted (Ctrl-C): the run of the decision was stopped"
            );
            self.code = EXIT_ERRORS;
        }
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(RunError::Stopped {
                run_id,
                status,
                reason,
                source,
            }) => {
                let _ = writeln!(self.out, "run {run_id} {status}");
                if let Some(reason) = reason {
                    let _ = writeln!(self.err, "agentic: run {run_id}: {reason}");
                }
                return Err(format!(
                    "run {run_id}: the store refused a command while the run was journaled: {source}. The run is recorded as {status}; `agentic status` shows it"
                ));
            }
            Err(e) => {
                return Err(format!(
                    "the decision was recorded, and the run did not go on: {e}"
                ));
            }
        };
        let _ = writeln!(self.out, "run {} {}", outcome.run_id, outcome.status);
        match outcome.status.as_str() {
            "succeeded" => {}
            "waiting_human" => {
                if let Some(item) = outcome.item {
                    let _ = writeln!(self.out, "Review item {item} waits for you in the Inbox");
                    // S1: after an interrupt nothing more is shown.
                    if !interrupted {
                        queue.push_front(item);
                    }
                }
            }
            _ => {
                if let Some(reason) = &outcome.reason {
                    let _ = writeln!(
                        self.err,
                        "agentic: run {} {}: {reason}",
                        outcome.run_id, outcome.status
                    );
                }
                if let Some(item) = &outcome.item {
                    let _ = writeln!(
                        self.err,
                        "agentic: run {}: Failure item {item} is in the Inbox",
                        outcome.run_id
                    );
                }
                self.code = EXIT_ERRORS;
            }
        }
        // S1: an interrupt ends the command once the run's outcome is
        // reported: no later item is shown, and nothing more is read.
        if interrupted {
            return Ok(Next::Stop(EXIT_ERRORS));
        }
        Ok(Next::Go)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_text_never_drives_the_terminal() {
        let shown = neutralised("a\u{1b}[31mb\u{7}c\rd\u{9b}e\u{7f}f\n\tg\u{202E}");
        assert_eq!(
            shown,
            "a\\u{001B}[31mb\\u{0007}c\\u{000D}d\\u{009B}e\\u{007F}f\n\tg\\u{202E}"
        );
    }

    #[test]
    fn every_payload_line_is_prefixed_and_blank_runs_collapse() {
        assert_eq!(
            payload_block("a\n\n\n\nb\u{200B}\tc"),
            "  | a\n  | \n  | b\\u{200B}    c\n  --- end of the output ---\n  1 hidden or control characters in this output are shown as \\u{XXXX}\n"
        );
    }

    #[test]
    fn segments_are_cut_at_72_columns_never_inside_an_escape() {
        let (shown, _) = segmented(&format!("{}\u{202E}", "x".repeat(70)));
        assert_eq!(shown, format!("  | {}\n  | \\u{{202E}}\n", "x".repeat(70)));
        let (shown, _) = segmented(&"\u{4E00}".repeat(37));
        let widths: Vec<usize> = shown.lines().map(|l| l.chars().count() - 4).collect();
        assert_eq!(widths, vec![36, 1]);
    }

    #[test]
    fn reviews_come_first() {
        let mut kinds = vec![InboxKind::Failure, InboxKind::Question, InboxKind::Review];
        kinds.sort_by_key(|k| urgency(*k));
        assert_eq!(
            kinds,
            vec![InboxKind::Review, InboxKind::Question, InboxKind::Failure]
        );
    }
}
