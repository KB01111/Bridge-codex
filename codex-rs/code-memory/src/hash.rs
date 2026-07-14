use crate::Language;

pub(crate) fn stable_chunk_id(
    path: &str,
    language: Language,
    kind: &str,
    symbol: Option<&str>,
    start: usize,
    end: usize,
    source: &str,
) -> String {
    let start = start.to_string();
    let end = end.to_string();
    stable_id(&[
        path,
        language.tag(),
        kind,
        symbol.unwrap_or(""),
        &start,
        &end,
        source,
    ])
}

fn stable_id(parts: &[&str]) -> String {
    const OFFSET: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;

    let mut hash = OFFSET;
    for part in parts {
        for byte in part.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(PRIME);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(PRIME);
    }
    format!("cm-{hash:016x}")
}
