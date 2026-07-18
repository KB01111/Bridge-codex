#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> anyhow::Result<()> {
    let arg0_guard = codex_arg0::arg0_dispatch();
    let mut arg0_paths = arg0_guard
        .as_ref()
        .map(|guard| guard.paths().clone())
        .unwrap_or_default();
    if arg0_paths.codex_self_exe.is_none() {
        arg0_paths.codex_self_exe = std::env::current_exe().ok();
    }
    codex_desktop::run(arg0_paths)
}
