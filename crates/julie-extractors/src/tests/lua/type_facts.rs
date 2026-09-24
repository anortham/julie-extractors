use crate::base::{Identifier, IdentifierKind, Symbol, SymbolKind, TypeInfo};
use crate::lua::LuaExtractor;
use crate::tests::lua::init_test_parser;
use std::path::PathBuf;

fn extract(source: &str) -> (Vec<Symbol>, LuaExtractor) {
    let mut parser = init_test_parser();
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
    let mut parser = init_test_parser();
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
        !extractor.base.type_info.contains_key(&symbol.id),
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

#[test]
fn implicit_self_symbol_spans_method_name_without_parameter_list_signature() {
    let (symbols, _) = extract(account_class_source());
    let self_param = symbol(&symbols, "self", SymbolKind::Variable);
    let amount = symbol(&symbols, "amount", SymbolKind::Variable);

    assert_eq!(self_param.signature, None);
    assert_eq!(self_param.start_line, amount.start_line);
    assert!(self_param.start_column < amount.start_column);
    assert_eq!(
        self_param.end_column - self_param.start_column,
        "deposit".len() as u32
    );
}

fn inferred_type(source: &str, name: &str) -> Option<(String, bool)> {
    let (symbols, extractor) = extract(source);
    let symbol = symbol(&symbols, name, SymbolKind::Variable);
    extractor
        .base
        .type_info
        .get(&symbol.id)
        .map(|fact| (fact.resolved_type.clone(), fact.is_inferred))
}

fn assert_inferred(source: &str, name: &str, expected: &str) {
    assert_eq!(
        inferred_type(source, name),
        Some((expected.to_string(), true)),
        "inferred type of {name}"
    );
}

fn assert_no_fact(source: &str, name: &str) {
    assert_eq!(inferred_type(source, name), None, "type fact of {name}");
}

#[test]
fn local_function_call_records_annotated_return_type() {
    let source = r#"
---@return Config
local function load() end
local config = load()
"#;
    assert_inferred(source, "config", "Config");
}

#[test]
fn call_before_the_annotated_global_function_records_its_return_type() {
    let source = r#"
local function run()
  local config = load()
end
---@return Config
function load() end
"#;
    assert_inferred(source, "config", "Config");
}

#[test]
fn module_dot_and_colon_calls_record_annotated_return_types() {
    let source = r#"
local M = {}
---@return Config
function M.load() end
---@return Session
function M:open() end
local config = M.load()
local session = M:open()
"#;
    assert_inferred(source, "config", "Config");
    assert_inferred(source, "session", "Session");
}

#[test]
fn nested_table_owner_matches_the_full_table_path() {
    let source = r#"
---@return Config
function app.config.load() end
local config = app.config.load()
"#;
    assert_inferred(source, "config", "Config");
}

#[test]
fn self_calls_inside_colon_method_use_the_owner_methods() {
    let source = r#"
local Store = {}
---@return Cursor
function Store:cursor() end
---@return Cursor
function Store.static_cursor() end
function Store:scan()
  local cursor = self:cursor()
  local other = self.static_cursor()
end
"#;
    assert_inferred(source, "cursor", "Cursor");
    assert_inferred(source, "other", "Cursor");
}

#[test]
fn function_value_assignments_record_annotated_return_types() {
    let source = r#"
local M = {}
---@return Config
local load = function() end
---@return Session
M.open = function() end
local config = load()
local session = M.open()
"#;
    assert_inferred(source, "config", "Config");
    assert_inferred(source, "session", "Session");
}

#[test]
fn optional_return_records_the_base_type_and_keeps_the_annotation() {
    let source = r#"
---@return Config?
local function find() end
---@return Config|nil
local function lookup() end
local found = find()
local looked = lookup()
"#;
    let (symbols, extractor) = extract(source);
    let found = fact(&extractor, &symbols, "found", SymbolKind::Variable);
    assert_eq!(found.resolved_type, "Config");
    assert!(found.is_inferred);
    assert_eq!(
        found.metadata.as_ref().and_then(|m| m.get("declared")),
        Some(&serde_json::Value::String("Config?".to_string()))
    );
    assert_inferred(source, "looked", "Config");
}

#[test]
fn self_return_type_resolves_to_the_owner_table() {
    let source = r#"
local Builder = {}
---@return self
function Builder:clone() end
local copy = Builder:clone()
"#;
    assert_inferred(source, "copy", "Builder");
}

#[test]
fn first_return_value_binds_first_variable_only() {
    let source = r#"
---@return Config
---@return string
local function load() end
---@return Session, string
local function open() end
local config, err = load()
local session = open()
"#;
    assert_inferred(source, "config", "Config");
    assert_no_fact(source, "err");
    assert_inferred(source, "session", "Session");
    let (symbols, extractor) = extract(source);
    let open = fact(&extractor, &symbols, "open", SymbolKind::Function);
    assert_eq!(open.resolved_type, "Session");
    assert!(!open.is_inferred);
}

#[test]
fn each_expression_binds_its_own_variable() {
    let source = r#"
---@return Config
local function load() end
---@return Session
local function open() end
local config, session = load(), open()
"#;
    assert_inferred(source, "config", "Config");
    assert_inferred(source, "session", "Session");
}

#[test]
fn same_file_class_constructor_wins_over_annotated_return_type() {
    let source = r#"
local Account = {}
Account.__index = Account
---@return AccountProxy
function Account.new()
  return setmetatable({}, Account)
end
function Account:deposit() end
local account = Account.new()
"#;
    assert_inferred(source, "account", "Account");
}

#[test]
fn written_type_annotation_wins_over_call_inference() {
    let source = r#"
---@return Config
local function load() end
---@type Settings
local config = load()
"#;
    assert_eq!(
        inferred_type(source, "config"),
        Some(("Settings".to_string(), false))
    );
}

#[test]
fn unannotated_function_records_no_fact() {
    let source = r#"
local function load() end
local config = load()
"#;
    assert_no_fact(source, "config");
}

#[test]
fn disagreeing_same_named_functions_record_no_fact() {
    let source = r#"
local M = {}
---@return Config
function M.load() end
---@return Settings
function M.load() end
---@return Config
local function open() end
local function open() end
local config = M.load()
local session = open()
"#;
    assert_no_fact(source, "config");
    assert_no_fact(source, "session");
}

#[test]
fn generic_return_types_record_no_fact() {
    let source = r#"
---@generic T
---@param value T
---@return T
local function identity(value) end
---@class Stack<V>
local Stack = {}
---@return V
function Stack:pop() end
local id = identity(1)
local top = Stack:pop()
"#;
    assert_no_fact(source, "id");
    assert_no_fact(source, "top");
}

#[test]
fn union_any_array_and_overloaded_returns_record_no_fact() {
    let source = r#"
---@return Config|Settings
local function either() end
---@return any
local function anything() end
---@return Config[]
local function many() end
---@overload fun(name: string): Settings
---@return Config
local function load() end
local a = either()
local b = anything()
local c = many()
local d = load()
"#;
    for name in ["a", "b", "c", "d"] {
        assert_no_fact(source, name);
    }
}

#[test]
fn owner_and_free_name_must_match_the_declaration() {
    let source = r#"
local M = {}
---@return Config
function M.load() end
local by_other_owner = N.load()
local by_free_name = load()
local by_unknown_receiver = obj:load()
"#;
    assert_no_fact(source, "by_other_owner");
    assert_no_fact(source, "by_free_name");
    assert_no_fact(source, "by_unknown_receiver");
}

#[test]
fn unknown_method_at_the_end_of_a_chain_records_no_fact() {
    let source = r#"
local M = {}
---@return Config
function M.load() end
local value = M.load():get()
local field = M.load().name
"#;
    assert_no_fact(source, "value");
    assert_no_fact(source, "field");
}

#[test]
fn self_outside_a_colon_method_or_redeclared_records_no_fact() {
    let source = r#"
local Store = {}
---@return Cursor
function Store:cursor() end
function Store.scan(self)
  local explicit = self:cursor()
end
function Store:walk()
  local self = other
  local shadowed = self:cursor()
end
"#;
    assert_no_fact(source, "explicit");
    assert_no_fact(source, "shadowed");
}

#[test]
fn shadowed_free_function_and_rebound_owner_record_no_fact() {
    let source = r#"
---@return Config
local function load() end
local M = {}
---@return Session
function M.open() end
local function run(load)
  local config = load()
end
local function other()
  local M = require("other")
  local session = M.open()
end
"#;
    assert_no_fact(source, "config");
    assert_no_fact(source, "session");
}

#[test]
fn local_function_outside_its_scope_at_the_call_site_records_no_fact() {
    let source = r#"
local function outer()
  ---@return Foo
  local function helper() end
  local inner = helper()
  return helper()
end
local function other()
  local h1 = helper()
end
local function run()
  local k1 = load()
end
---@return Config
local function load() end
if ready then
  ---@return Session
  local function open() end
end
local k3 = open()
"#;
    assert_inferred(source, "inner", "Foo");
    for name in ["h1", "k1", "k3"] {
        assert_no_fact(source, name);
    }
}

#[test]
fn recursive_call_inside_a_local_function_uses_the_local_function() {
    let source = r#"
---@return Config
function walk() end
---@return Node
local function walk()
  local next = walk()
end
"#;
    assert_inferred(source, "next", "Node");
}

#[test]
fn loop_variables_and_parameters_that_shadow_the_callee_record_no_fact() {
    let source = r#"
local M = {}
---@return Config
function M.load() end
for _, M in ipairs(mods) do local h2 = M.load() end
---@return Config
local function make() end
for _, make in ipairs(fns) do local h3 = make() end
for make = 1, 3 do local h10 = make() end
local app = { config = {} }
---@return Config
function app.config.load() end
for k, app in pairs(apps) do local h9 = app.config.load() end
local nested = app.config.load()
local counted = make()
"#;
    for name in ["h2", "h3", "h10", "h9"] {
        assert_no_fact(source, name);
    }
    assert_inferred(source, "nested", "Config");
    assert_inferred(source, "counted", "Config");
}

#[test]
fn parameter_that_shadows_a_nested_owner_root_records_no_fact() {
    let source = r#"
local app = { config = {} }
---@return Config
function app.config.load() end
local function f(app) local h8 = app.config.load() end
"#;
    assert_no_fact(source, "h8");
}

#[test]
fn owner_reassigned_by_a_plain_assignment_records_no_fact() {
    let source = r#"
local M = {}
---@return Config
function M.open() end
local function reset()
  M = require("other")
  local k2 = M.open()
end
"#;
    assert_no_fact(source, "k2");
}

#[test]
fn spaced_union_returns_keep_the_whole_union() {
    let source = r#"
---@return Foo | Bar
local function either() end
---@return Foo | nil
local function maybe() end
local h4 = either()
local found = maybe()
"#;
    assert_no_fact(source, "h4");
    assert_inferred(source, "found", "Foo");
    let (symbols, extractor) = extract(source);
    no_fact(&extractor, &symbols, "either", SymbolKind::Function);
}

#[test]
fn reassigned_member_and_literal_returns_record_no_fact() {
    let source = r#"
local M = {}
---@return Config
function M.get() end
M.get = memoize(M.get)
local h6 = M.get()
---@return true
local function yes() end
---@return false
local function no() end
local h7 = yes()
local h11 = no()
"#;
    for name in ["h6", "h7", "h11"] {
        assert_no_fact(source, name);
    }
}
