//! The run driver (epic 3 story 1; AD-8, AD-6, AD-16): a plan of steps in
//! order, one model turn per step, every transition committed before the
//! effect it authorises.
//!
//! The plan is plain types: am-app maps a config snapshot to it and
//! resolves each step's provider (AD-4; this crate reads no config). The
//! driver then writes, each command with its audit entry in one
//! transaction:
//!
//! 1. the run (`running`, with the plan's snapshot hash pinned), as the
//!    Actor who started it;
//! 2. per step, as `System(engine)`: the step's start, then the model
//!    call's reservation, both before the provider is called; after the
//!    call, the call's settlement and the step's finish together;
//! 3. the run's finish.
//!
//! One turn per step: the request is the step's system instructions, its
//! instruction as the one user part, its model and output limit, and no
//! tools. A turn that ends `EndTurn` stores its text as the step's output.
//! A provider error, a tool request (no tools exist yet), a reply cut off
//! at its output limit or a cancelled call fails the step and the run, with
//! the reason in words; the failed turn's text is not stored, and later
//! steps stay queued.
//!
//! Not here: prompt assembly beyond the two parts (story 5), tools (story
//! 3), amounts (story 7: the reservation is zero and settles as the store's
//! rules say), retries, fallback and queueing (story 8), resume (story 9).
//! The spike's resumable driver stays in [`crate::journal`].

use std::sync::Arc;

use am_domain::{
    Actor, ChatOutcome, ChatRequest, EventSink, Instruction, LlmProvider, Message, Part,
    ProviderError, Role, StopReason, StreamEvent,
};
use am_store::{Store, StoreError, WriteCommand, new_id};
use tokio_util::sync::CancellationToken;

use crate::TokenSignal;

/// The rate-snapshot id of a zero reservation: no price was read (amounts
/// arrive with the cost ledger, story 7).
const NO_RATE_SNAPSHOT: &str = "none";

/// One step: one model turn on `provider`.
#[derive(Clone)]
pub struct StepPlan {
    /// The step's key in the run (its id in the workflow).
    pub key: String,
    /// The seat's instructions, as the request's system part.
    pub system: Vec<Instruction>,
    /// The step's instruction, as the one user part.
    pub instruction: Instruction,
    /// The model id on `provider`'s Account.
    pub model: String,
    pub max_output_tokens: u32,
    pub provider: Arc<dyn LlmProvider>,
}

/// A run to make: the workflow's steps in order, built from one config
/// snapshot.
#[derive(Clone)]
pub struct RunPlan {
    pub workflow: String,
    /// The hash of the config snapshot the plan was built from (AD-16),
    /// pinned on the run row.
    pub config_hash: String,
    pub steps: Vec<StepPlan>,
}

/// How a run ended. A failed run is an outcome, not an error: the caller
/// still has its id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    pub run_id: String,
    /// `succeeded` or `failed`.
    pub status: String,
    /// Why the run failed, in words; `None` when it succeeded.
    pub reason: Option<String>,
}

/// Why a run could not be made or journaled.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("the workflow {0} has no steps, so there is nothing to run")]
    NoSteps(String),
    /// The run could not be created: the journal holds nothing of it.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The store failed a command after the run was created. The engine
    /// tried once to record how the run ended: the status it was about to
    /// record when every started step had finished, `failed` (with the
    /// open step) when one had not. `status` is what the journal holds
    /// now: that status, or `running` when the store refused that too.
    /// `reason` is why a step failed, when one did before the store erred.
    #[error("run {run_id} stopped ({status}): {source}")]
    Stopped {
        run_id: String,
        status: String,
        reason: Option<String>,
        source: Box<StoreError>,
    },
}

fn engine() -> Actor {
    Actor::System {
        component: "engine".into(),
    }
}

struct NullSink;

impl EventSink for NullSink {
    fn on_event(&self, _event: StreamEvent) {}
}

/// The one request a step makes.
fn request_of(step: &StepPlan) -> ChatRequest {
    ChatRequest {
        model: step.model.clone(),
        system: step.system.clone(),
        messages: vec![Message {
            role: Role::User,
            parts: vec![Part::Instruction(step.instruction.clone())],
        }],
        tools: vec![],
        max_output_tokens: step.max_output_tokens,
    }
}

/// Why a turn did not finish its step, in words; `None` for a finished turn
/// (`EndTurn` with no tool request).
fn failure_of(step: &StepPlan, turn: &Result<ChatOutcome, ProviderError>) -> Option<String> {
    let out = match turn {
        Ok(out) => out,
        Err(e) => return Some(format!("the model call failed: {e}")),
    };
    match out.stop {
        StopReason::Cancelled => Some("the model call was cancelled before it finished".into()),
        StopReason::MaxTokens => Some(format!(
            "the reply was cut off at the step's output limit of {} tokens (max_tokens)",
            step.max_output_tokens
        )),
        StopReason::ToolUse => Some(tool_words(out)),
        _ if !out.tool_calls.is_empty() => Some(tool_words(out)),
        StopReason::EndTurn => None,
        StopReason::Other => Some("the model stopped before it finished its turn".into()),
    }
}

fn tool_words(out: &ChatOutcome) -> String {
    let names: Vec<String> = out
        .tool_calls
        .iter()
        .map(|c| format!("`{}`", c.name))
        .collect();
    let asked = if names.is_empty() {
        "a tool".to_string()
    } else {
        names.join(", ")
    };
    format!("the model asked for {asked}, and tools are not available in this version")
}

/// The function that executes one write command (the store's writer).
type Exec<'a> = &'a (dyn Fn(Actor, WriteCommand) -> Result<(), StoreError> + Send + Sync);

/// How far a run got, for the cleanup after a store error.
#[derive(Default)]
struct Progress {
    /// The step run that is started and not finished.
    open_step: Option<String>,
    /// Why a step failed, from the moment its turn's failure is known.
    reason: Option<String>,
    /// The status the engine is about to record for the run, once every
    /// started step has finished: `succeeded` after the last step, `failed`
    /// after a failed one.
    finishing: Option<&'static str>,
}

impl Progress {
    /// What the cleanup records for the run: what the engine was about to
    /// record, or `failed` when a step did not finish.
    fn to_record(&self) -> &'static str {
        match (&self.open_step, self.finishing) {
            (None, Some(status)) => status,
            _ => "failed",
        }
    }
}

/// Creates the run for `plan` as `started_by` and drives it to its end.
/// `origin` says what started it (`manual`). See the module docs for the
/// order of the commits.
///
/// A store error once the run exists never leaves it `running` unsaid: the
/// engine tries once to record how the run ended (see
/// [`RunError::Stopped`]) and returns that error with the run's id, the
/// status the journal now holds and the failed step's reason, if any. (A
/// call the store could not settle stays as the journal has it.)
pub async fn run_plan(
    store: &Store,
    started_by: Actor,
    origin: &str,
    plan: &RunPlan,
    cancel: CancellationToken,
) -> Result<RunOutcome, RunError> {
    let writer = store.writer();
    let exec = move |actor: Actor, cmd: WriteCommand| writer.exec(actor, cmd);
    run_on(store, &exec, started_by, origin, plan, cancel).await
}

/// [`run_plan`], with every write command executed by `exec` (the seam for
/// the tests of a store that fails a command).
async fn run_on(
    store: &Store,
    exec: Exec<'_>,
    started_by: Actor,
    origin: &str,
    plan: &RunPlan,
    cancel: CancellationToken,
) -> Result<RunOutcome, RunError> {
    if plan.steps.is_empty() {
        return Err(RunError::NoSteps(plan.workflow.clone()));
    }
    let run_id = new_id();
    exec(
        started_by,
        WriteCommand::CreateRun {
            run_id: run_id.clone(),
            workflow: plan.workflow.clone(),
            origin: origin.into(),
            steps: plan.steps.iter().map(|s| s.key.clone()).collect(),
            config_hash: Some(plan.config_hash.clone()),
        },
    )?;
    let mut progress = Progress::default();
    match drive(store, exec, &run_id, plan, cancel, &mut progress).await {
        Ok(status) => Ok(RunOutcome {
            run_id,
            status: status.into(),
            reason: progress.reason,
        }),
        Err(source) => {
            let to_record = progress.to_record();
            if let Some(step_run_id) = progress.open_step {
                let _ = exec(
                    engine(),
                    WriteCommand::FinishStep {
                        step_run_id,
                        status: "failed".into(),
                        output: None,
                    },
                );
            }
            let recorded = exec(
                engine(),
                WriteCommand::FinishRun {
                    run_id: run_id.clone(),
                    status: to_record.into(),
                },
            );
            Err(RunError::Stopped {
                run_id,
                status: if recorded.is_ok() {
                    to_record
                } else {
                    "running"
                }
                .into(),
                reason: progress.reason,
                source: Box::new(source),
            })
        }
    }
}

/// Drives the created run `run_id` through `plan`'s steps to its final
/// status. `progress` says how far it got, whatever this returns: the open
/// step, a failed step's reason, the status about to be recorded.
async fn drive(
    store: &Store,
    exec: Exec<'_>,
    run_id: &str,
    plan: &RunPlan,
    cancel: CancellationToken,
    progress: &mut Progress,
) -> Result<&'static str, StoreError> {
    // The store minted the step runs' ids: read them back, in order.
    let rows = store.reader()?.steps(run_id)?;
    if rows.len() != plan.steps.len() {
        return Err(StoreError::Invalid(format!(
            "the journal of run {run_id} holds {} steps, and its plan has {}",
            rows.len(),
            plan.steps.len()
        )));
    }
    let finish_run = |status: &str| {
        exec(
            engine(),
            WriteCommand::FinishRun {
                run_id: run_id.into(),
                status: status.into(),
            },
        )
    };
    for (step, row) in plan.steps.iter().zip(&rows) {
        // Committed before the call (AD-8): the step's start, then the
        // call's reservation.
        exec(
            engine(),
            WriteCommand::StartStep {
                step_run_id: row.id.clone(),
            },
        )?;
        progress.open_step = Some(row.id.clone());
        let call_id = new_id();
        exec(
            engine(),
            WriteCommand::ReserveCall {
                step_run_id: row.id.clone(),
                call_id: call_id.clone(),
                call_seq: 1,
                reserved_micro_usd: 0,
                rate_snapshot: NO_RATE_SNAPSHOT.into(),
            },
        )?;
        let turn = step
            .provider
            .stream(
                request_of(step),
                TokenSignal::signal(cancel.clone()),
                Arc::new(NullSink),
            )
            .await;
        let failed = match failure_of(step, &turn) {
            Some(why) => {
                progress.reason = Some(format!("step `{}` failed: {why}", step.key));
                true
            }
            None => false,
        };
        // Whatever usage the provider reported is settled, finished turn or
        // not; a call with none settles at its reservation.
        let (usage, text) = match turn {
            Ok(out) => (out.usage, Some(out.text)),
            Err(_) => (None, None),
        };
        // The settlement and the step's finish commit together (AD-8).
        exec(
            engine(),
            WriteCommand::SettleAndFinishStep {
                call_id,
                usage,
                cost_micro_usd: None,
                step_run_id: row.id.clone(),
                status: if failed { "failed" } else { "succeeded" }.into(),
                // A failed turn's text is not the step's output.
                output: if failed { None } else { text },
            },
        )?;
        progress.open_step = None;
        if failed {
            progress.finishing = Some("failed");
            finish_run("failed")?;
            return Ok("failed");
        }
    }
    progress.finishing = Some("succeeded");
    finish_run("succeeded")?;
    Ok("succeeded")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plan with no steps is refused before anything is written.
    #[tokio::test]
    async fn a_plan_with_no_steps_creates_no_run() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("state.db")).unwrap();
        let plan = RunPlan {
            workflow: "wf".into(),
            config_hash: "0".repeat(64),
            steps: vec![],
        };
        let refused = run_plan(
            &store,
            Actor::System {
                component: "test".into(),
            },
            "manual",
            &plan,
            CancellationToken::new(),
        )
        .await;
        assert!(matches!(refused, Err(RunError::NoSteps(wf)) if wf == "wf"));
        assert!(store.reader().unwrap().runs().unwrap().is_empty());
    }

    // -------------------------------------------------------------- store errors
    //
    // The seam (second review of story 3.1): `run_on` takes the function
    // that executes a write command, so these tests run the driver on a
    // real store whose writer refuses the commands a test picks.

    use std::sync::atomic::{AtomicUsize, Ordering};

    use am_store::Writer;
    use am_testkit::{FakeLlm, ScriptedTurn};

    fn owner() -> Actor {
        Actor::Human {
            person: "owner".into(),
            hat: "Owner".into(),
        }
    }

    fn one_step(provider: Arc<dyn LlmProvider>) -> RunPlan {
        RunPlan {
            workflow: "wf".into(),
            config_hash: "0".repeat(64),
            steps: vec![StepPlan {
                key: "only".into(),
                system: vec![],
                instruction: Instruction::authored("Do the work."),
                model: "example-local".into(),
                max_output_tokens: 64,
                provider,
            }],
        }
    }

    fn refused() -> StoreError {
        StoreError::Invalid("the disk is full".into())
    }

    /// Runs `plan` on `store` with a writer that refuses every command for
    /// which `refuse` says so; `refuse` is given the command and how many
    /// commands of the same audit action came before it.
    async fn run_refusing(
        store: &Store,
        plan: &RunPlan,
        refuse: impl Fn(&WriteCommand, usize) -> bool + Send + Sync,
    ) -> Result<RunOutcome, RunError> {
        let writer: Writer = store.writer();
        let (finishes, others) = (AtomicUsize::new(0), AtomicUsize::new(0));
        let exec = move |actor: Actor, cmd: WriteCommand| {
            let seen = match &cmd {
                WriteCommand::FinishRun { .. } => finishes.fetch_add(1, Ordering::SeqCst),
                _ => others.fetch_add(1, Ordering::SeqCst),
            };
            if refuse(&cmd, seen) {
                return Err(refused());
            }
            writer.exec(actor, cmd)
        };
        run_on(
            store,
            &exec,
            owner(),
            "manual",
            plan,
            CancellationToken::new(),
        )
        .await
    }

    /// The last `FinishRun` fails once: every step succeeded, so the
    /// cleanup records what the run earned, `succeeded`, not `failed`, and
    /// the error says that is what the journal holds.
    #[tokio::test]
    async fn a_refused_last_finish_is_retried_with_the_status_the_run_earned() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("state.db")).unwrap();
        let plan = one_step(Arc::new(FakeLlm::new(vec![ScriptedTurn::text("done")])));

        let stopped = run_refusing(&store, &plan, |cmd, seen| {
            matches!(cmd, WriteCommand::FinishRun { .. }) && seen == 0
        })
        .await
        .expect_err("the store refused a command");

        let RunError::Stopped {
            run_id,
            status,
            reason,
            source,
        } = &stopped
        else {
            panic!("{stopped:?}");
        };
        assert_eq!(status, "succeeded");
        assert_eq!(reason, &None);
        assert!(source.to_string().contains("the disk is full"), "{source}");
        let reader = store.reader().unwrap();
        let run = reader.run(run_id).unwrap().unwrap();
        assert_eq!(run.status, "succeeded");
        assert!(run.finished_at.is_some());
        let steps = reader.steps(run_id).unwrap();
        assert_eq!(steps[0].status, "succeeded");
        assert_eq!(steps[0].output.as_deref(), Some("done"));
    }

    /// The store refuses every `FinishRun`: the run stays `running`, and
    /// the error says so instead of naming a status that was not recorded.
    #[tokio::test]
    async fn a_store_that_refuses_every_finish_leaves_the_run_running_and_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("state.db")).unwrap();
        let plan = one_step(Arc::new(FakeLlm::new(vec![ScriptedTurn::text("done")])));

        let stopped = run_refusing(&store, &plan, |cmd, _| {
            matches!(cmd, WriteCommand::FinishRun { .. })
        })
        .await
        .expect_err("the store refused a command");

        let RunError::Stopped { run_id, status, .. } = &stopped else {
            panic!("{stopped:?}");
        };
        assert_eq!(status, "running");
        let run = store.reader().unwrap().run(run_id).unwrap().unwrap();
        assert_eq!(run.status, "running");
        assert_eq!(run.finished_at, None);
    }

    /// A step failed for the provider's reason, then the store refused a
    /// command (the settlement, or the run's finish): the error keeps the
    /// step's reason beside the store's, and the run is recorded `failed`.
    #[tokio::test]
    async fn a_store_error_after_a_failed_step_keeps_the_steps_reason() {
        type Refuse = fn(&WriteCommand, usize) -> bool;
        let cases: [(&str, Refuse); 2] = [
            ("the settlement", |cmd, _| {
                matches!(cmd, WriteCommand::SettleAndFinishStep { .. })
            }),
            ("the run's first finish", |cmd, seen| {
                matches!(cmd, WriteCommand::FinishRun { .. }) && seen == 0
            }),
        ];
        for (case, refuse) in cases {
            let dir = tempfile::tempdir().unwrap();
            let store = Store::open(&dir.path().join("state.db")).unwrap();
            let failing = FakeLlm::new(vec![ScriptedTurn::fail(am_domain::ProviderError::Http {
                status: 500,
                body: "upstream is down".into(),
            })]);
            let plan = one_step(Arc::new(failing));

            let stopped = run_refusing(&store, &plan, refuse)
                .await
                .expect_err("the store refused a command");

            let RunError::Stopped {
                run_id,
                status,
                reason,
                source,
            } = &stopped
            else {
                panic!("{case}: {stopped:?}");
            };
            assert_eq!(status, "failed", "{case}");
            let reason = reason.as_deref().expect("the step's reason is kept");
            assert!(
                reason.contains("only") && reason.contains("500"),
                "{case}: {reason}"
            );
            assert!(source.to_string().contains("the disk is full"), "{case}");
            let reader = store.reader().unwrap();
            assert_eq!(
                reader.run(run_id).unwrap().unwrap().status,
                "failed",
                "{case}"
            );
            assert_eq!(reader.steps(run_id).unwrap()[0].status, "failed", "{case}");
        }
    }
}
