use crate::base::{ExtractionResults, Identifier, IdentifierKind, Symbol, SymbolKind};
use crate::extract_canonical;
use std::path::Path;

fn extract(source: &str) -> ExtractionResults {
    extract_canonical("R/state.R", source, Path::new("/tmp/test")).expect("r extraction")
}

fn named<'a>(result: &'a ExtractionResults, name: &str) -> Vec<&'a Symbol> {
    result.symbols.iter().filter(|s| s.name == name).collect()
}

fn one<'a>(result: &'a ExtractionResults, name: &str) -> &'a Symbol {
    let found = named(result, name);
    assert_eq!(found.len(), 1, "{name}: {:#?}", result.symbols);
    found[0]
}

fn meta<'a>(symbol: &'a Symbol, key: &str) -> Option<&'a str> {
    symbol.metadata.as_ref()?.get(key)?.as_str()
}

fn writes<'a>(result: &'a ExtractionResults, name: &str) -> Vec<&'a Identifier> {
    result
        .identifiers
        .iter()
        .filter(|i| {
            i.name == name
                && i.metadata
                    .as_ref()
                    .is_some_and(|m| m.contains_key("assignment"))
        })
        .collect()
}

fn container<'a>(result: &'a ExtractionResults, identifier: &Identifier) -> &'a str {
    let id = identifier.containing_symbol_id.as_deref();
    result
        .symbols
        .iter()
        .find(|s| Some(s.id.as_str()) == id)
        .map_or("", |s| s.name.as_str())
}

fn assert_write(result: &ExtractionResults, name: &str, line: u32, caller: &str, operator: &str) {
    let found = writes(result, name);
    let site = found
        .iter()
        .find(|i| i.start_line == line)
        .unwrap_or_else(|| panic!("{name} at {line}: {found:#?}"));
    assert_eq!(site.kind, IdentifierKind::VariableRef, "{name}");
    assert_eq!(container(result, site), caller, "{name}");
    assert_eq!(
        site.metadata.as_ref().and_then(|m| m.get("assignment")),
        Some(&serde_json::json!(operator)),
        "{name}"
    );
}

#[test]
fn superassignment_with_no_binding_in_the_file_creates_a_global() {
    let result = extract(
        "setup <- function() {\n  registry <<- list()\n}\nadd <- function(x) { registry <<- c(registry, x) }\n",
    );
    let registry = one(&result, "registry");
    assert_eq!(registry.kind, SymbolKind::Variable);
    assert_eq!(registry.parent_id, None);
    assert_eq!(registry.start_line, 2);
    assert_eq!(meta(registry, "assignment"), Some("<<-"));
    assert_write(&result, "registry", 2, "setup", "<<-");
    assert_write(&result, "registry", 4, "add", "<<-");
    assert!(
        result.identifiers.iter().any(|i| i.name == "registry"
            && i.start_line == 4
            && i.metadata
                .as_ref()
                .and_then(|metadata| metadata.get("access"))
                == Some(&serde_json::json!("read"))
            && i.metadata
                .as_ref()
                .is_some_and(|metadata| !metadata.contains_key("assignment"))
            && container(&result, i) == "add"),
        "the read of registry in add stays a plain read"
    );
}

#[test]
fn superassignment_of_a_function_creates_a_global_function() {
    let result = extract("install <- function() {\n  handler <<- function(req) respond(req)\n}\n");
    let handler = one(&result, "handler");
    assert_eq!(handler.kind, SymbolKind::Function);
    assert_eq!(handler.parent_id, None);
    assert_eq!(meta(handler, "assignment"), Some("<<-"));
    let req = result
        .symbols
        .iter()
        .find(|s| s.name == "req")
        .expect("parameter req");
    assert_eq!(req.parent_id.as_deref(), Some(handler.id.as_str()));
    assert_write(&result, "handler", 2, "install", "<<-");
}

#[test]
fn superassignment_to_a_closure_variable_writes_only_a_write_site() {
    let result = extract(
        "make_account <- function(balance) {\n  deposit <- function(v) { balance <<- balance - v }\n  log <- NULL\n  record <- function(x) {\n    log <<- c(log, x)\n    hits <- 0\n    inner <- function() hits <<- hits + 1\n  }\n  deposit\n}\n",
    );
    for name in ["balance", "log", "hits"] {
        assert!(
            named(&result, name)
                .iter()
                .all(|s| meta(s, "assignment").is_none()),
            "{name}: {:#?}",
            result.symbols
        );
    }
    assert_eq!(named(&result, "balance").len(), 1);
    assert_eq!(named(&result, "log").len(), 1);
    assert_eq!(named(&result, "hits").len(), 1);
    assert_write(&result, "balance", 2, "deposit", "<<-");
    assert_write(&result, "log", 5, "record", "<<-");
    assert_write(&result, "hits", 7, "inner", "<<-");
    assert!(
        result.identifiers.iter().any(|i| i.name == "balance"
            && i.start_line == 2
            && i.metadata
                .as_ref()
                .and_then(|metadata| metadata.get("access"))
                == Some(&serde_json::json!("read"))
            && i.metadata
                .as_ref()
                .is_some_and(|metadata| !metadata.contains_key("assignment"))),
        "the read of balance stays"
    );
}

#[test]
fn superassignment_to_a_top_level_variable_creates_no_second_symbol() {
    let result = extract(
        "counter <- 0\nbump <- function() { counter <<- counter + 1 }\nlater <- function() { total <<- 1 }\ntotal = 0\n",
    );
    for name in ["counter", "total"] {
        let symbol = one(&result, name);
        assert_eq!(meta(symbol, "assignment"), None, "{name}");
    }
    assert_write(&result, "counter", 2, "bump", "<<-");
    assert_write(&result, "total", 3, "later", "<<-");
}

#[test]
fn the_innermost_function_own_local_does_not_bind_the_target() {
    let result = extract("f <- function() {\n  cache <- 1\n  cache <<- 2\n}\n");
    let globals: Vec<&Symbol> = named(&result, "cache")
        .into_iter()
        .filter(|s| s.parent_id.is_none())
        .collect();
    assert_eq!(globals.len(), 1, "{:#?}", result.symbols);
    assert_eq!(meta(globals[0], "assignment"), Some("<<-"));
}

#[test]
fn right_superassignment_follows_the_same_rules() {
    let result = extract(
        "outer <- function() {\n  total <- 0\n  inner <- function() { 1 ->> total; 2 ->> seen }\n  inner()\n}\n",
    );
    let total = one(&result, "total");
    assert_eq!(
        result
            .symbols
            .iter()
            .find(|s| Some(s.id.as_str()) == total.parent_id.as_deref())
            .map(|s| s.name.as_str()),
        Some("outer")
    );
    let seen = one(&result, "seen");
    assert_eq!(seen.kind, SymbolKind::Variable);
    assert_eq!(seen.parent_id, None);
    assert_eq!(meta(seen, "assignment"), Some("->>"));
    assert_write(&result, "total", 3, "inner", "->>");
    assert_write(&result, "seen", 3, "inner", "->>");
}

#[test]
fn class_methods_and_top_level_superassignment_keep_their_rows() {
    let result = extract(
        "Stack <- setRefClass(\"Stack\", fields = list(items = \"list\"),\n  methods = list(push = function(x) { items <<- c(items, x) }))\nAccount <- setRefClass(\"Account\", fields = c(balance = \"numeric\"))\nAccount$methods(withdraw = function(x) { balance <<- balance - x })\nglobal_var <<- 500\n",
    );
    assert_eq!(named(&result, "items").len(), 1);
    assert_eq!(one(&result, "items").kind, SymbolKind::Field);
    assert_eq!(one(&result, "balance").kind, SymbolKind::Field);
    let global = one(&result, "global_var");
    assert_eq!(global.kind, SymbolKind::Variable);
    assert_eq!(global.parent_id, None);
    assert_eq!(meta(global, "assignment"), None);
    assert!(writes(&result, "global_var").is_empty());
    assert_write(&result, "items", 2, "push", "<<-");
}

#[test]
fn assign_into_the_global_environment_creates_a_global() {
    let result = extract(
        "counter <- 0\npublish <- function() {\n  assign(\"shared\", 1, envir = .GlobalEnv)\n  assign(\"other\", 2, envir = globalenv())\n  assign(\"counter\", 3, envir = .GlobalEnv)\n  assign(\"local_only\", 4)\n  assign(\"in_env\", 5, envir = cache)\n}\nagain <- function() assign(\"shared\", 6, envir = .GlobalEnv)\n",
    );
    for (name, line) in [("shared", 3), ("other", 4)] {
        let global = one(&result, name);
        assert_eq!(global.kind, SymbolKind::Variable, "{name}");
        assert_eq!(global.parent_id, None, "{name}");
        assert_eq!(global.start_line, line, "{name}");
        assert_eq!(meta(global, "assignment"), Some("assign"), "{name}");
    }
    assert_eq!(meta(one(&result, "counter"), "assignment"), None);
    for name in ["local_only", "in_env"] {
        assert!(named(&result, name).is_empty(), "{name}");
    }
}

#[test]
fn ordinary_assignments_write_no_write_sites() {
    let result =
        extract("x <- 1\nf <- function() {\n  y <- 2\n  3 -> z\n  y = x\n  obj$v <<- 1\n}\n");
    assert!(
        result.identifiers.iter().all(|i| i
            .metadata
            .as_ref()
            .is_none_or(|m| !m.contains_key("assignment"))),
        "{:#?}",
        result.identifiers
    );
    assert!(
        result
            .symbols
            .iter()
            .all(|s| meta(s, "assignment").is_none() && s.name != "v")
    );
}

#[test]
fn superassignment_records_write_and_read_access() {
    let result = extract("update <- function(x) { x <<- x + 1 }\n");
    let write = writes(&result, "x")[0];

    assert_eq!(
        write
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("access")),
        Some(&serde_json::json!("write"))
    );
    assert!(result.identifiers.iter().any(|identifier| {
        identifier.name == "x"
            && identifier.start_line == 1
            && identifier.start_byte != write.start_byte
            && identifier
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("access"))
                == Some(&serde_json::json!("read"))
    }));
}

#[test]
fn ordinary_reassignments_and_loop_bindings_get_write_access() {
    let source = "value <- 1\nvalue <- 2\nvalue %<>% increment\nitems <- list()\nitems[offset + index] <- value\nnames(items) <- value\niterate <- function(items) {\n  for (item in items) {\n    print(item)\n  }\n}\n";
    let result = extract(source);
    let access_at = |start: usize| {
        result
            .identifiers
            .iter()
            .find(|identifier| {
                identifier.start_byte == start as u32
                    && identifier.kind == crate::base::IdentifierKind::VariableRef
            })
            .and_then(|identifier| identifier.metadata.as_ref())
            .and_then(|metadata| metadata.get("access"))
            .and_then(serde_json::Value::as_str)
    };

    assert_eq!(access_at(source.find("value <- 1").unwrap()), None);
    assert_eq!(access_at(source.find("value <- 2").unwrap()), Some("write"));
    assert_eq!(
        access_at(source.find("value %<>%").unwrap()),
        Some("read_write")
    );
    assert_eq!(
        access_at(source.find("items[offset").unwrap()),
        Some("read")
    );
    assert_eq!(
        access_at(source.find("offset + index").unwrap()),
        Some("read")
    );
    assert_eq!(access_at(source.find("index] <-").unwrap()), Some("read"));
    assert_eq!(
        access_at(source.find("names(items)").unwrap() + "names(".len()),
        Some("read")
    );
    assert_eq!(access_at(source.find("item in").unwrap()), Some("write"));
    assert_eq!(
        access_at(source.find("item in items").unwrap() + "item in ".len()),
        Some("read")
    );
}

#[test]
fn function_and_class_declarations_are_symbols_and_same_name_reassignments_are_writes() {
    let source = "helper <- function() {\n  helper <- 1\n  helper <- 2\n  counter <- 0\n  counter <- 1\n}\nWorker <- R6::R6Class(\"Worker\")\n";
    let result = extract(source);
    let access_at = |start: usize| {
        result
            .identifiers
            .iter()
            .find(|identifier| {
                identifier.kind == crate::base::IdentifierKind::VariableRef
                    && identifier.start_byte == start as u32
            })
            .and_then(|identifier| identifier.metadata.as_ref())
            .and_then(|metadata| metadata.get("access"))
            .and_then(serde_json::Value::as_str)
    };

    let function = result
        .symbols
        .iter()
        .find(|symbol| {
            symbol.name == "helper"
                && symbol.kind == SymbolKind::Function
                && symbol.start_byte == source.find("helper <- function").unwrap() as u32
        })
        .expect("helper function symbol");
    assert!(result.symbols.iter().any(|symbol| {
        symbol.name == "Worker"
            && symbol.kind == SymbolKind::Class
            && symbol.start_byte == source.find("Worker <-").unwrap() as u32
    }));
    assert!(result.symbols.iter().any(|symbol| {
        symbol.name == "helper"
            && symbol.kind == SymbolKind::Variable
            && symbol.parent_id.as_deref() == Some(function.id.as_str())
            && symbol.start_byte == source.find("helper <- 1").unwrap() as u32
    }));
    assert_eq!(access_at(source.find("helper <- function").unwrap()), None);
    assert_eq!(access_at(source.find("Worker <-").unwrap()), None);
    assert_eq!(access_at(source.find("helper <- 1").unwrap()), None);
    assert_eq!(
        access_at(source.find("helper <- 2").unwrap()),
        Some("write")
    );
    assert_eq!(
        access_at(source.find("counter <- 1").unwrap()),
        Some("write")
    );
}
