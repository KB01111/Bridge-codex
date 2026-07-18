mod a2a;
mod a2a_handlers;
mod a2a_protocol;
mod a2a_task_store;
mod agent_runtime;
mod agent_runtime_config;
mod agent_runtime_protocol;
mod agent_runtime_tools;
mod bridge_tools;
mod browser_agent;
mod browser_agent_commands;
mod cli_proxy;
mod cli_proxy_conformance;
mod cli_proxy_http;
mod cli_proxy_manager;
mod code_memory;
mod code_policy;
mod code_policy_checks;
mod data_management;
mod desktop_agent;
mod diagnostics;
mod proxy_credentials;

use a2a::A2aServer;
use a2a_handlers::cancel_a2a_task;
use a2a_handlers::configure_a2a_server;
use a2a_handlers::delegate_a2a_task;
use a2a_handlers::delete_a2a_token;
use a2a_handlers::generate_a2a_token;
use a2a_handlers::get_a2a_status;
use a2a_handlers::get_a2a_task;
use a2a_handlers::list_a2a_tasks;
use a2a_handlers::regenerate_a2a_token;
use agent_runtime::BridgeAgentRuntime;
use agent_runtime::agent_request;
use agent_runtime::clear_proxy_credentials;
use agent_runtime::configure_proxy;
use agent_runtime::ensure_agent_runtime;
use agent_runtime::get_agent_runtime_status;
use agent_runtime::list_pending_agent_requests;
use agent_runtime::reject_agent_request;
use agent_runtime::resolve_agent_request;
use anyhow::Context;
use bridge_tools::BridgeToolExecutor;
use bridge_tools::execute_agent_dynamic_tool;
use bridge_tools::grant_agent_browser_consent;
use bridge_tools::revoke_agent_browser_consent;
use browser_agent::BrowserAgent;
use browser_agent_commands::browser_click_at;
use browser_agent_commands::browser_click_selector;
use browser_agent_commands::browser_navigate;
use browser_agent_commands::browser_start;
use browser_agent_commands::browser_status;
use browser_agent_commands::browser_stop;
use browser_agent_commands::browser_type;
use cli_proxy::fetch_active_models;
use cli_proxy_manager::CliProxyManager;
use cli_proxy_manager::get_proxy_status;
use code_memory::CodeMemoryManager;
use code_memory::clear_code_memory;
use code_memory::get_code_memory_status;
use code_memory::index_code_memory;
use code_memory::search_code_memory;
use code_policy::get_code_policy_status;
use code_policy::validate_sandbox_response;
use codex_arg0::Arg0DispatchPaths;
use data_management::delete_all_local_data;
use desktop_agent::DesktopAgent;
use desktop_agent::desktop_click;
use desktop_agent::desktop_status;
use desktop_agent::desktop_type;
use desktop_agent::disable_desktop_work_mode;
use desktop_agent::enable_desktop_work_mode;
use diagnostics::Diagnostics;
use diagnostics::export_support_bundle;
use tauri::Emitter;
use tauri::Manager;

pub fn run(arg0_paths: Arg0DispatchPaths) -> anyhow::Result<()> {
    let proxy = CliProxyManager::from_credentials().context("initialize CLIProxyAPI client")?;
    let agent_runtime = BridgeAgentRuntime::new(proxy.clone(), arg0_paths);
    let browser = BrowserAgent::default();

    let app = tauri::Builder::default()
        .manage(proxy.clone())
        .manage(agent_runtime.clone())
        .manage(browser.clone())
        .manage(BridgeToolExecutor::default())
        .manage(DesktopAgent::new())
        .invoke_handler(tauri::generate_handler![
            get_proxy_status,
            fetch_active_models,
            get_agent_runtime_status,
            list_pending_agent_requests,
            ensure_agent_runtime,
            agent_request,
            resolve_agent_request,
            reject_agent_request,
            configure_proxy,
            clear_proxy_credentials,
            execute_agent_dynamic_tool,
            grant_agent_browser_consent,
            revoke_agent_browser_consent,
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
            configure_a2a_server,
            generate_a2a_token,
            regenerate_a2a_token,
            delete_a2a_token,
            list_a2a_tasks,
            delegate_a2a_task,
            get_a2a_task,
            cancel_a2a_task,
            get_code_memory_status,
            index_code_memory,
            search_code_memory,
            clear_code_memory,
            export_support_bundle,
            delete_all_local_data,
        ])
        .setup(|app| {
            let app_data_dir = app
                .path()
                .app_data_dir()
                .context("resolve Bridge Codex application data directory")?;
            let diagnostics = Diagnostics::install(app_data_dir.clone())
                .context("initialize local diagnostics")?;
            if !app.manage(diagnostics) {
                return Err(anyhow::anyhow!("diagnostics state was already initialized").into());
            }

            let code_memory = CodeMemoryManager::new(app_data_dir.join("code-memory-v1.json"));
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
                let _ = proxy_app.emit("proxy-status", proxy.status().await);
            });

            let agent_runtime = app.state::<BridgeAgentRuntime>().inner().clone();
            let a2a = A2aServer::from_app_data_dir(agent_runtime.clone(), app_data_dir);
            if !app.manage(a2a.clone()) {
                return Err(anyhow::anyhow!("A2A state was already initialized").into());
            }

            let agent_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = agent_runtime.start(agent_app).await {
                    tracing::warn!(%error, "embedded agent runtime is unavailable");
                }
            });

            let a2a_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = a2a.start(a2a_app.clone()).await {
                    tracing::warn!(%error, "A2A service is unavailable");
                    let _ = a2a_app.emit("a2a-status", a2a.status().await);
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .context("build Bridge Codex application")?;

    let app_handle = app.handle().clone();
    let exit_code = app.run_return(|_, _| {});
    tauri::async_runtime::block_on(async {
        app_handle.state::<A2aServer>().shutdown().await;
        if let Err(error) = agent_runtime.shutdown().await {
            tracing::warn!(%error, "failed to stop the embedded agent runtime cleanly");
        }
        if let Err(error) = browser.shutdown().await {
            tracing::warn!(%error, "failed to stop the browser and remove its profiles");
        }
        if let Err(error) = proxy.shutdown().await {
            tracing::warn!(%error, "failed to close the CLIProxyAPI boundary cleanly");
        }
    });
    if exit_code != 0 {
        anyhow::bail!("Bridge Codex exited with status {exit_code}");
    }
    Ok(())
}
