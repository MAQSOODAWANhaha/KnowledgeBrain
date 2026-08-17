//! Structural check: the api crate source does not invoke parse/chunk/vector.

#[test]
fn api_sources_have_no_pipeline_calls() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut joined = String::new();
    for name in ["lib.rs", "main.rs"] {
        joined.push_str(&std::fs::read_to_string(root.join(name)).unwrap());
    }
    let lower = joined.to_ascii_lowercase();
    for needle in ["docreader", "chunker", "embedding", "readstream", "oxana"] {
        assert!(!lower.contains(needle), "api src must not mention {needle}");
    }
}
