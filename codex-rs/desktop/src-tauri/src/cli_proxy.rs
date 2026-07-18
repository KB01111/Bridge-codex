use std::collections::HashSet;
use std::net::IpAddr;
use std::sync::Arc;
use std::sync::RwLock;
use std::time::Duration;

use crate::cli_proxy_conformance::probe_responses_conformance;
use crate::cli_proxy_http::MAX_MODELS_RESPONSE_BYTES;
use crate::cli_proxy_http::read_success_body;
use crate::cli_proxy_manager::CliProxyManager;
use crate::proxy_credentials::ProxyCredentialStore;
use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use codex_protocol::openai_models::ModelVisibility;
use serde::Deserialize;
use serde::Serialize;
use tauri::State;
use url::Url;

const DEFAULT_BASE_URL: &str = "http://127.0.0.1:8317/";
const CAPABILITY_PROBE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProxyModelClassification {
    Known,
    #[default]
    Experimental,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyModel {
    pub id: String,
    pub object: Option<String>,
    #[serde(rename(serialize = "ownedBy", deserialize = "owned_by"))]
    pub owned_by: Option<String>,
    #[serde(skip_deserializing, default)]
    pub classification: ProxyModelClassification,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProxyCompatibility {
    Unavailable,
    Basic,
    Conformant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyCapabilities {
    pub models_api: bool,
    pub responses_api: bool,
    pub compatibility: ProxyCompatibility,
    pub conformance_error: Option<String>,
    pub probed_model: Option<String>,
    pub experimental_model_count: usize,
}

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    data: Vec<ProxyModel>,
}

#[derive(Clone)]
pub struct CliProxyClient {
    http: reqwest::Client,
    connection: Arc<RwLock<ProxyConnection>>,
}

struct ProxyConnection {
    base_url: Url,
    api_key: Option<String>,
}

impl CliProxyClient {
    pub(super) fn from_credentials(credentials: &ProxyCredentialStore) -> Result<Self> {
        let configured_base_url = credentials
            .load_base_url()?
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        let base_url = Self::parse_base_url(&configured_base_url)?;
        Self::new(base_url, credentials.load()?)
    }

    pub(crate) fn parse_base_url(configured_base_url: &str) -> Result<Url> {
        let base_url = Url::parse(configured_base_url).context("invalid CLIProxyAPI base URL")?;
        validate_loopback_base_url(&base_url)?;
        Ok(base_url)
    }

    pub(crate) fn default_base_url() -> Result<Url> {
        Self::parse_base_url(DEFAULT_BASE_URL)
    }

    pub(crate) fn new(base_url: Url, api_key: Option<String>) -> Result<Self> {
        validate_loopback_base_url(&base_url)?;
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(90))
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .context("failed to build CLIProxyAPI HTTP client")?;
        Ok(Self {
            http,
            connection: Arc::new(RwLock::new(ProxyConnection { base_url, api_key })),
        })
    }

    pub(super) fn request(
        &self,
        method: reqwest::Method,
        path: &str,
    ) -> Result<reqwest::RequestBuilder> {
        let connection = self
            .connection
            .read()
            .map_err(|_| anyhow!("CLIProxyAPI connection lock is poisoned"))?;
        let url = connection
            .base_url
            .join(path)
            .with_context(|| format!("invalid CLIProxyAPI path {path}"))?;
        let api_key = connection
            .api_key
            .clone()
            .filter(|value| !value.is_empty())
            .context("CLIProxyAPI authentication is not configured")?;
        Ok(self.http.request(method, url).bearer_auth(api_key))
    }

    pub(crate) fn has_api_key(&self) -> bool {
        self.connection.read().is_ok_and(|connection| {
            connection
                .api_key
                .as_ref()
                .is_some_and(|value| !value.is_empty())
        })
    }

    pub(crate) fn api_key(&self) -> Result<String> {
        self.connection
            .read()
            .map_err(|_| anyhow!("CLIProxyAPI connection lock is poisoned"))?
            .api_key
            .clone()
            .filter(|value| !value.is_empty())
            .context("CLIProxyAPI authentication is not configured")
    }

    pub(crate) fn replace_connection(&self, base_url: Url, api_key: Option<String>) -> Result<()> {
        validate_loopback_base_url(&base_url)?;
        *self
            .connection
            .write()
            .map_err(|_| anyhow!("CLIProxyAPI connection lock is poisoned"))? =
            ProxyConnection { base_url, api_key };
        Ok(())
    }

    pub(crate) fn configured_base_url(&self) -> Result<String> {
        Ok(self
            .connection
            .read()
            .map_err(|_| anyhow!("CLIProxyAPI connection lock is poisoned"))?
            .base_url
            .to_string())
    }

    pub(crate) fn responses_provider_base_url(&self) -> Result<String> {
        let base_url = self
            .connection
            .read()
            .map_err(|_| anyhow!("CLIProxyAPI connection lock is poisoned"))?
            .base_url
            .clone();
        Ok(base_url
            .join("v1")
            .context("failed to construct the CLIProxyAPI Responses base URL")?
            .to_string()
            .trim_end_matches('/')
            .to_string())
    }

    pub async fn probe_capabilities(&self) -> Result<ProxyCapabilities> {
        let models = tokio::time::timeout(CAPABILITY_PROBE_TIMEOUT, self.fetch_models())
            .await
            .context("CLIProxyAPI model capability probe timed out")??;
        let experimental_model_count = models
            .iter()
            .filter(|model| model.classification == ProxyModelClassification::Experimental)
            .count();
        let known_model = select_known_gateway_model(&models)?;
        let response = tokio::time::timeout(
            CAPABILITY_PROBE_TIMEOUT,
            self.request(reqwest::Method::POST, "v1/responses")?
                .json(&serde_json::json!({}))
                .send(),
        )
        .await
        .context("CLIProxyAPI Responses capability probe timed out")?
        .context("failed to probe the CLIProxyAPI Responses endpoint")?;
        let status = response.status();
        let responses_api = match status.as_u16() {
            200 | 400 | 422 | 429 => true,
            404 | 405 | 501 => false,
            401 | 403 => bail!("CLIProxyAPI rejected the configured API key ({status})"),
            _ => bail!("CLIProxyAPI Responses capability probe failed with {status}"),
        };
        if !responses_api {
            return Ok(ProxyCapabilities {
                models_api: true,
                responses_api: false,
                compatibility: ProxyCompatibility::Unavailable,
                conformance_error: None,
                probed_model: None,
                experimental_model_count,
            });
        }
        let Some(model) = known_model else {
            let conformance_error = if models.is_empty() {
                "CLIProxyAPI returned no model for the Responses conformance probe"
            } else {
                "CLIProxyAPI exposes only unrecognized experimental models; a bundled Codex model is required for conformance"
            };
            return Ok(ProxyCapabilities {
                models_api: true,
                responses_api: true,
                compatibility: ProxyCompatibility::Basic,
                conformance_error: Some(conformance_error.to_string()),
                probed_model: None,
                experimental_model_count,
            });
        };
        match probe_responses_conformance(self, &model.id).await {
            Ok(()) => Ok(ProxyCapabilities {
                models_api: true,
                responses_api: true,
                compatibility: ProxyCompatibility::Conformant,
                conformance_error: None,
                probed_model: Some(model.id.clone()),
                experimental_model_count,
            }),
            Err(error) => Ok(ProxyCapabilities {
                models_api: true,
                responses_api: true,
                compatibility: ProxyCompatibility::Basic,
                conformance_error: Some(error.to_string()),
                probed_model: Some(model.id.clone()),
                experimental_model_count,
            }),
        }
    }

    pub async fn fetch_models(&self) -> Result<Vec<ProxyModel>> {
        let response = self
            .request(reqwest::Method::GET, "v1/models")?
            .send()
            .await
            .context("failed to reach CLIProxyAPI model registry")?;
        let response =
            read_success_body(response, MAX_MODELS_RESPONSE_BYTES, "models request").await?;
        let mut models = serde_json::from_slice::<ModelsResponse>(&response)
            .context("CLIProxyAPI returned an invalid model list")?
            .data;
        let known_model_ids = known_gateway_model_ids()?
            .into_iter()
            .collect::<HashSet<_>>();
        for model in &mut models {
            model.classification = if known_model_ids.contains(&model.id) {
                ProxyModelClassification::Known
            } else {
                ProxyModelClassification::Experimental
            };
        }
        models.sort_by(|left, right| left.id.cmp(&right.id));
        models.dedup_by(|left, right| left.id == right.id);
        Ok(models)
    }
}

fn validate_loopback_base_url(base_url: &Url) -> Result<()> {
    let host = base_url
        .host_str()
        .ok_or_else(|| anyhow!("CLIProxyAPI URL must include a host"))?;
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback());
    if base_url.scheme() != "http" || !loopback {
        bail!("CLIProxyAPI URL must use HTTP on a loopback host");
    }
    if !base_url.username().is_empty() || base_url.password().is_some() {
        bail!("CLIProxyAPI URL must not contain embedded credentials");
    }
    if base_url.path() != "/" || base_url.query().is_some() || base_url.fragment().is_some() {
        bail!("CLIProxyAPI URL must be a loopback origin without a path, query, or fragment");
    }
    Ok(())
}

pub(crate) fn known_gateway_model_ids() -> Result<Vec<String>> {
    let mut models = codex_models_manager::bundled_models_response()
        .context("failed to load the bundled Codex model catalog")?
        .models
        .into_iter()
        .filter(|model| model.supported_in_api && model.visibility == ModelVisibility::List)
        .collect::<Vec<_>>();
    models.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then_with(|| left.slug.cmp(&right.slug))
    });
    Ok(models.into_iter().map(|model| model.slug).collect())
}

fn select_known_gateway_model(models: &[ProxyModel]) -> Result<Option<&ProxyModel>> {
    for known_id in known_gateway_model_ids()? {
        if let Some(model) = models.iter().find(|model| model.id == known_id) {
            return Ok(Some(model));
        }
    }
    Ok(None)
}

#[tauri::command]
pub async fn fetch_active_models(
    state: State<'_, CliProxyManager>,
) -> Result<Vec<ProxyModel>, String> {
    state
        .client()
        .fetch_models()
        .await
        .map_err(|error| error.to_string())
}

#[cfg(test)]
#[path = "cli_proxy_tests.rs"]
mod tests;
