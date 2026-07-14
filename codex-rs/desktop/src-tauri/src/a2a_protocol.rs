use axum::Json;
use axum::http::HeaderValue;
use axum::http::header::CONTENT_TYPE;
use axum::response::IntoResponse;
use axum::response::Response;
use serde::Deserialize;
use serde::Serialize;

const A2A_BASE_URL: &str = "http://127.0.0.1:8120/a2a";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AgentCard {
    name: String,
    description: String,
    supported_interfaces: Vec<AgentInterface>,
    provider: AgentProvider,
    version: String,
    capabilities: AgentCapabilities,
    default_input_modes: Vec<String>,
    default_output_modes: Vec<String>,
    skills: Vec<AgentSkill>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentInterface {
    url: String,
    protocol_binding: String,
    protocol_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct AgentProvider {
    organization: String,
    url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentCapabilities {
    streaming: bool,
    push_notifications: bool,
    extended_agent_card: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentSkill {
    id: String,
    name: String,
    description: String,
    tags: Vec<String>,
    examples: Vec<String>,
    input_modes: Vec<String>,
    output_modes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TaskState {
    #[serde(rename = "TASK_STATE_WORKING")]
    Working,
    #[serde(rename = "TASK_STATE_COMPLETED")]
    Completed,
    #[serde(rename = "TASK_STATE_FAILED")]
    Failed,
    #[serde(rename = "TASK_STATE_CANCELED")]
    Canceled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct A2aTask {
    pub id: String,
    pub context_id: String,
    pub status: TaskStatus,
    pub artifacts: Vec<Artifact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStatus {
    pub state: TaskState,
    pub timestamp: String,
    pub message: Option<A2aMessage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct A2aMessage {
    pub message_id: String,
    pub role: A2aRole,
    pub parts: Vec<TextPart>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum A2aRole {
    #[serde(rename = "ROLE_USER")]
    User,
    #[serde(rename = "ROLE_AGENT")]
    Agent,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct TextPart {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub artifact_id: String,
    pub name: String,
    pub parts: Vec<TextPart>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SendMessageRequest {
    pub(super) message: A2aMessage,
    pub(super) model: Option<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct SendMessageResponse {
    pub(super) task: A2aTask,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DelegateTaskRequest {
    pub(super) prompt: String,
    pub(super) model: Option<String>,
    pub(super) context_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct A2aServerStatus {
    pub running: bool,
    pub address: String,
    pub error: Option<String>,
}

pub(super) struct A2aJson<T>(pub(super) T);

impl<T> IntoResponse for A2aJson<T>
where
    T: Serialize,
{
    fn into_response(self) -> Response {
        let mut response = Json(self.0).into_response();
        response.headers_mut().insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/a2a+json"),
        );
        response
    }
}

pub(super) async fn agent_card() -> Json<AgentCard> {
    Json(AgentCard {
        name: "Bridge Codex Work Mode".to_string(),
        description: "Local multi-model coding and browser automation agent".to_string(),
        supported_interfaces: vec![AgentInterface {
            url: A2A_BASE_URL.to_string(),
            protocol_binding: "HTTP+JSON".to_string(),
            protocol_version: "1.0".to_string(),
        }],
        provider: AgentProvider {
            organization: "Bridge Codex".to_string(),
            url: "https://github.com/KB01111/Bridge-codex".to_string(),
        },
        version: env!("CARGO_PKG_VERSION").to_string(),
        capabilities: AgentCapabilities {
            streaming: false,
            push_notifications: false,
            extended_agent_card: false,
        },
        default_input_modes: vec!["text/plain".to_string()],
        default_output_modes: vec!["text/plain".to_string()],
        skills: vec![AgentSkill {
            id: "local-codex-run".to_string(),
            name: "Local Codex run".to_string(),
            description: "Delegates a text task to a CLIProxyAPI-backed local model".to_string(),
            tags: vec!["coding".to_string(), "automation".to_string()],
            examples: vec!["Inspect this repository and explain the failing test".to_string()],
            input_modes: vec!["text/plain".to_string()],
            output_modes: vec!["text/plain".to_string()],
        }],
    })
}
