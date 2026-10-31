use crate::base::{Identifier, IdentifierKind, Symbol};
use crate::swift::SwiftExtractor;
use std::path::PathBuf;

fn extract_all(code: &str) -> (Vec<Symbol>, Vec<Identifier>) {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_swift::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(code, None).unwrap();
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = SwiftExtractor::new(
        "swift".to_string(),
        "test.swift".to_string(),
        code.to_string(),
        &workspace_root,
    );
    let symbols = extractor.extract_symbols(&tree);
    let identifiers = extractor.extract_identifiers(&tree, &symbols);
    (symbols, identifiers)
}

#[test]
fn test_swift_type_usage_identifiers_cover_properties_params_returns_and_generics() {
    let code = r#"
class User {}
class LoginRequest {}
class AuthResult {}
class Repository<T> {}

class AuthController {
    private let repository: Repository<User>
    var cachedUsers: [User]
    var lookup: [String: AuthResult]

    func login(request: LoginRequest, fallback: User?) -> AuthResult {
        return AuthResult()
    }
}
"#;

    let (_symbols, identifiers) = extract_all(code);
    let type_names: Vec<&str> = identifiers
        .iter()
        .filter(|id| id.kind == IdentifierKind::TypeUsage)
        .map(|id| id.name.as_str())
        .collect();

    for expected in ["Repository", "User", "AuthResult", "LoginRequest"] {
        assert!(
            type_names.contains(&expected),
            "missing Swift type usage {expected}; got {type_names:?}"
        );
    }

    assert!(
        !type_names.contains(&"String"),
        "Swift primitive/library noise should stay filtered: {type_names:?}"
    );
}

#[test]
fn test_swift_variable_ref_emission() {
    let code = r#"
enum GraphTraversal {
    static func reach() -> Int { return 0 }
}

@available(iOS 13.0, *)
protocol Job {
    func run() -> Int
}

@available(*, deprecated, message: "use Sample")
class Legacy {}

class Sample {
    var count = 0
    let visibilityUnknown = 3

    // GhostToken appears only in this comment and must never be an identifier.
    func evaluate(seed: Int, unusedParam: Int) -> Int {
        count += 1
        var x = 5
        x = 7
        let total = seed
        let g = GraphTraversal.reach()
        let f = Widget(bar: seed)
        let s = "brace \(limit + 1)"
        return total > 0 ? total : visibilityUnknown
    }
}
"#;

    let (symbols, identifiers) = extract_all(code);

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

    for expected in [
        "GraphTraversal",
        "count",
        "seed",
        "total",
        "visibilityUnknown",
        "limit",
    ] {
        assert!(
            var_refs.contains(&expected),
            "expected variable_ref for {expected}; got {var_refs:?}"
        );
    }

    assert!(
        identifiers
            .iter()
            .any(|id| id.name == "reach" && id.kind == IdentifierKind::Call),
        "GraphTraversal.reach() must still yield a call named reach"
    );

    for forbidden in [
        "unusedParam",
        "GhostToken",
        "Sample",
        "evaluate",
        "bar",
        "Widget",
        "Int",
        "iOS",
        "deprecated",
        "message",
        "run",
    ] {
        assert!(
            !var_refs.contains(&forbidden),
            "{forbidden} must NOT be a variable_ref; got {var_refs:?}"
        );
    }
    assert_eq!(access("x", code.find("x = 7").unwrap()), Some("write"));
    assert!(
        !identifiers.iter().any(|id| id.name == "GhostToken"),
        "comment-only GhostToken must not be extracted at all"
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
        .expect("evaluate function extracted");
    let graph_ref = identifiers
        .iter()
        .find(|id| id.name == "GraphTraversal" && id.kind == IdentifierKind::VariableRef)
        .expect("GraphTraversal variable_ref");
    assert_eq!(
        graph_ref.containing_symbol_id.as_deref(),
        Some(evaluate.id.as_str()),
        "receiver variable_ref should be contained in evaluate"
    );
}

#[test]
fn test_swift_variable_access_facts() {
    let code = r#"
struct Holder { var value: Int }
func consume(_ value: Int) {}
func bump(_ value: inout Int) {}
func writes(initial: Int, box: Holder, argument: Int, records: [Int]) {
    let initialized = initial
    var plain = 0
    plain = initial
    var compound = 0
    compound += initial
    box.value = initial
    var items = [0]
    let offset = 0
    let index = 0
    items[offset + index] = initial
    consume(argument)
    _ = argument
    bump(&argument)
    for captured in records {
        consume(captured)
    }
}
"#;
    let (symbols, identifiers) = extract_all(code);
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

    let initialized_target = code.find("let initialized = initial").unwrap() + "let ".len();
    let plain_declaration = code.find("var plain = 0").unwrap() + "var ".len();
    let plain = code.find("plain = initial").unwrap();
    let compound = code.find("compound += initial").unwrap();
    let member = code.find("box.value = initial").unwrap() + "box.".len();
    let complex_target = code.find("items[offset + index]").unwrap();
    let offset = complex_target + "items[".len();
    let complex_index = complex_target + "items[offset + ".len();
    let argument = code.find("consume(argument)").unwrap() + "consume(".len();
    let discard = code.find("_ = argument").unwrap();
    let discard_value = discard + "_ = ".len();
    let inout = code.find("&argument").unwrap() + 1;
    let binding = code.find("captured in records").unwrap();
    assert!(!has_variable_ref_at(initialized_target));
    assert!(!has_variable_ref_at(plain_declaration));
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
        access("offset", IdentifierKind::VariableRef, offset),
        Some("read")
    );
    assert_eq!(
        access("index", IdentifierKind::VariableRef, complex_index),
        Some("read")
    );
    assert_eq!(
        access("argument", IdentifierKind::VariableRef, argument),
        Some("read")
    );
    assert!(!has_variable_ref_at(discard));
    assert_eq!(
        access("argument", IdentifierKind::VariableRef, discard_value),
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
    assert_eq!(
        access("argument", IdentifierKind::VariableRef, inout),
        Some("read_write")
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
