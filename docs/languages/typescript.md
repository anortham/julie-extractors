# TypeScript support

Julie registers two TypeScript languages: `typescript` handles `.ts`, `.mts`,
and `.cts` files, and `tsx` handles `.tsx` files. Both use
`tree-sitter-typescript`.

Test-role detection is shared with JavaScript. The rules, the gate, and the
callee vocabulary are documented once in
[`docs/languages/javascript.md`](javascript.md); this page records only what is
specific to TypeScript.

## Continuous testing

Run the language targets when changing TypeScript extraction:

```bash
cargo xtask test language typescript
cargo xtask test language tsx
```

Each command runs the dialect unit-test module and the golden extraction test
with `JULIE_GOLDEN_LANGUAGE` set to that dialect. The normal golden target
stays unfiltered:

```bash
cargo xtask test golden
```

## Decorator test frameworks

TypeScript adds one detection path JavaScript does not have: a decorator on a
declared method. `apply_declared_test_metadata` in
`crates/julie-extractors/src/javascript/test_symbols.rs` reads the decorator
first, and a decorator wins over the name-and-path rule, the way a JUnit
annotation does in Java.

| Decorator | Role |
| --- | --- |
| `@test`, and its chained `@test.only` / `@test.skip` spellings | `test_case` |
| `@params(...)` | `parameterized_test` |

Two limits are deliberate.

`@suite` on a class does **not** produce `test_container`. The decorator pass
classifies callables only, so a testdeck suite class stays unclassified while
its `@test` methods carry roles. Container evidence for TypeScript comes from
the call DSL instead — `describe(...)` and Playwright's `test.describe(...)`.

The decorator is matched by the name written at the call site, not by the
import it resolves to. `import { test as testdeckTest }` followed by
`@testdeckTest` produces no role, because the annotation key is
`testdeckTest`. Resolving an aliased import back to its module needs
import-aware annotation normalization, which does not exist yet.

## Registered evidence

| Golden | Framework and idioms |
| --- | --- |
| `typescript/test_roles` | Vitest `describe`/`it`/`beforeEach`; testdeck `@suite` class with `@test`, `@test.only`, and doubled `@params` methods; an undecorated method and a member call as controls |
| `typescript/playwright_roles` | Playwright `test`, `test.describe`, `test.describe.serial`, `test.describe.parallel`, `test.beforeAll`/`beforeEach`/`afterEach`/`afterAll`, `test.only`/`test.skip`, and `test.step` as a control |
| `tsx/test_roles` | Vitest inside TSX, member-call control |
| `tsx/qunit_roles` | QUnit `QUnit.module`, `QUnit.module.only`, `QUnit.test`, with `hooks.beforeEach` and bare `QUnit.only`/`skip`/`todo` as controls |

The named exclusions those controls lock in — `test.step`, the QUnit `hooks`
object, and bare `QUnit.only` — are listed in
[`docs/languages/javascript.md`](javascript.md).

## Grammar gap: variance annotations on type parameters

`tree-sitter-typescript` does not parse the `in` and `out` variance modifiers
that TypeScript 4.7 added to type parameters. A declaration such as

```ts
export interface $ZodTypeInternals<out O = unknown, out I = unknown> { }
```

yields an `ERROR` node and one parse diagnostic per affected type parameter.
The surrounding file still extracts; only the annotated parameter list is lost.
This is an upstream grammar limitation, not an extractor rule, and it accounts
for every TypeScript diagnostic in the corpus scan below.

## Real-world evidence

`colinhacks/zod` at commit `fc90cad8ee4db751ec0e1e297c7c4bcd83588adb` (MIT)
was cloned shallowly into a temporary directory and scanned. No project build
script, hook, or third-party binary was run.

```bash
CORPUS="$(mktemp -d)"
git clone --depth 50 https://github.com/colinhacks/zod "$CORPUS"
git -C "$CORPUS" checkout --detach fc90cad8ee4db751ec0e1e297c7c4bcd83588adb

cargo build --locked --bin julie-extract
ARTIFACT="$(mktemp -d)"
./target/debug/julie-extract scan \
  --root "$CORPUS" \
  --db "$ARTIFACT/artifact.sqlite" \
  --json >"$ARTIFACT/scan-report.json"
```

The scan reported `status=ok` with `files_failed=0` and empty `warnings` and
`errors`. It scanned 665 files and skipped 100 as unsupported.

| Artifact evidence | zod |
| --- | ---: |
| TypeScript files indexed | 452 |
| TypeScript test files | 194 |
| TSX files indexed | 29 |
| TypeScript symbols | 21,196 |
| TSX symbols | 313 |
| `test_case` | 2,463 |
| `test_container` | 58 |
| `parameterized_test` | 9 |
| `fixture_setup` | 8 |
| `fixture_teardown` | 7 |

Zod is a Vitest project, so the corpus exercises `test`, `describe`, the four
hooks, and `test.each`. Its 29 TSX files are type-level fixtures and carry no
test roles.

### The import gate earns its keep

Only 2 of the 2,545 role rows sit outside a test path, and both are correct:
`beforeAll` and `afterAll` in `scripts/fail-on-console.ts`, a Vitest global
setup file that imports from `vitest`. A path-only rule would publish no role
for either, and Miller would not know that editing that file invalidates the
whole suite.

### Diagnostic breakdown

The scan produced 30 parse diagnostics:

| Language | Rows | Cause |
| --- | ---: | --- |
| `typescript` | 21 | `in`/`out` variance annotations on type parameters |
| `json` | 5 | trailing commas in four `tsconfig` files, which JSON does not allow |
| `css` | 4 | the Tailwind v4 `@source` at-rule in `packages/docs/app/global.css`, reported as 3 error rows and 1 missing-node row |

All 21 TypeScript rows come from one cause across four files:
`packages/zod/src/v4/classic/schemas.ts` (7),
`packages/zod/src/v4/core/schemas.ts` (7),
`packages/zod/src/v4/mini/schemas.ts` (6), and
`packages/zod/src/v4/core/checks.ts` (1). See "Grammar gap" above.

### Precision

Of the 2,545 flagged symbols, 2 are wrong, and both are declared callables
named `describe` inside a `.test.ts` file: a value formatter at
`packages/zod/src/v4/core/tests/compile-differential.test.ts:7` and an
object getter at
`packages/zod/src/v4/classic/tests/recursive-types.test.ts:450`. That is
99.92 percent precision, and the cause is the shared limitation recorded in
[`docs/languages/javascript.md`](javascript.md).

Across both corpora the combined precision is 4,325 correct of 4,328 flagged
symbols, or 99.93 percent, and all three failures share one cause.

The temporary checkout and SQLite artifact were removed after recording this
evidence.
