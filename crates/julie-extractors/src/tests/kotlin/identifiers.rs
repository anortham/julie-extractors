//! Kotlin identifier extraction tests — type_usage
//!
//! Validates that type annotations in Kotlin produce TypeUsage identifiers
//! for centrality scoring. Same bug pattern as TypeScript, Scala, GDScript, Zig.

use crate::base::IdentifierKind;
use crate::kotlin::KotlinExtractor;
use std::path::PathBuf;
use tree_sitter::Parser;

fn init_test_parser() -> Parser {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_kotlin_ng::LANGUAGE.into())
        .expect("Error loading Kotlin grammar");
    parser
}

fn extract_identifiers(code: &str) -> Vec<crate::base::Identifier> {
    let mut parser = init_test_parser();
    let tree = parser.parse(code, None).unwrap();
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = KotlinExtractor::new(
        "kotlin".to_string(),
        "test.kt".to_string(),
        code.to_string(),
        &workspace_root,
    );
    let symbols = extractor.extract_symbols(&tree);
    extractor.extract_identifiers(&tree, &symbols)
}

#[test]
fn test_kotlin_type_usage_identifiers() {
    // Type annotations in Kotlin should produce TypeUsage identifiers.
    // These drive centrality scoring — without them, heavily-referenced types
    // like JsonAdapter get centrality 0.00 despite 99 references.
    let code = r#"
interface UserService {
    fun getUser(id: Long): User
}

class AuthController(service: UserService) {
    fun login(request: LoginRequest): AuthResult {
        val config: AppConfig = loadConfig()
    }
}

typealias Handler = Request
"#;

    let identifiers = extract_identifiers(code);
    let type_usages: Vec<_> = identifiers
        .iter()
        .filter(|id| id.kind == IdentifierKind::TypeUsage)
        .collect();

    assert!(
        !type_usages.is_empty(),
        "Kotlin type annotations must produce TypeUsage identifiers for centrality scoring"
    );

    let type_names: Vec<&str> = type_usages.iter().map(|id| id.name.as_str()).collect();

    // Core type references that MUST be extracted
    assert!(
        type_names.contains(&"User"),
        "Return type 'User' must be extracted. Got: {:?}",
        type_names
    );
    assert!(
        type_names.contains(&"UserService"),
        "Constructor param type 'UserService' must be extracted. Got: {:?}",
        type_names
    );
    assert!(
        type_names.contains(&"LoginRequest"),
        "Method param type 'LoginRequest' must be extracted. Got: {:?}",
        type_names
    );
    assert!(
        type_names.contains(&"AuthResult"),
        "Return type 'AuthResult' must be extracted. Got: {:?}",
        type_names
    );
    assert!(
        type_names.contains(&"AppConfig"),
        "Val type annotation 'AppConfig' must be extracted. Got: {:?}",
        type_names
    );
    assert!(
        type_names.contains(&"Request"),
        "Type alias target 'Request' must be extracted. Got: {:?}",
        type_names
    );

    // Declaration names must NOT appear as TypeUsage
    assert!(
        !type_names.contains(&"AuthController"),
        "Class declaration name 'AuthController' must NOT be TypeUsage. Got: {:?}",
        type_names
    );

    // Kotlin builtins should be filtered
    assert!(
        !type_names.contains(&"Long"),
        "Builtin 'Long' must NOT be a TypeUsage identifier. Got: {:?}",
        type_names
    );
}

#[test]
fn test_kotlin_type_usage_skips_noise_types() {
    // Kotlin primitive/wrapper types and single-letter generics should NOT
    // produce TypeUsage identifiers — they pollute centrality with noise.
    let code = r#"
fun greet(name: String, age: Int): Boolean {
    return true
}
val x: Any = null
val items: List<T> = emptyList()
"#;

    let identifiers = extract_identifiers(code);
    let type_usages: Vec<_> = identifiers
        .iter()
        .filter(|id| id.kind == IdentifierKind::TypeUsage)
        .collect();
    let type_names: Vec<&str> = type_usages.iter().map(|id| id.name.as_str()).collect();

    assert!(
        !type_names.contains(&"String"),
        "Builtin 'String' must NOT be a TypeUsage identifier"
    );
    assert!(
        !type_names.contains(&"Int"),
        "Builtin 'Int' must NOT be a TypeUsage identifier"
    );
    assert!(
        !type_names.contains(&"Boolean"),
        "Builtin 'Boolean' must NOT be a TypeUsage identifier"
    );
    assert!(
        !type_names.contains(&"Any"),
        "Builtin 'Any' must NOT be a TypeUsage identifier"
    );
    // Single-letter generic
    assert!(
        !type_names.contains(&"T"),
        "Single-letter generic 'T' must NOT be a TypeUsage identifier"
    );

    // But List should be extracted (it's a real type, not a primitive)
    assert!(
        type_names.contains(&"List"),
        "Non-primitive type 'List' should be extracted as TypeUsage. Got: {:?}",
        type_names
    );
}

#[test]
fn test_kotlin_variable_ref_emission() {
    let code = r#"
object GraphTraversal {
    fun reach(): Int = 0
}

enum class Level { LOW, HIGH }

typealias Alias = List<Int>

class Sample(val bar: Int) {
    var count = 0
    val visibilityUnknown = 3

    // GhostToken appears only in this comment and must never be an identifier.
    fun evaluate(seed: Int, unusedParam: Int): Int {
        count += 1                       // compound assignment -> read count
        var x = 5                        // declaration name, no ref
        x = 7                            // plain write target
        val total = seed                 // initializer -> read seed
        val g = GraphTraversal.reach()   // receiver -> read; reach -> call
        val f = Sample(bar = seed)       // named-arg label names a parameter -> NOT a read
        val l = Level.LOW                // Level receiver -> read; LOW -> member access
        val s = "brace ${limit + 1}"     // interpolation expression -> read limit
        listOf(count).forEach { println(it) } // `it` soft keyword -> NOT a read
        val r = lo until cap               // infix callee -> NOT a read; operands are
        return if (total > 0) total else visibilityUnknown
    }
}
"#;

    let mut parser = init_test_parser();
    let tree = parser.parse(code, None).unwrap();
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = KotlinExtractor::new(
        "kotlin".to_string(),
        "test.kt".to_string(),
        code.to_string(),
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
        "GraphTraversal",    // object receiver
        "lo",                // infix left operand
        "cap",               // infix right operand
        "count",             // compound-assignment target + argument value
        "x",                 // plain assignment target
        "seed",              // initializer + named-argument VALUE read
        "total",             // if condition + branch reads
        "visibilityUnknown", // bare read in else branch
        "Level",             // enum receiver
        "limit",             // string interpolation expression read
    ] {
        assert!(
            var_refs.contains(&expected),
            "expected variable_ref for {expected}; got {var_refs:?}"
        );
    }

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
        "Sample",      // class declaration name + call callee
        "evaluate",    // function declaration name
        "bar",         // named-argument label + class parameter name
        "LOW",         // enum entry declaration + member access (owned elsewhere)
        "Alias",       // typealias declaration name
        "it",          // implicit lambda parameter soft keyword
        "until",       // infix function callee
        "Int",         // builtin type (noise filter)
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

    let plain_write_start = code.find("x = 7").unwrap() as u32;
    let declaration_start = code.find("var x = 5").unwrap() as u32 + 4;
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
        .expect("evaluate function extracted");
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
fn test_kotlin_variable_access_facts() {
    let code = r#"
class Box { var value: Int = 0 }
fun call(value: Int) = value
fun run(box: Box, seed: Int, pair: Pair<Int, Int>, values: IntArray) {
    var plain = seed
    plain = call(seed)
    var compound = seed
    compound += 1
    compound++
    box.value = compound
    values[seed] = compound
    val (captured, other) = pair
    call(captured)
}
"#;
    let mut parser = init_test_parser();
    let tree = parser.parse(code, None).unwrap();
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = KotlinExtractor::new(
        "kotlin".to_string(),
        "test.kt".to_string(),
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
    let type_start = code.find("box: Box").unwrap() as u32 + 5;
    assert_eq!(access(type_start, "Box", IdentifierKind::TypeUsage), None);
}
