//! The desktop shell (AD-32): links the core in-process (no sidecar), starts
//! the loopback server and opens one window on it with a single-use launch
//! code. The webview reaches the core only over `/api/v1` and the WebSocket,
//! exactly like browser mode (AD-14); Tauri IPC is not used by the page, and
//! the window gets no capabilities.
//!
//! Spike: the criterion 8 stand-in (ad-hoc signed). The signed and notarised
//! build is the gate at the start of epic 11 story 3.
#![forbid(unsafe_code)]

#[cfg(target_os = "macos")]
fn main() {
    use tauri::{WebviewUrl, WebviewWindowBuilder};

    let (listener, port) =
        tauri::async_runtime::block_on(am_server::bind(0)).expect("bind 127.0.0.1");
    let state = am_server::AppState::new(port);
    let server_state = state.clone();
    tauri::async_runtime::spawn(async move {
        let _ = am_server::serve(listener, server_state, std::future::pending()).await;
    });
    println!("agentic-desktop: serving on http://127.0.0.1:{port}");

    tauri::Builder::default()
        .setup(move |app| {
            let url: tauri::Url = state.launch_url().parse()?;
            WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url))
                .title("Agentic Manager")
                .inner_size(1100.0, 760.0)
                .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("run the desktop shell");
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("agentic-desktop: the desktop app is not supported on this platform yet");
    std::process::exit(1);
}
