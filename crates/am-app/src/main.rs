//! The `agentic` binary. Commands:
//!
//! - `agentic serve [--project <path> [--confirm-move] [--take-over]] [--port N] [--no-browser]`
//!   (story 3.2: prints the address without any code, where this launch's
//!   CLI token file is, and a terminal pairing code; while serve runs in the
//!   foreground of a terminal, pressing Enter prints a new pairing code;
//!   story 3.19: the terminal is read only while serve is its foreground
//!   job, so `agentic serve &` keeps answering)
//! - `agentic validate [--template] [<folder>]` (exit 0 no errors, 1 errors, 2 usage or no folder)
//! - `agentic audit export <project> [--out <file>]`, `agentic audit verify <file>`
//!   (exit 0 OK, 1 failure or a broken chain, 2 usage)
//! - `agentic accounts add <id> --provider anthropic|openai|openai-compatible [--base-url <url>] [--name <text>]`, `remove <id>`,
//!   `list`, `test <id> [--model M]` (story 2.1, AD-15's interim CLI Account
//!   edit; add and remove only in a terminal; since story 2.12 no project),
//!   and `migrate --project <folder> [--as <old>=<new>]... [--dry-run |
//!   --rollback]` (story 2.13: a project's old `models/accounts.yaml` to this
//!   Mac's Accounts; only in a terminal, `--dry-run` anywhere)
//! - `agentic run <workflow> [--project <folder>]` (epic 3 story 1: one run of
//!   a configured workflow, only in a terminal; exit 0 succeeded, 1 failed or
//!   refused, 2 usage) and `agentic status [--project <folder>]` (the runs,
//!   newest first, with their steps; reads only)
//! - `agentic spike c3 --db PATH [--cron EXPR] [--tz ZONE]` (sleep/wake demo)
//! - `agentic spike c7` (Keychain store, read, delete with a canary)
//! - `agentic spike local-context` (LM Studio's loaded context length)
//!
//! The spike's `c1` and `c4`, which read the Keychain entry
//! `spike-anthropic` and called a provider with no configured Account, left
//! the binary in story 2.2 (owner decision 9, 2026-11-22); their recorded
//! evidence is `docs/spike-report.md`, and the live tests in
//! `crates/am-e2e/tests/spike/` still run them on the owner's Mac.
#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use am_app::spike;
use am_domain::{Caller, HttpRequest, SecretString};

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
        Some("gate") => {
            // The code is returned after the command dropped the open
            // project (the marker, then the store's lock): no
            // `process::exit` in between.
            let code = gate(&argv[1..]).await;
            std::process::exit(code);
        }
        Some("__spike-run") => spike_run(&args).await,
        Some("spike") => match argv.get(1).map(String::as_str) {
            Some("c3") => c3(&args).await,
            Some("c7") => c7(),
            Some("local-context") => local_context().await,
            _ => {
                eprintln!("usage: agentic spike c3|c7|local-context");
                std::process::exit(2);
            }
        },
        Some("--version") => println!("agentic {}", env!("CARGO_PKG_VERSION")),
        _ => {
            eprintln!(
                "usage: agentic serve [--project <path>] | agentic validate [<project>] | agentic audit export|verify | agentic accounts add|remove|list|test|migrate | agentic run <workflow> | agentic status | agentic gate [<item id>] | agentic spike c3|c7|local-context"
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

/// `agentic accounts add|remove|list|test|migrate` (stories 2.1, 2.13; AD-15): the command
/// with the owner's side ([`owner_side`]), the Keychain and `am-net`'s
/// client over the directory and audit sink the command hands it.
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
        egress: Arc::new(move |directory, audit| {
            Arc::new(am_net::NetEgress::new(directory, for_egress.clone(), audit))
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
        clock: am_app::cli::system_clock(),
    };
    am_app::cli::run(args, ctx, &mut std::io::stdout(), &mut std::io::stderr()).await
}

/// `agentic gate [<item id>]` (epic 3 story 4, AD-15): the command with the
/// owner's side ([`owner_side`]), from which it reads every decision, and
/// the factory that wires no provider yet (so approve and return are
/// refused before anything is written; reject and dismiss work). Nothing
/// here can swap in a fake terminal or provider, and no option decides.
async fn gate(args: &[String]) -> i32 {
    use am_app::cli::{GateContext, OwnerSide};
    let OwnerSide {
        places,
        identity,
        terminal,
    } = match owner_side() {
        Ok(owner) => owner,
        Err(code) => return code,
    };
    let ctx = GateContext {
        places,
        identity,
        terminal,
        providers: am_app::cli::unwired_providers(),
        clock: am_app::cli::system_clock(),
    };
    am_app::cli::gate(args, ctx, &mut std::io::stdout(), &mut std::io::stderr()).await
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

/// Story 3.19: the usage says when Enter works and how to run serve in the
/// background without it ever reading the terminal.
const SERVE_USAGE: &str = "usage: agentic serve [--project <path> [--confirm-move] [--take-over]] [--port N] [--no-browser]
  With agentic serve in the foreground of a terminal, press Enter there for a new pairing code.
  To run it in the background, start it as `agentic serve < /dev/null &`: it then never reads the terminal.";

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
    // Owner-only at start (story 3.2, AD-13): the app data dir, its
    // `serving` folder and the open project's runtime folder are 0700 and
    // every file in them 0600, or the server does not start.
    let places = match am_app::runtime::Places::from_env() {
        Ok(places) => places,
        Err(e) => {
            eprintln!("agentic: {e}");
            return 1;
        }
    };
    if let Err(e) =
        am_app::serving::owner_only_at_start(&places, opened.as_ref().map(|o| &o.layout))
    {
        eprintln!("agentic: {e}");
        return 1;
    }
    let app_data = places.app_data.clone();
    // What a crashed server left (dead pids' port records and CLI token
    // files) is pruned before this one records its own (review P3).
    let _ = am_app::runtime::serving_ports(&app_data);
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
    // The bound port, recorded owner-only while we serve (removed at exit):
    // in the user-level folder of running servers, with or without a
    // project, and in the project's runtime folder. Every egress directory,
    // `agentic accounts test`'s included, refuses every recorded port to
    // Accounts and MCP servers (AD-12).
    if let Err(e) = am_app::runtime::record_serving(&app_data, port) {
        eprintln!("agentic: {e}");
        return 1;
    }
    if let Some(o) = &opened
        && let Err(e) = am_app::runtime::record_serve_port(&o.layout, port)
    {
        eprintln!("agentic: {e}");
        am_app::runtime::clear_serving(&app_data);
        return 1;
    }
    // The project's egress client, handed the port too. Its blocks go to the
    // project's audit log.
    let egress = match (&opened, &project) {
        (Some(o), Some(p)) => match egress_for(p, o) {
            Ok(egress) => {
                egress.set_own_port(port);
                Some(egress)
            }
            Err(e) => {
                eprintln!("agentic: cannot read the project's network allow-lists: {e}");
                am_app::runtime::clear_serve_port(&o.layout);
                am_app::runtime::clear_serving(&app_data);
                return 1;
            }
        },
        _ => None,
    };
    let mut builder = am_server::AppState::builder(port);
    if let (Some(o), Some(p)) = (&opened, &project) {
        // Story 3.4: the Inbox's resolve records a person's decision and
        // applies it through the project's one writer, and runs `agentic
        // gate`'s check 5 before an approve or a return (review R1).
        builder = builder
            .project_db(o.layout.state_db())
            .project_writer(o.store.writer())
            .pin_check(am_app::serving::pin_check(
                &places,
                &PathBuf::from(p),
                &o.store,
            ));
    }
    let state = builder.build();
    // The per-launch CLI token (AD-13, AD-15): owner-only, beside the port
    // record, removed at exit. Its value is never printed, only its place.
    let pid = std::process::id();
    let token_file = match am_app::serving::write_cli_token(&places, pid, &state.cli_token()) {
        Ok(path) => path,
        Err(e) => {
            eprintln!("agentic: {e}");
            if let Some(o) = &opened {
                am_app::runtime::clear_serve_port(&o.layout);
            }
            am_app::runtime::clear_serving(&app_data);
            return 1;
        }
    };
    // The URL without any code: the launch code goes only to the browser,
    // in the fragment, never to stdout (AD-13).
    println!("agentic: serving on http://127.0.0.1:{port}");
    println!(
        "agentic: the CLI token for this launch is in {} (owner-only; removed at exit)",
        token_file.display()
    );
    // The pairing code, on this terminal only.
    let _ = am_server::print_pairing_code(&state, &mut std::io::stdout());
    // Enter prints a new one, read only when stdin is a terminal (review
    // P9): with stdin redirected (a service, `< /dev/null`, a test) nothing
    // reads it. The Enter key works with serve in the foreground. Off macOS
    // the foreground check is not supported yet (AD-47): no Enter reader
    // and no Enter line there (story 3.19, review P2).
    let enter_for_codes = cfg!(target_os = "macos") && am_app::serving::stdin_reads_the_owner();
    if enter_for_codes {
        println!(
            "agentic: press Enter here for a new pairing code (with agentic serve in the foreground)"
        );
    }
    if !args.flag("--no-browser") {
        let url = state.launch_url();
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("open").arg(&url).status();
        #[cfg(not(target_os = "macos"))]
        {
            let _ = url;
            eprintln!("agentic: opening a browser is not supported on this platform yet");
        }
    }
    if enter_for_codes {
        // Enter in this terminal prints a fresh pairing code (and
        // invalidates the unused one). Story 3.19: it reads only while serve
        // is the terminal's foreground job, asked before each read; in the
        // background it re-checks without touching stdin (1 s, backing off
        // to 10 s), so job control never stops a backgrounded serve
        // (SIGTTIN) and no code is issued there. The thread ends at the end
        // of input or once stdin is no longer a terminal.
        let state = state.clone();
        std::thread::spawn(move || {
            let stdin = std::io::stdin();
            let mut input = am_app::serving::ForegroundInput::new(
                stdin.lock(),
                Box::new(am_app::serving::serve_is_foreground),
                Box::new(am_app::serving::stdin_reads_the_owner),
                Box::new(std::thread::sleep),
            );
            let _ = am_server::pairing_terminal(&state, &mut input, &mut std::io::stdout());
        });
    }
    // Run status changes (also from `agentic run` in another process)
    // become events, once a second.
    let watcher = am_server::watch_runs(&state, Duration::from_secs(1));
    // Story 3.4: the state holds the project's writer (and the pin check,
    // which holds one too) for the Inbox's resolve; both are released
    // below, before the project is dropped.
    let held = state.clone();
    let code = match am_server::serve(listener, state, stop).await {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("agentic: {e}");
            1
        }
    };
    watcher.abort();
    // The client and the server's state hold handles on the store's
    // writer: dropped first.
    held.release_project_writer();
    drop(held);
    drop(egress);
    am_app::serving::clear_cli_token(&places, pid);
    if let Some(o) = &opened {
        am_app::runtime::clear_serve_port(&o.layout);
    }
    am_app::runtime::clear_serving(&app_data);
    drop(opened);
    code
}

/// The open project's egress client (story 2.2): its egress directory, the
/// Keychain, and a sink writing to its audit log.
fn egress_for(
    project: &str,
    opened: &am_app::runtime::Opened,
) -> Result<am_net::NetEgress, String> {
    let places = am_app::runtime::Places::from_env().map_err(|e| e.to_string())?;
    let directory = am_app::egress::ProjectDirectory::open(&PathBuf::from(project), &places)?;
    Ok(am_net::NetEgress::new(
        Arc::new(directory),
        am_app::keychain(),
        Arc::new(am_app::egress::StoreAuditSink::new(opened.store.writer())),
    ))
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

/// Blocks are printed: the spike's commands have no project store.
struct PrintAudit;
impl am_domain::AuditSink for PrintAudit {
    fn record(&self, e: am_domain::AuditEvent) {
        eprintln!("audit {} {} {} {:?}", e.action, e.target, e.outcome, e.args);
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
    use am_domain::EgressClient;
    let lmstudio = am_domain::net::reserved_local_account(am_domain::net::LOCAL_LMSTUDIO)
        .map(|e| e.to_string())
        .unwrap_or_default();
    // The reserved local Account `local-lmstudio` only (AD-12, AD-31).
    let egress = am_net::NetEgress::new(
        Arc::new(am_app::egress::LocalAccountsOnly),
        am_app::keychain(),
        Arc::new(PrintAudit),
    );
    let resp = egress
        .send(HttpRequest {
            caller: Some(Caller::Account {
                account: am_domain::net::LOCAL_LMSTUDIO.into(),
                seat: None,
            }),
            ..HttpRequest::get(format!("{lmstudio}/api/v1/models"))
        })
        .await
        .unwrap_or_else(|e| fail(&format!("LM Studio not reachable: {e}")));
    let body = am_net::read_all(resp.body.as_ref())
        .await
        .unwrap_or_else(|e| fail(&e.to_string()));
    println!("{}", String::from_utf8_lossy(&body));
}
