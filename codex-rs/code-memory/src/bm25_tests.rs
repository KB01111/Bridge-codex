use super::tokenize;

#[test]
fn tokenization_preserves_and_splits_code_identifiers() {
    assert_eq!(
        tokenize("HTTPClient parse_value"),
        vec![
            "httpclient",
            "http",
            "client",
            "parse_value",
            "parse",
            "value"
        ]
    );
}
