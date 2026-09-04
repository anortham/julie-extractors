//! Kotlin Spring MVC annotation-controller route facts (`spring.request_mapping.v1`).

use std::path::Path;

use crate::base::StructuralFact;
use crate::tests::helpers::metadata_str;

const SPRING_REQUEST_MAPPING_PATTERN_ID: &str = "spring.request_mapping.v1";

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical extraction should succeed")
}

fn routes(results: &crate::ExtractionResults) -> Vec<&StructuralFact> {
    results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == SPRING_REQUEST_MAPPING_PATTERN_ID)
        .collect()
}

fn metadata_array<'a>(fact: &'a StructuralFact, key: &str) -> Vec<&'a str> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_array())
        .map(|values| values.iter().filter_map(|value| value.as_str()).collect())
        .unwrap_or_default()
}

fn binding_symbol_name<'a>(
    results: &'a crate::ExtractionResults,
    fact: &StructuralFact,
) -> Option<&'a str> {
    let id = fact.containing_symbol_id.as_deref()?;
    results
        .symbols
        .iter()
        .find(|symbol| symbol.id == id)
        .map(|symbol| symbol.name.as_str())
}

fn find_route<'a>(facts: &[&'a StructuralFact], route_template: &str) -> &'a StructuralFact {
    facts
        .iter()
        .copied()
        .find(|fact| metadata_str(fact, "route_template") == Some(route_template))
        .unwrap_or_else(|| panic!("route_template {route_template:?} not found in {facts:#?}"))
}

const SPRING_IMPORT: &str = "import org.springframework.web.bind.annotation.*";

#[test]
fn kotlin_controller_method_mappings_join_class_prefix_and_bind_to_handler() {
    let source = format!(
        r#"{SPRING_IMPORT}

@RestController
@RequestMapping("/api")
class UserController {{
    @GetMapping("/users/{{id}}")
    fun getUser(): String {{ return "x" }}

    @PostMapping("/users")
    fun createUser(): String {{ return "y" }}

    @DeleteMapping("/users/{{id}}")
    fun deleteUser(): String {{ return "z" }}
}}
"#
    );
    let results = extract("src/UserController.kt", &source);
    let facts = routes(&results);
    assert_eq!(facts.len(), 3, "{facts:#?}");

    let get = find_route(&facts, "/users/{id}");
    assert_eq!(metadata_str(get, "framework"), Some("spring"));
    assert_eq!(metadata_str(get, "api_style"), Some("annotation_routing"));
    assert_eq!(metadata_str(get, "attribute_kind"), Some("http_method"));
    assert_eq!(metadata_str(get, "verb"), Some("GET"));
    assert_eq!(metadata_str(get, "verb_source"), Some("attested"));
    assert_eq!(metadata_str(get, "class_route_template"), Some("/api"));
    assert_eq!(
        metadata_str(get, "effective_route_template"),
        Some("/api/users/{id}")
    );
    assert_eq!(
        metadata_str(get, "normalized_route_template"),
        Some("/api/users/:id")
    );
    assert_eq!(metadata_array(get, "dynamic_segments"), vec!["id"]);
    assert_eq!(binding_symbol_name(&results, get), Some("getUser"));

    let post = facts
        .iter()
        .find(|fact| metadata_str(fact, "verb") == Some("POST"))
        .expect("post route");
    assert_eq!(
        metadata_str(post, "normalized_route_template"),
        Some("/api/users")
    );
    assert_eq!(binding_symbol_name(&results, post), Some("createUser"));

    let delete = facts
        .iter()
        .find(|fact| metadata_str(fact, "verb") == Some("DELETE"))
        .expect("delete route");
    assert_eq!(
        metadata_str(delete, "normalized_route_template"),
        Some("/api/users/:id")
    );
    assert_eq!(binding_symbol_name(&results, delete), Some("deleteUser"));
}

#[test]
fn kotlin_bracket_array_paths_cross_product_the_class_prefix() {
    // Kotlin annotation arrays use brackets, not Java `{...}`.
    let source = format!(
        r#"{SPRING_IMPORT}

@RestController
@RequestMapping("/api")
class MultiController {{
    @GetMapping(["/a", "/b"])
    fun both(): String {{ return "x" }}
}}
"#
    );
    let results = extract("src/MultiController.kt", &source);
    let facts = routes(&results);
    let normalized: Vec<&str> = facts
        .iter()
        .filter_map(|fact| metadata_str(fact, "normalized_route_template"))
        .collect();
    assert_eq!(normalized, vec!["/api/a", "/api/b"], "{facts:#?}");
    for fact in &facts {
        assert_eq!(binding_symbol_name(&results, fact), Some("both"));
    }
}

#[test]
fn kotlin_request_mapping_with_method_array_emits_one_fact_per_verb() {
    let source = format!(
        r#"{SPRING_IMPORT}

@RestController
class SearchController {{
    @RequestMapping(value = ["/search/{{term}}"], method = [RequestMethod.GET, RequestMethod.POST])
    fun search(): String {{ return "x" }}
}}
"#
    );
    let results = extract("src/SearchController.kt", &source);
    let facts = routes(&results);
    assert_eq!(facts.len(), 2, "{facts:#?}");
    let mut verbs: Vec<&str> = facts
        .iter()
        .filter_map(|fact| metadata_str(fact, "verb"))
        .collect();
    verbs.sort_unstable();
    assert_eq!(verbs, vec!["GET", "POST"]);
    for fact in &facts {
        assert_eq!(
            metadata_str(fact, "attribute_kind"),
            Some("request_mapping")
        );
        assert_eq!(
            metadata_str(fact, "normalized_route_template"),
            Some("/search/:term")
        );
        assert_eq!(metadata_array(fact, "dynamic_segments"), vec!["term"]);
        assert_eq!(binding_symbol_name(&results, fact), Some("search"));
    }
}

#[test]
fn kotlin_request_mapping_without_method_is_not_verb_restricted() {
    let source = format!(
        r#"{SPRING_IMPORT}

@RestController
class LegacyController {{
    @RequestMapping("/legacy")
    fun legacy(): String {{ return "ok" }}
}}
"#
    );
    let results = extract("src/LegacyController.kt", &source);
    let facts = routes(&results);
    assert_eq!(facts.len(), 1, "{facts:#?}");
    // Verb omission = not verb-restricted.
    assert_eq!(metadata_str(facts[0], "verb"), None);
    assert_eq!(metadata_str(facts[0], "verb_source"), None);
    assert_eq!(
        metadata_str(facts[0], "attribute_kind"),
        Some("request_mapping")
    );
    assert_eq!(
        metadata_str(facts[0], "normalized_route_template"),
        Some("/legacy")
    );
    assert_eq!(binding_symbol_name(&results, facts[0]), Some("legacy"));
}

#[test]
fn kotlin_single_line_mapping_binds_to_handler() {
    let source = format!(
        r#"{SPRING_IMPORT}

@RestController
class InlineController {{
    @GetMapping("/x") fun handler(): String {{ return "x" }}
}}
"#
    );
    let results = extract("src/InlineController.kt", &source);
    let facts = routes(&results);
    assert_eq!(facts.len(), 1, "{facts:#?}");
    assert_eq!(metadata_str(facts[0], "verb"), Some("GET"));
    assert_eq!(
        metadata_str(facts[0], "normalized_route_template"),
        Some("/x")
    );
    assert_eq!(binding_symbol_name(&results, facts[0]), Some("handler"));
}

#[test]
fn kotlin_bare_mapping_resolves_to_class_prefix_alone() {
    let source = format!(
        r#"{SPRING_IMPORT}

@RestController
@RequestMapping("/api")
class RootController {{
    @GetMapping
    fun index(): String {{ return "root" }}
}}
"#
    );
    let results = extract("src/RootController.kt", &source);
    let facts = routes(&results);
    assert_eq!(facts.len(), 1, "{facts:#?}");
    assert_eq!(metadata_str(facts[0], "route_template"), Some(""));
    assert_eq!(metadata_str(facts[0], "class_route_template"), Some("/api"));
    assert_eq!(
        metadata_str(facts[0], "effective_route_template"),
        Some("/api")
    );
    assert_eq!(
        metadata_str(facts[0], "normalized_route_template"),
        Some("/api")
    );
    assert_eq!(binding_symbol_name(&results, facts[0]), Some("index"));
}

#[test]
fn kotlin_object_and_companion_object_reset_prefix_per_type() {
    let source = format!(
        r#"{SPRING_IMPORT}

@RestController
@RequestMapping("/health")
object HealthController {{
    @GetMapping("/live")
    fun live(): String {{ return "ok" }}

    companion object {{
        @GetMapping("/ready")
        fun ready(): String {{ return "ok" }}
    }}
}}
"#
    );
    let results = extract("src/HealthController.kt", &source);
    let facts = routes(&results);

    // The object owns the /health prefix.
    let live = find_route(&facts, "/live");
    assert_eq!(
        metadata_str(live, "normalized_route_template"),
        Some("/health/live")
    );
    assert_eq!(binding_symbol_name(&results, live), Some("live"));

    // The companion object is its own prefix scope: no class prefix (it carries
    // no @RequestMapping), so /ready does not inherit /health.
    let ready = find_route(&facts, "/ready");
    assert_eq!(metadata_str(ready, "class_route_template"), None);
    assert_eq!(metadata_str(ready, "effective_route_template"), None);
    assert_eq!(
        metadata_str(ready, "normalized_route_template"),
        Some("/ready")
    );
    assert_eq!(binding_symbol_name(&results, ready), Some("ready"));
}

#[test]
fn kotlin_interpolated_and_concatenated_paths_stay_silent() {
    let source = format!(
        r#"{SPRING_IMPORT}

const val BASE = "/api"

@RestController
class DynamicController {{
    @GetMapping("$base/x")
    fun interpolated(): String {{ return "x" }}

    @GetMapping("${{base}}/y")
    fun braced(): String {{ return "y" }}

    @PostMapping("/a/" + suffix)
    fun concatenated(): String {{ return "z" }}

    @PutMapping(BASE)
    fun constRef(): String {{ return "w" }}
}}
"#
    );
    let results = extract("src/DynamicController.kt", &source);
    let facts = routes(&results);
    assert!(facts.is_empty(), "expected silence, got {facts:#?}");
}

#[test]
fn kotlin_non_static_class_prefix_poisons_join_but_keeps_route() {
    let source = format!(
        r#"{SPRING_IMPORT}

@RestController
@RequestMapping(BASE)
class PoisonedController {{
    @GetMapping("/users/{{id}}")
    fun getUser(): String {{ return "x" }}
}}
"#
    );
    let results = extract("src/PoisonedController.kt", &source);
    let facts = routes(&results);
    assert_eq!(facts.len(), 1, "{facts:#?}");
    // The class prefix is dynamic, so it is dropped; the method route still emits
    // with its own path only.
    assert_eq!(metadata_str(facts[0], "class_route_template"), None);
    assert_eq!(metadata_str(facts[0], "effective_route_template"), None);
    assert_eq!(
        metadata_str(facts[0], "route_template"),
        Some("/users/{id}")
    );
    assert_eq!(
        metadata_str(facts[0], "normalized_route_template"),
        Some("/users/:id")
    );
    assert_eq!(binding_symbol_name(&results, facts[0]), Some("getUser"));
}

#[test]
fn kotlin_requires_spring_import() {
    // The annotation shape exists but the import gate keeps the collector silent.
    let source = r#"@RestController
@RequestMapping("/api")
class UserController {
    @GetMapping("/users")
    fun getUsers(): String { return "x" }
}
"#;
    let results = extract("src/UserController.kt", source);
    assert!(routes(&results).is_empty());
}
