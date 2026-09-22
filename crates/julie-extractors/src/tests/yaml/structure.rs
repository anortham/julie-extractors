use crate::base::{ExtractionResults, Symbol, SymbolKind};
use crate::extract_canonical;
use std::path::Path;

fn extract(file: &str, source: &str) -> ExtractionResults {
    extract_canonical(file, source, Path::new("/tmp/test")).unwrap()
}

fn find<'a>(results: &'a ExtractionResults, name: &str, line: u32) -> &'a Symbol {
    results
        .symbols
        .iter()
        .find(|s| s.name == name && s.start_line == line)
        .unwrap_or_else(|| panic!("missing {name} on line {line}: {:#?}", results.symbols))
}

fn body<'a>(source: &'a str, symbol: &Symbol) -> Option<&'a str> {
    let span = symbol.body_span?;
    source.get(span.start_byte as usize..span.end_byte as usize)
}

#[test]
fn sequence_of_mappings_gets_indexed_item_containers() {
    let source = "steps:\n  - name: Checkout\n    uses: actions/checkout@v5\n  - name: Format\n    run: cargo fmt --check\n";
    let results = extract("ci.yaml", source);

    let steps = find(&results, "steps", 1);
    let first = find(&results, "[0]", 2);
    let second = find(&results, "[1]", 4);
    assert_eq!(steps.kind, SymbolKind::Module);
    assert_eq!(first.kind, SymbolKind::Module);
    assert_eq!(first.parent_id.as_ref(), Some(&steps.id));
    assert_eq!(second.parent_id.as_ref(), Some(&steps.id));
    assert_eq!(
        find(&results, "uses", 3).parent_id.as_ref(),
        Some(&first.id)
    );
    assert_eq!(
        find(&results, "run", 5).parent_id.as_ref(),
        Some(&second.id)
    );
}

#[test]
fn body_spans_are_container_values_not_text_heuristics() {
    let source = "jobs:\n  build:\n    runs-on: ${{ matrix.os }}\n    steps:\n      - name: Setup node (LTS)\n        with:\n          node-version: ${{ env.NODE }}\n";
    let results = extract("ci.yaml", source);

    assert_eq!(
        body(source, find(&results, "build", 2)),
        Some(
            "runs-on: ${{ matrix.os }}\n    steps:\n      - name: Setup node (LTS)\n        with:\n          node-version: ${{ env.NODE }}"
        )
    );
    assert_eq!(
        body(source, find(&results, "with", 6)),
        Some("node-version: ${{ env.NODE }}")
    );
    for (name, line) in [("runs-on", 3), ("name", 5), ("node-version", 7)] {
        let symbol = find(&results, name, line);
        assert!(
            symbol.body_span.is_none() && symbol.body_hash.is_none(),
            "{name} is a scalar and has no body"
        );
    }
}

#[test]
fn comment_blocks_directly_above_keys_are_doc_comments() {
    let source = "# Number of replicas\nreplicaCount: 2\n\n# Container image settings\nimage:\n  # Image repository\n  repository: nginx\n  # Image pull policy.\n  # One of Always, IfNotPresent, Never.\n  pullPolicy: IfNotPresent\nservice:\n  ports:\n    - 80\n  # Port exposed by the service\n  port: 80\n";
    let results = extract("values.yaml", source);

    let doc = |name: &str, line: u32| find(&results, name, line).doc_comment.clone();
    assert_eq!(
        doc("replicaCount", 2).as_deref(),
        Some("# Number of replicas")
    );
    assert_eq!(
        doc("image", 5).as_deref(),
        Some("# Container image settings")
    );
    assert_eq!(doc("repository", 7).as_deref(), Some("# Image repository"));
    assert_eq!(
        doc("pullPolicy", 10).as_deref(),
        Some("# Image pull policy.\n# One of Always, IfNotPresent, Never.")
    );
    assert_eq!(
        doc("port", 15).as_deref(),
        Some("# Port exposed by the service")
    );
    assert_eq!(doc("service", 11), None);

    let doc_regions: Vec<u32> = results
        .source_regions
        .iter()
        .filter(|region| region.kind == crate::base::SourceRegionKind::DocComment)
        .map(|region| region.start_line)
        .collect();
    assert_eq!(doc_regions, vec![1, 4, 6, 8, 9, 14]);
}

#[test]
fn plain_url_values_are_literals() {
    let results = extract("config.yaml", "api:\n  url: https://api.example.com/v1\n");

    assert!(results.literals.iter().any(|literal| {
        literal.literal_text == "https://api.example.com/v1"
            && literal.carrier.as_deref() == Some("api.url")
    }));
}
