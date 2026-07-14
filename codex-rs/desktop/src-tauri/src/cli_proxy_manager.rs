use std::path::PathBuf;
use std::process::Child;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use serde::Serialize;
use tauri::State;
use tokio::sync::Mutex;
use tokio::sync::Semaphore;

use crate::cli_proxy::CliProxyClient;
use crate::cli_proxy_process::START_POLL_INTERVAL;
use crate::cli_proxy_process::START_TIMEOUT;
use crate::cli_proxy_process::proxy_command;
use crate::cli_proxy_process::proxy_port_is_open;
use crate::cli_proxy_process::resolve_binary;
use crate::cli_proxy_process::terminate_child;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyStatus {
    pub running: bool,
    pub binary_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginLaunch {
    pub process_id: u32,
}

#[derive(Clone)]
pub struct CliProxyManager {
    client: CliProxyClient,
    startup_gate: Arc<Semaphore>,
    login_gate: Arc<Semaphore>,
    daemon: Arc<Mutex<Option<Child>>>,
    login: Arc<Mutex<Option<Child>>>,
}

impl CliProxyManager {
    pub fn from_environment() -> Result<Self> {
        Ok(Self {
            client: CliProxyClient::from_environment()?,
            startup_gate: Arc::new(Semaphore::new(1)),
            login_gate: Arc::new(Semaphore::new(1)),
            daemon: Arc::new(Mutex::new(None)),
            login: Arc::new(Mutex::new(None)),
        })
    }

    pub fn client(&self) -> CliProxyClient {
        self.client.clone()
    }

    pub async fn status(&self) -> ProxyStatus {
        self.reap_finished_children().await;
        ProxyStatus {
            running: proxy_port_is_open().await,
            binary_path: resolve_binary().ok(),
        }
    }

    pub async fn ensure_running(&self) -> Result<ProxyStatus> {
        let _startup_permit = self
            .startup_gate
            .clone()
            .acquire_owned()
            .await
            .context("CLIProxyAPI startup gate was closed")?;
        if proxy_port_is_open().await {
            return Ok(self.status().await);
        }

        let binary = resolve_binary()?;
        let mut child = proxy_command(&binary)
            .spawn()
            .with_context(|| format!("failed to launch CLIProxyAPI from {}", binary.display()))?;
        let deadline = tokio::time::Instant::now() + START_TIMEOUT;
        while tokio::time::Instant::now() < deadline {
            if proxy_port_is_open().await {
                *self.daemon.lock().await = Some(child);
                return Ok(ProxyStatus {
                    running: true,
                    binary_path: Some(binary),
                });
            }
            if let Some(exit_status) = child
                .try_wait()
                .context("failed to inspect CLIProxyAPI process")?
            {
                bail!("CLIProxyAPI exited before port 8317 was ready ({exit_status})");
            }
            tokio::time::sleep(START_POLL_INTERVAL).await;
        }
        stop_owned_child(Some(child), "daemon").await?;
        bail!("timed out waiting for CLIProxyAPI on localhost:8317")
    }

    pub async fn launch_codex_login(&self) -> Result<LoginLaunch> {
        let _login_permit = self
            .login_gate
            .clone()
            .acquire_owned()
            .await
            .context("CLIProxyAPI login gate was closed")?;
        {
            let mut login = self.login.lock().await;
            if let Some(child) = login.as_mut()
                && child
                    .try_wait()
                    .context("failed to inspect the previous CLIProxyAPI login process")?
                    .is_none()
            {
                bail!("CLIProxyAPI browser login is already running");
            }
            *login = None;
        }

        let binary = resolve_binary()?;
        let mut command = proxy_command(&binary);
        command.arg("-codex-login");
        let mut child = command.spawn().with_context(|| {
            format!(
                "failed to launch the CLIProxyAPI Codex login from {}",
                binary.display()
            )
        })?;
        let process_id = child.id();
        tokio::time::sleep(Duration::from_millis(400)).await;
        if let Some(exit_status) = child
            .try_wait()
            .context("failed to inspect CLIProxyAPI login process")?
            && !exit_status.success()
        {
            bail!("CLIProxyAPI could not start browser login ({exit_status})");
        }
        *self.login.lock().await = Some(child);
        Ok(LoginLaunch { process_id })
    }

    async fn reap_finished_children(&self) {
        for slot in [&self.daemon, &self.login] {
            let mut child = slot.lock().await;
            let finished = child
                .as_mut()
                .and_then(|child| child.try_wait().ok())
                .is_some();
            if finished {
                *child = None;
            }
        }
    }

    pub async fn shutdown(&self) -> Result<()> {
        let _startup_permit = self
            .startup_gate
            .clone()
            .acquire_owned()
            .await
            .context("CLIProxyAPI startup gate was closed")?;
        let _login_permit = self
            .login_gate
            .clone()
            .acquire_owned()
            .await
            .context("CLIProxyAPI login gate was closed")?;
        let daemon = self.daemon.lock().await.take();
        let login = self.login.lock().await.take();
        stop_owned_child(daemon, "daemon").await?;
        stop_owned_child(login, "login").await?;
        Ok(())
    }
}

async fn stop_owned_child(child: Option<Child>, label: &'static str) -> Result<()> {
    let Some(child) = child else {
        return Ok(());
    };
    tokio::task::spawn_blocking(move || terminate_child(child))
        .await
        .with_context(|| format!("failed to join CLIProxyAPI {label} cleanup"))?
        .with_context(|| format!("failed to stop CLIProxyAPI {label} process"))
}

#[tauri::command]
pub async fn get_proxy_status(state: State<'_, CliProxyManager>) -> Result<ProxyStatus, String> {
    Ok(state.status().await)
}

#[tauri::command]
pub async fn ensure_cliproxyapi(state: State<'_, CliProxyManager>) -> Result<ProxyStatus, String> {
    state
        .ensure_running()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn run_chatgpt_browser_login(
    state: State<'_, CliProxyManager>,
) -> Result<LoginLaunch, String> {
    state
        .launch_codex_login()
        .await
        .map_err(|error| error.to_string())
}
