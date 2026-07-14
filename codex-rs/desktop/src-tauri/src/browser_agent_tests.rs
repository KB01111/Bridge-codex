use pretty_assertions::assert_eq;

use super::BrowserAgent;
use super::BrowserHealth;
use super::BrowserStatus;
use super::normalize_navigation_url;

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
