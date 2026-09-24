use crate::base::{Symbol, SymbolKind, TypeInfo};
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
        "initializer_types.kt".to_string(),
        source.to_string(),
        &workspace_root,
    );
    let symbols = extractor.extract_symbols(&tree);
    (symbols, extractor)
}

fn fact_for<'a>(
    extractor: &'a KotlinExtractor,
    symbols: &[Symbol],
    name: &str,
) -> Option<&'a TypeInfo> {
    let symbol = symbols
        .iter()
        .find(|s| s.name == name && matches!(s.kind, SymbolKind::Variable | SymbolKind::Property))
        .unwrap_or_else(|| panic!("missing symbol {name}"));
    extractor.base.type_info.get(&symbol.id)
}

fn declared(fact: &TypeInfo) -> &str {
    fact.metadata
        .as_ref()
        .and_then(|m| m.get("declared"))
        .and_then(|v| v.as_str())
        .unwrap_or(&fact.resolved_type)
}

fn assert_inferred(source: &str, name: &str, resolved: &str, declared_text: &str) {
    let (symbols, extractor) = extract(source);
    let fact = fact_for(&extractor, &symbols, name)
        .unwrap_or_else(|| panic!("missing type fact for {name}"));
    assert_eq!(fact.resolved_type, resolved, "resolved type of {name}");
    assert!(fact.is_inferred, "{name} should be inferred");
    assert_eq!(declared(fact), declared_text, "declared text of {name}");
}

fn assert_no_fact(source: &str, name: &str) {
    let (symbols, extractor) = extract(source);
    assert!(
        fact_for(&extractor, &symbols, name).is_none(),
        "unexpected type fact for {name}"
    );
}

#[test]
fn top_level_function_call_records_declared_return_type() {
    let source = r#"
class Repo
fun load(): Repo = Repo()
fun run() {
    val repo = load()
}
"#;
    assert_inferred(source, "repo", "Repo", "Repo");
}

#[test]
fn block_body_function_and_var_record_declared_return_type() {
    let source = r#"
class Repo
fun open(path: String): Repo {
    return Repo()
}
fun run() {
    var repo = open("x")
}
"#;
    assert_inferred(source, "repo", "Repo", "Repo");
}

#[test]
fn nullable_return_records_base_type_and_not_null_assertion_drops_the_marker() {
    let source = r#"
class Repo
fun find(): Repo? = null
fun run() {
    val maybe = find()
    val sure = find()!!
}
"#;
    assert_inferred(source, "maybe", "Repo", "Repo?");
    assert_inferred(source, "sure", "Repo", "Repo");
}

#[test]
fn generic_return_type_records_its_base_name() {
    let source = r#"
class Repo
fun all(): List<Repo> = emptyList()
fun run() {
    val repos = all()
}
"#;
    assert_inferred(source, "repos", "List", "List<Repo>");
}

#[test]
fn parenthesized_call_and_trailing_lambda_call_record_return_type() {
    let source = r#"
class Repo
fun build(size: Int, block: () -> Unit): Repo = Repo()
fun load(): Repo = Repo()
fun run() {
    val wrapped = (load())
    val built = build(1) { }
}
"#;
    assert_inferred(source, "wrapped", "Repo", "Repo");
    assert_inferred(source, "built", "Repo", "Repo");
}

#[test]
fn this_call_and_bare_member_call_use_the_enclosing_class_method() {
    let source = r#"
class Repo
class Service {
    fun repo(): Repo = Repo()
    fun run() {
        val viaThis = this.repo()
        val bare = repo()
    }
}
"#;
    assert_inferred(source, "viaThis", "Repo", "Repo");
    assert_inferred(source, "bare", "Repo", "Repo");
}

#[test]
fn companion_and_object_calls_through_the_type_name_record_return_type() {
    let source = r#"
class Repo {
    companion object {
        fun create(): Repo = Repo()
    }
}
object Registry {
    fun make(): Repo = Repo()
}
fun run() {
    val created = Repo.create()
    val made = Registry.make()
}
"#;
    assert_inferred(source, "created", "Repo", "Repo");
    assert_inferred(source, "made", "Repo", "Repo");
}

#[test]
fn local_function_declared_before_the_call_records_return_type() {
    let source = r#"
class Repo
fun run() {
    fun local(): Repo = Repo()
    val repo = local()
}
"#;
    assert_inferred(source, "repo", "Repo", "Repo");
}

#[test]
fn class_property_initializer_records_return_type() {
    let source = r#"
class Repo
fun load(): Repo = Repo()
class Holder {
    val repo = load()
}
"#;
    assert_inferred(source, "repo", "Repo", "Repo");
}

#[test]
fn agreeing_overloads_record_the_shared_return_type() {
    let source = r#"
class Repo
fun load(): Repo = Repo()
fun load(id: Int): Repo = Repo()
fun run() {
    val repo = load(1)
}
"#;
    assert_inferred(source, "repo", "Repo", "Repo");
}

#[test]
fn written_type_wins_over_the_call_return_type() {
    let source = r#"
open class Base
class Repo : Base()
fun load(): Repo = Repo()
fun run() {
    val repo: Base = load()
}
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact_for(&extractor, &symbols, "repo").unwrap();
    assert_eq!(fact.resolved_type, "Base");
    assert!(!fact.is_inferred);
}

#[test]
fn constructor_call_still_records_the_class() {
    let source = r#"
class Repo
fun run() {
    val repo = Repo()
    val lambda = Repo { }
}
"#;
    assert_inferred(source, "repo", "Repo", "Repo");
    assert_inferred(source, "lambda", "Repo", "Repo");
}

#[test]
fn call_to_a_function_outside_the_file_records_nothing() {
    let source = r#"
fun run() {
    val remote = fetchRemote()
}
"#;
    assert_no_fact(source, "remote");
}

#[test]
fn chain_that_ends_in_another_method_records_nothing() {
    let source = r#"
class Repo
fun load(): Repo = Repo()
fun run() {
    val copied = load().copy()
    val mapped = load()?.let { it }
    val elvis = load() ?: return
}
"#;
    assert_no_fact(source, "copied");
    assert_no_fact(source, "mapped");
    assert_no_fact(source, "elvis");
}

#[test]
fn type_parameter_return_records_nothing() {
    let source = r#"
fun <T> make(): T = TODO()
class Box<T> {
    fun get(): T = TODO()
    fun maybe(): T? = null
    fun run() {
        val got = this.get()
        val nullable = maybe()
    }
}
fun run() {
    val made = make<String>()
}
"#;
    assert_no_fact(source, "made");
    assert_no_fact(source, "got");
    assert_no_fact(source, "nullable");
}

#[test]
fn function_without_declared_return_type_records_nothing() {
    let source = r#"
class Repo
fun make() = Repo()
fun run() {
    val made = make()
}
"#;
    assert_no_fact(source, "made");
}

#[test]
fn disagreeing_same_named_candidates_record_nothing() {
    let source = r#"
class A
class B
fun load(): A = A()
fun load(id: Int): B = B()
class Service {
    fun find(): A = A()
    fun run() {
        val found = find()
    }
}
fun find(): B = B()
fun run() {
    val loaded = load()
}
"#;
    assert_no_fact(source, "loaded");
    assert_no_fact(source, "found");
}

#[test]
fn call_whose_argument_count_no_candidate_accepts_records_nothing() {
    let source = r#"
class Repo
fun load(id: Int): Repo = Repo()
fun more(vararg ids: Int): Repo = Repo()
fun optional(id: Int = 0): Repo = Repo()
fun run() {
    val none = load()
    val many = more(1, 2, 3)
    val defaulted = optional()
}
"#;
    assert_no_fact(source, "none");
    assert_inferred(source, "many", "Repo", "Repo");
    assert_inferred(source, "defaulted", "Repo", "Repo");
}

#[test]
fn explicitly_imported_name_records_nothing() {
    let source = r#"
import com.acme.load
import com.acme.Registry
class Repo
fun load(): Repo = Repo()
object Registry {
    fun make(): Repo = Repo()
}
fun run() {
    val loaded = load()
    val made = Registry.make()
}
"#;
    assert_no_fact(source, "loaded");
    assert_no_fact(source, "made");
}

#[test]
fn nested_object_out_of_scope_at_the_call_records_nothing() {
    let source = r#"
class Repo
class Holder {
    object Registry {
        fun make(): Repo = Repo()
    }
}
fun run() {
    val made = Registry.make()
}
"#;
    assert_no_fact(source, "made");
}

#[test]
fn call_inside_a_lambda_records_nothing() {
    let source = r#"
class Repo
fun load(): Repo = Repo()
class Service {
    fun repo(): Repo = Repo()
    fun run(items: List<Int>) {
        items.forEach { val bare = load() }
        items.apply { val viaThis = this.repo() }
    }
}
"#;
    assert_no_fact(source, "bare");
    assert_no_fact(source, "viaThis");
}

#[test]
fn call_inside_an_extension_function_records_nothing() {
    let source = r#"
class Repo
fun load(): Repo = Repo()
class Service {
    fun repo(): Repo = Repo()
    fun Other.work() {
        val bare = load()
        val viaThis = this.repo()
    }
}
"#;
    assert_no_fact(source, "bare");
    assert_no_fact(source, "viaThis");
}

#[test]
fn class_with_a_supertype_hides_outer_candidates_but_keeps_its_own() {
    let source = r#"
class Repo
fun load(): Repo = Repo()
class Service : Base() {
    fun own(): Repo = Repo()
    fun run() {
        val outer = load()
        val mine = own()
    }
}
"#;
    assert_no_fact(source, "outer");
    assert_inferred(source, "mine", "Repo", "Repo");
}

#[test]
fn labelled_this_and_companion_method_through_this_record_nothing() {
    let source = r#"
class A
class B
class Outer {
    fun load(): A = A()
    inner class Inner {
        fun load(): B = B()
        fun run() {
            val labelled = this@Outer.load()
        }
    }
    fun run() {
        val companion = this.create()
    }
    companion object {
        fun create(): Outer = Outer()
    }
}
"#;
    assert_no_fact(source, "labelled");
    assert_no_fact(source, "companion");
}

#[test]
fn local_function_declared_after_the_call_or_elsewhere_records_nothing() {
    let source = r#"
class Repo
fun first() {
    val early = local()
    fun local(): Repo = Repo()
}
fun second() {
    val elsewhere = local()
}
"#;
    assert_no_fact(source, "early");
    assert_no_fact(source, "elsewhere");
}

#[test]
fn destructuring_declaration_records_nothing() {
    let source = r#"
data class Pair2(val a: Int, val b: Int)
fun load(): Pair2 = Pair2(1, 2)
fun run() {
    val (a, b) = load()
}
"#;
    let (_, extractor) = extract(source);
    assert!(
        extractor
            .base
            .type_info
            .values()
            .all(|fact| !fact.is_inferred),
        "unexpected inferred fact in {:?}",
        extractor.base.type_info
    );
}
