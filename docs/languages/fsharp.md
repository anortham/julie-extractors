# F# support

Julie registers `fsharp` for `.fs`, `.fsx`, and `.fsi` files. The source and
script extensions use `tree_sitter_fsharp::LANGUAGE_FSHARP`; signature files
use `tree_sitter_fsharp::LANGUAGE_SIGNATURE`. All three parser choices publish
the stable artifact language `fsharp`.

Run the language target when changing F# extraction:

```bash
cargo xtask test language fsharp
```

The command runs the F# unit-test modules and the golden extraction test with
`JULIE_GOLDEN_LANGUAGE=fsharp`. The canonical evidence families are:

- `fixtures/extraction/fsharp/basic` — modules, classes, records, unions,
  methods, properties, fields, annotations, calls, inheritance, types,
  literals, and complexity.
- `fixtures/extraction/fsharp/script` — top-level `.fsx` values and functions.
- `fixtures/extraction/fsharp/signature` — `.fsi` namespaces, modules,
  signatures, record fields, and the signature parser.
- `fixtures/extraction/fsharp/test_roles` — xUnit `Fact` and `Theory`
  functions, including a qualified attribute and an ordinary control.
- `fixtures/extraction/fsharp/negative` — a bare value read, member access,
  and unresolved call control.

## Recorded facts

F# emits symbol rows for declarations: modules, namespaces, classes, structs,
unions, union cases, methods, properties, fields, functions, variables, and
the type declarations exposed by the grammar. Identifier rows are usage-only:
`call`, `member_access`, `type_usage`, and `variable_ref`. A declaration is
never represented by an identifier kind.

Relationships include exact local calls and inheritance, plus structured
pending calls and imports when the grammar supplies a caller scope. Explicit
type annotations produce non-inferred type facts; scalar literal initializers
produce inferred type facts. Generic type arguments are retained in
`type_argument_usages` with nested argument positions.
The F# extraction policy retains unclassified scalar literals as `other` rows;
it does not assign URL, SQL, or route carriers without evidence.

Comments, `///` XML documentation comments, and F# string forms publish exact
`source_regions` spans. Attribute nodes publish the registered
`fsharp.attribute.v1` structural fact with `metadata` query-family metadata and
the annotated declaration as `containing_symbol_id`. The attribute fact captures
the grammar's `attribute` node span (the attribute name, excluding `[<` and
`>]`). The annotated declaration must sit in the attribute's own scope; an
attribute with no in-scope annotated target, such as `[<assembly: ...>]`, keeps
its lexical container as `containing_symbol_id`.

F# tests use the .NET attribute contract shared with C#: xUnit `[<Fact>]`,
NUnit `[<Test>]`, and MSTest `[<TestMethod>]` publish `test_case`;
`[<Theory>]`, `[<TestCase>]`, `[<TestCaseSource>]`, and `[<DataTestMethod>]`
publish `parameterized_test`; NUnit and MSTest setup and teardown attributes
publish `fixture_setup` and `fixture_teardown`. Qualified attributes such as
`[<Xunit.Fact>]` match by their last segment. A class with a container
attribute (`[<TestFixture>]`, `[<TestClass>]`) or a test member is a
`test_container`. An Expecto `[<Tests>]` value publishes `test_case`, so test
impact reaches the test list. The `testCase` entries inside the list are
expressions, not declarations, so they carry no role of their own. Similar names and unannotated functions remain
ordinary symbols.

Calls include pipelines and operators: `x |> f`, `f <| x`, and `a + f x` all
emit a call to `f`, and a generic call (`f<T>()`) is a `call` whose type
arguments ride on the call identifier. A call belongs to the smallest
enclosing declaration: a method, property, module value, or nested module, not
the surrounding namespace. A call to a class in the same file is an
`instantiates` edge. `type ... and ...` and `let rec ... and ...` groups emit
one symbol per declaration. `exception` declarations are classes; an unnamed
union or exception field (`Cash of decimal`) is only a type, not a field
symbol. `private` and `internal` on `let` bindings and types set visibility,
and a `let` inside a class is private.

Body spans come from the syntax tree: the expression after `=` for bindings
and members, the member blocks of a type, and the declarations after a module
or namespace header.

## Recorded gaps

The capability row in `fixtures/extraction/capabilities.json` is the source of
truth for F# gaps. It records the current limits with a reason, required
closure, and planned follow-up for:

- top-level `.fsx` imports without an enclosing symbol;
- F# domain-native facts beyond attributes, including computation expressions,
  active patterns, and quotations.

The pinned Expecto evidence scan also shows the current boundary: its F# files
produce symbols, relationships, identifiers, types, type-argument usages,
complexity metrics, annotations, scalar literals, source regions, and attribute
structural facts, while xUnit roles remain absent because the corpus does not
use xUnit attributes. Parse diagnostics are reported rather than hidden; the
121 `error` and 8 `missing` rows in the pinned scan are all upstream grammar
limitations observed at concrete source forms, not extractor failures. They
cover semicolonless multiline record fields (`Expecto.Sample`), qualified
union-case patterns in record patterns (`Expecto.Tests/Prelude.fs` and
`Expecto.Hopac.Tests/Tests.fs`), multiline ordinary strings with embedded
escaped quotes (`Expecto.Tests/FsCheckTests.fs`), compact `function |` and
no-space `->` forms (`Build/Program.fs` and `Expecto/Expecto.Impl.fs`),
file-level `module internal` declarations (`Expecto/Async.fs`), numeric
unit-of-measure aliases (`Expecto/Performance.fs`), pointer/flexible externs
and type-extension/task-builder syntax (`Expecto/Logging.fs` and
`Expecto/Expecto.fs`), and layout-heavy computation-expression/test-list
forms (`Expecto/Progress.fs` and `Expecto.Tests/Tests.fs`).

## Grammar freshness

`tree-sitter-fsharp` is pinned exactly to version `0.3.0` from crates.io in
`Cargo.lock` (checksum
`054fba748f8bf3604fc14191b4e7da66d1b887de0e285e32cf6dbd2a3db3fc42`). Run:

```bash
node scripts/grammar-freshness-report.mjs --format json
```
