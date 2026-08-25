#[cfg(test)]
mod ruby_test_detection_tests {
    use crate::base::{Symbol, SymbolKind};
    use crate::ruby::RubyExtractor;
    use crate::ruby::helpers::{extract_call_receiver, extract_method_name_from_call};
    use std::path::PathBuf;

    fn extract(file_path: &str, code: &str) -> Vec<Symbol> {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_ruby::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(code, None).unwrap();
        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor =
            RubyExtractor::new(file_path.to_string(), code.to_string(), &workspace_root);
        extractor.extract_symbols(&tree)
    }

    fn find<'a>(symbols: &'a [Symbol], name: &str) -> &'a Symbol {
        symbols
            .iter()
            .find(|symbol| symbol.name == name)
            .unwrap_or_else(|| panic!("no symbol named {name}"))
    }

    fn role(symbol: &Symbol) -> Option<&str> {
        symbol
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("test_role"))
            .and_then(|value| value.as_str())
    }

    fn flag(symbol: &Symbol, key: &str) -> bool {
        symbol
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get(key))
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
    }

    fn base_types(symbol: &Symbol) -> Vec<String> {
        symbol
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("base_types"))
            .and_then(|value| value.as_array())
            .map(|values| {
                values
                    .iter()
                    .filter_map(|value| value.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn rails_test_macro_emits_a_test_case_symbol() {
        let symbols = extract(
            "test/models/order_test.rb",
            r#"
class OrderTest < ActiveSupport::TestCase
  test "computes the total" do
    assert_equal 3, Order.new.total
  end
end
"#,
        );

        let case_symbol = find(&symbols, "computes the total");
        assert_eq!(case_symbol.kind, SymbolKind::Function);
        assert_eq!(role(case_symbol), Some("test_case"));
        assert!(flag(case_symbol, "is_test"));
        assert!(!flag(case_symbol, "test_lifecycle"));
        assert_eq!(
            case_symbol.parent_id.as_deref(),
            Some(find(&symbols, "OrderTest").id.as_str())
        );
    }

    #[test]
    fn rails_block_form_setup_and_teardown_are_lifecycle_hooks() {
        let symbols = extract(
            "test/models/order_test.rb",
            r#"
class OrderTest < ActiveSupport::TestCase
  setup do
    @order = Order.new
  end

  teardown do
    @order = nil
  end

  test "keeps the order" do
    assert @order
  end
end
"#,
        );

        assert_eq!(role(find(&symbols, "setup")), Some("fixture_setup"));
        assert_eq!(role(find(&symbols, "teardown")), Some("fixture_teardown"));
        assert!(flag(find(&symbols, "setup"), "test_lifecycle"));
    }

    #[test]
    fn minitest_base_type_class_is_a_test_container() {
        let symbols = extract(
            "test/calculator_test.rb",
            r#"
class CalculatorTest < Minitest::Test
  def setup
    @calculator = Calculator.new
  end

  def teardown
    @calculator = nil
  end

  def test_adds
    assert_equal 3, @calculator.add(1, 2)
  end

  def build_operand
    1
  end
end
"#,
        );

        let container = find(&symbols, "CalculatorTest");
        assert_eq!(base_types(container), vec!["Minitest::Test".to_string()]);
        assert!(flag(container, "test_container"));
        assert_eq!(role(container), Some("test_container"));

        assert_eq!(role(find(&symbols, "setup")), Some("fixture_setup"));
        assert_eq!(role(find(&symbols, "teardown")), Some("fixture_teardown"));
        assert_eq!(role(find(&symbols, "test_adds")), Some("test_case"));
        assert_eq!(role(find(&symbols, "build_operand")), None);
    }

    #[test]
    fn every_registered_minitest_base_type_marks_a_container() {
        for base_type in [
            "Minitest::Test",
            "Test::Unit::TestCase",
            "ActiveSupport::TestCase",
            "ActionDispatch::IntegrationTest",
        ] {
            let symbols = extract(
                "test/thing_test.rb",
                &format!(
                    r#"
class ThingTest < {base_type}
  def test_works
    assert true
  end
end
"#
                ),
            );
            assert!(
                flag(find(&symbols, "ThingTest"), "test_container"),
                "{base_type} should mark its subclass as a test container"
            );
            assert_eq!(role(find(&symbols, "test_works")), Some("test_case"));
        }
    }

    #[test]
    fn minitest_lifecycle_names_outside_a_container_stay_unclassified() {
        let symbols = extract(
            "test/support/fixture_builder.rb",
            r#"
class FixtureBuilder
  def setup
    @rows = []
  end

  def test_rows
    @rows
  end
end
"#,
        );

        assert_eq!(role(find(&symbols, "setup")), None);
        assert!(!flag(find(&symbols, "setup"), "is_test"));
        assert_eq!(role(find(&symbols, "test_rows")), None);
        assert!(!flag(find(&symbols, "test_rows"), "is_test"));
        assert!(!flag(find(&symbols, "FixtureBuilder"), "test_container"));
    }

    #[test]
    fn production_ruby_never_earns_a_test_role() {
        let symbols = extract(
            "app/services/report_service.rb",
            r#"
describe "not a suite" do
end

before do
end

test "not a rails case" do
end

class ReportService
  def setup
    @rows = []
  end

  def teardown
    @rows = nil
  end

  def test_connection
    true
  end
end
"#,
        );

        for symbol in &symbols {
            assert!(
                !flag(symbol, "is_test"),
                "{} must not be a test in production code",
                symbol.name
            );
            assert!(
                !flag(symbol, "test_container"),
                "{} must not be a test container in production code",
                symbol.name
            );
            assert_eq!(role(symbol), None, "{} must carry no role", symbol.name);
        }
    }

    #[test]
    fn rspec_blocks_carry_explicit_roles() {
        let symbols = extract(
            "spec/order_spec.rb",
            r#"
RSpec.describe Order do
  let(:order) { Order.new }
  let!(:eager) { Order.new }
  subject { described_class.new }

  before do
  end

  after do
  end

  around do |example|
    example.run
  end

  it "adds a line" do
  end

  specify "keeps the total" do
  end
end
"#,
        );

        assert_eq!(role(find(&symbols, "Order")), Some("test_container"));
        assert_eq!(find(&symbols, "Order").kind, SymbolKind::Namespace);
        assert_eq!(role(find(&symbols, "order")), Some("fixture_setup"));
        assert_eq!(role(find(&symbols, "eager")), Some("fixture_setup"));
        assert_eq!(role(find(&symbols, "subject")), Some("fixture_setup"));
        assert_eq!(role(find(&symbols, "before")), Some("fixture_setup"));
        assert_eq!(role(find(&symbols, "after")), Some("fixture_teardown"));
        assert_eq!(role(find(&symbols, "around")), Some("fixture_setup"));
        assert_eq!(role(find(&symbols, "adds a line")), Some("test_case"));
        assert_eq!(role(find(&symbols, "keeps the total")), Some("test_case"));
    }

    #[test]
    fn rspec_focus_and_skip_aliases_are_cases_and_containers() {
        let symbols = extract(
            "spec/alias_spec.rb",
            r#"
xdescribe "skipped suite" do
  xit "skipped case" do
  end
end

fdescribe "focused suite" do
  fit "focused case" do
  end

  xspecify "skipped specify" do
  end
end

fcontext "focused context" do
  it "nested case" do
  end
end

xcontext "skipped context" do
end
"#,
        );

        for container in [
            "skipped suite",
            "focused suite",
            "focused context",
            "skipped context",
        ] {
            assert_eq!(
                role(find(&symbols, container)),
                Some("test_container"),
                "{container} should be a container"
            );
        }
        for case in [
            "skipped case",
            "focused case",
            "skipped specify",
            "nested case",
        ] {
            assert_eq!(
                role(find(&symbols, case)),
                Some("test_case"),
                "{case} should be a case"
            );
        }
    }

    #[test]
    fn shared_example_groups_are_test_containers() {
        let symbols = extract(
            "spec/shared_spec.rb",
            r#"
shared_examples "a countable" do
  it "counts" do
  end
end

shared_context "with a user" do
  let(:user) { User.new }
end
"#,
        );

        assert_eq!(role(find(&symbols, "a countable")), Some("test_container"));
        assert_eq!(role(find(&symbols, "with a user")), Some("test_container"));
        assert_eq!(role(find(&symbols, "counts")), Some("test_case"));
        assert_eq!(role(find(&symbols, "user")), Some("fixture_setup"));
    }

    #[test]
    fn receiver_named_after_a_test_keyword_does_not_create_a_case() {
        let symbols = extract(
            "spec/receiver_spec.rb",
            r#"
test = ExampleRunner.new
test.describe "receiver named test" do
end
"#,
        );

        assert!(
            !symbols
                .iter()
                .any(|symbol| symbol.name == "receiver named test"),
            "a member call must not create a test symbol"
        );
    }

    #[test]
    fn a_member_call_reads_the_method_name_not_the_receiver() {
        let code = "ordinary.it \"member\" do\nend\n";
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_ruby::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(code, None).unwrap();
        let call = tree.root_node().child(0).unwrap();
        assert_eq!(call.kind(), "call");

        let text = |node: &tree_sitter::Node| code[node.byte_range()].to_string();
        assert_eq!(
            extract_method_name_from_call(call, text),
            Some("it".to_string())
        );
        assert_eq!(
            extract_call_receiver(call, |node: &tree_sitter::Node| code[node.byte_range()]
                .to_string()),
            Some("ordinary".to_string())
        );
    }

    #[test]
    fn member_call_named_it_is_not_a_test_case() {
        let symbols = extract(
            "spec/member_spec.rb",
            r#"
ordinary = Object.new
ordinary.it "ordinary member call" do
end
"#,
        );

        assert!(
            !symbols
                .iter()
                .any(|symbol| symbol.name == "ordinary member call"),
            "a member call must not create a test symbol"
        );
    }

    #[test]
    fn top_level_method_in_a_spec_file_without_a_container_is_not_a_case() {
        let symbols = extract(
            "spec/helper_spec.rb",
            r#"
def test_named_case
end
"#,
        );

        assert_eq!(role(find(&symbols, "test_named_case")), None);
        assert!(!flag(find(&symbols, "test_named_case"), "is_test"));
    }
}
