use crate::base::{Identifier, IdentifierKind, Relationship, Symbol, SymbolKind};
use crate::regex::RegexExtractor;
use crate::tests::helpers::init_parser;
use std::path::PathBuf;

fn extract(code: &str) -> (Vec<Symbol>, Vec<Relationship>, Vec<Identifier>) {
    let workspace_root = PathBuf::from("/tmp/test");
    let tree = init_parser(code, "regex");
    let mut extractor = RegexExtractor::new(
        "regex".to_string(),
        "test.regex".to_string(),
        code.to_string(),
        &workspace_root,
    );
    let symbols = extractor.extract_symbols(&tree);
    let relationships = extractor.extract_relationships(&tree, &symbols);
    let identifiers = extractor.extract_identifiers(&tree, &symbols);
    (symbols, relationships, identifiers)
}

fn roots(symbols: &[Symbol]) -> Vec<&Symbol> {
    symbols
        .iter()
        .filter(|symbol| symbol.parent_id.is_none() && metadata_type(symbol) == "regex-pattern")
        .collect()
}

fn metadata_type(symbol: &Symbol) -> &str {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("type"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
}

fn body_text<'a>(code: &'a str, symbol: &Symbol) -> Option<&'a str> {
    let body = symbol.body_span?;
    Some(&code[body.start_byte as usize..body.end_byte as usize])
}

#[test]
fn root_pattern_kind_is_variable_regardless_of_content_or_trailing_newline() {
    for code in [
        "(GET|POST|PUT)",
        "(GET|POST|PUT)\n",
        "foo(?=bar)\n",
        "^\\d{4}$\n",
        "[a-z]",
    ] {
        let (symbols, _, _) = extract(code);
        let roots = roots(&symbols);
        assert_eq!(roots.len(), 1, "{code:?}: {symbols:#?}");
        assert_eq!(roots[0].kind, SymbolKind::Variable, "{code:?}");
        assert_eq!(roots[0].name, code.trim_end_matches('\n'), "{code:?}");
        assert!(!roots[0].name.ends_with('\n'));
    }
}

#[test]
fn single_group_file_keeps_root_and_named_group_symbols() {
    let (symbols, _, _) = extract("(?<word>\\w+)");
    assert_eq!(roots(&symbols).len(), 1, "{symbols:#?}");
    let group = symbols
        .iter()
        .find(|symbol| symbol.name == "word")
        .expect("named group symbol");
    assert_eq!(group.kind, SymbolKind::Function);
}

#[test]
fn named_group_symbol_takes_the_bare_group_name() {
    let code = "^(?<slug>[\\w-]+)/\\k<slug>$";
    let (symbols, _, identifiers) = extract(code);
    let group = symbols
        .iter()
        .find(|symbol| symbol.kind == SymbolKind::Function)
        .expect("named group");
    assert_eq!(group.name, "slug");
    assert!(
        group
            .signature
            .as_deref()
            .unwrap()
            .contains("(?<slug>[\\w-]+)")
    );
    let definition = identifiers
        .iter()
        .find(|identifier| identifier.kind == IdentifierKind::MemberAccess)
        .expect("definition identifier");
    assert_eq!(definition.name, "slug");
    assert_eq!(
        &code[definition.start_byte as usize..definition.end_byte as usize],
        "slug"
    );
}

#[test]
fn backreference_directly_after_its_group_references_from_the_pattern() {
    for code in ["(\\w)\\1", "(?<word>\\w+)\\k<word>"] {
        let (symbols, relationships, identifiers) = extract(code);
        let root = roots(&symbols)[0];
        let group = symbols
            .iter()
            .find(|symbol| symbol.kind == SymbolKind::Function)
            .expect("group");
        assert_eq!(relationships.len(), 1, "{code:?}: {relationships:#?}");
        assert_eq!(relationships[0].from_symbol_id, root.id, "{code:?}");
        assert_eq!(relationships[0].to_symbol_id, group.id, "{code:?}");
        for call in identifiers
            .iter()
            .filter(|identifier| identifier.kind == IdentifierKind::Call)
        {
            assert_eq!(call.containing_symbol_id.as_ref(), Some(&root.id));
        }
    }
}

#[test]
fn adjacent_named_group_identifier_binds_to_its_own_group() {
    let code = "(?<host>[^/\\s]+)(?<path>/[^\\s?#]*)?$";
    let (symbols, _, identifiers) = extract(code);
    let path_group = symbols.iter().find(|symbol| symbol.name == "path").unwrap();
    let path_ident = identifiers
        .iter()
        .find(|identifier| identifier.name == "path")
        .unwrap();
    assert_eq!(
        path_ident.containing_symbol_id.as_ref(),
        Some(&path_group.id)
    );
}

#[test]
fn body_spans_cover_the_symbol_node_not_quantifier_braces() {
    let code = "^\\d{3}-(?<line>\\d{4})$";
    let (symbols, _, _) = extract(code);
    let root = roots(&symbols)[0];
    assert_eq!(body_text(code, root), Some(code));
    let group = symbols.iter().find(|symbol| symbol.name == "line").unwrap();
    assert_eq!(body_text(code, group), Some("(?<line>\\d{4})"));
    assert!(group.body_hash.is_some());

    let class_code = "[\\p{Ll}\\p{M}'-]+";
    let (symbols, _, _) = extract(class_code);
    let class = symbols
        .iter()
        .find(|symbol| symbol.kind == SymbolKind::Class)
        .unwrap();
    assert_eq!(class.body_span, None);
}

#[test]
fn each_line_of_a_pattern_list_is_its_own_pattern() {
    let code = "^(\\d{4})-(\\d{2})-(\\d{2})$\n^(\\w+)\\s+\\1$\n\n^[a-z]+|[0-9]+$\n";
    let (symbols, relationships, _) = extract(code);
    let roots = roots(&symbols);
    let names: Vec<_> = roots.iter().map(|root| root.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "^(\\d{4})-(\\d{2})-(\\d{2})$",
            "^(\\w+)\\s+\\1$",
            "^[a-z]+|[0-9]+$"
        ]
    );
    assert_eq!(roots[1].start_line, 2);
    assert_eq!(roots[1].end_line, 2);
    let word_group = symbols
        .iter()
        .find(|symbol| symbol.name == "(\\w+)")
        .expect("line 2 group is referenced by line 2 backreference");
    assert_eq!(
        word_group
            .metadata
            .as_ref()
            .unwrap()
            .get("captureIndex")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
    assert_eq!(relationships.len(), 1, "{relationships:#?}");
    assert_eq!(relationships[0].from_symbol_id, roots[1].id);
    assert_eq!(relationships[0].to_symbol_id, word_group.id);
    assert!(
        !symbols.iter().any(|symbol| symbol.name == "(\\d{4})"),
        "line 1 groups are not referenced: {symbols:#?}"
    );
}

#[test]
fn verbose_flag_pattern_over_many_lines_stays_one_pattern() {
    let code = "(?x)\n^(?<year>\\d{4})\n-(?<month>\\d{2})$\n";
    let (symbols, _, _) = extract(code);
    let roots = roots(&symbols);
    assert_eq!(roots.len(), 1, "{symbols:#?}");
    assert_eq!(roots[0].name, "(?x)");
    assert_eq!(roots[0].end_line, 3);
}
