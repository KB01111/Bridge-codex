use pretty_assertions::assert_eq;

use super::BridgeToolExecutor;
use super::MAX_MEMORY_OUTPUT_BYTES;
use super::canonical_workspace_path;
use super::truncate_utf8;

#[test]
fn canonical_workspace_paths_reject_parent_traversal() {
    let root = tempfile::tempdir().expect("workspace root");
    assert_eq!(canonical_workspace_path(root.path(), "../secret"), None);
}

#[test]
fn utf8_truncation_preserves_character_boundaries() {
    let value = "aåäö";
    let truncated = truncate_utf8(value, 6);
    assert!(truncated.is_char_boundary(truncated.len()));
    assert!(truncated.len() <= 6);
}

#[tokio::test]
async fn browser_consent_is_scoped_and_revocable_per_thread() {
    let executor = BridgeToolExecutor::default();
    assert!(executor.require_browser_consent("thread-a").await.is_err());
    executor
        .grant_browser_consent("thread-a")
        .await
        .expect("grant consent");

    assert!(executor.has_browser_consent("thread-a").await);
    assert!(!executor.has_browser_consent("thread-b").await);

    executor.revoke_browser_consent("thread-a").await;
    assert!(!executor.has_browser_consent("thread-a").await);
    assert!(executor.require_browser_consent("thread-a").await.is_err());
}

#[test]
fn configured_memory_output_limit_is_eight_kibibytes() {
    assert_eq!(MAX_MEMORY_OUTPUT_BYTES, 8 * 1024);
}
