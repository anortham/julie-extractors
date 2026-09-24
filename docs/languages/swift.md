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

## Extraction contract

The golden fixture `swift:structure` holds the evidence for these rules.

- **Calls.** The target comes from the callee node, not from the call text.
  The receiver is the expression before the called name, without `try` or
  `await`: `api.client().fetch(id:)` has the receiver `api.client()`. A
  `Type(...)` construction with generic arguments calls the type. A subscript
  (`cache[id]`) and a `defer` block are not calls.
- **Callers.** A call belongs to the declaration whose syntax holds it: a
  property accessor, observer, or lazy initializer, a `deinit`, or a Quick
  group. A Quick group never calls itself.
- **Conformances.** A protocol that refines a protocol `extends` it. A class
  or actor extends its first base and implements the rest. A struct, an enum,
  and an extension implement their protocols. An extension of a type from
  another file publishes the edge from the extension symbol.
- **Multi-name declarations.** `case a = 1, b, c(Int)`, `var x, y: Double`,
  and `let (head, tail) = ...` publish one symbol per name. The first name
  spans the whole declaration, and each later name spans its own pattern.
- **Return types.** Signatures keep `async`, `throws`, and the declared return
  type of any shape. Functions, protocol requirements, and subscripts record
  the declared return type as a non-inferred type fact. `some P` and `any P`
  record `P`. No declaration keyword (`class`, `extension`, `initializer`)
  and no placeholder (`Any`, `Void`) is ever a type fact.
- **Initializer inference.** A `let` or `var` with one name and no written
  type gets an inferred type fact from its initializer. `Type(...)` records a
  same-file type. A call records the declared return type of a same-file
  callee: a free function (`load()`), an in-scope local function, a member of
  the enclosing same-file type or its same-file extensions (`load()`,
  `self.load()`), a static member of an outer same-file type (`helper()` in a
  nested type), or a static or class member (`Type.make()`, `Self.make()`).
  An unqualified call walks out from the call, and the first scope that
  declares the name wins: a local function in the body that holds it, a
  member in its type, and a free function at file scope. So a method of a
  local type shadows a local function outside that type. `-> Self` records
  the enclosing type. A candidate counts only when the call's argument labels
  and argument count fit its parameters, with default values and variadics
  taken into account. At least one candidate must surely fit, and all
  candidates that fit or may fit must agree. An unlabeled trailing closure
  surely fits only a required parameter with a function type. If no
  same-file candidate fits, the call goes to an overload in another file, so
  it records nothing. A return spelled `Optional<T>`, `Swift.Optional<T>`,
  or `T!` records `T` with the declared text `T?`. `try`, `try!`, and
  `await` keep the type, `try?` makes the declared text optional, and a
  postfix `!` removes one optional layer. A `Type.make()` receiver names a
  nested type only inside a type that declares it, and names a top-level
  type anywhere else, unless a generic parameter, typealias, or other type
  of that name shadows it at the call. The first name of the return type
  must mean the same declaration at the binding as at the callee: a
  callee's nested `Item` read at file scope, a top-level `Item` read inside
  a type that nests its own `Item`, or a name that a generic parameter or
  typealias shadows at the binding records nothing. An `extension` whose name matches only a nested type
  extends a type from another file. No fact comes from a generic return type
  (a typealias for a generic parameter counts as generic), a chain that ends
  in another call, a call with both parentheses and a trailing closure
  (`load(x) { }`), a subscript (`Foo[0]`), a receiver other than `self` or a
  same-file type name, a callee name that is also a value (a parameter, a
  variable, an `if let` binding, or a capture-list name), a protocol or other-file extension
  member, a local type used as a receiver, or a type name that the file
  declares more than once (two nested `Node` types). A type with an
  inheritance clause records nothing for its own member calls, for
  unqualified calls inside it, or for a `Type.make()` receiver inside it,
  because a base class or a protocol can add a same-named overload or nested
  type that the call picks instead. Other files are out of scope because each
  file is extracted alone.
- **Access levels.** An explicit modifier wins. Without one, a declaration is
  `internal`, a member of a private type is `fileprivate`, an extension member
  takes the extension's level (`private` there means `fileprivate`), and a
  protocol requirement or an enum case takes its parent's level. Locals have
  no access level.
- **Body spans.** A property has a body only when it runs code: an accessor
  block, an observer block, or a lazy closure. Such a property also gets a
  complexity row. Enum cases, protocol requirements, and type aliases have no
  body.

The golden fixture `swift:declarations` holds the evidence for these rules.

- **Operators and macros.** `static func ==`, `prefix func -`, and a custom
  `func <>` are `operator` symbols named by the operator, with parameters and
  a body. `infix operator <>: AdditionPrecedence` is an `operator` symbol with
  no body. A `macro` declaration is a `function` symbol with metadata type
  `macro`; its signature stops before the `= #externalMacro(...)` definition.
- **Functions.** A `func` inside a type, extension, or protocol body is a
  `method`; a `func` inside a function body is a local `function`. `init?`
  and `init!` keep their failable marker in the signature. A signature carries
  a `where` clause only from the declaration's own constraints.
- **Actors.** An actor is a `class` symbol whose signature starts with `actor`
  and whose metadata type is `actor`.
- **Implicit members and macro expansions.** `.retryPolicy(...)`,
  `.init(...)`, and `#stringify(...)` are calls. An implicit member call
  never resolves to a free function; when it initializes an annotated
  property, the annotated type is its receiver. `Self.make()` is a call on
  the enclosing type.
- **Identifiers.** Compiler attributes such as `@available`, `@main`, and
  `@discardableResult` are not type usages; property wrappers, global actors,
  and macros such as `@Published` and `@MainActor` are. Associated-value
  labels and `.self` are not references.
- **Comments.** `/* */` blocks are comment regions and `/** */` blocks are
  doc-comment regions, so a `TODO` inside a block comment is a marker fact.

## Framework facts

| Pattern | Evidence | Rule |
| --- | --- | --- |
| `vapor.route.v1` | `swift:vapor_http` | In a file that imports Vapor, a `get`/`post`/`put`/`patch`/`delete`/`on(.VERB, ...)` call with static path components and a `use:` handler or trailing closure. Same-file `grouped(...)` bindings and `group(...) { builder in }` closures supply the prefix. |
| `http.client_request.v1` | `swift:vapor_http` | `AF.request("url", method: .verb)` (client `alamofire`) and a URLSession task whose first argument is `URL(string: "url")` or `URLRequest(url: URL(string: "url"))` (client `urlsession`, default `GET`). |
| `swiftpm.package.v1`, `swiftpm.product.v1`, `swiftpm.target.v1`, `manifest.dependency.v1` | `swift:package_manifest` | A file named `Package.swift`: the `Package(name:)` call, each product, each target with its static dependencies, and each `.package(url:)` or `.package(path:)` dependency. |

## Test-role contract

Swift ships three test frameworks and none of them marks a suite the same way.
XCTest subclasses a base class, Swift Testing applies a macro, and Quick makes
a call. The contract reads all three.

| Idiom | Role | Source of the rule |
| --- | --- | --- |
| `class X: XCTestCase` | `test_container` | XCTest base class |
| `class X: QuickSpec`, `class X: AsyncSpec` | `test_container` | Quick spec base class |
| `extension X` of a container in the same file | `test_container` | XCTest and Swift Testing split declarations |
| `class Y: X` where `X` is a container in the same file | `test_container` | XCTest project base case |
| `class Y: Base` with a parameterless `func testXxx`, in a file that imports `XCTest` | `test_container` | XCTest base case from another file |
| `func testXxx()` in a container, with no parameters | `test_case` | XCTest method prefix |
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
  Testing suites through the `@Suite` macro, Quick spec classes through their
  `QuickSpec` or `AsyncSpec` base type, and Quick groups through the call
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

One surface is recorded as an `open_gaps` entry on the swift row in
`fixtures/extraction/capabilities.json`, under
`kind_coverage.structural_facts.open_gaps`. The `test_detection` vocabulary is
frozen to `test_case`, `test_container`, and `test_lifecycle`, and swift
classifies each exactly once, so a swift-specific gap cannot live there.

- `swift_testing.test_traits`. `@Test(.tags(.slow))`, `@Test(.disabled("flaky"))`,
  and `@Suite(.serialized)` attach traits through extra macro arguments. The
  annotation normalizer keys on the macro name and drops its argument list, so a
  skip, a tag, and a serialization constraint reach no channel. No language
  publishes a test tag or skip channel yet: Ruby records RSpec metadata tags
  as the same kind of gap, so the channel is a cross-language decision.

## Windows grammar defect

tree-sitter-swift 0.7.3 builds the `try!` suppression mask in `scanner.c` with
`1UL << FAKE_TRY_BANG`, and `FAKE_TRY_BANG` is token 32. `long` is 32 bits on
Windows, so the shift is undefined there, the scanner emits `!` as an
operator, and `try! f()` parses as `try (!f)()` with no syntax error. Linux and
macOS parse it correctly.

- Call-initializer inference undoes the split for a bare call such as
  `let x = try! load()`, so that case records the same fact on every platform.
- Other `try!` forms on Windows, such as `try! self.load()` or
  `try! Type.make()`, record no inferred fact, and other rows under a `try!`
  can differ from Linux.
- Closure: an `anortham`-owned fork of tree-sitter-swift that writes `1ULL`,
  added under the [grammar dependency policy](../architecture/grammar-dependency-policy.md).

## Evidence

The golden fixture `swift:test_roles` registers two sources:

| Source | What it proves |
| --- | --- |
| `test_source.swift` | the XCTest container, its four hooks, a case, a case in an extension, a non-test method, a subclass of a same-file base case, a subclass of a base case from another file with a helper that takes parameters, the Quick tree including the shared group and the suite and wrapping hooks, `QuickSpec` and `AsyncSpec` subclasses beside a same-shaped `NSObject` subclass as the control, and the in-test-path controls |
| `production_roles.swift` | the Swift Testing suite, case, parameterized case, `init`/`deinit` hooks, a top-level `@Test` function, and the production-path control, all outside a test path |

The registered goldens observe 11 `test_case` rows, 12 `test_container` rows,
and 11 `test_lifecycle` rows for swift.

No real-world corpus scan was run for this contract. The evidence above is
golden-fixture evidence only.
