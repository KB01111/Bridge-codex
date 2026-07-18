use pretty_assertions::assert_eq;
use serde_json::json;
use url::Url;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::body_json;
use wiremock::matchers::method;
use wiremock::matchers::path;

use super::PROBE_CONTINUATION_INSTRUCTION;
use super::PROBE_TOOL_NAME;
use super::ProbeFunctionCall;
use super::continuation_payload;
use super::forced_call_payload;
use super::probe_responses_conformance;
use super::probe_responses_conformance_with_marker;
use crate::cli_proxy::CliProxyClient;

fn client(server: &MockServer) -> CliProxyClient {
    CliProxyClient::new(
        Url::parse(&format!("{}/", server.uri())).expect("server URL"),
        Some("secret".to_string()),
    )
    .expect("proxy client")
}

fn forced_call_stream() -> String {
    [
        format!(
            "data: {}\n\n",
            json!({
                "type": "response.output_item.done",
                "item": {
                    "type": "function_call",
                    "call_id": "call-1",
                    "name": PROBE_TOOL_NAME,
                    "arguments": "{\"value\":\"ping\"}"
                }
            })
        ),
        format!(
            "data: {}\n\n",
            json!({
                "type": "response.completed",
                "response": {"id": "resp-1", "status": "completed", "output": []}
            })
        ),
    ]
    .concat()
}

fn completed_stream(response_id: &str, output_text: Option<&str>) -> String {
    let output = output_text.map_or_else(Vec::new, |text| {
        vec![json!({
            "type": "message",
            "role": "assistant",
            "content": [{"type": "output_text", "text": text}]
        })]
    });
    format!(
        "data: {}\n\n",
        json!({
            "type": "response.completed",
            "response": {"id": response_id, "status": "completed", "output": output}
        })
    )
}

#[test]
fn continuation_requests_an_exact_echo_without_repeating_the_marker() {
    let marker = "bridge-probe-test-marker";
    let payload = continuation_payload(
        "model-a",
        &ProbeFunctionCall {
            call_id: "call-1".to_string(),
            name: PROBE_TOOL_NAME.to_string(),
            arguments: "{\"value\":\"ping\"}".to_string(),
        },
        marker,
    );

    assert_eq!(payload["input"][1]["output"], marker);
    assert_eq!(
        payload["input"][2]["content"][0]["text"],
        PROBE_CONTINUATION_INSTRUCTION
    );
    assert_eq!(
        serde_json::to_string(&payload)
            .expect("payload JSON")
            .matches(marker)
            .count(),
        1
    );
}

#[tokio::test]
async fn conformance_probe_rejects_malformed_sse() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_json(forced_call_payload("model-a")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw("data: this-is-not-json\n\n", "text/event-stream"),
        )
        .expect(1)
        .mount(&server)
        .await;

    let error = probe_responses_conformance(&client(&server), "model-a")
        .await
        .expect_err("malformed SSE must fail");

    assert!(
        error
            .to_string()
            .contains("invalid CLIProxyAPI forced function call SSE stream")
    );
}

#[tokio::test]
async fn conformance_probe_rejects_a_forced_call_failure() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_json(forced_call_payload("model-a")))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            completed_stream("resp-1", /*output_text*/ None),
            "text/event-stream",
        ))
        .expect(1)
        .mount(&server)
        .await;

    let error = probe_responses_conformance(&client(&server), "model-a")
        .await
        .expect_err("missing forced call must fail");

    assert_eq!(
        error.to_string(),
        "Responses probe completed without the required function call"
    );
}

#[tokio::test]
async fn conformance_probe_continues_with_function_call_output() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_json(forced_call_payload("model-a")))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(forced_call_stream(), "text/event-stream"),
        )
        .expect(1)
        .mount(&server)
        .await;
    let call = ProbeFunctionCall {
        call_id: "call-1".to_string(),
        name: PROBE_TOOL_NAME.to_string(),
        arguments: "{\"value\":\"ping\"}".to_string(),
    };
    let marker = "bridge-probe-test-marker";
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_json(continuation_payload("model-a", &call, marker)))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            completed_stream("resp-2", Some(marker)),
            "text/event-stream",
        ))
        .expect(1)
        .mount(&server)
        .await;

    probe_responses_conformance_with_marker(&client(&server), "model-a", marker)
        .await
        .expect("conformant proxy");
}

#[tokio::test]
async fn conformance_probe_rejects_a_continuation_that_ignores_tool_output() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_json(forced_call_payload("model-a")))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(forced_call_stream(), "text/event-stream"),
        )
        .expect(1)
        .mount(&server)
        .await;
    let call = ProbeFunctionCall {
        call_id: "call-1".to_string(),
        name: PROBE_TOOL_NAME.to_string(),
        arguments: "{\"value\":\"ping\"}".to_string(),
    };
    let marker = "bridge-probe-test-marker";
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_json(continuation_payload("model-a", &call, marker)))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            completed_stream("resp-2", Some("ignored")),
            "text/event-stream",
        ))
        .expect(1)
        .mount(&server)
        .await;

    let error = probe_responses_conformance_with_marker(&client(&server), "model-a", marker)
        .await
        .expect_err("an ignored tool result must fail conformance");

    assert_eq!(
        error.to_string(),
        "Responses probe did not return the exact function-call output marker"
    );
}
