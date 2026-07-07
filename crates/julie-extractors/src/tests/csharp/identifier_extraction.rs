// C# Identifier Extraction Tests (TDD RED phase)
//
// These tests validate the extract_identifiers() functionality which extracts:
// - Function calls (invocation_expression)
// - Member access (member_access_expression)
// - Proper containing symbol tracking (file-scoped)
//
// Following the Rust extractor reference implementation pattern

#![allow(unused_imports)]

use crate::base::{Identifier, IdentifierKind, Symbol, SymbolKind};
use crate::csharp::CSharpExtractor;
use crate::tests::csharp::init_parser;
use std::path::PathBuf;

#[cfg(test)]
mod identifier_extraction_tests {
    use super::*;

    fn extract_all(csharp_code: &str) -> (Vec<Symbol>, Vec<Identifier>) {
        let mut parser = init_parser();
        let tree = parser.parse(csharp_code, None).unwrap();
        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor = CSharpExtractor::new(
            "csharp".to_string(),
            "test.cs".to_string(),
            csharp_code.to_string(),
            &workspace_root,
        );
        let symbols = extractor.extract_symbols(&tree);
        let identifiers = extractor.extract_identifiers(&tree, &symbols);
        (symbols, identifiers)
    }

    #[test]
    fn test_extract_function_calls() {
        let csharp_code = r#"
using System;

public class Calculator {
    public int Add(int a, int b) {
        return a + b;
    }

    public int Calculate() {
        int result = Add(5, 3);      // Function call to Add
        Console.WriteLine(result);    // Function call to WriteLine
        return result;
    }
}
"#;

        let mut parser = init_parser();
        let tree = parser.parse(csharp_code, None).unwrap();

        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor = CSharpExtractor::new(
            "csharp".to_string(),
            "test.cs".to_string(),
            csharp_code.to_string(),
            &workspace_root,
        );

        // Extract symbols first
        let symbols = extractor.extract_symbols(&tree);

        // NOW extract identifiers (this will FAIL until we implement it)
        let identifiers = extractor.extract_identifiers(&tree, &symbols);

        // Verify we found the function calls
        let add_call = identifiers.iter().find(|id| id.name == "Add");
        assert!(
            add_call.is_some(),
            "Should extract 'Add' function call identifier"
        );
        let add_call = add_call.unwrap();
        assert_eq!(add_call.kind, IdentifierKind::Call);

        let writeline_call = identifiers.iter().find(|id| id.name == "WriteLine");
        assert!(
            writeline_call.is_some(),
            "Should extract 'WriteLine' function call identifier"
        );
        let writeline_call = writeline_call.unwrap();
        assert_eq!(writeline_call.kind, IdentifierKind::Call);

        // Verify containing symbol is set correctly (should be inside Calculate method)
        assert!(
            add_call.containing_symbol_id.is_some(),
            "Function call should have containing symbol"
        );

        // Find the Calculate method symbol
        let calculate_method = symbols.iter().find(|s| s.name == "Calculate").unwrap();

        // Verify the Add call is contained within Calculate method
        assert_eq!(
            add_call.containing_symbol_id.as_ref(),
            Some(&calculate_method.id),
            "Add call should be contained within Calculate method"
        );
    }

    #[test]
    fn test_extract_member_access() {
        let csharp_code = r#"
public class User {
    public string Name { get; set; }
    public string Email { get; set; }

    public void PrintInfo() {
        Console.WriteLine(this.Name);   // Member access: this.Name
        var email = this.Email;          // Member access: this.Email
    }
}
"#;

        let mut parser = init_parser();
        let tree = parser.parse(csharp_code, None).unwrap();

        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor = CSharpExtractor::new(
            "csharp".to_string(),
            "test.cs".to_string(),
            csharp_code.to_string(),
            &workspace_root,
        );

        let symbols = extractor.extract_symbols(&tree);
        let identifiers = extractor.extract_identifiers(&tree, &symbols);

        // Verify we found member access identifiers
        let name_access = identifiers
            .iter()
            .filter(|id| id.name == "Name" && id.kind == IdentifierKind::MemberAccess)
            .count();
        assert!(
            name_access > 0,
            "Should extract 'Name' member access identifier"
        );

        let email_access = identifiers
            .iter()
            .filter(|id| id.name == "Email" && id.kind == IdentifierKind::MemberAccess)
            .count();
        assert!(
            email_access > 0,
            "Should extract 'Email' member access identifier"
        );
    }

    #[test]
    fn test_file_scoped_containing_symbol() {
        // This test ensures we ONLY match symbols from the SAME FILE
        // Critical bug fix from Rust implementation (line 1311-1318 in rust.rs)
        let csharp_code = r#"
public class Service {
    public void Process() {
        Helper();              // Call to Helper in same file
    }

    private void Helper() {
        // Helper method
    }
}
"#;

        let mut parser = init_parser();
        let tree = parser.parse(csharp_code, None).unwrap();

        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor = CSharpExtractor::new(
            "csharp".to_string(),
            "test.cs".to_string(),
            csharp_code.to_string(),
            &workspace_root,
        );

        let symbols = extractor.extract_symbols(&tree);
        let identifiers = extractor.extract_identifiers(&tree, &symbols);

        // Find the Helper call
        let helper_call = identifiers.iter().find(|id| id.name == "Helper");
        assert!(helper_call.is_some());
        let helper_call = helper_call.unwrap();

        // Verify it has a containing symbol (the Process method)
        assert!(
            helper_call.containing_symbol_id.is_some(),
            "Helper call should have containing symbol from same file"
        );

        // Verify the containing symbol is the Process method
        let process_method = symbols.iter().find(|s| s.name == "Process").unwrap();
        assert_eq!(
            helper_call.containing_symbol_id.as_ref(),
            Some(&process_method.id),
            "Helper call should be contained within Process method"
        );
    }

    #[test]
    fn test_chained_member_access() {
        let csharp_code = r#"
public class DataService {
    public void Execute() {
        var result = user.Account.Balance;   // Chained member access
        var name = customer.Profile.Name;     // Chained member access
    }
}
"#;

        let mut parser = init_parser();
        let tree = parser.parse(csharp_code, None).unwrap();

        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor = CSharpExtractor::new(
            "csharp".to_string(),
            "test.cs".to_string(),
            csharp_code.to_string(),
            &workspace_root,
        );

        let symbols = extractor.extract_symbols(&tree);
        let identifiers = extractor.extract_identifiers(&tree, &symbols);

        // Should extract the rightmost identifiers in chains
        let balance_access = identifiers
            .iter()
            .find(|id| id.name == "Balance" && id.kind == IdentifierKind::MemberAccess);
        assert!(
            balance_access.is_some(),
            "Should extract 'Balance' from chained member access"
        );

        let name_access = identifiers
            .iter()
            .find(|id| id.name == "Name" && id.kind == IdentifierKind::MemberAccess);
        assert!(
            name_access.is_some(),
            "Should extract 'Name' from chained member access"
        );
    }

    #[test]
    fn test_no_duplicate_identifiers() {
        let csharp_code = r#"
public class Test {
    public void Run() {
        Process();
        Process();  // Same call twice
    }

    private void Process() {
    }
}
"#;

        let mut parser = init_parser();
        let tree = parser.parse(csharp_code, None).unwrap();

        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor = CSharpExtractor::new(
            "csharp".to_string(),
            "test.cs".to_string(),
            csharp_code.to_string(),
            &workspace_root,
        );

        let symbols = extractor.extract_symbols(&tree);
        let identifiers = extractor.extract_identifiers(&tree, &symbols);

        // Should extract BOTH calls (they're at different locations)
        let process_calls: Vec<_> = identifiers
            .iter()
            .filter(|id| id.name == "Process" && id.kind == IdentifierKind::Call)
            .collect();

        assert_eq!(
            process_calls.len(),
            2,
            "Should extract both Process calls at different locations"
        );

        // Verify they have different line numbers
        assert_ne!(
            process_calls[0].start_line, process_calls[1].start_line,
            "Duplicate calls should have different line numbers"
        );
    }

    #[test]
    fn test_csharp_type_usage_identifiers_cover_fields_params_returns_and_generics() {
        let csharp_code = r#"
using System.Collections.Generic;

public class User {}
public class UserRequest {}
public class Result<T> {}
public class Repository<T> {}

public class Controller {
    private Repository<User> repository;

    public Result<User> Load(UserRequest request, List<User> users) {
        return new Result<User>();
    }
}
"#;

        let (_symbols, identifiers) = extract_all(csharp_code);
        let type_names: Vec<&str> = identifiers
            .iter()
            .filter(|id| id.kind == IdentifierKind::TypeUsage)
            .map(|id| id.name.as_str())
            .collect();

        for expected in ["Repository", "User", "Result", "UserRequest", "List"] {
            assert!(
                type_names.contains(&expected),
                "missing C# type usage {expected}; got {type_names:?}"
            );
        }
    }

    #[test]
    fn test_csharp_variable_ref_emission() {
        // Locked variable_ref contract: receivers + bare value reads, cross-cutting
        // the call/member/type arms. See identifiers.rs doc-comment for the 6 rules.
        let csharp_code = r#"
namespace Demo;

using System;

public static class GraphTraversal {
    public static int Reach() => 0;
}

public class Sample {
    private int count;
    public int Bar;

    // GhostToken appears only in this comment and must never be an identifier.
    public int Evaluate(int seed, int unusedParam) {
        count += 1;                          // compound assignment -> read count
        int x = 5;                           // declaration name, no ref
        x = 7;                               // plain write LHS -> NOT a read
        var total = seed;                    // seed on RHS -> read
        var g = GraphTraversal.Reach();      // GraphTraversal receiver -> read; Reach -> call
        var f = new Sample { Bar = seed };   // Bar initializer member -> read
        var n = nameof(VisibilityUnknown);   // nameof operand -> read
        var w = Filter(IsUserType);          // method-group arg -> read
        return total > 0 ? total : VisibilityUnknown; // bare read of VisibilityUnknown
    }

    private bool IsUserType(int a) => true;
    private int Filter(Func<int, bool> p) => 0;
    private const int VisibilityUnknown = 3;

    // Return-type identifier is a type usage, not a value read.
    public Widget Build() => null;
}

public sealed class Widget {}

public sealed class FooAttribute : Attribute {
    public int Baz;
}

public class Decorated {
    [Foo(Baz = 1)]                           // attribute named arg -> read Baz
    public void M() {}
}
"#;

        let (symbols, identifiers) = extract_all(csharp_code);

        let var_refs: Vec<&str> = identifiers
            .iter()
            .filter(|id| id.kind == IdentifierKind::VariableRef)
            .map(|id| id.name.as_str())
            .collect();

        // --- Positive cases (rule 1/4) ---
        for expected in [
            "GraphTraversal",    // static-access receiver
            "VisibilityUnknown", // nameof operand + bare return read
            "IsUserType",        // method-group argument
            "Bar",               // object-initializer member LHS
            "Baz",               // attribute named argument
            "count",             // compound-assignment target
            "seed",              // RHS / argument value read
        ] {
            assert!(
                var_refs.contains(&expected),
                "expected variable_ref for {expected}; got {var_refs:?}"
            );
        }

        // Receiver + call coexist: GraphTraversal.Reach()
        assert!(
            identifiers
                .iter()
                .any(|id| id.name == "Reach" && id.kind == IdentifierKind::Call),
            "GraphTraversal.Reach() must still yield a call named Reach"
        );

        // --- Negative cases (rules 2/3/4/5) ---
        for forbidden in [
            "x",           // declaration name + plain-write LHS
            "unusedParam", // parameter name only
            "int",         // builtin type
            "GhostToken",  // comment-only mention
            "Sample",      // type/declaration name, never a value read
            "Evaluate",    // method declaration name
            "Demo",        // file-scoped namespace name
            "Widget",      // method return type -> type usage, not a value read
        ] {
            assert!(
                !var_refs.contains(&forbidden),
                "{forbidden} must NOT be a variable_ref; got {var_refs:?}"
            );
        }
        // GhostToken must not appear as ANY identifier kind.
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
            .find(|s| s.name == "Evaluate")
            .expect("Evaluate method extracted");
        let graph_ref = identifiers
            .iter()
            .find(|id| id.name == "GraphTraversal" && id.kind == IdentifierKind::VariableRef)
            .expect("GraphTraversal variable_ref");
        assert_eq!(
            graph_ref.containing_symbol_id.as_deref(),
            Some(evaluate.id.as_str()),
            "receiver variable_ref should be contained in Evaluate"
        );
    }

    #[test]
    fn test_csharp_object_creation_emits_constructor_call_identifier() {
        let csharp_code = r#"
public class User {}
public class Result<T> {}

public class Controller {
    public Result<User> Build() {
        var user = new User();
        return new Result<User>();
    }
}
"#;

        let (symbols, identifiers) = extract_all(csharp_code);
        let build = symbols
            .iter()
            .find(|symbol| symbol.name == "Build")
            .expect("Build method should be extracted");

        for expected in ["User", "Result"] {
            let call = identifiers
                .iter()
                .find(|id| id.name == expected && id.kind == IdentifierKind::Call)
                .unwrap_or_else(|| {
                    panic!(
                        "missing constructor call identifier {expected}; got {:?}",
                        identifiers
                            .iter()
                            .map(|id| (&id.name, &id.kind))
                            .collect::<Vec<_>>()
                    )
                });
            assert_eq!(
                call.containing_symbol_id.as_deref(),
                Some(build.id.as_str())
            );
        }
    }
}
