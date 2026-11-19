//! Criterion 4: files to models. A mixed folder is linked read-only, listed
//! and read under the confinement rules, and each file is attached through
//! `LlmProvider` file parts to a frontier model (Anthropic) and a local vision
//! model (LM Studio). Types a model's flags exclude are refused before any
//! request, in words, with zero bytes sent. Offline on synthetic fixtures and
//! recorded responses; live on the owner's folder (`AM_SPIKE_DOCS_DIR`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use am_app::spike::{
    C4Outcome, ModelUnderTest, anthropic_price, c4_passed, c4_table, files_to_models,
};
use am_domain::{Access, Actor, PathError};
use am_providers::{AnthropicProvider, OpenAiCompatProvider};
use am_testkit::{CollectAudit, FakeEgress, FakeResponse, fixture, recorded};
use am_tools::{FolderAccess, FolderError, link_folder};

use super::live;

fn copy_fixture_folder() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("docs");
    std::fs::create_dir(&dir).unwrap();
    for e in std::fs::read_dir(fixture("folders/spike")).unwrap() {
        let e = e.unwrap();
        std::fs::copy(e.path(), dir.join(e.file_name())).unwrap();
    }
    (tmp, dir)
}

fn models(egress: &Arc<FakeEgress>) -> Vec<ModelUnderTest> {
    vec![
        ModelUnderTest {
            label: "frontier".into(),
            provider: Arc::new(AnthropicProvider::new(egress.clone(), "spike-anthropic")),
            model: "claude-opus-5-5".into(),
            price: Some(anthropic_price("claude-opus-5-5")),
        },
        ModelUnderTest {
            label: "local".into(),
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
    assert!(c4_passed(&rows));
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
        Some(m) => models.push(ModelUnderTest {
            label: format!("local: lmstudio {m}"),
            provider: Arc::new(OpenAiCompatProvider::lm_studio(egress.clone()).with_vision(true)),
            price: None,
            model: m,
        }),
        None => eprintln!("local half skipped: LM Studio is not answering"),
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
    assert!(c4_passed(&rows), "{table}");
}
