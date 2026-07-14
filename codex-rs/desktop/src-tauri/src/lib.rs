mod a2a;
mod a2a_handlers;
mod a2a_protocol;
mod a2a_task_store;
mod browser_agent;
mod browser_agent_commands;
mod cli_proxy;
mod cli_proxy_http;
mod cli_proxy_manager;
mod cli_proxy_process;
mod cli_proxy_request;
mod code_memory;
mod code_policy;
mod code_policy_checks;
mod desktop_agent;

use a2a::A2aServer;
use a2a_handlers::cancel_a2a_task;
use a2a_handlers::delegate_a2a_task;
use a2a_handlers::get_a2a_status;
use a2a_handlers::get_a2a_task;
use a2a_handlers::list_a2a_tasks;
use anyhow::Context;
use browser_agent::BrowserAgent;
use browser_agent_commands::browser_click_at;
use browser_agent_commands::browser_click_selector;
use browser_agent_commands::browser_navigate;
use browser_agent_commands::browser_start;
use browser_agent_commands::browser_status;
use browser_agent_commands::browser_stop;
use browser_agent_commands::browser_type;
use cli_proxy::fetch_active_models;
use cli_proxy::start_chat_completion;
use cli_proxy_manager::CliProxyManager;
use cli_proxy_manager::ensure_cliproxyapi;
use cli_proxy_manager::get_proxy_status;
use cli_proxy_manager::run_chatgpt_browser_login;
use code_memory::CodeMemoryManager;
use code_memory::clear_code_memory;
use code_memory::get_code_memory_status;
use code_memory::index_code_memory;
use code_memory::search_code_memory;
use code_policy::get_code_policy_status;
use code_policy::validate_sandbox_response;
use desktop_agent::DesktopAgent;
use desktop_agent::desktop_click;
use desktop_agent::desktop_status;
use desktop_agent::desktop_type;
use desktop_agent::disable_desktop_work_mode;
use desktop_agent::enable_desktop_work_mode;
use tauri::Emitter;
use tauri::Manager;

pub fn run() -> anyhow::Result<()> {
    let proxy = CliProxyManager::from_environment().context("initialize CLIProxyAPI client")?;
    let a2a = A2aServer::new(proxy.client());
    let browser = BrowserAgent::default();

    let app = tauri::Builder::default()
        .manage(proxy.clone())
        .manage(browser.clone())
        .manage(DesktopAgent::new())
        .manage(a2a.clone())
        .invoke_handler(tauri::generate_handler![
            get_proxy_status,
            ensure_cliproxyapi,
            run_chatgpt_browser_login,
            fetch_active_models,
            start_chat_completion,
            get_code_policy_status,
            validate_sandbox_response,
            browser_status,
            browser_start,
            browser_navigate,
            browser_click_selector,
            browser_click_at,
            browser_type,
            browser_stop,
            desktop_status,
            enable_desktop_work_mode,
            disable_desktop_work_mode,
            desktop_click,
            desktop_type,
            get_a2a_status,
            list_a2a_tasks,
            delegate_a2a_task,
            get_a2a_task,
            cancel_a2a_task,
            get_code_memory_status,
            index_code_memory,
            search_code_memory,
            clear_code_memory,
        ])
        .setup(|app| {
            let code_memory = CodeMemoryManager::new(
                app.path()
                    .app_data_dir()
                    .context("resolve Bridge Codex application data directory")?
                    .join("code-memory-v1.json"),
            );
            if !app.manage(code_memory.clone()) {
                return Err(anyhow::anyhow!("code-memory state was already initialized").into());
            }
            let code_memory_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                code_memory.restore().await;
                let _ = code_memory_app.emit("code-memory-status", code_memory.status().await);
            });

            let proxy = app.state::<CliProxyManager>().inner().clone();
            let proxy_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let status = proxy.ensure_running().await;
                let _ = proxy_app.emit(
                    "proxy-status",
                    match status {
                        Ok(status) => serde_json::json!({
                            "running": status.running,
                            "binaryPath": status.binary_path,
                            "error": null,
                        }),
                        Err(error) => serde_json::json!({
                            "running": false,
                            "binaryPath": null,
                            "error": error.to_string(),
                        }),
                    },
                );
            });

            let a2a = app.state::<A2aServer>().inner().clone();
            let a2a_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = a2a.start(a2a_app.clone()).await {
                    let _ = a2a_app.emit(
                        "a2a-status",
                        serde_json::json!({
                            "running": false,
                            "address": "127.0.0.1:8120",
                            "error": error.to_string(),
                        }),
                    );
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .context("build Bridge Codex application")?;

    let exit_code = app.run_return(|_, _| {});
    tauri::async_runtime::block_on(async {
        browser.shutdown().await;
        a2a.shutdown().await;
        if let Err(error) = proxy.shutdown().await {
            tracing::warn!(%error, "failed to stop a CLIProxyAPI child process cleanly");
        }
    });
    if exit_code != 0 {
        anyhow::bail!("Bridge Codex exited with status {exit_code}");
    }
    Ok(())
}
