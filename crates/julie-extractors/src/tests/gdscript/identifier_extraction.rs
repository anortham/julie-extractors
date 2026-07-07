// GDScript Identifier Extraction Tests (TDD RED phase)
//
// These tests validate the extract_identifiers() functionality which extracts:
// - Function calls (call, get_node)
// - Member access (attribute, subscript)
// - Proper containing symbol tracking (file-scoped)
//
// Following the Rust extractor reference implementation pattern

#![allow(unused_imports)]

use crate::base::{IdentifierKind, SymbolKind};
use crate::gdscript::GDScriptExtractor;
use crate::tests::helpers::init_parser;
use std::path::PathBuf;

#[cfg(test)]
mod identifier_extraction_tests {
    use super::*;

    #[test]
    fn test_gdscript_function_calls() {
        let gdscript_code = r#"
extends Node

func process_data():
    var result = calculate(5, 3)  # Function call to calculate
    print(result)                  # Function call to print
    return result

func calculate(a, b):
    return a + b
"#;

        let tree = init_parser(gdscript_code, "gdscript");
        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor = GDScriptExtractor::new(
            "gdscript".to_string(),
            "test.gd".to_string(),
            gdscript_code.to_string(),
            &workspace_root,
        );

        // Extract symbols first
        let symbols = extractor.extract_symbols(&tree);

        // NOW extract identifiers (this will FAIL until we implement it)
        let identifiers = extractor.extract_identifiers(&tree, &symbols);

        // Verify we found the function calls
        let calculate_call = identifiers.iter().find(|id| id.name == "calculate");
        assert!(
            calculate_call.is_some(),
            "Should extract 'calculate' function call identifier"
        );
        let calculate_call = calculate_call.unwrap();
        assert_eq!(calculate_call.kind, IdentifierKind::Call);

        let print_call = identifiers.iter().find(|id| id.name == "print");
        assert!(
            print_call.is_some(),
            "Should extract 'print' function call identifier"
        );
        let print_call = print_call.unwrap();
        assert_eq!(print_call.kind, IdentifierKind::Call);

        // Verify containing symbol is set correctly (should be inside process_data function)
        assert!(
            calculate_call.containing_symbol_id.is_some(),
            "Function call should have containing symbol"
        );

        // Find the process_data function symbol
        let process_data_func = symbols.iter().find(|s| s.name == "process_data").unwrap();

        // Verify the calculate call is contained within process_data function
        assert_eq!(
            calculate_call.containing_symbol_id.as_ref(),
            Some(&process_data_func.id),
            "calculate call should be contained within process_data function"
        );
    }

    #[test]
    fn test_gdscript_member_access() {
        let gdscript_code = r#"
extends Node

var player

func _ready():
    var pos = player.position    # Member access: player.position
    var health = player.health    # Member access: player.health
"#;

        let tree = init_parser(gdscript_code, "gdscript");
        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor = GDScriptExtractor::new(
            "gdscript".to_string(),
            "test.gd".to_string(),
            gdscript_code.to_string(),
            &workspace_root,
        );

        let symbols = extractor.extract_symbols(&tree);
        let identifiers = extractor.extract_identifiers(&tree, &symbols);

        // Verify we found member access identifiers
        let position_access = identifiers
            .iter()
            .filter(|id| id.name == "position" && id.kind == IdentifierKind::MemberAccess)
            .count();
        assert!(
            position_access > 0,
            "Should extract 'position' member access identifier"
        );

        let health_access = identifiers
            .iter()
            .filter(|id| id.name == "health" && id.kind == IdentifierKind::MemberAccess)
            .count();
        assert!(
            health_access > 0,
            "Should extract 'health' member access identifier"
        );
    }

    #[test]
    fn test_gdscript_attribute_call_identifier_uses_rightmost_name() {
        let gdscript_code = r#"
extends Node

func update_inventory(item):
    player.inventory.add_item(item)
"#;

        let tree = init_parser(gdscript_code, "gdscript");
        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor = GDScriptExtractor::new(
            "gdscript".to_string(),
            "test.gd".to_string(),
            gdscript_code.to_string(),
            &workspace_root,
        );

        let symbols = extractor.extract_symbols(&tree);
        let identifiers = extractor.extract_identifiers(&tree, &symbols);

        let add_item_call = identifiers
            .iter()
            .find(|id| id.name == "add_item" && id.kind == IdentifierKind::Call);
        assert!(
            add_item_call.is_some(),
            "Should extract the rightmost method name from attribute calls"
        );

        assert!(
            identifiers
                .iter()
                .all(|id| id.name != "player.inventory.add_item"),
            "Attribute calls should not be indexed under the dotted receiver chain"
        );
    }

    #[test]
    fn test_gdscript_identifiers_have_containing_symbol() {
        // This test ensures we ONLY match symbols from the SAME FILE
        // Critical bug fix from Rust implementation (line 1311-1318 in rust.rs)
        let gdscript_code = r#"
extends Node

func start():
    helper()              # Call to helper in same file

func helper():
    pass
"#;

        let tree = init_parser(gdscript_code, "gdscript");
        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor = GDScriptExtractor::new(
            "gdscript".to_string(),
            "test.gd".to_string(),
            gdscript_code.to_string(),
            &workspace_root,
        );

        let symbols = extractor.extract_symbols(&tree);
        let identifiers = extractor.extract_identifiers(&tree, &symbols);

        // Find the helper call
        let helper_call = identifiers.iter().find(|id| id.name == "helper");
        assert!(helper_call.is_some());
        let helper_call = helper_call.unwrap();

        // Verify it has a containing symbol (the start function)
        assert!(
            helper_call.containing_symbol_id.is_some(),
            "helper call should have containing symbol from same file"
        );

        // Verify the containing symbol is the start function
        let start_func = symbols.iter().find(|s| s.name == "start").unwrap();
        assert_eq!(
            helper_call.containing_symbol_id.as_ref(),
            Some(&start_func.id),
            "helper call should be contained within start function"
        );
    }

    #[test]
    fn test_gdscript_chained_member_access() {
        let gdscript_code = r#"
extends Node

func get_data():
    var balance = user.account.balance   # Chained member access
    var name = customer.profile.name      # Chained member access
"#;

        let tree = init_parser(gdscript_code, "gdscript");
        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor = GDScriptExtractor::new(
            "gdscript".to_string(),
            "test.gd".to_string(),
            gdscript_code.to_string(),
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
    fn test_gdscript_duplicate_calls_at_different_locations() {
        let gdscript_code = r#"
extends Node

func run():
    process()
    process()  # Same call twice

func process():
    pass
"#;

        let tree = init_parser(gdscript_code, "gdscript");
        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor = GDScriptExtractor::new(
            "gdscript".to_string(),
            "test.gd".to_string(),
            gdscript_code.to_string(),
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

    // ========================================================================
    // Type annotation identifier extraction (Bug fix: GDScript type refs)
    // ========================================================================

    #[test]
    fn test_gdscript_extends_creates_type_usage_identifier() {
        let gdscript_code = r#"
extends PandoraEntity

class_name PandoraCategory

func get_name() -> String:
    return _name
"#;

        let tree = init_parser(gdscript_code, "gdscript");
        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor = GDScriptExtractor::new(
            "gdscript".to_string(),
            "test.gd".to_string(),
            gdscript_code.to_string(),
            &workspace_root,
        );

        let symbols = extractor.extract_symbols(&tree);
        let identifiers = extractor.extract_identifiers(&tree, &symbols);

        // extends PandoraEntity should create a TypeUsage identifier
        let extends_ref = identifiers
            .iter()
            .find(|id| id.name == "PandoraEntity" && id.kind == IdentifierKind::TypeUsage);
        assert!(
            extends_ref.is_some(),
            "Should extract 'PandoraEntity' as TypeUsage from extends clause.\n\
             Found identifiers: {:?}",
            identifiers
                .iter()
                .map(|id| format!("{}:{:?}", id.name, id.kind))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_gdscript_variable_type_annotation_creates_identifier() {
        let gdscript_code = r#"
extends Node

var _property: PandoraProperty
var _backend: PandoraEntityBackend
var _count: int = 0
"#;

        let tree = init_parser(gdscript_code, "gdscript");
        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor = GDScriptExtractor::new(
            "gdscript".to_string(),
            "test.gd".to_string(),
            gdscript_code.to_string(),
            &workspace_root,
        );

        let symbols = extractor.extract_symbols(&tree);
        let identifiers = extractor.extract_identifiers(&tree, &symbols);

        let type_usages: Vec<_> = identifiers
            .iter()
            .filter(|id| id.kind == IdentifierKind::TypeUsage)
            .collect();

        // Should find PandoraProperty, PandoraEntityBackend, int, and Node (from extends)
        let property_ref = type_usages.iter().find(|id| id.name == "PandoraProperty");
        assert!(
            property_ref.is_some(),
            "Should extract 'PandoraProperty' TypeUsage from var type annotation.\n\
             Found type usages: {:?}",
            type_usages.iter().map(|id| &id.name).collect::<Vec<_>>()
        );

        let backend_ref = type_usages
            .iter()
            .find(|id| id.name == "PandoraEntityBackend");
        assert!(
            backend_ref.is_some(),
            "Should extract 'PandoraEntityBackend' TypeUsage from var type annotation"
        );
    }

    #[test]
    fn test_gdscript_function_param_and_return_type_creates_identifier() {
        let gdscript_code = r#"
extends Node

func create_entity(name: String, category: PandoraCategory) -> PandoraEntity:
    pass
"#;

        let tree = init_parser(gdscript_code, "gdscript");
        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor = GDScriptExtractor::new(
            "gdscript".to_string(),
            "test.gd".to_string(),
            gdscript_code.to_string(),
            &workspace_root,
        );

        let symbols = extractor.extract_symbols(&tree);
        let identifiers = extractor.extract_identifiers(&tree, &symbols);

        let type_usages: Vec<_> = identifiers
            .iter()
            .filter(|id| id.kind == IdentifierKind::TypeUsage)
            .collect();

        // Parameter types
        let string_ref = type_usages.iter().find(|id| id.name == "String");
        assert!(
            string_ref.is_some(),
            "Should extract 'String' TypeUsage from parameter type.\n\
             Found type usages: {:?}",
            type_usages.iter().map(|id| &id.name).collect::<Vec<_>>()
        );

        let category_ref = type_usages.iter().find(|id| id.name == "PandoraCategory");
        assert!(
            category_ref.is_some(),
            "Should extract 'PandoraCategory' TypeUsage from parameter type"
        );

        // Return type
        let return_ref = type_usages.iter().find(|id| id.name == "PandoraEntity");
        assert!(
            return_ref.is_some(),
            "Should extract 'PandoraEntity' TypeUsage from return type"
        );
    }

    #[test]
    fn test_gdscript_type_usage_has_containing_symbol() {
        let gdscript_code = r#"
extends Node

func process(entity: PandoraEntity) -> void:
    var prop: PandoraProperty = entity.get_property("name")
"#;

        let tree = init_parser(gdscript_code, "gdscript");
        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor = GDScriptExtractor::new(
            "gdscript".to_string(),
            "test.gd".to_string(),
            gdscript_code.to_string(),
            &workspace_root,
        );

        let symbols = extractor.extract_symbols(&tree);
        let identifiers = extractor.extract_identifiers(&tree, &symbols);

        // Type annotations inside a function should have the function as containing symbol
        let prop_type = identifiers
            .iter()
            .find(|id| id.name == "PandoraProperty" && id.kind == IdentifierKind::TypeUsage);
        assert!(
            prop_type.is_some(),
            "Should extract PandoraProperty TypeUsage"
        );

        let process_func = symbols.iter().find(|s| s.name == "process").unwrap();
        let prop_type = prop_type.unwrap();
        assert_eq!(
            prop_type.containing_symbol_id.as_ref(),
            Some(&process_func.id),
            "Type annotation inside function should have function as containing symbol"
        );
    }

    #[test]
    fn test_gdscript_variable_ref_emission() {
        // Locked variable_ref contract (see csharp/identifiers.rs): receivers +
        // bare value reads, the complement of the Call/MemberAccess/TypeUsage arms.
        let gdscript_code = r#"
extends Node

var speed: int = 5
var health := 10

enum State { IDLE, RUNNING }

func evaluate(seed, unused_param) -> int:
    health -= seed              # compound target + RHS -> both reads
    var local_x = 5
    local_x = 7                 # plain write LHS -> NOT a read
    var total = speed           # bare RHS read
    var g = GraphUtil.reach()   # receiver read; reach stays a call
    var fn_ref = compute        # bare read of a function name
    var arr = [seed, health]    # collection elements -> reads
    for item in items:
        total += item
    var w = self.hp             # self receiver is a keyword, not a variable_ref
    match total:
        speed:
            pass
        _:                      # wildcard pattern is not a read
            pass
    # GhostToken appears only in this comment
    return total

func compute(v: int) -> int:
    return v
"#;

        let tree = init_parser(gdscript_code, "gdscript");
        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor = GDScriptExtractor::new(
            "gdscript".to_string(),
            "test.gd".to_string(),
            gdscript_code.to_string(),
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
            "GraphUtil", // member-access receiver
            "speed",     // bare RHS read
            "seed",      // compound-assignment RHS + array element
            "health",    // compound-assignment target
            "compute",   // bare read of a function name (method-group analogue)
            "total",     // augmented target + bare return read
            "items",     // for-loop collection
        ] {
            assert!(
                var_refs.contains(&expected),
                "expected variable_ref for {expected}; got {var_refs:?}"
            );
        }

        // Receiver + call coexist: GraphUtil.reach()
        assert!(
            identifiers
                .iter()
                .any(|id| id.name == "reach" && id.kind == IdentifierKind::Call),
            "GraphUtil.reach() must still yield a call named reach"
        );

        // --- Negative cases (rules 2/3/4/5) ---
        for forbidden in [
            "local_x",      // declaration name + plain-write LHS only
            "unused_param", // parameter name only
            "int",          // type annotation -> TypeUsage territory
            "GhostToken",   // comment-only mention
            "evaluate",     // function declaration name (a `name` node)
            "IDLE",         // enum member declaration name
            "self",         // keyword receiver
            "reach",        // call callee, owned by the Call arm
            "_",            // match wildcard pattern, not a read
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

        // containing_symbol_id is populated on a variable_ref.
        let evaluate = symbols
            .iter()
            .find(|s| s.name == "evaluate")
            .expect("evaluate function extracted");
        let graph_ref = identifiers
            .iter()
            .find(|id| id.name == "GraphUtil" && id.kind == IdentifierKind::VariableRef)
            .expect("GraphUtil variable_ref");
        assert_eq!(
            graph_ref.containing_symbol_id.as_deref(),
            Some(evaluate.id.as_str()),
            "receiver variable_ref should be contained in evaluate"
        );
    }
}
