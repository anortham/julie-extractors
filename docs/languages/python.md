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
types records nothing, also inside a wrapper (`Optional[A | B]`,
`Union[A, B, None]`). Unannotated returns and values record nothing, except
the inferred facts (`is_inferred=true`) of an unannotated single-target
assignment (`x = ...` or `self.x = ...`):

- `Foo()` records `Foo` when `Foo` is a same-file class.
- A call to a same-file function (`load()`), a method through `self` or `cls`
  inside its class, a method through a same-file class name
  (`Workspace.create()`), or a method on such a call's result
  (`Workspace.open().root()`) records the callee's declared return type,
  reduced by the annotation rules above. `-> Self` records the owning class.
  `TypeGuard[X]`, `TypeIs[X]`, and `Literal[...]` returns record nothing,
  because the value is a `bool` or a literal, not the named type.
- Every same-named candidate with the same owner must declare the same return
  type. An `async def` counts only under `await`; an awaited plain `def`
  records nothing.
- A bare name follows Python scope rules. A parameter, assignment, `for` or
  `with` target, import, lambda, or comprehension variable of the same name
  in the call's function or an enclosing one, or at module level, records
  nothing. A walrus (`:=`) inside a comprehension binds in the enclosing
  function, as in Python. A nested `def` counts only for calls inside its
  function. A method counts only through `self`, `cls`, or its class, also
  when it is defined in an `if` or `try` block of the class body. A
  `from m import *` does not count as a binding, because the names it binds
  are in another file.
- A class is identified by its definition, not by its name. `self` and `cls`
  are the class of the method whose parameter they are, so two same-named
  classes in different functions or classes keep their own methods. A class
  name that has two definitions in one scope records no method type. A
  method name that the class body also binds another way
  (`load = contextmanager(load)`) records nothing. Inherited methods record
  nothing.
- A return type that is a `TypeVar`/`ParamSpec`/`TypeVarTuple` of the file
  (also through an alias such as `from typing import TypeVar as TV`), a
  PEP 695 type parameter, or a type argument of the class bases records
  nothing. The callee's parameter annotations, and those of the functions
  around it, can also show that an imported name is a type variable. The
  return name records nothing when it is not a same-file class, a builtin, or
  a `typing` name such as `Any` or `Dict`, and those annotations use it as a
  type argument (`def first(items: list[T]) -> T`), or use it at all when
  the name is spelled like a type variable (`T`, `KT`, `T1`, `_T_co`,
  `ModelT`, `AnyStr`). So `-> Item` with `items: list[Item]` records nothing
  even when `Item` is an imported class, and `-> Response` with
  `response: Response` records `Response`. An imported
  type variable with another spelling that the callee uses only as a whole
  annotation, or only in the return type (`def item(self) -> U`), still
  records its name, because the syntax does not show that the name is not
  a class.
- A callee with a decorator other than `staticmethod`, `classmethod`,
  `abstractmethod`, `overload`, `override`, `final`, `cache`, or `lru_cache`
  records nothing. These names count only when the file does not bind them
  (a builtin or an unseen import) or imports them from `functools`, `typing`,
  `typing_extensions`, `abc`, or `builtins`, also through an alias
  (`from functools import cache as memo`, `import typing as t`). A same-file
  `def cache` or `from mylib import cache` records nothing. So does a call to
  anything in another file, because each file is extracted alone.

Imports, variables, constants, attributes, and parameters have no body span.

## Declarations and documentation

- A PEP 695 `type Name[T] = ...` statement is a `type` symbol. Its name and
  the type parameters it declares are not type usages. Class and function
  signatures keep their `[T]` type parameters.
- A class is an `interface` only when a base is `Protocol`,
  `typing.Protocol`, or `typing_extensions.Protocol` (bare or subscripted).
  It is an `enum` only for `Enum`, `IntEnum`, `StrEnum`, `Flag`, `IntFlag`,
  or `ReprEnum` (bare or `enum.`-qualified) and for the Django choices bases
  `TextChoices`, `IntegerChoices`, and `models.Choices`. A base whose name
  merely contains `Protocol` or `Enum` changes nothing.
- Every plain assignment directly in an enum body is an `enum_member`,
  whatever its case, except `_sunder_` and `__dunder__` names.
- Class visibility follows the same underscore rule as functions.
- A docstring is the first statement of the body when it is a plain string.
  `r`/`u` prefixes and the quotes are stripped; f-strings and byte strings are
  not docstrings.
- A module or class attribute is documented by Sphinx `#:` comment lines
  directly above it, else by a string statement directly after it (PEP 257
  attribute docstring). A plain `#` comment documents nothing.

## References

- A call or name inside a decorator is owned by the decorated function or
  class, so `@retry(3)` gives `fetch -calls-> retry`.
- `super().m()` carries the first declared base as `receiver_type` and
  resolves to a same-file `Base.m`; the `super` builtin call itself records no
  pending row. `cls(...)` is a pending call to the enclosing class.
- A forward-reference annotation string (`"Repo"`, `"User | None"`) gives one
  `type_usage` per name at its exact span. `Literal[...]` values and
  `Annotated[...]` metadata strings are not types.
- A builtin generic with arguments (`list[User]`, `dict[str, User]`) records
  a `type_usage` for its head so its type arguments have a row to join to.
- In `match`, a class pattern head (`case Point(...)`) is a `type_usage`, and a
  dotted value pattern (`case Color.RED`) reads `Color` and accesses `RED`.

## Framework facts

- Django `re_path` keeps raw-string backslashes verbatim. Its
  `normalized_route_template` follows a conservative policy: anchors drop, a
  trailing `/?` is an optional trailing slash, `(?P<id>...)` becomes `:id`, an
  unnamed group becomes the positional `:arg1`, `:arg2`, ..., and an escaped
  punctuation character is literal. Alternation, optional or repeated
  fragments, lookaround, bare character classes, and mixed named and unnamed
  groups emit no normalized template.
- `include(("api.urls", "app"), namespace="v1")` records `included_module`
  `api.urls`; a non-literal argument records its source text (`router.urls`).
- Django REST Framework emits `drf.router_registration.v1` for
  `router.register(prefix, ViewSet)` on a same-file `DefaultRouter` or
  `SimpleRouter`, `drf.viewset_action.v1` for `@action(...)`, and
  `drf.api_view.v1` for `@api_view([...])`. The URL prefix of the router is
  joined across files by code-kb.
- `http.client_request.v1` covers `requests`/`httpx` module calls,
  from-imported verbs (`from requests import get`), a `url=` keyword literal,
  and receivers constructed in the same function (`s = requests.Session()`,
  `with httpx.AsyncClient() as client`). An unproven receiver stays silent.
- A Flask or FastAPI receiver declared with an annotation
  (`app: Flask = Flask(__name__)`) keeps its route facts.

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
