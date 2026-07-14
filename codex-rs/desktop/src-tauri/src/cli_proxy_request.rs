use anyhow::Result;
use anyhow::bail;

use crate::cli_proxy::ChatMessage;
use crate::cli_proxy::ChatRequest;
use crate::cli_proxy::ChatRole;
use crate::code_policy::SANDBOX_EXECUTION_SYSTEM_PROMPT;

const MAX_CHAT_MESSAGES: usize = 128;
const MAX_CHAT_REQUEST_BYTES: usize = 64 * 1024;
const MAX_MODEL_ID_BYTES: usize = 256;

pub(super) fn validate_chat_request(request: &ChatRequest) -> Result<()> {
    let model = request.model.trim();
    if model.is_empty() {
        bail!("select a model before sending a message");
    }
    if model.len() > MAX_MODEL_ID_BYTES {
        bail!("model identifier exceeds the {MAX_MODEL_ID_BYTES}-byte limit");
    }
    if request.messages.is_empty() {
        bail!("a chat request must include at least one message");
    }
    if request.messages.len() > MAX_CHAT_MESSAGES {
        bail!("chat request exceeds the {MAX_CHAT_MESSAGES}-message limit");
    }

    let mut request_bytes = 0usize;
    let mut contains_content = false;
    for message in &request.messages {
        request_bytes = request_bytes
            .checked_add(message.content.len())
            .ok_or_else(|| anyhow::anyhow!("chat request size overflow"))?;
        contains_content |= !message.content.trim().is_empty();
        if request_bytes > MAX_CHAT_REQUEST_BYTES {
            bail!("chat request exceeds the {MAX_CHAT_REQUEST_BYTES}-byte content limit");
        }
    }
    if !contains_content {
        bail!("a chat request must include non-empty message content");
    }
    Ok(())
}

pub(super) fn sandbox_messages(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    let mut constrained = Vec::with_capacity(messages.len() + 1);
    constrained.push(ChatMessage {
        role: ChatRole::System,
        content: SANDBOX_EXECUTION_SYSTEM_PROMPT.to_string(),
    });
    constrained.extend_from_slice(messages);
    constrained
}
