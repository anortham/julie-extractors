use std::path::Path;

use crate::base::{ComplexityMetric, ExtractionResults, Symbol, SymbolKind};
use crate::{ExtractionLevel, extract_canonical, extract_canonical_at};

fn extract(source: &str) -> ExtractionResults {
    extract_canonical("pages/metrics.html", source, Path::new("/repo")).expect("HTML extraction")
}

fn extract_at(source: &str, level: ExtractionLevel) -> ExtractionResults {
    extract_canonical_at("pages/metrics.html", source, Path::new("/repo"), level)
        .expect("HTML extraction")
}

fn metric<'a>(
    metrics: &'a [ComplexityMetric],
    scope: &str,
    symbol_id: Option<&str>,
) -> &'a ComplexityMetric {
    metrics
        .iter()
        .find(|metric| metric.scope == scope && metric.symbol_id.as_deref() == symbol_id)
        .unwrap_or_else(|| panic!("missing {scope} complexity metric for {symbol_id:?}"))
}

fn symbol<'a>(symbols: &'a [Symbol], name: &str, kind: SymbolKind) -> &'a Symbol {
    symbols
        .iter()
        .find(|symbol| symbol.name == name && symbol.kind == kind)
        .unwrap_or_else(|| panic!("missing {kind:?} symbol {name}"))
}

#[test]
fn plain_markup_has_no_code_complexity_metrics() {
    let source = "<main><h1>Summary</h1><p>Text only</p></main>\n";
    let results = extract(source);

    assert!(results.parse_diagnostics.is_empty());
    assert!(results.complexity_metrics.is_empty());
}

#[test]
fn embedded_script_metrics_keep_host_spans_languages_and_owners() {
    let cases = [
        (
            "<script>function choose(value) { if (value) { return value; } return null; }</script>\n",
            "choose",
            SymbolKind::Function,
            "javascript",
        ),
        (
            "<script type=\"text/babel\">@tracked class View { select(values) { for (const value of values) { if (value.ok) return value; } } }</script>\n",
            "select",
            SymbolKind::Method,
            "jsx",
        ),
    ];

    for (source, name, kind, language) in cases {
        let results = extract_at(source, ExtractionLevel::Full);
        let callable = symbol(&results.symbols, name, kind);
        let file_metric = metric(&results.complexity_metrics, "file", None);
        let symbol_metric = metric(&results.complexity_metrics, "symbol", Some(&callable.id));
        let body = callable.body_span.expect("embedded callable body");

        assert!(results.parse_diagnostics.is_empty());
        assert_eq!(file_metric.language, "html");
        let content_start = source.find('>').unwrap() + 1;
        let content_end = source.find("</script>").unwrap();
        assert_eq!(
            (
                file_metric.start_byte as usize,
                file_metric.end_byte as usize
            ),
            (content_start, content_end)
        );
        assert!(source[content_start..content_end].contains(name));
        assert_eq!(symbol_metric.language, language);
        assert_eq!(
            (symbol_metric.start_byte, symbol_metric.end_byte),
            (body.start_byte, body.end_byte)
        );
        assert_eq!(symbol_metric.parameter_count, Some(1));
        assert!(symbol_metric.decision_count > 0);
    }

    let source =
        "<script>function choose(value) { if (value) return value; return null; }</script>\n";
    let references = extract_at(source, ExtractionLevel::References);
    assert!(references.complexity_metrics.is_empty());
}
