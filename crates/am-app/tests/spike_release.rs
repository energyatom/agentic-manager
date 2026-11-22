//! Story 2.2, owner decision 9 (2026-11-22): `agentic spike c1` and `c4`
//! read the Keychain entry `spike-anthropic` and call a provider with no
//! configured Account, so they leave the release binary. The spike's
//! recorded evidence (`docs/spike-report.md`) and its tests stay; anything
//! that still needs a binary is built only for development.
//!
//! The guard runs the `agentic` binary that `just check`, CI and the
//! release build make (no extra feature) with arguments that, while the
//! commands are still dispatched, fail before any Keychain read or network
//! use: `c1` with a provider no one has, `c4` with no folder. A removed
//! command answers with the usage line instead, which names neither.
//! Written first.
#![forbid(unsafe_code)]

use std::process::Command;

fn agentic(args: &[&str]) -> (i32, String) {
    let home = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_agentic"))
        .args(args)
        .env_remove("AM_SPIKE_DOCS_DIR")
        .env_remove("AM_SPIKE_LOCAL_MODEL")
        .env_remove("AM_LIVE")
        .env("HOME", home.path())
        .output()
        .expect("the agentic binary runs");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.code().unwrap_or(-1), text)
}

#[test]
fn the_release_binary_does_not_dispatch_spike_c1_or_c4() {
    for (args, still_dispatched) in [
        (
            &["spike", "c1", "--provider", "no-such-provider"][..],
            "unknown provider",
        ),
        (
            &["spike", "c4", "--no-anthropic", "--no-local"][..],
            "AM_SPIKE_DOCS_DIR",
        ),
    ] {
        let (code, text) = agentic(args);
        assert_ne!(code, 0, "{args:?}: {text}");
        assert!(
            !text.contains(still_dispatched),
            "{args:?} is still dispatched by the release binary: {text}"
        );
        assert!(
            text.contains("usage"),
            "{args:?}: answers with usage: {text}"
        );
        let usage = text
            .lines()
            .find(|l| l.contains("usage"))
            .unwrap_or_default();
        assert!(
            !usage.contains("c1") && !usage.contains("c4"),
            "{args:?}: the usage names neither command: {usage}"
        );
    }
}

#[test]
fn the_binarys_usage_no_longer_lists_spike_c1_or_c4() {
    let (code, text) = agentic(&["no-such-command"]);
    assert_ne!(code, 0, "{text}");
    assert!(text.contains("usage"), "{text}");
    assert!(
        !text.contains("c1") && !text.contains("c4"),
        "the usage lists a removed spike command: {text}"
    );
    // The spike's other commands stay.
    assert!(text.contains("c7"), "{text}");
}
