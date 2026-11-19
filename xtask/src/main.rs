//! Repo automation. Story 1.2 adds `check-deps`; later stories add `gen-types`
//! and `bundle-services`. The spike ships only this placeholder.
#![forbid(unsafe_code)]

fn main() {
    let task = std::env::args().nth(1).unwrap_or_default();
    eprintln!("xtask: `{task}` is not implemented yet (story 1.2 adds check-deps)");
    std::process::exit(2);
}
