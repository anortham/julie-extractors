use crate::base::{IdentifierKind, RelationshipKind, Symbol, SymbolKind};
use crate::{ExtractionResults, extract_canonical};
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("ruby extraction")
}

fn one<'a>(result: &'a ExtractionResults, name: &str) -> &'a Symbol {
    let found: Vec<_> = result.symbols.iter().filter(|s| s.name == name).collect();
    assert_eq!(found.len(), 1, "expected one `{name}`, got {found:#?}");
    found[0]
}

fn pending_targets(result: &ExtractionResults) -> Vec<(String, Option<String>, u32)> {
    let mut targets: Vec<_> = result
        .structured_pending_relationships
        .iter()
        .filter(|p| p.pending.kind == RelationshipKind::Calls)
        .map(|p| {
            (
                p.target.terminal_name.clone(),
                p.target.receiver.clone(),
                p.pending.line_number,
            )
        })
        .collect();
    targets.sort();
    targets
}

fn calls(result: &ExtractionResults) -> Vec<(String, String)> {
    let name = |id: &str| {
        result
            .symbols
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.name.clone())
            .unwrap_or_default()
    };
    let mut edges: Vec<_> = result
        .relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::Calls)
        .map(|r| (name(&r.from_symbol_id), name(&r.to_symbol_id)))
        .collect();
    edges.sort();
    edges
}

fn role(symbol: &Symbol) -> Option<&str> {
    symbol.metadata.as_ref()?.get("test_role")?.as_str()
}

#[test]
fn pending_call_targets_come_from_call_fields() {
    let source = r#"class Importer
  def run(rows)
    render json: @user.attributes
    Mailer.with(to: email).receipt.deliver_later
    logger.info "Imported rows. Done (ok)"
    rows.map { |r| r.to_h(1) }.join(", ")
    Api::V1::Base.new
    self.mode = 1
  end
end
"#;
    let result = extract("importer.rb", source);
    let targets = pending_targets(&result);
    let mut expected: Vec<(String, Option<String>, u32)> = [
        ("attributes", Some("@user"), 3),
        ("deliver_later", Some("receipt"), 4),
        ("info", Some("logger"), 5),
        ("join", Some("map"), 6),
        ("logger", None, 5),
        ("map", Some("rows"), 6),
        ("mode=", Some("self"), 8),
        ("new", Some("Base"), 7),
        ("receipt", Some("with"), 4),
        ("render", None, 3),
        ("to_h", Some("r"), 6),
        ("with", Some("Mailer"), 4),
        ("email", None, 4),
    ]
    .into_iter()
    .map(|(n, r, l)| (n.to_string(), r.map(str::to_string), l))
    .collect();
    expected.sort();
    assert_eq!(targets, expected);
    let new_call = result
        .structured_pending_relationships
        .iter()
        .find(|p| p.target.terminal_name == "new")
        .unwrap();
    assert_eq!(new_call.target.namespace_path, vec!["Api", "V1"]);
}

#[test]
fn constant_and_global_reads_and_writer_calls_define_no_symbols() {
    let source = r#"class Parser
  DEFAULTS = { strict: true }.freeze
  def defaults
    DEFAULTS
  end
  def coerce(value)
    case value
    when Hash then value
    end
    kind = Symbol
  rescue TypeError, ArgumentError => e
    warn e.message
  end
  def program
    $PROGRAM_NAME || $0
  end
  def configure(options)
    self.mode = options[:mode]
    options[:seen] = true
    Parser.default_mode = :strict
    $verbose = true
  end
end
"#;
    let result = extract("parser.rb", source);
    let names: Vec<_> = result
        .symbols
        .iter()
        .filter(|s| matches!(s.kind, SymbolKind::Constant | SymbolKind::Variable))
        .filter(|s| s.metadata.as_ref().and_then(|m| m.get("role")).is_none())
        .map(|s| (s.name.as_str(), s.kind.clone()))
        .collect();
    assert_eq!(
        names,
        vec![
            ("DEFAULTS", SymbolKind::Constant),
            ("kind", SymbolKind::Variable),
            ("$verbose", SymbolKind::Variable),
        ]
    );
    assert!(
        result
            .identifiers
            .iter()
            .any(|i| i.name == "Hash" && i.kind == IdentifierKind::TypeUsage)
    );
}

#[test]
fn receiverless_calls_resolve_to_same_class_methods() {
    let source = r#"class Order
  def total
    subtotal + tax
  end
  def subtotal
    items.sum(&:price)
  end
  def tax
    rate = tax_rate
    subtotal * rate
  end
  def audit_all(entries)
    entries.each { |entry| send(:audit, entry) }
    each(&method(:audit))
  end
  def audit(entry); entry; end
  def tax_rate; 0.1; end
  def items; []; end
end
"#;
    let result = extract("order.rb", source);
    let edges = calls(&result);
    for (from, to) in [
        ("total", "subtotal"),
        ("total", "tax"),
        ("subtotal", "items"),
        ("tax", "tax_rate"),
        ("tax", "subtotal"),
        ("audit_all", "audit"),
    ] {
        assert!(
            edges.contains(&(from.to_string(), to.to_string())),
            "missing {from} -> {to}: {edges:?}"
        );
    }
    assert!(!edges.iter().any(|(_, to)| to == "rate"));
    let rate_ident = result
        .identifiers
        .iter()
        .find(|i| i.name == "rate" && i.start_line == 10)
        .unwrap();
    assert_eq!(rate_ident.kind, IdentifierKind::VariableRef);
    let tax_rate_ident = result
        .identifiers
        .iter()
        .find(|i| i.name == "tax_rate")
        .unwrap();
    assert_eq!(tax_rate_ident.kind, IdentifierKind::Call);
    assert!(
        pending_targets(&result)
            .iter()
            .any(|(name, _, line)| name == "price" && *line == 6)
    );
}

#[test]
fn super_calls_the_enclosing_method_on_the_superclass() {
    let source = r#"class Admin < User
  def save(record)
    super
    super(record)
  end
end
"#;
    let result = extract("admin.rb", source);
    let supers: Vec<_> = result
        .structured_pending_relationships
        .iter()
        .filter(|p| p.target.receiver.as_deref() == Some("super"))
        .collect();
    assert_eq!(supers.len(), 2, "{supers:#?}");
    for pending in supers {
        assert_eq!(pending.target.terminal_name, "save");
        assert_eq!(pending.receiver_type.as_deref(), Some("User"));
    }
}

#[test]
fn doc_comments_attach_to_adjacent_declarations_only() {
    let source = r#"#!/usr/bin/env ruby
# frozen_string_literal: true

module Billing
  # First class doc.
  class Invoice
    # First method doc.
    def total; 1; end
    # Reader doc.
    attr_reader :amount

    # rubocop:disable Metrics/AbcSize
    def other; end

    # TODO: remove this

    def later; end
  end
end
"#;
    let result = extract("billing.rb", source);
    let doc = |name: &str| one(&result, name).doc_comment.clone();
    assert_eq!(doc("Billing"), None);
    assert_eq!(doc("Invoice").as_deref(), Some("# First class doc."));
    assert_eq!(doc("total").as_deref(), Some("# First method doc."));
    assert_eq!(doc("amount").as_deref(), Some("# Reader doc."));
    assert_eq!(doc("other"), None);
    assert_eq!(doc("later"), None);
}

#[test]
fn compact_scoped_declarations_use_the_terminal_name() {
    let source = r#"module Api::V1
  class UsersController < Api::V1::BaseController
  end
end
class Api::V1::BaseController < ApplicationController
end
"#;
    let result = extract("controllers.rb", source);
    let base = one(&result, "BaseController");
    assert_eq!(base.kind, SymbolKind::Class);
    assert_eq!(
        base.signature.as_deref(),
        Some("class Api::V1::BaseController < ApplicationController")
    );
    assert_eq!(
        base.metadata.as_ref().unwrap()["qualifiedName"],
        "Api::V1::BaseController"
    );
    let module = one(&result, "V1");
    assert_eq!(module.kind, SymbolKind::Module);
    let users = one(&result, "UsersController");
    assert_eq!(
        users.metadata.as_ref().unwrap()["qualifiedName"],
        "Api::V1::UsersController"
    );
    assert!(
        result
            .relationships
            .iter()
            .any(|r| r.kind == RelationshipKind::Extends
                && r.from_symbol_id == users.id
                && r.to_symbol_id == base.id),
        "the qualified superclass resolves to the same-file BaseController"
    );
    let external = extract(
        "users_controller.rb",
        "class UsersController < Api::V1::BaseController
end
",
    );
    let pending = external
        .structured_pending_relationships
        .iter()
        .find(|p| p.pending.kind == RelationshipKind::Extends)
        .unwrap();
    assert_eq!(pending.target.terminal_name, "BaseController");
    assert_eq!(pending.target.namespace_path, vec!["Api", "V1"]);
}

#[test]
fn rails_component_test_cases_are_containers() {
    let source = r#"class UsersTest < ApplicationSystemTestCase
  setup do
    @user = users(:one)
  end
  test "visiting the index" do
    visit users_url
  end
end
class UserMailerTest < ActionMailer::TestCase
  def test_welcome
  end
end
class CleanupJobTest < ActiveJob::TestCase
  def test_enqueues
  end
end
"#;
    let result = extract("test/system/users_test.rb", source);
    for name in ["UsersTest", "UserMailerTest", "CleanupJobTest"] {
        assert_eq!(role(one(&result, name)), Some("test_container"), "{name}");
    }
    assert_eq!(role(one(&result, "test_welcome")), Some("test_case"));
    assert_eq!(role(one(&result, "test_enqueues")), Some("test_case"));
    let roles: Vec<_> = result
        .symbols
        .iter()
        .filter(|s| s.parent_id.as_deref() == Some(one(&result, "UsersTest").id.as_str()))
        .filter_map(role)
        .collect();
    assert!(roles.contains(&"fixture_setup"), "{roles:?}");
    assert!(roles.contains(&"test_case"), "{roles:?}");
}

#[test]
fn sinatra_routes_and_filters_emit_facts() {
    let source = r#"class Api < Sinatra::Base
  before "/admin/*" do
    halt 401
  end
  get "/users/:id" do
    User.find(params[:id]).to_json
  end
  post "/users" do
    status 201
  end
  get "/dynamic/#{VERSION}" do
  end
end
class Plain
  get "/not/a/route" do
  end
end
"#;
    let result = extract("app.rb", source);
    let facts: Vec<(String, String, Option<String>)> = result
        .structural_facts
        .iter()
        .filter(|f| f.pattern_id.starts_with("sinatra."))
        .map(|f| {
            let m = f.metadata.as_ref().unwrap();
            (
                f.pattern_id.clone(),
                m["route_template"].as_str().unwrap().to_string(),
                m.get("verb").and_then(|v| v.as_str()).map(str::to_string),
            )
        })
        .collect();
    assert_eq!(
        facts,
        vec![
            (
                "sinatra.filter.v1".to_string(),
                "/admin/*".to_string(),
                None
            ),
            (
                "sinatra.route.v1".to_string(),
                "/users/:id".to_string(),
                Some("GET".to_string())
            ),
            (
                "sinatra.route.v1".to_string(),
                "/users".to_string(),
                Some("POST".to_string())
            ),
        ]
    );
    let classic = extract(
        "classic.rb",
        "require \"sinatra\"\n\nget \"/\" do\n  \"hi\"\nend\n",
    );
    assert_eq!(
        classic
            .structural_facts
            .iter()
            .filter(|f| f.pattern_id == "sinatra.route.v1")
            .count(),
        1
    );
}
