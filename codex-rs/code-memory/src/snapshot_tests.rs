use std::fs;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use super::load_snapshot;
use super::save_snapshot;
use crate::CodeMemory;
use crate::IndexLimits;
use crate::Language;
use crate::SearchRequest;
use crate::SourceDocument;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[test]
fn snapshot_roundtrip_preserves_status_and_results() {
    let memory = CodeMemory::from_documents(
        vec![SourceDocument {
            path: "src/lib.rs".into(),
            language: Language::Rust,
            source: "pub fn deterministic_snapshot() -> bool { true }\n".into(),
        }],
        IndexLimits::default(),
    )
    .unwrap();
    let path = test_snapshot_path();
    save_snapshot(&memory, &path).unwrap();
    let loaded = load_snapshot(&path, IndexLimits::default()).unwrap();

    assert_eq!(loaded.status(), memory.status());
    assert_eq!(
        loaded
            .search(SearchRequest::new("deterministic_snapshot"))
            .unwrap(),
        memory
            .search(SearchRequest::new("deterministic_snapshot"))
            .unwrap()
    );
    fs::remove_file(path).unwrap();
}

fn test_snapshot_path() -> PathBuf {
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "codex-code-memory-snapshot-{}-{counter}.json",
        std::process::id()
    ))
}
