use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use anyhow::Result;
use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::StatusCode;
use axum::middleware;
use axum::routing::get;
use axum::routing::post;
use chrono::SecondsFormat;
use chrono::Utc;
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use tokio::sync::Semaphore;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use uuid::Uuid;

#[path = "a2a_credentials.rs"]
mod credentials;
#[path = "a2a_execution.rs"]
mod execution;
#[path = "a2a_lifecycle.rs"]
mod lifecycle;
#[path = "a2a_persistence.rs"]
mod persistence;
#[path = "a2a_runtime.rs"]
mod runtime;
#[path = "a2a_security.rs"]
mod security;

use self::credentials::A2aCredentialStore;
use self::execution::A2aExecution;
use self::persistence::A2aPersistence;
use self::runtime::A2aRuntimeClient;
use self::security::A2aHttpConfig;
use self::security::A2aRateLimitState;
use self::security::MAX_A2A_BODY_BYTES;
use self::security::authorize_and_limit_request;
#[cfg(test)]
use self::security::constant_time_eq;
#[cfg(test)]
use self::security::validate_a2a_token;

use crate::a2a_handlers::agent_card;
use crate::a2a_handlers::cancel_task;
use crate::a2a_handlers::delegate_task;
use crate::a2a_handlers::get_task;
use crate::a2a_handlers::list_tasks;
use crate::a2a_handlers::send_message;
use crate::a2a_protocol::A2aMessage;
use crate::a2a_protocol::A2aRole;
use crate::a2a_protocol::A2aServerSettings;
use crate::a2a_protocol::TextPart;
use crate::a2a_task_store::TaskStore;

pub use crate::a2a_protocol::A2aServerStatus;
const MAX_A2A_CONCURRENT_TASKS: usize = 4;
const MAX_ARTIFACT_BYTES: usize = 64 * 1024;
const MAX_CONTEXT_ID_BYTES: usize = 256;
pub(super) const MAX_MESSAGE_ID_BYTES: usize = 256;
pub(super) const MAX_MESSAGE_PARTS: usize = 32;
const MAX_MODEL_ID_BYTES: usize = 256;
pub(super) const MAX_PROMPT_BYTES: usize = 32 * 1024;
const MAX_STATUS_MESSAGE_BYTES: usize = 4 * 1024;

#[derive(Debug)]
pub(super) enum A2aError {
    InvalidRequest(String),
    Upstream(String),
    NotFound(String),
    Internal(String),
}

impl std::fmt::Display for A2aError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest(message)
            | Self::Upstream(message)
            | Self::NotFound(message)
            | Self::Internal(message) => formatter.write_str(message),
        }
    }
}

impl A2aError {
    pub(super) fn into_http(self) -> (StatusCode, String) {
        let status = match &self {
            Self::InvalidRequest(_) => StatusCode::BAD_REQUEST,
            Self::Upstream(_) => StatusCode::BAD_GATEWAY,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, self.to_string())
    }
}

#[derive(Clone)]
pub struct A2aServer {
    runtime: A2aRuntimeClient,
    tasks: Arc<RwLock<TaskStore>>,
    task_mutation_gate: Arc<Semaphore>,
    status: Arc<RwLock<A2aServerStatus>>,
    execution_gate: Arc<Semaphore>,
    executions: Arc<Mutex<HashMap<String, A2aExecution>>>,
    settings: Arc<RwLock<A2aServerSettings>>,
    token: Arc<RwLock<Option<String>>>,
    credentials: A2aCredentialStore,
    persistence: Option<A2aPersistence>,
    http_config: Arc<A2aHttpConfig>,
    request_gate: Arc<Semaphore>,
    rate_limit: Arc<Mutex<A2aRateLimitState>>,
    server_gate: Arc<Semaphore>,
    server_task: Arc<Mutex<Option<JoinHandle<()>>>>,
    stop_generation: watch::Sender<u64>,
    terminated: Arc<AtomicBool>,
}

impl A2aServer {
    fn router(self) -> Router {
        let middleware_state = self.clone();
        Router::new()
            .route("/.well-known/agent-card.json", get(agent_card))
            .route("/a2a/message:send", post(send_message))
            .route("/a2a/tasks", get(list_tasks).post(delegate_task))
            .route("/a2a/tasks/{id}", get(get_task).post(cancel_task))
            .layer(DefaultBodyLimit::max(MAX_A2A_BODY_BYTES))
            .layer(middleware::from_fn_with_state(
                middleware_state,
                authorize_and_limit_request,
            ))
            .with_state(self)
    }

    pub async fn shutdown(&self) {
        self.terminated.store(true, Ordering::Release);
        let Ok(_server_permit) = self.server_gate.clone().acquire_owned().await else {
            return;
        };
        let Ok(publication_permit) = self.task_mutation_gate.clone().acquire_owned().await else {
            return;
        };
        let working = self.tasks.read().await.working_ids();
        drop(publication_permit);
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
        for execution in remaining {
            execution.task.abort();
            let _ = execution.task.await;
        }
        self.stop_listener_locked().await;
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
