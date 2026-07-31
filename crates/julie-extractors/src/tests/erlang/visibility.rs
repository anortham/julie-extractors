use super::support::{extract, find, find_kind};
use crate::base::{SymbolKind, Visibility};

#[test]
fn exported_functions_are_public_and_the_rest_are_private() {
    let symbols = extract(
        "-module(bank).\n-export([open/1, deposit/2]).\nopen(Id) -> Id.\ndeposit(A, B) -> {A, B}.\naudit(A) -> A.\n",
    );

    assert_eq!(find(&symbols, "open").visibility, Some(Visibility::Public));
    assert_eq!(
        find(&symbols, "deposit").visibility,
        Some(Visibility::Public)
    );
    assert_eq!(
        find(&symbols, "audit").visibility,
        Some(Visibility::Private)
    );
}

#[test]
fn export_entries_match_on_arity_not_just_name() {
    let symbols = extract("-module(bank).\n-export([open/1]).\nopen() -> ok.\nopen(Id) -> Id.\n");
    let visibilities: Vec<_> = symbols
        .iter()
        .filter(|symbol| symbol.name == "open")
        .map(|symbol| symbol.visibility.clone())
        .collect();

    assert_eq!(
        visibilities,
        vec![Some(Visibility::Private), Some(Visibility::Public)]
    );
}

#[test]
fn compile_export_all_makes_every_function_public() {
    let symbols = extract("-module(bank).\n-compile(export_all).\naudit(A) -> A.\n");

    assert_eq!(find(&symbols, "audit").visibility, Some(Visibility::Public));
}

#[test]
fn compile_export_all_is_honoured_inside_an_options_list() {
    let symbols = extract(
        "-module(bank).\n-compile([export_all, nowarn_unused_function]).\naudit(A) -> A.\n",
    );

    assert_eq!(find(&symbols, "audit").visibility, Some(Visibility::Public));
}

#[test]
fn compile_options_without_export_all_leave_functions_private() {
    let symbols = extract("-module(bank).\n-compile([nowarn_unused_function]).\naudit(A) -> A.\n");

    assert_eq!(
        find(&symbols, "audit").visibility,
        Some(Visibility::Private)
    );
}

#[test]
fn exported_types_are_public_and_unexported_types_are_private() {
    let symbols = extract(
        "-module(bank).\n-export_type([account/0]).\n-type account() :: term().\n-type token() :: binary().\n",
    );

    assert_eq!(
        find_kind(&symbols, "account", SymbolKind::Type).visibility,
        Some(Visibility::Public)
    );
    assert_eq!(
        find_kind(&symbols, "token", SymbolKind::Type).visibility,
        Some(Visibility::Private)
    );
}

#[test]
fn records_and_macros_are_private() {
    let symbols = extract("-module(bank).\n-define(PI, 3.14).\n-record(account, {id}).\n");

    assert_eq!(find(&symbols, "PI").visibility, Some(Visibility::Private));
    assert_eq!(
        find_kind(&symbols, "account", SymbolKind::Struct).visibility,
        Some(Visibility::Private)
    );
}
