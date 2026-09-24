use crate::base::{Identifier, IdentifierKind, Relationship, RelationshipKind, Symbol, SymbolKind};
use crate::c::CExtractor;
use std::path::PathBuf;

fn extract(source: &str) -> (Vec<Symbol>, CExtractor) {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_c::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let mut extractor = CExtractor::new(
        "c".to_string(),
        "initializer_types.c".to_string(),
        source.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    (symbols, extractor)
}

struct Extraction {
    symbols: Vec<Symbol>,
    relationships: Vec<Relationship>,
    identifiers: Vec<Identifier>,
    pending_targets: Vec<(RelationshipKind, String)>,
}

fn extract_all(source: &str) -> Extraction {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_c::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let mut extractor = CExtractor::new(
        "c".to_string(),
        "initializer_types.c".to_string(),
        source.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    let relationships = extractor.extract_relationships(&tree, &symbols);
    let identifiers = extractor.extract_identifiers(&tree, &symbols);
    let pending_targets = extractor
        .get_structured_pending_relationships()
        .into_iter()
        .map(|pending| (pending.pending.kind, pending.target.terminal_name))
        .collect();
    Extraction {
        symbols,
        relationships,
        identifiers,
        pending_targets,
    }
}

fn fact_of(source: &str, variable: &str) -> Option<(String, bool, Option<String>)> {
    let (symbols, extractor) = extract(source);
    let local = symbols
        .iter()
        .find(|s| s.name == variable && s.kind == SymbolKind::Variable)
        .unwrap_or_else(|| panic!("missing variable {variable}"));
    extractor.base.type_info.get(&local.id).map(|fact| {
        let declared = fact
            .metadata
            .as_ref()
            .and_then(|m| m.get("declared"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
        (fact.resolved_type.clone(), fact.is_inferred, declared)
    })
}

fn in_function(prelude: &str, body: &str) -> String {
    format!("{prelude}\nvoid run(int x) {{\n    {body}\n}}\n")
}

const MAKERS: &str = r#"
struct widget { int id; };
struct widget *make(void);
int count(void) { return 1; }
"#;

fn widget() -> Option<(String, bool, Option<String>)> {
    Some((
        "widget".to_string(),
        true,
        Some("struct widget *".to_string()),
    ))
}

#[test]
fn gnu_auto_type_from_same_file_prototype_records_return_type_as_inferred() {
    assert_eq!(
        fact_of(&in_function(MAKERS, "__auto_type w = make();"), "w"),
        widget()
    );
}

#[test]
fn gnu_auto_type_from_same_file_definition_records_return_type() {
    assert_eq!(
        fact_of(&in_function(MAKERS, "__auto_type n = count();"), "n"),
        Some(("int".to_string(), true, None))
    );
}

#[test]
fn c23_auto_call_records_return_type_on_the_named_variable() {
    for body in [
        "auto w = make();",
        "auto w = make(x);",
        "auto w = make(1, x);",
        "auto const w = make();",
        "const auto w = make();",
        "static auto w = make();",
    ] {
        assert_eq!(fact_of(&in_function(MAKERS, body), "w"), widget(), "{body}");
    }
}

#[test]
fn c23_auto_declaration_is_a_variable_not_a_prototype() {
    let (symbols, _) = extract(&in_function(MAKERS, "static auto w = make(x);"));
    let functions: Vec<_> = symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Function && s.name == "make")
        .collect();
    assert_eq!(functions.len(), 1);
    let w = symbols
        .iter()
        .find(|s| s.name == "w")
        .expect("missing variable w");
    assert_eq!(w.kind, SymbolKind::Variable);
    assert_eq!(w.signature.as_deref(), Some("static auto w = make(x)"));
    assert!(symbols.iter().all(|s| !s.name.is_empty()));
}

#[test]
fn auto_function_prototype_with_a_typedef_return_is_not_a_variable() {
    let source = in_function(
        "typedef struct widget *widget_ref;",
        "auto widget_ref helper(void);",
    );
    let (symbols, _) = extract(&source);
    assert!(
        symbols
            .iter()
            .all(|s| s.name != "widget_ref" || s.kind != SymbolKind::Variable)
    );
    assert!(
        symbols
            .iter()
            .any(|s| s.name == "helper" && s.kind == SymbolKind::Function)
    );
}

#[test]
fn c23_auto_with_several_declarators_types_each_variable() {
    let source = in_function(MAKERS, "auto first = make(), second = make();");
    assert_eq!(fact_of(&source, "first"), widget());
    assert_eq!(fact_of(&source, "second"), widget());
}

#[test]
fn prototype_and_definition_that_agree_record_the_type() {
    let prelude =
        "struct widget;\nstruct widget *make(void);\nstruct widget *make(void) { return 0; }";
    assert_eq!(
        fact_of(&in_function(prelude, "__auto_type w = make();"), "w"),
        widget()
    );
}

#[test]
fn same_named_declarations_that_disagree_record_no_fact() {
    let prelude =
        "#ifdef GADGETS\nstruct gadget *make(void);\n#else\nstruct widget *make(void);\n#endif";
    assert_eq!(
        fact_of(&in_function(prelude, "__auto_type w = make();"), "w"),
        None
    );
}

#[test]
fn same_named_macro_records_no_fact() {
    for prelude in [
        "struct widget *make(void);\n#define make() make_gadget()",
        "struct widget *make(void);\n#define make make_gadget",
    ] {
        assert_eq!(
            fact_of(&in_function(prelude, "auto w = make();"), "w"),
            None,
            "{prelude}"
        );
    }
}

#[test]
fn function_pointer_shadowing_the_callee_records_no_fact() {
    let body = "struct gadget *(*make)(void) = pick;\n    __auto_type w = make();";
    assert_eq!(fact_of(&in_function(MAKERS, body), "w"), None);
}

#[test]
fn variable_or_parameter_shadowing_the_callee_records_no_fact() {
    let typedef = "typedef struct gadget *(*maker_fn)(void);";
    for source in [
        format!("{MAKERS}{typedef}\nvoid run(maker_fn make) {{\n    __auto_type w = make();\n}}\n"),
        format!(
            "{MAKERS}\nvoid run(struct gadget *(*make)(void)) {{\n    __auto_type w = make();\n}}\n"
        ),
        in_function(
            &format!("{MAKERS}{typedef}"),
            "maker_fn make = pick;\n    __auto_type w = make();",
        ),
    ] {
        assert_eq!(fact_of(&source, "w"), None, "{source}");
    }
}

const PICKER: &str = r#"
struct widget *make(void);
typedef long (*factory)(void);
factory pick(void);
"#;

#[test]
fn c23_auto_local_shadowing_the_callee_records_no_fact() {
    for source in [
        in_function(PICKER, "auto make = pick();\n    auto w = make();"),
        in_function(PICKER, "auto make = pick(1);\n    auto w = make(2);"),
        in_function(PICKER, "auto make = (factory)pick();\n    auto w = make();"),
        in_function(
            PICKER,
            "for (auto make = pick(); make; ) {\n        auto w = make();\n    }",
        ),
        in_function(
            &format!("{PICKER}static auto make = pick();"),
            "__auto_type w = make();",
        ),
    ] {
        assert_eq!(fact_of(&source, "w"), None, "{source}");
    }
}

#[test]
fn malformed_declaration_shadowing_the_callee_records_no_fact() {
    let prelude = "struct widget *make(void);\nstruct holder { long (*make)(void); int node; };";
    for body in [
        "long (*make)(void) = container_of(p, struct holder, node)->make;\n    __auto_type w = make();",
        "factory make = OFFSET(struct widget, x);\n    __auto_type w = make();",
    ] {
        let source = in_function(prelude, body);
        assert_eq!(fact_of(&source, "w"), None, "{source}");
    }
}

#[test]
fn malformed_prototype_that_disagrees_records_no_fact() {
    let prelude = "#ifdef GADGET\nstruct gadget *make(void) DEPRECATED(\"use other\" 2);\n#else\nstruct widget *make(void);\n#endif";
    assert_eq!(
        fact_of(&in_function(prelude, "__auto_type w = make();"), "w"),
        None
    );
}

#[test]
fn parenthesized_attribute_prototype_that_disagrees_records_no_fact() {
    for (misread, call) in [
        (
            "struct gadget *__attribute__((malloc)) make(void);",
            "make()",
        ),
        (
            "struct gadget *__attribute__((malloc)) make(void) { return 0; }",
            "make()",
        ),
        ("struct gadget *NONNULL(1) make(void *p);", "make(0)"),
        ("struct gadget *__declspec(dllexport) make(void);", "make()"),
    ] {
        let prelude = format!(
            "struct widget {{ int n; }}; struct gadget {{ int g; }};\n#ifdef USE_GADGET\n{misread}\n#else\nstruct widget *make(void);\n#endif"
        );
        let body = format!("__auto_type w = {call};");
        assert_eq!(
            fact_of(&in_function(&prelude, &body), "w"),
            None,
            "{misread}"
        );
    }
}

#[test]
fn macro_between_return_type_and_name_records_no_fact() {
    let prelude = "struct node *attr_pure find(void);";
    assert_eq!(
        fact_of(&in_function(prelude, "__auto_type n = find();"), "n"),
        None
    );
}

#[test]
fn function_returning_function_pointer_records_no_fact() {
    let prelude = "struct widget *(*factory(void))(void);";
    assert_eq!(
        fact_of(&in_function(prelude, "__auto_type f = factory();"), "f"),
        None
    );
}

#[test]
fn callee_from_another_file_records_no_fact() {
    for body in [
        "__auto_type w = elsewhere();",
        "auto w = elsewhere();",
        "auto w = elsewhere(1);",
    ] {
        assert_eq!(fact_of(&in_function(MAKERS, body), "w"), None, "{body}");
    }
}

#[test]
fn non_call_initializers_record_no_fact() {
    for body in [
        "__auto_type w = 5;",
        "__auto_type w = ctx->make();",
        "__auto_type w = (make());",
        "auto w = ctx->make();",
        "auto w = &x;",
    ] {
        assert_eq!(fact_of(&in_function(MAKERS, body), "w"), None, "{body}");
    }
}

#[test]
fn gnu_auto_type_on_a_pointer_declarator_records_no_fact() {
    assert_eq!(
        fact_of(&in_function(MAKERS, "__auto_type *w = make();"), "w"),
        None
    );
}

#[test]
fn misparsed_c23_auto_does_not_declare_a_return_type() {
    let body = "auto w = make();\n    __auto_type v = make();";
    assert_eq!(fact_of(&in_function("", body), "v"), None);
}

#[test]
fn written_type_wins_over_the_call_return_type() {
    assert_eq!(
        fact_of(&in_function(MAKERS, "struct gadget *w = make();"), "w"),
        Some((
            "gadget".to_string(),
            false,
            Some("struct gadget *".to_string())
        ))
    );
    assert_eq!(
        fact_of(&in_function(MAKERS, "auto int w = count();"), "w"),
        Some(("int".to_string(), false, None))
    );
}

#[test]
fn legacy_signature_inference_skips_compiler_inferred_types() {
    let source = in_function(
        "",
        "__auto_type v = elsewhere();\n    auto w = elsewhere();\n    auto t = elsewhere(1);",
    );
    let (symbols, extractor) = extract(&source);
    let inferred = extractor.infer_types(&symbols);
    for name in ["v", "w", "t"] {
        let variable = symbols
            .iter()
            .find(|s| s.name == name)
            .unwrap_or_else(|| panic!("missing variable {name}"));
        assert_eq!(inferred.get(&variable.id), None, "{name}");
    }
}

#[test]
fn legacy_signature_inference_matches_the_whole_variable_name() {
    let (symbols, extractor) = extract(&in_function("", "float a = 1.0f;"));
    let inferred = extractor.infer_types(&symbols);
    let a = symbols
        .iter()
        .find(|s| s.name == "a")
        .expect("missing variable a");
    assert_eq!(inferred.get(&a.id).map(String::as_str), Some("float"));
}

#[test]
fn legacy_signature_inference_skips_unnamed_variables() {
    let (symbols, extractor) = extract("static DEFINE_IRQ_WORK(crash_work, crash_fn);\n");
    let inferred = extractor.infer_types(&symbols);
    let unnamed: Vec<_> = symbols.iter().filter(|s| s.name.is_empty()).collect();
    assert!(!unnamed.is_empty());
    for symbol in unnamed {
        assert_eq!(inferred.get(&symbol.id), None, "{:?}", symbol.signature);
    }
}

#[test]
fn auto_placeholders_are_not_type_usages() {
    for body in [
        "__auto_type w = make();",
        "auto w = make();",
        "auto w = make(x);",
    ] {
        let extraction = extract_all(&in_function(MAKERS, body));
        let placeholder_usages: Vec<_> = extraction
            .identifiers
            .iter()
            .filter(|i| {
                i.kind == IdentifierKind::TypeUsage
                    && matches!(i.name.as_str(), "w" | "x" | "__auto_type")
            })
            .collect();
        assert!(
            placeholder_usages.is_empty(),
            "{body}: {placeholder_usages:?}"
        );
        let placeholder_uses: Vec<_> = extraction
            .pending_targets
            .iter()
            .filter(|(kind, name)| {
                *kind == RelationshipKind::Uses
                    && matches!(name.as_str(), "w" | "x" | "__auto_type")
            })
            .collect();
        assert!(placeholder_uses.is_empty(), "{body}: {placeholder_uses:?}");
    }
}

#[test]
fn c23_auto_call_without_arguments_is_a_call() {
    let extraction = extract_all(&in_function(MAKERS, "auto w = make();"));
    let id_of = |name: &str| {
        extraction
            .symbols
            .iter()
            .find(|s| s.name == name && s.kind == SymbolKind::Function)
            .unwrap_or_else(|| panic!("missing function {name}"))
            .id
            .clone()
    };
    let (run, make) = (id_of("run"), id_of("make"));
    assert!(extraction.relationships.iter().any(|r| {
        r.kind == RelationshipKind::Calls && r.from_symbol_id == run && r.to_symbol_id == make
    }));
    assert!(
        extraction
            .identifiers
            .iter()
            .any(|i| i.kind == IdentifierKind::Call && i.name == "make")
    );
}

#[test]
fn c23_auto_call_arguments_are_variable_references() {
    let extraction = extract_all(&in_function(MAKERS, "auto w = make(x);"));
    assert!(
        extraction
            .identifiers
            .iter()
            .any(|i| i.kind == IdentifierKind::VariableRef && i.name == "x")
    );
}

#[test]
fn c23_auto_call_type_arguments_stay_type_usages() {
    let source = in_function(MAKERS, "auto w = container_of(x, struct widget, id);");
    let call_line = source
        .lines()
        .position(|line| line.contains("container_of"))
        .unwrap() as u32
        + 1;
    let extraction = extract_all(&source);
    let kinds_of = |name: &str| {
        extraction
            .identifiers
            .iter()
            .filter(|i| i.name == name && i.start_line == call_line)
            .map(|i| i.kind.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(kinds_of("widget"), vec![IdentifierKind::TypeUsage]);
    assert_eq!(kinds_of("x"), vec![IdentifierKind::VariableRef]);
    assert_eq!(kinds_of("id"), vec![IdentifierKind::VariableRef]);
}

#[test]
fn c23_auto_call_to_another_file_is_a_pending_call() {
    let extraction = extract_all(&in_function(MAKERS, "auto w = elsewhere();"));
    assert!(
        extraction
            .pending_targets
            .contains(&(RelationshipKind::Calls, "elsewhere".to_string()))
    );
}
