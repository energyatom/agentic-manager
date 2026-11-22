# Review 2026-11-22: AD-15 *Key use* and keyless loopback Accounts (versions / reality check)

Scope: `git diff` of `architecture-agentic-manager.md` on branch `replan/2026-11-22-accounts`: AD-15 **Prevents** (key reaching an unconfirmed provider), *Record:* now naming the provider, new *Key use*, *Later confirmations*, *Keyless loopback Account*, and the last Open Questions bullet. Lens: is each committed decision reality-checked against the code story 2.3 built, the local servers' documented behaviour, and the Stack (no new crate or version).

## Verdict

**PASS WITH FINDINGS.** The *Record* and *Key use* text describes what story 2.3 actually built, check for check. The keyless claim holds for the four servers named. No crate or version is introduced. One medium gap (the `agentic spike` commands read and send a Keychain key with no confirmed check, contrary to "the rule for every use of a key now") and three low or nit items, all fixable by a sentence in the spine or a ticket line. None blocks the change.

## 1. Spine against the code on this branch

| Spine claim | Code | Match |
|---|---|---|
| `account_added` carries file, Account id, **provider**, scheme, host, port, references, `confirmation: typed_id`, never the key | `crates/am-app/src/accounts.rs:816-835` (`AuditNote` action `config.widened`, target id, args `kind, file, account, provider, scheme, host, port, references, confirmation`) | Yes |
| History entry and audit entry in one transaction | `accounts.rs:838-856` (`apply_with_audit` / `create` with `audit: vec![note]`) | Yes |
| File holds exactly the bytes of its **latest** History entry | `accounts.rs:1118-1129`: `file.sha256 == history.last().after_sha256`; `am-store/src/config_history.rs:397-403` orders by `seq` | Yes (by sha256 of the bytes read, the same read the endpoint is derived from) |
| **Latest** `account_added` for that id names the same provider, scheme, host, port | `accounts.rs:1133-1162` (`rfind` over `audit_args`, which is oldest first: `am-store/src/audit.rs:265-296`) | Yes |
| No entry: refused (hand-written, never added) | `accounts.rs:1137-1141` | Yes |
| Read-only reader, no project lock | `accounts.rs:1103-1110`: `find_runtime` (`am-app/src/runtime.rs:991`, "without a lock, a write"), `check_database` (read-only open, `am-store/src/lib.rs:562`), `Reader::open` (`SQLITE_OPEN_READ_ONLY`, `query_only`, `am-store/src/lib.rs:923-931`); never migrates (`accounts.rs:1111-1117`) | Yes |
| No terminal, any stdin | `test` (`accounts.rs:1171ff`) never calls `require_terminal` | Yes |
| Check runs before any key read or send | `confirmed` at `accounts.rs:1222`, Keychain `get` at `:1243`, egress at `:1258` | Yes |
| A run is a key use that must check | `am-app/src/run.rs:44-46, 95-101`: `unwired_providers` wires no provider, so no run reads a key yet | Consistent (the obligation is forward-looking and carried by the Open Questions bullet) |
| Keyless loopback built later, not story 3 | Today `add` always writes `secret_ref` (`am-config/src/account_edit.rs:253-264`), `add` always reads a key, `test` refuses an Account with no Keychain entry (`accounts.rs:1243-1250`); `OpenAiCompatProvider::new` has `auth: None` until `with_auth` (`am-providers/src/openai_compat.rs:49-60`) | Consistent: the spine attributes it to a later story, and the provider already sends no credential when no `AuthRef` is given |

No mismatch between the spine and story 2.3's code.

## 2. Keyless loopback claim against the servers' docs (checked 2026-11-22)

- **llama.cpp `llama-server`**: `--api-key` / `--api-key-file` default "none" (tools/server/README.md); host defaults to 127.0.0.1. With no key configured, no key is checked. Holds.
- **vLLM**: `vllm serve --api-key` defaults to None; "If provided, the server will require this key". The OpenAI-compatible server page adds that, when set, it guards only `/v1`, `/v2`, `/inference`, `/cohere`. Holds.
- **LM Studio**: "By default, LM Studio does not require authentication for API requests"; an auth toggle with API tokens exists in Server Settings. Holds.
- **Ollama `/v1`**: "The client requires an API key value, but Ollama ignores it." Holds (the OpenAI SDK needs a placeholder; this app's own client sends no header, which is fine).

Reality caveat (see F3): every one of these servers can be switched to require a key (llama-server `--api-key`, vLLM `--api-key`, LM Studio's auth toggle). The spine allows "may be added with no key", so a keyed loopback Account stays possible. That is correct, but the spine does not say it outright.

## 3. Versions

The change names no crate, no version and no new dependency. `nix` termios was already in the Stack before this change and is only referenced. The ticket diff (`tickets.toml`) adds a story with no dependency line. Nothing needs pinning.

## Findings

### F1 (medium): `agentic spike c1` / `c4` send a Keychain key with no confirmed check

The spine says the *Key use* check "is the rule for every use of a key now". The release binary still dispatches `agentic spike` (`crates/am-app/src/main.rs:89-95`). `c1` and `c4` build `AnthropicProvider::new(egress, SPIKE_ANTHROPIC_ACCOUNT)` (`main.rs:356-359`, `446-449`) over `default_egress`, which allows `api.anthropic.com` and reads keys from the shared `agentic-manager` Keychain service (`crates/am-app/src/lib.rs:36, 43-49`). The id `spike-anthropic` is not reserved (`reserved` covers only `local-lmstudio` and `local-ollama`).

Failure scenario: the owner adds an `openai` (or remote compatible) Account with id `spike-anthropic`. `agentic spike c1` then sends that key to `api.anthropic.com`, a provider the owner never confirmed for it, with no bytes check and no `account_added` check. This is low likelihood, but it contradicts both the quoted owner rule and the new **Prevents** clause.

Fix (pick one, in the spine or as a ticket line): (a) the *Key use* clause excludes the spike commands by name and requires that `agentic spike` leave the release binary before the first release (a guard test like the fake-terminal one); or (b) reserve the `spike-` id prefix in `add`'s reserved check. Option (a) is cleaner.

### F2 (low): "endpoint" in the record and check is scheme, host and port, not the base URL path

The record and the check compare provider, scheme, host and port (`accounts.rs:822-825`, `1145-1148`). An `openai-compatible` Account's `base_url` path is not recorded. On a path-routed gateway (one host whose path selects the tenant or the upstream provider), a path change sends the key somewhere the owner did not confirm while the check still passes.

Today the bytes check blocks any hand edit. But after epic 4 story 18 (Reload) or 5.5, a confirmation that "changes its provider or endpoint" would not be triggered by a path-only change if "endpoint" means host and port.

Fix: either record the normalised `base_url` (path included) in `account_added` and compare it, or state in *Key use* that "endpoint" means scheme, host and port and that a path change is not a re-confirmation. Recording it is cheap and matches the owner's words more closely.

### F3 (low): keyless rules leave two cases implicit

(a) The spine says how a `key: none` note interacts with a `secret_ref`. It does not say that a note with **no** `key` field (every story 2.3 note) means "keyed". Write that down, so the later story does not refuse or re-interpret existing Accounts.

(b) It is implicit that a loopback Account **with** a key stays allowed, which the owner needs for a local server with auth switched on (LM Studio's toggle, `--api-key`). One clause fixes it: "a loopback Account may also be added with a key, as today."

### F4 (nit): "Before any use of an Account's key … it reads no key" vs `list`

`agentic accounts list` reads the Keychain entry to show a masked key (`accounts.rs:1054`) without the confirmed check. Nothing is sent, so this is not a breach of "a key is sent only to …". But the literal "before any use … reads no key" covers it. Narrow the clause to "before a key is read to be sent" or name `list`'s masked read as exempt.

## Sources

- llama.cpp server README: https://raw.githubusercontent.com/ggml-org/llama.cpp/master/tools/server/README.md
- vLLM OpenAI-compatible server: https://docs.vllm.ai/en/latest/serving/online_serving/openai_compatible_server/ ; `vllm serve --api-key` default None (mirrored): https://docs.redhat.com/en/documentation/red_hat_ai_inference_server/3.2/html-single/vllm_server_arguments/index
- LM Studio authentication: https://lmstudio.ai/docs/developer/core/authentication
- Ollama OpenAI compatibility: https://docs.ollama.com/api/openai-compatibility
