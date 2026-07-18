use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use serde::Deserialize;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::a2a_protocol::A2aServerSettings;
use crate::a2a_protocol::A2aTask;
use crate::a2a_task_store::TaskStore;

const A2A_DIRECTORY: &str = "a2a";
const SETTINGS_FILE: &str = "settings-v1.json";
const TASKS_FILE: &str = "tasks-v1.json";
const PERSISTENCE_VERSION: u32 = 1;
const MAX_SETTINGS_BYTES: u64 = 4 * 1024;
const MAX_TASK_STORE_BYTES: u64 = 12 * 1024 * 1024;
pub(super) const MIN_A2A_PORT: u16 = 1024;

#[derive(Clone)]
pub(super) struct A2aPersistence {
    directory: Arc<PathBuf>,
    write_gate: Arc<Mutex<()>>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PersistedSettings {
    version: u32,
    settings: A2aServerSettings,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PersistedTasks {
    version: u32,
    tasks: Vec<A2aTask>,
}

impl A2aPersistence {
    pub(super) fn new(app_data_dir: PathBuf) -> Self {
        Self {
            directory: Arc::new(app_data_dir.join(A2A_DIRECTORY)),
            write_gate: Arc::new(Mutex::new(())),
        }
    }

    pub(super) fn load_settings(&self) -> Result<A2aServerSettings> {
        let Some(persisted) = read_bounded_json::<PersistedSettings>(
            &self.directory.join(SETTINGS_FILE),
            MAX_SETTINGS_BYTES,
        )?
        else {
            return Ok(A2aServerSettings::default());
        };
        if persisted.version != PERSISTENCE_VERSION {
            bail!("unsupported A2A settings version {}", persisted.version);
        }
        validate_settings(persisted.settings)?;
        Ok(persisted.settings)
    }

    pub(super) fn load_tasks(&self) -> Result<TaskStore> {
        let Some(persisted) = read_bounded_json::<PersistedTasks>(
            &self.directory.join(TASKS_FILE),
            MAX_TASK_STORE_BYTES,
        )?
        else {
            return Ok(TaskStore::default());
        };
        if persisted.version != PERSISTENCE_VERSION {
            bail!("unsupported A2A task-store version {}", persisted.version);
        }
        Ok(TaskStore::from_oldest_first(persisted.tasks))
    }

    pub(super) async fn save_settings(&self, settings: A2aServerSettings) -> Result<()> {
        validate_settings(settings)?;
        let persistence = self.clone();
        tokio::task::spawn_blocking(move || persistence.save_settings_blocking(settings))
            .await
            .context("A2A settings persistence task failed")?
    }

    pub(super) async fn save_tasks(&self, tasks: Vec<A2aTask>) -> Result<()> {
        let persistence = self.clone();
        tokio::task::spawn_blocking(move || persistence.save_tasks_blocking(tasks))
            .await
            .context("A2A task persistence task failed")?
    }

    pub(super) fn save_tasks_blocking(&self, tasks: Vec<A2aTask>) -> Result<()> {
        self.write_json(
            &self.directory.join(TASKS_FILE),
            &PersistedTasks {
                version: PERSISTENCE_VERSION,
                tasks,
            },
            MAX_TASK_STORE_BYTES,
        )
    }

    fn save_settings_blocking(&self, settings: A2aServerSettings) -> Result<()> {
        self.write_json(
            &self.directory.join(SETTINGS_FILE),
            &PersistedSettings {
                version: PERSISTENCE_VERSION,
                settings,
            },
            MAX_SETTINGS_BYTES,
        )
    }

    fn write_json<T>(&self, path: &Path, value: &T, max_bytes: u64) -> Result<()>
    where
        T: Serialize,
    {
        let bytes = serde_json::to_vec_pretty(value).context("serialize A2A persistent state")?;
        if bytes.len() as u64 > max_bytes {
            bail!("A2A persistent state exceeds its {max_bytes}-byte limit");
        }
        let _guard = self
            .write_gate
            .lock()
            .map_err(|_| anyhow!("A2A persistence lock is poisoned"))?;
        atomic_write(path, &bytes)
    }
}

pub(super) fn validate_settings(settings: A2aServerSettings) -> Result<()> {
    if settings.port < MIN_A2A_PORT {
        bail!("A2A port must be between {MIN_A2A_PORT} and {}", u16::MAX);
    }
    Ok(())
}

fn read_bounded_json<T>(path: &Path, max_bytes: u64) -> Result<Option<T>>
where
    T: DeserializeOwned,
{
    match read_candidate(path, max_bytes) {
        Ok(Some(value)) => Ok(Some(value)),
        Ok(None) => read_candidate(&with_suffix(path, ".bak"), max_bytes),
        Err(primary_error) => match read_candidate(&with_suffix(path, ".bak"), max_bytes) {
            Ok(Some(value)) => Ok(Some(value)),
            Ok(None) => Err(primary_error),
            Err(backup_error) => Err(anyhow!(
                "failed to read {} ({primary_error}) and its recovery backup ({backup_error})",
                path.display()
            )),
        },
    }
}

fn read_candidate<T>(path: &Path, max_bytes: u64) -> Result<Option<T>>
where
    T: DeserializeOwned,
{
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("inspect {}", path.display())),
    };
    if metadata.len() > max_bytes {
        bail!("{} exceeds its {max_bytes}-byte limit", path.display());
    }
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_slice(&bytes)
        .with_context(|| format!("parse {}", path.display()))
        .map(Some)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("{} has no parent directory", path.display()))?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("create A2A data directory {}", parent.display()))?;
    let temporary = with_suffix(path, ".new");
    let backup = with_suffix(path, ".bak");
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temporary)
        .with_context(|| format!("open {}", temporary.display()))?;
    file.write_all(bytes)
        .with_context(|| format!("write {}", temporary.display()))?;
    file.sync_all()
        .with_context(|| format!("sync {}", temporary.display()))?;
    drop(file);

    if backup.exists() {
        std::fs::remove_file(&backup)
            .with_context(|| format!("remove stale backup {}", backup.display()))?;
    }
    if path.exists() {
        std::fs::rename(path, &backup).with_context(|| {
            format!(
                "move {} to recovery backup {}",
                path.display(),
                backup.display()
            )
        })?;
    }
    if let Err(error) = std::fs::rename(&temporary, path) {
        if backup.exists() {
            let _ = std::fs::rename(&backup, path);
        }
        return Err(error)
            .with_context(|| format!("replace {} with {}", path.display(), temporary.display()));
    }
    if backup.exists() {
        std::fs::remove_file(&backup)
            .with_context(|| format!("remove recovery backup {}", backup.display()))?;
    }
    Ok(())
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value = OsString::from(path.as_os_str());
    value.push(suffix);
    PathBuf::from(value)
}

#[cfg(test)]
#[path = "a2a_persistence_tests.rs"]
mod tests;
