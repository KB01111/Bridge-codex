use std::fs::File;
use std::fs::OpenOptions;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use anyhow::Context;
use anyhow::Result;
use chrono::SecondsFormat;
use chrono::Utc;
use serde::Serialize;
use tracing::Level;
use tracing_subscriber::filter::Targets;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

const LOG_DIRECTORY: &str = "logs";
const SUPPORT_DIRECTORY: &str = "support";
const LOG_BASENAME: &str = "bridge.log";
const LOG_SEGMENT_STARTED: &str = "bridge.current.started";
const MAX_LOG_FILES: usize = 5;
const MAX_LOG_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_LOG_RECORD_BYTES: usize = 64 * 1024;
const MAX_LOG_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

#[derive(Clone)]
pub struct Diagnostics {
    app_data_dir: Arc<PathBuf>,
    writer: RedactedLogWriter,
}

impl Diagnostics {
    pub fn install(app_data_dir: PathBuf) -> Result<Self> {
        let writer = RedactedLogWriter::new(app_data_dir.join(LOG_DIRECTORY))?;
        let file_layer = tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_target(true)
            .with_writer(writer.clone());
        let filter = Targets::new().with_target("codex_desktop", Level::INFO);
        let _ = tracing_subscriber::registry()
            .with(filter)
            .with(file_layer)
            .try_init();
        Ok(Self {
            app_data_dir: Arc::new(app_data_dir),
            writer,
        })
    }

    pub fn app_data_dir(&self) -> &Path {
        self.app_data_dir.as_path()
    }

    pub fn export_support_bundle(&self) -> Result<PathBuf> {
        let mut log_state = self
            .writer
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("diagnostic log lock is poisoned"))?;
        expire_current_segment(&self.writer.root, &mut log_state)?;
        prune_expired_logs(&self.writer.root)?;
        let support_dir = self.app_data_dir.join(SUPPORT_DIRECTORY);
        std::fs::create_dir_all(&support_dir).context("create support export directory")?;
        let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ");
        let destination = support_dir.join(format!("bridge-support-{timestamp}.zip"));
        let file = File::create(&destination).context("create support bundle")?;
        let mut archive = ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        let manifest = SupportManifest {
            generated_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
            application: "Bridge Codex",
            version: env!("CARGO_PKG_VERSION"),
            operating_system: std::env::consts::OS,
            architecture: std::env::consts::ARCH,
            exclusions: [
                "prompts",
                "source text",
                "screenshots",
                "browser profiles",
                "rollouts",
                "credentials",
            ],
        };
        archive
            .start_file("diagnostics.json", options)
            .context("add support manifest")?;
        archive
            .write_all(&serde_json::to_vec_pretty(&manifest)?)
            .context("write support manifest")?;

        for (index, path) in managed_log_paths(&self.writer.root) {
            let Ok(log) = File::open(path) else {
                continue;
            };
            let mut bytes = Vec::new();
            log.take(MAX_LOG_FILE_BYTES)
                .read_to_end(&mut bytes)
                .context("read diagnostic log")?;
            archive
                .start_file(format!("logs/bridge-{index}.log"), options)
                .context("add diagnostic log")?;
            archive
                .write_all(&sanitize_log_record(&bytes))
                .context("write diagnostic log")?;
        }
        archive.finish().context("finish support bundle")?;
        Ok(destination)
    }

    pub fn disable_and_clear(&self) -> Result<()> {
        self.writer.enabled.store(false, Ordering::Release);
        let _log_guard = self
            .writer
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("diagnostic log lock is poisoned"))?;
        match std::fs::remove_dir_all(&*self.writer.root) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error).context("remove diagnostic logs"),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SupportManifest {
    generated_at: String,
    application: &'static str,
    version: &'static str,
    operating_system: &'static str,
    architecture: &'static str,
    exclusions: [&'static str; 6],
}

#[derive(Clone)]
struct RedactedLogWriter {
    root: Arc<PathBuf>,
    gate: Arc<Mutex<LogState>>,
    enabled: Arc<AtomicBool>,
}

struct LogState {
    segment_started_at: SystemTime,
}

impl RedactedLogWriter {
    fn new(root: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&root).context("create diagnostic log directory")?;
        prune_expired_logs(&root)?;
        let mut state = LogState {
            segment_started_at: load_segment_started_at(&root),
        };
        expire_current_segment(&root, &mut state)?;
        persist_segment_started_at(&root, state.segment_started_at)?;
        Ok(Self {
            root: Arc::new(root),
            gate: Arc::new(Mutex::new(state)),
            enabled: Arc::new(AtomicBool::new(true)),
        })
    }

    fn append(&self, record: &[u8]) -> std::io::Result<()> {
        if !self.enabled.load(Ordering::Acquire) {
            return Ok(());
        }
        let mut state = self
            .gate
            .lock()
            .map_err(|_| std::io::Error::other("diagnostic log lock is poisoned"))?;
        expire_current_segment(&self.root, &mut state)
            .map_err(|error| std::io::Error::other(format!("{error:#}")))?;
        prune_expired_logs(&self.root)
            .map_err(|error| std::io::Error::other(format!("{error:#}")))?;
        let record = sanitize_log_record(record);
        let current = self.root.join(LOG_BASENAME);
        let current_size = current.metadata().map_or(0, |metadata| metadata.len());
        if current_size.saturating_add(record.len() as u64) > MAX_LOG_FILE_BYTES {
            preserve_segment_start_as_modified(&current, state.segment_started_at)?;
            rotate_logs(&self.root)?;
            state.segment_started_at = SystemTime::now();
            persist_segment_started_at(&self.root, state.segment_started_at)
                .map_err(|error| std::io::Error::other(format!("{error:#}")))?;
            prune_expired_logs(&self.root)
                .map_err(|error| std::io::Error::other(format!("{error:#}")))?;
        }
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(current)?
            .write_all(&record)
    }
}

fn load_segment_started_at(root: &Path) -> SystemTime {
    let marker = root.join(LOG_SEGMENT_STARTED);
    if let Ok(seconds) = std::fs::read_to_string(marker)
        && let Ok(seconds) = seconds.trim().parse::<u64>()
        && let Some(started_at) = UNIX_EPOCH.checked_add(Duration::from_secs(seconds))
    {
        return started_at;
    }
    let current = root.join(LOG_BASENAME);
    current
        .metadata()
        .ok()
        .and_then(|metadata| metadata.created().or_else(|_| metadata.modified()).ok())
        .unwrap_or_else(SystemTime::now)
}

fn persist_segment_started_at(root: &Path, started_at: SystemTime) -> Result<()> {
    let seconds = started_at
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    std::fs::write(root.join(LOG_SEGMENT_STARTED), seconds.to_string())
        .context("persist diagnostic log segment age")
}

fn expire_current_segment(root: &Path, state: &mut LogState) -> Result<()> {
    let now = SystemTime::now();
    let expired = now
        .duration_since(state.segment_started_at)
        .is_ok_and(|age| age >= MAX_LOG_AGE);
    if !expired {
        return Ok(());
    }
    let current = root.join(LOG_BASENAME);
    if current.exists() {
        preserve_segment_start_as_modified(&current, state.segment_started_at)?;
        rotate_logs(root).context("rotate expired diagnostic log segment")?;
    }
    state.segment_started_at = now;
    persist_segment_started_at(root, now)?;
    prune_expired_logs(root)
}

fn preserve_segment_start_as_modified(path: &Path, started_at: SystemTime) -> std::io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    File::options()
        .write(true)
        .open(path)?
        .set_times(std::fs::FileTimes::new().set_modified(started_at))
}

impl<'writer> tracing_subscriber::fmt::MakeWriter<'writer> for RedactedLogWriter {
    type Writer = RedactedRecord<'writer>;

    fn make_writer(&'writer self) -> Self::Writer {
        RedactedRecord {
            owner: self,
            bytes: Vec::new(),
        }
    }
}

struct RedactedRecord<'writer> {
    owner: &'writer RedactedLogWriter,
    bytes: Vec<u8>,
}

impl Write for RedactedRecord<'_> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let remaining = MAX_LOG_RECORD_BYTES.saturating_sub(self.bytes.len());
        self.bytes
            .extend_from_slice(&buffer[..buffer.len().min(remaining)]);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for RedactedRecord<'_> {
    fn drop(&mut self) {
        if !self.bytes.is_empty() {
            let _ = self.owner.append(&self.bytes);
        }
    }
}

fn sanitize_log_record(record: &[u8]) -> Vec<u8> {
    let bounded = &record[..record.len().min(MAX_LOG_RECORD_BYTES)];
    let text = String::from_utf8_lossy(bounded);
    let lowercase = text.to_ascii_lowercase();
    let sensitive = [
        "authorization",
        "bearer ",
        "api_key",
        "api key",
        "credential",
        "prompt=",
        "source_text",
        "screenshot",
    ]
    .iter()
    .any(|marker| lowercase.contains(marker));
    if sensitive {
        return b"[redacted sensitive diagnostic record]\n".to_vec();
    }
    bounded.to_vec()
}

fn prune_expired_logs(root: &Path) -> Result<()> {
    let now = SystemTime::now();
    for (_, path) in managed_log_paths(root) {
        let Ok(metadata) = path.metadata() else {
            continue;
        };
        let expired = metadata
            .modified()
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age > MAX_LOG_AGE);
        if expired {
            std::fs::remove_file(&path)
                .with_context(|| format!("remove expired log {}", path.display()))?;
        }
    }
    Ok(())
}

fn rotate_logs(root: &Path) -> std::io::Result<()> {
    for index in (1..MAX_LOG_FILES).rev() {
        let source = rotated_log_path(root, index - 1);
        let destination = rotated_log_path(root, index);
        if destination.exists() {
            std::fs::remove_file(&destination)?;
        }
        if source.exists() {
            std::fs::rename(source, destination)?;
        }
    }
    Ok(())
}

fn managed_log_paths(root: &Path) -> impl Iterator<Item = (usize, PathBuf)> + '_ {
    (0..MAX_LOG_FILES).map(|index| (index, rotated_log_path(root, index)))
}

fn rotated_log_path(root: &Path, index: usize) -> PathBuf {
    if index == 0 {
        root.join(LOG_BASENAME)
    } else {
        root.join(format!("bridge.{index}.log"))
    }
}

#[tauri::command]
pub fn export_support_bundle(state: tauri::State<'_, Diagnostics>) -> Result<PathBuf, String> {
    state
        .export_support_bundle()
        .map_err(|error| format!("{error:#}"))
}

#[cfg(test)]
#[path = "diagnostics_tests.rs"]
mod tests;
