use crate::base::{
    ExtractionResults, IdentifierKind, RelationshipKind, Symbol, SymbolKind, Visibility,
};
use crate::extract_canonical;
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("lua extraction")
}

fn find<'a>(result: &'a ExtractionResults, name: &str) -> Vec<&'a Symbol> {
    result.symbols.iter().filter(|s| s.name == name).collect()
}

fn one<'a>(result: &'a ExtractionResults, name: &str) -> &'a Symbol {
    let found = find(result, name);
    assert_eq!(found.len(), 1, "{name}: {found:#?}");
    found[0]
}

fn meta_str<'a>(symbol: &'a Symbol, key: &str) -> Option<&'a str> {
    symbol.metadata.as_ref()?.get(key)?.as_str()
}

fn type_of(result: &ExtractionResults, symbol: &Symbol) -> Option<String> {
    result
        .types
        .get(&symbol.id)
        .map(|info| info.resolved_type.clone())
}

fn has_extends(result: &ExtractionResults, from: &Symbol, to: &Symbol) -> bool {
    result.relationships.iter().any(|r| {
        r.kind == RelationshipKind::Extends
            && r.from_symbol_id == from.id
            && r.to_symbol_id == to.id
    })
}

fn is_test(symbol: &Symbol) -> bool {
    symbol
        .metadata
        .as_ref()
        .and_then(|m| m.get("is_test"))
        .and_then(|v| v.as_bool())
        == Some(true)
}

fn test_role(symbol: &Symbol) -> Option<&str> {
    meta_str(symbol, "test_role")
}

#[test]
fn method_declaration_names_emit_no_call_or_member_identifiers() {
    let source = "local Worker = {}\nfunction Worker:run()\n  return helper(self.id)\nend\nlocal M = {}\nfunction M.add(a, b)\n  return a + b\nend\n";
    let result = extract("decl.lua", source);
    let names: Vec<_> = result
        .identifiers
        .iter()
        .filter(|i| i.kind != IdentifierKind::VariableRef)
        .map(|i| i.name.as_str())
        .collect();
    assert!(!names.contains(&"run"), "{names:?}");
    assert!(!names.contains(&"add"), "{names:?}");
    assert!(names.contains(&"helper"), "{names:?}");
}

#[test]
fn colon_calls_carry_receiver_and_keep_chain_members() {
    let source = "local function render(canvas, list)\n  canvas:clear()\n  list.items:sort()\n  (canvas):flush()\nend\n";
    let result = extract("render.lua", source);
    let clear = result
        .identifiers
        .iter()
        .find(|i| i.name == "clear")
        .unwrap();
    let clear_meta = clear.metadata.as_ref().expect("clear receiver");
    assert_eq!(clear_meta["receiver"], "canvas");
    let sort = result
        .identifiers
        .iter()
        .find(|i| i.name == "sort")
        .unwrap();
    let sort_meta = sort.metadata.as_ref().expect("sort receiver");
    assert_eq!(sort_meta["receiver"], "items");
    assert_eq!(sort_meta["receiver_qualifier"], "list");
    assert!(
        result
            .identifiers
            .iter()
            .any(|i| i.name == "items" && i.kind == IdentifierKind::MemberAccess)
    );
    let flush = result
        .identifiers
        .iter()
        .find(|i| i.name == "flush")
        .unwrap();
    assert!(flush.metadata.is_none(), "{flush:?}");
}

#[test]
fn busted_dsl_calls_in_production_files_stay_plain_calls() {
    let source =
        "local setup = plugin.setup\nlocal function configure()\n  setup({ debug = true })\nend\n";
    let result = extract("lua/plugin/init.lua", source);
    assert!(
        result.symbols.iter().all(|s| !is_test(s)),
        "{:#?}",
        result.symbols
    );
    assert!(
        result
            .structured_pending_relationships
            .iter()
            .any(|p| p.target.terminal_name == "setup")
    );
}

#[test]
fn busted_aliases_and_luaunit_fixtures_are_classified() {
    let source = "describe(\"calc\", function()\n  test(\"subtracts\", function() end)\n  spec(\"multiplies\", function() end)\n  pending(\"divides later\")\n  strict_setup(function() end)\n  insulate(\"isolated\", function()\n    it(\"still runs\", function() end)\n  end)\nend)\n";
    let result = extract("spec/calc_spec.lua", source);
    assert_eq!(test_role(one(&result, "subtracts")), Some("test_case"));
    assert_eq!(test_role(one(&result, "multiplies")), Some("test_case"));
    assert_eq!(test_role(one(&result, "divides later")), Some("test_case"));
    assert_eq!(
        test_role(one(&result, "strict_setup")),
        Some("fixture_setup")
    );
    let isolated = one(&result, "isolated");
    assert_eq!(test_role(isolated), Some("test_container"));
    assert_eq!(
        one(&result, "still runs").parent_id.as_deref(),
        Some(isolated.id.as_str())
    );

    let luaunit = "TestCalc = {}\nfunction TestCalc:setUp() end\nfunction TestCalc:tearDown() end\nfunction TestCalc:testAdd() end\n";
    let result = extract("tests/test_calc.lua", luaunit);
    assert_eq!(test_role(one(&result, "setUp")), Some("fixture_setup"));
    assert_eq!(
        test_role(one(&result, "tearDown")),
        Some("fixture_teardown")
    );
    assert_eq!(test_role(one(&result, "testAdd")), Some("test_case"));
    assert_eq!(test_role(one(&result, "TestCalc")), Some("test_container"));
}

#[test]
fn pil_and_middleclass_classes_record_bases_types_and_extends_edges() {
    let source = r#"local class = require("middleclass")
Account = { balance = 0 }
function Account:new(o)
  o = o or {}
  setmetatable(o, self)
  self.__index = self
  return o
end
function Account:deposit(v) self.balance = self.balance + v end
SpecialAccount = Account:new()
function SpecialAccount:withdraw(v) end
local Animal = class("Animal")
local Dog = class("Dog", Animal)
local Cat = Animal:subclass("Cat")
local function main()
  local acc = Account:new{ balance = 100 }
  local d = Dog("rex")
  acc:deposit(10)
end
"#;
    let result = extract("pil.lua", source);
    let account = one(&result, "Account");
    assert_eq!(account.kind, SymbolKind::Class);
    let special = one(&result, "SpecialAccount");
    assert_eq!(special.kind, SymbolKind::Class);
    assert!(has_extends(&result, special, account));
    let animal = one(&result, "Animal");
    let dog = one(&result, "Dog");
    let cat = one(&result, "Cat");
    assert_eq!(animal.kind, SymbolKind::Class);
    assert_eq!(dog.kind, SymbolKind::Class);
    assert_eq!(cat.kind, SymbolKind::Class);
    assert!(has_extends(&result, dog, animal));
    assert!(has_extends(&result, cat, animal));
    assert_eq!(
        type_of(&result, one(&result, "acc")).as_deref(),
        Some("Account")
    );
    assert_eq!(type_of(&result, one(&result, "d")).as_deref(), Some("Dog"));
    let deposit_call = result
        .structured_pending_relationships
        .iter()
        .find(|p| p.target.terminal_name == "deposit");
    let resolved_deposit = result
        .relationships
        .iter()
        .any(|r| r.kind == RelationshipKind::Calls && r.to_symbol_id == one(&result, "deposit").id);
    assert!(resolved_deposit || deposit_call.is_some());
}

#[test]
fn metatable_inheritance_forms_emit_extends_edges() {
    let source = r#"local Base = require("base")
local Animal = {}
Animal.__index = Animal
local Dog = setmetatable({}, Animal)
Dog.__index = Dog
local Cat = Animal:extend()
local Pet = setmetatable({}, {__index = Animal})
local Remote = setmetatable({}, Base)
Remote.__index = Remote
"#;
    let result = extract("inh.lua", source);
    let animal = one(&result, "Animal");
    for name in ["Dog", "Cat", "Pet"] {
        let class = one(&result, name);
        assert_eq!(class.kind, SymbolKind::Class, "{name}");
        assert!(has_extends(&result, class, animal), "{name}");
    }
    assert!(
        find(&result, "__index")
            .iter()
            .all(|field| field.parent_id.is_some())
    );
    let remote = one(&result, "Remote");
    assert!(result.structured_pending_relationships.iter().any(|p| {
        p.pending.kind == RelationshipKind::Extends
            && p.pending.from_symbol_id == remote.id
            && p.target.terminal_name == "Base"
    }));
}

#[test]
fn setmetatable_instances_are_typed_variables_and_fields_belong_to_the_class() {
    let source = r#"local Account = {}
Account.__index = Account
function Account.new(balance)
  local self = setmetatable({}, Account)
  self.balance = balance
  return self
end
function Account:deposit(v) self.balance = self.balance + v end
function Account:withdraw(v) self.balance = self.balance - v end
local cache = setmetatable({}, { __mode = "k" })
local boxed = setmetatable({}, Account)
"#;
    let result = extract("inst.lua", source);
    let account = one(&result, "Account");
    let instance = find(&result, "self")
        .into_iter()
        .find(|s| {
            s.signature
                .as_deref()
                .is_some_and(|sig| sig.contains("setmetatable"))
        })
        .unwrap();
    assert_eq!(instance.kind, SymbolKind::Variable);
    assert_eq!(type_of(&result, instance).as_deref(), Some("Account"));
    assert_eq!(one(&result, "cache").kind, SymbolKind::Variable);
    assert!(find(&result, "__mode").is_empty());
    let boxed = one(&result, "boxed");
    assert_eq!(boxed.kind, SymbolKind::Variable);
    assert_eq!(type_of(&result, boxed).as_deref(), Some("Account"));
    let fields: Vec<_> = find(&result, "balance")
        .into_iter()
        .filter(|s| s.kind == SymbolKind::Field)
        .collect();
    assert_eq!(fields.len(), 1, "{fields:#?}");
    assert_eq!(fields[0].parent_id.as_deref(), Some(account.id.as_str()));
}

#[test]
fn table_fields_belong_to_declared_tables_and_argument_tables_emit_nothing() {
    let source = r#"local M = {}
Account = { balance = 0 }
M.config = {
  --- Enabled flag.
  enabled = true,
  nested = { width = 80 },
}
local function register(buf)
  vim.keymap.set("n", "q", "<cmd>close<cr>", { buffer = buf, desc = "Close" })
  vim.api.nvim_create_autocmd("BufEnter", { callback = function()
    local inner = require("inner")
  end })
  return { render = "index" }
end
function M.load()
  player = { x = 0 }
end
return { setup = function(opts) end }
"#;
    let result = extract("tables.lua", source);
    assert!(find(&result, "buffer").is_empty());
    assert!(find(&result, "desc").is_empty());
    assert!(find(&result, "callback").is_empty());
    assert!(find(&result, "render").is_empty());
    assert_eq!(
        one(&result, "balance").parent_id.as_deref(),
        Some(one(&result, "Account").id.as_str())
    );
    let config = one(&result, "config");
    let enabled = one(&result, "enabled");
    assert_eq!(enabled.parent_id.as_deref(), Some(config.id.as_str()));
    assert_eq!(enabled.doc_comment.as_deref(), Some("--- Enabled flag."));
    let nested = one(&result, "nested");
    assert_eq!(
        one(&result, "width").parent_id.as_deref(),
        Some(nested.id.as_str())
    );
    assert_eq!(one(&result, "inner").kind, SymbolKind::Import);
    assert_eq!(
        one(&result, "x").parent_id.as_deref(),
        Some(one(&result, "player").id.as_str())
    );
    assert_eq!(one(&result, "setup").kind, SymbolKind::Method);
}

#[test]
fn forward_declared_local_functions_are_private() {
    let source = "local cache\nlocal is_even, is_odd\nfunction is_even(n) return is_odd(n - 1) end\nfunction is_odd(n) return is_even(n - 1) end\n";
    let result = extract("fwd.lua", source);
    assert_eq!(one(&result, "cache").visibility, Some(Visibility::Private));
    for symbol in find(&result, "is_even")
        .into_iter()
        .chain(find(&result, "is_odd"))
    {
        assert_eq!(symbol.visibility, Some(Visibility::Private), "{symbol:?}");
    }
}

#[test]
fn qualified_calls_to_same_file_table_functions_resolve() {
    let source = "local M = {}\nfunction M.add(a, b) return a + b end\nfunction M.sub(a, b)\n  return M.add(a, -b)\nend\nlocal Dog = {}\nDog.__index = Dog\nfunction Dog.new(name) return setmetatable({}, Dog) end\nlocal function play()\n  return Dog.new(\"fido\")\nend\n";
    let result = extract("q.lua", source);
    let calls = |from: &str, to: &str| {
        result.relationships.iter().any(|r| {
            r.kind == RelationshipKind::Calls
                && r.from_symbol_id == one(&result, from).id
                && r.to_symbol_id == one(&result, to).id
        })
    };
    assert!(calls("sub", "add"));
    assert!(calls("play", "new"));
}

#[test]
fn expression_receivers_do_not_leak_source_text_into_pending_targets() {
    let source = "local function f(path, bufnr)\n  require(\"lazy.core.config\").setup({})\n  for dir in (path .. \":\"):gmatch(\"([^:]*):\") do end\n  vim.lsp.get_clients({ bufnr = bufnr })[1]:stop()\nend\n";
    let result = extract("recv.lua", source);
    let targets: Vec<_> = result
        .structured_pending_relationships
        .iter()
        .filter(|p| p.pending.kind == RelationshipKind::Calls)
        .map(|p| &p.target)
        .collect();
    let setup = targets.iter().find(|t| t.terminal_name == "setup").unwrap();
    assert_eq!(setup.receiver, None);
    assert_eq!(setup.import_context.as_deref(), Some("lazy.core.config"));
    for name in ["gmatch", "stop"] {
        let target = targets.iter().find(|t| t.terminal_name == name).unwrap();
        assert_eq!(target.receiver, None, "{target:?}");
        assert_eq!(target.display_name, name);
    }
}

#[test]
fn luals_annotations_record_declared_types_and_class_inheritance() {
    let source = r#"---@class Logger
local Logger = {}
---@class FileLogger : Logger
local FileLogger = setmetatable({}, { __index = Logger })
---@param sink FileLogger
---@param level? integer
---@return Logger
local function wrap(sink, level)
  ---@type Logger
  local log = sink
  return log
end
"#;
    let result = extract("ann.lua", source);
    let logger = one(&result, "Logger");
    let file_logger = one(&result, "FileLogger");
    assert_eq!(logger.kind, SymbolKind::Class);
    assert!(has_extends(&result, file_logger, logger));
    assert_eq!(
        type_of(&result, one(&result, "sink")).as_deref(),
        Some("FileLogger")
    );
    assert_eq!(
        type_of(&result, one(&result, "level")).as_deref(),
        Some("integer")
    );
    assert_eq!(
        type_of(&result, one(&result, "wrap")).as_deref(),
        Some("Logger")
    );
    assert_eq!(
        type_of(&result, one(&result, "log")).as_deref(),
        Some("Logger")
    );
}

#[test]
fn doc_comments_do_not_attach_across_blank_lines() {
    let source = "---@brief\n--- Module header.\n\nlocal api = vim.api\n\n--- Separated.\n\nlocal orphan = 1\n--- Attached.\nlocal kept = 2\n";
    let result = extract("docs.lua", source);
    assert_eq!(one(&result, "api").doc_comment, None);
    assert_eq!(one(&result, "orphan").doc_comment, None);
    assert_eq!(
        one(&result, "kept").doc_comment.as_deref(),
        Some("--- Attached.")
    );
}

fn facts<'a>(result: &'a ExtractionResults, pattern: &str) -> Vec<&'a crate::base::StructuralFact> {
    result
        .structural_facts
        .iter()
        .filter(|f| f.pattern_id == pattern)
        .collect()
}

fn fact_str<'a>(fact: &'a crate::base::StructuralFact, key: &str) -> Option<&'a str> {
    fact.metadata.as_ref()?.get(key)?.as_str()
}

#[test]
fn lapis_routes_emit_route_facts() {
    let source = r#"local lapis = require("lapis")
local app = lapis.Application()
app:get("/", function(self) return "hi" end)
app:post("/users", function(self) end)
app:match("user", "/users/:id", function(self) end)
app:match("/about", function(self) end)
return app
"#;
    let result = extract("app.lua", source);
    let routes = facts(&result, "lapis.route.v1");
    assert_eq!(routes.len(), 4, "{routes:#?}");
    assert_eq!(fact_str(routes[0], "verb"), Some("GET"));
    assert_eq!(fact_str(routes[0], "route_template"), Some("/"));
    assert_eq!(fact_str(routes[2], "route_name"), Some("user"));
    assert_eq!(
        fact_str(routes[2], "normalized_route_template"),
        Some("/users/:id")
    );
    assert_eq!(fact_str(routes[2], "verb"), None);
}

#[test]
fn neovim_and_love_apis_emit_framework_facts() {
    let source = r#"vim.api.nvim_create_user_command("Greet", function(a) end, { nargs = 1 })
vim.api.nvim_create_autocmd({ "BufWritePre", "BufEnter" }, { pattern = "*.lua", callback = fn })
vim.keymap.set("n", "<leader>g", M.greet, { desc = "Greet" })
vim.keymap.set({ "n", "v" }, "<leader>q", "<cmd>qa<cr>")
function love.draw() end
function love.update(dt) end
"#;
    let result = extract("plugin.lua", source);
    let command = facts(&result, "neovim.user_command.v1");
    assert_eq!(command.len(), 1);
    assert_eq!(fact_str(command[0], "command_name"), Some("Greet"));
    let autocmd = facts(&result, "neovim.autocmd.v1");
    assert_eq!(autocmd.len(), 1);
    assert_eq!(
        autocmd[0].metadata.as_ref().unwrap()["events"],
        serde_json::json!(["BufWritePre", "BufEnter"])
    );
    assert_eq!(
        autocmd[0].metadata.as_ref().unwrap()["patterns"],
        serde_json::json!(["*.lua"])
    );
    let keymaps = facts(&result, "neovim.keymap.v1");
    assert_eq!(keymaps.len(), 2);
    assert_eq!(fact_str(keymaps[0], "lhs"), Some("<leader>g"));
    assert_eq!(fact_str(keymaps[0], "desc"), Some("Greet"));
    assert_eq!(
        keymaps[1].metadata.as_ref().unwrap()["modes"],
        serde_json::json!(["n", "v"])
    );
    let love = facts(&result, "love.callback.v1");
    assert_eq!(love.len(), 2);
    assert_eq!(fact_str(love[0], "callback"), Some("draw"));
}

#[test]
fn lazy_nvim_plugin_specs_emit_plugin_facts() {
    let source = r#"return {
  {
    "nvim-telescope/telescope.nvim",
    cmd = "Telescope",
    dependencies = { "nvim-lua/plenary.nvim" },
    config = function() end,
  },
  { "folke/tokyonight.nvim", lazy = false },
}
"#;
    let result = extract("lua/plugins/telescope.lua", source);
    let specs = facts(&result, "lazy_nvim.plugin_spec.v1");
    assert_eq!(specs.len(), 2, "{specs:#?}");
    assert_eq!(
        fact_str(specs[0], "plugin"),
        Some("nvim-telescope/telescope.nvim")
    );
    assert_eq!(
        specs[0].metadata.as_ref().unwrap()["dependencies"],
        serde_json::json!(["nvim-lua/plenary.nvim"])
    );
    assert_eq!(fact_str(specs[1], "plugin"), Some("folke/tokyonight.nvim"));
}

#[test]
fn luals_marker_tags_become_annotations() {
    let source = "---@deprecated\n---@nodiscard\nlocal function old() end\nlocal M = {\n  ---@private\n  secret = 1,\n}\n";
    let result = extract("marks.lua", source);
    let keys = |name: &str| -> Vec<String> {
        one(&result, name)
            .annotations
            .iter()
            .map(|marker| marker.annotation_key.clone())
            .collect()
    };
    assert_eq!(keys("old"), vec!["deprecated", "nodiscard"]);
    assert_eq!(keys("secret"), vec!["private"]);
}
