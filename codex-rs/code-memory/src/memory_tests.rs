use crate::CodeMemory;
use crate::GraphRelation;
use crate::IndexLimits;
use crate::Language;
use crate::SearchRequest;
use crate::SourceDocument;

#[test]
fn retrieves_cross_file_references_with_graph_explanations() {
    let memory = CodeMemory::from_documents(
        vec![
            SourceDocument {
                path: "src/auth.rs".into(),
                language: Language::Rust,
                source: "pub fn verify_token(value: &str) -> bool { !value.is_empty() }\n".into(),
            },
            SourceDocument {
                path: "src/handler.rs".into(),
                language: Language::Rust,
                source: "use crate::auth::verify_token;\npub fn handle(value: &str) { let _ = verify_token(value); }\n".into(),
            },
        ],
        IndexLimits::default(),
    )
    .unwrap();
    let mut request = SearchRequest::new("verify_token");
    request.max_results = 10;
    request.graph_weight = 0.5;
    let results = memory.search(request).unwrap();

    assert!(results.iter().any(|result| {
        result.chunk.path == "src/handler.rs"
            && result.graph_score > 0.0
            && result.explanations.iter().any(|explanation| {
                matches!(
                    explanation.relation,
                    GraphRelation::Import | GraphRelation::Reference
                )
            })
    }));
    assert!(memory.status().statistics.graph_edges > 0);
}

#[test]
fn rejects_duplicate_and_traversing_document_paths() {
    let duplicate = vec![
        SourceDocument {
            path: "src/lib.rs".into(),
            language: Language::Rust,
            source: "fn one() {}".into(),
        },
        SourceDocument {
            path: "src/./lib.rs".into(),
            language: Language::Rust,
            source: "fn two() {}".into(),
        },
    ];
    assert!(CodeMemory::from_documents(duplicate, IndexLimits::default()).is_err());

    let traversal = vec![SourceDocument {
        path: "../secret.rs".into(),
        language: Language::Rust,
        source: "fn secret() {}".into(),
    }];
    assert!(CodeMemory::from_documents(traversal, IndexLimits::default()).is_err());
}
