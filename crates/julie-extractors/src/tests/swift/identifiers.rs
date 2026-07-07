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
    // Locked variable_ref contract (see csharp/identifiers.rs doc comment):
    // receivers + bare value reads, the complement of the Call/MemberAccess/
    // TypeUsage arms.
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
        count += 1                          // compound assignment -> read count
        var x = 5                           // declaration name, no ref
        x = 7                               // plain write LHS -> NOT a read
        let total = seed                    // initializer -> read seed
        let g = GraphTraversal.reach()      // receiver -> read; reach -> call
        let f = Widget(bar: seed)           // arg label names a parameter -> NOT a read
        let s = "brace \(limit + 1)"        // interpolation expression -> read limit
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

    // --- Positive cases (rules 1/4) ---
    for expected in [
        "GraphTraversal",    // static-access receiver
        "count",             // compound-assignment target
        "seed",              // initializer + argument VALUE read
        "total",             // ternary condition + consequence
        "visibilityUnknown", // bare read in ternary alternative
        "limit",             // string interpolation expression read
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
        "evaluate",    // function declaration name
        "bar",         // value-argument label (names a parameter)
        "Widget",      // call callee (owned by the Call arm)
        "Int",         // builtin type
        "iOS",         // attribute meta-argument
        "deprecated",  // attribute meta-argument
        "message",     // attribute meta-argument label
        "run",         // protocol requirement declaration name
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
