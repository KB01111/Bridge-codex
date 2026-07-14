use std::time::Duration;

use axum::body::Body;
use axum::body::to_bytes;
use axum::http::Request;
use axum::http::StatusCode;
use axum::http::header::CONTENT_TYPE;
use pretty_assertions::assert_eq;
use serde_json::json;
use tower::ServiceExt;
use url::Url;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

use super::A2aServer;
use super::A2aServerStatus;
use super::MAX_PROMPT_BYTES;
use super::TaskState;
use crate::a2a_protocol::A2aTask;
use crate::a2a_protocol::TaskStatus;
use crate::a2a_task_store::MAX_RETAINED_TASKS;
use crate::a2a_task_store::TaskStore;
use crate::cli_proxy::CliProxyClient;

#[tokio::test]
async fn server_status_is_queryable_before_startup() {
    let server = MockServer::start().await;
    let client = CliProxyClient::new(
        Url::parse(&format!("{}/", server.uri())).expect("server URL"),
        /*api_key*/ None,
    )
    .expect("client");
    let status = A2aServer::new(client).status().await;

    assert_eq!(
        status,
        A2aServerStatus {
            running: false,
            address: "127.0.0.1:8120".to_string(),
            error: None,
        }
    );
}

#[tokio::test]
async fn agent_card_declares_v1_http_json_interface() {
    let server = MockServer::start().await;
    let client = CliProxyClient::new(
        Url::parse(&format!("{}/", server.uri())).expect("server URL"),
        /*api_key*/ None,
    )
    .expect("client");
    let response = A2aServer::new(client)
        .router()
        .oneshot(
            Request::builder()
                .uri("/.well-known/agent-card.json")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("agent card response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("agent card body");
    let card: serde_json::Value = serde_json::from_slice(&body).expect("agent card JSON");
    assert_eq!(
        card["supportedInterfaces"][0],
        json!({
            "url": "http://127.0.0.1:8120/a2a",
            "protocolBinding": "HTTP+JSON",
            "protocolVersion": "1.0"
        })
    );
}

#[tokio::test]
async fn task_list_uses_a2a_media_type() {
    let server = MockServer::start().await;
    let client = CliProxyClient::new(
        Url::parse(&format!("{}/", server.uri())).expect("server URL"),
        /*api_key*/ None,
    )
    .expect("client");
    let response = A2aServer::new(client)
        .router()
        .oneshot(
            Request::builder()
                .uri("/a2a/tasks")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("task list response");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[CONTENT_TYPE], "application/a2a+json");
}

#[tokio::test]
async fn delegated_task_reaches_completed_state_with_artifact() {
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{"message": {"content": "task result"}}]
        })))
        .mount(&mock_server)
        .await;
    let client = CliProxyClient::new(
        Url::parse(&format!("{}/", mock_server.uri())).expect("server URL"),
        /*api_key*/ None,
    )
    .expect("client");
    let server = A2aServer::new(client);
    let task = server
        .enqueue(
            "do work".to_string(),
            Some("model-a".to_string()),
            /*context_id*/ None,
        )
        .await
        .expect("task");

    let completed = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let current = server
                .tasks
                .read()
                .await
                .get(&task.id)
                .expect("stored task");
            if current.status.state != TaskState::Working {
                break current;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("task completion");

    assert_eq!(completed.status.state, TaskState::Completed);
    assert_eq!(completed.artifacts[0].parts[0].text, "task result");
}

#[tokio::test]
async fn delegate_rejects_oversized_prompt_before_upstream_work() {
    let mock_server = MockServer::start().await;
    let client = CliProxyClient::new(
        Url::parse(&format!("{}/", mock_server.uri())).expect("server URL"),
        /*api_key*/ None,
    )
    .expect("client");
    let response = A2aServer::new(client)
        .router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/a2a/tasks")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "prompt": "x".repeat(MAX_PROMPT_BYTES + 1),
                        "model": "model-a"
                    }))
                    .expect("request JSON"),
                ))
                .expect("request"),
        )
        .await
        .expect("delegate response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(
        mock_server
            .received_requests()
            .await
            .expect("received request log")
            .is_empty()
    );
}

#[tokio::test]
async fn task_store_is_bounded_and_lists_newest_first() {
    let mut store = TaskStore::default();
    let inserted = (0..MAX_RETAINED_TASKS + 2)
        .map(test_task)
        .collect::<Vec<_>>();
    for task in inserted.iter().cloned() {
        store.insert(task);
    }
    let expected = inserted[2..].iter().rev().cloned().collect::<Vec<_>>();

    assert_eq!(store.list_newest_first(), expected);
}

#[tokio::test]
async fn cancel_aborts_execution_and_preserves_canceled_state() {
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(1))
                .set_body_json(json!({
                    "choices": [{"message": {"content": "late result"}}]
                })),
        )
        .mount(&mock_server)
        .await;
    let client = CliProxyClient::new(
        Url::parse(&format!("{}/", mock_server.uri())).expect("server URL"),
        /*api_key*/ None,
    )
    .expect("client");
    let server = A2aServer::new(client);
    let task = server
        .enqueue(
            "do work".to_string(),
            Some("model-a".to_string()),
            /*context_id*/ None,
        )
        .await
        .expect("task");
    let canceled = server.cancel(&task.id).await.expect("cancel task");
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert_eq!(server.task(&task.id).await.expect("stored task"), canceled);
    assert_eq!(canceled.status.state, TaskState::Canceled);
    assert_eq!(canceled.artifacts, Vec::new());
}

fn test_task(index: usize) -> A2aTask {
    A2aTask {
        id: format!("task-{index:03}"),
        context_id: format!("context-{index:03}"),
        status: TaskStatus {
            state: TaskState::Completed,
            timestamp: format!("timestamp-{index:03}"),
            message: None,
        },
        artifacts: Vec::new(),
    }
}
