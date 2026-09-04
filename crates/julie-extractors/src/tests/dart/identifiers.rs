// Dart identifier extraction tests — type_usage identifiers for type annotations
//
// Dart uses `type_identifier` tree-sitter nodes for type annotations in
// variable declarations, parameter types, return types, generic type arguments,
// implements, extends, with clauses. These must produce TypeUsage identifiers
// for centrality scoring.

use crate::base::IdentifierKind;
use crate::dart::DartExtractor;
use std::path::PathBuf;

fn init_test_parser() -> tree_sitter::Parser {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_dart::LANGUAGE.into())
        .expect("Error loading Dart grammar");
    parser
}

#[test]
fn test_dart_type_usage_identifiers() {
    // Dart type annotations should produce TypeUsage identifiers.
    // These drive centrality scoring for classes, mixins, and typedefs.
    let code = r#"
class UserService {
  User getUser(String name) {
    return User(name);
  }
}

class AuthController extends BaseController {
  late final AuthService service;
  final ProviderContainer container;

  AuthController(this.service, this.container);

  Future<AuthResult> login(LoginRequest request) {
    return service.authenticate(request);
  }
}

mixin LoggerMixin on BaseLogger {
  void log(LogEntry entry);
}

typedef Callback = void Function(Event event);
"#;
    let mut parser = init_test_parser();
    let tree = parser.parse(code, None).unwrap();

    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = DartExtractor::new(
        "dart".to_string(),
        "test.dart".to_string(),
        code.to_string(),
        &workspace_root,
    );
    let symbols = extractor.extract_symbols(&tree);
    let identifiers = extractor.extract_identifiers(&tree, &symbols);

    let type_usages: Vec<_> = identifiers
        .iter()
        .filter(|id| id.kind == IdentifierKind::TypeUsage)
        .collect();

    assert!(
        !type_usages.is_empty(),
        "Dart type annotations must produce TypeUsage identifiers for centrality scoring"
    );

    let type_names: Vec<&str> = type_usages.iter().map(|id| id.name.as_str()).collect();

    // Return type
    assert!(
        type_names.contains(&"User"),
        "Return type 'User' must be extracted. Got: {:?}",
        type_names
    );

    // Field types
    assert!(
        type_names.contains(&"AuthService"),
        "Field type 'AuthService' must be extracted. Got: {:?}",
        type_names
    );
    assert!(
        type_names.contains(&"ProviderContainer"),
        "Field type 'ProviderContainer' must be extracted. Got: {:?}",
        type_names
    );

    // Superclass
    assert!(
        type_names.contains(&"BaseController"),
        "Superclass 'BaseController' must be extracted. Got: {:?}",
        type_names
    );

    // Parameter types
    assert!(
        type_names.contains(&"LoginRequest"),
        "Parameter type 'LoginRequest' must be extracted. Got: {:?}",
        type_names
    );

    // Generic type arguments
    assert!(
        type_names.contains(&"AuthResult"),
        "Generic arg 'AuthResult' must be extracted. Got: {:?}",
        type_names
    );

    // Mixin constraint type
    assert!(
        type_names.contains(&"BaseLogger"),
        "Mixin 'on' constraint 'BaseLogger' must be extracted. Got: {:?}",
        type_names
    );

    // Typedef parameter type
    assert!(
        type_names.contains(&"Event"),
        "Typedef param type 'Event' must be extracted. Got: {:?}",
        type_names
    );

    // LogEntry parameter type
    assert!(
        type_names.contains(&"LogEntry"),
        "Parameter type 'LogEntry' must be extracted. Got: {:?}",
        type_names
    );

    // Should NOT contain declaration names
    assert!(
        !type_names.contains(&"UserService"),
        "Class declaration name 'UserService' must NOT be type_usage. Got: {:?}",
        type_names
    );
    assert!(
        !type_names.contains(&"AuthController"),
        "Class declaration name 'AuthController' must NOT be type_usage. Got: {:?}",
        type_names
    );
    assert!(
        !type_names.contains(&"LoggerMixin"),
        "Mixin declaration name 'LoggerMixin' must NOT be type_usage. Got: {:?}",
        type_names
    );
    assert!(
        !type_names.contains(&"Callback"),
        "Typedef declaration name 'Callback' must NOT be type_usage. Got: {:?}",
        type_names
    );

    // Should NOT contain single-letter generics
    // (not in this test, but verified by the skip logic)
}

#[test]
fn test_dart_type_usage_skips_single_letter_generics() {
    let code = r#"
class Container<T> {
  T value;

  Container(this.value);

  R transform<R>(R Function(T) mapper) {
    return mapper(value);
  }
}
"#;
    let mut parser = init_test_parser();
    let tree = parser.parse(code, None).unwrap();

    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = DartExtractor::new(
        "dart".to_string(),
        "test.dart".to_string(),
        code.to_string(),
        &workspace_root,
    );
    let symbols = extractor.extract_symbols(&tree);
    let identifiers = extractor.extract_identifiers(&tree, &symbols);

    let type_usages: Vec<_> = identifiers
        .iter()
        .filter(|id| id.kind == IdentifierKind::TypeUsage)
        .collect();
    let type_names: Vec<&str> = type_usages.iter().map(|id| id.name.as_str()).collect();

    // Single-letter generics T, R should be filtered
    assert!(
        !type_names.contains(&"T"),
        "Single-letter generic 'T' must be filtered. Got: {:?}",
        type_names
    );
    assert!(
        !type_names.contains(&"R"),
        "Single-letter generic 'R' must be filtered. Got: {:?}",
        type_names
    );

    // Container should NOT appear (it's the declaration name)
    assert!(
        !type_names.contains(&"Container"),
        "Class declaration name 'Container' must NOT be type_usage. Got: {:?}",
        type_names
    );
}

#[test]
fn test_dart_variable_ref_emission() {
    // Locked variable_ref contract (see csharp/identifiers.rs doc comment):
    // receivers + bare value reads, the complement of the Call/MemberAccess/
    // TypeUsage arms.
    let code = r#"
class GraphTraversal {
  static int reach() => 0;
}

class Sample {
  int count = 0;
  int visibilityUnknown = 3;

  // GhostToken appears only in this comment and must never be an identifier.
  int evaluate(int seed, int unusedParam) {
    count += 1;                        // compound assignment -> read count
    var x = 5;                         // declaration name, no ref
    x = 7;                             // plain write LHS -> NOT a read
    var total = seed;                  // initializer -> read seed
    var g = GraphTraversal.reach();    // receiver -> read; reach -> call
    var f = Widget(bar: seed);         // named-arg label names a parameter -> NOT a read
    var s = 'plain $limit brace ${cap + 1}'; // interpolations -> read limit, cap
    return total > 0 ? total : visibilityUnknown;
  }
}
"#;

    let mut parser = init_test_parser();
    let tree = parser.parse(code, None).unwrap();
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = DartExtractor::new(
        "dart".to_string(),
        "test.dart".to_string(),
        code.to_string(),
        &workspace_root,
    );
    let symbols = extractor.extract_symbols(&tree);
    let identifiers = extractor.extract_identifiers(&tree, &symbols);

    let var_refs: Vec<&str> = identifiers
        .iter()
        .filter(|id| id.kind == IdentifierKind::VariableRef)
        .map(|id| id.name.as_str())
        .collect();

    // --- Positive cases (rules 1/4) ---
    for expected in [
        "GraphTraversal",    // static-access receiver
        "count",             // compound-assignment target
        "seed",              // initializer + named-argument VALUE read
        "total",             // ternary condition + consequence
        "visibilityUnknown", // bare read in ternary alternative
        "limit",             // simple `$limit` interpolation read
        "cap",               // `${cap + 1}` interpolation read
    ] {
        assert!(
            var_refs.contains(&expected),
            "expected variable_ref for {expected}; got {var_refs:?}"
        );
    }

    // Receiver + call coexist: GraphTraversal.reach()
    assert!(
        identifiers
            .iter()
            .any(|id| id.name == "reach" && id.kind == IdentifierKind::Call),
        "GraphTraversal.reach() must still yield a call named reach"
    );

    // --- Negative cases (rules 2/3/4/5) ---
    for forbidden in [
        "x",           // declaration name + plain-write LHS
        "unusedParam", // parameter name only
        "GhostToken",  // comment-only mention
        "Sample",      // class declaration name
        "evaluate",    // method declaration name
        "bar",         // named-argument label (names a parameter)
        "Widget",      // call callee (owned by the Call arm)
    ] {
        assert!(
            !var_refs.contains(&forbidden),
            "{forbidden} must NOT be a variable_ref; got {var_refs:?}"
        );
    }
    assert!(
        !identifiers.iter().any(|id| id.name == "GhostToken"),
        "comment-only GhostToken must not be extracted at all"
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

    // Rule 6: containing_symbol_id comes from the SAME byte-range containment
    // helper the sibling arms use. Dart extracts local variables as symbols, so
    // the innermost container of `GraphTraversal.reach()` is the local `g` —
    // assert parity with the sibling Call row rather than a specific symbol.
    assert!(!symbols.is_empty(), "symbols extracted");
    let graph_ref = identifiers
        .iter()
        .find(|id| id.name == "GraphTraversal" && id.kind == IdentifierKind::VariableRef)
        .expect("GraphTraversal variable_ref");
    let reach_call = identifiers
        .iter()
        .find(|id| id.name == "reach" && id.kind == IdentifierKind::Call)
        .expect("reach call identifier");
    assert!(
        graph_ref.containing_symbol_id.is_some(),
        "receiver variable_ref must have a containing symbol"
    );
    assert_eq!(
        graph_ref.containing_symbol_id, reach_call.containing_symbol_id,
        "receiver variable_ref and its sibling call must share a containing symbol"
    );
}
