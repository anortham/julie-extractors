use crate::base::{Symbol, SymbolKind, TypeInfo};
use crate::c::CExtractor;
use std::path::PathBuf;

fn extract(source: &str) -> (Vec<Symbol>, CExtractor) {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_c::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = CExtractor::new(
        "c".to_string(),
        "type_facts.c".to_string(),
        source.to_string(),
        &workspace_root,
    );
    let symbols = extractor.extract_symbols(&tree);
    (symbols, extractor)
}

fn symbol<'a>(symbols: &'a [Symbol], name: &str, kind: SymbolKind) -> &'a Symbol {
    symbols
        .iter()
        .find(|s| s.name == name && s.kind == kind)
        .unwrap_or_else(|| panic!("missing symbol {name}"))
}

fn fact<'a>(extractor: &'a CExtractor, symbol: &Symbol) -> &'a TypeInfo {
    extractor
        .base
        .type_info
        .get(&symbol.id)
        .unwrap_or_else(|| panic!("missing type fact for {}", symbol.name))
}

fn declared(fact: &TypeInfo) -> Option<&str> {
    fact.metadata
        .as_ref()
        .and_then(|m| m.get("declared"))
        .and_then(|v| v.as_str())
}

fn role(symbol: &Symbol) -> Option<&str> {
    symbol
        .metadata
        .as_ref()
        .and_then(|m| m.get("role"))
        .and_then(|v| v.as_str())
}

fn parameter<'a>(symbols: &'a [Symbol], name: &str) -> &'a Symbol {
    let matches: Vec<_> = symbols
        .iter()
        .filter(|s| {
            s.name == name && s.kind == SymbolKind::Variable && role(s) == Some("parameter")
        })
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "expected one parameter symbol named {name}"
    );
    matches[0]
}

#[test]
fn function_parameters_record_structural_base_and_declared_text() {
    let source = r#"
        struct foo;
        void f(struct foo *x, const char *s, int n) {
        }
    "#;
    let (symbols, extractor) = extract(source);
    let function = symbol(&symbols, "f", SymbolKind::Function);

    let x = parameter(&symbols, "x");
    assert_eq!(x.parent_id.as_deref(), Some(function.id.as_str()));
    let x_fact = fact(&extractor, x);
    assert_eq!(x_fact.resolved_type, "foo");
    assert_eq!(declared(x_fact), Some("struct foo *"));
    assert!(!x_fact.is_inferred);

    let s = parameter(&symbols, "s");
    assert_eq!(s.parent_id.as_deref(), Some(function.id.as_str()));
    let s_fact = fact(&extractor, s);
    assert_eq!(s_fact.resolved_type, "char");
    assert_eq!(declared(s_fact), Some("const char *"));
    assert!(!s_fact.is_inferred);

    let n = parameter(&symbols, "n");
    assert_eq!(n.parent_id.as_deref(), Some(function.id.as_str()));
    let n_fact = fact(&extractor, n);
    assert_eq!(n_fact.resolved_type, "int");
    assert_eq!(declared(n_fact), None);
    assert!(!n_fact.is_inferred);

    for info in extractor.base.type_info.values() {
        assert!(
            !info.resolved_type.ends_with('*'),
            "resolved_type must not end in *: {}",
            info.resolved_type
        );
    }
}

#[test]
fn pointer_local_records_struct_base_and_parents_to_function() {
    let source = r#"
        struct foo;
        struct foo *make(void);
        void f(void) {
            struct foo *p = make();
        }
    "#;
    let (symbols, extractor) = extract(source);
    let function = symbol(&symbols, "f", SymbolKind::Function);
    let local = symbol(&symbols, "p", SymbolKind::Variable);
    assert_eq!(local.parent_id.as_deref(), Some(function.id.as_str()));
    assert_ne!(role(local), Some("parameter"));
    let local_fact = fact(&extractor, local);
    assert_eq!(local_fact.resolved_type, "foo");
    assert_eq!(declared(local_fact), Some("struct foo *"));
    assert!(!local_fact.is_inferred);
}

#[test]
fn array_local_records_unsized_base_and_sized_declared() {
    let source = r#"
        void f(void) {
            int buf[8];
        }
    "#;
    let (symbols, extractor) = extract(source);
    let function = symbol(&symbols, "f", SymbolKind::Function);
    let local = symbol(&symbols, "buf", SymbolKind::Variable);
    assert_eq!(local.parent_id.as_deref(), Some(function.id.as_str()));
    let local_fact = fact(&extractor, local);
    assert_eq!(local_fact.resolved_type, "int[]");
    assert_eq!(declared(local_fact), Some("int[8]"));
    assert!(!local_fact.is_inferred);
}

#[test]
fn struct_field_records_pointer_base_without_star() {
    let source = r#"
        struct bar;
        struct node {
            struct bar *next;
        };
    "#;
    let (symbols, extractor) = extract(source);
    let field = symbol(&symbols, "next", SymbolKind::Field);
    let field_fact = fact(&extractor, field);
    assert_eq!(field_fact.resolved_type, "bar");
    assert_eq!(declared(field_fact), Some("struct bar *"));
    assert!(!field_fact.is_inferred);
    assert!(!field_fact.resolved_type.ends_with('*'));
}

#[test]
fn function_pointer_parameter_is_a_symbol_without_a_fact() {
    let source = r#"
        void handler(void (*cb)(int)) {
        }
    "#;
    let (symbols, extractor) = extract(source);
    let function = symbol(&symbols, "handler", SymbolKind::Function);
    let cb = parameter(&symbols, "cb");
    assert_eq!(cb.parent_id.as_deref(), Some(function.id.as_str()));
    assert!(!extractor.base.type_info.contains_key(&cb.id));
}

#[test]
fn function_prototype_does_not_create_parameter_symbols() {
    let source = r#"
        void proto(int n);
    "#;
    let (symbols, _) = extract(source);
    let params: Vec<_> = symbols
        .iter()
        .filter(|s| role(s) == Some("parameter"))
        .collect();
    assert!(params.is_empty());
}

#[test]
fn multi_word_sized_types_record_no_fact_and_single_word_sized_types_record_the_word() {
    let source = r#"
        void sized(unsigned int a, long long b, unsigned c, long d) {
            unsigned long e = 0;
        }
    "#;
    let (symbols, extractor) = extract(source);
    let function = symbol(&symbols, "sized", SymbolKind::Function);
    for name in ["a", "b"] {
        let param = parameter(&symbols, name);
        assert_eq!(param.parent_id.as_deref(), Some(function.id.as_str()));
        assert!(!extractor.base.type_info.contains_key(&param.id));
    }
    let e = symbol(&symbols, "e", SymbolKind::Variable);
    assert!(!extractor.base.type_info.contains_key(&e.id));

    assert_eq!(
        fact(&extractor, parameter(&symbols, "c")).resolved_type,
        "unsigned"
    );
    assert_eq!(
        fact(&extractor, parameter(&symbols, "d")).resolved_type,
        "long"
    );
    for fact in extractor.base.type_info.values() {
        assert!(!fact.resolved_type.contains(char::is_whitespace));
    }
}

#[test]
fn legacy_signature_inference_rejects_test_macro_text() {
    let source = r#"
        TestSuite(math, .init = setup_suite);

        Test(math, addition) {
            cr_assert_eq(2 + 2, 4);
        }

        int count(void) { return 1; }
    "#;
    let (symbols, extractor) = extract(source);
    let types = extractor.infer_types(&symbols);
    for symbol in symbols.iter().filter(|s| s.name != "count") {
        assert!(
            !types.contains_key(&symbol.id),
            "unexpected type row for `{}`",
            symbol.name
        );
    }
    let count = symbol(&symbols, "count", SymbolKind::Function);
    assert_eq!(types.get(&count.id).map(String::as_str), Some("int"));
}
