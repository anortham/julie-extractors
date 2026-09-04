use std::path::Path;

use crate::base::StructuralFact;
use crate::tests::helpers::metadata_str;

const NESTJS_ROUTE_PATTERN_ID: &str = "nestjs.route.v1";

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical extraction should succeed")
}

fn routes(results: &crate::ExtractionResults) -> Vec<&StructuralFact> {
    results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == NESTJS_ROUTE_PATTERN_ID)
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
fn nestjs_controller_method_decorators_emit_boundary_facts() {
    let source = r#"import { Controller, Get, Post, Delete } from '@nestjs/common';

@Controller('users')
export class UsersController {
  @Get()
  findAll() {
    return [];
  }

  @Get(':id')
  findOne() {
    return null;
  }

  @Post()
  create() {
    return null;
  }

  @Delete(':id')
  remove() {
    return null;
  }
}
"#;
    let results = extract("src/users.controller.ts", source);
    let facts = routes(&results);
    assert_eq!(facts.len(), 4, "{facts:#?}");

    // @Get() with an empty method path resolves to the class prefix alone (no
    // trailing slash), so its raw route_template is "".
    let find_all = find_route(&facts, "");
    assert_eq!(metadata_str(find_all, "framework"), Some("nestjs"));
    assert_eq!(
        metadata_str(find_all, "api_style"),
        Some("decorator_routing")
    );
    assert_eq!(metadata_str(find_all, "verb"), Some("GET"));
    assert_eq!(metadata_str(find_all, "verb_source"), Some("attested"));
    assert_eq!(
        metadata_str(find_all, "class_route_template"),
        Some("users")
    );
    assert_eq!(
        metadata_str(find_all, "effective_route_template"),
        Some("users")
    );
    assert_eq!(
        metadata_str(find_all, "normalized_route_template"),
        Some("/users")
    );
    assert_eq!(binding_symbol_name(&results, find_all), Some("findAll"));

    let find_one = find_route(&facts, ":id");
    assert_eq!(metadata_str(find_one, "verb"), Some("GET"));
    assert_eq!(
        metadata_str(find_one, "class_route_template"),
        Some("users")
    );
    assert_eq!(
        metadata_str(find_one, "effective_route_template"),
        Some("users/:id")
    );
    assert_eq!(
        metadata_str(find_one, "normalized_route_template"),
        Some("/users/:id")
    );
    assert_eq!(metadata_array(find_one, "dynamic_segments"), vec!["id"]);
    assert_eq!(binding_symbol_name(&results, find_one), Some("findOne"));

    let create = facts
        .iter()
        .find(|fact| metadata_str(fact, "verb") == Some("POST"))
        .expect("post route");
    assert_eq!(
        metadata_str(create, "normalized_route_template"),
        Some("/users")
    );
    assert_eq!(binding_symbol_name(&results, create), Some("create"));

    let remove = facts
        .iter()
        .find(|fact| metadata_str(fact, "verb") == Some("DELETE"))
        .expect("delete route");
    assert_eq!(
        metadata_str(remove, "normalized_route_template"),
        Some("/users/:id")
    );
    assert_eq!(binding_symbol_name(&results, remove), Some("remove"));
}

#[test]
fn nestjs_javascript_controller_emits_route_facts() {
    let source = r#"const { Controller, Get } = require('@nestjs/common');

@Controller('health')
class HealthController {
  @Get(':check')
  status() {
    return 'ok';
  }
}
"#;
    let results = extract("src/health.controller.js", source);
    let facts = routes(&results);
    assert_eq!(facts.len(), 1, "{facts:#?}");
    let status = facts[0];
    assert_eq!(metadata_str(status, "verb"), Some("GET"));
    assert_eq!(metadata_str(status, "class_route_template"), Some("health"));
    assert_eq!(
        metadata_str(status, "normalized_route_template"),
        Some("/health/:check")
    );
    assert_eq!(metadata_array(status, "dynamic_segments"), vec!["check"]);
    assert_eq!(binding_symbol_name(&results, status), Some("status"));
}

#[test]
fn nestjs_all_decorator_omits_verb() {
    let source = r#"import { Controller, All } from '@nestjs/common';

@Controller('gateway')
export class GatewayController {
  @All('proxy')
  proxy() {
    return null;
  }
}
"#;
    let results = extract("src/gateway.controller.ts", source);
    let facts = routes(&results);
    assert_eq!(facts.len(), 1, "{facts:#?}");
    assert_eq!(metadata_str(facts[0], "verb"), None);
    assert_eq!(metadata_str(facts[0], "verb_source"), None);
    assert_eq!(
        metadata_str(facts[0], "normalized_route_template"),
        Some("/gateway/proxy")
    );
}

#[test]
fn nestjs_dynamic_decorator_arguments_stay_silent() {
    let source = r#"import { Controller, Get, Post, Put, Delete } from '@nestjs/common';

const PATHS = { USER: '/user' };

@Controller('users')
export class UsersController {
  @Get(`/tpl/${id}`)
  interpolated() {
    return null;
  }

  @Post('/a/' + suffix)
  concatenated() {
    return null;
  }

  @Put(PATHS.USER)
  memberRef() {
    return null;
  }

  @Delete(routeVar)
  identifierRef() {
    return null;
  }
}
"#;
    let results = extract("src/users.controller.ts", source);
    let facts = routes(&results);
    assert!(facts.is_empty(), "expected silence, got {facts:#?}");
}

#[test]
fn nestjs_non_static_controller_prefix_poisons_join_but_keeps_route() {
    let source = r#"import { Controller, Get } from '@nestjs/common';

@Controller(BASE_PATH)
export class UsersController {
  @Get(':id')
  findOne() {
    return null;
  }
}
"#;
    let results = extract("src/users.controller.ts", source);
    let facts = routes(&results);
    assert_eq!(facts.len(), 1, "{facts:#?}");
    // The class prefix is dynamic, so it is dropped (poisoned); the method
    // route still emits with route_template only.
    assert_eq!(metadata_str(facts[0], "class_route_template"), None);
    assert_eq!(metadata_str(facts[0], "effective_route_template"), None);
    assert_eq!(metadata_str(facts[0], "route_template"), Some(":id"));
    assert_eq!(
        metadata_str(facts[0], "normalized_route_template"),
        Some("/:id")
    );
    assert_eq!(binding_symbol_name(&results, facts[0]), Some("findOne"));
}

#[test]
fn nestjs_controller_without_prefix_uses_method_template_only() {
    let source = r#"import { Controller, Get } from '@nestjs/common';

@Controller()
export class AppController {
  @Get('status')
  status() {
    return 'ok';
  }
}
"#;
    let results = extract("src/app.controller.ts", source);
    let facts = routes(&results);
    assert_eq!(facts.len(), 1, "{facts:#?}");
    assert_eq!(metadata_str(facts[0], "class_route_template"), None);
    assert_eq!(metadata_str(facts[0], "effective_route_template"), None);
    assert_eq!(
        metadata_str(facts[0], "normalized_route_template"),
        Some("/status")
    );
}

#[test]
fn nestjs_controller_object_and_array_prefixes_join() {
    let source = r#"import { Controller, Get } from '@nestjs/common';

@Controller({ path: 'objs', version: '1' })
export class ObjController {
  @Get(':id')
  one() {
    return null;
  }
}

@Controller(['a', 'b'])
export class ArrController {
  @Get('list')
  list() {
    return null;
  }
}
"#;
    let results = extract("src/controllers.ts", source);
    let facts = routes(&results);

    let obj = find_route(&facts, ":id");
    assert_eq!(metadata_str(obj, "class_route_template"), Some("objs"));
    assert_eq!(
        metadata_str(obj, "normalized_route_template"),
        Some("/objs/:id")
    );

    // Array prefix cross-products with the method template.
    let mut normalized: Vec<&str> = facts
        .iter()
        .filter(|fact| metadata_str(fact, "route_template") == Some("list"))
        .filter_map(|fact| metadata_str(fact, "normalized_route_template"))
        .collect();
    normalized.sort_unstable();
    assert_eq!(normalized, vec!["/a/list", "/b/list"], "{facts:#?}");
}

#[test]
fn nestjs_requires_nest_import() {
    // No `@nestjs/common` import: the decorator shape exists but the import gate
    // keeps the collector silent.
    let source = r#"@Controller('users')
export class UsersController {
  @Get(':id')
  findOne() {
    return null;
  }
}
"#;
    let results = extract("src/users.controller.ts", source);
    assert!(routes(&results).is_empty());
}
