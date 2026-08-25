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

Miller runs a pytest continuous-testing provider for Python, so a wrong test
role turns straight into a wrong staleness verdict. The detector follows the
two collectors that actually run the code.

| Idiom | Role | Source of the rule |
| --- | --- | --- |
| `def test_x` / `def testX` in a test path | `test_case` | pytest `python_functions = test*`; unittest `TestLoader.testMethodPrefix = "test"` |
| `@pytest.mark.parametrize` | `parameterized_test` | pytest parametrize runs one case per argument set |
| any other `@pytest.mark.*` | `test_case` | a pytest mark is only applied to collected items |
| `@unittest.skip`, `skipIf`, `skipUnless`, `expectedFailure` | `test_case` | unittest decorators are only applied to test methods |
| `@pytest.fixture` | `fixture_setup` | pytest fixture factory |
| `setUp`, `setUpClass`, `setUpModule`, `asyncSetUp` | `fixture_setup` | unittest fixtures |
| `tearDown`, `tearDownClass`, `tearDownModule`, `asyncTearDown` | `fixture_teardown` | unittest fixtures |
| `setup_method`, `setup_class`, `setup_function`, `setup_module` | `fixture_setup` | pytest xunit-style setup |
| `teardown_method`, `teardown_class`, `teardown_function`, `teardown_module` | `fixture_teardown` | pytest xunit-style teardown |
| class with a `TestCase` base, or with a collected member | `test_container` | unittest suite and pytest class collection |

Two rules carry a deliberate cost.

The name rule takes a bare `test` prefix, not `test_`. Both collectors use the
bare prefix, so `def testAddition` is a real case. Because production code
shares that vocabulary, the name rule stays guarded by `is_test_path`. An
annotation rule needs no path guard, because a `pytest.mark` or `unittest`
decorator is only ever written on a test.

A `@pytest.fixture` reports `fixture_setup`, not `test_case`. A fixture that
yields also tears down after the test, but the setup half always runs, so
setup is the honest single direction. This reverses an earlier decision that
excluded fixtures from roles entirely; see
`docs/decisions/2026-08-20-test-role-contract-closure.md`.

## Known limitation: nested callables

Test detection reads a symbol's name, path, kind, and annotation keys. It does
not know whether a callable is defined at module or class level, or nested
inside another function. One consequence remains:

- A nested `def test(...)` inside a real test function is flagged as a case.
  pytest does not collect nested functions.

That needs nesting depth at the extraction site, which is a Python extractor
change rather than a detection-rule change. The measured cost is in the
real-world evidence below.

The second consequence is fixed. A nested callable used to inherit the
enclosing decorated definition's decorators, because `find_decorated_node` in
`crates/julie-extractors/src/python/decorators.rs` walked up to the nearest
`decorated_definition` ancestor without stopping at an enclosing definition. A
helper nested inside a `@pytest.mark.parametrize` test reported
`parameterized_test`, a closure inside a `@pytest.fixture` helper reported
`fixture_setup`, and every method of a decorated class carried the class
decorator. The walk now stops at the first enclosing `function_definition` or
`class_definition`, so decorators reach only the definition they are written
on.

## Known gap: cross-file inheritance

A class that inherits an imported base emits neither a resolved `extends`
relationship nor a pending one. `extract_class_relationships` in
`crates/julie-extractors/src/python/relationships.rs` only emits when the base
class symbol is found in the same file, unlike cross-module calls, which do
emit a structured pending relationship. A project base class such as
`class ApiTestCase(TestCase)` in another module is therefore invisible to
workspace-level resolution.

Test containers still work in that case, because
`mark_python_test_containers` matches the `superclasses` metadata name
`TestCase` textually. The gap is recorded as
`python.relationships.open_gaps[extends]` in
`fixtures/extraction/capabilities.json`.

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

That is 98.2 percent precision on the corpus. The remaining failure mode is
recorded under "Known limitation: nested callables" above.

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
