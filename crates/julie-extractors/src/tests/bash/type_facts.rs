use crate::base::{Symbol, SymbolKind};
use crate::bash::BashExtractor;
use std::path::PathBuf;

fn extract(source: &str) -> Vec<Symbol> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_bash::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = BashExtractor::new(
        "bash".to_string(),
        "type_facts.sh".to_string(),
        source.to_string(),
        &workspace_root,
    );
    extractor.extract_symbols(&tree)
}

fn symbol<'a>(symbols: &'a [Symbol], name: &str, kind: SymbolKind) -> &'a Symbol {
    symbols
        .iter()
        .find(|s| s.name == name && s.kind == kind)
        .unwrap_or_else(|| panic!("missing symbol {name}"))
}

fn role(symbol: &Symbol) -> Option<&str> {
    symbol
        .metadata
        .as_ref()
        .and_then(|m| m.get("role"))
        .and_then(|v| v.as_str())
}

#[test]
fn positional_parameters_carry_role_parameter() {
    let source = r#"
greet() {
    echo "$1"
    echo "$2"
}
"#;
    let symbols = extract(source);
    let greet = symbol(&symbols, "greet", SymbolKind::Function);
    let one = symbol(&symbols, "$1", SymbolKind::Variable);
    let two = symbol(&symbols, "$2", SymbolKind::Variable);
    assert_eq!(role(one), Some("parameter"));
    assert_eq!(role(two), Some("parameter"));
    assert_eq!(one.parent_id.as_deref(), Some(greet.id.as_str()));
    assert_eq!(two.parent_id.as_deref(), Some(greet.id.as_str()));
}

#[test]
fn local_assignment_parents_to_enclosing_function() {
    let source = r#"
deploy() {
    local count=$1
}
"#;
    let symbols = extract(source);
    let deploy = symbol(&symbols, "deploy", SymbolKind::Function);
    let count = symbol(&symbols, "count", SymbolKind::Variable);
    assert_eq!(count.parent_id.as_deref(), Some(deploy.id.as_str()));
}

#[test]
fn readonly_and_exported_uppercase_are_constants() {
    let source = r#"
readonly MAX=3
export API_URL=x
"#;
    let symbols = extract(source);
    let max = symbol(&symbols, "MAX", SymbolKind::Constant);
    let api_url = symbol(&symbols, "API_URL", SymbolKind::Constant);
    assert_eq!(max.kind, SymbolKind::Constant);
    assert_eq!(api_url.kind, SymbolKind::Constant);
}
