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
use crate::tests::csharp::init_test_parser;
use std::path::PathBuf;

#[cfg(test)]
mod identifier_extraction_tests {
    use super::*;

    #[test]
    fn test_csharp_pointer_misparse_multiplication_emits_variable_refs() {
        // tree-sitter-c-sharp resolves `identifier * identifier` in argument/expression
        // position as a pointer-type declaration_expression. C# pointer types are legal
        // ONLY in `unsafe` contexts, so with no enclosing unsafe context this is really a
        // multiplication: BOTH operands are value reads. See identifiers.rs LOCKED CONTRACT.
        let csharp_code = r#"
public class Calc {
    // (a) argument-position multiplication misparsed as `pointer_type limit` / name `K`
    public int Score(int limit, int K) {
        int s = System.Math.Max(limit * K, 24);
        return s;
    }

    // (c) literal RHS cannot be a declarator, so this already parses as multiplication
    public int Twice(int limit) {
        int t = limit * 3;
        return t;
    }
}
"#;

        let (_symbols, identifiers) = extract_all(csharp_code);

        let var_refs: Vec<&str> = identifiers
            .iter()
            .filter(|id| id.kind == IdentifierKind::VariableRef)
            .map(|id| id.name.as_str())
            .collect();

        // (a) BOTH operands of `limit * K` are value reads.
        assert!(
            var_refs.contains(&"limit"),
            "expected variable_ref for `limit`; got {var_refs:?}"
        );
        assert!(
            var_refs.contains(&"K"),
            "expected variable_ref for `K` (declaration_expression name of the misparse); got {var_refs:?}"
        );

        // (a) `limit` must NOT be emitted as a bogus type usage (kind honesty).
        assert!(
            !identifiers
                .iter()
                .any(|id| id.name == "limit" && id.kind == IdentifierKind::TypeUsage),
            "`limit` must not be a type_usage in the misparse shape"
        );

        // (a)+(c) `limit` appears once in Score (`limit * K`) and once in Twice
        // (`limit * 3`); BOTH are value reads, no bogus type_usage, no duplicates.
        let limit_rows: Vec<&IdentifierKind> = identifiers
            .iter()
            .filter(|id| id.name == "limit")
            .map(|id| &id.kind)
            .collect();
        assert_eq!(
            limit_rows.len(),
            2,
            "expected exactly one `limit` row per usage (Score + Twice); got {limit_rows:?}"
        );
        assert!(
            limit_rows
                .iter()
                .all(|k| **k == IdentifierKind::VariableRef),
            "every `limit` usage must be a variable_ref; got {limit_rows:?}"
        );
    }

    #[test]
    fn test_csharp_unsafe_pointer_declaration_unchanged() {
        // Genuine unsafe pointer syntax must be left exactly as today: the pointee is a
        // type usage and the declarator name is excluded. The unsafe-context gate must
        // ALSO suppress the multiplication-recovery on the misparse shape.
        let csharp_code = r#"
public class Node {}

public class Ptr {
    // Genuine pointer declaration inside an unsafe method: `Node` is a type usage.
    public unsafe void Use(int seed) {
        Node* p = null;
    }

    // Misparse shape wrapped in an unsafe block: gate honored, behavior unchanged.
    public void Guarded(int limit, int K) {
        unsafe {
            int s = System.Math.Max(limit * K, 24);
        }
    }
}
"#;

        let (_symbols, identifiers) = extract_all(csharp_code);

        // Genuine unsafe pointer: pointee `Node` stays a type usage.
        assert!(
            identifiers
                .iter()
                .any(|id| id.name == "Node" && id.kind == IdentifierKind::TypeUsage),
            "genuine unsafe pointer pointee `Node` must remain a type_usage"
        );

        // Unsafe gate honored on the misparse shape: `K` (declaration name) is not
        // resurrected as a value read, and `limit` keeps its current type_usage.
        let var_refs: Vec<&str> = identifiers
            .iter()
            .filter(|id| id.kind == IdentifierKind::VariableRef)
            .map(|id| id.name.as_str())
            .collect();
        assert!(
            !var_refs.contains(&"K"),
            "inside `unsafe`, the pointer parse is authoritative; `K` must not become a variable_ref; got {var_refs:?}"
        );
        assert!(
            identifiers
                .iter()
                .any(|id| id.name == "limit" && id.kind == IdentifierKind::TypeUsage),
            "inside `unsafe`, `limit` keeps its (unchanged) type_usage behavior"
        );
    }

    fn extract_all(csharp_code: &str) -> (Vec<Symbol>, Vec<Identifier>) {
        let mut parser = init_test_parser();
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
    fn conditional_access_emits_member_and_call_identifiers() {
        let source = r#"
public class ResultReader {
    public object? Read(ResultModel result) {
        result?.Refresh();
        return result?.UploadFailures;
    }
}
"#;
        let results =
            crate::pipeline::extract_canonical("test.cs", source, &PathBuf::from("/tmp/test"))
                .expect("canonical C# extraction should succeed");

        assert!(results.parse_diagnostics.is_empty());
        assert!(results.identifiers.iter().any(|identifier| {
            identifier.name == "UploadFailures" && identifier.kind == IdentifierKind::MemberAccess
        }));
        assert!(results.identifiers.iter().any(|identifier| {
            identifier.name == "Refresh" && identifier.kind == IdentifierKind::Call
        }));
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

        let mut parser = init_test_parser();
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

        let mut parser = init_test_parser();
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

        let mut parser = init_test_parser();
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

        let mut parser = init_test_parser();
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

        let mut parser = init_test_parser();
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
    fn this_receiver_call_records_enclosing_type_as_receiver_type() {
        let source = r#"
public class OrderService : ServiceBase
{
    public void Process()
    {
        this.Persist();
        base.Restore();
        Log();
    }
}
"#;
        let results =
            crate::extract_canonical("OrderService.cs", source, std::path::Path::new("/tmp/test"))
                .expect("canonical C# extraction must succeed");

        let call = |name: &str| {
            results
                .identifiers
                .iter()
                .find(|id| id.name == name && id.kind == IdentifierKind::Call)
                .unwrap_or_else(|| panic!("missing call identifier {name}"))
        };
        assert_eq!(
            call("Persist").receiver_type.as_deref(),
            Some("OrderService")
        );
        assert_eq!(
            call("Restore").receiver_type.as_deref(),
            Some("ServiceBase")
        );
        assert_eq!(call("Log").receiver_type, None);

        let pending = |name: &str| {
            results
                .structured_pending_relationships
                .iter()
                .find(|p| p.target.terminal_name == name)
                .unwrap_or_else(|| panic!("missing structured pending for {name}"))
        };
        assert_eq!(
            pending("Persist").receiver_type.as_deref(),
            Some("OrderService")
        );
        assert_eq!(
            pending("Restore").receiver_type.as_deref(),
            Some("ServiceBase")
        );
        assert_eq!(pending("Log").receiver_type, None);
    }

    #[test]
    fn base_receiver_without_declared_base_type_records_no_receiver_type() {
        let source = r#"
public class Standalone
{
    public void Run()
    {
        base.Finish();
    }
}
"#;
        let results =
            crate::extract_canonical("Standalone.cs", source, std::path::Path::new("/tmp/test"))
                .expect("canonical C# extraction must succeed");

        let finish = results
            .identifiers
            .iter()
            .find(|id| id.name == "Finish" && id.kind == IdentifierKind::Call)
            .expect("missing call identifier Finish");
        assert_eq!(finish.receiver_type, None);
        let pending = results
            .structured_pending_relationships
            .iter()
            .find(|p| p.target.terminal_name == "Finish")
            .expect("missing structured pending for Finish");
        assert_eq!(pending.receiver_type, None);
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
