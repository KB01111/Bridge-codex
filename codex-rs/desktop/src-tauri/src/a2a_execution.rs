use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tokio::sync::OwnedSemaphorePermit;
use tokio::sync::Semaphore;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use uuid::Uuid;

use super::A2aError;
use super::A2aServer;
use super::MAX_ARTIFACT_BYTES;
use super::MAX_CONTEXT_ID_BYTES;
use super::MAX_MODEL_ID_BYTES;
use super::MAX_PROMPT_BYTES;
use super::MAX_STATUS_MESSAGE_BYTES;
use super::agent_message;
use super::bounded_text;
use super::now;
use super::runtime::A2aTurnActivity;
use super::validate_text_field;
use crate::a2a_protocol::A2aTask;
use crate::a2a_protocol::Artifact;
use crate::a2a_protocol::TaskState;
use crate::a2a_protocol::TaskStatus;
use crate::a2a_protocol::TextPart;
use crate::a2a_task_store::TaskStore;
use crate::agent_runtime_protocol::BridgeAgentEvent;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnStatus;

const MAX_TURN_ID_BYTES: usize = 256;
const INTERRUPT_RETRY_DELAY: Duration = Duration::from_millis(100);

enum TurnStopOutcome {
    Stopped,
    TerminalAfterError(String),
    MissingAfterError(String),
    RuntimeUnavailable {
        interrupt_error: String,
        status_error: String,
    },
    ShuttingDown,
}

enum InterruptAttempt {
    Succeeded,
    Failed(String),
    ShuttingDown,
}

#[derive(Clone, Copy)]
enum TurnCleanupPolicy {
    RequireConfirmation,
    ReleaseIfRuntimeUnavailable,
}

pub(super) struct A2aExecution {
    pub(super) thread_id: String,
    pub(super) turn_id: String,
    pub(super) task: JoinHandle<()>,
    cancel_gate: Arc<Semaphore>,
}

impl A2aServer {
    pub(crate) async fn enqueue(
        &self,
        prompt: String,
        model: Option<String>,
        context_id: Option<String>,
    ) -> Result<A2aTask, A2aError> {
        validate_text_field("A2A task prompt", &prompt, MAX_PROMPT_BYTES)?;
        let model = model.filter(|model| !model.trim().is_empty());
        if let Some(model) = &model {
            validate_text_field("A2A model ID", model, MAX_MODEL_ID_BYTES)?;
        }
        if let Some(context_id) = &context_id {
            validate_text_field("A2A context ID", context_id, MAX_CONTEXT_ID_BYTES)?;
        }
        if self.terminated.load(Ordering::Acquire) {
            return Err(A2aError::Internal(
                "A2A server is shutting down".to_string(),
            ));
        }

        let permit = self
            .execution_gate
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| A2aError::Internal("A2A execution gate was closed".to_string()))?;
        let task_id = Uuid::new_v4().to_string();
        let context_id = self
            .runtime
            .start_or_resume_thread(context_id, model.clone())
            .await
            .map_err(|error| A2aError::Upstream(error.to_string()))?;
        validate_text_field(
            "embedded agent thread ID",
            &context_id,
            MAX_CONTEXT_ID_BYTES,
        )?;
        let events = self.runtime.subscribe_events();
        let turn_id = self
            .runtime
            .start_turn(context_id.clone(), task_id.clone(), prompt, model)
            .await
            .map_err(|error| A2aError::Upstream(error.to_string()))?;
        let mutation_permit = match self.acquire_task_mutation_permit().await {
            Ok(permit) => permit,
            Err(error) => {
                let _ = self
                    .ensure_turn_inactive(
                        &task_id,
                        &context_id,
                        &turn_id,
                        TurnCleanupPolicy::RequireConfirmation,
                    )
                    .await;
                return Err(error);
            }
        };
        if self.terminated.load(Ordering::Acquire) {
            let _ = self
                .ensure_turn_inactive(
                    &task_id,
                    &context_id,
                    &turn_id,
                    TurnCleanupPolicy::RequireConfirmation,
                )
                .await;
            return Err(A2aError::Internal(
                "A2A server is shutting down".to_string(),
            ));
        }
        if let Err(error) =
            validate_text_field("embedded agent turn ID", &turn_id, MAX_TURN_ID_BYTES)
        {
            let _ = self
                .ensure_turn_inactive(
                    &task_id,
                    &context_id,
                    &turn_id,
                    TurnCleanupPolicy::RequireConfirmation,
                )
                .await;
            return Err(error);
        }

        let task = A2aTask {
            id: task_id.clone(),
            context_id: context_id.clone(),
            status: TaskStatus {
                state: TaskState::Working,
                timestamp: now(),
                message: None,
            },
            artifacts: Vec::new(),
        };
        let mut tasks = {
            let tasks = self.tasks.read().await;
            tasks.clone()
        };
        if tasks.insert(task.clone()).is_err() {
            let _ = self
                .ensure_turn_inactive(
                    &task_id,
                    &context_id,
                    &turn_id,
                    TurnCleanupPolicy::RequireConfirmation,
                )
                .await;
            return Err(A2aError::Internal(
                "A2A task history is full of active tasks".to_string(),
            ));
        }
        if let Err(error) = self.persist_task_store(&tasks).await {
            let _ = self
                .ensure_turn_inactive(
                    &task_id,
                    &context_id,
                    &turn_id,
                    TurnCleanupPolicy::RequireConfirmation,
                )
                .await;
            return Err(error);
        }
        *self.tasks.write().await = tasks;
        self.spawn_execution(task_id, context_id, turn_id, events, permit)
            .await;
        drop(mutation_permit);
        Ok(task)
    }

    async fn acquire_task_mutation_permit(&self) -> Result<OwnedSemaphorePermit, A2aError> {
        self.task_mutation_gate
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| A2aError::Internal("A2A task mutation gate was closed".to_string()))
    }

    async fn persist_task_store(&self, tasks: &TaskStore) -> Result<(), A2aError> {
        let Some(persistence) = &self.persistence else {
            return Ok(());
        };
        persistence
            .save_tasks(tasks.list_oldest_first())
            .await
            .map_err(|error| {
                A2aError::Internal(format!("failed to persist A2A task state: {error}"))
            })
    }

    async fn spawn_execution(
        &self,
        task_id: String,
        thread_id: String,
        turn_id: String,
        events: broadcast::Receiver<BridgeAgentEvent>,
        permit: OwnedSemaphorePermit,
    ) {
        self.reap_finished_executions().await;
        let server = self.clone();
        let task_id_for_job = task_id.clone();
        let thread_id_for_job = thread_id.clone();
        let turn_id_for_job = turn_id.clone();
        let task = tokio::spawn(async move {
            server
                .watch_execution(
                    &task_id_for_job,
                    &thread_id_for_job,
                    &turn_id_for_job,
                    events,
                    permit,
                )
                .await;
        });
        self.executions.lock().await.insert(
            task_id,
            A2aExecution {
                thread_id,
                turn_id,
                task,
                cancel_gate: Arc::new(Semaphore::new(1)),
            },
        );
    }

    async fn watch_execution(
        &self,
        task_id: &str,
        thread_id: &str,
        turn_id: &str,
        mut events: broadcast::Receiver<BridgeAgentEvent>,
        _permit: OwnedSemaphorePermit,
    ) {
        loop {
            match events.recv().await {
                Ok(BridgeAgentEvent::ServerNotification { payload }) => {
                    let Ok(ServerNotification::TurnCompleted(notification)) =
                        serde_json::from_value(payload)
                    else {
                        continue;
                    };
                    if notification.thread_id != thread_id || notification.turn.id != turn_id {
                        continue;
                    }
                    self.finish_turn(task_id, notification).await;
                    return;
                }
                Ok(BridgeAgentEvent::ServerRequest { .. }) => {
                    // The Bridge UI owns approval resolution. The task deliberately remains
                    // Working until the corresponding turn/completed notification arrives.
                }
                Ok(BridgeAgentEvent::Lagged { skipped }) => {
                    self.interrupt_then_fail(
                        task_id,
                        thread_id,
                        turn_id,
                        &format!("embedded agent event stream skipped {skipped} events"),
                    )
                    .await;
                    return;
                }
                Ok(BridgeAgentEvent::Disconnected { message }) => {
                    self.fail_task(task_id, &message).await;
                    return;
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    self.interrupt_then_fail(
                        task_id,
                        thread_id,
                        turn_id,
                        &format!("A2A event subscriber skipped {skipped} events"),
                    )
                    .await;
                    return;
                }
                Err(broadcast::error::RecvError::Closed) => {
                    self.fail_task(task_id, "embedded agent event stream closed")
                        .await;
                    return;
                }
            }
        }
    }

    async fn interrupt_then_fail(
        &self,
        task_id: &str,
        thread_id: &str,
        turn_id: &str,
        message: &str,
    ) {
        let message = match self
            .ensure_turn_inactive(
                task_id,
                thread_id,
                turn_id,
                TurnCleanupPolicy::ReleaseIfRuntimeUnavailable,
            )
            .await
        {
            TurnStopOutcome::Stopped => message.to_string(),
            TurnStopOutcome::TerminalAfterError(error) => format!(
                "{message}; the skipped agent turn completion was confirmed after interrupt failed: {error}"
            ),
            TurnStopOutcome::MissingAfterError(error) => format!(
                "{message}; the agent turn no longer exists after interrupt failed: {error}"
            ),
            TurnStopOutcome::RuntimeUnavailable {
                interrupt_error,
                status_error,
            } => format!(
                "{message}; agent runtime became unavailable during lag recovery: {status_error}; interrupt failed: {interrupt_error}"
            ),
            TurnStopOutcome::ShuttingDown => return,
        };
        self.fail_task(task_id, &message).await;
    }

    async fn ensure_turn_inactive(
        &self,
        task_id: &str,
        thread_id: &str,
        turn_id: &str,
        policy: TurnCleanupPolicy,
    ) -> TurnStopOutcome {
        loop {
            match self
                .interrupt_turn_or_shutdown(thread_id.to_string(), turn_id.to_string())
                .await
            {
                InterruptAttempt::Succeeded => return TurnStopOutcome::Stopped,
                InterruptAttempt::ShuttingDown => return TurnStopOutcome::ShuttingDown,
                InterruptAttempt::Failed(error) => {
                    if self.terminated.load(Ordering::Acquire) {
                        // The shutdown publication barrier makes this turn invisible to new
                        // callers, and application shutdown forcibly stops the embedded runtime
                        // immediately after A2A exits. Do not deadlock that final stop.
                        return TurnStopOutcome::ShuttingDown;
                    }
                    match self
                        .runtime
                        .turn_activity(thread_id.to_string(), turn_id)
                        .await
                    {
                        Ok(A2aTurnActivity::Active) => {
                            tracing::warn!(
                                %error,
                                task_id,
                                "retaining A2A capacity until the confirmed-active turn is interrupted"
                            );
                            tokio::time::sleep(INTERRUPT_RETRY_DELAY).await;
                        }
                        Ok(A2aTurnActivity::Terminal) => {
                            return TurnStopOutcome::TerminalAfterError(error);
                        }
                        Ok(A2aTurnActivity::Missing) => {
                            return TurnStopOutcome::MissingAfterError(error);
                        }
                        Err(status_error) => match policy {
                            TurnCleanupPolicy::RequireConfirmation => {
                                tracing::warn!(
                                    %error,
                                    %status_error,
                                    task_id,
                                    "retaining A2A capacity until an unpublished turn is confirmed inactive"
                                );
                                tokio::time::sleep(INTERRUPT_RETRY_DELAY).await;
                            }
                            TurnCleanupPolicy::ReleaseIfRuntimeUnavailable => {
                                return TurnStopOutcome::RuntimeUnavailable {
                                    interrupt_error: error,
                                    status_error: status_error.to_string(),
                                };
                            }
                        },
                    }
                }
            }
        }
    }

    async fn finish_turn(&self, task_id: &str, notification: TurnCompletedNotification) {
        match notification.turn.status {
            TurnStatus::Completed => {
                let content =
                    notification
                        .turn
                        .items
                        .into_iter()
                        .rev()
                        .find_map(|item| match item {
                            ThreadItem::AgentMessage { text, .. } if !text.trim().is_empty() => {
                                Some(text)
                            }
                            _ => None,
                        });
                match content {
                    Some(content) if content.len() <= MAX_ARTIFACT_BYTES => {
                        self.complete_task(task_id, content).await;
                    }
                    Some(_) => {
                        self.fail_task(
                            task_id,
                            "embedded agent result exceeded the A2A artifact limit",
                        )
                        .await;
                    }
                    None => {
                        self.fail_task(
                            task_id,
                            "embedded agent turn completed without a final message",
                        )
                        .await;
                    }
                }
            }
            TurnStatus::Interrupted => {
                self.cancel_from_runtime(task_id).await;
            }
            TurnStatus::Failed => {
                let message = notification
                    .turn
                    .error
                    .map(|error| error.message)
                    .unwrap_or_else(|| "embedded agent turn failed".to_string());
                self.fail_task(task_id, &message).await;
            }
            TurnStatus::InProgress => {
                self.fail_task(
                    task_id,
                    "embedded agent emitted turn/completed with an in-progress status",
                )
                .await;
            }
        }
    }

    async fn complete_task(&self, task_id: &str, content: String) {
        let Ok(_mutation_permit) = self.acquire_task_mutation_permit().await else {
            return;
        };
        let mut tasks = {
            let tasks = self.tasks.read().await;
            tasks.clone()
        };
        let Some(task) = tasks.get_mut(task_id) else {
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
        let persistence_error = self.persist_task_store(&tasks).await.err();
        *self.tasks.write().await = tasks;
        if let Some(error) = persistence_error {
            self.report_persistence_error(error).await;
        }
    }

    async fn fail_task(&self, task_id: &str, message: &str) {
        let Ok(_mutation_permit) = self.acquire_task_mutation_permit().await else {
            return;
        };
        let mut tasks = {
            let tasks = self.tasks.read().await;
            tasks.clone()
        };
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
        let persistence_error = self.persist_task_store(&tasks).await.err();
        *self.tasks.write().await = tasks;
        if let Some(error) = persistence_error {
            self.report_persistence_error(error).await;
        }
    }

    async fn cancel_from_runtime(&self, task_id: &str) {
        let Ok(_mutation_permit) = self.acquire_task_mutation_permit().await else {
            return;
        };
        let mut tasks = {
            let tasks = self.tasks.read().await;
            tasks.clone()
        };
        let Some(task) = tasks.get_mut(task_id) else {
            return;
        };
        if task.status.state != TaskState::Working {
            return;
        }
        task.status = TaskStatus {
            state: TaskState::Canceled,
            timestamp: now(),
            message: Some(agent_message("Agent turn interrupted".to_string())),
        };
        let persistence_error = self.persist_task_store(&tasks).await.err();
        *self.tasks.write().await = tasks;
        if let Some(error) = persistence_error {
            self.report_persistence_error(error).await;
        }
    }

    async fn report_persistence_error(&self, error: A2aError) {
        tracing::warn!(%error, "failed to persist A2A task state");
        let running = self.status.read().await.running;
        self.set_status(running, Some(error.to_string())).await;
    }

    pub(crate) async fn task(&self, id: &str) -> Result<A2aTask, A2aError> {
        self.tasks
            .read()
            .await
            .get(id)
            .ok_or_else(|| A2aError::NotFound(format!("A2A task `{id}` was not found")))
    }

    pub(crate) async fn cancel(&self, id: &str) -> Result<A2aTask, A2aError> {
        let current = self.task(id).await?;
        if current.status.state != TaskState::Working {
            return Ok(current);
        }
        let cancel_gate = {
            let executions = self.executions.lock().await;
            executions
                .get(id)
                .map(|execution| execution.cancel_gate.clone())
        };
        let Some(cancel_gate) = cancel_gate else {
            let current = self.task(id).await?;
            if current.status.state != TaskState::Working {
                return Ok(current);
            }
            return Err(A2aError::Internal(format!(
                "active A2A execution `{id}` was not found"
            )));
        };
        let _cancel_permit = cancel_gate
            .acquire_owned()
            .await
            .map_err(|_| A2aError::Internal("A2A cancellation gate was closed".to_string()))?;
        let current = self.task(id).await?;
        if current.status.state != TaskState::Working {
            return Ok(current);
        }
        let interrupt_result = self.interrupt_execution(id).await;
        let _mutation_permit = self.acquire_task_mutation_permit().await?;
        let current = self.task(id).await?;
        if current.status.state != TaskState::Working {
            drop(_mutation_permit);
            self.remove_execution(id).await;
            return Ok(current);
        }
        interrupt_result?;
        let status = TaskStatus {
            state: TaskState::Canceled,
            timestamp: now(),
            message: Some(agent_message("Task canceled".to_string())),
        };
        let mut tasks = {
            let tasks = self.tasks.read().await;
            tasks.clone()
        };
        let task = tasks
            .cancel(id, status)
            .ok_or_else(|| A2aError::NotFound(format!("A2A task `{id}` was not found")))?;
        let persistence_result = self.persist_task_store(&tasks).await;
        *self.tasks.write().await = tasks;
        drop(_mutation_permit);
        self.remove_execution(id).await;
        persistence_result?;
        Ok(task)
    }

    async fn interrupt_execution(&self, id: &str) -> Result<(), A2aError> {
        let execution = self
            .executions
            .lock()
            .await
            .get(id)
            .map(|execution| (execution.thread_id.clone(), execution.turn_id.clone()));
        let Some((thread_id, turn_id)) = execution else {
            return Ok(());
        };
        match self.interrupt_turn_or_shutdown(thread_id, turn_id).await {
            InterruptAttempt::Succeeded => Ok(()),
            InterruptAttempt::Failed(error) => Err(A2aError::Upstream(error)),
            InterruptAttempt::ShuttingDown => Err(A2aError::Upstream(
                "A2A server is shutting down".to_string(),
            )),
        }
    }

    async fn interrupt_turn_or_shutdown(
        &self,
        thread_id: String,
        turn_id: String,
    ) -> InterruptAttempt {
        tokio::select! {
            biased;
            _ = self.wait_for_termination() => InterruptAttempt::ShuttingDown,
            result = self.runtime.interrupt_turn(thread_id, turn_id) => match result {
                Ok(()) => InterruptAttempt::Succeeded,
                Err(error) => InterruptAttempt::Failed(error.to_string()),
            },
        }
    }

    async fn wait_for_termination(&self) {
        while !self.terminated.load(Ordering::Acquire) {
            tokio::time::sleep(INTERRUPT_RETRY_DELAY).await;
        }
    }

    async fn remove_execution(&self, id: &str) {
        let execution = self.executions.lock().await.remove(id);
        if let Some(execution) = execution {
            execution.task.abort();
            let _ = execution.task.await;
        }
    }

    async fn reap_finished_executions(&self) {
        self.executions
            .lock()
            .await
            .retain(|_, execution| !execution.task.is_finished());
    }

    pub(crate) async fn tasks(&self) -> Vec<A2aTask> {
        self.tasks.read().await.list_newest_first()
    }
}
