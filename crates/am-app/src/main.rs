//! The `agentic` binary. Commands:
//!
//! - `agentic serve [--project <path>] [--port N] [--no-browser]`
//! - `agentic validate [<project>]` (exit 0 no errors, 1 errors, 2 usage or no folder)
//! - `agentic audit export <project> [--out <file>]`, `agentic audit verify <file>`
//!   (exit 0 OK, 1 failure or a broken chain, 2 usage)
//! - `agentic spike c1 [--provider anthropic|lmstudio|ollama] [--model M]`
//! - `agentic spike c3 --db PATH [--cron EXPR] [--tz ZONE]` (sleep/wake demo)
//! - `agentic spike c4 [--dir PATH] [--anthropic-model M] [--local-model M] [--no-anthropic] [--no-local] [--out FILE]`
//! - `agentic spike c7` (Keychain store, read, delete with a canary)
//! - `agentic spike local-context` (LM Studio's loaded context length)
#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use am_app::spike;
use am_domain::{HttpRequest, LlmProvider, SecretString};
use am_providers::{AnthropicProvider, OpenAiCompatProvider};

struct Args(Vec<String>);

impl Args {
    fn flag(&self, name: &str) -> bool {
        self.0.iter().any(|a| a == name)
    }
    fn value(&self, name: &str) -> Option<String> {
        self.0
            .iter()
            .position(|a| a == name)
            .and_then(|i| self.0.get(i + 1))
            .cloned()
    }
}

fn fail(msg: &str) -> ! {
    eprintln!("agentic: {msg}");
    std::process::exit(1);
}

#[tokio::main]
async fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = Args(argv.clone());
    match argv.first().map(String::as_str) {
        Some("serve") => serve(&args).await,
        Some("validate") => {
            let code =
                am_app::cli::validate(&argv[1..], &mut std::io::stdout(), &mut std::io::stderr());
            std::process::exit(code);
        }
        Some("audit") => {
            let code =
                am_app::cli::audit(&argv[1..], &mut std::io::stdout(), &mut std::io::stderr());
            std::process::exit(code);
        }
        Some("__spike-run") => spike_run(&args).await,
        Some("spike") => match argv.get(1).map(String::as_str) {
            Some("c1") => c1(&args).await,
            Some("c3") => c3(&args).await,
            Some("c4") => c4(&args).await,
            Some("c7") => c7(),
            Some("local-context") => local_context().await,
            _ => fail("usage: agentic spike c1|c3|c4|c7|local-context"),
        },
        Some("--version") => println!("agentic {}", env!("CARGO_PKG_VERSION")),
        _ => {
            eprintln!(
                "usage: agentic serve [--project <path>] | agentic validate [<project>] | agentic audit export|verify | agentic spike c1|c3|c4|c7|local-context"
            );
            std::process::exit(2);
        }
    }
}

async fn serve(args: &Args) {
    // With a project: open it (owner-only `.agentic/`, the exclusive lock,
    // backup and migrations), print the report and hold the store (whose
    // writer owns the lock) for the server's life (AD-32, AD-7). Without
    // one, nothing is opened.
    let project = args.value("--project");
    if args.flag("--project") && project.as_deref().is_none_or(|p| p.starts_with("--")) {
        eprintln!("usage: agentic serve [--project <path>] [--port N] [--no-browser]");
        eprintln!("agentic: --project needs the project folder");
        std::process::exit(2);
    }
    let _store = project.map(|p| {
        let root = PathBuf::from(&p);
        let (store, report) = am_store::open_project(&root)
            .unwrap_or_else(|e| fail(&format!("cannot open the project {p}: {e}")));
        let lines = report.lines();
        if lines.is_empty() {
            println!(
                "agentic: project {p}: database at version {}",
                report.version
            );
        }
        for line in lines {
            println!("agentic: project {p}: {line}");
        }
        store
    });
    let port: u16 = args
        .value("--port")
        .and_then(|p| p.parse().ok())
        .unwrap_or(0);
    let (listener, port) = am_server::bind(port)
        .await
        .unwrap_or_else(|e| fail(&format!("cannot bind 127.0.0.1: {e}")));
    let state = am_server::AppState::new(port);
    println!("agentic: serving on http://127.0.0.1:{port}");
    if !args.flag("--no-browser") {
        // The launch code goes only to the browser, never to stdout (AD-13).
        let url = state.launch_url();
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("open").arg(&url).status();
        #[cfg(not(target_os = "macos"))]
        {
            let _ = url;
            eprintln!("agentic: opening a browser is not supported on this platform yet");
        }
    }
    let shutdown = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    if let Err(e) = am_server::serve(listener, state, shutdown).await {
        fail(&e.to_string());
    }
}

async fn spike_run(args: &Args) {
    let db = PathBuf::from(
        args.value("--db")
            .unwrap_or_else(|| fail("--db is required")),
    );
    let steps = args
        .value("--steps")
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);
    let delay = Duration::from_millis(
        args.value("--delay-ms")
            .and_then(|s| s.parse().ok())
            .unwrap_or(500),
    );
    match spike::spike_run(&db, steps, delay, args.flag("--resume")).await {
        Ok((run, status)) => println!("run {run} {status}"),
        Err(e) => fail(&e),
    }
}

fn anthropic_model(args: &Args) -> String {
    args.value("--anthropic-model")
        .or_else(|| args.value("--model"))
        .or_else(|| std::env::var("AM_SPIKE_ANTHROPIC_MODEL").ok())
        .unwrap_or_else(|| "claude-opus-5-5".into())
}

/// The LM Studio model to use: named with the flag or `AM_SPIKE_LOCAL_MODEL`.
/// It is never guessed (the first listed model may be an embedding or a
/// text-only model).
fn local_model(named: Option<String>) -> Result<String, String> {
    named
        .or_else(|| std::env::var("AM_SPIKE_LOCAL_MODEL").ok())
        .ok_or_else(|| {
            "name the LM Studio model with --local-model (c4), --model (c1) or AM_SPIKE_LOCAL_MODEL: a tool-capable model for C1, a vision-capable model for C4".to_string()
        })
}

async fn c1(args: &Args) {
    let secrets = am_app::keychain();
    let egress = am_app::default_egress(secrets);
    let which = args
        .value("--provider")
        .unwrap_or_else(|| "anthropic".into());
    let (provider, model): (Arc<dyn LlmProvider>, String) = match which.as_str() {
        "anthropic" => (
            Arc::new(AnthropicProvider::new(
                egress.clone(),
                am_app::SPIKE_ANTHROPIC_ACCOUNT,
            )),
            anthropic_model(args),
        ),
        "lmstudio" => {
            let m = local_model(args.value("--model")).unwrap_or_else(|e| fail(&e));
            (Arc::new(OpenAiCompatProvider::lm_studio(egress.clone())), m)
        }
        "ollama" => (
            Arc::new(OpenAiCompatProvider::ollama(egress.clone())),
            args.value("--model")
                .unwrap_or_else(|| fail("--model is required for ollama")),
        ),
        other => fail(&format!("unknown provider {other}")),
    };
    match spike::c1_probe(provider, &model, 2048).await {
        Ok(r) => {
            println!("{r:#?}");
            println!("C1 {}: {}", which, if r.passed() { "PASS" } else { "FAIL" });
        }
        Err(e) => fail(&e.to_string()),
    }
}

async fn c3(args: &Args) {
    use am_engine::scheduler::{CatchUp, Scheduler};
    struct SystemClock;
    impl am_domain::Clock for SystemClock {
        fn now(&self) -> jiff::Timestamp {
            jiff::Timestamp::now()
        }
    }
    let db = PathBuf::from(
        args.value("--db")
            .unwrap_or_else(|| fail("--db is required")),
    );
    let cron = args.value("--cron").unwrap_or_else(|| "*/5 * * * *".into());
    let tz = args.value("--tz").unwrap_or_else(|| {
        jiff::tz::TimeZone::system()
            .iana_name()
            .unwrap_or("UTC")
            .to_string()
    });
    let store = am_store::Store::open(&db).unwrap_or_else(|e| fail(&e.to_string()));
    let mut s = Scheduler::new(Arc::new(SystemClock), store.writer(), db.clone());
    s.add_trigger("lid-demo", &cron, &tz, CatchUp::Once)
        .unwrap_or_else(|e| fail(&e.to_string()));
    println!(
        "{} scheduler started: cron `{cron}` in {tz}; close the lid across a fire time, then open it.",
        jiff::Timestamp::now()
    );
    let token = tokio_util::sync::CancellationToken::new();
    let t2 = token.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        t2.cancel();
    });
    let r = s
        .run(token, |fire, jump| {
            println!(
                "{} FIRE trigger={} scheduled_for={} kind={}{}",
                jiff::Timestamp::now(),
                fire.trigger_id,
                fire.scheduled_for,
                fire.kind.as_str(),
                jump.map(|j| format!(" (wall clock jumped {}s: sleep/wake)", j.as_secs()))
                    .unwrap_or_default()
            );
        })
        .await;
    if let Err(e) = r {
        fail(&e.to_string());
    }
}

async fn c4(args: &Args) {
    let dir = args
        .value("--dir")
        .or_else(|| std::env::var("AM_SPIKE_DOCS_DIR").ok())
        .unwrap_or_else(|| fail("AM_SPIKE_DOCS_DIR not set"));
    let secrets = am_app::keychain();
    let egress = am_app::default_egress(secrets);
    let mut models = Vec::new();
    if !args.flag("--no-anthropic") {
        let m = anthropic_model(args);
        models.push(spike::ModelUnderTest {
            label: format!("frontier: anthropic {m}"),
            kind: spike::ModelKind::Frontier,
            provider: Arc::new(AnthropicProvider::new(
                egress.clone(),
                am_app::SPIKE_ANTHROPIC_ACCOUNT,
            )),
            price: Some(spike::anthropic_price(&m)),
            model: m,
        });
    }
    if !args.flag("--no-local") {
        match local_model(args.value("--local-model")) {
            Ok(m) => models.push(spike::ModelUnderTest {
                label: format!("local: lmstudio {m} (vision)"),
                kind: spike::ModelKind::Local,
                provider: Arc::new(
                    OpenAiCompatProvider::lm_studio(egress.clone()).with_vision(true),
                ),
                price: None,
                model: m,
            }),
            Err(e) => eprintln!("agentic: local half not run: {e}"),
        }
    }
    let audit = Arc::new(PrintAudit);
    let rows = spike::files_to_models(&PathBuf::from(dir), &models, audit)
        .await
        .unwrap_or_else(|e| fail(&e));
    let table = spike::c4_table(&rows);
    println!("{table}");
    match spike::c4_verdict(&rows) {
        Ok(()) => println!("C4: PASS"),
        Err(why) => println!("C4: FAIL\n{why}"),
    }
    if let Some(out) = args.value("--out") {
        std::fs::write(&out, &table).unwrap_or_else(|e| fail(&e.to_string()));
    }
}

struct PrintAudit;
impl am_domain::AuditSink for PrintAudit {
    fn record(&self, e: am_domain::AuditEvent) {
        eprintln!("audit {} {} {}", e.action, e.target, e.outcome);
    }
}

fn c7() {
    let store = am_secrets::KeychainStore::new();
    use am_domain::SecretStore;
    let account = format!("spike-canary-{}", std::process::id());
    let canary = "am-canary-7f3c9a";
    let steps = (|| -> Result<(), am_domain::SecretError> {
        store.set(&account, SecretString::new(canary.into()))?;
        let got = store.get(&account)?;
        if got.as_ref().map(|s| s.expose()) != Some(canary) {
            return Err(am_domain::SecretError::Store(
                "read back a different value".into(),
            ));
        }
        store.delete(&account)?;
        if store.get(&account)?.is_some() {
            return Err(am_domain::SecretError::Store(
                "still present after delete".into(),
            ));
        }
        Ok(())
    })();
    match steps {
        Ok(()) => {
            println!("C7: PASS (stored, read and deleted {account} under service agentic-manager)")
        }
        Err(e) => fail(&format!("C7: FAIL: {e}")),
    }
}

async fn local_context() {
    let egress = am_app::default_egress(am_app::keychain());
    let resp = egress
        .send(HttpRequest::get("http://127.0.0.1:1234/api/v1/models"))
        .await
        .unwrap_or_else(|e| fail(&format!("LM Studio not reachable: {e}")));
    let body = am_net::read_all(resp.body.as_ref())
        .await
        .unwrap_or_else(|e| fail(&e.to_string()));
    println!("{}", String::from_utf8_lossy(&body));
}
