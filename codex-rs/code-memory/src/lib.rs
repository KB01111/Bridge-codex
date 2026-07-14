//! Offline, deterministic codebase indexing and retrieval.
//!
//! The crate deliberately has no network, database, model, or operating-system
//! integration. Callers provide source documents or an explicitly scoped root
//! directory and receive a serializable index that can be searched locally.

mod bm25;
mod chunker;
mod graph;
mod hash;
mod indexer;
mod language;
mod memory;
mod snapshot;
mod types;

pub use indexer::index_directory;
pub use indexer::index_subdirectory;
pub use language::Language;
pub use memory::CodeMemory;
pub use snapshot::load_snapshot;
pub use snapshot::save_snapshot;
pub use types::CodeMemorySnapshot;
pub use types::CodeMemoryStatus;
pub use types::GraphExplanation;
pub use types::GraphRelation;
pub use types::IndexLimits;
pub use types::IndexStatistics;
pub use types::IndexingReport;
pub use types::SearchRequest;
pub use types::SearchResult;
pub use types::SourceDocument;
pub use types::StructuralChunk;
pub use types::SyntaxIssue;
pub use types::SyntaxIssueKind;

/// Snapshot schema understood by this version of the crate.
pub const SNAPSHOT_SCHEMA_VERSION: u32 = 1;
