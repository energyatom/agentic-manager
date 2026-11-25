//! The journaled run executor (AD-8, AD-9), spike scope: a linear workflow of
//! model-call steps. Every transition commits before the side effect it
//! authorises; on restart an unfinished run resumes at its last committed
//! transition, a call without `settled` is marked lost (settled at its
//! reservation) and re-issued.
//!
//! Story 3.3 (AD-10, amended 2026-11-23): earlier steps' journaled outputs
//! reach a step as inputs, never as an `Instruction`. Each goes through
//! `TrustedInput::from_step_output` with the producing step run's taint as
//! the journal holds it (also on resume): a clean producer's output is a
//! trusted input labelled `step:<key>@<attempt>`, a tainted one's is fenced
//! `UntrustedData` with the same label and taints the step.

use std::path::Path;
use std::sync::Arc;

use am_domain::{
    Actor, ChatRequest, EventSink, Instruction, LlmProvider, Message, Part, Role, StopReason,
    StreamEvent, Usage,
};
use am_store::{
    AttemptWrites, CallPurpose, Reader, StepInput, StepRow, StoreError, WriteCommand, Writer,
    new_id,
};
use tokio_util::sync::CancellationToken;

use crate::TokenSignal;

/// Prices in micro-USD per million tokens, with the rate-snapshot id (AD-9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Price {
    pub input_micro_per_mtok: i64,
    pub output_micro_per_mtok: i64,
    pub snapshot: String,
}

impl Price {
    pub fn cost(&self, input_tokens: u64, output_tokens: u64) -> i64 {
        (input_tokens as i64 * self.input_micro_per_mtok
            + output_tokens as i64 * self.output_micro_per_mtok)
            / 1_000_000
    }
}

#[derive(Debug, Clone)]
pub struct LinearWorkflow {
    pub workflow: String,
    pub steps: Vec<String>,
    pub model: String,
    pub max_output_tokens: u32,
    pub price: Price,
    /// The system part of every step's request. The caller builds it:
    /// am-engine authors no `Instruction` (AD-10, story 3.5).
    pub system: Instruction,
    /// The step instruction, from the step's key and the workflow's name;
    /// built by the caller for the same reason.
    pub step_prompt: fn(step_key: &str, workflow: &str) -> Instruction,
}

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("model call failed: {0}")]
    Provider(String),
    #[error("run {0} not found")]
    NotFound(String),
}

fn system(component: &str) -> Actor {
    Actor::System {
        component: component.into(),
    }
}

/// Create a run with its steps queued, as `actor` (AD-24: no default).
/// Returns the run id.
pub fn create_run(
    writer: &Writer,
    actor: Actor,
    wf: &LinearWorkflow,
    origin: &str,
) -> Result<String, RunError> {
    let run_id = new_id();
    writer.exec(
        actor,
        WriteCommand::CreateRun {
            run_id: run_id.clone(),
            workflow: wf.workflow.clone(),
            origin: origin.into(),
            steps: wf.steps.clone(),
            // The spike's hard-coded workflow comes from no config snapshot.
            config_hash: None,
        },
    )?;
    Ok(run_id)
}

struct NullSink;
impl EventSink for NullSink {
    fn on_event(&self, _event: StreamEvent) {}
}

/// Drive a run to its end, resuming from whatever the journal holds.
/// Returns the run's final status.
pub async fn drive_run(
    db: &Path,
    writer: &Writer,
    provider: Arc<dyn LlmProvider>,
    wf: &LinearWorkflow,
    run_id: &str,
    cancel: CancellationToken,
) -> Result<String, RunError> {
    let reader = Reader::open(db)?;
    let run = reader
        .run(run_id)?
        .ok_or_else(|| RunError::NotFound(run_id.to_string()))?;
    if matches!(run.status.as_str(), "succeeded" | "failed" | "cancelled") {
        return Ok(run.status);
    }
    let calls = reader.calls_for_run(run_id)?;
    // The earlier steps that succeeded, as the journal holds them (their
    // outputs and their committed taint).
    let mut earlier: Vec<StepRow> = Vec::new();
    for step in reader.steps(run_id)? {
        if step.status == "succeeded" {
            earlier.push(step);
            continue;
        }
        let step_calls: Vec<_> = calls.iter().filter(|c| c.step_run_id == step.id).collect();
        for c in step_calls.iter().filter(|c| !c.settled) {
            // Resume: the in-flight call is lost; settle it at its reservation.
            writer.exec(
                system("resume"),
                WriteCommand::MarkLost {
                    call_id: c.id.clone(),
                },
            )?;
        }
        // Each earlier output is an input: trusted from a clean producer,
        // fenced from a tainted one (by its committed taint, AD-10).
        let mut inputs = Vec::new();
        let (mut trusted, mut fenced) = (Vec::new(), Vec::new());
        let mut taint = step.taint;
        for prev in &earlier {
            let label = format!("step:{}@{}", prev.step_key, prev.attempt);
            let text = prev.output.clone().unwrap_or_default();
            crate::parts::sort_output(label.clone(), text, prev.taint, &mut trusted, &mut fenced);
            taint = taint.raise(prev.taint);
            inputs.push(StepInput {
                kind: "step_output".into(),
                from_step_run: Some(prev.id.clone()),
                from_item: None,
                from_row: None,
                label,
                taint: prev.taint,
            });
        }
        if step.status == "queued" {
            writer.exec(
                system("engine"),
                WriteCommand::StartAttempt {
                    step_run_id: step.id.clone(),
                    inputs,
                    taint,
                    prompt: Vec::new(),
                    // The spike's linear workflow runs no seat and writes
                    // no table (story 3.6, the store slice).
                    writes: AttemptWrites {
                        seat: String::new(),
                        table: None,
                        document: None,
                    },
                },
            )?;
        }
        let call_seq = step_calls.iter().map(|c| c.call_seq).max().unwrap_or(0) + 1;
        let prompt = (wf.step_prompt)(&step.step_key, &wf.workflow);
        let est_input = (prompt.text().len() as u64 / 4).max(1);
        let reserved = wf.price.cost(est_input, wf.max_output_tokens as u64);
        let call_id = new_id();
        writer.exec(
            system("engine"),
            WriteCommand::ReserveCall {
                step_run_id: step.id.clone(),
                call_id: call_id.clone(),
                call_seq,
                reserved_micro_usd: reserved,
                rate_snapshot: wf.price.snapshot.clone(),
                taint,
                purpose: CallPurpose::Turn,
                prompt: Vec::new(),
            },
        )?;
        let mut parts = vec![Part::Instruction(prompt)];
        parts.extend(trusted);
        parts.extend(fenced);
        let request = ChatRequest {
            model: wf.model.clone(),
            system: vec![wf.system.clone()],
            messages: vec![Message {
                role: Role::User,
                parts,
            }],
            tools: vec![],
            max_output_tokens: wf.max_output_tokens,
        };
        let outcome = provider
            .stream(
                request,
                TokenSignal::signal(cancel.clone()),
                Arc::new(NullSink),
            )
            .await;
        match outcome {
            Ok(out) => {
                let cost = out
                    .usage
                    .map(|u: Usage| wf.price.cost(u.input_tokens, u.output_tokens));
                if out.stop == StopReason::Cancelled {
                    writer.exec(
                        system("engine"),
                        WriteCommand::SettleAndFinishStep {
                            call_id,
                            usage: out.usage,
                            cost_micro_usd: cost,
                            step_run_id: step.id.clone(),
                            status: "cancelled".into(),
                            output: None,
                        },
                    )?;
                    writer.exec(
                        system("engine"),
                        WriteCommand::FinishRun {
                            run_id: run_id.into(),
                            status: "cancelled".into(),
                        },
                    )?;
                    return Ok("cancelled".into());
                }
                // Settle and finish in one transaction: no crash window in
                // which a paid call sits under an unfinished step (AD-8).
                writer.exec(
                    system("engine"),
                    WriteCommand::SettleAndFinishStep {
                        call_id,
                        usage: out.usage,
                        cost_micro_usd: cost,
                        step_run_id: step.id.clone(),
                        status: "succeeded".into(),
                        output: Some(out.text),
                    },
                )?;
                // The step as the journal now holds it (its output and its
                // committed taint) is an earlier step of the next, on a
                // fresh run as on a resumed one (review P6).
                if let Some(done) = reader.steps(run_id)?.into_iter().find(|s| s.id == step.id) {
                    earlier.push(done);
                }
            }
            Err(e) => {
                writer.exec(
                    system("engine"),
                    WriteCommand::SettleAndFinishStep {
                        call_id,
                        usage: None,
                        cost_micro_usd: None,
                        step_run_id: step.id.clone(),
                        status: "failed".into(),
                        output: Some(e.to_string()),
                    },
                )?;
                writer.exec(
                    system("engine"),
                    WriteCommand::FinishRun {
                        run_id: run_id.into(),
                        status: "failed".into(),
                    },
                )?;
                return Err(RunError::Provider(e.to_string()));
            }
        }
    }
    writer.exec(
        system("engine"),
        WriteCommand::FinishRun {
            run_id: run_id.into(),
            status: "succeeded".into(),
        },
    )?;
    Ok("succeeded".into())
}
