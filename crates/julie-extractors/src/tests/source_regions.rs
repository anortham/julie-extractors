use std::path::Path;

use crate::base::SourceRegionKind;

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical extraction should succeed")
}

#[test]
fn rust_source_regions_capture_comments_doc_comments_and_string_literals() {
    let source = r#"// regular module comment
/// Explains greet.
pub fn greet() {
    let name = "Murphy";
    println!("{}", name);
}
"#;

    let results = extract("src/lib.rs", source);
    let greet = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "greet")
        .expect("expected greet symbol");

    let comment = results
        .source_regions
        .iter()
        .find(|region| region.kind == SourceRegionKind::Comment)
        .expect("expected regular comment source region");
    assert_eq!(comment.start_line, 1);
    assert_eq!(comment.start_byte, 0);

    let doc_comment = results
        .source_regions
        .iter()
        .find(|region| region.kind == SourceRegionKind::DocComment)
        .expect("expected doc comment source region");
    assert_eq!(
        doc_comment.containing_symbol_id.as_deref(),
        Some(greet.id.as_str())
    );

    let string_literal = results
        .source_regions
        .iter()
        .find(|region| region.kind == SourceRegionKind::StringLiteral)
        .expect("expected string literal source region");
    assert_eq!(
        string_literal.containing_symbol_id.as_deref(),
        Some(greet.id.as_str())
    );
    assert!(string_literal.end_byte > string_literal.start_byte);
}

#[test]
fn vue_source_regions_capture_embedded_script_and_style_blocks() {
    let source = r#"<template>
  <button>{{ count }}</button>
</template>
<script>
export default {
  data() {
    return { count: 0 }
  }
}
</script>
<style>
button { color: red; }
</style>
"#;

    let results = extract("src/App.vue", source);
    let embedded = results
        .source_regions
        .iter()
        .filter(|region| region.kind == SourceRegionKind::Embedded)
        .collect::<Vec<_>>();

    assert!(
        embedded.len() >= 2,
        "expected script and style embedded regions, got {embedded:?}"
    );
    assert!(embedded.iter().any(|region| {
        region
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("embedded_language"))
            .and_then(|value| value.as_str())
            == Some("javascript")
    }));
    assert!(embedded.iter().any(|region| {
        region
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("embedded_language"))
            .and_then(|value| value.as_str())
            == Some("css")
    }));
}
