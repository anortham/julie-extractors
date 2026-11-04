// C++ Identifier Extraction Tests (TDD RED phase)
//
// These tests validate the extract_identifiers() functionality which extracts:
// - Function calls (call_expression)
// - Member access (field_expression)
// - Proper containing symbol tracking (file-scoped)
//
// Following the Rust extractor reference implementation pattern

#![allow(unused_imports)]

use crate::base::{IdentifierKind, SymbolKind};
use crate::cpp::CppExtractor;
use crate::tests::cpp::init_test_parser;
use std::path::PathBuf;

#[cfg(test)]
mod identifier_extraction_tests {
    use super::*;

    #[test]
    fn member_callees_have_one_identifier_per_token() {
        let source = "void use(Thing& object) { object.send(); object.send(); auto value = object.child.value; }";
        let results = crate::extract_canonical_for_language_at(
            "cpp",
            "source.cpp",
            source,
            &PathBuf::from("."),
            crate::ExtractionLevel::Full,
        )
        .unwrap();
        let calls: Vec<_> = results
            .identifiers
            .iter()
            .filter(|identifier| identifier.name == "send")
            .collect();
        assert_eq!(calls.len(), 2);
        assert!(
            calls
                .iter()
                .all(|identifier| identifier.kind == IdentifierKind::Call)
        );
        assert_ne!(calls[0].id, calls[1].id);
        for name in ["child", "value"] {
            assert!(results.identifiers.iter().any(|identifier| {
                identifier.name == name && identifier.kind == IdentifierKind::MemberAccess
            }));
        }
        let mut ids = std::collections::HashSet::new();
        assert!(
            results
                .identifiers
                .iter()
                .all(|identifier| ids.insert(&identifier.id))
        );
    }

    #[test]
    fn qualified_and_template_calls_select_the_terminal_name() {
        let source = "void use(Worker& object) { remote::ns::run(); Box<Item>::make(); object.send<Item>(); }";
        let results = crate::extract_canonical_for_language_at(
            "cpp",
            "source.cpp",
            source,
            &PathBuf::from("."),
            crate::ExtractionLevel::Full,
        )
        .unwrap();
        for (name, receiver, qualifier) in [
            ("run", "ns", Some("remote")),
            ("make", "Box", None),
            ("send", "object", None),
        ] {
            let call = results
                .identifiers
                .iter()
                .find(|identifier| {
                    identifier.kind == IdentifierKind::Call && identifier.name == name
                })
                .unwrap_or_else(|| panic!("missing call {name}: {:?}", results.identifiers));
            assert_eq!(
                &source[call.start_byte as usize..call.end_byte as usize],
                name
            );
            let metadata = call.metadata.as_ref().expect("named receiver metadata");
            assert_eq!(metadata["receiver"].as_str(), Some(receiver));
            assert_eq!(
                metadata
                    .get("receiver_qualifier")
                    .and_then(serde_json::Value::as_str),
                qualifier
            );
        }
    }

    #[test]
    fn test_extract_function_calls() {
        let cpp_code = r#"
class Calculator {
public:
    int add(int a, int b) {
        return a + b;
    }

    int calculate() {
        int result = add(5, 3);      // Function call to add
        printf("Result: %d\n", result);    // Function call to printf
        return result;
    }
};
"#;

        let mut parser = init_test_parser();
        let tree = parser.parse(cpp_code, None).unwrap();

        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor = CppExtractor::new(
            "test.cpp".to_string(),
            cpp_code.to_string(),
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

        let printf_call = identifiers.iter().find(|id| id.name == "printf");
        assert!(
            printf_call.is_some(),
            "Should extract 'printf' function call identifier"
        );
        let printf_call = printf_call.unwrap();
        assert_eq!(printf_call.kind, IdentifierKind::Call);

        // Verify containing symbol is set correctly (should be inside calculate method)
        assert!(
            add_call.containing_symbol_id.is_some(),
            "Function call should have containing symbol"
        );

        // Find the calculate method symbol
        let calculate_method = symbols.iter().find(|s| s.name == "calculate").unwrap();

        // Verify the add call is contained within calculate method
        assert_eq!(
            add_call.containing_symbol_id.as_ref(),
            Some(&calculate_method.id),
            "add call should be contained within calculate method"
        );
    }

    #[test]
    fn test_extract_method_call_names_from_field_expression() {
        let cpp_code = r#"
class Service {
public:
    void process() {
    }

    void run() {
        service.process();
        this->process();
    }

private:
    Service service;
};
"#;

        let mut parser = init_test_parser();
        let tree = parser.parse(cpp_code, None).unwrap();

        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor = CppExtractor::new(
            "test.cpp".to_string(),
            cpp_code.to_string(),
            &workspace_root,
        );

        let symbols = extractor.extract_symbols(&tree);
        let identifiers = extractor.extract_identifiers(&tree, &symbols);

        let process_calls: Vec<_> = identifiers
            .iter()
            .filter(|id| id.kind == IdentifierKind::Call && id.name == "process")
            .collect();
        assert_eq!(
            process_calls.len(),
            2,
            "Should extract both method calls as 'process'"
        );

        assert!(
            identifiers
                .iter()
                .all(|id| !(id.kind == IdentifierKind::Call && id.name == "service.process")),
            "Should not keep receiver-qualified call name for service.process()"
        );

        assert!(
            identifiers
                .iter()
                .all(|id| !(id.kind == IdentifierKind::Call && id.name == "this->process")),
            "Should not keep receiver-qualified call name for this->process()"
        );
    }

    #[test]
    fn test_extract_member_access() {
        let cpp_code = r#"
class User {
public:
    std::string name;
    std::string email;

    void printInfo() {
        std::cout << this->name << std::endl;   // Member access: this->name
        auto userEmail = this->email;           // Member access: this->email
    }
};
"#;

        let mut parser = init_test_parser();
        let tree = parser.parse(cpp_code, None).unwrap();

        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor = CppExtractor::new(
            "test.cpp".to_string(),
            cpp_code.to_string(),
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
        // Critical bug fix from Rust implementation (line 1311-1318 in rust.rs)
        let cpp_code = r#"
class Service {
public:
    void process() {
        helper();              // Call to helper in same file
    }

private:
    void helper() {
        // Helper method
    }
};
"#;

        let mut parser = init_test_parser();
        let tree = parser.parse(cpp_code, None).unwrap();

        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor = CppExtractor::new(
            "test.cpp".to_string(),
            cpp_code.to_string(),
            &workspace_root,
        );

        let symbols = extractor.extract_symbols(&tree);
        let identifiers = extractor.extract_identifiers(&tree, &symbols);

        // Find the helper call
        let helper_call = identifiers.iter().find(|id| id.name == "helper");
        assert!(helper_call.is_some());
        let helper_call = helper_call.unwrap();

        // Verify it has a containing symbol (the process method)
        assert!(
            helper_call.containing_symbol_id.is_some(),
            "helper call should have containing symbol from same file"
        );

        // Verify the containing symbol is the process method
        let process_method = symbols.iter().find(|s| s.name == "process").unwrap();
        assert_eq!(
            helper_call.containing_symbol_id.as_ref(),
            Some(&process_method.id),
            "helper call should be contained within process method"
        );
    }

    #[test]
    fn test_chained_member_access() {
        let cpp_code = r#"
class DataService {
public:
    void execute() {
        auto balance = user.account.balance;   // Chained member access
        auto name = customer.profile.name;     // Chained member access
    }
};
"#;

        let mut parser = init_test_parser();
        let tree = parser.parse(cpp_code, None).unwrap();

        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor = CppExtractor::new(
            "test.cpp".to_string(),
            cpp_code.to_string(),
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
        let cpp_code = r#"
class Test {
public:
    void run() {
        process();
        process();  // Same call twice
    }

private:
    void process() {
    }
};
"#;

        let mut parser = init_test_parser();
        let tree = parser.parse(cpp_code, None).unwrap();

        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor = CppExtractor::new(
            "test.cpp".to_string(),
            cpp_code.to_string(),
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
    fn test_cpp_variable_ref_emission() {
        let cpp_code = r#"
namespace demo {

class GraphTraversal {
public:
    static int reach();
    static int limit;
};

class Sample {
public:
    int bar;
    static const int visibility_unknown = 3;

    int evaluate(int seed, int unused_param) {
        int local = seed;                    // declaration name, seed read
        int dead_local = 1;                  // declaration name, never read
        local = 7;                           // plain write target
        local += seed;                       // compound assignment -> read local
        int got = GraphTraversal::reach();   // GraphTraversal scope receiver -> read
        int lim = GraphTraversal::limit;     // limit static-member value -> read
        this->bar = seed;                    // bar owned by MemberAccess arm
        Sample s{seed};
        s.bar = local;                       // s member-write receiver -> read
        return local + lim + got + visibility_unknown;
    }
};

// GhostToken appears only in this comment and must never be an identifier.
}
"#;

        let mut parser = init_test_parser();
        let tree = parser.parse(cpp_code, None).unwrap();
        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor = CppExtractor::new(
            "test.cpp".to_string(),
            cpp_code.to_string(),
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
            "seed",               // initializer / RHS / argument value reads
            "local",              // compound-assignment target + bare return read
            "GraphTraversal",     // static-access scope receiver (`X` in `X::Y`)
            "limit",              // qualified static-member value read
            "s",                  // member-write receiver `s.bar = ...`
            "got",                // bare return read
            "lim",                // bare return read
            "visibility_unknown", // bare return read
        ] {
            assert!(
                var_refs.contains(&expected),
                "expected C++ variable_ref for {expected}; got {var_refs:?}"
            );
        }

        // --- Negative cases (rules 2/3/4/5) ---
        for forbidden in [
            "dead_local",   // declaration name only
            "unused_param", // parameter name only
            "evaluate",     // method declaration name
            "reach",        // call callee, owned by the Call arm
            "bar",          // accessed member, owned by the MemberAccess arm
            "Sample",       // type usage, owned by the TypeUsage arm
            "demo",         // namespace definition name
            "GhostToken",   // comment-only mention
        ] {
            assert!(
                !var_refs.contains(&forbidden),
                "{forbidden} must NOT be a C++ variable_ref; got {var_refs:?}"
            );
        }

        // Receiver + call coexist: GraphTraversal::reach() must still yield a Call.
        assert!(
            identifiers
                .iter()
                .any(|id| id.kind == IdentifierKind::Call && id.name.ends_with("reach")),
            "GraphTraversal::reach() must still yield a Call identifier"
        );

        // GhostToken must not appear as ANY identifier kind.
        assert!(
            !identifiers.iter().any(|id| id.name == "GhostToken"),
            "comment-only GhostToken must not be extracted at all"
        );
    }

    #[test]
    fn test_cpp_variable_access_facts() {
        let code = r#"
struct Box { int value; };
int call(int value) { return value; }
int& ref_at(int value);
int run(Box *box, int seed, int *values) {
    int plain = seed;
    plain = call(seed);
    int compound = seed;
    compound += 1;
    compound++;
    box->value = compound;
    values[seed] = compound;
    ref_at(seed) = 1;
    auto [captured, other] = std::pair{seed, 2};
    return call(captured);
}
"#;
        let mut parser = init_test_parser();
        let tree = parser.parse(code, None).unwrap();
        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor =
            CppExtractor::new("test.cpp".to_string(), code.to_string(), &workspace_root);
        let symbols = extractor.extract_symbols(&tree);
        let identifiers = extractor.extract_identifiers(&tree, &symbols);
        let plain_start = code.find("plain = call").unwrap() as u32;
        let compound_start = code.find("compound +=").unwrap() as u32;
        let increment_start = code.find("compound++").unwrap() as u32;
        let member_start = code.find("box->value =").unwrap() as u32 + 5;
        let member_receiver_start = code.find("box->value =").unwrap() as u32;
        let array_start = code.find("values[seed]").unwrap() as u32;
        let index_start = array_start + 7;
        let argument_start = code.find("call(seed)").unwrap() as u32 + 5;
        let pattern_use_start = code.find("call(captured)").unwrap() as u32 + 5;
        let ref_target_argument_start =
            code.find("ref_at(seed)").unwrap() as u32 + "ref_at(".len() as u32;
        let ref_target_callee_start = code.find("ref_at(seed)").unwrap() as u32;
        let access = |start: u32, name: &str, kind: IdentifierKind| {
            identifiers
                .iter()
                .find(|id| id.start_byte == start && id.name == name && id.kind == kind)
                .and_then(|id| id.metadata.as_ref()?.get("access")?.as_str())
                .map(str::to_owned)
        };
        assert_eq!(
            access(plain_start, "plain", IdentifierKind::VariableRef).as_deref(),
            Some("write")
        );
        assert_eq!(
            access(compound_start, "compound", IdentifierKind::VariableRef).as_deref(),
            Some("read_write")
        );
        assert_eq!(
            access(increment_start, "compound", IdentifierKind::VariableRef).as_deref(),
            Some("write")
        );
        assert_eq!(
            access(member_start, "value", IdentifierKind::MemberAccess).as_deref(),
            Some("write")
        );
        assert_eq!(
            access(member_receiver_start, "box", IdentifierKind::VariableRef).as_deref(),
            Some("read")
        );
        assert_eq!(
            access(array_start, "values", IdentifierKind::VariableRef).as_deref(),
            Some("read")
        );
        assert_eq!(
            access(index_start, "seed", IdentifierKind::VariableRef).as_deref(),
            Some("read")
        );
        assert_eq!(
            access(argument_start, "seed", IdentifierKind::VariableRef).as_deref(),
            Some("read")
        );
        assert_eq!(
            access(
                ref_target_argument_start,
                "seed",
                IdentifierKind::VariableRef
            )
            .as_deref(),
            Some("read")
        );
        assert!(identifiers.iter().any(|id| {
            id.start_byte == ref_target_callee_start
                && id.name == "ref_at"
                && id.kind == IdentifierKind::Call
        }));
        assert!(!identifiers.iter().any(|id| {
            id.start_byte == ref_target_callee_start
                && id.name == "ref_at"
                && id.kind == IdentifierKind::VariableRef
        }));
        assert_eq!(
            access(pattern_use_start, "captured", IdentifierKind::VariableRef).as_deref(),
            Some("read")
        );
        let pattern_start = code.find("captured, other").unwrap() as u32;
        for (name, start) in [
            ("captured", pattern_start),
            ("other", pattern_start + "captured, ".len() as u32),
        ] {
            let pattern_identifier = identifiers.iter().find(|id| {
                id.start_byte == start && id.name == name && id.kind == IdentifierKind::VariableRef
            });
            let pattern_symbol = symbols.iter().any(|symbol| {
                symbol.name == name
                    && symbol.start_byte <= start
                    && symbol.end_byte >= start + name.len() as u32
            });
            let pattern_identifier_is_write = pattern_identifier.is_some_and(|id| {
                id.metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("access"))
                    .and_then(|access| access.as_str())
                    == Some("write")
            });
            assert!(
                pattern_identifier_is_write || pattern_symbol,
                "missing binding fact for {name}"
            );
            assert!(
                !(pattern_identifier.is_some() && pattern_symbol),
                "duplicate binding fact for {name}"
            );
        }
        let call_start = code.find("call(seed)").unwrap() as u32;
        assert_eq!(access(call_start, "call", IdentifierKind::Call), None);
        let type_start = code.find("Box *box").unwrap() as u32;
        assert_eq!(access(type_start, "Box", IdentifierKind::TypeUsage), None);
    }

    #[test]
    fn test_cpp_array_bounds_are_reads_inside_initialized_declarations() {
        let code = r#"
constexpr int kSize = 3;
constexpr int size_for(int value) { return value; }
int run(int seed) {
    int arr[kSize] = {seed};
    int matrix[size_for(kSize)][kSize] = {};
    int direct = seed;
    direct = seed;
    return arr[0] + matrix[0][0] + direct;
}
"#;
        let mut parser = init_test_parser();
        let tree = parser.parse(code, None).unwrap();
        assert!(!tree.root_node().has_error());
        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor =
            CppExtractor::new("test.cpp".to_string(), code.to_string(), &workspace_root);
        let symbols = extractor.extract_symbols(&tree);
        let identifiers = extractor.extract_identifiers(&tree, &symbols);
        let access = |start: u32, name: &str, kind: IdentifierKind| {
            identifiers
                .iter()
                .find(|id| id.start_byte == start && id.name == name && id.kind == kind)
                .and_then(|id| id.metadata.as_ref()?.get("access")?.as_str())
        };
        let arr_start = code.find("arr[kSize]").unwrap() as u32;
        let first_size_start = arr_start + "arr[".len() as u32;
        let initializer_start = code.find("{seed}").unwrap() as u32 + 1;
        let size_call_start = code.find("size_for(kSize)").unwrap() as u32;
        let size_argument_start = size_call_start + "size_for(".len() as u32;
        let second_size_start = code.find("][kSize]").unwrap() as u32 + 2;
        let direct_write_start = code.rfind("direct = seed").unwrap() as u32;
        assert_eq!(
            access(first_size_start, "kSize", IdentifierKind::VariableRef),
            Some("read")
        );
        assert_eq!(
            access(initializer_start, "seed", IdentifierKind::VariableRef),
            Some("read")
        );
        assert_eq!(
            access(size_argument_start, "kSize", IdentifierKind::VariableRef),
            Some("read")
        );
        assert_eq!(
            access(second_size_start, "kSize", IdentifierKind::VariableRef),
            Some("read")
        );
        assert_eq!(
            access(direct_write_start, "direct", IdentifierKind::VariableRef),
            Some("write")
        );
        assert!(identifiers.iter().any(|id| {
            id.start_byte == size_call_start
                && id.name == "size_for"
                && id.kind == IdentifierKind::Call
        }));
        assert!(!identifiers.iter().any(|id| {
            id.start_byte == arr_start && id.name == "arr" && id.kind == IdentifierKind::VariableRef
        }));
        let matrix_start = code.find("matrix[").unwrap() as u32;
        assert!(!identifiers.iter().any(|id| {
            id.start_byte == matrix_start
                && id.name == "matrix"
                && id.kind == IdentifierKind::VariableRef
        }));
    }

    #[test]
    fn shadowed_bindings_keep_exact_access_and_non_access_facts() {
        let code = r#"
struct Marker { int payload; };
void consume(int value) {}
void update(Marker marker, int count) {
    consume(count);
    if (1) {
        int count = 0;
        count += 1;
        consume(count);
    }
    consume(count);
}
"#;
        let mut parser = init_test_parser();
        let tree = parser.parse(code, None).unwrap();
        assert!(!tree.root_node().has_error());
        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor =
            CppExtractor::new("test.cpp".to_string(), code.to_string(), &workspace_root);
        let symbols = extractor.extract_symbols(&tree);
        let identifiers = extractor.extract_identifiers(&tree, &symbols);

        let call_name = "consume";
        let call_expression = format!("{call_name}(count)");
        let call_starts: Vec<usize> = code
            .match_indices(&call_expression)
            .map(|(start, _)| start)
            .collect();
        assert_eq!(call_starts.len(), 3);
        let calls: Vec<_> = identifiers
            .iter()
            .filter(|identifier| {
                identifier.name == call_name && identifier.kind == IdentifierKind::Call
            })
            .collect();
        assert_eq!(calls.len(), call_starts.len());
        assert!(calls.iter().all(|identifier| {
            identifier
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("access"))
                .is_none()
        }));

        let outer_before_start = call_starts[0] + call_name.len() + 1;
        let inner_read_start = call_starts[1] + call_name.len() + 1;
        let outer_after_start = call_starts[2] + call_name.len() + 1;
        let compound_start = code.find("count += 1").unwrap();
        let access_at = |start: usize| {
            identifiers
                .iter()
                .find(|identifier| {
                    identifier.name == "count"
                        && identifier.kind == IdentifierKind::VariableRef
                        && identifier.start_byte as usize == start
                })
                .and_then(|identifier| identifier.metadata.as_ref())
                .and_then(|metadata| metadata.get("access"))
                .and_then(serde_json::Value::as_str)
        };
        assert_eq!(access_at(outer_before_start), Some("read"));
        assert_eq!(access_at(compound_start), Some("read_write"));
        assert_eq!(access_at(inner_read_start), Some("read"));
        assert_eq!(access_at(outer_after_start), Some("read"));

        let owner_id = identifiers
            .iter()
            .find(|identifier| {
                identifier.name == "count"
                    && identifier.kind == IdentifierKind::VariableRef
                    && identifier.start_byte as usize == outer_before_start
            })
            .and_then(|identifier| identifier.containing_symbol_id.as_deref())
            .expect("outer variable reference should have a containing function");
        for start in [compound_start, inner_read_start, outer_after_start] {
            let identifier = identifiers
                .iter()
                .find(|identifier| {
                    identifier.name == "count"
                        && identifier.kind == IdentifierKind::VariableRef
                        && identifier.start_byte as usize == start
                })
                .expect("shadowed variable occurrence");
            assert_eq!(identifier.containing_symbol_id.as_deref(), Some(owner_id));
        }
        let owner = symbols
            .iter()
            .find(|symbol| symbol.id.as_str() == owner_id)
            .expect("containing function should be extracted");
        assert_eq!(owner.name, "update");

        let type_usages: Vec<_> = identifiers
            .iter()
            .filter(|identifier| identifier.kind == IdentifierKind::TypeUsage)
            .collect();
        assert!(!type_usages.is_empty());
        let type_start = code.find("Marker marker").unwrap();
        let type_usage = type_usages
            .iter()
            .find(|identifier| {
                identifier.name == "Marker" && identifier.start_byte as usize == type_start
            })
            .expect("Marker type usage at exact source offset");
        assert!(
            type_usage
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("access"))
                .is_none()
        );
    }
}
