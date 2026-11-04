use crate::base::SymbolKind;
use crate::javascript::JavaScriptExtractor;
use std::path::PathBuf;
use tree_sitter::{Parser, Tree};

fn parse_jsx(source: &str) -> Tree {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .expect("JavaScript grammar loads");
    parser.parse(source, None).expect("JSX source parses")
}

fn extract_jsx(source: &str) -> (Vec<crate::base::Symbol>, JavaScriptExtractor) {
    let tree = parse_jsx(source);
    assert!(!tree.root_node().has_error());
    let mut extractor = JavaScriptExtractor::new(
        "jsx".to_string(),
        "View.jsx".to_string(),
        source.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    (symbols, extractor)
}

#[test]
fn jsx_type_facts_keep_local_constructors_and_reject_qualified_guesses() {
    let source = r#"
class Item {}
const item = new Item();
const remote = new namespace.Item();
function View() { return <span>{item.name}</span>; }
"#;
    let (symbols, extractor) = extract_jsx(source);
    let item = symbols
        .iter()
        .find(|symbol| symbol.name == "item" && symbol.kind == SymbolKind::Variable)
        .expect("local constructor variable");
    let remote = symbols
        .iter()
        .find(|symbol| symbol.name == "remote" && symbol.kind == SymbolKind::Variable)
        .expect("qualified constructor variable");
    let fact = extractor
        .base
        .type_info
        .get(&item.id)
        .expect("local constructor type fact");

    assert_eq!(fact.resolved_type, "Item");
    assert!(fact.is_inferred);
    assert_eq!(fact.language, "jsx");
    assert!(!extractor.base.type_info.contains_key(&remote.id));
}

#[test]
fn jsx_doc_comments_attach_to_the_documented_component_only() {
    let source = r#"/** Component documentation. */
export function View() { return <span>view</span>; }
function Plain() { return <span>plain</span>; }
"#;
    let (symbols, _) = extract_jsx(source);
    let view = symbols
        .iter()
        .find(|symbol| symbol.name == "View" && symbol.kind == SymbolKind::Function)
        .expect("documented component");
    let plain = symbols
        .iter()
        .find(|symbol| symbol.name == "Plain" && symbol.kind == SymbolKind::Function)
        .expect("undocumented component");
    let documented = symbols
        .iter()
        .filter(|symbol| symbol.doc_comment.is_some())
        .map(|symbol| symbol.name.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        view.doc_comment.as_deref(),
        Some("/** Component documentation. */")
    );
    assert_eq!(plain.doc_comment, None);
    assert_eq!(documented, ["View"]);
}

#[test]
fn jsx_tag_and_attribute_names_do_not_become_class_fields() {
    let source = r#"
class Panel {
    title = "panel";
    render() { return <div aria-label={this.title}>panel</div>; }
}
function View() { return <Panel title="view" />; }
"#;
    let (symbols, _) = extract_jsx(source);
    let panel = symbols
        .iter()
        .find(|symbol| symbol.name == "Panel" && symbol.kind == SymbolKind::Class)
        .expect("Panel class");
    let title_fields = symbols
        .iter()
        .filter(|symbol| symbol.name == "title" && symbol.kind == SymbolKind::Field)
        .collect::<Vec<_>>();

    assert_eq!(title_fields.len(), 1);
    assert_eq!(
        title_fields[0].parent_id.as_deref(),
        Some(panel.id.as_str())
    );
    assert!(!symbols.iter().any(|symbol| symbol.name == "div"));
    assert!(!symbols.iter().any(|symbol| symbol.name == "aria-label"));
}
