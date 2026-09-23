use crate::base::{
    ExtractionResults, IdentifierKind, StructuralFact, Symbol, SymbolKind, Visibility,
};
use crate::extract_canonical;
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("r extraction")
}

fn named<'a>(result: &'a ExtractionResults, name: &str) -> Vec<&'a Symbol> {
    result.symbols.iter().filter(|s| s.name == name).collect()
}

fn one<'a>(result: &'a ExtractionResults, name: &str) -> &'a Symbol {
    let found = named(result, name);
    assert_eq!(found.len(), 1, "{name}: {found:#?}");
    found[0]
}

fn meta<'a>(symbol: &'a Symbol, key: &str) -> Option<&'a str> {
    symbol.metadata.as_ref()?.get(key)?.as_str()
}

fn type_of(result: &ExtractionResults, symbol: &Symbol) -> Option<String> {
    result
        .types
        .get(&symbol.id)
        .map(|info| info.resolved_type.clone())
}

fn facts<'a>(result: &'a ExtractionResults, pattern: &str) -> Vec<&'a StructuralFact> {
    result
        .structural_facts
        .iter()
        .filter(|f| f.pattern_id == pattern)
        .collect()
}

fn fact_str<'a>(fact: &'a StructuralFact, key: &str) -> Option<&'a str> {
    fact.metadata.as_ref()?.get(key)?.as_str()
}

fn variable_refs(result: &ExtractionResults) -> Vec<&str> {
    result
        .identifiers
        .iter()
        .filter(|i| i.kind == IdentifierKind::VariableRef)
        .map(|i| i.name.as_str())
        .collect()
}

#[test]
fn argument_names_loop_variables_and_placeholders_are_not_references() {
    let source = "build <- function(amount, df, cols) {\n  obj <- structure(list(amount = amount), class = \"money\")\n  for (col in cols) print(col)\n  dplyr::summarise(df, revenue = sum(amount), .groups = \"drop\")\n  df |> lm(y ~ x, data = _)\n  df %>% head(n = .)\n}\n";
    let result = extract("build.R", source);
    let refs = variable_refs(&result);
    for name in ["class", "revenue", ".groups", "data", "_", "."] {
        assert!(!refs.contains(&name), "{name}: {refs:?}");
    }
    assert_eq!(
        refs.iter().filter(|name| **name == "amount").count(),
        2,
        "{refs:?}"
    );
    assert_eq!(
        refs.iter().filter(|name| **name == "col").count(),
        1,
        "{refs:?}"
    );
}

#[test]
fn assigned_set_class_yields_one_documented_class() {
    let source = "#' A person.\nPerson <- setClass(\"Person\", slots = c(name = \"character\"))\n";
    let result = extract("person.R", source);
    let person = one(&result, "Person");
    assert_eq!(person.kind, SymbolKind::Class);
    assert!(person.doc_comment.is_some());
}

#[test]
fn s4_slots_and_refclass_fields_are_typed_field_symbols() {
    let source = r#"setClass("Person", representation(name = "character", age = "numeric"))
setClass("Employee", contains = "Person", slots = c(boss = "Person", salary = "numeric"))
Queue <- setRefClass("Queue", fields = list(items = "list", owner = "Person"))
"#;
    let result = extract("s4.R", source);
    let person = one(&result, "Person");
    let employee = one(&result, "Employee");
    let queue = one(&result, "Queue");
    for (field, owner, type_name) in [
        ("name", person, "character"),
        ("age", person, "numeric"),
        ("boss", employee, "Person"),
        ("salary", employee, "numeric"),
        ("items", queue, "list"),
        ("owner", queue, "Person"),
    ] {
        let symbol = one(&result, field);
        assert_eq!(symbol.kind, SymbolKind::Field, "{field}");
        assert_eq!(
            symbol.parent_id.as_deref(),
            Some(owner.id.as_str()),
            "{field}"
        );
        assert_eq!(
            type_of(&result, symbol).as_deref(),
            Some(type_name),
            "{field}"
        );
        assert!(!result.types[&symbol.id].is_inferred, "{field}");
    }
}

#[test]
fn r6_active_bindings_refclass_methods_and_visibility() {
    let source = r#"Animal <- R6::R6Class("Animal",
  public = list(name = NULL, speak = function() 1),
  private = list(secret = NULL, log_it = function(msg) message(msg)),
  active = list(upper_name = function(value) toupper(self$name)))
Account <- setRefClass("Account", fields = c(balance = "numeric"))
Account$methods(withdraw = function(x) { balance <<- balance - x },
  report = function() cat(balance))
#' @export
money <- function(amount) amount
.internal <- function() 1
"#;
    let result = extract("classes.R", source);
    let animal = one(&result, "Animal");
    let upper = one(&result, "upper_name");
    assert_eq!(upper.parent_id.as_deref(), Some(animal.id.as_str()));
    assert_eq!(meta(upper, "r6_member_kind"), Some("active"));
    assert_eq!(upper.visibility, Some(Visibility::Public));
    assert_eq!(one(&result, "secret").visibility, Some(Visibility::Private));
    assert_eq!(one(&result, "log_it").visibility, Some(Visibility::Private));
    assert_eq!(one(&result, "speak").visibility, Some(Visibility::Public));
    let account = one(&result, "Account");
    let balance = result
        .symbols
        .iter()
        .find(|s| s.name == "balance" && s.kind == SymbolKind::Field)
        .expect("balance field");
    assert_eq!(balance.parent_id.as_deref(), Some(account.id.as_str()));
    for method in ["withdraw", "report"] {
        let symbol = one(&result, method);
        assert_eq!(symbol.kind, SymbolKind::Method);
        assert_eq!(symbol.parent_id.as_deref(), Some(account.id.as_str()));
    }
    assert_eq!(one(&result, "money").visibility, Some(Visibility::Public));
    assert_eq!(
        one(&result, ".internal").visibility,
        Some(Visibility::Private)
    );
}

#[test]
fn package_load_forms_become_imports() {
    let source = r#"source(file.path("R", "helpers.R"), local = TRUE)
source(file.path(dir, "x.R"))
requireNamespace("jsonlite")
box::use(dplyr[filter, select], app/logic/utils)
import::from(magrittr, "%>%")
pacman::p_load(dplyr, tidyr)
"#;
    let result = extract("imports.R", source);
    let imports: Vec<&str> = result
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Import)
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(
        imports,
        vec![
            "R/helpers.R",
            "jsonlite",
            "dplyr",
            "app/logic/utils",
            "magrittr",
            "dplyr",
            "tidyr"
        ]
    );
    let loads = facts(&result, "r.library_call.v1");
    assert_eq!(loads.len(), 4, "{loads:#?}");
}

#[test]
fn testthat_namespaced_calls_lifecycle_and_runit_conventions() {
    let testthat = "setup({ tmp <- tempfile() })\nteardown({ unlink(tmp) })\ntestthat::test_that(\"qualified\", {\n  expect_true(TRUE)\n})\ntest_helper_value <- function() 42\n";
    let result = extract("tests/testthat/test-legacy.R", testthat);
    assert_eq!(
        meta(one(&result, "qualified"), "test_role"),
        Some("test_case")
    );
    assert_eq!(
        meta(one(&result, "setup"), "test_role"),
        Some("fixture_setup")
    );
    assert_eq!(
        meta(one(&result, "teardown"), "test_role"),
        Some("fixture_teardown")
    );
    assert_eq!(meta(one(&result, "test_helper_value"), "test_role"), None);

    let runit = ".setUp <- function() options(x = 1)\n.tearDown <- function() options(x = NULL)\ntestMoneyPrint <- function() checkTrue(TRUE)\ntest.money_creation <- function() checkTrue(TRUE)\n";
    let result = extract("inst/unitTests/runit.money.R", runit);
    assert_eq!(
        meta(one(&result, ".setUp"), "test_role"),
        Some("fixture_setup")
    );
    assert_eq!(
        meta(one(&result, ".tearDown"), "test_role"),
        Some("fixture_teardown")
    );
    assert_eq!(
        meta(one(&result, "testMoneyPrint"), "test_role"),
        Some("test_case")
    );
    assert_eq!(
        meta(one(&result, "test.money_creation"), "test_role"),
        Some("test_case")
    );

    let production = "test_connection <- function(url) httr::HEAD(url)\n";
    let result = extract("R/test_utils.R", production);
    assert_eq!(meta(one(&result, "test_connection"), "test_role"), None);
}

#[test]
fn pipe_and_formula_facts_follow_the_operator() {
    let source = r#"fit <- function(df, home) {
  if (home == "~/data") stop("bad")
  model = y ~ x + z
  df %>% filter(x > 1) %>% mutate(z = x * 2)
  lm(y ~ x, data = df) |> summary()
  facet_wrap(~ cyl)
}
"#;
    let result = extract("fit.R", source);
    let formulas: Vec<_> = facts(&result, "r.formula_expression.v1")
        .into_iter()
        .map(|f| fact_str(f, "formula_text").unwrap().to_string())
        .collect();
    assert_eq!(formulas, vec!["y ~ x + z", "y ~ x", "~ cyl"]);
    let pipes = facts(&result, "r.pipe_expression.v1");
    assert_eq!(pipes.len(), 2, "{pipes:#?}");
    assert_eq!(fact_str(pipes[0], "pipe_operator"), Some("%>%"));
    assert_eq!(
        pipes[0].metadata.as_ref().unwrap()["stage_count"],
        serde_json::json!(2)
    );
    assert_eq!(fact_str(pipes[1], "pipe_operator"), Some("|>"));
}

#[test]
fn s7_classes_generics_and_methods() {
    let source = r#"Dog <- S7::new_class("Dog",
  properties = list(name = S7::class_character, age = S7::class_numeric))
speak <- S7::new_generic("speak", "x")
S7::method(speak, Dog) <- function(x) cat("Woof from", x@name)
make_dog <- function() {
  d <- Dog(name = "Rex")
  speak(d)
}
"#;
    let result = extract("s7.R", source);
    let dog = one(&result, "Dog");
    assert_eq!(dog.kind, SymbolKind::Class);
    assert_eq!(meta(dog, "r_class_system"), Some("S7"));
    let name = one(&result, "name");
    assert_eq!(name.parent_id.as_deref(), Some(dog.id.as_str()));
    assert_eq!(type_of(&result, name).as_deref(), Some("character"));
    assert_eq!(one(&result, "speak").kind, SymbolKind::Function);
    let method = one(&result, "speak,Dog");
    assert_eq!(method.kind, SymbolKind::Method);
    assert_eq!(type_of(&result, one(&result, "d")).as_deref(), Some("Dog"));
}

#[test]
fn plumber_annotations_and_router_calls_emit_routes_and_handlers() {
    let source = r#"library(plumber)
#* Echo back the input
#* @get /echo
function(msg = "") {
  list(msg = msg)
}
#* @post /users/<id:int>
function(id) find_user(id)
pr() %>% pr_get("/health", function() list(ok = TRUE))
"#;
    let result = extract("plumber.R", source);
    let routes = facts(&result, "plumber.route.v1");
    assert_eq!(routes.len(), 3, "{routes:#?}");
    assert_eq!(fact_str(routes[0], "verb"), Some("GET"));
    assert_eq!(fact_str(routes[0], "route_template"), Some("/echo"));
    assert_eq!(
        fact_str(routes[1], "normalized_route_template"),
        Some("/users/:id")
    );
    assert_eq!(fact_str(routes[2], "verb"), Some("GET"));
    let handler = one(&result, "POST /users/<id:int>");
    assert_eq!(handler.kind, SymbolKind::Function);
    assert!(result.structured_pending_relationships.iter().any(|p| {
        p.pending.from_symbol_id == handler.id && p.target.terminal_name == "find_user"
    }));
}

#[test]
fn shiny_apps_emit_framework_facts() {
    let source = r#"library(shiny)
ui <- fluidPage(selectInput("region", "Region", choices = c("N", "S")), plotOutput("salesPlot"))
server <- function(input, output, session) {
  sales <- reactive({ load_sales() })
  observeEvent(input$refresh, { showNotification("Refreshed") })
  output$salesPlot <- renderPlot({ plot(sales()) })
  moduleServer("detail", function(input, output, session) {})
}
shinyApp(ui = ui, server = server)
"#;
    let result = extract("app.R", source);
    let inputs = facts(&result, "shiny.input.v1");
    assert_eq!(inputs.len(), 1);
    assert_eq!(fact_str(inputs[0], "input_id"), Some("region"));
    assert_eq!(fact_str(inputs[0], "widget"), Some("selectInput"));
    let outputs = facts(&result, "shiny.output.v1");
    assert_eq!(outputs.len(), 2, "{outputs:#?}");
    assert_eq!(fact_str(outputs[0], "role"), Some("placeholder"));
    assert_eq!(fact_str(outputs[1], "role"), Some("render"));
    assert_eq!(fact_str(outputs[1], "function"), Some("renderPlot"));
    let reactives = facts(&result, "shiny.reactive.v1");
    assert_eq!(reactives.len(), 2, "{reactives:#?}");
    assert_eq!(fact_str(reactives[0], "name"), Some("sales"));
    assert_eq!(fact_str(reactives[1], "trigger"), Some("input$refresh"));
    assert_eq!(facts(&result, "shiny.module.v1").len(), 1);
    assert_eq!(facts(&result, "shiny.app.v1").len(), 1);
}

#[test]
fn package_namespace_files_are_r_with_imports_and_directive_facts() {
    let source = "export(add_one)\nS3method(print,shape_summary)\nimportFrom(dplyr,filter)\nimport(R6, jsonlite)\n";
    assert_eq!(
        crate::language_spec::detect_language_for_source("pkg/NAMESPACE", source),
        Some("r")
    );
    let result = extract("pkg/NAMESPACE", source);
    let imports: Vec<&str> = result
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Import)
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(imports, vec!["dplyr", "R6", "jsonlite"]);
    let directives = facts(&result, "r.namespace_directive.v1");
    assert_eq!(directives.len(), 4);
    assert_eq!(fact_str(directives[1], "directive"), Some("S3method"));
    assert_eq!(
        directives[1].metadata.as_ref().unwrap()["arguments"],
        serde_json::json!(["print", "shape_summary"])
    );
    let plain = extract("scripts/run.R", "import(R6)\n");
    assert!(plain.symbols.iter().all(|s| s.kind != SymbolKind::Import));
}
