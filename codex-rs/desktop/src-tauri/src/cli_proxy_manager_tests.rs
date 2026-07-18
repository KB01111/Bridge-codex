use std::sync::Arc;
use std::sync::RwLock;

use codex_keyring_store::KeyringStore;
use codex_keyring_store::tests::MockKeyringStore;
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio::sync::Semaphore;
use url::Url;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::body_json;
use wiremock::matchers::body_partial_json;
use wiremock::matchers::method;
use wiremock::matchers::path;

use super::CliProxyManager;
use crate::cli_proxy::CliProxyClient;
use crate::cli_proxy::known_gateway_model_ids;
use crate::proxy_credentials::ProxyCredentialStore;

fn manager(base_url: &str, api_key: Option<String>) -> (CliProxyManager, ProxyCredentialStore) {
    let credentials = ProxyCredentialStore::with_keyring(Arc::new(MockKeyringStore::default()));
    let client = CliProxyClient::new(Url::parse(base_url).expect("base URL"), api_key)
        .expect("proxy client");
    (
        CliProxyManager {
            client,
            credentials: credentials.clone(),
            connection_gate: Arc::new(Semaphore::new(1)),
            startup_error: Arc::new(RwLock::new(None)),
        },
        credentials,
    )
}

fn assert_configuration_unchanged(manager: &CliProxyManager, credentials: &ProxyCredentialStore) {
    assert_eq!(
        manager.client.configured_base_url().expect("client URL"),
        "http://127.0.0.1:8317/"
    );
    assert_eq!(manager.client.api_key().expect("client key"), "working-key");
    assert_eq!(
        credentials.load_base_url().expect("stored URL"),
        Some("http://127.0.0.1:8317/".to_string())
    );
    assert_eq!(
        credentials.load().expect("stored key"),
        Some("working-key".to_string())
    );
}

#[tokio::test]
async fn unauthenticated_status_exposes_only_the_configured_base_url() {
    let (manager, _) = manager("http://127.0.0.1:8317/", /*api_key*/ None);

    let status = manager.status().await;
    let serialized = serde_json::to_string(&status).expect("serialize status");

    assert_eq!(status.base_url, "http://127.0.0.1:8317/");
    assert!(!status.authenticated);
    assert!(!serialized.contains("apiKey"));
    assert!(!serialized.contains("secret"));
}

#[tokio::test]
async fn corrupt_persisted_proxy_configuration_does_not_block_manager_startup() {
    let keyring = MockKeyringStore::default();
    keyring
        .save(
            "Bridge Codex",
            "cliproxyapi|base-url",
            "https://example.com/",
        )
        .expect("seed invalid URL");
    keyring
        .save("Bridge Codex", "cliproxyapi|api-key", "secret-value")
        .expect("seed key");
    let manager =
        CliProxyManager::from_store(ProxyCredentialStore::with_keyring(Arc::new(keyring)))
            .expect("manager construction");

    let status = manager.status().await;
    let serialized = serde_json::to_string(&status).expect("serialize status");

    assert!(!status.running);
    assert!(!status.authenticated);
    assert_eq!(
        status.compatibility,
        crate::cli_proxy::ProxyCompatibility::Unavailable
    );
    assert!(
        status
            .error
            .as_deref()
            .is_some_and(|error| error.contains("credentials are unavailable"))
    );
    assert!(!serialized.contains("secret-value"));
}

#[tokio::test]
async fn configuration_rejects_non_loopback_urls_before_storing_credentials() {
    let (manager, credentials) = manager("http://127.0.0.1:8317/", /*api_key*/ None);

    let error = manager
        .configure(
            "https://example.com/".to_string(),
            "secret-value".to_string(),
        )
        .await
        .expect_err("remote URL must fail");

    assert!(error.to_string().contains("loopback host"));
    assert_eq!(credentials.load().expect("stored key"), None);
    assert_eq!(credentials.load_base_url().expect("stored URL"), None);
}

#[tokio::test]
async fn incompatible_candidates_never_overwrite_a_working_configuration() {
    let (manager, credentials) = manager("http://127.0.0.1:8317/", Some("working-key".to_string()));
    credentials.save("working-key").expect("save working key");
    credentials
        .save_base_url("http://127.0.0.1:8317/")
        .expect("save working URL");
    let model = known_gateway_model_ids()
        .expect("bundled model catalog")
        .into_iter()
        .next()
        .expect("known model");

    let unauthorized = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&unauthorized)
        .await;
    manager
        .configure(
            format!("{}/", unauthorized.uri()),
            "candidate-key".to_string(),
        )
        .await
        .expect_err("invalid credentials must fail");
    assert_configuration_unchanged(&manager, &credentials);

    let chat_only = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{"id": model.clone()}]
        })))
        .mount(&chat_only)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_json(json!({})))
        .respond_with(ResponseTemplate::new(404))
        .mount(&chat_only)
        .await;
    manager
        .configure(format!("{}/", chat_only.uri()), "candidate-key".to_string())
        .await
        .expect_err("chat-only candidate must fail");
    assert_configuration_unchanged(&manager, &credentials);

    let malformed = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{"id": model}]
        })))
        .mount(&malformed)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_json(json!({})))
        .respond_with(ResponseTemplate::new(400))
        .mount(&malformed)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_partial_json(json!({
            "tool_choice": {"type": "function", "name": "bridge_probe"}
        })))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw("data: malformed\n\n", "text/event-stream"),
        )
        .mount(&malformed)
        .await;
    manager
        .configure(format!("{}/", malformed.uri()), "candidate-key".to_string())
        .await
        .expect_err("malformed SSE candidate must fail");
    assert_configuration_unchanged(&manager, &credentials);
}

#[tokio::test]
async fn clearing_credentials_removes_the_key_and_durable_url() {
    let (manager, credentials) =
        manager("http://127.0.0.1:8318/", Some("secret-value".to_string()));
    credentials.save("secret-value").expect("save key");
    credentials
        .save_base_url("http://127.0.0.1:8318/")
        .expect("save URL");

    manager
        .clear_credentials()
        .await
        .expect("clear credentials");

    assert!(!manager.client.has_api_key());
    assert_eq!(credentials.load().expect("stored key"), None);
    assert_eq!(credentials.load_base_url().expect("stored URL"), None);
}
