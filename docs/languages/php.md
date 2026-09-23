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

Five frameworks are adopted. PHPUnit declares a suite as a class and a case as
a method. Pest declares a case as a top-level `test()` or `it()` call.
Codeception declares a suite as a `*Cest` class. PHPSpec declares a suite as an
`ObjectBehavior` subclass. Behat declares step definitions and hooks on a
context class.

| Idiom | Role | Source of the rule |
| --- | --- | --- |
| `#[Test]` attribute, or a `@test` docblock tag | `test_case` | PHPUnit test metadata |
| `testXxx` method of a container | `test_case` | PHPUnit method-name prefix |
| `#[DataProvider]` attribute or `@dataProvider` docblock tag on a case | `parameterized_test` | PHPUnit data provider |
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
| `*Cest` class in a `*Cest.php` file | `test_container` | Codeception Cest |
| `_before` / `_after` method of a Cest | `fixture_setup` / `fixture_teardown` | Codeception hooks |
| public, not `_`-prefixed Cest method with a parameter type that ends in `Tester` | `test_case` | Codeception actor argument |
| class extending `ObjectBehavior` | `test_container` | PHPSpec base class |
| `let` / `letGo` method of a spec | `fixture_setup` / `fixture_teardown` | PHPSpec hooks |
| `it_*` / `its_*` method of a spec | `test_case` | PHPSpec example prefix |
| class implementing `Context`, `SnippetAcceptingContext`, `CustomSnippetAcceptingContext`, or `TranslatableContext` from the `Behat\` namespace, written qualified or bound by a `use Behat\...` import | `test_container` | Behat context |
| `#[Given]`, `#[When]`, `#[Then]`, or a `@Given`, `@When`, `@Then` docblock tag on a context method | `step_definition` | Behat step |
| `BeforeSuite`, `BeforeFeature`, `BeforeScenario`, `BeforeStep` attribute or docblock tag on a context method | `fixture_setup` | Behat hooks |
| `AfterSuite`, `AfterFeature`, `AfterScenario`, `AfterStep` attribute or docblock tag on a context method | `fixture_teardown` | Behat hooks |

### Attributes and docblocks are one vocabulary

PHPUnit spells the same metadata two ways. `#[Before]` is an attribute;
`@before` is a PHPDoc tag. The PHP extractor reads the docblock tags `@test`,
`@before`, `@after`, `@beforeClass`, `@afterClass`, and `@dataProvider` and
passes each one to the shared detector under the same key its attribute
produces, so a docblock hook classifies exactly like the attribute form. A tag
matches whole and starts a word: `@tested`, `@tested-by`, `@testdox`, and the
`@testing` in `qa@testing.example.com` are not `@test`.

Stacked attribute groups (`#[Test]` on one line, `#[DataProvider('rows')]` on
the next) share one `attribute_list` node. The extractor reads each attribute
on its own, so every attribute gives one annotation row with a clean key.

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

## Step definitions

A Behat step sets `test_role = "step_definition"` in `metadata_json` and leaves
`is_test`, `test_container`, and `test_lifecycle` at `0`, even when a path or
name rule flagged the method first: the scenario lives in a `.feature` file, so
a step is neither a case nor a hook. PHPUnit's `testXxx` and `setUp` name rules
do not apply inside a Behat context. `Context` is a common class name, so a
class whose `Context` base type is not in the `Behat\` namespace, such as
`App\Context` or a `use App\Support\Context` import, is not a Behat context,
and step attributes and tags on it publish no role.
See the [decision](../decisions/2026-09-23-step-definition-test-role.md).

Codeception (`LoginCest.php`), PHPSpec (`MoneySpec.php`), and Behat
(`FeatureContext.php`) each carry a class or method with no role as the
control.

## Members, types, and references

- A constructor-promoted parameter gives a `property` on the class and a
  parameter `variable` on the constructor. The parameter anchors on its
  `$name`, so the two rows have distinct ids.
- An assignment declares a variable only for a `$local` target or each
  target of `[$a, $b] = ...` and `list($a, $b) = ...`. A property, element,
  or static-property target declares nothing.
- Type facts come from declared parameter, property, and return types
  (`is_inferred=false`) and from a `new Foo()` initializer
  (`is_inferred=true`). A declaration kind, a namespace, an import, or an
  untyped assignment has no type fact.
- A trait `use` in a class, trait, or enum body gives a `uses` relationship:
  resolved to a same-file trait, pending otherwise. It is not an import. The
  names in its conflict list (`A::m as protected alias`) give no identifiers.
- `$x?->m()` and `$x?->p` give the same call, member-access, relationship,
  pending, and literal rows as `->`.
- `Foo::class` gives a `type_usage` of `Foo`. `Status::Active`,
  `self::ROLE`, and `static::$registry` give a `member_access` of the
  member; a `self`, `static`, or `parent` scope sets `receiver_type`.

## Wave-2 semantics

- `const A = 1, B = 2;` and `public $a, $b;` give one symbol for each element.
  A single-element declaration anchors on the declaration; a multi-element one
  anchors each symbol on its element. A typed constant keeps its type in the
  signature, in `constantType`, and as a type fact.
- `define('NAME', value)` gives a file-level `constant` symbol with `value`.
- Each `use` clause gives one `import` symbol and one
  `php.namespace_use_declaration.v1` fact. A group prefix joins the clause
  (`App\Models\{User, Post as BlogPost}`), an alias sets `import_alias`, and
  `use function` / `use const` set `importKind` / `import_kind`.
- `require`, `require_once`, `include`, and `include_once` give
  `php.include_call.v1` with `include_kind`, a static `included_path`, and a
  `path_base` of `__DIR__` when the path starts from `__DIR__` or
  `dirname(__FILE__)`.
- A function inside a braced namespace or a closure is a `function`, not a
  `method`. An abstract or interface method has no body span or body hash.
- Enums and anonymous classes give `extends` and `implements` edges. A trait
  `use` gives a `php.trait_use_declaration.v1` fact for each trait name.
- `self`, `static`, and the class name resolve `Foo::m()`, `new self`, and
  `new static` to same-file symbols. A `self` or `static` return type gives a
  type fact of the enclosing class, with the spelling in `declared`.
- A call through a chain keeps a clean receiver path: `$this->repo->find()`
  gives the display name `$this.repo.find`. A receiver that is a call gives
  only the terminal name. A variable callee (`$fn()`) gives no call row.
- Call, `new`, `instanceof`, and type identifiers use the terminal name. The
  namespace path goes to `namespace_qualifier` metadata.
- Heredoc and nowdoc arguments give literals with the dedented text.
  DBAL (`executeQuery`, `fetchOne`, ...), Doctrine (`createQuery`), and the
  Laravel `DB::` and `*Raw` methods are SQL carriers. `$request->query()` is
  excluded.
- A Pest symbol does not record a call to its own `it`/`test`/`describe` call.
- `$this->prop->get(...)` gives `http.client_request.v1` when the property is
  typed as a Guzzle or Symfony client (also a promoted parameter) or is set
  from `new Client()` or `HttpClient::create()` in the class.
- Laravel routes add `route_name`, join a `Route::controller()` group
  controller to a bare action, and map an invokable controller to
  `Ctrl@__invoke`. A provider-side `Route::prefix('api')->group(base_path(...))`
  gives `laravel.route_prefix.v1` with `mount_target`. Symfony routes add
  `route_name` with the class name prefix.
- The `string` type keyword no longer gives a string source region.

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

`php:test_roles` observes 7 `test_case` rows, 1 `parameterized_test` row,
4 `test_container` rows, 6 `fixture_setup` rows, and 5 `fixture_teardown` rows.

The golden fixture `php:wave2_semantics` proves the wave-2 rows above. Its
`tests/acceptance/LoginCest.php` and `spec/MoneySpec.php` sources add
3 `test_case`, 2 `test_container`, 2 `fixture_setup`, and 1
`fixture_teardown` rows. The `fillForm` and `helper` methods are the controls.
Focused unit tests live in `crates/julie-extractors/src/tests/php/wave2_gaps.rs`.

No real-world corpus scan was run for this contract. The evidence above is
golden-fixture evidence only.
