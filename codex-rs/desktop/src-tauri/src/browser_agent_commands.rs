use std::future::Future;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use chromiumoxide::layout::Point;
use tauri::AppHandle;
use tauri::State;

use crate::browser_agent::BrowserAgent;
use crate::browser_agent::BrowserStatus;
use crate::browser_agent::COMMAND_TIMEOUT;
use crate::browser_agent::VIEWPORT_HEIGHT;
use crate::browser_agent::VIEWPORT_WIDTH;
use crate::browser_agent::normalize_navigation_url;

const MAX_SELECTOR_BYTES: usize = 2 * 1024;
const MAX_BROWSER_TEXT_BYTES: usize = 32 * 1024;

fn validate_selector(selector: &str) -> Result<()> {
    if selector.trim().is_empty() {
        bail!("browser selector cannot be empty");
    }
    if selector.len() > MAX_SELECTOR_BYTES {
        bail!("browser selector exceeds the {MAX_SELECTOR_BYTES}-byte limit");
    }
    Ok(())
}

fn validate_browser_text(text: &str) -> Result<()> {
    if text.len() > MAX_BROWSER_TEXT_BYTES {
        bail!("browser text exceeds the {MAX_BROWSER_TEXT_BYTES}-byte limit");
    }
    Ok(())
}

async fn run_browser_command<T>(
    operation: &'static str,
    future: impl Future<Output = Result<T>>,
) -> Result<T, String> {
    tokio::time::timeout(COMMAND_TIMEOUT, future)
        .await
        .map_err(|_| format!("browser {operation} timed out"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn browser_status(state: State<'_, BrowserAgent>) -> Result<BrowserStatus, String> {
    Ok(state.status().await)
}

#[tauri::command]
pub async fn browser_start(
    app: AppHandle,
    state: State<'_, BrowserAgent>,
) -> Result<BrowserStatus, String> {
    state
        .ensure_started(app)
        .await
        .map_err(|error| error.to_string())?;
    Ok(state.status().await)
}

#[tauri::command]
pub async fn browser_stop(state: State<'_, BrowserAgent>) -> Result<BrowserStatus, String> {
    state.shutdown().await.map_err(|error| error.to_string())?;
    Ok(state.status().await)
}

#[tauri::command]
pub async fn browser_navigate(
    app: AppHandle,
    state: State<'_, BrowserAgent>,
    url: String,
) -> Result<(), String> {
    state
        .ensure_started(app)
        .await
        .map_err(|error| error.to_string())?;
    let url = normalize_navigation_url(&url).map_err(|error| error.to_string())?;
    let page = state
        .active_page()
        .await
        .map_err(|error| error.to_string())?;
    run_browser_command("navigation", async move {
        page.goto(url).await.context("browser navigation failed")?;
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn browser_click_selector(
    state: State<'_, BrowserAgent>,
    selector: String,
) -> Result<(), String> {
    validate_selector(&selector).map_err(|error| error.to_string())?;
    let page = state
        .active_page()
        .await
        .map_err(|error| error.to_string())?;
    run_browser_command("selector click", async move {
        page.find_element(selector)
            .await
            .context("browser selector was not found")?
            .click()
            .await
            .context("browser selector click failed")?;
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn browser_type(
    state: State<'_, BrowserAgent>,
    selector: String,
    text: String,
) -> Result<(), String> {
    validate_selector(&selector).map_err(|error| error.to_string())?;
    validate_browser_text(&text).map_err(|error| error.to_string())?;
    let page = state
        .active_page()
        .await
        .map_err(|error| error.to_string())?;
    run_browser_command("typing", async move {
        page.find_element(selector)
            .await
            .context("browser selector was not found")?
            .click()
            .await
            .context("browser selector click failed")?
            .type_str(text)
            .await
            .context("browser typing failed")?;
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn browser_click_at(
    state: State<'_, BrowserAgent>,
    x: f64,
    y: f64,
) -> Result<(), String> {
    if !x.is_finite()
        || !y.is_finite()
        || !(0.0..f64::from(VIEWPORT_WIDTH)).contains(&x)
        || !(0.0..f64::from(VIEWPORT_HEIGHT)).contains(&y)
    {
        return Err("browser click is outside the 1280x720 viewport".to_string());
    }
    let page = state
        .active_page()
        .await
        .map_err(|error| error.to_string())?;
    run_browser_command("coordinate click", async move {
        page.click(Point::new(x, y))
            .await
            .context("browser coordinate click failed")?;
        Ok(())
    })
    .await
}
