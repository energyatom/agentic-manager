//! The journaled run executor (AD-8, AD-9), spike scope: a linear workflow of
//! model-call steps. Every transition commits before the side effect it
//! authorises; on restart an unfinished run resumes at its last committed
//! transition, a call without `settled` is marked lost (settled at its
//! reservation) and re-issued.

use std::path::Path;
use std::sync::Arc;

use am_domain::{
    Actor, ChatRequest, EventSink, Instruction, LlmProvider, Message, Part, Role, StopReason,
    StreamEvent, Usage,
};
use am_store::{Reader, StoreError, WriteCommand, Writer, new_id};
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
    let mut previous_outputs: Vec<String> = Vec::new();
    for step in reader.steps(run_id)? {
        if step.status == "succeeded" {
            previous_outputs.push(step.output.unwrap_or_default());
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
        if step.status == "queued" {
            writer.exec(
                system("engine"),
                WriteCommand::StartStep {
                    step_run_id: step.id.clone(),
                },
            )?;
        }
        let call_seq = step_calls.iter().map(|c| c.call_seq).max().unwrap_or(0) + 1;
        let prompt = format!(
            "Step {} of workflow {}. Earlier steps said: {}. Reply with one short line.",
            step.step_key,
            wf.workflow,
            previous_outputs.join(" | ")
        );
        let est_input = (prompt.len() as u64 / 4).max(1);
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
            },
        )?;
        let request = ChatRequest {
            model: wf.model.clone(),
            system: vec![Instruction::authored("You are a step in a test workflow.")],
            messages: vec![Message {
                role: Role::User,
                parts: vec![Part::Instruction(Instruction::authored(prompt))],
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
                previous_outputs.push(out.text.clone());
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

#[cfg(test)]
mod tests {
    use super::*;
    use am_testkit::{FakeLlm, ScriptedTurn};
    use std::time::Duration;

    fn wf() -> LinearWorkflow {
        LinearWorkflow {
            workflow: "wf".into(),
            steps: vec!["a".into(), "b".into()],
            model: "fake".into(),
            max_output_tokens: 100,
            price: Price {
                input_micro_per_mtok: 3_000_000,
                output_micro_per_mtok: 15_000_000,
                snapshot: "test".into(),
            },
        }
    }

    #[tokio::test]
    async fn runs_to_success_and_settles_from_usage() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("state.db");
        let store = am_store::Store::open(&db).unwrap();
        let w = store.writer();
        let llm = Arc::new(FakeLlm::new(vec![
            ScriptedTurn::text("ok")
                .usage(1000, 100)
                .delay(Duration::from_millis(1)),
        ]));
        let run = create_run(&w, system("test"), &wf(), "manual").unwrap();
        let status = drive_run(&db, &w, llm.clone(), &wf(), &run, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(status, "succeeded");
        assert_eq!(llm.calls(), 2);
        let l = store.reader().unwrap().ledger_for_run(&run).unwrap();
        assert!(l.iter().all(|e| e.settled_micro_usd == Some(4500)));
    }

    #[tokio::test]
    async fn cancel_settles_at_reservation_when_usage_missing() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("state.db");
        let store = am_store::Store::open(&db).unwrap();
        let w = store.writer();
        let llm = Arc::new(FakeLlm::new(vec![
            ScriptedTurn::text("slow").delay(Duration::from_secs(30)),
        ]));
        let run = create_run(&w, system("test"), &wf(), "manual").unwrap();
        let token = CancellationToken::new();
        let t2 = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            t2.cancel();
        });
        let status = drive_run(&db, &w, llm, &wf(), &run, token).await.unwrap();
        assert_eq!(status, "cancelled");
        let l = store.reader().unwrap().ledger_for_run(&run).unwrap();
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].settled_micro_usd, Some(l[0].reserved_micro_usd));
    }
}
