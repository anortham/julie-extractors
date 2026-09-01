use crate::base::{IdentifierKind, Symbol, SymbolKind, TypeInfo};
use crate::kotlin::KotlinExtractor;
use std::path::PathBuf;

fn extract(source: &str) -> (Vec<Symbol>, KotlinExtractor) {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_kotlin_ng::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = KotlinExtractor::new(
        "kotlin".to_string(),
        "type_facts.kt".to_string(),
        source.to_string(),
        &workspace_root,
    );
    let symbols = extractor.extract_symbols(&tree);
    (symbols, extractor)
}

fn extract_calls(source: &str) -> (Vec<Symbol>, KotlinExtractor) {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_kotlin_ng::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = KotlinExtractor::new(
        "kotlin".to_string(),
        "type_facts.kt".to_string(),
        source.to_string(),
        &workspace_root,
    );
    let symbols = extractor.extract_symbols(&tree);
    extractor.extract_identifiers(&tree, &symbols);
    extractor.extract_relationships(&tree, &symbols);
    (symbols, extractor)
}

fn symbol<'a>(symbols: &'a [Symbol], name: &str, kind: SymbolKind) -> &'a Symbol {
    symbols
        .iter()
        .find(|s| s.name == name && s.kind == kind)
        .unwrap_or_else(|| panic!("missing symbol {name}"))
}

fn fact<'a>(
    extractor: &'a KotlinExtractor,
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

fn no_fact(extractor: &KotlinExtractor, symbols: &[Symbol], name: &str, kind: SymbolKind) {
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
fn typed_parameter_becomes_symbol_with_declared_fact() {
    let source = r#"
class Sample {
    fun run(job: Job, count: Int) {
    }
}
"#;
    let (symbols, extractor) = extract(source);
    let method = symbol(&symbols, "run", SymbolKind::Method);
    let job = symbol(&symbols, "job", SymbolKind::Variable);
    assert_eq!(job.parent_id.as_deref(), Some(method.id.as_str()));
    assert_eq!(role(job), Some("parameter"));
    let job_fact = fact(&extractor, &symbols, "job", SymbolKind::Variable);
    assert_eq!(job_fact.resolved_type, "Job");
    assert!(!job_fact.is_inferred);
    let count_fact = fact(&extractor, &symbols, "count", SymbolKind::Variable);
    assert_eq!(count_fact.resolved_type, "Int");
    assert!(!count_fact.is_inferred);
}

#[test]
fn generic_parameter_records_base_name() {
    let source = r#"
class Sample {
    fun run(index: List<Job>) {
    }
}
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact(&extractor, &symbols, "index", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "List");
    assert!(!fact.is_inferred);
    assert_eq!(declared(fact), Some("List<Job>"));
}

#[test]
fn nullable_local_records_base_name_as_variable() {
    let source = r#"
class Sample {
    fun run() {
        val item: Job? = null
    }
}
"#;
    let (symbols, extractor) = extract(source);
    let method = symbol(&symbols, "run", SymbolKind::Method);
    let item = symbol(&symbols, "item", SymbolKind::Variable);
    assert_eq!(item.parent_id.as_deref(), Some(method.id.as_str()));
    let fact = fact(&extractor, &symbols, "item", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "Job");
    assert!(!fact.is_inferred);
    assert_eq!(declared(fact), Some("Job?"));
}

#[test]
fn same_file_constructor_local_records_inferred_fact() {
    let source = r#"
class Repo

fun run() {
    val repo = Repo()
}
"#;
    let (symbols, extractor) = extract(source);
    let function = symbol(&symbols, "run", SymbolKind::Function);
    let repo = symbol(&symbols, "repo", SymbolKind::Variable);
    assert_eq!(repo.parent_id.as_deref(), Some(function.id.as_str()));
    let fact = fact(&extractor, &symbols, "repo", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "Repo");
    assert!(fact.is_inferred);
}

#[test]
fn negative_locals_record_symbol_without_fact() {
    let source = r#"
fun run() {
    val missing = Unknown()
    val numbers = listOf(1)
    val remote = com.acme.Foo()
}
"#;
    let (symbols, extractor) = extract(source);
    no_fact(&extractor, &symbols, "missing", SymbolKind::Variable);
    no_fact(&extractor, &symbols, "numbers", SymbolKind::Variable);
    no_fact(&extractor, &symbols, "remote", SymbolKind::Variable);
}

#[test]
fn class_property_records_declared_fact_and_keeps_kind() {
    let source = r#"
class Sample {
    val index: List<Job> = emptyList()
    const val name: String = "n"
}
"#;
    let (symbols, extractor) = extract(source);
    let index = symbol(&symbols, "index", SymbolKind::Property);
    let index_fact = fact(&extractor, &symbols, "index", SymbolKind::Property);
    assert_eq!(index_fact.resolved_type, "List");
    assert!(!index_fact.is_inferred);
    assert_eq!(declared(index_fact), Some("List<Job>"));
    assert_eq!(index.kind, SymbolKind::Property);
    let name_fact = fact(&extractor, &symbols, "name", SymbolKind::Constant);
    assert_eq!(name_fact.resolved_type, "String");
    assert!(!name_fact.is_inferred);
}

#[test]
fn primary_constructor_property_records_declared_fact() {
    let source = r#"
class Sample(val job: Job, count: Int)
"#;
    let (symbols, extractor) = extract(source);
    let class = symbol(&symbols, "Sample", SymbolKind::Class);
    let job = symbol(&symbols, "job", SymbolKind::Property);
    assert_eq!(job.parent_id.as_deref(), Some(class.id.as_str()));
    assert_ne!(role(job), Some("parameter"));
    let job_fact = fact(&extractor, &symbols, "job", SymbolKind::Property);
    assert_eq!(job_fact.resolved_type, "Job");
    assert!(!job_fact.is_inferred);
    assert!(
        !symbols
            .iter()
            .any(|s| s.name == "job" && s.kind == SymbolKind::Variable)
    );
}

#[test]
fn this_and_super_calls_record_receiver_type_on_identifier_and_pending() {
    let source = r#"
open class ServiceBase
class OrderService : ServiceBase() {
    fun process(other: Worker) {
        this.persist()
        super.restore()
        other.fetch()
    }
}
class Solo {
    fun run() {
        super.absent()
    }
}
"#;
    let (_symbols, extractor) = extract_calls(source);
    let call = |name: &str| {
        extractor
            .base
            .identifiers
            .iter()
            .find(|id| id.name == name && id.kind == IdentifierKind::Call)
            .unwrap_or_else(|| panic!("missing call identifier {name}"))
    };
    assert_eq!(
        call("persist").receiver_type.as_deref(),
        Some("OrderService")
    );
    assert_eq!(
        call("restore").receiver_type.as_deref(),
        Some("ServiceBase")
    );
    assert_eq!(call("fetch").receiver_type, None);
    assert_eq!(call("absent").receiver_type, None);

    let pending = |name: &str| {
        extractor
            .get_structured_pending_relationships()
            .into_iter()
            .find(|p| p.target.terminal_name == name)
            .unwrap_or_else(|| panic!("missing structured pending for {name}"))
    };
    assert_eq!(
        pending("persist").receiver_type.as_deref(),
        Some("OrderService")
    );
    assert_eq!(
        pending("restore").receiver_type.as_deref(),
        Some("ServiceBase")
    );
    assert_eq!(pending("fetch").receiver_type, None);
    assert_eq!(pending("absent").receiver_type, None);
}
