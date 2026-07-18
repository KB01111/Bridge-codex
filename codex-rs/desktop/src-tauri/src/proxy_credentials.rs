use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use codex_keyring_store::DefaultKeyringStore;
use codex_keyring_store::KeyringStore;

const KEYRING_SERVICE: &str = "Bridge Codex";
const API_KEY_ACCOUNT: &str = "cliproxyapi|api-key";
const BASE_URL_ACCOUNT: &str = "cliproxyapi|base-url";

#[derive(Clone)]
struct ProxyCredentialSnapshot {
    api_key: Option<String>,
    base_url: Option<String>,
}

/// Keeps the CLIProxyAPI bearer token in the operating system credential store.
///
/// The secret is only exposed to Rust callers that construct authenticated HTTP
/// clients; Tauri status payloads deliberately contain only an authentication
/// readiness flag.
#[derive(Clone)]
pub struct ProxyCredentialStore {
    keyring: Arc<dyn KeyringStore>,
}

impl Default for ProxyCredentialStore {
    fn default() -> Self {
        Self {
            keyring: Arc::new(DefaultKeyringStore),
        }
    }
}

impl ProxyCredentialStore {
    pub fn load(&self) -> Result<Option<String>> {
        self.keyring
            .load(KEYRING_SERVICE, API_KEY_ACCOUNT)
            .map_err(anyhow::Error::new)
            .context("failed to load the CLIProxyAPI key from the OS credential store")
            .map(|value| value.and_then(|value| normalize_api_key(&value)))
    }

    pub fn save(&self, api_key: &str) -> Result<()> {
        let api_key = Self::validated_api_key(api_key)?;
        self.keyring
            .save(KEYRING_SERVICE, API_KEY_ACCOUNT, &api_key)
            .map_err(anyhow::Error::new)
            .context("failed to save the CLIProxyAPI key in the OS credential store")
    }

    pub(crate) fn validated_api_key(api_key: &str) -> Result<String> {
        normalize_api_key(api_key).context("CLIProxyAPI API key must not be empty")
    }

    pub fn delete(&self) -> Result<bool> {
        let previous = self.snapshot()?;
        let result = (|| {
            let api_key_deleted = self.delete_entry(API_KEY_ACCOUNT)?;
            let base_url_deleted = self.delete_entry(BASE_URL_ACCOUNT)?;
            Ok(api_key_deleted || base_url_deleted)
        })();
        self.rollback_on_error(result, &previous)
    }

    pub fn load_base_url(&self) -> Result<Option<String>> {
        self.keyring
            .load(KEYRING_SERVICE, BASE_URL_ACCOUNT)
            .map_err(anyhow::Error::new)
            .context("failed to load the CLIProxyAPI URL from the OS credential store")
            .map(|value| value.and_then(|value| normalize_base_url(&value)))
    }

    pub fn save_base_url(&self, base_url: &str) -> Result<()> {
        let Some(base_url) = normalize_base_url(base_url) else {
            bail!("CLIProxyAPI base URL must not be empty");
        };
        self.keyring
            .save(KEYRING_SERVICE, BASE_URL_ACCOUNT, &base_url)
            .map_err(anyhow::Error::new)
            .context("failed to save the CLIProxyAPI URL in the OS credential store")
    }

    pub(crate) fn save_connection(&self, base_url: &str, api_key: &str) -> Result<()> {
        let Some(base_url) = normalize_base_url(base_url) else {
            bail!("CLIProxyAPI base URL must not be empty");
        };
        let api_key = Self::validated_api_key(api_key)?;
        let previous = self.snapshot()?;
        let result = (|| {
            self.save_base_url(&base_url)?;
            self.save(&api_key)
        })();
        self.rollback_on_error(result, &previous)
    }

    fn snapshot(&self) -> Result<ProxyCredentialSnapshot> {
        Ok(ProxyCredentialSnapshot {
            api_key: self.load()?,
            base_url: self.load_base_url()?,
        })
    }

    fn rollback_on_error<T>(
        &self,
        result: Result<T>,
        previous: &ProxyCredentialSnapshot,
    ) -> Result<T> {
        match result {
            Ok(value) => Ok(value),
            Err(error) => match self.restore(previous) {
                Ok(()) => Err(error),
                Err(rollback_error) => Err(anyhow!(
                    "{error:#}; additionally failed to restore the previous CLIProxyAPI credentials: {rollback_error:#}"
                )),
            },
        }
    }

    fn restore(&self, snapshot: &ProxyCredentialSnapshot) -> Result<()> {
        self.replace_entry(BASE_URL_ACCOUNT, snapshot.base_url.as_deref())
            .context("restore the previous CLIProxyAPI URL")?;
        self.replace_entry(API_KEY_ACCOUNT, snapshot.api_key.as_deref())
            .context("restore the previous CLIProxyAPI key")
    }

    fn replace_entry(&self, account: &str, value: Option<&str>) -> Result<()> {
        match value {
            Some(value) => self
                .keyring
                .save(KEYRING_SERVICE, account, value)
                .map_err(anyhow::Error::new),
            None => self
                .keyring
                .delete(KEYRING_SERVICE, account)
                .map(|_| ())
                .map_err(anyhow::Error::new),
        }
    }

    fn delete_entry(&self, account: &str) -> Result<bool> {
        self.keyring
            .delete(KEYRING_SERVICE, account)
            .map_err(anyhow::Error::new)
    }

    #[cfg(test)]
    pub(crate) fn with_keyring(keyring: Arc<dyn KeyringStore>) -> Self {
        Self { keyring }
    }
}

fn normalize_api_key(api_key: &str) -> Option<String> {
    let api_key = api_key.trim();
    (!api_key.is_empty()).then(|| api_key.to_string())
}

fn normalize_base_url(base_url: &str) -> Option<String> {
    let base_url = base_url.trim();
    (!base_url.is_empty()).then(|| base_url.to_string())
}

#[cfg(test)]
#[path = "proxy_credentials_tests.rs"]
mod tests;
