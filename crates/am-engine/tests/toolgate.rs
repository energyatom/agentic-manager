//! Story 3.3 review patch round 2 (coordinator's triage, 2026-11-23): R7
//! (only ToolGate runs a tool, and that holds by construction: a `Permit`
//! runs only on the gate that admitted it; a runner whose manifests repeat
//! an id is refused) and R9 (ToolGate labels the tool names it audits
//! itself, so a raw model-chosen name never reaches the audit sink or the
//! refusal's words, whatever the caller passes). Through ToolGate's own
//! API (`ToolGate::over`, `admit`, `run`) and `run_plan_with_tools`, with
//! test-local runners; tests are not shipped. Written before the patches.
#![forbid(unsafe_code)]

use std::sync::{Arc, Mutex};

use am_domain::tool_manifest::{
    ActionClass as Declared, NetworkNeed, ToolManifest as DomainManifest,
};
use am_domain::{Actor, BoxFuture, CancelSignal, Instruction, ToolCall, ToolRunner, UntrustedData};
use am_engine::TokenSignal;
use am_engine::run::{HandOff, Limits, RunPlan, StepPlan, run_plan_with_tools};
use am_engine::toolgate::ToolGate;
use am_store::{Reader, Store};
use am_testkit::{CollectAudit, FakeLlm, ScriptedTurn};
use tokio_util::sync::CancellationToken;

/// A test-local runner: manifests as given (ids may repeat), and the id of
/// every call it ran.
struct Runner {
    manifests: Vec<(&'static str, Declared)>,
    ran: Mutex<Vec<String>>,
}

impl Runner {
    fn new(manifests: Vec<(&'static str, Declared)>) -> Arc<Runner> {
        Arc::new(Runner {
            manifests,
            ran: Mutex::new(Vec::new()),
        })
    }

    fn ran(&self) -> Vec<String> {
        self.ran.lock().unwrap().clone()
    }
}

impl ToolRunner for Runner {
    fn manifests(&self) -> Vec<DomainManifest> {
        self.manifests
            .iter()
            .map(|(id, class)| DomainManifest {
                id: id.to_string(),
                class: *class,
                network: NetworkNeed::None,
                max_uses_per_step: 100,
            })
            .collect()
    }

    fn run(
        &self,
        call: ToolCall,
        _cancel: Arc<dyn CancelSignal>,
    ) -> BoxFuture<Result<UntrustedData, String>> {
        self.ran.lock().unwrap().push(call.name.clone());
        Box::pin(async move { Ok(UntrustedData::text(format!("tool:{}", call.name), "ran")) })
    }
}

fn seat() -> Actor {
    Actor::Seat {
        seat: "seat-work".into(),
        run: uuid::Uuid::now_v7(),
        step: "work".into(),
    }
}

fn call(name: &str) -> ToolCall {
    ToolCall {
        id: "call-1".into(),
        name: name.into(),
        arguments_json: "{}".into(),
    }
}

fn cancel() -> Arc<dyn CancelSignal> {
    TokenSignal::signal(CancellationToken::new())
}

/// R7: a `Permit` runs only on the gate that admitted it. Two gates over
/// two runners with the same tool: a permit admitted by the first, handed
/// to the second, is refused, and neither runner runs it.
#[tokio::test]
async fn a_permit_from_one_gate_is_refused_by_another() {
    let (a, b) = (
        Runner::new(vec![("lookup", Declared::Read)]),
        Runner::new(vec![("lookup", Declared::Read)]),
    );
    let gate_a = ToolGate::over(a.clone(), CollectAudit::new());
    let gate_b = ToolGate::over(b.clone(), CollectAudit::new());
    let permit = gate_a
        .admit(&seat(), "step-run-1", &call("lookup"))
        .expect("gate A admits a read tool");

    let ran = gate_b.run(permit, cancel()).await;

    assert!(ran.is_err(), "another gate's permit is refused: {ran:?}");
    assert!(b.ran().is_empty(), "gate B's runner never runs it");
    assert!(a.ran().is_empty(), "nor gate A's");
}

/// R7, the control: the gate that admitted the permit runs it.
#[tokio::test]
async fn a_permit_runs_on_its_own_gate() {
    let a = Runner::new(vec![("lookup", Declared::Read)]);
    let gate = ToolGate::over(a.clone(), CollectAudit::new());
    let permit = gate.admit(&seat(), "step-run-1", &call("lookup")).unwrap();

    let ran = gate.run(permit, cancel()).await;

    assert!(ran.is_ok(), "{ran:?}");
    assert_eq!(a.ran(), vec!["lookup"]);
}

/// R7: a runner whose manifests name one id twice is refused (which
/// manifest would decide is not left to chance): with tools, the run never
/// runs that tool, and nothing reports success.
#[tokio::test]
async fn duplicate_manifest_ids_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state.db");
    let store = Store::open(&db).unwrap();
    // The same id, declared external-effect first and read second.
    let runner = Runner::new(vec![
        ("twice_named", Declared::ExternalEffect),
        ("twice_named", Declared::Read),
    ]);
    let llm = Arc::new(FakeLlm::new(vec![
        ScriptedTurn::text("looking").tool_call("twice_named", "{}"),
        ScriptedTurn::text("done"),
    ]));
    let plan = RunPlan {
        workflow: "wf".into(),
        config_hash: "0".repeat(64),
        steps: vec![StepPlan {
            key: "work".into(),
            seat: "seat-work".into(),
            system: vec![],
            instruction: Instruction::authored("Do the work."),
            inputs: vec![],
            hand_off: HandOff::Next,
            limits: Limits::default(),
            model: "example-local".into(),
            max_output_tokens: 64,
            provider: llm,
            prompt: am_engine::prompt::StepPrompt::default(),
            output_schema: None,
        }],
        clock: am_testkit::FakeClock::at("2026-11-24T10:00:00Z"),
    };
    let owner = Actor::Human {
        person: "owner".into(),
        hat: "Owner".into(),
    };

    let result = run_plan_with_tools(
        &store,
        owner,
        "manual",
        &plan,
        CancellationToken::new(),
        runner.clone(),
    )
    .await;

    assert!(runner.ran().is_empty(), "the tool never ran: {result:?}");
    match &result {
        Err(e) => assert!(e.to_string().contains("twice_named"), "{e}"),
        Ok(outcome) => {
            assert_eq!(outcome.status, "failed", "{outcome:?}");
            let reason = outcome.reason.clone().unwrap_or_default();
            assert!(reason.contains("twice_named"), "{reason}");
            let reader = Reader::open(&db).unwrap();
            let step = reader.steps(&outcome.run_id).unwrap().remove(0);
            assert!(
                reader
                    .tool_calls(&step.id)
                    .unwrap()
                    .iter()
                    .all(|t| t.status == "refused"),
                "no call of it is allowed"
            );
        }
    }
}

/// The sha256 of `text`'s UTF-8 bytes, lowercase hex.
fn sha256_hex(text: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(text.as_bytes()))
}

/// R9: ToolGate labels the names it audits and words itself. A caller that
/// passes the model's raw name (an unknown one, one with control
/// characters) gets a refusal, and the audit sink an entry, that hold
/// `unknown:<sha256 of the name>` and nothing of the raw name; a name a
/// manifest holds, or a built-in tool id, stays as itself.
#[test]
fn toolgate_labels_the_names_it_audits() {
    for (what, name, marker) in [
        (
            "an unknown name",
            "exfiltrate_every_key".to_string(),
            "exfiltrate_every_key".to_string(),
        ),
        (
            "control characters",
            "zq9\u{1b}[2J\u{7}wipe\nname".to_string(),
            "zq9".to_string(),
        ),
        ("10 KB", "x".repeat(10 * 1024), "x".repeat(64)),
    ] {
        let audit = CollectAudit::new();
        let gate = ToolGate::over(Runner::new(vec![("lookup", Declared::Read)]), audit.clone());

        let refused = gate
            .admit(&seat(), "step-run-1", &call(&name))
            .expect_err("an unknown tool is refused");

        let label = format!("unknown:{}", sha256_hex(&name));
        assert!(
            !refused.contains(&marker),
            "{what}: the words: {refused:.200}"
        );
        assert!(!refused.chars().any(char::is_control), "{what}");
        let events = audit.events();
        assert_eq!(events.len(), 1, "{what}");
        let e = &events[0];
        let seen = format!("{} {} {:?}", e.target, e.outcome, e.args);
        assert!(
            !seen.contains(&marker),
            "{what}: the audit entry: {seen:.300}"
        );
        assert!(!seen.chars().any(char::is_control), "{what}: {seen:.300}");
        assert!(e.target.contains(&label), "{what}: labelled: {}", e.target);
    }

    // Names a manifest holds, and built-in ids, stay as themselves.
    let audit = CollectAudit::new();
    let gate = ToolGate::over(
        Runner::new(vec![("lookup", Declared::ExternalEffect)]),
        audit.clone(),
    );
    assert!(gate.admit(&seat(), "step-run-1", &call("lookup")).is_err());
    assert!(
        gate.admit(&seat(), "step-run-1", &call("web_search"))
            .is_err()
    );
    let targets: Vec<String> = audit.events().into_iter().map(|e| e.target).collect();
    assert!(targets[0].contains("tool:lookup"), "{targets:?}");
    assert!(targets[1].contains("tool:web_search"), "{targets:?}");
}
