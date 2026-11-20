# sha2 0.11.1

Pinned `=0.11.1` (RustCrypto, MIT OR Apache-2.0), hex output through
`hex =0.4.3`.

## Where it is used

- `am-store`: the hash chain of the audit log (AD-25).
- `am-tools`: file hashes for folder access (AD-30).
- `am-config` (story 1.4): the `ConfigSnapshot` content hash (AD-16), a
  sha256 over the project's config files sorted by project-relative path,
  each pair framed by its lengths: `len(path) as u64 LE`, path bytes,
  `len(bytes) as u64 LE`, bytes. The same files give the same hash wherever
  the folder is; any changed byte, added or removed file changes it.

## What we use

- `Sha256::new()`, `update(&[u8])`, `finalize()`; `Sha256::digest(bytes)` for
  one-shot hashes. 0.11 moved to `digest` 0.11; `finalize()` returns an
  array type that `hex::encode` takes directly.

## Do not

- Do not let `sha2` types into `am-domain` or a port signature; pass the hex
  string or the 32 bytes.
