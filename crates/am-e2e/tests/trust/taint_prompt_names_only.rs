//! Trust test (AD-10, AD-27, AD-30, AD-38; epic 3 story 5, matrix rows 5,
//! 6 and 32): a skill's reference files, a seat's reference files and a
//! seat's saved links reach a prompt by name only.
//!
//! On a copy of `fixtures/projects/prompt-parts` through `agentic run` (the
//! real plan builder, a FakeLlm model): canary text planted in the skill's
//! reference file (which the config loader reads into the snapshot, so it
//! is there to leak), in the seat's two reference files (in the runtime
//! folder, where the app keeps them) and in a reference file's `note` and a
//! link's `group` appears in no request part, no `prompt_parts` row, no
//! audit entry and nothing the command printed; the names do appear (the
//! skill's `references/<name>`, the reference files' names, each link as
//! `title — url`). No file is fetched: a saved link's page has no source in
//! the run at all.
//!
//! Written before the plan builder fills these parts.
#![forbid(unsafe_code)]
#![cfg(unix)]

#[path = "../engine/support/mod.rs"]
mod support;

use am_domain::Part;
use support::*;

const SKILL_REFERENCE: &str = "skill-reference-canary-4c1e77";
const GUIDE_FILE: &str = "seat-file-canary-9b2740";
const TERMS_FILE: &str = "seat-file-canary-terms-61d0f3";
const NOTE: &str = "note-canary-83ab1c";
const GROUP: &str = "group-canary-5d2e90";
const CANARIES: [&str; 5] = [SKILL_REFERENCE, GUIDE_FILE, TERMS_FILE, NOTE, GROUP];

const GOOD: &str = r#"{"line":"hello"}"#;

#[tokio::test]
async fn canaries_in_reference_files_skill_references_and_links_reach_no_request_journal_or_audit()
{
    let cli = cli();
    let root = cli.copy_of("prompt-parts");
    cli.open_once(&root);
    cli.seed_knowledge(&root, "alpha");

    // The canaries are really where the app reads: the loader holds the
    // skill's reference text, the config holds the note and the group, and
    // the runtime folder holds the reference files.
    let snapshot = am_config::load(&root)
        .unwrap()
        .snapshot
        .expect("the fixture loads");
    let skill = &snapshot.skills()["brief"];
    assert!(
        skill
            .references
            .values()
            .any(|t| t.contains(SKILL_REFERENCE)),
        "the loader read the skill reference: {:?}",
        skill.references.keys().collect::<Vec<_>>()
    );
    let seat = &snapshot.seats()["alpha"];
    assert!(
        seat.knowledge
            .iter()
            .any(|k| k.note.as_deref() == Some(NOTE))
    );
    assert!(seat.links.iter().any(|l| l.group.as_deref() == Some(GROUP)));
    let layout = cli.layout(&root).unwrap();
    for (file, canary) in [("guide.md", GUIDE_FILE), ("terms.txt", TERMS_FILE)] {
        let text = std::fs::read_to_string(layout.knowledge("alpha").join(file)).unwrap();
        assert!(text.contains(canary), "{file}");
    }

    let llm = texts(&[GOOD]);
    let ran = cli.run(&root, "hello", local(llm.clone())).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    let request = llm.requests()[0].clone();
    let sent = format!("{request:?}");
    for canary in CANARIES {
        assert!(!sent.contains(canary), "{canary} is in the request");
        assert!(!ran.all().contains(canary), "{canary} was printed");
    }
    let system: Vec<String> = request
        .system
        .iter()
        .map(|i| i.text().to_string())
        .collect();
    let joined = system.join("\n");
    // Names only.
    assert!(joined.contains("references/layout.md"), "{joined}");
    for name in ["guide.md", "terms.txt"] {
        assert!(joined.contains(name), "{name}: {joined}");
    }
    assert!(
        joined.contains("Home \u{2014} https://example.org/"),
        "{joined}"
    );
    assert!(
        joined.contains("Reference \u{2014} https://docs.example.org/reference"),
        "{joined}"
    );
    assert!(
        request
            .messages
            .iter()
            .flat_map(|m| m.parts.iter())
            .all(|p| matches!(p, Part::Instruction(_))),
        "only the step instruction on the user side: {:?}",
        request.messages
    );

    let j = journal(&cli.db(&root));
    assert_eq!(j.runs.len(), 1);
    let id = j.attempt("write", 1).id;
    let rows = j.parts_of(&id);
    assert!(!rows.is_empty(), "the prompt was journaled");
    for row in &rows {
        for canary in CANARIES {
            assert!(
                !row.label.contains(canary)
                    && !row.text.as_deref().unwrap_or_default().contains(canary),
                "{canary} is in a journal row: {row:?}"
            );
        }
    }
    let files = rows
        .iter()
        .find(|r| r.label == "Reference files")
        .expect("the part");
    let links = rows
        .iter()
        .find(|r| r.label == "Saved links")
        .expect("the part");
    let (files, links) = (files.text.clone().unwrap(), links.text.clone().unwrap());
    assert!(
        files.contains("guide.md") && files.contains("terms.txt"),
        "{files}"
    );
    assert_eq!(
        links.lines().filter(|l| l.contains(" \u{2014} ")).count(),
        2,
        "one line per link: {links:?}"
    );
    let export = serde_json::to_string(&j.audit).unwrap();
    for canary in CANARIES {
        assert!(!export.contains(canary), "{canary} is in the audit log");
    }
    assert!(
        !export.contains("layout.md") && !export.contains("example.org"),
        "not even a name: {export}"
    );
}
