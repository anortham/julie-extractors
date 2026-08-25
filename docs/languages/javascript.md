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

## Test-role contract

Miller drives continuous testing from these roles, so a wrong role becomes a
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
`before`, and `after` at volume. Of the 1,783 flagged symbols, 1 is wrong:
`function test(app)` at `test/res.format.js:182`, a helper that wraps `it(...)`
calls. That is 99.94 percent precision, and the single failure has the cause
recorded under "Known limitation" above.

The temporary checkout and SQLite artifact were removed after recording this
evidence.
