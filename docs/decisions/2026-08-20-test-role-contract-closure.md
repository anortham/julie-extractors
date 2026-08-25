# Test-role contract closure (2026-08-20)

## Decision

Close the 28 legacy `test_detection` gaps against named language or tool
contracts. A role is `supported` only when its contract is implemented and a
registered golden emits the role. A role is `not_applicable` only when the
adopted contract has no such role; it is not inferred from an empty extraction.
Every row below has `test_detection.open_gaps: []` in
`fixtures/extraction/capabilities.json`.

| Language | Named contract and evidence | `test_case` | `test_container` | `test_lifecycle` |
| --- | --- | --- | --- | --- |
| Rust | Named test attribute macros, the qualified `::test` suffix rule, rstest `#[fixture]`, and `#[cfg(test)]`/compound-`cfg` module containers in `rust:test_roles` | supported | supported | supported |
| C | Criterion `TestSuite`/`Test` macros and suite/test init/fini hooks in `c:test_roles` | supported | supported | supported |
| C++ | Catch2 `TEST_CASE`/`SECTION` and GoogleTest fixture hooks in `cpp:test_roles` | supported | supported | supported |
| Zig | Zig `test` declarations in `zig:test_roles` | supported | not applicable | not applicable |
| HTML | Browser Mocha document marker plus BDD calls in `html:test_roles` | supported | supported | supported |
| SQL | pgTAP runner, schema, routine, and setup/teardown naming contract in `sql:test_roles` | supported | supported | supported |
| Markdown | rustdoc executable Rust/unspecified fences in `markdown:test_roles` | supported | not applicable | not applicable |
| JSON | JSON Schema Test Suite group/case shape in `json:test_roles` | supported | supported | not applicable |
| TOML | trycmd case shape and nextest group/setup tables in `toml:trycmd_roles` and `toml:nextest_roles` | supported | supported | supported |
| YAML | Google container-structure-test v2 command-test shape in `yaml:test_roles` | supported | supported | supported |
| XML | Apache Ant project/target/junit/test chain in `xml:test_roles` | supported | supported | not applicable |
| Python | pytest collection prefixes, `@pytest.mark.parametrize`, `@pytest.fixture`, pytest xunit hooks, and unittest fixtures in `python:test_roles` | supported | supported | supported |
| C# | NUnit, MSTest, and xUnit.net attributes plus the xUnit constructor/`IDisposable`/`IAsyncLifetime` lifecycle in `csharp:test_roles` | supported | supported | supported |

The `not_applicable` cells are contract-level conclusions. Zig's `test`
declarations provide no adopted lifecycle or suite syntax;
rustdoc treats executable fences as examples, not suites or hooks; JSON Schema
Test Suite groups cases but defines no setup/teardown role; and Ant's JUnit
task contract defines targets and tests but no lifecycle symbol. Ordinary
helpers, headings, nested lookalikes, and arbitrary keys are therefore not
promoted into roles. This is distinct from the supported cells, where the
fixture contains positive role metadata and nearby controls that must remain
unclassified.

## Named contracts and primary sources

- Rust: the [Rust Reference `cfg` attribute](https://doc.rust-lang.org/reference/conditional-compilation.html#the-cfg-attribute) defines the built-in `test` predicate and the `all`/`any`/`not` predicate combinators; the [Rust Reference testing attributes](https://doc.rust-lang.org/reference/attributes/testing.html) define `#[test]`; [`tokio::test`](https://docs.rs/tokio/latest/tokio/attr.test.html), [`sqlx::test`](https://docs.rs/sqlx/latest/sqlx/attr.test.html), [`actix_web::test`](https://docs.rs/actix-web/latest/actix_web/attr.test.html), [`async_std::test`](https://docs.rs/async-std/latest/async_std/attr.test.html), and [`googletest::test`](https://docs.rs/googletest/latest/googletest/attr.test.html) define the qualified async and framework replacements for it; [rstest](https://docs.rs/rstest/latest/rstest/) defines `#[rstest]`, its `#[case]`/`#[values]` per-case attributes, and `#[fixture]`; [test-case](https://docs.rs/test-case/latest/test_case/) defines `#[test_case(..)]`; [rustdoc documentation tests](https://doc.rust-lang.org/rustdoc/write-documentation/documentation-tests.html) define executable Rust examples and the default language for unspecified fences.
- C: [Criterion features and test suites](https://criterion.readthedocs.io/en/master/features.html) define the `TestSuite`/`Test` and init/fini vocabulary used by the fixture.
- C++: [Catch2 test cases and sections](https://github.com/catchorg/Catch2/blob/devel/docs/test-cases-and-sections.md) define `TEST_CASE`/`SECTION`; [GoogleTest testing reference](https://google.github.io/googletest/reference/testing.html) defines fixture setup and teardown methods.
- Zig: the official [Zig language reference](https://ziglang.org/documentation/master/#test) defines `test` declarations; it does not define a suite or lifecycle role for this extractor contract.
- HTML: Mocha's [browser runner](https://mochajs.org/running/browsers/), [BDD interface](https://mochajs.org/interfaces/bdd/), and [hooks](https://mochajs.org/features/hooks/) define the required `mocha.js`/BDD marker and `describe`/`context`/`it`/hook calls.
- SQL: [pgTAP](https://pgtap.org/) documents the `runtests`/`do_tap` runner and TAP-returning test routines used by the fixture.
- Markdown: [CommonMark fenced code blocks](https://spec.commonmark.org/current/#fenced-code-blocks) provide the host syntax; [rustdoc documentation tests](https://doc.rust-lang.org/rustdoc/write-documentation/documentation-tests.html) provide the executable-fence contract.
- JSON: the [JSON Schema Test Suite](https://github.com/json-schema-org/JSON-Schema-Test-Suite) defines schema groups with `description`, `schema`, and `tests`, and case objects with `description`, `data`, and `valid`; JSON itself has no test-role semantics (see the [JSON Schema data model](https://json-schema.org/understanding-json-schema/basics)).
- TOML: [trycmd](https://github.com/assert-rs/trycmd) defines command-case TOML; [cargo-nextest configuration](https://nexte.st/docs/configuration/) defines test groups and setup scripts. These are tool schemas layered on TOML, not TOML-native roles.
- YAML: Google's [container-structure-test command tests](https://github.com/GoogleContainerTools/container-structure-test/blob/main/docs/tests/command_test.md) define the v2 `schemaVersion`, `commandTests`, `name`, `setup`, and `teardown` contract.
- XML: Apache Ant's [JUnit task](https://ant.apache.org/manual/Tasks/junit.html) defines `<project>` targets containing `<junit>` and nested `<test>` elements; report XML and lookalike tags are outside that contract.
- Python: [pytest test discovery](https://docs.pytest.org/en/stable/explanation/goodpractices.html#conventions-for-python-test-discovery) defines the `test*` function prefix and the `Test*` class prefix; [pytest fixtures](https://docs.pytest.org/en/stable/reference/fixture.html) define `@pytest.fixture`; [pytest xunit-style setup](https://docs.pytest.org/en/stable/how-to/xunit_setup.html) defines `setup_method`/`teardown_method` and the class, function, and module variants; [`unittest.TestLoader.testMethodPrefix`](https://docs.python.org/3/library/unittest.html#unittest.TestLoader.testMethodPrefix) defines the bare `test` method prefix, and `unittest` defines `setUp`/`tearDown`, their class and module variants, and `IsolatedAsyncioTestCase.asyncSetUp`/`asyncTearDown`.

## Python fixture-role reversal (2026-08-25)

An earlier Python rule deliberately excluded `@pytest.fixture` from every
role, and a unit test asserted that a fixture factory is not a test symbol.
That reversal is now recorded: `@pytest.fixture` reports `fixture_setup`.

The old exclusion treated a fixture as production support code. It is not. A
fixture only ever runs inside a test session, and Miller's pytest continuous
testing provider must know that editing a fixture invalidates every test that
requests it. Publishing no role hid that dependency.

A fixture that yields also tears down after the test, so the true direction is
"both sides". The extractor cannot tell a yielding fixture from a returning
one without reading the body, and the setup half always runs, so the contract
publishes the single honest direction: setup.

This changes published output for real Python projects. A `conftest.py` that
previously produced no roles now produces `fixture_setup` rows, and
`test_lifecycle` is set, which keeps a fixture out of the `test_case` count
and keeps a class holding only fixtures out of `test_container`.

Two further Python contract changes land with it. The name rule now takes the
bare `test` prefix that both collectors use, so `def testAddition` is a real
case; it stays guarded by the shared test-path check because production code
shares that vocabulary. And `@pytest.mark.parametrize` now reports
`parameterized_test` instead of `test_case`, because one decorated definition
runs one case per argument set. Real-world precision and recall measurements
for all three changes are in `docs/languages/python.md`.

## Rust lifecycle reversal (2026-08-25)

The Rust row previously read `test_lifecycle: not applicable`. The stated
reason was that `cfg(test)` defines no lifecycle syntax. That reason held only
because the row named `cfg(test)` as the whole contract. The adopted contract
is wider than that, and it does define a hook: rstest's `#[fixture]` builds a
value a test case asks for by name.

A fixture only ever runs inside a test session, so publishing no role hid a
real dependency — Miller's Rust continuous testing provider must know that
editing a fixture invalidates every case that requests it. The row therefore
moves to `test_lifecycle: supported`. A fixture that returns a guard also tears
down after the case, and the extractor cannot tell a guard-returning fixture
from a plain one without reading the body, so the contract publishes the single
honest direction: `fixture_setup`.

Rust has no teardown attribute at all, so `fixture_teardown` is never written
for Rust. That is a narrower claim than the `test_lifecycle` ledger cell can
express, because the cell covers both directions with one unit.

Three further Rust contract changes land with it:

- Detection stays annotation-only. A Rust function earns a role from an
  attribute macro and never from its name or its path.
- A qualified attribute macro whose last `::` segment is exactly `test` is a
  test attribute. This is what makes `tokio::test`, `sqlx::test`,
  `actix_web::test`, `actix_rt::test`, `async_std::test`, `googletest::test`,
  and `test_log::test` classify without naming each crate. The segment must
  match whole, so `latest`, `contest`, `test_util`, and `tokio::main` stay
  production attributes.
- `#[cfg(all(test, ..))]` and `#[cfg(any(test, ..))]` now mark a module as
  `test_container`, at any nesting depth. A `test` inside a `not` never
  contributes, because such an item is compiled out of test builds. A module
  also publishes its `cfg` attribute, which it previously dropped.

`#[test_case(..)]` and an `#[rstest]` carrying `#[case]` or `#[values]`
attributes report `parameterized_test`, because one decorated definition runs
one case per data row. A bare `#[rstest]` stays `test_case`.

Two named Rust test surfaces are excluded and recorded as `open_gaps` on the
rust row: benchmark harnesses (`#[bench]`, criterion, divan) and rustdoc
doc-tests. Both sit under `kind_coverage.structural_facts.open_gaps` rather
than `test_detection`, because the `test_detection` vocabulary is frozen to
`test_case`, `test_container`, and `test_lifecycle` and each is already
classified exactly once for rust. Real-world precision and recall measurements
for the Rust changes are in `docs/languages/rust.md`.

- C#: [NUnit attributes](https://docs.nunit.org/articles/nunit/writing-tests/attributes.html) define `TestFixture`, `SetUpFixture`, `Test`, `TestCase`, `TestCaseSource`, `TestFixtureSource`, `SetUp`/`TearDown`, and `OneTimeSetUp`/`OneTimeTearDown`; [MSTest attributes](https://learn.microsoft.com/en-us/dotnet/core/testing/unit-testing-mstest-writing) define `TestClass`, `TestMethod`, `DataTestMethod`, `TestInitialize`/`TestCleanup`, `ClassInitialize`/`ClassCleanup`, and `AssemblyInitialize`/`AssemblyCleanup`; [xUnit.net shared context](https://xunit.net/docs/shared-context) defines the constructor, `IDisposable`, `IAsyncDisposable`, and `IAsyncLifetime` as the fixture hooks and `CollectionDefinition` as the collection declaration.

## C# named exclusions

`[TestFixtureSource]` is classified as a container, not a case: NUnit applies
it to a fixture class to supply constructor arguments, so it declares a
parameterized fixture rather than a parameterized method.

The xUnit lifecycle rule is name-based, so it is deliberately scoped. A
constructor, `InitializeAsync`, `Dispose`, or `DisposeAsync` earns a fixture
role only inside a type the attribute or member pass already marked as a test
container, and never overrides a role an attribute on the same member already
set. `fixtures/extraction/csharp/test_roles/source.cs` carries
`ManagedResource` — an ordinary `IDisposable` class with all four member names
— as the control that must stay unclassified.

Two named frameworks are excluded and recorded as `open_gaps` on the csharp
row: SpecFlow/Reqnroll step bindings (the executable case lives in a `.feature`
file, and `[Binding]`/`[Given]`/`[When]`/`[Then]` are unclassified) and
Machine.Specifications (cases are delegate fields, not callable symbols). Those
two entries sit under `kind_coverage.structural_facts.open_gaps` rather than
`test_detection`, because the `test_detection` vocabulary is frozen to
`test_case`, `test_container`, and `test_lifecycle` and each is already
classified exactly once for csharp.

## Registered evidence and controls

The reconciliation registers these new goldens in the capability matrix:

`html/test_roles`, `json/test_roles`, `markdown/test_roles`, `sql/test_roles`,
`toml/trycmd_roles`, `toml/nextest_roles`, `yaml/test_roles`, and
`xml/test_roles`. Existing `rust/test_roles`, `c/test_roles`,
`cpp/test_roles`, `zig/test_roles`, and `csharp/test_roles` remain registered
and are included in the matrix above.

The fixtures retain false-positive controls: qualified/member Mocha calls and
documents missing the Mocha marker; missing pgTAP runners and schemas; rustdoc `ignore` and non-Rust
fences; malformed or nested JSON lookalikes; incomplete trycmd and unmarked
nextest tables; YAML keys outside direct v2 command tests; and Ant report,
outside-target, id-only, and non-JUnit XML shapes. These controls establish
that `not_applicable` and unmarked symbols are deliberate contract boundaries,
not artifacts of missing extraction.
