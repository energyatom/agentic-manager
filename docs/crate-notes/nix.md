# nix 0.31.4

Pinned `=0.31.4` (MIT) with `default-features = false, features = ["term"]`
in `[workspace.dependencies]` (Stack). It was already in the graph through
`process-wrap`; story 2.1 links it from `am-app` alone, for the interim CLI
Account edit's key prompt (AD-15). Its own `unsafe` stays inside the
dependency: every crate here still forbids unsafe code, so only nix's safe
wrappers are used.

## Where it is used

- `crates/am-app/src/terminal.rs` (`OwnerTerminal`, the real `Terminal`
  port): `nix::sys::termios` to turn `ECHO`, `ICANON` and `ISIG` off while
  the key is typed, and to put the saved settings back.
- `crates/am-app/tests/accounts_tty.rs`: `nix::pty::openpty` to run the real
  binary on a pseudo-terminal, and `tcgetattr` to read its `ECHO` flag.

## What we use

- `tcgetattr(fd) -> Termios` and `tcsetattr(fd, SetArg, &Termios)`; both take
  any `AsFd` (a duplicate of stdin from `try_clone_to_owned`). `Termios` is
  `Clone`: the saved copy lives in a drop guard that restores it on every way
  out of the read, a panic's unwind included.
- `Termios::local_flags` (`LocalFlags::ECHO | ICANON | ISIG`) and
  `control_chars[SpecialCharacterIndices::VMIN as usize]` / `VTIME` (1 and 0:
  a read returns each byte as it is typed).
- `SetArg::TCSAFLUSH` when changing and when restoring: it also discards
  input typed and not yet read, so the rest of a key after `^C` never reaches
  the shell. `TCSANOW` is the fallback for the restore.
- `tcflush(fd, FlushArg::TCIFLUSH)` (story 3.4, R9; same `term` feature):
  `OwnerTerminal::discard_typed_ahead` drops input typed and not yet read
  before each `agentic gate` prompt, so lines typed or pasted ahead never
  answer it. On a stdin that is not a terminal it fails, and is ignored.
- With `ISIG` off a typed `^C` is the byte `0x03`, which the reader takes as
  cancel. Signals sent to the process (SIGINT, SIGTERM, SIGHUP, SIGQUIT)
  are not nix's business: they come through `tokio::signal` (nix's
  `sigaction` is `unsafe` and needs the `signal` feature, which is not
  enabled), and each ends the read so the guard restores the terminal.

## Do not

- Do not enable another nix feature (`signal`, `poll`, `process`, `fs`…)
  without a Stack change: the Stack allows `term` only. A time limit on a
  read is a helper thread and a channel, not `poll`.
- Do not use nix outside `am-app` (AD-47: OS-specific code lives in adapters
  and the composition root), and do not let its types (`Termios`, `Errno`)
  into `am-domain` or a port signature: the `Terminal` port speaks
  `TerminalError`.
- Do not change the terminal's settings without the restore guard already in
  place, and never leave a path that returns with echo off.
- Do not add a fake or scripted terminal to `am-app`, behind any `cfg`,
  feature, flag or environment variable: the only one is am-testkit's
  `FakeTerminal` (a dev-dependency), and `xtask/tests/release_deps.rs` fails
  when am-testkit reaches the `agentic` binary.
