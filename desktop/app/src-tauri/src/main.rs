#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! AI status panel. Started by Claude Code with `--hook`, it only records the hook event;
//! otherwise it runs the desktop app.
mod app;
mod commands;
mod device_server;
mod discovery;
mod net;
mod provision;
mod runtime;

fn main() {
    if std::env::args().any(|arg| arg == "--hook") {
        panel_core::hook::run();
        return;
    }
    app::run();
}
