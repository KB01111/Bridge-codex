use pretty_assertions::assert_eq;
use serde_json::json;
use url::Url;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::Request;
use wiremock::ResponseTemplate;
use wiremock::matchers::body_json;
use wiremock::matchers::body_partial_json;
use wiremock::matchers::header;
use wiremock::matchers::method;
use wiremock::matchers::path;

use super::CliProxyClient;
use super::ProxyCapabilities;
use super::ProxyCompatibility;
use super::ProxyModel;
use super::ProxyModelClassification;
use super::known_gateway_model_ids;
use crate::cli_proxy_http::MAX_MODELS_RESPONSE_BYTES;

fn known_model_id() -> String {
    known_gateway_model_ids()
        .expect("bundled model catalog")
        .into_iter()
        .next()
        .expect("bundled catalog should expose a stable API model")
}

#[tokio::test]
async fn fetch_models_authenticates_sorts_and_deduplicates() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("authorization", "Bearer secret"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list",
            "data": [
                {"id": "model-z", "object": "model", "owned_by": "codex"},
                {"id": "model-a", "object": "model", "owned_by": "codex"},
                {"id": "model-a", "object": "model", "owned_by": "codex"}
            ]
        })))
        .mount(&server)
        .await;
    let client = client(&server, Some("secret"));

    assert_eq!(
        client.fetch_models().await.expect("models"),
        vec![
            ProxyModel {
                id: "model-a".to_string(),
                object: Some("model".to_string()),
                owned_by: Some("codex".to_string()),
                classification: ProxyModelClassification::Experimental,
            },
            ProxyModel {
                id: "model-z".to_string(),
                object: Some("model".to_string()),
                owned_by: Some("codex".to_string()),
                classification: ProxyModelClassification::Experimental,
            },
        ]
    );
}

#[tokio::test]
async fn capability_probe_requires_authenticated_models_and_responses_routes() {
    let server = MockServer::start().await;
    let model = known_model_id();
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("authorization", "Bearer secret"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{"id": model.clone(), "object": "model", "owned_by": "bridge"}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_partial_json(json!({
            "tool_choice": {"type": "function", "name": "bridge_probe"}
        })))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            concat!(
                "data: {\"type\":\"response.output_item.done\",\"item\":{",
                "\"type\":\"function_call\",\"call_id\":\"call-1\",",
                "\"name\":\"bridge_probe\",\"arguments\":\"{\\\"value\\\":\\\"ping\\\"}\"}}\n\n",
                "data: {\"type\":\"response.completed\",\"response\":{\"output\":[]}}\n\n"
            ),
            "text/event-stream",
        ))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_partial_json(json!({"tool_choice": "none"})))
        .respond_with(|request: &Request| {
            let body: serde_json::Value =
                serde_json::from_slice(&request.body).expect("continuation body");
            let marker = body["input"]
                .as_array()
                .and_then(|input| {
                    input
                        .iter()
                        .find(|item| item["type"] == "function_call_output")
                })
                .and_then(|item| item["output"].as_str())
                .expect("probe marker");
            ResponseTemplate::new(200).set_body_raw(
                format!(
                    "data: {}\n\n",
                    json!({
                        "type": "response.completed",
                        "response": {
                            "output": [{
                                "type": "message",
                                "role": "assistant",
                                "content": [{"type": "output_text", "text": marker}]
                            }]
                        }
                    })
                ),
                "text/event-stream",
            )
        })
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(header("authorization", "Bearer secret"))
        .and(body_json(json!({})))
        .respond_with(ResponseTemplate::new(400))
        .expect(1)
        .mount(&server)
        .await;
    let client = client(&server, Some("secret"));

    let capabilities = client.probe_capabilities().await.expect("capabilities");

    assert_eq!(
        capabilities,
        ProxyCapabilities {
            models_api: true,
            responses_api: true,
            compatibility: ProxyCompatibility::Conformant,
            conformance_error: None,
            probed_model: Some(model),
            experimental_model_count: 0,
        }
    );
}

#[tokio::test]
async fn capability_probe_reports_a_missing_responses_route() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    assert_eq!(
        client(&server, Some("secret"))
            .probe_capabilities()
            .await
            .expect("capabilities"),
        ProxyCapabilities {
            models_api: true,
            responses_api: false,
            compatibility: ProxyCompatibility::Unavailable,
            conformance_error: None,
            probed_model: None,
            experimental_model_count: 0,
        }
    );
}

#[tokio::test]
async fn capability_probe_classifies_malformed_responses_sse_as_basic() {
    let server = MockServer::start().await;
    let model = known_model_id();
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{"id": model.clone()}]
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_json(json!({})))
        .respond_with(ResponseTemplate::new(400))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_partial_json(json!({
            "tool_choice": {"type": "function", "name": "bridge_probe"}
        })))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw("data: not-json\n\n", "text/event-stream"),
        )
        .mount(&server)
        .await;

    let capabilities = client(&server, Some("secret"))
        .probe_capabilities()
        .await
        .expect("capabilities");

    assert!(capabilities.models_api);
    assert!(capabilities.responses_api);
    assert_eq!(capabilities.compatibility, ProxyCompatibility::Basic);
    assert_eq!(capabilities.probed_model, Some(model));
    assert_eq!(capabilities.experimental_model_count, 0);
    assert!(
        capabilities.conformance_error.as_deref().is_some_and(
            |error| error.contains("invalid CLIProxyAPI forced function call SSE stream")
        )
    );
}

#[tokio::test]
async fn unrecognized_models_remain_experimental_and_are_not_probed() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{"id": "vendor-model-next"}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_json(json!({})))
        .respond_with(ResponseTemplate::new(400))
        .expect(1)
        .mount(&server)
        .await;

    let capabilities = client(&server, Some("secret"))
        .probe_capabilities()
        .await
        .expect("capabilities");

    assert_eq!(capabilities.compatibility, ProxyCompatibility::Basic);
    assert_eq!(capabilities.probed_model, None);
    assert_eq!(capabilities.experimental_model_count, 1);
    assert!(
        capabilities
            .conformance_error
            .as_deref()
            .is_some_and(|error| error.contains("unrecognized experimental models"))
    );
    assert_eq!(
        server
            .received_requests()
            .await
            .expect("request recording")
            .len(),
        2
    );
}

#[tokio::test]
async fn requests_fail_closed_without_an_api_key() {
    let server = MockServer::start().await;
    let error = client(&server, /*api_key*/ None)
        .fetch_models()
        .await
        .expect_err("missing key must fail");

    assert!(
        error
            .to_string()
            .contains("authentication is not configured")
    );
    assert!(
        server
            .received_requests()
            .await
            .expect("request recording")
            .is_empty()
    );
}

#[test]
fn client_rejects_non_loopback_or_path_bearing_origins() {
    let remote_error = CliProxyClient::new(
        Url::parse("http://example.com:8317/").expect("remote URL"),
        Some("secret".to_string()),
    )
    .err()
    .expect("remote host must fail");
    let path_error = CliProxyClient::new(
        Url::parse("http://127.0.0.1:8317/proxy/").expect("path URL"),
        Some("secret".to_string()),
    )
    .err()
    .expect("path-bearing origin must fail");

    assert!(remote_error.to_string().contains("loopback host"));
    assert!(path_error.to_string().contains("without a path"));
}

#[tokio::test]
async fn client_does_not_follow_redirects() {
    let source = MockServer::start().await;
    let target = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", format!("{}/capture", target.uri())),
        )
        .mount(&source)
        .await;

    let error = client(&source, Some("secret"))
        .fetch_models()
        .await
        .expect_err("redirect must fail");

    assert!(error.to_string().contains("302"));
    assert!(
        target
            .received_requests()
            .await
            .expect("request recording")
            .is_empty()
    );
}

#[tokio::test]
async fn model_bodies_are_bounded_and_error_bodies_are_never_exposed() {
    let oversized = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(
            ResponseTemplate::new(200).set_body_bytes(vec![b'x'; MAX_MODELS_RESPONSE_BYTES + 1]),
        )
        .mount(&oversized)
        .await;
    let model_error = client(&oversized, Some("secret"))
        .fetch_models()
        .await
        .expect_err("oversized model body must fail");

    let remote_error = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(
            ResponseTemplate::new(500).set_body_string("echoed-secret-value".repeat(2 * 1024)),
        )
        .mount(&remote_error)
        .await;
    let bounded_error = client(&remote_error, Some("secret"))
        .fetch_models()
        .await
        .expect_err("remote error must fail")
        .to_string();

    assert!(model_error.to_string().contains("exceeds the"));
    assert!(bounded_error.contains("models request"));
    assert!(bounded_error.contains("500"));
    assert!(bounded_error.contains("response body omitted"));
    assert!(!bounded_error.contains("echoed-secret-value"));
}

fn client(server: &MockServer, api_key: Option<&str>) -> CliProxyClient {
    CliProxyClient::new(
        Url::parse(&format!("{}/", server.uri())).expect("server URL"),
        api_key.map(ToString::to_string),
    )
    .expect("client")
}
