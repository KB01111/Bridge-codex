use std::collections::BTreeMap;
use std::collections::BTreeSet;

use crate::GraphExplanation;
use crate::GraphRelation;
use crate::StructuralChunk;
use crate::bm25::tokenize;

const MAX_EDGES_PER_CHUNK: usize = 64;
const MAX_EXPLANATIONS_PER_RESULT: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Edge {
    target: usize,
    relation: GraphRelation,
}

pub(crate) struct CodeGraph {
    adjacency: Vec<Vec<Edge>>,
    symbol_count: usize,
    edge_count: usize,
}

impl CodeGraph {
    pub(crate) fn build(chunks: &[StructuralChunk]) -> Self {
        let mut symbols: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        let mut paths: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (index, chunk) in chunks.iter().enumerate() {
            paths
                .entry(chunk.path.to_ascii_lowercase())
                .or_default()
                .push(index);
            if let Some(symbol) = &chunk.symbol {
                symbols
                    .entry(symbol.to_ascii_lowercase())
                    .or_default()
                    .push(index);
            }
        }

        let aliases = path_aliases(&paths);
        let mut edge_sets = vec![BTreeSet::new(); chunks.len()];
        add_same_file_edges(&paths, &mut edge_sets);
        add_reference_edges(chunks, &symbols, &mut edge_sets);
        add_import_edges(chunks, &aliases, &mut edge_sets);

        let mut adjacency = Vec::with_capacity(edge_sets.len());
        for edges in edge_sets {
            adjacency.push(edges.into_iter().collect());
        }
        let directed_edges: usize = adjacency.iter().map(Vec::len).sum();
        Self {
            adjacency,
            symbol_count: symbols.len(),
            edge_count: directed_edges / 2,
        }
    }

    pub(crate) fn symbol_count(&self) -> usize {
        self.symbol_count
    }

    pub(crate) fn edge_count(&self) -> usize {
        self.edge_count
    }

    pub(crate) fn boosts(
        &self,
        chunks: &[StructuralChunk],
        lexical_scores: &[f32],
        graph_weight: f32,
    ) -> (Vec<f32>, Vec<Vec<GraphExplanation>>) {
        let mut boosts = vec![0.0; chunks.len()];
        let mut explanations = vec![Vec::new(); chunks.len()];
        let mut seeds: Vec<usize> = lexical_scores
            .iter()
            .enumerate()
            .filter_map(|(index, score)| (*score > 0.0).then_some(index))
            .collect();
        seeds.sort_by(|left, right| {
            lexical_scores[*right]
                .total_cmp(&lexical_scores[*left])
                .then_with(|| chunks[*left].id.cmp(&chunks[*right].id))
        });

        for seed in seeds.into_iter().take(8) {
            for edge in &self.adjacency[seed] {
                let contribution =
                    lexical_scores[seed] * graph_weight * relation_weight(edge.relation);
                if contribution <= 0.0 {
                    continue;
                }
                boosts[edge.target] += contribution;
                explanations[edge.target].push(GraphExplanation {
                    from_chunk_id: chunks[seed].id.clone(),
                    relation: edge.relation,
                    contribution,
                });
            }
        }
        for items in &mut explanations {
            items.sort_by(|left, right| {
                right
                    .contribution
                    .total_cmp(&left.contribution)
                    .then_with(|| left.from_chunk_id.cmp(&right.from_chunk_id))
                    .then_with(|| left.relation.cmp(&right.relation))
            });
            items.truncate(MAX_EXPLANATIONS_PER_RESULT);
        }
        (boosts, explanations)
    }
}

fn add_same_file_edges(paths: &BTreeMap<String, Vec<usize>>, edges: &mut [BTreeSet<Edge>]) {
    for indexes in paths.values() {
        for pair in indexes.windows(2) {
            add_bidirectional(edges, pair[0], pair[1], GraphRelation::SameFile);
        }
    }
}

fn add_reference_edges(
    chunks: &[StructuralChunk],
    symbols: &BTreeMap<String, Vec<usize>>,
    edges: &mut [BTreeSet<Edge>],
) {
    for (source_index, chunk) in chunks.iter().enumerate() {
        let tokens: BTreeSet<String> = tokenize(&chunk.source).into_iter().collect();
        for token in tokens {
            let Some(targets) = symbols.get(&token) else {
                continue;
            };
            for target in targets {
                if source_index != *target {
                    add_bidirectional(edges, source_index, *target, GraphRelation::Reference);
                }
            }
        }
    }
}

fn add_import_edges(
    chunks: &[StructuralChunk],
    aliases: &BTreeMap<String, Vec<usize>>,
    edges: &mut [BTreeSet<Edge>],
) {
    for (source_index, chunk) in chunks.iter().enumerate() {
        for import in imports_from_source(&chunk.source) {
            for alias in import_aliases(&import) {
                let Some(targets) = aliases.get(&alias) else {
                    continue;
                };
                for target in targets {
                    if source_index != *target {
                        add_bidirectional(edges, source_index, *target, GraphRelation::Import);
                    }
                }
            }
        }
    }
}

fn add_bidirectional(
    edges: &mut [BTreeSet<Edge>],
    left: usize,
    right: usize,
    relation: GraphRelation,
) {
    if edges[left].len() >= MAX_EDGES_PER_CHUNK || edges[right].len() >= MAX_EDGES_PER_CHUNK {
        return;
    }
    edges[left].insert(Edge {
        target: right,
        relation,
    });
    edges[right].insert(Edge {
        target: left,
        relation,
    });
}

fn path_aliases(paths: &BTreeMap<String, Vec<usize>>) -> BTreeMap<String, Vec<usize>> {
    let mut aliases: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (path, indexes) in paths {
        for alias in import_aliases(path) {
            aliases.entry(alias).or_default().extend(indexes);
        }
    }
    for indexes in aliases.values_mut() {
        indexes.sort_unstable();
        indexes.dedup();
    }
    aliases
}

fn import_aliases(value: &str) -> Vec<String> {
    let mut normalized = value
        .trim_matches(|character: char| matches!(character, '\'' | '"' | '<' | '>' | ';'))
        .trim_start_matches("./")
        .trim_start_matches("crate::")
        .replace("::", "/")
        .to_ascii_lowercase();
    while normalized.starts_with("../") {
        normalized = normalized[3..].to_string();
    }
    let known_extensions = [
        ".c", ".h", ".cc", ".cpp", ".hpp", ".go", ".js", ".jsx", ".ts", ".tsx", ".py", ".rs",
        ".swift",
    ];
    let without_extension = known_extensions
        .iter()
        .find_map(|extension| normalized.strip_suffix(extension))
        .unwrap_or(&normalized)
        .to_string();
    let stem = without_extension
        .rsplit('/')
        .next()
        .unwrap_or(&without_extension)
        .rsplit('.')
        .next()
        .unwrap_or(&without_extension)
        .to_string();
    let dotted_module = without_extension.replace('.', "/");
    let path_parts = dotted_module
        .split('/')
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let mut aliases = vec![normalized, without_extension, dotted_module, stem];
    aliases.extend(path_parts);
    aliases.retain(|alias| !alias.is_empty());
    aliases.sort();
    aliases.dedup();
    aliases
}

fn imports_from_source(source: &str) -> Vec<String> {
    let mut imports = Vec::new();
    for line in source.lines().map(str::trim) {
        if line.starts_with("#include") {
            if let Some(value) = quoted_value(line) {
                imports.push(value);
            }
        } else if let Some(rest) = line.strip_prefix("use ") {
            imports.push(rest.trim_end_matches(';').to_string());
        } else if let Some(rest) = line.strip_prefix("from ") {
            if let Some((module, _)) = rest.split_once(" import ") {
                imports.push(module.to_string());
            }
        } else if let Some(rest) = line.strip_prefix("import ") {
            if let Some((_, module)) = rest.rsplit_once(" from ") {
                imports.push(module.to_string());
            } else if let Some(value) = quoted_value(rest) {
                imports.push(value);
            } else {
                imports.push(rest.trim_end_matches(';').to_string());
            }
        }
    }
    imports
}

fn quoted_value(text: &str) -> Option<String> {
    for quote in ['"', '\''] {
        if let Some(start) = text.find(quote).map(|index| index + 1)
            && let Some(length) = text[start..].find(quote)
        {
            return Some(text[start..start + length].to_string());
        }
    }
    None
}

fn relation_weight(relation: GraphRelation) -> f32 {
    match relation {
        GraphRelation::Import => 1.0,
        GraphRelation::Reference => 0.8,
        GraphRelation::SameFile => 0.35,
    }
}
