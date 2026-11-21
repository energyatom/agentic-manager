# notify 8.2.2

Pinned `=8.2.2` with default features in `[workspace.dependencies]` (Stack),
linked from `am-config` for the config watcher (epic 1 story 9, AD-16); AD-39's
drop folders use it later.

## Licence

notify is CC0-1.0, which is not on the AD-3 allow-list. The owner allowed it
for this crate alone (2026-11-21, "Exception for notify"): `deny.toml` has one
`[[licenses.exceptions]]` entry, `crate = "notify@8.2.2"`, `allow =
["CC0-1.0"]`. The exception names the version, so a bump is reviewed again.
Its dependencies stay under the allow-list: notify-types, bitflags, libc and
log (MIT OR Apache-2.0); objc2-core-foundation and objc2-core-services on
macOS (Zlib OR Apache-2.0 OR MIT); walkdir and same-file (taken under MIT);
inotify, inotify-sys (ISC) and mio (MIT) on Linux. Any other CC0-1.0 crate
fails `cargo deny check licenses`
(`xtask/tests/supply_chain.rs`, `cc0_crate_other_than_notify_fails_the_licence_policy`).

## Backends

- macOS: FSEvents (`RecommendedWatcher`). Events are coalesced and arrive
  with some latency; paths are reported in their canonical form
  (`/private/var/...` for a folder under `/var/...`), so callers strip the
  canonicalised root. An event may carry the `need_rescan` flag.
- Linux: inotify (CI builds and runs the unit tests there).

## What we use

- `notify::recommended_watcher(handler)` with a closure handler, and
  `Watcher::watch(root, RecursiveMode::Recursive)` on the project folder.
- `Event::paths` and `Event::need_rescan()`. The event kind is not trusted
  to say what changed (editors save by temp file and rename), with one
  exception: `EventKind::Access(_)` events are ignored, because they change
  no content and inotify reports every open as one, the session's own reads
  included, which would otherwise wake the worker for ever. `am-config`'s
  session settles each
  path for 300 ms, then compares sha256 of the bytes on disk with the
  accepted bytes, the last announced hash and the writer's own writes.
- Dropping the watcher stops it and closes the handler's channel, which ends
  the session's worker thread.

## Do not

- Do not add a polling fallback or a second watcher (story 1.9 Never).
- Do not let notify types into `am-domain` or a port signature.
