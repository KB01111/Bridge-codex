use std::sync::Arc;

use codex_keyring_store::KeyringStore;
use codex_keyring_store::tests::MockKeyringStore;
use pretty_assertions::assert_eq;

use super::A2aCredentialStore;
use crate::a2a::security::validate_a2a_token;

#[test]
fn generated_tokens_round_trip_through_the_os_credential_abstraction() {
    let credentials = A2aCredentialStore::with_keyring(Arc::new(MockKeyringStore::default()));

    let first = credentials
        .generate(/*replace_existing*/ false)
        .expect("generate");
    let duplicate = credentials
        .generate(/*replace_existing*/ false)
        .expect_err("duplicate generation must fail");
    let replacement = credentials
        .generate(/*replace_existing*/ true)
        .expect("regenerate");

    assert!(validate_a2a_token(Some(&first)).is_ok());
    assert!(validate_a2a_token(Some(&replacement)).is_ok());
    assert_ne!(first, replacement);
    assert_eq!(
        duplicate.to_string(),
        "an A2A bearer token is already configured; regenerate it explicitly"
    );
    assert_eq!(credentials.load().expect("load"), Some(replacement));
    assert!(credentials.delete().expect("delete"));
    assert_eq!(credentials.load().expect("load deleted"), None);
}

#[test]
fn invalid_keyring_values_fail_closed() {
    let keyring = MockKeyringStore::default();
    keyring
        .save("Bridge Codex", "a2a|bearer-token", "too-short")
        .expect("seed invalid token");
    let credentials = A2aCredentialStore::with_keyring(Arc::new(keyring));

    let error = credentials.load().expect_err("invalid token must fail");

    assert!(error.to_string().contains("credential store is invalid"));
}
