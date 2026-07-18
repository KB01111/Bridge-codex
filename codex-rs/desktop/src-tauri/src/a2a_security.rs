use std::time::Duration;
use std::time::Instant;

use axum::extract::Request;
use axum::extract::State;
use axum::http::HeaderValue;
use axum::http::StatusCode;
use axum::http::header::AUTHORIZATION;
use axum::http::header::RETRY_AFTER;
use axum::http::header::WWW_AUTHENTICATE;
use axum::middleware::Next;
use axum::response::IntoResponse;
use axum::response::Response;

use super::A2aServer;

const A2A_RATE_LIMIT_WINDOW: Duration = Duration::from_secs(60);
pub(super) const MAX_A2A_BODY_BYTES: usize = 1024 * 1024;
const MAX_A2A_CONCURRENT_REQUESTS: usize = 8;
const MAX_A2A_REQUESTS_PER_WINDOW: usize = 60;
const MIN_A2A_TOKEN_BYTES: usize = 32;
const MAX_A2A_TOKEN_BYTES: usize = 512;

#[derive(Clone)]
pub(super) struct A2aHttpConfig {
    pub(super) max_requests_per_window: usize,
    pub(super) rate_limit_window: Duration,
    pub(super) max_concurrent_requests: usize,
}

impl A2aHttpConfig {
    pub(super) fn production() -> Self {
        Self {
            max_requests_per_window: MAX_A2A_REQUESTS_PER_WINDOW,
            rate_limit_window: A2A_RATE_LIMIT_WINDOW,
            max_concurrent_requests: MAX_A2A_CONCURRENT_REQUESTS,
        }
    }
}

pub(super) struct A2aRateLimitState {
    pub(super) window_started: Instant,
    pub(super) admitted_requests: usize,
}

impl Default for A2aRateLimitState {
    fn default() -> Self {
        Self {
            window_started: Instant::now(),
            admitted_requests: 0,
        }
    }
}

pub(super) async fn authorize_and_limit_request(
    State(server): State<A2aServer>,
    request: Request,
    next: Next,
) -> Response {
    let supplied_token = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    let token = {
        let token = server.token.read().await;
        token.clone()
    };
    let authorized = supplied_token
        .zip(token.as_deref())
        .is_some_and(|(supplied, expected)| constant_time_eq(supplied, expected));
    if !authorized {
        let mut response = (StatusCode::UNAUTHORIZED, "A2A bearer token required").into_response();
        response
            .headers_mut()
            .insert(WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        return response;
    }

    if !server.admit_rate_limited_request().await {
        let mut response =
            (StatusCode::TOO_MANY_REQUESTS, "A2A request rate exceeded").into_response();
        response
            .headers_mut()
            .insert(RETRY_AFTER, HeaderValue::from_static("60"));
        return response;
    }

    let Ok(_request_permit) = server.request_gate.clone().try_acquire_owned() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "A2A request concurrency exceeded",
        )
            .into_response();
    };
    next.run(request).await
}

impl A2aServer {
    async fn admit_rate_limited_request(&self) -> bool {
        let mut state = self.rate_limit.lock().await;
        if state.window_started.elapsed() >= self.http_config.rate_limit_window {
            state.window_started = Instant::now();
            state.admitted_requests = 0;
        }
        if state.admitted_requests >= self.http_config.max_requests_per_window {
            return false;
        }
        state.admitted_requests += 1;
        true
    }
}

pub(super) fn validate_a2a_token(token: Option<&str>) -> Result<(), String> {
    let Some(token) = token else {
        return Err("an A2A bearer token is required before enabling the server".to_string());
    };
    if !(MIN_A2A_TOKEN_BYTES..=MAX_A2A_TOKEN_BYTES).contains(&token.len()) {
        return Err(format!(
            "the A2A bearer token must contain between {MIN_A2A_TOKEN_BYTES} and {MAX_A2A_TOKEN_BYTES} bytes"
        ));
    }
    if !token.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err(
            "the A2A bearer token must contain printable ASCII without whitespace".to_string(),
        );
    }
    Ok(())
}

pub(super) fn constant_time_eq(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    let mut difference = left.len() ^ right.len();
    for index in 0..left.len().max(right.len()) {
        difference |= usize::from(
            left.get(index).copied().unwrap_or_default()
                ^ right.get(index).copied().unwrap_or_default(),
        );
    }
    difference == 0
}
