//! Identifier Extraction Tests for JavaScript
//!
//! Tests for extracting identifiers (function calls, member access, chained access)
//! from JavaScript code. Validates that identifier extraction correctly:
//! - Finds function calls
//! - Finds member access patterns
//! - Handles chained member access
//! - Tracks containing symbols
//! - Avoids duplicate identifiers at same location

use crate::base::IdentifierKind;
use crate::javascript::JavaScriptExtractor;
use std::path::PathBuf;
use tree_sitter::Parser;

fn init_parser() -> Parser {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .expect("Error loading JavaScript grammar");
    parser
}

#[test]
fn this_receiver_call_records_enclosing_class_as_receiver_type() {
    let js_code = r#"
class OrderService {
    process() {
        this.persist();
        log();
    }
}
"#;
    let mut parser = init_parser();
    let tree = parser.parse(js_code, None).unwrap();

    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = JavaScriptExtractor::new(
        "javascript".to_string(),
        "orderService.js".to_string(),
        js_code.to_string(),
        &workspace_root,
    );
    let symbols = extractor.extract_symbols(&tree);
    let identifiers = extractor.extract_identifiers(&tree, &symbols);

    let persist = identifiers
        .iter()
        .find(|id| id.name == "persist" && id.kind == IdentifierKind::Call)
        .expect("missing call identifier persist");
    assert_eq!(persist.receiver_type.as_deref(), Some("OrderService"));

    let log = identifiers
        .iter()
        .find(|id| id.name == "log" && id.kind == IdentifierKind::Call)
        .expect("missing call identifier log");
    assert_eq!(log.receiver_type, None);
}

#[test]
fn test_extract_function_calls() {
    let js_code = r#"
function add(a, b) {
    return a + b;
}

function calculate() {
    const result = add(5, 3);      // Function call to add
    console.log(result);            // Function call to log
    return result;
}
"#;

    let mut parser = init_parser();
    let tree = parser.parse(js_code, None).unwrap();

    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = JavaScriptExtractor::new(
        "javascript".to_string(),
        "test.js".to_string(),
        js_code.to_string(),
        &workspace_root,
    );

    // Extract symbols first
    let symbols = extractor.extract_symbols(&tree);

    // NOW extract identifiers (this will FAIL until we implement it)
    let identifiers = extractor.extract_identifiers(&tree, &symbols);

    // Verify we found the function calls
    let add_call = identifiers.iter().find(|id| id.name == "add");
    assert!(
        add_call.is_some(),
        "Should extract 'add' function call identifier"
    );
    let add_call = add_call.unwrap();
    assert_eq!(add_call.kind, IdentifierKind::Call);

    let log_call = identifiers.iter().find(|id| id.name == "log");
    assert!(
        log_call.is_some(),
        "Should extract 'log' function call identifier"
    );
    let log_call = log_call.unwrap();
    assert_eq!(log_call.kind, IdentifierKind::Call);

    // Verify containing symbol is set correctly (should be inside calculate function)
    assert!(
        add_call.containing_symbol_id.is_some(),
        "Function call should have containing symbol"
    );

    // Find the calculate function symbol
    let calculate_fn = symbols.iter().find(|s| s.name == "calculate").unwrap();

    // Verify the add call is contained within calculate function
    assert_eq!(
        add_call.containing_symbol_id.as_ref(),
        Some(&calculate_fn.id),
        "add call should be contained within calculate function"
    );
}

#[test]
fn test_javascript_new_expression_emits_constructor_call_identifier() {
    let js_code = r#"
class ServiceClient {}

function build() {
    const client = new ServiceClient();
    const widget = new ui.Widget();
    return { client, widget };
}
"#;

    let mut parser = init_parser();
    let tree = parser.parse(js_code, None).unwrap();

    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = JavaScriptExtractor::new(
        "javascript".to_string(),
        "constructors.js".to_string(),
        js_code.to_string(),
        &workspace_root,
    );

    let symbols = extractor.extract_symbols(&tree);
    let identifiers = extractor.extract_identifiers(&tree, &symbols);

    let build_symbol = symbols
        .iter()
        .find(|symbol| symbol.name == "build")
        .expect("build function should be extracted");

    for expected in ["ServiceClient", "Widget"] {
        let call = identifiers
            .iter()
            .find(|identifier| {
                identifier.name == expected && identifier.kind == IdentifierKind::Call
            })
            .unwrap_or_else(|| {
                panic!(
                    "new expression should emit constructor call identifier {expected}; got {:?}",
                    identifiers
                        .iter()
                        .map(|identifier| (&identifier.name, &identifier.kind))
                        .collect::<Vec<_>>()
                )
            });
        assert_eq!(
            call.containing_symbol_id.as_deref(),
            Some(build_symbol.id.as_str())
        );
    }
}

#[test]
fn test_extract_member_access() {
    let js_code = r#"
class User {
    constructor(name, email) {
        this.name = name;
        this.email = email;
    }

    printInfo() {
        console.log(this.name);   // Member access: this.name
        const email = this.email;  // Member access: this.email
    }
}
"#;

    let mut parser = init_parser();
    let tree = parser.parse(js_code, None).unwrap();

    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = JavaScriptExtractor::new(
        "javascript".to_string(),
        "test.js".to_string(),
        js_code.to_string(),
        &workspace_root,
    );

    let symbols = extractor.extract_symbols(&tree);
    let identifiers = extractor.extract_identifiers(&tree, &symbols);

    // Verify we found member access identifiers
    let name_access = identifiers
        .iter()
        .filter(|id| id.name == "name" && id.kind == IdentifierKind::MemberAccess)
        .count();
    assert!(
        name_access > 0,
        "Should extract 'name' member access identifier"
    );

    let email_access = identifiers
        .iter()
        .filter(|id| id.name == "email" && id.kind == IdentifierKind::MemberAccess)
        .count();
    assert!(
        email_access > 0,
        "Should extract 'email' member access identifier"
    );
}

#[test]
fn test_file_scoped_containing_symbol() {
    // This test ensures we ONLY match symbols from the SAME FILE
    // Critical bug fix from Rust implementation
    let js_code = r#"
function process() {
    helper();              // Call to helper in same file
}

function helper() {
    // Helper function
}
"#;

    let mut parser = init_parser();
    let tree = parser.parse(js_code, None).unwrap();

    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = JavaScriptExtractor::new(
        "javascript".to_string(),
        "test.js".to_string(),
        js_code.to_string(),
        &workspace_root,
    );

    let symbols = extractor.extract_symbols(&tree);
    let identifiers = extractor.extract_identifiers(&tree, &symbols);

    // Find the helper call
    let helper_call = identifiers.iter().find(|id| id.name == "helper");
    assert!(helper_call.is_some());
    let helper_call = helper_call.unwrap();

    // Verify it has a containing symbol (the process function)
    assert!(
        helper_call.containing_symbol_id.is_some(),
        "helper call should have containing symbol from same file"
    );

    // Verify the containing symbol is the process function
    let process_fn = symbols.iter().find(|s| s.name == "process").unwrap();
    assert_eq!(
        helper_call.containing_symbol_id.as_ref(),
        Some(&process_fn.id),
        "helper call should be contained within process function"
    );
}

#[test]
fn test_chained_member_access() {
    let js_code = r#"
class DataService {
    execute() {
        const result = user.account.balance;   // Chained member access
        const name = customer.profile.name;     // Chained member access
    }
}
"#;

    let mut parser = init_parser();
    let tree = parser.parse(js_code, None).unwrap();

    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = JavaScriptExtractor::new(
        "javascript".to_string(),
        "test.js".to_string(),
        js_code.to_string(),
        &workspace_root,
    );

    let symbols = extractor.extract_symbols(&tree);
    let identifiers = extractor.extract_identifiers(&tree, &symbols);

    // Should extract the rightmost identifiers in chains
    let balance_access = identifiers
        .iter()
        .find(|id| id.name == "balance" && id.kind == IdentifierKind::MemberAccess);
    assert!(
        balance_access.is_some(),
        "Should extract 'balance' from chained member access"
    );

    let name_access = identifiers
        .iter()
        .find(|id| id.name == "name" && id.kind == IdentifierKind::MemberAccess);
    assert!(
        name_access.is_some(),
        "Should extract 'name' from chained member access"
    );
}

#[test]
fn test_no_duplicate_identifiers() {
    let js_code = r#"
function run() {
    process();
    process();  // Same call twice
}

function process() {
}
"#;

    let mut parser = init_parser();
    let tree = parser.parse(js_code, None).unwrap();

    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = JavaScriptExtractor::new(
        "javascript".to_string(),
        "test.js".to_string(),
        js_code.to_string(),
        &workspace_root,
    );

    let symbols = extractor.extract_symbols(&tree);
    let identifiers = extractor.extract_identifiers(&tree, &symbols);

    // Should extract BOTH calls (they're at different locations)
    let process_calls: Vec<_> = identifiers
        .iter()
        .filter(|id| id.name == "process" && id.kind == IdentifierKind::Call)
        .collect();

    assert_eq!(
        process_calls.len(),
        2,
        "Should extract both process calls at different locations"
    );

    // Verify they have different line numbers
    assert_ne!(
        process_calls[0].start_line, process_calls[1].start_line,
        "Duplicate calls should have different line numbers"
    );
}

#[test]
fn test_javascript_variable_ref_emission() {
    // Locked variable_ref contract: receivers + bare value reads, the complement
    // of the Call/MemberAccess arms. See csharp/identifiers.rs for the 6 rules.
    let js_code = r#"
// GhostToken appears only in this comment and must never be an identifier.
const graphKit = {
    reach() { return 1; }
};
const fallbackValue = 3;
const otherBinding = 1;
const registry = { limit: 5, ghost: otherBinding };

function evaluate(seed, unusedParam) {
    let count = 0;
    count += 1;                        // compound assignment -> read count
    let x = 5;                         // declaration name, no ref
    x = 7;                             // plain write LHS -> NOT a read
    const total = seed;                // seed on RHS -> read
    const g = graphKit.reach();        // graphKit receiver -> read; reach -> call
    const pack = { seed };             // shorthand property -> read of seed
    const { alpha } = sourceObj;       // alpha declares a binding; sourceObj -> read
    const msg = `total ${label}`;      // template interpolation -> read label
    const u = undefined;               // keyword, distinct node kind -> never a ref
    return total > 0 ? total : fallbackValue; // bare reads
}

const sourceObj = {};
const label = 'x';
"#;

    let mut parser = init_parser();
    let tree = parser.parse(js_code, None).unwrap();
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = JavaScriptExtractor::new(
        "javascript".to_string(),
        "test.js".to_string(),
        js_code.to_string(),
        &workspace_root,
    );
    let symbols = extractor.extract_symbols(&tree);
    let identifiers = extractor.extract_identifiers(&tree, &symbols);

    let var_refs: Vec<&str> = identifiers
        .iter()
        .filter(|id| id.kind == IdentifierKind::VariableRef)
        .map(|id| id.name.as_str())
        .collect();

    // --- Positive cases (rules 1/4) ---
    for expected in [
        "count",         // compound-assignment target
        "seed",          // RHS read + shorthand property {seed}
        "graphKit",      // member-access receiver
        "sourceObj",     // destructuring source object
        "fallbackValue", // bare ternary read
        "total",         // declarator RHS + ternary reads
        "label",         // template-literal interpolation
        "otherBinding",  // object-literal pair value
    ] {
        assert!(
            var_refs.contains(&expected),
            "expected variable_ref for {expected}; got {var_refs:?}"
        );
    }

    // Receiver + call coexist: graphKit.reach()
    assert!(
        identifiers
            .iter()
            .any(|id| id.name == "reach" && id.kind == IdentifierKind::Call),
        "graphKit.reach() must still yield a call named reach"
    );

    // --- Negative cases (rules 2/3/4/5) ---
    for forbidden in [
        "x",           // declaration name + plain-write LHS
        "unusedParam", // parameter name only
        "alpha",       // destructuring pattern declares, not reads
        "limit",       // object-literal key
        "ghost",       // object-literal key
        "GhostToken",  // comment-only mention
        "evaluate",    // function declaration name
        "registry",    // declared, never read
        "undefined",   // keyword (distinct node kind)
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

    // containing_symbol_id is populated on variable_refs inside evaluate.
    let count_ref = identifiers
        .iter()
        .find(|id| id.name == "count" && id.kind == IdentifierKind::VariableRef)
        .expect("count variable_ref");
    assert!(
        count_ref.containing_symbol_id.is_some(),
        "variable_ref must carry containing_symbol_id"
    );
}
