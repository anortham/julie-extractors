use crate::extract_canonical;
use std::path::Path;

#[test]
fn markdown_pending_relationships_target_headings_in_other_documents() {
    let source = include_str!("../../../../../fixtures/extraction/markdown/cross_file/source.md");
    let result = extract_canonical("source.md", source, Path::new("/tmp/test"))
        .expect("canonical Markdown extraction must succeed");

    let targets: Vec<_> = result
        .structured_pending_relationships
        .iter()
        .map(|pending| {
            (
                pending.target.terminal_name.as_str(),
                pending.target.import_context.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        targets,
        vec![
            ("setup", Some("./other.md")),
            ("rate-limits", Some("docs/api.md")),
        ]
    );
    assert_eq!(result.pending_relationships.len(), targets.len());
}

#[test]
fn markdown_pending_relationships_exclude_same_file_and_nonlocal_links() {
    let source = "# Overview\n\nSee [local](#target), [web](https://example.com/guide#install), and [network](//cdn.example.com/guide#install).\n\n<!-- [comment](guide.md#install) -->\n\n```md\n[code](guide.md#install)\n```\n\n# Target\n";
    let result = extract_canonical("source.md", source, Path::new("/tmp/test"))
        .expect("canonical Markdown extraction must succeed");

    assert_eq!(result.relationships.len(), 1);
    assert!(result.structured_pending_relationships.is_empty());
    assert!(result.pending_relationships.is_empty());
}
