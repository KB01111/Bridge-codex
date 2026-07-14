use pretty_assertions::assert_eq;
use serde_json::json;
use url::Url;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::body_json;
use wiremock::matchers::header;
use wiremock::matchers::method;
use wiremock::matchers::path;

use super::ChatMessage;
use super::ChatRequest;
use super::ChatRole;
use super::CliProxyClient;
use super::MAX_CHAT_STREAM_BYTES;
use super::MAX_CONCURRENT_CHATS;
use super::ProxyModel;
use crate::code_policy::SANDBOX_EXECUTION_SYSTEM_PROMPT;

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
    let client = CliProxyClient::new(
        Url::parse(&format!("{}/", server.uri())).expect("server URL"),
        Some("secret".to_string()),
    )
    .expect("client");

    assert_eq!(
        client.fetch_models().await.expect("models"),
        vec![
            ProxyModel {
                id: "model-a".to_string(),
                object: Some("model".to_string()),
                owned_by: Some("codex".to_string()),
            },
            ProxyModel {
                id: "model-z".to_string(),
                object: Some("model".to_string()),
                owned_by: Some("codex".to_string()),
            },
        ]
    );
}

#[tokio::test]
async fn complete_chat_uses_only_the_chat_completions_route() {
    let server = MockServer::start().await;
    let request = ChatRequest {
        model: "model-a".to_string(),
        messages: vec![ChatMessage {
            role: ChatRole::User,
            content: "hello".to_string(),
        }],
    };
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_json(json!({
            "model": "model-a",
            "messages": [
                {"role": "system", "content": SANDBOX_EXECUTION_SYSTEM_PROMPT},
                {"role": "user", "content": "hello"}
            ],
            "stream": false
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{"message": {"content": "hello back"}}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let client = CliProxyClient::new(
        Url::parse(&format!("{}/", server.uri())).expect("server URL"),
        /*api_key*/ None,
    )
    .expect("client");

    assert_eq!(
        client.complete_chat(&request).await.expect("completion"),
        "hello back"
    );
}

#[tokio::test]
async fn streamed_chat_returns_tree_sitter_validation() {
    let server = MockServer::start().await;
    let request = ChatRequest {
        model: "model-a".to_string(),
        messages: vec![ChatMessage {
            role: ChatRole::User,
            content: "write a program".to_string(),
        }],
    };
    let code = "```rust\\nfn main() { println!(\\\"ok\\\"); }\\n```\\n\\nExpected stdout: ok";
    let event = format!(
        "data: {{\"choices\":[{{\"delta\":{{\"content\":\"{code}\"}}}}]}}\n\ndata: [DONE]\n\n"
    );
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_json(json!({
            "model": "model-a",
            "messages": [
                {"role": "system", "content": SANDBOX_EXECUTION_SYSTEM_PROMPT},
                {"role": "user", "content": "write a program"}
            ],
            "stream": true
        })))
        .respond_with(ResponseTemplate::new(200).set_body_raw(event, "text/event-stream"))
        .expect(1)
        .mount(&server)
        .await;
    let client = CliProxyClient::new(
        Url::parse(&format!("{}/", server.uri())).expect("server URL"),
        /*api_key*/ None,
    )
    .expect("client");
    let mut streamed = String::new();

    let validation = client
        .stream_chat(&request, |delta| streamed.push_str(&delta))
        .await
        .expect("stream");

    assert!(validation.valid);
    assert_eq!(
        streamed,
        "```rust\nfn main() { println!(\"ok\"); }\n```\n\nExpected stdout: ok"
    );
}

#[tokio::test]
async fn client_does_not_follow_proxy_redirects() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(
            ResponseTemplate::new(307)
                .insert_header("location", format!("{}/redirected", server.uri())),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/redirected"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
        .expect(0)
        .mount(&server)
        .await;
    let client = CliProxyClient::new(
        Url::parse(&format!("{}/", server.uri())).expect("server URL"),
        /*api_key*/ None,
    )
    .expect("client");

    let error = client.fetch_models().await.expect_err("redirect must fail");

    assert!(error.to_string().contains("307 Temporary Redirect"));
}

#[tokio::test]
async fn proxy_error_bodies_are_bounded() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(500).set_body_string("x".repeat(64 * 1024)))
        .mount(&server)
        .await;
    let client = CliProxyClient::new(
        Url::parse(&format!("{}/", server.uri())).expect("server URL"),
        /*api_key*/ None,
    )
    .expect("client");

    let error = client.fetch_models().await.expect_err("error response");
    let message = error.to_string();

    assert!(message.len() < 9 * 1024);
    assert!(message.ends_with("[truncated]"));
}

#[tokio::test]
async fn streamed_chat_requires_done_marker_and_content() {
    let server = MockServer::start().await;
    let partial = "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n";
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(partial, "text/event-stream"))
        .mount(&server)
        .await;
    let client = CliProxyClient::new(
        Url::parse(&format!("{}/", server.uri())).expect("server URL"),
        /*api_key*/ None,
    )
    .expect("client");
    let request = ChatRequest {
        model: "model-a".to_string(),
        messages: vec![ChatMessage {
            role: ChatRole::User,
            content: "hello".to_string(),
        }],
    };

    let error = client
        .stream_chat(&request, |_| {})
        .await
        .expect_err("partial stream must fail");

    assert!(error.to_string().contains("before the [DONE] marker"));

    let empty_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw("data: [DONE]\n\n", "text/event-stream"),
        )
        .mount(&empty_server)
        .await;
    let empty_client = CliProxyClient::new(
        Url::parse(&format!("{}/", empty_server.uri())).expect("server URL"),
        /*api_key*/ None,
    )
    .expect("client");

    let error = empty_client
        .stream_chat(&request, |_| {})
        .await
        .expect_err("empty stream must fail");
    assert!(error.to_string().contains("no assistant content"));
}

#[tokio::test]
async fn complete_and_streamed_chat_reject_empty_or_unbounded_content() {
    let empty_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{"message": {"content": "   "}}]
        })))
        .mount(&empty_server)
        .await;
    let empty_client = CliProxyClient::new(
        Url::parse(&format!("{}/", empty_server.uri())).expect("server URL"),
        /*api_key*/ None,
    )
    .expect("client");
    let request = ChatRequest {
        model: "model-a".to_string(),
        messages: vec![ChatMessage {
            role: ChatRole::User,
            content: "hello".to_string(),
        }],
    };

    let empty_error = empty_client
        .complete_chat(&request)
        .await
        .expect_err("empty completion must fail");
    assert!(empty_error.to_string().contains("no assistant content"));

    let oversized_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            format!("data: {}\n\n", "x".repeat(MAX_CHAT_STREAM_BYTES)),
            "text/event-stream",
        ))
        .mount(&oversized_server)
        .await;
    let oversized_client = CliProxyClient::new(
        Url::parse(&format!("{}/", oversized_server.uri())).expect("server URL"),
        /*api_key*/ None,
    )
    .expect("client");

    let stream_error = oversized_client
        .stream_chat(&request, |_| {})
        .await
        .expect_err("oversized stream must fail");
    assert!(stream_error.to_string().contains("wire limit"));
}

#[tokio::test]
async fn chat_concurrency_is_bounded_for_client_clones() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(std::time::Duration::from_millis(250))
                .set_body_json(json!({
                    "choices": [{"message": {"content": "ok"}}]
                })),
        )
        .mount(&server)
        .await;
    let client = CliProxyClient::new(
        Url::parse(&format!("{}/", server.uri())).expect("server URL"),
        /*api_key*/ None,
    )
    .expect("client");
    let request = ChatRequest {
        model: "model-a".to_string(),
        messages: vec![ChatMessage {
            role: ChatRole::User,
            content: "hello".to_string(),
        }],
    };
    let handles = (0..=MAX_CONCURRENT_CHATS)
        .map(|_| {
            let client = client.clone();
            let request = request.clone();
            tokio::spawn(async move { client.complete_chat(&request).await })
        })
        .collect::<Vec<_>>();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let received = server
        .received_requests()
        .await
        .expect("request recording is enabled");
    assert_eq!(received.len(), MAX_CONCURRENT_CHATS);

    for handle in handles {
        assert_eq!(
            handle.await.expect("chat task completed").expect("chat"),
            "ok"
        );
    }
}
