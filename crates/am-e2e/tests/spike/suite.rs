//! The spike suite: test names read `spike::cN_...`.

#[path = "c1_providers.rs"]
pub mod c1_providers;
#[path = "c2_crash_resume.rs"]
pub mod c2_crash_resume;
#[path = "c3_scheduler.rs"]
pub mod c3_scheduler;
#[path = "c4_files_to_models.rs"]
pub mod c4_files_to_models;
#[path = "c5_mcp.rs"]
pub mod c5_mcp;
#[path = "c6_server.rs"]
pub mod c6_server;
#[path = "c7_keychain.rs"]
pub mod c7_keychain;
#[path = "live.rs"]
pub mod live;
