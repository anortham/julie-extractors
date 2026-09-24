# JavaScript support

Julie registers two JavaScript languages: `javascript` handles `.js`, `.mjs`,
and `.cjs` files, and `jsx` handles `.jsx` files. Both use
`tree-sitter-javascript` and share every extractor module, including test-role
detection.

## Continuous testing

Run the language targets when changing JavaScript extraction:

```bash
cargo xtask test language javascript
cargo xtask test language jsx
```

Each command runs the dialect unit-test module and the golden extraction test
with `JULIE_GOLDEN_LANGUAGE` set to that dialect. The normal golden target
stays unfiltered:

```bash
cargo xtask test golden
```

## Qt QML directives

Qt lets a `.js` file open with QML directives. They are not JavaScript, so the
extractor blanks each directive line to same-length spaces before it parses the
file. Byte spans of the symbols below therefore still point at the original
text, and `julie-extract check` accepts such a file instead of reporting a
syntax error.

- `.pragma library` becomes a structural fact, `javascript.qml_directive.v1`,
  with the metadata keys `directive` (`pragma`) and `name` (`library`).
- `.import "helpers.js" as Helpers` becomes an `import` symbol named after the
  source, with `alias`, `local_name`, `imported_name`, and `is_namespace` in
  its metadata.

The producer lives in
`crates/julie-extractors/src/javascript/qml_directives.rs`.

## Test-role contract

code-kb drives continuous testing from these roles, so a wrong role becomes a
wrong staleness verdict. JavaScript test frameworks declare cases as *calls*,
not as named functions, so the detector reads the callee chain of a
`call_expression`. The rules live in
`crates/julie-extractors/src/javascript/test_symbols.rs` and are shared by all
four dialects (`javascript`, `jsx`, `typescript`, `tsx`).

### Detection is gated

The DSL vocabulary overlaps ordinary production names: `setup`, `teardown`,
`before`, `after`, `context`, `suite`. A file is read for test DSL only when
one of two gates holds:

- the file path is a test path (`test/`, `tests/`, `__tests__/`, `spec/`,
  `e2e/`, `cypress/`, a `.test.` or `.spec.` or `.cy.` infix, and the other
  shared rules in `crates/julie-extractors/src/test_detection.rs`), or
- the file imports a known test framework.

The recognized module specifiers are `vitest`, `jest`, `mocha`, `chai`,
`qunit`, `jasmine`, `ava`, `tape`, `uvu`, `bun:test`, `node:test`,
`playwright/test`, and `testdeck`, plus the `@jest/`, `@playwright/`,
`@vitest/`, `@testing-library/`, `@testdeck/`, `node:test/`, and `uvu/`
prefixes. A `require(...)` counts as an import.

The import gate matters in practice. Vitest global setup files live outside any
test directory; the zod corpus below has one, and the path gate alone would
miss both of its hooks.

### Callee vocabulary

A dotted callee is split, run-modifier segments are dropped, and the remaining
word decides the role.

| Callee shape | Role | Example |
| --- | --- | --- |
| `it`, `test`, `specify`, `bench`, `xit`, `fit`, `xtest` | `test_case` | `it("adds", fn)` |
| `describe`, `context`, `suite`, `xdescribe`, `fdescribe`, `xcontext` | `test_container` | `describe("cart", fn)` |
| `beforeEach`, `beforeAll`, `before`, `setup`, `suiteSetup` | `fixture_setup` | `suiteSetup(fn)` |
| `afterEach`, `afterAll`, `after`, `teardown`, `suiteTeardown` | `fixture_teardown` | `teardown(fn)` |
| a case word with a `.each` table | `parameterized_test` | `test.each([1, 2])("doubles %i", fn)` |
| a container word with a `.each` table | `test_container` | `describe.each(rows)("suite %i", fn)` |
| `module` behind a namespace root | `test_container` | `QUnit.module("badge", fn)` |

The dropped run modifiers are `only`, `skip`, `todo`, `failing`, `fails`,
`concurrent`, `sequential`, `serial`, `parallel`, `skipIf`, and `runIf`. They
change how a test runs, never what it is, so `it.only` stays a `test_case` and
`test.describe.serial` stays a `test_container`.

A dotted chain resolves only behind a namespace root: `test`, `it`, `describe`,
`suite`, `QUnit`, or `t`. That is what makes Playwright's `test.describe`,
QUnit's `QUnit.test`, and a `node:test` subtest `t.test` resolve, while an
ordinary member call such as `reporter.test("...", fn)` resolves to nothing.

### `.each` keeps the word's own category

`test.each`/`it.each` report `parameterized_test`: the table multiplies
runnable cases. `describe.each(table)("name", fn)` reports `test_container`:
Jest and Vitest run it as a suite factory, one group per table row, and the
cases inside it come from its own `it`/`test` calls. The rationale is recorded
in `docs/decisions/2026-08-20-test-role-contract-closure.md`.

### Registered evidence

| Golden | Framework and idioms |
| --- | --- |
| `javascript/test_roles` | Vitest `describe`/`test`/`beforeEach`, member-call control |
| `javascript/jest_vitest_roles` | Jest and Vitest hooks, `.only`/`.skip`/`.todo`/`.failing`, `xit`/`fit`/`xtest`/`xdescribe`/`fdescribe`, `test.each`, `describe.each`, `bench` |
| `javascript/mocha_tdd_roles` | Mocha TDD `suite`/`test`/`setup`/`teardown`/`suiteSetup`/`suiteTeardown` and BDD `context`/`specify`/`xcontext`, gated by the test path rather than an import |
| `jsx/test_roles` | Vitest inside JSX, member-call control |
| `jsx/node_test_roles` | `node:test` `describe`/`it`/`before`/`after` plus `t.test` and `t.beforeEach` subtests |

Every fixture carries production controls that must stay unclassified: a plain
helper function, an object-literal method whose name matches a DSL word, and a
member call such as `reporter.test(...)` or `harness.suite(...)`. The
`node:test` fixture also carries `t.diagnostic(...)`, which is a real
`TestContext` method that declares no test.

## Known limitation: a declared callable named `describe`, `it`, or `test`

Besides the call-expression rules, a *declared* function or method earns
`test_case` when it is literally named `describe`, `it`, or `test` and lives in
a test file (`detect_js_ts`, `crates/julie-extractors/src/test_detection.rs`).
The rule exists for hand-rolled harnesses, and it is the only source of false
positives measured below. A test-file helper named `test(app)` or a formatter
named `describe(value)` is flagged as a case.

Closing it needs the extractor to tell a DSL call site from an ordinary
declaration at the point of declaration, which is an extractor change rather
than a detection-rule change. The measured cost is 3 rows in 4,328.

## Call edges and pending calls

The JavaScript extractor follows the TypeScript contract in
[`docs/languages/typescript.md`](typescript.md), with these additions:

- A function value bound by `const`/`let`/`var` is one `function` symbol
  named by the binding. A function value bound by an object key
  (`post: function () {}`), a prototype or static member assignment
  (`A.prototype.m = function () {}`, `A.create = () => {}`), a
  `this.m = function () {}` inside a constructor function, or a class field
  (`handle = () => {}`) is one `method` symbol. Its parameters hang off that
  symbol.
- Calls inside function expressions and generators belong to the nearest
  callable symbol, the same as calls inside declarations and arrows.
- A bare call never binds to a method. A bare call whose name is a local
  binding (variable, parameter, destructured name, local function) emits no
  pending row.
- `const { a, b: c } = require("./x")` and `const A = require("./x").A` are
  CommonJS `import` symbols with `importedName` and `source`, so calls to
  them carry an import context.
- Renamed, defaulted, nested, and rest destructuring bindings are variables.
  Destructured parameters are parameter symbols, one per binding.
- Test-DSL call sites (`describe`, `it`, hooks) emit no call edges.

## Declarations, visibility, and types (wave 2)

- Prototype and static members are parented to the same-file constructor
  function or class, carry `className`, and have a header-only signature
  (`Queue.prototype.clear = function clear()`). `this.m()` between them is a
  `calls` edge, `new Queue()` on a constructor function is an
  `instantiates` edge, and `this`/`super` call identifiers carry
  `receiver_type`.
- `module.exports = function auth() {}` is the function `auth` (`default`
  when anonymous). `exports.x = fn` and `module.exports.x = fn` are the
  function `x`. None of them is a method of a fake `module` or `exports`
  class.
- Class expressions are classes named by their own name, their binding, or
  `default` for `export default class {}`. Heritage belongs to the nearest
  class, so a class expression inside a method adds no edge to the outer
  class.
- `this.x = value` in a class constructor is a `property` of the class
  unless the class declares `x`.
- Type facts: `@param {T} name` on parameters, `@returns {T}` on callables,
  `@type {T}` on variables, properties, and fields, and `new T()` field and
  constructor-property initializers. `resolved_type` is the base name
  (`Promise`, `Repo`); the JSDoc text is in `metadata.declared`.
- A `const`, `let`, or `var` with no `@type` gets an inferred type fact from
  its initializer: `new T()`, or a call whose callee declares
  `@returns {T}` in the same file. The callee is a function declaration or a
  function bound by `const`/`let`/`var` (`load()`), a static method of a
  same-file class (`Workspace.open()`), a method through `this` inside the
  class body, or a method of the class a chained call returns
  (`loadWorkspace().child()`). A chained call's class must be the same
  same-file class at the callee's declaration and at the call. Same-named
  candidates must agree. `await`
  unwraps one `Promise<T>`; an `async` callee must declare a `Promise`, and
  a generator must declare a `Generator`, `Iterator`, `IterableIterator`, or
  `Iterable` (the `Async` forms for an async generator). The callee's
  declaration must be in scope at the call: `var` binds in its function,
  `let`, `const`, and `class` in their block, and a function declaration in
  its block (a call elsewhere in the same function records nothing, since
  sloppy code hoists it). A `@template` or `this` return type, a union, a
  getter, an optional call (`?.`), `this` in a computed member name or a
  decorator (it runs outside the class), a parameter, loop, or catch binding or
  the own name of a named function expression that shadows the
  callee, a chain that ends in any other method, or a callee in another file
  records no fact.
- A doc comment before a `const`, `let`, or `var` with several declarators
  documents only the first declarator. JSDoc is the only source of return types, so a callee with no
  `@returns` records nothing.
- JSDoc tags come from the last `/** */` block before a declaration. A
  `@callback` or `@typedef` block there documents that type, not the
  declaration, and a declaration with `@overload` blocks gets its call type
  from the arguments, so both record no `@returns`, `@type`, or `@param`
  fact.
- Visibility: in a module (a file with `import`, `export`, `require`, or
  CommonJS export assignments) a top-level class, function, or variable is
  `public` when the module exports it by any form (`export` wrapper,
  `export { x }`, `export default x`, `module.exports = X`,
  `module.exports = { X }`, `module.exports.x = X`, `exports.x = X`) and
  `private` otherwise. In a script every top-level declaration is `public`.
  Locals of a callable carry no visibility.
- Every exported name has its own `export` row, shared with TypeScript
  (see [`docs/languages/typescript.md`](typescript.md)). Export rows never
  parent or own the declaration they export.
- Object literals emit `property` symbols only when a declaration binds
  them (`const config = {...}`, `export default {...}`,
  `module.exports = {...}`, a class field), including nested literals.
  Literals in call arguments, JSX attributes, return values, arrays, and
  decorator arguments emit none. Function-valued pairs are always
  `method` symbols.
- Class field decorators are annotations, the same as method and class
  decorators.
- `koa.route.v1`: verb-method routes on a router built from `@koa/router` or
  `koa-router`, with the constructor `prefix` joined into
  `effective_route_template`. `hapi.route.v1`: `server.route` objects on a
  server built by `Hapi.server` or `new Hapi.Server`.

The `javascript/language_gaps` and `jsx/language_gaps` goldens hold the
evidence, together with Express mounts for imported, required, and
middleware-prefixed routers.

## Named exclusions

- `test.step(...)` is not a role. A Playwright step is a report annotation
  inside a case, not a case.
- `hooks.beforeEach(...)` inside `QUnit.module("name", (hooks) => …)` is not a
  role. `hooks` is a runtime callback parameter, not a namespace root, so the
  name rule cannot separate it from any other object named `hooks`.
- Bare `QUnit.only(...)`, `QUnit.skip(...)`, and `QUnit.todo(...)` are not
  roles. Dropping the run modifier leaves only `QUnit`, which is a namespace,
  not a DSL word.
- `tape` is recognized as a framework import, so it opens the gate, but its
  `test(t)` idiom is covered by the shared `test` word rather than a
  tape-specific rule.

## Real-world evidence

`expressjs/express` at commit `023767fe9872e029271df1418f73401bff20ff40`
(MIT) was cloned shallowly into a temporary directory and scanned. No project
build script, hook, or third-party binary was run.

```bash
CORPUS="$(mktemp -d)"
git clone --depth 50 https://github.com/expressjs/express "$CORPUS"
git -C "$CORPUS" checkout --detach 023767fe9872e029271df1418f73401bff20ff40

cargo build --locked --bin julie-extract
ARTIFACT="$(mktemp -d)"
./target/debug/julie-extract scan \
  --root "$CORPUS" \
  --db "$ARTIFACT/artifact.sqlite" \
  --json >"$ARTIFACT/scan-report.json"
```

The scan reported `status=ok` with `files_failed=0` and empty `warnings` and
`errors`. It scanned 213 files and skipped 49 as unsupported.

| Artifact evidence | express |
| --- | ---: |
| JavaScript files indexed | 141 |
| JavaScript files under `test/` | 91 |
| JavaScript symbols | 4,847 |
| `test_case` | 1,127 |
| `test_container` | 557 |
| `fixture_setup` | 59 |
| `fixture_teardown` | 40 |

Every one of the 1,783 role rows sits inside `test/`, so the gate produced no
role in production code.

### Diagnostic breakdown

The scan produced 24 parse diagnostics. All 24 are HTML rows from three EJS
template files under `examples/ejs/views/`, whose `<% … %>` tags the HTML
grammar does not parse. JavaScript produced zero parse diagnostics across all
141 files.

### Precision

Express is a Mocha BDD project, so the corpus exercises `describe`, `it`,
`before`, and `after` at volume. The first count missed a class of false
positives: 37 of the 40 `fixture_teardown` rows were calls to the `after` npm
counter (`var after = require('after')`, then `after(2, done)`), not Mocha
hooks. Two rules now remove them:

- A bare DSL word is not a role when the file binds it at module level to a
  package that is not a test framework (`require`/`import`) or to a local
  function.
- A lifecycle call is a hook only when it passes a function literal, or one
  function reference such as `afterEach(cleanup)`.

A rescan at commit `bed501c695a61886399ee622875f3be933c716d8` gives 3
`fixture_teardown` rows (the three `after(function () {...})` hooks in
`test/app.js`), 59 `fixture_setup`, 1,127 `test_case`, and 557
`test_container`, all inside `test/`. One known false positive stays:
`function test(app)` at `test/res.format.js:182`, a helper that wraps
`it(...)` calls, with the cause recorded under "Known limitation" above.

The temporary checkout and SQLite artifact were removed after recording this
evidence.
