use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Context;

use crate::CodeMemorySnapshot;
use crate::CodeMemoryStatus;
use crate::IndexLimits;
use crate::IndexStatistics;
use crate::IndexingReport;
use crate::SNAPSHOT_SCHEMA_VERSION;
use crate::SearchRequest;
use crate::SearchResult;
use crate::SourceDocument;
use crate::StructuralChunk;
use crate::SyntaxIssue;
use crate::bm25::Bm25Index;
use crate::chunker::chunk_documents;
use crate::graph::CodeGraph;
use crate::hash::stable_chunk_id;
use crate::index_directory;
use crate::indexer::normalize_document_path;

const MAX_QUERY_BYTES: usize = 4 * 1024;
const MAX_SEARCH_RESULTS: usize = 100;

/// Immutable offline source index with lexical and graph-assisted retrieval.
pub struct CodeMemory {
    chunks: Vec<StructuralChunk>,
    syntax_issues: Vec<SyntaxIssue>,
    statistics: IndexStatistics,
    lexical_index: Bm25Index,
    graph: CodeGraph,
}

impl CodeMemory {
    /// Parses and indexes caller-supplied UTF-8 source documents.
    pub fn from_documents(
        mut documents: Vec<SourceDocument>,
        limits: IndexLimits,
    ) -> anyhow::Result<Self> {
        let limits = limits.validate()?;
        normalize_and_sort_documents(&mut documents)?;
        let statistics = IndexStatistics {
            discovered_entries: documents.len(),
            discovered_files: documents.len(),
            indexed_files: documents.len(),
            ..IndexStatistics::default()
        };
        Self::build(documents, limits, statistics)
    }

    /// Discovers and indexes sources below an explicitly selected directory.
    pub fn from_directory(root: impl AsRef<Path>, limits: IndexLimits) -> anyhow::Result<Self> {
        let report = index_directory(root, limits)?;
        Self::from_indexing_report(report, limits)
    }

    /// Builds a memory from a previously inspected filesystem report.
    pub fn from_indexing_report(
        mut report: IndexingReport,
        limits: IndexLimits,
    ) -> anyhow::Result<Self> {
        let limits = limits.validate()?;
        normalize_and_sort_documents(&mut report.documents)?;
        Self::build(report.documents, limits, report.statistics)
    }

    /// Rehydrates a validated, versioned snapshot.
    pub fn from_snapshot(
        mut snapshot: CodeMemorySnapshot,
        limits: IndexLimits,
    ) -> anyhow::Result<Self> {
        let limits = limits.validate()?;
        anyhow::ensure!(
            snapshot.schema_version == SNAPSHOT_SCHEMA_VERSION,
            "unsupported code-memory snapshot schema {}",
            snapshot.schema_version
        );
        anyhow::ensure!(
            snapshot.chunks.len() <= limits.max_chunks,
            "snapshot exceeds the configured chunk limit"
        );
        anyhow::ensure!(
            snapshot.syntax_issues.len() <= limits.max_chunks,
            "snapshot exceeds the configured syntax issue limit"
        );
        anyhow::ensure!(
            snapshot.statistics.source_bytes <= limits.max_source_bytes,
            "snapshot exceeds the configured source byte limit"
        );
        anyhow::ensure!(
            snapshot.statistics.indexed_files <= limits.max_files,
            "snapshot exceeds the configured file limit"
        );
        anyhow::ensure!(
            snapshot.statistics.discovered_entries <= limits.max_discovered_entries,
            "snapshot exceeds the configured discovery entry limit"
        );
        let mut ids = BTreeSet::new();
        let mut stored_source_bytes = 0_usize;
        for chunk in &mut snapshot.chunks {
            chunk.path = normalize_document_path(&chunk.path)?;
            anyhow::ensure!(
                chunk.source.len() <= limits.max_chunk_bytes,
                "snapshot chunk {} exceeds the configured byte limit",
                chunk.id
            );
            anyhow::ensure!(
                chunk.start_byte <= chunk.end_byte,
                "invalid chunk byte range"
            );
            anyhow::ensure!(
                chunk.end_byte - chunk.start_byte == chunk.source.len(),
                "snapshot chunk {} has an inconsistent source range",
                chunk.id
            );
            anyhow::ensure!(
                chunk.start_line > 0 && chunk.start_line <= chunk.end_line,
                "snapshot chunk {} has an invalid line range",
                chunk.id
            );
            anyhow::ensure!(!chunk.id.is_empty(), "snapshot contains an empty chunk id");
            anyhow::ensure!(
                ids.insert(chunk.id.clone()),
                "snapshot contains duplicate chunk ids"
            );
            let expected_id = stable_chunk_id(
                &chunk.path,
                chunk.language,
                &chunk.kind,
                chunk.symbol.as_deref(),
                chunk.start_byte,
                chunk.end_byte,
                &chunk.source,
            );
            anyhow::ensure!(
                chunk.id == expected_id,
                "snapshot chunk has an invalid stable id"
            );
            stored_source_bytes = stored_source_bytes
                .checked_add(chunk.source.len())
                .context("snapshot chunk byte count overflowed")?;
        }
        anyhow::ensure!(
            stored_source_bytes <= limits.max_source_bytes,
            "snapshot chunks exceed the configured source byte limit"
        );
        for issue in &mut snapshot.syntax_issues {
            issue.path = normalize_document_path(&issue.path)?;
            anyhow::ensure!(
                issue.start_byte <= issue.end_byte,
                "invalid syntax issue byte range"
            );
            anyhow::ensure!(
                issue.start_line > 0 && issue.start_line <= issue.end_line,
                "invalid syntax issue line range"
            );
        }
        snapshot.chunks.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then_with(|| left.start_byte.cmp(&right.start_byte))
                .then_with(|| left.id.cmp(&right.id))
        });
        Self::assemble(snapshot.chunks, snapshot.syntax_issues, snapshot.statistics)
    }

    /// Returns a versioned JSON-ready representation of the complete index.
    pub fn snapshot(&self) -> CodeMemorySnapshot {
        CodeMemorySnapshot {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            chunks: self.chunks.clone(),
            syntax_issues: self.syntax_issues.clone(),
            statistics: self.statistics.clone(),
        }
    }

    /// Searches locally using BM25 followed by a bounded one-hop graph boost.
    pub fn search(&self, request: SearchRequest) -> anyhow::Result<Vec<SearchResult>> {
        let query = request.query.trim();
        anyhow::ensure!(!query.is_empty(), "search query must not be empty");
        anyhow::ensure!(
            query.len() <= MAX_QUERY_BYTES,
            "search query exceeds the {MAX_QUERY_BYTES}-byte limit"
        );
        anyhow::ensure!(
            (1..=MAX_SEARCH_RESULTS).contains(&request.max_results),
            "max_results must be between 1 and {MAX_SEARCH_RESULTS}"
        );
        anyhow::ensure!(
            request.graph_weight.is_finite() && (0.0..=2.0).contains(&request.graph_weight),
            "graph_weight must be finite and between 0 and 2"
        );

        let lexical_scores = self.lexical_index.scores(query);
        let (graph_scores, explanations) =
            self.graph
                .boosts(&self.chunks, &lexical_scores, request.graph_weight);
        let mut results: Vec<SearchResult> = self
            .chunks
            .iter()
            .cloned()
            .enumerate()
            .filter_map(|(index, chunk)| {
                let score = lexical_scores[index] + graph_scores[index];
                (score > 0.0).then(|| SearchResult {
                    chunk,
                    score,
                    lexical_score: lexical_scores[index],
                    graph_score: graph_scores[index],
                    explanations: explanations[index].clone(),
                })
            })
            .collect();
        results.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| left.chunk.path.cmp(&right.chunk.path))
                .then_with(|| left.chunk.start_byte.cmp(&right.chunk.start_byte))
                .then_with(|| left.chunk.id.cmp(&right.chunk.id))
        });
        results.truncate(request.max_results);
        Ok(results)
    }

    /// Reports parser, storage, retrieval, and index counters.
    pub fn status(&self) -> CodeMemoryStatus {
        CodeMemoryStatus {
            ready: true,
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            parser: "tree-sitter".to_string(),
            retrieval: "bm25+symbol-import-graph".to_string(),
            storage: "versioned-json".to_string(),
            network_access: "none".to_string(),
            statistics: self.statistics.clone(),
        }
    }

    /// Returns deterministic structural chunks in path and source order.
    pub fn chunks(&self) -> &[StructuralChunk] {
        &self.chunks
    }

    /// Returns bounded Tree-sitter syntax diagnostics collected during parsing.
    pub fn syntax_issues(&self) -> &[SyntaxIssue] {
        &self.syntax_issues
    }

    /// Returns immutable index counters.
    pub fn statistics(&self) -> &IndexStatistics {
        &self.statistics
    }

    fn build(
        documents: Vec<SourceDocument>,
        limits: IndexLimits,
        mut statistics: IndexStatistics,
    ) -> anyhow::Result<Self> {
        let output = chunk_documents(&documents, limits)?;
        statistics.indexed_files = documents.len();
        statistics.source_bytes = output.source_bytes;
        statistics.chunks = output.chunks.len();
        statistics.syntax_issues = output.syntax_issues.len();
        Self::assemble(output.chunks, output.syntax_issues, statistics)
    }

    fn assemble(
        chunks: Vec<StructuralChunk>,
        syntax_issues: Vec<SyntaxIssue>,
        mut statistics: IndexStatistics,
    ) -> anyhow::Result<Self> {
        let graph = CodeGraph::build(&chunks);
        statistics.chunks = chunks.len();
        statistics.syntax_issues = syntax_issues.len();
        statistics.symbols = graph.symbol_count();
        statistics.graph_edges = graph.edge_count();
        let lexical_index = Bm25Index::new(&chunks);
        Ok(Self {
            chunks,
            syntax_issues,
            statistics,
            lexical_index,
            graph,
        })
    }
}

fn normalize_and_sort_documents(documents: &mut [SourceDocument]) -> anyhow::Result<()> {
    for document in documents.iter_mut() {
        document.path = normalize_document_path(&document.path)?;
    }
    documents.sort_by(|left, right| left.path.cmp(&right.path));
    for pair in documents.windows(2) {
        anyhow::ensure!(
            pair[0].path != pair[1].path,
            "duplicate document path {}",
            pair[0].path
        );
    }
    Ok(())
}

#[cfg(test)]
#[path = "memory_tests.rs"]
mod tests;
