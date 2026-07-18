use codex_app_server_client::legacy_core::config::Config;
use codex_app_server_protocol::ConfigWarningNotification;
use toml::Value as TomlValue;

use crate::agent_runtime_protocol::PROVIDER_ID;

pub(crate) fn bridge_codex_home(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("codex-home")
}

pub(crate) fn provider_cli_overrides(
    provider_base_url: &str,
    api_key: &str,
    model: &str,
) -> Vec<(String, TomlValue)> {
    let provider = toml::Table::from_iter([
        (
            "name".to_string(),
            TomlValue::String("CLIProxyAPI".to_string()),
        ),
        (
            "base_url".to_string(),
            TomlValue::String(provider_base_url.to_string()),
        ),
        (
            "wire_api".to_string(),
            TomlValue::String("responses".to_string()),
        ),
        (
            "experimental_bearer_token".to_string(),
            TomlValue::String(api_key.to_string()),
        ),
        (
            "requires_openai_auth".to_string(),
            TomlValue::Boolean(false),
        ),
        ("supports_websockets".to_string(), TomlValue::Boolean(false)),
    ]);
    vec![
        (
            "model_provider".to_string(),
            TomlValue::String(PROVIDER_ID.to_string()),
        ),
        ("model".to_string(), TomlValue::String(model.to_string())),
        (
            format!("model_providers.{PROVIDER_ID}"),
            TomlValue::Table(provider),
        ),
        ("analytics.enabled".to_string(), TomlValue::Boolean(false)),
        ("feedback.enabled".to_string(), TomlValue::Boolean(false)),
        (
            "otel.log_user_prompt".to_string(),
            TomlValue::Boolean(false),
        ),
        (
            "otel.exporter".to_string(),
            TomlValue::String("none".to_string()),
        ),
        (
            "otel.trace_exporter".to_string(),
            TomlValue::String("none".to_string()),
        ),
        (
            "otel.metrics_exporter".to_string(),
            TomlValue::String("none".to_string()),
        ),
        (
            "check_for_update_on_startup".to_string(),
            TomlValue::Boolean(false),
        ),
    ]
}

pub(crate) fn config_warnings(config: &Config) -> Vec<ConfigWarningNotification> {
    config
        .startup_warnings
        .iter()
        .map(|warning| ConfigWarningNotification {
            summary: warning.clone(),
            details: None,
            path: None,
            range: None,
        })
        .collect()
}
use std::path::Path;
use std::path::PathBuf;
