//! The `ProjectIdentity` adapters (AD-32, AD-47, story 1.13): a project
//! root's volume and file id and this Mac's name, read from the OS. No value
//! is ever guessed or filled in from a fallback: when a tool fails, times
//! out or returns output without the expected value, the open is refused
//! naming the tool and what it returned.
//!
//! - **macOS** ([`MacosIdentity`]): the root's device from `/usr/bin/stat -f
//!   %Sd <root>`, then its `VolumeUUID` from `/usr/sbin/diskutil info -plist
//!   <device>` (never `diskutil` on `/`, whose sealed-snapshot UUID changes
//!   with macOS updates); the file id is the root's inode from std metadata;
//!   the Mac's name from `/usr/sbin/scutil --get ComputerName`. Each tool
//!   runs by absolute path with a time limit (10 s in [`MacosIdentity::macos`];
//!   tests build the adapter over stub tools with a shorter one, by
//!   constructor parameter only). A root on a volume with no VolumeUUID (a
//!   network share: a device that is not a disk, or a disk whose `diskutil`
//!   plist has no VolumeUUID) is refused as not supported in v1; output that
//!   is not such a plist names the tool instead. A recorded root is
//!   located with `diskutil info -plist <VolumeUUID>` (its mount point) and
//!   `/.vol/<device>/<file id>` through std metadata.
//! - **Linux** ([`LinuxIdentity`], so the core opens projects in the CI
//!   job): device and inode from std metadata, the name from
//!   `/proc/sys/kernel/hostname`; a volume is always mounted and a recorded
//!   root is looked up by its recorded path's inode.
//! - Anywhere else: "not supported on this platform yet".

use std::sync::Arc;
#[cfg(any(target_os = "macos", test))]
use std::time::Duration;

use am_domain::ProjectIdentity;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use am_domain::{IdentityError, RootIdentity, VolumeState};

/// Each identity tool's time limit in production.
#[cfg(target_os = "macos")]
pub const TOOL_LIMIT: Duration = Duration::from_secs(10);

/// The identity adapter for this platform.
pub fn system() -> Arc<dyn ProjectIdentity> {
    #[cfg(target_os = "macos")]
    {
        Arc::new(MacosIdentity::macos())
    }
    #[cfg(target_os = "linux")]
    {
        Arc::new(LinuxIdentity)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Arc::new(UnsupportedIdentity)
    }
}

/// Output shown in a refusal: trimmed, one line per line, at most 400 chars.
#[cfg(any(target_os = "macos", test))]
fn shown(text: &str) -> String {
    let t = text.trim();
    let t: String = t.chars().take(400).collect();
    if t.is_empty() {
        "nothing".to_string()
    } else {
        format!("\"{t}\"")
    }
}

/// The `<string>` value following `<key>{key}</key>` in a plist, if any.
#[cfg(any(target_os = "macos", test))]
fn plist_string(plist: &str, key: &str) -> Option<String> {
    let marker = format!("<key>{key}</key>");
    let rest = &plist[plist.find(&marker)? + marker.len()..];
    let rest = rest.trim_start();
    let value = rest.strip_prefix("<string>")?;
    let end = value.find("</string>")?;
    let v = value[..end]
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&");
    Some(v)
}

#[cfg(target_os = "macos")]
mod macos {
    use std::io::Read;
    use std::os::unix::fs::MetadataExt;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use super::*;

    /// The macOS adapter, over the tools it is built with.
    #[derive(Debug, Clone)]
    pub struct MacosIdentity {
        stat: PathBuf,
        diskutil: PathBuf,
        scutil: PathBuf,
        limit: Duration,
    }

    impl MacosIdentity {
        /// The production adapter: `/usr/bin/stat`, `/usr/sbin/diskutil`
        /// and `/usr/sbin/scutil`, each with a 10-second limit.
        pub fn macos() -> MacosIdentity {
            MacosIdentity::with_tools(
                PathBuf::from("/usr/bin/stat"),
                PathBuf::from("/usr/sbin/diskutil"),
                PathBuf::from("/usr/sbin/scutil"),
                TOOL_LIMIT,
            )
        }

        /// An adapter over these tools (absolute paths) and this limit.
        pub fn with_tools(
            stat: PathBuf,
            diskutil: PathBuf,
            scutil: PathBuf,
            limit: Duration,
        ) -> MacosIdentity {
            MacosIdentity {
                stat,
                diskutil,
                scutil,
                limit,
            }
        }

        pub fn limit(&self) -> Duration {
            self.limit
        }

        pub fn tools(&self) -> [&Path; 3] {
            [&self.stat, &self.diskutil, &self.scutil]
        }

        fn run(&self, tool: &Path, args: &[&str]) -> Result<String, IdentityError> {
            run_tool(tool, args, self.limit)
        }
    }

    fn tool_error(tool: &Path, args: &[&str], output: String) -> IdentityError {
        IdentityError::Tool {
            tool: format!("{} {}", tool.display(), args.join(" ")),
            output,
        }
    }

    /// Runs `tool args` (no stdin), waiting at most `limit`; its stdout on
    /// success, otherwise a refusal naming the tool and what it returned.
    fn run_tool(tool: &Path, args: &[&str], limit: Duration) -> Result<String, IdentityError> {
        let mut child = Command::new(tool)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| tool_error(tool, args, format!("it could not be started: {e}")))?;
        // Each stream is read on its own thread and sent back whole; the
        // wait for them is bounded too, since a hung tool's own children can
        // keep a pipe open after it is killed.
        let receivers: Vec<std::sync::mpsc::Receiver<String>> = [
            child
                .stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
            child
                .stderr
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
        ]
        .into_iter()
        .flatten()
        .map(|mut s| {
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let _ = s.read_to_end(&mut buf);
                let _ = tx.send(String::from_utf8_lossy(&buf).into_owned());
            });
            rx
        })
        .collect();
        let deadline = Instant::now() + limit;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                Err(e) => {
                    return Err(tool_error(
                        tool,
                        args,
                        format!("it could not be waited for: {e}"),
                    ));
                }
            }
        };
        let grace = Instant::now() + Duration::from_millis(200);
        let mut texts = receivers.into_iter().map(|rx| {
            let wait = deadline
                .max(grace)
                .saturating_duration_since(Instant::now());
            rx.recv_timeout(wait).unwrap_or_default()
        });
        let out = texts.next().unwrap_or_default();
        let err = texts.next().unwrap_or_default();
        match status {
            None => Err(tool_error(
                tool,
                args,
                format!(
                    "no answer within {} s (output so far: {})",
                    limit.as_secs_f64(),
                    shown(&format!("{out}{err}"))
                ),
            )),
            Some(s) if !s.success() => Err(tool_error(
                tool,
                args,
                format!("{s}: {}", shown(&format!("{out}{err}"))),
            )),
            Some(_) => Ok(out),
        }
    }

    /// A device name `stat -f %Sd` gives for a disk (`disk3s5`).
    fn is_disk(device: &str) -> bool {
        device
            .strip_prefix("disk")
            .is_some_and(|r| !r.is_empty() && r.chars().all(|c| c.is_ascii_digit() || c == 's'))
    }

    /// A device name of a network or automounted share.
    fn is_share(device: &str) -> bool {
        device.starts_with("//") || device.starts_with("map ") || device.contains(":/")
    }

    impl ProjectIdentity for MacosIdentity {
        fn root(&self, root: &Path) -> Result<RootIdentity, IdentityError> {
            let shown_root = root.to_string_lossy().into_owned();
            let stat_args = ["-f", "%Sd", shown_root.as_str()];
            let device = self.run(&self.stat, &stat_args)?.trim().to_string();
            if is_share(&device) {
                return Err(IdentityError::NetworkShare {
                    path: root.to_path_buf(),
                });
            }
            if !is_disk(&device) {
                return Err(tool_error(
                    &self.stat,
                    &stat_args,
                    format!("{} (no disk device)", shown(&device)),
                ));
            }
            let info_args = ["info", "-plist", device.as_str()];
            let plist = self.run(&self.diskutil, &info_args)?;
            let volume = match plist_string(&plist, "VolumeUUID")
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
            {
                Some(v) => v,
                // A volume `diskutil` describes (its plist names it) but
                // gives no VolumeUUID: not supported in v1 (AD-32).
                None if plist_string(&plist, "DeviceIdentifier").is_some() => {
                    return Err(IdentityError::NetworkShare {
                        path: root.to_path_buf(),
                    });
                }
                None => {
                    return Err(tool_error(
                        &self.diskutil,
                        &info_args,
                        format!("{} (no VolumeUUID)", shown(&plist)),
                    ));
                }
            };
            let file_id = std::fs::metadata(root)
                .map_err(|e| tool_error(Path::new("stat(2)"), &[&shown_root], e.to_string()))?
                .ino();
            Ok(RootIdentity {
                volume,
                file_id,
                path: root.to_path_buf(),
            })
        }

        fn locate(&self, recorded: &RootIdentity) -> Result<VolumeState, IdentityError> {
            let info_args = ["info", "-plist", recorded.volume.as_str()];
            let plist = match self.run(&self.diskutil, &info_args) {
                Ok(p) => p,
                Err(IdentityError::Tool { output, .. })
                    if output.contains("Could not find disk") =>
                {
                    return Ok(VolumeState::Unmounted);
                }
                Err(e) => return Err(e),
            };
            let Some(mount) = plist_string(&plist, "MountPoint").filter(|m| !m.trim().is_empty())
            else {
                return Ok(VolumeState::Unmounted);
            };
            let device = std::fs::metadata(&mount)
                .map_err(|e| tool_error(Path::new("stat(2)"), &[&mount], e.to_string()))?
                .dev();
            let by_id = PathBuf::from(format!("/.vol/{device}/{}", recorded.file_id));
            match std::fs::metadata(&by_id) {
                Ok(m) if m.is_dir() => Ok(VolumeState::Mounted { root: Some(by_id) }),
                Ok(_) => Ok(VolumeState::Mounted { root: None }),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    Ok(VolumeState::Mounted { root: None })
                }
                Err(e) => Err(tool_error(
                    Path::new("stat(2)"),
                    &[&by_id.to_string_lossy()],
                    e.to_string(),
                )),
            }
        }

        fn mac_name(&self) -> Result<String, IdentityError> {
            let args = ["--get", "ComputerName"];
            let out = self.run(&self.scutil, &args)?;
            let name = out.trim();
            if name.is_empty() || name.contains('\n') {
                return Err(tool_error(&self.scutil, &args, shown(&out)));
            }
            Ok(name.to_string())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn disk_and_share_devices() {
            assert!(is_disk("disk3s5"));
            assert!(is_disk("disk4"));
            assert!(!is_disk("disk"));
            assert!(!is_disk("devfs"));
            assert!(is_share("//me@host/share"));
            assert!(is_share("host:/export"));
            assert!(is_share("map auto_home"));
            assert!(!is_share("disk3s5"));
        }
    }
}

#[cfg(target_os = "macos")]
pub use macos::MacosIdentity;

/// The Linux adapter (CI): std metadata and `/proc/sys/kernel/hostname`.
#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy, Default)]
pub struct LinuxIdentity;

#[cfg(target_os = "linux")]
impl ProjectIdentity for LinuxIdentity {
    fn root(&self, root: &std::path::Path) -> Result<RootIdentity, IdentityError> {
        use std::os::unix::fs::MetadataExt;
        let m = std::fs::metadata(root).map_err(|e| IdentityError::Tool {
            tool: format!("stat(2) {}", root.display()),
            output: e.to_string(),
        })?;
        Ok(RootIdentity {
            volume: format!("dev-{}", m.dev()),
            file_id: m.ino(),
            path: root.to_path_buf(),
        })
    }

    fn locate(&self, recorded: &RootIdentity) -> Result<VolumeState, IdentityError> {
        use std::os::unix::fs::MetadataExt;
        let present = std::fs::metadata(&recorded.path).is_ok_and(|m| {
            m.is_dir()
                && m.ino() == recorded.file_id
                && format!("dev-{}", m.dev()) == recorded.volume
        });
        Ok(VolumeState::Mounted {
            root: present.then(|| recorded.path.clone()),
        })
    }

    fn mac_name(&self) -> Result<String, IdentityError> {
        let file = "/proc/sys/kernel/hostname";
        let text = std::fs::read_to_string(file).map_err(|e| IdentityError::Tool {
            tool: file.into(),
            output: e.to_string(),
        })?;
        let name = text.trim();
        if name.is_empty() {
            return Err(IdentityError::Tool {
                tool: file.into(),
                output: "nothing".into(),
            });
        }
        Ok(name.to_string())
    }
}

/// Any other platform.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub struct UnsupportedIdentity;

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
impl ProjectIdentity for UnsupportedIdentity {
    fn root(
        &self,
        _root: &std::path::Path,
    ) -> Result<am_domain::RootIdentity, am_domain::IdentityError> {
        Err(am_domain::IdentityError::Unsupported)
    }
    fn locate(
        &self,
        _recorded: &am_domain::RootIdentity,
    ) -> Result<am_domain::VolumeState, am_domain::IdentityError> {
        Err(am_domain::IdentityError::Unsupported)
    }
    fn mac_name(&self) -> Result<String, am_domain::IdentityError> {
        Err(am_domain::IdentityError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_values_are_read_by_key() {
        let p = "<dict>\n\t<key>MountPoint</key>\n\t<string>/Volumes/A &amp; B</string>\n\t<key>VolumeUUID</key>\n\t<string>0687DA5D-9642</string>\n</dict>";
        assert_eq!(
            plist_string(p, "MountPoint").as_deref(),
            Some("/Volumes/A & B")
        );
        assert_eq!(
            plist_string(p, "VolumeUUID").as_deref(),
            Some("0687DA5D-9642")
        );
        assert_eq!(plist_string(p, "Nope"), None);
        assert_eq!(shown("  "), "nothing");
        let _ = Duration::from_secs(1);
    }
}
