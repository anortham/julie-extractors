# Python support

Julie registers one Python language: `python` handles `.py`, `.pyi`, and `.pyw`
files.

## Continuous testing

Run the language target when changing Python extraction:

```bash
cargo xtask test language python
```

The command runs the Python unit-test module and the golden extraction test
with `JULIE_GOLDEN_LANGUAGE=python`. The normal golden target stays
unfiltered:

```bash
cargo xtask test golden
```

## Test-role contract

code-kb runs a pytest continuous-testing provider for Python, so a wrong test
role turns straight into a wrong staleness verdict. The detector follows the
two collectors that actually run the code.

| Idiom | Role | Source of the rule |
| --- | --- | --- |
| `def test_x` / `def testX` in a test path | `test_case` | pytest `python_functions = test*`; unittest `TestLoader.testMethodPrefix = "test"` |
| `@pytest.mark.parametrize` | `parameterized_test` | pytest parametrize runs one case per argument set |
| any other `@pytest.mark.*` | `test_case` | a pytest mark is only applied to collected items |
| `@unittest.skip`, `skipIf`, `skipUnless`, `expectedFailure` | `test_case` | unittest decorators are only applied to test methods |
| `@pytest.fixture` | `fixture_setup` | pytest fixture factory |
| `setUp`, `setUpClass`, `setUpModule`, `asyncSetUp` in a test path | `fixture_setup` | unittest fixtures |
| `tearDown`, `tearDownClass`, `tearDownModule`, `asyncTearDown` in a test path | `fixture_teardown` | unittest fixtures |
| `setup_method`, `setup_class`, `setup_function`, `setup_module` in a test path | `fixture_setup` | pytest xunit-style setup |
| `teardown_method`, `teardown_class`, `teardown_function`, `teardown_module` in a test path | `fixture_teardown` | pytest xunit-style teardown |
| `setUpTestData` in a test path | `fixture_setup` | Django class-level fixture |
| `@pytest_asyncio.fixture` | `fixture_setup` | pytest-asyncio fixture factory |
| class with a `TestCase` base, or with a collected member | `test_container` | unittest suite and pytest class collection |
| `test*` method or unittest hook in a `TestCase` subclass, in any path | `test_case` or fixture role | unittest loads every `TestCase` subclass |

`TestCase` bases are matched by their last dotted segment against the
standard-library, Django, and DRF classes: `TestCase`,
`IsolatedAsyncioTestCase`, `SimpleTestCase`, `TransactionTestCase`,
`LiveServerTestCase`, `StaticLiveServerTestCase`, and the DRF `API*` forms.
A file named `tests.py` is a test path, because unittest discovery and
Django's `startapp` layout both use it.

Two rules carry a deliberate cost.

The name rule takes a bare `test` prefix, not `test_`. Both collectors use the
bare prefix, so `def testAddition` is a real case. Because production code
shares that vocabulary, the name rule stays guarded by `is_test_path`. The
lifecycle name rule carries the same guard for the same reason: a production
`ConnectionPool.setUp` in `src/client.py` earns no role. An annotation rule
needs no path guard, because a `pytest.mark` or `unittest` decorator is only
ever written on a test.

A `@pytest.fixture` reports `fixture_setup`, not `test_case`. A fixture that
yields also tears down after the test, but the setup half always runs, so
setup is the honest single direction. This reverses an earlier decision that
excluded fixtures from roles entirely; see
`docs/decisions/2026-08-20-test-role-contract-closure.md`.

## Nesting

A `def` or `class` is parented to its nearest enclosing definition. A `def`
in a class body is a method of that class. A `def` or `class` in a function
body is a nested definition of that function: `wrapper` inside `decorate` is a
function with parent `decorate`, not a top-level function or a method of the
outer class. A lambda is one `function` symbol parented to its enclosing
definition.

Test detection skips nested callables, because pytest and unittest collect
only module-level and class-level callables. A nested `def test(...)` inside a
real test function has no role.

Decorators reach only the definition they are written on:
`find_decorated_node` in `crates/julie-extractors/src/python/decorators.rs`
stops at the first enclosing `function_definition` or `class_definition`.

## Inheritance

A base class defined in the same file gives a resolved `extends` (or
`implements` for a `Protocol`) relationship. Any other base, such as
`unittest.TestCase`, `models.Model`, or an imported project class, gives a
structured pending `extends` relationship. Its target keeps the dotted chain,
and `import_context` names the import binding when the leading segment is an
import in the file. A subscripted base such as `Generic[T]` uses its base
name.

## Imports and type facts

Each import binding carries `source`, `importedName`, `specifier`,
`isWildcard`, and, for a relative import, `relativeLevel`. `import os.path`
binds `os`, so the symbol is named `os` with source `os.path`. A pending call
whose target or receiver starts with an import binding sets `import_context`
to that binding.

Type facts come only from annotation nodes: parameters, assignments, and
return types. `Optional[X]`, `X | None`, `Union[X, None]`, `Annotated[X, ...]`,
`ClassVar[X]`, `Final[X]`, `Mapped[X]`, and the forward reference `"X"` record
`X`; the full annotation stays in `metadata.declared`. A union of two real
types records nothing. Unannotated returns and values record nothing, except
the same-file constructor fact `x = Foo()`.

Imports, variables, constants, attributes, and parameters have no body span.

## Grammar freshness

The live maintenance report was run with:

```bash
node scripts/grammar-freshness-report.mjs --format json
```

The Python-specific findings were:

- `tree-sitter-python` is current: declared and locked at `0.25.0`, matching
  the latest stable release.
- The shared `tree-sitter` runtime is marked drift at locked `0.26.11` versus
  latest stable `0.26.13`. This is a repository-wide freshness finding, not an
  unrecorded Python dependency change.

## Real-world evidence

Two corpora were scanned, one per collector style. Both were cloned shallowly
into temporary directories. No project build script, hook, or third-party
binary was run.

- pytest style: `pallets/flask` at commit
  `d318b683471101618febed18996405ad26462110`, BSD-3-Clause.
- unittest style: `google/python-fire` at commit
  `716bbc23d7eca949fdb682172283c8d18f742cb6`, Apache-2.0.

Reproducible checkout and scan commands:

```bash
CORPUS="$(mktemp -d)"
git clone --depth 1 https://github.com/pallets/flask "$CORPUS"
git -C "$CORPUS" checkout --detach \
  d318b683471101618febed18996405ad26462110

cargo build --locked --bin julie-extract
ARTIFACT="$(mktemp -d)"
./target/debug/julie-extract scan \
  --root "$CORPUS" \
  --db "$ARTIFACT/artifact.sqlite" \
  --json >"$ARTIFACT/scan-report.json" \
  2>"$ARTIFACT/scan-stderr.log"
```

Both scans reported `status=ok` with `files_failed=0` and empty `warnings`
and `errors`. Flask scanned 236 files, 83 of them Python; python-fire scanned
79 files, 61 of them Python. Neither scan produced a single Python parse
diagnostic. Flask's 11 diagnostics were 9 HTML and 2 SQL rows.

| Artifact evidence | flask | python-fire |
| --- | ---: | ---: |
| Python files indexed | 83 | 61 |
| Python symbols | 3,819 | 2,377 |
| `test_case` | 369 | 274 |
| `parameterized_test` | 35 | 0 |
| `fixture_setup` | 23 | 3 |
| `fixture_teardown` | 0 | 0 |
| `test_container` | 7 | 26 |

The Flask column was re-measured on 2026-08-25 against the same pinned commit,
after the decorator-scope fix. It previously read 40 `parameterized_test` and
24 `fixture_setup`; the six removed rows are exactly the six that inherited an
enclosing decorator.

python-fire measures the bare-`test`-prefix rule. Of its 274 cases, 238 are
camelCase `testXxx` methods that the previous `test_` rule could not see at
all. Every flagged symbol is a real absltest case, a real `setUp`, or a real
`unittest.TestCase` subclass: the corpus produced zero false positives.

Flask measures the cost of the same rule plus the nested-callable limitation.
Of its 434 flagged symbols, 8 are wrong, and every one of them is a nested
local function: nested `def test(...)` Flask routes and Click commands written
inside test bodies, in `tests/test_basic.py`, `tests/test_cli.py`, and
`tests/test_regression.py`.

That was 98.2 percent precision on the corpus. Those 8 nested functions now
carry no role, because test detection skips nested callables (see "Nesting"
above).

The second failure mode is closed. Six symbols used to inherit the enclosing
decorator — `check`, `run_simple_mock` twice, `reset_path`, `create_app`, and
`inner`. Each now carries no role, which is why the Flask column above dropped
five `parameterized_test` rows and one `fixture_setup` row.

Flask also proves the two new roles. Its 35 `parameterized_test` rows and 23
`fixture_setup` rows had no equivalent before this contract: parametrized
cases reported as plain `test_case`, and `@pytest.fixture` factories carried
no role at all.

The temporary checkouts and SQLite artifacts were removed after recording this
evidence.
