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

#[test]
fn test_go_variable_access_facts() {
    let code = r#"
package main
type Holder struct { value int; count int }
func consume(int) {}
func writes(initial int, holder Holder, argument int, records []int) {
    initialized := initial
    plain := 0
    plain = initial
    compound := 0
    compound += initial
    holder.value = initial
    items := []int{0}
    offset := 0
    index := 0
    items[offset + index] = initial
    counter := 0
    counter++
    consume(argument)
    for captured := range records {
        consume(captured)
    }
}
func update(ptr *int, other *int, box *Holder) {
    ptr = other
    *ptr = 1
    *ptr += 1
    (*box).count = 2
    box.count++
    box.count--
    items := []int{0}
    index := 0
    items[index]++
    items[index]--
}
"#;
    let tree = init_parser(code, "go");
    assert!(!tree.root_node().has_error());
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

    let initialized_target = code.find("initialized := initial").unwrap();
    let plain_declaration = code.find("plain := 0").unwrap();
    let plain = code.find("plain = initial").unwrap();
    let compound = code.find("compound += initial").unwrap();
    let member = code.find("holder.value = initial").unwrap() + "holder.".len();
    let complex_target = code.find("items[offset + index]").unwrap();
    let offset = complex_target + "items[".len();
    let complex_index = complex_target + "items[offset + ".len();
    let increment = code.find("counter++").unwrap();
    let pointer_parameter = code.find("func update(ptr").unwrap() + "func update(".len();
    let indexed_items_declaration = code.rfind("items := []int{0}").unwrap();
    let indexed_index_declaration = code.rfind("index := 0").unwrap();
    let pointer_rebind = code.find("ptr = other").unwrap();
    let pointer_write = code.find("*ptr = 1").unwrap() + "*".len();
    let pointer_compound = code.find("*ptr += 1").unwrap() + "*".len();
    let nested_member = code.find("(*box).count = 2").unwrap() + "(*box).".len();
    let nested_receiver = code.find("box).count = 2").unwrap();
    let member_increment = code.find("box.count++").unwrap() + "box.".len();
    let member_decrement = code.find("box.count--").unwrap() + "box.".len();
    let indexed_increment = code.find("items[index]++").unwrap();
    let indexed_decrement = code.find("items[index]--").unwrap();
    assert!(!has_variable_ref_at(initialized_target));
    assert!(!has_variable_ref_at(plain_declaration));
    let argument = code.find("consume(argument)").unwrap() + "consume(".len();
    let binding = code.find("captured := range").unwrap();
    assert_eq!(
        access(
            "initial",
            IdentifierKind::VariableRef,
            code.find("initialized := initial").unwrap() + "initialized := ".len()
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
        access("counter", IdentifierKind::VariableRef, increment),
        Some("write")
    );
    assert!(!has_variable_ref_at(pointer_parameter));
    assert!(!has_variable_ref_at(indexed_items_declaration));
    assert!(!has_variable_ref_at(indexed_index_declaration));
    assert_eq!(
        access("ptr", IdentifierKind::VariableRef, pointer_rebind),
        Some("write")
    );
    assert_eq!(
        access("ptr", IdentifierKind::VariableRef, pointer_write),
        Some("read")
    );
    assert_eq!(
        access("ptr", IdentifierKind::VariableRef, pointer_compound),
        Some("read")
    );
    assert_eq!(
        access("count", IdentifierKind::MemberAccess, nested_member),
        Some("write")
    );
    assert_eq!(
        access("box", IdentifierKind::VariableRef, nested_receiver),
        Some("read")
    );
    assert_eq!(
        access("count", IdentifierKind::MemberAccess, member_increment),
        Some("write")
    );
    assert_eq!(
        access("count", IdentifierKind::MemberAccess, member_decrement),
        Some("write")
    );
    for start in [nested_member, member_increment, member_decrement] {
        assert_eq!(
            identifiers
                .iter()
                .filter(|identifier| identifier.start_byte == start as u32)
                .count(),
            1
        );
    }
    for start in [member_increment, member_decrement] {
        assert_eq!(
            access("box", IdentifierKind::VariableRef, start - "box.".len()),
            Some("read")
        );
    }
    assert_eq!(
        access("items", IdentifierKind::VariableRef, indexed_increment),
        Some("read")
    );
    assert_eq!(
        access(
            "index",
            IdentifierKind::VariableRef,
            indexed_increment + "items[".len()
        ),
        Some("read")
    );
    assert_eq!(
        access("items", IdentifierKind::VariableRef, indexed_decrement),
        Some("read")
    );
    assert_eq!(
        access(
            "index",
            IdentifierKind::VariableRef,
            indexed_decrement + "items[".len()
        ),
        Some("read")
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
