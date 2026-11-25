//! ToolGate (AD-11): the single choke point for every tool call. It checks
//! a call against its tool's manifest (`max_uses_per_step`, overridable per
//! seat) and refuses unknown tools; every refusal is written to the audit
//! sink. Taint, egress, budget and the other caps land in later stories.
//!
//! Story 3.3 (owner condition 2; review P2, P8): ToolGate is also the only
//! caller of a [`ToolRunner`]. A gate built [`ToolGate::over`] a runner
//! admits a call ([`ToolGate::admit`], which counts uses per step run) and
//! hands back a [`Permit`] only it can make; only [`ToolGate::run`] runs a
//! permitted call, so no other code reaches the runner.
//!
//! Review round 2 (R7, R9): a [`Permit`] carries the id of the gate that
//! admitted it, and [`ToolGate::run`] refuses another gate's permit; a
//! runner that declares one id twice gets that id refused (which manifest
//! decides is not left to chance; [`ToolGate::duplicates`] names them). The
//! gate labels every name it words or audits itself ([`ToolGate::label`]):
//! a name a manifest holds, or a built-in tool's id, stays as it is; any
//! other (the model's choice) is only `unknown:<sha256 of the name>`.
//!
//! Story 3.6, the ToolGate slice (AD-10 and AD-21 as amended 2026-11-24,
//! AD-11, AD-46; owner Q1 and Q4): every run has a gate, the shipped one
//! ([`ToolGate::shipped`]) included, and every tool request goes through
//! it. A gate knows the built-in tools' manifests (AD-11's table) besides
//! its runner's, and admits a call in this order, the first refusal
//! winning: a Seat actor; no duplicated id; a known tool; a tool the seat
//! is granted ([`StepTools::grants`], which only capabilities fill); its
//! class (a read tool its runner runs; of the local-write class only
//! `table_write`; no external effect); for `table_write`, the step declares
//! a table of the seat's own team and the call's `table`, when it is text,
//! names it (a call with no `table` or arguments that are no JSON object
//! go to the row tool, which words them for the model); the per-step use
//! limit. A runner never takes a built-in tool's id (review round 1, R5). A refusal is audited `toolgate.refused` with the call's taint
//! and worded from config ids, never the model's text.
//!
//! `table_write` runs here: the row tool (am-tools', handed in by the
//! caller) checks the call's arguments into a prepared row, the gate mints
//! an insert's row id and writes through the row-writer port, the one
//! caller of [`RowWriter::write_row`] (`cargo xtask check-deps`). A
//! committed write comes back as an [`EngineResult`], which only this file
//! builds ([`EngineResult::row_written`], from the row id, the version and
//! the status: engine values only; it does not taint); a refusal the model
//! may act on is [`GateError::Tool`], words the engine sends as untrusted
//! data; a store failure is [`GateError::Store`] and stops the step.
//! [`engine_result_of`] rebuilds a write's result from the store's
//! data-free receipt (a resume, a drill-down), never from journal text.

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use am_domain::rows::{
    DeclaredTable, PreparedRow, RowOp, RowStatus, RowTool, RowWrite, RowWriteError, RowWriter,
    RowWritten,
};
use am_domain::{
    Actor, AuditEvent, AuditSink, BoxFuture, CancelSignal, EngineResult, Taint, ToolCall,
    ToolRunner, ToolSpec, UntrustedData,
};
use am_store::{Reader, StoreError};

/// The built-in row tool's id: the one local-write tool this version runs.
pub const TABLE_WRITE: &str = "table_write";

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

    fn of(declared: am_domain::tool_manifest::ActionClass) -> ActionClass {
        use am_domain::tool_manifest::ActionClass as Declared;
        match declared {
            Declared::Read => ActionClass::Read,
            Declared::LocalWrite => ActionClass::LocalWrite,
            Declared::ExternalEffect => ActionClass::ExternalEffect,
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

impl ToolManifest {
    fn of(m: am_domain::tool_manifest::ToolManifest) -> ToolManifest {
        ToolManifest {
            name: m.id,
            class: ActionClass::of(m.class),
            max_uses_per_step: m.max_uses_per_step,
        }
    }
}

/// ToolGate's answer for one call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateDecision {
    Allow,
    /// Refused, with the reason in plain words.
    Refuse(String),
}

/// What the gate needs to know of the step a call is made in (story 3.6):
/// the seat's grants (its capabilities' expansion, AD-46: nothing else
/// grants a tool), the seat's team and the one table the step declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepTools {
    pub grants: BTreeSet<String>,
    pub seat_team: String,
    pub table: Option<DeclaredTable>,
}

/// Where a call stands in the journal (story 3.6): its attempt's step run,
/// its call sequence and its `tool_calls` row id (AD-8: a row write's key),
/// and the taint the attempt carries when it is made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallPlace {
    pub step_run_id: String,
    pub call_seq: i64,
    pub tool_call_id: String,
    pub taint: Taint,
}

/// What a call the gate ran gives the model (story 3.6, AD-10 as amended):
/// the engine's own result of a row write, or a runner's untrusted result.
#[derive(Debug)]
pub enum GateOutput {
    Engine(EngineResult),
    Untrusted(UntrustedData),
}

/// Why a call the gate admitted gave no result.
#[derive(Debug)]
pub enum GateError {
    /// Words the model is shown as the call's result, untrusted: a runner
    /// tool's failure, a row the row tool refused, a write the row's state
    /// refused (a stale version). The step goes on.
    Tool(String),
    /// The store, or the gate itself, failed: the step stops.
    Store(String),
}

/// A call ToolGate admitted, which only [`ToolGate::run`] of the same gate
/// runs. Only [`ToolGate::admit`] makes one.
#[derive(Debug)]
pub struct Permit {
    /// The id of the gate that admitted it.
    gate: u64,
    call: ToolCall,
    /// The seat inside the step that made the call.
    actor: Actor,
    place: CallPlace,
    /// The table the call writes (`table_write` only).
    table: Option<DeclaredTable>,
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
    /// Every tool the gate knows, by name: the built-in tools' manifests
    /// (a gate made [`ToolGate::shipped`] or [`ToolGate::over`] a runner)
    /// and the runner's own; a runner manifest with a built-in's id never
    /// replaces the built-in's (review round 1, R5: it is listed in
    /// `shadowing` instead).
    manifests: HashMap<String, ToolManifest>,
    /// The tools other than `table_write` this gate runs: its runner's.
    runs: BTreeSet<String>,
    /// (seat, tool) -> limit that replaces the manifest's for that seat.
    seat_overrides: HashMap<(String, String), u32>,
    /// Allowed calls so far, per (run, step, tool), or per (run, step run,
    /// tool) for an admitted call. A new step, or a new attempt of one,
    /// starts at zero.
    counts: Mutex<HashMap<StepTool, u32>>,
    audit: Arc<dyn AuditSink>,
    /// The runner of the admitted calls; none for a gate that only checks.
    runner: Option<Arc<dyn ToolRunner>>,
    /// The row-writer port `table_write` writes through (story 3.6).
    row_writer: Option<Arc<dyn RowWriter>>,
    /// The row tool that prepares a `table_write` call (am-tools').
    row_tool: Option<Arc<dyn RowTool>>,
    /// This gate's id, carried by its permits.
    id: u64,
    /// Ids the runner declared more than once: always refused.
    duplicates: Vec<String>,
    /// Built-in ids the runner declared too: the built-in's manifest stands.
    shadowing: Vec<String>,
}

impl ToolGate {
    /// A gate that checks calls against `manifests` (each one a tool it
    /// admits) and runs none.
    pub fn new(manifests: Vec<ToolManifest>, audit: Arc<dyn AuditSink>) -> Self {
        ToolGate {
            runs: manifests.iter().map(|m| m.name.clone()).collect(),
            manifests: manifests.into_iter().map(|m| (m.name.clone(), m)).collect(),
            seat_overrides: HashMap::new(),
            counts: Mutex::new(HashMap::new()),
            audit,
            runner: None,
            row_writer: None,
            row_tool: None,
            id: NEXT_GATE.fetch_add(1, Ordering::Relaxed),
            duplicates: Vec::new(),
            shadowing: Vec::new(),
        }
    }

    /// The shipped gate (story 3.6, AD-11): every built-in tool's manifest,
    /// no runner, and `table_write` through `row_writer` with `row_tool`
    /// (none: the run writes no row). Every run has one.
    pub fn shipped(
        row_writer: Arc<dyn RowWriter>,
        row_tool: Option<Arc<dyn RowTool>>,
        audit: Arc<dyn AuditSink>,
    ) -> Self {
        let builtins = am_domain::tool_manifest::builtin_tools()
            .into_iter()
            .map(ToolManifest::of)
            .map(|m| (m.name.clone(), m))
            .collect();
        ToolGate {
            manifests: builtins,
            runs: BTreeSet::new(),
            row_writer: Some(row_writer),
            row_tool,
            ..ToolGate::new(Vec::new(), audit)
        }
    }

    /// A gate over `runner`'s tools (story 3.3): the shipped gate, plus the
    /// runner's manifests and the runner that only this gate calls, for the
    /// calls it admits.
    pub fn over(
        runner: Arc<dyn ToolRunner>,
        row_writer: Arc<dyn RowWriter>,
        row_tool: Option<Arc<dyn RowTool>>,
        audit: Arc<dyn AuditSink>,
    ) -> Self {
        let builtins = builtin_names();
        // Review round 1, R5: a runner never takes a built-in's id over (its
        // class, its use limit or its path); such a tool is refused.
        let (shadowing, declared): (Vec<_>, Vec<_>) = runner
            .manifests()
            .into_iter()
            .partition(|m| builtins.contains(&m.id));
        let mut seen = BTreeSet::new();
        let duplicates: BTreeSet<String> = declared
            .iter()
            .filter(|m| !seen.insert(m.id.clone()))
            .map(|m| m.id.clone())
            .collect();
        let mut gate = ToolGate::shipped(row_writer, row_tool, audit);
        for manifest in declared
            .into_iter()
            .filter(|m| !duplicates.contains(&m.id))
            .map(ToolManifest::of)
        {
            gate.runs.insert(manifest.name.clone());
            gate.manifests.insert(manifest.name.clone(), manifest);
        }
        let shadowing: BTreeSet<String> = shadowing.into_iter().map(|m| m.id).collect();
        ToolGate {
            runner: Some(runner),
            duplicates: duplicates.into_iter().collect(),
            shadowing: shadowing.into_iter().collect(),
            ..gate
        }
    }

    /// The ids the runner declared that are built-in tools' (review round
    /// 1, R5): never run on the runner; a caller may refuse the runner as a
    /// whole.
    pub fn shadowing(&self) -> Vec<String> {
        self.shadowing.clone()
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

    /// The tools a step is offered (story 3.6, AD-46): those the gate would
    /// admit for it by grant and class. A granted read tool of the runner,
    /// in name order; then, for a step that declares a table of the seat's
    /// team and whose seat is granted `table_write`, the row tool's spec
    /// for that table. A capability that is off means its tool is absent.
    pub fn offered(&self, step: &StepTools) -> Vec<ToolSpec> {
        let mut specs: Vec<ToolSpec> = self
            .manifests()
            .into_iter()
            .filter(|m| {
                m.class == ActionClass::Read
                    && self.runs.contains(&m.name)
                    && step.grants.contains(&m.name)
            })
            .map(|m| ToolSpec {
                description: format!("The tool `{}`.", m.name),
                name: m.name,
                input_schema_json: "{\"type\":\"object\"}".into(),
            })
            .collect();
        if let (Some(tool), Some(_), Some(table), true) = (
            &self.row_tool,
            &self.row_writer,
            &step.table,
            step.grants.contains(TABLE_WRITE),
        ) && table.team == step.seat_team
        {
            specs.push(tool.spec(table));
        }
        specs
    }

    /// Replace the manifest's `max_uses_per_step` for one seat and tool.
    pub fn set_seat_override(&mut self, seat: String, tool: String, max_uses_per_step: u32) {
        self.seat_overrides.insert((seat, tool), max_uses_per_step);
    }

    /// The manifest ToolGate holds for a tool, if any.
    pub fn manifest(&self, tool: &str) -> Option<ToolManifest> {
        self.manifests.get(tool).cloned()
    }

    /// Decide one tool call made by `actor` (a seat inside a step) whose
    /// step is `step`.
    ///
    /// A call within the tool's per-step limit is allowed and counted; a call
    /// past it, a call to a tool with no manifest, one the seat is not
    /// granted, one of a class this version does not run, or a call from an
    /// actor that is not a seat in a step is refused, audited and not
    /// counted.
    pub fn check(&self, actor: &Actor, step: &StepTools, call: &ToolCall) -> GateDecision {
        let scope = match actor {
            Actor::Seat { step, .. } => step.clone(),
            _ => String::new(),
        };
        let call = ToolCall {
            name: self.label(&call.name),
            ..call.clone()
        };
        self.decide(actor, &scope, step, None, &call)
    }

    /// Admits one call made by `actor` (a seat inside the step run of
    /// `place`, whose step is `step`), as [`ToolGate::check`] decides it,
    /// except that uses count per step run: a new attempt of a step starts
    /// at zero (review P8). An admitted call comes back as a [`Permit`] for
    /// [`ToolGate::run`]; a refusal in words, audited with `place`'s taint.
    pub fn admit(
        &self,
        actor: &Actor,
        step: &StepTools,
        place: &CallPlace,
        call: &ToolCall,
    ) -> Result<Permit, String> {
        let call = ToolCall {
            name: self.label(&call.name),
            ..call.clone()
        };
        match self.decide(actor, &place.step_run_id, step, Some(place.taint), &call) {
            GateDecision::Allow => Ok(Permit {
                gate: self.id,
                table: (call.name == TABLE_WRITE)
                    .then(|| step.table.clone())
                    .flatten(),
                call,
                actor: actor.clone(),
                place: place.clone(),
            }),
            GateDecision::Refuse(why) => Err(why),
        }
    }

    /// Runs an admitted call: `table_write` through the row tool and the
    /// row-writer port, any other on the gate's runner, the only place a
    /// `ToolRunner` is called (AD-11).
    pub fn run(
        &self,
        permit: Permit,
        cancel: Arc<dyn CancelSignal>,
    ) -> BoxFuture<Result<GateOutput, GateError>> {
        if permit.gate != self.id {
            return Box::pin(async {
                Err(GateError::Store(
                    "the permit was admitted by another ToolGate, so this one runs nothing"
                        .to_string(),
                ))
            });
        }
        if permit.call.name == TABLE_WRITE {
            let written = self.write(permit);
            return Box::pin(async move { written });
        }
        match &self.runner {
            Some(runner) => {
                let running = runner.run(permit.call, cancel);
                Box::pin(async move {
                    running
                        .await
                        .map(GateOutput::Untrusted)
                        .map_err(GateError::Tool)
                })
            }
            None => {
                Box::pin(async { Err(GateError::Store("this ToolGate runs no tool".to_string())) })
            }
        }
    }

    /// A permitted `table_write`: prepared by the row tool against the
    /// declared table, written through the port as the seat, at the call's
    /// place and with the attempt's taint; an insert's row id is minted
    /// here (a UUIDv7, never the model's text).
    fn write(&self, permit: Permit) -> Result<GateOutput, GateError> {
        let (Some(tool), Some(rows), Some(table)) =
            (&self.row_tool, &self.row_writer, &permit.table)
        else {
            return Err(GateError::Store(
                "this ToolGate has no row tool or table for table_write, so it writes no row"
                    .to_string(),
            ));
        };
        let op = match tool
            .prepare(&permit.call.arguments_json, table)
            .map_err(GateError::Tool)?
        {
            PreparedRow::Insert { data_json } => RowOp::Insert {
                row_id: uuid::Uuid::now_v7(),
                data_json,
            },
            PreparedRow::Update {
                row_id,
                expected_version,
                patch_json,
            } => RowOp::Update {
                row_id,
                expected_version,
                patch_json,
            },
        };
        let place = permit.place;
        let write = RowWrite {
            tool_call_id: place.tool_call_id,
            step_run_id: place.step_run_id,
            call_seq: place.call_seq,
            table_id: table.id.clone(),
            op,
            taint: place.taint,
        };
        match rows.write_row(permit.actor, write) {
            Ok(written) => Ok(GateOutput::Engine(EngineResult::row_written(&written))),
            Err(RowWriteError::Refused(words)) => Err(GateError::Tool(words)),
            Err(RowWriteError::Failed(words)) => Err(GateError::Store(words)),
        }
    }

    /// The decision for `call` (its name already labelled), uses counted
    /// per (run, `scope`, tool); a refusal is audited with `taint`.
    fn decide(
        &self,
        actor: &Actor,
        scope: &str,
        step: &StepTools,
        taint: Option<Taint>,
        call: &ToolCall,
    ) -> GateDecision {
        let tool = call.name.as_str();
        let refuse = |reason: String| self.refuse(actor, tool, taint, reason);
        let Actor::Seat {
            seat,
            run,
            step: step_key,
        } = actor
        else {
            return refuse(format!(
                "Tool \"{tool}\" was refused: only a seat inside a step may call tools ({}).",
                actor.describe()
            ));
        };
        if self.duplicates.iter().any(|d| d == tool) {
            return refuse(format!(
                "Tool \"{tool}\" was refused: its runner declares it more than once, so which manifest decides is not known."
            ));
        }
        let Some(manifest) = self.manifests.get(tool) else {
            return refuse(format!(
                "Tool \"{tool}\" was refused: it is an unknown tool (no manifest)."
            ));
        };
        // AD-46: only the seat's capabilities grant a tool.
        if !step.grants.contains(tool) {
            let why = match am_domain::capability::capability_of_tool(tool) {
                Some(capability) => format!("its {} capability is off", capability.id()),
                None => "it is not in the seat's grants".to_string(),
            };
            return refuse(format!(
                "Tool \"{tool}\" was refused: it is not available to seat {seat}, which is not granted it ({why})."
            ));
        }
        // Of the local-write class only `table_write` runs, until the other
        // tools' stories; external effects wait for story 4.8 (review P2).
        let runs = match manifest.class {
            ActionClass::Read => true,
            ActionClass::LocalWrite => tool == TABLE_WRITE,
            ActionClass::ExternalEffect => false,
        };
        if !runs {
            return refuse(format!(
                "Tool \"{tool}\" was refused: it is a {} tool, and this version runs read tools and table_write only.",
                manifest.class.words()
            ));
        }
        if tool == TABLE_WRITE {
            if self.row_tool.is_none() || self.row_writer.is_none() {
                return refuse(format!(
                    "Tool \"{tool}\" was refused: this run has no row tool, so it writes no row."
                ));
            }
            if let Err(why) = binding(step, step_key, seat, &call.arguments_json) {
                return refuse(format!("Tool \"{tool}\" was refused: {why}"));
            }
        } else if !self.runs.contains(tool) {
            return refuse(format!(
                "Tool \"{tool}\" was refused: this version has no runner for it, so it does not run yet."
            ));
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
            return refuse(reason);
        }
        *used += 1;
        GateDecision::Allow
    }

    fn refuse(
        &self,
        actor: &Actor,
        tool: &str,
        taint: Option<Taint>,
        reason: String,
    ) -> GateDecision {
        let step = match actor {
            Actor::Seat { run, step, .. } => format!(" run:{run} step:{step}"),
            _ => String::new(),
        };
        let mut args = std::collections::BTreeMap::new();
        if let Some(taint) = taint {
            args.insert(
                "taint".to_string(),
                if taint.is_tainted() { "1" } else { "0" }.to_string(),
            );
        }
        self.audit.record(AuditEvent {
            actor: actor.clone(),
            action: "toolgate.refused".to_string(),
            target: format!("tool:{tool}{step}"),
            outcome: reason.clone(),
            hash: None,
            args,
        });
        GateDecision::Refuse(reason)
    }
}

/// `table_write`'s binding (AD-21, AD-10 (b), RULE-8): the step declares a
/// table, the call names it, and it is a table of the seat's team. The
/// call's `table` is read and compared, and kept nowhere: the words name
/// config ids only.
fn binding(
    step: &StepTools,
    step_key: &str,
    seat: &str,
    arguments_json: &str,
) -> Result<(), String> {
    let Some(table) = &step.table else {
        return Err(format!(
            "step {step_key} declares no table, so it writes none."
        ));
    };
    // Review round 1, R6: only a `table` that is a string naming another
    // table is refused here. A call with no `table`, or arguments that are
    // not a JSON object, is admitted: the row tool words it for the model
    // as an untrusted error result, and the step goes on.
    let named = serde_json::from_str::<serde_json::Value>(arguments_json)
        .ok()
        .and_then(|args| {
            args.get("table")
                .and_then(|t| t.as_str().map(|t| t == table.id))
        });
    if named == Some(false) {
        return Err(format!(
            "step {step_key} writes only table {}, and this call names another table.",
            table.id
        ));
    }
    if table.team != step.seat_team {
        return Err(format!(
            "table {} belongs to team {}, and seat {seat} is in team {}; a seat writes only its own team's tables.",
            table.id, table.team, step.seat_team
        ));
    }
    Ok(())
}

/// The engine result of the `table_write` call at (`step_run_id`,
/// `call_seq`), rebuilt from the store's data-free receipt of its committed
/// write (AD-8; AD-10 as amended 2026-11-24): exactly the result the model
/// received for it, whatever the row's state now. `None` for a call that
/// wrote no row (refused, failed, never journaled) and an unknown step
/// run. Nothing is parsed from journal text or tool output.
pub fn engine_result_of(
    reader: &Reader,
    step_run_id: &str,
    call_seq: i64,
) -> Result<Option<EngineResult>, StoreError> {
    let Some(receipt) = reader.row_receipt(step_run_id, call_seq)? else {
        return Ok(None);
    };
    let row_id = uuid::Uuid::parse_str(&receipt.row_id).map_err(|e| {
        StoreError::Invalid(format!(
            "the receipt of call {call_seq} of step run {step_run_id} names a row id that is not a UUID: {e}"
        ))
    })?;
    let status = match receipt.op.as_str() {
        "insert" => RowStatus::Inserted,
        "update" => RowStatus::Updated,
        other => {
            return Err(StoreError::Invalid(format!(
                "the receipt of call {call_seq} of step run {step_run_id} records the op `{other}`, which no row write makes"
            )));
        }
    };
    Ok(Some(EngineResult::row_written(&RowWritten {
        row_id,
        version: receipt.version,
        status,
    })))
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

    /// Story 3.6: the step's seat is granted `lookup`.
    fn granted() -> StepTools {
        StepTools {
            grants: ["lookup".to_string()].into_iter().collect(),
            seat_team: "core".to_string(),
            table: None,
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

        assert_eq!(
            gate.check(&a, &granted(), &call("lookup")),
            GateDecision::Allow
        );
        assert_eq!(
            gate.check(&a, &granted(), &call("lookup")),
            GateDecision::Allow
        );
        let third = gate.check(&a, &granted(), &call("lookup"));
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
                gate.check(&a, &granted(), &call("lookup")),
                GateDecision::Allow,
                "call {i}"
            );
        }
        assert!(refused(&gate.check(&a, &granted(), &call("lookup"))));

        // Another seat keeps the manifest's limit.
        let b = seat("seat-y", Uuid::now_v7(), "s1");
        assert_eq!(
            gate.check(&b, &granted(), &call("lookup")),
            GateDecision::Allow
        );
        assert_eq!(
            gate.check(&b, &granted(), &call("lookup")),
            GateDecision::Allow
        );
        assert!(refused(&gate.check(&b, &granted(), &call("lookup"))));
    }

    #[test]
    fn counts_are_per_step() {
        let audit = CollectAudit::new();
        let gate = ToolGate::new(vec![manifest("lookup", 1)], audit.clone());
        let run = Uuid::now_v7();
        let s1 = seat("seat-x", run, "s1");
        let s2 = seat("seat-x", run, "s2");

        assert_eq!(
            gate.check(&s1, &granted(), &call("lookup")),
            GateDecision::Allow
        );
        assert!(refused(&gate.check(&s1, &granted(), &call("lookup"))));
        assert_eq!(
            gate.check(&s2, &granted(), &call("lookup")),
            GateDecision::Allow
        );

        // Same step name in another run is another step.
        let other_run = seat("seat-x", Uuid::now_v7(), "s1");
        assert_eq!(
            gate.check(&other_run, &granted(), &call("lookup")),
            GateDecision::Allow
        );
    }

    #[test]
    fn limit_zero_refuses_first_call() {
        let audit = CollectAudit::new();
        let gate = ToolGate::new(vec![manifest("lookup", 0)], audit.clone());
        let a = seat("seat-x", Uuid::now_v7(), "s1");

        assert!(refused(&gate.check(&a, &granted(), &call("lookup"))));
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
        match gate.check(&a, &granted(), &call("no_such_tool")) {
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

        assert_eq!(
            gate.check(&a, &granted(), &call("lookup")),
            GateDecision::Allow
        );
        assert_eq!(
            gate.check(&a, &granted(), &call("lookup")),
            GateDecision::Allow
        );
        for _ in 0..3 {
            assert!(refused(&gate.check(&a, &granted(), &call("lookup"))));
        }
        // An unknown-tool refusal in the same step does not touch the count of
        // another tool in a fresh step.
        let b = seat("seat-x", Uuid::now_v7(), "s1");
        assert!(refused(&gate.check(&b, &granted(), &call("nope"))));
        assert_eq!(
            gate.check(&b, &granted(), &call("lookup")),
            GateDecision::Allow
        );
        assert_eq!(
            gate.check(&b, &granted(), &call("lookup")),
            GateDecision::Allow
        );
        assert!(refused(&gate.check(&b, &granted(), &call("lookup"))));

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
        assert!(refused(&gate.check(&sys, &granted(), &call("lookup"))));
        assert_eq!(audit.events().len(), 1);
    }
}
