# PHP support

Julie registers one PHP language: `php` handles `.php` and `.phtml` files.

## Continuous testing

Run the language target when changing PHP extraction:

```bash
cargo xtask test language php
```

The command runs the PHP unit-test modules and the golden extraction test with
`JULIE_GOLDEN_LANGUAGE=php`. The normal golden target stays unfiltered:

```bash
cargo xtask test golden
```

## Test-role contract

Two frameworks are adopted. PHPUnit declares a suite as a class and a case as a
method. Pest declares a case as a top-level `test()` or `it()` call.

| Idiom | Role | Source of the rule |
| --- | --- | --- |
| `#[Test]` attribute, or a `@test` docblock tag | `test_case` | PHPUnit test metadata |
| `testXxx` method of a container | `test_case` | PHPUnit method-name prefix |
| `#[DataProvider]` on a case | `parameterized_test` | PHPUnit data provider |
| `setUp`, `setUpBeforeClass` | `fixture_setup` | PHPUnit fixture methods |
| `tearDown`, `tearDownAfterClass` | `fixture_teardown` | PHPUnit fixture methods |
| `#[Before]`, `#[BeforeClass]`, or a `@before` docblock tag | `fixture_setup` | PHPUnit hook metadata |
| `#[After]`, `#[AfterClass]`, or an `@after` docblock tag | `fixture_teardown` | PHPUnit hook metadata |
| class extending `TestCase` | `test_container` | PHPUnit base class |
| class holding a member that carries a role | `test_container` | container pass |
| `it(...)`, `test(...)` call | `test_case` | Pest case |
| `describe(...)` call | `test_container` | Pest group |
| `beforeEach(...)`, `beforeAll(...)` call | `fixture_setup` | Pest hooks |
| `afterEach(...)`, `afterAll(...)` call | `fixture_teardown` | Pest hooks |

### Attributes and docblocks are one vocabulary

PHPUnit spells the same metadata two ways. `#[Before]` is an attribute;
`@before` is a PHPDoc tag. The PHP extractor reads the docblock tags `@test`,
`@before`, `@after`, `@beforeClass`, and `@afterClass` and passes each one to
the shared detector under the same key its attribute produces, so a docblock
hook classifies exactly like the attribute form. A tag matches whole: `@tested`
is not `@test`.

### A provider is a helper, not a case

A `#[DataProvider('provideRows')]` method supplies argument rows and asserts
nothing, so it earns no role. The rule needs no special case: a provider is not
`test`-prefixed and carries no hook metadata. The method that *names* the
provider is the case, and it reports `parameterized_test` because PHPUnit
reports one result per row instead of one per method.
`ArithmeticTest.php` carries `provideRows` as the control.

### Two proofs outside a test path

An attribute or a docblock tag names a case wherever the file sits, because
neither spelling occurs in ordinary PHP. Two more proofs work outside a test
path:

- **`extends TestCase`.** PHPUnit's own base class makes a class a test
  container. The base type is compared on its last namespace segment, so the
  short imported name and the fully qualified `\PHPUnit\Framework\TestCase`
  both match.
- **The `*Test.php` filename.** The shared path guard reads it as a test path,
  which is what PHPUnit's own default suffix configuration collects.

The member pass then classifies the container's name-convention members —
PHPUnit collects `testXxx` and runs `setUp`/`tearDown` on the name alone.
`legacy_suite.php` proves both proofs from a production path.

### The name rules stay guarded

`testConnection()` and `setUp()` are ordinary PHP. The name rules therefore
fire only inside a test path, or inside a class the container pass already
marked. `ConnectionProbe` in `legacy_suite.php` carries both names in a
production path, outside any container, and publishes no role.

Pest needs the same guard from the other direction. `test('x', fn)` and
`it('x', fn)` are plain function calls, so outside a test path a Pest role
survives only inside a container. `production_roles.php` calls `describe`,
`it`, `test`, and `beforeEach` at file scope from a production path and
publishes no role at all.

## Recorded gaps

Three PHP test frameworks are recorded as `open_gaps` on the php row in
`fixtures/extraction/capabilities.json`, under
`kind_coverage.structural_facts.open_gaps`. The `test_detection` vocabulary is
frozen to `test_case`, `test_container`, and `test_lifecycle`, and php
classifies each exactly once, so a php-specific gap cannot live there.

- `codeception.cest_and_actor_roles`. A `*Cest.php` class extends nothing and
  carries no attribute, its hooks are named `_before` and `_after`, and a case
  is a public method that takes an actor argument. None of that reaches a rule
  today.
- `behat.step_definition_roles`. Behat binds steps with `#[Given]`, `#[When]`,
  and `#[Then]` on a context class, and the executable scenario lives in a
  `.feature` file, not in PHP.
- `phpspec.example_roles`. PHPSpec collects a `*Spec.php` class extending
  `ObjectBehavior` and runs its `it_`/`its_` methods as examples. Neither the
  base class nor the name convention matches a rule today.

## Class base types

`extract_class` now emits a `base_types` array beside the existing `extends`
and `implements` strings: the extended class first, then each implemented
interface, each spelled the way the source spells it with a leading `\`
trimmed. This is the same metadata key the C++, Java, Ruby, GDScript, and QML
extractors publish, so a consumer reads one key for every language.

## Evidence

The golden fixture `php:test_roles` registers four sources:

| Source | What it proves |
| --- | --- |
| `ArithmeticTest.php` | the PHPUnit class: four fixture names, `#[Before]`/`#[After]`, `@before`/`@after`/`@test` docblocks, `#[Test]`, the `testXxx` prefix, `#[DataProvider]`, and the provider and helper controls |
| `PestFeatureTest.php` | the Pest DSL: `describe`, `it`, `test`, `beforeEach`, `afterEach`, and the `$ordinary->test(...)` member-call control |
| `legacy_suite.php` | the two out-of-tree proofs — a fully qualified `TestCase` subclass and a `#[Test]`-holding class — plus the `ConnectionProbe` production control |
| `production_roles.php` | the production-path Pest control |

The registered goldens observe 7 `test_case` rows, 1 `parameterized_test` row,
4 `test_container` rows, 6 `fixture_setup` rows, and 5 `fixture_teardown` rows
for php.

No real-world corpus scan was run for this contract. The evidence above is
golden-fixture evidence only.
