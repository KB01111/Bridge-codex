use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use axum::Router;
use axum::http::StatusCode;
use axum::routing::get;
use axum::routing::post;
use chrono::SecondsFormat;
use chrono::Utc;
use tauri::AppHandle;
use tauri::Emitter;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use tokio::sync::Semaphore;
use tokio::sync::oneshot;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::a2a_handlers::delegate_task;
use crate::a2a_handlers::get_task;
use crate::a2a_handlers::list_tasks;
use crate::a2a_handlers::send_message;
use crate::a2a_protocol::A2aMessage;
use crate::a2a_protocol::A2aRole;
use crate::a2a_protocol::Artifact;
use crate::a2a_protocol::TaskStatus;
use crate::a2a_protocol::TextPart;
use crate::a2a_protocol::agent_card;
use crate::a2a_task_store::TaskStore;
use crate::cli_proxy::ChatMessage;
use crate::cli_proxy::ChatRequest;
use crate::cli_proxy::ChatRole;
use crate::cli_proxy::CliProxyClient;

pub use crate::a2a_protocol::A2aServerStatus;
pub use crate::a2a_protocol::A2aTask;
pub use crate::a2a_protocol::TaskState;

const A2A_BIND_ADDRESS: &str = "127.0.0.1:8120";
const MAX_A2A_CONCURRENT_TASKS: usize = 4;
const MAX_ARTIFACT_BYTES: usize = 64 * 1024;
const MAX_CONTEXT_ID_BYTES: usize = 256;
pub(super) const MAX_MESSAGE_ID_BYTES: usize = 256;
pub(super) const MAX_MESSAGE_PARTS: usize = 32;
const MAX_MODEL_ID_BYTES: usize = 256;
pub(super) const MAX_PROMPT_BYTES: usize = 32 * 1024;
const MAX_STATUS_MESSAGE_BYTES: usize = 4 * 1024;
const SERVER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug)]
pub(super) enum A2aError {
    InvalidRequest(String),
    Upstream(String),
    NotFound(String),
}

impl std::fmt::Display for A2aError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest(message) | Self::Upstream(message) | Self::NotFound(message) => {
                formatter.write_str(message)
            }
        }
    }
}

impl A2aError {
    pub(super) fn into_http(self) -> (StatusCode, String) {
        let status = match &self {
            Self::InvalidRequest(_) => StatusCode::BAD_REQUEST,
            Self::Upstream(_) => StatusCode::BAD_GATEWAY,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
        };
        (status, self.to_string())
    }
}

#[derive(Clone)]
pub struct A2aServer {
    proxy: CliProxyClient,
    tasks: Arc<RwLock<TaskStore>>,
    status: Arc<RwLock<A2aServerStatus>>,
    execution_gate: Arc<Semaphore>,
    executions: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
    server_gate: Arc<Semaphore>,
    server_task: Arc<Mutex<Option<JoinHandle<()>>>>,
    shutdown_tx: watch::Sender<bool>,
}

impl A2aServer {
    pub fn new(proxy: CliProxyClient) -> Self {
        let (shutdown_tx, _) = watch::channel(false);
        Self {
            proxy,
            tasks: Arc::new(RwLock::new(TaskStore::default())),
            status: Arc::new(RwLock::new(A2aServerStatus {
                running: false,
                address: A2A_BIND_ADDRESS.to_string(),
                error: None,
            })),
            execution_gate: Arc::new(Semaphore::new(MAX_A2A_CONCURRENT_TASKS)),
            executions: Arc::new(Mutex::new(HashMap::new())),
            server_gate: Arc::new(Semaphore::new(1)),
            server_task: Arc::new(Mutex::new(None)),
            shutdown_tx,
        }
    }

    pub async fn start(&self, app: AppHandle) -> Result<()> {
        let _permit = self
            .server_gate
            .clone()
            .acquire_owned()
            .await
            .context("A2A server startup gate was closed")?;
        if *self.shutdown_tx.borrow() {
            return Err(anyhow!("A2A server is shutting down"));
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
        let server = self.clone();
        *self.server_task.lock().await = Some(tokio::spawn(async move {
            if let Err(error) = server.serve(app.clone()).await {
                let _ = app.emit(
                    "a2a-status",
                    A2aServerStatus {
                        running: false,
                        address: A2A_BIND_ADDRESS.to_string(),
                        error: Some(error.to_string()),
                    },
                );
            }
        }));
        Ok(())
    }

    async fn serve(&self, app: AppHandle) -> Result<()> {
        let listener = match TcpListener::bind(A2A_BIND_ADDRESS).await {
            Ok(listener) => listener,
            Err(error) => {
                let message = format!("failed to bind A2A server on {A2A_BIND_ADDRESS}: {error}");
                self.set_status(/*running*/ false, Some(message.clone()))
                    .await;
                return Err(anyhow!(message));
            }
        };
        self.set_status(/*running*/ true, /*error*/ None).await;
        let _ = app.emit("a2a-status", self.status().await);
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let result = axum::serve(listener, self.clone().router())
            .with_graceful_shutdown(async move {
                if *shutdown_rx.borrow() {
                    return;
                }
                while shutdown_rx.changed().await.is_ok() {
                    if *shutdown_rx.borrow() {
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

    async fn set_status(&self, running: bool, error: Option<String>) {
        *self.status.write().await = A2aServerStatus {
            running,
            address: A2A_BIND_ADDRESS.to_string(),
            error,
        };
    }

    pub(super) async fn status(&self) -> A2aServerStatus {
        self.status.read().await.clone()
    }

    fn router(self) -> Router {
        Router::new()
            .route("/.well-known/agent-card.json", get(agent_card))
            .route("/a2a/message:send", post(send_message))
            .route("/a2a/tasks", get(list_tasks).post(delegate_task))
            .route("/a2a/tasks/{id}", get(get_task))
            .with_state(self)
    }

    pub(super) async fn enqueue(
        &self,
        prompt: String,
        model: Option<String>,
        context_id: Option<String>,
    ) -> Result<A2aTask, A2aError> {
        validate_text_field("A2A task prompt", &prompt, MAX_PROMPT_BYTES)?;
        let model = self.select_model(model).await?;
        let context_id = match context_id {
            Some(context_id) => {
                validate_text_field("A2A context ID", &context_id, MAX_CONTEXT_ID_BYTES)?;
                context_id
            }
            None => Uuid::new_v4().to_string(),
        };
        let task = A2aTask {
            id: Uuid::new_v4().to_string(),
            context_id,
            status: TaskStatus {
                state: TaskState::Working,
                timestamp: now(),
                message: None,
            },
            artifacts: Vec::new(),
        };
        let evicted = self.tasks.write().await.insert(task.clone());
        if let Some(evicted) = evicted {
            self.abort_execution(&evicted).await;
        }
        self.spawn_execution(task.id.clone(), prompt, model).await;
        Ok(task)
    }

    async fn select_model(&self, requested: Option<String>) -> Result<String, A2aError> {
        if let Some(model) = requested.filter(|model| !model.trim().is_empty()) {
            validate_text_field("A2A model ID", &model, MAX_MODEL_ID_BYTES)?;
            return Ok(model);
        }
        if let Some(model) = std::env::var("CLIPROXYAPI_A2A_MODEL")
            .ok()
            .filter(|model| !model.trim().is_empty())
        {
            validate_text_field("configured A2A model ID", &model, MAX_MODEL_ID_BYTES)?;
            return Ok(model);
        }
        self.proxy
            .fetch_models()
            .await
            .map_err(|error| A2aError::Upstream(error.to_string()))?
            .into_iter()
            .next()
            .map(|model| model.id)
            .ok_or_else(|| A2aError::Upstream("CLIProxyAPI reported no A2A model".to_string()))
    }

    async fn spawn_execution(&self, task_id: String, prompt: String, model: String) {
        self.reap_finished_executions().await;
        let (start_tx, start_rx) = oneshot::channel();
        let server = self.clone();
        let task_id_for_job = task_id.clone();
        let handle = tokio::spawn(async move {
            if start_rx.await.is_ok() {
                server.execute(task_id_for_job, prompt, model).await;
            }
        });
        self.executions.lock().await.insert(task_id, handle);
        let _ = start_tx.send(());
    }

    async fn execute(&self, task_id: String, prompt: String, model: String) {
        let Ok(_permit) = self.execution_gate.acquire().await else {
            self.fail_task(&task_id, "A2A execution gate was closed")
                .await;
            return;
        };
        let result = self
            .proxy
            .complete_chat(&ChatRequest {
                model,
                messages: vec![ChatMessage {
                    role: ChatRole::User,
                    content: prompt,
                }],
            })
            .await;
        match result {
            Ok(content) if content.len() <= MAX_ARTIFACT_BYTES => {
                let mut tasks = self.tasks.write().await;
                let Some(task) = tasks.get_mut(&task_id) else {
                    return;
                };
                if task.status.state != TaskState::Working {
                    return;
                }
                task.status = TaskStatus {
                    state: TaskState::Completed,
                    timestamp: now(),
                    message: None,
                };
                task.artifacts = vec![Artifact {
                    artifact_id: Uuid::new_v4().to_string(),
                    name: "Bridge Codex result".to_string(),
                    parts: vec![TextPart { text: content }],
                }];
            }
            Ok(_) => {
                self.fail_task(
                    &task_id,
                    "CLIProxyAPI result exceeded the A2A artifact limit",
                )
                .await;
            }
            Err(error) => self.fail_task(&task_id, &error.to_string()).await,
        }
    }

    async fn fail_task(&self, task_id: &str, message: &str) {
        let mut tasks = self.tasks.write().await;
        let Some(task) = tasks.get_mut(task_id) else {
            return;
        };
        if task.status.state != TaskState::Working {
            return;
        }
        task.status = TaskStatus {
            state: TaskState::Failed,
            timestamp: now(),
            message: Some(agent_message(bounded_text(
                message,
                MAX_STATUS_MESSAGE_BYTES,
            ))),
        };
    }

    pub(super) async fn task(&self, id: &str) -> Result<A2aTask, A2aError> {
        self.tasks
            .read()
            .await
            .get(id)
            .ok_or_else(|| A2aError::NotFound(format!("A2A task `{id}` was not found")))
    }

    pub(super) async fn cancel(&self, id: &str) -> Result<A2aTask, A2aError> {
        let status = TaskStatus {
            state: TaskState::Canceled,
            timestamp: now(),
            message: Some(agent_message("Task canceled".to_string())),
        };
        let task = self
            .tasks
            .write()
            .await
            .cancel(id, status)
            .ok_or_else(|| A2aError::NotFound(format!("A2A task `{id}` was not found")))?;
        self.abort_execution(id).await;
        Ok(task)
    }

    async fn abort_execution(&self, id: &str) {
        let handle = self.executions.lock().await.remove(id);
        if let Some(handle) = handle {
            handle.abort();
            let _ = handle.await;
        }
    }

    async fn reap_finished_executions(&self) {
        self.executions
            .lock()
            .await
            .retain(|_, task| !task.is_finished());
    }

    pub(super) async fn tasks(&self) -> Vec<A2aTask> {
        self.tasks.read().await.list_newest_first()
    }

    pub async fn shutdown(&self) {
        self.shutdown_tx.send_replace(true);
        let Ok(_server_permit) = self.server_gate.clone().acquire_owned().await else {
            return;
        };
        let working = self.tasks.read().await.working_ids();
        for id in working {
            let _ = self.cancel(&id).await;
        }
        let remaining = self
            .executions
            .lock()
            .await
            .drain()
            .map(|(_, task)| task)
            .collect::<Vec<_>>();
        for task in remaining {
            task.abort();
            let _ = task.await;
        }
        let server_task = self.server_task.lock().await.take();
        if let Some(mut task) = server_task
            && tokio::time::timeout(SERVER_SHUTDOWN_TIMEOUT, &mut task)
                .await
                .is_err()
        {
            task.abort();
            let _ = task.await;
        }
        self.set_status(/*running*/ false, /*error*/ None).await;
    }
}

pub(super) fn validate_text_field(
    name: &str,
    value: &str,
    max_bytes: usize,
) -> Result<(), A2aError> {
    if value.trim().is_empty() {
        return Err(A2aError::InvalidRequest(format!("{name} cannot be empty")));
    }
    if value.len() > max_bytes {
        return Err(A2aError::InvalidRequest(format!(
            "{name} exceeds the {max_bytes}-byte limit"
        )));
    }
    Ok(())
}

fn bounded_text(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let mut end = max_bytes.saturating_sub(3);
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    format!("{}...", &value[..end])
}

fn agent_message(text: String) -> A2aMessage {
    A2aMessage {
        message_id: Uuid::new_v4().to_string(),
        role: A2aRole::Agent,
        parts: vec![TextPart { text }],
    }
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
#[path = "a2a_tests.rs"]
mod tests;
