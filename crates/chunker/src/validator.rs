//! Permissive tier validator (brain `validator.go`).

use crate::TextChunk;

pub fn validate_chunks(chunks: &[TextChunk], total_chars: usize, chunk_size: usize) -> bool {
    if chunks.is_empty() {
        return false;
    }
    if chunks.len() == 1 && total_chars > 2 * chunk_size {
        return false;
    }
    let mut max_len = 0usize;
    let mut tiny = 0usize;
    for (i, c) in chunks.iter().enumerate() {
        let l = c.content.chars().count();
        max_len = max_len.max(l);
        if i + 1 != chunks.len() && l < 50 {
            tiny += 1;
        }
    }
    if tiny > chunks.len() / 4 && tiny > 2 {
        return false;
    }
    if max_len < chunk_size / 4 && total_chars > chunk_size {
        return false;
    }
    if chunk_size > 0 && max_len > 2 * chunk_size {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(s: &str) -> TextChunk {
        TextChunk {
            content: s.to_string(),
            ..TextChunk::default()
        }
    }

    #[test]
    fn rejects_single_chunk_on_large_doc() {
        assert!(!validate_chunks(&[ch(&"a".repeat(2000))], 2000, 512));
    }

    #[test]
    fn rejects_empty() {
        assert!(!validate_chunks(&[], 1000, 500));
    }

    #[test]
    fn accepts_reasonable() {
        let chunks = vec![
            ch(&"a".repeat(480)),
            ch(&"b".repeat(510)),
            ch(&"c".repeat(460)),
        ];
        assert!(validate_chunks(&chunks, 1500, 512));
    }

    #[test]
    fn rejects_oversized() {
        let chunks = vec![ch(&"a".repeat(100)), ch(&"b".repeat(5000))];
        assert!(!validate_chunks(&chunks, 5100, 1000));
    }
}
