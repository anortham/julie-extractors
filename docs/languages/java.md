# Java support

Julie registers `java` for `.java` files. Kotlin reuses the same test-role
rules: `java` and `kotlin` share `detect_java_kotlin`, the annotation key
lists, and the `mark_java_test_containers` pass that runs at the end of
`extract_symbols`. A change to the annotation lists changes both languages.

## Continuous testing

Run the language target when changing Java extraction:

```bash
cargo xtask test language java
```

The command runs the Java unit-test modules and the golden extraction test with
`JULIE_GOLDEN_LANGUAGE=java`. Run the Kotlin target as well when you touch the
shared lists:

```bash
cargo xtask test language kotlin
```

The normal golden target remains unfiltered:

```bash
cargo xtask test golden
```

## Test roles

The extractor adopts three Java test frameworks by name: JUnit 3, JUnit 4/5
(Jupiter), and TestNG. Annotation names are matched after normalization: the
key is lower-cased and reduced to its rightmost name, so
`@org.testng.annotations.BeforeMethod` and `@BeforeMethod` both produce the key
`beforemethod`.

| Role | Annotation keys |
| --- | --- |
| `test_case` | `test`, `testfactory`, `testtemplate` |
| `parameterized_test` | `parameterizedtest`, `repeatedtest` |
| `fixture_setup` | `beforeeach`, `beforeall`, `before`, `beforeclass`, `beforemethod`, `beforesuite`, `beforetest`, `beforegroups` |
| `fixture_teardown` | `aftereach`, `afterall`, `after`, `afterclass`, `aftermethod`, `aftersuite`, `aftertest`, `aftergroups` |
| `test_container` | `nested`, `test` on a class |

`parameterized_test` means the runner reports one result per case, not one
result per method. `@ParameterizedTest` runs once per argument source and
`@RepeatedTest` runs once per repetition, so both earn it. `@TestFactory` and
`@TestTemplate` stay `test_case`: each is one declaration that the engine
expands at run time, and the expansion is not visible in the source.

A class also becomes a `test_container` when it directly contains a method
carrying any of those case or hook annotations. A class holding only hooks — a
shared JUnit base class — counts, because a hook is test infrastructure.

Two further container rules have no annotation on the class itself:

- A class whose `base_types` metadata contains `TestCase` is a JUnit 3
  container.
- Every class ancestor of a marked container is marked too, because JUnit runs
  an outer class whose only test content is a `@Nested` inner class.

### The TestNG class-level rule

TestNG allows `@Test` on the class. The engine then runs every public method of
that class as a case, and those methods carry no annotation of their own. The
member pass follows the .NET precedent: a public method of a `@Test`-annotated
class earns `test_case`, and a hook annotation on the same method wins, because
TestNG runs a hook around the cases instead of as one.

Non-public members are excluded. `fixtures/extraction/java/test_roles/test_source.java`
carries `LedgerTestNgTest.helperTotal` — a private method inside the
`@Test`-annotated class — as the control that must stay unclassified.

### The JUnit 3 name rule is scoped

JUnit 3 has no annotation. Its cases are `public void testXxx()` methods inside
a `TestCase` subclass, so the extractor falls back to the `test` name prefix
guarded by the shared test-path check. That vocabulary is common in ordinary
Java: listeners, extensions, and factories are full of methods named
`testName`, `testMethod`, and `testFactory`.

The fallback is therefore scoped with `normalize_scoped_test_roles`: a callable
that no test container encloses loses the role. `LedgerTestHelpers` in the
golden is that control — a plain class in a test path holding
`testDataForLedger`, which the golden shows with no test metadata.

Scoping runs for Kotlin too. Kotlin shares this pass and also earns roles from
the Kotest and Spek call DSLs; `mark_kotlin_test_containers` marks those spec
classes as containers before this pass runs, so scoping keeps the call-DSL
roles. See `docs/languages/kotlin.md`.

Because scoping clears by position and cannot see where a role came from, a
second pass re-derives every role that an annotation alone justifies. That pass
also restores an annotated Kotlin top-level test function, which has no
enclosing class at all.

### Recorded gaps

Two named Java test framework families are not adopted. Both are recorded as
`open_gaps` on the java row in `fixtures/extraction/capabilities.json`:

- `cucumber.step_binding_test_roles` — Cucumber-JVM puts the executable
  scenario in a `.feature` file and binds steps with `@Given`/`@When`/`@Then`
  on methods of a glue class. Neither the glue class nor its step methods is
  classified.
- `junit_platform.suite_container_roles` — the JUnit Platform Suite engine
  declares an aggregating suite with `@Suite` plus selectors such as
  `@SelectClasses`. The class holds no test member of its own, so the container
  pass never marks it and the selected classes are never linked to it.

They are recorded under `structural_facts` rather than `test_detection` because
the `test_detection` coverage vocabulary is frozen to `test_case`,
`test_container`, and `test_lifecycle`, and each of those three is already
classified exactly once for java.

The `@Suite` gap is measurable. The JUnit 5 corpus below holds 47 classes
carrying `@Suite`; only 2 of them are marked containers, and those 2 qualify
through unrelated members.

## Relationships

Java emits an `extends` edge when the superclass is declared in the same file,
and a structured pending `extends` edge when it is not. The ledger row
previously advertised only `calls` and `implements`; `extends` was emitted but
undeclared. The golden now carries `LedgerJUnit5Test extends AbstractLedgerTest`
as the registered evidence.

## Grammar freshness

```bash
node scripts/grammar-freshness-report.mjs --format json
```

The report says `tree-sitter-java` is `current`: declared `0.23.5`, locked
`0.23.5`, latest stable `0.23.5`.

## Real-world evidence

Two corpora were used, one per framework family. Both were cloned shallowly
into temporary directories. No project build scripts, hooks, or third-party
binaries were executed.

- TestNG at commit `5b0746bc2a396faaa27ba7420ef9e7c52d574c92`, Apache-2.0.
- JUnit at commit `9cd9a3cfb6cd98aec355bd49fc8d801058762441`, EPL-2.0.

Reproducible checkout and scan commands:

```bash
CORPUS="$(mktemp -d)"
git clone --depth 1 https://github.com/testng-team/testng "$CORPUS"
git -C "$CORPUS" checkout --detach 5b0746bc2a396faaa27ba7420ef9e7c52d574c92

cargo build --locked --bin julie-extract
ARTIFACT="$(mktemp -d)"
./target/debug/julie-extract scan \
  --root "$CORPUS" \
  --db "$ARTIFACT/artifact.sqlite" \
  --json >"$ARTIFACT/scan-report.json" \
  2>"$ARTIFACT/scan-stderr.log"
```

Both scan reports were `status=ok` with `files_failed=0`, empty `errors`, and
empty `warnings`. TestNG reported `files_scanned=2707`, `files_changed=2706`,
and `files_unsupported=43`. JUnit reported `files_scanned=2360`,
`files_changed=2360`, and `files_unsupported=318`. Per-language counts below
come from the SQLite artifacts.

| Artifact evidence | TestNG | JUnit |
| --- | ---: | ---: |
| Indexed `java` files | 2,403 | 1,738 |
| Symbols | 32,576 | 41,149 |
| Identifiers | 130,787 | 187,853 |
| Resolved relationships | 3,096 | 6,708 |
| Pending relationships | 38,383 | 65,984 |
| Complexity metrics | 13,471 | 16,676 |
| Structural facts | 9,523 | 23,054 |
| Parse diagnostics | 0 | 12 |

### Test-role evidence from the corpora

Each corpus was scanned twice from the same checkout: once with the extractor
at the task's base commit and once with the rules described above. The tables
compare the two artifacts symbol by symbol.

TestNG, the framework whose hook annotations this work added:

| Role | Before | After |
| --- | ---: | ---: |
| `test_case` | 3,484 | 3,550 |
| `test_container` | 1,473 | 1,632 |
| `fixture_setup` | 188 | 596 |
| `fixture_teardown` | 92 | 308 |

408 more setup hooks and 216 more teardown hooks come from the eight TestNG
keys. The corpus carries 416 setup-key annotations — `beforemethod` 214,
`beforesuite` 102, `beforetest` 60, `beforegroups` 40 — spread over 410
distinct methods, and 226 teardown-key annotations: `aftermethod` 97,
`aftersuite` 65, `aftergroups` 33, `aftertest` 31. Only 2 of the setup methods
and 2 of the teardown methods also carry a key the extractor already knew, so
almost every one of them published no role at all before.

106 classes in the corpus carry class-level `@Test`. They and their public
methods account for the 159 new containers and most of the 77 new cases.
`AnnotationTransformerClassInvocationSampleTest` is a plain example: the class
carries `@Test(invocationCount = 3)` and its two methods, `f1` and `f2`, carry
nothing at all. Both now publish `test_case`.

Scoping removed 8 roles and no more. All 8 are helper methods outside any test
container: `DataProviderListener.testName`, a private display-name helper;
`OrderFactory.testF`, a `@Factory` method that builds instances;
`AbstractTestClassGenerator.testFactory`; and five similar cases. Three more
methods moved from `test_case` to `fixture_setup` — each is named `testXxx`
*and* annotated `@BeforeMethod`, so the annotation now wins over the name.

JUnit, the framework whose parameterized and dynamic annotations this work
added:

| Role | Before | After |
| --- | ---: | ---: |
| `test_case` | 5,799 | 5,394 |
| `parameterized_test` | 0 | 497 |
| `test_container` | 1,518 | 1,590 |
| `fixture_setup` | 319 | 333 |
| `fixture_teardown` | 193 | 207 |

The 497 `parameterized_test` rows are exactly the corpus's 441
`@ParameterizedTest` and 56 `@RepeatedTest` methods; all of them reported
`test_case` before. 103 methods gained `test_case` from `@TestFactory` (100 in
the corpus) and `@TestTemplate` (28). The 14 new setup and 14 new teardown
hooks are JUnit Platform Suite's `@BeforeSuite` and `@AfterSuite`, which share
the TestNG keys.

Scoping removed 11 roles here, again all helpers outside a container:
`NoopTestExecutionListener.testPlanExecutionStarted`, four
`KitchenSinkExtension` callbacks named `testDisabled`, `testSuccessful`,
`testAborted`, and `testFailed`, and six more of the same shape.

### Diagnostic breakdown

The 12 JUnit diagnostics fall in 6 files of 1,738; the other 1,732 parse clean.
TestNG produced none. Two grammar limitations explain all 12, and neither is an
extractor defect:

- `ClassUtils.java` and `ReflectionUtils.java` declare a type-use annotation on
  a varargs array type: `@Nullable Class<?> @Nullable... classes`.
  tree-sitter-java 0.23.5 does not accept the annotation between the element
  type and the `...`.
- The four files under `platform-tooling-support-tests/projects/junit-start/`
  are Java 25 compact source files: they use `import module org.junit.start;`
  and a top-level instance `void main()`, neither of which the pinned grammar
  knows.

The temporary checkouts and SQLite artifacts were removed after recording this
evidence.
