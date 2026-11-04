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
        const w = Sample{ .bar = seed }; // Sample struct-literal type -> type_usage
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

    assert!(
        identifiers.iter().any(|id| id.name == "Sample"
            && id.kind == IdentifierKind::TypeUsage
            && id.start_line == 13),
        "the struct-literal type Sample must be a type_usage"
    );

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

fn variable_access_at(identifiers: &[Identifier], start: usize) -> Option<&str> {
    identifiers
        .iter()
        .find(|identifier| identifier.start_byte as usize == start)?
        .metadata
        .as_ref()?
        .get("access")?
        .as_str()
}

#[test]
fn variable_access_facts_follow_zig_assignment_and_capture_syntax() {
    let code = r#"
const Box = struct { value: i32 };
fn consume(value: i32) void {}
fn update(input: i32, box: *Box, values: []i32) void {
    var output: i32 = input;
    output = input;
    output += 1;
    box.value = input;
    consume(output);
    for (values) |item| {
        consume(item);
    }
}
"#;
    let (_, identifiers) = extract_all(code);

    assert_eq!(
        variable_access_at(&identifiers, code.find("output = input").unwrap()),
        Some("write")
    );
    assert_eq!(
        variable_access_at(
            &identifiers,
            code.find("output = input").unwrap() + "output = ".len()
        ),
        Some("read")
    );
    assert_eq!(
        variable_access_at(&identifiers, code.find("output += 1").unwrap()),
        Some("read_write")
    );
    assert_eq!(
        variable_access_at(
            &identifiers,
            code.find("box.value = input").unwrap() + "box.".len()
        ),
        Some("write")
    );
    assert_eq!(
        variable_access_at(&identifiers, code.find("box.value = input").unwrap()),
        Some("read")
    );
    assert_eq!(
        variable_access_at(
            &identifiers,
            code.find("consume(output)").unwrap() + "consume(".len()
        ),
        Some("read")
    );
    assert_eq!(
        variable_access_at(&identifiers, code.find("|item|").unwrap() + 1),
        Some("write")
    );
    assert_eq!(
        variable_access_at(
            &identifiers,
            code.find("consume(item)").unwrap() + "consume(".len()
        ),
        Some("read")
    );
    let declaration = code.find("output: i32 =").unwrap();
    assert!(
        identifiers
            .iter()
            .all(|identifier| identifier.start_byte as usize != declaration)
    );
}
#[test]
fn shadowed_bindings_keep_exact_access_and_non_access_facts() {
    let code = r#"
const Marker = struct { payload: i32 };
fn consume(value: i32) void {}
fn update(count: i32) void {
    const marker = Marker{ .payload = 1 };
    consume(count);
    if (true) {
        var count: i32 = 0;
        count += 1;
        consume(count);
    }
    consume(count);
}
"#;
    let (symbols, identifiers) = extract_all(code);

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
    let type_start = code.find("Marker{").unwrap();
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
