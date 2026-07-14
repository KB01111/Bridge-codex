use super::chunk_documents;
use crate::IndexLimits;
use crate::Language;
use crate::SourceDocument;
use crate::SyntaxIssueKind;

#[test]
fn creates_deterministic_rust_and_tsx_structural_chunks() {
    let documents = vec![
        SourceDocument {
            path: "src/lib.rs".into(),
            language: Language::Rust,
            source: "use crate::thing;\n\npub fn greet(name: &str) -> String { format!(\"hi {name}\") }\n".into(),
        },
        SourceDocument {
            path: "ui/App.tsx".into(),
            language: Language::Tsx,
            source: "export function App() { return <main>Hello</main>; }\n".into(),
        },
    ];
    let first = chunk_documents(&documents, IndexLimits::default()).unwrap();
    let second = chunk_documents(&documents, IndexLimits::default()).unwrap();

    assert_eq!(first.chunks, second.chunks);
    assert!(first.chunks.iter().any(|chunk| {
        chunk.language == Language::Rust && chunk.symbol.as_deref() == Some("greet")
    }));
    assert!(first.chunks.iter().any(|chunk| {
        chunk.language == Language::Tsx && chunk.symbol.as_deref() == Some("App")
    }));
}

#[test]
fn reports_syntax_errors_and_enforces_chunk_bytes() {
    let documents = vec![SourceDocument {
        path: "broken.py".into(),
        language: Language::Python,
        source: "def broken(:\n    return 'abcdefghijklmnopqrstuvwxyz'\n".into(),
    }];
    let limits = IndexLimits {
        max_chunk_bytes: 12,
        ..IndexLimits::default()
    };
    let output = chunk_documents(&documents, limits).unwrap();

    assert!(output.chunks.iter().all(|chunk| chunk.source.len() <= 12));
    assert!(output.syntax_issues.iter().any(
        |issue| issue.kind == SyntaxIssueKind::Error || issue.kind == SyntaxIssueKind::Missing
    ));
}
