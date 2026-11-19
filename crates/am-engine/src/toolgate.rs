//! ToolGate (AD-11): the single choke point for every tool call. This slice
//! enforces each tool's manifest (action class, `max_uses_per_step`, overridable
//! per seat) and refuses unknown tools; every refusal is written to the audit
//! sink. Grants, taint, egress, budget and the other caps land in later stories.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use am_domain::{Actor, AuditEvent, AuditSink, ToolCall};

/// The action class a tool's manifest declares (AD-11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionClass {
    Read,
    LocalWrite,
    ExternalEffect,
}

/// What a tool declares about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolManifest {
    pub name: String,
    pub class: ActionClass,
    /// How many times one step may call this tool; 0 means never.
    pub max_uses_per_step: u32,
}

/// ToolGate's answer for one call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateDecision {
    Allow,
    /// Refused, with the reason in plain words.
    Refuse(String),
}

pub struct ToolGate {
    manifests: HashMap<String, ToolManifest>,
    /// (seat, tool) -> limit that replaces the manifest's for that seat.
    seat_overrides: HashMap<(String, String), u32>,
    /// Allowed calls so far, per (run, step, tool). A new step starts at zero.
    counts: Mutex<HashMap<StepTool, u32>>,
    audit: Arc<dyn AuditSink>,
}

impl ToolGate {
    pub fn new(manifests: Vec<ToolManifest>, audit: Arc<dyn AuditSink>) -> Self {
        ToolGate {
            manifests: manifests.into_iter().map(|m| (m.name.clone(), m)).collect(),
            seat_overrides: HashMap::new(),
            counts: Mutex::new(HashMap::new()),
            audit,
        }
    }

    /// Replace the manifest's `max_uses_per_step` for one seat and tool.
    pub fn set_seat_override(&mut self, seat: String, tool: String, max_uses_per_step: u32) {
        self.seat_overrides.insert((seat, tool), max_uses_per_step);
    }

    /// The manifest ToolGate holds for a tool, if any.
    pub fn manifest(&self, tool: &str) -> Option<ToolManifest> {
        self.manifests.get(tool).cloned()
    }

    /// Decide one tool call made by `actor` (a seat inside a step).
    ///
    /// A call within the tool's per-step limit is allowed and counted; a call
    /// past it, a call to a tool with no manifest, or a call from an actor that
    /// is not a seat in a step is refused, audited and not counted.
    pub fn check(&self, actor: &Actor, call: &ToolCall) -> GateDecision {
        let tool = call.name.as_str();
        let Actor::Seat { seat, run, step } = actor else {
            return self.refuse(
                actor,
                tool,
                format!(
                    "Tool \"{tool}\" was refused: only a seat inside a step may call tools ({}).",
                    actor.describe()
                ),
            );
        };
        let Some(manifest) = self.manifests.get(tool) else {
            return self.refuse(
                actor,
                tool,
                format!("Tool \"{tool}\" was refused: it is an unknown tool (no manifest)."),
            );
        };
        let limit = self
            .seat_overrides
            .get(&(seat.clone(), tool.to_string()))
            .copied()
            .unwrap_or(manifest.max_uses_per_step);

        let key = StepTool {
            run: run.to_string(),
            step: step.clone(),
            tool: tool.to_string(),
        };
        let mut counts = self.counts.lock().unwrap_or_else(|p| p.into_inner());
        let used = counts.entry(key).or_insert(0);
        if *used >= limit {
            drop(counts);
            let reason = if limit == 0 {
                format!(
                    "Tool \"{tool}\" was refused: its limit is 0 uses per step, so this step may not call it."
                )
            } else {
                format!(
                    "Tool \"{tool}\" was refused: this step already used it {limit} times, its limit of {limit} uses per step."
                )
            };
            return self.refuse(actor, tool, reason);
        }
        *used += 1;
        GateDecision::Allow
    }

    fn refuse(&self, actor: &Actor, tool: &str, reason: String) -> GateDecision {
        let step = match actor {
            Actor::Seat { run, step, .. } => format!(" run:{run} step:{step}"),
            _ => String::new(),
        };
        self.audit.record(AuditEvent {
            actor: actor.clone(),
            action: "toolgate.refused".to_string(),
            target: format!("tool:{tool}{step}"),
            outcome: reason.clone(),
            hash: None,
        });
        GateDecision::Refuse(reason)
    }
}

/// The counting key: one tool inside one step of one run.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct StepTool {
    run: String,
    step: String,
    tool: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use am_testkit::CollectAudit;
    use uuid::Uuid;

    fn seat(seat: &str, run: Uuid, step: &str) -> Actor {
        Actor::Seat {
            seat: seat.to_string(),
            run,
            step: step.to_string(),
        }
    }

    fn call(name: &str) -> ToolCall {
        ToolCall {
            id: "call-1".to_string(),
            name: name.to_string(),
            arguments_json: "{}".to_string(),
        }
    }

    fn manifest(name: &str, max: u32) -> ToolManifest {
        ToolManifest {
            name: name.to_string(),
            class: ActionClass::Read,
            max_uses_per_step: max,
        }
    }

    fn refused(d: &GateDecision) -> bool {
        matches!(d, GateDecision::Refuse(_))
    }

    #[test]
    fn third_call_past_limit_of_two_is_refused_with_reason_and_audited() {
        let audit = CollectAudit::new();
        let gate = ToolGate::new(vec![manifest("lookup", 2)], audit.clone());
        let a = seat("seat-x", Uuid::now_v7(), "s1");

        assert_eq!(gate.check(&a, &call("lookup")), GateDecision::Allow);
        assert_eq!(gate.check(&a, &call("lookup")), GateDecision::Allow);
        let third = gate.check(&a, &call("lookup"));
        match &third {
            GateDecision::Refuse(reason) => {
                assert!(reason.contains("lookup"), "reason names the tool: {reason}");
                assert!(reason.contains('2'), "reason names the limit: {reason}");
            }
            GateDecision::Allow => panic!("third call must be refused"),
        }

        let events = audit.events();
        let refusals: Vec<_> = events
            .iter()
            .filter(|e| e.action == "toolgate.refused")
            .collect();
        assert_eq!(refusals.len(), 1);
        assert_eq!(refusals[0].actor, a);
        assert!(refusals[0].target.contains("lookup"));
    }

    #[test]
    fn seat_override_replaces_manifest_limit() {
        let audit = CollectAudit::new();
        let mut gate = ToolGate::new(vec![manifest("lookup", 2)], audit.clone());
        gate.set_seat_override("seat-x".to_string(), "lookup".to_string(), 5);
        let a = seat("seat-x", Uuid::now_v7(), "s1");

        for i in 1..=5 {
            assert_eq!(
                gate.check(&a, &call("lookup")),
                GateDecision::Allow,
                "call {i}"
            );
        }
        assert!(refused(&gate.check(&a, &call("lookup"))));

        // Another seat keeps the manifest's limit.
        let b = seat("seat-y", Uuid::now_v7(), "s1");
        assert_eq!(gate.check(&b, &call("lookup")), GateDecision::Allow);
        assert_eq!(gate.check(&b, &call("lookup")), GateDecision::Allow);
        assert!(refused(&gate.check(&b, &call("lookup"))));
    }

    #[test]
    fn counts_are_per_step() {
        let audit = CollectAudit::new();
        let gate = ToolGate::new(vec![manifest("lookup", 1)], audit.clone());
        let run = Uuid::now_v7();
        let s1 = seat("seat-x", run, "s1");
        let s2 = seat("seat-x", run, "s2");

        assert_eq!(gate.check(&s1, &call("lookup")), GateDecision::Allow);
        assert!(refused(&gate.check(&s1, &call("lookup"))));
        assert_eq!(gate.check(&s2, &call("lookup")), GateDecision::Allow);

        // Same step name in another run is another step.
        let other_run = seat("seat-x", Uuid::now_v7(), "s1");
        assert_eq!(gate.check(&other_run, &call("lookup")), GateDecision::Allow);
    }

    #[test]
    fn limit_zero_refuses_first_call() {
        let audit = CollectAudit::new();
        let gate = ToolGate::new(vec![manifest("lookup", 0)], audit.clone());
        let a = seat("seat-x", Uuid::now_v7(), "s1");

        assert!(refused(&gate.check(&a, &call("lookup"))));
        assert_eq!(audit.events().len(), 1);
        assert_eq!(audit.events()[0].action, "toolgate.refused");
    }

    #[test]
    fn unknown_tool_is_refused_and_audited() {
        let audit = CollectAudit::new();
        let gate = ToolGate::new(vec![manifest("lookup", 2)], audit.clone());
        let a = seat("seat-x", Uuid::now_v7(), "s1");

        match gate.check(&a, &call("no_such_tool")) {
            GateDecision::Refuse(reason) => {
                assert!(reason.contains("unknown"), "{reason}");
                assert!(reason.contains("no_such_tool"), "{reason}");
            }
            GateDecision::Allow => panic!("unknown tool must be refused"),
        }
        let events = audit.events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].action, "toolgate.refused");
        assert!(events[0].target.contains("no_such_tool"));
    }

    #[test]
    fn refusals_do_not_consume_a_use_and_stay_refused() {
        let audit = CollectAudit::new();
        let gate = ToolGate::new(vec![manifest("lookup", 2)], audit.clone());
        let a = seat("seat-x", Uuid::now_v7(), "s1");

        assert_eq!(gate.check(&a, &call("lookup")), GateDecision::Allow);
        assert_eq!(gate.check(&a, &call("lookup")), GateDecision::Allow);
        for _ in 0..3 {
            assert!(refused(&gate.check(&a, &call("lookup"))));
        }
        // An unknown-tool refusal in the same step does not touch the count of
        // another tool in a fresh step.
        let b = seat("seat-x", Uuid::now_v7(), "s1");
        assert!(refused(&gate.check(&b, &call("nope"))));
        assert_eq!(gate.check(&b, &call("lookup")), GateDecision::Allow);
        assert_eq!(gate.check(&b, &call("lookup")), GateDecision::Allow);
        assert!(refused(&gate.check(&b, &call("lookup"))));

        assert_eq!(
            audit
                .events()
                .iter()
                .filter(|e| e.action == "toolgate.refused")
                .count(),
            5
        );
    }

    #[test]
    fn a_non_seat_actor_cannot_call_tools() {
        let audit = CollectAudit::new();
        let gate = ToolGate::new(vec![manifest("lookup", 2)], audit.clone());
        let sys = Actor::System {
            component: "scheduler".to_string(),
        };
        assert!(refused(&gate.check(&sys, &call("lookup"))));
        assert_eq!(audit.events().len(), 1);
    }
}
