use crate::base::{ExtractionResults, IdentifierKind, RelationshipKind, Symbol, SymbolKind};
use crate::extract_canonical;
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("r extraction")
}

fn symbol<'a>(result: &'a ExtractionResults, name: &str) -> &'a Symbol {
    result
        .symbols
        .iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("missing {name}: {:#?}", result.symbols))
}

fn edge(result: &ExtractionResults, kind: RelationshipKind, from: &str, to: &str) -> bool {
    let from = symbol(result, from);
    let to = symbol(result, to);
    result
        .relationships
        .iter()
        .any(|r| r.kind == kind && r.from_symbol_id == from.id && r.to_symbol_id == to.id)
}

fn pending_from<'a>(
    result: &'a ExtractionResults,
    kind: RelationshipKind,
    from: &str,
) -> Vec<&'a crate::base::StructuredPendingRelationship> {
    let from = symbol(result, from);
    result
        .structured_pending_relationships
        .iter()
        .filter(|p| p.pending.kind == kind && p.pending.from_symbol_id == from.id)
        .collect()
}

#[test]
fn calls_inside_tests_never_target_a_describe_container() {
    let source = r#"test_that("add_one increments", {
  expect_equal(add_one(1), 2)
})
describe("add_one", {
  it("works on vectors", { expect_equal(add_one(1:2), 2:3) })
})
"#;
    let result = extract("tests/testthat/test-math.R", source);
    let container = result
        .symbols
        .iter()
        .find(|s| s.name == "add_one")
        .expect("describe container");
    assert!(
        result
            .relationships
            .iter()
            .all(|r| r.to_symbol_id != container.id)
    );
    let targets: Vec<_> = pending_from(&result, RelationshipKind::Calls, "add_one increments")
        .iter()
        .map(|p| p.target.display_name.clone())
        .collect();
    assert!(targets.contains(&"add_one".to_string()), "{targets:?}");
}

#[test]
fn r6_self_private_and_super_calls_resolve_with_receiver_types() {
    let source = r#"Queue <- R6::R6Class("Queue", public = list(
  add = function(x) { self$size() },
  size = function() 1))
Animal <- R6::R6Class("Animal", public = list(speak = function() private$log_it("s")),
  private = list(log_it = function(msg) message(msg)))
Dog <- R6::R6Class("Dog", inherit = Animal, public = list(bark = function() super$speak()))
"#;
    let result = extract("r6.R", source);
    assert!(edge(&result, RelationshipKind::Calls, "add", "size"));
    assert!(edge(&result, RelationshipKind::Calls, "speak", "log_it"));
    assert!(edge(&result, RelationshipKind::Calls, "bark", "speak"));
    let private_call = result
        .identifiers
        .iter()
        .find(|i| i.name == "log_it" && i.kind == IdentifierKind::Call)
        .expect("log_it call identifier");
    let receiver_type = |i: &crate::base::Identifier| i.receiver_type.clone();
    assert_eq!(receiver_type(private_call).as_deref(), Some("Animal"));
    let super_call = result
        .identifiers
        .iter()
        .find(|i| i.name == "speak" && i.kind == IdentifierKind::Call)
        .expect("speak call identifier");
    assert_eq!(receiver_type(super_call).as_deref(), Some("Animal"));
}

#[test]
fn only_known_generic_prefixes_make_s3_methods() {
    let source = r#".onLoad <- function(libname, pkgname) { .register_hooks() }
.register_hooks <- function() invisible(NULL)
as.data.frame.my_tbl <- function(x, ...) x
print.money <- function(x, ...) cat(x)
load.config <- function(path) yaml::read_yaml(path)
area <- function(shape) UseMethod("area")
area.circle <- function(shape) pi
run <- function() { .register_hooks(); load.config("a") }
"#;
    let result = extract("dots.R", source);
    let kind_and_s3 = |name: &str| {
        let s = symbol(&result, name);
        let meta = |key: &str| {
            s.metadata
                .as_ref()
                .and_then(|m| m.get(key))
                .and_then(|v| v.as_str())
                .map(str::to_string)
        };
        (s.kind.clone(), meta("s3_method"), meta("s3_class"))
    };
    assert_eq!(kind_and_s3(".onLoad"), (SymbolKind::Function, None, None));
    assert_eq!(
        kind_and_s3(".register_hooks"),
        (SymbolKind::Function, None, None)
    );
    assert_eq!(
        kind_and_s3("load.config"),
        (SymbolKind::Function, None, None)
    );
    assert_eq!(
        kind_and_s3("as.data.frame.my_tbl"),
        (
            SymbolKind::Method,
            Some("as.data.frame".to_string()),
            Some("my_tbl".to_string())
        )
    );
    assert_eq!(
        kind_and_s3("print.money"),
        (
            SymbolKind::Method,
            Some("print".to_string()),
            Some("money".to_string())
        )
    );
    assert_eq!(
        kind_and_s3("area.circle"),
        (
            SymbolKind::Method,
            Some("area".to_string()),
            Some("circle".to_string())
        )
    );
    assert!(edge(
        &result,
        RelationshipKind::Calls,
        ".onLoad",
        ".register_hooks"
    ));
    assert!(edge(
        &result,
        RelationshipKind::Calls,
        "run",
        ".register_hooks"
    ));
    assert!(edge(&result, RelationshipKind::Calls, "run", "load.config"));
}

#[test]
fn class_inheritance_emits_extends_edges() {
    let source = r#"Animal <- R6::R6Class("Animal", public = list(speak = function() 1))
Dog <- R6::R6Class("Dog", inherit = Animal, public = list())
Cat <- R6::R6Class("Cat", inherit = pets::Pet)
setClass("Shape", representation("VIRTUAL"))
setClass("Circle", contains = "Shape", slots = c(radius = "numeric"))
Base <- setRefClass("Base", fields = list(id = "numeric"))
Child <- setRefClass("Child", contains = "Base")
Account <- setRefClass("Account", contains = "Remote")
"#;
    let result = extract("classes.R", source);
    assert!(edge(&result, RelationshipKind::Extends, "Dog", "Animal"));
    assert!(edge(&result, RelationshipKind::Extends, "Circle", "Shape"));
    assert!(edge(&result, RelationshipKind::Extends, "Child", "Base"));
    let child = symbol(&result, "Child");
    assert_eq!(
        child
            .metadata
            .as_ref()
            .and_then(|m| m.get("contains"))
            .and_then(|v| v.as_str()),
        Some("Base")
    );
    let cat = pending_from(&result, RelationshipKind::Extends, "Cat");
    assert_eq!(cat.len(), 1);
    assert_eq!(cat[0].target.terminal_name, "Pet");
    assert_eq!(cat[0].target.namespace_path, vec!["pets".to_string()]);
    let account = pending_from(&result, RelationshipKind::Extends, "Account");
    assert_eq!(account[0].target.terminal_name, "Remote");
}

#[test]
fn braceless_function_bodies_hash_their_own_expression() {
    let source = r#"f_add <- function(x) x + 1
f_mul <- function(x) x * 2
f_lambda <- \(x) x - 3
Plain <- R6::R6Class("Plain", public = list(run = function(x) x + 1, go = function(x) x * 3))
"#;
    let result = extract("body.R", source);
    let body_text = |name: &str| {
        let span = symbol(&result, name).body_span.expect("body span");
        source[span.start_byte as usize..span.end_byte as usize].to_string()
    };
    assert_eq!(body_text("f_add"), "x + 1");
    assert_eq!(body_text("f_mul"), "x * 2");
    assert_eq!(body_text("f_lambda"), "x - 3");
    assert_eq!(body_text("run"), "x + 1");
    assert_eq!(body_text("go"), "x * 3");
    assert_ne!(
        symbol(&result, "f_add").body_hash,
        symbol(&result, "f_mul").body_hash
    );
}

#[test]
fn class_method_locals_belong_to_the_method() {
    let source = r#"Queue <- R6::R6Class("Queue", public = list(
  add = function(x) {
    total <- length(self$items) + 1
    fmt <- function(v) format(v)
    fmt(x)
  }))
Stack <- setRefClass("Stack", fields = list(items = "list"),
  methods = list(push = function(x) {
    n <- length(items)
    items <<- c(items, x)
  }))
"#;
    let result = extract("r6local.R", source);
    let add = symbol(&result, "add");
    let push = symbol(&result, "push");
    for name in ["total", "fmt"] {
        assert_eq!(
            symbol(&result, name).parent_id.as_deref(),
            Some(add.id.as_str())
        );
    }
    assert_eq!(
        symbol(&result, "n").parent_id.as_deref(),
        Some(push.id.as_str())
    );
    assert!(
        result
            .symbols
            .iter()
            .any(|s| s.name == "x" && s.parent_id.as_deref() == Some(push.id.as_str()))
    );
    assert_eq!(
        result.symbols.iter().filter(|s| s.name == "items").count(),
        1
    );
}

#[test]
fn member_and_subset_assignments_create_no_symbols() {
    let source = r#"cache <- new.env()
cache$items <- list()
cache[["key"]] <- 1
x@age <- 2
Person <- R6::R6Class("Person", public = list(name = NULL,
  initialize = function(name) { self$name <- name }))
server <- function(input, output) { output$plot <- renderPlot({ 1 }) }
bump <- function() {
  counter <<- counter + 1
}
env$handler <- function(req) respond(req)
"#;
    let result = extract("assign.R", source);
    assert!(
        result
            .symbols
            .iter()
            .all(|s| !s.name.contains('$') && !s.name.contains('[') && !s.name.contains('@')),
        "{:#?}",
        result.symbols
    );
    assert!(result.symbols.iter().all(|s| s.name != "counter"));
    let handler = symbol(&result, "handler");
    assert_eq!(handler.kind, SymbolKind::Function);
}

#[test]
fn receivers_user_operators_and_namespace_values_emit_identifiers() {
    let source = r#"Queue <- R6::R6Class("Queue")
`%+%` <- function(a, b) paste0(a, b)
make <- function(cfg, x) {
  q <- Queue$new()
  cfg$db$host
  "a" %+% "b"
  purrr::map(x, mypkg::transform)
}
"#;
    let result = extract("ids.R", source);
    let has = |name: &str, kind: IdentifierKind| {
        result
            .identifiers
            .iter()
            .any(|i| i.name == name && i.kind == kind)
    };
    assert!(has("Queue", IdentifierKind::VariableRef));
    assert!(has("cfg", IdentifierKind::VariableRef));
    assert!(has("%+%", IdentifierKind::Call));
    assert!(has("transform", IdentifierKind::VariableRef));
    assert!(edge(&result, RelationshipKind::Calls, "make", "%+%"));
}

#[test]
fn pipes_emit_one_edge_per_call_site_and_handle_bare_targets() {
    let source = r#"clean <- function(df) df
pipeline <- function(df) {
  df %>%
    clean() %>%
    external_step()
}
bare <- function(df) df %>% clean %>% ext_step
"#;
    let result = extract("pipes.R", source);
    let pipeline = symbol(&result, "pipeline");
    let clean = symbol(&result, "clean");
    let pipeline_clean: Vec<_> = result
        .relationships
        .iter()
        .filter(|r| r.from_symbol_id == pipeline.id && r.to_symbol_id == clean.id)
        .collect();
    assert_eq!(pipeline_clean.len(), 1);
    assert_eq!(pipeline_clean[0].line_number, 4);
    let external: Vec<_> = pending_from(&result, RelationshipKind::Calls, "pipeline")
        .into_iter()
        .filter(|p| p.target.display_name == "external_step")
        .collect();
    assert_eq!(external.len(), 1);
    assert_eq!(external[0].pending.line_number, 5);
    assert!(edge(&result, RelationshipKind::Calls, "bare", "clean"));
    assert!(
        pending_from(&result, RelationshipKind::Calls, "bare")
            .iter()
            .any(|p| p.target.display_name == "ext_step")
    );
    assert!(
        result
            .identifiers
            .iter()
            .any(|i| i.name == "ext_step" && i.kind == IdentifierKind::Call)
    );
}

#[test]
fn top_level_initializer_calls_belong_to_the_variable() {
    let source = r#"helper <- function() 1
result <- helper()
config <- yaml::read_yaml("config.yml")
"#;
    let result = extract("toplevel.R", source);
    assert!(edge(&result, RelationshipKind::Calls, "result", "helper"));
    let config = pending_from(&result, RelationshipKind::Calls, "config");
    assert_eq!(config.len(), 1);
    assert_eq!(config[0].target.namespace_path, vec!["yaml".to_string()]);
}

#[test]
fn s4_declarations_read_arguments_by_name_and_position() {
    let source = r#"setMethod("area", signature(shape = "Circle"), function(shape) pi * shape@r^2)
setMethod("show", signature("Employee"), function(object) cat("x"))
setMethod(f = "summary", signature = "Circle", definition = function(object, ...) cat("r"))
setMethod("combine", c("A", "B"), function(x, y) x)
setClass(Class = "Square", representation("Shape", side = "numeric"))
setReplaceMethod("age", "Person", function(x, value) { x@age <- value; x })
"#;
    let result = extract("s4more.R", source);
    let names: Vec<_> = result.symbols.iter().map(|s| s.name.as_str()).collect();
    for expected in [
        "area,Circle",
        "show,Employee",
        "summary,Circle",
        "combine,A,B",
        "Square",
        "age<-,Person",
    ] {
        assert!(names.contains(&expected), "missing {expected}: {names:?}");
    }
    let area = symbol(&result, "area,Circle");
    assert_eq!(
        area.metadata
            .as_ref()
            .and_then(|m| m.get("s4_class"))
            .and_then(|v| v.as_str()),
        Some("Circle")
    );
}
