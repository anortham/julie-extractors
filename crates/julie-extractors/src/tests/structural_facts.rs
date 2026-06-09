use std::path::Path;

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical extraction should succeed")
}

#[test]
fn rust_unsafe_blocks_emit_structural_facts_with_containing_symbol() {
    let source = r#"pub fn read_flag(value: &i32) -> i32 {
    unsafe {
        core::ptr::read_volatile(value)
    }
}
"#;

    let results = extract("src/lib.rs", source);
    let read_flag = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "read_flag")
        .expect("expected read_flag symbol");

    let fact = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "rust.unsafe_block.v1")
        .expect("expected unsafe-block structural fact");

    assert_eq!(fact.capture_name, "unsafe_block");
    assert_eq!(fact.node_kind, "unsafe_block");
    assert_eq!(
        fact.containing_symbol_id.as_deref(),
        Some(read_flag.id.as_str())
    );
    assert_eq!(fact.confidence, 1.0);
    assert_eq!(
        fact.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("query_family"))
            .and_then(|value| value.as_str()),
        Some("safety")
    );
    assert!(fact.end_byte > fact.start_byte);
}
