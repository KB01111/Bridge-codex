use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use codex_app_server_protocol::ThreadReadParams;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnInterruptParams;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::UserInput;
use serde_json::Value;
use tokio::sync::broadcast;

use crate::agent_runtime::BridgeAgentRuntime;
use crate::agent_runtime_protocol::BridgeAgentEvent;
use crate::agent_runtime_protocol::BridgeAgentRequest;

type RuntimeRequestFuture<'a> = Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>>;

/// Restricted app-server boundary used by A2A delegation.
///
/// Implementations must preserve the Bridge runtime's provider, approval, and
/// sandbox policy checks and publish the same event stream shown in the Bridge UI.
pub(super) trait A2aRuntimeBackend: Send + Sync {
    fn request(&self, request: BridgeAgentRequest) -> RuntimeRequestFuture<'_>;

    fn subscribe_events(&self) -> broadcast::Receiver<BridgeAgentEvent>;
}

impl A2aRuntimeBackend for BridgeAgentRuntime {
    fn request(&self, request: BridgeAgentRequest) -> RuntimeRequestFuture<'_> {
        Box::pin(BridgeAgentRuntime::request(self, request))
    }

    fn subscribe_events(&self) -> broadcast::Receiver<BridgeAgentEvent> {
        BridgeAgentRuntime::subscribe_events(self)
    }
}

#[derive(Clone)]
pub(super) struct A2aRuntimeClient {
    backend: Arc<dyn A2aRuntimeBackend>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum A2aTurnActivity {
    Active,
    Terminal,
    Missing,
}

impl A2aRuntimeClient {
    pub(super) fn bridge(runtime: BridgeAgentRuntime) -> Self {
        Self {
            backend: Arc::new(runtime),
        }
    }

    #[cfg(test)]
    pub(super) fn with_backend(backend: Arc<dyn A2aRuntimeBackend>) -> Self {
        Self { backend }
    }

    pub(super) fn subscribe_events(&self) -> broadcast::Receiver<BridgeAgentEvent> {
        self.backend.subscribe_events()
    }

    pub(super) async fn start_or_resume_thread(
        &self,
        context_id: Option<String>,
        model: Option<String>,
    ) -> Result<String> {
        let response = match context_id {
            Some(thread_id) => {
                self.backend
                    .request(BridgeAgentRequest::ThreadResume(ThreadResumeParams {
                        thread_id,
                        model,
                        exclude_turns: true,
                        ..Default::default()
                    }))
                    .await?
            }
            None => {
                self.backend
                    .request(BridgeAgentRequest::ThreadStart(ThreadStartParams {
                        model,
                        ephemeral: Some(false),
                        ..Default::default()
                    }))
                    .await?
            }
        };
        response
            .pointer("/thread/id")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .context("embedded agent thread response did not contain a thread ID")
    }

    pub(super) async fn start_turn(
        &self,
        thread_id: String,
        task_id: String,
        prompt: String,
        model: Option<String>,
    ) -> Result<String> {
        let response = self
            .backend
            .request(BridgeAgentRequest::TurnStart(TurnStartParams {
                thread_id,
                client_user_message_id: Some(task_id),
                input: vec![UserInput::Text {
                    text: prompt,
                    text_elements: Vec::new(),
                }],
                model,
                ..Default::default()
            }))
            .await?;
        response
            .pointer("/turn/id")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .context("embedded agent turn response did not contain a turn ID")
    }

    pub(super) async fn interrupt_turn(&self, thread_id: String, turn_id: String) -> Result<()> {
        self.backend
            .request(BridgeAgentRequest::TurnInterrupt(TurnInterruptParams {
                thread_id,
                turn_id,
            }))
            .await?;
        Ok(())
    }

    pub(super) async fn turn_activity(
        &self,
        thread_id: String,
        turn_id: &str,
    ) -> Result<A2aTurnActivity> {
        let response = self
            .backend
            .request(BridgeAgentRequest::ThreadRead(ThreadReadParams {
                thread_id,
                include_turns: true,
            }))
            .await?;
        let turns = response
            .pointer("/thread/turns")
            .and_then(Value::as_array)
            .context("embedded agent thread read did not contain turns")?;
        let Some(turn) = turns
            .iter()
            .find(|turn| turn.get("id").and_then(Value::as_str) == Some(turn_id))
        else {
            return Ok(A2aTurnActivity::Missing);
        };
        match turn.get("status").and_then(Value::as_str) {
            Some("inProgress") => Ok(A2aTurnActivity::Active),
            Some("completed" | "interrupted" | "failed") => Ok(A2aTurnActivity::Terminal),
            Some(status) => anyhow::bail!("embedded agent returned unknown turn status `{status}`"),
            None => anyhow::bail!("embedded agent turn omitted its status"),
        }
    }
}
