# croner 4.0.3

Pinned `=4.0.3`, `default-features = false`, feature `jiff`. Used only in
`am-engine/src/scheduler.rs`.

## Finding (spike, 2026-11-19): croner's jiff backend is jiff 0.2

croner 4.0.3 depends on `jiff = "0.2.35"`; the workspace pins jiff `=1.0.0`.
The two are different crates to the compiler, so croner's `CronDateTime`
impls are for `jiff 0.2`'s `Zoned` and `civil::DateTime` only. The scheduler
therefore also depends on `jiff-croner = { package = "jiff", version =
"=0.2.46" }` and converts at one boundary, `scheduler::next_after`: the
instant crosses as Unix milliseconds and the zone as its IANA name. Nothing
else may use `jiff-croner`. Revisit when croner publishes a jiff 1.x backend.

## What we use

- `Cron::from_str("30 9 * * *")` (5-part POSIX; `L`, `#`, `W` supported).
- `cron.find_next_occurrence(&zoned, false)`: the first occurrence strictly
  after the given zoned time, evaluated in that zone (DST follows croner's
  documented semantics, AD-18).

## Do not

- Do not let `croner::Cron` or `jiff-croner` types into `am-domain` or a port.
- Do not enable the default `chrono` feature.
