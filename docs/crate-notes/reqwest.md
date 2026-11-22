# reqwest 0.13.5

Pinned `=0.13.5`, `default-features = false`, features `rustls`, `stream`.
Used only in `am-net` (AD-12); clippy's `disallowed-methods` (clippy.toml)
bans `reqwest::Client::new`, `Client::builder`, `ClientBuilder::new` and
`reqwest::get` everywhere else.

## What we use

- `Client::builder().redirect(Policy::none()).no_proxy().connect_timeout(..)
  .resolve(host, checked_addr).build()`: one client per hop, pinned to the
  one address the client checked for that hop (DNS pinning: every answer of
  the one lookup is checked, and the connection goes to the first; a later
  answer cannot move it). This is am-net's `ReqwestTransport`, the only
  construction site.
- Redirects are never followed by reqwest (`Policy::none()`): am-net follows
  at most 3 itself (story 2.2), GET only, each re-checked by the caller's
  rule, re-resolved and pinned again with a new client; a credential is
  dropped for the rest of the chain once a hop changes scheme, host or port.
- `resp.chunk().await` in a pump task feeding a bounded channel; dropping the
  receiver ends the task and drops the connection (cancellation). The
  response cap is applied above the transport, so a scripted transport in
  tests is capped the same way.
- `e.without_url()` before turning an error into text (no URLs with query
  strings in logs).
- `reqwest::Url` (the `url` crate) parses every URL the client checks, so
  the host and port the check reads are the ones the connection uses
  (`port_or_known_default`, lowercase host, IPv6 brackets stripped).

## Notes

- In 0.13 `rustls` is the default TLS and uses aws-lc-rs (`__rustls-aws-lc-rs`),
  which builds C code with the system compiler; no cmake was needed on macOS.
- `system-proxy` is a default feature; we leave it off.
- clippy cannot ban `reqwest::Client::default()` or
  `reqwest::ClientBuilder::default()` through `disallowed-methods` (checked
  on the pinned toolchain, story 2.2): a path to a trait impl's method does
  not resolve (`does not refer to a reachable function`), and the call
  resolves to `core::default::Default::default`, which cannot be banned for
  one type only.
