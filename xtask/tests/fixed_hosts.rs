//! `cargo xtask fixed-hosts` (story 2.2, AD-12 "No telemetry"): a scan over
//! the shipped source, `crates/*/src` (a crate's `tests/` and its
//! `#[cfg(test)]` modules are left out), finds every fixed host in a string
//! literal, by its URL or its bare name, and fails on any host that is not
//! on the scan's one named list, naming the host and the file. The list
//! holds each host with its reason; today: the Anthropic and OpenAI
//! provider endpoints, and `127.0.0.1` with the two local Account ports.
//! It has no release host yet (epic 11) and no skill-import hosts (epic 9
//! story 16). Comments are not code. Written first.
#![forbid(unsafe_code)]

mod common;

use common::{stderr, stdout, write, xtask};

fn scan(root: &std::path::Path) -> std::process::Output {
    xtask(&["fixed-hosts", "--root", &root.to_string_lossy()])
}

/// The workspace passes, and the scan prints its named list.
#[test]
fn the_repository_names_only_listed_hosts() {
    let out = xtask(&["fixed-hosts"]);
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
    let said = stdout(&out);
    for host in ["api.anthropic.com", "api.openai.com", "127.0.0.1"] {
        assert!(said.contains(host), "the list names {host}: {said}");
    }
}

/// A telemetry host, by URL or bare name, fails naming the host and file.
#[test]
fn a_fixed_host_not_on_the_list_fails_naming_host_and_file() {
    for (what, source) in [
        (
            "url",
            "pub const SINK: &str = \"https://telemetry.example.com/v1/collect\";\n",
        ),
        (
            "bare name",
            "pub fn sink() -> String {\n    format!(\"{}:443\", \"telemetry.example.com\")\n}\n",
        ),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "crates/x/src/report.rs", source);
        let out = scan(tmp.path());
        let err = stderr(&out);
        assert!(!out.status.success(), "{what}: must fail: {err}");
        assert!(
            err.contains("telemetry.example.com"),
            "{what}: names the host: {err}"
        );
        assert!(
            err.contains("crates/x/src/report.rs"),
            "{what}: names the file: {err}"
        );
    }
}

/// Hosts that are not on today's list fail: a skill-import host (added
/// only by epic 9 story 16), a loopback port that is not a local Account's,
/// and a fixed IP address.
#[test]
fn hosts_not_yet_listed_fail() {
    for (host, source) in [
        (
            "api.github.com",
            "pub const U: &str = \"https://api.github.com/repos\";\n",
        ),
        (
            "codeload.github.com",
            "pub const U: &str = \"codeload.github.com\";\n",
        ),
        (
            "127.0.0.1:9999",
            "pub const U: &str = \"http://127.0.0.1:9999/v1\";\n",
        ),
        (
            "10.0.0.9",
            "pub const U: &str = \"https://10.0.0.9/collect\";\n",
        ),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "crates/x/src/lib.rs", source);
        let out = scan(tmp.path());
        let err = stderr(&out);
        assert!(!out.status.success(), "{host}: must fail: {err}");
        assert!(err.contains(host), "{host}: names it: {err}");
    }
}

/// The listed hosts pass, as URLs and bare names; so do hosts in comments,
/// in a crate's tests/ folder and in a `#[cfg(test)]` module, and file
/// names that are not hosts.
#[test]
fn listed_hosts_comments_and_tests_pass() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "crates/x/src/lib.rs",
        concat!(
            "//! See https://docs.example.com/guide for the format.\n",
            "pub const ANTHROPIC: &str = \"api.anthropic.com\";\n",
            "pub const ANTHROPIC_URL: &str = \"https://api.anthropic.com/v1/messages\";\n",
            "pub const OPENAI: &str = \"https://api.openai.com/v1\";\n",
            "pub const LMSTUDIO: &str = \"http://127.0.0.1:1234/v1\";\n",
            "pub const OLLAMA: &str = \"http://127.0.0.1:11434/v1\";\n",
            "pub const FILE: &str = \"models/accounts.yaml\";\n",
            "// https://telemetry.example.com is only a comment\n",
            "#[cfg(test)]\n",
            "mod tests {\n",
            "    const FAKE: &str = \"https://telemetry.example.com/\";\n",
            "    #[test]\n",
            "    fn t() {\n",
            "        assert!(FAKE.contains(\"example\"));\n",
            "    }\n",
            "}\n",
        ),
    );
    write(
        tmp.path(),
        "crates/x/tests/t.rs",
        "const U: &str = \"https://telemetry.example.com/\";\n",
    );
    let out = scan(tmp.path());
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
}

/// Story 2.2 review (low): bare names on any common top-level domain (the
/// generic ones and the two-letter country codes that are not file
/// extensions here) and bare IPv6 literals are found too.
#[test]
fn more_top_level_domains_and_ipv6_literals_fail() {
    for (host, source) in [
        (
            "collector.vercel.app",
            "pub const U: &str = \"collector.vercel.app\";\n",
        ),
        ("t.ly", "pub const U: &str = \"t.ly\";\n"),
        ("2001:db8::1", "pub const U: &str = \"[2001:db8::1]\";\n"),
        ("2001:db8::2", "pub const U: &str = \"2001:db8::2\";\n"),
        (
            "sink.example.de",
            "pub const U: &str = \"sink.example.de:8443\";\n",
        ),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "crates/x/src/lib.rs", source);
        let out = scan(tmp.path());
        let err = stderr(&out);
        assert!(!out.status.success(), "{host}: must fail: {err}");
        assert!(err.contains(host), "{host}: names it: {err}");
    }
}

/// Story 2.2 review (low): a `#[cfg(test)]` on a field, a variant or a
/// match arm excludes only that item, never the shipped code after it.
#[test]
fn a_cfg_test_on_a_field_variant_or_arm_excludes_only_that_item() {
    for (what, source) in [
        (
            "a field",
            "pub struct S {\n    #[cfg(test)]\n    pub probe: u8,\n    pub n: u8,\n}\nimpl S {\n    pub fn sink() -> &'static str {\n        \"https://telemetry.example.com/collect\"\n    }\n}\n",
        ),
        (
            "the last field, no comma",
            "pub struct S {\n    pub n: u8,\n    #[cfg(test)]\n    pub probe: u8\n}\npub fn sink() -> &'static str {\n    \"https://telemetry.example.com/collect\"\n}\n",
        ),
        (
            "a variant",
            "pub enum E {\n    A,\n    #[cfg(test)]\n    Probe(u8),\n}\npub fn sink() -> &'static str {\n    \"https://telemetry.example.com/collect\"\n}\n",
        ),
        (
            "a match arm",
            "pub fn sink(n: u8) -> &'static str {\n    match n {\n        #[cfg(test)]\n        0 => \"probe\",\n        _ => \"https://telemetry.example.com/collect\",\n    }\n}\n",
        ),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "crates/x/src/lib.rs", source);
        let out = scan(tmp.path());
        let err = stderr(&out);
        assert!(!out.status.success(), "{what}: must fail: {err}");
        assert!(err.contains("telemetry.example.com"), "{what}: {err}");
    }
}

/// Story 2.2 review (4): a URL whose scheme is a format placeholder still
/// names its host.
#[test]
fn a_placeholder_scheme_still_names_the_host() {
    for source in [
        "pub fn u(scheme: &str) -> String {\n    format!(\"{scheme}://telemetry.example.com/x\")\n}\n",
        "pub fn u(s: &str) -> String {\n    format!(\"{}://telemetry.example.com/x\", s)\n}\n",
    ] {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "crates/x/src/lib.rs", source);
        let out = scan(tmp.path());
        let err = stderr(&out);
        assert!(!out.status.success(), "must fail: {source}: {err}");
        assert!(err.contains("telemetry.example.com"), "{err}");
    }
}

/// Story 3.3 review patch round 1 (P4): a C-string literal never hides a
/// host. A host after a raw C string that ends in a backslash, a host
/// inside a raw C string that holds quotes, and a host in a C string are
/// all found; a literal that never ends fails the scan closed.
#[test]
fn a_host_after_or_inside_a_c_string_is_found() {
    for (what, source) in [
        (
            "after a raw C string ending in a backslash",
            format!(
                "{}\npub const SINK: &str = \"https://telemetry.example.com/v1\";\n",
                r##"pub const S: &core::ffi::CStr = cr#"\"#;"##
            ),
        ),
        (
            "inside a raw C string holding quotes",
            format!(
                "{}\n",
                r##"pub const S: &core::ffi::CStr = cr#"see "https://telemetry.example.com" now"#;"##
            ),
        ),
        (
            "in a C string",
            "pub const S: &core::ffi::CStr = c\"https://telemetry.example.com\";\n".to_string(),
        ),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "crates/x/src/report.rs", &source);
        let out = scan(tmp.path());
        let err = stderr(&out);
        assert!(!out.status.success(), "{what}: must fail: {err}");
        assert!(
            err.contains("telemetry.example.com"),
            "{what}: names the host: {err}"
        );
    }

    // Literals that never end: each runs to the end of the file (a raw C
    // string with no closing `"#`, a C string with no closing quote).
    for (what, source) in [
        (
            "an unterminated raw C string",
            format!(
                "{}\npub const T: &str = \"https://telemetry.example.com\";\n",
                r##"pub const S: &core::ffi::CStr = cr#"never closed;"##
            ),
        ),
        (
            "an unterminated C string",
            "pub const S: &core::ffi::CStr = c\"never closed; see https://telemetry.example.com/v1\npub fn f() {}\n"
                .to_string(),
        ),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "crates/x/src/report.rs", &source);
        let out = scan(tmp.path());
        let err = stderr(&out);
        assert!(!out.status.success(), "{what}: must fail: {err}");
        assert!(err.contains("crates/x/src/report.rs"), "{what}: {err}");
    }
}

/// Story 3.3 review patch round 2 (R1): a shebang first line hides no
/// host after it; a host after an inner attribute on line 1 is still found.
#[test]
fn a_shebang_hides_no_host() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "crates/am-app/src/hide.rs",
        "#!/x \"\npub const H: &str = \"evil.example.com\"; // \"\npub fn u() {} // \"\n",
    );
    let out = scan(tmp.path());
    let err = stderr(&out);
    assert!(!out.status.success(), "after a shebang: must fail: {err}");
    assert!(err.contains("evil.example.com"), "names the host: {err}");

    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "crates/am-app/src/attr.rs",
        "#! [forbid(unsafe_code)]\npub const H: &str = \"https://telemetry.example.com\";\n",
    );
    let out = scan(tmp.path());
    let err = stderr(&out);
    assert!(!out.status.success(), "after an inner attribute: {err}");
    assert!(err.contains("telemetry.example.com"), "{err}");
}

/// Story 3.3 review patch round 2 (R8): valid code where a field or a
/// binding named `https` or `http` is followed by a `//` comment
/// (`Bar { https:// note⏎ 443 }`) is not a finding; the scan fails closed
/// only on a literal that really never ends.
#[test]
fn a_scheme_like_field_before_a_comment_is_no_finding() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "crates/am-app/src/nospace.rs",
        "struct Bar { https: u16 }\nfn b() -> Bar { Bar { https:// note\n 443 } }\nfn c() { let http:// typed\n u8 = 0; let _ = http; }\n",
    );
    write(
        tmp.path(),
        "crates/am-app/src/space.rs",
        "struct Foo { https: u16 }\nfn a() -> Foo { Foo { https: // note\n 443 } }\n",
    );
    let out = scan(tmp.path());
    assert!(
        out.status.success(),
        "valid code is not a finding: {}{}",
        stdout(&out),
        stderr(&out)
    );
}
