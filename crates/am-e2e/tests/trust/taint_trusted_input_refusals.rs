//! Trust test (AD-10, amended 2026-11-23; epic 3 story 3): there is no
//! conversion into or out of `TrustedInput`. It has private fields, no
//! `Deserialize`, no `Default`, and no `From` or `TryFrom` with
//! `Instruction`, `UntrustedData` or plain text, in either direction; and
//! still none from `UntrustedData` to `Instruction`. Its only constructors
//! are the two named ones (`taint_trusted_input_sources.rs`), which only
//! am-engine calls (`cargo xtask check-deps`, `xtask/tests/check_deps.rs`).
//!
//! The absence of a trait impl is checked here with probes that compile
//! either way (an inherent associated const beats a trait's default only
//! when the impl's bounds hold), each with a positive control. The
//! `compile_fail` proof lives in am-domain's own doctests, each paired with
//! a doctest that builds the type the allowed way (so a `compile_fail`
//! cannot pass merely because the type is missing); the second test here
//! holds the domain to that pair. Written before the type exists.
#![forbid(unsafe_code)]

use std::marker::PhantomData;

use am_domain::{Instruction, TrustedInput, UntrustedData};

/// `T: From<S>`?
trait NoFrom {
    const YES: bool = false;
}
struct FromProbe<S, T>(PhantomData<(S, T)>);
impl<S, T> NoFrom for FromProbe<S, T> {}
#[allow(dead_code)]
impl<S, T: From<S>> FromProbe<S, T> {
    const YES: bool = true;
}

/// `T: TryFrom<S>`?
trait NoTryFrom {
    const YES: bool = false;
}
struct TryFromProbe<S, T>(PhantomData<(S, T)>);
impl<S, T> NoTryFrom for TryFromProbe<S, T> {}
#[allow(dead_code)]
impl<S, T: TryFrom<S>> TryFromProbe<S, T> {
    const YES: bool = true;
}

/// `T: DeserializeOwned`?
trait NoDeserialize {
    const YES: bool = false;
}
struct DeserializeProbe<T>(PhantomData<T>);
impl<T> NoDeserialize for DeserializeProbe<T> {}
#[allow(dead_code)]
impl<T: serde::de::DeserializeOwned> DeserializeProbe<T> {
    const YES: bool = true;
}

/// `T: Default`?
trait NoDefault {
    const YES: bool = false;
}
struct DefaultProbe<T>(PhantomData<T>);
impl<T> NoDefault for DefaultProbe<T> {}
#[allow(dead_code)]
impl<T: Default> DefaultProbe<T> {
    const YES: bool = true;
}

#[test]
fn no_conversion_compiles() {
    // The probes see an impl that exists.
    let controls = [
        ("From<&str> for String", FromProbe::<&str, String>::YES),
        ("TryFrom<i64> for u8", TryFromProbe::<i64, u8>::YES),
        ("Deserialize for String", DeserializeProbe::<String>::YES),
        ("Default for String", DefaultProbe::<String>::YES),
    ];
    for (what, implemented) in controls {
        assert!(implemented, "control: the probe sees {what}");
    }

    let refused = [
        // Into a trusted input: from nothing.
        (
            "From<UntrustedData> for TrustedInput",
            FromProbe::<UntrustedData, TrustedInput>::YES,
        ),
        (
            "TryFrom<UntrustedData> for TrustedInput",
            TryFromProbe::<UntrustedData, TrustedInput>::YES,
        ),
        (
            "From<Instruction> for TrustedInput",
            FromProbe::<Instruction, TrustedInput>::YES,
        ),
        (
            "TryFrom<Instruction> for TrustedInput",
            TryFromProbe::<Instruction, TrustedInput>::YES,
        ),
        (
            "From<String> for TrustedInput",
            FromProbe::<String, TrustedInput>::YES,
        ),
        (
            "From<&str> for TrustedInput",
            FromProbe::<&str, TrustedInput>::YES,
        ),
        (
            "TryFrom<String> for TrustedInput",
            TryFromProbe::<String, TrustedInput>::YES,
        ),
        (
            "Deserialize for TrustedInput",
            DeserializeProbe::<TrustedInput>::YES,
        ),
        (
            "Default for TrustedInput",
            DefaultProbe::<TrustedInput>::YES,
        ),
        // Out of a trusted input: into nothing.
        (
            "From<TrustedInput> for Instruction",
            FromProbe::<TrustedInput, Instruction>::YES,
        ),
        (
            "TryFrom<TrustedInput> for Instruction",
            TryFromProbe::<TrustedInput, Instruction>::YES,
        ),
        (
            "From<TrustedInput> for UntrustedData",
            FromProbe::<TrustedInput, UntrustedData>::YES,
        ),
        (
            "TryFrom<TrustedInput> for UntrustedData",
            TryFromProbe::<TrustedInput, UntrustedData>::YES,
        ),
        // And, as before, untrusted data never becomes an instruction.
        (
            "From<UntrustedData> for Instruction",
            FromProbe::<UntrustedData, Instruction>::YES,
        ),
        (
            "TryFrom<UntrustedData> for Instruction",
            TryFromProbe::<UntrustedData, Instruction>::YES,
        ),
    ];
    for (what, implemented) in refused {
        assert!(!implemented, "no conversion may exist: {what}");
    }
}

/// The `compile_fail` doctests of `crates/am-domain/src/content.rs` (run by
/// `cargo test --doc -p am-domain`) name each refused conversion, and a
/// plain doctest builds a `TrustedInput` through its step-output
/// constructor, so each `compile_fail` fails for the conversion, not for a
/// missing type. (am-domain has no serde_json, so the `Deserialize` one
/// bounds on `serde::de::DeserializeOwned`; a `serde_json::from_str` would
/// fail to compile for the missing crate, the wrong reason.)
#[test]
fn the_domain_doctests_pair_each_refusal_with_an_allowed_build() {
    let src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../am-domain/src/content.rs"),
    )
    .unwrap();
    // The doc-comment code blocks: (fence info, code).
    let mut blocks: Vec<(String, String)> = Vec::new();
    let mut open: Option<(String, String)> = None;
    for line in src.lines() {
        let trimmed = line.trim_start();
        let Some(doc) = trimmed
            .strip_prefix("///")
            .or_else(|| trimmed.strip_prefix("//!"))
        else {
            continue;
        };
        let doc = doc.strip_prefix(' ').unwrap_or(doc);
        if let Some(info) = doc.strip_prefix("```") {
            match open.take() {
                Some(block) => blocks.push(block),
                None => open = Some((info.trim().to_string(), String::new())),
            }
        } else if let Some((_, code)) = open.as_mut() {
            code.push_str(doc);
            code.push('\n');
        }
    }
    let refused: Vec<&String> = blocks
        .iter()
        .filter(|(info, _)| info.contains("compile_fail"))
        .map(|(_, code)| code)
        .collect();
    for (what, needles) in [
        (
            "TrustedInput from UntrustedData",
            &["TrustedInput::from(", "UntrustedData"][..],
        ),
        (
            "Instruction from TrustedInput",
            &["Instruction::from(", "TrustedInput"][..],
        ),
        (
            "TrustedInput by Deserialize",
            &["Deserialize", "TrustedInput"][..],
        ),
    ] {
        assert!(
            refused
                .iter()
                .any(|code| needles.iter().all(|n| code.contains(n))),
            "a compile_fail doctest refusing {what} (naming {needles:?}) in am-domain's content.rs; found {refused:#?}"
        );
    }
    let allowed = blocks.iter().any(|(info, code)| {
        !info.contains("compile_fail")
            && !info.contains("ignore")
            && code.contains("TrustedInput::from_step_output(")
    });
    assert!(
        allowed,
        "a doctest that builds a TrustedInput through from_step_output, beside the compile_fail ones"
    );
}

/// Story 3.3 review patch round 1 (P14): the one renderer of a trusted
/// input, `render_trusted_input`, is am-providers' own (`pub(crate)`): no
/// other crate can render model-written text as a trusted block. Its
/// definition is not `pub`, and am-providers re-exports it nowhere.
#[test]
fn the_trusted_renderer_is_the_providers_own() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../am-providers/src");
    let mut defined = false;
    for entry in std::fs::read_dir(&src).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        for line in text.lines().map(str::trim) {
            if line.starts_with("//") {
                continue;
            }
            if line.contains("fn render_trusted_input") {
                defined = true;
                assert!(
                    !line.starts_with("pub fn") && !line.starts_with("pub async fn"),
                    "{}: `{line}` is public",
                    path.display()
                );
            }
            assert!(
                !(line.starts_with("pub use") && line.contains("render_trusted_input")),
                "{}: `{line}` re-exports it",
                path.display()
            );
        }
    }
    assert!(defined, "am-providers defines render_trusted_input");
}
