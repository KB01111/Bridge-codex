use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use codex_keyring_store::CredentialStoreError;
use codex_keyring_store::KeyringStore;
use codex_keyring_store::tests::MockKeyringStore;
use keyring::Error as KeyringError;
use pretty_assertions::assert_eq;

use super::ProxyCredentialStore;

#[test]
fn keyring_round_trip_never_requires_a_plaintext_file() {
    let keyring = MockKeyringStore::default();
    let credentials = ProxyCredentialStore::with_keyring(Arc::new(keyring));

    assert_eq!(credentials.load().expect("initial load"), None);
    credentials.save("  secret-value  ").expect("save key");
    credentials
        .save_base_url("  http://127.0.0.1:8317/  ")
        .expect("save base URL");
    assert_eq!(
        credentials.load().expect("load saved key"),
        Some("secret-value".to_string())
    );
    assert_eq!(
        credentials.load_base_url().expect("load saved URL"),
        Some("http://127.0.0.1:8317/".to_string())
    );
    assert!(credentials.delete().expect("delete key"));
    assert_eq!(credentials.load().expect("load deleted key"), None);
    assert_eq!(credentials.load_base_url().expect("load deleted URL"), None);
}

#[test]
fn empty_api_keys_are_rejected() {
    let credentials = ProxyCredentialStore::with_keyring(Arc::new(MockKeyringStore::default()));

    let error = credentials.save(" \n\t ").expect_err("empty key must fail");

    assert_eq!(error.to_string(), "CLIProxyAPI API key must not be empty");
}

#[test]
fn connection_save_restores_both_prior_values_after_partial_keyring_failure() {
    let keyring = Arc::new(FailOnceKeyring::default());
    let credentials = ProxyCredentialStore::with_keyring(keyring.clone());
    credentials.save("working-key").expect("seed API key");
    credentials
        .save_base_url("http://127.0.0.1:8317/")
        .expect("seed base URL");
    keyring.fail_next_save_for("cliproxyapi|api-key");

    let error = credentials
        .save_connection("http://127.0.0.1:9417/", "candidate-key")
        .expect_err("candidate persistence must fail");

    assert!(
        error
            .to_string()
            .contains("failed to save the CLIProxyAPI key")
    );
    assert_eq!(
        credentials.load().expect("restored key"),
        Some("working-key".to_string())
    );
    assert_eq!(
        credentials.load_base_url().expect("restored URL"),
        Some("http://127.0.0.1:8317/".to_string())
    );
}

#[derive(Debug, Default)]
struct FailOnceKeyring {
    entries: Mutex<HashMap<String, String>>,
    failing_account: Mutex<Option<String>>,
}

impl FailOnceKeyring {
    fn fail_next_save_for(&self, account: &str) {
        *self.failing_account.lock().expect("failure lock") = Some(account.to_string());
    }
}

impl KeyringStore for FailOnceKeyring {
    fn load(&self, _service: &str, account: &str) -> Result<Option<String>, CredentialStoreError> {
        Ok(self
            .entries
            .lock()
            .expect("entry lock")
            .get(account)
            .cloned())
    }

    fn save(&self, _service: &str, account: &str, value: &str) -> Result<(), CredentialStoreError> {
        let mut failing_account = self.failing_account.lock().expect("failure lock");
        let should_fail = failing_account.as_deref() == Some(account);
        if should_fail {
            failing_account.take();
        }
        drop(failing_account);
        if should_fail {
            return Err(CredentialStoreError::new(KeyringError::NoStorageAccess(
                Box::new(std::io::Error::other("injected keyring failure")),
            )));
        }
        self.entries
            .lock()
            .expect("entry lock")
            .insert(account.to_string(), value.to_string());
        Ok(())
    }

    fn delete(&self, _service: &str, account: &str) -> Result<bool, CredentialStoreError> {
        Ok(self
            .entries
            .lock()
            .expect("entry lock")
            .remove(account)
            .is_some())
    }
}
