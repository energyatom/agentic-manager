//! Story 2.1: the rows of `agentic accounts` that need the real binary
//! (`CARGO_BIN_EXE_agentic`) and a real terminal, with HOME and
//! XDG_DATA_HOME in a temporary folder as in `audit_cli.rs`:
//!
//! - Not a terminal: `add` and `remove` with piped or redirected stdin exit
//!   non-zero with a worded refusal naming the terminal rule, and nothing is
//!   read, written or stored.
//! - Interrupt: on a pseudo-terminal (`nix::pty::openpty`), a typed `^C` at
//!   the key prompt, and a SIGINT sent there, end the command non-zero; the
//!   pty's `ECHO` flag (read with `tcgetattr`) is set again; nothing is
//!   written or stored. These reach the key prompt only where the Keychain
//!   exists (macOS, AD-47), so they run there. With HOME in a temporary
//!   folder macOS finds no default keychain, so the setup gives that HOME
//!   its own (`/usr/bin/security`: create, make default, unlock), apart from
//!   the login keychain; the id is unique and is checked absent from that
//!   keychain afterwards.
//!
//! Written before the command exists.
//!
//! Story 2.12 (AD-17): the commands take no `--project`; they are run from
//! inside the project folder, and "nothing written" means no user Accounts
//! file in the temporary HOME's app data dir (and no old project file).
#![forbid(unsafe_code)]
#![cfg(unix)]

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use am_app::runtime::{OpenRequest, Places, Prompt, open_project};

const COMPANY: &str = "schema_version: 2\nname: Example\ntimezone: UTC\ncurrency: EUR\n";
const LEGACY_FILE: &str = "models/accounts.yaml";
const CANARY: &str = "sk-ant-canary-tty-0a1b2c3d4e5f-WXYZ";

struct NoPrompt;

impl Prompt for NoPrompt {
    fn interactive(&self) -> bool {
        false
    }
    fn ask(&mut self, _question: &str) -> bool {
        false
    }
}

/// A temporary HOME (and XDG_DATA_HOME) with one project opened once on
/// this Mac through the system identity adapter, as the binary resolves it.
struct Env {
    dir: tempfile::TempDir,
    root: PathBuf,
    places: Places,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let support = if cfg!(target_os = "macos") {
        home.join("Library/Application Support")
    } else {
        dir.path().join("xdg")
    };
    let places = Places::under(&support, &home);
    let root = dir.path().join("project");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("company.yaml"), COMPANY).unwrap();
    let root = root.canonicalize().unwrap();
    let identity = am_app::identity::system();
    let req = OpenRequest {
        root: root.clone(),
        confirm_move: false,
        take_over: false,
    };
    drop(open_project(&places, identity.as_ref(), &req, &mut NoPrompt).unwrap());
    Env { dir, root, places }
}

impl Env {
    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_agentic"));
        cmd.arg("accounts")
            .args(args)
            .current_dir(&self.root)
            .env("HOME", self.dir.path().join("home"))
            .env("XDG_DATA_HOME", self.dir.path().join("xdg"));
        cmd
    }

    /// The user Accounts file; nothing a refused command does may make it,
    /// nor the old project file.
    fn accounts_file(&self) -> PathBuf {
        assert!(!self.root.join(LEGACY_FILE).exists());
        self.places.user.accounts_file()
    }
}

fn unique_id() -> String {
    format!(
        "tty-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    )
}

/// Not a terminal: piped stdin (the typed id and a key ready on it) and a
/// stdin redirected from a file are both refused in words naming the
/// terminal rule; no file is written.
#[test]
fn add_and_remove_with_stdin_that_is_not_a_terminal_are_refused() {
    let e = env();
    let id = unique_id();
    let input = format!("{id}\n{CANARY}\n");

    for args in [
        vec!["add", id.as_str(), "--provider", "anthropic"],
        vec!["remove", id.as_str()],
    ] {
        let mut child = e
            .command(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // The command may refuse before reading: a closed pipe is fine.
        let _ = child.stdin.take().unwrap().write_all(input.as_bytes());
        let out = child.wait_with_output().unwrap();
        let all = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(!out.status.success(), "{args:?}: {all}");
        assert!(all.contains("terminal"), "{args:?}: {all}");
        assert!(!all.contains(CANARY), "{args:?}: {all}");
        assert!(!e.accounts_file().exists(), "{args:?}: nothing written");

        let file = e.dir.path().join("typed.txt");
        std::fs::write(&file, &input).unwrap();
        let out = e
            .command(&args)
            .stdin(Stdio::from(std::fs::File::open(&file).unwrap()))
            .output()
            .unwrap();
        let all = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(!out.status.success(), "{args:?} < file: {all}");
        assert!(all.contains("terminal"), "{args:?} < file: {all}");
        assert!(!e.accounts_file().exists());
    }
}

/// Story 2.14: an Account on this Mac written by the app with no
/// confirmation record (as `accounts migrate` leaves one), so `change`
/// takes the new-key path. Returns its id.
fn unconfirmed_account(e: &Env) -> String {
    let id = unique_id();
    let guard =
        am_store::user::UserStore::locked(&e.places.user, std::time::Duration::from_secs(10))
            .unwrap();
    let writer =
        am_config::ConfigWriter::for_user_accounts(e.places.user.dir(), std::sync::Arc::new(guard))
            .unwrap();
    writer
        .create(
            am_domain::Actor::Human {
                person: "owner".into(),
                hat: "Owner".into(),
            },
            am_config::ConfigCreate {
                file: "accounts.yaml".into(),
                bytes: am_config::new_accounts_file(&id, "anthropic", None, None).unwrap(),
                audit: Vec::new(),
            },
        )
        .unwrap();
    id
}

/// Story 2.14, Not interactive: `change` with piped or redirected stdin
/// (the id, `yes` and a key ready on it) is refused in words naming the
/// terminal rule; the user file is unchanged.
#[test]
fn change_with_stdin_that_is_not_a_terminal_is_refused() {
    let e = env();
    let id = unconfirmed_account(&e);
    let before = std::fs::read(e.accounts_file()).unwrap();
    let input = format!("{id}\nyes\n{CANARY}\n");
    let args = ["change", id.as_str(), "--provider", "openai"];

    let mut child = e
        .command(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _ = child.stdin.take().unwrap().write_all(input.as_bytes());
    let out = child.wait_with_output().unwrap();
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.status.code(), Some(1), "{all}");
    assert!(all.contains("interactive terminal"), "{all}");
    assert!(!all.contains(CANARY), "{all}");

    let file = e.dir.path().join("typed.txt");
    std::fs::write(&file, &input).unwrap();
    let out = e
        .command(&args)
        .stdin(Stdio::from(std::fs::File::open(&file).unwrap()))
        .output()
        .unwrap();
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.status.code(), Some(1), "{all}");
    assert!(all.contains("interactive terminal"), "{all}");
    assert_eq!(std::fs::read(e.accounts_file()).unwrap(), before);
}

#[cfg(target_os = "macos")]
mod pty {
    use std::io::{Read, Write};
    use std::os::fd::OwnedFd;
    use std::path::PathBuf;
    use std::process::{Child, Command, Output, Stdio};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use nix::sys::termios::{LocalFlags, tcgetattr};

    use super::{CANARY, env, unique_id};

    /// The Keychain service the app stores keys under (AD-17).
    const SERVICE: &str = "agentic-manager";

    fn home(e: &super::Env) -> PathBuf {
        e.dir.path().join("home")
    }

    /// `/usr/bin/security <args>` with HOME in the test's temporary home, so
    /// it reads and writes that home's keychain and never the login one.
    fn security(e: &super::Env, args: &[&str]) -> Output {
        Command::new("/usr/bin/security")
            .args(args)
            .env("HOME", home(e))
            .stdin(Stdio::null())
            .output()
            .unwrap_or_else(|err| panic!("/usr/bin/security {args:?} did not run: {err}"))
    }

    /// Gives the temporary HOME its own default keychain, unlocked: with
    /// HOME redirected macOS finds no default keychain, and the binary
    /// (rightly) refuses before the key prompt when it cannot read one. A
    /// failing step fails the test with `security`'s output.
    fn temp_keychain(e: &super::Env) {
        let home = home(e);
        std::fs::create_dir_all(home.join("Library/Keychains")).unwrap();
        std::fs::create_dir_all(home.join("Library/Preferences")).unwrap();
        let keychain = home.join("Library/Keychains/test.keychain-db");
        let keychain = keychain.to_str().unwrap();
        for args in [
            vec!["create-keychain", "-p", "", keychain],
            vec!["default-keychain", "-s", keychain],
            vec!["unlock-keychain", "-p", "", keychain],
        ] {
            let out = security(e, &args);
            assert!(
                out.status.success(),
                "security {args:?} failed ({}): {}{}",
                out.status,
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }

    /// The command on a pseudo-terminal: its stdin and stdout are the pty's
    /// slave; the test keeps the master (to type) and a slave handle (to
    /// read the terminal's flags).
    struct OnPty {
        child: Child,
        master: std::fs::File,
        slave: OwnedFd,
        seen: Arc<Mutex<Vec<u8>>>,
    }

    impl OnPty {
        fn echo(&self) -> bool {
            tcgetattr(&self.slave)
                .unwrap()
                .local_flags
                .contains(LocalFlags::ECHO)
        }

        fn seen(&self) -> String {
            String::from_utf8_lossy(&self.seen.lock().unwrap()).into_owned()
        }

        fn wait_until(&mut self, what: &str, done: impl Fn(&OnPty) -> bool) {
            let start = Instant::now();
            while !done(self) {
                if let Ok(Some(status)) = self.child.try_wait() {
                    panic!("exited ({status}) before {what}; terminal: {}", self.seen());
                }
                assert!(
                    start.elapsed() < Duration::from_secs(30),
                    "timed out waiting for {what}; terminal: {}",
                    self.seen()
                );
                std::thread::sleep(Duration::from_millis(20));
            }
        }

        fn exit_code(&mut self) -> Option<i32> {
            let start = Instant::now();
            loop {
                if let Some(status) = self.child.try_wait().unwrap() {
                    return status.code();
                }
                if start.elapsed() > Duration::from_secs(30) {
                    let _ = self.child.kill();
                    panic!("the command did not end; terminal: {}", self.seen());
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }

    fn start(e: &super::Env, id: &str) -> OnPty {
        start_with(e, &["add", id, "--provider", "anthropic"])
    }

    fn start_with(e: &super::Env, args: &[&str]) -> OnPty {
        let pty = nix::pty::openpty(None, None).unwrap();
        let slave = pty.slave;
        let master = std::fs::File::from(pty.master);
        let child = e
            .command(args)
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .stdout(Stdio::from(slave.try_clone().unwrap()))
            .stderr(Stdio::from(slave.try_clone().unwrap()))
            .spawn()
            .unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let mut reader = master.try_clone().unwrap();
        let sink = seen.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 1024];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                sink.lock().unwrap().extend_from_slice(&buf[..n]);
            }
        });
        let on = OnPty {
            child,
            master,
            slave,
            seen,
        };
        assert!(on.echo(), "a new terminal echoes");
        on
    }

    /// Types the id at the name prompt and waits for the key prompt (echo
    /// turned off).
    fn to_the_key_prompt(on: &mut OnPty, id: &str) {
        on.wait_until("the name prompt", |p| p.seen().contains(id));
        on.master.write_all(format!("{id}\n").as_bytes()).unwrap();
        on.wait_until("echo off at the key prompt", |p| !p.echo());
    }

    fn assert_nothing_stored(e: &super::Env, id: &str, on: &OnPty) {
        assert!(!e.accounts_file().exists(), "nothing written");
        assert_nothing_stored_for(e, id, on);
    }

    /// No Keychain item for `id` and no key on the terminal.
    fn assert_nothing_stored_for(e: &super::Env, id: &str, on: &OnPty) {
        // The temporary HOME's keychain (the one the binary used) must hold
        // no item for the id: the lookup fails saying it was not found.
        let found = security(e, &["find-generic-password", "-s", SERVICE, "-a", id]);
        let words = format!(
            "{}{}",
            String::from_utf8_lossy(&found.stdout),
            String::from_utf8_lossy(&found.stderr)
        );
        assert!(
            !found.status.success(),
            "the key was stored for {id}: {words}"
        );
        assert!(
            words.contains("could not be found"),
            "the keychain lookup for {id} failed another way: {words}"
        );
        assert!(!on.seen().contains(CANARY));
    }

    /// Interrupt: `^C` typed at the key prompt (read as a byte, ISIG off)
    /// ends the command non-zero and the terminal echoes again.
    #[test]
    fn typed_ctrl_c_at_the_key_prompt_restores_echo_and_stores_nothing() {
        let e = env();
        temp_keychain(&e);
        let id = unique_id();
        let mut on = start(&e, &id);
        to_the_key_prompt(&mut on, &id);
        on.master.write_all(&[0x03]).unwrap();
        let code = on.exit_code();
        assert_ne!(code, Some(0), "terminal: {}", on.seen());
        assert!(on.echo(), "ECHO is set again; terminal: {}", on.seen());
        let flags = tcgetattr(&on.slave).unwrap().local_flags;
        assert!(flags.contains(LocalFlags::ICANON), "canonical mode is back");
        assert!(flags.contains(LocalFlags::ISIG), "signals are back");
        assert_nothing_stored(&e, &id, &on);
    }

    /// Story 2.14, Echo: `change` of an Account with no confirmation record
    /// takes the new-key path; `^C` typed at its hidden key prompt (after
    /// the id and `yes`) ends the command non-zero with the terminal's
    /// settings back; the user file is unchanged and no key is stored.
    #[test]
    fn typed_ctrl_c_at_the_change_key_prompt_restores_echo_and_writes_nothing() {
        let e = env();
        temp_keychain(&e);
        let id = super::unconfirmed_account(&e);
        let before = std::fs::read(e.accounts_file()).unwrap();
        let mut on = start_with(&e, &["change", id.as_str(), "--provider", "openai"]);
        on.wait_until("the id prompt", |p| p.seen().contains(id.as_str()));
        on.master
            .write_all(format!("{id}\nyes\n").as_bytes())
            .unwrap();
        on.wait_until("echo off at the key prompt", |p| !p.echo());
        on.master.write_all(&[0x03]).unwrap();
        let code = on.exit_code();
        assert_eq!(code, Some(130), "terminal: {}", on.seen());
        assert!(on.echo(), "ECHO is set again; terminal: {}", on.seen());
        let flags = tcgetattr(&on.slave).unwrap().local_flags;
        assert!(flags.contains(LocalFlags::ICANON), "canonical mode is back");
        assert!(flags.contains(LocalFlags::ISIG), "signals are back");
        assert_eq!(std::fs::read(e.accounts_file()).unwrap(), before);
        assert_nothing_stored_for(&e, &id, &on);
    }

    /// Interrupt: SIGINT (`kill -INT`) at the key prompt also ends the
    /// command non-zero with echo restored.
    #[test]
    fn sigint_at_the_key_prompt_restores_echo_and_stores_nothing() {
        let e = env();
        temp_keychain(&e);
        let id = unique_id();
        let mut on = start(&e, &id);
        to_the_key_prompt(&mut on, &id);
        let status = std::process::Command::new("/bin/kill")
            .args(["-INT", &on.child.id().to_string()])
            .status()
            .unwrap();
        assert!(status.success());
        let code = on.exit_code();
        assert_ne!(code, Some(0), "terminal: {}", on.seen());
        assert!(on.echo(), "ECHO is set again; terminal: {}", on.seen());
        assert_nothing_stored(&e, &id, &on);
    }

    /// Echo is restored on every exit path: SIGTERM (`kill -TERM`) at the
    /// key prompt ends the command non-zero with the terminal's settings
    /// back, instead of killing it with echo off.
    #[test]
    fn sigterm_at_the_key_prompt_restores_echo_and_stores_nothing() {
        let e = env();
        temp_keychain(&e);
        let id = unique_id();
        let mut on = start(&e, &id);
        to_the_key_prompt(&mut on, &id);
        let status = std::process::Command::new("/bin/kill")
            .args(["-TERM", &on.child.id().to_string()])
            .status()
            .unwrap();
        assert!(status.success());
        let code = on.exit_code();
        assert_ne!(code, Some(0), "terminal: {}", on.seen());
        assert!(on.echo(), "ECHO is set again; terminal: {}", on.seen());
        let flags = tcgetattr(&on.slave).unwrap().local_flags;
        assert!(flags.contains(LocalFlags::ICANON), "canonical mode is back");
        assert!(flags.contains(LocalFlags::ISIG), "signals are back");
        assert_nothing_stored(&e, &id, &on);
    }
}
