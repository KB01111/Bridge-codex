use axum::Json;
use axum::extract::Path;
use axum::extract::State;
use axum::http::StatusCode;
use tauri::State as TauriState;

use crate::a2a::A2aError;
use crate::a2a::A2aServer;
use crate::a2a::MAX_MESSAGE_ID_BYTES;
use crate::a2a::MAX_MESSAGE_PARTS;
use crate::a2a::MAX_PROMPT_BYTES;
use crate::a2a::validate_text_field;
use crate::a2a_protocol::A2aJson;
use crate::a2a_protocol::A2aMessage;
use crate::a2a_protocol::A2aRole;
use crate::a2a_protocol::A2aServerStatus;
use crate::a2a_protocol::A2aTask;
use crate::a2a_protocol::DelegateTaskRequest;
use crate::a2a_protocol::SendMessageRequest;
use crate::a2a_protocol::SendMessageResponse;

pub(super) async fn send_message(
    State(server): State<A2aServer>,
    Json(request): Json<SendMessageRequest>,
) -> Result<A2aJson<SendMessageResponse>, (StatusCode, String)> {
    let prompt = prompt_from_message(request.message).map_err(A2aError::into_http)?;
    let task = server
        .enqueue(prompt, request.model, /*context_id*/ None)
        .await
        .map_err(A2aError::into_http)?;
    Ok(A2aJson(SendMessageResponse { task }))
}

pub(super) async fn delegate_task(
    State(server): State<A2aServer>,
    Json(request): Json<DelegateTaskRequest>,
) -> Result<A2aJson<A2aTask>, (StatusCode, String)> {
    server
        .enqueue(request.prompt, request.model, request.context_id)
        .await
        .map(A2aJson)
        .map_err(A2aError::into_http)
}

pub(super) async fn list_tasks(State(server): State<A2aServer>) -> A2aJson<Vec<A2aTask>> {
    A2aJson(server.tasks().await)
}

pub(super) async fn get_task(
    State(server): State<A2aServer>,
    Path(id): Path<String>,
) -> Result<A2aJson<A2aTask>, (StatusCode, String)> {
    server
        .task(&id)
        .await
        .map(A2aJson)
        .map_err(A2aError::into_http)
}

#[tauri::command]
pub async fn list_a2a_tasks(state: TauriState<'_, A2aServer>) -> Result<Vec<A2aTask>, String> {
    Ok(state.tasks().await)
}

#[tauri::command]
pub async fn get_a2a_status(state: TauriState<'_, A2aServer>) -> Result<A2aServerStatus, String> {
    Ok(state.status().await)
}

#[tauri::command]
pub async fn delegate_a2a_task(
    state: TauriState<'_, A2aServer>,
    prompt: String,
    model: Option<String>,
    context_id: Option<String>,
) -> Result<A2aTask, String> {
    state
        .enqueue(prompt, model, context_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn get_a2a_task(state: TauriState<'_, A2aServer>, id: String) -> Result<A2aTask, String> {
    state.task(&id).await.map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn cancel_a2a_task(
    state: TauriState<'_, A2aServer>,
    id: String,
) -> Result<A2aTask, String> {
    state.cancel(&id).await.map_err(|error| error.to_string())
}

fn prompt_from_message(message: A2aMessage) -> Result<String, A2aError> {
    if message.role != A2aRole::User {
        return Err(A2aError::InvalidRequest(
            "A2A message must use ROLE_USER".to_string(),
        ));
    }
    validate_text_field("A2A message ID", &message.message_id, MAX_MESSAGE_ID_BYTES)?;
    if message.parts.is_empty() || message.parts.len() > MAX_MESSAGE_PARTS {
        return Err(A2aError::InvalidRequest(format!(
            "A2A message must contain between 1 and {MAX_MESSAGE_PARTS} text parts"
        )));
    }
    let total_bytes = message.parts.iter().try_fold(0usize, |total, part| {
        total
            .checked_add(part.text.len())
            .ok_or_else(|| A2aError::InvalidRequest("A2A message size overflow".to_string()))
    })?;
    if total_bytes > MAX_PROMPT_BYTES {
        return Err(A2aError::InvalidRequest(format!(
            "A2A message exceeds the {MAX_PROMPT_BYTES}-byte prompt limit"
        )));
    }
    Ok(message
        .parts
        .into_iter()
        .map(|part| part.text)
        .collect::<Vec<_>>()
        .join("\n"))
}
