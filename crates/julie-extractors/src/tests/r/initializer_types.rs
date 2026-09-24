use crate::base::{Symbol, SymbolKind};
use crate::r::RExtractor;
use crate::tests::helpers::init_parser;
use std::path::PathBuf;

fn extract(source: &str) -> (Vec<Symbol>, RExtractor) {
    let tree = init_parser(source, "r");
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = RExtractor::new(
        "r".to_string(),
        "test.R".to_string(),
        source.to_string(),
        &workspace_root,
    );
    let symbols = extractor.extract_symbols(&tree);
    (symbols, extractor)
}

fn variable<'a>(symbols: &'a [Symbol], name: &str) -> &'a Symbol {
    symbols
        .iter()
        .find(|s| s.name == name && s.kind == SymbolKind::Variable)
        .unwrap_or_else(|| panic!("missing variable {name}"))
}

fn inferred_type(source: &str, name: &str) -> Option<String> {
    let (symbols, extractor) = extract(source);
    let symbol = variable(&symbols, name);
    extractor.base.type_info.get(&symbol.id).map(|fact| {
        assert!(fact.is_inferred, "fact for {name} must be inferred");
        fact.resolved_type.clone()
    })
}

const ACCOUNT_GENERIC: &str = r#"
Account <- setRefClass("Account", fields = list(balance = "numeric"))
setGeneric("open_account", function(bank) standardGeneric("open_account"), valueClass = "Account")
"#;

#[test]
fn generic_value_class_types_a_top_level_call_initializer() {
    let source = format!("{ACCOUNT_GENERIC}acct <- open_account(bank)\n");
    assert_eq!(inferred_type(&source, "acct").as_deref(), Some("Account"));
}

#[test]
fn generic_value_class_types_a_local_inside_a_function() {
    let source = format!(
        "{ACCOUNT_GENERIC}deposit_all <- function(bank) {{\n  acct <- open_account(bank)\n  acct$deposit(1)\n}}\n"
    );
    assert_eq!(inferred_type(&source, "acct").as_deref(), Some("Account"));
}

#[test]
fn equals_and_superassignment_initializers_are_typed() {
    let source = format!("{ACCOUNT_GENERIC}a = open_account(bank)\nb <<- open_account(bank)\n");
    assert_eq!(inferred_type(&source, "a").as_deref(), Some("Account"));
    assert_eq!(inferred_type(&source, "b").as_deref(), Some("Account"));
}

#[test]
fn right_assignment_initializers_are_typed() {
    let source = format!(
        "{ACCOUNT_GENERIC}open_account(bank) -> acct\nWorker <- R6::R6Class(\"Worker\")\nWorker$new() -> w\n"
    );
    assert_eq!(inferred_type(&source, "acct").as_deref(), Some("Account"));
    assert_eq!(inferred_type(&source, "w").as_deref(), Some("Worker"));
}

#[test]
fn positional_value_class_and_namespaced_set_generic_are_read() {
    let source = r#"
methods::setGeneric("make_point", function(x) standardGeneric("make_point"), list(), "Point")
p <- make_point(1)
"#;
    assert_eq!(inferred_type(source, "p").as_deref(), Some("Point"));
}

#[test]
fn value_class_does_not_need_a_same_file_class() {
    let source = r#"
setGeneric("area", function(shape) standardGeneric("area"), valueClass = "numeric")
a <- area(s)
"#;
    assert_eq!(inferred_type(source, "a").as_deref(), Some("numeric"));
}

#[test]
fn call_before_the_generic_declaration_is_typed() {
    let source = r#"
acct <- open_account(bank)
setGeneric("open_account", function(bank) standardGeneric("open_account"), valueClass = "Account")
"#;
    assert_eq!(inferred_type(source, "acct").as_deref(), Some("Account"));
}

#[test]
fn agreeing_generic_redeclarations_keep_the_type() {
    let source = format!(
        "{ACCOUNT_GENERIC}setGeneric(\"open_account\", function(bank) standardGeneric(\"open_account\"), valueClass = \"Account\")\nacct <- open_account(bank)\n"
    );
    assert_eq!(inferred_type(&source, "acct").as_deref(), Some("Account"));
}

#[test]
fn generic_without_value_class_records_nothing() {
    let source = r#"
setGeneric("open_account", function(bank) standardGeneric("open_account"))
acct <- open_account(bank)
"#;
    assert_eq!(inferred_type(source, "acct"), None);
}

#[test]
fn value_class_union_records_nothing() {
    let source = r#"
setGeneric("open_account", function(bank) standardGeneric("open_account"), valueClass = c("Account", "Loan"))
acct <- open_account(bank)
"#;
    assert_eq!(inferred_type(source, "acct"), None);
}

#[test]
fn disagreeing_generic_redeclarations_record_nothing() {
    let source = format!(
        "{ACCOUNT_GENERIC}setGeneric(\"open_account\", function(bank) standardGeneric(\"open_account\"), valueClass = \"Loan\")\nacct <- open_account(bank)\n"
    );
    assert_eq!(inferred_type(&source, "acct"), None);
}

#[test]
fn redeclaration_without_value_class_records_nothing() {
    let source = format!(
        "{ACCOUNT_GENERIC}setGeneric(\"open_account\", function(bank) standardGeneric(\"open_account\"))\nacct <- open_account(bank)\n"
    );
    assert_eq!(inferred_type(&source, "acct"), None);
}

#[test]
fn name_rebound_by_assignment_records_nothing() {
    let source = format!(
        "{ACCOUNT_GENERIC}open_account <- function(bank) list()\nacct <- open_account(bank)\n"
    );
    assert_eq!(inferred_type(&source, "acct"), None);
}

#[test]
fn name_shadowed_by_a_parameter_records_nothing() {
    let source = format!(
        "{ACCOUNT_GENERIC}run <- function(open_account) {{\n  acct <- open_account(bank)\n}}\n"
    );
    assert_eq!(inferred_type(&source, "acct"), None);
}

#[test]
fn namespaced_call_records_nothing() {
    let source = format!("{ACCOUNT_GENERIC}acct <- bankpkg::open_account(bank)\n");
    assert_eq!(inferred_type(&source, "acct"), None);
}

#[test]
fn member_call_or_access_on_the_result_records_nothing() {
    let source = format!(
        "{ACCOUNT_GENERIC}a <- open_account(bank)$deposit(1)\nb <- open_account(bank)$balance\n"
    );
    assert_eq!(inferred_type(&source, "a"), None);
    assert_eq!(inferred_type(&source, "b"), None);
}

#[test]
fn set_method_value_class_is_ignored() {
    let source = r#"
setMethod("open_account", "Bank", function(bank) new("Account"), valueClass = "Account")
acct <- open_account(bank)
"#;
    assert_eq!(inferred_type(source, "acct"), None);
}

#[test]
fn roxygen_return_tag_records_nothing() {
    let source = r#"
#' Open an account.
#' @return An Account object.
open_account <- function(bank) new("Account")
acct <- open_account(bank)
"#;
    assert_eq!(inferred_type(source, "acct"), None);
}

#[test]
fn name_rebound_by_assign_records_nothing() {
    let source = format!(
        "{ACCOUNT_GENERIC}assign(\"open_account\", function(bank) list())\nacct <- open_account(bank)\n"
    );
    assert_eq!(inferred_type(&source, "acct"), None);
}

#[test]
fn set_generic_without_def_records_nothing() {
    let source = r#"
setGeneric("summary", valueClass = "Account")
setGeneric("show", valueClass = "Account")
s_summary <- summary(obj)
s_show <- show(obj)
"#;
    assert_eq!(inferred_type(source, "s_summary"), None);
    assert_eq!(inferred_type(source, "s_show"), None);
}

#[test]
fn set_generic_with_a_non_function_def_records_nothing() {
    let source = r#"
setGeneric("mk", def = mk_def, valueClass = "Account")
s <- mk(1)
"#;
    assert_eq!(inferred_type(source, "s"), None);
}

#[test]
fn redeclaration_without_def_records_nothing() {
    let source = format!(
        "{ACCOUNT_GENERIC}setGeneric(\"open_account\", valueClass = \"Account\")\nacct <- open_account(bank)\n"
    );
    assert_eq!(inferred_type(&source, "acct"), None);
}

#[test]
fn conditional_set_generic_records_nothing() {
    let source = r#"
if (!isGeneric("mk")) setGeneric("mk", function(x) standardGeneric("mk"), valueClass = "Account")
isGeneric("mk2") || setGeneric("mk2", function(x) standardGeneric("mk2"), valueClass = "Account")
s_isgen <- mk(1)
s_or <- mk2(1)
"#;
    assert_eq!(inferred_type(source, "s_isgen"), None);
    assert_eq!(inferred_type(source, "s_or"), None);
}

#[test]
fn set_generic_from_another_namespace_records_nothing() {
    let source = r#"
foo::setGeneric("ng", function(x) standardGeneric("ng"), valueClass = "Account")
s_ns <- ng(1)
"#;
    assert_eq!(inferred_type(source, "s_ns"), None);
}

#[test]
fn name_rebound_by_a_for_loop_variable_records_nothing() {
    let source = format!(
        "{ACCOUNT_GENERIC}for (open_account in list(function(x) 1)) {{\n  s_for <- open_account(1)\n}}\n"
    );
    assert_eq!(inferred_type(&source, "s_for"), None);
}

#[test]
fn name_rebound_by_delayed_assign_records_nothing() {
    let source = format!(
        "{ACCOUNT_GENERIC}delayedAssign(\"open_account\", function(x) 1)\ns_delayed <- open_account(1)\n"
    );
    assert_eq!(inferred_type(&source, "s_delayed"), None);
}

#[test]
fn name_rebound_by_make_active_binding_records_nothing() {
    let source = format!(
        "{ACCOUNT_GENERIC}makeActiveBinding(fun = function() function(x) 1, sym = \"open_account\", env = e)\ns_active <- open_account(1)\n"
    );
    assert_eq!(inferred_type(&source, "s_active"), None);
}

#[test]
fn name_rebound_by_named_assign_records_nothing() {
    let source = format!(
        "{ACCOUNT_GENERIC}assign(value = function(x) 1, x = \"open_account\")\ns_named <- open_account(1)\n"
    );
    assert_eq!(inferred_type(&source, "s_named"), None);
}

#[test]
fn name_bound_by_with_data_records_nothing() {
    let source = format!(
        "{ACCOUNT_GENERIC}with(list(open_account = function(x) 1), {{\n  s_with <- open_account(1)\n}})\n"
    );
    assert_eq!(inferred_type(&source, "s_with"), None);
}

#[test]
fn native_pipe_into_a_generic_is_typed() {
    let source = format!(
        "{ACCOUNT_GENERIC}s_pipe <- bank |> open_account()\nbank |> open_account() -> s_right\n"
    );
    assert_eq!(inferred_type(&source, "s_pipe").as_deref(), Some("Account"));
    assert_eq!(
        inferred_type(&source, "s_right").as_deref(),
        Some("Account")
    );
}

#[test]
fn native_pipe_ending_in_another_call_records_nothing() {
    let source = format!(
        "{ACCOUNT_GENERIC}a <- bank |> open_account() |> summary()\nb <- bank |> bankpkg::open_account()\n"
    );
    assert_eq!(inferred_type(&source, "a"), None);
    assert_eq!(inferred_type(&source, "b"), None);
}

#[test]
fn parenthesized_initializers_are_typed() {
    let source = format!(
        "{ACCOUNT_GENERIC}a <- (open_account(bank))\nWorker <- R6::R6Class(\"Worker\")\nw <- ((Worker$new()))\n"
    );
    assert_eq!(inferred_type(&source, "a").as_deref(), Some("Account"));
    assert_eq!(inferred_type(&source, "w").as_deref(), Some("Worker"));
}

#[test]
fn ref_class_method_with_the_generic_name_records_nothing() {
    let source = r#"
setGeneric("describe", function(x) standardGeneric("describe"), valueClass = "character")
Person <- setRefClass("Person", fields = list(name = "character"), methods = list(describe = function() length(name), show = function() { d <- describe(); cat(d) }))
"#;
    assert_eq!(inferred_type(source, "d"), None);
}

#[test]
fn ref_class_methods_call_with_the_generic_name_records_nothing() {
    let source = r#"
Acc <- setRefClass("Acc")
setGeneric("mk", function(x) standardGeneric("mk"), valueClass = "Account")
Acc$methods(mk = function(x) 42, run = function() { r <- mk(1) })
setGeneric("mk2", function(x) standardGeneric("mk2"), valueClass = "Account")
Acc$methods(list(mk2 = function(x) 42))
r2 <- mk2(1)
"#;
    assert_eq!(inferred_type(source, "r"), None);
    assert_eq!(inferred_type(source, "r2"), None);
}

#[test]
fn ref_class_field_with_the_generic_name_records_nothing() {
    let source = r#"
setGeneric("cb", function(x) standardGeneric("cb"), valueClass = "Account")
setGeneric("cb2", function(x) standardGeneric("cb2"), valueClass = "Account")
Job <- setRefClass("Job", fields = list(cb = "function"))
setGeneric("cb3", function(x) standardGeneric("cb3"), valueClass = "Account")
Job2 <- setRefClass("Job2", c("cb2"))
Job2$fields(cb3 = "function")
r <- cb(1)
r2 <- cb2(1)
r3 <- cb3(1)
"#;
    assert_eq!(inferred_type(source, "r"), None);
    assert_eq!(inferred_type(source, "r2"), None);
    assert_eq!(inferred_type(source, "r3"), None);
}

#[test]
fn set_generic_that_may_not_run_records_nothing() {
    let source = r#"
switch(mode, a = setGeneric("mk", function(x) standardGeneric("mk"), valueClass = "Account"))
while (FALSE) setGeneric("mk2", function(x) standardGeneric("mk2"), valueClass = "Account")
repeat { break; setGeneric("mk3", function(x) standardGeneric("mk3"), valueClass = "Account") }
for (i in seq_len(0)) setGeneric("mk4", function(x) standardGeneric("mk4"), valueClass = "Account")
define <- function() setGeneric("mk5", function(x) standardGeneric("mk5"), valueClass = "Account")
r_switch <- mk(1)
r_while <- mk2(1)
r_repeat <- mk3(1)
r_for <- mk4(1)
r_fn <- mk5(1)
"#;
    for name in ["r_switch", "r_while", "r_repeat", "r_for", "r_fn"] {
        assert_eq!(inferred_type(source, name), None, "{name}");
    }
}

#[test]
fn set_generic_bound_into_another_environment_records_nothing() {
    let source = r#"
e <- new.env()
setGeneric("mk", function(x) standardGeneric("mk"), valueClass = "Account", where = e)
setGeneric("mk2", function(x) standardGeneric("mk2"), list(), "Account", e)
setGeneric("mk3", function(x) standardGeneric("mk3"), valueClass = "Account", wh = e)
r_where <- mk(1)
r_positional <- mk2(1)
r_partial <- mk3(1)
"#;
    for name in ["r_where", "r_positional", "r_partial"] {
        assert_eq!(inferred_type(source, name), None, "{name}");
    }
}

#[test]
fn name_rebound_through_an_environment_member_records_nothing() {
    let source = format!(
        "{ACCOUNT_GENERIC}.GlobalEnv$open_account <- function(x) 42\nr <- open_account(1)\n"
    );
    assert_eq!(inferred_type(&source, "r"), None);
    let source = format!(
        "{ACCOUNT_GENERIC}environment()[[\"open_account\"]] <- function(x) 42\nr <- open_account(1)\n"
    );
    assert_eq!(inferred_type(&source, "r"), None);
}

#[test]
fn name_rebound_by_a_replacement_call_records_nothing() {
    for replacement in [
        "body(open_account) <- quote(42)",
        "formals(open_account) <- alist(x = )",
        "environment(open_account) <- e",
    ] {
        let source = format!("{ACCOUNT_GENERIC}{replacement}\nr <- open_account(1)\n");
        assert_eq!(inferred_type(&source, "r"), None, "{replacement}");
    }
}

#[test]
fn name_bound_by_list2env_records_nothing() {
    let source = format!(
        "{ACCOUNT_GENERIC}list2env(list(open_account = function(x) 42), envir = environment())\nr <- open_account(1)\n"
    );
    assert_eq!(inferred_type(&source, "r"), None);
}

#[test]
fn constructor_name_rebound_by_a_function_records_nothing() {
    let source = r#"
setClass("Account", representation(n = "numeric"))
Account <- function(x) 42
r_ctor <- Account(1)
Account(1) -> r_ctor_right
"#;
    assert_eq!(inferred_type(source, "r_ctor"), None);
    assert_eq!(inferred_type(source, "r_ctor_right"), None);
}

#[test]
fn generator_name_rebound_by_a_list_records_nothing() {
    let source = r#"
Worker <- R6::R6Class("Worker")
Worker <- list(new = function() 42)
w <- Worker$new()
"#;
    assert_eq!(inferred_type(source, "w"), None);
}

#[test]
fn constructor_bound_to_its_generator_is_typed() {
    let source = r#"
Account <- setClass("Account", representation(n = "numeric"))
r <- Account(n = 1)
"#;
    assert_eq!(inferred_type(source, "r").as_deref(), Some("Account"));
}
