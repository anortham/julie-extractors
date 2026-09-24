use crate::base::SymbolKind;
use crate::ruby::RubyExtractor;
use std::path::PathBuf;

fn fact_for(source: &str, name: &str) -> Option<(String, bool)> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_ruby::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let mut extractor = RubyExtractor::new(
        "initializer_types.rb".to_string(),
        source.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    let symbol = symbols
        .iter()
        .find(|s| s.name == name && matches!(s.kind, SymbolKind::Variable | SymbolKind::Field))
        .unwrap_or_else(|| panic!("missing symbol {name}"));
    extractor
        .base
        .type_info
        .get(&symbol.id)
        .map(|fact| (fact.resolved_type.clone(), fact.is_inferred))
}

fn inferred(name: &str) -> Option<(String, bool)> {
    Some((name.to_string(), true))
}

fn written(name: &str) -> Option<(String, bool)> {
    Some((name.to_string(), false))
}

fn in_factory(members: &str, body: &str) -> String {
    format!("class Widget\nend\n\nclass Factory\n{members}\n\n  def run\n    {body}\n  end\nend\n")
}

const SORBET_MEMBERS: &str = r#"
  sig { returns(Widget) }
  def build_widget
  end

  sig { params(size: Integer).returns(Widget) }
  def sized_widget(size)
  end

  sig { returns(T.nilable(Widget)) }
  def maybe_widget
  end

  sig { void }
  def reset
  end

  def untyped_widget
  end
"#;

fn sorbet_local(body: &str) -> Option<(String, bool)> {
    fact_for(&in_factory(SORBET_MEMBERS, body), "w")
}

#[test]
fn sorbet_returns_types_a_bare_receiverless_call() {
    assert_eq!(sorbet_local("w = build_widget"), inferred("Widget"));
}

#[test]
fn sorbet_returns_types_receiverless_and_self_calls_with_arguments() {
    for call in [
        "sized_widget(3)",
        "sized_widget 3",
        "self.sized_widget(3)",
        "self&.build_widget",
    ] {
        assert_eq!(
            sorbet_local(&format!("w = {call}")),
            inferred("Widget"),
            "{call}"
        );
    }
}

#[test]
fn sorbet_nilable_return_records_the_inner_class() {
    assert_eq!(sorbet_local("w = maybe_widget"), inferred("Widget"));
}

#[test]
fn t_must_keeps_the_type_of_its_argument() {
    assert_eq!(sorbet_local("w = T.must(maybe_widget)"), inferred("Widget"));
}

#[test]
fn or_assign_records_the_call_type() {
    assert_eq!(sorbet_local("w ||= build_widget"), inferred("Widget"));
}

#[test]
fn instance_variable_takes_the_call_type() {
    assert_eq!(
        fact_for(&in_factory(SORBET_MEMBERS, "@w = build_widget"), "@w"),
        inferred("Widget")
    );
}

#[test]
fn void_unannotated_and_unknown_methods_record_no_fact() {
    for call in ["reset", "untyped_widget", "missing_widget(1)"] {
        assert_eq!(sorbet_local(&format!("w = {call}")), None, "{call}");
    }
}

#[test]
fn a_call_on_another_receiver_or_a_chained_call_records_no_fact() {
    for value in [
        "other.build_widget",
        "@other.build_widget",
        "build_widget.dup",
        "build_widget.name",
        "Factory.new.build_widget",
    ] {
        assert_eq!(sorbet_local(&format!("w = {value}")), None, "{value}");
    }
}

#[test]
fn a_local_that_shadows_the_method_name_records_no_fact() {
    assert_eq!(
        sorbet_local("build_widget = fetch\n    w = build_widget"),
        None
    );
}

#[test]
fn operator_assignment_records_no_fact() {
    assert_eq!(sorbet_local("w += build_widget"), None);
}

#[test]
fn sorbet_do_block_sig_with_modifiers_and_private_def_records_the_type() {
    let members = r#"
  sig do
    override
      .params(size: Integer)
      .returns(Widget)
      .checked(:never)
  end
  private def build_widget(size)
  end
"#;
    assert_eq!(
        fact_for(&in_factory(members, "w = build_widget(1)"), "w"),
        inferred("Widget")
    );
}

#[test]
fn sorbet_types_without_one_class_record_no_fact() {
    for returned in [
        "T.any(Widget, String)",
        "T::Array[Widget]",
        "T::Boolean",
        "T.untyped",
        "T.self_type",
    ] {
        let members = format!("  sig {{ returns({returned}) }}\n  def self.build_widget\n  end\n");
        let source = format!(
            "class Widget\nend\n\nclass Factory\n{members}\nend\nw = Factory.build_widget\n"
        );
        assert_eq!(fact_for(&source, "w"), None, "{returned}");
    }
}

#[test]
fn sorbet_runtime_classes_under_t_are_ordinary_classes() {
    let members = "  sig { returns(T::Types::Base) }\n  def build_widget\n  end\n";
    assert_eq!(
        fact_for(&in_factory(members, "w = build_widget"), "w"),
        inferred("T::Types::Base")
    );
}

#[test]
fn sorbet_type_parameters_and_type_members_record_no_fact() {
    let source = r#"
class Box
  extend T::Generic
  Elem = type_member

  sig { returns(Elem) }
  def first
  end

  sig { type_parameters(:U).params(x: T.type_parameter(:U)).returns(T.type_parameter(:U)) }
  def pass(x)
  end

  def run
    e = first
    p = pass(1)
  end
end
"#;
    assert_eq!(fact_for(source, "e"), None);
    assert_eq!(fact_for(source, "p"), None);
}

#[test]
fn class_method_calls_on_a_same_file_class_record_the_return_type() {
    let source = r#"
class Widget
  sig { returns(T.attached_class) }
  def self.create
  end

  class << self
    sig { returns(Widget) }
    def load
    end
  end

  #: () -> instance
  def self.open
  end
end

a = Widget.create
b = Widget.load
c = Widget.open
"#;
    assert_eq!(fact_for(source, "a"), inferred("Widget"));
    assert_eq!(fact_for(source, "b"), inferred("Widget"));
    assert_eq!(fact_for(source, "c"), inferred("Widget"));
}

#[test]
fn receiverless_calls_in_a_class_method_use_class_methods_only() {
    let source = r#"
class Widget
end

class Factory
  sig { returns(Widget) }
  def self.create
  end

  sig { returns(String) }
  def create
  end

  def self.run
    w = create
  end
end
"#;
    assert_eq!(fact_for(source, "w"), inferred("Widget"));
}

#[test]
fn an_instance_method_called_on_the_class_records_no_fact() {
    let source = format!(
        "{}\nw = Factory.build_widget\n",
        in_factory(SORBET_MEMBERS, "nil")
    );
    assert_eq!(fact_for(&source, "w"), None);
}

#[test]
fn a_class_method_called_without_receiver_from_an_instance_method_records_no_fact() {
    let members = "  sig { returns(Widget) }\n  def self.build_widget\n  end\n";
    assert_eq!(
        fact_for(&in_factory(members, "w = build_widget"), "w"),
        None
    );
}

#[test]
fn a_method_of_another_class_records_no_fact() {
    let source = r#"
class Widget
end

class Factory
  sig { returns(Widget) }
  def build_widget
  end
end

class Consumer
  def run
    w = build_widget
  end
end
"#;
    assert_eq!(fact_for(source, "w"), None);
}

#[test]
fn same_named_methods_that_disagree_record_no_fact() {
    let source = r#"
class Widget
end

class Gadget
end

class Factory
  sig { returns(Widget) }
  def build
  end
end

class Factory
  sig { returns(Gadget) }
  def build
  end

  def run
    w = build
  end
end
"#;
    assert_eq!(fact_for(source, "w"), None);
}

#[test]
fn same_named_methods_that_agree_record_the_type() {
    let source = r#"
class Widget
end

class Factory
  sig { returns(Widget) }
  def build
  end
end

class Factory
  #: () -> Widget
  def build
  end

  def run
    w = build
  end
end
"#;
    assert_eq!(fact_for(source, "w"), inferred("Widget"));
}

#[test]
fn a_same_named_def_whose_owner_is_unknown_blocks_the_lookup() {
    let source = r#"
class Widget
end

class Factory
  sig { returns(Widget) }
  def build
  end

  included do
    def build
    end
  end

  def run
    w = build
  end
end
"#;
    assert_eq!(fact_for(source, "w"), None);
}

#[test]
fn a_block_that_rebinds_self_records_no_fact() {
    let body = "config.instance_eval do\n      w = build_widget\n    end";
    assert_eq!(sorbet_local(body), None);
}

#[test]
fn rbs_method_types_record_the_return_type() {
    let members = r#"
  #: (Integer) -> Widget
  def sized(size)
  end

  #: () -> Widget?
  def maybe
  end

  # Builds a widget.
  # @rbs size: Integer
  # @rbs return: Widget -- the new widget
  def documented(size)
  end

  #: (Integer size,
  #|  String name) -> Widget
  def continued(size, name)
  end

  #: () -> Widget
  #: (Integer) -> Widget
  def overloaded(size = nil)
  end

  #: () -> self
  def itself_again
  end

  #: () -> Widget | nil
  def or_nil
  end
"#;
    for (call, expected) in [
        ("sized(1)", "Widget"),
        ("maybe", "Widget"),
        ("documented(1)", "Widget"),
        ("continued(1, 'a')", "Widget"),
        ("overloaded", "Widget"),
        ("itself_again", "Factory"),
        ("or_nil", "Widget"),
    ] {
        assert_eq!(
            fact_for(&in_factory(members, &format!("w = {call}")), "w"),
            inferred(expected),
            "{call}"
        );
    }
}

#[test]
fn rbs_method_types_without_one_class_record_no_fact() {
    let members = r#"
  #: () -> Widget | String
  def either
  end

  #: () -> void
  def nothing
  end

  #: () -> untyped
  def anything
  end

  #: () -> Widget
  #: (Integer) -> String
  def mixed(size = nil)
  end

  #: [T] (T) -> T
  def pass(value)
  end

  #: () -> [Widget, String]
  def pair
  end
"#;
    for call in ["either", "nothing", "anything", "mixed", "pass(1)", "pair"] {
        assert_eq!(
            fact_for(&in_factory(members, &format!("w = {call}")), "w"),
            None,
            "{call}"
        );
    }
}

#[test]
fn rbs_self_in_a_class_method_records_no_fact() {
    let source = r#"
class Widget
  #: () -> self
  def self.current
  end
end

w = Widget.current
"#;
    assert_eq!(fact_for(source, "w"), None);
}

#[test]
fn yard_return_tags_record_no_fact() {
    let members = "  # @return [Widget] the widget\n  def build_widget\n  end\n";
    assert_eq!(
        fact_for(&in_factory(members, "w = build_widget"), "w"),
        None
    );
}

#[test]
fn a_comment_that_does_not_touch_the_def_is_not_its_type() {
    let members = "  #: () -> Widget\n\n  def build_widget\n  end\n";
    assert_eq!(
        fact_for(&in_factory(members, "w = build_widget"), "w"),
        None
    );
}

#[test]
fn top_level_methods_type_top_level_calls() {
    let source = r#"
class Widget
end

sig { returns(Widget) }
def build_widget
end

w = build_widget
"#;
    assert_eq!(fact_for(source, "w"), inferred("Widget"));
}

#[test]
fn written_types_win_over_the_initializer() {
    for value in [
        "T.let(build_widget, Gadget)",
        "T.cast(build_widget, Gadget)",
        "build_widget #: Gadget",
        "build_widget #: as Gadget",
        "build_widget #: Gadget?",
    ] {
        assert_eq!(
            sorbet_local(&format!("w = {value}")),
            written("Gadget"),
            "{value}"
        );
    }
}

#[test]
fn a_written_type_without_one_class_blocks_inference() {
    for value in [
        "T.let(build_widget, T.untyped)",
        "build_widget #: untyped",
        "build_widget # see #: notes",
    ] {
        assert_eq!(sorbet_local(&format!("w = {value}")), None, "{value}");
    }
}

#[test]
fn rbs_not_nil_assertion_keeps_the_inferred_type() {
    assert_eq!(
        sorbet_local("w = maybe_widget #: as !nil"),
        inferred("Widget")
    );
}

#[test]
fn same_file_new_still_records_the_class() {
    assert_eq!(sorbet_local("w = Widget.new(1)"), inferred("Widget"));
}

const LABEL_ON_BOTH_SIDES: &str = r#"class Foo
  extend T::Sig
  sig { returns(String) }
  def self.label; "x"; end
  sig { returns(Integer) }
  def label; 1; end
  define_method(:thing) do
    dm_v = label
  end
  before_action do
    ba_v = label
  end
  scope :recent, -> { sc_v = label }
  class_body_v = label
end
"#;

#[test]
fn a_block_in_a_class_body_records_no_fact() {
    for name in ["dm_v", "ba_v", "sc_v"] {
        assert_eq!(fact_for(LABEL_ON_BOTH_SIDES, name), None, "{name}");
    }
    assert_eq!(
        fact_for(LABEL_ON_BOTH_SIDES, "class_body_v"),
        inferred("String")
    );
}

#[test]
fn define_method_in_a_method_records_no_fact() {
    let source = r#"class Foo
  extend T::Sig
  sig { returns(String) }
  def self.label; "x"; end
  sig { returns(Integer) }
  def label; 1; end
  def self.install
    define_method(:thing) { dm_v = label }
    define_singleton_method(:other) { ds_v = label }
    [1].each { each_v = label }
  end
end
"#;
    assert_eq!(fact_for(source, "dm_v"), None);
    assert_eq!(fact_for(source, "ds_v"), None);
    assert_eq!(fact_for(source, "each_v"), inferred("String"));
}

#[test]
fn a_receiverless_call_in_a_same_named_class_of_another_namespace_records_no_fact() {
    let source = r#"module A
  class Item
    extend T::Sig
    sig { returns(String) }
    def name; "a"; end

    def own
      own_v = name
    end
  end
end
module B
  class Item
    def show
      ns_inst = name
    end
  end
end
"#;
    assert_eq!(fact_for(source, "ns_inst"), None);
    assert_eq!(fact_for(source, "own_v"), inferred("String"));
}

#[test]
fn a_class_receiver_resolves_through_the_lexical_nesting() {
    let source = r#"class Item
  extend T::Sig
  sig { returns(String) }
  def self.build; "a"; end
end
module Shop
  class Item
  end
  class Cart
    def add
      cart_item = Item.build
    end
  end
  class Order
    extend T::Sig
    sig { returns(Integer) }
    def self.count; 1; end
  end
  class Report
    def run
      order_count = Order.count
    end
  end
end
class Shop::Ledger
  def run
    outer_count = Order.count
  end
end
"#;
    assert_eq!(fact_for(source, "cart_item"), None);
    assert_eq!(fact_for(source, "order_count"), inferred("Integer"));
    assert_eq!(fact_for(source, "outer_count"), None);
}

#[test]
fn an_ivar_used_by_the_class_and_by_instances_records_no_type() {
    let source = r#"class Registry; end
class Cache
  extend T::Sig
  sig { returns(Registry) }
  def self.registry; Registry.new; end
  def self.setup
    @store = registry
  end
  def initialize
    @store = {}
  end
  def get(k)
    @store.fetch(k)
  end
end
"#;
    assert_eq!(fact_for(source, "@store"), None);
    let instance_side = "def get
    @x.fetch
  end";
    for (class_side, name) in [
        ("def self.setup\n    @x = Registry.new\n  end", "@x"),
        (
            "class << self\n    def setup\n      @x = Registry.new\n    end\n  end",
            "@x",
        ),
        ("@x = Registry.new", "@x"),
        ("@x = T.let(Registry.new, Registry)", "@x"),
        ("before_action do\n    @x = Registry.new\n  end", "@x"),
    ] {
        let source =
            format!("class Registry; end\nclass Cache\n  {class_side}\n  {instance_side}\nend\n");
        assert_eq!(fact_for(&source, name), None, "{class_side}");
    }
}

#[test]
fn an_ivar_used_only_by_the_class_keeps_its_type() {
    let source = "class Registry; end\nclass Cache\n  class << self\n    def setup\n      @x = Registry.new\n    end\n\n    def get\n      @x.fetch\n    end\n  end\nend\n";
    assert_eq!(fact_for(source, "@x"), inferred("Registry"));
}

#[test]
fn rbs_tag_method_types_record_the_return_type() {
    let source = r#"class P
  # @rbs () -> String
  def label; ""; end

  # @rbs (Integer) -> String
  # @rbs (String) -> String
  def agreed(x); ""; end

  # @rbs () -> String
  #    | (Integer) -> Integer
  def overloaded(x = nil); ""; end

  # @rbs [T] (T) -> T
  def identity(x); x; end

  def run
    at_v = label
    agreed_v = agreed(1)
    overloaded_v = overloaded
    identity_v = identity(1)
  end
end
"#;
    assert_eq!(fact_for(source, "at_v"), inferred("String"));
    assert_eq!(fact_for(source, "agreed_v"), inferred("String"));
    assert_eq!(fact_for(source, "overloaded_v"), None);
    assert_eq!(fact_for(source, "identity_v"), None);
}

#[test]
fn a_sorbet_type_alias_records_no_fact() {
    let source = r#"Amount = T.type_alias { T.any(Integer, Float) }
class Calc
  extend T::Sig
  sig { returns(Amount) }
  def total; 1; end
  def run
    alias_v = total
  end
end
"#;
    assert_eq!(fact_for(source, "alias_v"), None);
}

#[test]
fn module_function_and_extend_self_methods_type_module_calls() {
    let source = r#"module Util
  extend T::Sig
  sig { returns(Symbol) }
  def before; :a; end
  module_function
  sig { returns(Symbol) }
  def sym; :a; end
  private
  sig { returns(Symbol) }
  def hidden; :a; end
end
module Named
  extend T::Sig
  sig { returns(Symbol) }
  def one; :a; end
  module_function :one
  sig { returns(Symbol) }
  module_function def two; :a; end
end
module Shared
  extend self
  extend T::Sig
  sig { returns(Symbol) }
  def three; :a; end
end
mf_sym = Util.sym
mf_before = Util.before
mf_hidden = Util.hidden
mf_one = Named.one
mf_two = Named.two
es_three = Shared.three
"#;
    for name in ["mf_sym", "mf_one", "mf_two", "es_three"] {
        assert_eq!(fact_for(source, name), inferred("Symbol"), "{name}");
    }
    for name in ["mf_before", "mf_hidden"] {
        assert_eq!(fact_for(source, name), None, "{name}");
    }
}

#[test]
fn an_ivar_used_at_two_self_levels_gets_no_literal_type() {
    let source = "class Cache\n  def self.setup\n    @store = {}\n  end\n  LIMIT = 3\n  def initialize\n    @size = 1\n    @store.clear\n  end\nend\n";
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_ruby::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let mut extractor = RubyExtractor::new(
        "initializer_types.rb".to_string(),
        source.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    let literal_types = extractor.infer_types(&symbols);
    let literal_type_of = |name: &str| {
        let symbol = symbols.iter().find(|s| s.name == name).unwrap();
        literal_types.get(&symbol.id).cloned()
    };
    assert_eq!(literal_type_of("@store"), None);
    assert_eq!(literal_type_of("LIMIT"), Some("Integer".to_string()));
    assert_eq!(literal_type_of("@size"), Some("Integer".to_string()));
}

#[test]
fn a_class_receiver_that_ruby_finds_through_ancestors_records_no_fact() {
    let items = r#"class Item
  #: -> String
  def self.count; end
end
class Base
  class Item
    #: -> Integer
    def self.count; end
  end
end
module Mixin
  class Item
    #: -> Integer
    def self.count; end
  end
end
"#;
    for (holder, name) in [
        (
            "class Child < Base\n  def f\n    v = Item.count\n  end\nend",
            "superclass",
        ),
        (
            "class Child\n  include Mixin\n  def f\n    v = Item.count\n  end\nend",
            "include",
        ),
        (
            "class Child\n  prepend Mixin\n  def f\n    v = Item.count\n  end\nend",
            "prepend",
        ),
        (
            "class Child < Base; end\nclass Child\n  def f\n    v = Item.count\n  end\nend",
            "reopened",
        ),
        (
            "class Child\n  extend Mixin\n  class << self\n    def f\n      v = Item.count\n    end\n  end\nend",
            "extend",
        ),
        (
            "class Child\n  include Mixin\n  class << self\n    def f\n      v = Item.count\n    end\n  end\nend",
            "include seen from class << self",
        ),
        (
            "class Child\n  def f\n    v = Item.count\n  end\nend\nChild.include(Mixin)",
            "include on a constant receiver",
        ),
    ] {
        assert_eq!(fact_for(&format!("{items}{holder}\n"), "v"), None, "{name}");
    }
    for holder in [
        "class Child\n  def f\n    v = Item.count\n  end\nend",
        "class Child\n  extend Mixin\n  def f\n    v = Item.count\n  end\nend",
        "class Base\n  def f\n    v = Item.count\n  end\nend",
    ] {
        let expected = if holder.starts_with("class Base") {
            "Integer"
        } else {
            "String"
        };
        assert_eq!(
            fact_for(&format!("{items}{holder}\n"), "v"),
            inferred(expected),
            "{holder}"
        );
    }
}

#[test]
fn a_same_file_constant_assignment_shadows_a_class() {
    let source = r#"class Widget
  #: -> String
  def self.make; end
end
class Other
  #: -> Integer
  def self.make; end
end
class Holder
  Widget = Other
  def f
    via_alias = Widget.make
  end
end
class Plain
  def f
    via_class = Widget.make
  end
end
"#;
    assert_eq!(fact_for(source, "via_alias"), None);
    assert_eq!(fact_for(source, "via_class"), inferred("String"));
}

#[test]
fn a_receiverless_call_in_a_top_level_block_records_no_fact() {
    let source = r#"#: -> String
def value; "s"; end

RSpec.describe "x" do
  let(:value) { 42 }
  it "works" do
    in_example = value
  end
end

outside = value
"#;
    assert_eq!(fact_for(source, "in_example"), None);
    assert_eq!(fact_for(source, "outside"), inferred("String"));
}

#[test]
fn a_method_redefined_without_def_records_no_fact() {
    let typed = "  #: -> String\n  def label; end\n  #: -> Integer\n  def count; end\n  #: -> String\n  def other; end\n";
    for redefinition in [
        "alias label count",
        "alias :label :count",
        "alias_method :label, :count",
        "attr_reader :label",
        "attr_accessor :label",
        "attr :label",
        "private attr_reader :label",
        "define_method(:label) { 1 }",
        "define_method('label') { 1 }",
        "delegate :label, to: :target",
        "def_delegators :@target, :label",
        "[:label].each { |name| define_method(name) { 1 } }",
        "prepend Loud",
    ] {
        let source = format!(
            "module Loud\n  def label; 1; end\nend\nclass A\n{typed}  {redefinition}\n  def f\n    v = label\n    kept = other\n  end\nend\n"
        );
        assert_eq!(fact_for(&source, "v"), None, "{redefinition}");
        let kept = if redefinition.contains("each") || redefinition.starts_with("prepend") {
            None
        } else {
            inferred("String")
        };
        assert_eq!(fact_for(&source, "kept"), kept, "{redefinition}");
    }
    let class_side = "class A\n  #: -> String\n  def self.label; end\n  define_singleton_method(:label) { 1 }\n  class << self\n    def f\n      v = label\n    end\n  end\nend\n";
    assert_eq!(fact_for(class_side, "v"), None);
    for prepend in [
        "A.prepend(Loud)",
        "Other.prepend(Loud)",
        "base.prepend(Loud)",
    ] {
        let source = format!(
            "module Loud\n  def label; 1; end\nend\nclass A\n  #: -> String\n  def label; end\n  def f\n    v = label\n  end\nend\n{prepend}\n"
        );
        let expected = if prepend.starts_with("Other") {
            inferred("String")
        } else {
            None
        };
        assert_eq!(fact_for(&source, "v"), expected, "{prepend}");
    }
    let prefixed = "class A\n  #: -> String\n  def target_label; end\n  delegate :label, to: :target, prefix: true\n  def f\n    v = target_label\n  end\nend\n";
    assert_eq!(fact_for(prefixed, "v"), None);
    let other_owner = "class A\n  #: -> String\n  def label; end\n  def f\n    v = label\n  end\nend\nclass B\n  attr_reader :label\nend\n";
    assert_eq!(fact_for(other_owner, "v"), inferred("String"));
}

#[test]
fn locals_bound_by_patterns_and_regex_captures_record_no_fact() {
    let source = r#"class Parser
  #: () -> Parser
  def token = self
  #: () -> Parser
  def word = self
  def shorthand(input)
    case input
    in {token:} then pa = token
    end
  end
  def as_pattern(input)
    case input
    in [Integer => token] then pb = token
    end
  end
  def rightward(input)
    input => {token:}
    pc = token
  end
  def array_element(input)
    case input
    in [token, *] then pd = token
    end
  end
  def keyword_value(input)
    case input
    in {key: token} then pk = token
    end
  end
  def boolean_test(input)
    if input in [token]
      pe = token
    end
  end
  def capture(input)
    /(?<word>\w+)/ =~ input
    rw = word
  end
  def pinned(input)
    case input
    in ^(token) then kept_pin = token
    end
  end
  def unbound
    kept = token
  end
end
"#;
    for name in ["pa", "pb", "pc", "pd", "pk", "pe", "rw"] {
        assert_eq!(fact_for(source, name), None, "{name}");
    }
    assert_eq!(fact_for(source, "kept_pin"), inferred("Parser"));
    assert_eq!(fact_for(source, "kept"), inferred("Parser"));
}

#[test]
fn a_trailing_comment_on_the_statement_above_a_def_is_not_its_type() {
    let source = r#"class Foo
  attr_accessor :callback #: ^() -> String
  def label = 1
  HANDLER = ->(x) { x.to_s } #: ^(Integer) -> String
  def other = 1
  #: -> String
  def named = 1
  def run
    ta = label
    tc = other
    kept = named
  end
end
"#;
    assert_eq!(fact_for(source, "ta"), None);
    assert_eq!(fact_for(source, "tc"), None);
    assert_eq!(fact_for(source, "kept"), inferred("String"));
}

#[test]
fn a_rebound_self_records_no_fact() {
    let source = r#"class Config
end
class Foo
  extend T::Sig
  sig { returns(String) }
  def name; end
  def in_block
    thing.configure do
      T.bind(self, Config)
      bound = name
    end
  end
  def in_def
    T.bind(self, Config)
    tb = name
  end
  def steep_self
    thing.configure do
      # @type self: Config
      steep_v = name
    end
  end
  def unbound
    kept = name
  end
end
class Bar
  extend T::Sig
  # @type instance: Config
  sig { returns(String) }
  def name; end
  def run
    steep_instance = name
  end
end
"#;
    for name in ["bound", "tb", "steep_v", "steep_instance"] {
        assert_eq!(fact_for(source, name), None, "{name}");
    }
    assert_eq!(fact_for(source, "kept"), inferred("String"));
}

#[test]
fn a_qualified_type_alias_or_type_member_records_no_fact() {
    let alias = r#"module Mod
  Result = T.type_alias { T.any(String, Integer) }
end
class Foo
  extend T::Sig
  sig { returns(Mod::Result) }
  def self.get; end
  sig { returns(T.nilable(::Mod::Result)) }
  def self.maybe; end
end
alias_x = Foo.get
alias_y = Foo.maybe
"#;
    assert_eq!(fact_for(alias, "alias_x"), None);
    assert_eq!(fact_for(alias, "alias_y"), None);
    let member = r#"class Box
  extend T::Generic
  Elem = type_member
  sig { returns(Box::Elem) }
  def get; end
  def run
    elem_x = get
  end
end
"#;
    assert_eq!(fact_for(member, "elem_x"), None);
}

#[test]
fn a_trailing_written_type_blocks_the_literal_type() {
    let source = "class Foo\n  def initialize\n    @value = nil #: String | Integer | nil\n    items = [] #: untyped\n    count = 1\n  end\nend\n";
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_ruby::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let mut extractor = RubyExtractor::new(
        "initializer_types.rb".to_string(),
        source.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    let literal_types = extractor.infer_types(&symbols);
    let literal_type_of = |name: &str| {
        let symbol = symbols.iter().find(|s| s.name == name).unwrap();
        literal_types.get(&symbol.id).cloned()
    };
    assert_eq!(literal_type_of("@value"), None);
    assert_eq!(literal_type_of("items"), None);
    assert_eq!(literal_type_of("count"), Some("Integer".to_string()));
    assert_eq!(fact_for(source, "@value"), None);
    assert_eq!(fact_for(source, "items"), None);
}

#[test]
fn constant_with_trailing_written_type_records_the_written_type() {
    let source = r#"
MAX = 5 #: Integer
class Foo
  LIMIT = 10 #: Integer
  PLAIN = 3
end
"#;
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_ruby::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let mut extractor = RubyExtractor::new(
        "initializer_types.rb".to_string(),
        source.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    let fact = |name: &str| {
        let symbol = symbols
            .iter()
            .find(|s| s.name == name && s.kind == SymbolKind::Constant)
            .unwrap_or_else(|| panic!("missing constant {name}"));
        extractor
            .base
            .type_info
            .get(&symbol.id)
            .map(|fact| (fact.resolved_type.clone(), fact.is_inferred))
    };
    assert_eq!(fact("MAX"), written("Integer"));
    assert_eq!(fact("LIMIT"), written("Integer"));
    assert_eq!(fact("PLAIN"), None);
}
