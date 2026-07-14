use serde::Deserialize;
use serde::Serialize;

use crate::Language;

/// Hard bounds applied while reading and chunking a source tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexLimits {
    /// Maximum filesystem entries inspected, including directories and links.
    pub max_discovered_entries: usize,
    /// Maximum number of source files accepted into one index.
    pub max_files: usize,
    /// Maximum number of bytes read from an individual file.
    pub max_file_bytes: usize,
    /// Maximum combined source bytes accepted into one index.
    pub max_source_bytes: usize,
    /// Maximum number of structural chunks in one index.
    pub max_chunks: usize,
    /// Maximum source bytes stored in an individual chunk.
    pub max_chunk_bytes: usize,
    /// Maximum directory depth below the selected root.
    pub max_depth: usize,
    /// Maximum retained discovery warnings.
    pub max_warnings: usize,
}

impl Default for IndexLimits {
    fn default() -> Self {
        Self {
            max_discovered_entries: 25_000,
            max_files: 5_000,
            max_file_bytes: 1024 * 1024,
            max_source_bytes: 128 * 1024 * 1024,
            max_chunks: 50_000,
            max_chunk_bytes: 32 * 1024,
            max_depth: 32,
            max_warnings: 256,
        }
    }
}

impl IndexLimits {
    pub(crate) fn validate(self) -> anyhow::Result<Self> {
        anyhow::ensure!(self.max_files > 0, "max_files must be greater than zero");
        anyhow::ensure!(
            self.max_discovered_entries > 0,
            "max_discovered_entries must be greater than zero"
        );
        anyhow::ensure!(
            self.max_file_bytes > 0,
            "max_file_bytes must be greater than zero"
        );
        anyhow::ensure!(
            self.max_source_bytes > 0,
            "max_source_bytes must be greater than zero"
        );
        anyhow::ensure!(self.max_chunks > 0, "max_chunks must be greater than zero");
        anyhow::ensure!(
            self.max_chunk_bytes >= 4,
            "max_chunk_bytes must be at least four to preserve UTF-8 characters"
        );
        anyhow::ensure!(self.max_depth > 0, "max_depth must be greater than zero");
        anyhow::ensure!(
            self.max_warnings > 0,
            "max_warnings must be greater than zero"
        );
        Ok(self)
    }
}

/// UTF-8 source supplied directly by a caller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceDocument {
    /// Normalized, root-relative display path.
    pub path: String,
    /// Explicit parser language.
    pub language: Language,
    /// Complete UTF-8 source text.
    pub source: String,
}

/// Kind of syntax problem reported by Tree-sitter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SyntaxIssueKind {
    Error,
    Missing,
}

/// Bounded source location for a parser problem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyntaxIssue {
    pub path: String,
    pub kind: SyntaxIssueKind,
    pub node_kind: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: usize,
    pub end_line: usize,
}

/// A deterministic source span produced from a syntax tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuralChunk {
    /// Stable FNV-derived identifier tied to the source and span.
    pub id: String,
    pub path: String,
    pub language: Language,
    pub symbol: Option<String>,
    pub kind: String,
    pub start_byte: usize,
    pub end_byte: usize,
    /// One-based inclusive start line.
    pub start_line: usize,
    /// One-based inclusive end line.
    pub end_line: usize,
    pub source: String,
}

/// Counters explaining what was accepted or skipped during indexing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexStatistics {
    pub discovered_entries: usize,
    pub discovered_files: usize,
    pub indexed_files: usize,
    pub skipped_files: usize,
    pub source_bytes: usize,
    pub chunks: usize,
    pub syntax_issues: usize,
    pub symbols: usize,
    pub graph_edges: usize,
}

/// Result of filesystem discovery before a searchable memory is built.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexingReport {
    pub documents: Vec<SourceDocument>,
    pub statistics: IndexStatistics,
    pub warnings: Vec<String>,
}

/// Type of relationship used for graph-assisted retrieval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GraphRelation {
    Import,
    Reference,
    SameFile,
}

/// Why a graph traversal contributed to a search result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphExplanation {
    pub from_chunk_id: String,
    pub relation: GraphRelation,
    pub contribution: f32,
}

/// Bounded local search parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchRequest {
    pub query: String,
    pub max_results: usize,
    pub graph_weight: f32,
}

impl SearchRequest {
    /// Creates a request with conservative defaults suitable for UI search.
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            max_results: 12,
            graph_weight: 0.25,
        }
    }
}

/// One ranked structural chunk and its scoring breakdown.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub chunk: StructuralChunk,
    pub score: f32,
    pub lexical_score: f32,
    pub graph_score: f32,
    pub explanations: Vec<GraphExplanation>,
}

/// Serializable health and capability summary for a loaded memory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeMemoryStatus {
    pub ready: bool,
    pub schema_version: u32,
    pub parser: String,
    pub retrieval: String,
    pub storage: String,
    pub network_access: String,
    pub statistics: IndexStatistics,
}

/// Versioned, self-contained JSON representation of an index.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeMemorySnapshot {
    pub schema_version: u32,
    pub chunks: Vec<StructuralChunk>,
    pub syntax_issues: Vec<SyntaxIssue>,
    pub statistics: IndexStatistics,
}
