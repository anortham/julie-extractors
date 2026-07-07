use crate::base::{Identifier, IdentifierKind, Symbol};
use crate::python::PythonExtractor;
use std::path::PathBuf;

fn extract_all(code: &str) -> (Vec<Symbol>, Vec<Identifier>, PythonExtractor) {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(code, None).unwrap();
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor =
        PythonExtractor::new("test.py".to_string(), code.to_string(), &workspace_root);
    let symbols = extractor.extract_symbols(&tree);
    let identifiers = extractor.extract_identifiers(&tree, &symbols);
    (symbols, identifiers, extractor)
}

#[test]
fn test_python_type_usage_identifiers_cover_annotations() {
    let code = r#"
from typing import List, Optional

class UserService: pass
class LoginRequest: pass
class AuthResult: pass
class User: pass

service: UserService
users: list[User]

def login(request: LoginRequest, fallback: Optional[User]) -> AuthResult:
    return AuthResult()
"#;

    let (_symbols, identifiers, _extractor) = extract_all(code);
    let type_names: Vec<&str> = identifiers
        .iter()
        .filter(|id| id.kind == IdentifierKind::TypeUsage)
        .map(|id| id.name.as_str())
        .collect();

    for expected in [
        "UserService",
        "User",
        "LoginRequest",
        "Optional",
        "AuthResult",
    ] {
        assert!(
            type_names.contains(&expected),
            "missing Python type usage {expected}; got {type_names:?}"
        );
    }
}

#[test]
fn test_python_return_type_hint_uses_annotation_node() {
    let code = r#"
class AuthResult: pass
class User: pass

def login() -> list[AuthResult | User]:
    return []
"#;

    let (symbols, _identifiers, extractor) = extract_all(code);
    let types = extractor.infer_types(&symbols);
    let login = symbols
        .iter()
        .find(|symbol| symbol.name == "login")
        .expect("login function should be extracted");

    assert_eq!(
        types.get(&login.id).map(String::as_str),
        Some("list[AuthResult | User]")
    );
}

#[test]
fn test_python_variable_ref_emission() {
    // Locked variable_ref contract (see csharp/identifiers.rs): receivers + bare
    // value reads, the complement of the Call/MemberAccess/TypeUsage arms.
    let code = r#"
GHOST = 3

class Sample:
    def evaluate(self, seed, unused_param):
        count = 0
        count += 1                              # compound assignment -> read count
        x = 5
        x = 7                                   # plain write LHS -> NOT a read
        total = seed                            # seed on RHS -> read
        g = GraphTraversal.reach()              # receiver -> read; reach -> call
        f = configure(mode=5, source=seed)      # kwarg NAME mode skipped; seed read
        h = filter_items(is_user_type)          # bare function-ref argument -> read
        # ghost_token appears only in this comment and must never be extracted
        return total if total > 0 else visibility_unknown
"#;

    let (symbols, identifiers, _extractor) = extract_all(code);

    let var_refs: Vec<&str> = identifiers
        .iter()
        .filter(|id| id.kind == IdentifierKind::VariableRef)
        .map(|id| id.name.as_str())
        .collect();

    // Positive cases (rules 1/4)
    for expected in [
        "count",              // compound-assignment target
        "seed",               // RHS + keyword-argument VALUE read
        "GraphTraversal",     // attribute receiver
        "is_user_type",       // bare function-ref argument
        "total",              // condition + return reads
        "visibility_unknown", // bare return read
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

    // Negative cases (rules 2/3/4/5)
    for forbidden in [
        "x",            // plain-write LHS only
        "unused_param", // parameter name only
        "mode",         // keyword-argument NAME (parameter ref, per plan)
        "ghost_token",  // comment-only mention
        "Sample",       // class declaration name
        "evaluate",     // method declaration name
        "GHOST",        // module-level plain-assignment LHS (declaration form)
        "self",         // receiver convention, filtered
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
fn test_python_variable_ref_excludes_match_pattern_bindings() {
    // Fix round 1 (adversarial review): structural pattern-matching BINDINGS
    // must not emit variable_ref reads. Per the language spec, a bare lowercase
    // name in a case pattern is always a capture (binding); the grammar wraps
    // those in dotted_name/splat_pattern/as_pattern/keyword_pattern nodes under
    // case_pattern. Guards and case bodies are ordinary value slots (reads).
    let code = r#"
def route(v, default):
    match v:
        case [*items]:                          # splat BINDING, no ref
            return items                        # genuine read
        case _ as handler:                      # as-pattern BINDING, no ref
            return handler()                    # call read (Call arm)
        case {"k": payload, **rest}:            # captures/splat BINDING, no ref
            return payload                      # genuine read
        case Point(x=px, y=py):                 # attr name + capture BINDINGS
            return px + py                      # genuine reads
        case Color.RED:                         # value reference (dotted)
            return v
        case captured if captured > default:    # capture BINDING; guard reads
            return captured                     # genuine read
        case other:                             # capture BINDING, no ref
            return other                        # genuine read
"#;

    let (_symbols, identifiers, _extractor) = extract_all(code);

    let var_refs: Vec<(&str, u32)> = identifiers
        .iter()
        .filter(|id| id.kind == IdentifierKind::VariableRef)
        .map(|id| (id.name.as_str(), id.start_line))
        .collect();
    let names: Vec<&str> = var_refs.iter().map(|(n, _)| *n).collect();

    // Pattern BINDING positions must not emit at their binding line.
    // (source line numbers: case lines are 4,6,8,10,12,14,16)
    for (binding, case_line) in [
        ("items", 4),     // [*items] splat binding
        ("handler", 6),   // _ as handler binding
        ("payload", 8),   // dict-value capture binding
        ("rest", 8),      // **rest splat binding
        ("x", 10),        // class-pattern attribute NAME
        ("px", 10),       // class-pattern capture binding
        ("py", 10),       // class-pattern capture binding
        ("captured", 14), // bare capture binding (guard read is line 14 too — see below)
        ("other", 16),    // bare capture binding
    ] {
        let at_binding = var_refs
            .iter()
            .filter(|(n, l)| *n == binding && *l == case_line)
            .count();
        // `captured` legitimately reads once on line 14 inside the guard.
        let allowed = if binding == "captured" { 1 } else { 0 };
        assert!(
            at_binding <= allowed,
            "{binding} bound on line {case_line} leaked {at_binding} variable_ref row(s) \
             (allowed {allowed}); got {var_refs:?}"
        );
    }

    // Case-body and guard reads must still emit.
    for read in [
        "items", "payload", "px", "py", "captured", "default", "other",
    ] {
        assert!(
            names.contains(&read),
            "expected case-body/guard variable_ref for {read}; got {var_refs:?}"
        );
    }
    // handler() in the body is a Call, not a variable_ref.
    assert!(
        identifiers
            .iter()
            .any(|id| id.name == "handler" && id.kind == IdentifierKind::Call),
        "handler() body call must still emit a Call identifier"
    );
    // `captured` reads exactly twice (guard + body), never at the binding slot.
    assert_eq!(
        names.iter().filter(|n| **n == "captured").count(),
        2,
        "captured must read in guard + body only; got {var_refs:?}"
    );
    // `items` reads exactly once (body), proving the splat binding is silent.
    assert_eq!(
        names.iter().filter(|n| **n == "items").count(),
        1,
        "items must read in the body only; got {var_refs:?}"
    );

    // Documented capture-semantics boundary: dotted VALUE references in case
    // patterns (`case Color.RED:`) are grammar-wrapped in dotted_name, which
    // stays excluded (shared with import machinery) — they do not emit today.
    // A liveness MISS is the safe direction; a binding leak is not.
    assert!(
        !names.contains(&"Color") && !names.contains(&"RED"),
        "dotted case-pattern value refs stay non-emitting (documented boundary)"
    );
}
