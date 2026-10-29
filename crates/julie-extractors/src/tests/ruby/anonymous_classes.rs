use crate::ExtractionResults;
use crate::extract_canonical;
use crate::tests::helpers::{
    call_owners, owner_rows, supertype_edges, symbol_metadata, symbol_named, test_flags,
};
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("ruby extraction")
}

#[test]
fn class_new_block_without_a_constant_owns_its_methods() {
    let result = extract(
        "lib/make.rb",
        "class Base; end\n\
         def make\n\
         \x20 Class.new(Base) do\n\
         \x20   def go\n\
         \x20     helper\n\
         \x20   end\n\
         \x20 end\n\
         end\n\
         row = Struct.new(:a) do\n\
         \x20 def b; a; end\n\
         end\n",
    );

    assert_eq!(
        owner_rows(&result),
        [
            "class base < -",
            "method make < -",
            "class anonymous_class_l3 < make",
            "method go < anonymous_class_l3",
            "class anonymous_class_l9 < -",
            "property a < anonymous_class_l9",
            "method b < anonymous_class_l9",
        ]
    );
    let anonymous = symbol_named(&result, "anonymous_class_L3");
    assert_eq!(symbol_metadata(anonymous, "type"), Some("anonymous_class"));
    assert_eq!(anonymous.signature.as_deref(), Some("Class.new(Base)"));
    assert_eq!(
        anonymous.metadata.as_ref().unwrap()["base_types"],
        serde_json::json!(["Base"])
    );
    assert_eq!(
        supertype_edges(&result),
        ["extends anonymous_class_l3 -> base"]
    );
    assert_eq!(call_owners(&result), ["helper in go", "a in b"]);
}

#[test]
fn anonymous_test_classes_keep_test_detection() {
    let result = extract(
        "test/worker_test.rb",
        "require \"minitest/autorun\"\n\
         class WorkerTest < Minitest::Test\n\
         \x20 def test_runs\n\
         \x20   klass = Class.new(Minitest::Test) do\n\
         \x20     def test_inner; end\n\
         \x20   end\n\
         \x20 end\n\
         end\n\
         def build\n\
         \x20 Class.new(Minitest::Test) do\n\
         \x20   def test_outside; end\n\
         \x20 end\n\
         end\n",
    );

    assert_eq!(
        test_flags(&result),
        [
            "WorkerTest: test_container,test_role",
            "test_inner: is_test,test_role",
            "test_runs: is_test,test_role",
        ]
    );
}

#[test]
fn constant_named_and_blockless_constructors_keep_their_rows() {
    let result = extract(
        "lib/named.rb",
        "class Base; end\n\
         Worker = Class.new(Base) do\n\
         \x20 def run; end\n\
         end\n\
         Point = Struct.new(:x)\n\
         class Row < Struct.new(:a)\n\
         end\n\
         plain = Class.new\n",
    );

    assert_eq!(
        owner_rows(&result),
        [
            "class base < -",
            "class worker < -",
            "method run < worker",
            "class point < -",
            "property x < point",
            "class row < -",
        ]
    );
    assert_eq!(supertype_edges(&result), ["extends worker -> base"]);
}
