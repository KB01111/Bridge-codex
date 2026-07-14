use anyhow::Context;
use tree_sitter::Node;
use tree_sitter::Parser;

use crate::IndexLimits;
use crate::SourceDocument;
use crate::StructuralChunk;
use crate::SyntaxIssue;
use crate::SyntaxIssueKind;
use crate::hash::stable_chunk_id;

pub(crate) struct ChunkingOutput {
    pub(crate) chunks: Vec<StructuralChunk>,
    pub(crate) syntax_issues: Vec<SyntaxIssue>,
    pub(crate) source_bytes: usize,
}

pub(crate) fn chunk_documents(
    documents: &[SourceDocument],
    limits: IndexLimits,
) -> anyhow::Result<ChunkingOutput> {
    anyhow::ensure!(
        documents.len() <= limits.max_files,
        "document count exceeds the configured file limit"
    );
    let source_bytes = documents.iter().try_fold(0_usize, |total, document| {
        total.checked_add(document.source.len())
    });
    let Some(source_bytes) = source_bytes else {
        anyhow::bail!("combined source byte count overflowed");
    };
    anyhow::ensure!(
        source_bytes <= limits.max_source_bytes,
        "documents exceed the configured total source byte limit"
    );

    let mut chunks = Vec::new();
    let mut syntax_issues = Vec::new();
    for document in documents {
        anyhow::ensure!(
            document.source.len() <= limits.max_file_bytes,
            "{} exceeds the configured per-file byte limit",
            document.path
        );
        let mut parser = Parser::new();
        parser
            .set_language(&document.language.tree_sitter_language())
            .with_context(|| format!("failed to configure parser for {}", document.path))?;
        let tree = parser
            .parse(&document.source, /*old_tree*/ None)
            .with_context(|| format!("parser returned no tree for {}", document.path))?;
        let root = tree.root_node();
        collect_syntax_issues(root, document, &mut syntax_issues, limits.max_chunks);

        let mut structures = Vec::new();
        collect_outer_structures(root, document.language, &mut structures);
        structures.sort_by_key(Node::start_byte);
        append_document_chunks(document, &structures, limits.max_chunk_bytes, &mut chunks);
        anyhow::ensure!(
            chunks.len() <= limits.max_chunks,
            "structural chunk count exceeds the configured limit"
        );
    }

    Ok(ChunkingOutput {
        chunks,
        syntax_issues,
        source_bytes,
    })
}

fn collect_outer_structures<'tree>(
    node: Node<'tree>,
    language: crate::Language,
    output: &mut Vec<Node<'tree>>,
) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if language.is_structure(child.kind()) {
            output.push(child);
        } else {
            collect_outer_structures(child, language, output);
        }
    }
}

fn collect_syntax_issues(
    node: Node<'_>,
    document: &SourceDocument,
    output: &mut Vec<SyntaxIssue>,
    issue_limit: usize,
) {
    if output.len() >= issue_limit {
        return;
    }
    let issue_kind = if node.is_error() {
        Some(SyntaxIssueKind::Error)
    } else if node.is_missing() {
        Some(SyntaxIssueKind::Missing)
    } else {
        None
    };
    if let Some(kind) = issue_kind {
        output.push(SyntaxIssue {
            path: document.path.clone(),
            kind,
            node_kind: node.kind().to_string(),
            start_byte: node.start_byte(),
            end_byte: node.end_byte(),
            start_line: node.start_position().row + 1,
            end_line: node.end_position().row + 1,
        });
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_syntax_issues(child, document, output, issue_limit);
        if output.len() >= issue_limit {
            break;
        }
    }
}

fn append_document_chunks(
    document: &SourceDocument,
    structures: &[Node<'_>],
    max_chunk_bytes: usize,
    output: &mut Vec<StructuralChunk>,
) {
    let mut previous_end = 0;
    for structure in structures {
        append_span(
            document,
            previous_end,
            structure.start_byte(),
            "module",
            /*symbol*/ None,
            max_chunk_bytes,
            output,
        );
        let symbol = find_symbol(*structure, document);
        append_span(
            document,
            structure.start_byte(),
            structure.end_byte(),
            structure.kind(),
            symbol.as_deref(),
            max_chunk_bytes,
            output,
        );
        previous_end = structure.end_byte();
    }
    let fallback_kind = if structures.is_empty() {
        "file"
    } else {
        "module"
    };
    append_span(
        document,
        previous_end,
        document.source.len(),
        fallback_kind,
        /*symbol*/ None,
        max_chunk_bytes,
        output,
    );
}

fn append_span(
    document: &SourceDocument,
    start: usize,
    end: usize,
    kind: &str,
    symbol: Option<&str>,
    max_chunk_bytes: usize,
    output: &mut Vec<StructuralChunk>,
) {
    let Some((trimmed_start, trimmed_end)) = trim_span(&document.source, start, end) else {
        return;
    };
    let mut part_start = trimmed_start;
    while part_start < trimmed_end {
        let part_end = choose_part_end(&document.source, part_start, trimmed_end, max_chunk_bytes);
        let Some(source) = document.source.get(part_start..part_end) else {
            return;
        };
        let id = stable_chunk_id(
            &document.path,
            document.language,
            kind,
            symbol,
            part_start,
            part_end,
            source,
        );
        output.push(StructuralChunk {
            id,
            path: document.path.clone(),
            language: document.language,
            symbol: symbol.map(ToOwned::to_owned),
            kind: kind.to_string(),
            start_byte: part_start,
            end_byte: part_end,
            start_line: line_at(&document.source, part_start),
            end_line: line_at(&document.source, part_end.saturating_sub(1)),
            source: source.to_string(),
        });
        part_start = part_end;
    }
}

fn find_symbol(node: Node<'_>, document: &SourceDocument) -> Option<String> {
    if node.kind() == "decorated_definition" {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if document.language.is_structure(child.kind()) {
                return find_symbol(child, document);
            }
        }
    }
    for field in ["name", "type", "declarator"] {
        if let Some(candidate) = node.child_by_field_name(field)
            && let Some(symbol) = identifier_text(candidate, document)
        {
            return Some(symbol);
        }
    }
    identifier_text(node, document)
}

fn identifier_text(node: Node<'_>, document: &SourceDocument) -> Option<String> {
    if document.language.identifier_kinds().contains(&node.kind()) {
        return document
            .source
            .get(node.byte_range())
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(ToOwned::to_owned);
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(symbol) = identifier_text(child, document) {
            return Some(symbol);
        }
    }
    None
}

fn trim_span(source: &str, start: usize, end: usize) -> Option<(usize, usize)> {
    let text = source.get(start..end)?;
    let leading = text.len() - text.trim_start().len();
    let trailing = text.len() - text.trim_end().len();
    let trimmed_start = start + leading;
    let trimmed_end = end.saturating_sub(trailing);
    (trimmed_start < trimmed_end).then_some((trimmed_start, trimmed_end))
}

fn choose_part_end(source: &str, start: usize, end: usize, max_bytes: usize) -> usize {
    if end - start <= max_bytes {
        return end;
    }
    let mut boundary = start + max_bytes;
    while boundary > start && !source.is_char_boundary(boundary) {
        boundary -= 1;
    }
    if boundary == start {
        boundary = source[start..]
            .char_indices()
            .nth(1)
            .map_or(end, |(offset, _)| start + offset);
    }
    let minimum_break = start + max_bytes / 2;
    if let Some(relative_newline) = source[start..boundary].rfind('\n') {
        let newline_end = start + relative_newline + 1;
        if newline_end >= minimum_break {
            return newline_end;
        }
    }
    boundary
}

fn line_at(source: &str, byte: usize) -> usize {
    source
        .as_bytes()
        .get(..byte)
        .unwrap_or_default()
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        + 1
}

#[cfg(test)]
#[path = "chunker_tests.rs"]
mod tests;
