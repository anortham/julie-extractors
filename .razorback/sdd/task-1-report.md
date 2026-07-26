# Task 1 report: producer reference and artifact contract

## Result

- Worktree: `/Users/murphy/source/julie-extractors/.worktrees/cross-repo-dogfood-repair`
- Branch: `codex/cross-repo-dogfood-repair`
- Base: `06d8de6d`
- Status: complete

## Architecture gate

- Affected boundaries: extractor relationship constructors and normalization, extraction result models, artifact model/schema/writer, JSONL export, capability snapshots, release and dogfood contract guards.
- Caller interface: `ExtractionResult` carries producer-attested site evidence into `ArtifactFile`, which serializes the same canonical site identity to SQLite and JSONL.
- Locality decision: exactness is attested only at the provider that owns the target token. Broad provider nodes produce explicit spanless, non-exact sites.
- Risk: high because this intentionally breaks the SQLite, extracted-data, and JSONL contracts.
- Rejected: consumer-side overlap/name/line/nearest-token inference, compatibility aliases, version ranges, and separate assertion tables.

## Implemented

- Bumped SQLite schema to 5 and extracted-data/JSONL contracts to 4.
- Added canonical `reference_sites` with exact identity from `(file_id,start_byte,end_byte)`, producer provenance, strict exact/spanless invariants, foreign keys, and collision rejection.
- Carried site IDs through identifier, relationship, pending, resolution, SQLite, JSONL, reports, and row counts.
- Grouped resolved assertions by `(site,target_symbol,canonical_kind)` and unresolved assertions by `(site,target_name,canonical_kind)` without adding assertion tables.
- Removed post-hoc site inference. C, Bash, PowerShell, and audited Python call paths now attest target-token spans; unaudited broad paths remain non-exact.
- Typed capability-gap status as `open | exception`, rejected unknown values, and retained the certified 70-open-gap invariant: 36 tier 3 and 34 tier 2.
- Added authoritative normalized SQLite catalog fingerprint and current contract documentation.

## RED evidence

- Schema tests expected version 5 but observed 4 and lacked `reference_sites`.
- Writer tests showed missing evidence foreign keys, accepted conflicting physical identities, and accepted non-exact sites with spans.
- JSONL tests lacked reference-site records and site foreign keys.
- CLI operations failed the new status constraint while snapshots still emitted `open_gaps`.
- Resolution tests exposed line/name-based inference and same-line token collapse.
- The first full default gate exposed receiver extraction limited to `member_access`; the fix reads only the immediate source prefix for `call | member_access` while retaining producer-owned attestation.

## GREEN evidence

- `cargo +1.96.0 fmt --all -- --check`
- `cargo +1.96.0 xtask test default`
- `cargo +1.96.0 test -p xtask --tests`
- `git diff --check`
- Final default-gate resolution result: 16 passed, 0 failed.
- Final xtask result: 15 passed, 0 failed.

## Remaining concerns

- Providers not explicitly migrated to target-token constructors intentionally emit spanless, non-exact sites. Their language/provider gaps remain visible in the capability snapshot instead of being hidden by consumer inference.
- No blocker remains for the Miller and Eros consumer tasks.
