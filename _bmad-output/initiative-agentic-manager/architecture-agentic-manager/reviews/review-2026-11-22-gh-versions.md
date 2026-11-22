# Review 2026-11-22: skill import from a GitHub URL (AD-12 App requests, AD-38 From GitHub), versions and reality-check lens

- **Spine:** `architecture-agentic-manager.md`, working-tree change on branch `replan/2026-11-22-skill-github` (AD-12 Binds, Prevents, the new **App requests** passage and the no-telemetry sentence; AD-15's Human-command list; AD-38 Binds, Prevents, "left out and named" and the new **From GitHub** passage; Structural Seed, the diagram label, the R60 row, two Deferred rows).
- **Lens:** every committed factual claim checked today against the live service, the vendor's documentation or the existing project. Nothing asserted from memory.
- **Date of every check:** 2026-11-22 (03:03 to 03:12 UTC), from one unauthenticated IP, user agent `arch-check`.
- **Reviewer stance:** read-only; nothing in the spine, tickets, code, AGENTS.md or UX files was edited. Scratch work (two small archive downloads, a throwaway crate that calls the built `am-config` writer on a copy of `fixtures/projects/valid`) lived only in the session scratchpad.
- **Tools:** `curl` for the GitHub API and codeload; gstack's headless browser for documentation pages; the built `agentic` binary (built 2026-11-22 05:16 local, after HEAD `1e6b847`) and a scratch crate for the project checks.

## Verdict

**Approve with changes.** The core of the amendment holds on the live service: two hosts are enough, the SHA lookup and the archive redirect behave as written, the redirect chain never left the two hosts and never exceeded two hops, neither host serves a robots.txt, and the built loader and writer take the two `metadata` keys with no code change. Six sentences need a correction or an added fact before the story builds (C1 to C6 below); two of them would produce a wrong result for a real repository (a ref whose last segment is an API sub-route word; a source URL the secret check refuses).

| # | Claim | Verdict |
|---|---|---|
| 1 | Two hosts, request shapes, redirects | Confirmed, with two corrections (C1 ref encoding, C2 renamed-repository redirect shapes and the request count) |
| 2 | User agent, 60 an hour, rate-limit answer | Confirmed; one wording correction (C3) |
| 3 | No robots.txt on either host | Confirmed |
| 4 | 404 for missing or private, 422 for unknown ref | Confirmed (private: by the vendor's documentation only) |
| 5 | Owner and repository name rules | Confirmed from documentation; rules stated below for the builder |
| 6 | Agent Skills format (`metadata`, 64, 1,024) | Confirmed; the spine's field list is out of date by one field (C4) |
| 7 | The archive is a plain zip; no byte-stable promise | Confirmed (not run against the `zip` crate itself, see below) |
| 8 | Built loader and writer take the two keys unchanged; `am-net` and `am-domain` | Confirmed in general; wrong at one edge (C5); five facts about `am-net` the builder must know (C6) |

---

## Claim 1. The host set, the request shapes and the redirects

**What I ran** (each `curl -sS -A "arch-check"`, no credentials).

| Request | Answer |
|---|---|
| `GET api.github.com/repos/obra/superpowers/commits/HEAD`, `Accept: application/vnd.github.sha` | `200`, `content-type: application/vnd.github.sha; charset=utf-8`, `content-length: 40`, body `22f0b463e7a5e579904dd6211a520321035526f7` (no newline), `x-github-media-type: github.v3; param=sha` |
| same, `/commits/main` (branch) | `200`, same 40 hex |
| `torvalds/linux` `/commits/HEAD` (default branch is `master`) | `200`, 40 hex |
| `torvalds/linux` `/commits/v6.6` (tag) | `200`, `ffc253263a1375a65fa6c9f62a893e9767fbebfa` |
| `microsoft/vscode` `/commits/release/1.95` (ref with `/`, raw) | `200`, `f1a4fb101478ce6ec82fe9627c43efbf9e98c813` |
| `microsoft/vscode` `/commits/release%2F1.95` (ref with `/`, encoded) | `200`, same SHA |
| `microsoft/vscode` `/commits/release` (a prefix of that ref) | `422`, `{"message":"No commit found for SHA: release", …}` |
| `obra/superpowers` `/commits/<full 40-hex SHA>` | `200`, the same SHA |
| `obra/superpowers` `/commits/main/status` (raw slash, last segment is a sub-route word) | **`200`, `content-type: application/json`, 4,361 bytes**: the combined-status endpoint answered, not the commit lookup |
| `obra/superpowers` `/commits/main%2Fstatus` | `422`, `No commit found for SHA: main/status` (routed to the commit lookup, as wanted) |
| `GET api.github.com/repos/obra/superpowers/zipball/<sha>` | `302`, `content-length: 0`, `location: https://codeload.github.com/obra/superpowers/legacy.zip/<sha>` (no query string) |
| `torvalds/linux` `/zipball/<sha>` | `302` to `codeload.github.com/torvalds/linux/legacy.zip/<sha>` |
| `GET codeload.github.com/obra/superpowers/legacy.zip/<sha>` | `200`, `content-type: application/zip`, `content-disposition: attachment; filename=obra-superpowers-v7.0.2-0-g22f0b46.zip`, an `etag`, **no `content-length`**, no `location` |
| Renamed repository: `facebook/jest` `/commits/HEAD` | `301`, `location: https://api.github.com/repositories/15062869/commits/HEAD`; that URL then `200` with 40 hex |
| Renamed repository: `facebook/jest` `/zipball/<sha>` | `301` to `https://api.github.com/repositories/15062869/zipball/<sha>`, then `302` to `https://codeload.github.com/jestjs/jest/legacy.zip/<sha>`, then `200` |
| `codeload.github.com/facebook/jest/legacy.zip/<sha>` (old name, direct) | `200` (no redirect) |
| `zipball/0000…0000` (unknown SHA) | `302` to codeload, then codeload `404` (`text/plain`, 14 bytes) |
| Full download `curl -L --max-redirs 3 …/zipball/<sha>` | `code=200 redirects=1`, 682,935 bytes |

**Documentation read.** `docs.github.com/en/rest/commits/commits` (the `application/vnd.github.sha` media type returns the commit's SHA-1; `ref` may be a SHA, a branch or a tag) and `docs.github.com/en/rest/repos/contents` (the zip endpoint returns a redirect; follow it or use `Location`; for private repositories the link is temporary, five minutes).

**Findings.**

- The host set is exactly `api.github.com` and `codeload.github.com` in every case tried: default branch not `main`, a tag, a ref with `/`, a full SHA, a renamed repository. No step pointed at `github.com`, `objects.githubusercontent.com`, `raw.githubusercontent.com` or a regional host (the answers carry `x-github-edge-region: centralindia` as a header only; the host does not change). The codeload answer itself did not redirect. **Confirmed.**
- Longest chain seen: **2 redirects** (renamed repository: 301 then 302). Three is enough. **Confirmed.**
- `HEAD` resolves the default branch. **Confirmed.**
- The answer is exactly 40 hex characters with no trailing newline. **Confirmed.**
- **C1 (wrong as written).** "percent-encoded per path segment" lets a builder keep the slashes of a ref raw. Raw works for `release/1.95`, but a ref whose last segment is one of the commit sub-routes (`status`, `statuses`, `comments`, `pulls`, `check-runs`, `check-suites`, `branches-where-head`) is answered by a different endpoint with a JSON body (shown above for `main/status`). The 40-hex check then fails and a real branch such as `feature/status` looks missing. The same collision can happen during the "shortest ref first" trials, because a path segment is sent as part of a candidate ref. Encoding the whole ref as one segment (`/` as `%2F`) routes correctly in both tests.
  **Corrected sentence:** "Owner and repository are checked against GitHub's name rules; the ref is sent as one percent-encoded path segment, its `/` as `%2F`, so it can never select another API route; `<path>` is never part of a request except as the tail of a candidate ref."
- **C2 (missing fact).** A renamed or transferred repository answers both API calls with `301` to `https://api.github.com/repositories/<numeric id>/…`, a path shape the passage does not name. If "the same rules" is read as "the same two request shapes", every renamed repository fails. The codeload URL shape is `/<owner>/<repo>/legacy.zip/<sha>` with the repository's current name.
  **Corrected sentence:** "A redirect target is accepted only when it is `https://api.github.com/repositories/<digits>/commits/<ref>`, `https://api.github.com/repositories/<digits>/zipball/<sha>` or `https://codeload.github.com/<owner>/<repo>/legacy.zip/<sha>` (checked on the live service 2026-11-22); anything else is refused."
- **C2b (arithmetic).** With "at most 8 requests, redirects included": a normal repository costs N lookups (N = segments tried as the ref) + 2, so N ≤ 6. A renamed repository costs 2N + 3 if each call is redirected again, so N ≤ 2: a three-segment ref (for example a `dependabot/<eco>/<name>` branch) on a renamed repository needs 9 and is refused.
  **Corrected sentence:** "After the first `301` the app sends the remaining requests of that import to the `/repositories/<id>` form it was given, so a renamed repository costs one extra request, not one per call."
- Not checked: an empty repository (GitHub's documentation says the archive redirect ends in 404), and a ref that is both a tag and a branch prefix.

---

## Claim 2. User agent, the 60-an-hour limit, the rate-limited answer

**What I ran.**

- `commits/HEAD` with the `User-Agent` header removed: `403`, `content-type: text/html`, body says the request was forbidden by administrative rules and asks for a User-Agent header. It carries **no** `x-ratelimit-*` headers. `zipball/<sha>` with no user agent: `403` too. codeload with no user agent: `200` (codeload does not require one).
- `GET /rate_limit` before: `core {limit: 60, remaining: 60}`. After 12 commit lookups (200, 422, 404 and the 301 all counted, one each) and 5 `zipball` calls: `core used: 13` (the 13th was one more lookup). Every `zipball` answer carried `x-ratelimit-limit: 60`, `x-ratelimit-remaining: 60`, `x-ratelimit-used: 0` with a reset one hour ahead of that request. codeload answers carry no rate-limit headers.
- `GET api.github.com/robots.txt` (404) **did** count (`used: 14`).
- The counter also moved for reasons outside my calls (46 remaining, then 37 two calls later): the limit is per IP, shared with everything else behind it. GitHub's documentation also says the header values can differ between responses because several regions answer.
- I did not exhaust the limit to see a real rate-limited answer.

**Documentation read.** `docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api` and `…/troubleshooting-the-rest-api`: unauthenticated requests are tied to the originating IP and limited to 60 an hour; past the primary limit the answer is 403 or 429 with `x-ratelimit-remaining: 0`, and the client should wait until `x-ratelimit-reset` (UTC epoch seconds); past a secondary limit the answer is 403 or 429 with a message, and `retry-after` (seconds) when present; with neither, wait at least a minute. The one quoted line: "Requests without a valid User-Agent header will be rejected."

**Findings.**

- A request without a user agent is refused (403). **Confirmed.**
- 60 an hour per IP, unauthenticated. **Confirmed** (documentation and `x-ratelimit-limit: 60`).
- Commit lookups count, including the 422, 404 and 301 answers. The `zipball` call and the codeload download did **not** count against the core limit in this run. That is observed behaviour, not documented: the builder must not rely on it. **Confirmed as observed.**
- Whether codeload has a limit of its own: **not confirmable** (no headers, no documentation found).
- **C3 (incomplete).** "a rate-limit answer (403 or 429 with GitHub's rate-limit headers) is reported in words with the reset time": a secondary limit may carry `retry-after` and no usable reset, and a 403 without `x-ratelimit-remaining: 0` or `retry-after` is not a rate limit at all (the missing-user-agent 403 above is HTML with no such header).
  **Corrected sentence:** "A 403 or 429 is a rate-limit answer only when it carries `x-ratelimit-remaining: 0` or `retry-after`; it is reported in words with the time from `x-ratelimit-reset` (UTC epoch seconds) or, when only `retry-after` is present, that many seconds; any other 403 is reported as refused."
- Also true and worth one clause: without an `X-GitHub-Api-Version` header the answers carry `x-github-api-version-selected: 2022-11-28`, while the documentation's examples now show `2026-03-10`. The two request shapes worked under the default; the spine names no version, which is fine, but the builder should not send one it has not tested.

---

## Claim 3. Neither host serves a robots.txt

- `GET https://api.github.com/robots.txt`: `404`, `content-type: application/json`, counted against the rate limit.
- `GET https://codeload.github.com/robots.txt`: `404`, `text/plain`, 14 bytes.

**Confirmed.** Because the API one costs a request from the 60, it is one more reason these calls should not pass through `Fetcher`, as the passage already says.

---

## Claim 4. Missing or private repository; unknown ref

- Missing repository, `commits/HEAD`: `404`, `{"message":"Not Found", …}`. Missing repository, `zipball/<sha>`: `404` JSON (no redirect). codeload for a missing repository: `404` `text/plain`.
- Unknown ref on an existing repository: `422`, `{"message":"No commit found for SHA: no-such-ref-zzz", … "status":"422"}`.
- Unknown SHA on `zipball`: the API still answers `302`; the `404` comes from codeload.
- Private repository, unauthenticated: not tested live (I will not guess a private repository's name). GitHub's troubleshooting page says a private resource without proper authentication answers 404, so that its existence is not confirmed. **Confirmed by documentation.**

**Confirmed.** For the builder: `422` on a lookup means "this candidate is not a ref, try the next longer one"; `404` means "no such public repository" and the trials stop; the words for a 404 must say "missing or private", since the two cannot be told apart.

---

## Claim 5. GitHub's owner and repository name rules

**Documentation read.**

- Owner (user or organisation): GitHub's username page (`docs.github.com/en/enterprise-cloud@latest/admin/managing-iam/iam-configuration-reference/username-considerations-for-external-authentication`) says usernames hold only alphanumeric characters and dashes, at most 39 characters; a normalised name cannot start or end with a dash or hold two dashes in a row. Managed-enterprise users end in `_<shortcode>`, but those accounts cannot own public repositories.
- Repository: the "Creating a new repository" page says the name must not exceed 100 characters and can hold only ASCII letters, digits, `.`, `-` and `_`. The REST reference says the `repo` parameter is the name without the `.git` extension, and that owner and repository names are not case sensitive.

**Rules for the builder.**

| Part | Rule |
|---|---|
| owner | 1 to 39 characters from `[A-Za-z0-9-]`. Do not enforce the dash placement rules: they are documented for new and normalised names, and whether older accounts break them is not confirmable today; let the 404 decide. |
| repository | 1 to 100 characters from `[A-Za-z0-9._-]`; refuse `.` and `..`; drop one trailing `.git`. |
| both | compared case-insensitively by GitHub; keep the owner's spelling in the source record. |
| ref | GitHub has no rule of its own: it is git's. Checked locally with `git check-ref-format` (git 2.54.0): refused are a space, `~ ^ : ? * [ \`, `..`, `@{`, a leading or trailing `/`, `//`, a segment starting with `.`, a segment ending `.lock`, a trailing `.`; accepted are `%`, `#`, non-ASCII letters and a leading `-`. So a ref can hold characters that must be percent-encoded (`%` as `%25`, `#` as `%23`, `/` as `%2F`, non-ASCII as UTF-8 bytes). |
| path | any bytes git allows in a file name; it is never sent in a request (see C1). |

**Confirmed** from documentation; the ref row is new information the spine's sentence does not carry ("GitHub's name rules" covers owner and repository only).

---

## Claim 6. The Agent Skills open format

**Read:** `https://agentskills.io/specification` as published today.

- `metadata`: optional; a map from string keys to string values; meant for properties the specification does not define; the page recommends reasonably unique key names. The `agentic-` prefix fits. **Confirmed.**
- `name`: 1 to 64 characters, lowercase letters, digits and hyphens, no leading, trailing or double hyphen, must match the parent directory name. `description`: 1 to 1,024 characters. The spine's cut to 64 and 1,024 is right. **Confirmed.**
- **C4 (out of date).** The specification's frontmatter table has **six** fields today: `name`, `description`, `license`, `compatibility` (optional, at most 500 characters), `metadata`, `allowed-tools` (a space-separated string, marked experimental). AD-38 lists five and the built loader's allow-list is the same five (`crates/am-config/src/kinds.rs`, `SKILL_KEYS`). I ran the built binary on a copy of the valid fixture with `compatibility: Requires git and internet access` added: `agentic validate` answers `unknown key 'compatibility' in SKILL.md` as an **error**. The amendment's "left out and named" sentence lists files and folders only, so the builder has no rule for a frontmatter key the loader does not accept, and a public skill that uses a published field cannot be imported.
  **Corrected sentence (owner's choice of behaviour, the fact is fixed):** "A frontmatter key outside the five the loader accepts (the open format's `compatibility`, or any client's own key) is left out of the imported SKILL.md and named in the summary and the diff, like a file that is left out."
  The alternative, accepting `compatibility` in the loader, contradicts "the loader and `agentic validate` are unchanged" and is a code change in a built crate.
- The specification's optional directories are `scripts/`, `references/` and `assets/`, plus any other file. The first archive I listed (15 skills) holds `scripts/` in 3 skill folders, `prompts/` in 1 and `templates/` in 1: "left out and named" will fire on ordinary public skills, as the amendment intends.

---

## Claim 7. The archive

**What I ran.** Downloaded two small public archives into two new empty scratch folders and listed them with `zipinfo`, `unzip -tq` and a read-only Python `zipfile` listing (nothing extracted to disk, nothing run).

| Fact | `obra/superpowers` at `22f0b46…` | `github/gitignore` at `eeb775f…` |
|---|---|---|
| Size, entries | 682,935 bytes, 309 entries, 1,600,920 bytes expanded | 151,425 bytes, 343 entries |
| Top level | one folder, `obra-superpowers-22f0b46/` | one folder, `github-gitignore-eeb775f/` |
| Archive comment | the full 40-hex commit SHA | the full 40-hex commit SHA |
| Methods | stored and deflate only | stored and deflate only |
| General-purpose flags | `0x0` on every entry (no encryption, no data descriptor) | `0x0` |
| Version needed | 1.0 on every entry | same |
| zip64 | no zip64 end-of-central-directory locator | none |
| Extra fields | only `0x5455` (timestamp) | same |
| Symlinks | none in this repository | 3, each an entry made on Unix with mode `0120777`, stored, whose content is the link target (13 to 19 bytes) |
| Executable bit | 60 entries carry a Unix mode with an execute bit | n/a |
| Integrity | `unzip -tq`: no errors | no errors |

- The top folder's name is `<owner>-<repo>-<short sha>` with the repository's current name; the short SHA's length is not fixed. The builder strips the first path component and never predicts it.
- The archive comment equals the commit SHA in both downloads: a free cross-check against the pin (observed, not documented).
- codeload sends no `content-length`, so the 64 MB cap can only be enforced while the body streams. The built client already cuts a stream that way (`EgressError::TooLarge`).
- **Byte stability.** `docs.github.com/en/repositories/working-with-files/using-files/downloading-source-code-archives`, section on stability: an archive of a commit ID always has the same file contents, but the compression settings may change and the outer archive may have a different byte layout; GitHub promises six months' notice of such a change; the repository name is part of the root folder, so a rename changes it. Observed too: the codeload `etag` (`51059f3a…`) is not the sha256 of the bytes I received (`4ea66846…`). So the commit SHA is the pin, and the archive's sha256 in the audit entry is a record of what was received, not something a later download can be compared with. **Confirmed.** The spine already treats it that way; a clause saying the sha256 is not comparable across downloads would stop a future "verify" feature from being built on it.
- **The `zip` crate.** `zip` 9.6.1 exists on crates.io (MIT, not yanked, published 2026-11-16, needs Rust 1.88; the toolchain pin is 1.100.0; the newest is 9.8.1). Its documentation for 9.6.1 shows `ZipFile::is_symlink`, `unix_mode` and `enclosed_name`, and `deflate` among its features. Everything in these archives is the baseline of the format, so there is nothing a reader could lack. I did **not** run the crate against the archive: it is not yet a dependency of the workspace and I did not download it. **Confirmed by documentation and archive features; not run.** One fact for the story: 14 of the crate's 26 features are on by default (among them AES, bzip2, lzma, xz, zstd), so the dependency should be declared with `default-features = false` and `deflate` only, or the licence check and the hostile-input surface both grow.

---

## Claim 8. The existing project

### The two `metadata` keys

**Read:** `crates/am-config/src/kinds.rs` (`SKILL_KEYS`, `fn skill`), `fields.rs` (`text`), `checks.rs` (`is_secret_key`, `has_secret_prefix`), `skills.rs`, `entities.rs` (`Skill.metadata: BTreeMap<String, String>`), `canon.rs`, `writer.rs`, `tests/writer.rs`, `tests/skills.rs`.

**Ran (loader):** the built `agentic validate` on a scratch copy of `fixtures/projects/valid` with the skill's frontmatter changed.

| Frontmatter | Result |
|---|---|
| `agentic-source-url: https://github.com/obra/superpowers/tree/main/skills/brainstorming` and a 40-hex `agentic-source-commit` | 0 errors, 0 warnings |
| an all-digit 40-character commit, unquoted | 0 errors (read back as its text) |
| `0000…0000`, unquoted | 0 errors |
| a URL with a slash ref and `%20` | 0 errors |
| commit `1234567890e23456789012345678901234567890`, **unquoted** | error: `not valid YAML: non-finite float …` |
| URL `https://github.com/huggingface/hf_transfer/tree/main/skills/x` | **error: `secret-looking value in 'agentic-source-url'`** |
| URL with a segment `sk-learn-extension-toolkit-pack` | **the same error** |

**Ran (writer and canonical form):** a scratch crate outside the repository calling `ConfigWriter::apply` with two `EditOp::Set` on `["metadata", "agentic-source-url"]` and `["metadata", "agentic-source-commit"]`, then `canonical_bytes` and `load`.

| Case | Result |
|---|---|
| normal URL and SHA | applied; written plain under `metadata:` (keys sorted, commit before URL); canonical form idempotent; both values round-trip |
| all-digit SHA; `1234567890e2…` SHA; all-zero SHA | applied; the writer **quotes** each one; idempotent; round-trips |
| URL holding ` #` and `: ` | applied; quoted; round-trips |
| `hf_transfer` URL | **refused by the writer:** "the change would leave the project invalid: … secret-looking value in 'agentic-source-url'" |

**Findings.**

- `metadata` is kept as free string-to-string keys; no key name is checked except by the secret rule (exact `token`, `secret`, `password`, `api_key`, `private_key` or those as a `_` suffix), which neither `agentic-source-url` nor `agentic-source-commit` matches. A 40-hex value can never match a secret prefix (the prefixes need `s`, `g`, `h`, `x`, `A` or `-`/`_`). **Confirmed: no code change for the keys.**
- Through the writer the values are safe in every number-like case, because the emitter quotes them. A SHA written by any other route must be written quoted: the one unquoted failure above (a SHA of digits with a single `e`) is rare (roughly 3 in 10^8) but costs nothing to rule out.
- **C5 (wrong at one edge).** "The skill format, the loader and `agentic validate` are unchanged: these are two more `metadata` strings" is true, and that is exactly why a source URL can be refused: the secret check runs on every frontmatter value, and a URL whose owner, repository, ref or path has a segment starting `hf_` plus 8 name characters, `sk-` plus 20, `ghp_`, `github_pat_`, `xox?-`, `AKIA…` or `AIza…` is an error in both the loader and the writer. `huggingface/hf_transfer` is a real public repository that trips it. The amendment does not say what the import does then, and weakening the secret check is not the reviewer's to propose.
  **Corrected sentence:** "The skill format, the loader and `agentic validate` are unchanged: these are two more `metadata` strings, so they pass the same secret check as every value; when the source URL trips it (a repository or path segment shaped like a key, for example `hf_transfer`), the import is refused in words that name the check, before any Proposal is made." (Or the owner picks another outcome; the built behaviour is the refusal.)

### `am-net` and `am-domain`

**Read:** `crates/am-net/src/lib.rs`, `crates/am-domain/src/net.rs`; grep for user agent, `Caller` and redirect across `crates/`.

Nothing built contradicts the amended AD-12 in a way that breaks an existing story, and the app-purpose enum is future work as stated. These are the facts the builder inherits (**C6**: none is in the amendment, each changes what the story must build):

1. **Redirects are not followed today.** The one reqwest client is built with `redirect::Policy::none()` and the module header says so. AD-12's long-standing "Redirects (at most 3) are re-checked" has no code behind it yet. The import either follows by hand (each hop a new checked request, which gives the re-check for free and makes the 8-request count easy) or `am-net` gains redirect following in this story.
2. **No user agent is set anywhere.** No `User-Agent` string exists in `crates/`; reqwest sends none by default; GitHub answers 403 without one (claim 2). The "product user agent (AD-39)" does not exist yet as a constant.
3. **One body cap per client.** `EgressPolicy.max_body_bytes` is a single value (default 64 MB) for the whole `NetEgress`. "64 KB for a lookup answer, 64 MB for the archive" needs a per-request cap or two policies.
4. **`Caller` is two `Option`s and `HttpRequest::get` builds `Caller::default()`.** A request with neither a seat nor an Account is already constructible by any crate that holds the port (the MCP HTTP client does it), and `EgressPolicy.allowed_hosts` is one flat list, not per seat, Account or purpose. "No tool manifest, ToolGate path or engine run can build a request with an app purpose" therefore needs the purpose to be something only the import's command path can construct, plus the fixed host constant checked inside `am-net`; the type as built does not give that.
5. **Audit and port.** `am-net` audits only blocked requests, as `Actor::System { component: "am-net" }` with action `egress.blocked`. The amendment wants every app request, allowed or blocked, as an entry by the Human actor: a blocked app request would get both entries unless the story says which one stands. `check_url` requires https for a remote host but does not pin the port to 443; "to port 443" is new.

What does hold as written: https only, no credentials in the URL, the blocked-address list, DNS pinning to the checked address (`.resolve(host, addr)`), a streamed and capped body, and `HttpMethod::Get`.

---

## Corrections, in one place

| Id | Where | Change |
|---|---|---|
| C1 | AD-12 *How* | Send the ref as one percent-encoded segment (`/` as `%2F`); say `<path>` is never in a request. |
| C2 | AD-12 *How* | Name the three redirect target shapes (`/repositories/<id>/commits/<ref>`, `/repositories/<id>/zipball/<sha>`, codeload `/<owner>/<repo>/legacy.zip/<sha>`); reuse the `/repositories/<id>` form after the first 301 so the 8-request cap holds. |
| C3 | AD-12 *How* | A rate-limit answer is a 403 or 429 carrying `x-ratelimit-remaining: 0` or `retry-after`; report the reset time or the retry seconds. |
| C4 | AD-38 Rule | The open format has a sixth field, `compatibility`; say what the import does with a frontmatter key the loader does not accept. |
| C5 | AD-38 *Source record* | Say what happens when the source URL trips the secret check (built behaviour: validate error, writer refusal). |
| C6 | AD-12 (or the story) | Record the five `am-net` facts: no redirect following, no user agent, one body cap, an unguarded `Caller`, blocked-only audit and no port pin. |

Smaller notes, no change required: the archive's sha256 is not comparable across downloads; `zip` should be declared without default features; the zipball call not counting against the 60 is observed, not promised; a private repository cannot be told from a missing one.

## Not done

- No rate-limited answer was provoked; its shape is from the documentation.
- No private repository and no empty repository was requested.
- The `zip` crate was not run.
- gstack's start-up asked two one-time questions (an upgrade from 1.99.5.0 to 1.102.2.0 and a telemetry choice). A reviewer sub-agent cannot ask the owner, so neither was acted on; both will be asked again on the next run.
