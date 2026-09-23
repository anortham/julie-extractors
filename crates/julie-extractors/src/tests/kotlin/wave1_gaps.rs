use crate::ExtractionResults;
use crate::base::RelationshipKind;
use crate::extract_canonical;
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("kotlin extraction")
}

fn symbol_name(result: &ExtractionResults, id: &str) -> String {
    result
        .symbols
        .iter()
        .find(|symbol| symbol.id == id)
        .map(|symbol| symbol.name.clone())
        .unwrap_or_default()
}

fn resolved(result: &ExtractionResults, kind: RelationshipKind) -> Vec<(String, String)> {
    result
        .relationships
        .iter()
        .filter(|relationship| relationship.kind == kind)
        .map(|relationship| {
            (
                symbol_name(result, &relationship.from_symbol_id),
                symbol_name(result, &relationship.to_symbol_id),
            )
        })
        .collect()
}

fn pending(result: &ExtractionResults, kind: RelationshipKind) -> Vec<(String, String, String)> {
    result
        .structured_pending_relationships
        .iter()
        .filter(|pending| pending.pending.kind == kind)
        .map(|pending| {
            (
                symbol_name(result, &pending.pending.from_symbol_id),
                pending.target.terminal_name.clone(),
                pending.target.receiver.clone().unwrap_or_default(),
            )
        })
        .collect()
}

fn pair(from: &str, to: &str) -> (String, String) {
    (from.to_string(), to.to_string())
}

fn triple(from: &str, terminal: &str, receiver: &str) -> (String, String, String) {
    (from.to_string(), terminal.to_string(), receiver.to_string())
}

fn symbol<'a>(result: &'a ExtractionResults, name: &str) -> &'a crate::base::Symbol {
    result
        .symbols
        .iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("missing symbol {name}"))
}

fn body_text<'a>(source: &'a str, symbol: &crate::base::Symbol) -> &'a str {
    let span = symbol.body_span.expect("body span");
    &source[span.start_byte as usize..span.end_byte as usize]
}

fn annotation_keys(symbol: &crate::base::Symbol) -> Vec<String> {
    symbol
        .annotations
        .iter()
        .map(|a| a.annotation_key.clone())
        .collect()
}

#[test]
fn function_body_spans_cover_the_function_body_node() {
    let source = r#"class UserController(private val service: UserService) {
    @GetMapping("/{id}")
    fun get(@PathVariable id: Long): User {
        return service.find(id)
    }
    @GetMapping("/{id}/orders")
    fun orders(id: Long): List<Order> = service.orders(id)
    fun names(users: List<User>) = users.map { it.name }
    fun abs(a: Int) = if (a > 0) a else -a
    fun render(onClick: () -> Unit = {}) { onClick() }
}
"#;
    let result = extract("Bodies.kt", source);
    assert_eq!(
        body_text(source, symbol(&result, "get")),
        "{\n        return service.find(id)\n    }"
    );
    assert_eq!(
        body_text(source, symbol(&result, "orders")),
        "service.orders(id)"
    );
    assert_eq!(
        body_text(source, symbol(&result, "names")),
        "users.map { it.name }"
    );
    assert_eq!(
        body_text(source, symbol(&result, "abs")),
        "if (a > 0) a else -a"
    );
    assert_eq!(
        body_text(source, symbol(&result, "render")),
        "{ onClick() }"
    );
    let abs_id = symbol(&result, "abs").id.clone();
    let abs_metric = result
        .complexity_metrics
        .iter()
        .find(|m| m.symbol_id.as_deref() == Some(abs_id.as_str()))
        .expect("abs complexity");
    assert_eq!(abs_metric.decision_count, 1);
}

#[test]
fn top_level_annotated_declarations_keep_annotations_docs_and_members() {
    let source = r#"package com.example

fun first() {}

/** Dev-only beans. */
@Component
@Profile("dev")
class DevConfig {
    fun bean() {}
}

/** Deprecated helper. */
@Deprecated("use other")
fun old() {}

@Keep
enum class Mode { A }

@Retention(AnnotationRetention.RUNTIME)
@Target(AnnotationTarget.FUNCTION)
annotation class FromJson

@R
@M("/a")
class Swallowed {}

@RestController
@RequestMapping("/api/users")
class UserController(private val service: String) {
    fun list() = service
}

class Tail {
    fun t() = 1
}
"#;
    let result = extract("Annotated.kt", source);

    let dev = symbol(&result, "DevConfig");
    assert_eq!(annotation_keys(dev), ["component", "profile"]);
    assert_eq!(dev.doc_comment.as_deref(), Some("/** Dev-only beans. */"));
    assert_eq!(dev.start_line, 6);
    assert_eq!(
        symbol(&result, "bean").parent_id.as_deref(),
        Some(dev.id.as_str())
    );

    let old = symbol(&result, "old");
    assert_eq!(annotation_keys(old), ["deprecated"]);
    assert_eq!(
        old.doc_comment.as_deref(),
        Some("/** Deprecated helper. */")
    );
    assert_eq!(
        old.signature.as_deref(),
        Some("@Deprecated(\"use other\") fun old()")
    );

    let mode = symbol(&result, "Mode");
    assert_eq!(mode.kind, crate::base::SymbolKind::Enum);
    assert_eq!(annotation_keys(mode), ["keep"]);
    assert_eq!(
        symbol(&result, "A").parent_id.as_deref(),
        Some(mode.id.as_str())
    );

    assert_eq!(
        annotation_keys(symbol(&result, "FromJson")),
        ["retention", "target"]
    );
    assert_eq!(annotation_keys(symbol(&result, "Swallowed")), ["r", "m"]);
    let controller = symbol(&result, "UserController");
    assert_eq!(
        annotation_keys(controller),
        ["restcontroller", "requestmapping"]
    );
    assert_eq!(controller.start_line, 27);
    assert!(symbol(&result, "Tail").parent_id.is_none());

    assert!(
        !result
            .identifiers
            .iter()
            .any(|i| matches!(i.name.as_str(), "class" | "enum" | "annotation")),
        "declaration keywords leaked into identifiers"
    );
    assert!(
        result
            .identifiers
            .iter()
            .any(|i| i.name == "RequestMapping" && i.kind == crate::base::IdentifierKind::TypeUsage)
    );
    assert!(
        result
            .structural_facts
            .iter()
            .filter(|f| f.pattern_id == "kotlin.annotation.v1")
            .all(|f| f.containing_symbol_id.is_some()),
        "every annotation fact needs its declaration"
    );
}

#[test]
fn supertype_targets_drop_type_arguments_and_split_qualified_names() {
    let result = extract(
        "Supertypes.kt",
        r#"interface Repository<T> {
    fun find(id: Int): T?
}
abstract class BaseViewModel<S>
class User
class UserRepo : Repository<User> {
    override fun find(id: Int): User? = null
}
class UserVm : BaseViewModel<String>()
class StringAdapter : JsonAdapter<String>(), JsonAdapter.Factory, Comparable<StringAdapter>, Handler by inner
"#,
    );
    assert!(
        resolved(&result, RelationshipKind::Implements).contains(&pair("UserRepo", "Repository"))
    );
    assert!(
        resolved(&result, RelationshipKind::Extends).contains(&pair("UserVm", "BaseViewModel"))
    );
    let extends = pending(&result, RelationshipKind::Extends);
    let implements = pending(&result, RelationshipKind::Implements);
    assert!(
        extends.contains(&triple("StringAdapter", "JsonAdapter", "")),
        "{extends:?}"
    );
    assert!(
        implements.contains(&triple("StringAdapter", "Factory", "JsonAdapter")),
        "{implements:?}"
    );
    assert!(
        implements.contains(&triple("StringAdapter", "Comparable", "")),
        "{implements:?}"
    );
    assert!(
        implements.contains(&triple("StringAdapter", "Handler", "")),
        "{implements:?}"
    );
}

#[test]
fn same_named_nested_classes_own_their_inheritance_edges() {
    let result = extract(
        "States.kt",
        r#"sealed class LoginState {
    object Loading : LoginState()
    data class Error(val msg: String) : LoginState()
}
sealed class ProfileState {
    object Loading : ProfileState()
    data class Error(val code: Int) : ProfileState()
}
"#,
    );
    let edges: Vec<(u32, String)> = result
        .relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::Extends)
        .map(|r| {
            let from = result
                .symbols
                .iter()
                .find(|s| s.id == r.from_symbol_id)
                .unwrap();
            (from.start_line, symbol_name(&result, &r.to_symbol_id))
        })
        .collect();
    for expected in [
        (2, "LoginState"),
        (3, "LoginState"),
        (6, "ProfileState"),
        (7, "ProfileState"),
    ] {
        assert!(
            edges.contains(&(expected.0, expected.1.to_string())),
            "{edges:?}"
        );
    }
}

#[test]
fn top_level_property_initializers_own_their_calls() {
    let result = extract(
        "Props.kt",
        r#"val appModule = module { single { createRepo() } }
val handler: (String) -> Unit = { msg -> logMessage(msg) }
private val config by lazy { loadConfig() }
val greeting: String
    get() = buildGreeting()
fun createRepo(): Any = Any()
fun logMessage(m: String) {}
fun loadConfig(): String = ""
fun buildGreeting(): String = ""
"#,
    );
    let calls = resolved(&result, RelationshipKind::Calls);
    for (from, to) in [
        ("appModule", "createRepo"),
        ("handler", "logMessage"),
        ("config", "loadConfig"),
        ("get", "buildGreeting"),
    ] {
        assert!(
            calls.contains(&pair(from, to)),
            "{from}->{to} missing from {calls:?}"
        );
    }
    let pending_calls = pending(&result, RelationshipKind::Calls);
    assert!(pending_calls.contains(&triple("appModule", "module", "")));
    assert!(pending_calls.contains(&triple("config", "lazy", "")));
}

#[test]
fn qualified_type_usages_name_the_terminal_type() {
    let result = extract(
        "Types.kt",
        r#"sealed interface ApiResult {
    data class Success(val data: String) : ApiResult
}
fun render(result: ApiResult): String = when (result) {
    is ApiResult.Success -> result.data
    else -> ""
}
fun f() {
    try { } catch (e: java.io.IOException) { }
    val entry: Map.Entry<String, Int>? = null
}
"#,
    );
    let usages: Vec<(&str, u32)> = result
        .identifiers
        .iter()
        .filter(|i| i.kind == crate::base::IdentifierKind::TypeUsage)
        .map(|i| (i.name.as_str(), i.start_line))
        .collect();
    assert!(usages.contains(&("Success", 5)), "{usages:?}");
    assert!(usages.contains(&("IOException", 9)), "{usages:?}");
    assert!(usages.contains(&("Entry", 10)), "{usages:?}");
    assert!(!usages.contains(&("java", 9)), "{usages:?}");
    let entry = result
        .identifiers
        .iter()
        .find(|i| i.name == "Entry")
        .unwrap();
    assert_eq!(
        entry.metadata.as_ref().unwrap().get("receiver"),
        Some(&serde_json::json!("Map"))
    );
}

#[test]
fn infix_calls_and_function_references_are_calls() {
    let result = extract(
        "Infix.kt",
        r#"class Money(val cents: Long) {
    infix fun plusCents(other: Long): Money = Money(cents + other)
}
infix fun Money.sameAs(other: Money): Boolean = cents == other.cents
fun isValid(m: Money): Boolean = m.cents > 0
fun total(a: Money, b: Money, items: List<Money>): Boolean {
    val c = a plusCents 5
    val pair = a to b
    val valid = items.filter(::isValid)
    return c sameAs b
}
"#,
    );
    let call_names: Vec<&str> = result
        .identifiers
        .iter()
        .filter(|i| i.kind == crate::base::IdentifierKind::Call)
        .map(|i| i.name.as_str())
        .collect();
    for name in ["plusCents", "to", "sameAs", "isValid"] {
        assert!(
            call_names.contains(&name),
            "{name} missing from {call_names:?}"
        );
    }
    assert!(resolved(&result, RelationshipKind::Calls).contains(&pair("total", "isValid")));
    let calls = pending(&result, RelationshipKind::Calls);
    assert!(
        calls.contains(&triple("total", "plusCents", "a")),
        "{calls:?}"
    );
    assert!(calls.contains(&triple("total", "to", "a")), "{calls:?}");
    assert!(calls.contains(&triple("total", "sameAs", "c")), "{calls:?}");
}

#[test]
fn calls_on_expression_receivers_do_not_resolve_by_bare_name() {
    let result = extract(
        "Chains.kt",
        r#"class Poller {
    fun start() {}
}
class Adapter {
    fun fromJson(json: String): String = json
}
fun build(): Int = 1

fun main(moshi: Moshi) {
    embeddedServer(Netty, port = 8080).start(wait = true)
    val user = moshi.adapter<User>().fromJson("{}")
    val n = listOf(1).map { it }.build()
}
"#,
    );
    let calls = resolved(&result, RelationshipKind::Calls);
    for name in ["start", "fromJson", "build"] {
        assert!(
            !calls.contains(&pair("main", name)),
            "false edge main->{name}: {calls:?}"
        );
        assert!(
            pending(&result, RelationshipKind::Calls).contains(&triple("main", name, "")),
            "{name} should stay pending"
        );
    }
}

#[test]
fn extension_receivers_resolve_by_declared_receiver_type() {
    let result = extract(
        "Ext.kt",
        r#"class TypeName(val simpleName: String)
private fun TypeName.toVariableName(): String = simpleName.lowercase()
fun String?.orBlank(): String = this ?: ""
val TypeName.display: String get() = simpleName
fun render(type: TypeName, s: String?, other: Other): String =
    type.toVariableName() + s.orBlank() + other.toVariableName()
"#,
    );
    let calls = resolved(&result, RelationshipKind::Calls);
    assert!(
        calls.contains(&pair("render", "toVariableName")),
        "{calls:?}"
    );
    assert!(calls.contains(&pair("render", "orBlank")), "{calls:?}");
    assert!(pending(&result, RelationshipKind::Calls).contains(&triple(
        "render",
        "toVariableName",
        "other"
    )));
    let or_blank = symbol(&result, "orBlank");
    assert!(
        or_blank
            .signature
            .as_deref()
            .unwrap()
            .starts_with("fun String?.orBlank()"),
        "{:?}",
        or_blank.signature
    );
    assert_eq!(
        or_blank.metadata.as_ref().unwrap().get("extendedType"),
        Some(&serde_json::json!("String?"))
    );
    let display = symbol(&result, "display");
    assert_eq!(
        display.signature.as_deref(),
        Some("val TypeName.display: String")
    );
}
