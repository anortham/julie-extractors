//! Python test-role emission: unittest/pytest cases, containers, lifecycle.
//!
//! - **pytest**: `python_functions = test*` and `python_classes = Test*` collect
//!   any `test`-prefixed callable in a test path; `@pytest.mark.parametrize`
//!   marks a parameterized case; `@pytest.fixture` marks a fixture factory.
//! - **unittest**: `TestLoader.testMethodPrefix` is `test`, so `testAddition` is
//!   a real case; `setUp`/`tearDown` and their class and module variants are
//!   lifecycle hooks.
//! - **pytest xunit**: `setup_method`/`teardown_method` and their class,
//!   function, and module variants are lifecycle hooks.

use crate::base::{Symbol, SymbolKind};
use crate::python::PythonExtractor;
use std::path::PathBuf;
use tree_sitter::Parser;

fn symbols(code: &str, file: &str) -> Vec<Symbol> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .expect("load Python grammar");
    let tree = parser.parse(code, None).expect("parse Python");
    let mut ext = PythonExtractor::new(file.to_string(), code.to_string(), &PathBuf::from("/tmp"));
    ext.extract_symbols(&tree)
}

fn find<'a>(symbols: &'a [Symbol], name: &str) -> &'a Symbol {
    symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap_or_else(|| panic!("expected symbol {name}, got {symbols:?}"))
}

fn role(symbols: &[Symbol], name: &str, key: &str) -> bool {
    find(symbols, name)
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(serde_json::Value::as_bool)
        == Some(true)
}

fn test_role(symbols: &[Symbol], name: &str) -> Option<String> {
    find(symbols, name)
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("test_role"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

#[test]
fn unittest_setup_emits_test_lifecycle_and_testcase_is_container() {
    let code = r#"
import unittest

class ArithmeticTest(unittest.TestCase):
    def setUp(self):
        self.total = 2

    def tearDown(self):
        pass

    def test_unittest_case(self):
        self.assertEqual(self.total, 2)

    def production_helper(self):
        return self.total

class OrdinaryHelper:
    def helper(self):
        return 1

class MisleadingBase(NotATestCase):
    def helper(self):
        return 1

def test_pytest_case():
    assert True

def calculate_total():
    return 2
"#;
    let syms = symbols(code, "tests/test_arithmetic.py");

    assert!(role(&syms, "ArithmeticTest", "test_container"));
    assert!(!role(&syms, "OrdinaryHelper", "test_container"));
    assert!(!role(&syms, "MisleadingBase", "test_container"));

    assert_eq!(test_role(&syms, "setUp").as_deref(), Some("fixture_setup"));
    assert!(role(&syms, "setUp", "is_test"));
    assert!(role(&syms, "setUp", "test_lifecycle"));
    assert_eq!(
        test_role(&syms, "tearDown").as_deref(),
        Some("fixture_teardown")
    );
    assert!(role(&syms, "tearDown", "is_test"));
    assert!(role(&syms, "tearDown", "test_lifecycle"));

    assert_eq!(
        test_role(&syms, "test_unittest_case").as_deref(),
        Some("test_case")
    );
    assert!(!role(&syms, "test_unittest_case", "test_lifecycle"));
    assert_eq!(
        test_role(&syms, "test_pytest_case").as_deref(),
        Some("test_case")
    );
    assert!(!role(&syms, "test_pytest_case", "test_lifecycle"));

    assert!(!role(&syms, "production_helper", "is_test"));
    assert!(!role(&syms, "calculate_total", "is_test"));
    assert!(!role(&syms, "ArithmeticTest", "is_test"));

    assert!(
        syms.iter()
            .any(|s| s.name == "ArithmeticTest" && s.kind == SymbolKind::Class)
    );
}

#[test]
fn camel_case_unittest_methods_are_test_cases() {
    let code = r#"
import unittest

class ArithmeticTest(unittest.TestCase):
    def testAddition(self):
        self.assertEqual(1 + 1, 2)

    def helperAddition(self):
        return 2
"#;
    let syms = symbols(code, "tests/test_arithmetic.py");

    assert_eq!(
        test_role(&syms, "testAddition").as_deref(),
        Some("test_case")
    );
    assert!(!role(&syms, "helperAddition", "is_test"));
    assert!(role(&syms, "ArithmeticTest", "test_container"));
}

#[test]
fn test_prefixed_names_outside_a_test_path_are_not_tests() {
    let code = r#"
def test_connection():
    return True

class TestHarness:
    def testConnection(self):
        return True
"#;
    let syms = symbols(code, "src/client.py");

    assert!(!role(&syms, "test_connection", "is_test"));
    assert!(!role(&syms, "testConnection", "is_test"));
    assert!(!role(&syms, "TestHarness", "test_container"));
}

#[test]
fn lifecycle_names_outside_a_test_path_emit_no_role() {
    let code = r#"
class ConnectionPool:
    def setUp(self):
        return True

    def tearDown(self):
        return True

def setup_module():
    return True

def teardown_function():
    return True
"#;
    let syms = symbols(code, "src/client.py");

    for name in ["setUp", "tearDown", "setup_module", "teardown_function"] {
        assert_eq!(test_role(&syms, name), None, "{name} must carry no role");
        assert!(!role(&syms, name, "is_test"));
        assert!(!role(&syms, name, "test_lifecycle"));
    }
}

#[test]
fn lifecycle_names_inside_a_test_path_keep_their_roles() {
    let code = r#"
def setup_module():
    return True

def teardown_module():
    return True
"#;
    let syms = symbols(code, "tests/test_hooks.py");

    assert_eq!(
        test_role(&syms, "setup_module").as_deref(),
        Some("fixture_setup")
    );
    assert_eq!(
        test_role(&syms, "teardown_module").as_deref(),
        Some("fixture_teardown")
    );
}

#[test]
fn pytest_fixture_is_a_fixture_setup_hook() {
    let code = r#"
import contextlib
import pytest

@pytest.fixture
def build_client():
    return object()

@pytest.fixture(scope="module")
def build_session():
    yield object()

@contextlib.contextmanager
def temporary_client():
    yield object()
"#;
    let syms = symbols(code, "tests/conftest.py");

    assert_eq!(
        test_role(&syms, "build_client").as_deref(),
        Some("fixture_setup")
    );
    assert!(role(&syms, "build_client", "test_lifecycle"));
    assert_eq!(
        test_role(&syms, "build_session").as_deref(),
        Some("fixture_setup")
    );
    assert!(!role(&syms, "temporary_client", "is_test"));
}

#[test]
fn pytest_parametrize_is_a_parameterized_test() {
    let code = r#"
import functools
import pytest

@pytest.mark.parametrize("left,right", [(1, 1), (2, 2)])
def test_addition(left, right):
    assert left + right == right + left

@pytest.mark.skip(reason="flaky")
def test_plain_case():
    assert True

@functools.lru_cache(maxsize=None)
def cached_total():
    return 2
"#;
    let syms = symbols(code, "tests/test_addition.py");

    assert_eq!(
        test_role(&syms, "test_addition").as_deref(),
        Some("parameterized_test")
    );
    assert!(role(&syms, "test_addition", "is_test"));
    assert!(!role(&syms, "test_addition", "test_lifecycle"));
    assert_eq!(
        test_role(&syms, "test_plain_case").as_deref(),
        Some("test_case")
    );
    assert!(!role(&syms, "cached_total", "is_test"));
}

#[test]
fn pytest_xunit_and_unittest_module_hooks_carry_a_direction() {
    let code = r#"
def setUpModule():
    pass

def tearDownModule():
    pass

def setup_module(module):
    pass

def teardown_module(module):
    pass

def setup_function(function):
    pass

def teardown_function(function):
    pass

def setup_client():
    pass

class TestArithmetic:
    def setup_class(cls):
        pass

    def teardown_class(cls):
        pass

    def setup_method(self, method):
        pass

    def teardown_method(self, method):
        pass

    async def asyncSetUp(self):
        pass

    async def asyncTearDown(self):
        pass

    def test_total(self):
        assert True
"#;
    let syms = symbols(code, "tests/test_hooks.py");

    for name in [
        "setUpModule",
        "setup_module",
        "setup_function",
        "setup_class",
        "setup_method",
        "asyncSetUp",
    ] {
        assert_eq!(
            test_role(&syms, name).as_deref(),
            Some("fixture_setup"),
            "{name} must be a setup hook"
        );
    }

    for name in [
        "tearDownModule",
        "teardown_module",
        "teardown_function",
        "teardown_class",
        "teardown_method",
        "asyncTearDown",
    ] {
        assert_eq!(
            test_role(&syms, name).as_deref(),
            Some("fixture_teardown"),
            "{name} must be a teardown hook"
        );
    }

    assert!(!role(&syms, "setup_client", "is_test"));
    assert!(role(&syms, "TestArithmetic", "test_container"));
}

#[test]
fn unittest_skip_decorators_mark_tests_outside_a_test_path() {
    let code = r#"
import unittest

@unittest.skip("not ready")
def verify_skip():
    pass

@unittest.skipIf(True, "guarded")
def verify_skip_if():
    pass

@unittest.skipUnless(False, "guarded")
def verify_skip_unless():
    pass

@unittest.expectedFailure
def verify_expected_failure():
    pass

@unittest.mock.patch("module.target")
def patch_client():
    return object()
"#;
    let syms = symbols(code, "src/client.py");

    for name in [
        "verify_skip",
        "verify_skip_if",
        "verify_skip_unless",
        "verify_expected_failure",
    ] {
        assert_eq!(
            test_role(&syms, name).as_deref(),
            Some("test_case"),
            "{name} must be a test case"
        );
    }

    assert!(!role(&syms, "patch_client", "is_test"));
}

#[test]
fn pytest_class_holding_only_fixtures_is_not_a_test_container() {
    let code = r#"
import pytest

class FixtureBundle:
    @pytest.fixture
    def client(self):
        return object()
"#;
    let syms = symbols(code, "tests/conftest.py");

    assert_eq!(test_role(&syms, "client").as_deref(), Some("fixture_setup"));
    assert!(!role(&syms, "FixtureBundle", "test_container"));
}
