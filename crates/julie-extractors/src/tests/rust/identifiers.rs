// Tests for Rust identifier extraction with scoped/qualified paths
//
// Bug: `crate::module::function()` was indexed as "crate::module::function"
// instead of "function", causing fast_refs to miss the reference.

use crate::base::{Identifier, IdentifierKind, Symbol};
use crate::rust::RustExtractor;
use std::path::PathBuf;
use tree_sitter::Parser;

fn init_test_parser() -> Parser {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .expect("Error loading Rust grammar");
    parser
}

fn extract_all(code: &str) -> (Vec<Symbol>, Vec<Identifier>) {
    let mut parser = init_test_parser();
    let tree = parser.parse(code, None).unwrap();
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = RustExtractor::new(
        "rust".to_string(),
        "test.rs".to_string(),
        code.to_string(),
        &workspace_root,
    );
    let symbols = extractor.extract_symbols(&tree);
    let identifiers = extractor.extract_identifiers(&tree, &symbols);
    (symbols, identifiers)
}

#[test]
fn test_scoped_call_extracts_last_segment() {
    let code = r#"
fn caller() {
    crate::search::hybrid::should_use_semantic_fallback("query", 5);
}
"#;
    let (_symbols, identifiers) = extract_all(code);
    let calls: Vec<&Identifier> = identifiers
        .iter()
        .filter(|id| id.kind == IdentifierKind::Call)
        .collect();

    assert!(
        !calls.is_empty(),
        "Should find at least one call identifier"
    );
    let call = calls
        .iter()
        .find(|id| id.name == "should_use_semantic_fallback");
    assert!(
        call.is_some(),
        "Should find should_use_semantic_fallback call, got: {:?}",
        calls.iter().map(|c| &c.name).collect::<Vec<_>>()
    );
    assert_eq!(
        call.unwrap().name,
        "should_use_semantic_fallback",
        "Should extract bare name, not qualified path"
    );
}

#[test]
fn test_simple_call_still_works() {
    let code = r#"
fn caller() {
    do_something();
}
fn do_something() {}
"#;
    let (_symbols, identifiers) = extract_all(code);
    let calls: Vec<&Identifier> = identifiers
        .iter()
        .filter(|id| id.kind == IdentifierKind::Call)
        .collect();

    assert!(
        calls.iter().any(|c| c.name == "do_something"),
        "Simple calls should still work, got: {:?}",
        calls.iter().map(|c| &c.name).collect::<Vec<_>>()
    );
}

#[test]
fn test_nested_scoped_call() {
    let code = r#"
fn example() {
    std::collections::HashMap::new();
}
"#;
    let (_symbols, identifiers) = extract_all(code);
    let calls: Vec<&Identifier> = identifiers
        .iter()
        .filter(|id| id.kind == IdentifierKind::Call)
        .collect();

    // Should extract "new" as the call name, not the full qualified path
    assert!(
        calls.iter().any(|c| c.name == "new"),
        "Should extract 'new' from HashMap::new(), got: {:?}",
        calls.iter().map(|c| &c.name).collect::<Vec<_>>()
    );
}

#[test]
fn test_enum_variant_scoped_identifiers_are_type_usages() {
    let code = r#"
enum IndexingRepairReason {
    SemanticVersionChanged,
    StaleFiles,
}

impl IndexingRepairReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::SemanticVersionChanged => "semantic_version_changed",
            Self::StaleFiles => "stale_files",
        }
    }
}

fn mark(reasons: &mut Vec<IndexingRepairReason>) {
    reasons.push(IndexingRepairReason::SemanticVersionChanged);
}
"#;
    let (_symbols, identifiers) = extract_all(code);
    let variant_usages: Vec<&Identifier> = identifiers
        .iter()
        .filter(|id| id.name == "SemanticVersionChanged" && id.kind == IdentifierKind::TypeUsage)
        .collect();

    assert_eq!(
        variant_usages.len(),
        2,
        "Should extract enum variant usages from Self::Variant and Enum::Variant, got: {:?}",
        identifiers
            .iter()
            .map(|id| (&id.name, &id.kind, id.start_line))
            .collect::<Vec<_>>()
    );
    assert!(
        variant_usages
            .iter()
            .all(|id| id.containing_symbol_id.is_some()),
        "Enum variant usages should retain containing symbols"
    );
}

#[test]
fn test_rust_type_usage_identifiers_cover_type_identifier_nodes() {
    let code = r#"
struct UserService {
    repository: Repository<User>,
}

struct LoginRequest;
struct AuthResult;
struct Repository<T> { item: T }
struct User;
struct Error;

impl UserService {
    fn login(&self, request: &LoginRequest, users: Vec<User>) -> Result<AuthResult, Error> {
        let fallback: Option<User> = None;
        todo!()
    }
}
"#;
    let (_symbols, identifiers) = extract_all(code);
    let type_names: Vec<&str> = identifiers
        .iter()
        .filter(|id| id.kind == IdentifierKind::TypeUsage)
        .map(|id| id.name.as_str())
        .collect();

    for expected in [
        "Repository",
        "User",
        "LoginRequest",
        "Vec",
        "Result",
        "AuthResult",
        "Error",
        "Option",
    ] {
        assert!(
            type_names.contains(&expected),
            "missing Rust type usage {expected}; got {type_names:?}"
        );
    }
}

#[test]
fn test_rust_variable_ref_emission() {
    let code = r#"
const VISIBILITY_UNKNOWN: i32 = 3;

pub struct Sample {
    bar: i32,
}

impl Sample {
    pub fn default_bar() -> i32 {
        1
    }
}

pub fn reach() -> i32 {
    0
}

#[derive(Debug)]
pub struct Tagged;

pub fn evaluate(seed: i32, unused_param: i32) -> i32 {
    let mut local = seed;
    let dead_local = 1;
    local = 7;
    local += seed;
    let s = Sample { bar: seed };
    let size = seed;
    let shorthand = Sample { bar: size };
    let b = s.bar + Sample::default_bar() + reach();
    let macro_only = seed;
    println!("{} {}", local, macro_only);
    match local {
        0 => seed,
        _other => 0,
    };
    b + local + VISIBILITY_UNKNOWN + shorthand.bar
}

// GhostToken appears only in this comment and must never be an identifier.
"#;

    let (_symbols, identifiers) = extract_all(code);
    let var_refs: Vec<&str> = identifiers
        .iter()
        .filter(|id| id.kind == IdentifierKind::VariableRef)
        .map(|id| id.name.as_str())
        .collect();
    let access = |name: &str| {
        identifiers
            .iter()
            .find(|id| id.name == name && id.kind == IdentifierKind::VariableRef)
            .and_then(|id| id.metadata.as_ref())
            .and_then(|metadata| metadata.get("access"))
            .and_then(serde_json::Value::as_str)
    };

    for expected in [
        "seed",
        "local",
        "bar",
        "size",
        "s",
        "Sample",
        "b",
        "VISIBILITY_UNKNOWN",
        "shorthand",
        "macro_only",
    ] {
        assert!(
            var_refs.contains(&expected),
            "expected Rust variable_ref for {expected}; got {var_refs:?}"
        );
    }

    for forbidden in [
        "dead_local",
        "unused_param",
        "evaluate",
        "reach",
        "default_bar",
        "Debug",
        "Tagged",
        "GhostToken",
    ] {
        assert!(
            !var_refs.contains(&forbidden),
            "{forbidden} must NOT be a Rust variable_ref; got {var_refs:?}"
        );
    }
    assert_eq!(access("_other"), Some("write"));

    assert!(
        identifiers
            .iter()
            .any(|id| id.name == "default_bar" && id.kind == IdentifierKind::Call),
        "Sample::default_bar() must still yield a Call identifier"
    );

    assert!(
        !identifiers.iter().any(|id| id.name == "GhostToken"),
        "comment-only GhostToken must not be extracted at all"
    );
}

fn call_named<'a>(identifiers: &'a [Identifier], name: &str) -> &'a Identifier {
    identifiers
        .iter()
        .find(|id| id.name == name && id.kind == IdentifierKind::Call)
        .unwrap_or_else(|| panic!("missing call identifier {name}"))
}

#[test]
fn self_call_inside_impl_records_enclosing_type_as_receiver_type() {
    let code = r#"
struct Store;

impl Store {
    fn run(&self) {
        self.helper();
    }
}
"#;
    let (_symbols, identifiers) = extract_all(code);
    assert_eq!(
        call_named(&identifiers, "helper").receiver_type.as_deref(),
        Some("Store")
    );
}

#[test]
fn self_call_inside_trait_impl_records_type_target_as_receiver_type() {
    let code = r#"
struct Store;

trait Persist {
    fn persist(&self);
}

impl Persist for Store {
    fn persist(&self) {
        self.helper();
    }
}
"#;
    let (_symbols, identifiers) = extract_all(code);
    assert_eq!(
        call_named(&identifiers, "helper").receiver_type.as_deref(),
        Some("Store")
    );
}

#[test]
fn self_call_inside_trait_default_method_has_no_receiver_type() {
    let code = r#"
trait Persist {
    fn persist(&self) {
        self.helper();
    }
}
"#;
    let (_symbols, identifiers) = extract_all(code);
    assert_eq!(call_named(&identifiers, "helper").receiver_type, None);
}

#[test]
fn other_receiver_call_has_no_receiver_type() {
    let code = r#"
struct Store;

impl Store {
    fn run(&self, other: Store) {
        other.helper();
    }
}
"#;
    let (_symbols, identifiers) = extract_all(code);
    assert_eq!(call_named(&identifiers, "helper").receiver_type, None);
}

#[test]
fn self_type_call_inside_impl_records_enclosing_type_as_receiver_type() {
    let code = r#"
struct Store;

impl Store {
    fn run() {
        Self::helper();
    }
}
"#;
    let (_symbols, identifiers) = extract_all(code);
    assert_eq!(
        call_named(&identifiers, "helper").receiver_type.as_deref(),
        Some("Store")
    );
}

#[test]
fn self_call_inside_nested_impl_records_innermost_type_as_receiver_type() {
    let code = r#"
struct Store;
struct Local;

impl Store {
    fn run(&self) {
        impl Local {
            fn inner(&self) {
                self.helper();
            }
        }
        self.finish();
    }
}
"#;
    let (_symbols, identifiers) = extract_all(code);
    assert_eq!(
        call_named(&identifiers, "helper").receiver_type.as_deref(),
        Some("Local")
    );
    assert_eq!(
        call_named(&identifiers, "finish").receiver_type.as_deref(),
        Some("Store")
    );
}

#[test]
fn test_rust_variable_access_facts() {
    let code = r#"
struct Holder { value: i32 }
fn consume(_: i32) {}
fn mutate(value: &mut i32) { *value += 1; }
fn borrow(_: &i32) {}
fn writes(initial: i32, mut holder: Holder, mut argument: i32, records: Option<i32>) {
    let initialized = initial;
    let mut plain = 0;
    plain = initial;
    let mut compound = 0;
    compound += initial;
    holder.value = initial;
    let mut items = vec![0];
    let index = 0;
    items[index] = initial;
    let offset = 0;
    items[offset + index] = initial;
    consume(argument);
    borrow(&argument);
    mutate(&mut argument);
    match records {
        Some(captured) => consume(captured),
        None => {}
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
    let plain_declaration = code.find("let mut plain = 0").unwrap() + "let mut ".len();
    let plain = code.find("plain = initial").unwrap();
    let compound = code.find("compound += initial").unwrap();
    let member = code.find("holder.value = initial").unwrap() + "holder.".len();
    let receiver = code.find("holder.value = initial").unwrap();
    let indexed_target = code.find("items[index]").unwrap();
    let index = indexed_target + "items[".len();
    let complex_target = code.find("items[offset + index]").unwrap();
    let offset = complex_target + "items[".len();
    let complex_index = complex_target + "items[offset + ".len();
    let argument = code.find("consume(argument)").unwrap() + "consume(".len();
    let by_reference = code.find("&mut argument").unwrap() + "&mut ".len();
    let shared_reference = code.find("&argument").unwrap() + 1;
    let binding = code.find("Some(captured)").unwrap() + "Some(".len();
    assert!(!has_variable_ref_at(initialized_target));
    assert!(!has_variable_ref_at(plain_declaration));
    assert_eq!(
        access(
            "initial",
            IdentifierKind::VariableRef,
            code.find("let initialized = initial").unwrap() + "let initialized = ".len()
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
        access("holder", IdentifierKind::VariableRef, receiver),
        Some("read")
    );
    assert_eq!(
        access("items", IdentifierKind::VariableRef, indexed_target),
        Some("read")
    );
    assert_eq!(
        access("index", IdentifierKind::VariableRef, index),
        Some("read")
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
        access("argument", IdentifierKind::VariableRef, by_reference),
        Some("write")
    );
    assert_eq!(
        access("argument", IdentifierKind::VariableRef, shared_reference),
        Some("read")
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
