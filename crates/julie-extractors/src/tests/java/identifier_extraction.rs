// Java Identifier Extraction Tests (TDD RED phase)
//
// These tests validate the extract_identifiers() functionality which extracts:
// - Function/method calls (method_invocation)
// - Member/field access (field_access)
// - Proper containing symbol tracking (file-scoped)
//
// Following the Rust extractor reference implementation pattern

#![allow(unused_imports)]

use crate::base::{IdentifierKind, SymbolKind};
use crate::java::JavaExtractor;
use crate::tests::helpers::init_parser;

#[cfg(test)]
mod identifier_extraction_tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_extract_function_calls() {
        let workspace_root = PathBuf::from("/tmp/test");
        let java_code = r#"
public class Calculator {
    public int add(int a, int b) {
        return a + b;
    }

    public int calculate() {
        int result = add(5, 3);           // Method call to add
        System.out.println(result);       // Method call to println
        return result;
    }
}
"#;

        let tree = init_parser(java_code, "java");

        let mut extractor = JavaExtractor::new(
            "java".to_string(),
            "test.java".to_string(),
            java_code.to_string(),
            &workspace_root,
        );

        // Extract symbols first
        let symbols = extractor.extract_symbols(&tree);

        // NOW extract identifiers (this will FAIL until we implement it)
        let identifiers = extractor.extract_identifiers(&tree, &symbols);

        // Verify we found the method calls
        let add_call = identifiers.iter().find(|id| id.name == "add");
        assert!(
            add_call.is_some(),
            "Should extract 'add' method call identifier"
        );
        let add_call = add_call.unwrap();
        assert_eq!(add_call.kind, IdentifierKind::Call);

        let println_call = identifiers.iter().find(|id| id.name == "println");
        assert!(
            println_call.is_some(),
            "Should extract 'println' method call identifier"
        );
        let println_call = println_call.unwrap();
        assert_eq!(println_call.kind, IdentifierKind::Call);

        // Verify containing symbol is set correctly (should be inside calculate method)
        assert!(
            add_call.containing_symbol_id.is_some(),
            "Method call should have containing symbol"
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
    fn test_extract_member_access() {
        let workspace_root = PathBuf::from("/tmp/test");
        let java_code = r#"
public class User {
    public String name;
    public String email;

    public void printInfo() {
        System.out.println(this.name);    // Field access: this.name
        String userEmail = this.email;    // Field access: this.email
    }
}
"#;

        let tree = init_parser(java_code, "java");

        let mut extractor = JavaExtractor::new(
            "java".to_string(),
            "test.java".to_string(),
            java_code.to_string(),
            &workspace_root,
        );

        let symbols = extractor.extract_symbols(&tree);
        let identifiers = extractor.extract_identifiers(&tree, &symbols);

        // Verify we found field access identifiers
        let name_access = identifiers
            .iter()
            .filter(|id| id.name == "name" && id.kind == IdentifierKind::MemberAccess)
            .count();
        assert!(
            name_access > 0,
            "Should extract 'name' field access identifier"
        );

        let email_access = identifiers
            .iter()
            .filter(|id| id.name == "email" && id.kind == IdentifierKind::MemberAccess)
            .count();
        assert!(
            email_access > 0,
            "Should extract 'email' field access identifier"
        );
    }

    #[test]
    fn test_file_scoped_containing_symbol() {
        let workspace_root = PathBuf::from("/tmp/test");
        // This test ensures we ONLY match symbols from the SAME FILE
        // Critical bug fix from Rust implementation (line 1311-1318 in rust.rs)
        let java_code = r#"
public class Service {
    public void process() {
        helper();              // Call to helper in same file
    }

    private void helper() {
        // Helper method
    }
}
"#;

        let tree = init_parser(java_code, "java");

        let mut extractor = JavaExtractor::new(
            "java".to_string(),
            "test.java".to_string(),
            java_code.to_string(),
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
        let workspace_root = PathBuf::from("/tmp/test");
        let java_code = r#"
public class DataService {
    public void execute() {
        double result = user.account.balance;      // Chained field access
        String name = customer.profile.fullName;   // Chained field access
    }
}
"#;

        let tree = init_parser(java_code, "java");

        let mut extractor = JavaExtractor::new(
            "java".to_string(),
            "test.java".to_string(),
            java_code.to_string(),
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
            "Should extract 'balance' from chained field access"
        );

        let fullname_access = identifiers
            .iter()
            .find(|id| id.name == "fullName" && id.kind == IdentifierKind::MemberAccess);
        assert!(
            fullname_access.is_some(),
            "Should extract 'fullName' from chained field access"
        );
    }

    #[test]
    fn test_no_duplicate_identifiers() {
        let workspace_root = PathBuf::from("/tmp/test");
        let java_code = r#"
public class Test {
    public void run() {
        process();
        process();  // Same call twice
    }

    private void process() {
    }
}
"#;

        let tree = init_parser(java_code, "java");

        let mut extractor = JavaExtractor::new(
            "java".to_string(),
            "test.java".to_string(),
            java_code.to_string(),
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
    fn test_java_type_usage_identifiers() {
        // Java type annotations should produce TypeUsage identifiers.
        // These drive centrality scoring for classes like Gson, TypeAdapter, etc.
        let workspace_root = PathBuf::from("/tmp/test");
        let java_code = r#"
public class GsonService {
    private Gson gson;
    private TypeAdapter<JsonElement> adapter;

    public GsonService(Gson gson, TypeAdapter<JsonElement> adapter) {
        this.gson = gson;
        this.adapter = adapter;
    }

    public JsonElement parse(String input) {
        return gson.fromJson(input, JsonElement.class);
    }

    public List<UserProfile> getProfiles() {
        return null;
    }
}
"#;

        let tree = init_parser(java_code, "java");

        let mut extractor = JavaExtractor::new(
            "java".to_string(),
            "test.java".to_string(),
            java_code.to_string(),
            &workspace_root,
        );

        let symbols = extractor.extract_symbols(&tree);
        let identifiers = extractor.extract_identifiers(&tree, &symbols);

        let type_usages: Vec<_> = identifiers
            .iter()
            .filter(|id| id.kind == IdentifierKind::TypeUsage)
            .collect();

        let type_names: Vec<&str> = type_usages.iter().map(|id| id.name.as_str()).collect();

        // Core type references that MUST be extracted
        assert!(
            type_names.contains(&"Gson"),
            "Field/param type 'Gson' must be extracted. Got: {:?}",
            type_names
        );
        assert!(
            type_names.contains(&"TypeAdapter"),
            "Generic type 'TypeAdapter' must be extracted. Got: {:?}",
            type_names
        );
        assert!(
            type_names.contains(&"JsonElement"),
            "Generic arg 'JsonElement' must be extracted. Got: {:?}",
            type_names
        );
        assert!(
            type_names.contains(&"UserProfile"),
            "Generic arg 'UserProfile' must be extracted. Got: {:?}",
            type_names
        );
        // String and List are common standard library types but should NOT be filtered
        assert!(
            type_names.contains(&"String"),
            "Standard library type 'String' must be extracted (not filtered). Got: {:?}",
            type_names
        );
        assert!(
            type_names.contains(&"List"),
            "Standard library type 'List' must be extracted (not filtered). Got: {:?}",
            type_names
        );

        // Class declaration name should NOT be extracted as TypeUsage
        assert!(
            !type_names.contains(&"GsonService"),
            "Class declaration name 'GsonService' must NOT be extracted as TypeUsage. Got: {:?}",
            type_names
        );
    }

    #[test]
    fn test_java_type_usage_skips_single_letter_generics() {
        // Single-letter generics (T, K, V, E) carry no cross-file signal
        let workspace_root = PathBuf::from("/tmp/test");
        let java_code = r#"
public class Container<T> {
    private T value;

    public <K, V> Map<K, V> toMap() {
        return null;
    }

    public <E extends Exception> void handle(E error) {
    }
}
"#;

        let tree = init_parser(java_code, "java");

        let mut extractor = JavaExtractor::new(
            "java".to_string(),
            "test.java".to_string(),
            java_code.to_string(),
            &workspace_root,
        );

        let symbols = extractor.extract_symbols(&tree);
        let identifiers = extractor.extract_identifiers(&tree, &symbols);

        let type_usages: Vec<_> = identifiers
            .iter()
            .filter(|id| id.kind == IdentifierKind::TypeUsage)
            .collect();

        let type_names: Vec<&str> = type_usages.iter().map(|id| id.name.as_str()).collect();

        // Single-letter generics should be filtered
        assert!(
            !type_names.contains(&"T"),
            "Single-letter generic 'T' must be filtered. Got: {:?}",
            type_names
        );
        assert!(
            !type_names.contains(&"K"),
            "Single-letter generic 'K' must be filtered. Got: {:?}",
            type_names
        );
        assert!(
            !type_names.contains(&"V"),
            "Single-letter generic 'V' must be filtered. Got: {:?}",
            type_names
        );
        assert!(
            !type_names.contains(&"E"),
            "Single-letter generic 'E' must be filtered. Got: {:?}",
            type_names
        );

        // But Map and Exception should be extracted
        assert!(
            type_names.contains(&"Map"),
            "Type 'Map' must be extracted. Got: {:?}",
            type_names
        );
        assert!(
            type_names.contains(&"Exception"),
            "Type 'Exception' must be extracted. Got: {:?}",
            type_names
        );

        // Container (declaration name) should NOT be extracted
        assert!(
            !type_names.contains(&"Container"),
            "Class declaration name 'Container' must NOT be extracted. Got: {:?}",
            type_names
        );
    }

    #[test]
    fn test_java_type_usage_excludes_declaration_names() {
        // Declaration-position type_identifier nodes must NOT be extracted as TypeUsage
        let workspace_root = PathBuf::from("/tmp/test");
        let java_code = r#"
public class MyClass {
    private SomeType field;
}

public interface MyInterface {
    OtherType process();
}

public enum MyEnum {
    VALUE_ONE
}
"#;

        let tree = init_parser(java_code, "java");

        let mut extractor = JavaExtractor::new(
            "java".to_string(),
            "test.java".to_string(),
            java_code.to_string(),
            &workspace_root,
        );

        let symbols = extractor.extract_symbols(&tree);
        let identifiers = extractor.extract_identifiers(&tree, &symbols);

        let type_usages: Vec<_> = identifiers
            .iter()
            .filter(|id| id.kind == IdentifierKind::TypeUsage)
            .collect();

        let type_names: Vec<&str> = type_usages.iter().map(|id| id.name.as_str()).collect();

        // Declaration names must NOT appear
        assert!(
            !type_names.contains(&"MyClass"),
            "Class declaration 'MyClass' must not be TypeUsage. Got: {:?}",
            type_names
        );
        assert!(
            !type_names.contains(&"MyInterface"),
            "Interface declaration 'MyInterface' must not be TypeUsage. Got: {:?}",
            type_names
        );
        assert!(
            !type_names.contains(&"MyEnum"),
            "Enum declaration 'MyEnum' must not be TypeUsage. Got: {:?}",
            type_names
        );

        // Reference-position types MUST appear
        assert!(
            type_names.contains(&"SomeType"),
            "Field type 'SomeType' must be extracted. Got: {:?}",
            type_names
        );
        assert!(
            type_names.contains(&"OtherType"),
            "Return type 'OtherType' must be extracted. Got: {:?}",
            type_names
        );
    }

    #[test]
    fn test_java_variable_ref_emission() {
        let java_code = r#"
public class GraphTraversal {
    public static int reach() { return 0; }
}

public class Sample {
    private int count;
    private static final int VISIBILITY_UNKNOWN = 3;

    // GhostToken appears only in this comment and must never be an identifier.
    public int evaluate(int seed, int unusedParam) {
        count += 1;                       // compound assignment -> read count
        int x = 5;                        // declaration name, no ref
        x = 7;                            // plain write target
        int total = seed;                 // initializer value -> read seed
        int g = GraphTraversal.reach();   // receiver -> read; reach -> call
        java.util.function.IntSupplier s = this::helper; // method reference -> call helper
        return total > 0 ? total : VISIBILITY_UNKNOWN;   // bare reads
    }

    private int helper() { return 1; }
}

@interface Foo { int baz(); }

class Decorated {
    @Foo(baz = 1)                         // annotation named arg -> read baz
    void m() {}
}
"#;

        let workspace_root = PathBuf::from("/tmp/test");
        let tree = init_parser(java_code, "java");
        let mut extractor = JavaExtractor::new(
            "java".to_string(),
            "test.java".to_string(),
            java_code.to_string(),
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
            "GraphTraversal",     // static-access receiver
            "count",              // compound-assignment target
            "x",                  // plain assignment target
            "seed",               // initializer value read
            "total",              // ternary condition + consequence
            "VISIBILITY_UNKNOWN", // bare read in ternary alternative
            "baz",                // annotation named argument (member reference)
        ] {
            assert!(
                var_refs.contains(&expected),
                "expected variable_ref for {expected}; got {var_refs:?}"
            );
        }

        assert!(
            identifiers
                .iter()
                .any(|id| id.name == "helper" && id.kind == IdentifierKind::Call),
            "a method-reference member is a call identifier"
        );
        assert!(!var_refs.contains(&"helper"));

        // Receiver + call coexist: GraphTraversal.reach()
        assert!(
            identifiers
                .iter()
                .any(|id| id.name == "reach" && id.kind == IdentifierKind::Call),
            "GraphTraversal.reach() must still yield a call named reach"
        );

        // --- Negative cases (rules 2/3/4/5) ---
        for forbidden in [
            "unusedParam", // parameter name only
            "GhostToken",  // comment-only mention
            "Sample",      // class declaration name
            "evaluate",    // method declaration name
            "Foo",         // annotation name (type-ish usage, not a value read)
            "m",           // method declaration name
            "int",         // builtin type keyword (never an identifier node)
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

        let plain_write_start = java_code.find("x = 7").unwrap() as u32;
        let declaration_start = java_code.find("int x = 5").unwrap() as u32 + 4;
        assert!(identifiers.iter().any(|id| {
            id.start_byte == plain_write_start
                && id.kind == IdentifierKind::VariableRef
                && id
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("access"))
                    .and_then(|access| access.as_str())
                    == Some("write")
        }));
        assert!(
            !identifiers
                .iter()
                .any(|id| id.start_byte == declaration_start)
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
    fn test_java_variable_access_facts() {
        let code = r#"
class Box { int value; }
class Sample {
    int call(int value) { return value; }
    void run(Box box, int seed, int[] values) {
        int plain = seed;
        plain = call(seed);
        int compound = seed;
        compound += 1;
        compound++;
        box.value = compound;
        values[seed] = compound;
        if (seed instanceof Integer captured) { call(captured); }
    }
}
"#;
        let workspace_root = PathBuf::from("/tmp/test");
        let tree = init_parser(code, "java");
        let mut extractor = JavaExtractor::new(
            "java".to_string(),
            "test.java".to_string(),
            code.to_string(),
            &workspace_root,
        );
        let symbols = extractor.extract_symbols(&tree);
        let identifiers = extractor.extract_identifiers(&tree, &symbols);
        let plain_start = code.find("plain = call").unwrap() as u32;
        let compound_start = code.find("compound +=").unwrap() as u32;
        let increment_start = code.find("compound++").unwrap() as u32;
        let member_start = code.find("box.value =").unwrap() as u32 + 4;
        let member_receiver_start = code.find("box.value =").unwrap() as u32;
        let array_start = code.find("values[seed]").unwrap() as u32;
        let index_start = array_start + 7;
        let argument_start = code.find("call(seed)").unwrap() as u32 + 5;
        let pattern_use_start = code.find("call(captured)").unwrap() as u32 + 5;
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
            identifiers
                .iter()
                .filter(|id| {
                    id.start_byte == member_start
                        && id.name == "value"
                        && id.kind == IdentifierKind::MemberAccess
                })
                .count(),
            1
        );
        assert!(!identifiers.iter().any(|id| {
            id.start_byte == member_start
                && id.name == "value"
                && id.kind == IdentifierKind::VariableRef
        }));
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
            access(pattern_use_start, "captured", IdentifierKind::VariableRef).as_deref(),
            Some("read")
        );
        let pattern_start = code.find("instanceof Integer captured").unwrap() as u32
            + "instanceof Integer ".len() as u32;
        let pattern_identifier = identifiers.iter().find(|id| {
            id.start_byte == pattern_start
                && id.name == "captured"
                && id.kind == IdentifierKind::VariableRef
        });
        let pattern_symbol = symbols.iter().any(|symbol| {
            symbol.name == "captured"
                && symbol.start_byte <= pattern_start
                && symbol.end_byte >= pattern_start + "captured".len() as u32
        });
        let pattern_identifier_is_write = pattern_identifier.is_some_and(|id| {
            id.metadata
                .as_ref()
                .and_then(|metadata| metadata.get("access"))
                .and_then(|access| access.as_str())
                == Some("write")
        });
        assert!(pattern_identifier_is_write || pattern_symbol);
        assert!(!(pattern_identifier.is_some() && pattern_symbol));
        let call_start = code.find("call(seed)").unwrap() as u32;
        assert_eq!(access(call_start, "call", IdentifierKind::Call), None);
        let type_start = code.find("Box box").unwrap() as u32;
        assert_eq!(access(type_start, "Box", IdentifierKind::TypeUsage), None);
        let declaration_start = code.find("int plain").unwrap() as u32 + 4;
        assert!(
            !identifiers
                .iter()
                .any(|id| id.start_byte == declaration_start)
        );
    }
}

#[cfg(test)]
mod fq_static_receiver_tests {
    use super::*;
    use crate::base::IdentifierKind;
    use crate::java::JavaExtractor;
    use crate::tests::helpers::init_parser;
    use std::path::PathBuf;

    /// Fix round 1 (adversarial-review finding): in a fully-qualified static
    /// METHOD call `com.acme.GraphTraversal.reach()`, the terminal class
    /// receiver `GraphTraversal` emitted NO identifier row (while `com` was a
    /// variable_ref and `acme` a member_access), so a helper class referenced
    /// ONLY via FQ static calls looked dead to code-kb's name-liveness. The
    /// terminal receiver of a qualified invocation chain must be name-visible.
    #[test]
    fn test_java_fq_static_method_call_receiver_is_name_visible() {
        let java_code = r#"
public class GraphTraversal {
    public static int reach() { return 0; }
}

public class T {
    void m() {
        com.acme.GraphTraversal.reach();      // FQ static method call -> GraphTraversal must be visible
        int x = com.acme.Config.MAX;          // FQ static field access (regression guard)
        GraphTraversal.reach();               // unqualified receiver (regression guard)
    }
}
"#;

        let workspace_root = PathBuf::from("/tmp/test");
        let tree = init_parser(java_code, "java");
        let mut extractor = JavaExtractor::new(
            "java".to_string(),
            "test.java".to_string(),
            java_code.to_string(),
            &workspace_root,
        );
        let symbols = extractor.extract_symbols(&tree);
        let identifiers = extractor.extract_identifiers(&tree, &symbols);

        let rows: Vec<(&str, IdentifierKind)> = identifiers
            .iter()
            .map(|id| (id.name.as_str(), id.kind.clone()))
            .collect();

        // THE defect: the FQ-call terminal receiver must produce a row. Two
        // GraphTraversal usages exist (FQ + unqualified); both must be visible.
        let graph_rows: Vec<&IdentifierKind> = rows
            .iter()
            .filter(|(n, _)| *n == "GraphTraversal")
            .map(|(_, k)| k)
            .collect();
        assert_eq!(
            graph_rows.len(),
            2,
            "both the FQ-chain terminal receiver and the unqualified receiver \
             must emit a row; got {rows:?}"
        );
        // Kind-honesty: the FQ terminal receiver is the `field` of a
        // field_access (like `Config` in the FQ field read) -> member_access;
        // the unqualified receiver stays a variable_ref.
        assert!(
            graph_rows
                .iter()
                .any(|k| **k == IdentifierKind::MemberAccess),
            "FQ terminal receiver should be a member_access; got {graph_rows:?}"
        );
        assert!(
            graph_rows
                .iter()
                .any(|k| **k == IdentifierKind::VariableRef),
            "unqualified receiver should stay a variable_ref; got {graph_rows:?}"
        );

        // Regression guards: FQ static FIELD access already worked.
        assert!(
            rows.contains(&("Config", IdentifierKind::MemberAccess)),
            "FQ static field access must keep emitting member_access Config; got {rows:?}"
        );
        // Both calls still emit the callee.
        assert_eq!(
            rows.iter()
                .filter(|(n, k)| *n == "reach" && *k == IdentifierKind::Call)
                .count(),
            2,
            "both reach() calls must remain Call rows; got {rows:?}"
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
    }

    #[test]
    fn shadowed_bindings_keep_exact_access_and_non_access_facts() {
        let code = r#"
class Marker {}
class Sample {
    int count;
    void consume(int value) {}
    void update(Marker marker) {
        consume(count);
        if (true) {
            int count = 0;
            count += 1;
            consume(count);
        }
        consume(count);
    }
}
"#;
        let tree = init_parser(code, "java");
        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor = JavaExtractor::new(
            "java".to_string(),
            "test.java".to_string(),
            code.to_string(),
            &workspace_root,
        );
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
