//! Symfony `#[Route]` attribute structural facts (`symfony.route.v1`).

use std::path::Path;

use crate::base::StructuralFact;
use crate::tests::helpers::metadata_str;

const SYMFONY_ROUTE_PATTERN_ID: &str = "symfony.route.v1";
const LARAVEL_ROUTE_PATTERN_ID: &str = "laravel.route.v1";

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical extraction should succeed")
}

fn routes(results: &crate::ExtractionResults) -> Vec<&StructuralFact> {
    results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == SYMFONY_ROUTE_PATTERN_ID)
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

#[test]
fn symfony_method_route_emits_verb_template_and_binds_to_handler() {
    let source = r#"<?php
use Symfony\Component\Routing\Attribute\Route;

class UserController {
    #[Route('/users/{id}', methods: ['GET'])]
    public function show($id) {
        return $id;
    }

    #[Route('/users', methods: ['POST'])]
    public function create() {
        return null;
    }
}
"#;
    let results = extract("src/Controller/UserController.php", source);
    let facts = routes(&results);
    assert_eq!(facts.len(), 2, "{facts:#?}");

    let show = find_route(&facts, "/users/{id}");
    assert_eq!(metadata_str(show, "framework"), Some("symfony"));
    assert_eq!(metadata_str(show, "api_style"), Some("annotation_routing"));
    assert_eq!(metadata_str(show, "attribute_kind"), Some("http_method"));
    assert_eq!(metadata_str(show, "verb"), Some("GET"));
    assert_eq!(metadata_str(show, "verb_source"), Some("attested"));
    assert_eq!(
        metadata_str(show, "normalized_route_template"),
        Some("/users/:id")
    );
    assert_eq!(metadata_array(show, "dynamic_segments"), vec!["id"]);
    assert_eq!(binding_symbol_name(&results, show), Some("show"));

    let create = facts
        .iter()
        .find(|fact| metadata_str(fact, "verb") == Some("POST"))
        .expect("post route");
    assert_eq!(
        metadata_str(create, "normalized_route_template"),
        Some("/users")
    );
    assert_eq!(binding_symbol_name(&results, create), Some("create"));

    assert!(
        results
            .structural_facts
            .iter()
            .all(|fact| fact.pattern_id != LARAVEL_ROUTE_PATTERN_ID)
    );
}

#[test]
fn symfony_class_route_prefix_joins_method_templates() {
    let source = r#"<?php
use Symfony\Component\Routing\Annotation\Route;

#[Route('/api')]
class UserController {
    #[Route('/users/{id}', methods: ['GET'])]
    public function show($id) {
        return $id;
    }
}
"#;
    let results = extract("src/Controller/UserController.php", source);
    let facts = routes(&results);

    let class_route = facts
        .iter()
        .find(|fact| metadata_str(fact, "attribute_kind") == Some("class_route"))
        .expect("class_route fact");
    assert_eq!(metadata_str(class_route, "route_template"), Some("/api"));
    assert_eq!(
        metadata_str(class_route, "normalized_route_template"),
        Some("/api")
    );
    assert_eq!(
        binding_symbol_name(&results, class_route),
        Some("UserController")
    );

    let method = find_route(&facts, "/users/{id}");
    assert_eq!(metadata_str(method, "attribute_kind"), Some("http_method"));
    assert_eq!(metadata_str(method, "class_route_template"), Some("/api"));
    assert_eq!(
        metadata_str(method, "effective_route_template"),
        Some("/api/users/{id}")
    );
    assert_eq!(
        metadata_str(method, "normalized_route_template"),
        Some("/api/users/:id")
    );
    assert_eq!(binding_symbol_name(&results, method), Some("show"));
}

#[test]
fn symfony_route_without_methods_omits_verb() {
    let source = r#"<?php
use Symfony\Component\Routing\Attribute\Route;

class WebhookController {
    #[Route('/webhook')]
    public function handle() {
        return null;
    }
}
"#;
    let results = extract("src/Controller/WebhookController.php", source);
    let facts = routes(&results);
    assert_eq!(facts.len(), 1, "{facts:#?}");
    let fact = facts[0];
    assert_eq!(metadata_str(fact, "route_template"), Some("/webhook"));
    assert_eq!(metadata_str(fact, "verb"), None);
    assert_eq!(metadata_str(fact, "verb_source"), None);
    assert_eq!(
        metadata_str(fact, "attribute_kind"),
        Some("request_mapping")
    );
}

#[test]
fn symfony_multi_method_array_emits_one_fact_per_verb() {
    let source = r#"<?php
use Symfony\Component\Routing\Attribute\Route;

class SearchController {
    #[Route('/search', methods: ['GET', 'POST'])]
    public function search() {
        return null;
    }
}
"#;
    let results = extract("src/Controller/SearchController.php", source);
    let facts = routes(&results);
    assert_eq!(facts.len(), 2, "{facts:#?}");
    let mut verbs: Vec<&str> = facts
        .iter()
        .filter_map(|fact| metadata_str(fact, "verb"))
        .collect();
    verbs.sort_unstable();
    assert_eq!(verbs, vec!["GET", "POST"]);
}

#[test]
fn symfony_dynamic_path_args_stay_silent() {
    let source = r#"<?php
use Symfony\Component\Routing\Attribute\Route;

class DynController {
    #[Route("/users/$id")]
    public function interpolated($id) {
        return $id;
    }

    #[Route('/users/' . $suffix)]
    public function concatenated($suffix) {
        return $suffix;
    }
}
"#;
    let results = extract("src/Controller/DynController.php", source);
    assert!(
        routes(&results).is_empty(),
        "interpolated/concat #[Route] paths must stay silent: {:#?}",
        routes(&results)
    );
}

#[test]
fn symfony_requires_routing_import() {
    let source = r#"<?php
class UserController {
    #[Route('/users/{id}', methods: ['GET'])]
    public function show($id) {
        return $id;
    }
}
"#;
    let results = extract("src/Controller/UserController.php", source);
    assert!(routes(&results).is_empty());
}

#[test]
fn symfony_names_in_comments_and_strings_do_not_activate_routes() {
    let source = r#"<?php
// use Symfony\Component\Routing\Attribute\Route;
$frameworkHint = 'Symfony\Component\Routing\Attribute\Route';

class UserController {
    #[Route('/users/{id}', methods: ['GET'])]
    public function show($id) {
        return $id;
    }
}
"#;
    let results = extract("src/Controller/UserController.php", source);
    assert!(routes(&results).is_empty(), "{:#?}", routes(&results));
}

#[test]
fn symfony_name_in_comment_inside_unrelated_use_does_not_activate_routes() {
    let source = r#"<?php
use /* Symfony\Component\Routing\Attribute\Route */ App\Controller\BaseController;

class UserController {
    #[Route('/users/{id}', methods: ['GET'])]
    public function show($id) {
        return $id;
    }
}
"#;
    let results = extract("src/Controller/UserController.php", source);
    assert!(routes(&results).is_empty(), "{:#?}", routes(&results));
}
