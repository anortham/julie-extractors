# C# support

Julie registers `csharp` for `.cs` files. Two more languages reuse the same
.NET test-role rules: `vbnet` and `razor`. All three call
`mark_dotnet_test_containers` at the end of `extract_symbols`, so a change to
the .NET attribute lists changes all three.

## Continuous testing

Run the language target when changing C# extraction:

```bash
cargo xtask test language csharp
```

The command runs the C# unit-test modules and the golden extraction test with
`JULIE_GOLDEN_LANGUAGE=csharp`. The normal golden target remains unfiltered:

```bash
cargo xtask test golden
```

## Visibility

Explicit `internal` declarations publish `visibility = "internal"`; class
metadata also records `csharp_visibility = "internal"`. A declaration with no
visibility modifier takes the C# default for its position:

| Position | Default |
| --- | --- |
| Type declared in a namespace or at file level | `internal` |
| Interface member, enum member | `public` |
| Any other member, including constructors and nested types | `private` |

The `fixtures/extraction/csharp/basic` fixture covers internal classes,
constructors, methods, properties, and fields alongside private controls, and
an interface whose members publish `public`.

## Body spans

The body span is the declaration's own body node: the block, the `=>` arrow
clause, the accessor list, or the member list. Abstract, extern, interface,
and partial-definition methods, delegates, fields, locals, parameters, and
records without a body publish no body span and no body hash. A `foreach`
loop variable spans its `Type name` header, not the whole loop.

## Base lists and calls

- A base-list edge starts at the declaring type and targets a type symbol
  only. The target name is the bare type name: `RepositoryBase<Order>`
  targets `RepositoryBase`, and `System.Exception` targets `Exception` with
  namespace path `["System"]`. Primary-constructor arguments are not bases.
- A generic call (`Create<T>()`) is a `call` identifier named `Create`. Its
  type arguments ride on that identifier, and the name is not a `type_usage`.
- A null-conditional call (`x?.M()`) is a call with receiver `x`.
- `base.M()` targets the base type's `M` when that type is in the file.
  Otherwise it is a pending call with receiver `base` and `receiver_type` set
  to the first base-list type.
- `nameof(x)` is not a call.

## Test roles

The extractor adopts these .NET test frameworks by name: NUnit, MSTest
(`Microsoft.VisualStudio.TestTools.UnitTesting`), xUnit.net, TUnit,
SpecFlow/Reqnroll hooks, and Machine.Specifications (MSpec). Attribute
names are matched after normalization: the key is lower-cased, reduced to its
rightmost type name, stripped of a trailing `Attribute`, and stripped of its
argument list. `[NUnit.Framework.TestCaseAttribute(1)]` and `[TestCase(1)]`
therefore both produce the key `testcase`.

| Role | Attribute keys |
| --- | --- |
| `test_case` | `test`, `testmethod`, `fact` |
| `parameterized_test` | `theory`, `datatestmethod`, `testcase`, `testcasesource`, `arguments`, `methoddatasource`, `classdatasource`, `matrixdatasource` |
| `fixture_setup` | `setup`, `onetimesetup`, `testinitialize`, `classinitialize`, `assemblyinitialize`, `before`, `beforeevery`, `beforescenario`, `beforefeature`, `beforetestrun`, `beforestep`, `beforescenarioblock` |
| `fixture_teardown` | `teardown`, `onetimeteardown`, `testcleanup`, `classcleanup`, `assemblycleanup`, `after`, `afterevery`, `afterscenario`, `afterfeature`, `aftertestrun`, `afterstep`, `afterscenarioblock` |
| `test_container` | `testfixture`, `testclass`, `collectiondefinition`, `setupfixture`, `testfixturesource`, `binding` |
| `step_definition` | `given`, `when`, `then`, `stepdefinition`, on a method of a `binding` class |

A class or struct also becomes a `test_container` when it directly contains a
method carrying any `test_case` or `parameterized_test` attribute. This is how
xUnit containers are found: xUnit has no class-level attribute.

`parameterized_test` means the runner reports one result per data row, not one
result per method. `[TestFixtureSource]` is NUnit's class-level parameterized
fixture attribute — it supplies constructor arguments to the fixture — so it
names a container, not a case.

### The xUnit lifecycle pass

xUnit defines no setup or teardown attribute. The constructor is the per-test
setup, and `IDisposable.Dispose`, `IAsyncDisposable.DisposeAsync`, and
`IAsyncLifetime.InitializeAsync`/`DisposeAsync` are the remaining hooks.

Those names are ordinary C# everywhere else, so they are classified by name
only inside a type the attribute or member pass already marked as a test
container:

- constructor and `InitializeAsync` → `fixture_setup`
- `Dispose` and `DisposeAsync` → `fixture_teardown`

A member that carries its own .NET test attribute keeps the role that attribute
gives it; the name rule never overrides an attribute. An ordinary
`IDisposable` class outside a test container stays unclassified.
`fixtures/extraction/csharp/test_roles/source.cs` holds `ManagedResource` as
that control: it declares a constructor, `InitializeAsync`, `Dispose`, and
`DisposeAsync`, and the golden shows all four with no test metadata at all.

### Structs are containers too

The container pass accepts `SymbolKind::Class` and `SymbolKind::Struct`, so a
`struct` or `record struct` test type is marked. The golden covers both.

### MSpec contexts

Machine.Specifications declares cases as delegate fields with no attribute:
`It should_x = () => ...`. A class with an `It`, `Establish`, or `Because`
field is a `test_container`. The lambda each field holds gets the role:
`It` → `test_case`, `Establish` and `Because` → `fixture_setup`, and
`Cleanup` → `fixture_teardown`. The field type comes from its declared type
fact, so a field of any other delegate type stays unclassified.

### Step definitions

A method of a `[Binding]` class that carries `[Given]`, `[When]`, `[Then]`, or
`[StepDefinition]` gets the `step_definition` role (SpecFlow and Reqnroll). The
scenario lives in a `.feature` file, so a step is neither a case nor a hook: it
sets `test_role = "step_definition"` in `metadata_json` and leaves `is_test`,
`test_container`, and `test_lifecycle` at `0`. The same attributes on a method
of any other class publish no role. See the
[decision](../decisions/2026-09-23-step-definition-test-role.md).

## Declarations and scopes

- A positional record parameter is a public `Property` of the record:
  `{ get; init; }`, or `{ get; set; }` for a mutable `record struct`. A class
  primary-constructor parameter stays a private parameter variable.
- A `params T[] name` parameter is a parameter variable with a declared type
  fact. The grammar flattens it into the parameter list.
- A pattern designation (`is Order o`, `case Customer c`, `Invoice { } inv`)
  is a local variable of the matched type.
- An implicitly typed lambda parameter has no type fact. Its signature is the
  name alone.
- An `extension(T receiver)` block publishes the receiver as a parameter
  variable. Each member gets metadata `extendedType` with the receiver type.
- An `event` with `add`/`remove` accessors is an `Event` symbol with a body
  span and a type fact.
- A `using` import keeps its alias target and markers:
  `global using static A.B`, `using Json = System.Text.Json.JsonSerializer`,
  `using Pair = (int X, int Y)`.
- An attribute with a target (`[return: NotNull]`) is an annotation whose
  `carrier` is the target. Parameter attributes are annotations on the
  parameter variable. Every attribute name is a `type_usage` identifier owned
  by the decorated declaration.
- Type facts come only from the syntax tree. Tuple, pointer, and `void`
  types record no fact. Signatures show the written type (`int*`, `ref int`,
  `Bits?`).
- A `var` local gets an inferred type fact from `new T(..)` or from a call to
  a same-file method or local function with a declared return type:
  `Load()`, `this.Load()`, or a static `Factory.Create()` on a same-file type.
  A simple name finds an in-scope local function, then the innermost enclosing
  type that declares the name. The search stops at a type with a base list or
  `partial`, which may get members from another file. Overloads that accept
  the argument count must agree. `await` removes one `Task<T>` or
  `ValueTask<T>` layer, also through `.ConfigureAwait(..)`. `!` and
  parentheses keep the type. A type-parameter return, a tuple or `void`
  return, a call on any other receiver, a chained call, or a callee in another
  file records no fact. Razor applies the same rule to `@code` and `@{ }`
  blocks, and `@typeparam` names count as type parameters.
- The innermost member owns a reference site: a property, indexer, or event
  accessor body owns its calls, identifiers, and complexity metric.

## Calls and instantiation

- A bare call `M()` binds to the one method `M` of the enclosing type. When
  it stays pending, `receiver_type` is the enclosing type name.
- Target-typed `new(...)` instantiates the declared type of the local,
  field, property, or return position it initializes.
- `new T()` for a type parameter `T` records no instantiation.

## Conditional compilation

Before the parse, preprocessor directive lines are blanked to spaces, and so
is every `#elif`/`#else` branch of each `#if` group. The `#if` branch stays.
Byte offsets do not change. This keeps an `#if` inside an `else if` chain, a
switch section, or a base list from breaking the parse. `#:` file-app
directives are not blanked.

## Framework facts

- Attribute routes: a method-level `[Route("x")]` joins a template-less
  `[HttpGet]`. `[AcceptVerbs("GET", "POST")]` gives one fact per verb.
- Minimal APIs: nested `MapGroup` prefixes compose. `MapHub<T>`, `Map`, and
  `MapHealthChecks` give `aspnet.minimal_api.route.v1` facts with
  `endpoint_kind`. `MapGrpcService` and `MapFallbackToFile` have no route
  template and give no fact.
- `aspnet.conventional_route.v1`: `MapControllerRoute`,
  `MapAreaControllerRoute`, and `MapDefaultControllerRoute`.
- EF Core: `efcore.db_set.v1` for `DbSet<T>` properties,
  `efcore.table_mapping.v1` for `Entity<T>().ToTable("t")`,
  `EntityTypeBuilder<T>` parameter `ToTable`, and `[Table("t")]`, and
  `efcore.entity_configuration.v1` for `IEntityTypeConfiguration<T>` classes.
- SQL carriers include `FromSql`, `ExecuteSql*`, `SqlQuery*`, and the
  ADO.NET `*Command` constructors.

`fixtures/extraction/csharp/language_idioms` is the golden evidence for this
section and the two before it.

## Grammar freshness

The grammar is pinned in `Cargo.lock` to the fork
`https://github.com/anortham/tree-sitter-c-sharp` at
`688cf95ae4c984638557dab73253bd66719bdd5c`, package version `0.23.5`.

```bash
node scripts/grammar-freshness-report.mjs --format json
```

The report could not compare that pin against the remote head during this
work: GitHub answered `HTTP 403` for `anortham/tree-sitter-c-sharp`, the
unauthenticated rate-limit response. The pin above comes from `Cargo.lock`, not
from the report. Re-run the report with a GitHub token to get the drift
verdict.

## Real-world evidence

The evidence corpus was `Newtonsoft.Json` at commit
`09bb545d72969ad7fb4ea07db0d5c34f4fc07877`. It was cloned shallowly into a
temporary directory. No project build scripts, hooks, or third-party binaries
were executed. The checkout is MIT-licensed.

Reproducible checkout and scan commands:

```bash
CORPUS="$(mktemp -d)"
git clone --depth 1 https://github.com/JamesNK/Newtonsoft.Json "$CORPUS"
git -C "$CORPUS" checkout --detach \
  09bb545d72969ad7fb4ea07db0d5c34f4fc07877

cargo build --locked --bin julie-extract
ARTIFACT="$(mktemp -d)"
./target/debug/julie-extract scan \
  --root "$CORPUS" \
  --db "$ARTIFACT/artifact.sqlite" \
  --json >"$ARTIFACT/scan-report.json" \
  2>"$ARTIFACT/scan-stderr.log"
```

The scan report was `status=ok` with `files_scanned=1170`,
`files_changed=984`, `files_unsupported=186`, `files_failed=0`, and empty
`errors`. One warning was raised: `slow_file_skipped` for
`Src/Newtonsoft.Json.Tests/large.json`, which exceeds the 1,048,576-byte
extraction limit. Per-language counts below come from the SQLite artifact.

| Artifact evidence | `csharp` |
| --- | ---: |
| Indexed files | 945 |
| Symbols | 37,801 |
| Identifiers | 195,763 |
| Resolved relationships | 3,112 |
| Pending relationships | 44,546 |
| Complexity metrics | 8,454 |
| Structural facts | 9 |
| Parse diagnostics | 857 |

### Test-role evidence from the corpus

| Role | Symbols |
| --- | ---: |
| `test_case` | 3,254 |
| `test_container` | 311 |
| `parameterized_test` | 8 |
| `fixture_setup` | 2 |

The corpus is an NUnit suite. The attribute keys it actually uses are `test`
(3,254), `testfixture` (307), `testcasesource` (5), `testcase` (3), and `setup`
(1).

Three of this task's changes fire on this real project:

- The five methods carrying only `[TestCaseSource]` were previously
  unclassified — `testcasesource` was not a recognized key — and now publish
  `parameterized_test`.
- The three `[TestCase]` methods moved from `test_case` to
  `parameterized_test`.
- `TestFixtureBase`'s constructor, inside the `[TestFixture]`-marked
  `TestFixtureBase` class, now publishes `fixture_setup` through the xUnit
  lifecycle pass.

The corpus contains no MSTest attributes, no xUnit attributes, and no struct
test types, so evidence for those paths comes from the golden fixture only.

### Diagnostic breakdown

The 857 C# diagnostics are 842 `error` and 15 `missing`, spread over 42 files.
903 of the 945 C# files produced none.

Every one of the 42 files uses C# conditional compilation
(`#if` / `#elif` / `#else` / `#endif`). tree-sitter does not evaluate
preprocessor conditions, so a directive that splits a statement, a `switch`
section, or an `if`/`else` chain breaks the parse at that point, and the error
region can then cascade through the rest of the file. One file,
`Src/Newtonsoft.Json/Serialization/JsonSerializerInternalReader.cs`, accounts
for 621 of the 857 by exactly that cascade: it places `case` labels inside
`#if HAVE_DYNAMIC` and `#if HAVE_BINARY_SERIALIZATION` blocks.

The directive alone is not the trigger. 528 of the 945 files contain
conditional directives and 486 of those parse clean, because their directives
sit between whole declarations rather than inside one.

To confirm the cause, the corpus was copied and every conditional directive was
resolved by keeping the first branch and dropping the directive lines and the
alternate branches. Re-scanning that copy produced **1** C# diagnostic instead
of 857. The single remaining diagnostic is a `missing` at end of file in
`Src/Newtonsoft.Json.Tests/Issues/Issue3080.cs`, which parsed clean in the
original scan — the rewrite unbalanced a brace in that file. No grammar
limitation and no extractor defect was found in valid, preprocessor-free C#.

The temporary checkouts and SQLite artifacts were removed after recording this
evidence.
