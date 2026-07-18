use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::DynamicToolNamespaceTool;
use codex_app_server_protocol::DynamicToolSpec;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::SandboxMode;
use codex_app_server_protocol::SandboxPolicy;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnStartParams;
use codex_config::types::OtelExporterKind;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::ConfigBuilder;
use super::MAX_PENDING_SERVER_REQUEST_BYTES;
use super::MAX_PENDING_SERVER_REQUESTS;
use super::PendingServerRequests;
use crate::agent_runtime_config::bridge_codex_home;
use crate::agent_runtime_config::provider_cli_overrides;
use crate::agent_runtime_protocol::BridgeAgentRequest;
use crate::agent_runtime_protocol::BridgePendingServerRequest;
use crate::agent_runtime_protocol::PROVIDER_ID;

const TEST_MODEL: &str = "verified-gateway-model";

#[test]
fn thread_start_is_pinned_to_the_bridge_provider() {
    let request = BridgeAgentRequest::ThreadStart(ThreadStartParams::default())
        .into_client_request(RequestId::Integer(7), TEST_MODEL)
        .expect("request");

    let ClientRequest::ThreadStart { request_id, params } = request else {
        panic!("expected thread/start request");
    };
    assert_eq!(request_id, RequestId::Integer(7));
    assert_eq!(params.model_provider, Some(PROVIDER_ID.to_string()));
    assert_eq!(params.model, Some(TEST_MODEL.to_string()));
    assert_eq!(
        params.approval_policy,
        Some(codex_app_server_protocol::AskForApproval::OnRequest)
    );
    assert_eq!(
        params.approvals_reviewer,
        Some(codex_app_server_protocol::ApprovalsReviewer::User)
    );
    assert_eq!(params.sandbox, Some(SandboxMode::WorkspaceWrite));
}

#[test]
fn thread_start_rejects_an_alternate_provider() {
    let params = ThreadStartParams {
        model_provider: Some("openai".to_string()),
        ..Default::default()
    };

    let error = BridgeAgentRequest::ThreadStart(params)
        .into_client_request(RequestId::Integer(1), TEST_MODEL)
        .expect_err("alternate provider must fail");

    assert!(error.to_string().contains("restricted"));
}

#[test]
fn thread_start_rejects_an_unverified_model() {
    let params = ThreadStartParams {
        model: Some("unverified-model".to_string()),
        ..Default::default()
    };

    let error = BridgeAgentRequest::ThreadStart(params)
        .into_client_request(RequestId::Integer(1), TEST_MODEL)
        .expect_err("unverified model must fail");

    assert!(
        error
            .to_string()
            .contains("conformance-tested gateway model")
    );
}

#[test]
fn facade_deserializes_only_explicitly_supported_methods() {
    let supported = serde_json::from_value::<BridgeAgentRequest>(json!({
        "method": "model/list",
        "params": {}
    }))
    .expect("supported request");
    let unsupported = serde_json::from_value::<BridgeAgentRequest>(json!({
        "method": "config/read",
        "params": {}
    }));

    assert!(matches!(supported, BridgeAgentRequest::ModelList(_)));
    assert!(unsupported.is_err());
}

#[test]
fn facade_supports_the_complete_thread_lifecycle() {
    for method in ["thread/name/set", "thread/unarchive", "thread/delete"] {
        let params = match method {
            "thread/name/set" => json!({"threadId": "thread-1", "name": "renamed"}),
            _ => json!({"threadId": "thread-1"}),
        };
        serde_json::from_value::<BridgeAgentRequest>(json!({
            "method": method,
            "params": params,
        }))
        .unwrap_or_else(|error| panic!("{method} should be supported: {error}"));
    }
}

#[test]
fn thread_lifecycle_rejects_weakened_security_and_config_overrides() {
    for method in ["thread/start", "thread/resume", "thread/fork"] {
        for weakened in [
            json!({"approvalPolicy": "never"}),
            json!({"approvalsReviewer": "auto_review"}),
            json!({"sandbox": "danger-full-access"}),
            json!({"permissions": "danger-full-access"}),
            json!({"config": {"sandbox_mode": "danger-full-access"}}),
        ] {
            let mut params = weakened;
            if method != "thread/start" {
                params["threadId"] = json!("thread-1");
            }
            let request = serde_json::from_value::<BridgeAgentRequest>(json!({
                "method": method,
                "params": params,
            }))
            .expect("thread request");

            assert!(
                request
                    .into_client_request(RequestId::Integer(1), TEST_MODEL)
                    .is_err()
            );
        }
    }
}

#[test]
fn turn_start_applies_safe_defaults_and_rejects_weakened_security() {
    let request = BridgeAgentRequest::TurnStart(TurnStartParams {
        thread_id: "thread-1".to_string(),
        ..Default::default()
    })
    .into_client_request(RequestId::Integer(9), TEST_MODEL)
    .expect("safe turn request");
    let ClientRequest::TurnStart { params, .. } = request else {
        panic!("expected turn/start request");
    };
    assert_eq!(
        params.approval_policy,
        Some(codex_app_server_protocol::AskForApproval::OnRequest)
    );
    assert_eq!(params.model, Some(TEST_MODEL.to_string()));
    assert_eq!(
        params.sandbox_policy,
        Some(SandboxPolicy::WorkspaceWrite {
            writable_roots: Vec::new(),
            network_access: false,
            exclude_tmpdir_env_var: false,
            exclude_slash_tmp: false,
        })
    );

    for params in [
        json!({"threadId": "thread-1", "input": [], "approvalPolicy": "never"}),
        json!({
            "threadId": "thread-1",
            "input": [],
            "sandboxPolicy": {"type": "dangerFullAccess"}
        }),
        json!({
            "threadId": "thread-1",
            "input": [],
            "sandboxPolicy": {"type": "workspaceWrite", "networkAccess": true}
        }),
        json!({
            "threadId": "thread-1",
            "input": [],
            "sandboxPolicy": {"type": "externalSandbox", "networkAccess": "restricted"}
        }),
    ] {
        let params = serde_json::from_value::<TurnStartParams>(params).expect("turn params");
        let result = BridgeAgentRequest::TurnStart(params)
            .into_client_request(RequestId::Integer(1), TEST_MODEL);

        assert!(result.is_err());
    }
}

#[test]
fn thread_start_injects_only_bounded_memory_and_browser_tools() {
    let request = BridgeAgentRequest::ThreadStart(ThreadStartParams::default())
        .into_client_request(RequestId::Integer(5), TEST_MODEL)
        .expect("request");
    let ClientRequest::ThreadStart { params, .. } = request else {
        panic!("expected thread/start request");
    };
    let [DynamicToolSpec::Namespace(namespace)] = params
        .dynamic_tools
        .as_deref()
        .expect("Bridge tools should be injected")
    else {
        panic!("expected one Bridge namespace");
    };
    assert_eq!(namespace.name, "bridge");
    let names = namespace
        .tools
        .iter()
        .map(|tool| match tool {
            DynamicToolNamespaceTool::Function(function) => function.name.as_str(),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec![
            "memory_search",
            "browser_observe",
            "browser_navigate",
            "browser_click",
            "browser_type",
            "browser_key",
        ]
    );
    assert!(names.iter().all(|name| {
        name.chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
    }));
    let schema_for = |name: &str| {
        namespace
            .tools
            .iter()
            .map(|tool| match tool {
                DynamicToolNamespaceTool::Function(function) => function,
            })
            .find(|function| function.name == name)
            .unwrap_or_else(|| panic!("missing {name} tool"))
            .input_schema
            .clone()
    };
    assert_eq!(
        schema_for("memory_search"),
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "minLength": 1, "maxLength": 2048},
                "maxResults": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 8,
                    "default": 8
                }
            },
            "required": ["query"],
            "additionalProperties": false
        })
    );
    assert_eq!(
        schema_for("browser_click"),
        json!({
            "type": "object",
            "properties": {
                "x": {"type": "integer", "minimum": 0, "maximum": 16384},
                "y": {"type": "integer", "minimum": 0, "maximum": 16384}
            },
            "required": ["x", "y"],
            "additionalProperties": false
        })
    );
    assert_eq!(
        schema_for("browser_type"),
        json!({
            "type": "object",
            "properties": {
                "text": {"type": "string", "maxLength": 16384}
            },
            "required": ["text"],
            "additionalProperties": false
        })
    );
    assert!(namespace.tools.iter().all(|tool| {
        !serde_json::to_string(tool)
            .expect("tool schema")
            .contains("desktop")
    }));
}

#[tokio::test]
async fn provider_override_is_responses_only_and_keeps_auth_in_memory() {
    let codex_home = tempfile::tempdir().expect("temporary Codex home");
    let config = ConfigBuilder::default()
        .codex_home(codex_home.path().to_path_buf())
        .cli_overrides(provider_cli_overrides(
            "http://127.0.0.1:8317/v1",
            "secret-value",
            TEST_MODEL,
        ))
        .build()
        .await
        .expect("config");

    assert_eq!(config.model_provider_id, PROVIDER_ID);
    assert_eq!(config.model, Some(TEST_MODEL.to_string()));
    assert_eq!(
        config.model_provider.base_url,
        Some("http://127.0.0.1:8317/v1".to_string())
    );
    assert_eq!(config.model_provider.wire_api.to_string(), "responses");
    assert_eq!(
        config.model_provider.experimental_bearer_token,
        Some("secret-value".to_string())
    );
    assert_eq!(config.model_provider.env_key, None);
    assert!(!config.model_provider.requires_openai_auth);
    assert!(!config.model_provider.supports_websockets);
    assert_eq!(config.analytics_enabled, Some(false));
    assert!(!config.feedback_enabled);
    assert!(!config.check_for_update_on_startup);
    assert!(!config.otel.log_user_prompt);
    assert_eq!(config.otel.exporter, OtelExporterKind::None);
    assert_eq!(config.otel.trace_exporter, OtelExporterKind::None);
    assert_eq!(config.otel.metrics_exporter, OtelExporterKind::None);
}

#[test]
fn bridge_codex_home_is_isolated_under_application_data() {
    let app_data_dir = std::path::Path::new("C:/Users/example/AppData/Roaming/Bridge Codex");

    assert_eq!(
        bridge_codex_home(app_data_dir),
        app_data_dir.join("codex-home")
    );
}

#[test]
fn pending_server_requests_are_hydratable_until_resolution_succeeds() {
    let request_id = RequestId::String("approval-1".to_string());
    let request = BridgePendingServerRequest {
        request_id: request_id.clone(),
        payload: json!({
            "id": "approval-1",
            "method": "item/commandExecution/requestApproval"
        }),
    };
    let mut pending = PendingServerRequests::default();
    pending.retain(request.clone()).expect("retain request");

    pending.finish_resolution(&request_id, &Err("transport closed".to_string()));
    assert_eq!(pending.list(), vec![request]);

    pending.finish_resolution(&request_id, &Ok(()));
    assert_eq!(pending.list(), Vec::new());
}

#[test]
fn pending_server_requests_reject_count_and_payload_overflow() {
    let mut pending = PendingServerRequests::default();
    for index in 0..MAX_PENDING_SERVER_REQUESTS {
        pending
            .retain(BridgePendingServerRequest {
                request_id: RequestId::Integer(index as i64),
                payload: json!({"id": index, "method": "item/tool/requestUserInput"}),
            })
            .expect("bounded request");
    }
    let count_error = pending
        .retain(BridgePendingServerRequest {
            request_id: RequestId::String("overflow".to_string()),
            payload: json!({"id": "overflow"}),
        })
        .expect_err("count overflow must fail closed");
    let mut oversized = PendingServerRequests::default();
    let payload_error = oversized
        .retain(BridgePendingServerRequest {
            request_id: RequestId::String("oversized".to_string()),
            payload: json!({"value": "x".repeat(MAX_PENDING_SERVER_REQUEST_BYTES)}),
        })
        .expect_err("payload overflow must fail closed");

    assert!(count_error.contains("128-request pending limit"));
    assert!(payload_error.contains("262144-byte pending-request limit"));
    assert_eq!(pending.list().len(), MAX_PENDING_SERVER_REQUESTS);
    assert_eq!(oversized.list(), Vec::new());
}

#[test]
fn runtime_disconnect_clears_stale_pending_server_requests() {
    let mut pending = PendingServerRequests::default();
    pending
        .retain(BridgePendingServerRequest {
            request_id: RequestId::String("approval-1".to_string()),
            payload: json!({"id": "approval-1"}),
        })
        .expect("retain request");

    pending.clear();

    assert_eq!(pending.list(), Vec::new());
}
