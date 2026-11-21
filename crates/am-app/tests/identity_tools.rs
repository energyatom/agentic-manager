//! Story 1.13: the macOS `ProjectIdentity` adapter over stub tools (AD-32,
//! AD-47). The adapter is built by constructor parameter (its tool paths and
//! its time limit); no flag or env var selects either. A tool that fails,
//! hangs past the limit or prints output without the expected value refuses
//! the open naming the tool and what it returned, with nothing written; a
//! share is not supported in v1; the production constructor uses the real
//! absolute paths and 10 s.
#![forbid(unsafe_code)]
#![cfg(target_os = "macos")]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use am_app::identity::MacosIdentity;
use am_app::runtime::{OpenRequest, Places, Prompt, open_project};
use am_domain::{IdentityError, ProjectIdentity, RootIdentity, VolumeState};

const PLIST: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\">\n<dict>\n\t<key>MountPoint</key>\n\t<string>/</string>\n\t<key>VolumeUUID</key>\n\t<string>0A1B2C3D-0000-4000-8000-00000000ABCD</string>\n</dict>\n</plist>\n";

/// A stub tool: a shell script printing `out` and exiting with `code`
/// (after `sleep` seconds when given).
fn stub(dir: &Path, name: &str, out: &str, code: i32, sleep: Option<u32>) -> PathBuf {
    let path = dir.join(name);
    let pause = sleep.map(|s| format!("sleep {s}\n")).unwrap_or_default();
    std::fs::write(
        &path,
        format!("#!/bin/sh\n{pause}cat <<'STUB_EOF'\n{out}\nSTUB_EOF\nexit {code}\n"),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

struct Tools {
    stat: PathBuf,
    diskutil: PathBuf,
    scutil: PathBuf,
}

fn working(dir: &Path) -> Tools {
    Tools {
        stat: stub(dir, "stat", "disk9s1", 0, None),
        diskutil: stub(dir, "diskutil", PLIST, 0, None),
        scutil: stub(dir, "scutil", "Stub Mac", 0, None),
    }
}

fn adapter(t: &Tools, limit: Duration) -> MacosIdentity {
    MacosIdentity::with_tools(t.stat.clone(), t.diskutil.clone(), t.scutil.clone(), limit)
}

fn tool_error(e: IdentityError) -> (String, String) {
    match e {
        IdentityError::Tool { tool, output } => (tool, output),
        other => panic!("expected a tool error, got {other:?}"),
    }
}

#[test]
fn working_tools_give_the_volume_uuid_the_inode_and_the_name() {
    let dir = tempfile::tempdir().unwrap();
    let t = working(dir.path());
    let a = adapter(&t, Duration::from_secs(5));
    let root = a.root(dir.path()).unwrap();
    assert_eq!(root.volume, "0A1B2C3D-0000-4000-8000-00000000ABCD");
    use std::os::unix::fs::MetadataExt;
    assert_eq!(root.file_id, std::fs::metadata(dir.path()).unwrap().ino());
    assert_eq!(a.mac_name().unwrap(), "Stub Mac");
}

#[test]
fn a_failing_tool_is_named_with_what_it_returned() {
    let dir = tempfile::tempdir().unwrap();
    let mut t = working(dir.path());
    t.scutil = stub(dir.path(), "scutil-fails", "scutil: boom", 3, None);
    let (tool, output) = tool_error(adapter(&t, Duration::from_secs(5)).mac_name().unwrap_err());
    assert!(tool.contains("scutil-fails --get ComputerName"), "{tool}");
    assert!(output.contains("boom"), "{output}");
}

#[test]
fn a_hanging_tool_is_stopped_at_the_limit_and_named() {
    let dir = tempfile::tempdir().unwrap();
    let mut t = working(dir.path());
    t.diskutil = stub(dir.path(), "diskutil-hangs", PLIST, 0, Some(30));
    let started = Instant::now();
    // The limit is wide enough for the stub `stat` to answer while the
    // whole suite runs in parallel; only `diskutil` hangs (30 s).
    let err = adapter(&t, Duration::from_secs(3))
        .root(dir.path())
        .unwrap_err();
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "stopped at the limit"
    );
    let (tool, output) = tool_error(err);
    assert!(
        tool.contains("diskutil-hangs info -plist disk9s1"),
        "{tool}"
    );
    assert!(output.contains("no answer within 3 s"), "{output}");
}

#[test]
fn output_without_the_expected_value_is_refused_naming_the_tool() {
    let dir = tempfile::tempdir().unwrap();
    let mut t = working(dir.path());
    t.diskutil = stub(
        dir.path(),
        "diskutil-nokey",
        "<plist><dict></dict></plist>",
        0,
        None,
    );
    let (tool, output) = tool_error(
        adapter(&t, Duration::from_secs(5))
            .root(dir.path())
            .unwrap_err(),
    );
    assert!(tool.contains("diskutil-nokey"), "{tool}");
    assert!(output.contains("no VolumeUUID"), "{output}");

    let mut t = working(dir.path());
    t.scutil = stub(dir.path(), "scutil-empty", "", 0, None);
    let (tool, _) = tool_error(adapter(&t, Duration::from_secs(5)).mac_name().unwrap_err());
    assert!(tool.contains("scutil-empty"), "{tool}");

    let mut t = working(dir.path());
    t.stat = stub(dir.path(), "stat-odd", "devfs", 0, None);
    let (tool, output) = tool_error(
        adapter(&t, Duration::from_secs(5))
            .root(dir.path())
            .unwrap_err(),
    );
    assert!(tool.contains("stat-odd -f %Sd"), "{tool}");
    assert!(output.contains("devfs"), "{output}");
}

#[test]
fn a_share_is_not_supported_in_v1() {
    let dir = tempfile::tempdir().unwrap();
    let mut t = working(dir.path());
    t.stat = stub(dir.path(), "stat-share", "//me@nas.local/projects", 0, None);
    let err = adapter(&t, Duration::from_secs(5))
        .root(dir.path())
        .unwrap_err();
    assert!(matches!(err, IdentityError::NetworkShare { .. }), "{err:?}");
    assert!(err.to_string().contains("not supported in v1"), "{err}");
}

#[test]
fn a_disk_volume_with_no_volume_uuid_is_not_supported_in_v1() {
    let dir = tempfile::tempdir().unwrap();
    let mut t = working(dir.path());
    t.diskutil = stub(
        dir.path(),
        "diskutil-nouuid",
        "<plist><dict><key>DeviceIdentifier</key><string>disk9s1</string><key>MountPoint</key><string>/Volumes/X</string></dict></plist>",
        0,
        None,
    );
    let err = adapter(&t, Duration::from_secs(5))
        .root(dir.path())
        .unwrap_err();
    assert!(matches!(err, IdentityError::NetworkShare { .. }), "{err:?}");
    assert!(err.to_string().contains("no VolumeUUID"), "{err}");
    assert!(err.to_string().contains("not supported in v1"), "{err}");
}

#[test]
fn an_unknown_volume_reads_as_unmounted() {
    let dir = tempfile::tempdir().unwrap();
    let mut t = working(dir.path());
    t.diskutil = stub(
        dir.path(),
        "diskutil-gone",
        "<plist><dict><key>ErrorMessage</key><string>Could not find disk: X</string></dict></plist>",
        1,
        None,
    );
    let rec = RootIdentity {
        volume: "X".into(),
        file_id: 1,
        path: dir.path().to_path_buf(),
    };
    assert_eq!(
        adapter(&t, Duration::from_secs(5)).locate(&rec).unwrap(),
        VolumeState::Unmounted
    );
}

#[test]
fn the_production_adapter_uses_the_real_tools_and_ten_seconds() {
    let a = MacosIdentity::macos();
    assert_eq!(a.limit(), Duration::from_secs(10));
    assert_eq!(
        a.tools(),
        [
            Path::new("/usr/bin/stat"),
            Path::new("/usr/sbin/diskutil"),
            Path::new("/usr/sbin/scutil")
        ]
    );
}

struct NoTerminal;
impl Prompt for NoTerminal {
    fn interactive(&self) -> bool {
        false
    }
    fn ask(&mut self, _: &str) -> bool {
        false
    }
}

#[test]
fn a_failing_tool_refuses_the_open_with_nothing_written() {
    let dir = tempfile::tempdir().unwrap();
    let tools = dir.path().join("tools");
    std::fs::create_dir(&tools).unwrap();
    let mut t = working(&tools);
    t.diskutil = stub(&tools, "diskutil-fails", "diskutil: no such disk", 1, None);
    let home = dir.path().join("home");
    let places = Places::under(&home.join("Library/Application Support"), &home);
    let root = dir.path().join("project");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("company.yaml"), "schema_version: 1\nname: X\n").unwrap();
    let req = OpenRequest {
        root: root.clone(),
        confirm_move: false,
        take_over: false,
    };
    let err = match open_project(
        &places,
        &adapter(&t, Duration::from_secs(5)),
        &req,
        &mut NoTerminal,
    ) {
        Ok(_) => panic!("refused"),
        Err(e) => e.0,
    };
    assert!(err.contains("diskutil-fails info -plist disk9s1"), "{err}");
    assert!(err.contains("no such disk"), "{err}");
    assert!(!home.exists(), "nothing written");
    assert_eq!(
        std::fs::read_to_string(root.join("company.yaml")).unwrap(),
        "schema_version: 1\nname: X\n"
    );
}
