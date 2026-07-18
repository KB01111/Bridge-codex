use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use reqwest::header::CONTENT_TYPE;
use serde_json::Value;
use serde_json::json;

use crate::cli_proxy::CliProxyClient;
use crate::cli_proxy_http::ensure_success;
use crate::cli_proxy_http::read_success_body;

const PROBE_TOOL_NAME: &str = "bridge_probe";
const PROBE_VALUE: &str = "ping";
const PROBE_CONTINUATION_INSTRUCTION: &str =
    "Return exactly the function tool output and nothing else.";
const MAX_PROBE_RESPONSE_BYTES: usize = 64 * 1024;
const PROBE_STEP_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProbeFunctionCall {
    call_id: String,
    name: String,
    arguments: String,
}

pub(super) async fn probe_responses_conformance(
    client: &CliProxyClient,
    model: &str,
) -> Result<()> {
    let marker = format!("bridge-probe-{}", uuid::Uuid::new_v4().simple());
    probe_responses_conformance_with_marker(client, model, &marker).await
}

async fn probe_responses_conformance_with_marker(
    client: &CliProxyClient,
    model: &str,
    marker: &str,
) -> Result<()> {
    let forced_events =
        post_sse(client, &forced_call_payload(model), "forced function call").await?;
    require_completed(&forced_events, "forced function call")?;
    let call = extract_function_call(&forced_events)?
        .context("Responses probe completed without the required function call")?;
    if call.name != PROBE_TOOL_NAME {
        bail!(
            "Responses probe called `{}` instead of `{PROBE_TOOL_NAME}`",
            call.name
        );
    }
    let arguments: Value = serde_json::from_str(&call.arguments)
        .context("Responses probe returned invalid function-call arguments")?;
    if arguments != json!({"value": PROBE_VALUE}) {
        bail!("Responses probe returned unexpected function-call arguments");
    }

    let continuation_events = post_sse(
        client,
        &continuation_payload(model, &call, marker),
        "function-call output continuation",
    )
    .await?;
    require_completed(&continuation_events, "function-call output continuation")?;
    if extract_function_call(&continuation_events)?.is_some() {
        bail!("Responses probe ignored toolChoice=none after function-call output");
    }
    let final_output = extract_final_output_text(&continuation_events)
        .context("Responses probe continuation completed without final output text")?;
    if final_output != marker {
        bail!("Responses probe did not return the exact function-call output marker");
    }
    Ok(())
}

async fn post_sse(client: &CliProxyClient, payload: &Value, phase: &str) -> Result<Vec<Value>> {
    let response = tokio::time::timeout(
        PROBE_STEP_TIMEOUT,
        client
            .request(reqwest::Method::POST, "v1/responses")?
            .json(payload)
            .send(),
    )
    .await
    .with_context(|| format!("CLIProxyAPI {phase} probe timed out"))?
    .with_context(|| format!("failed to send the CLIProxyAPI {phase} probe"))?;
    let response = tokio::time::timeout(PROBE_STEP_TIMEOUT, ensure_success(response, phase))
        .await
        .with_context(|| format!("CLIProxyAPI {phase} error body timed out"))??;
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if !content_type
        .to_ascii_lowercase()
        .starts_with("text/event-stream")
    {
        bail!("CLIProxyAPI {phase} probe did not return an SSE response");
    }
    let body = tokio::time::timeout(
        PROBE_STEP_TIMEOUT,
        read_success_body(response, MAX_PROBE_RESPONSE_BYTES, phase),
    )
    .await
    .with_context(|| format!("CLIProxyAPI {phase} SSE body timed out"))??;
    parse_sse_events(&body).with_context(|| format!("invalid CLIProxyAPI {phase} SSE stream"))
}

fn forced_call_payload(model: &str) -> Value {
    json!({
        "model": model,
        "stream": true,
        "input": "Call bridge_probe exactly once with value ping.",
        "tools": [probe_tool()],
        "tool_choice": {"type": "function", "name": PROBE_TOOL_NAME},
        "parallel_tool_calls": false
    })
}

fn continuation_payload(model: &str, call: &ProbeFunctionCall, marker: &str) -> Value {
    json!({
        "model": model,
        "stream": true,
        "input": [
            {
                "type": "function_call",
                "call_id": call.call_id,
                "name": call.name,
                "arguments": call.arguments
            },
            {
                "type": "function_call_output",
                "call_id": call.call_id,
                "output": marker
            },
            {
                "role": "user",
                "content": [{
                    "type": "input_text",
                    "text": PROBE_CONTINUATION_INSTRUCTION
                }]
            }
        ],
        "tools": [probe_tool()],
        "tool_choice": "none",
        "parallel_tool_calls": false
    })
}

fn extract_final_output_text(events: &[Value]) -> Option<String> {
    let mut streamed_text = None;
    for event in events {
        if event.get("type").and_then(Value::as_str) == Some("response.output_text.done") {
            streamed_text = event
                .get("text")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
        }
        if event.get("type").and_then(Value::as_str) != Some("response.completed") {
            continue;
        }
        let output = event
            .get("response")
            .and_then(|response| response.get("output"))
            .and_then(Value::as_array)?;
        let text = output
            .iter()
            .filter(|item| item.get("type").and_then(Value::as_str) == Some("message"))
            .filter_map(|item| item.get("content").and_then(Value::as_array))
            .flatten()
            .filter(|content| content.get("type").and_then(Value::as_str) == Some("output_text"))
            .filter_map(|content| content.get("text").and_then(Value::as_str))
            .collect::<String>();
        if !text.is_empty() {
            return Some(text);
        }
    }
    streamed_text
}

fn probe_tool() -> Value {
    json!({
        "type": "function",
        "name": PROBE_TOOL_NAME,
        "description": "Return a bounded conformance probe value.",
        "parameters": {
            "type": "object",
            "properties": {
                "value": {"type": "string", "enum": [PROBE_VALUE]}
            },
            "required": ["value"],
            "additionalProperties": false
        },
        "strict": true
    })
}

fn parse_sse_events(body: &[u8]) -> Result<Vec<Value>> {
    let stream = std::str::from_utf8(body).context("SSE stream was not valid UTF-8")?;
    let mut events = Vec::new();
    let mut data = String::new();
    for raw_line in stream.lines() {
        let line = raw_line.trim_end_matches('\r');
        if line.is_empty() {
            push_sse_event(&mut data, &mut events)?;
            continue;
        }
        if let Some(value) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(value.trim_start());
        }
    }
    push_sse_event(&mut data, &mut events)?;
    if events.is_empty() {
        bail!("SSE stream contained no JSON events");
    }
    Ok(events)
}

fn push_sse_event(data: &mut String, events: &mut Vec<Value>) -> Result<()> {
    if data.is_empty() {
        return Ok(());
    }
    if data != "[DONE]" {
        events.push(serde_json::from_str(data).context("SSE data was not valid JSON")?);
    }
    data.clear();
    Ok(())
}

fn require_completed(events: &[Value], phase: &str) -> Result<()> {
    for event in events {
        match event.get("type").and_then(Value::as_str) {
            Some("response.completed") => return Ok(()),
            Some("response.failed" | "response.incomplete" | "error") => {
                bail!("CLIProxyAPI {phase} probe reported a failed response")
            }
            _ => {}
        }
    }
    bail!("CLIProxyAPI {phase} SSE stream ended without response.completed")
}

fn extract_function_call(events: &[Value]) -> Result<Option<ProbeFunctionCall>> {
    for event in events {
        if let Some(call) = event
            .get("item")
            .map(parse_function_call)
            .transpose()?
            .flatten()
        {
            return Ok(Some(call));
        }
        if let Some(output) = event
            .get("response")
            .and_then(|response| response.get("output"))
            .and_then(Value::as_array)
        {
            for item in output {
                if let Some(call) = parse_function_call(item)? {
                    return Ok(Some(call));
                }
            }
        }
    }
    Ok(None)
}

fn parse_function_call(item: &Value) -> Result<Option<ProbeFunctionCall>> {
    if item.get("type").and_then(Value::as_str) != Some("function_call") {
        return Ok(None);
    }
    Ok(Some(ProbeFunctionCall {
        call_id: required_string(item, "call_id")?,
        name: required_string(item, "name")?,
        arguments: required_string(item, "arguments")?,
    }))
}

fn required_string(item: &Value, field: &str) -> Result<String> {
    item.get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .with_context(|| format!("Responses function call omitted `{field}`"))
}

#[cfg(test)]
#[path = "cli_proxy_conformance_tests.rs"]
mod tests;
