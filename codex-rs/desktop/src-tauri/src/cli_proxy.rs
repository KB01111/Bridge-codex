use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use crate::cli_proxy_http::MAX_CHAT_RESPONSE_BYTES;
use crate::cli_proxy_http::MAX_MODELS_RESPONSE_BYTES;
use crate::cli_proxy_http::ensure_success;
use crate::cli_proxy_http::read_success_body;
use crate::cli_proxy_manager::CliProxyManager;
use crate::cli_proxy_request::sandbox_messages;
use crate::cli_proxy_request::validate_chat_request;
use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use serde::Deserialize;
use serde::Serialize;
use tauri::AppHandle;
use tauri::Emitter;
use tauri::State;
use tokio::sync::Semaphore;
use url::Url;
use uuid::Uuid;

use crate::code_policy::CodeValidation;
use crate::code_policy::SandboxValidationEvent;
use crate::code_policy::validate_markdown_response;

const DEFAULT_BASE_URL: &str = "http://localhost:8317/";
const MAX_CONCURRENT_CHATS: usize = 4;
const MAX_CHAT_STREAM_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyModel {
    pub id: String,
    pub object: Option<String>,
    #[serde(rename(serialize = "ownedBy", deserialize = "owned_by"))]
    pub owned_by: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatStreamEvent {
    request_id: String,
    delta: String,
    done: bool,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    data: Vec<ProxyModel>,
}

#[derive(Debug, Serialize)]
struct ChatCompletionRequest<'a> {
    model: &'a str,
    messages: &'a [ChatMessage],
    stream: bool,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatCompletionChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionChoice {
    message: ChatCompletionMessage,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionMessage {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionChunk {
    choices: Vec<ChatCompletionChunkChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionChunkChoice {
    delta: ChatCompletionDelta,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionDelta {
    content: Option<String>,
}

#[derive(Clone)]
pub struct CliProxyClient {
    http: reqwest::Client,
    base_url: Url,
    api_key: Option<String>,
    chat_gate: Arc<Semaphore>,
}

impl CliProxyClient {
    pub(super) fn from_environment() -> Result<Self> {
        Self::new(
            Url::parse(DEFAULT_BASE_URL).context("invalid built-in CLIProxyAPI URL")?,
            std::env::var("CLIPROXYAPI_API_KEY")
                .ok()
                .filter(|value| !value.trim().is_empty()),
        )
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
            base_url,
            api_key,
            chat_gate: Arc::new(Semaphore::new(MAX_CONCURRENT_CHATS)),
        })
    }

    fn request(&self, method: reqwest::Method, path: &str) -> Result<reqwest::RequestBuilder> {
        let url = self
            .base_url
            .join(path)
            .with_context(|| format!("invalid CLIProxyAPI path {path}"))?;
        let request = self.http.request(method, url);
        Ok(match &self.api_key {
            Some(api_key) => request.bearer_auth(api_key),
            None => request,
        })
    }

    pub async fn fetch_models(&self) -> Result<Vec<ProxyModel>> {
        let response = self
            .request(reqwest::Method::GET, "v1/models")?
            .send()
            .await
            .context("failed to reach CLIProxyAPI model registry")?;
        let response = read_success_body(response, MAX_MODELS_RESPONSE_BYTES).await?;
        let mut models = serde_json::from_slice::<ModelsResponse>(&response)
            .context("CLIProxyAPI returned an invalid model list")?
            .data;
        models.sort_by(|left, right| left.id.cmp(&right.id));
        models.dedup_by(|left, right| left.id == right.id);
        Ok(models)
    }

    pub async fn complete_chat(&self, request: &ChatRequest) -> Result<String> {
        validate_chat_request(request)?;
        let _permit = self
            .chat_gate
            .clone()
            .acquire_owned()
            .await
            .context("CLIProxyAPI chat concurrency gate was closed")?;
        let messages = sandbox_messages(&request.messages);
        let payload = ChatCompletionRequest {
            model: &request.model,
            messages: &messages,
            stream: false,
        };
        let response = self
            .request(reqwest::Method::POST, "v1/chat/completions")?
            .json(&payload)
            .send()
            .await
            .context("failed to reach CLIProxyAPI chat completions")?;
        let response = read_success_body(response, MAX_CHAT_RESPONSE_BYTES).await?;
        let response = serde_json::from_slice::<ChatCompletionResponse>(&response)
            .context("CLIProxyAPI returned an invalid chat completion")?;
        let content = response
            .choices
            .into_iter()
            .find_map(|choice| choice.message.content)
            .ok_or_else(|| anyhow!("CLIProxyAPI returned no assistant content"))?;
        if content.trim().is_empty() {
            bail!("CLIProxyAPI returned no assistant content");
        }
        let validation = validate_markdown_response(&content);
        if !validation.valid {
            bail!(
                "CLIProxyAPI returned code that violates the sandbox contract: {}",
                validation.failure_summary()
            );
        }
        Ok(content)
    }

    async fn stream_chat<F>(&self, request: &ChatRequest, mut on_delta: F) -> Result<CodeValidation>
    where
        F: FnMut(String),
    {
        validate_chat_request(request)?;
        let _permit = self
            .chat_gate
            .clone()
            .acquire_owned()
            .await
            .context("CLIProxyAPI chat concurrency gate was closed")?;
        let messages = sandbox_messages(&request.messages);
        let payload = ChatCompletionRequest {
            model: &request.model,
            messages: &messages,
            stream: true,
        };
        let response = self
            .request(reqwest::Method::POST, "v1/chat/completions")?
            .json(&payload)
            .send()
            .await
            .context("failed to reach CLIProxyAPI chat stream")?;
        let response = ensure_success(response).await?;
        let mut stream_bytes = 0usize;
        let bounded_stream = response.bytes_stream().map(move |chunk| {
            let chunk = chunk.context("failed while reading the CLIProxyAPI chat stream")?;
            stream_bytes = stream_bytes
                .checked_add(chunk.len())
                .ok_or_else(|| anyhow!("CLIProxyAPI chat stream size overflow"))?;
            if stream_bytes > MAX_CHAT_STREAM_BYTES {
                bail!(
                    "CLIProxyAPI chat stream exceeds the {MAX_CHAT_STREAM_BYTES}-byte wire limit"
                );
            }
            Ok::<_, anyhow::Error>(chunk)
        });
        let mut events = bounded_stream.eventsource();
        let mut content = String::new();
        let mut saw_done = false;
        while let Some(event) = events.next().await {
            let event =
                event.map_err(|error| anyhow!("CLIProxyAPI chat stream failed: {error}"))?;
            if event.data == "[DONE]" {
                saw_done = true;
                break;
            }
            let chunk: ChatCompletionChunk = serde_json::from_str(&event.data)
                .context("CLIProxyAPI returned an invalid chat stream event")?;
            for delta in chunk
                .choices
                .into_iter()
                .filter_map(|choice| choice.delta.content)
            {
                if content.len().saturating_add(delta.len()) > MAX_CHAT_RESPONSE_BYTES {
                    bail!(
                        "CLIProxyAPI chat stream exceeds the {MAX_CHAT_RESPONSE_BYTES}-byte limit"
                    );
                }
                content.push_str(&delta);
                on_delta(delta);
            }
        }
        if !saw_done {
            bail!("CLIProxyAPI chat stream ended before the [DONE] marker");
        }
        if content.trim().is_empty() {
            bail!("CLIProxyAPI chat stream returned no assistant content");
        }
        Ok(validate_markdown_response(&content))
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
    Ok(())
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

#[tauri::command]
pub async fn start_chat_completion(
    app: AppHandle,
    state: State<'_, CliProxyManager>,
    request: ChatRequest,
) -> Result<String, String> {
    validate_chat_request(&request).map_err(|error| error.to_string())?;
    let request_id = Uuid::new_v4().to_string();
    let stream_request_id = request_id.clone();
    let client = state.client();
    tauri::async_runtime::spawn(async move {
        let result = client
            .stream_chat(&request, |delta| {
                let _ = app.emit(
                    "chat-chunk",
                    ChatStreamEvent {
                        request_id: stream_request_id.clone(),
                        delta,
                        done: false,
                        error: None,
                    },
                );
            })
            .await;
        let validation_error = match &result {
            Ok(validation) => {
                let _ = app.emit(
                    "sandbox-validation",
                    SandboxValidationEvent {
                        request_id: stream_request_id.clone(),
                        validation: validation.clone(),
                    },
                );
                (!validation.valid).then(|| {
                    format!(
                        "generated code violates the sandbox contract: {}",
                        validation.failure_summary()
                    )
                })
            }
            Err(_) => None,
        };
        let _ = app.emit(
            "chat-chunk",
            ChatStreamEvent {
                request_id: stream_request_id,
                delta: String::new(),
                done: true,
                error: result
                    .err()
                    .map(|error| error.to_string())
                    .or(validation_error),
            },
        );
    });
    Ok(request_id)
}

#[cfg(test)]
#[path = "cli_proxy_tests.rs"]
mod tests;
