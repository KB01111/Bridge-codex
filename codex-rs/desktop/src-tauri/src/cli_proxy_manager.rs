use std::sync::Arc;
use std::sync::RwLock;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use serde::Serialize;
use tauri::State;
use tokio::sync::Semaphore;

use crate::cli_proxy::CliProxyClient;
use crate::cli_proxy::ProxyCompatibility;
use crate::proxy_credentials::ProxyCredentialStore;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyStatus {
    pub running: bool,
    pub base_url: String,
    pub authenticated: bool,
    pub responses_api: bool,
    pub compatibility: ProxyCompatibility,
    pub probed_model: Option<String>,
    pub experimental_model_count: usize,
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct CliProxyManager {
    client: CliProxyClient,
    credentials: ProxyCredentialStore,
    connection_gate: Arc<Semaphore>,
    startup_error: Arc<RwLock<Option<String>>>,
}

impl CliProxyManager {
    pub fn from_credentials() -> Result<Self> {
        Self::from_store(ProxyCredentialStore::default())
    }

    fn from_store(credentials: ProxyCredentialStore) -> Result<Self> {
        let (client, startup_error) = match CliProxyClient::from_credentials(&credentials) {
            Ok(client) => (client, None),
            Err(error) => (
                CliProxyClient::new(CliProxyClient::default_base_url()?, /*api_key*/ None)?,
                Some(format!(
                    "CLIProxyAPI credentials are unavailable; reconnect to replace them: {error:#}"
                )),
            ),
        };
        Ok(Self {
            client,
            credentials,
            connection_gate: Arc::new(Semaphore::new(1)),
            startup_error: Arc::new(RwLock::new(startup_error)),
        })
    }

    pub fn client(&self) -> CliProxyClient {
        self.client.clone()
    }

    pub async fn status(&self) -> ProxyStatus {
        let startup_error = match self.startup_error.read() {
            Ok(error) => error.clone(),
            Err(_) => Some("CLIProxyAPI startup-error lock is poisoned".to_string()),
        };
        if let Some(error) = startup_error {
            return ProxyStatus {
                running: false,
                base_url: self.client.configured_base_url().unwrap_or_default(),
                authenticated: false,
                responses_api: false,
                compatibility: ProxyCompatibility::Unavailable,
                probed_model: None,
                experimental_model_count: 0,
                error: Some(error),
            };
        }
        status_for_client(&self.client).await
    }

    /// Validates a user-managed CLIProxyAPI service without discovering or
    /// launching an executable.
    pub async fn ensure_running(&self) -> Result<ProxyStatus> {
        let _connection_permit = self
            .connection_gate
            .clone()
            .acquire_owned()
            .await
            .context("CLIProxyAPI connection gate was closed")?;
        let status = self.status().await;
        if !status.running {
            bail!(
                "{}",
                status
                    .error
                    .as_deref()
                    .unwrap_or("CLIProxyAPI is not ready")
            );
        }
        Ok(status)
    }

    pub async fn configure(&self, base_url: String, api_key: String) -> Result<ProxyStatus> {
        let _connection_permit = self
            .connection_gate
            .clone()
            .acquire_owned()
            .await
            .context("CLIProxyAPI connection gate was closed")?;
        let base_url = CliProxyClient::parse_base_url(&base_url)?;
        let api_key = ProxyCredentialStore::validated_api_key(&api_key)?;
        let candidate = CliProxyClient::new(base_url.clone(), Some(api_key.clone()))?;
        let candidate_status = status_for_client(&candidate).await;
        if !candidate_status.running {
            bail!(
                "{}",
                candidate_status
                    .error
                    .as_deref()
                    .unwrap_or("CLIProxyAPI candidate is not compatible")
            );
        }

        let durable_base_url = base_url.to_string();
        let credentials = self.credentials.clone();
        let stored_key = api_key.clone();
        tokio::task::spawn_blocking(move || {
            credentials.save_connection(&durable_base_url, &api_key)
        })
        .await
        .context("failed to join CLIProxyAPI credential storage")??;
        self.client.replace_connection(base_url, Some(stored_key))?;
        *self
            .startup_error
            .write()
            .map_err(|_| anyhow::anyhow!("CLIProxyAPI startup-error lock is poisoned"))? = None;
        Ok(candidate_status)
    }

    pub async fn clear_credentials(&self) -> Result<()> {
        let credentials = self.credentials.clone();
        tokio::task::spawn_blocking(move || credentials.delete())
            .await
            .context("failed to join CLIProxyAPI credential cleanup")??;
        self.client
            .replace_connection(CliProxyClient::default_base_url()?, /*api_key*/ None)?;
        *self
            .startup_error
            .write()
            .map_err(|_| anyhow::anyhow!("CLIProxyAPI startup-error lock is poisoned"))? = None;
        Ok(())
    }

    pub async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}

async fn status_for_client(client: &CliProxyClient) -> ProxyStatus {
    let base_url = match client.configured_base_url() {
        Ok(base_url) => base_url,
        Err(error) => {
            return ProxyStatus {
                running: false,
                base_url: String::new(),
                authenticated: false,
                responses_api: false,
                compatibility: ProxyCompatibility::Unavailable,
                probed_model: None,
                experimental_model_count: 0,
                error: Some(error.to_string()),
            };
        }
    };
    if !client.has_api_key() {
        return ProxyStatus {
            running: false,
            base_url,
            authenticated: false,
            responses_api: false,
            compatibility: ProxyCompatibility::Unavailable,
            probed_model: None,
            experimental_model_count: 0,
            error: Some("CLIProxyAPI authentication is not configured".to_string()),
        };
    }

    match client.probe_capabilities().await {
        Ok(capabilities) => {
            let error = capabilities.conformance_error.or_else(|| {
                (!capabilities.responses_api).then(|| {
                    "CLIProxyAPI does not expose the required /v1/responses endpoint".to_string()
                })
            });
            ProxyStatus {
                running: capabilities.models_api
                    && capabilities.compatibility == ProxyCompatibility::Conformant,
                base_url,
                authenticated: true,
                responses_api: capabilities.responses_api,
                compatibility: capabilities.compatibility,
                probed_model: capabilities.probed_model,
                experimental_model_count: capabilities.experimental_model_count,
                error,
            }
        }
        Err(error) => ProxyStatus {
            running: false,
            base_url,
            authenticated: true,
            responses_api: false,
            compatibility: ProxyCompatibility::Unavailable,
            probed_model: None,
            experimental_model_count: 0,
            error: Some(error.to_string()),
        },
    }
}

#[tauri::command]
pub async fn get_proxy_status(state: State<'_, CliProxyManager>) -> Result<ProxyStatus, String> {
    Ok(state.status().await)
}

#[cfg(test)]
#[path = "cli_proxy_manager_tests.rs"]
mod tests;
