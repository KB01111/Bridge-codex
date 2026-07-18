use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use codex_keyring_store::DefaultKeyringStore;
use codex_keyring_store::KeyringStore;
use uuid::Uuid;

use super::security::validate_a2a_token;

const KEYRING_SERVICE: &str = "Bridge Codex";
const KEYRING_ACCOUNT: &str = "a2a|bearer-token";

/// Stores the A2A bearer token in the operating system credential vault.
#[derive(Clone)]
pub(super) struct A2aCredentialStore {
    keyring: Arc<dyn KeyringStore>,
}

impl Default for A2aCredentialStore {
    fn default() -> Self {
        Self {
            keyring: Arc::new(DefaultKeyringStore),
        }
    }
}

impl A2aCredentialStore {
    pub(super) fn load(&self) -> Result<Option<String>> {
        self.keyring
            .load(KEYRING_SERVICE, KEYRING_ACCOUNT)
            .map_err(anyhow::Error::new)
            .context("failed to load the A2A bearer token from the OS credential store")?
            .map(|token| {
                validate_a2a_token(Some(&token)).map_err(|error| {
                    anyhow!("the A2A bearer token in the OS credential store is invalid: {error}")
                })?;
                Ok(token)
            })
            .transpose()
    }

    pub(super) fn save(&self, token: &str) -> Result<()> {
        validate_a2a_token(Some(token)).map_err(anyhow::Error::msg)?;
        self.keyring
            .save(KEYRING_SERVICE, KEYRING_ACCOUNT, token)
            .map_err(anyhow::Error::new)
            .context("failed to save the A2A bearer token in the OS credential store")
    }

    pub(super) fn delete(&self) -> Result<bool> {
        self.keyring
            .delete(KEYRING_SERVICE, KEYRING_ACCOUNT)
            .map_err(anyhow::Error::new)
            .context("failed to delete the A2A bearer token from the OS credential store")
    }

    pub(super) fn generate(&self, replace_existing: bool) -> Result<String> {
        if !replace_existing && self.load()?.is_some() {
            bail!("an A2A bearer token is already configured; regenerate it explicitly");
        }
        let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        self.save(&token)?;
        Ok(token)
    }

    #[cfg(test)]
    pub(super) fn with_keyring(keyring: Arc<dyn KeyringStore>) -> Self {
        Self { keyring }
    }
}

#[cfg(test)]
#[path = "a2a_credentials_tests.rs"]
mod tests;
