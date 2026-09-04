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
    // Locked variable_ref contract (see csharp/identifiers.rs): receivers +
    // bare value reads, the complement of the Call/MemberAccess/TypeUsage arms.
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
    let mut local = seed;              // pattern binding, seed read
    let dead_local = 1;                // pattern binding, never read
    local = 7;                         // plain write LHS -> NOT a read
    local += seed;                     // compound assignment -> read local
    let s = Sample { bar: seed };      // field-initializer member bar -> read
    let size = seed;
    let shorthand = Sample { bar: size }; // size value read
    let b = s.bar + Sample::default_bar() + reach(); // s receiver + Sample scope path -> reads
    let macro_only = seed;
    println!("{} {}", local, macro_only); // macro-invocation args -> reads
    match local {
        0 => seed,
        _other => 0,                   // pattern binding, never a read
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

    // --- Positive cases (rules 1/4) ---
    for expected in [
        "seed",               // RHS / argument / match-arm value reads
        "local",              // compound-assignment target + macro arg read
        "bar",                // struct-literal field initializer `Sample { bar: seed }`
        "size",               // struct-literal field value read
        "s",                  // field-access receiver `s.bar`
        "Sample",             // scoped-access path receiver (`X` in `X::Y()`)
        "b",                  // bare return read
        "VISIBILITY_UNKNOWN", // bare value read
        "shorthand",          // field-access receiver in return expression
        "macro_only",         // read only inside a macro-invocation token tree
    ] {
        assert!(
            var_refs.contains(&expected),
            "expected Rust variable_ref for {expected}; got {var_refs:?}"
        );
    }

    // --- Negative cases (rules 2/3/4/5) ---
    for forbidden in [
        "dead_local",   // let-pattern binding only
        "unused_param", // parameter name only
        "evaluate",     // function declaration name
        "reach",        // call callee, owned by the Call arm
        "default_bar",  // scoped call callee, owned by the Call arm
        "_other",       // match-pattern binding
        "Debug",        // attribute/token-tree meta position
        "Tagged",       // struct declaration name
        "GhostToken",   // comment-only mention
    ] {
        assert!(
            !var_refs.contains(&forbidden),
            "{forbidden} must NOT be a Rust variable_ref; got {var_refs:?}"
        );
    }

    // Receiver + call coexist: Sample::default_bar() must still yield a Call.
    assert!(
        identifiers
            .iter()
            .any(|id| id.name == "default_bar" && id.kind == IdentifierKind::Call),
        "Sample::default_bar() must still yield a Call identifier"
    );

    // GhostToken must not appear as ANY identifier kind.
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
