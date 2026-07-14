use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use chrono::Utc;
use codex_code_memory::CodeMemory;
use codex_code_memory::IndexLimits;
use codex_code_memory::IndexStatistics;
use codex_code_memory::SNAPSHOT_SCHEMA_VERSION;
use codex_code_memory::SearchRequest;
use codex_code_memory::SearchResult;
use codex_code_memory::index_directory;
use codex_code_memory::load_snapshot;
use codex_code_memory::save_snapshot;
use serde::Serialize;
use tauri::State;
use tokio::sync::RwLock;
use tokio::sync::Semaphore;

const MAX_ROOT_BYTES: usize = 4 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopCodeMemoryStatus {
    pub ready: bool,
    pub indexing: bool,
    pub root: Option<String>,
    pub indexed_at: Option<String>,
    pub schema_version: u32,
    pub parser: String,
    pub retrieval: String,
    pub storage: String,
    pub network_access: String,
    pub statistics: IndexStatistics,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeMemoryIndexResult {
    pub status: DesktopCodeMemoryStatus,
    pub warnings: Vec<String>,
}

#[derive(Default)]
struct LoadedCodeMemory {
    memory: Option<Arc<CodeMemory>>,
    root: Option<String>,
    indexed_at: Option<String>,
    error: Option<String>,
}

#[derive(Clone)]
pub struct CodeMemoryManager {
    loaded: Arc<RwLock<LoadedCodeMemory>>,
    operation_gate: Arc<Semaphore>,
    snapshot_path: Arc<PathBuf>,
}

impl CodeMemoryManager {
    pub fn new(snapshot_path: PathBuf) -> Self {
        Self {
            loaded: Arc::new(RwLock::new(LoadedCodeMemory::default())),
            operation_gate: Arc::new(Semaphore::new(1)),
            snapshot_path: Arc::new(snapshot_path),
        }
    }

    pub async fn restore(&self) {
        let snapshot_path = self.snapshot_path.as_ref().clone();
        if !snapshot_path.exists() {
            return;
        }
        let restored = tokio::task::spawn_blocking(move || {
            load_snapshot(snapshot_path, IndexLimits::default()).map(Arc::new)
        })
        .await;
        let mut loaded = self.loaded.write().await;
        match restored {
            Ok(Ok(memory)) => {
                loaded.memory = Some(memory);
                loaded.indexed_at = Some(Utc::now().to_rfc3339());
                loaded.error = None;
            }
            Ok(Err(error)) => {
                loaded.error = Some(format!("failed to restore code memory: {error:#}"));
            }
            Err(error) => {
                loaded.error = Some(format!("code-memory restore task failed: {error}"));
            }
        }
    }

    pub async fn status(&self) -> DesktopCodeMemoryStatus {
        let loaded = self.loaded.read().await;
        status_from_loaded(&loaded, self.operation_gate.available_permits() == 0)
    }

    pub async fn index_root(&self, root: String) -> Result<CodeMemoryIndexResult> {
        let root = root.trim();
        if root.is_empty() {
            bail!("choose a source directory to index");
        }
        if root.len() > MAX_ROOT_BYTES {
            bail!("indexing root exceeds the {MAX_ROOT_BYTES}-byte limit");
        }
        let _permit = self
            .operation_gate
            .clone()
            .try_acquire_owned()
            .context("another code-memory operation is already running")?;
        let requested_root = PathBuf::from(root);
        let snapshot_path = self.snapshot_path.as_ref().clone();
        let indexed = tokio::task::spawn_blocking(move || {
            let canonical_root = std::fs::canonicalize(&requested_root).with_context(|| {
                format!(
                    "failed to resolve indexing root {}",
                    requested_root.display()
                )
            })?;
            let limits = IndexLimits::default();
            let report = index_directory(&canonical_root, limits)?;
            let warnings = report.warnings.clone();
            let memory = CodeMemory::from_indexing_report(report, limits)?;
            save_snapshot(&memory, snapshot_path)?;
            Ok::<_, anyhow::Error>((Arc::new(memory), canonical_root, warnings))
        })
        .await
        .context("code-memory indexing task failed")?;

        match indexed {
            Ok((memory, canonical_root, warnings)) => {
                let mut loaded = self.loaded.write().await;
                loaded.memory = Some(memory);
                loaded.root = Some(canonical_root.to_string_lossy().into_owned());
                loaded.indexed_at = Some(Utc::now().to_rfc3339());
                loaded.error = None;
                Ok(CodeMemoryIndexResult {
                    status: status_from_loaded(&loaded, /*indexing*/ false),
                    warnings,
                })
            }
            Err(error) => {
                self.loaded.write().await.error = Some(format!("{error:#}"));
                Err(error)
            }
        }
    }

    pub async fn search(&self, request: SearchRequest) -> Result<Vec<SearchResult>> {
        let memory = self
            .loaded
            .read()
            .await
            .memory
            .clone()
            .context("index a source directory before searching code memory")?;
        tokio::task::spawn_blocking(move || memory.search(request))
            .await
            .context("code-memory search task failed")?
    }

    pub async fn clear(&self) -> Result<DesktopCodeMemoryStatus> {
        let _permit = self
            .operation_gate
            .clone()
            .try_acquire_owned()
            .context("another code-memory operation is already running")?;
        let snapshot_path = self.snapshot_path.as_ref().clone();
        tokio::task::spawn_blocking(move || match std::fs::remove_file(snapshot_path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        })
        .await
        .context("code-memory cleanup task failed")?
        .context("failed to remove the code-memory snapshot")?;
        let mut loaded = self.loaded.write().await;
        *loaded = LoadedCodeMemory::default();
        Ok(status_from_loaded(&loaded, /*indexing*/ false))
    }
}

fn status_from_loaded(loaded: &LoadedCodeMemory, indexing: bool) -> DesktopCodeMemoryStatus {
    let memory_status = loaded.memory.as_ref().map(|memory| memory.status());
    DesktopCodeMemoryStatus {
        ready: memory_status.is_some(),
        indexing,
        root: loaded.root.clone(),
        indexed_at: loaded.indexed_at.clone(),
        schema_version: memory_status
            .as_ref()
            .map_or(SNAPSHOT_SCHEMA_VERSION, |status| status.schema_version),
        parser: memory_status
            .as_ref()
            .map_or_else(|| "tree-sitter".to_string(), |status| status.parser.clone()),
        retrieval: memory_status.as_ref().map_or_else(
            || "bm25+symbol-import-graph".to_string(),
            |status| status.retrieval.clone(),
        ),
        storage: memory_status.as_ref().map_or_else(
            || "versioned-json".to_string(),
            |status| status.storage.clone(),
        ),
        network_access: memory_status.as_ref().map_or_else(
            || "none".to_string(),
            |status| status.network_access.clone(),
        ),
        statistics: memory_status.map_or_else(IndexStatistics::default, |status| status.statistics),
        error: loaded.error.clone(),
    }
}

#[tauri::command]
pub async fn get_code_memory_status(
    state: State<'_, CodeMemoryManager>,
) -> Result<DesktopCodeMemoryStatus, String> {
    Ok(state.status().await)
}

#[tauri::command]
pub async fn index_code_memory(
    state: State<'_, CodeMemoryManager>,
    root: String,
) -> Result<CodeMemoryIndexResult, String> {
    state
        .index_root(root)
        .await
        .map_err(|error| format!("{error:#}"))
}

#[tauri::command]
pub async fn search_code_memory(
    state: State<'_, CodeMemoryManager>,
    request: SearchRequest,
) -> Result<Vec<SearchResult>, String> {
    state
        .search(request)
        .await
        .map_err(|error| format!("{error:#}"))
}

#[tauri::command]
pub async fn clear_code_memory(
    state: State<'_, CodeMemoryManager>,
) -> Result<DesktopCodeMemoryStatus, String> {
    state.clear().await.map_err(|error| format!("{error:#}"))
}

#[cfg(test)]
#[path = "code_memory_tests.rs"]
mod tests;
