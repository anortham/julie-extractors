//! Tests for test call expression extraction in TypeScript/JavaScript.
//!
//! Validates that Jest/Vitest/Mocha/Bun/Playwright test DSL call expressions
//! (describe/it/test/beforeEach/etc.) are extracted as named symbols.

use crate::base::{RelationshipKind, Symbol, SymbolKind};
use crate::typescript::TypeScriptExtractor;
use std::path::PathBuf;

fn init_parser() -> tree_sitter::Parser {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .expect("Error loading JavaScript grammar");
    parser
}

fn extractor_at(file_path: &str, code: &str) -> TypeScriptExtractor {
    TypeScriptExtractor::new(
        "typescript".to_string(),
        file_path.to_string(),
        code.to_string(),
        &PathBuf::from("/tmp/test"),
    )
}

fn symbols_at(file_path: &str, code: &str) -> Vec<Symbol> {
    let mut parser = init_parser();
    let tree = parser.parse(code, None).unwrap();
    extractor_at(file_path, code).extract_symbols(&tree)
}

fn role(symbols: &[Symbol], name: &str) -> Option<String> {
    symbols
        .iter()
        .find(|symbol| symbol.name == name)?
        .metadata
        .as_ref()?
        .get("test_role")?
        .as_str()
        .map(str::to_string)
}

#[test]
fn test_extract_test_call_symbols() {
    let code = r#"
describe("UserService", () => {
    beforeEach(() => {
        setupDatabase();
    });

    it("should create a user", () => {
        const user = createUser("Alice");
        expect(user.name).toBe("Alice");
    });

    it("should delete a user", () => {
        deleteUser("Alice");
    });
});
"#;

    let mut parser = init_parser();
    let tree = parser.parse(code, None).unwrap();

    let workspace_root = PathBuf::from("/tmp/test");

    let mut extractor = TypeScriptExtractor::new(
        "javascript".to_string(),
        "__tests__/user.test.js".to_string(),
        code.to_string(),
        &workspace_root,
    );

    let symbols = extractor.extract_symbols(&tree);

    // Should extract describe block
    let describe_sym = symbols.iter().find(|s| s.name == "UserService");
    assert!(
        describe_sym.is_some(),
        "Should extract describe block as symbol. Got symbols: {:?}",
        symbols.iter().map(|s| &s.name).collect::<Vec<_>>()
    );
    let describe_sym = describe_sym.unwrap();
    assert_eq!(describe_sym.kind, SymbolKind::Function);

    // describe should NOT have is_test metadata
    let describe_is_test = describe_sym
        .metadata
        .as_ref()
        .and_then(|m| m.get("is_test"))
        .and_then(|v| v.as_bool());
    assert_ne!(
        describe_is_test,
        Some(true),
        "describe block should NOT have is_test = true"
    );

    // describe should have test_container metadata
    let describe_is_container = describe_sym
        .metadata
        .as_ref()
        .and_then(|m| m.get("test_container"))
        .and_then(|v| v.as_bool());
    assert_eq!(
        describe_is_container,
        Some(true),
        "describe block should have test_container = true"
    );

    // Should extract it blocks
    let it_create = symbols.iter().find(|s| s.name == "should create a user");
    assert!(
        it_create.is_some(),
        "Should extract 'it' block with name 'should create a user'"
    );
    let it_create = it_create.unwrap();
    let it_is_test = it_create
        .metadata
        .as_ref()
        .and_then(|m| m.get("is_test"))
        .and_then(|v| v.as_bool());
    assert_eq!(
        it_is_test,
        Some(true),
        "it block should have is_test = true"
    );

    let it_delete = symbols.iter().find(|s| s.name == "should delete a user");
    assert!(it_delete.is_some(), "Should extract second 'it' block");

    // Should extract beforeEach
    let before_each = symbols.iter().find(|s| s.name == "beforeEach");
    assert!(
        before_each.is_some(),
        "Should extract beforeEach lifecycle block"
    );
    let before_each = before_each.unwrap();
    let before_is_test = before_each
        .metadata
        .as_ref()
        .and_then(|m| m.get("is_test"))
        .and_then(|v| v.as_bool());
    assert_eq!(
        before_is_test,
        Some(true),
        "beforeEach should have is_test = true"
    );

    // Total: describe + beforeEach + 2 it blocks = 4 test call symbols
    let test_call_symbols: Vec<_> = symbols
        .iter()
        .filter(|s| {
            s.metadata
                .as_ref()
                .is_some_and(|m| m.contains_key("is_test") || m.contains_key("test_container"))
        })
        .collect();
    assert_eq!(
        test_call_symbols.len(),
        4,
        "Should extract exactly 4 test call symbols (describe + beforeEach + 2 it). Got: {:?}",
        test_call_symbols
            .iter()
            .map(|s| &s.name)
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_relationships_attributed_to_test_symbols() {
    // This test verifies that when production code is called inside an it() callback,
    // the relationship's from_symbol_id points to the it() test symbol, not nothing.
    let code = r#"
function helper() {
    return 42;
}

it("should process", () => {
    helper();
    processPayment();
});
"#;

    let mut parser = init_parser();
    let tree = parser.parse(code, None).unwrap();

    let workspace_root = PathBuf::from("/tmp/test");

    let mut extractor = TypeScriptExtractor::new(
        "javascript".to_string(),
        "__tests__/payment.test.js".to_string(),
        code.to_string(),
        &workspace_root,
    );

    let symbols = extractor.extract_symbols(&tree);
    let relationships = extractor.extract_relationships(&tree, &symbols);
    let pending = extractor.get_pending_relationships();

    // The it() symbol should exist
    let it_sym = symbols.iter().find(|s| s.name == "should process");
    assert!(
        it_sym.is_some(),
        "Should extract it() test symbol. Got: {:?}",
        symbols.iter().map(|s| &s.name).collect::<Vec<_>>()
    );
    let it_sym = it_sym.unwrap();

    // helper() is a local function — should produce a direct Relationship
    let helper_rel = relationships.iter().find(|r| {
        let to_sym = symbols.iter().find(|s| s.id == r.to_symbol_id);
        matches!(to_sym, Some(s) if s.name == "helper")
    });
    assert!(
        helper_rel.is_some(),
        "Should have a direct relationship to helper(). Rels: {:?}",
        relationships
            .iter()
            .map(|r| (&r.from_symbol_id, &r.kind))
            .collect::<Vec<_>>()
    );
    let helper_rel = helper_rel.unwrap();
    assert_eq!(helper_rel.kind, RelationshipKind::Calls);
    assert_eq!(
        helper_rel.from_symbol_id, it_sym.id,
        "helper() call should be attributed to the it() test symbol"
    );

    // processPayment() is unknown/imported — should produce a PendingRelationship
    let process_pending = pending.iter().find(|p| p.callee_name == "processPayment");
    assert!(
        process_pending.is_some(),
        "Should have a pending relationship for processPayment(). Pending: {:?}",
        pending.iter().map(|p| &p.callee_name).collect::<Vec<_>>()
    );
    let process_pending = process_pending.unwrap();
    assert_eq!(
        process_pending.from_symbol_id, it_sym.id,
        "processPayment() pending call should be attributed to the it() test symbol"
    );
}

const PLAYWRIGHT_SPEC: &str = r#"
import { test } from "@playwright/test";

test.describe("checkout", () => {
    test.beforeEach(async () => {});
    test.afterAll(async () => {});
    test("pays with a card", async () => {});
});
"#;

#[test]
fn playwright_dotted_calls_extract_container_and_lifecycle_symbols() {
    let symbols = symbols_at("tests/checkout.spec.ts", PLAYWRIGHT_SPEC);

    assert_eq!(
        role(&symbols, "checkout").as_deref(),
        Some("test_container"),
        "test.describe declares a container, got: {:?}",
        symbols.iter().map(|s| &s.name).collect::<Vec<_>>()
    );
    assert_eq!(
        role(&symbols, "beforeEach").as_deref(),
        Some("fixture_setup")
    );
    assert_eq!(
        role(&symbols, "afterAll").as_deref(),
        Some("fixture_teardown")
    );
    assert_eq!(
        role(&symbols, "pays with a card").as_deref(),
        Some("test_case")
    );
}

#[test]
fn dotted_lifecycle_symbols_nest_under_their_container() {
    let symbols = symbols_at("tests/checkout.spec.ts", PLAYWRIGHT_SPEC);

    let container = symbols
        .iter()
        .find(|symbol| symbol.name == "checkout")
        .expect("container symbol");
    let hook = symbols
        .iter()
        .find(|symbol| symbol.name == "beforeEach")
        .expect("lifecycle symbol");

    assert_eq!(hook.parent_id.as_deref(), Some(container.id.as_str()));
}

#[test]
fn each_table_call_extracts_a_parameterized_symbol() {
    let code = r#"
import { test } from "vitest";

test.each([[1, 2]])("adds %i", (a, b) => {});
"#;
    let symbols = symbols_at("tests/math.spec.ts", code);

    assert_eq!(
        role(&symbols, "adds %i").as_deref(),
        Some("parameterized_test"),
        "test.each(table)(name, fn) declares a parameterized test, got: {:?}",
        symbols.iter().map(|s| &s.name).collect::<Vec<_>>()
    );
}

#[test]
fn test_dsl_vocabulary_needs_a_framework_import_or_a_test_path() {
    let code = r#"
describe("migration runner", () => {
    setup(() => {});
    teardown(() => {});
    it("applies a migration", () => {});
});
"#;

    let production = symbols_at("src/migrations/runner.ts", code);
    let production_roles: Vec<_> = production
        .iter()
        .filter(|symbol| {
            symbol
                .metadata
                .as_ref()
                .is_some_and(|metadata| metadata.contains_key("test_role"))
        })
        .map(|symbol| &symbol.name)
        .collect();
    assert!(
        production_roles.is_empty(),
        "production file should carry no test roles, got: {production_roles:?}"
    );

    let spec = symbols_at("src/migrations/runner.spec.ts", code);
    assert_eq!(
        role(&spec, "migration runner").as_deref(),
        Some("test_container")
    );
    assert_eq!(role(&spec, "setup").as_deref(), Some("fixture_setup"));
    assert_eq!(role(&spec, "teardown").as_deref(), Some("fixture_teardown"));
}

#[test]
fn calls_inside_a_dotted_lifecycle_hook_are_attributed_to_it() {
    let code = r#"
import { test } from "@playwright/test";

function seedDatabase() {}

test.beforeEach(async () => {
    seedDatabase();
});
"#;

    let mut parser = init_parser();
    let tree = parser.parse(code, None).unwrap();
    let mut extractor = extractor_at("tests/checkout.spec.ts", code);
    let symbols = extractor.extract_symbols(&tree);
    let relationships = extractor.extract_relationships(&tree, &symbols);

    let hook = symbols
        .iter()
        .find(|symbol| symbol.name == "beforeEach")
        .expect("test.beforeEach should emit a lifecycle symbol");
    let seed_call = relationships
        .iter()
        .find(|relationship| {
            symbols.iter().any(|symbol| {
                symbol.id == relationship.to_symbol_id && symbol.name == "seedDatabase"
            })
        })
        .expect("seedDatabase() call should produce a relationship");

    assert_eq!(seed_call.kind, RelationshipKind::Calls);
    assert_eq!(seed_call.from_symbol_id, hook.id);
}
