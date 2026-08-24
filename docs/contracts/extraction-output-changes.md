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
output correctly. `classification: incompatible` means it does not, and the change needs an epoch
bump once the store's epoch machinery exists. Until then, the classification recorded here is the
contract.

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
Miller must use query-time resolution before pinning this binary.

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
