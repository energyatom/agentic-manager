//! ToolGate (AD-11): the single choke point for every tool call. It checks
//! a call against its tool's manifest (`max_uses_per_step`, overridable per
//! seat) and refuses unknown tools; every refusal is written to the audit
//! sink. Grants, taint, egress, budget and the other caps land in later
//! stories.
//!
//! Story 3.3 (owner condition 2; review P2, P8): ToolGate is also the only
//! caller of a [`ToolRunner`]. A gate built [`ToolGate::over`] a runner
//! admits a call ([`ToolGate::admit`], which counts uses per step run) and
//! hands back a [`Permit`] only it can make; only [`ToolGate::run`] runs a
//! permitted call, so no other code reaches the runner. Until stories 3.6
//! and 4.8 bring local-write and external-effect tools with their own
//! checks, every class but `Read` is refused. The shipped binary builds no
//! gate over a runner: its one runner is am-testkit's, in tests.
//!
//! Review round 2 (R7, R9): a [`Permit`] carries the id of the gate that
//! admitted it, and [`ToolGate::run`] refuses another gate's permit; a
//! runner that declares one id twice gets that id refused (which manifest
//! decides is not left to chance; [`ToolGate::duplicates`] names them). The
//! gate labels every name it words or audits itself ([`ToolGate::label`]):
//! a name a manifest holds, or a built-in tool's id, stays as it is; any
//! other (the model's choice) is only `unknown:<sha256 of the name>`.

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use am_domain::{
    Actor, AuditEvent, AuditSink, BoxFuture, CancelSignal, ToolCall, ToolRunner, UntrustedData,
};

/// The action class a tool's manifest declares (AD-11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionClass {
    Read,
    LocalWrite,
    ExternalEffect,
}

impl ActionClass {
    fn words(self) -> &'static str {
        match self {
            ActionClass::Read => "read",
            ActionClass::LocalWrite => "local-write",
            ActionClass::ExternalEffect => "external-effect",
        }
    }
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

/// A call ToolGate admitted, which only [`ToolGate::run`] of the same gate
/// runs. Only [`ToolGate::admit`] makes one.
#[derive(Debug)]
pub struct Permit {
    /// The id of the gate that admitted it.
    gate: u64,
    call: ToolCall,
}

/// The ids of the gates made so far, so each gate has its own.
static NEXT_GATE: AtomicU64 = AtomicU64::new(1);

/// How a tool name is worded, journaled and audited (review P15, R9): a
/// name in `known` (a manifest's id, or a built-in tool's) as itself; any
/// other only as `unknown:<sha256 hex of the name's UTF-8 bytes>`, so no
/// control character, no length and no word of a model-chosen name reaches
/// a row, the audit log or a terminal.
pub fn label_of(name: &str, known: &[String]) -> String {
    if known.iter().any(|k| k == name) {
        name.to_string()
    } else {
        format!("unknown:{}", am_store::audit::sha256_hex(name))
    }
}

/// The built-in tools' ids (AD-11's table): names that are never the
/// model's own words.
pub fn builtin_names() -> Vec<String> {
    am_domain::tool_manifest::builtin_tool_ids()
}

pub struct ToolGate {
    manifests: HashMap<String, ToolManifest>,
    /// (seat, tool) -> limit that replaces the manifest's for that seat.
    seat_overrides: HashMap<(String, String), u32>,
    /// Allowed calls so far, per (run, step, tool), or per (run, step run,
    /// tool) for an admitted call. A new step, or a new attempt of one,
    /// starts at zero.
    counts: Mutex<HashMap<StepTool, u32>>,
    audit: Arc<dyn AuditSink>,
    /// The runner of the admitted calls; none for a gate that only checks.
    runner: Option<Arc<dyn ToolRunner>>,
    /// This gate's id, carried by its permits.
    id: u64,
    /// Ids the runner declared more than once: always refused.
    duplicates: Vec<String>,
}

impl ToolGate {
    pub fn new(manifests: Vec<ToolManifest>, audit: Arc<dyn AuditSink>) -> Self {
        ToolGate {
            manifests: manifests.into_iter().map(|m| (m.name.clone(), m)).collect(),
            seat_overrides: HashMap::new(),
            counts: Mutex::new(HashMap::new()),
            audit,
            runner: None,
            id: NEXT_GATE.fetch_add(1, Ordering::Relaxed),
            duplicates: Vec::new(),
        }
    }

    /// A gate over `runner`'s tools (story 3.3): their manifests, and the
    /// runner that only this gate calls, for the calls it admits.
    pub fn over(runner: Arc<dyn ToolRunner>, audit: Arc<dyn AuditSink>) -> Self {
        use am_domain::tool_manifest::ActionClass as Declared;
        let declared = runner.manifests();
        let mut seen = BTreeSet::new();
        let duplicates: BTreeSet<String> = declared
            .iter()
            .filter(|m| !seen.insert(m.id.clone()))
            .map(|m| m.id.clone())
            .collect();
        let manifests = declared
            .into_iter()
            .filter(|m| !duplicates.contains(&m.id))
            .map(|m| ToolManifest {
                name: m.id,
                class: match m.class {
                    Declared::Read => ActionClass::Read,
                    Declared::LocalWrite => ActionClass::LocalWrite,
                    Declared::ExternalEffect => ActionClass::ExternalEffect,
                },
                max_uses_per_step: m.max_uses_per_step,
            })
            .collect();
        ToolGate {
            runner: Some(runner),
            duplicates: duplicates.into_iter().collect(),
            ..ToolGate::new(manifests, audit)
        }
    }

    /// The ids the runner declared more than once (refused whenever asked
    /// for; a caller may refuse the runner as a whole).
    pub fn duplicates(&self) -> Vec<String> {
        self.duplicates.clone()
    }

    /// How this gate words, journals and audits the tool name `name` (see
    /// [`label_of`]): its manifests' ids, the duplicated ones and the
    /// built-in ids stay as they are.
    pub fn label(&self, name: &str) -> String {
        let mut known: Vec<String> = self.manifests.keys().cloned().collect();
        known.extend(self.duplicates.iter().cloned());
        known.extend(builtin_names());
        label_of(name, &known)
    }

    /// Every manifest the gate holds, by tool name.
    pub fn manifests(&self) -> Vec<ToolManifest> {
        let mut all: Vec<ToolManifest> = self.manifests.values().cloned().collect();
        all.sort_by(|a, b| a.name.cmp(&b.name));
        all
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
        let scope = match actor {
            Actor::Seat { step, .. } => step.clone(),
            _ => String::new(),
        };
        let call = ToolCall {
            name: self.label(&call.name),
            ..call.clone()
        };
        self.decide(actor, &scope, &call)
    }

    /// Admits one call made by `actor` (a seat inside the step run
    /// `step_run`), as [`ToolGate::check`] decides it, except that uses
    /// count per step run: a new attempt of a step starts at zero (review
    /// P8). An admitted call comes back as a [`Permit`] for
    /// [`ToolGate::run`]; a refusal in words.
    pub fn admit(&self, actor: &Actor, step_run: &str, call: &ToolCall) -> Result<Permit, String> {
        let call = ToolCall {
            name: self.label(&call.name),
            ..call.clone()
        };
        match self.decide(actor, step_run, &call) {
            GateDecision::Allow => Ok(Permit {
                gate: self.id,
                call,
            }),
            GateDecision::Refuse(why) => Err(why),
        }
    }

    /// Runs an admitted call on the gate's runner, the only place a
    /// `ToolRunner` is called (AD-11). A gate with no runner runs nothing.
    pub fn run(
        &self,
        permit: Permit,
        cancel: Arc<dyn CancelSignal>,
    ) -> BoxFuture<Result<UntrustedData, String>> {
        if permit.gate != self.id {
            return Box::pin(async {
                Err(
                    "the permit was admitted by another ToolGate, so this one runs nothing"
                        .to_string(),
                )
            });
        }
        match &self.runner {
            Some(runner) => runner.run(permit.call, cancel),
            None => Box::pin(async { Err("this ToolGate runs no tool".to_string()) }),
        }
    }

    /// The decision for `call` (its name already labelled), uses counted
    /// per (run, `scope`, tool).
    fn decide(&self, actor: &Actor, scope: &str, call: &ToolCall) -> GateDecision {
        let tool = call.name.as_str();
        let Actor::Seat { seat, run, .. } = actor else {
            return self.refuse(
                actor,
                tool,
                format!(
                    "Tool \"{tool}\" was refused: only a seat inside a step may call tools ({}).",
                    actor.describe()
                ),
            );
        };
        if self.duplicates.iter().any(|d| d == tool) {
            return self.refuse(
                actor,
                tool,
                format!(
                    "Tool \"{tool}\" was refused: its runner declares it more than once, so which manifest decides is not known."
                ),
            );
        }
        let Some(manifest) = self.manifests.get(tool) else {
            return self.refuse(
                actor,
                tool,
                format!("Tool \"{tool}\" was refused: it is an unknown tool (no manifest)."),
            );
        };
        // Only read tools run until stories 3.6 and 4.8 bring the checks of
        // the other classes (review P2).
        if manifest.class != ActionClass::Read {
            return self.refuse(
                actor,
                tool,
                format!(
                    "Tool \"{tool}\" was refused: it is a {} tool, and this version runs read tools only.",
                    manifest.class.words()
                ),
            );
        }
        let limit = self
            .seat_overrides
            .get(&(seat.clone(), tool.to_string()))
            .copied()
            .unwrap_or(manifest.max_uses_per_step);

        let key = StepTool {
            run: run.to_string(),
            step: scope.to_string(),
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
            args: Default::default(),
        });
        GateDecision::Refuse(reason)
    }
}

/// The counting key: one tool inside one step (or one step run) of one run.
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

        // Review R9: the model's name is never worded or audited raw.
        let label = format!("unknown:{}", am_store::audit::sha256_hex("no_such_tool"));
        match gate.check(&a, &call("no_such_tool")) {
            GateDecision::Refuse(reason) => {
                assert!(reason.contains("unknown"), "{reason}");
                assert!(reason.contains(&label), "{reason}");
                assert!(!reason.contains("no_such_tool"), "{reason}");
            }
            GateDecision::Allow => panic!("unknown tool must be refused"),
        }
        let events = audit.events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].action, "toolgate.refused");
        assert!(events[0].target.contains(&label));
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
