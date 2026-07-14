use std::sync::Arc;
use std::sync::Weak;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chromiumoxide::Browser;
use chromiumoxide::BrowserConfig;
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::page::Page;
use chromiumoxide::page::ScreenshotParams;
use chrono::SecondsFormat;
use chrono::Utc;
use futures::StreamExt;
use serde::Serialize;
use tauri::AppHandle;
use tauri::Emitter;
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use tokio::sync::Semaphore;
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;
use url::Url;

pub(super) const VIEWPORT_WIDTH: u32 = 1280;
pub(super) const VIEWPORT_HEIGHT: u32 = 720;
const FRAME_INTERVAL: Duration = Duration::from_millis(125);
const FRAME_TIMEOUT: Duration = Duration::from_secs(3);
const START_TIMEOUT: Duration = Duration::from_secs(20);
pub(super) const COMMAND_TIMEOUT: Duration = Duration::from_secs(15);
const STATUS_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_NAVIGATION_URL_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct BrowserFrame {
    jpeg_base64: String,
    url: String,
    width: u32,
    height: u32,
    sequence: u64,
    captured_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BrowserHealth {
    Stopped,
    Starting,
    Running,
    Degraded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserStatus {
    pub running: bool,
    pub url: Option<String>,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub health: BrowserHealth,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
struct HealthState {
    health: BrowserHealth,
    error: Option<String>,
}

#[derive(Clone, Default)]
pub struct BrowserAgent {
    inner: Arc<BrowserAgentInner>,
}

struct BrowserAgentInner {
    startup_gate: Semaphore,
    browser: Mutex<Option<Browser>>,
    page: RwLock<Option<Page>>,
    handler_task: Mutex<Option<JoinHandle<()>>>,
    frame_task: Mutex<Option<JoinHandle<()>>>,
    health: RwLock<HealthState>,
    frame_sequence: AtomicU64,
}

impl Default for BrowserAgentInner {
    fn default() -> Self {
        Self {
            startup_gate: Semaphore::new(1),
            browser: Mutex::new(None),
            page: RwLock::new(None),
            handler_task: Mutex::new(None),
            frame_task: Mutex::new(None),
            health: RwLock::new(HealthState {
                health: BrowserHealth::Stopped,
                error: None,
            }),
            frame_sequence: AtomicU64::new(0),
        }
    }
}

impl BrowserAgent {
    pub(super) async fn ensure_started(&self, app: AppHandle) -> Result<()> {
        let _startup_permit = self
            .inner
            .startup_gate
            .acquire()
            .await
            .context("browser startup gate was closed")?;
        if self.is_live().await {
            return Ok(());
        }
        self.stop_components().await;
        self.set_health(BrowserHealth::Starting, /*error*/ None)
            .await;

        let result = self.launch(app).await;
        match result {
            Ok(()) => {
                self.set_health(BrowserHealth::Running, /*error*/ None)
                    .await;
                Ok(())
            }
            Err(error) => {
                self.stop_components().await;
                self.set_health(BrowserHealth::Failed, Some(error.to_string()))
                    .await;
                Err(error)
            }
        }
    }

    async fn launch(&self, app: AppHandle) -> Result<()> {
        let config = BrowserConfig::builder()
            .window_size(VIEWPORT_WIDTH, VIEWPORT_HEIGHT)
            .arg("--force-device-scale-factor=1")
            .build()
            .map_err(anyhow::Error::msg)
            .context("failed to configure Chromium")?;
        let (mut browser, mut handler) =
            tokio::time::timeout(START_TIMEOUT, Browser::launch(config))
                .await
                .context("timed out while launching Chromium")?
                .context("failed to launch Chromium; install Chrome or Chromium first")?;

        let weak_inner = Arc::downgrade(&self.inner);
        let handler_task = tokio::spawn(async move {
            let mut failure = None;
            while let Some(event) = handler.next().await {
                if let Err(error) = event {
                    failure = Some(format!("Chromium handler failed: {error}"));
                    break;
                }
            }
            if let Some(inner) = weak_inner.upgrade() {
                let browser_running = inner.browser.lock().await.is_some();
                if !browser_running {
                    return;
                }
                *inner.health.write().await = HealthState {
                    health: BrowserHealth::Failed,
                    error: Some(
                        failure
                            .unwrap_or_else(|| "Chromium handler stopped unexpectedly".to_string()),
                    ),
                };
            }
        });
        *self.inner.handler_task.lock().await = Some(handler_task);

        let page = match tokio::time::timeout(START_TIMEOUT, browser.new_page("about:blank")).await
        {
            Ok(Ok(page)) => page,
            Ok(Err(error)) => {
                let _ = tokio::time::timeout(FRAME_TIMEOUT, browser.close()).await;
                bail!("failed to create the agent browser page: {error}");
            }
            Err(_) => {
                let _ = tokio::time::timeout(FRAME_TIMEOUT, browser.close()).await;
                bail!("timed out while creating the agent browser page");
            }
        };
        *self.inner.page.write().await = Some(page.clone());
        *self.inner.browser.lock().await = Some(browser);
        self.inner.frame_sequence.store(0, Ordering::Relaxed);
        let frame_task = tokio::spawn(stream_frames(page, app, Arc::downgrade(&self.inner)));
        *self.inner.frame_task.lock().await = Some(frame_task);
        Ok(())
    }

    async fn is_live(&self) -> bool {
        if self.inner.browser.lock().await.is_none() || self.inner.page.read().await.is_none() {
            return false;
        }
        self.inner
            .handler_task
            .lock()
            .await
            .as_ref()
            .is_some_and(|task| !task.is_finished())
    }

    pub(super) async fn active_page(&self) -> Result<Page> {
        self.inner
            .page
            .read()
            .await
            .clone()
            .ok_or_else(|| anyhow!("the agent browser has not started"))
    }

    async fn set_health(&self, health: BrowserHealth, error: Option<String>) {
        *self.inner.health.write().await = HealthState { health, error };
    }

    async fn stop_components(&self) {
        let frame_task = self.inner.frame_task.lock().await.take();
        if let Some(task) = frame_task {
            task.abort();
            let _ = task.await;
        }
        *self.inner.page.write().await = None;
        let browser = self.inner.browser.lock().await.take();
        if let Some(mut browser) = browser {
            let _ = tokio::time::timeout(FRAME_TIMEOUT, browser.close()).await;
        }
        let handler_task = self.inner.handler_task.lock().await.take();
        if let Some(task) = handler_task {
            task.abort();
            let _ = task.await;
        }
    }

    pub async fn shutdown(&self) {
        let Ok(_permit) = self.inner.startup_gate.acquire().await else {
            return;
        };
        self.stop_components().await;
        self.set_health(BrowserHealth::Stopped, /*error*/ None)
            .await;
    }

    pub async fn status(&self) -> BrowserStatus {
        let running = self.is_live().await;
        let url = if running {
            match tokio::time::timeout(STATUS_TIMEOUT, self.active_page()).await {
                Ok(Ok(page)) => tokio::time::timeout(STATUS_TIMEOUT, page.url())
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .flatten(),
                _ => None,
            }
        } else {
            None
        };
        let mut health = self.inner.health.read().await.clone();
        if !running
            && matches!(
                health.health,
                BrowserHealth::Running | BrowserHealth::Degraded
            )
        {
            health = HealthState {
                health: BrowserHealth::Failed,
                error: Some("Chromium is no longer responding".to_string()),
            };
            *self.inner.health.write().await = health.clone();
        }
        BrowserStatus {
            running,
            url,
            viewport_width: VIEWPORT_WIDTH,
            viewport_height: VIEWPORT_HEIGHT,
            health: health.health,
            error: health.error,
        }
    }
}

async fn stream_frames(page: Page, app: AppHandle, weak_inner: Weak<BrowserAgentInner>) {
    let mut interval = tokio::time::interval(FRAME_INTERVAL);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut consecutive_failures = 0usize;
    loop {
        interval.tick().await;
        let screenshot = ScreenshotParams::builder()
            .format(CaptureScreenshotFormat::Jpeg)
            .quality(72)
            .full_page(false)
            .build();
        let jpeg = match tokio::time::timeout(FRAME_TIMEOUT, page.screenshot(screenshot)).await {
            Ok(Ok(jpeg)) => jpeg,
            Ok(Err(error)) => {
                consecutive_failures += 1;
                mark_frame_failure(&weak_inner, consecutive_failures, error.to_string()).await;
                continue;
            }
            Err(_) => {
                consecutive_failures += 1;
                mark_frame_failure(
                    &weak_inner,
                    consecutive_failures,
                    "browser frame capture timed out".to_string(),
                )
                .await;
                continue;
            }
        };
        if consecutive_failures > 0 {
            consecutive_failures = 0;
            set_inner_health(&weak_inner, BrowserHealth::Running, /*error*/ None).await;
        }
        let url = tokio::time::timeout(STATUS_TIMEOUT, page.url())
            .await
            .ok()
            .and_then(Result::ok)
            .flatten()
            .unwrap_or_else(|| "about:blank".to_string());
        let Some(inner) = weak_inner.upgrade() else {
            break;
        };
        let sequence = inner.frame_sequence.fetch_add(1, Ordering::Relaxed) + 1;
        drop(inner);
        if let Err(error) = app.emit(
            "browser-frame",
            BrowserFrame {
                jpeg_base64: STANDARD.encode(jpeg),
                url,
                width: VIEWPORT_WIDTH,
                height: VIEWPORT_HEIGHT,
                sequence,
                captured_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            },
        ) {
            set_inner_health(
                &weak_inner,
                BrowserHealth::Failed,
                Some(format!("failed to emit browser frame: {error}")),
            )
            .await;
            break;
        }
    }
}

async fn mark_frame_failure(
    inner: &Weak<BrowserAgentInner>,
    consecutive_failures: usize,
    error: String,
) {
    if consecutive_failures >= 3 {
        set_inner_health(inner, BrowserHealth::Degraded, Some(error)).await;
    }
}

async fn set_inner_health(
    inner: &Weak<BrowserAgentInner>,
    health: BrowserHealth,
    error: Option<String>,
) {
    if let Some(inner) = inner.upgrade() {
        *inner.health.write().await = HealthState { health, error };
    }
}

pub(super) fn normalize_navigation_url(input: &str) -> Result<String> {
    let input = input.trim();
    if input.is_empty() {
        bail!("enter an address to navigate");
    }
    if input.len() > MAX_NAVIGATION_URL_BYTES {
        bail!("browser address exceeds the {MAX_NAVIGATION_URL_BYTES}-byte limit");
    }
    let candidate = if input.contains("://") {
        input.to_string()
    } else {
        format!("https://{input}")
    };
    let url = Url::parse(&candidate).context("invalid browser address")?;
    if !matches!(url.scheme(), "http" | "https") {
        bail!("agent browser navigation supports only http and https URLs");
    }
    Ok(url.to_string())
}

#[cfg(test)]
#[path = "browser_agent_tests.rs"]
mod tests;
