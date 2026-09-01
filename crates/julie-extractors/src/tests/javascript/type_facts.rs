#[cfg(test)]
mod javascript_type_fact_tests {
    use crate::base::{Symbol, SymbolKind};
    use crate::javascript::JavaScriptExtractor;
    use std::path::PathBuf;
    use tree_sitter::Parser;

    fn extract(code: &str) -> (Vec<Symbol>, JavaScriptExtractor) {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_javascript::LANGUAGE.into())
            .expect("Error loading JavaScript grammar");
        let tree = parser.parse(code, None).expect("Error parsing code");
        let mut extractor = JavaScriptExtractor::new(
            "javascript".to_string(),
            "test.js".to_string(),
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

    fn parameter_symbols<'a>(symbols: &'a [Symbol], name: &str) -> Vec<&'a Symbol> {
        symbols
            .iter()
            .filter(|s| {
                s.name == name
                    && s.metadata
                        .as_ref()
                        .and_then(|m| m.get("role"))
                        .map(|role| role == &serde_json::json!("parameter"))
                        .unwrap_or(false)
            })
            .collect()
    }

    #[test]
    fn new_expression_local_records_inferred_fact() {
        let (symbols, extractor) = extract("const graph = new GraphTraversal();");

        let graph = symbol(&symbols, "graph");
        let fact = extractor
            .base
            .type_info
            .get(&graph.id)
            .expect("missing type fact for `graph`");
        assert_eq!(fact.resolved_type, "GraphTraversal");
        assert!(fact.is_inferred);
        assert_eq!(fact.language, "javascript");
    }

    #[test]
    fn namespaced_new_expression_records_nothing() {
        let (symbols, extractor) = extract("const graph = new ns.GraphTraversal();");

        let graph = symbol(&symbols, "graph");
        assert!(!extractor.base.type_info.contains_key(&graph.id));
    }

    #[test]
    fn function_parameters_become_symbols_without_facts() {
        let (symbols, extractor) = extract("function process(input, count = 3, ...rest) {}");

        let process = symbol(&symbols, "process");
        for name in ["input", "count", "rest"] {
            let params = parameter_symbols(&symbols, name);
            assert_eq!(params.len(), 1, "expected one `{name}` parameter symbol");
            let param = params[0];
            assert_eq!(param.kind, SymbolKind::Variable);
            assert_eq!(param.parent_id.as_deref(), Some(process.id.as_str()));
            assert!(!extractor.base.type_info.contains_key(&param.id));
        }
    }

    #[test]
    fn method_and_constructor_parameters_become_symbols() {
        let (symbols, _extractor) = extract(
            r#"
class Worker {
    constructor(id) {}
    run(task) {}
}
"#,
        );

        let constructor = symbol(&symbols, "constructor");
        let id_params = parameter_symbols(&symbols, "id");
        assert_eq!(id_params.len(), 1);
        assert_eq!(
            id_params[0].parent_id.as_deref(),
            Some(constructor.id.as_str())
        );

        let run = symbol(&symbols, "run");
        let task_params = parameter_symbols(&symbols, "task");
        assert_eq!(task_params.len(), 1);
        assert_eq!(task_params[0].parent_id.as_deref(), Some(run.id.as_str()));
    }

    #[test]
    fn arrow_function_parameter_becomes_symbol_under_a_handler_function() {
        let (symbols, _extractor) = extract("const handler = (event) => event.id;");

        let handler_ids: Vec<&str> = symbols
            .iter()
            .filter(|s| s.name == "handler" && s.kind == SymbolKind::Function)
            .map(|s| s.id.as_str())
            .collect();
        assert!(!handler_ids.is_empty());

        let event_params = parameter_symbols(&symbols, "event");
        assert_eq!(event_params.len(), 1);
        let parent = event_params[0].parent_id.as_deref().expect("parent id");
        assert!(handler_ids.contains(&parent));
    }

    #[test]
    fn destructured_parameters_get_no_symbols() {
        let (symbols, _extractor) = extract("function draw({x, y}) {}");

        assert!(!symbols.iter().any(|s| {
            s.metadata
                .as_ref()
                .and_then(|m| m.get("role"))
                .map(|role| role == &serde_json::json!("parameter"))
                .unwrap_or(false)
        }));
    }
}
