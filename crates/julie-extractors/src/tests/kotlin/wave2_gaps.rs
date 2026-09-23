use crate::ExtractionResults;
use crate::base::{IdentifierKind, LiteralKind, RelationshipKind, SymbolKind, Visibility};
use crate::extract_canonical;
use crate::tests::helpers::metadata_str;
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("kotlin extraction")
}

fn symbol<'a>(result: &'a ExtractionResults, name: &str) -> &'a crate::base::Symbol {
    result
        .symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap_or_else(|| panic!("no symbol {name}: {:?}", names(result)))
}

fn names(result: &ExtractionResults) -> Vec<String> {
    result
        .symbols
        .iter()
        .map(|symbol| format!("{}:{:?}", symbol.name, symbol.kind))
        .collect()
}

fn symbol_name(result: &ExtractionResults, id: &str) -> String {
    result
        .symbols
        .iter()
        .find(|symbol| symbol.id == id)
        .map(|symbol| symbol.name.clone())
        .unwrap_or_default()
}

fn parent_name(result: &ExtractionResults, symbol: &crate::base::Symbol) -> String {
    symbol
        .parent_id
        .as_deref()
        .map(|id| symbol_name(result, id))
        .unwrap_or_default()
}

fn identifiers(result: &ExtractionResults, kind: IdentifierKind) -> Vec<(String, String)> {
    result
        .identifiers
        .iter()
        .filter(|identifier| identifier.kind == kind)
        .map(|identifier| {
            (
                identifier.name.clone(),
                identifier
                    .containing_symbol_id
                    .as_deref()
                    .map(|id| symbol_name(result, id))
                    .unwrap_or_default(),
            )
        })
        .collect()
}

fn pair(left: &str, right: &str) -> (String, String) {
    (left.to_string(), right.to_string())
}

fn meta<'a>(symbol: &'a crate::base::Symbol, key: &str) -> Option<&'a serde_json::Value> {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
}

fn test_role<'a>(result: &'a ExtractionResults, name: &str) -> Option<&'a str> {
    meta(symbol(result, name), "test_role").and_then(|value| value.as_str())
}

fn route_facts(result: &ExtractionResults, pattern_id: &str) -> Vec<(String, String, String)> {
    result
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == pattern_id)
        .map(|fact| {
            (
                metadata_str(fact, "verb").unwrap_or_default().to_string(),
                metadata_str(fact, "route_template")
                    .unwrap_or_default()
                    .to_string(),
                metadata_str(fact, "effective_route_template")
                    .or_else(|| metadata_str(fact, "route_template"))
                    .unwrap_or_default()
                    .to_string(),
            )
        })
        .collect()
}

#[test]
fn operator_fun_bodies_own_their_calls() {
    let result = extract(
        "Vec.kt",
        r#"
data class Vec(val x: Int) {
    operator fun plus(other: Vec): Vec {
        return combine(other)
    }
    fun combine(other: Vec): Vec = Vec(x + other.x)
}
"#,
    );

    let calls = identifiers(&result, IdentifierKind::Call);
    assert!(calls.contains(&pair("combine", "plus")), "{calls:?}");
}

#[test]
fn enum_entry_members_attach_to_the_entry_and_local_functions_are_functions() {
    let result = extract(
        "Shapes.kt",
        r#"
private enum class Mode { FAST, SLOW }
enum class Op {
    ADD {
        override fun apply(a: Int, b: Int) = a + b
    };
    abstract fun apply(a: Int, b: Int): Int
}
fun outer(x: Int): Int {
    fun helper(y: Int) = y * 2
    return helper(x)
}
"#,
    );

    assert_eq!(symbol(&result, "Mode").kind, SymbolKind::Enum);
    let apply_parents: Vec<String> = result
        .symbols
        .iter()
        .filter(|symbol| symbol.name == "apply")
        .map(|symbol| parent_name(&result, symbol))
        .collect();
    assert_eq!(apply_parents, vec!["ADD", "Op"]);
    let helper = symbol(&result, "helper");
    assert_eq!(helper.kind, SymbolKind::Function);
    assert_eq!(helper.visibility, None);
}

#[test]
fn internal_private_companion_and_local_visibility_follow_the_modifiers() {
    let result = extract(
        "Cache.kt",
        r#"
internal class Cache(private val size: Int, internal val tag: String) {
    internal val entries = mutableMapOf<String, Int>()
    private companion object Factory {
        fun create(): Cache {
            val local = Cache(1, "x")
            return local
        }
    }
}
"#,
    );

    assert_eq!(
        symbol(&result, "Cache").visibility,
        Some(Visibility::Internal)
    );
    assert_eq!(
        symbol(&result, "entries").visibility,
        Some(Visibility::Internal)
    );
    assert_eq!(
        symbol(&result, "tag").visibility,
        Some(Visibility::Internal)
    );
    assert_eq!(
        symbol(&result, "size").visibility,
        Some(Visibility::Private)
    );
    assert_eq!(
        symbol(&result, "Factory").visibility,
        Some(Visibility::Private)
    );
    assert_eq!(symbol(&result, "local").visibility, None);
}

#[test]
fn this_calls_in_extensions_use_the_extension_receiver_type() {
    let result = extract(
        "Ext.kt",
        r#"
class Builder {
    fun append(s: String) {}
}
class Outer {
    fun Builder.ext() {
        this.append("x")
    }
}
fun String.shout(): String = this.uppercase()
val String.twice: String
    get() = this.repeat(2)
"#,
    );

    let receiver_types: Vec<(String, String)> = result
        .identifiers
        .iter()
        .filter(|identifier| identifier.kind == IdentifierKind::Call)
        .map(|identifier| {
            (
                identifier.name.clone(),
                identifier.receiver_type.clone().unwrap_or_default(),
            )
        })
        .collect();
    assert_eq!(
        receiver_types,
        vec![
            pair("append", "Builder"),
            pair("uppercase", "String"),
            pair("repeat", "String"),
        ]
    );
}

#[test]
fn jvm_sql_url_and_query_annotation_literals_are_classified() {
    let result = extract(
        "Repo.kt",
        r#"
interface UserRepo : JpaRepository<User, Long> {
    @Query("SELECT u FROM User u WHERE u.name = :name")
    fun findByName(name: String): List<User>
    @Query(value = "select * from users", nativeQuery = true)
    fun all(): List<User>
}
class Dao(private val jdbc: JdbcTemplate, private val rest: RestTemplate) {
    fun load() {
        jdbc.query("SELECT * FROM accounts", mapper)
        rest.getForObject("https://api.example.com/users", String::class.java)
        val uri = URI.create("https://api.example.com/health")
    }
}
"#,
    );

    let mut literals = result.literals.clone();
    crate::language_policy::classify_literals_by_carrier(&mut literals);
    let classified: Vec<(LiteralKind, &str)> = literals
        .iter()
        .filter(|literal| matches!(literal.kind, LiteralKind::Sql | LiteralKind::Url))
        .map(|literal| (literal.kind.clone(), literal.literal_text.as_str()))
        .collect();
    assert_eq!(
        classified,
        vec![
            (
                LiteralKind::Sql,
                "SELECT u FROM User u WHERE u.name = :name"
            ),
            (LiteralKind::Sql, "select * from users"),
            (LiteralKind::Sql, "SELECT * FROM accounts"),
            (LiteralKind::Url, "https://api.example.com/users"),
            (LiteralKind::Url, "https://api.example.com/health"),
        ]
    );
}

#[test]
fn declarations_keep_aliases_wildcards_platform_modifiers_and_function_types() {
    let result = extract(
        "Decls.kt",
        r#"
import com.example.db.UserRepository as Repo
import com.example.util.*
expect fun platformName(): String
val String.lastChar: Char
    get() = this[length - 1]
class Button(val onClick: () -> Unit, label: String)
val listener: (String) -> Unit = {}
"#,
    );

    let alias = symbol(&result, "com.example.db.UserRepository");
    assert_eq!(
        alias.signature.as_deref(),
        Some("import com.example.db.UserRepository as Repo")
    );
    assert_eq!(meta(alias, "alias").and_then(|v| v.as_str()), Some("Repo"));
    assert_eq!(
        meta(alias, "importedName").and_then(|v| v.as_str()),
        Some("UserRepository")
    );
    let wildcard = symbol(&result, "com.example.util");
    assert_eq!(
        wildcard.signature.as_deref(),
        Some("import com.example.util.*")
    );
    assert_eq!(
        meta(wildcard, "isWildcard").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        symbol(&result, "platformName").signature.as_deref(),
        Some("expect fun platformName(): String")
    );
    assert_eq!(
        symbol(&result, "lastChar").signature.as_deref(),
        Some("val String.lastChar: Char")
    );
    assert_eq!(
        symbol(&result, "onClick").signature.as_deref(),
        Some("val onClick: () -> Unit")
    );
    let label = symbol(&result, "label");
    assert_eq!(label.signature.as_deref(), Some("label: String"));
    assert_eq!(
        meta(label, "binding").and_then(|v| v.as_str()),
        Some("none")
    );
    assert_eq!(label.visibility, Some(Visibility::Private));
    assert_eq!(
        symbol(&result, "listener").signature.as_deref(),
        Some("val listener: (String) -> Unit = {}")
    );
}

#[test]
fn keyword_literals_are_not_refs_and_class_literals_are_type_usages() {
    let result = extract(
        "Refs.kt",
        r#"
fun check(flag: Boolean): Any? {
    val kind = Widget::class
    if (flag == true) return null
    return false
}
"#,
    );

    let refs = identifiers(&result, IdentifierKind::VariableRef);
    assert!(
        refs.iter()
            .all(|(name, _)| !matches!(name.as_str(), "null" | "true" | "false" | "Widget")),
        "{refs:?}"
    );
    assert!(
        identifiers(&result, IdentifierKind::MemberAccess)
            .iter()
            .all(|(name, _)| name != "class")
    );
    assert!(identifiers(&result, IdentifierKind::TypeUsage).contains(&pair("Widget", "check")));
}

#[test]
fn test_dsl_words_in_production_code_create_no_symbols() {
    let result = extract(
        "src/main/kotlin/Config.kt",
        r#"
class Config {
    fun setup(r: Registry) {
        r.apply {
            context("admin") {
                register("x")
            }
            feature("billing") {
                enable()
            }
        }
    }
}
"#,
    );

    assert!(
        result
            .symbols
            .iter()
            .all(|symbol| !matches!(symbol.name.as_str(), "admin" | "billing")),
        "{:?}",
        names(&result)
    );
    let calls = identifiers(&result, IdentifierKind::Call);
    assert!(calls.contains(&pair("register", "setup")), "{calls:?}");
    assert!(calls.contains(&pair("enable", "setup")), "{calls:?}");
}

#[test]
fn spec_base_constructor_calls_enable_the_test_dsl_without_an_import() {
    let result = extract(
        "src/main/kotlin/Roles.kt",
        r#"
class ManagedTestRoles : DescribeSpec({
    describe("kotlin roles") {
        it("extracts a Kotlin test case") {
        }
    }
})
"#,
    );

    assert_eq!(test_role(&result, "kotlin roles"), Some("test_container"));
    assert_eq!(
        test_role(&result, "extracts a Kotlin test case"),
        Some("test_case")
    );
}

#[test]
fn property_accessors_are_symbols_that_own_their_bodies() {
    let result = extract(
        "Props.kt",
        r#"
class Account {
    var balance: Int = 0
        get() = audit(field)
        set(value) {
            record(value)
            field = value
        }
    fun audit(v: Int): Int = v
    fun record(v: Int) {}
}
"#,
    );

    let getter = symbol(&result, "get");
    assert_eq!(getter.kind, SymbolKind::Method);
    assert_eq!(parent_name(&result, getter), "balance");
    assert_eq!(
        meta(getter, "accessor").and_then(|v| v.as_str()),
        Some("getter")
    );
    assert_eq!(parent_name(&result, symbol(&result, "set")), "balance");
    let calls: Vec<(String, String)> = result
        .relationships
        .iter()
        .filter(|relationship| relationship.kind == RelationshipKind::Calls)
        .map(|relationship| {
            (
                symbol_name(&result, &relationship.from_symbol_id),
                symbol_name(&result, &relationship.to_symbol_id),
            )
        })
        .collect();
    assert!(calls.contains(&pair("get", "audit")), "{calls:?}");
    assert!(calls.contains(&pair("set", "record")), "{calls:?}");
}

#[test]
fn kotest_behavior_spec_steps_lifecycle_hooks_and_with_data_get_roles() {
    let result = extract(
        "src/test/kotlin/CartSpec.kt",
        r#"
import io.kotest.core.spec.style.BehaviorSpec

class CartSpec : BehaviorSpec({
    beforeSpec {
        reset()
    }
    Given("an empty cart") {
        `when`("an item is added") {
            Then("the total grows") {
                check()
            }
        }
        xGiven("a disabled branch") {
            then("never runs") {
                check()
            }
        }
    }
    context("pythag triples") {
        withData(Triple(3, 4, 5), Triple(6, 8, 10)) { (a, b, c) ->
            check()
        }
    }
})
"#,
    );

    assert_eq!(test_role(&result, "an empty cart"), Some("test_container"));
    assert_eq!(
        test_role(&result, "an item is added"),
        Some("test_container")
    );
    assert_eq!(test_role(&result, "the total grows"), Some("test_case"));
    assert_eq!(
        test_role(&result, "a disabled branch"),
        Some("test_container")
    );
    assert_eq!(test_role(&result, "beforeSpec"), Some("fixture_setup"));
    assert_eq!(test_role(&result, "withData"), Some("parameterized_test"));
    assert_eq!(
        parent_name(&result, symbol(&result, "withData")),
        "pythag triples"
    );
}

#[test]
fn ktor_pathless_verbs_inherit_the_route_prefix() {
    let result = extract(
        "Routing.kt",
        r#"
import io.ktor.server.routing.*

fun Application.configureRouting() {
    routing {
        route("/orders") {
            get {
                call.respondText("all")
            }
            get("{id}") {
                call.respondText("one")
            }
        }
        post {
            call.respondText("root")
        }
    }
}
"#,
    );

    assert_eq!(
        route_facts(&result, "ktor.route.v1"),
        vec![
            ("GET".to_string(), String::new(), "/orders".to_string()),
            (
                "GET".to_string(),
                "{id}".to_string(),
                "/orders/{id}".to_string()
            ),
        ]
    );
}

#[test]
fn spring_functional_routers_emit_nested_route_facts() {
    let result = extract(
        "Routes.kt",
        r#"
import org.springframework.web.reactive.function.server.coRouter
import org.springframework.web.servlet.function.router

class Routes(private val handler: UserHandler) {
    fun api() = coRouter {
        "/api/users".nest {
            GET("", handler::list)
            GET("/{id}", handler::get)
        }
        GET("/health") {
            ok().bodyValueAndAwait("up")
        }
        path("/v2").nest {
            PUT("/items/{id}", handler::update)
        }
        "$base/dyn".nest {
            GET("/hidden", handler::hidden)
        }
    }

    fun mvc() = router {
        accept(APPLICATION_JSON).nest {
            DELETE("/mvc/{id}", handler::delete)
        }
    }
}
"#,
    );

    let routes: Vec<(String, String)> = route_facts(&result, "spring.functional_route.v1")
        .into_iter()
        .map(|(verb, _, effective)| (verb, effective))
        .collect();
    assert_eq!(
        routes,
        vec![
            pair("GET", "/api/users"),
            pair("GET", "/api/users/{id}"),
            pair("GET", "/health"),
            pair("PUT", "/v2/items/{id}"),
            pair("DELETE", "/mvc/{id}"),
        ]
    );
    let owners: Vec<String> = result
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == "spring.functional_route.v1")
        .map(|fact| symbol_name(&result, fact.containing_symbol_id.as_deref().unwrap_or("")))
        .collect();
    assert_eq!(owners, vec!["api", "api", "api", "api", "mvc"]);
}

#[test]
fn typed_class_properties_prove_http_client_receivers() {
    let result = extract(
        "Gateway.kt",
        r#"
import io.ktor.client.HttpClient
import org.springframework.web.client.RestTemplate

class Gateway {
    private lateinit var rest: RestTemplate
    private lateinit var ktor: HttpClient
    private lateinit var other: RestTemplateFactory
    fun b() = rest.getForObject("https://api.example.com/b", String::class.java)
    suspend fun e() = ktor.get("https://api.example.com/e")
    fun f() = other.getForObject("https://api.example.com/f", String::class.java)
}
"#,
    );

    let targets: Vec<(String, String)> = result
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == "http.client_request.v1")
        .map(|fact| {
            pair(
                metadata_str(fact, "client").unwrap_or_default(),
                metadata_str(fact, "target_path").unwrap_or_default(),
            )
        })
        .collect();
    assert_eq!(
        targets,
        vec![
            pair("spring_resttemplate", "https://api.example.com/b"),
            pair("ktor", "https://api.example.com/e"),
        ]
    );
}

#[test]
fn retrofit_http_annotations_with_static_method_and_path_are_client_requests() {
    let result = extract(
        "Api.kt",
        r#"
import retrofit2.http.HTTP

interface Api {
    @HTTP(method = "DELETE", path = "users/{id}", hasBody = true)
    fun remove(): Call<Unit>
    @HTTP("PATCH", "users")
    fun patch(): Call<Unit>
    @HTTP(method = "BREW", path = "pot")
    fun brew(): Call<Unit>
    @HTTP(method = VERB, path = "dynamic")
    fun dynamic(): Call<Unit>
}
"#,
    );

    let requests: Vec<(String, String)> = result
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == "http.client_request.v1")
        .map(|fact| {
            pair(
                metadata_str(fact, "verb").unwrap_or_default(),
                metadata_str(fact, "target_path").unwrap_or_default(),
            )
        })
        .collect();
    assert_eq!(
        requests,
        vec![pair("DELETE", "users/{id}"), pair("PATCH", "users")]
    );
}
