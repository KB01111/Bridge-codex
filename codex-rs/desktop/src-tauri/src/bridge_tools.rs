use std::collections::HashSet;
use std::path::Component;
use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chromiumoxide::cdp::browser_protocol::input::DispatchKeyEventParams;
use chromiumoxide::cdp::browser_protocol::input::DispatchKeyEventType;
use chromiumoxide::cdp::browser_protocol::input::InsertTextParams;
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::layout::Point;
use chromiumoxide::page::ScreenshotParams;
use codex_app_server_protocol::DynamicToolCallOutputContentItem;
use codex_app_server_protocol::DynamicToolCallParams;
use codex_app_server_protocol::DynamicToolCallResponse;
use codex_code_memory::SearchRequest;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use tauri::AppHandle;
use tauri::State;
use tokio::sync::RwLock;

use crate::browser_agent::BrowserAgent;
use crate::browser_agent::COMMAND_TIMEOUT;
use crate::browser_agent::VIEWPORT_HEIGHT;
use crate::browser_agent::VIEWPORT_WIDTH;
use crate::browser_agent::normalize_navigation_url;
use crate::code_memory::CodeMemoryManager;

const BRIDGE_NAMESPACE: &str = "bridge";
const MAX_MEMORY_QUERY_BYTES: usize = 2 * 1024;
const MAX_MEMORY_RESULTS: usize = 8;
const MAX_MEMORY_OUTPUT_BYTES: usize = 8 * 1024;
const MAX_MEMORY_EXCERPT_BYTES: usize = 1024;
const MAX_BROWSER_TEXT_BYTES: usize = 16 * 1024;
const MAX_BROWSER_KEY_BYTES: usize = 64;
const MAX_OBSERVATION_SCREENSHOT_BYTES: usize = 256 * 1024;
const MAX_THREAD_ID_BYTES: usize = 256;

#[derive(Clone, Default)]
pub struct BridgeToolExecutor {
    browser_consents: Arc<RwLock<HashSet<String>>>,
}

impl BridgeToolExecutor {
    async fn execute(
        &self,
        app: AppHandle,
        memory: &CodeMemoryManager,
        browser: &BrowserAgent,
        request: DynamicToolCallParams,
    ) -> Result<DynamicToolCallResponse> {
        if request.namespace.as_deref() != Some(BRIDGE_NAMESPACE) {
            bail!("unsupported dynamic tool namespace");
        }
        match request.tool.as_str() {
            "memory_search" => memory_search(memory, request.arguments).await,
            "browser_observe" => {
                self.require_browser_consent(&request.thread_id).await?;
                browser_observe(app, browser).await
            }
            "browser_navigate" => {
                self.require_browser_consent(&request.thread_id).await?;
                browser_navigate(app, browser, request.arguments).await
            }
            "browser_click" => {
                self.require_browser_consent(&request.thread_id).await?;
                browser_click(browser, request.arguments).await
            }
            "browser_type" => {
                self.require_browser_consent(&request.thread_id).await?;
                browser_type(browser, request.arguments).await
            }
            "browser_key" => {
                self.require_browser_consent(&request.thread_id).await?;
                browser_key(browser, request.arguments).await
            }
            _ => bail!("unsupported Bridge dynamic tool `{}`", request.tool),
        }
    }

    async fn grant_browser_consent(&self, thread_id: &str) -> Result<()> {
        validate_thread_id(thread_id)?;
        self.browser_consents
            .write()
            .await
            .insert(thread_id.to_string());
        Ok(())
    }

    async fn require_browser_consent(&self, thread_id: &str) -> Result<()> {
        validate_thread_id(thread_id)?;
        if !self.browser_consents.read().await.contains(thread_id) {
            bail!("browser content sharing requires explicit consent for this thread");
        }
        Ok(())
    }

    async fn revoke_browser_consent(&self, thread_id: &str) {
        self.browser_consents.write().await.remove(thread_id);
    }

    #[cfg(test)]
    async fn has_browser_consent(&self, thread_id: &str) -> bool {
        self.browser_consents.read().await.contains(thread_id)
    }
}

fn validate_thread_id(thread_id: &str) -> Result<()> {
    if thread_id.trim().is_empty() || thread_id.len() > MAX_THREAD_ID_BYTES {
        bail!("thread ID must contain 1 to {MAX_THREAD_ID_BYTES} bytes");
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MemorySearchArguments {
    query: String,
    #[serde(default = "default_memory_results")]
    max_results: usize,
}

const fn default_memory_results() -> usize {
    MAX_MEMORY_RESULTS
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MemoryHit {
    path: String,
    start_line: usize,
    end_line: usize,
    symbol: Option<String>,
    excerpt: String,
    score: f32,
}

async fn memory_search(
    memory: &CodeMemoryManager,
    arguments: Value,
) -> Result<DynamicToolCallResponse> {
    let arguments: MemorySearchArguments =
        serde_json::from_value(arguments).context("invalid memory_search arguments")?;
    let query = arguments.query.trim();
    if query.is_empty() || query.len() > MAX_MEMORY_QUERY_BYTES {
        bail!("memory_search query must contain 1 to {MAX_MEMORY_QUERY_BYTES} bytes");
    }
    if !(1..=MAX_MEMORY_RESULTS).contains(&arguments.max_results) {
        bail!("memory_search maxResults must be between 1 and {MAX_MEMORY_RESULTS}");
    }
    let root = memory
        .status()
        .await
        .root
        .map(std::path::PathBuf::from)
        .context("code memory has no canonical workspace root")?;
    let canonical_root = std::fs::canonicalize(&root)
        .with_context(|| format!("resolve code-memory root {}", root.display()))?;
    let results = memory
        .search(SearchRequest {
            query: query.to_string(),
            max_results: arguments.max_results,
            graph_weight: 0.25,
        })
        .await?;

    let mut hits = Vec::new();
    for result in results {
        let Some(path) = canonical_workspace_path(&canonical_root, &result.chunk.path) else {
            continue;
        };
        let hit = MemoryHit {
            path: path.to_string_lossy().into_owned(),
            start_line: result.chunk.start_line,
            end_line: result.chunk.end_line,
            symbol: result.chunk.symbol,
            excerpt: truncate_utf8(&result.chunk.source, MAX_MEMORY_EXCERPT_BYTES),
            score: result.score,
        };
        hits.push(hit);
        if serde_json::to_vec(&hits)?.len() > MAX_MEMORY_OUTPUT_BYTES {
            hits.pop();
            break;
        }
    }
    let text = serde_json::to_string(&hits)?;
    debug_assert!(text.len() <= MAX_MEMORY_OUTPUT_BYTES);
    Ok(text_response(text))
}

fn canonical_workspace_path(root: &Path, relative: &str) -> Option<std::path::PathBuf> {
    let relative = Path::new(relative);
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return None;
    }
    let canonical = std::fs::canonicalize(root.join(relative)).ok()?;
    canonical.starts_with(root).then_some(canonical)
}

async fn browser_observe(
    app: AppHandle,
    browser: &BrowserAgent,
) -> Result<DynamicToolCallResponse> {
    browser.ensure_started(app).await?;
    let page = browser.active_page().await?;
    let title = tokio::time::timeout(COMMAND_TIMEOUT, page.get_title())
        .await
        .context("browser title timed out")??
        .unwrap_or_default();
    let url = tokio::time::timeout(COMMAND_TIMEOUT, page.url())
        .await
        .context("browser URL timed out")??
        .unwrap_or_else(|| "about:blank".to_string());
    let screenshot = ScreenshotParams::builder()
        .format(CaptureScreenshotFormat::Jpeg)
        .quality(45)
        .full_page(false)
        .build();
    let screenshot = tokio::time::timeout(COMMAND_TIMEOUT, page.screenshot(screenshot))
        .await
        .context("browser observation screenshot timed out")??;
    let mut content_items = vec![DynamicToolCallOutputContentItem::InputText {
        text: serde_json::to_string(&serde_json::json!({
            "url": truncate_utf8(&url, 8 * 1024),
            "title": truncate_utf8(&title, 512),
            "viewport": {"width": VIEWPORT_WIDTH, "height": VIEWPORT_HEIGHT},
            "screenshotIncluded": screenshot.len() <= MAX_OBSERVATION_SCREENSHOT_BYTES,
        }))?,
    }];
    if screenshot.len() <= MAX_OBSERVATION_SCREENSHOT_BYTES {
        content_items.push(DynamicToolCallOutputContentItem::InputImage {
            image_url: format!("data:image/jpeg;base64,{}", STANDARD.encode(screenshot)),
        });
    }
    Ok(DynamicToolCallResponse {
        content_items,
        success: true,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NavigateArguments {
    url: String,
}

async fn browser_navigate(
    app: AppHandle,
    browser: &BrowserAgent,
    arguments: Value,
) -> Result<DynamicToolCallResponse> {
    let arguments: NavigateArguments =
        serde_json::from_value(arguments).context("invalid browser_navigate arguments")?;
    let url = normalize_navigation_url(&arguments.url)?;
    browser.ensure_started(app).await?;
    let page = browser.active_page().await?;
    tokio::time::timeout(COMMAND_TIMEOUT, page.goto(url.clone()))
        .await
        .context("browser navigation timed out")?
        .context("browser navigation failed")?;
    Ok(text_response(format!("Navigated to {url}")))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClickArguments {
    x: f64,
    y: f64,
}

async fn browser_click(
    browser: &BrowserAgent,
    arguments: Value,
) -> Result<DynamicToolCallResponse> {
    let arguments: ClickArguments =
        serde_json::from_value(arguments).context("invalid browser_click arguments")?;
    if !arguments.x.is_finite()
        || !arguments.y.is_finite()
        || !(0.0..f64::from(VIEWPORT_WIDTH)).contains(&arguments.x)
        || !(0.0..f64::from(VIEWPORT_HEIGHT)).contains(&arguments.y)
    {
        bail!("browser_click coordinates are outside the managed viewport");
    }
    let page = browser.active_page().await?;
    tokio::time::timeout(
        COMMAND_TIMEOUT,
        page.click(Point::new(arguments.x, arguments.y)),
    )
    .await
    .context("browser click timed out")?
    .context("browser click failed")?;
    Ok(text_response(format!(
        "Clicked ({}, {})",
        arguments.x, arguments.y
    )))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TypeArguments {
    text: String,
}

async fn browser_type(browser: &BrowserAgent, arguments: Value) -> Result<DynamicToolCallResponse> {
    let arguments: TypeArguments =
        serde_json::from_value(arguments).context("invalid browser_type arguments")?;
    if arguments.text.len() > MAX_BROWSER_TEXT_BYTES {
        bail!("browser_type text exceeds the {MAX_BROWSER_TEXT_BYTES}-byte limit");
    }
    let page = browser.active_page().await?;
    tokio::time::timeout(
        COMMAND_TIMEOUT,
        page.execute(InsertTextParams::new(arguments.text)),
    )
    .await
    .context("browser typing timed out")?
    .context("browser typing failed")?;
    Ok(text_response("Typed into the active element".to_string()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyArguments {
    key: String,
}

async fn browser_key(browser: &BrowserAgent, arguments: Value) -> Result<DynamicToolCallResponse> {
    let arguments: KeyArguments =
        serde_json::from_value(arguments).context("invalid browser_key arguments")?;
    if arguments.key.is_empty() || arguments.key.len() > MAX_BROWSER_KEY_BYTES {
        bail!("browser_key key must contain 1 to {MAX_BROWSER_KEY_BYTES} bytes");
    }
    let key_definition = chromiumoxide::keys::get_key_definition(&arguments.key)
        .with_context(|| format!("unsupported browser key `{}`", arguments.key))?;
    let key_down_event_type = if key_definition.text.is_some() || key_definition.key.len() == 1 {
        DispatchKeyEventType::KeyDown
    } else {
        DispatchKeyEventType::RawKeyDown
    };
    let mut command = DispatchKeyEventParams::builder()
        .r#type(DispatchKeyEventType::KeyDown)
        .key(key_definition.key)
        .code(key_definition.code)
        .windows_virtual_key_code(key_definition.key_code)
        .native_virtual_key_code(key_definition.key_code);
    if let Some(text) = key_definition.text {
        command = command.text(text);
    } else if key_definition.key.len() == 1 {
        command = command.text(key_definition.key);
    }
    let key_down = command
        .clone()
        .r#type(key_down_event_type)
        .build()
        .map_err(anyhow::Error::msg)?;
    let key_up = command
        .r#type(DispatchKeyEventType::KeyUp)
        .build()
        .map_err(anyhow::Error::msg)?;
    let page = browser.active_page().await?;
    tokio::time::timeout(COMMAND_TIMEOUT, async {
        page.execute(key_down)
            .await
            .context("browser key-down failed")?;
        page.execute(key_up)
            .await
            .context("browser key-up failed")?;
        Ok::<(), anyhow::Error>(())
    })
    .await
    .context("browser key press timed out")?
    .with_context(|| format!("browser key press `{}` failed", arguments.key))?;
    Ok(text_response(format!("Pressed {}", arguments.key)))
}

fn text_response(text: String) -> DynamicToolCallResponse {
    DynamicToolCallResponse {
        content_items: vec![DynamicToolCallOutputContentItem::InputText { text }],
        success: true,
    }
}

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let mut end = max_bytes.saturating_sub(3);
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    format!("{}...", &value[..end])
}

#[tauri::command]
pub async fn execute_agent_dynamic_tool(
    app: AppHandle,
    executor: State<'_, BridgeToolExecutor>,
    memory: State<'_, CodeMemoryManager>,
    browser: State<'_, BrowserAgent>,
    request: DynamicToolCallParams,
) -> Result<DynamicToolCallResponse, String> {
    executor
        .execute(app, &memory, &browser, request)
        .await
        .map_err(|error| format!("{error:#}"))
}

#[tauri::command]
pub async fn grant_agent_browser_consent(
    executor: State<'_, BridgeToolExecutor>,
    thread_id: String,
) -> Result<(), String> {
    executor
        .grant_browser_consent(&thread_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn revoke_agent_browser_consent(
    executor: State<'_, BridgeToolExecutor>,
    thread_id: String,
) -> Result<(), String> {
    validate_thread_id(&thread_id).map_err(|error| error.to_string())?;
    executor.revoke_browser_consent(&thread_id).await;
    Ok(())
}

#[cfg(test)]
#[path = "bridge_tools_tests.rs"]
mod tests;
