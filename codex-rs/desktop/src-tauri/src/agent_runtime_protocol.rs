use anyhow::Result;
use anyhow::bail;
use codex_app_server_protocol::ApprovalsReviewer;
use codex_app_server_protocol::AskForApproval;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ModelListParams;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::SandboxMode;
use codex_app_server_protocol::SandboxPolicy;
use codex_app_server_protocol::ThreadArchiveParams;
use codex_app_server_protocol::ThreadDeleteParams;
use codex_app_server_protocol::ThreadForkParams;
use codex_app_server_protocol::ThreadListParams;
use codex_app_server_protocol::ThreadReadParams;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadSetNameParams;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadUnarchiveParams;
use codex_app_server_protocol::TurnInterruptParams;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnSteerParams;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

use crate::agent_runtime_tools::bridge_dynamic_tools;

pub(crate) const PROVIDER_ID: &str = "bridge_cliproxy";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeAgentRuntimeStatus {
    pub running: bool,
    pub authenticated: bool,
    pub responses_api: bool,
    pub provider_base_url: Option<String>,
    pub probed_model: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum BridgeAgentEvent {
    ServerNotification { payload: Value },
    ServerRequest { payload: Value },
    Lagged { skipped: usize },
    Disconnected { message: String },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgePendingServerRequest {
    pub request_id: RequestId,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "method", content = "params")]
pub enum BridgeAgentRequest {
    #[serde(rename = "thread/list")]
    ThreadList(ThreadListParams),
    #[serde(rename = "thread/read")]
    ThreadRead(ThreadReadParams),
    #[serde(rename = "thread/start")]
    ThreadStart(ThreadStartParams),
    #[serde(rename = "thread/resume")]
    ThreadResume(ThreadResumeParams),
    #[serde(rename = "thread/fork")]
    ThreadFork(ThreadForkParams),
    #[serde(rename = "thread/archive")]
    ThreadArchive(ThreadArchiveParams),
    #[serde(rename = "thread/unarchive")]
    ThreadUnarchive(ThreadUnarchiveParams),
    #[serde(rename = "thread/delete")]
    ThreadDelete(ThreadDeleteParams),
    #[serde(rename = "thread/name/set")]
    ThreadSetName(ThreadSetNameParams),
    #[serde(rename = "turn/start")]
    TurnStart(TurnStartParams),
    #[serde(rename = "turn/steer")]
    TurnSteer(TurnSteerParams),
    #[serde(rename = "turn/interrupt")]
    TurnInterrupt(TurnInterruptParams),
    #[serde(rename = "model/list")]
    ModelList(ModelListParams),
}

impl BridgeAgentRequest {
    pub(crate) fn into_client_request(
        self,
        request_id: RequestId,
        gateway_model: &str,
    ) -> Result<ClientRequest> {
        Ok(match self {
            Self::ThreadList(mut params) => {
                match &params.model_providers {
                    Some(providers) if providers.iter().any(|provider| provider != PROVIDER_ID) => {
                        bail!("thread/list is restricted to the Bridge CLIProxyAPI provider")
                    }
                    Some(_) => {}
                    None => params.model_providers = Some(vec![PROVIDER_ID.to_string()]),
                }
                ClientRequest::ThreadList { request_id, params }
            }
            Self::ThreadRead(params) => ClientRequest::ThreadRead { request_id, params },
            Self::ThreadStart(mut params) => {
                enforce_provider(&mut params.model_provider)?;
                enforce_model(&mut params.model, gateway_model)?;
                if params.allow_provider_model_fallback {
                    bail!("agent threads do not allow provider model fallback");
                }
                enforce_thread_security(
                    &mut params.approval_policy,
                    &mut params.approvals_reviewer,
                    &mut params.sandbox,
                    &params.permissions,
                )?;
                reject_config_overrides(&params.config)?;
                params.dynamic_tools = Some(bridge_dynamic_tools());
                ClientRequest::ThreadStart { request_id, params }
            }
            Self::ThreadResume(mut params) => {
                enforce_provider(&mut params.model_provider)?;
                enforce_model(&mut params.model, gateway_model)?;
                enforce_thread_security(
                    &mut params.approval_policy,
                    &mut params.approvals_reviewer,
                    &mut params.sandbox,
                    &params.permissions,
                )?;
                reject_config_overrides(&params.config)?;
                ClientRequest::ThreadResume { request_id, params }
            }
            Self::ThreadFork(mut params) => {
                enforce_provider(&mut params.model_provider)?;
                enforce_model(&mut params.model, gateway_model)?;
                enforce_thread_security(
                    &mut params.approval_policy,
                    &mut params.approvals_reviewer,
                    &mut params.sandbox,
                    &params.permissions,
                )?;
                reject_config_overrides(&params.config)?;
                ClientRequest::ThreadFork { request_id, params }
            }
            Self::ThreadArchive(params) => ClientRequest::ThreadArchive { request_id, params },
            Self::ThreadUnarchive(params) => ClientRequest::ThreadUnarchive { request_id, params },
            Self::ThreadDelete(params) => ClientRequest::ThreadDelete { request_id, params },
            Self::ThreadSetName(params) => ClientRequest::ThreadSetName { request_id, params },
            Self::TurnStart(mut params) => {
                enforce_model(&mut params.model, gateway_model)?;
                enforce_turn_security(&mut params)?;
                ClientRequest::TurnStart { request_id, params }
            }
            Self::TurnSteer(params) => ClientRequest::TurnSteer { request_id, params },
            Self::TurnInterrupt(params) => ClientRequest::TurnInterrupt { request_id, params },
            Self::ModelList(params) => ClientRequest::ModelList { request_id, params },
        })
    }
}

fn enforce_model(model: &mut Option<String>, gateway_model: &str) -> Result<()> {
    if model.as_deref().is_some_and(|model| model != gateway_model) {
        bail!("agent requests are restricted to the conformance-tested gateway model");
    }
    *model = Some(gateway_model.to_string());
    Ok(())
}

fn enforce_provider(model_provider: &mut Option<String>) -> Result<()> {
    if model_provider
        .as_deref()
        .is_some_and(|provider| provider != PROVIDER_ID)
    {
        bail!("agent requests are restricted to the Bridge CLIProxyAPI provider");
    }
    *model_provider = Some(PROVIDER_ID.to_string());
    Ok(())
}

fn enforce_thread_security(
    approval_policy: &mut Option<AskForApproval>,
    approvals_reviewer: &mut Option<ApprovalsReviewer>,
    sandbox: &mut Option<SandboxMode>,
    permissions: &Option<String>,
) -> Result<()> {
    match approval_policy {
        Some(AskForApproval::OnRequest) => {}
        Some(
            AskForApproval::UnlessTrusted | AskForApproval::Granular { .. } | AskForApproval::Never,
        ) => bail!("agent threads require approvalPolicy=on-request"),
        None => *approval_policy = Some(AskForApproval::OnRequest),
    }
    match approvals_reviewer {
        Some(ApprovalsReviewer::User) => {}
        Some(ApprovalsReviewer::AutoReview) => {
            bail!("agent threads require approvalsReviewer=user")
        }
        None => *approvals_reviewer = Some(ApprovalsReviewer::User),
    }
    match sandbox {
        Some(SandboxMode::ReadOnly | SandboxMode::WorkspaceWrite) => {}
        Some(SandboxMode::DangerFullAccess) => {
            bail!("agent threads do not allow danger-full-access")
        }
        None => *sandbox = Some(SandboxMode::WorkspaceWrite),
    }
    if permissions.is_some() {
        bail!("named permission profiles are not accepted by the agent boundary");
    }
    Ok(())
}

fn reject_config_overrides(
    config: &Option<std::collections::HashMap<String, Value>>,
) -> Result<()> {
    if config.as_ref().is_some_and(|config| !config.is_empty()) {
        bail!("thread config overrides are not accepted by the agent boundary");
    }
    Ok(())
}

fn enforce_turn_security(params: &mut TurnStartParams) -> Result<()> {
    match params.approval_policy {
        Some(AskForApproval::OnRequest) => {}
        Some(
            AskForApproval::UnlessTrusted | AskForApproval::Granular { .. } | AskForApproval::Never,
        ) => bail!("agent turns require approvalPolicy=on-request"),
        None => params.approval_policy = Some(AskForApproval::OnRequest),
    }
    match params.approvals_reviewer {
        Some(ApprovalsReviewer::User) => {}
        Some(ApprovalsReviewer::AutoReview) => {
            bail!("agent turns require approvalsReviewer=user")
        }
        None => params.approvals_reviewer = Some(ApprovalsReviewer::User),
    }
    match &params.sandbox_policy {
        Some(SandboxPolicy::DangerFullAccess) => {
            bail!("agent turns do not allow dangerFullAccess")
        }
        Some(SandboxPolicy::ReadOnly {
            network_access: true,
        })
        | Some(SandboxPolicy::WorkspaceWrite {
            network_access: true,
            ..
        }) => bail!("agent turns do not allow networkAccess=true"),
        Some(SandboxPolicy::ExternalSandbox { .. }) => {
            bail!("agent turns do not allow externalSandbox")
        }
        Some(
            SandboxPolicy::ReadOnly {
                network_access: false,
            }
            | SandboxPolicy::WorkspaceWrite {
                network_access: false,
                ..
            },
        ) => {}
        None => {
            params.sandbox_policy = Some(SandboxPolicy::WorkspaceWrite {
                writable_roots: Vec::new(),
                network_access: false,
                exclude_tmpdir_env_var: false,
                exclude_slash_tmp: false,
            });
        }
    }
    if params.permissions.is_some() {
        bail!("named permission profiles are not accepted by the agent boundary");
    }
    Ok(())
}
