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
    let login = symbols
        .iter()
        .find(|symbol| symbol.name == "login")
        .expect("login function should be extracted");

    let fact = &extractor.base.type_info[&login.id];
    assert_eq!(fact.resolved_type, "list");
    assert_eq!(
        fact.metadata.as_ref().unwrap()["declared"],
        "list[AuthResult | User]"
    );
}

#[test]
fn test_python_variable_ref_emission() {
    let code = r#"
GHOST = 3

class Sample:
    def evaluate(self, seed, unused_param):
        count = 0
        count += 1
        x = 5
        x = 7
        total = seed
        g = GraphTraversal.reach()
        f = configure(mode=5, source=seed)
        h = filter_items(is_user_type)
        # ghost_token appears only in this comment and must never be extracted
        return total if total > 0 else visibility_unknown
"#;

    let (symbols, identifiers, _extractor) = extract_all(code);

    let var_refs: Vec<&str> = identifiers
        .iter()
        .filter(|id| id.kind == IdentifierKind::VariableRef)
        .map(|id| id.name.as_str())
        .collect();
    let access = |name: &str, kind: IdentifierKind, start: usize| {
        identifiers
            .iter()
            .find(|id| id.name == name && id.kind == kind && id.start_byte == start as u32)
            .and_then(|id| id.metadata.as_ref())
            .and_then(|metadata| metadata.get("access"))
            .and_then(serde_json::Value::as_str)
    };

    for expected in [
        "count",
        "seed",
        "GraphTraversal",
        "is_user_type",
        "total",
        "visibility_unknown",
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
        "unused_param",
        "mode",
        "ghost_token",
        "Sample",
        "evaluate",
        "GHOST",
        "self",
    ] {
        assert!(
            !var_refs.contains(&forbidden),
            "{forbidden} must NOT be a variable_ref; got {var_refs:?}"
        );
    }
    assert_eq!(
        access(
            "x",
            IdentifierKind::VariableRef,
            code.find("x = 7").unwrap()
        ),
        Some("write")
    );
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
fn test_python_match_pattern_bindings_are_writes_and_values_are_reads() {
    let code = r#"
def route(v, default):
    match v:
        case [*items]:
            return items
        case _ as handler:
            return handler()
        case {"k": payload, **rest}:
            return payload
        case Point(x=px, y=py):
            return px + py
        case Color.RED:
            return v
        case captured if captured > default:
            return captured
        case other:
            return other
"#;

    let (symbols, identifiers, _extractor) = extract_all(code);
    let variable_ref_at = |name: &str, start: usize| {
        identifiers.iter().find(|id| {
            id.name == name
                && id.kind == IdentifierKind::VariableRef
                && id.start_byte == start as u32
        })
    };
    let access_at = |name: &str, start: usize| {
        variable_ref_at(name, start)
            .and_then(|id| id.metadata.as_ref())
            .and_then(|metadata| metadata.get("access"))
            .and_then(serde_json::Value::as_str)
    };
    let symbol_covers = |name: &str, start: usize| {
        symbols.iter().any(|symbol| {
            symbol.name == name
                && symbol.start_byte <= start as u32
                && symbol.end_byte >= (start + name.len()) as u32
        })
    };
    let bindings = [
        (
            "items",
            code.find("case [*items]").unwrap() + "case [*".len(),
        ),
        (
            "handler",
            code.find("case _ as handler").unwrap() + "case _ as ".len(),
        ),
        (
            "payload",
            code.find("\"k\": payload").unwrap() + "\"k\": ".len(),
        ),
        ("rest", code.find("**rest").unwrap() + "**".len()),
        ("px", code.find("x=px").unwrap() + "x=".len()),
        ("py", code.find("y=py").unwrap() + "y=".len()),
        (
            "captured",
            code.find("case captured if").unwrap() + "case ".len(),
        ),
        ("other", code.find("case other").unwrap() + "case ".len()),
    ];
    for (name, start) in bindings {
        if symbol_covers(name, start) {
            assert!(variable_ref_at(name, start).is_none());
        } else {
            assert_eq!(access_at(name, start), Some("write"), "{name} at {start}");
        }
    }
    let attribute_name = code.find("Point(x=px").unwrap() + "Point(".len();
    assert!(variable_ref_at("x", attribute_name).is_none());

    let reads = [
        (
            "items",
            code.find("return items").unwrap() + "return ".len(),
        ),
        (
            "payload",
            code.find("return payload").unwrap() + "return ".len(),
        ),
        ("px", code.find("return px").unwrap() + "return ".len()),
        (
            "py",
            code.find("return px + py").unwrap() + "return px + ".len(),
        ),
        ("captured", code.find("captured > default").unwrap()),
        (
            "default",
            code.find("captured > default").unwrap() + "captured > ".len(),
        ),
        (
            "captured",
            code.find("return captured").unwrap() + "return ".len(),
        ),
        (
            "other",
            code.find("return other").unwrap() + "return ".len(),
        ),
    ];
    for (name, start) in reads {
        assert_eq!(access_at(name, start), Some("read"), "{name} at {start}");
    }
    assert!(
        identifiers
            .iter()
            .any(|id| id.name == "handler" && id.kind == IdentifierKind::Call),
        "handler() body call must still emit a Call identifier"
    );
    let color = code.find("Color.RED").unwrap();
    assert_eq!(access_at("Color", color), Some("read"));
    assert_eq!(
        identifiers
            .iter()
            .find(|id| id.name == "RED" && id.kind == IdentifierKind::MemberAccess)
            .and_then(|id| id.metadata.as_ref())
            .and_then(|metadata| metadata.get("access"))
            .and_then(serde_json::Value::as_str),
        Some("read")
    );
}

#[test]
fn test_python_variable_access_facts() {
    let code = r#"
global_plain = 0
global_plain = 1
def writes(initial, box, argument, records):
    initialized = initial
    plain = 0
    plain = initial
    compound = 0
    compound += initial
    box.value = initial
    consume(argument)
    match records:
        case captured:
            consume(captured)
"#;
    let (symbols, identifiers, _) = extract_all(code);
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
    let binding = code.find("case captured").unwrap() + "case ".len();
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
fn python_annotation_without_value_has_no_target_access() {
    let code = r#"
def annotate(obj):
    x = 0
    x: int
    (x): int
    obj.attr: int
    (obj.attr): int
    only: int
"#;
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(code, None).unwrap();
    assert!(!tree.root_node().has_error());
    let (symbols, identifiers, _) = extract_all(code);
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

    let first_binding = code.find("x = 0").unwrap();
    let reannotation = code.find("x: int").unwrap();
    let attribute = code.find("obj.attr: int").unwrap() + "obj.".len();
    let receiver = code.find("obj.attr: int").unwrap();
    let parenthesized_reannotation = code.find("(x): int").unwrap() + "(".len();
    let parenthesized_attribute = code.find("(obj.attr): int").unwrap() + "(obj.".len();
    let parenthesized_receiver = code.find("(obj.attr): int").unwrap() + "(".len();
    let annotation_only = code.find("only: int").unwrap();

    assert!(!has_variable_ref_at(first_binding));
    assert!(!has_variable_ref_at(reannotation));
    assert!(!has_variable_ref_at(parenthesized_reannotation));
    assert_eq!(
        identifiers
            .iter()
            .filter(|identifier| {
                identifier.kind == IdentifierKind::MemberAccess
                    && identifier.start_byte == attribute as u32
            })
            .count(),
        1
    );
    assert_eq!(
        access("attr", IdentifierKind::MemberAccess, attribute),
        None
    );
    assert_eq!(
        identifiers
            .iter()
            .filter(|identifier| {
                identifier.kind == IdentifierKind::MemberAccess
                    && identifier.start_byte == parenthesized_attribute as u32
            })
            .count(),
        1
    );
    assert_eq!(
        access(
            "attr",
            IdentifierKind::MemberAccess,
            parenthesized_attribute
        ),
        None
    );
    assert_eq!(
        access("obj", IdentifierKind::VariableRef, parenthesized_receiver),
        Some("read")
    );
    assert_eq!(
        access("obj", IdentifierKind::VariableRef, receiver),
        Some("read")
    );
    assert!(!has_variable_ref_at(annotation_only));
    assert!(symbols.iter().any(|symbol| symbol.name == "only"));
}
