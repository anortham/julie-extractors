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
        "GraphTraversal", // static-access receiver
        "count",          // compound-assignment target
        "x",
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

fn variable_access_at(identifiers: &[crate::base::Identifier], start: usize) -> Option<&str> {
    identifiers
        .iter()
        .find(|identifier| identifier.start_byte as usize == start)?
        .metadata
        .as_ref()?
        .get("access")?
        .as_str()
}

fn has_variable_ref_at(identifiers: &[crate::base::Identifier], start: usize) -> bool {
    identifiers.iter().any(|identifier| {
        identifier.kind == IdentifierKind::VariableRef && identifier.start_byte as usize == start
    })
}

#[test]
fn explicit_reads_do_not_duplicate_dart_call_and_property_names() {
    let code = r#"
import 'package:collection/collection.dart' as coll;
class Box { int value = 0; }
class User {
  factory User.fromJson(Map<String, Object?> json) => User._();
  User._();
}
class Cache { Cache._internal(); }
void inspect(Box box, List<int> values, List<Map<String, Object?>> rows) {
  final converted = rows.map((row) => User.fromJson(row)).toList();
  coll.ListEquality<int>().equals([1], [1]);
  Cache._internal();
  print(values.length);
  values[0] = 1;
  box.value = values[0];
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

    let row_binding = code.find("(row) =>").unwrap() + 1;
    assert!(!has_variable_ref_at(&identifiers, row_binding));
    for start in [
        code.find(".map(").unwrap() + 1,
        code.find(".fromJson(").unwrap() + 1,
        code.find("ListEquality").unwrap(),
        code.rfind("_internal").unwrap(),
        code.find("values.length").unwrap() + "values.".len(),
    ] {
        assert!(!has_variable_ref_at(&identifiers, start));
    }
    assert!(
        identifiers
            .iter()
            .any(|identifier| identifier.name == "fromJson"
                && identifier.kind == IdentifierKind::Call)
    );
    assert!(
        identifiers
            .iter()
            .any(|identifier| identifier.name == "length"
                && identifier.kind == IdentifierKind::MemberAccess)
    );
    assert_eq!(
        variable_access_at(&identifiers, code.find("values[0] = 1").unwrap()),
        Some("read")
    );
}

#[test]
fn variable_access_facts_follow_dart_assignment_and_pattern_syntax() {
    let code = r#"
class Box { int value = 0; }
int consume(int value) => value;
void update(int output, int input, Box box, List<int> values) {
  output = input;
  output += 1;
  output++;
  ++output;
  values[output] = input;
  box.value = input;
  box.value++;
  consume(output);
  for (final item in values) {
    consume(item);
  }
  try { consume(output); } catch (error) { consume(error); }
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

    assert_eq!(
        variable_access_at(&identifiers, code.find("output = input").unwrap()),
        Some("write")
    );
    assert_eq!(
        variable_access_at(&identifiers, code.find("output += 1").unwrap()),
        Some("read_write")
    );
    assert_eq!(
        variable_access_at(&identifiers, code.find("output++").unwrap()),
        Some("write")
    );
    assert_eq!(
        variable_access_at(&identifiers, code.find("++output").unwrap() + 2),
        Some("write")
    );
    assert_eq!(
        variable_access_at(
            &identifiers,
            code.find("values[output] = input").unwrap() + "values[".len()
        ),
        Some("read")
    );
    assert_eq!(
        variable_access_at(&identifiers, code.find("values[output] = input").unwrap()),
        Some("read")
    );
    assert_eq!(
        variable_access_at(
            &identifiers,
            code.find("box.value = input").unwrap() + "box.".len()
        ),
        Some("write")
    );
    let member_start = (code.find("box.value = input").unwrap() + "box.".len()) as u32;
    let member_targets: Vec<_> = identifiers
        .iter()
        .filter(|identifier| identifier.start_byte == member_start)
        .collect();
    assert_eq!(member_targets.len(), 1);
    assert_eq!(member_targets[0].kind, IdentifierKind::VariableRef);
    assert_eq!(
        variable_access_at(&identifiers, code.find("box.value = input").unwrap()),
        Some("read")
    );
    assert_eq!(
        variable_access_at(
            &identifiers,
            code.find("box.value++").unwrap() + "box.".len()
        ),
        Some("write")
    );
    assert_eq!(
        variable_access_at(
            &identifiers,
            code.find("consume(output)").unwrap() + "consume(".len()
        ),
        Some("read")
    );
    assert_eq!(
        variable_access_at(
            &identifiers,
            code.find("final item").unwrap() + "final ".len()
        ),
        Some("write")
    );
    assert_eq!(
        variable_access_at(
            &identifiers,
            code.find("consume(item)").unwrap() + "consume(".len()
        ),
        Some("read")
    );
    assert_eq!(
        variable_access_at(
            &identifiers,
            code.find("catch (error)").unwrap() + "catch (".len()
        ),
        Some("write")
    );
    assert_eq!(
        variable_access_at(
            &identifiers,
            code.find("consume(error)").unwrap() + "consume(".len()
        ),
        Some("read")
    );
}
#[test]
fn shadowed_bindings_keep_exact_access_and_non_access_facts() {
    let code = r#"
class Marker {}
void consume(int value) {}
void update(Marker marker, int count) {
    consume(count);
    if (true) {
        var count = 0;
        count += 1;
        consume(count);
    }
    consume(count);
}
"#;
    let mut parser = init_test_parser();
    let tree = parser.parse(code, None).unwrap();
    assert!(!tree.root_node().has_error());
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = DartExtractor::new(
        "dart".to_string(),
        "test.dart".to_string(),
        code.to_string(),
        &workspace_root,
    );
    let symbols = extractor.extract_symbols(&tree);
    let identifiers = extractor.extract_identifiers(&tree, &symbols);

    let call_name = "consume";
    let call_expression = format!("{call_name}(count)");
    let call_starts: Vec<usize> = code
        .match_indices(&call_expression)
        .map(|(start, _)| start)
        .collect();
    assert_eq!(call_starts.len(), 3);
    let calls: Vec<_> = identifiers
        .iter()
        .filter(|identifier| {
            identifier.name == call_name && identifier.kind == IdentifierKind::Call
        })
        .collect();
    assert_eq!(calls.len(), call_starts.len());
    assert!(calls.iter().all(|identifier| {
        identifier
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("access"))
            .is_none()
    }));

    let outer_before_start = call_starts[0] + call_name.len() + 1;
    let inner_read_start = call_starts[1] + call_name.len() + 1;
    let outer_after_start = call_starts[2] + call_name.len() + 1;
    let compound_start = code.find("count += 1").unwrap();
    let access_at = |start: usize| {
        identifiers
            .iter()
            .find(|identifier| {
                identifier.name == "count"
                    && identifier.kind == IdentifierKind::VariableRef
                    && identifier.start_byte as usize == start
            })
            .and_then(|identifier| identifier.metadata.as_ref())
            .and_then(|metadata| metadata.get("access"))
            .and_then(serde_json::Value::as_str)
    };
    assert_eq!(access_at(outer_before_start), Some("read"));
    assert_eq!(access_at(compound_start), Some("read_write"));
    assert_eq!(access_at(inner_read_start), Some("read"));
    assert_eq!(access_at(outer_after_start), Some("read"));

    let owner_id = identifiers
        .iter()
        .find(|identifier| {
            identifier.name == "count"
                && identifier.kind == IdentifierKind::VariableRef
                && identifier.start_byte as usize == outer_before_start
        })
        .and_then(|identifier| identifier.containing_symbol_id.as_deref())
        .expect("outer variable reference should have a containing function");
    for start in [compound_start, inner_read_start, outer_after_start] {
        let identifier = identifiers
            .iter()
            .find(|identifier| {
                identifier.name == "count"
                    && identifier.kind == IdentifierKind::VariableRef
                    && identifier.start_byte as usize == start
            })
            .expect("shadowed variable occurrence");
        assert_eq!(identifier.containing_symbol_id.as_deref(), Some(owner_id));
    }
    let owner = symbols
        .iter()
        .find(|symbol| symbol.id.as_str() == owner_id)
        .expect("containing function should be extracted");
    assert_eq!(owner.name, "update");

    let type_usages: Vec<_> = identifiers
        .iter()
        .filter(|identifier| identifier.kind == IdentifierKind::TypeUsage)
        .collect();
    assert!(!type_usages.is_empty());
    let type_start = code.find("Marker marker").unwrap();
    let type_usage = type_usages
        .iter()
        .find(|identifier| {
            identifier.name == "Marker" && identifier.start_byte as usize == type_start
        })
        .expect("Marker type usage at exact source offset");
    assert!(
        type_usage
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("access"))
            .is_none()
    );
}

#[test]
fn code_inside_a_getter_belongs_to_the_getter() {
    let code = r#"
class Money {
  final int cents;
  Money(this.cents);
  int get hashCode => cents.hashCode;
  set rounded(int value) => print(value);
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

    let getter = symbols
        .iter()
        .find(|s| s.name == "hashCode" && s.kind == crate::base::SymbolKind::Property)
        .expect("hashCode getter");
    let read = identifiers
        .iter()
        .find(|id| id.name == "cents" && id.start_line == 5)
        .expect("cents read in the getter");
    assert_eq!(
        read.containing_symbol_id.as_deref(),
        Some(getter.id.as_str())
    );

    let setter = symbols
        .iter()
        .find(|s| s.name == "rounded" && s.kind == crate::base::SymbolKind::Property)
        .expect("rounded setter");
    let call = identifiers
        .iter()
        .find(|id| id.name == "print" && id.kind == IdentifierKind::Call)
        .expect("print call in the setter");
    assert_eq!(
        call.containing_symbol_id.as_deref(),
        Some(setter.id.as_str())
    );
}
