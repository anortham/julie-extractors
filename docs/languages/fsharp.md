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
`>]`).

F# xUnit functions carrying `[<Fact>]`, `[<Theory>]`, or qualified
`[<Xunit.Fact>]` publish `is_test = true` and `test_role` values `test_case` or
`parameterized_test`. Similar names and unannotated functions remain ordinary
symbols. Test containers and lifecycle roles are not claimed.

## Recorded gaps

The capability row in `fixtures/extraction/capabilities.json` is the source of
truth for F# gaps. It records the current limits with a reason, required
closure, and planned follow-up for:

- top-level `.fsx` imports without an enclosing symbol;
- F# domain-native facts beyond attributes, including computation expressions,
  active patterns, and quotations;
- test containers and lifecycle roles; and
- Expecto, NUnit, and FsUnit role coverage.

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
