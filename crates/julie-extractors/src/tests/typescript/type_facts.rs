#[cfg(test)]
mod typescript_type_fact_tests {
    use crate::base::{Symbol, SymbolKind, TypeInfo};
    use crate::typescript::TypeScriptExtractor;
    use std::path::PathBuf;
    use tree_sitter::Parser;

    fn extract(code: &str) -> (Vec<Symbol>, TypeScriptExtractor) {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
            .expect("Error loading TypeScript grammar");
        let tree = parser.parse(code, None).expect("Error parsing code");
        let mut extractor = TypeScriptExtractor::new(
            "typescript".to_string(),
            "test.ts".to_string(),
            code.to_string(),
            &PathBuf::from("/tmp/test"),
        );
        let symbols = extractor.extract_symbols(&tree);
        (symbols, extractor)
    }

    fn symbol<'a>(symbols: &'a [Symbol], name: &str) -> &'a Symbol {
        symbols
            .iter()
            .find(|s| s.name == name)
            .unwrap_or_else(|| panic!("missing symbol `{name}`"))
    }

    fn fact<'a>(extractor: &'a TypeScriptExtractor, symbol: &Symbol) -> &'a TypeInfo {
        extractor
            .base
            .type_info
            .get(&symbol.id)
            .unwrap_or_else(|| panic!("missing type fact for `{}`", symbol.name))
    }

    #[test]
    fn annotated_local_records_declared_type_fact() {
        let (symbols, extractor) = extract("const graph: GraphTraversal = build();");

        let graph = symbol(&symbols, "graph");
        let fact = fact(&extractor, graph);
        assert_eq!(fact.resolved_type, "GraphTraversal");
        assert!(!fact.is_inferred);
        assert_eq!(fact.metadata, None);
    }

    #[test]
    fn generic_annotation_records_base_name_with_declared_metadata() {
        let (symbols, extractor) = extract("const lookup: Map<string, GraphNode> = new Map();");

        let lookup = symbol(&symbols, "lookup");
        let fact = fact(&extractor, lookup);
        assert_eq!(fact.resolved_type, "Map");
        assert!(!fact.is_inferred);
        assert_eq!(
            fact.metadata.as_ref().and_then(|m| m.get("declared")),
            Some(&serde_json::json!("Map<string, GraphNode>"))
        );
    }

    #[test]
    fn new_expression_local_records_inferred_fact() {
        let (symbols, extractor) = extract("const graph = new GraphTraversal();");

        let graph = symbol(&symbols, "graph");
        let fact = fact(&extractor, graph);
        assert_eq!(fact.resolved_type, "GraphTraversal");
        assert!(fact.is_inferred);
    }

    #[test]
    fn annotation_beats_new_expression_inference() {
        let (symbols, extractor) = extract("const graph: BaseGraph = new GraphTraversal();");

        let graph = symbol(&symbols, "graph");
        let fact = fact(&extractor, graph);
        assert_eq!(fact.resolved_type, "BaseGraph");
        assert!(!fact.is_inferred);
    }

    #[test]
    fn namespaced_new_expression_records_nothing() {
        let (symbols, extractor) = extract("const graph = new ns.GraphTraversal();");

        let graph = symbol(&symbols, "graph");
        assert!(extractor.base.type_info.get(&graph.id).is_none());
    }

    #[test]
    fn union_intersection_object_and_literal_annotations_record_no_fact() {
        let (symbols, extractor) = extract(
            r#"
const a: Foo | Bar = x;
const b: Foo & Bar = y;
const c: { id: number } = z;
const d: "literal" = w;
"#,
        );

        for name in ["a", "b", "c", "d"] {
            let local = symbol(&symbols, name);
            assert!(
                extractor.base.type_info.get(&local.id).is_none(),
                "`{name}` must not carry a type fact"
            );
        }
    }

    #[test]
    fn method_parameters_become_symbols_with_facts() {
        let (symbols, extractor) = extract(
            r#"
class Worker {
    process(input: GraphNode, count: number): void {}
}
"#,
        );

        let process = symbol(&symbols, "process");
        let input = symbol(&symbols, "input");
        assert_eq!(input.kind, SymbolKind::Variable);
        assert_eq!(input.parent_id.as_deref(), Some(process.id.as_str()));
        assert_eq!(
            input.metadata.as_ref().and_then(|m| m.get("role")),
            Some(&serde_json::json!("parameter"))
        );
        assert_eq!(input.signature.as_deref(), Some("input: GraphNode"));
        assert_eq!(fact(&extractor, input).resolved_type, "GraphNode");
        assert!(!fact(&extractor, input).is_inferred);

        let count = symbol(&symbols, "count");
        assert_eq!(count.parent_id.as_deref(), Some(process.id.as_str()));
        assert_eq!(fact(&extractor, count).resolved_type, "number");
    }

    #[test]
    fn constructor_parameter_property_becomes_symbol_with_fact() {
        let (symbols, extractor) = extract(
            r#"
class Worker {
    constructor(private graph: GraphTraversal) {}
}
"#,
        );

        let constructor = symbol(&symbols, "constructor");
        let graph = symbol(&symbols, "graph");
        assert_eq!(graph.kind, SymbolKind::Variable);
        assert_eq!(graph.parent_id.as_deref(), Some(constructor.id.as_str()));
        assert_eq!(fact(&extractor, graph).resolved_type, "GraphTraversal");
    }

    #[test]
    fn union_parameter_gets_symbol_without_fact() {
        let (symbols, extractor) = extract("function run(mode: Mode | null): void {}");

        let run = symbol(&symbols, "run");
        let mode = symbol(&symbols, "mode");
        assert_eq!(mode.kind, SymbolKind::Variable);
        assert_eq!(mode.parent_id.as_deref(), Some(run.id.as_str()));
        assert!(extractor.base.type_info.get(&mode.id).is_none());
    }

    #[test]
    fn unannotated_parameter_gets_symbol_without_fact() {
        let (symbols, extractor) = extract("function run(mode) {}");

        let run = symbol(&symbols, "run");
        let mode = symbol(&symbols, "mode");
        assert_eq!(mode.kind, SymbolKind::Variable);
        assert_eq!(mode.parent_id.as_deref(), Some(run.id.as_str()));
        assert!(extractor.base.type_info.get(&mode.id).is_none());
    }

    #[test]
    fn arrow_function_parameter_becomes_symbol_with_fact() {
        let (symbols, extractor) = extract("const handler = (event: GraphEvent): void => {};");

        let handler = symbol(&symbols, "handler");
        assert_eq!(handler.kind, SymbolKind::Function);
        let event = symbol(&symbols, "event");
        assert_eq!(event.parent_id.as_deref(), Some(handler.id.as_str()));
        assert_eq!(fact(&extractor, event).resolved_type, "GraphEvent");
    }

    #[test]
    fn annotated_fields_record_declared_type_facts() {
        let (symbols, extractor) = extract(
            r#"
class Worker {
    graph: GraphTraversal;
    maybe?: GraphNode;
    inline: { id: number };
}
"#,
        );

        let graph = symbol(&symbols, "graph");
        assert_eq!(fact(&extractor, graph).resolved_type, "GraphTraversal");
        assert!(!fact(&extractor, graph).is_inferred);

        let maybe = symbol(&symbols, "maybe");
        assert_eq!(fact(&extractor, maybe).resolved_type, "GraphNode");

        let inline = symbol(&symbols, "inline");
        assert!(extractor.base.type_info.get(&inline.id).is_none());
    }

    #[test]
    fn unique_symbol_annotation_records_no_fact() {
        let (symbols, extractor) = extract("const $output: unique symbol = Symbol();");

        let output = symbol(&symbols, "$output");
        assert!(extractor.base.type_info.get(&output.id).is_none());
    }

    #[test]
    fn plain_symbol_annotation_still_records_fact() {
        let (symbols, extractor) = extract("const token: symbol = Symbol();");

        let token = symbol(&symbols, "token");
        assert_eq!(fact(&extractor, token).resolved_type, "symbol");
    }

    #[test]
    fn destructured_parameters_get_no_symbols() {
        let (symbols, _extractor) = extract("function draw({x, y}: Point) {}");

        assert!(!symbols.iter().any(|s| {
            s.metadata
                .as_ref()
                .and_then(|m| m.get("role"))
                .map(|role| role == &serde_json::json!("parameter"))
                .unwrap_or(false)
        }));
    }
}
