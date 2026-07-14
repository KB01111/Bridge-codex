use std::fs;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use super::index_directory;
use super::index_subdirectory;
use super::normalize_document_path;
use crate::IndexLimits;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[test]
fn rejects_unsafe_document_and_subdirectory_paths() {
    assert!(normalize_document_path("../secret.rs").is_err());
    assert!(normalize_document_path("C:\\secret.rs").is_err());

    let root = test_directory("path-safety");
    fs::create_dir_all(root.join("safe")).unwrap();
    assert!(index_subdirectory(&root, PathBuf::from(".."), IndexLimits::default()).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn enforces_file_source_and_vendor_bounds() {
    let root = test_directory("bounds");
    fs::create_dir_all(root.join("vendor")).unwrap();
    fs::write(root.join("a.rs"), "fn alpha() {}\n").unwrap();
    fs::write(root.join("b.rs"), "fn beta() {}\n").unwrap();
    fs::write(root.join("vendor/ignored.rs"), "fn ignored() {}\n").unwrap();

    let limits = IndexLimits {
        max_files: 1,
        max_file_bytes: 128,
        max_source_bytes: 128,
        max_chunks: 8,
        max_chunk_bytes: 128,
        max_depth: 4,
        ..IndexLimits::default()
    };
    let report = index_directory(&root, limits).unwrap();
    assert_eq!(report.documents.len(), 1);
    assert_eq!(report.documents[0].path, "a.rs");
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.contains("1 source files"))
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn bounds_discovery_entries_and_retained_warnings() {
    let discovery_root = test_directory("discovery-limit");
    fs::write(discovery_root.join("a.txt"), "ignored").unwrap();
    fs::write(discovery_root.join("b.txt"), "ignored").unwrap();
    fs::write(discovery_root.join("c.txt"), "ignored").unwrap();
    let discovery_limits = IndexLimits {
        max_discovered_entries: 2,
        ..IndexLimits::default()
    };
    assert!(index_directory(&discovery_root, discovery_limits).is_err());
    fs::remove_dir_all(discovery_root).unwrap();

    let warning_root = test_directory("warning-limit");
    fs::write(warning_root.join("a.rs"), "0123456789").unwrap();
    fs::write(warning_root.join("b.rs"), "0123456789").unwrap();
    fs::write(warning_root.join("c.rs"), "0123456789").unwrap();
    let warning_limits = IndexLimits {
        max_file_bytes: 4,
        max_warnings: 2,
        ..IndexLimits::default()
    };
    let report = index_directory(&warning_root, warning_limits).unwrap();
    assert_eq!(
        report.warnings,
        vec![
            "skipped file larger than 4 bytes: a.rs",
            "additional discovery warnings omitted",
        ]
    );
    fs::remove_dir_all(warning_root).unwrap();
}

fn test_directory(label: &str) -> PathBuf {
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "codex-code-memory-{label}-{}-{counter}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}
