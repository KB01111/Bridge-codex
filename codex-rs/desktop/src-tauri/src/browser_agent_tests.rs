use pretty_assertions::assert_eq;

use super::BrowserAgent;
use super::BrowserHealth;
use super::BrowserStatus;
use super::cleanup_browser_profile_in;
use super::create_browser_profile_dir_in;
use super::is_managed_browser_profile;
use super::normalize_navigation_url;
use super::remove_managed_browser_profile;
use super::sweep_managed_browser_profiles_in;
use super::sweep_managed_browser_profiles_with;

#[test]
fn navigation_adds_https_and_rejects_privileged_schemes() {
    assert_eq!(
        normalize_navigation_url("example.com/path").expect("valid URL"),
        "https://example.com/path"
    );
    assert_eq!(
        normalize_navigation_url("file:///etc/passwd")
            .expect_err("file URL must be rejected")
            .to_string(),
        "agent browser navigation supports only http and https URLs"
    );
}

#[test]
fn navigation_rejects_oversized_addresses() {
    let address = format!("https://example.com/{}", "x".repeat(8 * 1024));

    let error = normalize_navigation_url(&address).expect_err("oversized URL must fail");

    assert!(error.to_string().contains("browser address exceeds"));
}

#[tokio::test]
async fn default_browser_is_stopped_and_healthy() {
    let browser = BrowserAgent::default();

    assert_eq!(
        browser.status().await,
        BrowserStatus {
            running: false,
            url: None,
            viewport_width: 1280,
            viewport_height: 720,
            health: BrowserHealth::Stopped,
            error: None,
        }
    );
}

#[test]
fn navigation_rejects_embedded_credentials() {
    let error = normalize_navigation_url("https://user:secret@example.com")
        .expect_err("credentials must be rejected");

    assert!(error.to_string().contains("embedded credentials"));
}

#[test]
fn browser_profiles_are_unique_ephemeral_and_safely_removed() {
    let temp = tempfile::tempdir().expect("temporary root");
    let first = create_browser_profile_dir_in(temp.path()).expect("first profile");
    let second = create_browser_profile_dir_in(temp.path()).expect("second profile");
    std::fs::write(first.join("Cookies"), b"ephemeral").expect("profile fixture");

    assert_ne!(first, second);
    assert!(is_managed_browser_profile(&first, temp.path()));
    assert!(is_managed_browser_profile(&second, temp.path()));
    remove_managed_browser_profile(&first, temp.path()).expect("remove managed profile");
    assert!(!first.exists());
    assert!(second.exists());
}

#[test]
fn browser_profile_cleanup_refuses_unmanaged_directories() {
    let temp = tempfile::tempdir().expect("temporary root");
    let unrelated = temp.path().join("unrelated");
    std::fs::create_dir(&unrelated).expect("unrelated directory");

    let error = remove_managed_browser_profile(&unrelated, temp.path())
        .expect_err("unmanaged directory must be preserved");

    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(unrelated.exists());
}

#[tokio::test]
async fn orphan_sweep_removes_only_managed_browser_profiles() {
    let temp = tempfile::tempdir().expect("temporary root");
    let first = create_browser_profile_dir_in(temp.path()).expect("first profile");
    let second = create_browser_profile_dir_in(temp.path()).expect("second profile");
    let unrelated = temp.path().join("unrelated");
    std::fs::create_dir(&unrelated).expect("unrelated directory");

    sweep_managed_browser_profiles_in(temp.path())
        .await
        .expect("orphan sweep");

    assert!(!first.exists());
    assert!(!second.exists());
    assert!(unrelated.exists());
}

#[tokio::test]
async fn cleanup_reports_and_preserves_an_unmanaged_profile_path() {
    let temp = tempfile::tempdir().expect("temporary root");
    let unrelated = temp.path().join("unrelated");
    std::fs::create_dir(&unrelated).expect("unrelated directory");

    let error = cleanup_browser_profile_in(unrelated.clone(), temp.path())
        .await
        .expect_err("unmanaged cleanup must fail visibly");

    assert!(
        error
            .to_string()
            .contains("failed to remove ephemeral Chromium profile")
    );
    assert!(unrelated.exists());
}

#[tokio::test]
async fn orphan_sweep_preserves_live_owned_profiles_and_removes_confirmed_stale_profiles() {
    let temp = tempfile::tempdir().expect("temporary root");
    let live_pid = 41_001_u32;
    let stale_pid = 41_002_u32;
    let live = temp.path().join(format!(
        "{}{}-{}",
        super::BROWSER_PROFILE_PREFIX,
        live_pid,
        uuid::Uuid::new_v4()
    ));
    let stale = temp.path().join(format!(
        "{}{}-{}",
        super::BROWSER_PROFILE_PREFIX,
        stale_pid,
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir(&live).expect("live profile");
    std::fs::create_dir(&stale).expect("stale profile");

    let error = sweep_managed_browser_profiles_with(temp.path(), |pid| pid == live_pid)
        .await
        .expect_err("a live owner must block cleanup completion");

    assert!(error.to_string().contains("owned by live Bridge process"));
    assert!(live.exists());
    assert!(!stale.exists());
}
