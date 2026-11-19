//! Stack spike (E1 story 1): one module per criterion. Offline tests run by
//! default on fakes and recorded fixtures; live tests are `#[ignore]` and
//! skip, saying why, when their input is absent (`AM_LIVE=1`, the Anthropic
//! key in the Keychain, LM Studio, Node.js, `AM_SPIKE_DOCS_DIR`).
#![forbid(unsafe_code)]

#[path = "suite.rs"]
mod spike;
