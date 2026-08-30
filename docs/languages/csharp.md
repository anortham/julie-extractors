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
metadata also records `csharp_visibility = "internal"`. Explicit `private` and
member declarations without a visibility modifier remain `"private"`. The
`fixtures/extraction/csharp/basic` fixture covers internal classes,
constructors, methods, properties, and fields alongside private controls.

## Test roles

The extractor adopts three .NET test frameworks by name: NUnit, MSTest
(`Microsoft.VisualStudio.TestTools.UnitTesting`), and xUnit.net. Attribute
names are matched after normalization: the key is lower-cased, reduced to its
rightmost type name, stripped of a trailing `Attribute`, and stripped of its
argument list. `[NUnit.Framework.TestCaseAttribute(1)]` and `[TestCase(1)]`
therefore both produce the key `testcase`.

| Role | Attribute keys |
| --- | --- |
| `test_case` | `test`, `testmethod`, `fact` |
| `parameterized_test` | `theory`, `datatestmethod`, `testcase`, `testcasesource` |
| `fixture_setup` | `setup`, `onetimesetup`, `testinitialize`, `classinitialize`, `assemblyinitialize` |
| `fixture_teardown` | `teardown`, `onetimeteardown`, `testcleanup`, `classcleanup`, `assemblycleanup` |
| `test_container` | `testfixture`, `testclass`, `collectiondefinition`, `setupfixture`, `testfixturesource` |

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

### Recorded gaps

Two named .NET test framework families are not adopted. Both are recorded as
`open_gaps` on the csharp row in `fixtures/extraction/capabilities.json`:

- `specflow.step_binding_test_roles` — SpecFlow and Reqnroll put the executable
  case in a `.feature` file and bind steps with `[Given]`/`[When]`/`[Then]`
  inside a `[Binding]` class. Neither the binding class nor its step methods is
  classified.
- `mspec.delegate_field_test_cases` — Machine.Specifications declares cases as
  delegate fields (`It should_do_x = () => ...`) inside a class with no
  attribute. Test roles are written only for callable symbols, so an MSpec
  context class and its fields stay unclassified.

They are recorded under `structural_facts` rather than `test_detection` because
the `test_detection` coverage vocabulary is frozen to `test_case`,
`test_container`, and `test_lifecycle`, and each of those three is already
classified exactly once for csharp.

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
