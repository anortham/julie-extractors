use crate::base::{
    IdentifierKind, RelationshipKind, StructuralFact, Symbol, SymbolKind, Visibility,
};
use crate::tests::helpers::{facts_with_pattern, metadata_str};
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

fn at_line<'a>(result: &'a ExtractionResults, name: &str, line: u32) -> &'a Symbol {
    result
        .symbols
        .iter()
        .find(|s| s.name == name && s.start_line == line)
        .unwrap_or_else(|| panic!("no `{name}` at line {line}: {:#?}", result.symbols))
}

fn parent_name(result: &ExtractionResults, symbol: &Symbol) -> Option<String> {
    let parent = symbol.parent_id.as_deref()?;
    result
        .symbols
        .iter()
        .find(|s| s.id == parent)
        .map(|s| s.name.clone())
}

fn is_static(symbol: &Symbol) -> bool {
    symbol
        .metadata
        .as_ref()
        .and_then(|m| m.get("isStatic"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn role(symbol: &Symbol) -> Option<&str> {
    symbol.metadata.as_ref()?.get("test_role")?.as_str()
}

fn symbol_name(result: &ExtractionResults, id: &str) -> String {
    result
        .symbols
        .iter()
        .find(|s| s.id == id)
        .map(|s| s.name.clone())
        .unwrap_or_default()
}

fn edges(result: &ExtractionResults, kind: RelationshipKind) -> Vec<(String, String)> {
    let mut edges: Vec<_> = result
        .relationships
        .iter()
        .filter(|r| r.kind == kind)
        .map(|r| {
            (
                symbol_name(result, &r.from_symbol_id),
                symbol_name(result, &r.to_symbol_id),
            )
        })
        .collect();
    edges.sort();
    edges
}

fn pending(result: &ExtractionResults, kind: RelationshipKind) -> Vec<(String, String)> {
    let mut rows: Vec<_> = result
        .structured_pending_relationships
        .iter()
        .filter(|p| p.pending.kind == kind)
        .map(|p| {
            (
                symbol_name(result, &p.pending.from_symbol_id),
                p.target.terminal_name.clone(),
            )
        })
        .collect();
    rows.sort();
    rows
}

fn fact_array<'a>(fact: &'a StructuralFact, key: &str) -> Vec<&'a str> {
    fact.metadata
        .as_ref()
        .and_then(|m| m.get(key))
        .and_then(|v| v.as_array())
        .map(|values| values.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default()
}

#[test]
fn each_class_body_starts_public_and_symbol_arguments_change_named_members() {
    let source = r#"class Outer
  private

  class Inner
    def inner_public; end
  end

  def self.factory; end

  attr_reader :secret
end

class Widget
  def compute; end
  def shown; end
  def self.build; end
  def guarded; end
  private :compute
  private_class_method :build
  protected :guarded
  private attr_reader :token
  private def hidden; end
end
"#;
    let result = extract("widget.rb", source);
    assert_eq!(
        one(&result, "inner_public").visibility,
        Some(Visibility::Public)
    );
    assert_eq!(one(&result, "factory").visibility, Some(Visibility::Public));
    assert_eq!(one(&result, "secret").visibility, Some(Visibility::Private));
    assert_eq!(
        one(&result, "compute").visibility,
        Some(Visibility::Private)
    );
    assert_eq!(one(&result, "shown").visibility, Some(Visibility::Public));
    assert_eq!(one(&result, "build").visibility, Some(Visibility::Private));
    assert_eq!(
        one(&result, "guarded").visibility,
        Some(Visibility::Protected)
    );
    assert_eq!(one(&result, "token").visibility, Some(Visibility::Private));
    assert_eq!(one(&result, "hidden").visibility, Some(Visibility::Private));
}

#[test]
fn setter_and_index_targets_define_nothing_and_locals_are_defined_once() {
    let source = r#"class Form
  def apply(user, params)
    user.name = params[:name]
    params[:role] = "admin"
    counter = 0
    counter += 1
    counter = 5
    puts $stdout
  end
end
"#;
    let result = extract("form.rb", source);
    let variables: Vec<_> = result
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Variable)
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(variables, vec!["user", "params", "counter"]);
    assert_eq!(one(&result, "counter").start_line, 5);
    let counter_refs: Vec<_> = result
        .identifiers
        .iter()
        .filter(|i| i.name == "counter" && i.kind == IdentifierKind::VariableRef)
        .map(|i| i.start_line)
        .collect();
    assert!(counter_refs.contains(&6), "{counter_refs:?}");
    assert!(!result.symbols.iter().any(|s| s.name == "$stdout"));
}

#[test]
fn instance_class_and_global_variable_reads_are_variable_references() {
    let source = r#"class Service
  @@count = 0
  def initialize(repo)
    @repo = repo
  end

  def fetch(id)
    @@count += 1
    @repo.find(id)
    $stderr.puts(@repo)
  end
end
"#;
    let result = extract("service.rb", source);
    let fetch = one(&result, "fetch").id.clone();
    let refs: Vec<_> = result
        .identifiers
        .iter()
        .filter(|i| i.kind == IdentifierKind::VariableRef && i.name.starts_with(['@', '$']))
        .map(|i| {
            (
                i.name.as_str(),
                i.start_line,
                i.containing_symbol_id.clone(),
            )
        })
        .collect();
    assert!(
        refs.contains(&("@repo", 9, Some(fetch.clone()))),
        "{refs:?}"
    );
    assert!(
        refs.contains(&("@repo", 10, Some(fetch.clone()))),
        "{refs:?}"
    );
    assert!(
        refs.contains(&("@@count", 8, Some(fetch.clone()))),
        "{refs:?}"
    );
    assert!(refs.contains(&("$stderr", 10, Some(fetch))), "{refs:?}");
    assert!(
        !refs
            .iter()
            .any(|(name, line, _)| *name == "@repo" && *line == 4)
    );
}

#[test]
fn superclass_lookup_is_lexical_and_every_mixin_argument_is_recorded() {
    let source = r#"class Base; end

module Payments
  class Base; end
  class Card < Base
    include Auditable, Trackable
  end
end

module V2
  class Card < JsonCard; end
end

class CreateUsers < ActiveRecord::Migration[7.1]; end
Point = Struct.new(:x)
class Shape < Struct.new(:w); end
"#;
    let result = extract("card.rb", source);
    let card = at_line(&result, "Card", 5);
    let extends: Vec<_> = result
        .relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::Extends && r.from_symbol_id == card.id)
        .collect();
    assert_eq!(extends.len(), 1);
    assert_eq!(extends[0].to_symbol_id, at_line(&result, "Base", 4).id);
    assert!(
        pending(&result, RelationshipKind::Implements)
            .contains(&("Card".to_string(), "Auditable".to_string()))
    );
    assert!(
        pending(&result, RelationshipKind::Implements)
            .contains(&("Card".to_string(), "Trackable".to_string()))
    );
    let migration = result
        .structured_pending_relationships
        .iter()
        .find(|p| {
            p.pending.kind == RelationshipKind::Extends && p.target.terminal_name == "Migration"
        })
        .expect("migration superclass");
    assert_eq!(
        migration.target.namespace_path,
        vec!["ActiveRecord".to_string()]
    );
    let json_card = result
        .structured_pending_relationships
        .iter()
        .find(|p| p.target.terminal_name == "JsonCard")
        .expect("JsonCard superclass");
    assert_eq!(
        json_card.pending.from_symbol_id,
        at_line(&result, "Card", 11).id
    );
    assert!(
        !pending(&result, RelationshipKind::Extends)
            .iter()
            .any(|(from, _)| from == "Shape")
    );
    assert_eq!(
        one(&result, "Payments").signature.as_deref(),
        Some("module Payments")
    );
}

#[test]
fn class_new_and_data_define_declare_classes_that_own_their_block_methods() {
    let source = r#"Point = Data.define(:x, :y) do
  def norm
    x + y
  end
end

Handler = Class.new(StandardError) do
  def message
    "handled"
  end
end
"#;
    let result = extract("point.rb", source);
    assert_eq!(one(&result, "Point").kind, SymbolKind::Class);
    for field in ["x", "y"] {
        assert_eq!(one(&result, field).kind, SymbolKind::Property);
        assert_eq!(
            parent_name(&result, one(&result, field)).as_deref(),
            Some("Point")
        );
    }
    assert_eq!(
        parent_name(&result, one(&result, "norm")).as_deref(),
        Some("Point")
    );
    assert_eq!(one(&result, "Handler").kind, SymbolKind::Class);
    assert_eq!(
        parent_name(&result, one(&result, "message")).as_deref(),
        Some("Handler")
    );
    assert!(
        pending(&result, RelationshipKind::Extends)
            .contains(&("Handler".to_string(), "StandardError".to_string()))
    );
}

#[test]
fn singleton_setters_operators_and_class_self_bodies_are_class_methods_of_the_owner() {
    let source = r#"module Config
  class << self
    def load(id); end
  end
  def self.logger=(value); @logger = value; end
  def self.[](key); settings.fetch(key); end
  def total; end
  alias sum total
  alias_method :grand_total, :total
  define_method(:reset!) { }
end
"#;
    let result = extract("config.rb", source);
    for name in ["load", "logger=", "[]"] {
        let method = one(&result, name);
        assert_eq!(method.kind, SymbolKind::Method, "{name}");
        assert!(is_static(method), "{name}");
        assert_eq!(
            parent_name(&result, method).as_deref(),
            Some("Config"),
            "{name}"
        );
    }
    for name in ["sum", "grand_total", "reset!"] {
        let method = one(&result, name);
        assert_eq!(method.kind, SymbolKind::Method, "{name}");
        assert!(!is_static(method), "{name}");
        assert_eq!(
            parent_name(&result, method).as_deref(),
            Some("Config"),
            "{name}"
        );
    }
    assert!(!result.symbols.iter().any(|s| s.name.contains("<<")));
}

#[test]
fn delegation_and_metaprogramming_macros_define_methods_on_the_class() {
    let source = r#"class Map
  extend Forwardable
  def label; end
  alias summary label
  alias_method :to_label, :label
  define_method(:zoom) { 1 }
  def_delegator :@points, :first, :origin
  def_delegators :@points, :size, :each
  delegate :email, :phone, to: :user
  private
  define_singleton_method(:build) { new }
end
"#;
    let result = extract("map.rb", source);
    for name in [
        "summary", "to_label", "zoom", "origin", "size", "each", "email", "phone",
    ] {
        let method = one(&result, name);
        assert_eq!(method.kind, SymbolKind::Method, "{name}");
        assert_eq!(
            parent_name(&result, method).as_deref(),
            Some("Map"),
            "{name}"
        );
        assert_eq!(method.visibility, Some(Visibility::Public), "{name}");
    }
    assert!(is_static(one(&result, "build")));
    assert!(!result.symbols.iter().any(|s| s.name == "first"));
}

#[test]
fn rails_model_macros_define_members_and_callback_symbols_call_methods() {
    let source = r#"class User < ApplicationRecord
  has_many :orders
  has_one :profile
  belongs_to :account
  scope :active, -> { where(active: true) }
  before_save :normalize_email
  validate :email_present, if: :email_required?

  def normalize_email; end
  def email_present; end
  def email_required?; end
end
"#;
    let result = extract("user.rb", source);
    for name in ["orders", "profile", "account"] {
        let member = one(&result, name);
        assert_eq!(member.kind, SymbolKind::Property, "{name}");
        assert_eq!(
            parent_name(&result, member).as_deref(),
            Some("User"),
            "{name}"
        );
    }
    let active = one(&result, "active");
    assert_eq!(active.kind, SymbolKind::Method);
    assert!(is_static(active));
    let calls = edges(&result, RelationshipKind::Calls);
    for target in ["normalize_email", "email_present", "email_required?"] {
        assert!(
            calls.contains(&("User".to_string(), target.to_string())),
            "{target}: {calls:?}"
        );
        assert!(
            result
                .identifiers
                .iter()
                .any(|i| i.name == target && i.kind == IdentifierKind::Call),
            "{target}"
        );
    }
}

#[test]
fn controller_callbacks_and_symbol_dispatch_call_same_class_methods() {
    let source = r#"class PostsController < ApplicationController
  before_action :set_post, only: %i[show edit]
  after_action :track_view
  rescue_from ActiveRecord::RecordNotFound, with: :render_not_found
  helper_method :current_author
  def show
    send(:track_view)
    [1].each(&:set_post)
  end
  private
  def set_post; end
  def track_view; end
  def render_not_found; end
  def current_author; end
end
"#;
    let result = extract("posts_controller.rb", source);
    let calls = edges(&result, RelationshipKind::Calls);
    for (from, to) in [
        ("PostsController", "set_post"),
        ("PostsController", "track_view"),
        ("PostsController", "render_not_found"),
        ("PostsController", "current_author"),
        ("show", "track_view"),
    ] {
        assert!(
            calls.contains(&(from.to_string(), to.to_string())),
            "{from} -> {to}: {calls:?}"
        );
    }
}

#[test]
fn one_liner_examples_are_named_by_line_and_descriptions_are_decoded() {
    let source = r#"RSpec.describe Calculator do
  it { is_expected.to respond_to(:add) }
  specify { expect(subject.add(1, 2)).to eq(3) }
end
RSpec.describe "Quoting" do
  it 'maps "" to nil' do; end
  it "handles 'y'" do; end
  it %q(handles "x") do; end
end
"#;
    let result = extract("spec/calculator_spec.rb", source);
    for (name, signature) in [
        ("example at line 2", "it"),
        ("example at line 3", "specify"),
        ("maps \"\" to nil", "it 'maps \"\" to nil'"),
        ("handles 'y'", "it \"handles 'y'\""),
        ("handles \"x\"", "it %q(handles \"x\")"),
    ] {
        let example = one(&result, name);
        assert_eq!(role(example), Some("test_case"), "{name}");
        assert_eq!(example.signature.as_deref(), Some(signature), "{name}");
    }
    assert_eq!(
        one(&result, "Calculator").signature.as_deref(),
        Some("RSpec.describe Calculator")
    );
    assert!(
        !pending(&result, RelationshipKind::Calls)
            .iter()
            .any(|(_, target)| target == "it" || target == "specify")
    );
}

#[test]
fn hooks_inside_examples_and_fixtures_are_ordinary_calls() {
    let source = r#"RSpec.describe "app" do
  before { setup }
  let(:app) do
    Sinatra.new do
      before { content_type :json }
    end
  end
  it "runs filters" do
    mock_app do
      after { cleanup }
    end
  end
end
"#;
    let result = extract("spec/app_spec.rb", source);
    let hooks: Vec<_> = result
        .symbols
        .iter()
        .filter(|s| s.name == "before" || s.name == "after")
        .map(|s| s.start_line)
        .collect();
    assert_eq!(hooks, vec![2]);
    assert_eq!(role(at_line(&result, "app", 3)), Some("fixture_setup"));
}

#[test]
fn shared_example_inclusions_reference_the_shared_group() {
    let source = r#"RSpec.shared_examples "a countable" do
  it "counts" do; end
end

RSpec.describe List do
  it_behaves_like "a countable"
  include_examples "a countable"
  include_context "with a remote list"
end
"#;
    let result = extract("spec/list_spec.rb", source);
    assert_eq!(
        edges(&result, RelationshipKind::References),
        vec![
            ("List".to_string(), "a countable".to_string()),
            ("List".to_string(), "a countable".to_string()),
        ]
    );
    assert_eq!(
        pending(&result, RelationshipKind::References),
        vec![("List".to_string(), "with a remote list".to_string())]
    );
}

#[test]
fn rspec_metadata_tags_are_annotations_on_groups_and_examples() {
    let source = r#"RSpec.describe List, type: :model do
  it "is slow", :slow do
  end
end
"#;
    let result = extract("spec/list_spec.rb", source);
    let group = one(&result, "List");
    assert_eq!(group.annotations.len(), 1);
    assert_eq!(group.annotations[0].annotation_key, "type");
    assert_eq!(
        group.annotations[0].raw_text.as_deref(),
        Some("type: :model")
    );
    assert_eq!(
        group.annotations[0].carrier.as_deref(),
        Some("rspec_metadata")
    );
    let example = one(&result, "is slow");
    assert_eq!(example.annotations.len(), 1);
    assert_eq!(example.annotations[0].annotation, "slow");
}

#[test]
fn heredoc_arguments_and_query_fragments_are_captured_as_literals() {
    let source = r#"class Stats
  def self.count_orders
    connection.execute(<<~SQL)
      SELECT count(*)
      FROM orders
    SQL
  end

  def self.recent
    where("created_at > ?", 1.day.ago).order("created_at DESC")
  end
end
"#;
    let result = extract("stats.rb", source);
    let literals: Vec<_> = result
        .literals
        .iter()
        .map(|l| {
            (
                l.literal_text.as_str(),
                l.carrier.as_deref().unwrap_or_default(),
                l.arg_position,
                l.start_line,
            )
        })
        .collect();
    assert!(
        literals.contains(&("SELECT count(*)\nFROM orders", "connection.execute", 0, 3)),
        "{literals:?}"
    );
    assert!(
        literals.contains(&("created_at > ?", "where", 0, 10)),
        "{literals:?}"
    );
    assert!(
        literals.contains(&(
            "created_at DESC",
            "where(\"created_at > ?\", 1.day.ago).order",
            0,
            10
        )),
        "{literals:?}"
    );
}

#[test]
fn http_client_libraries_emit_client_request_facts() {
    let source = r#"class Api
  def fetch
    HTTParty.get("https://api.example.com/v1/reports")
    RestClient.post "https://hooks.example.com/x", payload
    conn = Faraday.new(url: "https://svc.example.com")
    conn.get("/items")
    Faraday.new(url: "https://api.example.com").delete("/health")
    uri = URI("https://x.example.com/y")
    Net::HTTP.get(uri)
  end
end
"#;
    let result = extract("api.rb", source);
    let mut requests: Vec<_> = facts_with_pattern(&result, "http.client_request.v1")
        .into_iter()
        .map(|fact| {
            (
                metadata_str(fact, "client").unwrap_or_default().to_string(),
                metadata_str(fact, "verb").unwrap_or_default().to_string(),
                metadata_str(fact, "target_path")
                    .unwrap_or_default()
                    .to_string(),
            )
        })
        .collect();
    requests.sort();
    let expected: Vec<(String, String, String)> = [
        ("faraday", "DELETE", "/health"),
        ("faraday", "GET", "/items"),
        ("httparty", "GET", "https://api.example.com/v1/reports"),
        ("net::http", "GET", "https://x.example.com/y"),
        ("rest-client", "POST", "https://hooks.example.com/x"),
    ]
    .into_iter()
    .map(|(a, b, c)| (a.to_string(), b.to_string(), c.to_string()))
    .collect();
    assert_eq!(requests, expected);
}

#[test]
fn literal_initializers_and_same_file_constructors_give_inferred_types_only() {
    let source = r#"class Client; end

class Parser
  SEPARATORS = %w[, ;]
  PATTERN = %r{\d+}
  def initialize
    @client = Client.new
  end

  def parse(input)
    parts = "a,b".split(",")
    size = "abc".length
  end
end
"#;
    let result = extract("parser.rb", source);
    let type_of = |name: &str| {
        result
            .types
            .get(&one(&result, name).id)
            .map(|t| (t.resolved_type.as_str(), t.is_inferred))
    };
    assert_eq!(type_of("SEPARATORS"), Some(("Array", true)));
    assert_eq!(type_of("PATTERN"), Some(("Regexp", true)));
    assert_eq!(type_of("@client"), Some(("Client", true)));
    assert_eq!(type_of("parts"), None);
    assert_eq!(type_of("size"), None);
    assert_eq!(type_of("input"), None);
}

#[test]
fn rescue_facts_list_every_exception_class_and_bare_rescues_list_none() {
    let source = r#"def run
  work
rescue Timeout::Error, IOError => e
  retry
rescue => e
  log(e)
rescue
  nil
end
"#;
    let result = extract("run.rb", source);
    let rescues = facts_with_pattern(&result, "ruby.rescue_clause.v1");
    assert_eq!(rescues.len(), 3, "{rescues:#?}");
    assert_eq!(
        metadata_str(rescues[0], "exception_type"),
        Some("Timeout::Error")
    );
    assert_eq!(
        fact_array(rescues[0], "exception_types"),
        vec!["Timeout::Error", "IOError"]
    );
    for bare in &rescues[1..] {
        assert_eq!(metadata_str(bare, "exception_type"), None);
    }
}

#[test]
fn require_and_mixin_facts_ignore_messages_to_other_objects() {
    let source = r#"require "json"
class UsersController
  include Auditable
  def user_params
    params.require(:user).permit(:email)
  end
  def decorate(record)
    record.extend(Presenter)
    Kernel.require "yaml"
  end
end
"#;
    let result = extract("users_controller.rb", source);
    let required: Vec<_> = facts_with_pattern(&result, "ruby.require_call.v1")
        .into_iter()
        .filter_map(|fact| metadata_str(fact, "required_path"))
        .collect();
    assert_eq!(required, vec!["json", "yaml"]);
    let mixins: Vec<_> = facts_with_pattern(&result, "ruby.mixin_call.v1")
        .into_iter()
        .filter_map(|fact| metadata_str(fact, "mixin_target"))
        .collect();
    assert_eq!(mixins, vec!["Auditable"]);
}

#[test]
fn ruby_build_files_route_to_the_ruby_extractor() {
    for path in [
        "Gemfile",
        "Rakefile",
        "Guardfile",
        "Capfile",
        "Vagrantfile",
        "Brewfile",
        "config.ru",
        "lib/tasks/seed.rake",
        "widget.gemspec",
        "app/views/users/show.json.jbuilder",
        "feed.builder",
        "tasks.thor",
    ] {
        assert_eq!(
            crate::language_spec::detect_language_for_path(Path::new(path), ""),
            Some("ruby"),
            "{path}"
        );
    }
    assert_eq!(
        crate::language_spec::detect_language_for_path(Path::new("gemfile"), ""),
        None
    );
}

#[test]
fn rake_namespaces_and_tasks_are_symbols_that_contain_their_calls() {
    let source = r#"namespace :cleanup do
  desc "Remove stale users"
  task stale_users: :environment do
    CleanupJob.perform_later(30)
  end
  task :purge, [:days] => :environment do |_t, args|
    Purger.run(args[:days])
  end
end
"#;
    let result = extract("lib/tasks/cleanup.rake", source);
    assert_eq!(one(&result, "cleanup").kind, SymbolKind::Namespace);
    let stale = one(&result, "stale_users");
    assert_eq!(stale.kind, SymbolKind::Function);
    assert_eq!(stale.doc_comment.as_deref(), Some("Remove stale users"));
    assert_eq!(parent_name(&result, stale).as_deref(), Some("cleanup"));
    let calls = pending(&result, RelationshipKind::Calls);
    assert!(calls.contains(&("stale_users".to_string(), "perform_later".to_string())));
    assert!(calls.contains(&("purge".to_string(), "run".to_string())));

    let plain = extract("app/lib/cleanup.rb", source);
    assert!(!plain.symbols.iter().any(|s| s.name == "stale_users"));
}
