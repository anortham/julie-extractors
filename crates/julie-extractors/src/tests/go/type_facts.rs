#[cfg(test)]
mod go_type_fact_tests {
    use crate::base::{Symbol, SymbolKind, TypeInfo};
    use crate::go::GoExtractor;
    use crate::tests::helpers::init_parser;
    use std::path::PathBuf;

    fn extract(code: &str) -> (Vec<Symbol>, GoExtractor) {
        let tree = init_parser(code, "go");
        let mut extractor = GoExtractor::new(
            "go".to_string(),
            "test.go".to_string(),
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

    fn variable<'a>(symbols: &'a [Symbol], name: &str) -> &'a Symbol {
        symbols
            .iter()
            .find(|s| s.name == name && s.kind == SymbolKind::Variable)
            .unwrap_or_else(|| panic!("missing variable symbol `{name}`"))
    }

    fn fact<'a>(extractor: &'a GoExtractor, symbol: &Symbol) -> &'a TypeInfo {
        extractor
            .base
            .type_info
            .get(&symbol.id)
            .unwrap_or_else(|| panic!("missing type fact for `{}`", symbol.name))
    }

    fn declared_metadata(fact: &TypeInfo) -> Option<&serde_json::Value> {
        fact.metadata.as_ref().and_then(|m| m.get("declared"))
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
    fn var_typed_local_records_declared_fact() {
        let (symbols, extractor) = extract(
            r#"
package main

type Store struct{}

func Use() {
    var s Store
    var p *Store
    _ = s
    _ = p
}
"#,
        );

        let use_fn = symbol(&symbols, "Use");
        let s = variable(&symbols, "s");
        assert_eq!(s.parent_id.as_deref(), Some(use_fn.id.as_str()));
        let s_fact = fact(&extractor, s);
        assert_eq!(s_fact.resolved_type, "Store");
        assert!(!s_fact.is_inferred);
        assert_eq!(s_fact.language, "go");
        assert_eq!(declared_metadata(s_fact), None);

        let p = variable(&symbols, "p");
        let p_fact = fact(&extractor, p);
        assert_eq!(p_fact.resolved_type, "Store");
        assert!(!p_fact.is_inferred);
        assert_eq!(
            declared_metadata(p_fact),
            Some(&serde_json::json!("*Store"))
        );
    }

    #[test]
    fn var_qualified_and_generic_types_record_base_names() {
        let (symbols, extractor) = extract(
            r#"
package main

import "example.com/pkg"

type Stack[T any] struct{}

func Use() {
    var c pkg.Config
    var w Stack[int]
    _ = c
    _ = w
}
"#,
        );

        let c_fact = fact(&extractor, variable(&symbols, "c"));
        assert_eq!(c_fact.resolved_type, "pkg.Config");
        assert_eq!(declared_metadata(c_fact), None);

        let w_fact = fact(&extractor, variable(&symbols, "w"));
        assert_eq!(w_fact.resolved_type, "Stack");
        assert_eq!(
            declared_metadata(w_fact),
            Some(&serde_json::json!("Stack[int]"))
        );
    }

    #[test]
    fn composite_literal_local_records_inferred_fact() {
        let (symbols, extractor) = extract(
            r#"
package main

type Store struct{}

func Use() {
    s := Store{}
    p := &Store{}
    _ = s
    _ = p
}
"#,
        );

        let use_fn = symbol(&symbols, "Use");
        let s = variable(&symbols, "s");
        assert_eq!(s.parent_id.as_deref(), Some(use_fn.id.as_str()));
        let s_fact = fact(&extractor, s);
        assert_eq!(s_fact.resolved_type, "Store");
        assert!(s_fact.is_inferred);

        let p = variable(&symbols, "p");
        let p_fact = fact(&extractor, p);
        assert_eq!(p_fact.resolved_type, "Store");
        assert!(p_fact.is_inferred);
    }

    #[test]
    fn multi_assignment_records_only_composite_literal_positions() {
        let (symbols, extractor) = extract(
            r#"
package main

type Store struct{}

func Use() {
    a, b := Store{}, NewThing()
    _ = a
    _ = b
}
"#,
        );

        let a = variable(&symbols, "a");
        let a_fact = fact(&extractor, a);
        assert_eq!(a_fact.resolved_type, "Store");
        assert!(a_fact.is_inferred);

        assert!(
            !symbols
                .iter()
                .any(|s| s.name == "b" && s.kind == SymbolKind::Variable)
        );
    }

    #[test]
    fn constructor_call_local_records_nothing() {
        let (symbols, _extractor) = extract(
            r#"
package main

func Use() {
    s := NewStore()
    m := map[string]int{}
    items := []int{1, 2}
    _ = s
    _ = m
    _ = items
}
"#,
        );

        for name in ["s", "m", "items"] {
            assert!(
                !symbols
                    .iter()
                    .any(|s| s.name == name && s.kind == SymbolKind::Variable),
                "expected no variable symbol for `{name}`"
            );
        }
    }

    #[test]
    fn typed_parameters_become_symbols_with_facts() {
        let (symbols, extractor) = extract(
            r#"
package main

type Store struct{}

func Handle(a, b string, s *Store) {}
"#,
        );

        let handle = symbol(&symbols, "Handle");
        for name in ["a", "b"] {
            let params = parameter_symbols(&symbols, name);
            assert_eq!(params.len(), 1, "expected one `{name}` parameter symbol");
            let param = params[0];
            assert_eq!(param.kind, SymbolKind::Variable);
            assert_eq!(param.parent_id.as_deref(), Some(handle.id.as_str()));
            assert_eq!(param.signature.as_deref(), Some("a, b string"));
            let param_fact = fact(&extractor, param);
            assert_eq!(param_fact.resolved_type, "string");
            assert!(!param_fact.is_inferred);
        }

        let s_params = parameter_symbols(&symbols, "s");
        assert_eq!(s_params.len(), 1);
        assert_eq!(s_params[0].signature.as_deref(), Some("s *Store"));
        let s_fact = fact(&extractor, s_params[0]);
        assert_eq!(s_fact.resolved_type, "Store");
        assert!(!s_fact.is_inferred);
        assert_eq!(
            declared_metadata(s_fact),
            Some(&serde_json::json!("*Store"))
        );
    }

    #[test]
    fn method_receiver_becomes_parameter_symbol_with_fact() {
        let (symbols, extractor) = extract(
            r#"
package main

type Store struct{}

func (s *Store) Get() string {
    return ""
}

func (*Store) Reset() {}
"#,
        );

        let get = symbol(&symbols, "Get");
        let receivers = parameter_symbols(&symbols, "s");
        assert_eq!(receivers.len(), 1);
        let receiver = receivers[0];
        assert_eq!(receiver.kind, SymbolKind::Variable);
        assert_eq!(receiver.parent_id.as_deref(), Some(get.id.as_str()));
        let receiver_fact = fact(&extractor, receiver);
        assert_eq!(receiver_fact.resolved_type, "Store");
        assert!(!receiver_fact.is_inferred);
        assert_eq!(
            declared_metadata(receiver_fact),
            Some(&serde_json::json!("*Store"))
        );

        let reset = symbol(&symbols, "Reset");
        assert!(!symbols.iter().any(|s| {
            s.parent_id.as_deref() == Some(reset.id.as_str()) && s.kind == SymbolKind::Variable
        }));
    }

    #[test]
    fn generic_pointer_receiver_records_base_type() {
        let (symbols, extractor) = extract(
            r#"
package main

type Stack[T any] struct{}

func (s *Stack[T]) Push(item T) {}
"#,
        );

        let receivers = parameter_symbols(&symbols, "s");
        assert_eq!(receivers.len(), 1);
        let receiver_fact = fact(&extractor, receivers[0]);
        assert_eq!(receiver_fact.resolved_type, "Stack");
        assert_eq!(
            declared_metadata(receiver_fact),
            Some(&serde_json::json!("*Stack[T]"))
        );
    }

    #[test]
    fn slice_map_and_variadic_parameters_get_no_facts() {
        let (symbols, extractor) = extract(
            r#"
package main

type Store struct{}

func Handle(items []Store, index map[string]Store, rest ...Store) {}
"#,
        );

        for name in ["items", "index", "rest"] {
            let params = parameter_symbols(&symbols, name);
            assert_eq!(params.len(), 1, "expected one `{name}` parameter symbol");
            assert!(
                extractor.base.type_info.get(&params[0].id).is_none(),
                "expected no type fact for `{name}`"
            );
        }
    }

    #[test]
    fn named_struct_fields_record_facts_and_embedded_fields_skip() {
        let (symbols, extractor) = extract(
            r#"
package main

type Store struct{}

type Registry struct {
    Primary *Store
    Names   []string
    Store
}
"#,
        );

        let primary = symbols
            .iter()
            .find(|s| s.name == "Primary" && s.kind == SymbolKind::Field)
            .expect("missing Primary field");
        let primary_fact = fact(&extractor, primary);
        assert_eq!(primary_fact.resolved_type, "Store");
        assert!(!primary_fact.is_inferred);
        assert_eq!(
            declared_metadata(primary_fact),
            Some(&serde_json::json!("*Store"))
        );

        let names = symbols
            .iter()
            .find(|s| s.name == "Names" && s.kind == SymbolKind::Field)
            .expect("missing Names field");
        assert!(extractor.base.type_info.get(&names.id).is_none());

        let embedded = symbols
            .iter()
            .find(|s| s.kind == SymbolKind::Field && s.name == "Store")
            .expect("missing embedded Store field");
        assert!(extractor.base.type_info.get(&embedded.id).is_none());
    }
}
