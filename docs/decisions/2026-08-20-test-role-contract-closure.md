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
| JavaScript family (`javascript`, `jsx`, `typescript`, `tsx`) | Jest/Vitest, Playwright, Mocha BDD and TDD, `node:test`, and QUnit call DSLs, plus testdeck method decorators, in `javascript:test_roles`, `javascript:jest_vitest_roles`, `javascript:mocha_tdd_roles`, `jsx:test_roles`, `jsx:node_test_roles`, `typescript:test_roles`, `typescript:playwright_roles`, `tsx:test_roles`, and `tsx:qunit_roles` | supported | supported | supported |
| Go | `go test` name prefixes and the `_test.go` compile gate, `TestMain`, testify suite embedding and hooks, gocheck hooks, and the Ginkgo v2 node vocabulary in `go:test_roles` | supported | supported | supported |
| Java | JUnit 3 `TestCase` subclasses, JUnit 4/5 annotations, and TestNG annotations including the class-level `@Test` in `java:test_roles` | supported | supported | supported |
| Ruby | RSpec example groups, examples, hooks, and helpers; Minitest and Test::Unit base classes; the Rails `test` macro and its `setup`/`teardown` blocks in `ruby:test_roles` | supported | supported | supported |
| PHP | PHPUnit attributes, PHPDoc tags, fixture and `testXxx` method names, `TestCase` subclasses, and `#[DataProvider]`, plus the Pest call DSL, in `php:test_roles` | supported | supported | supported |
| Kotlin | JUnit 4/5, TestNG, and kotlin.test annotations shared with Java; the Kotest and Spek call DSLs including the StringSpec string-invoke, WordSpec `should`, and FreeSpec `-` forms; Kotest and Spek spec classes as containers in `kotlin:test_roles`, `kotlin:junit_tests`, `kotlin:kotest_string_spec`, and `kotlin:kotlin_test_lifecycle` | supported | supported | supported |
| Swift | XCTest `XCTestCase` subclasses, the `test` method prefix, and the four `setUp`/`tearDown` hooks; the Swift Testing `@Suite`, `@Test`, and `@Test(arguments:)` macros with suite `init`/`deinit`; and the Quick call vocabulary including shared groups and the `aroundEach` wrapping hook in `swift:test_roles` | supported | supported | supported |

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
- JavaScript family: [Jest globals](https://jestjs.io/docs/api) and [Vitest API](https://vitest.dev/api/) define `describe`/`test`/`it`, the `only`/`skip`/`todo`/`failing`/`concurrent` run modifiers, the four `before*`/`after*` hooks, `describe.each`/`test.each`, and Vitest's `bench`; [Mocha interfaces](https://mochajs.org/interfaces/) define the BDD `describe`/`context`/`it`/`specify` set, the TDD `suite`/`test`/`setup`/`teardown`/`suiteSetup`/`suiteTeardown` set, and the `x`/`f` prefixed aliases; [Playwright test annotations](https://playwright.dev/docs/api/class-test) define `test`, `test.describe` with its `serial`/`parallel` modes, the `test.before*`/`test.after*` hooks, and `test.step`; the [Node.js test runner](https://nodejs.org/api/test.html) defines `test`/`describe`/`it`, the `before`/`after`/`beforeEach`/`afterEach` hooks, and the `TestContext` subtest methods `t.test` and `t.beforeEach`; [QUnit](https://qunitjs.com/api/QUnit/) defines `QUnit.module` and `QUnit.test` and passes lifecycle through a `hooks` callback parameter; [testdeck](https://testdeck.org/) defines the `@suite`, `@test`, and `@params` decorators.
- PHP: [PHPUnit attributes](https://docs.phpunit.de/en/11.5/attributes.html) define `#[Test]`, `#[Before]`, `#[After]`, `#[BeforeClass]`, `#[AfterClass]`, and `#[DataProvider]`, and document the `@test`, `@before`, `@after`, and `@dataProvider` PHPDoc spellings they replace; [PHPUnit fixtures](https://docs.phpunit.de/en/11.5/fixtures.html) define `setUp`, `tearDown`, `setUpBeforeClass`, and `tearDownAfterClass` on a `PHPUnit\Framework\TestCase` subclass; [PHPUnit organizing tests](https://docs.phpunit.de/en/11.5/organizing-tests.html) defines the `*Test.php` suffix a directory suite collects; [Pest](https://pestphp.com/docs/writing-tests) defines the `test()`/`it()` cases, `describe()` groups, and `beforeEach`/`afterEach`/`beforeAll`/`afterAll` hooks.
- Swift: [`XCTestCase`](https://developer.apple.com/documentation/xctest/xctestcase) defines the subclass contract, the `test` method prefix that XCTest collects, and the `setUp`/`setUpWithError`/`tearDown`/`tearDownWithError` hooks; [Swift Testing](https://developer.apple.com/documentation/testing) defines the `@Test` and `@Suite` macros, the `arguments:` parameterized form, and the per-case suite instance that makes `init` and `deinit` the setup and teardown hooks; [Quick](https://github.com/Quick/Quick/blob/main/Documentation/en-us/QuickExamplesAndGroups.md) defines `describe`/`context`/`it` with their `x`/`f` aliases, `beforeEach`/`afterEach`/`beforeSuite`/`afterSuite`/`justBeforeEach`/`aroundEach`, and the `sharedExamples`/`itBehavesLike` group pair.

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

- Go: the [`go test` command documentation](https://pkg.go.dev/cmd/go#hdr-Testing_flags) and the [`testing` package](https://pkg.go.dev/testing) define the `_test.go` file suffix, the `TestXxx`/`BenchmarkXxx`/`FuzzXxx`/`ExampleXxx` name prefixes, the rule that the character after the prefix must not be lower-case, and `TestMain` as the package entry point that wraps `m.Run()`; [testify's suite package](https://pkg.go.dev/github.com/stretchr/testify/suite) defines the embedded `suite.Suite`, `SetupSuite`/`TearDownSuite`, `SetupTest`/`TearDownTest`, `SetupSubTest`/`TearDownSubTest`, and `BeforeTest`/`AfterTest` interfaces; [gocheck](https://pkg.go.dev/gopkg.in/check.v1) defines `SetUpSuite`/`TearDownSuite` and `SetUpTest`/`TearDownTest`; [Ginkgo v2](https://onsi.github.io/ginkgo/) defines the container, subject, and setup node vocabulary and states that the spec tree is built at file scope with the suite as the implicit root.
- C#: [NUnit attributes](https://docs.nunit.org/articles/nunit/writing-tests/attributes.html) define `TestFixture`, `SetUpFixture`, `Test`, `TestCase`, `TestCaseSource`, `TestFixtureSource`, `SetUp`/`TearDown`, and `OneTimeSetUp`/`OneTimeTearDown`; [MSTest attributes](https://learn.microsoft.com/en-us/dotnet/core/testing/unit-testing-mstest-writing) define `TestClass`, `TestMethod`, `DataTestMethod`, `TestInitialize`/`TestCleanup`, `ClassInitialize`/`ClassCleanup`, and `AssemblyInitialize`/`AssemblyCleanup`; [xUnit.net shared context](https://xunit.net/docs/shared-context) defines the constructor, `IDisposable`, `IAsyncDisposable`, and `IAsyncLifetime` as the fixture hooks and `CollectionDefinition` as the collection declaration.

- Java: [JUnit 5 annotations](https://docs.junit.org/current/user-guide/#writing-tests-annotations) define `@Test`, `@ParameterizedTest`, `@RepeatedTest`, `@TestFactory`, `@TestTemplate`, `@Nested`, and the `@BeforeEach`/`@AfterEach`/`@BeforeAll`/`@AfterAll` hooks; [JUnit 4](https://github.com/junit-team/junit4/wiki/Test-fixtures) defines `@Before`/`@After` and `@BeforeClass`/`@AfterClass`; [`junit.framework.TestCase`](https://junit.org/junit4/javadoc/latest/junit/framework/TestCase.html) defines the JUnit 3 subclass-and-`testXxx` contract; [TestNG annotations](https://testng.org/#_annotations) define `@Test` on a class or a method and the `@BeforeSuite`/`@AfterSuite`, `@BeforeTest`/`@AfterTest`, `@BeforeGroups`/`@AfterGroups`, `@BeforeClass`/`@AfterClass`, and `@BeforeMethod`/`@AfterMethod` hooks; the [JUnit Platform Suite engine](https://docs.junit.org/current/user-guide/#junit-platform-suite-engine) defines `@Suite`; [Cucumber-JVM](https://github.com/cucumber/cucumber-jvm/blob/main/docs/step-definitions.md) defines glue-class step bindings.

## Java named contract and exclusions

TestNG's class-level `@Test` marks the class a container and every public
method of that class a case, because TestNG runs them that way. A hook
annotation on such a method wins over the class-level rule. Non-public members
are excluded, and `fixtures/extraction/java/test_roles/test_source.java` carries
`LedgerTestNgTest.helperTotal` as the control that must stay unclassified.

`@ParameterizedTest` and `@RepeatedTest` report `parameterized_test`, because
one declaration runs one result per argument source or per repetition.
`@TestFactory` and `@TestTemplate` stay `test_case`: each is one declaration
the engine expands at run time, and the expansion is not visible in the source.

JUnit 3 has no annotation, so the contract falls back to the `test` name prefix
guarded by the shared test-path check. Ordinary Java shares that vocabulary —
listeners and extensions are full of `testName` and `testMethod` — so the
fallback is scoped with `normalize_scoped_test_roles`: a callable that no test
container encloses loses the role. `LedgerTestHelpers.testDataForLedger` is the
golden's control. Scoping runs for Kotlin too, since the Kotest and Spek spec
classes that hold its call-DSL roles are now marked containers. A second pass
re-derives every role an annotation alone justifies, so scoping by position
cannot strip an annotated member.

Two named frameworks are excluded and recorded as `open_gaps` on the java row:
Cucumber-JVM step bindings (the executable scenario lives in a `.feature` file,
and `@Given`/`@When`/`@Then` on a glue class are unclassified) and JUnit
Platform `@Suite` containers (the suite class holds no test member of its own).
Those two entries sit under `kind_coverage.structural_facts.open_gaps` rather
than `test_detection`, for the same reason as the csharp pair below: the
`test_detection` vocabulary is frozen to three units and each is already
classified exactly once for java.

Real-world precision and recall measurements against the TestNG and JUnit
source trees are in `docs/languages/java.md`.

- Ruby: [RSpec example groups](https://rspec.info/features/3-13/rspec-core/example-groups/basic-structure/) define `describe`/`context` and `it`/`specify`/`example`, [`xit`/`fit` and the `x`/`f` prefixes](https://rspec.info/features/3-13/rspec-core/filtering/) define skipped and focused aliases, [hooks](https://rspec.info/features/3-13/rspec-core/hooks/before-and-after-hooks/) define `before`/`after`, [`around` hooks](https://rspec.info/features/3-13/rspec-core/hooks/around-hooks/) define the wrapping hook, [helper methods](https://rspec.info/features/3-13/rspec-core/helper-methods/let/) define `let`/`let!`/`subject`, and [shared examples](https://rspec.info/features/3-13/rspec-core/example-groups/shared-examples/) define `shared_examples`/`shared_context` and the `it_behaves_like`/`include_examples` call forms; [Minitest](https://github.com/minitest/minitest) collects `test_`-prefixed methods from a `Minitest::Test` subclass and calls `setup`/`teardown` around each one; [`ActiveSupport::TestCase`](https://guides.rubyonrails.org/testing.html) adds the `test "name" do` macro and block-form `setup`/`teardown`, and `ActionDispatch::IntegrationTest` extends it.

## Ruby named exclusions

Every name in the Ruby test vocabulary is ordinary Ruby somewhere else, and
none of it is syntax the parser can tell apart on its own. `setup`, `before`,
and `test` are plain method calls, and `describe` is a plain method name. The
contract therefore takes three guards together, and a role needs all three.

The file must read as a test path. The call must be bare or sent to `RSpec`
itself, so `runner.describe "x"` stays an ordinary message to an object that
answers `describe`. And a callable must sit inside a test container, so the
scoping pass strips a role from a `def setup` in a spec-directory support
class. RSpec blocks are their own containers; a Minitest-family suite is found
through the class's `base_types` metadata, for `Minitest::Test`,
`Test::Unit::TestCase`, `ActiveSupport::TestCase`, and
`ActionDispatch::IntegrationTest`.

`around` is the first hook in any supported language that wraps a case on both
sides. It reports `fixture_setup`, through the `Ambiguous` lifecycle direction,
because a wrapping hook always runs its setup half first.

Two RSpec surfaces are excluded and recorded as `open_gaps` on the ruby row:
shared example group references (`it_behaves_like`, `include_examples` name a
group defined elsewhere, so a symbol row would be a second definition) and
example metadata tags (`:slow`, `type: :model` are call arguments, and Ruby has
no annotation syntax to carry them). Both entries sit under
`kind_coverage.structural_facts.open_gaps`, for the same reason the two csharp
entries do.

- Kotlin: [kotlin.test](https://kotlinlang.org/api/core/kotlin-test/kotlin.test/) defines `@Test`, `@BeforeTest`/`@AfterTest`, and `@BeforeClass`/`@AfterClass`, and maps them onto the platform framework; the JUnit and TestNG contracts in the Java bullet above apply unchanged to Kotlin sources; [Kotest spec styles](https://kotest.io/docs/framework/testing-styles.html) define `StringSpec`, `FunSpec`, `DescribeSpec`, `ShouldSpec`, `WordSpec`, `FreeSpec`, `BehaviorSpec`, `FeatureSpec`, `ExpectSpec`, and `AnnotationSpec` together with the `test`/`it`/`should`/`then`/`scenario`/`expect` case words, the `describe`/`context`/`given`/`When`/`and`/`feature` group words, the `x`-prefixed disabled spellings, the StringSpec `"name" { }` string-invoke form, the WordSpec `"subject" should { }` infix form, and the FreeSpec `"subject" - { }` operator form; [Kotest lifecycle hooks](https://kotest.io/docs/framework/lifecycle-hooks.html) define `beforeTest`/`afterTest`, `beforeEach`/`afterEach`, and `beforeAll`/`afterAll`; [Spek](https://www.spekframework.org/) defines the `describe`/`it` specification style and the `beforeEachTest`/`afterEachTest` and `beforeGroup`/`afterGroup` hooks.

## Kotlin named contract and exclusions

Kotlin publishes roles from two independent sources and both are adopted.

The annotation source is Java's. `java` and `kotlin` share `detect_java_kotlin`,
the annotation key lists, and `mark_java_test_containers`, so JUnit 3/4/5,
TestNG, and kotlin.test all classify from the same rules. kotlin.test's
`@BeforeTest` and `@AfterTest` share the TestNG keys `beforetest` and
`aftertest`, so they need no Kotlin-only arm.
`fixtures/extraction/kotlin/kotlin_test_lifecycle/source.kt` is the evidence,
and it also carries the TestNG class-level rule for Kotlin: Kotlin members of a
class earn `SymbolKind::Method` and `Visibility::Public` by default, which is
exactly what that rule requires, so `LedgerTestNgTest.postsAnEntry` is a case
and the private `helperTotal` stays unclassified.

The call source is Kotest and Spek. Three of those forms are not named calls at
all, and each names the Kotest function that declares it:

- StringSpec, and the leaf step of WordSpec and FreeSpec, write a case as
  `"name" { }`. Kotest declares that with
  `operator fun String.invoke(test: suspend TestScope.() -> Unit)`, so the
  captured callee is `invoke`.
- WordSpec opens a group with `"subject" should { }`, an infix extension on
  `String`, so the captured callee is `should` and the group is named
  `"subject should"`.
- FreeSpec opens a group with `"subject" - { }`, declared as
  `operator fun String.minus(...)`, so the captured callee is `minus`.

Subtracting or invoking a lambda on a string has no other meaning in Kotlin,
which is what makes those three guards safe.

The `x`-prefixed spellings — `xdescribe`, `xcontext`, `xit`, `xtest` — earn the
same role as the enabled spelling, because the runner reports a disabled step as
skipped rather than dropping it.

A Kotest or Spek spec class carries no annotation, so
`mark_kotlin_test_containers` takes two proofs and either one is enough: the
class extends a named spec base type, or a call-DSL step already earned a role
and named the class as its parent. The second proof is what catches a project's
own spec base class, which no name list can know.

That container marker is what lets Kotlin take the same scoping the Java row
describes. `normalize_scoped_test_roles` now runs for every language that
reaches `mark_java_test_containers`, and the Java-only gate is deleted.
`LedgerTestHelpers.testDataForLedger` in the kotlin.test golden is the control
that must lose its name-convention role.

Backticked names are normalized. Kotlin lets a test method be written
`` fun `adds two numbers`() `` and the grammar keeps the backticks in the
identifier text, but every runner and report prints the name without them.
`symbols.name` and `identifiers.name` are therefore both stripped, so a call
site still matches its definition, and the escaped spelling stays available in
the symbol signature and in the `rawName` metadata key.

A lifecycle hook is written `beforeEach { }`, so the call adapter names the
symbol after the callee. The containing-symbol lookup then returned that same
symbol for the call node it was built from, and resolving the callee found it
again — a `beforeEach` calls `beforeEach` edge that describes nothing. A symbol
whose span is exactly the call node no longer opens a call edge; a declaration
never shares a span with a call expression, so a recursive function keeps its
real self-call edge.

Two named Kotest surfaces are excluded and recorded as `open_gaps` on the kotlin
row: data-driven testing (`forAll(rows) { … }`, where the rows are call
arguments rather than declarations, so there is no per-row symbol and no
`parameterized_test` role) and property testing (`checkAll`/`forAll` over
generators inside an already-classified step, so the generator block carries no
role of its own). Both sit under `kind_coverage.structural_facts.open_gaps`
rather than `test_detection`, for the same reason as the java, csharp, and ruby
entries: the `test_detection` vocabulary is frozen to three units and each is
already classified exactly once for kotlin.

Gradle's extra test source sets need no Kotlin rule: `is_test_path` already
accepts `integrationTest`, `functionalTest`, `androidTest`, and `testFixtures`.

Real-world precision and recall measurements are in `docs/languages/kotlin.md`.

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

## JavaScript family named contracts and exclusions (2026-08-25)

The four dialects share one classifier,
`crates/julie-extractors/src/javascript/test_symbols.rs`, so they share one
contract. Three choices in it are deliberate.

**Detection is gated.** The DSL vocabulary — `setup`, `teardown`, `before`,
`after`, `context`, `suite` — is ordinary production vocabulary. A file is read
for test DSL only when its path is a test path or it imports a named test
framework. `javascript:mocha_tdd_roles` proves the path gate; every other
JavaScript-family golden proves the import gate. The zod corpus in
`docs/languages/typescript.md` shows why both are needed: a Vitest global setup
file in `scripts/` carries real hooks that a path-only rule would drop.

**`.each` declares cases, not a group.** `describe.each(table)("name", fn)`
reports `parameterized_test`, not `test_container`. A table-driven `describe`
runs one case per table row, so what it declares is a case set. Both `.each`
spellings are in `javascript:jest_vitest_roles`.

**Decorator support covers methods, not classes.** testdeck `@test` and
`@params` on a method report `test_case` and `parameterized_test`. A testdeck
`@suite` class reports no role, because the decorator pass classifies callables
only. `typescript:test_roles` carries a decorated `@suite` class as the control
that must stay unclassified; container evidence for TypeScript comes from
`describe(...)` and Playwright `test.describe(...)` instead.

Four constructs are named exclusions rather than open gaps, because the
`test_detection` vocabulary is frozen to `test_case`, `test_container`, and
`test_lifecycle` and each is already classified exactly once for all four
dialects:

- `test.step(...)`: a Playwright step is a report annotation inside a case, not
  a case. Control in `typescript:playwright_roles`.
- `hooks.beforeEach(...)` inside `QUnit.module("name", (hooks) => …)`: `hooks`
  is a runtime callback parameter, so no name rule can separate it from any
  other object named `hooks`. Control in `tsx:qunit_roles`.
- Bare `QUnit.only(...)`, `QUnit.skip(...)`, `QUnit.todo(...)`: dropping the run
  modifier leaves the namespace `QUnit` with no DSL word behind it. Controls in
  `tsx:qunit_roles`.
- An aliased decorator import (`import { test as testdeckTest }` used as
  `@testdeckTest`): the annotation key is the written name, so the alias
  resolves to nothing. Closing it needs import-aware annotation normalization.

`tape` is adopted only as a gate: its module specifier opens detection, and its
`test(t, …)` idiom is then covered by the shared `test` word. It gets no
tape-specific rule and no separate golden.

One residual false-positive source is recorded, not excluded: a *declared*
callable literally named `describe`, `it`, or `test` inside a test file earns
`test_case` through `detect_js_ts`. Measured cost across the express and zod
corpora is 3 rows in 4,328; see `docs/languages/javascript.md`.

## Go named decisions (2026-08-25)

Go previously wrote `is_test` by hand in its extractor, so it could never
publish a lifecycle or a container role. Go callables now go through
`apply_callable_test_metadata` like every other language, and four contract
decisions land with that change.

`Benchmark` is a `test_case`. `go test -list` lists benchmarks beside tests,
fuzz targets, and examples, and `go test -bench` runs them, so a benchmark is a
selectable unit of work. The earlier rule excluded the prefix, which made a
benchmark-only file invisible; the five corpora in `docs/languages/go.md`
contain 142 such rows.

`TestMain` is a lifecycle hook, not a case. It matches the `Test` prefix, but
it wraps the whole package run around `m.Run()` and `go test -run` cannot
select it. It is an around-style hook, so it takes
`TestLifecycleDirection::Ambiguous` and reports the single honest direction,
`fixture_setup`. Go is the first language to use that variant.

A test container is a struct declared in a `_test.go` file that embeds a
qualified type whose final segment is `Suite`. That is testify's `suite.Suite`,
spelled so an aliased import still matches. Go attaches suite methods through
their receiver type rather than through lexical nesting, so a suite method is
not a child symbol of its struct; method roles come from the name plus the
`_test.go` gate, and the container row records the suite itself.

Ginkgo gets two guards, because its vocabulary is ordinary Go identifiers.
Ginkgo calls are read as tests only when `go test` compiles the file or the
file imports `github.com/onsi/ginkgo`. Inside such a file, a spec or hook
written in an ordinary function body loses its role through
`normalize_scoped_test_roles`, because Ginkgo builds its spec tree before any
spec runs and a node declared at run time never joins a suite. Ginkgo treats
the suite as the implicit root, so a top-level `It` or `BeforeSuite` is a real
node and is deliberately left out of that scoping; scoping top-level nodes as
well was measured first and dropped 39 real rows in Ginkgo's own repository,
including every `BeforeSuite` and `AfterSuite`. The measured residual cost of
the shipped rule is six rows out of 4,912.

Three named gaps are recorded as `open_gaps` on the go row rather than claimed:
`t.Run` subtest names (`go.subtest_names`), a `go.mod`/`go.sum` manifest
language keyed on an exact basename the way `qmldir` is
(`go.module_manifest_language`), and gocheck's `Suite(&T{})` container
registration (`gocheck.suite_registration`). All three sit under
`kind_coverage.structural_facts.open_gaps` for the same reason the C# entries
do: the `test_detection` vocabulary is frozen to three roles and each is
already classified exactly once for go.

## PHP named decisions (2026-08-25)

PHPUnit spells the same metadata as an attribute and as a PHPDoc tag, so the
PHP extractor reads the `@test`, `@before`, `@after`, `@beforeClass`, and
`@afterClass` tags out of the docblock and hands each to the shared detector
under the key its attribute produces. One vocabulary then covers both
spellings, and a tag must match whole, so `@tested` is not `@test`.

A `#[DataProvider]`-referenced method is a helper, not a case: it supplies
argument rows and asserts nothing. No rule is needed to exclude it — a provider
carries no hook metadata and is not `test`-prefixed. The method that names the
provider reports `parameterized_test`, because PHPUnit reports one result per
row.

Two proofs work outside a test path, which closes the gap where a suite sits in
a production directory. A class extending `TestCase` is a container, compared
on the last namespace segment so the short imported name and the fully
qualified `\PHPUnit\Framework\TestCase` both match. And the `*Test.php`
filename is a test path, which is what PHPUnit's own suffix configuration
collects. A container's `testXxx` and `setUp`/`tearDown` members are then
classified by the member pass, the same shape the C# xUnit member pass uses.

The name rules stay guarded in both directions. `testConnection()` and
`setUp()` are ordinary PHP, so a name earns a role only inside a test path or
inside a marked container. Pest's `test()` and `it()` are ordinary function
calls, so outside a test path a Pest role survives only inside a container,
through `normalize_scoped_test_roles`. `legacy_suite.php` carries
`ConnectionProbe` with both names as the production control, and
`production_roles.php` calls the whole Pest DSL at file scope and publishes
nothing.

Three named frameworks are recorded as `open_gaps` on the php row rather than
claimed: Codeception `*Cest.php` classes with their `_before`/`_after` hooks
and actor-argument cases (`codeception.cest_and_actor_roles`), Behat step
attributes on a context class whose scenario lives in a `.feature` file
(`behat.step_definition_roles`), and PHPSpec `ObjectBehavior` subclasses with
`it_`/`its_` examples (`phpspec.example_roles`). All three sit under
`kind_coverage.structural_facts.open_gaps` for the same reason the C# and go
entries do: the `test_detection` vocabulary is frozen to three roles and each
is already classified exactly once for php.

## Swift ledger correction and named decisions (2026-08-25)

The swift row claimed `test_container` and `test_lifecycle` before either role
had a declaration-driven emission path. Only the Quick call adapter published
them, and that adapter reached one golden block. XCTest classes published no
container row at all, and `setUp`, `setUpWithError`, `tearDown`, and
`tearDownWithError` published `is_test` alone, so the four hooks were reported
as cases. Swift Testing was worse: `detect_swift` never read the annotation
keys, so `@Test`, `@Test(arguments:)`, and `@Suite` published nothing at all
even though the annotation markers were already recorded. The correction adds
the emission paths, so the three claims now rest on the golden rows the swift
`test_roles` fixture carries.

Four contract decisions land with that change.

The macro is definitive and the name is not. `@Test` and `@Suite` name a test
in the source, so a `@Test` function in `Sources/` is a real case and the swift
fixture proves it from a production path. XCTest's `test` prefix and its four
hook names are ordinary Swift elsewhere, so they keep the path guard and are
scoped to a container by `normalize_scoped_test_roles`. The scoping pass runs
over every swift file, then the macro roles are re-derived, exactly as the Java
pass restores an annotated top-level Kotlin function.

An extension of a container is itself a container. Swift splits a type across
extensions and XCTest runs a `test`-prefixed method declared in one, and an
extension is its own symbol with its own children, so without this rule the
scoping pass would strip every case an extension holds. The match is by the
extended type's name within the file, which is what an extension records; an
extension of a container declared in another file is out of reach of a per-file
extractor.

`init` and `deinit` earn a fixture role only inside a container. Swift Testing
builds one instance of a suite per case, so `init` runs before the case and
`deinit` runs after it; outside a suite both names are ordinary Swift. This is
the rule the xUnit constructor already takes in C#. `deinit` is a `Destructor`
symbol rather than a callable, so the swift container pass assigns both roles
instead of the shared callable path.

`aroundEach` reports `fixture_setup` through the `Ambiguous` lifecycle
direction, because a wrapping hook always runs its setup half first. Ruby's
`around` and Go's `TestMain` take the same direction.

`itBehavesLike` is a container. Quick inserts the examples of the named shared
group at the call site, so the row names a group, not a case. Ruby records the
matching RSpec call as an open gap instead, because an RSpec shared group
usually lives in another file and a symbol row there would publish a second
definition. A Quick shared group and its use commonly sit in one spec file, and
the invocation site is where the examples run, so swift publishes both rows.

Two surfaces are excluded and recorded as `open_gaps` on the swift row:
QuickSpec subclass containers (`quick.quickspec_subclass_container`, where the
groups and examples publish roles but the `class X: QuickSpec` itself does not)
and Swift Testing traits (`swift_testing.test_traits`, where `.tags`,
`.disabled`, and `.serialized` are macro arguments that the annotation
normalizer drops). Both entries sit under
`kind_coverage.structural_facts.open_gaps` for the same reason the C# and go
entries do: the `test_detection` vocabulary is frozen to three roles and each is
already classified exactly once for swift.

## Registered evidence and controls

The reconciliation registers these new goldens in the capability matrix:

`html/test_roles`, `json/test_roles`, `markdown/test_roles`, `sql/test_roles`,
`toml/trycmd_roles`, `toml/nextest_roles`, `yaml/test_roles`, and
`xml/test_roles`. Existing `rust/test_roles`, `c/test_roles`,
`cpp/test_roles`, `zig/test_roles`, `csharp/test_roles`, and `go/test_roles`
remain registered and are included in the matrix above.

`go/test_roles` was rewritten from an eleven-line stub into a realistic
multi-framework file. It carries the standard-library prefixes including
`TestMain`, a benchmark, a fuzz target, and an example; a testify suite with
all four hook pairs; a gocheck suite; a Ginkgo tree; and four controls that
must stay unclassified: `Testable` (lower-case character after the prefix),
`AddsLikeATest`, a `recordingClock` struct embedding `sync.Mutex`, and an `It`
declared inside a plain helper function.

`php/test_roles` was rewritten from a single mixed file into four sources:
`ArithmeticTest.php` (the PHPUnit class, its four fixture names, both hook
spellings, `#[Test]`, the `testXxx` prefix, `#[DataProvider]`, and the provider
and helper controls), `PestFeatureTest.php` (the Pest DSL and the
`$ordinary->test(...)` member-call control), `legacy_suite.php` (a fully
qualified `TestCase` subclass and a `#[Test]`-holding class in a production
path, beside the `ConnectionProbe` control), and `production_roles.php` (the
production-path Pest control).

`swift/test_roles` was extended from a single 23-line source into a two-source
fixture. `test_source.swift` carries the XCTest container with all four hooks,
a case, a case declared in an extension, a non-test method, and the full Quick
tree including `sharedExamples`/`itBehavesLike` and the suite and wrapping
hooks; `production_roles.swift` carries the Swift Testing suite, case,
parameterized case, and `init`/`deinit` hooks from a production path. Four
controls must stay unclassified: `CalculatorSupport`, an in-test-path struct
with a `test`-prefixed method and a `setUp`; an extension of that struct
holding another `test`-prefixed method; `NetworkClient`, a production class
with `testConnection`, `setUp`, `init`, and `deinit`; and the top-level
`itNamedButNotCalled` function.

The JavaScript-family closure adds five framework goldens across the four
dialect rows: `javascript/jest_vitest_roles`, `javascript/mocha_tdd_roles`,
`typescript/playwright_roles`, `jsx/node_test_roles`, and `tsx/qunit_roles`.
The four existing `test_roles` goldens stay registered; `typescript/test_roles`
gains the testdeck decorator evidence.

The fixtures retain false-positive controls: qualified/member Mocha calls and
documents missing the Mocha marker; missing pgTAP runners and schemas; rustdoc `ignore` and non-Rust
fences; malformed or nested JSON lookalikes; incomplete trycmd and unmarked
nextest tables; YAML keys outside direct v2 command tests; and Ant report,
outside-target, id-only, and non-JUnit XML shapes. These controls establish
that `not_applicable` and unmarked symbols are deliberate contract boundaries,
not artifacts of missing extraction.

## Single-writer closure (2026-08-25)

`apply_test_role` is now the only writer of the three test booleans. Thirteen
languages still set `is_test`, `test_lifecycle`, or `test_container` by hand and
therefore published a boolean with no `test_role` string: C, C++, Zig, Elixir,
Erlang, Lua, R, SQL, Markdown, JSON, TOML, YAML, and XML. Every one of them now
routes through `apply_test_role` or `apply_callable_test_metadata`.

The routing kept every boolean exactly as it was. Across the registered corpus
the same 434 symbols carry a test flag before and after, with the same
`is_test`/`test_lifecycle`/`test_container` values; the only change is that the
74 symbols that carried a boolean without a role now carry the agreeing role.

Two parallel role vocabularies were removed in the process. `TomlTestRole`,
`JsonTestRole`, `PgTapRoutineRole`, and `ErlangTestRole` each restated a subset
of `TestRole`; each now returns `TestRole` directly, so a language cannot drift
from the shared spelling.

Four language-local writers had to name a direction they previously did not
record. Criterion takes it from the `.init`/`.fini` designated initializer at
the call site, not from the hook's own name. Common Test takes it from the
`init_per_*`/`end_per_*` prefix. pgTAP takes it from the
`startup`/`setup`/`teardown`/`shutdown` prefix. Google container-structure-test
takes it from the literal `setup`/`teardown` key. GoogleTest fixture hooks take
it from `SetUp*`/`TearDown*` after any `Class::` qualifier.

GoogleTest `TEST_P` and `TYPED_TEST_P` now publish `parameterized_test`. The
synthetic macro-keyword annotation already carried that evidence and the
extractor comment already named the intent; only the published role was
missing. `TEST`, `TEST_F`, and `TYPED_TEST` publish `test_case`.

### Invariants under test

Three gates hold the closure:

- `every_golden_test_boolean_carries_an_agreeing_test_role` walks every symbol
  of every registered golden and fails on any test boolean without a matching
  `test_role`, and on any role whose booleans disagree with what
  `apply_test_role` writes for it. It scans the whole registry rather than a
  chosen list, and asserts a floor on the languages and symbols it saw so a
  broken scan cannot pass empty.
- `every_shared_lifecycle_word_publishes_its_declared_direction` walks every
  lifecycle word of every vocabulary in `SHARED_TEST_CALL_VOCABS` and requires a
  declared half for each. `test_call_role` infers the half from the callee name,
  so a future word spelled `dispose` or `reset` would otherwise publish
  `fixture_setup` in silence; the gate fails the build instead.
- `every_language_with_a_shared_vocabulary_is_registered` scans the production
  sources for vocabulary declarations and fails when one is missing from the
  registry, so a new language cannot skip the direction gate.

## Python decorator scope defect (2026-08-25)

`find_decorated_node` walked up to the nearest `decorated_definition` without
stopping at an enclosing function or class. A nested `def` therefore inherited
the decorators of the function that contained it, and every method of a
decorated class inherited the class decorator. Measured on Flask, the defect
produced six wrong roles and eight false positives: a closure inside a
`@pytest.fixture` helper was reported as `fixture_setup`, and a helper inside a
`@pytest.mark.parametrize` test was reported as `parameterized_test`.

The walk now stops at the first enclosing `function_definition` or
`class_definition`. Decorators reach only the definition they are written on.
