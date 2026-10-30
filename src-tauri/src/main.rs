// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod claude;
mod claude_live;
mod codex;
mod model;
mod paths;
#[cfg(windows)]
mod proc;
mod settings;
mod statusline;
mod tokens;
mod window;

fn main() {
    // Claude Code runs `limi --statusline` for each status-line update.
    // Handle it before any GUI setup so it stays fast and never opens a window.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--statusline") {
        std::process::exit(statusline::run());
    }
    // Run by the uninstaller: restore the user's own Claude Code status line.
    if args.iter().any(|a| a == "--disconnect-claude") {
        std::process::exit(claude::disconnect_installed());
    }
    app::run();
}
