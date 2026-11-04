use std::path::Path;

use crate::base::{ComplexityMetric, Symbol, SymbolKind};
use crate::{ExtractionLevel, extract_canonical, extract_canonical_at};

const SOURCE: &str = include_str!("../../../../../fixtures/extraction/xml/complexity/source.xslt");

fn extract(source: &str, file_path: &str) -> crate::base::ExtractionResults {
    extract_canonical(file_path, source, Path::new("/repo")).expect("XML extraction")
}

fn extract_at(
    source: &str,
    file_path: &str,
    level: ExtractionLevel,
) -> crate::base::ExtractionResults {
    extract_canonical_at(file_path, source, Path::new("/repo"), level).expect("XML extraction")
}

fn function<'a>(symbols: &'a [Symbol], name: &str) -> &'a Symbol {
    symbols
        .iter()
        .find(|symbol| symbol.name == name && symbol.kind == SymbolKind::Function)
        .unwrap_or_else(|| panic!("missing XML function {name}"))
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

#[test]
fn xslt_instructions_emit_namespace_scoped_file_and_callable_metrics() {
    let results = extract(SOURCE, "fixtures/extraction/xml/complexity/source.xslt");

    assert!(results.parse_diagnostics.is_empty());
    let decide = function(&results.symbols, "decide");
    let double = function(&results.symbols, "double");
    assert_eq!(
        results
            .symbols
            .iter()
            .filter(|symbol| symbol.kind == SymbolKind::Function)
            .map(|symbol| symbol.name.as_str())
            .collect::<Vec<_>>(),
        ["decide", "double"]
    );
    let shadowed = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "shadowed")
        .expect("foreign namespace lookalike remains a symbol");
    assert!(
        results
            .complexity_metrics
            .iter()
            .all(|metric| { metric.symbol_id.as_deref() != Some(shadowed.id.as_str()) })
    );

    let file = metric(&results.complexity_metrics, "file", None);
    assert_eq!(file.algorithm_id, "julie-ast-complexity-v1");
    assert_eq!(
        (file.decision_count, file.loop_count, file.max_nesting_depth),
        (4, 4, 3)
    );
    assert_eq!(file.parameter_count, None);
    assert_eq!((file.start_byte, file.end_byte), (0, SOURCE.len() as u32));
    assert_eq!(file.covered_bytes, SOURCE.len() as u32);
    assert_eq!(
        file.metadata.as_ref().unwrap()["decision_signal_kinds"],
        "if,when"
    );
    assert_eq!(
        file.metadata.as_ref().unwrap()["loop_signal_kinds"],
        "for-each,for-each-group,iterate"
    );
    assert_eq!(
        file.metadata.as_ref().unwrap()["nesting_signal_kinds"],
        "if,when,for-each,for-each-group,iterate"
    );

    let decide_metric = metric(&results.complexity_metrics, "symbol", Some(&decide.id));
    assert_eq!(decide_metric.algorithm_id, "julie-ast-complexity-v1");
    assert_eq!(
        (
            decide_metric.decision_count,
            decide_metric.loop_count,
            decide_metric.max_nesting_depth,
            decide_metric.parameter_count,
        ),
        (3, 3, 3, Some(1))
    );
    let decide_body = decide.body_span.expect("named template body");
    assert_eq!(
        (decide_metric.start_byte, decide_metric.end_byte),
        (decide_body.start_byte, decide_body.end_byte)
    );
    assert_eq!(
        &SOURCE[decide_metric.start_byte as usize..decide_metric.end_byte as usize],
        "\n    <param name=\"input\"/>\n    <if test=\"$input\">\n      <choose>\n        <when test=\"$input &gt; 1\">\n          <for-each select=\"items/item\">\n            <value-of select=\".\"/>\n          </for-each>\n        </when>\n        <when test=\"$input = 1\">\n          <value-of select=\"$input\"/>\n        </when>\n        <otherwise>none</otherwise>\n      </choose>\n    </if>\n    <for-each-group select=\"items/item\" group-by=\"@kind\">\n      <iterate select=\"current-group()\">\n        <value-of select=\".\"/>\n      </iterate>\n    </for-each-group>\n    <call-template name=\"unresolved\">\n      <with-param name=\"extra\" select=\"$input\"/>\n    </call-template>\n  "
    );

    let double_metric = metric(&results.complexity_metrics, "symbol", Some(&double.id));
    assert_eq!(
        (
            double_metric.decision_count,
            double_metric.loop_count,
            double_metric.max_nesting_depth,
            double_metric.parameter_count,
        ),
        (0, 0, 0, Some(1))
    );

    let repeated = extract(SOURCE, "fixtures/extraction/xml/complexity/source.xslt");
    let repeated_file = metric(&repeated.complexity_metrics, "file", None);
    let repeated_decide = metric(
        &repeated.complexity_metrics,
        "symbol",
        Some(&function(&repeated.symbols, "decide").id),
    );
    assert_eq!(file.id, repeated_file.id);
    assert_eq!(decide_metric.id, repeated_decide.id);
}

#[test]
fn plain_and_foreign_xml_control_lookalikes_emit_no_metrics() {
    for (file_path, source) in [
        (
            "plain.xml",
            "<root><template name=\"fake\"><if/><when/><for-each/></template></root>",
        ),
        (
            "foreign.xml",
            "<root xmlns:f=\"urn:foreign\"><f:template name=\"fake\"><f:if/><f:when/><f:for-each/></f:template></root>",
        ),
    ] {
        let results = extract(source, file_path);
        assert!(results.parse_diagnostics.is_empty());
        assert!(results.complexity_metrics.is_empty(), "{file_path}");
    }
}

#[test]
fn default_namespace_shadowing_restores_xslt_callable_ownership() {
    let source = "<stylesheet xmlns=\"http://www.w3.org/1999/XSL/Transform\"><group xmlns=\"urn:foreign\"><template name=\"false\"><if/></template></group><template name=\"real\"><if test=\"1\"/></template></stylesheet>";
    let results = extract(source, "default-shadow.xslt");

    assert!(results.parse_diagnostics.is_empty());
    let real = function(&results.symbols, "real");
    let false_template = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "false")
        .expect("foreign template lookalike remains a generic symbol");
    assert_eq!(
        results
            .complexity_metrics
            .iter()
            .find(|metric| metric.scope == "file")
            .map(|metric| metric.decision_count),
        Some(1)
    );
    assert_eq!(
        metric(&results.complexity_metrics, "symbol", Some(&real.id)).decision_count,
        1
    );
    assert!(
        results
            .complexity_metrics
            .iter()
            .all(|metric| { metric.symbol_id.as_deref() != Some(false_template.id.as_str()) })
    );
}

#[test]
fn xslt_complexity_metrics_are_only_emitted_at_full_level() {
    for level in [
        ExtractionLevel::Symbols,
        ExtractionLevel::Facts,
        ExtractionLevel::References,
    ] {
        assert!(
            extract_at(SOURCE, "source.xslt", level)
                .complexity_metrics
                .is_empty()
        );
    }
    let full = extract_at(SOURCE, "source.xslt", ExtractionLevel::Full);
    assert_eq!(full.complexity_metrics.len(), 3);
}
