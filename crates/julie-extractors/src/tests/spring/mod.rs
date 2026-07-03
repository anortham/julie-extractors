use std::path::Path;

use crate::base::StructuralFact;

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

fn metadata_str<'a>(fact: &'a StructuralFact, key: &str) -> Option<&'a str> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_str())
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

#[test]
fn spring_class_and_method_mappings_emit_boundary_facts() {
    let source = r#"
import org.springframework.web.bind.annotation.*;

@RestController
@RequestMapping("/api")
class UserController {
    @GetMapping("/users/{id}")
    public User getUser() { return null; }

    @PostMapping({"/users", "/members"})
    public User create() { return null; }

    @RequestMapping(method = {RequestMethod.GET, RequestMethod.POST}, path = "/search/{term}")
    public User search() { return null; }
}
"#;
    let results = extract("src/UserController.java", source);
    let facts = routes(&results);
    assert_eq!(facts.len(), 6, "{facts:#?}");

    let class_route = facts
        .iter()
        .find(|fact| metadata_str(fact, "attribute_kind") == Some("class_route"))
        .expect("class route");
    assert_eq!(metadata_str(class_route, "route_template"), Some("/api"));

    let get = facts
        .iter()
        .find(|fact| metadata_str(fact, "route_template") == Some("/users/{id}"))
        .expect("get route");
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

    let search_verbs = facts
        .iter()
        .filter(|fact| metadata_str(fact, "route_template") == Some("/search/{term}"))
        .filter_map(|fact| metadata_str(fact, "verb"))
        .collect::<Vec<_>>();
    assert_eq!(search_verbs, vec!["GET", "POST"]);
}

#[test]
fn spring_class_prefix_does_not_leak_into_unmapped_controllers() {
    let source = r#"
import org.springframework.web.bind.annotation.*;

@RestController
@RequestMapping("/api/users")
class UserController {
    @GetMapping("/{id}")
    public User getUser() { return null; }
}

@RestController
class HealthController {
    @GetMapping("/healthz")
    public String health() { return "ok"; }
}
"#;
    let results = extract("src/Controllers.java", source);
    let facts = routes(&results);

    let health = facts
        .iter()
        .find(|fact| metadata_str(fact, "route_template") == Some("/healthz"))
        .expect("health route");
    assert_eq!(metadata_str(health, "class_route_template"), None);
    assert_eq!(metadata_str(health, "effective_route_template"), None);
    assert_eq!(
        metadata_str(health, "normalized_route_template"),
        Some("/healthz")
    );
}

#[test]
fn spring_non_literal_method_mapping_stays_silent() {
    let source = r#"
import org.springframework.web.bind.annotation.*;

@RestController
@RequestMapping("/api")
class UserController {
    static final String PATH_USERS = "/users";

    @GetMapping(PATH_USERS)
    public User getUser() { return null; }
}
"#;
    let results = extract("src/UserController.java", source);
    let facts = routes(&results);
    assert_eq!(facts.len(), 1, "{facts:#?}");
    assert_eq!(
        metadata_str(facts[0], "attribute_kind"),
        Some("class_route")
    );
    assert_eq!(metadata_str(facts[0], "route_template"), Some("/api"));
}

#[test]
fn spring_interface_controller_mappings_reset_class_prefix_and_apply_own_template() {
    let source = r#"
import org.springframework.web.bind.annotation.*;

@RestController
@RequestMapping("/api")
class FirstController {
}

@RequestMapping("/iface")
interface UserApi {
    @GetMapping("/users")
    String users();
}
"#;
    let results = extract("src/UserApi.java", source);
    let facts = routes(&results);
    assert_eq!(facts.len(), 3, "{facts:#?}");

    let iface = facts
        .iter()
        .find(|fact| metadata_str(fact, "route_template") == Some("/iface"))
        .expect("interface route");
    assert_eq!(metadata_str(iface, "attribute_kind"), Some("class_route"));
    assert_eq!(metadata_str(iface, "effective_route_template"), None);
    assert_eq!(
        metadata_str(iface, "normalized_route_template"),
        Some("/iface")
    );

    let method = facts
        .iter()
        .find(|fact| metadata_str(fact, "route_template") == Some("/users"))
        .expect("interface method route");
    assert_eq!(metadata_str(method, "class_route_template"), Some("/iface"));
    assert_eq!(
        metadata_str(method, "effective_route_template"),
        Some("/iface/users")
    );
    assert_eq!(
        metadata_str(method, "normalized_route_template"),
        Some("/iface/users")
    );
}

#[test]
fn spring_class_mapping_arrays_cross_product_with_method_templates() {
    let source = r#"
import org.springframework.web.bind.annotation.*;

@RestController
@RequestMapping({"/api", "/v2"})
class UserController {
    @GetMapping({"/users", "/members"})
    public User getUser() { return null; }
}
"#;
    let results = extract("src/UserController.java", source);
    let facts = routes(&results);
    let class_routes = facts
        .iter()
        .filter(|fact| metadata_str(fact, "attribute_kind") == Some("class_route"))
        .count();
    assert_eq!(class_routes, 2, "{facts:#?}");

    let effective = facts
        .iter()
        .filter_map(|fact| metadata_str(fact, "effective_route_template"))
        .collect::<Vec<_>>();
    assert_eq!(
        effective,
        vec!["/api/users", "/api/members", "/v2/users", "/v2/members"]
    );
}

#[test]
fn spring_produces_and_consumes_literals_are_not_route_templates() {
    let source = r#"
import org.springframework.web.bind.annotation.*;

@RestController
class UserController {
    @GetMapping(value = "/users", produces = "application/json")
    public User list() { return null; }

    @PostMapping(path = "/users", consumes = "application/json", headers = "X-Flag=1")
    public User create() { return null; }
}
"#;
    let results = extract("src/UserController.java", source);
    let facts = routes(&results);
    assert_eq!(facts.len(), 2, "{facts:#?}");
    let templates = facts
        .iter()
        .filter_map(|fact| metadata_str(fact, "route_template"))
        .collect::<Vec<_>>();
    assert_eq!(templates, vec!["/users", "/users"]);
}

#[test]
fn spring_bare_method_level_request_mapping_emits_request_mapping_kind() {
    let source = r#"
import org.springframework.web.bind.annotation.*;

@RestController
class LegacyController {
    @RequestMapping("/legacy")
    public String legacy() { return "ok"; }
}
"#;
    let results = extract("src/LegacyController.java", source);
    let facts = routes(&results);
    assert_eq!(facts.len(), 1, "{facts:#?}");
    assert_eq!(
        metadata_str(facts[0], "attribute_kind"),
        Some("request_mapping")
    );
    assert_eq!(metadata_str(facts[0], "verb"), None);
    assert_eq!(metadata_str(facts[0], "verb_source"), None);
}
