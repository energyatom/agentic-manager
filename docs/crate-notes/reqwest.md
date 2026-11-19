# reqwest 0.13.5

Pinned `=0.13.5`, `default-features = false`, features `rustls`, `stream`.
Used only in `am-net` (AD-12); clippy's `disallowed-methods` (clippy.toml)
bans `reqwest::Client::new`, `Client::builder`, `ClientBuilder::new` and
`reqwest::get` everywhere else.

## What we use

- `Client::builder().redirect(Policy::none()).no_proxy().connect_timeout(..)
  .resolve(host, checked_addr).build()`: one client per request, pinned to the
  address the policy checked (DNS pinning; private, loopback, link-local,
  CGNAT and metadata addresses refused before connecting).
- `resp.chunk().await` in a pump task feeding a bounded channel; dropping the
  receiver ends the task and drops the connection (cancellation).
- `e.without_url()` before turning an error into text (no URLs with query
  strings in logs).

## Notes

- In 0.13 `rustls` is the default TLS and uses aws-lc-rs (`__rustls-aws-lc-rs`),
  which builds C code with the system compiler; no cmake was needed on macOS.
- `system-proxy` is a default feature; we leave it off.
