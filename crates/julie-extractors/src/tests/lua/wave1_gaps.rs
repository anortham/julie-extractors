use crate::base::{ExtractionResults, RelationshipKind, Symbol, SymbolKind, Visibility};
use crate::extract_canonical;
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("lua extraction")
}

fn symbols_named<'a>(result: &'a ExtractionResults, name: &str) -> Vec<&'a Symbol> {
    result.symbols.iter().filter(|s| s.name == name).collect()
}

fn symbol<'a>(result: &'a ExtractionResults, name: &str, kind: SymbolKind) -> &'a Symbol {
    result
        .symbols
        .iter()
        .find(|s| s.name == name && s.kind == kind)
        .unwrap_or_else(|| panic!("missing {kind:?} {name}: {:#?}", result.symbols))
}

fn pending_targets_from(result: &ExtractionResults, from: &Symbol) -> Vec<String> {
    result
        .structured_pending_relationships
        .iter()
        .filter(|p| {
            p.pending.from_symbol_id == from.id && p.pending.kind == RelationshipKind::Calls
        })
        .map(|p| p.target.display_name.clone())
        .collect()
}

fn calls_edge(result: &ExtractionResults, from: &Symbol, to: &Symbol) -> bool {
    result.relationships.iter().any(|r| {
        r.kind == RelationshipKind::Calls && r.from_symbol_id == from.id && r.to_symbol_id == to.id
    })
}

#[test]
fn duplicate_method_names_keep_their_own_call_edges() {
    let source = r#"local Player = {}
Player.__index = Player
function Player:update(dt)
  self.x = clamp(self.x + dt)
  physics.step(dt)
end
local Enemy = {}
Enemy.__index = Enemy
function Enemy:update(dt)
  self.y = clamp(self.y - dt)
  ai.think(self)
end
"#;
    let result = extract("dup.lua", source);
    let updates = symbols_named(&result, "update");
    assert_eq!(updates.len(), 2);
    let player = pending_targets_from(&result, updates[0]);
    let enemy = pending_targets_from(&result, updates[1]);
    assert!(player.contains(&"clamp".to_string()), "{player:?}");
    assert!(player.contains(&"physics.step".to_string()), "{player:?}");
    assert!(enemy.contains(&"clamp".to_string()), "{enemy:?}");
    assert!(enemy.contains(&"ai.think".to_string()), "{enemy:?}");
}

#[test]
fn callers_sharing_a_name_with_export_fields_resolve_local_calls() {
    let source = r#"local function helper(x) return x end
local Cat = {}
function Cat.new() return helper(1) end
local Dog = {}
function Dog.new() return helper(2) end
local function unique() return helper(3) end
return { Cat = Cat, Dog = Dog, unique = unique }
"#;
    let result = extract("names.lua", source);
    let helper = symbol(&result, "helper", SymbolKind::Function);
    let unique = symbol(&result, "unique", SymbolKind::Function);
    let news = symbols_named(&result, "new");
    assert_eq!(news.len(), 2);
    assert!(calls_edge(&result, news[0], helper));
    assert!(calls_edge(&result, news[1], helper));
    assert!(calls_edge(&result, unique, helper));
}

#[test]
fn function_values_span_their_body_and_own_parameters_locals_and_calls() {
    let source = r#"local M = {}
local function helper(v) return v end
M.mul = function(a, b)
  local r = util.scale(a, b)
  return helper(r)
end
local handler = function(req)
  return process(req)
end
M.config = { run = function(opts) return util.run(opts) end }
M.g = function(x)
  if x then
    for i = 1, x do
      if i > 2 then return i end
    end
  end
end
"#;
    let result = extract("module.lua", source);
    let mul = symbol(&result, "mul", SymbolKind::Method);
    assert_eq!((mul.start_line, mul.end_line), (3, 6));
    assert!(mul.body_hash.is_some() && mul.body_span.is_some());
    for param in ["a", "b"] {
        let p = result
            .symbols
            .iter()
            .find(|s| s.name == param)
            .unwrap_or_else(|| panic!("missing parameter {param}"));
        assert_eq!(p.parent_id.as_deref(), Some(mul.id.as_str()));
    }
    let r = symbol(&result, "r", SymbolKind::Variable);
    assert_eq!(r.parent_id.as_deref(), Some(mul.id.as_str()));
    let helper = symbol(&result, "helper", SymbolKind::Function);
    assert!(calls_edge(&result, mul, helper));
    assert!(pending_targets_from(&result, mul).contains(&"util.scale".to_string()));

    let handler = symbol(&result, "handler", SymbolKind::Function);
    assert_eq!((handler.start_line, handler.end_line), (7, 9));
    assert!(handler.body_hash.is_some());
    assert_eq!(pending_targets_from(&result, handler), vec!["process"]);

    let run = symbol(&result, "run", SymbolKind::Method);
    assert!(run.body_hash.is_some());
    assert!(
        result
            .symbols
            .iter()
            .any(|s| s.name == "opts" && s.parent_id.as_deref() == Some(run.id.as_str()))
    );
    assert_eq!(pending_targets_from(&result, run), vec!["util.run"]);

    let g = symbol(&result, "g", SymbolKind::Method);
    let metric = result
        .complexity_metrics
        .iter()
        .find(|m| m.symbol_id.as_deref() == Some(g.id.as_str()))
        .expect("complexity for g");
    assert_eq!(metric.decision_count, 2);
    assert_eq!(metric.loop_count, 1);
    assert_eq!(metric.parameter_count, Some(1));
}

#[test]
fn busted_test_blocks_emit_calls_from_the_test_symbol() {
    let source = r#"describe("calc", function()
  it("adds", function()
    assert.are.equal(3, calc.add(1, 2))
  end)
end)
"#;
    let result = extract("spec/calc_spec.lua", source);
    let adds = result
        .symbols
        .iter()
        .find(|s| s.name == "adds")
        .expect("adds test");
    let targets = pending_targets_from(&result, adds);
    assert!(targets.contains(&"calc.add".to_string()), "{targets:?}");
    assert!(
        targets.contains(&"assert.are.equal".to_string()),
        "{targets:?}"
    );
}

#[test]
fn reassigning_locals_and_parameters_creates_no_new_symbols() {
    let source = r#"local count
local function evaluate(total_count, enabled)
  local total = 0
  if enabled then
    for i = 1, total_count do
      total = total + i
    end
  end
  total_count = 0
  count = (count or 0) + 1
  return total
end
local Account = {}
function Account:new(o)
  o = o or {}
  return o
end
local function bump()
  glob = 1
  glob = glob + 1
end
"#;
    let result = extract("basic.lua", source);
    let totals = symbols_named(&result, "total");
    assert_eq!(totals.len(), 1, "{totals:#?}");
    assert_eq!(totals[0].visibility, Some(Visibility::Private));
    assert_eq!(symbols_named(&result, "total_count").len(), 1);
    assert_eq!(symbols_named(&result, "o").len(), 1);
    let counts = symbols_named(&result, "count");
    assert_eq!(counts.len(), 1, "{counts:#?}");
    assert_eq!(counts[0].visibility, Some(Visibility::Private));
    let bump = symbol(&result, "bump", SymbolKind::Function);
    let globs = symbols_named(&result, "glob");
    assert_eq!(globs.len(), 1, "{globs:#?}");
    assert_eq!(globs[0].parent_id.as_deref(), Some(bump.id.as_str()));
}

#[test]
fn self_field_writes_attach_to_the_method_owner() {
    let source = r#"local Player = {}
Player.__index = Player
function Player:update(dt)
  self.x = dt
end
local Enemy = {}
Enemy.__index = Enemy
function Enemy:update(dt)
  self.y = dt
end
"#;
    let result = extract("owners.lua", source);
    let player = symbols_named(&result, "Player")[0];
    let enemy = symbols_named(&result, "Enemy")[0];
    let x = symbol(&result, "x", SymbolKind::Field);
    let y = symbol(&result, "y", SymbolKind::Field);
    assert_eq!(x.parent_id.as_deref(), Some(player.id.as_str()));
    assert_eq!(y.parent_id.as_deref(), Some(enemy.id.as_str()));
}

#[test]
fn multi_segment_dotted_names_use_the_last_segment_and_resolve_parents() {
    let source = r#"local M = {}
M.config = {}
M.config.timeout = 30
M.config.retry = function(n) return n end
function M.nested.deep(x)
  return util.deep(x)
end
function M.nested.inner:call(y)
  return self:other(y)
end
rules.match.class = 1
"#;
    let result = extract("dotted.lua", source);
    assert!(
        result.symbols.iter().all(|s| !s.name.contains('.')),
        "{:#?}",
        result.symbols
    );
    let config = symbol(&result, "config", SymbolKind::Field);
    let timeout = symbol(&result, "timeout", SymbolKind::Field);
    let retry = symbol(&result, "retry", SymbolKind::Method);
    assert_eq!(timeout.parent_id.as_deref(), Some(config.id.as_str()));
    assert_eq!(retry.parent_id.as_deref(), Some(config.id.as_str()));
    let deep = symbol(&result, "deep", SymbolKind::Method);
    assert!(deep.body_hash.is_some());
    assert_eq!(pending_targets_from(&result, deep), vec!["util.deep"]);
    let call = symbol(&result, "call", SymbolKind::Method);
    let self_param = result
        .symbols
        .iter()
        .find(|s| s.name == "self" && s.parent_id.as_deref() == Some(call.id.as_str()))
        .expect("self parameter for call");
    assert_eq!(
        result
            .types
            .get(&self_param.id)
            .map(|t| t.resolved_type.as_str()),
        Some("inner")
    );
    assert!(
        result
            .symbols
            .iter()
            .any(|s| s.name == "class" && s.kind == SymbolKind::Field)
    );
}

#[test]
fn local_require_alias_records_source_import_edge_and_call_context() {
    let source = r#"local util = require "stack.util"
local Stack = {}
function Stack:_notify(event, value)
  util.emit(self, event, value)
end
"#;
    let result = extract("init.lua", source);
    let util = symbol(&result, "util", SymbolKind::Import);
    let metadata = util.metadata.as_ref().expect("metadata");
    assert_eq!(
        metadata.get("source").and_then(|v| v.as_str()),
        Some("stack.util")
    );
    assert!(result.structured_pending_relationships.iter().any(|p| {
        p.pending.kind == RelationshipKind::Imports
            && p.pending.from_symbol_id == util.id
            && p.target.display_name == "stack.util"
    }));
    let emit = result
        .structured_pending_relationships
        .iter()
        .find(|p| p.target.display_name == "util.emit")
        .expect("pending util.emit");
    assert_eq!(emit.target.import_context.as_deref(), Some("util"));
}
