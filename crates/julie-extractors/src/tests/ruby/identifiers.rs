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
    // Locked variable_ref contract (see csharp/identifiers.rs): receivers + bare
    // value reads, the complement of the Call/MemberAccess/TypeUsage arms.
    //
    // Ruby boundary: a bare lowercase identifier in value position can be a local
    // read OR a receiverless zero-arg method call — tree-sitter-ruby yields a
    // `call` node only when there are parens/args/blocks/receivers, so this arm
    // owns the bare-`identifier` complement and both meanings become name-visible.
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

    // Positive cases (rules 1/4)
    for expected in [
        "count",              // compound-assignment target (`+=`)
        "y",                  // compound-assignment target (`||=`)
        "seed",               // RHS read + receiver of seed.persist
        "total",              // `||=` RHS + ternary reads
        "is_user_type",       // bare argument read
        "visibility_unknown", // bare value read
    ] {
        assert!(
            var_refs.contains(&expected),
            "expected variable_ref for {expected}; got {var_refs:?}"
        );
    }

    // Constants stay owned by the TypeUsage arm — no double emission.
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

    // Negative cases (rules 2/3/4/5)
    for forbidden in [
        "x",            // plain-write LHS only
        "unused_param", // parameter name only
        "ghost_token",  // comment-only mention
        "evaluate",     // method declaration name
        "filter_items", // call callee (owned by the Call arm)
        "persist",      // accessed member name (owned by the MemberAccess arm)
        "private",      // visibility modifier, filtered (rule 5)
    ] {
        assert!(
            !var_refs.contains(&forbidden),
            "{forbidden} must NOT be a variable_ref; got {var_refs:?}"
        );
    }
    assert!(
        !identifiers.iter().any(|id| id.name == "ghost_token"),
        "comment-only ghost_token must not be extracted at all"
    );

    // No duplicate rows: each (name, kind, span) is unique.
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
