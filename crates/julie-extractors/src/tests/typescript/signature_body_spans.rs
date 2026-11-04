use crate::base::{ExtractionResults, Symbol, SymbolKind, TypeInfo};
use crate::pipeline::extract_canonical;
use std::path::Path;

fn extract(file_path: &str, source: &str) -> ExtractionResults {
    extract_canonical(file_path, source, Path::new("/repo")).expect("canonical extraction")
}

fn symbol<'a>(results: &'a ExtractionResults, name: &str, kind: SymbolKind) -> &'a Symbol {
    results
        .symbols
        .iter()
        .find(|symbol| symbol.name == name && symbol.kind == kind)
        .unwrap_or_else(|| panic!("missing {kind:?} symbol {name}"))
}

fn child_symbol<'a>(
    results: &'a ExtractionResults,
    parent: &Symbol,
    name: &str,
    kind: SymbolKind,
) -> &'a Symbol {
    results
        .symbols
        .iter()
        .find(|symbol| {
            symbol.name == name
                && symbol.kind == kind
                && symbol.parent_id.as_deref() == Some(parent.id.as_str())
        })
        .unwrap_or_else(|| panic!("missing {kind:?} symbol {name} under {}", parent.name))
}

fn type_info<'a>(results: &'a ExtractionResults, symbol: &Symbol) -> &'a TypeInfo {
    results
        .types
        .get(&symbol.id)
        .unwrap_or_else(|| panic!("missing type fact for {}", symbol.name))
}

fn body_text<'a>(source: &'a str, symbol: &Symbol) -> Option<&'a str> {
    let span = symbol.body_span?;
    source.get(span.start_byte as usize..span.end_byte as usize)
}

fn assert_bodiless(symbol: &Symbol) {
    assert!(
        symbol.body_span.is_none(),
        "{}: {:?}",
        symbol.name,
        symbol.body_span
    );
    assert!(
        symbol.body_hash.is_none(),
        "{}: {:?}",
        symbol.name,
        symbol.body_hash
    );
}

fn typescript_source() -> String {
    [
        "/** Reader contract. */",
        "interface Reader {",
        "  /** Reads a value for café. */",
        "  read(input: Input): Result;",
        "  get label(): string;",
        "}",
        "type ReaderAlias = {",
        "  read(input: Input): Result;",
        "};",
        "abstract class BaseReader {",
        "  /** Finds a value. */",
        "  abstract find(input: Input): Result;",
        "  abstract get title(): string;",
        "}",
        "class ReaderImpl extends BaseReader implements Reader {",
        "  find(input: Input): Result {",
        "    const nested = () => {",
        "      return input;",
        "    };",
        "    return nested();",
        "  }",
        "  get title(): string { return `λ`; }",
        "  @Get()",
        "  read(input: Input): Result { return input; }",
        "  get label(): string { return `🦀`; }",
        "  transform = (input: string) => input + `λ`;",
        "}",
    ]
    .join("\r\n")
}

fn tsx_source() -> String {
    [
        "interface PanelContract {",
        "  /** Renders a value. */",
        "  render(value: string): React.ReactNode;",
        "}",
        "abstract class BasePanel {",
        "  abstract render(value: string): React.ReactNode;",
        "}",
        "class Panel extends BasePanel implements PanelContract {",
        "  render(value: string): React.ReactNode {",
        "    const nested = () => <strong>{value} λ</strong>;",
        "    return <section>{nested()}</section>;",
        "  }",
        "  get label(): string { return `🦊`; }",
        "}",
        "export function Preview() { return <Panel />; }",
    ]
    .join("\r\n")
}

#[test]
fn typescript_interface_and_abstract_method_signatures_have_no_body() {
    let source = typescript_source();
    let results = extract("src/reader.ts", &source);
    assert!(
        results.parse_diagnostics.is_empty(),
        "{:#?}",
        results.parse_diagnostics
    );

    let reader = symbol(&results, "Reader", SymbolKind::Interface);
    let read_signature = child_symbol(&results, reader, "read", SymbolKind::Method);
    assert_bodiless(read_signature);
    assert_eq!(
        read_signature.parent_id.as_deref(),
        Some(reader.id.as_str())
    );
    assert_eq!(
        read_signature.signature.as_deref(),
        Some("read(input: Input): Result")
    );
    assert!(
        read_signature
            .doc_comment
            .as_deref()
            .is_some_and(|doc| doc.contains("Reads a value for café"))
    );
    assert_eq!(type_info(&results, read_signature).resolved_type, "Result");

    let label_signature = child_symbol(&results, reader, "label", SymbolKind::Method);
    assert_bodiless(label_signature);

    let alias = symbol(&results, "ReaderAlias", SymbolKind::Type);
    let alias_signature = child_symbol(&results, alias, "read", SymbolKind::Method);
    assert_bodiless(alias_signature);
    assert_eq!(type_info(&results, alias_signature).resolved_type, "Result");

    let base = symbol(&results, "BaseReader", SymbolKind::Class);
    let find_signature = child_symbol(&results, base, "find", SymbolKind::Method);
    assert_bodiless(find_signature);
    assert_eq!(find_signature.parent_id.as_deref(), Some(base.id.as_str()));
    assert_eq!(
        find_signature.signature.as_deref(),
        Some("find(input: Input): Result")
    );
    assert!(
        find_signature
            .doc_comment
            .as_deref()
            .is_some_and(|doc| doc.contains("Finds a value"))
    );
    assert_eq!(type_info(&results, find_signature).resolved_type, "Result");

    let title_signature = child_symbol(&results, base, "title", SymbolKind::Method);
    assert_bodiless(title_signature);
}

#[test]
fn typescript_method_implementations_keep_ast_body_spans_across_crlf_and_unicode() {
    let source = typescript_source();
    let results = extract("src/reader.ts", &source);
    assert!(
        results.parse_diagnostics.is_empty(),
        "{:#?}",
        results.parse_diagnostics
    );
    let implementation = symbol(&results, "ReaderImpl", SymbolKind::Class);
    let find_method = child_symbol(&results, implementation, "find", SymbolKind::Method);
    assert_eq!(
        body_text(&source, find_method),
        Some(
            "{\r\n    const nested = () => {\r\n      return input;\r\n    };\r\n    return nested();\r\n  }"
        )
    );
    assert!(find_method.body_hash.is_some());

    let title_getter = child_symbol(&results, implementation, "title", SymbolKind::Method);
    assert_eq!(body_text(&source, title_getter), Some("{ return `λ`; }"));

    let transform = child_symbol(&results, implementation, "transform", SymbolKind::Method);
    assert_eq!(body_text(&source, transform), Some("input + `λ`"));
    assert!(transform.body_hash.is_some());

    let read_method = child_symbol(&results, implementation, "read", SymbolKind::Method);
    assert_eq!(
        read_method
            .annotations
            .iter()
            .map(|annotation| annotation.annotation_key.as_str())
            .collect::<Vec<_>>(),
        ["get"]
    );
}

#[test]
fn tsx_interface_and_abstract_method_signatures_have_no_body() {
    let source = tsx_source();
    let results = extract("src/panel.tsx", &source);
    assert!(
        results.parse_diagnostics.is_empty(),
        "{:#?}",
        results.parse_diagnostics
    );

    let contract = symbol(&results, "PanelContract", SymbolKind::Interface);
    let render_signature = child_symbol(&results, contract, "render", SymbolKind::Method);
    assert_bodiless(render_signature);
    assert_eq!(
        render_signature.parent_id.as_deref(),
        Some(contract.id.as_str())
    );
    assert!(
        render_signature
            .doc_comment
            .as_deref()
            .is_some_and(|doc| doc.contains("Renders a value"))
    );

    let base = symbol(&results, "BasePanel", SymbolKind::Class);
    let abstract_render = child_symbol(&results, base, "render", SymbolKind::Method);
    assert_bodiless(abstract_render);
    assert_eq!(
        type_info(&results, abstract_render).resolved_type,
        "React.ReactNode"
    );
}

#[test]
fn tsx_method_implementations_keep_ast_body_spans() {
    let source = tsx_source();
    let results = extract("src/panel.tsx", &source);
    assert!(
        results.parse_diagnostics.is_empty(),
        "{:#?}",
        results.parse_diagnostics
    );
    let panel = symbol(&results, "Panel", SymbolKind::Class);
    let render = child_symbol(&results, panel, "render", SymbolKind::Method);
    assert_eq!(
        body_text(&source, render),
        Some(
            "{\r\n    const nested = () => <strong>{value} λ</strong>;\r\n    return <section>{nested()}</section>;\r\n  }"
        )
    );
    assert!(render.body_hash.is_some());

    let label = child_symbol(&results, panel, "label", SymbolKind::Method);
    assert_eq!(body_text(&source, label), Some("{ return `🦊`; }"));
}
