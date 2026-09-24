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

## Call edges and pending calls

- A call to a same-file function or method is a `calls` relationship.
- A JSX element with a capitalized name (`<Badge />`, `<UI.Panel>`) is a
  call site. Intrinsic elements (`<div>`) emit nothing.
- A member call emits a pending `calls` row only when its receiver has
  evidence for the consumer to bind:
  - the receiver is a project-relative import, or a local built by
    `new ImportedType()`;
  - the receiver is a variable, parameter, or property with a type fact.
    Built-in types, arrays, and types imported from packages do not count;
  - the receiver is `this` or `super` and the enclosing class does not
    declare the member. The row carries `receiver_type`: the class name for
    `this`, the declared base class for `super`.
- `this.repo.save()` names the receiver `repo`, so the consumer joins the
  `repo` type fact.
- A member call is never dropped because an unrelated symbol in the file
  has the same member name.
- A doc comment before `export`, `const`, or `let` belongs to the declared
  symbol. The `export` row does not repeat it.

The `typescript/language_gaps` and `tsx/language_gaps` goldens hold the
evidence: abstract classes and members, enum members with initializers,
declaration docs, typed-receiver pending calls, JSX component edges, and
Express/Fastify routes on exported and type-annotated receivers.

## Declarations and ownership (wave 2)

- Generator declarations, function expressions, `export default` anonymous
  functions and classes, and class expressions are symbols. An anonymous
  default export is named `default`; a class or function expression bound
  to a variable takes the variable name.
- Overload signatures fold into the implementation, which carries
  `overloads`. A `declare function` without an implementation is a
  function with `isDefinition: false`.
- `declare module "x"`, `declare global`, `namespace`, and `module` blocks
  are `namespace` symbols. The ambient forms carry `isAmbientModule` or
  `isGlobalAugmentation`.
- Constructor parameter properties (`private readonly repo: Repo`) are also
  class `property` symbols with `isParameterProperty`, spanning the name.
- Property and parameter decorators are annotations on the property or
  parameter symbol.
- Members of a type alias object type (`type P = { a: T }`) belong to the
  alias. Object types in annotations and type arguments emit no symbols.
- Calls in arrow-function fields, object-literal methods, and variable
  initializers are owned by that field, method, or variable. Export rows
  never own code. Test DSL calls (`it`, `describe`) are never call targets.
- `interface A extends B, C` emits one `extends` edge per listed type.
- Every exported name has its own `export` row with `exportedName`,
  `isDefault`, `isNamed`, and, where they apply, `localName`, `source`,
  `isNamespace`, `isStar`, and `isTypeOnly`. A declaration exported by an
  `export` wrapper or listed in `export { ... }` has `public` visibility.
- `import x = require("m")`, `const x = require("m")`, side-effect
  `import "m"`, and literal dynamic `import("m")` are `import` rows
  (`isCommonJS`, `isSideEffect`, `isDynamic`).
- Declared return types are type facts; inferred placeholders (`any`,
  `function`, `Promise<any>`) are not published.
- A `const`, `let`, or `var` with no written type gets an inferred type fact
  from a call to a same-file callee with a declared return type: a function,
  an overload set, a function-valued `const` (`const make = (): User => ..`),
  a class method or arrow field through `this` in that class, or a static
  method through the class name (`Repo.create()`). Only bindings whose
  lexical scope contains the call count: a function nested in another
  function does not reach calls outside it. All visible same-named
  candidates must agree, and any other visible binding of the name (import,
  `import x = A.b`, parameter, local, namespace, enum, function-expression
  name) blocks the fact. Class members belong to one class declaration, so
  two same-named classes never share methods, and `Repo.create()` needs
  `Repo` to name exactly one visible class. `await` removes one
  `Promise`/`PromiseLike` layer; `!` removes `null`/`undefined`; a
  `T | null` result without `!` records nothing. `: this` resolves to the
  enclosing class. A result that is a bare type parameter (`T`, or `T` after
  `await`) records no fact. A generic result keeps its base type and its
  declared text as written, so `make<K>(): Map<K, User>` records `Map` with
  declared `Map<K, User>`. A getter, an optional call (`?.`), a method after
  the call, `this` rebound by a `function` or object literal, a
  namespace-qualified call (`Ns.load()`), a method inherited from a base
  class through `this`, `super.load()`, or a callee in another file records
  no fact. Other files are out of scope because each file is extracted
  alone.
- The `string` type keyword is not a `string_literal` source region.

## Frontend navigation facts

- `angular.route_definition.v1`: one fact per route object with a `path` in
  a `Routes` or `Route[]` array, or in the array passed to
  `RouterModule.forRoot/forChild` or `provideRouter`. The file must import
  from `@angular/router`. Metadata records `route_component`,
  `redirect_to`, `lazy_module_source` (`loadChildren`),
  `lazy_component_source` (`loadComponent`), and the joined
  `effective_route_template` for nested `children`.
- `react.route_reference.v1` with `source_kind: react_router_navigate`:
  `navigate("/x")` where `navigate` is bound from `useNavigate()` of React
  Router.
- `nextjs.route_reference.v1` with `source_kind: next_router_navigation`:
  `router.push/replace/prefetch("/x")` where `router` is bound from
  `useRouter()` of `next/navigation` or `next/router`.
- The hook binding is matched by name across the file, not by scope.

The `typescript/language_gaps`, `tsx/language_gaps`, and
`typescript/frontend_navigation` goldens hold the evidence.

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
for either, and code-kb would not know that editing that file invalidates the
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
