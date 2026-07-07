use crate::base::{Identifier, IdentifierKind, Symbol};
use crate::go::GoExtractor;
use crate::tests::helpers::init_parser;
use std::path::PathBuf;

fn extract_all(code: &str) -> (Vec<Symbol>, Vec<Identifier>) {
    let tree = init_parser(code, "go");
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = GoExtractor::new(
        "go".to_string(),
        "test.go".to_string(),
        code.to_string(),
        &workspace_root,
    );
    let symbols = extractor.extract_symbols(&tree);
    let identifiers = extractor.extract_identifiers(&tree, &symbols);
    (symbols, identifiers)
}

#[test]
fn test_go_type_usage_identifiers_cover_fields_params_returns_and_generics() {
    let code = r#"
package main

type User struct {}
type Request struct {}
type Response struct {}
type Store[T any] struct {}

type Controller struct {
    store *Store[User]
    users []User
    byID map[string]pkg.External
}

func (c *Controller) Load(req Request, users []User) (*Response, error) {
    var current User
    _ = current
    return &Response{}, nil
}
"#;

    let (_symbols, identifiers) = extract_all(code);
    let type_names: Vec<&str> = identifiers
        .iter()
        .filter(|id| id.kind == IdentifierKind::TypeUsage)
        .map(|id| id.name.as_str())
        .collect();

    for expected in [
        "Controller",
        "Store",
        "User",
        "Request",
        "Response",
        "External",
    ] {
        assert!(
            type_names.contains(&expected),
            "missing Go type usage {expected}; got {type_names:?}"
        );
    }

    assert!(
        !type_names.contains(&"string"),
        "builtin Go type string should not be a TypeUsage: {type_names:?}"
    );
}

#[test]
fn test_go_malformed_struct_recovery_does_not_emit_function_names_as_type_usage() {
    let code = r#"
package main

type Empty struct{}

type EmbeddedStruct struct {
    Empty
}

type MissingBrace struct {
    field int

func VariadicFunction(format string, args ...interface{}) {
    fmt.Printf(format, args...)
}
"#;

    let (_symbols, identifiers) = extract_all(code);
    let type_names: Vec<&str> = identifiers
        .iter()
        .filter(|id| id.kind == IdentifierKind::TypeUsage)
        .map(|id| id.name.as_str())
        .collect();

    assert!(
        type_names.contains(&"Empty"),
        "valid embedded fields should still be TypeUsage identifiers: {type_names:?}"
    );
    for unexpected in ["VariadicFunction", "format", "Printf"] {
        assert!(
            !type_names.contains(&unexpected),
            "malformed function text should not become a TypeUsage {unexpected}; got {type_names:?}"
        );
    }
}

#[test]
fn test_go_variable_ref_emission() {
    // Locked variable_ref contract (see csharp/identifiers.rs): receivers +
    // bare value reads, the complement of the Call/MemberAccess/TypeUsage arms.
    let code = r#"
package main

type Config struct {
	Limit int
}

var visibilityUnknown = 3

func reach() int { return 0 }

func evaluate(seed int, unusedParam int) int {
	local := seed              // declaration LHS, seed read
	deadLocal := 1             // declaration LHS, never read (parse-only fixture)
	local = 7                  // plain write LHS -> NOT a read
	local += seed              // compound assignment -> read local
	cfg := Config{Limit: seed} // composite-literal key Limit -> read
	cfg.Limit = local          // cfg selector receiver -> read
	got := reach()             // reach owned by the Call arm
	total := got + visibilityUnknown + cfg.Limit
	return total
}

// GhostToken appears only in this comment and must never be an identifier.
"#;

    let (_symbols, identifiers) = extract_all(code);
    let var_refs: Vec<&str> = identifiers
        .iter()
        .filter(|id| id.kind == IdentifierKind::VariableRef)
        .map(|id| id.name.as_str())
        .collect();

    // --- Positive cases (rules 1/4) ---
    for expected in [
        "seed",              // RHS / compound-assignment value reads
        "local",             // compound-assignment target + RHS read
        "Limit",             // composite-literal key `Config{Limit: seed}`
        "cfg",               // selector receiver `cfg.Limit`
        "got",               // binary-expression read
        "visibilityUnknown", // bare value read
        "total",             // bare return read
    ] {
        assert!(
            var_refs.contains(&expected),
            "expected Go variable_ref for {expected}; got {var_refs:?}"
        );
    }

    // --- Negative cases (rules 2/3/4/5) ---
    for forbidden in [
        "deadLocal",   // := declaration LHS only
        "unusedParam", // parameter name only
        "evaluate",    // function declaration name
        "reach",       // call callee, owned by the Call arm
        "Config",      // type usage, owned by the TypeUsage arm
        "int",         // builtin
        "GhostToken",  // comment-only mention
        "_",           // blank identifier is never a read
    ] {
        assert!(
            !var_refs.contains(&forbidden),
            "{forbidden} must NOT be a Go variable_ref; got {var_refs:?}"
        );
    }

    // Receiver + call coexist: reach() must still yield a Call identifier.
    assert!(
        identifiers
            .iter()
            .any(|id| id.name == "reach" && id.kind == IdentifierKind::Call),
        "reach() must still yield a Call identifier"
    );

    // GhostToken must not appear as ANY identifier kind.
    assert!(
        !identifiers.iter().any(|id| id.name == "GhostToken"),
        "comment-only GhostToken must not be extracted at all"
    );
}
