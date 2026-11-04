use crate::base::SourceRegionKind;
use crate::extract_canonical;
use std::path::Path;

#[test]
fn qmldir_source_regions_mark_only_comment_lines_at_crlf_byte_offsets() {
    let source = "# Café widgets\r\nmodule org.example.app\r\nButton 1.0 Button.qml\r\n";
    let result =
        extract_canonical("qmldir", source, Path::new("/repo")).expect("qmldir extraction");

    assert!(result.parse_diagnostics.is_empty());
    assert_eq!(result.source_regions.len(), 1);
    let comment = &result.source_regions[0];
    assert_eq!(comment.kind, SourceRegionKind::Comment);
    assert_eq!(comment.start_byte, 0);
    assert_eq!(comment.end_byte as usize, source.find('\n').unwrap());
    assert_eq!(
        &source[comment.start_byte as usize..comment.end_byte as usize],
        "# Café widgets\r"
    );
    assert_eq!(comment.start_line, 1);
    assert_eq!(comment.containing_symbol_id, None);

    assert!(result.symbols.iter().any(|symbol| symbol.name == "Button"));
    assert!(
        !result
            .source_regions
            .iter()
            .any(|region| region.kind == SourceRegionKind::StringLiteral)
    );
}
