use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::Ordering;

use anyhow::Context;
use anyhow::Result;
use codex_app_server_client::AppServerClient;
use codex_app_server_client::AppServerEvent;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_client::DEFAULT_IN_PROCESS_CHANNEL_CAPACITY;
use codex_app_server_client::EnvironmentManager;
use codex_app_server_client::ExecServerRuntimePaths;
use codex_app_server_client::InProcessAppServerClient;
use codex_app_server_client::InProcessClientStartArgs;
use codex_app_server_client::legacy_core::config::ConfigBuilder;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::RequestId;
use codex_arg0::Arg0DispatchPaths;
use codex_config::CloudConfigBundleLoader;
use codex_config::LoaderOverrides;
use codex_feedback::CodexFeedback;
use codex_protocol::protocol::SessionSource;
use serde_json::Value;
use tauri::AppHandle;
use tauri::Emitter;
use tauri::Manager;
use tauri::State;
use tokio::sync::RwLock;
use tokio::sync::Semaphore;
use tokio::sync::broadcast;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

use crate::agent_runtime_config::bridge_codex_home;
use crate::agent_runtime_config::config_warnings;
use crate::agent_runtime_config::provider_cli_overrides;
use crate::agent_runtime_protocol::BridgeAgentEvent;
use crate::agent_runtime_protocol::BridgeAgentRequest;
use crate::agent_runtime_protocol::BridgeAgentRuntimeStatus;
use crate::agent_runtime_protocol::BridgePendingServerRequest;
use crate::cli_proxy_manager::CliProxyManager;
use crate::cli_proxy_manager::ProxyStatus;

const AGENT_EVENT: &str = "agent-event";
const AGENT_STATUS_EVENT: &str = "agent-status";
const CONTROL_CHANNEL_CAPACITY: usize = 32;
const EVENT_CHANNEL_CAPACITY: usize = 256;
const MAX_PENDING_SERVER_REQUESTS: usize = 128;
const MAX_PENDING_SERVER_REQUEST_BYTES: usize = 256 * 1024;
const PENDING_REQUEST_REJECTION_CODE: i64 = -32011;

struct RuntimeState {
    status: BridgeAgentRuntimeStatus,
    request_handle: Option<AppServerRequestHandle>,
    control_tx: Option<mpsc::Sender<RuntimeControl>>,
    pending_requests: PendingServerRequests,
}

#[derive(Default)]
struct PendingServerRequests(BTreeMap<RequestId, BridgePendingServerRequest>);

impl PendingServerRequests {
    fn retain(&mut self, request: BridgePendingServerRequest) -> Result<(), String> {
        let payload_bytes = serde_json::to_vec(&request.payload)
            .map_err(|_| "agent request payload could not be bounded".to_string())?
            .len();
        if payload_bytes > MAX_PENDING_SERVER_REQUEST_BYTES {
            return Err(format!(
                "agent request exceeds the {MAX_PENDING_SERVER_REQUEST_BYTES}-byte pending-request limit"
            ));
        }
        if !self.0.contains_key(&request.request_id) && self.0.len() >= MAX_PENDING_SERVER_REQUESTS
        {
            return Err(format!(
                "agent request exceeds the {MAX_PENDING_SERVER_REQUESTS}-request pending limit"
            ));
        }
        self.0.insert(request.request_id.clone(), request);
        Ok(())
    }

    fn finish_resolution(&mut self, request_id: &RequestId, result: &Result<(), String>) {
        if result.is_ok() {
            self.0.remove(request_id);
        }
    }

    fn list(&self) -> Vec<BridgePendingServerRequest> {
        self.0.values().cloned().collect()
    }

    fn clear(&mut self) {
        self.0.clear();
    }
}

enum RuntimeControl {
    Resolve {
        request_id: RequestId,
        result: Value,
        response_tx: oneshot::Sender<Result<(), String>>,
    },
    Reject {
        request_id: RequestId,
        message: String,
        response_tx: oneshot::Sender<Result<(), String>>,
    },
    Shutdown {
        response_tx: oneshot::Sender<Result<(), String>>,
    },
}

#[derive(Clone)]
pub struct BridgeAgentRuntime {
    proxy: CliProxyManager,
    arg0_paths: Arg0DispatchPaths,
    state: Arc<RwLock<RuntimeState>>,
    start_gate: Arc<Semaphore>,
    next_request_id: Arc<AtomicI64>,
    events: broadcast::Sender<BridgeAgentEvent>,
}

impl BridgeAgentRuntime {
    pub fn new(proxy: CliProxyManager, arg0_paths: Arg0DispatchPaths) -> Self {
        let client = proxy.client();
        let (events, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        Self {
            proxy,
            arg0_paths,
            state: Arc::new(RwLock::new(RuntimeState {
                status: BridgeAgentRuntimeStatus {
                    running: false,
                    authenticated: client.has_api_key(),
                    responses_api: false,
                    provider_base_url: client.responses_provider_base_url().ok(),
                    probed_model: None,
                    error: None,
                },
                request_handle: None,
                control_tx: None,
                pending_requests: PendingServerRequests::default(),
            })),
            start_gate: Arc::new(Semaphore::new(1)),
            next_request_id: Arc::new(AtomicI64::new(1)),
            events,
        }
    }

    pub async fn status(&self) -> BridgeAgentRuntimeStatus {
        self.state.read().await.status.clone()
    }

    pub async fn start(&self, app: AppHandle) -> Result<BridgeAgentRuntimeStatus> {
        let _start_permit = self
            .start_gate
            .clone()
            .acquire_owned()
            .await
            .context("agent runtime start gate was closed")?;
        if self.state.read().await.status.running {
            return Ok(self.status().await);
        }

        if let Err(error) = self.start_inner(app.clone()).await {
            self.set_unavailable(&app, error.to_string()).await;
            return Err(error);
        }
        Ok(self.status().await)
    }

    async fn start_inner(&self, app: AppHandle) -> Result<()> {
        let proxy_status = self.proxy.ensure_running().await?;
        let probed_model = proxy_status
            .probed_model
            .context("CLIProxyAPI did not report a conformance-tested model")?;
        let proxy_client = self.proxy.client();
        let provider_base_url = proxy_client.responses_provider_base_url()?;
        let api_key = proxy_client.api_key()?;
        let cli_overrides = provider_cli_overrides(&provider_base_url, &api_key, &probed_model);
        let codex_home = bridge_codex_home(
            &app.path()
                .app_data_dir()
                .context("failed to resolve the Bridge Codex application data directory")?,
        );
        let create_home = codex_home.clone();
        tokio::task::spawn_blocking(move || std::fs::create_dir_all(create_home))
            .await
            .context("failed to join Bridge Codex home creation")??;
        let config = ConfigBuilder::default()
            .codex_home(codex_home)
            .cli_overrides(cli_overrides.clone())
            .build()
            .await
            .context("failed to build the embedded agent configuration")?;
        let runtime_paths = ExecServerRuntimePaths::from_optional_paths(
            self.arg0_paths.codex_self_exe.clone(),
            self.arg0_paths.codex_linux_sandbox_exe.clone(),
        )?;
        let state_db = codex_core::init_state_db(&config).await;
        let environment_manager =
            EnvironmentManager::from_codex_home(config.codex_home.clone(), Some(runtime_paths))
                .await?;
        let config_warnings = config_warnings(&config);
        let client = InProcessAppServerClient::start(InProcessClientStartArgs {
            arg0_paths: self.arg0_paths.clone(),
            config: Arc::new(config),
            cli_overrides,
            loader_overrides: LoaderOverrides::default(),
            strict_config: false,
            cloud_config_bundle: CloudConfigBundleLoader::default(),
            feedback: CodexFeedback::new(),
            log_db: None,
            state_db,
            environment_manager: Arc::new(environment_manager),
            config_warnings,
            session_source: SessionSource::Custom("bridge-desktop".to_string()),
            enable_codex_api_key_env: false,
            client_name: "bridge-desktop".to_string(),
            client_version: env!("CARGO_PKG_VERSION").to_string(),
            experimental_api: true,
            mcp_server_openai_form_elicitation: false,
            opt_out_notification_methods: Vec::new(),
            channel_capacity: DEFAULT_IN_PROCESS_CHANNEL_CAPACITY,
        })
        .await
        .context("failed to start the embedded app-server")?;
        let client = AppServerClient::InProcess(client);
        let request_handle = client.request_handle();
        let (control_tx, control_rx) = mpsc::channel(CONTROL_CHANNEL_CAPACITY);
        {
            let mut state = self.state.write().await;
            state.status = BridgeAgentRuntimeStatus {
                running: true,
                authenticated: true,
                responses_api: true,
                provider_base_url: Some(provider_base_url),
                probed_model: Some(probed_model),
                error: None,
            };
            state.request_handle = Some(request_handle);
            state.control_tx = Some(control_tx);
        }
        let _ = app.emit(AGENT_STATUS_EVENT, self.status().await);
        tauri::async_runtime::spawn(run_client_loop(
            client,
            control_rx,
            app,
            Arc::clone(&self.state),
            self.events.clone(),
        ));
        Ok(())
    }

    async fn set_unavailable(&self, app: &AppHandle, message: String) {
        let client = self.proxy.client();
        let status = BridgeAgentRuntimeStatus {
            running: false,
            authenticated: client.has_api_key(),
            responses_api: false,
            provider_base_url: client.responses_provider_base_url().ok(),
            probed_model: None,
            error: Some(message),
        };
        {
            let mut state = self.state.write().await;
            state.status = status.clone();
            state.request_handle = None;
            state.control_tx = None;
        }
        let _ = app.emit(AGENT_STATUS_EVENT, status);
    }

    pub(crate) async fn request(&self, request: BridgeAgentRequest) -> Result<Value> {
        let request_handle = self
            .state
            .read()
            .await
            .request_handle
            .clone()
            .context("embedded agent runtime is not running")?;
        let gateway_model = self
            .state
            .read()
            .await
            .status
            .probed_model
            .clone()
            .context("embedded agent runtime has no conformance-tested model")?;
        let request_id = RequestId::Integer(self.next_request_id.fetch_add(1, Ordering::Relaxed));
        let result = request_handle
            .request(request.into_client_request(request_id, &gateway_model)?)
            .await
            .context("embedded agent request transport failed")?;
        result.map_err(|error| {
            anyhow::anyhow!(
                "embedded agent request failed: {} (code {})",
                error.message,
                error.code
            )
        })
    }

    pub(crate) fn subscribe_events(&self) -> broadcast::Receiver<BridgeAgentEvent> {
        self.events.subscribe()
    }

    pub async fn pending_requests(&self) -> Vec<BridgePendingServerRequest> {
        self.state.read().await.pending_requests.list()
    }

    async fn send_control(&self, control: RuntimeControl) -> Result<()> {
        let control_tx = self
            .state
            .read()
            .await
            .control_tx
            .clone()
            .context("embedded agent runtime is not running")?;
        control_tx
            .send(control)
            .await
            .context("embedded agent control channel is closed")
    }

    pub async fn shutdown(&self) -> Result<()> {
        let Some(control_tx) = self.state.read().await.control_tx.clone() else {
            return Ok(());
        };
        let (response_tx, response_rx) = oneshot::channel();
        control_tx
            .send(RuntimeControl::Shutdown { response_tx })
            .await
            .context("embedded agent control channel is closed")?;
        response_rx
            .await
            .context("embedded agent shutdown response was dropped")?
            .map_err(anyhow::Error::msg)
    }
}

async fn run_client_loop(
    mut client: AppServerClient,
    mut control_rx: mpsc::Receiver<RuntimeControl>,
    app: AppHandle,
    state: Arc<RwLock<RuntimeState>>,
    events: broadcast::Sender<BridgeAgentEvent>,
) {
    let mut shutdown_response = None;
    let disconnect_message = loop {
        tokio::select! {
            event = client.next_event() => {
                let Some(event) = event else {
                    break "embedded app-server event stream closed".to_string();
                };
                match bridge_event(event) {
                    Ok((event, pending_request)) => {
                        if let Some(pending_request) = pending_request {
                            let request_id = pending_request.request_id.clone();
                            let retain_result = state
                                .write()
                                .await
                                .pending_requests
                                .retain(pending_request);
                            if let Err(message) = retain_result {
                                if let Err(error) = client
                                    .reject_server_request(
                                        request_id,
                                        JSONRPCErrorError {
                                            code: PENDING_REQUEST_REJECTION_CODE,
                                            message,
                                            data: None,
                                        },
                                    )
                                    .await
                                {
                                    break format!(
                                        "failed to reject an unbounded agent request: {error}"
                                    );
                                }
                                continue;
                            }
                        }
                        let _ = events.send(event.clone());
                        let _ = app.emit(AGENT_EVENT, event);
                    }
                    Err(error) => break error.to_string(),
                }
            }
            control = control_rx.recv() => {
                match control {
                    Some(RuntimeControl::Resolve { request_id, result, response_tx }) => {
                        let result = client
                            .resolve_server_request(request_id.clone(), result)
                            .await
                            .map_err(|error| error.to_string());
                        state
                            .write()
                            .await
                            .pending_requests
                            .finish_resolution(&request_id, &result);
                        let _ = response_tx.send(result);
                    }
                    Some(RuntimeControl::Reject { request_id, message, response_tx }) => {
                        let result = client
                            .reject_server_request(
                                request_id.clone(),
                                JSONRPCErrorError { code: -32010, message, data: None },
                            )
                            .await
                            .map_err(|error| error.to_string());
                        state
                            .write()
                            .await
                            .pending_requests
                            .finish_resolution(&request_id, &result);
                        let _ = response_tx.send(result);
                    }
                    Some(RuntimeControl::Shutdown { response_tx }) => {
                        shutdown_response = Some(response_tx);
                        break "embedded agent runtime stopped".to_string();
                    }
                    None => break "embedded agent control channel closed".to_string(),
                }
            }
        }
    };
    let shutdown_result = client.shutdown().await.map_err(|error| error.to_string());
    {
        let mut state = state.write().await;
        state.status.running = false;
        state.status.error = if shutdown_response.is_some() {
            None
        } else {
            Some(disconnect_message.clone())
        };
        state.request_handle = None;
        state.control_tx = None;
        state.pending_requests.clear();
    }
    let disconnected = BridgeAgentEvent::Disconnected {
        message: disconnect_message,
    };
    let _ = events.send(disconnected.clone());
    let _ = app.emit(AGENT_EVENT, disconnected);
    let _ = app.emit(AGENT_STATUS_EVENT, state.read().await.status.clone());
    if let Some(response_tx) = shutdown_response {
        let _ = response_tx.send(shutdown_result);
    }
}

fn bridge_event(
    event: AppServerEvent,
) -> Result<(BridgeAgentEvent, Option<BridgePendingServerRequest>)> {
    Ok(match event {
        AppServerEvent::Lagged { skipped } => (BridgeAgentEvent::Lagged { skipped }, None),
        AppServerEvent::ServerNotification(notification) => (
            BridgeAgentEvent::ServerNotification {
                payload: serde_json::to_value(notification)
                    .context("failed to serialize an agent notification")?,
            },
            None,
        ),
        AppServerEvent::ServerRequest(request) => {
            let request_id = request.id().clone();
            let payload =
                serde_json::to_value(request).context("failed to serialize an agent request")?;
            (
                BridgeAgentEvent::ServerRequest {
                    payload: payload.clone(),
                },
                Some(BridgePendingServerRequest {
                    request_id,
                    payload,
                }),
            )
        }
        AppServerEvent::Disconnected { message } => {
            (BridgeAgentEvent::Disconnected { message }, None)
        }
    })
}

#[tauri::command]
pub async fn get_agent_runtime_status(
    state: State<'_, BridgeAgentRuntime>,
) -> Result<BridgeAgentRuntimeStatus, String> {
    Ok(state.status().await)
}

#[tauri::command]
pub async fn list_pending_agent_requests(
    state: State<'_, BridgeAgentRuntime>,
) -> Result<Vec<BridgePendingServerRequest>, String> {
    Ok(state.pending_requests().await)
}

#[tauri::command]
pub async fn ensure_agent_runtime(
    app: AppHandle,
    state: State<'_, BridgeAgentRuntime>,
) -> Result<BridgeAgentRuntimeStatus, String> {
    state.start(app).await.map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn agent_request(
    state: State<'_, BridgeAgentRuntime>,
    request: BridgeAgentRequest,
) -> Result<Value, String> {
    state
        .request(request)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn resolve_agent_request(
    state: State<'_, BridgeAgentRuntime>,
    request_id: RequestId,
    result: Value,
) -> Result<(), String> {
    let (response_tx, response_rx) = oneshot::channel();
    state
        .send_control(RuntimeControl::Resolve {
            request_id,
            result,
            response_tx,
        })
        .await
        .map_err(|error| error.to_string())?;
    response_rx
        .await
        .map_err(|_| "agent resolve response was dropped".to_string())?
}

#[tauri::command]
pub async fn reject_agent_request(
    state: State<'_, BridgeAgentRuntime>,
    request_id: RequestId,
    message: String,
) -> Result<(), String> {
    let (response_tx, response_rx) = oneshot::channel();
    state
        .send_control(RuntimeControl::Reject {
            request_id,
            message,
            response_tx,
        })
        .await
        .map_err(|error| error.to_string())?;
    response_rx
        .await
        .map_err(|_| "agent rejection response was dropped".to_string())?
}

#[tauri::command]
pub async fn configure_proxy(
    app: AppHandle,
    runtime: State<'_, BridgeAgentRuntime>,
    proxy: State<'_, CliProxyManager>,
    base_url: String,
    api_key: String,
) -> Result<ProxyStatus, String> {
    let status = proxy
        .configure(base_url, api_key)
        .await
        .map_err(|error| error.to_string())?;
    let _ = app.emit("proxy-status", &status);
    runtime
        .shutdown()
        .await
        .map_err(|error| error.to_string())?;
    runtime
        .start(app)
        .await
        .map_err(|error| error.to_string())?;
    Ok(status)
}

#[tauri::command]
pub async fn clear_proxy_credentials(
    app: AppHandle,
    runtime: State<'_, BridgeAgentRuntime>,
    proxy: State<'_, CliProxyManager>,
) -> Result<ProxyStatus, String> {
    runtime
        .shutdown()
        .await
        .map_err(|error| error.to_string())?;
    proxy
        .clear_credentials()
        .await
        .map_err(|error| error.to_string())?;
    runtime
        .set_unavailable(
            &app,
            "CLIProxyAPI authentication is not configured".to_string(),
        )
        .await;
    let status = proxy.status().await;
    let _ = app.emit("proxy-status", &status);
    Ok(status)
}

#[cfg(test)]
#[path = "agent_runtime_tests.rs"]
mod tests;
