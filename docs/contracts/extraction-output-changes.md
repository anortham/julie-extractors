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
- `pending_resolutions`, `identifier_resolutions` — the deliberate work surface of the resolution
  tiers, gated by the resolution-contract fixtures rather than by byte equivalence against the
  previous release.
- `files.indexed_at` and `files.last_revision_id` — per-scan columns inside a compared table.

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

    ## 2.31.0

    classification: compatible

    What changed, which tables and columns move, and what a consumer must do — or why nothing
    breaks for a reader on the previous release.

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

No versions are declared. Every release so far byte-matches its predecessor on the fixture.
