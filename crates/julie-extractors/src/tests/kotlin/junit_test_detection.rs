//! Kotlin JUnit 4/5 annotation-driven test detection.
//!
//! Mirrors the Java lifecycle/container coverage for Kotlin: `@Test` methods
//! carry `is_test`, `@BeforeEach`/`@AfterEach`/`@BeforeAll`/`@AfterAll` carry
//! `test_lifecycle`, and a class holding annotated test members (or a `@Nested`
//! class) is marked `test_container`. Kotest/Spek call-DSL coverage lives in
//! `test_detection.rs`.

use crate::base::SymbolKind;
use crate::kotlin::KotlinExtractor;
use std::path::PathBuf;
use tree_sitter::Parser;

fn symbols(code: &str, file: &str) -> Vec<crate::base::Symbol> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_kotlin_ng::LANGUAGE.into())
        .expect("load Kotlin grammar");
    let tree = parser.parse(code, None).expect("parse Kotlin");
    let mut ext = KotlinExtractor::new(
        "kotlin".to_string(),
        file.to_string(),
        code.to_string(),
        &PathBuf::from("/test/workspace"),
    );
    ext.extract_symbols(&tree)
}

fn role(symbol: &crate::base::Symbol, key: &str) -> bool {
    symbol
        .metadata
        .as_ref()
        .and_then(|m| m.get(key))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn annotation_keys(symbol: &crate::base::Symbol) -> Vec<String> {
    symbol
        .annotations
        .iter()
        .map(|a| a.annotation_key.clone())
        .collect()
}

fn named<'a>(syms: &'a [crate::base::Symbol], name: &str) -> &'a crate::base::Symbol {
    syms.iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("expected symbol {name}, got {syms:?}"))
}

#[test]
fn junit5_test_method_has_annotation_and_is_test() {
    let code = r#"
class CalculatorTest {
    @Test
    fun shouldAdd() {
    }
}
"#;
    let syms = symbols(code, "src/test/kotlin/CalculatorTest.kt");
    let method = named(&syms, "shouldAdd");
    assert!(
        annotation_keys(method).iter().any(|k| k == "test"),
        "@Test must yield annotation_key 'test', got {:?}",
        annotation_keys(method)
    );
    assert!(
        role(method, "is_test"),
        "@Test method must be flagged is_test"
    );
    assert!(
        !role(method, "test_lifecycle"),
        "a plain @Test case is not a lifecycle hook"
    );
}

#[test]
fn junit5_lifecycle_methods_emit_test_lifecycle() {
    let code = r#"
class LifecycleTest {
    @BeforeEach
    fun setUp() {
    }

    @AfterEach
    fun tearDown() {
    }

    @BeforeAll
    fun setUpAll() {
    }

    @AfterAll
    fun tearDownAll() {
    }

    @Test
    fun caseOne() {
    }

    fun helper() {
    }
}
"#;
    let syms = symbols(code, "src/test/kotlin/LifecycleTest.kt");

    for hook in ["setUp", "tearDown", "setUpAll", "tearDownAll"] {
        let sym = named(&syms, hook);
        assert!(role(sym, "is_test"), "{hook} must be is_test");
        assert!(role(sym, "test_lifecycle"), "{hook} must be test_lifecycle");
    }

    let case = named(&syms, "caseOne");
    assert!(role(case, "is_test"));
    assert!(!role(case, "test_lifecycle"), "@Test case is not lifecycle");

    let helper = named(&syms, "helper");
    assert!(!role(helper, "is_test"));
    assert!(!role(helper, "test_lifecycle"));
}

#[test]
fn junit_test_members_and_nested_mark_classes_as_containers() {
    let code = r#"
class ManagedTestRoles {
    @Test
    fun junitCase() {
    }
}

@Nested
class WhenEmpty {
    @Test
    fun isEmpty() {
    }
}

class OrdinaryHelper {
    fun helper() {
    }
}
"#;
    let syms = symbols(code, "src/test/kotlin/ManagedTestRoles.kt");

    let managed = named(&syms, "ManagedTestRoles");
    assert_eq!(managed.kind, SymbolKind::Class);
    assert!(
        role(managed, "test_container"),
        "class with @Test members must be a test_container"
    );

    let when_empty = named(&syms, "WhenEmpty");
    assert!(
        role(when_empty, "test_container"),
        "@Nested class must be a test_container"
    );

    let ordinary = named(&syms, "OrdinaryHelper");
    assert!(
        !role(ordinary, "test_container"),
        "class without test members must not be a test_container"
    );
}

#[test]
fn outer_class_with_only_nested_test_class_is_marked_container() {
    let code = r#"
class OuterSuite {
    @Nested
    inner class WhenEmpty {
        @Test
        fun isEmpty() {
        }
    }
}

class PlainHelper {
    fun helper() {
    }
}
"#;
    let syms = symbols(code, "src/test/kotlin/OuterSuite.kt");

    assert!(role(named(&syms, "WhenEmpty"), "test_container"));
    assert!(
        role(named(&syms, "OuterSuite"), "test_container"),
        "outer class of a @Nested test class must be a test_container"
    );
    assert!(!role(named(&syms, "PlainHelper"), "test_container"));
}

#[test]
fn ordinary_class_and_methods_untouched() {
    let code = r#"
class OrderService {
    fun place(order: String) {
    }

    fun cancel(id: Int) {
    }
}
"#;
    let syms = symbols(code, "src/main/kotlin/OrderService.kt");

    let service = named(&syms, "OrderService");
    assert!(!role(service, "test_container"));

    for method in ["place", "cancel"] {
        let sym = named(&syms, method);
        assert!(!role(sym, "is_test"), "{method} must not be is_test");
        assert!(
            !role(sym, "test_lifecycle"),
            "{method} must not be test_lifecycle"
        );
    }
}
