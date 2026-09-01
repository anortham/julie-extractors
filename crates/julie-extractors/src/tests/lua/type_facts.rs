use crate::base::{Identifier, IdentifierKind, Symbol, SymbolKind, TypeInfo};
use crate::lua::LuaExtractor;
use crate::tests::lua::init_parser;
use std::path::PathBuf;

fn extract(source: &str) -> (Vec<Symbol>, LuaExtractor) {
    let mut parser = init_parser();
    let tree = parser.parse(source, None).expect("parse lua");
    let mut extractor = LuaExtractor::new(
        "lua".to_string(),
        "test.lua".to_string(),
        source.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    (symbols, extractor)
}

fn extract_calls(source: &str) -> (Vec<Symbol>, Vec<Identifier>, LuaExtractor) {
    let mut parser = init_parser();
    let tree = parser.parse(source, None).expect("parse lua");
    let mut extractor = LuaExtractor::new(
        "lua".to_string(),
        "test.lua".to_string(),
        source.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    let identifiers = extractor.extract_identifiers(&tree, &symbols);
    extractor.extract_relationships(&tree, &symbols);
    (symbols, identifiers, extractor)
}

fn symbol<'a>(symbols: &'a [Symbol], name: &str, kind: SymbolKind) -> &'a Symbol {
    symbols
        .iter()
        .find(|s| s.name == name && s.kind == kind)
        .unwrap_or_else(|| panic!("missing symbol {name}"))
}

fn fact<'a>(
    extractor: &'a LuaExtractor,
    symbols: &[Symbol],
    name: &str,
    kind: SymbolKind,
) -> &'a TypeInfo {
    let symbol = symbol(symbols, name, kind);
    extractor
        .base
        .type_info
        .get(&symbol.id)
        .unwrap_or_else(|| panic!("missing type fact for {name}"))
}

fn no_fact(extractor: &LuaExtractor, symbols: &[Symbol], name: &str, kind: SymbolKind) {
    let symbol = symbol(symbols, name, kind);
    assert!(
        extractor.base.type_info.get(&symbol.id).is_none(),
        "unexpected type fact for {name}"
    );
}

fn role(symbol: &Symbol) -> Option<&str> {
    symbol
        .metadata
        .as_ref()
        .and_then(|m| m.get("role"))
        .and_then(|v| v.as_str())
}

fn account_class_source() -> &'static str {
    r#"
local Account = {}

function Account.new(balance)
    return setmetatable({ balance = balance }, Account)
end

function Account:deposit(amount)
    self:log()
    self.m()
    other:log()
end
"#
}

#[test]
fn colon_method_emits_implicit_self_fact_and_named_parameter() {
    let (symbols, extractor) = extract(account_class_source());
    let deposit = symbol(&symbols, "deposit", SymbolKind::Method);
    let self_param = symbol(&symbols, "self", SymbolKind::Variable);
    let amount = symbol(&symbols, "amount", SymbolKind::Variable);
    let balance = symbol(&symbols, "balance", SymbolKind::Variable);
    let new_fn = symbol(&symbols, "new", SymbolKind::Method);

    assert_eq!(role(self_param), Some("parameter"));
    assert_eq!(self_param.parent_id.as_deref(), Some(deposit.id.as_str()));
    let self_fact = fact(&extractor, &symbols, "self", SymbolKind::Variable);
    assert_eq!(self_fact.resolved_type, "Account");
    assert!(!self_fact.is_inferred);

    assert_eq!(role(amount), Some("parameter"));
    assert_eq!(amount.parent_id.as_deref(), Some(deposit.id.as_str()));
    no_fact(&extractor, &symbols, "amount", SymbolKind::Variable);

    assert_eq!(role(balance), Some("parameter"));
    assert_eq!(balance.parent_id.as_deref(), Some(new_fn.id.as_str()));
    no_fact(&extractor, &symbols, "balance", SymbolKind::Variable);
}

#[test]
fn same_file_constructor_and_setmetatable_record_inferred_facts() {
    let source = format!(
        "{}\nlocal a = Account.new(10)\nlocal boxed = setmetatable({{}}, Account)\n",
        account_class_source()
    );
    let (symbols, extractor) = extract(&source);
    let a = fact(&extractor, &symbols, "a", SymbolKind::Variable);
    assert_eq!(a.resolved_type, "Account");
    assert!(a.is_inferred);

    let boxed = symbols
        .iter()
        .find(|s| s.name == "boxed")
        .expect("missing boxed symbol");
    let boxed_fact = extractor
        .base
        .type_info
        .get(&boxed.id)
        .expect("missing type fact for boxed");
    assert_eq!(boxed_fact.resolved_type, "Account");
    assert!(boxed_fact.is_inferred);
}

#[test]
fn unknown_imported_and_non_constructor_initializers_record_no_fact() {
    let source = r#"
local u = Unknown.new()
local r = require("x").new()
local t = {}
"#;
    let (symbols, extractor) = extract(source);
    no_fact(&extractor, &symbols, "u", SymbolKind::Variable);
    no_fact(&extractor, &symbols, "r", SymbolKind::Variable);
    no_fact(&extractor, &symbols, "t", SymbolKind::Variable);
}

#[test]
fn self_colon_and_dot_calls_record_receiver_type_on_identifier_and_pending() {
    let (_, identifiers, extractor) = extract_calls(account_class_source());
    let calls: Vec<_> = identifiers
        .iter()
        .filter(|id| id.kind == IdentifierKind::Call)
        .collect();
    let log_calls: Vec<_> = calls
        .iter()
        .filter(|id| id.name == "log")
        .copied()
        .collect();
    assert_eq!(log_calls.len(), 2);
    assert_eq!(log_calls[0].receiver_type.as_deref(), Some("Account"));
    assert_eq!(log_calls[1].receiver_type, None);
    let m = calls
        .iter()
        .find(|id| id.name == "m")
        .expect("missing self.m call");
    assert_eq!(m.receiver_type.as_deref(), Some("Account"));

    let pending = extractor.get_structured_pending_relationships();
    let pending_for = |receiver: &str| {
        pending
            .iter()
            .find(|p| {
                p.target.terminal_name == "log" && p.target.receiver.as_deref() == Some(receiver)
            })
            .unwrap_or_else(|| panic!("missing pending log on {receiver}"))
    };
    assert_eq!(
        pending_for("self").receiver_type.as_deref(),
        Some("Account")
    );
    assert_eq!(pending_for("other").receiver_type, None);
    let pending_m = pending
        .iter()
        .find(|p| p.target.terminal_name == "m")
        .expect("missing pending m");
    assert_eq!(pending_m.receiver_type.as_deref(), Some("Account"));
}
