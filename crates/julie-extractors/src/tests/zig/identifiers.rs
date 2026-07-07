// Zig variable_ref identifier extraction tests.
//
// Locked variable_ref contract (see csharp/identifiers.rs): receivers + bare
// value reads, the complement of the Call/MemberAccess/TypeUsage arms.

use crate::base::{Identifier, IdentifierKind, Symbol};
use crate::tests::helpers::init_parser;
use crate::zig::ZigExtractor;
use std::path::PathBuf;

fn extract_all(code: &str) -> (Vec<Symbol>, Vec<Identifier>) {
    let tree = init_parser(code, "zig");
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = ZigExtractor::new(
        "zig".to_string(),
        "test.zig".to_string(),
        code.to_string(),
        &workspace_root,
    );
    let symbols = extractor.extract_symbols(&tree);
    let identifiers = extractor.extract_identifiers(&tree, &symbols);
    (symbols, identifiers)
}

#[test]
fn test_zig_variable_ref_emission() {
    let code = r#"
const VISIBILITY_UNKNOWN: i32 = 3;

const Sample = struct {
    bar: i32,

    fn evaluate(self: *Sample, seed: i32, unused_param: i32) i32 {
        var local: i32 = seed; // declaration name, seed read
        var dead_local: i32 = 1; // declaration name, never read
        local = 7; // plain write LHS -> NOT a read
        local += seed; // compound assignment -> read local
        self.bar = local; // self receiver -> read; bar owned by MemberAccess
        const w = Sample{ .bar = seed }; // Sample struct-literal type -> read
        const got = reach(); // reach owned by the Call arm
        return local + got + w.bar + VISIBILITY_UNKNOWN;
    }
};

fn reach() i32 {
    return 0;
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
        "seed",               // initializer / compound-assignment value reads
        "local",              // compound-assignment target + RHS read
        "self",               // member-write receiver `self.bar = ...`
        "Sample",             // struct-initializer type read `Sample{ ... }`
        "w",                  // field-access receiver `w.bar`
        "got",                // bare return read
        "VISIBILITY_UNKNOWN", // bare return read
    ] {
        assert!(
            var_refs.contains(&expected),
            "expected Zig variable_ref for {expected}; got {var_refs:?}"
        );
    }

    // --- Negative cases (rules 2/3/4/5) ---
    for forbidden in [
        "dead_local",   // declaration name only
        "unused_param", // parameter name only
        "evaluate",     // function declaration name
        "reach",        // call callee, owned by the Call arm
        "bar",          // accessed/initialized member, owned by MemberAccess arm
        "i32",          // builtin type
        "GhostToken",   // comment-only mention
    ] {
        assert!(
            !var_refs.contains(&forbidden),
            "{forbidden} must NOT be a Zig variable_ref; got {var_refs:?}"
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
