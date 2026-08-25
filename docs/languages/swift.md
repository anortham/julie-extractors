# Swift support

Julie registers one Swift language: `swift` handles `.swift` files.

## Continuous testing

Run the language target when changing Swift extraction:

```bash
cargo xtask test language swift
```

The command runs the Swift unit-test modules and the golden extraction test
with `JULIE_GOLDEN_LANGUAGE=swift`. The normal golden target stays unfiltered:

```bash
cargo xtask test golden
```

## Test-role contract

Swift ships three test frameworks and none of them marks a suite the same way.
XCTest subclasses a base class, Swift Testing applies a macro, and Quick makes
a call. The contract reads all three.

| Idiom | Role | Source of the rule |
| --- | --- | --- |
| `class X: XCTestCase` | `test_container` | XCTest base class |
| `extension X` of a container in the same file | `test_container` | XCTest and Swift Testing split declarations |
| `func testXxx` in a container | `test_case` | XCTest method prefix |
| `setUp`, `setUpWithError` | `fixture_setup` | XCTest per-test hooks |
| `tearDown`, `tearDownWithError` | `fixture_teardown` | XCTest per-test hooks |
| `@Suite` on a struct, class, enum, or actor | `test_container` | Swift Testing suite macro |
| `@Test` on a function or method | `test_case` | Swift Testing test macro |
| `@Test(arguments:)` | `parameterized_test` | Swift Testing argument rows |
| `init` in a container | `fixture_setup` | Swift Testing per-case instance |
| `deinit` in a container | `fixture_teardown` | Swift Testing per-case instance |
| `describe`, `context` call (+ `x`/`f` aliases) | `test_container` | Quick example group |
| `sharedExamples`, `itBehavesLike` call | `test_container` | Quick shared example group |
| `it`, `specify`, `pending` call (+ `x`/`f` aliases) | `test_case` | Quick example |
| `beforeEach`, `beforeAll`, `beforeSuite`, `justBeforeEach` call | `fixture_setup` | Quick hooks |
| `afterEach`, `afterAll`, `afterSuite` call | `fixture_teardown` | Quick hooks |
| `aroundEach` call | `fixture_setup` | Quick wrapping hook |

### The macro is definitive, the name is not

`@Test` and `@Suite` name a test in the source, so they need no other evidence.
A `@Test` function in `Sources/` is a real case, and the Swift Testing rows in
`fixtures/extraction/swift/test_roles/production_roles.swift` prove it from a
production path.

Every other rule keys on a name that is ordinary Swift somewhere else, so those
rules take two guards together.

- **Path.** The file must read as a test path. `Tests.swift`, a `Tests/`
  directory, and the other shared rules all qualify. Without this guard a
  production `func testConnection()` would carry a role.
- **Container.** A callable must sit inside a test container. XCTest suites are
  found through the `base_types` metadata the type extractor emits, Swift
  Testing suites through the `@Suite` macro, and Quick groups through the call
  adapter. A `func testHelperNamedLikeACase()` in a support struct therefore
  earns no role, and neither does a `func setUp()` there.

`CalculatorSupport` in `fixtures/extraction/swift/test_roles/test_source.swift`
is the in-test-path control: a struct with a `test`-prefixed method and a
`setUp` method that publishes no role at all, plus an extension of it whose
`test`-prefixed method stays unclassified too. `NetworkClient` in
`production_roles.swift` is the production-path control: `testConnection`,
`setUp`, `init`, and `deinit` all stay unclassified.

### An extension of a container is a container

Swift splits a type across extensions, and XCTest runs a `test`-prefixed method
declared in one. An extension is its own symbol with its own children, so it
must be a container in its own right or the scoping pass strips every case it
holds. The match is by the extended type's name within the file, because that
is what an extension records. An extension of a container declared in another
file is out of reach of a per-file extractor.

### `init` and `deinit` need a container

Swift Testing builds one instance of a suite per case, so `init` runs before
the case and `deinit` runs after it. Both names are ordinary Swift, so they
earn a fixture role only inside a marked container — the same rule the xUnit
constructor takes in C#. `deinit` is a `Destructor` symbol rather than a
callable, so the container pass assigns both roles instead of the shared
callable path.

### `aroundEach` reports setup

Quick's `aroundEach` receives the example and runs it, so it wraps the case on
both sides. Its true direction is "both". The extractor cannot split a wrapping
hook without reading the body, and the setup half always runs first, so the
contract publishes the single honest direction: `fixture_setup`. Ruby's
`around` and Go's `TestMain` take the same direction.

### `itBehavesLike` is a container

`itBehavesLike("a calculator")` runs the group that `sharedExamples("a
calculator")` declares, and Quick inserts that group's examples at the call
site. The row is therefore a container, not a case. Ruby records the matching
RSpec call as an open gap instead, because RSpec's shared group usually lives
in another file and the Ruby contract chose not to publish a second row for it.
Quick's group and its use commonly sit in one spec file, and the invocation
site is where the examples run, so Swift publishes both rows.

## Recorded gaps

Two surfaces are recorded as `open_gaps` on the swift row in
`fixtures/extraction/capabilities.json`, under
`kind_coverage.structural_facts.open_gaps`. The `test_detection` vocabulary is
frozen to `test_case`, `test_container`, and `test_lifecycle`, and swift
classifies each exactly once, so a swift-specific gap cannot live there.

- `quick.quickspec_subclass_container`. Quick declares a spec as
  `class CalculatorSpec: QuickSpec` whose `override func spec()` body holds the
  `describe`/`it` tree. The groups and examples publish their roles, but the
  subclass itself publishes no container row.
- `swift_testing.test_traits`. `@Test(.tags(.slow))`, `@Test(.disabled("flaky"))`,
  and `@Suite(.serialized)` attach traits through extra macro arguments. The
  annotation normalizer keys on the macro name and drops its argument list, so a
  skip, a tag, and a serialization constraint reach no channel.

## Evidence

The golden fixture `swift:test_roles` registers two sources:

| Source | What it proves |
| --- | --- |
| `test_source.swift` | the XCTest container, its four hooks, a case, a case in an extension, a non-test method, the Quick tree including the shared group and the suite and wrapping hooks, and the in-test-path controls |
| `production_roles.swift` | the Swift Testing suite, case, parameterized case, `init`/`deinit` hooks, a top-level `@Test` function, and the production-path control, all outside a test path |

The registered goldens observe 7 `test_case` rows, 7 `test_container` rows, and
11 `test_lifecycle` rows for swift.

No real-world corpus scan was run for this contract. The evidence above is
golden-fixture evidence only.
