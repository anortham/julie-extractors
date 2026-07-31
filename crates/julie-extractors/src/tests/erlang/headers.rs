use super::support::{extract_from, find, find_kind};
use crate::base::SymbolKind;

const HEADER: &str = r#"%% @doc Shared account definitions.
-define(MAX_BALANCE, 1000000).

-record(account, {id :: integer(), balance = 0 :: integer()}).

-type account() :: #account{}.
"#;

#[test]
fn header_files_extract_standalone_without_a_module_attribute() {
    let symbols = extract_from("include/account.hrl", HEADER);
    let names: Vec<_> = symbols.iter().map(|symbol| symbol.name.as_str()).collect();

    assert_eq!(
        names,
        vec!["MAX_BALANCE", "account", "id", "balance", "account"]
    );
}

#[test]
fn header_symbols_have_no_module_parent() {
    let symbols = extract_from("include/account.hrl", HEADER);

    assert_eq!(find(&symbols, "MAX_BALANCE").parent_id, None);
    assert_eq!(
        find_kind(&symbols, "account", SymbolKind::Struct).parent_id,
        None
    );
}

#[test]
fn header_record_still_parents_its_fields() {
    let symbols = extract_from("include/account.hrl", HEADER);
    let record = find_kind(&symbols, "account", SymbolKind::Struct);

    assert_eq!(
        find(&symbols, "id").parent_id.as_deref(),
        Some(record.id.as_str())
    );
}

#[test]
fn hrl_extension_routes_to_the_erlang_extractor() {
    assert_eq!(
        crate::language::detect_language_from_extension("hrl"),
        Some("erlang")
    );
    assert_eq!(
        crate::language::detect_language_from_extension("erl"),
        Some("erlang")
    );
}
