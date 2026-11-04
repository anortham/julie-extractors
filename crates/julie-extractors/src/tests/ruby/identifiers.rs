// Tests for Ruby identifier extraction — type_usage for centrality scoring
//
// Ruby has no static type annotations, but constants serve as type references:
// - Superclass references: `class Foo < Bar`
// - Module includes: `include Helpers`
// - Scope resolution: `Sinatra::Base`
// These must produce TypeUsage identifiers so the centrality pipeline can
// boost well-connected classes/modules.
use crate::base::IdentifierKind;
use crate::ruby::RubyExtractor;
use std::path::PathBuf;

#[test]
fn test_ruby_type_usage_identifiers() {
    let code = r#"
class AppController < BaseController
  include Helpers
  extend ClassMethods
  prepend Logging
end

obj = Namespace::HelperClass.new
"#;

    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_ruby::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(code, None).unwrap();

    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor =
        RubyExtractor::new("test.rb".to_string(), code.to_string(), &workspace_root);
    let symbols = extractor.extract_symbols(&tree);
    let identifiers = extractor.extract_identifiers(&tree, &symbols);

    let type_usages: Vec<_> = identifiers
        .iter()
        .filter(|id| id.kind == IdentifierKind::TypeUsage)
        .collect();
    let type_names: Vec<&str> = type_usages.iter().map(|id| id.name.as_str()).collect();

    // Superclass reference MUST be TypeUsage
    assert!(
        type_names.contains(&"BaseController"),
        "Superclass reference 'BaseController' must produce TypeUsage. Got: {:?}",
        type_names
    );

    // include/extend/prepend arguments MUST be TypeUsage
    assert!(
        type_names.contains(&"Helpers"),
        "include arg 'Helpers' must produce TypeUsage. Got: {:?}",
        type_names
    );
    assert!(
        type_names.contains(&"ClassMethods"),
        "extend arg 'ClassMethods' must produce TypeUsage. Got: {:?}",
        type_names
    );
    assert!(
        type_names.contains(&"Logging"),
        "prepend arg 'Logging' must produce TypeUsage. Got: {:?}",
        type_names
    );

    // Scope resolution references MUST be TypeUsage
    assert!(
        type_names.contains(&"Namespace"),
        "Scope resolution namespace 'Namespace' must produce TypeUsage. Got: {:?}",
        type_names
    );
    assert!(
        type_names.contains(&"HelperClass"),
        "Scope resolution name 'HelperClass' must produce TypeUsage. Got: {:?}",
        type_names
    );

    // Class/module declaration names must NOT be TypeUsage
    assert!(
        !type_names.contains(&"AppController"),
        "Class declaration name 'AppController' must NOT be TypeUsage. Got: {:?}",
        type_names
    );
}

#[test]
fn test_ruby_variable_ref_emission() {
    let code = r#"
class Sample
  def evaluate(seed, unused_param)
    count = 0
    count += 1
    x = 5
    x = 7
    total = seed
    g = GraphTraversal.reach
    h = filter_items(is_user_type)
    y = nil
    y ||= total
    # ghost_token appears only in this comment and must never be extracted
    seed.persist
    total > 0 ? total : visibility_unknown
  end

  private
end
"#;

    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_ruby::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(code, None).unwrap();
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor =
        RubyExtractor::new("test.rb".to_string(), code.to_string(), &workspace_root);
    let symbols = extractor.extract_symbols(&tree);
    let identifiers = extractor.extract_identifiers(&tree, &symbols);

    let var_refs: Vec<&str> = identifiers
        .iter()
        .filter(|id| id.kind == IdentifierKind::VariableRef)
        .map(|id| id.name.as_str())
        .collect();
    let access = |name: &str, start: usize| {
        identifiers
            .iter()
            .find(|id| {
                id.name == name
                    && id.kind == IdentifierKind::VariableRef
                    && id.start_byte == start as u32
            })
            .and_then(|id| id.metadata.as_ref())
            .and_then(|metadata| metadata.get("access"))
            .and_then(serde_json::Value::as_str)
    };

    for expected in ["count", "y", "seed", "total"] {
        assert!(
            var_refs.contains(&expected),
            "expected variable_ref for {expected}; got {var_refs:?}"
        );
    }

    for method_call in ["is_user_type", "visibility_unknown"] {
        assert!(!var_refs.contains(&method_call), "{method_call}");
        assert!(
            identifiers
                .iter()
                .any(|id| id.name == method_call && id.kind == IdentifierKind::Call),
            "expected a call identifier for {method_call}"
        );
    }

    assert!(
        !var_refs.contains(&"GraphTraversal"),
        "constant receiver must stay a type_usage, not variable_ref"
    );
    assert!(
        identifiers
            .iter()
            .any(|id| id.name == "GraphTraversal" && id.kind == IdentifierKind::TypeUsage),
        "GraphTraversal receiver must still yield a type_usage"
    );
    assert!(
        identifiers
            .iter()
            .any(|id| id.name == "reach" && id.kind == IdentifierKind::MemberAccess),
        "GraphTraversal.reach must still yield a member access named reach"
    );

    for forbidden in [
        "unused_param",
        "ghost_token",
        "evaluate",
        "filter_items",
        "persist",
        "private",
    ] {
        assert!(
            !var_refs.contains(&forbidden),
            "{forbidden} must NOT be a variable_ref; got {var_refs:?}"
        );
    }
    assert_eq!(access("x", code.find("x = 7").unwrap()), Some("write"));
    assert!(
        !identifiers.iter().any(|id| id.name == "ghost_token"),
        "comment-only ghost_token must not be extracted at all"
    );

    let mut keys: Vec<(String, String, u32, u32)> = identifiers
        .iter()
        .map(|id| {
            (
                id.name.clone(),
                id.kind.to_string(),
                id.start_byte,
                id.end_byte,
            )
        })
        .collect();
    let before = keys.len();
    keys.sort();
    keys.dedup();
    assert_eq!(before, keys.len(), "duplicate identifier rows detected");

    // containing_symbol_id is populated on a variable_ref.
    let evaluate = symbols
        .iter()
        .find(|s| s.name == "evaluate")
        .expect("evaluate method extracted");
    let seed_ref = identifiers
        .iter()
        .find(|id| id.name == "seed" && id.kind == IdentifierKind::VariableRef)
        .expect("seed variable_ref");
    assert_eq!(
        seed_ref.containing_symbol_id.as_deref(),
        Some(evaluate.id.as_str()),
        "seed variable_ref should be contained in evaluate"
    );
}
#[test]
fn setter_declaration_names_do_not_become_calls() {
    let source = "class Config\n  def self.logger=(value); @logger = value; end\n  def logger=(value); @logger = value; end\n  def read; logger; end\nend\n";
    let results = crate::extract_canonical_for_language_at(
        "ruby",
        "source.rb",
        source,
        std::path::Path::new("."),
        crate::ExtractionLevel::Full,
    )
    .unwrap();
    let references: Vec<_> = results
        .identifiers
        .iter()
        .filter(|identifier| identifier.name == "logger")
        .collect();
    assert_eq!(references.len(), 1);
    assert_eq!(references[0].kind, crate::IdentifierKind::Call);
    assert_eq!(
        references[0].start_byte as usize,
        source.rfind("logger").unwrap()
    );
}

#[test]
fn test_ruby_variable_access_facts() {
    let code = r#"
global_plain = 0
global_plain = 1
class Sample
  def writes(initial, box, argument, records)
    initialized = initial
    plain = 0
    plain = initial
    compound = 0
    compound += initial
    box.value = initial
    consume(argument)
    records.each do |captured|
      consume(captured)
    end
  end
end
type_value = Namespace::Payload.new
"#;
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_ruby::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(code, None).unwrap();
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor =
        RubyExtractor::new("test.rb".to_string(), code.to_string(), &workspace_root);
    let symbols = extractor.extract_symbols(&tree);
    let identifiers = extractor.extract_identifiers(&tree, &symbols);
    let access = |name: &str, kind: IdentifierKind, start: usize| {
        identifiers
            .iter()
            .find(|identifier| {
                identifier.name == name
                    && identifier.kind == kind
                    && identifier.start_byte == start as u32
            })
            .and_then(|identifier| identifier.metadata.as_ref())
            .and_then(|metadata| metadata.get("access"))
            .and_then(serde_json::Value::as_str)
    };
    let has_variable_ref_at = |start: usize| {
        identifiers.iter().any(|identifier| {
            identifier.kind == IdentifierKind::VariableRef && identifier.start_byte == start as u32
        })
    };

    let initialized_target = code.find("initialized = initial").unwrap();
    let plain_declaration = code.find("plain = 0").unwrap();
    let plain = code.find("plain = initial").unwrap();
    let global_declaration = code.find("global_plain = 0").unwrap();
    let global_write = code.find("global_plain = 1").unwrap();
    let compound = code.find("compound += initial").unwrap();
    let member = code.find("box.value = initial").unwrap() + "box.".len();
    let argument = code.find("consume(argument)").unwrap() + "consume(".len();
    let binding = code.find("|captured|").unwrap() + 1;
    assert!(!has_variable_ref_at(initialized_target));
    assert!(!has_variable_ref_at(plain_declaration));
    assert!(!has_variable_ref_at(global_declaration));
    assert_eq!(
        access("global_plain", IdentifierKind::VariableRef, global_write),
        Some("write")
    );
    assert_eq!(
        access(
            "initial",
            IdentifierKind::VariableRef,
            code.find("initialized = initial").unwrap() + "initialized = ".len()
        ),
        Some("read")
    );
    assert_eq!(
        access("plain", IdentifierKind::VariableRef, plain),
        Some("write")
    );
    assert_eq!(
        access("compound", IdentifierKind::VariableRef, compound),
        Some("read_write")
    );
    assert_eq!(
        access("value", IdentifierKind::MemberAccess, member),
        Some("write")
    );
    assert_eq!(
        identifiers
            .iter()
            .filter(|identifier| identifier.start_byte == member as u32)
            .count(),
        1
    );
    assert_eq!(
        access("argument", IdentifierKind::VariableRef, argument),
        Some("read")
    );
    let call_start = code.find("consume(argument)").unwrap();
    let call = identifiers
        .iter()
        .find(|identifier| identifier.start_byte == call_start as u32)
        .expect("consume call identifier");
    assert_eq!(call.name, "consume");
    assert_eq!(call.kind, IdentifierKind::Call);
    assert_eq!(
        call.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("access")),
        None
    );
    assert!(
        identifiers
            .iter()
            .any(|identifier| identifier.kind == IdentifierKind::Call)
    );
    assert!(
        identifiers
            .iter()
            .filter(|identifier| identifier.kind == IdentifierKind::Call)
            .all(|identifier| identifier
                .metadata
                .as_ref()
                .is_none_or(|metadata| !metadata.contains_key("access")))
    );
    assert!(
        identifiers
            .iter()
            .any(|identifier| identifier.kind == IdentifierKind::TypeUsage)
    );
    assert!(
        identifiers
            .iter()
            .filter(|identifier| identifier.kind == IdentifierKind::TypeUsage)
            .all(|identifier| identifier
                .metadata
                .as_ref()
                .is_none_or(|metadata| !metadata.contains_key("access")))
    );
    if symbols.iter().any(|symbol| {
        symbol.name == "captured"
            && symbol.start_byte <= binding as u32
            && symbol.end_byte >= (binding + "captured".len()) as u32
    }) {
        assert!(
            identifiers
                .iter()
                .all(|identifier| identifier.start_byte != binding as u32)
        );
    } else {
        assert_eq!(
            access("captured", IdentifierKind::VariableRef, binding),
            Some("write")
        );
    }
}

#[test]
fn shadowed_parameter_access_stays_at_its_byte_occurrences() {
    let code = r#"
value = 1
def update(value)
  value += 1
  value
end
consume(value)
"#;
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_ruby::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(code, None).unwrap();
    assert!(!tree.root_node().has_error());
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor =
        RubyExtractor::new("test.rb".to_string(), code.to_string(), &workspace_root);
    let symbols = extractor.extract_symbols(&tree);
    let identifiers = extractor.extract_identifiers(&tree, &symbols);
    let access_at = |start: usize| -> Option<String> {
        identifiers
            .iter()
            .find(|identifier| {
                identifier.start_byte == start as u32
                    && identifier.kind == IdentifierKind::VariableRef
            })?
            .metadata
            .as_ref()?
            .get("access")?
            .as_str()
            .map(str::to_owned)
    };

    assert_eq!(
        access_at(code.find("value += 1").unwrap()),
        Some("read_write".to_string())
    );
    assert_eq!(
        access_at(code.find("  value\n").unwrap() + "  ".len()),
        Some("read".to_string())
    );
    assert_eq!(
        access_at(code.find("consume(value)").unwrap() + "consume(".len()),
        Some("read".to_string())
    );
}
