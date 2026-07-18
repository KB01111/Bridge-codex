use codex_app_server_protocol::DynamicToolFunctionSpec;
use codex_app_server_protocol::DynamicToolNamespaceSpec;
use codex_app_server_protocol::DynamicToolNamespaceTool;
use codex_app_server_protocol::DynamicToolSpec;
use serde_json::json;

pub(crate) fn bridge_dynamic_tools() -> Vec<DynamicToolSpec> {
    vec![DynamicToolSpec::Namespace(DynamicToolNamespaceSpec {
        name: "bridge".to_string(),
        description: "Bounded Bridge desktop memory and browser tools.".to_string(),
        tools: vec![
            function(
                "memory_search",
                "Search the local Bridge code-memory index.",
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
                }),
            ),
            function(
                "browser_observe",
                "Read the current managed browser page state.",
                json!({
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                }),
            ),
            function(
                "browser_navigate",
                "Navigate the managed browser to an HTTP or HTTPS URL.",
                json!({
                    "type": "object",
                    "properties": {
                        "url": {"type": "string", "minLength": 1, "maxLength": 8192}
                    },
                    "required": ["url"],
                    "additionalProperties": false
                }),
            ),
            function(
                "browser_click",
                "Click one viewport coordinate in the managed browser.",
                json!({
                    "type": "object",
                    "properties": {
                        "x": {"type": "integer", "minimum": 0, "maximum": 16384},
                        "y": {"type": "integer", "minimum": 0, "maximum": 16384}
                    },
                    "required": ["x", "y"],
                    "additionalProperties": false
                }),
            ),
            function(
                "browser_type",
                "Type bounded text into the active managed-browser element.",
                json!({
                    "type": "object",
                    "properties": {
                        "text": {"type": "string", "maxLength": 16384}
                    },
                    "required": ["text"],
                    "additionalProperties": false
                }),
            ),
            function(
                "browser_key",
                "Send one named key to the managed browser.",
                json!({
                    "type": "object",
                    "properties": {
                        "key": {"type": "string", "minLength": 1, "maxLength": 64}
                    },
                    "required": ["key"],
                    "additionalProperties": false
                }),
            ),
        ],
    })]
}

fn function(
    name: &str,
    description: &str,
    input_schema: serde_json::Value,
) -> DynamicToolNamespaceTool {
    DynamicToolNamespaceTool::Function(DynamicToolFunctionSpec {
        name: name.to_string(),
        description: description.to_string(),
        input_schema,
        defer_loading: false,
    })
}
