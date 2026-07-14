use std::fs;

use codex_code_memory::SearchRequest;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use super::CodeMemoryManager;

#[tokio::test]
async fn indexes_searches_persists_restores_and_clears() {
    let directory = tempdir().expect("create temporary directory");
    let source_root = directory.path().join("source");
    fs::create_dir(&source_root).expect("create source directory");
    fs::write(
        source_root.join("router.rs"),
        "pub fn route_request() -> &'static str { \"local\" }\n",
    )
    .expect("write source fixture");
    let snapshot = directory.path().join("state/code-memory.json");
    let manager = CodeMemoryManager::new(snapshot.clone());

    let indexed = manager
        .index_root(source_root.to_string_lossy().into_owned())
        .await
        .expect("index source fixture");
    assert!(indexed.status.ready);
    assert_eq!(indexed.status.statistics.indexed_files, 1);
    let results = manager
        .search(SearchRequest::new("route request"))
        .await
        .expect("search indexed source");
    assert_eq!(results[0].chunk.path, "router.rs");
    assert!(snapshot.exists());

    let restored = CodeMemoryManager::new(snapshot.clone());
    restored.restore().await;
    assert!(restored.status().await.ready);
    assert!(
        !restored
            .search(SearchRequest::new("local"))
            .await
            .expect("search restored snapshot")
            .is_empty()
    );

    let cleared = restored.clear().await.expect("clear restored snapshot");
    assert!(!cleared.ready);
    assert!(!snapshot.exists());
}

#[tokio::test]
async fn rejects_concurrent_index_operations() {
    let directory = tempdir().expect("create temporary directory");
    let manager = CodeMemoryManager::new(directory.path().join("memory.json"));
    let permit = manager
        .operation_gate
        .clone()
        .try_acquire_owned()
        .expect("hold operation gate");

    let error = manager
        .index_root(directory.path().to_string_lossy().into_owned())
        .await
        .expect_err("reject overlapping operation");
    drop(permit);

    assert!(error.to_string().contains("already running"));
}
