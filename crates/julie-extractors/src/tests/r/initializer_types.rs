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
