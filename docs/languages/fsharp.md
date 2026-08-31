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

F# xUnit functions carrying `[<Fact>]`, `[<Theory>]`, or qualified
`[<Xunit.Fact>]` publish `is_test = true` and `test_role` values `test_case` or
`parameterized_test`. Similar names and unannotated functions remain ordinary
symbols. Test containers and lifecycle roles are not claimed.

## Recorded gaps

The capability row in `fixtures/extraction/capabilities.json` is the source of
truth for F# gaps. It records the current limits with a reason, required
closure, and planned follow-up for:

- top-level `.fsx` imports without an enclosing symbol;
- F# domain-native structural facts and source-region rows;
- test containers and lifecycle roles; and
- Expecto, NUnit, and FsUnit role coverage.

The pinned Expecto evidence scan also shows the current boundary: its F# files
produce symbols, relationships, identifiers, types, type-argument usages,
complexity metrics, and annotations, while this artifact path emits no F#
literals, source regions, or xUnit roles for that corpus. Parse diagnostics are
reported rather than hidden; see the Task 4 verification ledger for the exact
counts and representative rows.

## Grammar freshness

`tree-sitter-fsharp` is pinned exactly to version `0.3.0` from crates.io in
`Cargo.lock` (checksum
`054fba748f8bf3604fc14191b4e7da66d1b887de0e285e32cf6dbd2a3db3fc42`). Run:

```bash
node scripts/grammar-freshness-report.mjs --format json
```
