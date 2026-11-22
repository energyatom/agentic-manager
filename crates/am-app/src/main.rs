//! The `agentic` binary. Commands:
//!
//! - `agentic serve [--project <path> [--confirm-move] [--take-over]] [--port N] [--no-browser]`
//! - `agentic validate [--template] [<folder>]` (exit 0 no errors, 1 errors, 2 usage or no folder)
//! - `agentic audit export <project> [--out <file>]`, `agentic audit verify <file>`
//!   (exit 0 OK, 1 failure or a broken chain, 2 usage)
//! - `agentic accounts add <id> --provider anthropic [--name <text>]`, `remove <id>`,
//!   `list`, `test <id> [--model M]`, each with `[--project <folder>]` (story
//!   2.1, AD-15's interim CLI Account edit; add and remove only in a terminal)
//! - `agentic run <workflow> [--project <folder>]` (epic 3 story 1: one run of
//!   a configured workflow, only in a terminal; exit 0 succeeded, 1 failed or
//!   refused, 2 usage) and `agentic status [--project <folder>]` (the runs,
//!   newest first, with their steps; reads only)
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
        Some("serve") => {
            // Every exit after the marker is written goes through here, after
            // `serve` has returned and dropped the open project (the marker
            // first, then the store's lock): no `process::exit` in between.
            let code = serve(&args).await;
            std::process::exit(code);
        }
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
        Some("accounts") => {
            // The code is returned after the command dropped the store (and
            // its lock): no `process::exit` in between.
            let code = accounts(&argv[1..]).await;
            std::process::exit(code);
        }
        Some("run") => {
            // The code is returned after the command dropped the open
            // project (the marker, then the store's lock): no
            // `process::exit` in between.
            let code = run(&argv[1..]).await;
            std::process::exit(code);
        }
        Some("status") => {
            let code = status(&argv[1..]);
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
                "usage: agentic serve [--project <path>] | agentic validate [<project>] | agentic audit export|verify | agentic accounts add|remove|list|test | agentic run <workflow> | agentic status | agentic spike c1|c3|c4|c7|local-context"
            );
            std::process::exit(2);
        }
    }
}

/// What the binary gives every CLI-class command (AD-15): this platform's
/// places, the OS identity adapter and the owner's real terminal. This is
/// the one place the binary builds a terminal (the release guard in
/// `xtask/tests/release_deps.rs` counts them): `accounts` and `run` both
/// take theirs from here, and nothing can swap it for another. `Err` is the
/// exit code, after the reason was printed.
fn owner_side() -> Result<am_app::cli::OwnerSide, i32> {
    let places = match am_app::runtime::Places::from_env() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("agentic: {e}");
            return Err(1);
        }
    };
    Ok(am_app::cli::OwnerSide {
        places,
        identity: am_app::identity::system(),
        terminal: Box::new(am_app::terminal::OwnerTerminal::new()),
    })
}

/// `agentic accounts add|remove|list|test` (story 2.1, AD-15): the command
/// with the owner's side ([`owner_side`]), the Keychain and `am-net`'s
/// client.
async fn accounts(args: &[String]) -> i32 {
    use am_app::cli::{AccountsContext, OwnerSide};
    let OwnerSide {
        places,
        identity,
        terminal,
    } = match owner_side() {
        Ok(owner) => owner,
        Err(code) => return code,
    };
    let secrets = am_app::keychain();
    let for_egress = secrets.clone();
    let ctx = AccountsContext {
        places,
        identity,
        terminal,
        secrets,
        egress: Arc::new(move |policy| {
            Arc::new(am_net::NetEgress::new(policy, for_egress.clone(), None))
                as Arc<dyn am_domain::EgressClient>
        }),
    };
    am_app::cli::accounts(args, ctx, &mut std::io::stdout(), &mut std::io::stderr()).await
}

/// `agentic run <workflow>` (epic 3 story 1, AD-15): the command with the
/// owner's side ([`owner_side`]) and the factory that wires no provider
/// yet. Nothing here can swap in a fake provider.
async fn run(args: &[String]) -> i32 {
    use am_app::cli::{OwnerSide, RunContext};
    let OwnerSide {
        places,
        identity,
        terminal,
    } = match owner_side() {
        Ok(owner) => owner,
        Err(code) => return code,
    };
    let ctx = RunContext {
        places,
        identity,
        terminal,
        providers: am_app::cli::unwired_providers(),
    };
    am_app::cli::run(args, ctx, &mut std::io::stdout(), &mut std::io::stderr()).await
}

/// `agentic status` (epic 3 story 1): reads the project's runs without its
/// lock, with any stdin.
fn status(args: &[String]) -> i32 {
    match am_app::runtime::Places::from_env() {
        Ok(places) => am_app::cli::status(
            args,
            &places,
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        ),
        Err(e) => {
            eprintln!("agentic: {e}");
            1
        }
    }
}

const SERVE_USAGE: &str = "usage: agentic serve [--project <path> [--confirm-move] [--take-over]] [--port N] [--no-browser]";

/// `agentic serve`: with a project, resolves and opens it (AD-32, story
/// 1.13: the runtime folder outside the project, the marker, the store and
/// the config writer), prints the report and holds it for the server's
/// life. Returns the exit code; the open project is dropped (marker removed,
/// then the store's lock released) before it returns.
async fn serve(args: &Args) -> i32 {
    let project = args.value("--project");
    if args.flag("--project") && project.as_deref().is_none_or(|p| p.starts_with("--")) {
        eprintln!("{SERVE_USAGE}");
        eprintln!("agentic: --project needs the project folder");
        return 2;
    }
    if project.is_none() && (args.flag("--confirm-move") || args.flag("--take-over")) {
        eprintln!("{SERVE_USAGE}");
        eprintln!("agentic: --confirm-move and --take-over need --project");
        return 2;
    }
    // Installed before the project opens (see `stop_signal`).
    let stop = stop_signal();
    let opened = match &project {
        None => None,
        Some(p) => match open(p, args) {
            Ok(o) => Some(o),
            Err(e) => {
                eprintln!("agentic: cannot open the project {p}: {e}");
                return 1;
            }
        },
    };
    if let (Some(o), Some(p)) = (&opened, &project) {
        for line in &o.lines {
            println!("agentic: project {p}: {line}");
        }
    }
    let port: u16 = args
        .value("--port")
        .and_then(|p| p.parse().ok())
        .unwrap_or(0);
    let (listener, port) = match am_server::bind(port).await {
        Ok(b) => b,
        Err(e) => {
            eprintln!("agentic: cannot bind 127.0.0.1: {e}");
            return 1;
        }
    };
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
    let code = match am_server::serve(listener, state, stop).await {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("agentic: {e}");
            1
        }
    };
    drop(opened);
    code
}

/// Resolves and opens the project at `p` (am-app's runtime resolver over
/// the OS identity adapter, with the terminal for `--confirm-move` and
/// `--take-over`).
fn open(p: &str, args: &Args) -> Result<am_app::runtime::Opened, am_app::runtime::Refusal> {
    use am_app::runtime::{OpenRequest, Places, TerminalPrompt, open_project};
    let places = Places::from_env()?;
    let identity = am_app::identity::system();
    let req = OpenRequest {
        root: PathBuf::from(p),
        confirm_move: args.flag("--confirm-move"),
        take_over: args.flag("--take-over"),
    };
    open_project(&places, identity.as_ref(), &req, &mut TerminalPrompt)
}

/// Resolves on Ctrl-C (SIGINT), SIGTERM or SIGHUP: a clean close.
/// The stop signals, installed when this is called, not when the returned
/// future is first polled: `serve` calls it before the project opens, so a
/// SIGINT, SIGTERM or SIGHUP that arrives after the marker is written but
/// before the server runs still ends in a clean stop that removes the marker
/// (AD-32), instead of the default action killing the process.
fn stop_signal() -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        if let (Ok(mut int), Ok(mut term), Ok(mut hup)) = (
            signal(SignalKind::interrupt()),
            signal(SignalKind::terminate()),
            signal(SignalKind::hangup()),
        ) {
            return Box::pin(async move {
                tokio::select! {
                    _ = int.recv() => {}
                    _ = term.recv() => {}
                    _ = hup.recv() => {}
                }
            });
        }
    }
    Box::pin(async {
        let _ = tokio::signal::ctrl_c().await;
    })
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
        .unwrap_or_else(|| am_app::DEFAULT_ANTHROPIC_MODEL.into())
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
