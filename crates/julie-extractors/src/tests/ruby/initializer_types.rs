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
