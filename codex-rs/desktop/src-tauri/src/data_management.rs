use std::path::Path;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use tauri::State;

use crate::a2a::A2aServer;
use crate::agent_runtime::BridgeAgentRuntime;
use crate::browser_agent::BrowserAgent;
use crate::cli_proxy_manager::CliProxyManager;
use crate::code_memory::CodeMemoryManager;
use crate::diagnostics::Diagnostics;

const APP_IDENTIFIER: &str = "com.kbhelios.bridge-codex";

#[tauri::command]
pub async fn delete_all_local_data(
    runtime: State<'_, BridgeAgentRuntime>,
    browser: State<'_, BrowserAgent>,
    a2a: State<'_, A2aServer>,
    proxy: State<'_, CliProxyManager>,
    memory: State<'_, CodeMemoryManager>,
    diagnostics: State<'_, Diagnostics>,
) -> Result<(), String> {
    delete_all(&runtime, &browser, &a2a, &proxy, &memory, &diagnostics)
        .await
        .map_err(|error| format!("{error:#}"))
}

async fn delete_all(
    runtime: &BridgeAgentRuntime,
    browser: &BrowserAgent,
    a2a: &A2aServer,
    proxy: &CliProxyManager,
    memory: &CodeMemoryManager,
    diagnostics: &Diagnostics,
) -> Result<()> {
    let app_data_dir = diagnostics.app_data_dir().to_path_buf();
    let mut errors = Vec::new();
    if let Err(error) = a2a.shutdown_and_delete_token().await {
        errors.push(format!("remove A2A bearer token: {error:#}"));
    }
    if let Err(error) = runtime.shutdown().await {
        errors.push(format!("stop the embedded agent runtime: {error:#}"));
    }
    if let Err(error) = browser.shutdown().await {
        errors.push(format!("stop browser and remove profiles: {error:#}"));
    }
    if let Err(error) = memory.clear().await {
        errors.push(format!("clear code-memory index: {error:#}"));
    }
    if let Err(error) = proxy.clear_credentials().await {
        errors.push(format!("remove CLIProxyAPI credentials: {error:#}"));
    }
    if let Err(error) = diagnostics.disable_and_clear() {
        errors.push(format!("clear diagnostic logs: {error:#}"));
    }
    if let Err(error) = remove_bridge_data_root(&app_data_dir) {
        errors.push(format!("remove Bridge application data: {error:#}"));
    }
    if !errors.is_empty() {
        bail!("local-data deletion was incomplete: {}", errors.join("; "));
    }
    Ok(())
}

fn remove_bridge_data_root(root: &Path) -> Result<()> {
    if !root.is_absolute() {
        bail!("refusing to remove a non-absolute application-data path");
    }
    let expected = root
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case(APP_IDENTIFIER));
    if !expected {
        bail!(
            "refusing to remove unexpected application-data path {}",
            root.display()
        );
    }
    match std::fs::remove_dir_all(root) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).context("remove Bridge application data"),
    }
}

#[cfg(test)]
#[path = "data_management_tests.rs"]
mod tests;
