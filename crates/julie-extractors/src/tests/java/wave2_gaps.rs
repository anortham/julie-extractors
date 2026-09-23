use crate::ExtractionResults;
use crate::base::{IdentifierKind, RelationshipKind, SymbolKind};
use crate::extract_canonical;
use crate::tests::helpers::metadata_str;
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("java extraction")
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

fn type_of(result: &ExtractionResults, name: &str) -> Option<String> {
    let id = &symbol(result, name).id;
    result.types.get(id).map(|info| info.resolved_type.clone())
}

fn facts<'a>(
    result: &'a ExtractionResults,
    pattern_id: &str,
) -> Vec<&'a crate::base::StructuralFact> {
    result
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == pattern_id)
        .collect()
}

fn fact_owner(result: &ExtractionResults, fact: &crate::base::StructuralFact) -> String {
    fact.containing_symbol_id
        .as_deref()
        .map(|id| symbol_name(result, id))
        .unwrap_or_default()
}

#[test]
fn interface_constants_annotation_elements_and_compact_constructors_are_symbols() {
    let result = extract(
        "Members.java",
        r#"
public interface Gateway { /** Max retries. */ int MAX_RETRIES = 3; }
@interface Audited { /** The level. */ String level() default "info"; int priority(); }
record Money(long cents, String currency) {
    Money { if (cents < 0) throw new IllegalArgumentException("neg"); validate(cents); }
    static void validate(long c) {}
}
"#,
    );

    let constant = symbol(&result, "MAX_RETRIES");
    assert_eq!(constant.kind, SymbolKind::Constant);
    assert_eq!(constant.signature.as_deref(), Some("int MAX_RETRIES = 3"));
    assert_eq!(constant.doc_comment.as_deref(), Some("/** Max retries. */"));
    assert_eq!(type_of(&result, "MAX_RETRIES").as_deref(), Some("int"));

    let level = symbol(&result, "level");
    assert_eq!(level.kind, SymbolKind::Method);
    assert_eq!(
        level.signature.as_deref(),
        Some(r#"String level() default "info""#)
    );
    assert_eq!(level.doc_comment.as_deref(), Some("/** The level. */"));
    assert_eq!(level.body_hash, None);
    assert_eq!(symbol(&result, "priority").kind, SymbolKind::Method);

    let constructor = result
        .symbols
        .iter()
        .find(|symbol| symbol.kind == SymbolKind::Constructor)
        .expect("compact constructor symbol");
    assert_eq!(constructor.name, "Money");
    assert!(constructor.body_hash.is_some());
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
    assert!(calls.contains(&("Money".to_string(), "validate".to_string())));
    let constructor_call = result
        .structured_pending_relationships
        .iter()
        .find(|pending| pending.target.terminal_name == "IllegalArgumentException")
        .expect("pending constructor call");
    assert_eq!(constructor_call.pending.from_symbol_id, constructor.id);
}

#[test]
fn annotation_usages_are_type_usages_in_the_annotated_symbol() {
    let result = extract(
        "Plain.java",
        r#"
class Plain { @Audited(level = "high") void close() {} @com.acme.Audited void open() {} }
"#,
    );

    let usages = identifiers(&result, IdentifierKind::TypeUsage);
    assert!(
        usages.contains(&("Audited".to_string(), "close".to_string())),
        "{usages:?}"
    );
    assert!(
        usages.contains(&("Audited".to_string(), "open".to_string())),
        "{usages:?}"
    );
    assert!(
        !usages
            .iter()
            .any(|(name, _)| name == "com" || name == "acme")
    );
}

#[test]
fn qualified_types_keep_their_text_and_drop_package_segments() {
    let result = extract(
        "Qualified.java",
        r#"
class Qualified {
    private java.util.concurrent.atomic.AtomicInteger count;
    java.time.Instant created;
    Map.Entry<String, Integer> entry;
    java.time.Instant stamp() { return null; }
    java.util.Map<String, java.util.List<Order>> byUser() { return null; }
    private java.util.List<String> names;
    void process() { var list = new java.util.ArrayList<String>(); }
}
"#,
    );

    assert_eq!(
        symbol(&result, "count").signature.as_deref(),
        Some("private java.util.concurrent.atomic.AtomicInteger count")
    );
    assert_eq!(
        symbol(&result, "stamp").signature.as_deref(),
        Some("java.time.Instant stamp()")
    );
    assert_eq!(type_of(&result, "names"), None);
    assert_eq!(type_of(&result, "byUser"), None);
    assert_eq!(type_of(&result, "process"), None);

    let usages: Vec<String> = identifiers(&result, IdentifierKind::TypeUsage)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    for segment in ["java", "util", "time", "concurrent", "atomic", "var"] {
        assert!(
            !usages.contains(&segment.to_string()),
            "{segment}: {usages:?}"
        );
    }
    assert!(usages.contains(&"Map".to_string()));
    assert!(usages.contains(&"Entry".to_string()));

    let generic_names: Vec<String> = result
        .type_argument_usages
        .iter()
        .filter_map(|usage| {
            result
                .identifiers
                .iter()
                .find(|identifier| identifier.id == usage.identifier_id)
                .map(|identifier| identifier.name.clone())
        })
        .collect();
    for generic in ["Entry", "Map", "List", "ArrayList"] {
        assert!(
            generic_names.contains(&generic.to_string()),
            "{generic}: {generic_names:?}"
        );
    }
}

#[test]
fn switch_and_record_patterns_bind_typed_variables() {
    let result = extract(
        "Patterns.java",
        r#"
class Patterns {
    double area(Shape s) {
        return switch (s) {
            case Circle c -> Math.PI * c.r() * c.r();
            case Rect(double w, double h) -> w * h;
            default -> 0;
        };
    }
}
"#,
    );

    assert_eq!(type_of(&result, "c").as_deref(), Some("Circle"));
    assert_eq!(type_of(&result, "w").as_deref(), Some("double"));
    assert_eq!(symbol(&result, "h").kind, SymbolKind::Variable);
    let usages = identifiers(&result, IdentifierKind::TypeUsage);
    assert!(
        usages.contains(&("Rect".to_string(), "area".to_string())),
        "{usages:?}"
    );
    let reads = identifiers(&result, IdentifierKind::VariableRef);
    assert!(!reads.iter().any(|(name, _)| name == "Rect"), "{reads:?}");
    assert_eq!(reads.iter().filter(|(name, _)| name == "w").count(), 1);
}

#[test]
fn non_class_type_declarations_carry_their_annotations() {
    let result = extract(
        "Types.java",
        r#"
@Repository interface UserRepo {}
@ConfigurationProperties(prefix = "shop") record ShopProps(@JsonProperty("max_lines") int maxLines) {}
@Deprecated enum Legacy { A }
@Retention(RetentionPolicy.RUNTIME) @interface Audited {}
"#,
    );

    for (name, annotation) in [
        ("UserRepo", "Repository"),
        ("ShopProps", "ConfigurationProperties"),
        ("maxLines", "JsonProperty"),
        ("Legacy", "Deprecated"),
        ("Audited", "Retention"),
    ] {
        let annotations: Vec<&str> = symbol(&result, name)
            .annotations
            .iter()
            .map(|marker| marker.annotation.as_str())
            .collect();
        assert_eq!(annotations, vec![annotation], "{name}");
    }
}

#[test]
fn module_declaration_is_a_namespace_with_directive_facts() {
    let result = extract(
        "module-info.java",
        r#"
/** Shop module. */
module com.acme.shop {
    requires java.sql;
    requires transitive com.acme.core;
    exports com.acme.shop.api to com.acme.web;
    uses com.acme.shop.spi.PaymentProvider;
    provides com.acme.shop.spi.PaymentProvider with com.acme.shop.impl.StripeProvider;
}
"#,
    );

    let module = symbol(&result, "com.acme.shop");
    assert_eq!(module.kind, SymbolKind::Namespace);
    assert_eq!(module.signature.as_deref(), Some("module com.acme.shop"));
    assert_eq!(module.doc_comment.as_deref(), Some("/** Shop module. */"));

    let directives = facts(&result, "java.module_directive.v1");
    let summary: Vec<(String, String)> = directives
        .iter()
        .map(|fact| {
            (
                metadata_str(fact, "directive")
                    .unwrap_or_default()
                    .to_string(),
                metadata_str(fact, "target").unwrap_or_default().to_string(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        vec![
            ("requires".to_string(), "java.sql".to_string()),
            ("requires".to_string(), "com.acme.core".to_string()),
            ("exports".to_string(), "com.acme.shop.api".to_string()),
            (
                "uses".to_string(),
                "com.acme.shop.spi.PaymentProvider".to_string()
            ),
            (
                "provides".to_string(),
                "com.acme.shop.spi.PaymentProvider".to_string()
            ),
        ]
    );
    let metadata =
        |index: usize, key: &str| directives[index].metadata.as_ref().unwrap()[key].clone();
    assert_eq!(metadata(1, "modifiers"), serde_json::json!(["transitive"]));
    assert_eq!(
        metadata(2, "to_modules"),
        serde_json::json!(["com.acme.web"])
    );
    assert_eq!(
        metadata(4, "providers"),
        serde_json::json!(["com.acme.shop.impl.StripeProvider"])
    );
    assert!(
        directives
            .iter()
            .all(|fact| fact_owner(&result, fact) == "com.acme.shop")
    );

    let usages = identifiers(&result, IdentifierKind::TypeUsage);
    assert!(usages.contains(&("StripeProvider".to_string(), "com.acme.shop".to_string())));
    assert!(identifiers(&result, IdentifierKind::VariableRef).is_empty());
}

#[test]
fn query_strings_of_jpa_calls_and_query_annotations_are_sql_literals() {
    let result = extract(
        "src/main/java/Dao.java",
        r#"
interface Repo {
    @Query("select o from Order o where o.status = :status") List<Order> byStatus(String status);
    @Query(value = "SELECT * FROM orders", nativeQuery = true) List<Order> all();
}
class Dao {
    void q(EntityManager em) {
        em.createQuery("select o from Order o", Order.class);
        em.createNativeQuery("SELECT count(*) FROM orders");
    }
}
"#,
    );

    let mut literals = result.literals.clone();
    crate::language_policy::classify_literals_by_carrier(&mut literals);
    let sql: Vec<(&str, &str)> = literals
        .iter()
        .filter(|literal| literal.kind == crate::base::LiteralKind::Sql)
        .map(|literal| {
            (
                literal.literal_text.as_str(),
                literal.carrier.as_deref().unwrap_or_default(),
            )
        })
        .collect();
    assert_eq!(
        sql,
        vec![
            ("select o from Order o where o.status = :status", "Query"),
            ("SELECT * FROM orders", "Query"),
            ("select o from Order o", "em.createQuery"),
            ("SELECT count(*) FROM orders", "em.createNativeQuery"),
        ]
    );
}

#[test]
fn junit_suites_test_interfaces_and_their_implementers_are_containers() {
    let result = extract(
        "src/test/java/p/StackTest.java",
        r#"
package p;
interface StackContract { Stack create(); @BeforeEach default void reset() {} @Test default void pushThenPop() {} }
class ArrayStackTest implements StackContract { public Stack create() { return null; } }
@Suite @SelectClasses({ArrayStackTest.class, q.Other.class}) class AllTests {}
class Aggregator {}
"#,
    );

    let container = |name: &str| {
        symbol(&result, name)
            .metadata
            .as_ref()
            .and_then(|m| m.get("test_container"))
            .and_then(|v| v.as_bool())
            == Some(true)
    };
    assert!(container("StackContract"));
    assert!(container("ArrayStackTest"));
    assert!(container("AllTests"));
    assert!(!container("Aggregator"));

    let references: Vec<(String, String)> = result
        .relationships
        .iter()
        .filter(|relationship| relationship.kind == RelationshipKind::References)
        .map(|relationship| {
            (
                symbol_name(&result, &relationship.from_symbol_id),
                symbol_name(&result, &relationship.to_symbol_id),
            )
        })
        .collect();
    assert_eq!(
        references,
        vec![("AllTests".to_string(), "ArrayStackTest".to_string())]
    );
    let pending = result
        .structured_pending_relationships
        .iter()
        .find(|pending| pending.pending.kind == RelationshipKind::References)
        .expect("pending selected class");
    assert_eq!(pending.target.terminal_name, "Other");
}

const SPRING_CONTROLLER: &str = r#"
import org.springframework.web.bind.annotation.*;
@RestController
@RequestMapping("/api/users")
public class UserController {
    static class UserDto { String name; }
    @GetMapping("/{id}") public UserDto get(@PathVariable long id) { return null; }
    @GetMapping("/inline") public String inline() { return ""; }
    public String helper() { return ""; }
    @ResponseStatus(HttpStatus.CREATED) @PostMapping("/{id}/items") public String addItem(long id) { return ""; }
    @org.springframework.web.bind.annotation.GetMapping("/fq") public String fq() { return ""; }
    @GetMapping public String list() { return ""; }
}
@RestController @RequestMapping("/users2") class UserApi { @GetMapping("/{id}") public String get(long id) { return ""; } }
@FeignClient(name = "users") interface UserClient { @GetMapping("/users/{id}") User findUser(@PathVariable("id") long id); }
"#;

fn route_rows(result: &ExtractionResults, pattern_id: &str) -> Vec<(String, String, String)> {
    facts(result, pattern_id)
        .iter()
        .map(|fact| {
            (
                fact_owner(result, fact),
                metadata_str(fact, "verb").unwrap_or_default().to_string(),
                metadata_str(fact, "effective_route_template")
                    .or_else(|| metadata_str(fact, "route_template"))
                    .unwrap_or_default()
                    .to_string(),
            )
        })
        .collect()
}

fn row(owner: &str, verb: &str, route: &str) -> (String, String, String) {
    (owner.to_string(), verb.to_string(), route.to_string())
}

#[test]
fn spring_routes_follow_the_tree_not_the_line_layout() {
    let result = extract("UserController.java", SPRING_CONTROLLER);

    assert_eq!(
        route_rows(&result, "spring.request_mapping.v1"),
        vec![
            row("UserController", "", "/api/users"),
            row("get", "GET", "/api/users/{id}"),
            row("inline", "GET", "/api/users/inline"),
            row("addItem", "POST", "/api/users/{id}/items"),
            row("fq", "GET", "/api/users/fq"),
            row("list", "GET", "/api/users"),
            row("UserApi", "", "/users2"),
            row("get", "GET", "/users2/{id}"),
        ]
    );
}

#[test]
fn jaxrs_resources_emit_joined_routes() {
    let result = extract(
        "OrderResource.java",
        r#"
import jakarta.ws.rs.*;
@Path("/orders")
public class OrderResource {
    @GET @Path("/{id}") public Order get(@PathParam("id") long id) { return null; }
    @POST public Order create(Order order) { return order; }
    @Path("{id}/items") public ItemsResource items() { return null; }
    public void helper() {}
}
"#,
    );

    assert_eq!(
        route_rows(&result, "jaxrs.route.v1"),
        vec![
            row("OrderResource", "", "/orders"),
            row("get", "GET", "/orders/{id}"),
            row("create", "POST", "/orders"),
            row("items", "", "/orders/{id}/items"),
        ]
    );
    let items = facts(&result, "jaxrs.route.v1")[3];
    assert_eq!(
        metadata_str(items, "attribute_kind"),
        Some("subresource_locator")
    );
}

#[test]
fn spring_clients_and_feign_interfaces_emit_client_requests() {
    let result = extract(
        "Clients.java",
        r#"
import org.springframework.web.client.RestTemplate;
import org.springframework.web.reactive.function.client.WebClient;
import org.springframework.cloud.openfeign.FeignClient;
import org.springframework.web.bind.annotation.*;
class Clients {
    private final RestTemplate rest = new RestTemplate();
    private final WebClient web = WebClient.create("https://api.example.com");
    private final Other other = new Other();
    String a() { return rest.getForObject("https://api.example.com/users/{id}", String.class, 1); }
    String b() { return web.get().uri("/users/{id}", 1).retrieve().bodyToMono(String.class).block(); }
    void c() { rest.postForEntity("/orders", null, String.class); }
    void d(RestTemplate template) { template.exchange("/ex", HttpMethod.PUT, null, String.class); }
    void e() { other.getForObject("/nope"); other.get().uri("/nope"); }
}
@FeignClient(name = "users", path = "/svc") interface UserClient {
    @GetMapping("/users/{id}") User findUser(@PathVariable("id") long id);
}
"#,
    );

    let rows: Vec<(String, String, String, String)> = facts(&result, "http.client_request.v1")
        .iter()
        .map(|fact| {
            (
                fact_owner(&result, fact),
                metadata_str(fact, "client").unwrap_or_default().to_string(),
                metadata_str(fact, "verb").unwrap_or_default().to_string(),
                metadata_str(fact, "target_path")
                    .unwrap_or_default()
                    .to_string(),
            )
        })
        .collect();
    let expected = |owner: &str, client: &str, verb: &str, path: &str| {
        (
            owner.to_string(),
            client.to_string(),
            verb.to_string(),
            path.to_string(),
        )
    };
    assert_eq!(
        rows,
        vec![
            expected(
                "a",
                "spring_resttemplate",
                "GET",
                "https://api.example.com/users/{id}"
            ),
            expected("b", "spring_webclient", "GET", "/users/{id}"),
            expected("c", "spring_resttemplate", "POST", "/orders"),
            expected("d", "spring_resttemplate", "PUT", "/ex"),
            expected("findUser", "openfeign", "GET", "/svc/users/{id}"),
        ]
    );
    assert!(facts(&result, "spring.request_mapping.v1").is_empty());
}
