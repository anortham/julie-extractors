use crate::base::{Symbol, SymbolKind, TypeInfo};
use crate::python::PythonExtractor;
use std::path::PathBuf;

fn extract(source: &str) -> (Vec<Symbol>, PythonExtractor) {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = PythonExtractor::new(
        "type_facts.py".to_string(),
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

fn fact<'a>(
    extractor: &'a PythonExtractor,
    symbols: &[Symbol],
    name: &str,
    kind: SymbolKind,
) -> &'a TypeInfo {
    let symbol = symbol(symbols, name, kind);
    extractor
        .base
        .type_info
        .get(&symbol.id)
        .unwrap_or_else(|| panic!("missing type fact for {name}"))
}

fn no_fact(extractor: &PythonExtractor, symbols: &[Symbol], name: &str, kind: SymbolKind) {
    let symbol = symbol(symbols, name, kind);
    assert!(
        extractor.base.type_info.get(&symbol.id).is_none(),
        "unexpected type fact for {name}"
    );
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

#[test]
fn annotated_local_records_declared_type() {
    let source = r#"
def run():
    repo: Repo = make_repo()
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, &symbols, "repo", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "Repo");
    assert!(!fact.is_inferred);
    assert_eq!(declared(fact), None);
}

#[test]
fn annotated_parameter_becomes_symbol_with_type_fact() {
    let source = r#"
class Handler:
    def handle(self, event: Event):
        pass
"#;
    let (symbols, extractor) = extract(source);
    let method = symbol(&symbols, "handle", SymbolKind::Method);
    let parameter = symbol(&symbols, "event", SymbolKind::Variable);
    assert_eq!(role(parameter), Some("parameter"));
    assert_eq!(parameter.parent_id.as_deref(), Some(method.id.as_str()));
    assert_eq!(parameter.signature.as_deref(), Some("event: Event"));
    let fact = fact(&extractor, &symbols, "event", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "Event");
    assert!(!fact.is_inferred);
}

#[test]
fn unannotated_parameter_gets_symbol_without_fact() {
    let source = r#"
def process(payload):
    pass
"#;
    let (symbols, extractor) = extract(source);
    let function = symbol(&symbols, "process", SymbolKind::Function);
    let parameter = symbol(&symbols, "payload", SymbolKind::Variable);
    assert_eq!(role(parameter), Some("parameter"));
    assert_eq!(parameter.parent_id.as_deref(), Some(function.id.as_str()));
    no_fact(&extractor, &symbols, "payload", SymbolKind::Variable);
}

#[test]
fn self_and_cls_parameters_get_symbols_without_facts() {
    let source = r#"
class Service:
    def start(self):
        pass

    @classmethod
    def build(cls):
        pass
"#;
    let (symbols, extractor) = extract(source);
    let start = symbol(&symbols, "start", SymbolKind::Method);
    let self_parameter = symbol(&symbols, "self", SymbolKind::Variable);
    assert_eq!(role(self_parameter), Some("parameter"));
    assert_eq!(self_parameter.parent_id.as_deref(), Some(start.id.as_str()));
    no_fact(&extractor, &symbols, "self", SymbolKind::Variable);
    no_fact(&extractor, &symbols, "cls", SymbolKind::Variable);
}

#[test]
fn typed_default_parameter_records_annotation() {
    let source = r#"
def retry(count: int = 3):
    pass
"#;
    let (symbols, extractor) = extract(source);
    let parameter = symbol(&symbols, "count", SymbolKind::Variable);
    assert_eq!(parameter.signature.as_deref(), Some("count: int = 3"));
    let fact = fact(&extractor, &symbols, "count", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "int");
    assert!(!fact.is_inferred);
}

#[test]
fn splat_parameters_get_symbols_without_facts() {
    let source = r#"
def collect(*items, **options):
    pass
"#;
    let (symbols, extractor) = extract(source);
    let function = symbol(&symbols, "collect", SymbolKind::Function);
    let items = symbol(&symbols, "items", SymbolKind::Variable);
    let options = symbol(&symbols, "options", SymbolKind::Variable);
    assert_eq!(items.parent_id.as_deref(), Some(function.id.as_str()));
    assert_eq!(options.parent_id.as_deref(), Some(function.id.as_str()));
    no_fact(&extractor, &symbols, "items", SymbolKind::Variable);
    no_fact(&extractor, &symbols, "options", SymbolKind::Variable);
}

#[test]
fn same_file_constructor_call_records_inferred_fact() {
    let source = r#"
class Repo:
    pass

def run():
    repo = Repo()
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, &symbols, "repo", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "Repo");
    assert!(fact.is_inferred);
}

#[test]
fn constructor_call_of_unknown_name_records_no_fact() {
    let source = r#"
def run():
    client = Missing()
"#;
    let (symbols, extractor) = extract(source);
    no_fact(&extractor, &symbols, "client", SymbolKind::Variable);
}

#[test]
fn subscripted_annotation_records_base_name() {
    let source = r#"
def run():
    items: list[Task] = []
    maybe: Optional[Task] = None
"#;
    let (symbols, extractor) = extract(source);
    let items = fact(&extractor, &symbols, "items", SymbolKind::Variable);
    assert_eq!(items.resolved_type, "list");
    assert_eq!(declared(items), Some("list[Task]"));
    let maybe = fact(&extractor, &symbols, "maybe", SymbolKind::Variable);
    assert_eq!(maybe.resolved_type, "Optional");
    assert_eq!(declared(maybe), Some("Optional[Task]"));
}

#[test]
fn union_annotation_records_no_fact() {
    let source = r#"
def run():
    value: int | None = None
"#;
    let (symbols, extractor) = extract(source);
    no_fact(&extractor, &symbols, "value", SymbolKind::Variable);
}

#[test]
fn string_annotation_records_no_fact() {
    let source = r#"
def run():
    ref: "Repo" = None
"#;
    let (symbols, extractor) = extract(source);
    no_fact(&extractor, &symbols, "ref", SymbolKind::Variable);
}

#[test]
fn dotted_annotation_records_as_written() {
    let source = r#"
def run():
    cfg: config.Settings = load()
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, &symbols, "cfg", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "config.Settings");
    assert!(!fact.is_inferred);
}

#[test]
fn annotated_class_attribute_records_fact() {
    let source = r#"
class Config:
    name: str = ""
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, &symbols, "name", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "str");
    assert!(!fact.is_inferred);
}

#[test]
fn annotated_self_attribute_records_fact() {
    let source = r#"
class Service:
    def __init__(self):
        self.repo: Repo = Repo()
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, &symbols, "repo", SymbolKind::Property);
    assert_eq!(fact.resolved_type, "Repo");
    assert!(!fact.is_inferred);
}

#[test]
fn annotated_parameter_of_same_name_wins_over_local_reuse() {
    let source = r#"
def run(task: Task):
    pass
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, &symbols, "task", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "Task");
    assert!(!fact.is_inferred);
}
