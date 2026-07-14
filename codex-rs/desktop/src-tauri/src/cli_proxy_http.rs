use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use futures::StreamExt;
use reqwest::Response;

pub(super) const MAX_CHAT_RESPONSE_BYTES: usize = 64 * 1024;
pub(super) const MAX_MODELS_RESPONSE_BYTES: usize = 512 * 1024;
const MAX_ERROR_BODY_BYTES: usize = 8 * 1024;

pub(super) async fn ensure_success(response: Response) -> Result<Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }

    let (body, truncated) = read_up_to(response, MAX_ERROR_BODY_BYTES)
        .await
        .context("failed to read the CLIProxyAPI error response")?;
    let body = String::from_utf8_lossy(&body);
    let suffix = if truncated { " [truncated]" } else { "" };
    bail!("CLIProxyAPI returned {status}: {body}{suffix}")
}

pub(super) async fn read_success_body(response: Response, limit: usize) -> Result<Vec<u8>> {
    let response = ensure_success(response).await?;
    let (body, truncated) = read_up_to(response, limit)
        .await
        .context("failed to read the CLIProxyAPI response")?;
    if truncated {
        bail!("CLIProxyAPI response exceeds the {limit}-byte limit");
    }
    Ok(body)
}

async fn read_up_to(response: Response, limit: usize) -> Result<(Vec<u8>, bool)> {
    let mut body = Vec::with_capacity(limit.min(8 * 1024));
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("failed while reading the CLIProxyAPI response body")?;
        let remaining = limit.saturating_sub(body.len());
        if chunk.len() > remaining {
            body.extend_from_slice(&chunk[..remaining]);
            return Ok((body, true));
        }
        body.extend_from_slice(&chunk);
    }
    Ok((body, false))
}
