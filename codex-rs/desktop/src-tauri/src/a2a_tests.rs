use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::Result;
use anyhow::bail;
use axum::body::Body;
use axum::body::to_bytes;
use axum::http::Request;
use axum::http::StatusCode;
use axum::http::header::AUTHORIZATION;
use axum::http::header::CONTENT_TYPE;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStatus;
use codex_keyring_store::KeyringStore;
use codex_keyring_store::tests::MockKeyringStore;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tokio::sync::OwnedSemaphorePermit;
use tokio::sync::Semaphore;
use tokio::sync::broadcast;
use tower::ServiceExt;

use super::A2aCredentialStore;
use super::A2aHttpConfig;
use super::A2aPersistence;
use super::A2aServer;
use super::A2aServerStatus;
use super::MAX_A2A_BODY_BYTES;
use super::MAX_PROMPT_BYTES;
use super::constant_time_eq;
use super::lifecycle::A2aServerComponents;
use super::runtime::A2aRuntimeBackend;
use super::runtime::A2aRuntimeClient;
use super::validate_a2a_token;
use crate::a2a_protocol::A2aServerSettings;
use crate::a2a_protocol::A2aTask;
use crate::a2a_protocol::TaskState;
use crate::a2a_protocol::TaskStatus;
use crate::a2a_task_store::MAX_RETAINED_TASKS;
use crate::a2a_task_store::TaskStore;
use crate::agent_runtime_protocol::BridgeAgentEvent;
use crate::agent_runtime_protocol::BridgeAgentRequest;

const TEST_TOKEN: &str = "bridge-a2a-test-token-0000000000000000";

#[tokio::test]
async fn server_status_is_queryable_before_startup() {
    let status = test_server(TestServerOptions::default()).status().await;

    assert_eq!(
        status,
        A2aServerStatus {
            enabled: false,
            running: false,
            address: "127.0.0.1:8120".to_string(),
            token_configured: false,
            error: None,
        }
    );
}

#[tokio::test]
async fn agent_card_declares_v1_http_json_interface() {
    let response = test_server(TestServerOptions {
        settings: A2aServerSettings {
            enabled: true,
            port: 18_120,
        },
        token: Some(TEST_TOKEN.to_string()),
        ..TestServerOptions::default()
    })
    .router()
    .oneshot(
        Request::builder()
            .uri("/.well-known/agent-card.json")
            .header(AUTHORIZATION, format!("Bearer {TEST_TOKEN}"))
            .body(Body::empty())
            .expect("request"),
    )
    .await
    .expect("agent card response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("agent card body");
    let card: Value = serde_json::from_slice(&body).expect("agent card JSON");
    assert_eq!(
        card["supportedInterfaces"][0],
        json!({
            "url": "http://127.0.0.1:18120/a2a",
            "protocolBinding": "HTTP+JSON",
            "protocolVersion": "1.0"
        })
    );
}

#[tokio::test]
async fn task_list_uses_a2a_media_type() {
    let response = enabled_server()
        .router()
        .oneshot(
            Request::builder()
                .uri("/a2a/tasks")
                .header(AUTHORIZATION, format!("Bearer {TEST_TOKEN}"))
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
    let (server, runtime) = server_with_runtime(TestServerOptions::default());
    let task = server
        .enqueue(
            "do work".to_string(),
            Some("model-a".to_string()),
            /*context_id*/ None,
        )
        .await
        .expect("task");
    runtime.emit(turn_completed(
        &task.context_id,
        &runtime.last_turn_id(),
        TurnStatus::Completed,
        Some("task result"),
    ));

    let completed = wait_for_terminal_task(&server, &task.id).await;

    assert_eq!(completed.status.state, TaskState::Completed);
    assert_eq!(completed.artifacts[0].parts[0].text, "task result");
}

#[tokio::test]
async fn delegated_approval_request_keeps_task_working_until_turn_completes() {
    let (server, runtime) = server_with_runtime(TestServerOptions::default());
    let task = server
        .enqueue(
            "approved work".to_string(),
            /*model*/ None,
            /*context_id*/ None,
        )
        .await
        .expect("task");

    runtime.emit(BridgeAgentEvent::ServerRequest {
        payload: json!({"id": 42, "method": "item/commandExecution/requestApproval"}),
    });
    tokio::time::sleep(Duration::from_millis(25)).await;
    assert_eq!(
        server
            .task(&task.id)
            .await
            .expect("working task")
            .status
            .state,
        TaskState::Working
    );

    runtime.emit(turn_completed(
        &task.context_id,
        &runtime.last_turn_id(),
        TurnStatus::Completed,
        Some("approved result"),
    ));
    assert_eq!(
        wait_for_terminal_task(&server, &task.id).await.status.state,
        TaskState::Completed
    );
}

#[tokio::test]
async fn delegation_resumes_the_durable_context_thread() {
    let (server, runtime) = server_with_runtime(TestServerOptions::default());

    let task = server
        .enqueue(
            "continue".to_string(),
            /*model*/ None,
            Some("thread-existing".to_string()),
        )
        .await
        .expect("task");

    assert_eq!(task.context_id, "thread-existing");
    assert!(runtime.requests().iter().any(|request| matches!(
        request,
        BridgeAgentRequest::ThreadResume(params) if params.thread_id == "thread-existing"
    )));
}

#[tokio::test]
async fn delegate_rejects_oversized_prompt_before_runtime_work() {
    let (server, runtime) = server_with_runtime(TestServerOptions {
        settings: A2aServerSettings {
            enabled: true,
            port: 8120,
        },
        token: Some(TEST_TOKEN.to_string()),
        ..TestServerOptions::default()
    });
    let response = server
        .router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/a2a/tasks")
                .header(AUTHORIZATION, format!("Bearer {TEST_TOKEN}"))
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
    assert_eq!(runtime.requests(), Vec::new());
}

#[tokio::test]
async fn http_routes_require_the_configured_bearer_token() {
    let router = enabled_server().router();
    let missing = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/.well-known/agent-card.json")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("missing token response");
    let wrong = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/.well-known/agent-card.json")
                .header(AUTHORIZATION, "Bearer wrong-token")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("wrong token response");
    let valid = router
        .oneshot(
            Request::builder()
                .uri("/.well-known/agent-card.json")
                .header(AUTHORIZATION, format!("Bearer {TEST_TOKEN}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("valid token response");

    assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(valid.status(), StatusCode::OK);
}

#[tokio::test]
async fn http_routes_enforce_body_rate_and_concurrency_limits() {
    let mut config = A2aHttpConfig::production();
    config.max_requests_per_window = 2;
    config.max_concurrent_requests = 1;
    let server = test_server(TestServerOptions {
        settings: A2aServerSettings {
            enabled: true,
            port: 8120,
        },
        token: Some(TEST_TOKEN.to_string()),
        http_config: config,
        ..TestServerOptions::default()
    });
    let router = server.clone().router();

    let oversized = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/a2a/tasks")
                .header(AUTHORIZATION, format!("Bearer {TEST_TOKEN}"))
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from("x".repeat(MAX_A2A_BODY_BYTES + 1)))
                .expect("request"),
        )
        .await
        .expect("oversized response");
    assert_eq!(oversized.status(), StatusCode::PAYLOAD_TOO_LARGE);

    let permit = server
        .request_gate
        .clone()
        .acquire_owned()
        .await
        .expect("request gate permit");
    let concurrent = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/.well-known/agent-card.json")
                .header(AUTHORIZATION, format!("Bearer {TEST_TOKEN}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("concurrency response");
    drop(permit);
    assert_eq!(concurrent.status(), StatusCode::SERVICE_UNAVAILABLE);

    let rate_limited = router
        .oneshot(
            Request::builder()
                .uri("/.well-known/agent-card.json")
                .header(AUTHORIZATION, format!("Bearer {TEST_TOKEN}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("rate response");
    assert_eq!(rate_limited.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[test]
fn token_validation_requires_a_strong_header_safe_secret() {
    assert!(validate_a2a_token(Some(TEST_TOKEN)).is_ok());
    assert!(validate_a2a_token(None).is_err());
    assert!(validate_a2a_token(Some("too-short")).is_err());
    assert!(validate_a2a_token(Some(&format!("{} ", "x".repeat(32)))).is_err());
    assert!(constant_time_eq(TEST_TOKEN, TEST_TOKEN));
    assert!(!constant_time_eq(TEST_TOKEN, "different-token"));
}

#[tokio::test]
async fn token_provisioning_returns_the_secret_once_and_status_never_contains_it() {
    let server = test_server(TestServerOptions::default());
    let generated = server.generate_token().await.expect("generate token");
    let duplicate = server
        .generate_token()
        .await
        .err()
        .expect("duplicate generation must fail");
    let replacement = server.regenerate_token().await.expect("regenerate token");
    let replacement_status_json =
        serde_json::to_string(&replacement.status).expect("serialize safe status");
    let deleted = server.delete_token().await.expect("delete token");

    assert!(validate_a2a_token(Some(&generated.token)).is_ok());
    assert_ne!(generated.token, replacement.token);
    assert!(generated.status.token_configured);
    assert!(replacement.status.token_configured);
    assert!(!replacement_status_json.contains(&replacement.token));
    assert_eq!(
        duplicate.to_string(),
        "an A2A bearer token is already configured; regenerate it explicitly"
    );
    assert!(!deleted.token_configured);
}

#[tokio::test]
async fn regenerated_and_deleted_tokens_take_effect_immediately() {
    let server = enabled_server();
    let router = server.clone().router();
    let replacement = server.regenerate_token().await.expect("regenerate token");
    let stale = authorized_agent_card(&router, TEST_TOKEN).await;
    let current = authorized_agent_card(&router, &replacement.token).await;
    server.delete_token().await.expect("delete token");
    let deleted = authorized_agent_card(&router, &replacement.token).await;

    assert_eq!(stale, StatusCode::UNAUTHORIZED);
    assert_eq!(current, StatusCode::OK);
    assert_eq!(deleted, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn token_provisioning_and_deletion_are_serialized() {
    let credentials = A2aCredentialStore::with_keyring(Arc::new(MockKeyringStore::default()));
    let (server, _) = server_with_credentials(TestServerOptions::default(), credentials.clone());
    let gate_blocker = server
        .server_gate
        .clone()
        .acquire_owned()
        .await
        .expect("server gate");
    let mut provisioning = Box::pin(server.generate_token());
    assert!(
        tokio::time::timeout(Duration::from_millis(25), provisioning.as_mut())
            .await
            .is_err()
    );
    let mut deletion = Box::pin(server.delete_token());
    assert!(
        tokio::time::timeout(Duration::from_millis(25), deletion.as_mut())
            .await
            .is_err()
    );

    drop(gate_blocker);
    let generated = tokio::time::timeout(Duration::from_secs(1), provisioning.as_mut())
        .await
        .expect("token provisioning timeout")
        .expect("provision token");
    let deleted = tokio::time::timeout(Duration::from_secs(1), deletion.as_mut())
        .await
        .expect("token deletion timeout")
        .expect("delete token");

    assert!(validate_a2a_token(Some(&generated.token)).is_ok());
    assert!(!deleted.token_configured);
    assert_eq!(credentials.load().expect("load credentials"), None);
    assert_eq!(server.status().await, deleted);
}

#[tokio::test]
async fn shutdown_rejects_queued_token_provisioning_before_final_deletion() {
    let credentials = A2aCredentialStore::with_keyring(Arc::new(MockKeyringStore::default()));
    credentials.save(TEST_TOKEN).expect("seed A2A token");
    let (server, _) = server_with_credentials(
        TestServerOptions {
            token: Some(TEST_TOKEN.to_string()),
            ..TestServerOptions::default()
        },
        credentials.clone(),
    );
    let gate_blocker = server
        .server_gate
        .clone()
        .acquire_owned()
        .await
        .expect("server gate");
    let cleanup_server = server.clone();
    let cleanup = tokio::spawn(async move { cleanup_server.shutdown_and_delete_token().await });
    tokio::time::timeout(Duration::from_secs(1), async {
        while !server.terminated.load(Ordering::Acquire) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("shutdown start");
    let provisioning_server = server.clone();
    let provisioning = tokio::spawn(async move { provisioning_server.regenerate_token().await });

    drop(gate_blocker);
    let deleted = cleanup
        .await
        .expect("cleanup task")
        .expect("shutdown and delete token");
    let provisioning_error = match provisioning.await.expect("provisioning task") {
        Ok(_) => panic!("shutdown must reject token provisioning"),
        Err(error) => error,
    };

    assert!(!deleted.token_configured);
    assert_eq!(credentials.load().expect("load credentials"), None);
    assert_eq!(
        provisioning_error.to_string(),
        "A2A server is shutting down"
    );
}

#[tokio::test]
async fn persisted_tasks_recover_context_mapping_after_restart() {
    let temp = tempfile::tempdir().expect("temporary app data");
    let persistence = A2aPersistence::new(temp.path().to_path_buf());
    persistence
        .save_settings(A2aServerSettings {
            enabled: false,
            port: 18_120,
        })
        .await
        .expect("persist settings");
    persistence
        .save_tasks(vec![A2aTask {
            id: "task-restart".to_string(),
            context_id: "thread-restart".to_string(),
            status: TaskStatus {
                state: TaskState::Working,
                timestamp: "before-restart".to_string(),
                message: None,
            },
            artifacts: Vec::new(),
        }])
        .await
        .expect("persist working task");
    let credentials = A2aCredentialStore::with_keyring(Arc::new(MockKeyringStore::default()));

    let restored = A2aServer::from_app_data_dir_with_credentials(
        A2aRuntimeClient::with_backend(Arc::new(FakeRuntime::default())),
        temp.path().to_path_buf(),
        credentials.clone(),
    );
    let recovered = restored.task("task-restart").await.expect("recovered task");
    let restored_again = A2aServer::from_app_data_dir_with_credentials(
        A2aRuntimeClient::with_backend(Arc::new(FakeRuntime::default())),
        temp.path().to_path_buf(),
        credentials,
    );

    assert_eq!(recovered.context_id, "thread-restart");
    assert_eq!(recovered.status.state, TaskState::Failed);
    assert_eq!(restored.status().await.address, "127.0.0.1:18120");
    assert_eq!(
        restored_again
            .task("task-restart")
            .await
            .expect("durable recovered task")
            .status
            .state,
        TaskState::Failed
    );
}

#[tokio::test]
async fn corrupt_persistence_and_invalid_keyring_token_fail_closed_without_blocking_construction() {
    let temp = tempfile::tempdir().expect("temporary app data");
    let a2a_dir = temp.path().join("a2a");
    std::fs::create_dir_all(&a2a_dir).expect("A2A data directory");
    std::fs::write(a2a_dir.join("settings-v1.json"), b"not-json")
        .expect("corrupt settings fixture");
    let keyring = MockKeyringStore::default();
    keyring
        .save("Bridge Codex", "a2a|bearer-token", "invalid")
        .expect("seed invalid token");

    let server = A2aServer::from_app_data_dir_with_credentials(
        A2aRuntimeClient::with_backend(Arc::new(FakeRuntime::default())),
        temp.path().to_path_buf(),
        A2aCredentialStore::with_keyring(Arc::new(keyring)),
    );
    let status = server.status().await;

    assert!(!status.enabled);
    assert!(!status.running);
    assert!(!status.token_configured);
    assert!(
        status
            .error
            .as_deref()
            .is_some_and(|error| error.contains("A2A is disabled until it is reconfigured"))
    );
}

#[tokio::test]
async fn task_store_is_bounded_and_lists_newest_first() {
    let mut store = TaskStore::default();
    let inserted = (0..MAX_RETAINED_TASKS + 2)
        .map(test_task)
        .collect::<Vec<_>>();
    for task in inserted.iter().cloned() {
        store.insert(task).expect("insert terminal task");
    }
    let expected = inserted[2..].iter().rev().cloned().collect::<Vec<_>>();

    assert_eq!(store.list_newest_first(), expected);
}

#[tokio::test]
async fn task_store_eviction_preserves_oldest_working_task() {
    let mut store = TaskStore::default();
    let working = A2aTask {
        id: "working-task".to_string(),
        context_id: "working-context".to_string(),
        status: TaskStatus {
            state: TaskState::Working,
            timestamp: "working-timestamp".to_string(),
            message: None,
        },
        artifacts: Vec::new(),
    };
    store.insert(working.clone()).expect("insert working task");
    for index in 0..MAX_RETAINED_TASKS - 1 {
        store
            .insert(test_task(index))
            .expect("insert terminal task");
    }

    let evicted = store
        .insert(test_task(MAX_RETAINED_TASKS))
        .expect("insert overflow task");

    assert_eq!(evicted, Some("task-000".to_string()));
    assert_eq!(store.get(&working.id), Some(working));
    assert_eq!(store.list_oldest_first().len(), MAX_RETAINED_TASKS);
}

#[tokio::test]
async fn server_retention_never_evicts_a_running_agent_turn() {
    let (server, runtime) = server_with_runtime(TestServerOptions::default());
    let oldest = server
        .enqueue(
            "oldest work".to_string(),
            /*model*/ None,
            /*context_id*/ None,
        )
        .await
        .expect("oldest task");
    let survivor = server
        .enqueue(
            "surviving work".to_string(),
            /*model*/ None,
            /*context_id*/ None,
        )
        .await
        .expect("surviving task");
    {
        let mut tasks = server.tasks.write().await;
        for index in 0..MAX_RETAINED_TASKS - 2 {
            tasks
                .insert(test_task(index))
                .expect("insert terminal task");
        }
        assert_eq!(tasks.list_oldest_first().len(), MAX_RETAINED_TASKS);
    }

    let overflow = server
        .enqueue(
            "overflow work".to_string(),
            /*model*/ None,
            /*context_id*/ None,
        )
        .await
        .expect("enqueue overflow task");
    assert_eq!(runtime.interrupt_count(), 0);
    assert_eq!(server.task(&oldest.id).await.expect("oldest task"), oldest);
    assert_eq!(
        server.task(&survivor.id).await.expect("surviving task"),
        survivor
    );
    assert!(server.task("task-000").await.is_err());
    assert_eq!(
        server.task(&overflow.id).await.expect("overflow task"),
        overflow
    );
}

#[tokio::test]
async fn failed_publication_retains_capacity_until_the_agent_turn_stops() {
    let temp = tempfile::tempdir().expect("temporary app data");
    let blocked_data_dir = temp.path().join("blocked-app-data");
    std::fs::write(&blocked_data_dir, b"not a directory").expect("blocked data path");
    let (server, runtime) = server_with_runtime(TestServerOptions {
        persistence: Some(A2aPersistence::new(blocked_data_dir)),
        ..TestServerOptions::default()
    });
    runtime.fail_interrupts(1);
    let interrupt_blocker = runtime.block_interrupts().await;
    let enqueue_server = server.clone();
    let enqueue = tokio::spawn(async move {
        enqueue_server
            .enqueue(
                "unpublishable work".to_string(),
                /*model*/ None,
                /*context_id*/ None,
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(1), async {
        while runtime.interrupt_count() != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("publication cleanup interrupt");

    assert_eq!(server.execution_gate.available_permits(), 3);
    assert!(!enqueue.is_finished());
    drop(interrupt_blocker);
    let error = tokio::time::timeout(Duration::from_secs(2), enqueue)
        .await
        .expect("failed enqueue timeout")
        .expect("enqueue task")
        .expect_err("persistence failure");

    assert!(
        error
            .to_string()
            .contains("failed to persist A2A task state")
    );
    assert_eq!(runtime.interrupt_count(), 2);
    assert_eq!(server.execution_gate.available_permits(), 4);
    assert_eq!(server.tasks().await, Vec::new());
}

#[tokio::test]
async fn shutdown_rejects_inflight_publication_when_runtime_is_unavailable() {
    assert_shutdown_rejects_inflight_publication(|runtime| runtime.set_available(false)).await;
}

#[tokio::test]
async fn shutdown_rejects_inflight_publication_when_interrupt_keeps_failing() {
    assert_shutdown_rejects_inflight_publication(|runtime| runtime.fail_interrupts(100)).await;
}

#[tokio::test]
async fn shutdown_does_not_wait_forever_for_a_hung_turn_interrupt() {
    let (server, runtime) = server_with_runtime(TestServerOptions::default());
    let task = server
        .enqueue(
            "work with a hung interrupt".to_string(),
            /*model*/ None,
            /*context_id*/ None,
        )
        .await
        .expect("task");
    let interrupt_blocker = runtime.block_interrupts().await;
    let cancel_server = server.clone();
    let task_id = task.id.clone();
    let cancellation = tokio::spawn(async move { cancel_server.cancel(&task_id).await });
    tokio::time::timeout(Duration::from_secs(1), async {
        while runtime.interrupt_count() != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("hung interrupt request");

    let shutdown_server = server.clone();
    tokio::time::timeout(Duration::from_secs(1), shutdown_server.shutdown())
        .await
        .expect("bounded A2A shutdown");
    let cancellation_error = cancellation
        .await
        .expect("cancellation task")
        .expect_err("shutdown must stop waiting for cancellation");
    drop(interrupt_blocker);

    assert_eq!(
        cancellation_error.to_string(),
        "A2A server is shutting down"
    );
    assert_eq!(runtime.interrupt_count(), 1);
    assert!(server.executions.lock().await.is_empty());
}

async fn assert_shutdown_rejects_inflight_publication(
    configure_runtime: impl FnOnce(&FakeRuntime),
) {
    let (server, runtime) = server_with_runtime(TestServerOptions::default());
    let publication_blocker = server
        .task_mutation_gate
        .clone()
        .acquire_owned()
        .await
        .expect("task publication gate");
    let mut enqueue = Box::pin(server.enqueue(
        "work racing shutdown".to_string(),
        /*model*/ None,
        /*context_id*/ None,
    ));
    assert!(
        tokio::time::timeout(Duration::from_millis(25), enqueue.as_mut())
            .await
            .is_err()
    );
    assert!(!runtime.last_turn_id().is_empty());
    configure_runtime(&runtime);
    let shutdown_server = server.clone();
    let shutdown = tokio::spawn(async move { shutdown_server.shutdown().await });
    tokio::time::timeout(Duration::from_secs(1), async {
        while !server.terminated.load(Ordering::Acquire) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("shutdown start");
    assert!(!shutdown.is_finished());

    drop(publication_blocker);
    let (enqueue_result, shutdown_result) = tokio::join!(enqueue.as_mut(), shutdown);
    shutdown_result.expect("shutdown task");
    let error = enqueue_result.expect_err("shutdown must reject publication");

    assert_eq!(error.to_string(), "A2A server is shutting down");
    assert_eq!(runtime.interrupt_count(), 0);
    assert_eq!(server.execution_gate.available_permits(), 4);
    assert_eq!(server.tasks().await, Vec::new());
    assert!(server.executions.lock().await.is_empty());
}

#[tokio::test]
async fn invalid_runtime_turn_id_is_interrupted_before_capacity_release() {
    let (server, runtime) = server_with_runtime(TestServerOptions::default());
    runtime.set_next_turn_id("x".repeat(257));

    let error = server
        .enqueue(
            "work with an invalid upstream turn ID".to_string(),
            /*model*/ None,
            /*context_id*/ None,
        )
        .await
        .expect_err("invalid turn ID");

    assert!(error.to_string().contains("embedded agent turn ID exceeds"));
    assert_eq!(runtime.interrupt_count(), 1);
    assert_eq!(server.execution_gate.available_permits(), 4);
    assert_eq!(server.tasks().await, Vec::new());
}

#[tokio::test]
async fn cancellation_interrupts_the_corresponding_agent_turn() {
    let (server, runtime) = server_with_runtime(TestServerOptions::default());
    let task = server
        .enqueue(
            "do work".to_string(),
            /*model*/ None,
            /*context_id*/ None,
        )
        .await
        .expect("task");
    let canceled = server.cancel(&task.id).await.expect("cancel task");

    assert_eq!(server.task(&task.id).await.expect("stored task"), canceled);
    assert_eq!(canceled.status.state, TaskState::Canceled);
    assert_eq!(canceled.artifacts, Vec::new());
    assert!(runtime.requests().iter().any(|request| matches!(
        request,
        BridgeAgentRequest::TurnInterrupt(params)
            if params.thread_id == task.context_id && params.turn_id == runtime.last_turn_id()
    )));
}

#[tokio::test]
async fn failed_cancellation_keeps_the_task_and_execution_retryable() {
    let (server, runtime) = server_with_runtime(TestServerOptions::default());
    let task = server
        .enqueue(
            "do work".to_string(),
            /*model*/ None,
            /*context_id*/ None,
        )
        .await
        .expect("task");
    runtime.fail_interrupts(1);

    let error = server
        .cancel(&task.id)
        .await
        .expect_err("failed interrupt must fail cancellation");

    assert!(error.to_string().contains("simulated interrupt failure"));
    assert_eq!(
        server
            .task(&task.id)
            .await
            .expect("working task")
            .status
            .state,
        TaskState::Working
    );
    assert!(server.executions.lock().await.contains_key(&task.id));

    let canceled = server.cancel(&task.id).await.expect("retry cancellation");
    assert_eq!(canceled.status.state, TaskState::Canceled);
}

#[tokio::test]
async fn concurrent_cancellations_are_idempotent_and_interrupt_once() {
    let (server, runtime) = server_with_runtime(TestServerOptions::default());
    let task = server
        .enqueue(
            "do work".to_string(),
            /*model*/ None,
            /*context_id*/ None,
        )
        .await
        .expect("task");
    let interrupt_blocker = runtime.block_interrupts().await;
    let first_server = server.clone();
    let first_task_id = task.id.clone();
    let first = tokio::spawn(async move { first_server.cancel(&first_task_id).await });
    let second_server = server.clone();
    let second_task_id = task.id.clone();
    let second = tokio::spawn(async move { second_server.cancel(&second_task_id).await });

    tokio::time::timeout(Duration::from_secs(1), async {
        while runtime.interrupt_count() != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("first interrupt request");
    tokio::time::sleep(Duration::from_millis(25)).await;
    assert_eq!(runtime.interrupt_count(), 1);
    drop(interrupt_blocker);

    let first = first
        .await
        .expect("first cancellation task")
        .expect("cancel");
    let second = second
        .await
        .expect("second cancellation task")
        .expect("cancel");
    assert_eq!(first.status.state, TaskState::Canceled);
    assert_eq!(second.status.state, TaskState::Canceled);
    assert_eq!(runtime.interrupt_count(), 1);
}

#[tokio::test]
async fn completion_wins_a_race_with_cancellation_without_being_overwritten() {
    let (server, runtime) = server_with_runtime(TestServerOptions::default());
    let task = server
        .enqueue(
            "do work".to_string(),
            /*model*/ None,
            /*context_id*/ None,
        )
        .await
        .expect("task");
    let interrupt_blocker = runtime.block_interrupts().await;
    let cancel_server = server.clone();
    let task_id = task.id.clone();
    let cancellation = tokio::spawn(async move { cancel_server.cancel(&task_id).await });
    tokio::time::timeout(Duration::from_secs(1), async {
        while runtime.interrupt_count() != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("interrupt request");

    runtime.emit(turn_completed(
        &task.context_id,
        &runtime.last_turn_id(),
        TurnStatus::Completed,
        Some("completed before cancellation"),
    ));
    let completed = wait_for_terminal_task(&server, &task.id).await;
    drop(interrupt_blocker);
    let cancellation = cancellation
        .await
        .expect("cancellation task")
        .expect("idempotent cancellation");

    assert_eq!(cancellation, completed);
    assert_eq!(completed.status.state, TaskState::Completed);
    assert_eq!(
        completed.artifacts[0].parts[0].text,
        "completed before cancellation"
    );
}

#[tokio::test]
async fn event_lag_retains_capacity_until_the_agent_turn_is_interrupted() {
    let (server, runtime) = server_with_runtime(TestServerOptions::default());
    let task = server
        .enqueue(
            "do work".to_string(),
            /*model*/ None,
            /*context_id*/ None,
        )
        .await
        .expect("task");
    runtime.fail_interrupts(100);

    runtime.emit(BridgeAgentEvent::Lagged { skipped: 1 });
    tokio::time::sleep(Duration::from_millis(30)).await;

    assert_eq!(
        server
            .task(&task.id)
            .await
            .expect("working task")
            .status
            .state,
        TaskState::Working
    );
    assert_eq!(server.execution_gate.available_permits(), 3);

    runtime.fail_interrupts(0);
    let failed = wait_for_terminal_task(&server, &task.id).await;
    assert_eq!(failed.status.state, TaskState::Failed);
    tokio::time::timeout(Duration::from_secs(1), async {
        while server.execution_gate.available_permits() != 4 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("execution capacity release");
    assert!(runtime.requests().iter().any(|request| matches!(
        request,
        BridgeAgentRequest::TurnInterrupt(params)
            if params.thread_id == task.context_id && params.turn_id == runtime.last_turn_id()
    )));
}

#[tokio::test]
async fn lag_after_a_skipped_completion_resynchronizes_and_releases_capacity() {
    let (server, runtime) = server_with_runtime(TestServerOptions::default());
    let task = server
        .enqueue(
            "do work".to_string(),
            /*model*/ None,
            /*context_id*/ None,
        )
        .await
        .expect("task");
    runtime.set_turn_status(TurnStatus::Completed);

    runtime.emit(BridgeAgentEvent::Lagged { skipped: 1 });

    let failed = wait_for_terminal_task(&server, &task.id).await;
    assert_eq!(failed.status.state, TaskState::Failed);
    wait_for_execution_capacity(&server).await;
    assert!(runtime.requests().iter().any(|request| matches!(
        request,
        BridgeAgentRequest::ThreadRead(params)
            if params.thread_id == task.context_id && params.include_turns
    )));
}

#[tokio::test]
async fn lag_racing_runtime_disconnect_fails_instead_of_retrying_forever() {
    let (server, runtime) = server_with_runtime(TestServerOptions::default());
    let task = server
        .enqueue(
            "do work".to_string(),
            /*model*/ None,
            /*context_id*/ None,
        )
        .await
        .expect("task");
    runtime.set_available(false);

    runtime.emit(BridgeAgentEvent::Lagged { skipped: 1 });

    assert_eq!(
        wait_for_terminal_task(&server, &task.id).await.status.state,
        TaskState::Failed
    );
    wait_for_execution_capacity(&server).await;
}

#[tokio::test]
async fn runtime_disconnect_releases_capacity_and_allows_recovery() {
    let (server, runtime) = server_with_runtime(TestServerOptions::default());
    let failed_task = server
        .enqueue(
            "first".to_string(),
            /*model*/ None,
            /*context_id*/ None,
        )
        .await
        .expect("first task");

    runtime.emit(BridgeAgentEvent::Disconnected {
        message: "proxy connection lost".to_string(),
    });
    let failed = wait_for_terminal_task(&server, &failed_task.id).await;
    assert_eq!(failed.status.state, TaskState::Failed);
    tokio::time::timeout(Duration::from_secs(1), async {
        while server.execution_gate.available_permits() != 4 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("capacity released after disconnect");

    let recovered_task = server
        .enqueue(
            "second".to_string(),
            /*model*/ None,
            /*context_id*/ None,
        )
        .await
        .expect("recovered task");
    runtime.emit(turn_completed(
        &recovered_task.context_id,
        &runtime.last_turn_id(),
        TurnStatus::Completed,
        Some("recovered"),
    ));

    assert_eq!(
        wait_for_terminal_task(&server, &recovered_task.id)
            .await
            .status
            .state,
        TaskState::Completed
    );
}

#[tokio::test]
async fn authenticated_http_cancellation_interrupts_the_corresponding_agent_turn() {
    let (server, runtime) = server_with_runtime(TestServerOptions {
        settings: A2aServerSettings {
            enabled: true,
            port: 8120,
        },
        token: Some(TEST_TOKEN.to_string()),
        ..TestServerOptions::default()
    });
    let task = server
        .enqueue(
            "do work".to_string(),
            /*model*/ None,
            /*context_id*/ None,
        )
        .await
        .expect("task");
    let router = server.router();
    let unauthorized = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/a2a/tasks/{}:cancel", task.id))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("unauthorized response");
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/a2a/tasks/{}:cancel", task.id))
                .header(AUTHORIZATION, format!("Bearer {TEST_TOKEN}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("cancellation response");
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("cancellation body");
    let canceled: A2aTask = serde_json::from_slice(&body).expect("canceled task");

    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(canceled.status.state, TaskState::Canceled);
    assert!(runtime.requests().iter().any(|request| matches!(
        request,
        BridgeAgentRequest::TurnInterrupt(params)
            if params.thread_id == task.context_id && params.turn_id == runtime.last_turn_id()
    )));
}

async fn authorized_agent_card(router: &axum::Router, token: &str) -> StatusCode {
    router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/.well-known/agent-card.json")
                .header(AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("agent card response")
        .status()
}

async fn wait_for_terminal_task(server: &A2aServer, task_id: &str) -> A2aTask {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let task = server.task(task_id).await.expect("stored task");
            if task.status.state != TaskState::Working {
                return task;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("task completion")
}

async fn wait_for_execution_capacity(server: &A2aServer) {
    tokio::time::timeout(Duration::from_secs(1), async {
        while server.execution_gate.available_permits() != 4 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("execution capacity release");
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

fn enabled_server() -> A2aServer {
    test_server(TestServerOptions {
        settings: A2aServerSettings {
            enabled: true,
            port: 8120,
        },
        token: Some(TEST_TOKEN.to_string()),
        ..TestServerOptions::default()
    })
}

struct TestServerOptions {
    settings: A2aServerSettings,
    token: Option<String>,
    persistence: Option<A2aPersistence>,
    tasks: TaskStore,
    http_config: A2aHttpConfig,
}

impl Default for TestServerOptions {
    fn default() -> Self {
        Self {
            settings: A2aServerSettings::default(),
            token: None,
            persistence: None,
            tasks: TaskStore::default(),
            http_config: A2aHttpConfig::production(),
        }
    }
}

fn test_server(options: TestServerOptions) -> A2aServer {
    server_with_runtime(options).0
}

fn server_with_runtime(options: TestServerOptions) -> (A2aServer, FakeRuntime) {
    let credentials = A2aCredentialStore::with_keyring(Arc::new(MockKeyringStore::default()));
    if let Some(token) = &options.token {
        credentials.save(token).expect("seed A2A token");
    }
    server_with_credentials(options, credentials)
}

fn server_with_credentials(
    options: TestServerOptions,
    credentials: A2aCredentialStore,
) -> (A2aServer, FakeRuntime) {
    let runtime = FakeRuntime::default();
    let server = A2aServer::new_with_components(A2aServerComponents {
        runtime: A2aRuntimeClient::with_backend(Arc::new(runtime.clone())),
        settings: options.settings,
        token: options.token,
        configuration_error: None,
        credentials,
        persistence: options.persistence,
        tasks: options.tasks,
        http_config: options.http_config,
    });
    (server, runtime)
}

#[derive(Clone)]
struct FakeRuntime {
    requests: Arc<StdMutex<Vec<BridgeAgentRequest>>>,
    events: broadcast::Sender<BridgeAgentEvent>,
    next_id: Arc<AtomicUsize>,
    next_turn_id: Arc<StdMutex<Option<String>>>,
    last_turn_id: Arc<StdMutex<Option<String>>>,
    interrupt_failures: Arc<AtomicUsize>,
    interrupt_gate: Arc<Semaphore>,
    turn_status: Arc<StdMutex<Option<TurnStatus>>>,
    available: Arc<AtomicBool>,
}

impl Default for FakeRuntime {
    fn default() -> Self {
        let (events, _) = broadcast::channel(32);
        Self {
            requests: Arc::new(StdMutex::new(Vec::new())),
            events,
            next_id: Arc::new(AtomicUsize::new(1)),
            next_turn_id: Arc::new(StdMutex::new(None)),
            last_turn_id: Arc::new(StdMutex::new(None)),
            interrupt_failures: Arc::new(AtomicUsize::new(0)),
            interrupt_gate: Arc::new(Semaphore::new(1)),
            turn_status: Arc::new(StdMutex::new(None)),
            available: Arc::new(AtomicBool::new(true)),
        }
    }
}

impl FakeRuntime {
    fn requests(&self) -> Vec<BridgeAgentRequest> {
        self.requests.lock().expect("request lock").clone()
    }

    fn last_turn_id(&self) -> String {
        self.last_turn_id
            .lock()
            .expect("turn ID lock")
            .clone()
            .expect("turn was started")
    }

    fn set_next_turn_id(&self, turn_id: String) {
        *self.next_turn_id.lock().expect("next turn ID lock") = Some(turn_id);
    }

    fn emit(&self, event: BridgeAgentEvent) {
        self.events.send(event).expect("A2A event subscriber");
    }

    fn fail_interrupts(&self, count: usize) {
        self.interrupt_failures.store(count, Ordering::Release);
    }

    async fn block_interrupts(&self) -> OwnedSemaphorePermit {
        self.interrupt_gate
            .clone()
            .acquire_owned()
            .await
            .expect("interrupt gate")
    }

    fn interrupt_count(&self) -> usize {
        self.requests()
            .iter()
            .filter(|request| matches!(request, BridgeAgentRequest::TurnInterrupt(_)))
            .count()
    }

    fn set_turn_status(&self, status: TurnStatus) {
        *self.turn_status.lock().expect("turn status lock") = Some(status);
    }

    fn set_available(&self, available: bool) {
        self.available.store(available, Ordering::Release);
    }
}

impl A2aRuntimeBackend for FakeRuntime {
    fn request(
        &self,
        request: BridgeAgentRequest,
    ) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + '_>> {
        Box::pin(async move {
            self.requests
                .lock()
                .expect("request lock")
                .push(request.clone());
            if !self.available.load(Ordering::Acquire) {
                bail!("simulated runtime disconnect");
            }
            match request {
                BridgeAgentRequest::ThreadStart(_) => {
                    let id = self.next_id.fetch_add(1, Ordering::Relaxed);
                    Ok(json!({"thread": {"id": format!("thread-{id}")}}))
                }
                BridgeAgentRequest::ThreadResume(params) => {
                    Ok(json!({"thread": {"id": params.thread_id}}))
                }
                BridgeAgentRequest::TurnStart(_) => {
                    let id = self.next_id.fetch_add(1, Ordering::Relaxed);
                    let turn_id = self
                        .next_turn_id
                        .lock()
                        .expect("next turn ID lock")
                        .take()
                        .unwrap_or_else(|| format!("turn-{id}"));
                    *self.last_turn_id.lock().expect("turn ID lock") = Some(turn_id.clone());
                    *self.turn_status.lock().expect("turn status lock") =
                        Some(TurnStatus::InProgress);
                    Ok(json!({"turn": {"id": turn_id}}))
                }
                BridgeAgentRequest::TurnInterrupt(_) => {
                    let _interrupt_permit = self
                        .interrupt_gate
                        .clone()
                        .acquire_owned()
                        .await
                        .expect("interrupt gate");
                    if self
                        .interrupt_failures
                        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |remaining| {
                            remaining.checked_sub(1)
                        })
                        .is_ok()
                    {
                        bail!("simulated interrupt failure");
                    }
                    let mut status = self.turn_status.lock().expect("turn status lock");
                    if !matches!(status.as_ref(), Some(TurnStatus::InProgress)) {
                        bail!("simulated missing terminal turn");
                    }
                    *status = Some(TurnStatus::Interrupted);
                    Ok(Value::Null)
                }
                BridgeAgentRequest::ThreadRead(_) => {
                    let turn_id = self.last_turn_id.lock().expect("turn ID lock").clone();
                    let status = self.turn_status.lock().expect("turn status lock").clone();
                    let turns = turn_id
                        .zip(status)
                        .map(|(id, status)| {
                            vec![json!({
                                "id": id,
                                "status": match status {
                                    TurnStatus::Completed => "completed",
                                    TurnStatus::Interrupted => "interrupted",
                                    TurnStatus::Failed => "failed",
                                    TurnStatus::InProgress => "inProgress",
                                }
                            })]
                        })
                        .unwrap_or_default();
                    Ok(json!({"thread": {"turns": turns}}))
                }
                request => bail!("unexpected A2A runtime request: {request:?}"),
            }
        })
    }

    fn subscribe_events(&self) -> broadcast::Receiver<BridgeAgentEvent> {
        self.events.subscribe()
    }
}

fn turn_completed(
    thread_id: &str,
    turn_id: &str,
    status: TurnStatus,
    content: Option<&str>,
) -> BridgeAgentEvent {
    let items = content
        .map(|text| {
            vec![ThreadItem::AgentMessage {
                id: "message-1".to_string(),
                text: text.to_string(),
                phase: None,
                memory_citation: None,
            }]
        })
        .unwrap_or_default();
    BridgeAgentEvent::ServerNotification {
        payload: serde_json::to_value(ServerNotification::TurnCompleted(
            TurnCompletedNotification {
                thread_id: thread_id.to_string(),
                turn: Turn {
                    id: turn_id.to_string(),
                    items,
                    items_view: TurnItemsView::Full,
                    status,
                    error: None,
                    started_at: Some(1),
                    completed_at: Some(2),
                    duration_ms: Some(1_000),
                },
            },
        ))
        .expect("turn notification JSON"),
    }
}
