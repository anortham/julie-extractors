#[cfg(test)]
mod cpp_type_fact_tests {
    use super::super::parse_cpp;
    use crate::base::{IdentifierKind, Symbol, SymbolKind, TypeInfo};
    use crate::cpp::CppExtractor;
    use std::path::PathBuf;
    use tree_sitter::Parser;

    fn extract(code: &str) -> (Vec<Symbol>, CppExtractor) {
        let (mut extractor, tree) = parse_cpp(code);
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

    fn fact<'a>(extractor: &'a CppExtractor, symbol: &Symbol) -> &'a TypeInfo {
        extractor
            .base
            .type_info
            .get(&symbol.id)
            .unwrap_or_else(|| panic!("missing type fact for `{}`", symbol.name))
    }

    fn declared_metadata(fact: &TypeInfo) -> Option<&serde_json::Value> {
        fact.metadata.as_ref().and_then(|m| m.get("declared"))
    }

    fn no_fact(extractor: &CppExtractor, symbol: &Symbol) {
        assert!(
            extractor.base.type_info.get(&symbol.id).is_none(),
            "expected no type fact for `{}`",
            symbol.name
        );
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

    fn assert_no_pointer_or_reference_resolved(extractor: &CppExtractor) {
        for fact in extractor.base.type_info.values() {
            assert!(
                !fact.resolved_type.ends_with('*') && !fact.resolved_type.ends_with('&'),
                "resolved_type `{}` ends in * or &",
                fact.resolved_type
            );
        }
    }

    #[test]
    fn typed_parameters_record_structural_base_names() {
        let (symbols, extractor) = extract(
            r#"
class Foo {};

void f(const Foo& a, Foo* b, std::vector<Foo> c, Foo&& d) {}
"#,
        );

        let f = symbol(&symbols, "f");
        let a = parameter_symbols(&symbols, "a");
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].kind, SymbolKind::Variable);
        assert_eq!(a[0].parent_id.as_deref(), Some(f.id.as_str()));
        let a_fact = fact(&extractor, a[0]);
        assert_eq!(a_fact.resolved_type, "Foo");
        assert!(!a_fact.is_inferred);
        assert_eq!(a_fact.language, "cpp");
        assert_eq!(
            declared_metadata(a_fact),
            Some(&serde_json::json!("const Foo&"))
        );

        let b = parameter_symbols(&symbols, "b");
        assert_eq!(b.len(), 1);
        let b_fact = fact(&extractor, b[0]);
        assert_eq!(b_fact.resolved_type, "Foo");
        assert!(!b_fact.is_inferred);
        assert_eq!(declared_metadata(b_fact), Some(&serde_json::json!("Foo*")));

        let c = parameter_symbols(&symbols, "c");
        assert_eq!(c.len(), 1);
        let c_fact = fact(&extractor, c[0]);
        assert_eq!(c_fact.resolved_type, "std::vector");
        assert!(!c_fact.is_inferred);
        assert_eq!(
            declared_metadata(c_fact),
            Some(&serde_json::json!("std::vector<Foo>"))
        );

        let d = parameter_symbols(&symbols, "d");
        assert_eq!(d.len(), 1);
        let d_fact = fact(&extractor, d[0]);
        assert_eq!(d_fact.resolved_type, "Foo");
        assert!(!d_fact.is_inferred);
        assert_eq!(declared_metadata(d_fact), Some(&serde_json::json!("Foo&&")));

        assert_no_pointer_or_reference_resolved(&extractor);
    }

    #[test]
    fn auto_make_unique_records_no_fact() {
        let (symbols, extractor) = extract(
            r#"
class Foo {};

void use() {
    auto x = std::make_unique<Foo>();
}
"#,
        );

        let use_fn = symbol(&symbols, "use");
        let x = variable(&symbols, "x");
        assert_eq!(x.parent_id.as_deref(), Some(use_fn.id.as_str()));
        no_fact(&extractor, x);
    }

    #[test]
    fn auto_unknown_call_records_no_fact() {
        let (symbols, extractor) = extract(
            r#"
void use() {
    auto y = Unknown();
}
"#,
        );

        let y = variable(&symbols, "y");
        no_fact(&extractor, y);
    }

    #[test]
    fn auto_qualified_constructor_records_no_fact() {
        let (symbols, extractor) = extract(
            r#"
class Foo {};

void use() {
    auto z = ns::Foo();
}
"#,
        );

        let z = variable(&symbols, "z");
        no_fact(&extractor, z);
    }

    #[test]
    fn declared_local_records_fact() {
        let (symbols, extractor) = extract(
            r#"
class Foo {};

void use() {
    Foo x;
    Foo listed{1};
    Foo direct(1);
}
"#,
        );

        let use_fn = symbol(&symbols, "use");
        for name in ["x", "listed", "direct"] {
            let local = variable(&symbols, name);
            assert_eq!(local.parent_id.as_deref(), Some(use_fn.id.as_str()));
            let local_fact = fact(&extractor, local);
            assert_eq!(local_fact.resolved_type, "Foo");
            assert!(!local_fact.is_inferred);
            assert_eq!(declared_metadata(local_fact), None);
        }
        assert_no_pointer_or_reference_resolved(&extractor);
    }

    #[test]
    fn auto_same_file_constructor_and_new_record_inferred_facts() {
        let (symbols, extractor) = extract(
            r#"
class Foo {};

void use() {
    auto w = Foo();
    auto n = new Foo();
}
"#,
        );

        let w = variable(&symbols, "w");
        let w_fact = fact(&extractor, w);
        assert_eq!(w_fact.resolved_type, "Foo");
        assert!(w_fact.is_inferred);

        let n = variable(&symbols, "n");
        let n_fact = fact(&extractor, n);
        assert_eq!(n_fact.resolved_type, "Foo");
        assert!(n_fact.is_inferred);
        assert_no_pointer_or_reference_resolved(&extractor);
    }

    #[test]
    fn field_declaration_records_fact() {
        let (symbols, extractor) = extract(
            r#"
class Foo {};

class Box {
    Foo item;
    Foo* ptr;
};
"#,
        );

        let box_cls = symbol(&symbols, "Box");
        let item = symbols
            .iter()
            .find(|s| s.name == "item" && s.kind == SymbolKind::Field)
            .expect("missing field item");
        assert_eq!(item.parent_id.as_deref(), Some(box_cls.id.as_str()));
        let item_fact = fact(&extractor, item);
        assert_eq!(item_fact.resolved_type, "Foo");
        assert!(!item_fact.is_inferred);
        assert_eq!(declared_metadata(item_fact), None);

        let ptr = symbols
            .iter()
            .find(|s| s.name == "ptr" && s.kind == SymbolKind::Field)
            .expect("missing field ptr");
        let ptr_fact = fact(&extractor, ptr);
        assert_eq!(ptr_fact.resolved_type, "Foo");
        assert!(!ptr_fact.is_inferred);
        assert_eq!(
            declared_metadata(ptr_fact),
            Some(&serde_json::json!("Foo*"))
        );
        assert_no_pointer_or_reference_resolved(&extractor);
    }

    fn extract_calls(code: &str) -> (Vec<crate::base::Identifier>, CppExtractor) {
        let tree = {
            let mut parser = Parser::new();
            parser
                .set_language(&tree_sitter_cpp::LANGUAGE.into())
                .expect("Error loading C++ grammar");
            parser.parse(code, None).expect("Failed to parse C++ code")
        };
        let mut extractor = CppExtractor::new(
            "test.cpp".to_string(),
            code.to_string(),
            &PathBuf::from("/tmp/test"),
        );
        let symbols = extractor.extract_symbols(&tree);
        let identifiers = extractor.extract_identifiers(&tree, &symbols);
        extractor.extract_relationships(&tree, &symbols);
        (identifiers, extractor)
    }

    #[test]
    fn this_arrow_call_inside_class_records_receiver_type() {
        let code = r#"
class Foo {
public:
    void run() {
        this->ping();
        other.ping();
    }
};
"#;
        let (identifiers, extractor) = extract_calls(code);
        let ping_calls: Vec<_> = identifiers
            .iter()
            .filter(|id| id.name == "ping" && id.kind == IdentifierKind::Call)
            .collect();
        assert_eq!(ping_calls.len(), 2);
        assert_eq!(
            ping_calls
                .iter()
                .filter(|id| id.receiver_type.as_deref() == Some("Foo"))
                .count(),
            1
        );
        assert_eq!(
            ping_calls
                .iter()
                .filter(|id| id.receiver_type.is_none())
                .count(),
            1
        );

        let ping_pending: Vec<_> = extractor
            .get_structured_pending_relationships()
            .into_iter()
            .filter(|pending| pending.target.terminal_name == "ping")
            .collect();
        assert_eq!(ping_pending.len(), 2);
        assert_eq!(
            ping_pending
                .iter()
                .filter(|pending| pending.receiver_type.as_deref() == Some("Foo"))
                .count(),
            1
        );
        assert_eq!(
            ping_pending
                .iter()
                .filter(|pending| pending.receiver_type.is_none())
                .count(),
            1
        );
    }

    #[test]
    fn this_call_inside_out_of_line_method_records_receiver_type() {
        let code = r#"
class Foo {
public:
    void run();
};

void Foo::run() {
    this->ping();
    (*this).ping();
}
"#;
        let (identifiers, extractor) = extract_calls(code);
        let ping_calls: Vec<_> = identifiers
            .iter()
            .filter(|id| id.name == "ping" && id.kind == IdentifierKind::Call)
            .collect();
        assert_eq!(ping_calls.len(), 2);
        assert!(
            ping_calls
                .iter()
                .all(|id| id.receiver_type.as_deref() == Some("Foo"))
        );

        let ping_pending: Vec<_> = extractor
            .get_structured_pending_relationships()
            .into_iter()
            .filter(|pending| pending.target.terminal_name == "ping")
            .collect();
        assert_eq!(ping_pending.len(), 2);
        assert!(
            ping_pending
                .iter()
                .all(|pending| pending.receiver_type.as_deref() == Some("Foo"))
        );
    }

    #[test]
    fn this_call_inside_namespace_qualified_method_records_final_scope_segment() {
        let code = r#"
namespace ns {
class Worker {
public:
    void m();
};
}

void ns::Worker::m() {
    this->ping();
}
"#;
        let (identifiers, extractor) = extract_calls(code);
        let ping_calls: Vec<_> = identifiers
            .iter()
            .filter(|id| id.name == "ping" && id.kind == IdentifierKind::Call)
            .collect();
        assert_eq!(ping_calls.len(), 1);
        assert_eq!(ping_calls[0].receiver_type.as_deref(), Some("Worker"));

        let ping_pending: Vec<_> = extractor
            .get_structured_pending_relationships()
            .into_iter()
            .filter(|pending| pending.target.terminal_name == "ping")
            .collect();
        assert_eq!(ping_pending.len(), 1);
        assert_eq!(ping_pending[0].receiver_type.as_deref(), Some("Worker"));
    }

    #[test]
    fn function_pointer_parameter_is_a_symbol_without_a_fact() {
        let (symbols, extractor) = extract(
            r#"
void handler(void (*cb)(int), int n) {}
"#,
        );

        let handler = symbol(&symbols, "handler");
        let cb = parameter_symbols(&symbols, "cb");
        assert_eq!(cb.len(), 1);
        assert_eq!(cb[0].kind, SymbolKind::Variable);
        assert_eq!(cb[0].parent_id.as_deref(), Some(handler.id.as_str()));
        no_fact(&extractor, cb[0]);

        let n = parameter_symbols(&symbols, "n");
        assert_eq!(fact(&extractor, n[0]).resolved_type, "int");
    }

    #[test]
    fn multi_word_sized_types_record_no_fact_and_single_word_sized_types_record_the_word() {
        let (symbols, extractor) = extract(
            r#"
void sized(unsigned int a, long long b, unsigned c, long d) {
    unsigned long e = 0;
}
"#,
        );

        for name in ["a", "b"] {
            let param = parameter_symbols(&symbols, name);
            assert_eq!(param.len(), 1);
            no_fact(&extractor, param[0]);
        }
        no_fact(&extractor, variable(&symbols, "e"));
        assert_eq!(
            fact(&extractor, parameter_symbols(&symbols, "c")[0]).resolved_type,
            "unsigned"
        );
        assert_eq!(
            fact(&extractor, parameter_symbols(&symbols, "d")[0]).resolved_type,
            "long"
        );
        for fact in extractor.base.type_info.values() {
            assert!(!fact.resolved_type.contains(char::is_whitespace));
        }
    }

    #[test]
    fn trailing_qualifier_keeps_declared_text_in_source_order() {
        let (symbols, extractor) = extract(
            r#"
class Foo {};

void f(Foo const& a, const Foo& b) {}
"#,
        );

        let a_fact = fact(&extractor, parameter_symbols(&symbols, "a")[0]);
        assert_eq!(a_fact.resolved_type, "Foo");
        assert_eq!(
            declared_metadata(a_fact),
            Some(&serde_json::json!("Foo const&"))
        );
        let b_fact = fact(&extractor, parameter_symbols(&symbols, "b")[0]);
        assert_eq!(b_fact.resolved_type, "Foo");
        assert_eq!(
            declared_metadata(b_fact),
            Some(&serde_json::json!("const Foo&"))
        );
    }
}
