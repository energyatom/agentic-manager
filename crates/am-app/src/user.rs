//! This Mac's Accounts on the app's side (story 2.12; AD-13, AD-15, AD-17):
//! the owner-only check of the app data dir, the user store's lock and the
//! writer of the user Accounts file under it, a read of where the file and
//! its records stand, and the scan of every project this Mac knows for the
//! places that name an Account.
//!
//! - [`owner_only`]: before the user store is opened, the app data dir must
//!   be 0700 and `accounts.yaml`, `user.db` (with its WAL files) and
//!   `user.lock` 0600 when present; anything else is refused naming the path
//!   and the `chmod` that fixes it.
//! - [`Locked`]: `user.lock` held (a bounded wait, refused in words after
//!   it), the user store migrated under it, and the writer of the user
//!   Accounts file, which has settled the writes a crash interrupted.
//! - [`Standing`]: the file as it is now, with the latest History entry and
//!   the pending intents of the user store, read-only and without the lock.
//! - [`scan`]: every project of the runtime base's root records whose root
//!   is reachable, read only through am-config.
//! - Story 2.13: [`Locked::take`] creates the user store marked `recovered`
//!   when this Mac's Accounts file or the `accounts migrate` backups already
//!   exist ([`would_be_recovered`]); [`recovered`] reads the mark, or what
//!   it would be, without the lock.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use am_config::{AccountUse, AccountsFile, USER_ACCOUNTS_FILE};
use am_domain::runtime::AppDataLayout;
use am_domain::{ConfigHistory, HistoryEntry, PendingIntent};
use am_store::user::{UserReader, UserStore};

use crate::runtime::Places;

/// How long a write waits for `user.lock`, and a key check for a write in
/// flight (AD-6, owner-approved: 10 s, polled).
pub const LOCK_WAIT: Duration = Duration::from_secs(10);

/// The sha256 of no bytes.
const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// Refuses an app data dir that is not owner-only, or a file in it that
/// others can read (AD-13, AD-17). Nothing is checked that is not there.
pub fn owner_only(layout: &AppDataLayout) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let dir = layout.dir();
        if let Ok(m) = std::fs::symlink_metadata(dir) {
            let mode = m.permissions().mode() & 0o777;
            if !m.is_dir() || mode & 0o077 != 0 {
                return Err(format!(
                    "this Mac's app data folder {} can be read or entered by other users (mode {mode:o}), and it holds your Accounts' records; run `chmod 700 {}`, then run the command again",
                    dir.display(),
                    dir.display()
                ));
            }
        }
        let db = layout.user_db();
        let name = db
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let files = [
            layout.accounts_file(),
            db.clone(),
            db.with_file_name(format!("{name}-wal")),
            db.with_file_name(format!("{name}-shm")),
            layout.user_lock(),
        ];
        for path in files {
            if let Ok(m) = std::fs::symlink_metadata(&path) {
                let mode = m.permissions().mode() & 0o777;
                if mode & 0o077 != 0 {
                    return Err(format!(
                        "{} can be read by other users (mode {mode:o}); run `chmod 600 {}`, then run the command again",
                        path.display(),
                        path.display()
                    ));
                }
            }
        }
    }
    #[cfg(not(unix))]
    let _ = layout;
    Ok(())
}

/// A read-only view of the user store (`None` words: refused).
pub fn reader(places: &Places) -> Result<UserReader, String> {
    owner_only(&places.user)?;
    UserReader::open(&places.user).map_err(|e| e.to_string())
}

/// The user Accounts file now, and where its records stand.
pub struct Standing {
    pub file: AccountsFile,
    /// The latest History entry of the file in the user store.
    pub last: Option<HistoryEntry>,
    pub pending: Vec<PendingIntent>,
}

impl Standing {
    /// Reads it without the lock (read-only).
    pub fn read(places: &Places) -> Result<Standing, String> {
        let reader = reader(places)?;
        let file = am_config::accounts_file(places.user.dir());
        let last = reader
            .latest_config_entry(USER_ACCOUNTS_FILE)
            .map_err(|e| e.to_string())?;
        let pending = reader.config_intents().map_err(|e| e.to_string())?;
        Ok(Standing {
            file,
            last,
            pending,
        })
    }

    /// Reads it through the lock's own History.
    fn under(places: &Places, history: &dyn ConfigHistory) -> Result<Standing, String> {
        let file = am_config::accounts_file(places.user.dir());
        let last = history
            .entries(Some(USER_ACCOUNTS_FILE.to_string()))
            .map_err(|e| e.to_string())?
            .pop();
        let pending = history.pending_intents().map_err(|e| e.to_string())?;
        Ok(Standing {
            file,
            last,
            pending,
        })
    }

    /// The file's bytes as compared with History: an empty file is the same
    /// as no file (AD-16).
    fn on_disk(&self) -> Option<&str> {
        self.file.sha256.as_deref().filter(|h| *h != EMPTY_SHA256)
    }

    /// True when the file was changed outside the app: its bytes differ from
    /// what its latest History entry wrote. A file with no History entry is
    /// not judged; a file holding the bytes of a pending intent is a write a
    /// crash interrupted, which the next lock settles.
    pub fn outside_change(&self) -> bool {
        let Some(last) = &self.last else {
            return false;
        };
        let after = (!last.after.is_empty()).then_some(last.after_sha256.as_str());
        let on_disk = self.on_disk();
        on_disk != after
            && !self.pending.iter().any(|p| {
                p.intent.file == USER_ACCOUNTS_FILE
                    && Some(p.intent.after_sha256.as_str()) == on_disk
            })
    }

    /// True when the file holds the bytes of a pending intent (its History
    /// entry not yet committed).
    pub fn pending_holds(&self) -> bool {
        let on_disk = self.on_disk();
        self.pending.iter().any(|p| {
            p.intent.file == USER_ACCOUNTS_FILE && Some(p.intent.after_sha256.as_str()) == on_disk
        })
    }

    /// The sequence of the latest History entry, 0 with none.
    pub fn seq(&self) -> i64 {
        self.last.as_ref().map_or(0, |e| e.seq)
    }

    /// Whether `now` (read later, under the lock) still stands where this
    /// read stood for an edit of `id`: the same bytes, or bytes the app
    /// wrote since (a newer History entry holds them) with `id` still in
    /// the file as it was (`present`). Anything else is a change the
    /// command was not shown.
    pub fn still(&self, now: &Standing, id: &str, present: bool) -> bool {
        if now.file.problem.is_some() || now.file.has(id) != present {
            return false;
        }
        if now.file.sha256 == self.file.sha256 {
            return true;
        }
        now.last.as_ref().is_some_and(|last| {
            last.seq > self.seq() && Some(last.after_sha256.as_str()) == now.file.sha256.as_deref()
        })
    }
}

/// The words of an outside change of the user file.
pub fn outside_words(places: &Places, did_not: &str) -> String {
    format!(
        "{USER_ACCOUNTS_FILE} ({}) changed on disk outside the app since the app last wrote it, so nothing is written over that change. {did_not}. Put the file back as the app last wrote it (undo your change), then run this command again; Reload, which takes such a change in, arrives with the app's UI",
        places.user.accounts_file().display()
    )
}

/// Whether a user store created now would be marked `recovered` (story
/// 2.13): this Mac's Accounts file or the `accounts migrate` backups already
/// exist. Only a store that is created is ever marked.
pub fn would_be_recovered(places: &Places) -> bool {
    places.user.accounts_file().exists() || places.user.accounts_migrate_backups_root().exists()
}

/// Whether the user store is marked `recovered`, or would be when the next
/// write creates it (read-only; story 2.13). A store from before the mark
/// reads as not marked.
pub fn recovered(places: &Places) -> Result<bool, String> {
    let reader = reader(places)?;
    if reader.is_created() {
        reader.is_recovered().map_err(|e| e.to_string())
    } else {
        Ok(would_be_recovered(places))
    }
}

/// `user.lock` held, the user store open under it, and the writer of the
/// user Accounts file. Dropping it releases the lock.
pub struct Locked {
    pub writer: am_config::ConfigWriter,
    pub history: Arc<UserStore>,
    /// What settling interrupted writes did, as printed.
    pub lines: Vec<String>,
    /// The History entry ids settling completed (story 2.14: their notes
    /// are not a change made since a command's sentence).
    pub settled: Vec<String>,
}

impl Locked {
    /// Takes `user.lock` (owner-only checks first; the app data dir is
    /// created 0700 on the first write on this Mac), opens and migrates the
    /// user store, and settles the writes a crash interrupted.
    pub fn take(places: &Places) -> Result<Locked, String> {
        owner_only(&places.user)?;
        crate::runtime::ensure_private_dir(places.user.dir()).map_err(|e| e.to_string())?;
        // Story 2.13 (AD-15 *Migrate*, AD-17 *Recovery*): a store created
        // while this Mac's Accounts file or the `accounts migrate` backups
        // already exist is marked `recovered` in its creating transaction.
        let guard =
            UserStore::locked_marking_new(&places.user, LOCK_WAIT, would_be_recovered(places))
                .map_err(|e| e.to_string())?;
        owner_only(&places.user)?;
        let history = Arc::new(guard);
        let writer = am_config::ConfigWriter::for_user_accounts(places.user.dir(), history.clone())
            .map_err(|e| e.to_string())?;
        let settled = writer
            .reconcile(am_domain::Actor::System {
                component: "config-writer".into(),
            })
            .map_err(|e| e.to_string())?;
        let mut lines = Vec::new();
        for entry in &settled.completed {
            lines.push(format!(
                "completed the interrupted write of {} (History entry {})",
                entry.file, entry.id
            ));
        }
        for dropped in &settled.dropped {
            lines.push(format!("dropped the unwritten config change {dropped}"));
        }
        // Tests only: stop this writer's writes as a crash would.
        #[cfg(feature = "test-support")]
        {
            let dir = places
                .user
                .dir()
                .canonicalize()
                .unwrap_or_else(|_| places.user.dir().to_path_buf());
            writer.fail_at(crate::runtime::testing::writer_fail_point(&dir));
        }
        Ok(Locked {
            writer,
            history,
            lines,
            settled: settled.completed.iter().map(|e| e.id.clone()).collect(),
        })
    }

    /// The file and its records, read again under the lock.
    pub fn standing(&self, places: &Places) -> Result<Standing, String> {
        Standing::under(places, self.history.as_ref())
    }
}

// ---------------------------------------------------------------- the scan

/// `text` as the terminal may show it: every control character escaped
/// (`\u{1b}`, `\r`, …), and every bidirectional override or isolate,
/// zero-width or invisible format character, line or paragraph separator
/// and byte order mark too, so no name or path can move the cursor, erase
/// a line, reorder what is shown or hide where a key goes.
pub fn shown(text: &str) -> String {
    let hidden = |c: char| {
        c.is_control()
            || matches!(
                c,
                '\u{200B}'..='\u{200F}'
                    | '\u{202A}'..='\u{202E}'
                    | '\u{2060}'..='\u{206F}'
                    | '\u{2028}'
                    | '\u{2029}'
                    | '\u{FEFF}'
            )
    };
    text.chars()
        .map(|c| {
            if hidden(c) {
                c.escape_default().to_string()
            } else {
                c.to_string()
            }
        })
        .collect()
}

/// What the scan of this Mac's projects found for one Account id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scan {
    /// Each place that names the id, with its project's root.
    pub uses: Vec<(PathBuf, AccountUse)>,
    /// Recorded projects whose root could not be reached (not checked).
    pub unreachable: Vec<PathBuf>,
    /// Projects that enable a child that runs as the owner, with why (a
    /// stdio MCP server, a Source with `render: true`, a file with an error,
    /// or, since story 2.14, an MCP server over HTTP on this Mac).
    pub children: Vec<(PathBuf, String)>,
}

impl Scan {
    /// The references in words: `<file> (<what>) in <project>`, joined.
    pub fn uses_words(&self) -> String {
        if self.uses.is_empty() {
            return "nothing".to_string();
        }
        let all: Vec<String> = self
            .uses
            .iter()
            .map(|(root, u)| shown(&format!("{u} in {}", root.display())))
            .collect();
        all.join(", ")
    }

    /// The unreachable projects in words, joined.
    pub fn unreachable_words(&self) -> String {
        let all: Vec<String> = self
            .unreachable
            .iter()
            .map(|p| shown(&p.display().to_string()))
            .collect();
        all.join(", ")
    }
}

/// Every project of the runtime base's root records whose root is
/// reachable, scanned read-only through am-config for the places that name
/// `id` and for children that run as the owner; the others are listed as
/// not checked.
pub fn scan(places: &Places, id: &str) -> Scan {
    let mut out = Scan::default();
    let mut roots: Vec<PathBuf> = crate::runtime::records(&places.base)
        .into_iter()
        .map(|r| r.root.path)
        .collect();
    roots.sort();
    roots.dedup();
    for root in roots {
        if !reachable(&root) {
            out.unreachable.push(root);
            continue;
        }
        for u in am_config::account_uses(&root, id) {
            out.uses.push((root.clone(), u));
        }
        // The Account commands' rule: stdio and `render: true` children and,
        // since story 2.14, loopback-HTTP MCP servers.
        if let Some(why) = am_config::owner_children(&root).account_words() {
            out.children.push((root.clone(), why));
        }
    }
    out
}

fn reachable(root: &Path) -> bool {
    root.is_dir() && root.join("company.yaml").is_file()
}
