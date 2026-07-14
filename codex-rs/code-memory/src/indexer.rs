use std::fs;
use std::fs::File;
use std::io::Read;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;

use crate::IndexLimits;
use crate::IndexStatistics;
use crate::IndexingReport;
use crate::Language;
use crate::SourceDocument;

const SKIPPED_DIRECTORIES: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    ".next",
    ".turbo",
    "build",
    "coverage",
    "dist",
    "generated",
    "node_modules",
    "target",
    "third_party",
    "vendor",
];

/// Reads supported UTF-8 sources below an explicitly selected directory.
///
/// Directory entries are sorted, symbolic links are ignored, and every read
/// is bounded before allocation. Paths in the report are relative to `root`.
pub fn index_directory(
    root: impl AsRef<Path>,
    limits: IndexLimits,
) -> anyhow::Result<IndexingReport> {
    let limits = limits.validate()?;
    let canonical_root = fs::canonicalize(root.as_ref()).with_context(|| {
        format!(
            "failed to resolve indexing root {}",
            root.as_ref().display()
        )
    })?;
    anyhow::ensure!(canonical_root.is_dir(), "indexing root must be a directory");

    let mut state = DiscoveryState {
        root: canonical_root.clone(),
        limits,
        documents: Vec::new(),
        statistics: IndexStatistics::default(),
        warnings: Vec::new(),
        source_limit_reached: false,
        file_limit_reached: false,
    };
    visit_directory(&canonical_root, /*depth*/ 0, &mut state)?;
    Ok(IndexingReport {
        documents: state.documents,
        statistics: state.statistics,
        warnings: state.warnings,
    })
}

/// Resolves and indexes a relative directory without permitting root escape.
///
/// This is the preferred entry point for sandbox hosts: pass `/sandbox` as
/// `allowed_root` and a caller-provided relative path as `relative`.
pub fn index_subdirectory(
    allowed_root: impl AsRef<Path>,
    relative: impl AsRef<Path>,
    limits: IndexLimits,
) -> anyhow::Result<IndexingReport> {
    validate_relative_components(relative.as_ref())?;
    let canonical_root = fs::canonicalize(allowed_root.as_ref()).with_context(|| {
        format!(
            "failed to resolve allowed root {}",
            allowed_root.as_ref().display()
        )
    })?;
    let target = fs::canonicalize(canonical_root.join(relative.as_ref())).with_context(|| {
        format!(
            "failed to resolve indexing subdirectory {}",
            relative.as_ref().display()
        )
    })?;
    anyhow::ensure!(
        target.starts_with(&canonical_root),
        "indexing path escapes the allowed root"
    );
    index_directory(target, limits)
}

pub(crate) fn normalize_document_path(path: &str) -> anyhow::Result<String> {
    let slash_path = path.replace('\\', "/");
    anyhow::ensure!(
        !slash_path.starts_with('/'),
        "document path must be relative"
    );
    anyhow::ensure!(
        !slash_path
            .split('/')
            .next()
            .is_some_and(|segment| segment.contains(':')),
        "document path must not contain a drive prefix"
    );
    let mut normalized = Vec::new();
    for component in slash_path.split('/') {
        match component {
            "" | "." => {}
            ".." => anyhow::bail!("document path must not contain parent traversal"),
            value => normalized.push(value),
        }
    }
    anyhow::ensure!(!normalized.is_empty(), "document path must not be empty");
    Ok(normalized.join("/"))
}

struct DiscoveryState {
    root: PathBuf,
    limits: IndexLimits,
    documents: Vec<SourceDocument>,
    statistics: IndexStatistics,
    warnings: Vec<String>,
    source_limit_reached: bool,
    file_limit_reached: bool,
}

fn visit_directory(
    directory: &Path,
    depth: usize,
    state: &mut DiscoveryState,
) -> anyhow::Result<()> {
    if state.file_limit_reached || state.source_limit_reached {
        return Ok(());
    }
    if depth > state.limits.max_depth {
        push_warning(
            state,
            format!(
                "skipped directory beyond depth limit: {}",
                display_relative(&state.root, directory)
            ),
        );
        return Ok(());
    }

    let mut entries = Vec::new();
    for entry in fs::read_dir(directory)
        .with_context(|| format!("failed to read directory {}", directory.display()))?
    {
        anyhow::ensure!(
            state.statistics.discovered_entries < state.limits.max_discovered_entries,
            "filesystem discovery exceeded the {}-entry limit",
            state.limits.max_discovered_entries
        );
        state.statistics.discovered_entries += 1;
        entries.push(entry?);
    }
    entries.sort_by_key(fs::DirEntry::file_name);

    for entry in entries {
        if state.file_limit_reached || state.source_limit_reached {
            break;
        }
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .with_context(|| format!("failed to inspect {}", path.display()))?;
        if metadata.file_type().is_symlink() {
            push_warning(
                state,
                format!(
                    "skipped symbolic link: {}",
                    display_relative(&state.root, &path)
                ),
            );
            continue;
        }
        if metadata.is_dir() {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if !SKIPPED_DIRECTORIES.contains(&name.as_str()) {
                visit_directory(&path, depth + 1, state)?;
            }
            continue;
        }
        if metadata.is_file() {
            visit_file(&path, metadata.len(), state)?;
        }
    }
    Ok(())
}

fn visit_file(path: &Path, reported_bytes: u64, state: &mut DiscoveryState) -> anyhow::Result<()> {
    state.statistics.discovered_files += 1;
    let relative = display_relative(&state.root, path);
    let Some(language) = Language::from_path(&relative) else {
        state.statistics.skipped_files += 1;
        return Ok(());
    };
    if is_generated_file(&relative) {
        state.statistics.skipped_files += 1;
        return Ok(());
    }
    if state.documents.len() >= state.limits.max_files {
        state.file_limit_reached = true;
        state.statistics.skipped_files += 1;
        push_warning(
            state,
            format!("stopped after {} source files", state.limits.max_files),
        );
        return Ok(());
    }
    if reported_bytes > state.limits.max_file_bytes as u64 {
        state.statistics.skipped_files += 1;
        push_warning(
            state,
            format!(
                "skipped file larger than {} bytes: {relative}",
                state.limits.max_file_bytes
            ),
        );
        return Ok(());
    }

    let mut bytes = Vec::with_capacity(reported_bytes as usize);
    File::open(path)
        .with_context(|| format!("failed to open {relative}"))?
        .take(state.limits.max_file_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("failed to read {relative}"))?;
    if bytes.len() > state.limits.max_file_bytes {
        state.statistics.skipped_files += 1;
        push_warning(
            state,
            format!("skipped file that grew beyond its byte limit: {relative}"),
        );
        return Ok(());
    }
    if bytes.contains(&0) {
        state.statistics.skipped_files += 1;
        push_warning(state, format!("skipped binary file: {relative}"));
        return Ok(());
    }
    let source = match String::from_utf8(bytes) {
        Ok(source) => source,
        Err(_) => {
            state.statistics.skipped_files += 1;
            push_warning(state, format!("skipped non-UTF-8 file: {relative}"));
            return Ok(());
        }
    };
    let Some(updated_source_bytes) = state.statistics.source_bytes.checked_add(source.len()) else {
        state.source_limit_reached = true;
        state.statistics.skipped_files += 1;
        push_warning(
            state,
            "stopped because the total source byte count overflowed".to_string(),
        );
        return Ok(());
    };
    if updated_source_bytes > state.limits.max_source_bytes {
        state.source_limit_reached = true;
        state.statistics.skipped_files += 1;
        push_warning(
            state,
            format!(
                "stopped before exceeding {} total source bytes",
                state.limits.max_source_bytes
            ),
        );
        return Ok(());
    }

    let canonical_file = fs::canonicalize(path)
        .with_context(|| format!("failed to resolve source file {relative}"))?;
    anyhow::ensure!(
        canonical_file.starts_with(&state.root),
        "source file escaped the indexing root"
    );
    state.statistics.indexed_files += 1;
    state.statistics.source_bytes = updated_source_bytes;
    state.documents.push(SourceDocument {
        path: normalize_document_path(&relative)?,
        language,
        source,
    });
    Ok(())
}

fn validate_relative_components(path: &Path) -> anyhow::Result<()> {
    anyhow::ensure!(
        !path.as_os_str().is_empty(),
        "relative path must not be empty"
    );
    for component in path.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                anyhow::bail!("indexing path must stay relative to the allowed root")
            }
        }
    }
    Ok(())
}

fn display_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn is_generated_file(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".min.js")
        || lower.ends_with(".min.css")
        || lower.ends_with(".generated.ts")
        || lower.ends_with(".generated.rs")
        || lower.ends_with("_generated.go")
}

fn push_warning(state: &mut DiscoveryState, warning: String) {
    if state.warnings.len() + 1 < state.limits.max_warnings {
        state.warnings.push(warning);
    } else if state.warnings.len() < state.limits.max_warnings {
        state
            .warnings
            .push("additional discovery warnings omitted".to_string());
    }
}

#[cfg(test)]
#[path = "indexer_tests.rs"]
mod tests;
