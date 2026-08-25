# Kotlin support

Julie registers `kotlin` for `.kt` and `.kts` files. Kotlin shares every
annotation rule with Java: `java` and `kotlin` share `detect_java_kotlin`, the
annotation key lists, and the `mark_java_test_containers` pass. A change to the
annotation lists changes both languages, so read `docs/languages/java.md` first.

Kotlin adds one thing Java has none of: a call-style test DSL. Kotest and Spek
write tests as calls, and three of their forms are not named calls at all.

## Continuous testing

Run the language target when changing Kotlin extraction:

```bash
cargo xtask test language kotlin
```

The command runs the Kotlin unit-test modules and the golden extraction test
with `JULIE_GOLDEN_LANGUAGE=kotlin`. Run the Java target as well when you touch
the shared annotation lists or the shared container pass:

```bash
cargo xtask test language java
```

## Test roles from annotations

Three annotation frameworks are adopted by name: JUnit 3/4/5, TestNG, and
kotlin.test. Annotation keys are lower-cased and reduced to their rightmost
name, so `@kotlin.test.BeforeTest` and `@BeforeTest` both produce `beforetest`.

| Role | Annotation keys |
| --- | --- |
| `test_case` | `test`, `testfactory`, `testtemplate` |
| `parameterized_test` | `parameterizedtest`, `repeatedtest` |
| `fixture_setup` | `beforeeach`, `beforeall`, `before`, `beforeclass`, `beforemethod`, `beforesuite`, `beforetest`, `beforegroups` |
| `fixture_teardown` | `aftereach`, `afterall`, `after`, `afterclass`, `aftermethod`, `aftersuite`, `aftertest`, `aftergroups` |
| `test_container` | `nested`, `test` on a class |

kotlin.test needs no Kotlin-only arm. Its `@BeforeTest` and `@AfterTest` share
the TestNG keys `beforetest` and `aftertest`, and its `@BeforeClass`/`@AfterClass`
share the JUnit 4 keys. `fixtures/extraction/kotlin/kotlin_test_lifecycle/source.kt`
is the registered evidence.

The TestNG class-level rule works for Kotlin because Kotlin's defaults line up
with what the rule needs: a function declared inside a class becomes
`SymbolKind::Method`, and a member with no visibility modifier becomes
`Visibility::Public`. `LedgerTestNgTest.postsAnEntry` in that golden is a case;
the `private fun helperTotal` beside it stays unclassified; and `@BeforeMethod`
on `prepareRun` wins over the class-level rule.

`@ParameterizedTest` and `@RepeatedTest` report `parameterized_test`, evidenced
in `fixtures/extraction/kotlin/junit_tests/source.kt`.

Gradle's extra test source sets need no Kotlin rule. The shared `is_test_path`
check already accepts `integrationTest`, `functionalTest`, `androidTest`, and
`testFixtures`.

## Test roles from the Kotest and Spek call DSLs

The adapter walks the Kotlin grammar and delegates classification and symbol
construction to the shared `crate::test_calls` core, so the published metadata
is identical to every other call-style language.

| Role | DSL words |
| --- | --- |
| `test_case` | `test`, `it`, `should`, `then`, `scenario`, `expect`, `xit`, `xtest` |
| `test_container` | `describe`, `context`, `given`, `When`, `and`, `feature`, `xdescribe`, `xcontext` |
| `fixture_setup` | `beforeEach`, `beforeAll`, `beforeTest`, `beforeEachTest`, `beforeGroup` |
| `fixture_teardown` | `afterEach`, `afterAll`, `afterTest`, `afterEachTest`, `afterGroup` |

A disabled step keeps the role of its enabled spelling, because the runner
reports it as skipped rather than dropping it.

### The three forms that are not named calls

Each of the three names the Kotest function that declares it, so the captured
callee is a real API name rather than an invented one:

- **String-invoke** — `"name" { }` is how StringSpec writes a case, and how
  WordSpec and FreeSpec write their leaf step. Kotest declares it with
  `operator fun String.invoke(test: suspend TestScope.() -> Unit)`, so the
  callee is `invoke`. In the grammar the whole clause is a `call_expression`
  whose first named child is a `string_literal`.
- **WordSpec `should`** — `"subject" should { }` opens a group. Kotest declares
  `should` as an infix extension on `String`, so the callee is `should` and the
  group is named `"subject should"`. The node is an `infix_expression` with a
  string receiver, the verb, and a `lambda_literal`.
- **FreeSpec `-`** — `"subject" - { }` opens a group, declared as
  `operator fun String.minus(...)`, so the callee is `minus`. The node is a
  `binary_expression`.

Kotlin uses `infix_expression` for every infix call, `shouldBe` assertions
included, so both operator guards require a string receiver and a lambda body.
Subtracting or invoking a lambda on a string has no other meaning in Kotlin,
which is what makes them safe.

`fixtures/extraction/kotlin/kotest_string_spec/source.kt` carries all three
forms plus the `LengthHelpers` control that must stay unclassified.

### Spec scopes are containers

A Kotest or Spek spec carries no annotation, so `mark_kotlin_test_containers`
takes two proofs and either one is enough:

- the class extends a named spec base type — `StringSpec`, `FunSpec`,
  `DescribeSpec`, `ShouldSpec`, `WordSpec`, `FreeSpec`, `BehaviorSpec`,
  `FeatureSpec`, `ExpectSpec`, `AnnotationSpec`, or `Spek`; or
- the declaration's body is a spec lambda: a call-DSL step already earned a role
  and named this declaration as its parent.

The second proof is what catches a project's own spec base class, which no name
list can know, and a Kotest test factory — `val factory = funSpec { … }`, a
property rather than a class. It is limited to a class or a property so a
nested step never turns the case above it into a container.

This pass runs before `mark_java_test_containers`, because that pass's scoping
step reads the marked containers.

Kotlin classes now publish their supertypes as a `base_types` metadata list.
The signature already carried them, but only as source text: for
`StringSpec({ … })` the signature holds the whole spec lambda, so a container
pass could not read a type name out of it.

### Scoping now covers Kotlin

`normalize_scoped_test_roles` strips the role from a callable that no test
container encloses. It previously ran for Java only, because Kotlin's spec
classes carried no container marker and scoping would have stripped real
Kotest roles. With those classes marked, the Java-only gate is deleted and
Kotlin takes the same scoping.

This matters because Kotlin shares Java's JUnit 3 fallback: a `testXxx` name in
a test path. Test framework source is full of that vocabulary.
`LedgerTestHelpers.testDataForLedger` in the kotlin.test golden is the control
that must lose the role.

### Recorded gaps

Two named Kotest surfaces are not adopted. Both are recorded as `open_gaps` on
the kotlin row in `fixtures/extraction/capabilities.json`:

- `kotest.data_driven_test_roles` — data-driven testing runs one case per row
  with `forAll(rows) { … }`. The rows are call arguments, not declarations, so
  there is no per-row symbol and no `parameterized_test` role.
- `kotest.property_test_roles` — property testing declares a case with
  `checkAll`/`forAll` over generators inside an already-classified step, so the
  generator block carries no role of its own.

They are recorded under `structural_facts` rather than `test_detection` because
the `test_detection` coverage vocabulary is frozen to `test_case`,
`test_container`, and `test_lifecycle`, and each of those three is already
classified exactly once for kotlin.

## Backticked names

Kotlin lets any identifier be written `` `like this` ``, and the grammar keeps
the backticks in the identifier node text. Every runner and report — JUnit,
Gradle, Kotest — prints the name without them, so a consumer matching a report
line against a symbol name never saw a match.

`symbols.name` and `identifiers.name` are both stripped, so a call site still
resolves to its definition. The escaped source spelling stays available in two
places: the symbol signature, which remains valid Kotlin, and a `rawName`
metadata key written only when the two spellings differ.

## Relationships

Two `calls` defects are fixed, both caused by the call DSL naming a symbol after
something that is not a declaration.

A lifecycle hook is written `beforeEach { }`, so the adapter names the symbol
after the callee. The containing-symbol lookup then returned that same symbol
for the call node it was built from, and resolving the callee found it again — a
`beforeEach` calls `beforeEach` edge that describes nothing. A symbol whose span
is exactly the call node no longer opens a call edge. A declaration never shares
a span with a call expression, so a recursive function keeps its real self-call
edge.

A case named after the thing it exercises captured every call to that thing.
`test("RuntimeException") { throw RuntimeException("foo") }` made the case
answer for `RuntimeException`, and Kotest's own suite is full of that shape. A
DSL symbol is now never a call target; such a call falls through to a structured
pending relationship, which is what an unresolved cross-file target already
does.

The ledger row previously advertised only `calls` and `implements`. Kotlin also
emits `extends`, `function` and `type` symbols, and `member_access` identifiers,
all of them already present in the goldens; those claims are now declared.

## Grammar freshness

```bash
node scripts/grammar-freshness-report.mjs --format json
```

The report says `tree-sitter-kotlin-ng` is `current`: declared `1.1.0`, locked
`1.1.0`, latest stable `1.1.0`.

## Real-world evidence

Two corpora were used, one per source of roles. Both were cloned shallowly into
temporary directories. No project build scripts, hooks, or third-party binaries
were executed.

- Kotest at commit `dfee83ac086872c0ffe788d975e39d66f074f141`, Apache-2.0 — the
  call DSL.
- kotlinx.coroutines at commit `3eadf938b1506351bffd7c015445d08faf1c4315`,
  Apache-2.0 — kotlin.test and JUnit annotations.

Reproducible checkout and scan commands:

```bash
CORPUS="$(mktemp -d)"
git clone --depth 50 --filter=blob:none https://github.com/kotest/kotest "$CORPUS"
git -C "$CORPUS" checkout --detach dfee83ac086872c0ffe788d975e39d66f074f141

cargo build --locked --bin julie-extract
ARTIFACT="$(mktemp -d)"
./target/debug/julie-extract scan \
  --root "$CORPUS" \
  --db "$ARTIFACT/artifact.sqlite" \
  --json >"$ARTIFACT/scan-report.json" \
  2>"$ARTIFACT/scan-stderr.log"
```

Both scan reports were `status=ok` with `files_failed=0`, empty `errors`, and
empty `warnings`. Kotest reported `files_scanned=4549`, `files_changed=4549`,
and `files_unsupported=481`. kotlinx.coroutines reported `files_scanned=1331`,
`files_changed=1331`, and `files_unsupported=149`. The per-language counts below
come from the SQLite artifacts and cover `kotlin` rows only.

| Artifact evidence | Kotest | kotlinx.coroutines |
| --- | ---: | ---: |
| Indexed `kotlin` files | 2,493 | 1,082 |
| Symbols | 55,304 | 22,918 |
| Identifiers | 209,309 | 99,985 |
| Resolved relationships | 3,842 | 3,183 |
| Pending relationships | 68,897 | 39,266 |
| Complexity metrics | 20,226 | 8,380 |
| Structural facts | 5,036 | 5,706 |
| Parse diagnostics | 23 | 35 |

### Test-role evidence from the corpora

Each corpus was scanned twice from the same checkout: once with the extractor at
the task's base commit and once with the rules described above.

Kotest, the corpus that exercises the call DSL:

| Role | Before | After |
| --- | ---: | ---: |
| `test_case` | 3,701 | 6,443 |
| `test_container` | 812 | 3,049 |
| `fixture_setup` | 98 | 98 |
| `fixture_teardown` | 114 | 114 |

The three previously invisible forms account for most of the gain: 2,802
string-invoke cases, 366 WordSpec `should` groups, and 287 FreeSpec `-` groups.
The vocabulary additions add 54 `scenario`, 44 `xtest`, 37 `expect`, 20
`xcontext`, 10 `xit` cases, and 62 `feature`, 11 `xdescribe` groups. Spec
classes marked as containers went from 19 to 1,494, and 21 test-factory
properties are marked as well.

Both hook counts are unchanged, and that is the point of the factory rule. An
earlier version of the container pass marked classes only, and scoping then
removed 8 hooks — the `beforeEach`/`beforeTest`/`afterTest` calls inside
`private val factory = funSpec { … }` in Kotest's own
`BeforeEachInFactoryTest.kt` and seven sibling files, which no class encloses.
Marking the factory property closes that hole.

Scoping removed 213 `test_case` roles and nothing else. All 213 are framework
infrastructure that shares the JUnit 3 vocabulary and sits outside any test
container: `FunSpecRootScope.test` and `FunSpecContainerScope.test`,
`InstancePerLeafSpecExecutor.testStarted`/`testIgnored`/`testFinished`,
`Spec.testCaseOrder` and `Spec.testExecutionMode`, and 200 more of the same
shape.

kotlinx.coroutines, the corpus that exercises the annotation path:

| Role | Before | After |
| --- | ---: | ---: |
| `test_case` | 2,933 | 2,921 |
| `test_container` | 503 | 503 |
| `fixture_setup` | 49 | 49 |
| `fixture_teardown` | 46 | 46 |

Nothing was gained here, which is the expected result: the corpus uses no Kotest
DSL, and the annotation lists were already complete before this work. The 12
removed roles are the same class of helper that Kotest shows —
`Helpers.testResultChain`, `Helpers.testResultMap`, `TestUtil.test`, and
`CoroutinesTimeoutTest.testTimedOut`, none of them inside a test container.

### Backtick and relationship evidence

Kotest carries 121 backticked declaration names. Before, all 121 published a
`symbols.name` beginning with a backtick and 79 identifiers matched that
spelling, so no report line could ever match them. After, 0 symbol names and 0
identifier names carry backticks, and all 121 publish `rawName`.
kotlinx.coroutines carries 4, all of them enum members in
`FlowFlattenMergeBenchmark.kt`.

Self-referential `calls` edges on `kotlin` symbols fell from 418 to 296 in
Kotest, and no new one appeared. 105 of the 122 removed edges name a DSL word —
`afterTest` 41, `beforeTest` 26, `context` 18, `beforeEach` 11, `afterEach` 6,
`given` 3, `describe` 3 — and the rest are description-name captures.

209 call sites lost a resolved edge, and every one of them was false. Besides
the DSL words, they are the description-name captures: 16 calls to the class
`StringShrinkerWithMin` that resolved to `describe("StringShrinkerWithMin")`, 4
`throw RuntimeException(...)` that resolved to `test("RuntimeException")`, and
30 `shouldBeEqualTo*` matcher calls that resolved to the cases named after them.
Resolved edges fell from 4,051 to 3,842 and pending edges rose from 68,878 to
68,897; the difference is the 190 call sites whose only "target" was the DSL
node itself.

A further 426 edges changed their `from` symbol without changing their target.
These are calls inside a StringSpec or WordSpec body that were previously
attributed to the enclosing declaration and are now attributed to the case that
contains them.

### Diagnostic breakdown

Kotest produced 23 `kotlin` diagnostics in 14 files of 2,493; kotlinx.coroutines
produced 35 in 16 files of 1,082. Both counts are identical before and after, so
none of them is caused by this work. Every remaining file parses clean.

The temporary checkouts and SQLite artifacts were removed after recording this
evidence.
