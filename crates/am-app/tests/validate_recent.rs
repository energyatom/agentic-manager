//! Story 1.14 review fix (AD-30): `agentic validate` on a project passes the
//! roots of the other projects recorded in this Mac's runtime base as recent
//! roots, so a seat linking a folder inside another recorded project is
//! refused at its `path` line. Runs the real binary with HOME (and
//! XDG_DATA_HOME) set to a temporary folder, as `serve_project.rs` does; the
//! other project's root record is written in the runtime base's format. The
//! project being validated is never its own recent root.
#![forbid(unsafe_code)]
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

use am_domain::runtime::RUNTIME_BASE_NAME;
use am_domain::{ProjectId, RuntimeBase};

const COMPANY: &str = "schema_version: 2\nname: Example\ntimezone: UTC\ncurrency: EUR\nteams:\n  - id: core\n    name: Core team\n";

fn seat(path: &str) -> String {
    format!(
        "---\nschema_version: 2\nname: Keeper\nrole: Writer\nteam: core\nfolders:\n  - id: work\n    path: \"{path}\"\n    access: read\n---\nKeep notes.\n"
    )
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// The runtime base the binary resolves for `home` (and `xdg` off macOS).
fn base(home: &Path, xdg: &Path) -> RuntimeBase {
    let support = if cfg!(target_os = "macos") {
        home.join("Library/Application Support")
    } else {
        xdg.to_path_buf()
    };
    RuntimeBase::new(support.join(RUNTIME_BASE_NAME))
}

/// Records `root` as a project in the runtime base (its root record).
fn record(base: &RuntimeBase, root: &Path) {
    let id = ProjectId::mint();
    let layout = base.project(&id);
    std::fs::create_dir_all(layout.root_record().parent().unwrap()).unwrap();
    let hex: String = root
        .as_os_str()
        .as_encoded_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    std::fs::write(
        layout.root_record(),
        format!("project_id: {id}\nvolume: test-volume\nfile_id: 1\npath: {hex}\n"),
    )
    .unwrap();
}

fn validate(home: &Path, xdg: &Path, project: &Path) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_agentic"))
        .args(["validate", project.to_str().unwrap()])
        .env("HOME", home)
        .env("XDG_DATA_HOME", xdg)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

#[test]
fn a_folder_inside_another_recorded_project_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let tmp: PathBuf = dir.path().canonicalize().unwrap();
    let (home, xdg) = (tmp.join("home"), tmp.join("xdg"));
    std::fs::create_dir_all(&home).unwrap();
    let mine = tmp.join("projects/mine");
    let other = tmp.join("projects/other");
    write(&mine, "company.yaml", COMPANY);
    write(&other, "company.yaml", COMPANY);
    std::fs::create_dir_all(other.join("files/in")).unwrap();
    let base = base(&home, &xdg);
    // Both projects are recorded; only the other one is a recent root.
    record(&base, &mine);
    record(&base, &other);

    write(&mine, "seats/keeper.md", &seat("../other/files/in"));
    let (code, out) = validate(&home, &xdg, &mine);
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains("seats/keeper.md:8: folder path '../other/files/in' is not allowed"),
        "{out}"
    );
    assert!(out.contains("recent project"), "{out}");

    // A folder outside every recorded project validates.
    std::fs::create_dir_all(tmp.join("elsewhere")).unwrap();
    write(&mine, "seats/keeper.md", &seat("../../elsewhere"));
    let (code, out) = validate(&home, &xdg, &mine);
    assert_eq!(code, 0, "{out}");
}

/// The project being validated is recorded too, and is not its own recent
/// root, however its path is written: validated through a symlink to it, a
/// seat linking its in-project `files/in` read-only validates.
#[test]
fn the_validated_project_is_not_its_own_recent_root() {
    let dir = tempfile::tempdir().unwrap();
    let tmp: PathBuf = dir.path().canonicalize().unwrap();
    let (home, xdg) = (tmp.join("home"), tmp.join("xdg"));
    std::fs::create_dir_all(&home).unwrap();
    let mine = tmp.join("projects/mine");
    write(&mine, "company.yaml", COMPANY);
    std::fs::create_dir_all(mine.join("files/in")).unwrap();
    std::fs::write(mine.join("files/in/notes.txt"), "notes\n").unwrap();
    let base = base(&home, &xdg);
    record(&base, &mine);
    let via = tmp.join("via-link");
    std::os::unix::fs::symlink(&mine, &via).unwrap();

    write(&mine, "seats/keeper.md", &seat("files/in"));
    let (code, out) = validate(&home, &xdg, &via);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("recent project"), "{out}");
}
