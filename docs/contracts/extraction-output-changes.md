# Declared extraction-output changes

Same-epoch compatibility is a gated claim, never an assumption (v4 contract §7/§16.8). Two
`julie-extract` binaries that report the same schema epoch must produce byte-equivalent extraction
output for the same source tree — otherwise a consumer that merges or trusts artifacts across
binaries reads a silently wrong index.

`cargo xtask compat-check` enforces that claim. It scans
`fixtures/extraction/resolution_contract/` with the previous published release binary and with the
current build, dumps every comparable extraction table from both artifacts, and byte-compares the
dumps.

**Gate invariant:** an extractor binary change that alters per-version extraction output cannot
merge silently. It either byte-matches the previous release on the fixture, or it names itself in
this ledger.

## What the gate compares

Tables are enumerated at run time from `sqlite_master`, so a table added or dropped by a schema
change is itself a reported difference that this ledger must declare.

Excluded from the comparison:

- `artifact_metadata`, `extraction_revisions`, `revision_file_changes` — per-scan identity and
  timestamps, so two runs of the *same* binary already differ there.
- `files.indexed_at` and `files.last_revision_id` — per-scan columns inside a compared table.
- `identifier_resolutions` and `pending_resolutions` — schema v7 removed these overlay tables.
  The previous release still writes them. Excluding them keeps the gate on fact-table identity
  and classifies the removal as intentional.
- `language_capability_gaps` — the previous release wrote `reference_resolution.*` gap rows.
  This binary does not. The remaining extractor capability-gap rows stay in the artifact;
  the gate excludes the table so that retired resolver snapshot is not an epoch bump.

The retired overlay tables are not a silent fact-table change. A v2.33.7 reader that joins
them will not find them on a v7 artifact. That break is recorded below.

Known blind spot: the enumeration filters `sqlite_master` to `type='table'`, so an index or trigger
added or dropped by a schema change is NOT independently visible to the gate — declare such changes
in this ledger on the strength of the DDL, not of a gate diff.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | Dumps are identical. |
| 0 | Dumps differ and this ledger declares the current version. A `NOTICE:` line names the entry. |
| 1 | Dumps differ and this ledger does NOT declare the current version. The gate fails. |
| 2 | Usage or environment error (bad flag, missing binary, failed scan). Never a gate verdict. |

## How to declare a change

The version checked is the `version` field of `crates/julie-extract-cli/Cargo.toml` in the current
build. Add one `## <version>` section per version whose extraction output changes, before the
change merges. A section needs a `classification:` line and prose that a consumer can act on.

```md
## 2.31.0

classification: compatible

What changed, which tables and columns move, and what a consumer must do — or why nothing
breaks for a reader on the previous release.
```

The example above is inside a code fence deliberately. `find_ledger_entry` trims each line before
matching, so an *indented* example heading is indistinguishable from a real declaration and would
declare whichever version it names — shadowing the real entry below, because the first match wins.
Fenced blocks are skipped. Keep any future example fenced.

`classification: compatible` means a reader built for the previous release still reads the new
output correctly. `classification: incompatible` means it does not, and the change needs a
schema or contract version bump. The classification recorded here is the contract.

## How to run it

Locally, against any older `julie-extract` binary:

    cargo xtask compat-check --previous-binary /path/to/previous/julie-extract

Useful flags: `--current-binary <path>` (skip building the current release binary),
`--fixture <path>`, `--out-dir <path>` (defaults to `target/compat-check/`, which keeps both
artifacts and both dumps for inspection), `--max-diff-rows <n>`.

In CI, the `Extractor Compatibility` job downloads the latest published release for
`x86_64-unknown-linux-gnu` and runs the same command.

## Declared changes

Every release before 2.30.0 byte-matches its predecessor on the fixture.

## 3.2.0

classification: compatible

Qt support changes the QML, qmldir, and JavaScript rows. The schema does not change, and every
column keeps its type, so a reader built for 3.1.3 still parses the output. The row content moves,
so a consumer that matched on the old shapes must follow the notes below. Consumers that already
hold an artifact must rebuild it, because an unchanged file keeps its stored rows.

`symbols`, QML plain property bindings: a binding such as `width: 100` is no longer a symbol. The
same evidence stays in `structural_facts` as `qml.binding.v1`. The `uses` relationships that pointed
at those rows go with them: on the `plasma-framework` corpus in `docs/languages/qml.md`, resolved QML
relationships fall from 1,112 to 235 and pending rows from 2,360 to 1,682. Consumer action for
code-kb: read bindings from the fact table, not from the symbol table.

`symbols`, QML grouped property blocks: a block such as `anchors { fill: parent }`,
`font { pixelSize: 12 }`, or `border { width: 1 }` parses as an object but binds a group of
properties, so it emits no `field` row, no `instantiates` relationship, no pending row, and no
`type_usage` identifier. A type name whose terminal segment starts with a lowercase letter is the
rule. Its inner bindings stay `qml.binding.v1` facts. Consumer action for code-kb: read a grouped
property block from the fact table, and expect no symbol row named `anchors`, `font`, or `border`.

`symbols`, QML nested objects: a nested object is now a `field` row. Its name is the object's `id`
when it has one, else its type name; its signature is `id: Type` or the bare type; its metadata
carries `object_type`, `binding_kind: "object"`, and `value_source_property` for a `Behavior on`
value source. Consumer action for code-kb: accept `field` as a QML symbol kind in skeletons and
search.

`symbols`, QML nested `id:` bindings: a nested object no longer emits a separate `property` row
for its `id:` binding. The id names the object's own `field` row instead, so the evidence moves
rather than disappears. The file root and an inline component's body still emit their `id:`
property rows, because those objects carry a `class` row that the id cannot name. Consumer action
for code-kb: resolve a nested id against `field` rows, not against `property` rows with an
`id: ` signature.

`symbols`, QML nested-object members: a property, function, signal, or nested object declared
inside a nested object now has that object's `field` row as its `parent_symbol_id`. Before this
release every such member parented to the enclosing `class` row. Consumer action for code-kb:
walk the parent chain to reach the owning component instead of reading `parent_symbol_id` as the
component directly.

`symbols`, QML multi-line property signatures: a `property` row whose value spans more than one
line keeps only the declaration head in `signature`, with the trailing colon removed, instead of
the whole declaration text. A single-line declaration is unchanged. Consumer action for code-kb:
render the stored signature as-is; it is already the one-line form a skeleton needs.

`symbols`, JavaScript `.import` directive rows: an `.import` line at the head of a `.js` file now
emits an `import` symbol. Its name is the imported source, its signature is the directive text, and
its metadata carries `source`, `source_kind` (`uri` or `quoted`), `import_kind`, `alias`,
`local_name`, `imported_name`, `is_namespace` (always true), and `version` for a module import.
Consumer action for code-kb: read JavaScript QML imports from these rows the same way it reads QML
`import` rows.

`symbols`, qmldir type rows: a type row's kind changes from `class` to `export`. Consumer action
for code-kb: query qmldir types by kind `export`.

`symbols`, new QML rows: inline components are `class` rows with the signature `component Name: Base`;
signals carry the signature `signal name(type arg, ...)` and a `parameters` metadata list; a bare
`required property` row carries `required: true`; a `pragma Singleton` file marks its root
`singleton: true`. Consumer action for code-kb: none required, the rows are additions.

`pending_relationships` and `relationships`, new kind: a QML root object emits an `extends`
relationship to its base type, concrete when the base type is a symbol in the same file and a
structured pending row otherwise. Consumer action for code-kb: include `extends` where it reads
QML inheritance.

`relationships` and `pending_relationships`, inline component bodies: the object directly under a
`component Name: Base` header is that inline class's body, so it emits `extends` from the inline
`class` row instead of `instantiates` from the outer class. A qualified base type such as
`QQC2.Button` never resolves to a same-file class; it always takes the structured pending row that
carries its terminal name and receiver. Consumer action for code-kb: read an inline component's
base type from its `extends` row, not from an `instantiates` row on the enclosing component.

`identifiers`, QML type usages: a qualified type name such as `QQC2.Button` is now recorded by its
terminal segment (`Button`) with the qualifier in the new `receiver` metadata key. Before this
release the whole dotted text was the identifier name. Consumer action for code-kb: match the
terminal name and read the qualifier from the metadata.

`identifiers.metadata_json`, new keys: `role` (`base_type`, `attached_type`, `signal_handler`),
`receiver`, and `change_handler` (true for an `onXChanged` handler). A `.qmltypes` root is a module
descriptor, so it records no `base_type` identifier. Consumer action for code-kb: read `role` when
it separates base types from ordinary type usages; the keys are optional and absent rows keep their
old meaning.

`structural_facts`, two new pattern ids: `qml.pragma.v1` (metadata `name`, `value`) and
`javascript.qml_directive.v1` (metadata `directive`, `name`) for a `.pragma` line at the top of a
JavaScript file. Consumer action for code-kb: none required, both are additions registered in
`structural-fact-patterns.json`.

`language_capabilities`, three rows: the `qml` row adds `field` to its symbol kinds, body spans,
annotation exclusions, and doc-comment exclusions, adds `extends` to its relationship kinds, and adds
`qml.pragma.v1` to its structural facts. The `qmldir` row replaces `class` with `export` in the same
four lists. The `javascript` row adds `javascript.qml_directive.v1` to its structural facts. Consumer
action for code-kb: none, the snapshot describes the row changes already listed above.

`language_capability_fixtures`, two new rows: `qml`/`qt_symbols` and `javascript`/`qml_directives`.
Consumer action for code-kb: none, the table lists this repository's own fixtures.

Public extractor API: `Identifier` gains an optional `metadata` field. A crate that builds an
`Identifier` literal must add the field; a crate that reads one is unaffected. Consumer action for
code-kb: none, it reads identifiers from SQLite.

## 3.1.1

classification: compatible

Two tables change: `pending_relationships` and `relationships`. A qualified call chain `Q1.Q2 ... Qn.t(...)` now
records `target_terminal_name = t`, `target_receiver = Qn`, and `target_namespace_json =
[Q1 .. Qn-1]`, with `target_display_name` joined by `.`. Before this release each language chose
its own split: Ruby kept `Net::HTTP` whole in the receiver, C++ kept `->` in the display name, Java,
C++, PowerShell, and Zig dropped the qualifiers or emitted no row, and C# recorded the receiver as
the terminal name. Eighteen languages change (C#, Java, Kotlin, Go, PowerShell, C++, Zig,
JavaScript, TypeScript, Python, Swift, Dart, Lua, GDScript, C, PHP, Ruby, R); Scala and VB.NET already
matched. Rust, Elixir, F#, and Erlang are unchanged: they record a path-style call with every
qualifier in `target_namespace_json` and an empty `target_receiver`. The Java fixture also loses two resolved `relationships`
rows for `fixture.Worker.evaluate` and `fixture.Worker.observeRun`, which now surface as pending
rows with receiver `Worker` and namespace `["fixture"]`.
Four Java fixture outputs also remove duplicate pending rows after the extractor stopped traversing
the same call site repeatedly; no distinct Java relationship is removed.
Ruby `RSpec.describe` pending rows now retain `RSpec.describe` as their terminal and display names
instead of including the enclosing block text.

Nothing in the schema changes and no reader breaks: the same columns carry the same kinds of
values, and a reader built for 3.1.0 reads the new output correctly. A reader that matched
`target_receiver` against a whole chain must match the last qualifier and read the rest from
`target_namespace_json`. Consumers that already hold an artifact must rebuild it rather than
rescan, because an unchanged file keeps its stored rows.

## 3.1.0

classification: compatible

Three tables change. First, `symbols`: a fenced code block in a standalone markdown file no longer
carries the rustdoc test rule. Blocks with no info string, an info string whose first token is
`rust` (such as `rust,no_run` or `rust,should_panic`), or an info string made only of `no_run` and
`compile_fail` previously set `is_test = 1` and `metadata_json.test_role = "test_case"`. They now carry neither, and markdown emits no test roles
at all. Second, `language_capabilities`: the markdown row moves `test_case` out of
`kind_coverage.test_detection.supported` and into `not_applicable`, which is the same change
stated as a capability claim. Third,
`symbol_annotations` and `complexity_metrics`: both tables are written only at `--level full`. A
`symbols` or `facts` scan leaves them empty; a `full` scan of non-markdown source is otherwise
byte-identical to 3.0.0.

Nothing in the schema changes and no reader breaks: a reader that counted markdown code blocks as
tests now sees fewer test symbols, and a reader that needs annotations or complexity metrics scans
at `full`. Consumers that already hold an artifact must rebuild it rather than rescan, because an
unchanged file keeps its stored rows.

## 3.0.0

classification: compatible

One table moves: `language_capabilities`. The `capability_gaps` JSON column carries prose that
named Miller as the consumer responsible for cross-file route-prefix joins (Phoenix, Laravel,
NestJS, and the `.NET` service lanes). That prose now names `code-kb`. No key, kind, or
structural value changes; only the human-readable `reason` and `required_closure` text differs.
Every other compared table is byte-identical to 2.43.0. A reader built for 2.43.0 reads the new
output correctly; nothing to do.

## 2.41.1

classification: compatible

Agent-usefulness extraction repairs add facts without changing tables, columns,
or JSONL fields. Named return types now emit exact `type_usage` identifiers in
all 25 languages with native return-type syntax. ASP.NET minimal APIs cover
`MapHead`, `MapOptions`, literal `MapMethods` verbs, route groups, and handler
metadata. htmx and component attributes preserve normalized template evidence
and explicit uncertainty. Java single-segment package declarations now emit
their namespace symbol.

Artifact schema remains 7, JSONL remains contract v5, and family stores remain
store schema 2. Extraction identity advances from 9 to 10 so unchanged files
are re-extracted instead of reusing epoch-9 rows that predate these facts.

Consumer action: replace the binary and re-extract, or let epoch-10 file
versions populate through the family-store writer. Existing artifacts remain
readable. Existing families preserve their stored reader floor; newly created
2.41.1 families stamp `min_reader_version = 2.41.1`, so consumers adopting the
binary must advertise 2.41.1 reader capability. Do not delete prior file
versions.

## 2.41.0

classification: compatible

Optional Rust host syntax API (`syntax-api` Cargo feature) and public crate-root
export of existing relationship fact types (`PendingSpan`, `UnresolvedTarget`).

Extraction output, database schemas, and CLI contracts remain completely unchanged:
- Artifact schema remains 7.
- JSONL export contract remains v5.
- Extraction identity epoch remains 9.
- SQLite/JSONL fact table outputs and CLI behavior remain 100% byte-identical.
- Default features and dependencies in `Cargo.toml` are unchanged.
- Primary CLI and artifact interfaces remain unchanged.

## 2.39.0

classification: compatible

Receiver-type facts waves 1 and 2 change extraction output for every
general-purpose language (wave 1: csharp, typescript, javascript, python, rust,
go, java; wave 2: the remaining twenty plus wave-1 refinements), and the
`RAZORBACK` marker token widens `code.marker.v1`, without touching any table,
column, or JSONL field. Artifacts stay schema 7, JSONL stays contract v5, and
family stores stay store schema 2. The extraction identity epoch advances from
8 to 9, so family-store import writes fresh epoch-9 file versions instead of
reusing epoch-8 identities that predate these rows.

- New `variable` symbol rows for parameters (`metadata_json.role = "parameter"`)
  and function-local declarations, parented to the enclosing callable.
- New `structural_facts` rows for `RAZORBACK` comments under `code.marker.v1`.
- Wave 1 dropped legacy `types` values that can never match a type symbol
  (whitespace, commas, dangling `<` or `>`) and the TypeScript `unique symbol`
  annotation row; 66 rows left the goldens.
- New `type_facts` rows for declared and same-file inferred types; every
  `resolved_type` is a base type name, with the full declared text in
  `metadata_json.declared` when it differs.
- `receiver_type` keys in `identifiers.metadata_json` and
  `pending_relationships.metadata_json` for self-style call sites.
- Kind changes on existing rows: function-local `val`/`let`/`const`/`final`
  rows become `variable` (kotlin, swift, gdscript, scala, zig, dart, ruby);
  ruby `@x`/`@@x` and razor `@code` fields become `field`; powershell class
  constructors become `constructor`; kotlin and scala primary-constructor
  parameters are `property` rows under the class.
- Span changes on existing rows: dart callable symbols span the whole
  declaration; R6 and RefClass members span their own argument. Containing
  and caller-scope keys inside those bodies move with them.
- Legacy inference rows that carried non-base-name text (`[Foo]`,
  `List<String>`, `void Function()`, `*Worker`, `final`, `inferred`) are gone.
- `language_capabilities` rows change for the nineteen closed `open_gaps`
  entries; lua and r claim `types`.

`EXTRACTION_CONTRACT_VERSION` gains the suffixes `marker-razorback-v1`,
`receiver-type-facts-v1`, and `receiver-type-facts-v2`.

Consumer action: replace the binary and re-extract. An existing artifact keeps
reading unchanged, but an index that merges 2.38.x and 2.39.0 output disagrees
on the kinds and spans above and misses the new fact rows until re-extracted.

## Rust API

Version 2.39.0 narrows the `julie-extractors` crate root to canonical extraction entrypoints, fact row types, enums, capability snapshot types, and language detection utilities:

- **Internal modules made `pub(crate)`:**
  - Infrastructure modules: `base`, `registry`, `pipeline`, `test_detection`, `test_calls`, `utils`, `language`.
  - All 38 language extractor modules: `bash`, `c`, `cpp`, `csharp`, `css`, `dart`, `elixir`, `erlang`, `fsharp`, `gdscript`, `go`, `html`, `java`, `javascript`, `json`, `kotlin`, `lua`, `markdown`, `php`, `powershell`, `python`, `qml`, `qmldir`, `r`, `razor`, `regex`, `ruby`, `rust`, `scala`, `sql`, `swift`, `toml`, `typescript`, `vbnet`, `vue`, `xml`, `yaml`, `zig`.
- **Removed exports from crate root:**
  - `BaseExtractor` (language implementations and base extractor state are crate-internal)
  - `is_test_symbol` (internal test detection logic)
  - `detect_language_from_extension` (internal extension mapping; consumers use `detect_language_for_path` or `detect_language_for_source`)
  - `get_tree_sitter_language` (internal tree-sitter language loader)
  - `LanguageRegistryEntry` (internal registry record; consumers query `supported_languages` or `capability_snapshot`)
- **New crate-root re-exports:**
  - `NormalizedSpan`, `StructuredPendingRelationship`
  - `supported_languages`
  - `classify_literals_by_carrier`
  - `structural_fact_patterns_json`

## 2.38.2

classification: compatible

Extraction identity epoch advances from 7 to 8. v2.38.0 already maps a C#
declaration marked `internal` to visibility `internal` instead of `private`,
but it left the identity epoch at 7. Family-store import reuses a completed
`(path, content_hash, extraction_epoch)` identity, so an unchanged C# file
kept its epoch-7 rows and never rewrote L1.

No table, column, or JSONL field is added, removed, or renamed. Artifact
dumps stay byte-identical to v2.38.1. A reader built for v2.38.1 still reads
schema 7 / JSONL v5 / store schema 2. The capability snapshot is keyed to
epoch 8 so a live store does not collide with the epoch-7 snapshot (39
languages vs 40 after F#).

Consumer action: replace the binary and let epoch-8 family-store file versions
populate on the next import or update. Epoch 7 rows stay immutable. Do not
delete live `file_versions` to force a rewrite.

## 2.38.0

classification: compatible

F# support and three extractor corrections change extraction output without
touching any table, column, or JSONL field. Artifacts stay schema 7, JSONL
stays contract v5, family stores stay store schema 2, and the extraction
identity epoch stays 7.

- New language `fsharp` (`.fs`, `.fsx`, `.fsi`): new rows in every fact table
  for F# sources, and a new `fsharp` row in the grammar inventory, which
  shifts the dump order of every inventory row that sorts after it.
- `language_capabilities.kind_coverage_json` widens with the pattern ids
  `rust.doc_test.v1` and `fsharp.attribute.v1`.
- Rust: `rust.doc_test.v1` structural facts record rustdoc fences in `///`
  and `//!` line comments and in `/** ... */` and `/*! ... */` block doc
  comments — new rows for rust sources that carry doc tests.
- C#: a declaration marked `internal` now reports visibility `internal`
  instead of `private`, so existing C# symbol rows change value on
  re-extract.
- Go: literal `t.Run` subtests emit test rows that did not exist before.

`EXTRACTION_CONTRACT_VERSION` gains the suffixes `csharp-visibility-v2`,
`go-subtests-v1`, `rust-doc-test-facts-v1`, and `fsharp-v1`.

Consumer action: replace the binary and re-extract. An existing artifact keeps
reading unchanged, but an index that merges 2.37.x and 2.38.0 output disagrees
on C# visibility values and misses the new fact rows until re-extracted.

## 2.37.1

classification: compatible

The xml language spec now claims the MSBuild and .NET project XML extensions:
`csproj`, `props`, `targets`, `vbproj`, `fsproj`, `slnx`, `nuspec`, and `resx`.
`sln` stays unclaimed because it is not XML. Files with these extensions were
previously dropped by discovery as `unsupported`; they now parse through the
xml extractor and publish the same data-language facts as `.xml` files, in the
existing schema 7 tables. No table, column, or JSONL field is added, removed,
or renamed. A reader built for v2.37.0 still reads schema 7 / JSONL v5 / store
schema 2.

Extraction identity epoch is 7. Family-store file versions re-extract because
identity is `(path, content_hash, extraction_epoch)`, and paths recorded as
`unsupported` in earlier scans re-enter extraction.

Consumer action: replace the binary and re-extract or rebuild standalone
artifacts, or let epoch-7 family-store file versions populate on the next
import or update. The compat fixture contains none of the new extensions, so
the gate byte-matches; this entry declares the change on the strength of the
spec diff.

## 2.36.0

classification: compatible

Test-role contract expansion: `test_role` string, lifecycle direction, per-language
role corrections.

Every symbol that the shared test-detection writer flags now carries a `test_role`
string in `symbols.metadata_json`, next to the existing `is_test`,
`test_container`, and `test_lifecycle` booleans. The value is one of `test_case`,
`parameterized_test`, `fixture_setup`, `fixture_teardown`, or `test_container`.
One helper writes the booleans and the string together, so the two can never
disagree. The lifecycle arms now report a direction — setup, teardown, ambiguous,
or none — instead of a bare "is a lifecycle hook" answer. A hook that wraps a test
case on both sides (an `around`-style hook) reports `Ambiguous` and takes the
`fixture_setup` role, because a wrapping hook always runs its setup half first.
Later work on this branch corrects per-language role classification for ten
languages; this one entry covers that whole branch.

The typed `symbols.is_test`, `symbols.test_container`, and `symbols.test_lifecycle`
columns keep their current values. No table, column, or JSONL field is added,
removed, or renamed. A reader built for v2.35.1 ignores the new metadata key and
still reads schema 7 / JSONL v5 / store schema 2.

Extraction identity epoch is 6. Family-store file versions re-extract because
identity is `(path, content_hash, extraction_epoch)`.

Consumer action: to read `test_role`, replace the binary and re-extract or rebuild
standalone artifacts, or let epoch-6 family-store file versions populate on the
next import or update. A consumer that reads only the booleans needs no action.

The same branch adds change-journal coverage for unsupported files. A scan now
writes one `files` row per path the discovery walk reached and dropped for an
unsupported extension, with `status = 'unsupported'`, `language = 'unsupported'`,
a content hash, a byte count, and a null `line_count`. Those paths are read once
for the hash and are never parsed, so they add no symbol or other fact rows. The
`files` table therefore gains rows on the compat fixture, and
`revision_file_changes` (already excluded from the gate) gains `unsupported` and
`deleted` entries for them. Ignored paths, hard-excluded paths, oversized source
files, and the artifact's own `-wal`/`-shm`/`-journal` companions stay out.

A store view built from such an artifact carries one manifest entry per
unsupported path, with `status = 'failed'` and `error_class = 'unsupported'` and
no file version — the existing from-artifact mapping, now reachable.

Consumer action: a reader that lists `files` must filter on
`status = 'indexed'` if it wants only parsed files. A reader that already
filters by status, or that reads the change journal, needs no action.

## 2.35.1

classification: compatible

The v2.35.1 release makes QML a first-class extraction family. QML source now
publishes normalized imports, type facts, object-instantiation relationships,
Qt Quick Test roles, and source evidence. `qmldir` files publish module,
component, import, plugin, typeinfo, and related manifest facts. `.qmltypes`
files publish tooling module, type, member, revision, and export evidence.
The existing SQLite and JSONL tables remain schema-compatible; the new rows and
the QML capability snapshot are the declared output change.

Extraction identity epoch is 5. A reader built for v2.35.0 still reads schema
7 / JSONL v5 / store schema 2. Family-store file versions re-extract because
identity is `(path, content_hash, extraction_epoch)`.

Consumer action: replace the binary and re-extract or rebuild standalone
artifacts, or let epoch-5 family-store file versions populate on the next
import or update.

## 2.34.4

classification: compatible

The v2.34.4 release expands `is_test` facts across the supported language
extractors. Test-role closure records 21 supported capability cells and 7
source-backed `not_applicable` entries across C, C++, Rust, Zig, HTML, SQL,
Markdown, JSON, TOML, YAML, and XML. Additional framework, annotation,
naming, and test-lifecycle evidence is emitted where the grammar and language
conventions support it; existing test-role facts remain schema-compatible.
The changed facts are otherwise within the existing schema 7 extraction
tables.

Extraction identity epoch is 4. A reader built for v2.34.3 still reads schema
7 / JSONL v5 / store schema 2. Family-store file versions re-extract because
identity is `(path, content_hash, extraction_epoch)`.

Consumer action: replace the binary and re-extract or rebuild standalone
artifacts, or let epoch-4 family-store file versions populate on the next
import or update.

## 2.34.3

classification: compatible

The v2.34.3 release narrows `is_test` facts. Python decorator evidence is
limited to `pytest.mark.*` and exact `unittest.skip`, `unittest.skipIf`,
`unittest.skipUnless`, and `unittest.expectedFailure`; `pytest.fixture` and
`unittest.mock.*` are not test evidence. Bare `test_` names still require
test-path evidence. Scala and Elixir no longer treat a test path alone as
callable test evidence; their test-name conventions and supported annotations
remain active. The changed facts are otherwise within the existing schema 7
extraction tables.

Extraction identity epoch is 3. A reader built for v2.34.2 still reads schema
7 / JSONL v5 / store schema 2. Family-store file versions re-extract because
identity is `(path, content_hash, extraction_epoch)`.

Consumer action: replace the binary and re-extract or rebuild standalone
artifacts, or let epoch-3 family-store file versions populate on the next
import or update.

## 2.34.2

classification: compatible

QML, GDScript, Bash, and Scala test-role flags are now emitted, and R
`test_lifecycle` is recorded as `not_applicable`. The `language_capabilities`
snapshot JSON changes for those languages. Fact tables on the compat fixture
are otherwise unchanged.

Extraction identity epoch is 2. A reader built for v2.34.1 still reads schema
7 / JSONL v5 / store schema 2. Family-store file versions re-extract because
identity is `(path, content_hash, extraction_epoch)`.

Consumer action: replace the binary. Rebuild standalone artifacts or let the
next extract rewrite them. Family stores write new epoch-2 file versions on
the next import or update.

## NEXT (unreleased)

classification: incompatible

The resolution write path is retired. Schema v7 removes `identifier_resolutions`
and `pending_resolutions`. JSONL v5 drops the overlay keys. `store resolve` is
gone. Family stores stay schema v2 and drop leftover resolution objects on
writer open.

The compat dump excludes the two overlay tables and `language_capability_gaps`
so fact-table identity remains the gate against v2.33.7. Their absence is this
classified break, not an undeclared table drop.

Consumer action: rebuild standalone artifacts. Family stores migrate in place.
code-kb must use query-time resolution before pinning this binary.

See [2026-08-18-resolution-write-path-retirement.md](../decisions/2026-08-18-resolution-write-path-retirement.md).

## 2.30.0

classification: incompatible

Two independent output changes ship in this release. The section-level classification above is the
stronger of the two — the schema v6 identifiers shape. The `metadata_json` canonicalization is
compatible on its own; it is folded into this entry because the gate reports one verdict per
version, and a reader that survives the key reordering still cannot read a v6 artifact.

**1. Canonical `metadata_json` serialization — compatible on its own.**

Every `metadata_json` value is now serialized through `serde_json::Value` at the CLI's single
serialization chokepoint, so keys are emitted in sorted order rather than in extractor insertion
order. The key SET is unchanged and every value is unchanged, so any JSON reader is unaffected.
Only a consumer byte-comparing the stored strings sees a difference, and it sees it once: rows
whose metadata carried 2+ keys in non-canonical order are rewritten in canonical order on the first
scan with this binary, after which the output is already canonical.

Tables affected: every metadata-carrying table. Measured across two scan processes on the
determinism gate's fixture before the fix, 90 of 210 metadata-carrying rows differed — `symbols` 73
of 192 and `structural_facts` 17 of 18 — with zero rows present in only one artifact. Against the
previous release binary on the compat fixture, the difference is confined to `symbols.metadata_json`
key ordering.

Consumer action: none, unless the consumer persists or diffs raw `metadata_json` bytes. Such a
consumer must expect one rewrite of the affected rows and should compare parsed objects rather than
text.

**2. Schema v6 — `identifiers` loses `target_symbol_id` — incompatible.**

The `identifiers` table drops the denormalized `target_symbol_id` column, its
`FOREIGN KEY (target_symbol_id) REFERENCES symbols(symbol_id) ON DELETE SET NULL`, and the
`idx_identifiers_target` index. `identifier_resolutions` becomes the sole source of identifier
resolution outcomes; the resolution store's lockstep writes into the column are deleted.

The gate reports this as the `identifiers` dump's `#columns` header losing a column, which this
entry declares. The dropped `idx_identifiers_target` index is declared here on the strength of the
DDL alone: the gate's enumeration compares tables only, so index drops are not independently
visible to it (see the blind-spot note under "What the gate compares").

Consumer action: **rebuild via a full rescan.** A v6 binary refuses a v5 artifact with exit code 3
and `schema_migration_required`; no migration engine exists. Any consumer SQL selecting
`identifiers.target_symbol_id` must read `identifier_resolutions.target_symbol_id` through a join
instead. This is why the classification is `incompatible`: a reader built for 2.29.0 cannot read a
v6 artifact at all.

Not a difference the gate reports, but worth recording beside the shape change: the JSONL export
contract is **unbumped at 4**. The identifier record keeps its `target_symbol_id` key, now sourced
through a `LEFT JOIN identifier_resolutions`, and is byte-identical to 2.29.0's output.

**Also in this release, and deliberately absent from the diff.** The extraction pass stopped
resolving symbol references across file boundaries (`SymbolLookup` is now per-file). That is a
producer behavior change, but it alters no output on any real corpus — a per-file extractor cannot
mint another file's stable symbol id, and the corpus survey behind the change found 0 cross-file
links over 703k rows. The compat harness re-ran after the narrowing and attributed no extraction-
output difference to it. It is named here so a future reader does not mistake its absence from the
dumps for an omission.
