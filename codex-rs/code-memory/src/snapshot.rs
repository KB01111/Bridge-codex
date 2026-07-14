use std::ffi::OsString;
use std::fs;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use anyhow::Context;

use crate::CodeMemory;
use crate::CodeMemorySnapshot;
use crate::IndexLimits;

static SNAPSHOT_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Writes a compact JSON snapshot using a sibling temporary file and rename.
pub fn save_snapshot(memory: &CodeMemory, path: impl AsRef<Path>) -> anyhow::Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create snapshot directory {}", parent.display()))?;
    }
    let temporary = sibling_path(path, "tmp");
    let write_result = (|| -> anyhow::Result<()> {
        let bytes = serde_json::to_vec(&memory.snapshot())?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .with_context(|| format!("failed to create snapshot {}", temporary.display()))?;
        file.write_all(&bytes)
            .with_context(|| format!("failed to write snapshot {}", temporary.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to flush snapshot {}", temporary.display()))?;
        replace_by_rename(&temporary, path)
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    write_result
}

/// Loads and validates a bounded JSON snapshot without network or database IO.
pub fn load_snapshot(path: impl AsRef<Path>, limits: IndexLimits) -> anyhow::Result<CodeMemory> {
    let limits = limits.validate()?;
    let byte_limit = limits
        .max_source_bytes
        .saturating_add(limits.max_chunks.saturating_mul(2_048))
        .saturating_add(1024 * 1024);
    let mut bytes = Vec::new();
    File::open(path.as_ref())
        .with_context(|| format!("failed to open snapshot {}", path.as_ref().display()))?
        .take(byte_limit as u64 + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("failed to read snapshot {}", path.as_ref().display()))?;
    anyhow::ensure!(
        bytes.len() <= byte_limit,
        "snapshot exceeds the configured read limit"
    );
    let snapshot: CodeMemorySnapshot = serde_json::from_slice(&bytes)
        .with_context(|| format!("invalid snapshot JSON in {}", path.as_ref().display()))?;
    CodeMemory::from_snapshot(snapshot, limits)
}

fn replace_by_rename(temporary: &Path, destination: &Path) -> anyhow::Result<()> {
    if !destination.exists() {
        return fs::rename(temporary, destination).with_context(|| {
            format!(
                "failed to move snapshot {} to {}",
                temporary.display(),
                destination.display()
            )
        });
    }

    let backup = sibling_path(destination, "backup");
    fs::rename(destination, &backup).with_context(|| {
        format!(
            "failed to prepare existing snapshot {} for replacement",
            destination.display()
        )
    })?;
    if let Err(error) = fs::rename(temporary, destination) {
        let restore_result = fs::rename(&backup, destination);
        if let Err(restore_error) = restore_result {
            return Err(anyhow::anyhow!(
                "snapshot replacement failed ({error}) and backup restoration failed ({restore_error})"
            ));
        }
        return Err(error)
            .with_context(|| format!("failed to replace snapshot {}", destination.display()));
    }
    fs::remove_file(&backup)
        .with_context(|| format!("failed to remove snapshot backup {}", backup.display()))?;
    Ok(())
}

fn sibling_path(path: &Path, label: &str) -> PathBuf {
    let counter = SNAPSHOT_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut name = path
        .file_name()
        .map_or_else(|| OsString::from("code-memory"), ToOwned::to_owned);
    name.push(format!(".{label}-{}-{counter}", std::process::id()));
    path.with_file_name(name)
}

#[cfg(test)]
#[path = "snapshot_tests.rs"]
mod tests;
