use crate::base::SymbolKind;
use crate::typescript::TypeScriptExtractor;
use std::path::PathBuf;
use tree_sitter::{Parser, Tree};

fn parse_tsx(source: &str) -> Tree {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TSX.into())
        .expect("TSX grammar loads");
    parser.parse(source, None).expect("TSX source parses")
}

fn extract_tsx(source: &str) -> (Vec<crate::base::Symbol>, TypeScriptExtractor) {
    let tree = parse_tsx(source);
    assert!(!tree.root_node().has_error());
    let mut extractor = TypeScriptExtractor::new(
        "tsx".to_string(),
        "View.tsx".to_string(),
        source.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    (symbols, extractor)
}

#[test]
fn tsx_type_facts_keep_declared_types_and_leave_unresolved_values_unknown() {
    let source = r#"
type Props = { label: string };
const props: Props = { label: "item" };
const unknown = remoteValue();
function View(props: Props) { return <span>{props.label}</span>; }
"#;
    let (symbols, extractor) = extract_tsx(source);
    let props = symbols
        .iter()
        .find(|symbol| symbol.name == "props" && symbol.kind == SymbolKind::Variable)
        .expect("typed props variable");
    let unknown = symbols
        .iter()
        .find(|symbol| symbol.name == "unknown" && symbol.kind == SymbolKind::Variable)
        .expect("unresolved variable");
    let fact = extractor
        .base
        .type_info
        .get(&props.id)
        .expect("props type fact");

    assert_eq!(fact.resolved_type, "Props");
    assert!(!fact.is_inferred);
    assert_eq!(fact.language, "tsx");
    assert!(!extractor.base.type_info.contains_key(&unknown.id));
}

#[test]
fn tsx_doc_comments_attach_to_their_component_only() {
    let source = r#"/** Component documentation. */
export function View() { return <span>view</span>; }
function Plain() { return <span>plain</span>; }
"#;
    let (symbols, _) = extract_tsx(source);
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
fn tsx_tag_and_attribute_names_do_not_become_class_members() {
    let source = r#"
class Panel {
    title = "panel";
    render() { return <div aria-label={this.title}>panel</div>; }
}
function View() { return <Panel title="view" />; }
"#;
    let (symbols, _) = extract_tsx(source);
    let panel = symbols
        .iter()
        .find(|symbol| symbol.name == "Panel" && symbol.kind == SymbolKind::Class)
        .expect("Panel class");
    let title_fields = symbols
        .iter()
        .filter(|symbol| symbol.name == "title" && symbol.kind == SymbolKind::Property)
        .collect::<Vec<_>>();

    assert_eq!(title_fields.len(), 1);
    assert_eq!(
        title_fields[0].parent_id.as_deref(),
        Some(panel.id.as_str())
    );
    assert!(!symbols.iter().any(|symbol| symbol.name == "div"));
    assert!(!symbols.iter().any(|symbol| symbol.name == "aria-label"));
}
