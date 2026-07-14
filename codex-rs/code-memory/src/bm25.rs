use std::collections::BTreeMap;
use std::collections::BTreeSet;

use crate::StructuralChunk;

const K1: f32 = 1.2;
const B: f32 = 0.75;

pub(crate) struct Bm25Index {
    term_frequencies: Vec<BTreeMap<String, usize>>,
    document_frequencies: BTreeMap<String, usize>,
    document_lengths: Vec<usize>,
    average_document_length: f32,
}

impl Bm25Index {
    pub(crate) fn new(chunks: &[StructuralChunk]) -> Self {
        let mut term_frequencies = Vec::with_capacity(chunks.len());
        let mut document_frequencies = BTreeMap::new();
        let mut document_lengths = Vec::with_capacity(chunks.len());

        for chunk in chunks {
            let tokens = chunk_tokens(chunk);
            document_lengths.push(tokens.len());
            let mut frequencies = BTreeMap::new();
            for token in tokens {
                *frequencies.entry(token).or_insert(0) += 1;
            }
            for token in frequencies.keys() {
                *document_frequencies.entry(token.clone()).or_insert(0) += 1;
            }
            term_frequencies.push(frequencies);
        }

        let total_length = document_lengths
            .iter()
            .fold(0_usize, |total, length| total.saturating_add(*length));
        let average_document_length = if document_lengths.is_empty() {
            1.0
        } else {
            total_length as f32 / document_lengths.len() as f32
        };

        Self {
            term_frequencies,
            document_frequencies,
            document_lengths,
            average_document_length,
        }
    }

    pub(crate) fn scores(&self, query: &str) -> Vec<f32> {
        let query_tokens: BTreeSet<String> = tokenize(query).into_iter().collect();
        let document_count = self.term_frequencies.len() as f32;
        self.term_frequencies
            .iter()
            .enumerate()
            .map(|(index, frequencies)| {
                query_tokens
                    .iter()
                    .map(|token| {
                        let term_frequency = *frequencies.get(token).unwrap_or(&0) as f32;
                        if term_frequency == 0.0 {
                            return 0.0;
                        }
                        let document_frequency =
                            *self.document_frequencies.get(token).unwrap_or(&0) as f32;
                        let inverse_document_frequency = ((document_count - document_frequency
                            + 0.5)
                            / (document_frequency + 0.5)
                            + 1.0)
                            .ln();
                        let document_length = self.document_lengths[index] as f32;
                        let normalization =
                            K1 * (1.0 - B + B * document_length / self.average_document_length);
                        inverse_document_frequency * (term_frequency * (K1 + 1.0))
                            / (term_frequency + normalization)
                    })
                    .sum()
            })
            .collect()
    }
}

pub(crate) fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for character in text.chars() {
        if character.is_alphanumeric() || character == '_' {
            current.push(character);
        } else {
            push_identifier_tokens(&mut tokens, &current);
            current.clear();
        }
    }
    push_identifier_tokens(&mut tokens, &current);
    tokens
}

fn chunk_tokens(chunk: &StructuralChunk) -> Vec<String> {
    let mut tokens = tokenize(&chunk.source);
    for metadata in [
        Some(chunk.path.as_str()),
        Some(chunk.kind.as_str()),
        chunk.symbol.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        let metadata_tokens = tokenize(metadata);
        for _ in 0..3 {
            tokens.extend(metadata_tokens.iter().cloned());
        }
    }
    tokens
}

fn push_identifier_tokens(tokens: &mut Vec<String>, identifier: &str) {
    if identifier.is_empty() {
        return;
    }
    let normalized = identifier.to_lowercase();
    tokens.push(normalized.clone());

    for snake_part in normalized.split('_').filter(|part| !part.is_empty()) {
        if snake_part != normalized {
            tokens.push(snake_part.to_string());
        }
    }

    let mut start = 0;
    let mut found_boundary = false;
    let characters: Vec<char> = identifier.chars().collect();
    for index in 1..characters.len() {
        let starts_word = characters[index].is_uppercase()
            && (!characters[index - 1].is_uppercase()
                || characters
                    .get(index + 1)
                    .is_some_and(|next| next.is_lowercase()));
        if starts_word {
            let part: String = characters[start..index].iter().collect();
            tokens.push(part.to_lowercase());
            start = index;
            found_boundary = true;
        }
    }
    if found_boundary {
        let part: String = characters[start..].iter().collect();
        tokens.push(part.to_lowercase());
    }
}

#[cfg(test)]
#[path = "bm25_tests.rs"]
mod tests;
