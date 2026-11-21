//! Criterion 4: files to models. A mixed folder is linked read-only, listed
//! and read under the confinement rules, and each file is attached through
//! `LlmProvider` file parts to a frontier model (Anthropic) and a local vision
//! model (LM Studio). Types a model's flags exclude are refused before any
//! request, in words, with zero bytes sent. Offline on synthetic fixtures and
//! recorded responses; live on the owner's folder (`AM_SPIKE_DOCS_DIR`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use am_app::spike::{
    C4Outcome, ModelKind, ModelUnderTest, anthropic_price, c4_table, c4_verdict, files_to_models,
};
use am_domain::{Access, Actor, PathError};
use am_providers::{AnthropicProvider, OpenAiCompatProvider};
use am_testkit::{CollectAudit, FakeEgress, FakeResponse, fixture, recorded};
use am_tools::{FolderAccess, FolderError, link_folder};

use super::live;

fn copy_fixture_folder() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("docs");
    // A stream copy (story 1.13: never `std::fs::copy`).
    am_testkit::copy_tree(&fixture("folders/spike"), &dir);
    (tmp, dir)
}

fn models(egress: &Arc<FakeEgress>) -> Vec<ModelUnderTest> {
    vec![
        ModelUnderTest {
            label: "frontier".into(),
            kind: ModelKind::Frontier,
            provider: Arc::new(AnthropicProvider::new(egress.clone(), "spike-anthropic")),
            model: "claude-opus-5-5".into(),
            price: Some(anthropic_price("claude-opus-5-5")),
        },
        ModelUnderTest {
            label: "local".into(),
            kind: ModelKind::Local,
            provider: Arc::new(OpenAiCompatProvider::lm_studio(egress.clone()).with_vision(true)),
            model: "local-vision".into(),
            price: None,
        },
    ]
}

#[tokio::test]
async fn mixed_folder_to_frontier_and_local_models() {
    let (_tmp, dir) = copy_fixture_folder();
    let egress = FakeEgress::new();
    for f in ["notes.txt", "table.csv", "report.pdf", "swatch.png"] {
        egress.respond_if(
            "api.anthropic.com/v1/messages",
            &format!("name=\\\"{f}\\\""),
            FakeResponse::sse(recorded(&format!(
                "anthropic/c4_{}.sse",
                f.replace('.', "_")
            ))),
        );
    }
    for f in ["notes.txt", "table.csv", "swatch.png"] {
        egress.respond_if(
            "127.0.0.1:1234/v1/chat/completions",
            &format!("name=\\\"{f}\\\""),
            FakeResponse::sse(recorded(&format!(
                "openai_compat/c4_{}.sse",
                f.replace('.', "_")
            ))),
        );
    }
    let audit = CollectAudit::new();
    let rows = files_to_models(&dir, &models(&egress), audit.clone())
        .await
        .unwrap();
    eprintln!("{}", c4_table(&rows));

    let outcome = |file: &str, model: &str| {
        rows.iter()
            .find(|r| r.file == file && r.model == model)
            .unwrap_or_else(|| panic!("no row for {file} / {model}"))
            .outcome
            .clone()
    };
    let accepted = |o: &C4Outcome| {
        matches!(
            o,
            C4Outcome::Accepted {
                names_known_word: Some(true),
                ..
            }
        )
    };
    let refused = |o: &C4Outcome| matches!(o, C4Outcome::Refused { .. });
    // Frontier: text, csv, PDF and image natively; office files refused.
    for f in ["notes.txt", "table.csv", "report.pdf", "swatch.png"] {
        assert!(accepted(&outcome(f, "frontier")), "{f} on frontier");
    }
    for f in ["sheet.xlsx", "memo.docx"] {
        assert!(refused(&outcome(f, "frontier")), "{f} on frontier");
    }
    // Local vision model: text, csv and image; PDF and office files refused.
    for f in ["notes.txt", "table.csv", "swatch.png"] {
        assert!(accepted(&outcome(f, "local")), "{f} on local");
    }
    for f in ["report.pdf", "sheet.xlsx", "memo.docx"] {
        assert!(refused(&outcome(f, "local")), "{f} on local");
    }
    match outcome("sheet.xlsx", "frontier") {
        C4Outcome::Refused { message } => {
            assert!(message.contains("cannot read sheet.xlsx"), "{message}");
            assert!(message.contains("Nothing was sent"), "{message}");
        }
        other => panic!("{other:?}"),
    }
    c4_verdict(&rows).unwrap();
    // Zero bytes sent for every refused file and model.
    let reqs = egress.requests();
    assert_eq!(reqs.len(), 7, "one request per accepted file and model");
    for r in &reqs {
        let body = String::from_utf8_lossy(&r.body);
        assert!(!body.contains("sheet.xlsx") && !body.contains("memo.docx"));
        if r.url.contains("1234") {
            assert!(!body.contains("report.pdf"));
        }
    }
    // Tokens and cost are recorded; local calls cost 0.
    let pdf = rows
        .iter()
        .find(|r| r.file == "report.pdf" && r.model == "frontier")
        .unwrap();
    assert_eq!(pdf.usage.map(|u| u.input_tokens), Some(1650));
    assert_eq!(pdf.cost_micro_usd, 1650 * 4 + 7 * 20);
    assert!(
        rows.iter()
            .filter(|r| r.model == "local")
            .all(|r| r.cost_micro_usd == 0)
    );
    // The PDF went as a native document block, the image as an image block.
    let pdf_req = reqs
        .iter()
        .find(|r| String::from_utf8_lossy(&r.body).contains("name=\\\"report.pdf\\\""))
        .unwrap();
    assert!(String::from_utf8_lossy(&pdf_req.body).contains(r#""type":"document""#));
    // Every read is audited with its hash.
    assert!(
        audit
            .events()
            .iter()
            .filter(|e| e.action == "folder.read")
            .all(|e| e.hash.is_some())
    );
}

#[cfg(unix)]
#[test]
fn confinement_refuses_symlink_escape_parent_path_and_writes() {
    let (tmp, dir) = copy_fixture_folder();
    let outside = tmp.path().join("outside-secret.txt");
    std::fs::write(&outside, "outside").unwrap();
    std::os::unix::fs::symlink(&outside, dir.join("escape.txt")).unwrap();
    std::os::unix::fs::symlink(tmp.path(), dir.join("escape-dir")).unwrap();
    std::fs::write(dir.join("inside-target.txt"), "fine").unwrap();
    std::os::unix::fs::symlink(dir.join("inside-target.txt"), dir.join("inside-link.txt")).unwrap();

    let root = link_folder("docs", &dir, Access::Read).unwrap();
    let audit = CollectAudit::new();
    let folder = FolderAccess::new(
        vec![root],
        audit.clone(),
        Actor::System {
            component: "test".into(),
        },
    );
    let listed: Vec<String> = folder
        .list_files("docs")
        .unwrap()
        .into_iter()
        .map(|e| e.relative_path)
        .collect();
    assert!(
        !listed.iter().any(|p| p.starts_with("escape")),
        "{listed:?}"
    );
    assert!(listed.contains(&"inside-link.txt".to_string()));

    assert!(matches!(
        folder.read_file("docs", "escape.txt"),
        Err(FolderError::Path(PathError::Escapes(_)))
    ));
    assert!(matches!(
        folder.read_file("docs", "escape-dir/outside-secret.txt"),
        Err(FolderError::Path(PathError::Escapes(_)))
    ));
    assert!(matches!(
        folder.read_file("docs", "../outside-secret.txt"),
        Err(FolderError::Path(PathError::ParentTraversal(_)))
    ));
    assert!(matches!(
        folder.read_file("docs", "/etc/hosts"),
        Err(FolderError::Path(PathError::Absolute(_)))
    ));
    assert!(matches!(
        folder.write_file("docs", "new.txt", b"x"),
        Err(FolderError::Path(PathError::ReadOnly(_)))
    ));
    assert!(!dir.join("new.txt").exists());
    assert!(folder.read_file("docs", "inside-link.txt").is_ok());

    let refusals = audit
        .events()
        .iter()
        .filter(|e| e.action == "folder.refused")
        .count();
    assert!(refusals >= 7, "every refusal is audited ({refusals})");

    // A read-write root still confines writes.
    let rw = FolderAccess::new(
        vec![link_folder("rw", &dir, Access::ReadWrite).unwrap()],
        audit.clone(),
        Actor::System {
            component: "test".into(),
        },
    );
    assert!(rw.write_file("rw", "out.txt", b"ok").is_ok());
    assert!(rw.write_file("rw", "escape.txt", b"overwrite").is_err());
    assert_eq!(std::fs::read_to_string(&outside).unwrap(), "outside");
    assert!(rw.write_file("rw", "../x.txt", b"no").is_err());
}

#[cfg(unix)]
#[test]
fn write_through_a_dangling_outward_symlink_is_refused() {
    let (tmp, dir) = copy_fixture_folder();
    let outside = tmp.path().join("created-outside.txt");
    std::os::unix::fs::symlink(&outside, dir.join("dangling.txt")).unwrap();
    let audit = CollectAudit::new();
    let rw = FolderAccess::new(
        vec![link_folder("rw", &dir, Access::ReadWrite).unwrap()],
        audit.clone(),
        Actor::System {
            component: "test".into(),
        },
    );
    assert!(matches!(
        rw.write_file("rw", "dangling.txt", b"leak"),
        Err(FolderError::Path(PathError::Escapes(_)))
    ));
    assert!(!outside.exists(), "nothing was written outside the root");
    assert!(
        audit
            .events()
            .iter()
            .any(|e| e.action == "folder.refused" && e.target.ends_with("dangling.txt"))
    );
}

#[cfg(unix)]
#[test]
fn a_folder_link_loop_is_listed_once() {
    let (_tmp, dir) = copy_fixture_folder();
    std::fs::create_dir(dir.join("sub")).unwrap();
    std::fs::write(dir.join("sub/inner.txt"), "inner").unwrap();
    std::os::unix::fs::symlink(dir.join("sub"), dir.join("sub/loop")).unwrap();
    let audit = CollectAudit::new();
    let folder = FolderAccess::new(
        vec![link_folder("docs", &dir, Access::Read).unwrap()],
        audit.clone(),
        Actor::System {
            component: "test".into(),
        },
    );
    let listed: Vec<String> = folder
        .list_files("docs")
        .unwrap()
        .into_iter()
        .map(|e| e.relative_path)
        .collect();
    let inner: Vec<&String> = listed.iter().filter(|p| p.ends_with("inner.txt")).collect();
    assert_eq!(inner, vec!["sub/inner.txt"], "{listed:?}");
    assert!(
        audit
            .events()
            .iter()
            .any(|e| e.action == "folder.refused" && e.target.ends_with("sub/loop"))
    );
}

#[test]
fn verdict_fails_on_gaps() {
    use am_app::spike::C4Row;
    let row = |kind, flags_allow, outcome| C4Row {
        file: "f".into(),
        media_type: "application/pdf".into(),
        model: format!("{kind:?}"),
        model_kind: kind,
        flags_allow,
        outcome,
        usage: None,
        cost_micro_usd: 0,
    };
    let ok = |known| C4Outcome::Accepted {
        answer: "w".into(),
        names_known_word: known,
    };
    let refused = || C4Outcome::Refused {
        message: "no".into(),
    };
    let both = |a, b| {
        vec![
            row(ModelKind::Frontier, true, a),
            row(ModelKind::Local, false, b),
        ]
    };

    assert!(c4_verdict(&both(ok(Some(true)), refused())).is_ok());
    assert!(c4_verdict(&[]).is_err(), "no rows");
    assert!(
        c4_verdict(&[row(ModelKind::Frontier, true, ok(Some(true)))]).is_err(),
        "only one model ran"
    );
    assert!(
        c4_verdict(&both(refused(), refused())).is_err(),
        "refused a type its flags allow"
    );
    let read = C4Outcome::ReadError {
        message: "too large".into(),
    };
    assert!(c4_verdict(&both(read, refused())).is_err(), "read error");
    let why = c4_verdict(&both(ok(None), refused())).unwrap_err();
    assert!(why.contains(".known-words"), "{why}");
    assert!(c4_verdict(&both(ok(Some(false)), refused())).is_err());
}

#[test]
fn linking_refuses_root_and_home() {
    assert!(link_folder("root", Path::new("/"), Access::Read).is_err());
    if let Some(home) = std::env::var_os("HOME") {
        assert!(link_folder("home", Path::new(&home), Access::Read).is_err());
        let lib = Path::new(&home).join("Library");
        if lib.exists() {
            assert!(link_folder("lib", &lib, Access::Read).is_err());
        }
    }
}

#[tokio::test]
#[ignore = "live: needs AM_LIVE=1 and AM_SPIKE_DOCS_DIR (the owner's folder)"]
async fn live_files_to_models_on_owner_folder() {
    if !live::live_enabled() {
        return live::skip("AM_LIVE is not 1");
    }
    let Ok(dir) = std::env::var("AM_SPIKE_DOCS_DIR") else {
        return live::skip("AM_SPIKE_DOCS_DIR not set");
    };
    let egress = live::egress();
    let mut models = Vec::new();
    if live::anthropic_key_present() {
        let m = live::anthropic_model();
        models.push(ModelUnderTest {
            label: format!("frontier: anthropic {m}"),
            kind: ModelKind::Frontier,
            provider: Arc::new(AnthropicProvider::new(
                egress.clone(),
                am_app::SPIKE_ANTHROPIC_ACCOUNT,
            )),
            price: Some(anthropic_price(&m)),
            model: m,
        });
    } else {
        eprintln!("frontier half skipped: no Anthropic key in the Keychain");
    }
    match live::lm_studio_model().await {
        Ok(m) => models.push(ModelUnderTest {
            label: format!("local: lmstudio {m}"),
            kind: ModelKind::Local,
            provider: Arc::new(OpenAiCompatProvider::lm_studio(egress.clone()).with_vision(true)),
            price: None,
            model: m,
        }),
        Err(why) => eprintln!("local half not run: {why}"),
    }
    if models.is_empty() {
        return live::skip("no model available (no key, no LM Studio)");
    }
    let rows = files_to_models(Path::new(&dir), &models, CollectAudit::new())
        .await
        .unwrap();
    let table = c4_table(&rows);
    println!("{table}");
    if let Ok(out) = std::env::var("AM_SPIKE_C4_OUT") {
        std::fs::write(out, &table).unwrap();
    }
    if let Err(why) = c4_verdict(&rows) {
        panic!("C4 on the owner's folder failed:\n{why}\n{table}");
    }
}
