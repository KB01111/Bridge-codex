use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use tauri::AppHandle;
use tauri::Emitter;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use tokio::sync::Semaphore;
use tokio::sync::watch;

use super::A2aServer;
use super::MAX_A2A_CONCURRENT_TASKS;
use super::agent_message;
use super::credentials::A2aCredentialStore;
use super::now;
use super::persistence::A2aPersistence;
use super::persistence::validate_settings;
use super::runtime::A2aRuntimeClient;
use super::security::A2aHttpConfig;
use super::security::A2aRateLimitState;
use crate::a2a_protocol::A2aServerSettings;
use crate::a2a_protocol::A2aServerStatus;
use crate::a2a_protocol::A2aTokenProvisioning;
use crate::a2a_protocol::TaskState;
use crate::a2a_protocol::TaskStatus;
use crate::a2a_task_store::TaskStore;
use crate::agent_runtime::BridgeAgentRuntime;

const SERVER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);

pub(super) struct A2aServerComponents {
    pub(super) runtime: A2aRuntimeClient,
    pub(super) settings: A2aServerSettings,
    pub(super) token: Option<String>,
    pub(super) configuration_error: Option<String>,
    pub(super) credentials: A2aCredentialStore,
    pub(super) persistence: Option<A2aPersistence>,
    pub(super) tasks: TaskStore,
    pub(super) http_config: A2aHttpConfig,
}

#[derive(Clone, Copy)]
enum TokenProvisionMode {
    Generate,
    Regenerate,
}

impl A2aServer {
    pub fn from_app_data_dir(runtime: BridgeAgentRuntime, app_data_dir: PathBuf) -> Self {
        Self::from_app_data_dir_with_credentials(
            A2aRuntimeClient::bridge(runtime),
            app_data_dir,
            A2aCredentialStore::default(),
        )
    }

    pub(super) fn from_app_data_dir_with_credentials(
        runtime: A2aRuntimeClient,
        app_data_dir: PathBuf,
        credentials: A2aCredentialStore,
    ) -> Self {
        let persistence = A2aPersistence::new(app_data_dir);
        let mut configuration_errors = Vec::new();
        let mut settings = persistence.load_settings().unwrap_or_else(|error| {
            configuration_errors.push(format!("failed to load A2A settings: {error:#}"));
            A2aServerSettings::default()
        });
        let token = credentials.load().unwrap_or_else(|error| {
            configuration_errors.push(format!("failed to load the A2A bearer token: {error:#}"));
            None
        });
        let mut tasks = persistence.load_tasks().unwrap_or_else(|error| {
            configuration_errors.push(format!("failed to load A2A tasks: {error:#}"));
            TaskStore::default()
        });
        let recovered = tasks.recover_working(TaskStatus {
            state: TaskState::Failed,
            timestamp: now(),
            message: Some(agent_message(
                "Task interrupted when Bridge Codex restarted".to_string(),
            )),
        });
        if recovered > 0
            && let Err(error) = persistence.save_tasks_blocking(tasks.list_oldest_first())
        {
            configuration_errors.push(format!("failed to persist recovered A2A tasks: {error:#}"));
        }
        if !configuration_errors.is_empty() {
            settings.enabled = false;
        }
        let configuration_error = (!configuration_errors.is_empty()).then(|| {
            format!(
                "A2A is disabled until it is reconfigured: {}",
                configuration_errors.join("; ")
            )
        });
        Self::new_with_components(A2aServerComponents {
            runtime,
            settings,
            token,
            configuration_error,
            credentials,
            persistence: Some(persistence),
            tasks,
            http_config: A2aHttpConfig::production(),
        })
    }

    pub(super) fn new_with_components(components: A2aServerComponents) -> Self {
        let (stop_generation, _) = watch::channel(0_u64);
        let max_concurrent_requests = components.http_config.max_concurrent_requests;
        let token_configured = components.token.is_some();
        Self {
            runtime: components.runtime,
            tasks: Arc::new(RwLock::new(components.tasks)),
            task_mutation_gate: Arc::new(Semaphore::new(1)),
            status: Arc::new(RwLock::new(A2aServerStatus {
                enabled: components.settings.enabled,
                running: false,
                address: bind_address(components.settings.port),
                token_configured,
                error: components.configuration_error,
            })),
            execution_gate: Arc::new(Semaphore::new(MAX_A2A_CONCURRENT_TASKS)),
            executions: Arc::new(Mutex::new(HashMap::new())),
            settings: Arc::new(RwLock::new(components.settings)),
            token: Arc::new(RwLock::new(components.token)),
            credentials: components.credentials,
            persistence: components.persistence,
            http_config: Arc::new(components.http_config),
            request_gate: Arc::new(Semaphore::new(max_concurrent_requests)),
            rate_limit: Arc::new(Mutex::new(A2aRateLimitState::default())),
            server_gate: Arc::new(Semaphore::new(1)),
            server_task: Arc::new(Mutex::new(None)),
            stop_generation,
            terminated: Arc::new(AtomicBool::new(false)),
        }
    }

    pub async fn start(&self, app: AppHandle) -> Result<()> {
        let _permit = self
            .server_gate
            .clone()
            .acquire_owned()
            .await
            .context("A2A server startup gate was closed")?;
        self.start_locked(app).await
    }

    async fn start_locked(&self, app: AppHandle) -> Result<()> {
        if self.terminated.load(Ordering::Acquire) {
            return Err(anyhow!("A2A server is shutting down"));
        }
        let settings = *self.settings.read().await;
        if !settings.enabled {
            self.set_status(/*running*/ false, /*error*/ None).await;
            return Ok(());
        }
        let token = {
            let token = self.token.read().await;
            token.clone()
        };
        if let Err(error) = super::security::validate_a2a_token(token.as_deref()) {
            self.set_status(/*running*/ false, Some(error.clone()))
                .await;
            return Err(anyhow!(error));
        }
        if self
            .server_task
            .lock()
            .await
            .as_ref()
            .is_some_and(|task| !task.is_finished())
        {
            return Ok(());
        }
        let finished_task = self.server_task.lock().await.take();
        if let Some(task) = finished_task {
            let _ = task.await;
        }
        let generation = *self.stop_generation.borrow();
        let server = self.clone();
        *self.server_task.lock().await = Some(tokio::spawn(async move {
            if server
                .serve(app.clone(), settings.port, generation)
                .await
                .is_err()
            {
                let _ = app.emit("a2a-status", server.status().await);
            }
        }));
        Ok(())
    }

    async fn serve(&self, app: AppHandle, port: u16, generation: u64) -> Result<()> {
        let address = bind_address(port);
        let listener = match TcpListener::bind(&address).await {
            Ok(listener) => listener,
            Err(error) => {
                let message = format!("failed to bind A2A server on {address}: {error}");
                self.set_status(/*running*/ false, Some(message.clone()))
                    .await;
                return Err(anyhow!(message));
            }
        };
        self.set_status(/*running*/ true, /*error*/ None).await;
        let _ = app.emit("a2a-status", self.status().await);
        let mut stop_rx = self.stop_generation.subscribe();
        let result = axum::serve(listener, self.clone().router())
            .with_graceful_shutdown(async move {
                while stop_rx.changed().await.is_ok() {
                    if *stop_rx.borrow() != generation {
                        break;
                    }
                }
            })
            .await
            .context("A2A server stopped");
        let error = result.as_ref().err().map(ToString::to_string);
        self.set_status(/*running*/ false, error).await;
        result
    }

    pub(super) async fn set_status(&self, running: bool, error: Option<String>) {
        let settings = *self.settings.read().await;
        let token_configured = self.token.read().await.is_some();
        *self.status.write().await = A2aServerStatus {
            enabled: settings.enabled,
            running,
            address: bind_address(settings.port),
            token_configured,
            error,
        };
    }

    pub(crate) async fn status(&self) -> A2aServerStatus {
        self.status.read().await.clone()
    }

    pub(crate) async fn port(&self) -> u16 {
        self.settings.read().await.port
    }

    pub(crate) async fn configure(
        &self,
        app: AppHandle,
        settings: A2aServerSettings,
    ) -> Result<A2aServerStatus> {
        validate_settings(settings)?;
        if settings.enabled {
            super::security::validate_a2a_token(self.token.read().await.as_deref())
                .map_err(anyhow::Error::msg)?;
        }
        let _permit = self
            .server_gate
            .clone()
            .acquire_owned()
            .await
            .context("A2A server configuration gate was closed")?;
        if self.terminated.load(Ordering::Acquire) {
            return Err(anyhow!("A2A server is shutting down"));
        }
        if let Some(persistence) = &self.persistence {
            persistence.save_settings(settings).await?;
        }
        self.stop_listener_locked().await;
        *self.settings.write().await = settings;
        self.start_locked(app).await?;
        Ok(self.status().await)
    }

    pub(crate) async fn generate_token(&self) -> Result<A2aTokenProvisioning> {
        self.provision_token(TokenProvisionMode::Generate).await
    }

    pub(crate) async fn regenerate_token(&self) -> Result<A2aTokenProvisioning> {
        self.provision_token(TokenProvisionMode::Regenerate).await
    }

    async fn provision_token(&self, mode: TokenProvisionMode) -> Result<A2aTokenProvisioning> {
        let _permit = self
            .server_gate
            .clone()
            .acquire_owned()
            .await
            .context("A2A server configuration gate was closed")?;
        if self.terminated.load(Ordering::Acquire) {
            return Err(anyhow!("A2A server is shutting down"));
        }
        let token = self
            .credentials
            .generate(matches!(mode, TokenProvisionMode::Regenerate))?;
        *self.token.write().await = Some(token.clone());
        let running = self.status.read().await.running;
        self.set_status(running, /*error*/ None).await;
        Ok(A2aTokenProvisioning {
            token,
            status: self.status().await,
        })
    }

    pub(crate) async fn delete_token(&self) -> Result<A2aServerStatus> {
        let _permit = self
            .server_gate
            .clone()
            .acquire_owned()
            .await
            .context("A2A server configuration gate was closed")?;
        self.credentials.delete()?;
        *self.token.write().await = None;
        self.stop_listener_locked().await;
        let error = self
            .settings
            .read()
            .await
            .enabled
            .then(|| "an A2A bearer token is required before enabling the server".to_string());
        self.set_status(/*running*/ false, error).await;
        Ok(self.status().await)
    }

    pub(crate) async fn shutdown_and_delete_token(&self) -> Result<A2aServerStatus> {
        self.shutdown().await;
        self.delete_token().await
    }

    pub(super) async fn stop_listener_locked(&self) {
        self.stop_generation
            .send_modify(|generation| *generation = generation.wrapping_add(1));
        let server_task = self.server_task.lock().await.take();
        if let Some(mut task) = server_task
            && tokio::time::timeout(SERVER_SHUTDOWN_TIMEOUT, &mut task)
                .await
                .is_err()
        {
            task.abort();
            let _ = task.await;
        }
    }
}

fn bind_address(port: u16) -> String {
    format!("127.0.0.1:{port}")
}
