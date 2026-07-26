# Task 1 report: producer reference and artifact contract

## Result

- Worktree: `/Users/murphy/source/julie-extractors/.worktrees/cross-repo-dogfood-repair`
- Branch: `codex/cross-repo-dogfood-repair`
- Base: `06d8de6d`
- Task commit: `cc501177efce1ff48e4daf62b14ab0f6ffe8b228`
- Status: complete after lead-review correction

## Architecture gate

- Affected boundaries: extractor relationship constructors and normalization, extraction result models, artifact model/schema/writer, JSONL export, capability snapshots, release and dogfood contract guards.
- Caller interface: `ExtractionResult` carries producer-attested site evidence into `ArtifactFile`, which serializes the same canonical site identity to SQLite and JSONL.
- Locality decision: exactness is attested only at the provider that owns the target token. Broad provider nodes produce explicit spanless, non-exact sites.
- Risk: high because this intentionally breaks the SQLite, extracted-data, and JSONL contracts.
- Rejected: consumer-side overlap/name/line/nearest-token inference, compatibility aliases, version ranges, and separate assertion tables.

## Miller evidence

- `workspace open path=/Users/murphy/source/julie-extractors/.worktrees/cross-repo-dogfood-repair` resolved workspace `2f441104f02d09df8d78551fd259a48dd17571c83f9541a241c0643bac64855b`; refresh converged it through revision 31.
- `impact target=Relationship max_depth=3 limit=200` reached 319 symbols and truncated at the requested 200-row cap. It identified direct provider consumers across C#, VB.NET, Java, JavaScript, TypeScript, Kotlin, Swift, Zig, Scala, PowerShell, R, Ruby, PHP, C, and other languages.
- `inspect target=Relationship depth=full scope=crates/julie-extractors/src/base/types.rs` proved the public serialized struct shape and identified `create_relationship`, `create_relationship_at_target`, `ExtractionResults.relationships`, and provider extraction paths as callers.
- `trace target=Relationship mode=refs limit=400` returned exact type-use references, including the two base constructors and C/Bash provider paths; the first bounded page contained 15 exact references and no fallback references.
- Before the correction, `inspect target=Relationship.attest_target_token_site depth=full` showed the method writing `reference_site_provenance=target_token` into `metadata`; `inspect target=Relationship.has_attested_target_token_site depth=full` showed exactness reading that user metadata.
- Before the correction, `inspect target=map_relationships depth=full` showed the CLI filtering spans through `has_attested_target_token_site` and serializing the same `metadata` through `optional_json` into `metadata_json`.
- Post-edit `impact git=true max_depth=2 limit=200` reached 338 symbols across 38 seeded files. The broad fan-out is expected for a public record field; the behavioral seam remains localized to the base constructors and `map_relationships`.

## API-shape evidence

- `Relationship` now has `#[serde(default)] pub reference_site_is_exact: bool`, matching `StructuredPendingRelationship`.
- `BaseExtractor::create_relationship` sets the field to `false`; `create_relationship_at_target` is the only relationship producer that changes it to `true`.
- Every direct `Relationship` literal explicitly sets the field to `false`, so broad providers cannot become exact by merely carrying a span.
- `map_relationships` uses the typed field when deciding whether a span is a canonical target-token site.
- No production source contains `reference_site_provenance`, `attest_target_token_site`, or `has_attested_target_token_site`; the former key remains only in the forgery regression input.

## Implemented

- Bumped SQLite schema to 5 and extracted-data/JSONL contracts to 4.
- Added canonical `reference_sites` with exact identity from `(file_id,start_byte,end_byte)`, producer provenance, strict exact/spanless invariants, foreign keys, and collision rejection.
- Carried site IDs through identifier, relationship, pending, resolution, SQLite, JSONL, reports, and row counts.
- Grouped resolved assertions by `(site,target_symbol,canonical_kind)` and unresolved assertions by `(site,target_name,canonical_kind)` without adding assertion tables.
- Removed post-hoc site inference. C, Bash, PowerShell, and audited Python call paths now attest target-token spans; unaudited broad paths remain non-exact.
- Typed capability-gap status as `open | exception`, rejected unknown values, and retained the certified 70-open-gap invariant: 36 tier 3 and 34 tier 2.
- Added authoritative normalized SQLite catalog fingerprint and current contract documentation.
- Replaced the review-found metadata attestation marker with the typed, serde-defaulted `Relationship.reference_site_is_exact` field.

## Gate invariants

- Exact relationship sites require both a producer-attested typed field and a target-token span.
- A broad relationship remains non-exact even if it has a diagnostic span or user metadata containing the former marker.
- Deserializing a relationship without the new field defaults to non-exact; metadata never changes that result.
- Exact constructors do not add structural data to user metadata, and an exact C scan stores `relationships.metadata_json` as `NULL` when the provider supplied no metadata.
- Identifier, relationship, and pending evidence for one attested token share the site derived from `(file_id,start_byte,end_byte)`.
- Same-line distinct tokens have distinct sites, while same-site distinct targets and canonical kinds remain distinct assertions.
- Capability-gap status remains closed to `open | exception`, with exactly 70 certified open gaps: 36 tier 3 and 34 tier 2.
- Schema, extracted-data, and JSONL versions remain exactly 5/4/4; old artifacts do not enter a compatibility path.

## Judgment calls

- Used the existing boolean shape rather than adding a relationship-only enum because exactness currently has two states and `StructuredPendingRelationship` already establishes the contract.
- Kept the field public because `Relationship` is a public data record directly constructed by language providers; hiding it would add a shallow builder seam without reducing caller obligations.
- Kept broad-provider spans for diagnostics while making their canonical sites spanless in the artifact. Span presence alone is not producer attestation.
- Preserved arbitrary user metadata as user data, but removed all structural meaning from it. The exact constructor no longer writes an internal marker.
- Updated all direct provider literals mechanically to `false`; audited exact providers continue through `create_relationship_at_target`.

## RED evidence

- Schema tests expected version 5 but observed 4 and lacked `reference_sites`.
- Writer tests showed missing evidence foreign keys, accepted conflicting physical identities, and accepted non-exact sites with spans.
- JSONL tests lacked reference-site records and site foreign keys.
- CLI operations failed the new status constraint while snapshots still emitted `open_gaps`.
- Resolution tests exposed line/name-based inference and same-line token collapse.
- The first full default gate exposed receiver extraction limited to `member_access`; the fix reads only the immediate source prefix for `call | member_access` while retaining producer-owned attestation.
- Lead review then exposed that the first attestation implementation used forgeable user metadata and leaked it into `metadata_json`. The follow-up RED tests failed because the typed field did not exist and the exact C artifact contained non-null marker metadata.

## GREEN evidence

- `cargo +1.96.0 fmt --all -- --check`
- `cargo +1.96.0 test -p julie-extractors relationship_ids_do_not_collide_for_multiple_calls_on_one_line` — 1 passed.
- `cargo +1.96.0 test -p julie-extractors broad_relationship_nodes_are_not_attested_as_exact_target_tokens` — 1 passed.
- `cargo +1.96.0 test -p julie-extract-artifact --tests` — all artifact test binaries passed; the final writer contract binary reported 38 passed and writer performance reported 2 passed.
- `cargo +1.96.0 test -p julie-extract-cli --test operations_contract` — 53 passed.
- `cargo +1.96.0 test -p julie-extract-cli --test resolution_contract` — 16 passed.
- `cargo +1.96.0 xtask test default`
- `cargo +1.96.0 test -p xtask --tests`
- `git diff --check`
- Final default-gate resolution result: 16 passed, 0 failed.
- Final xtask result: 15 passed, 0 failed.

## Self-review

- Complexity stays local: providers set one typed field, and the artifact mapper consumes it once.
- The caller-facing interface is smaller than the behavior it unlocks: callers provide a boolean attestation rather than understanding site hashing, provenance, or artifact normalization.
- Tests use the same interfaces as callers: base constructors, serde, and a real CLI scan into SQLite.
- No new seam or adapter was introduced.
- The change adds no speculative states or compatibility layer.
- The structural cause is fixed: user metadata can no longer control canonical identity, rather than merely filtering the old marker during export.

## Final pre-commit state

- Path: `/Users/murphy/source/julie-extractors/.worktrees/cross-repo-dogfood-repair`
- Branch: `codex/cross-repo-dogfood-repair`
- HEAD before the review-fix commit: `cc501177efce1ff48e4daf62b14ab0f6ffe8b228`
- Dirty state: only Task 1 review-fix source/tests, this report, and `.memories/2026-07-26/214321_5f29.md`; no unrelated changes.

## Integrated contract-gate follow-up

- Integrated HEAD: `f1e0c9561f2e0de47b770a672920110724c46de7`, including the marker contract.
- RED: `cargo +1.96.0 xtask test contract` failed `tests::golden::golden_fixtures_match_canonical_extraction` at `c:basic`.
- Exact mismatch: expected `helper(...)` and `worker_log(...)` call-expression spans; actual extraction correctly emitted the narrower `helper` and `worker_log` target-token spans. The same approved change affected audited Bash, PowerShell, and Python relationships and pending calls.
- Root cause: producer exactness changed, but canonical golden expectations were not regenerated. `NormalizedRelationship` also omitted `reference_site_is_exact`, so the golden contract did not directly assert the new typed field.
- Fix: added `reference_site_is_exact` to `NormalizedRelationship`, mapped it from `Relationship`, and regenerated through `UPDATE_GOLDEN=1 cargo +1.96.0 test -p julie-extractors --features test-golden --lib golden`.
- Authority update: 101 expected fixture files now explicitly cover 224 relationship rows: 14 audited exact rows and 210 non-exact rows. Existing-value changes are limited to the approved target-token span narrowing; all other fixture changes add the typed boolean.
- GREEN: `cargo +1.96.0 xtask test contract` passed all contract tiers: golden 3/3, capability matrix 39/39, pending-shape contract 1/1, downstream smoke 1/1, artifact schema 15/15, reports 8/8, JSONL 9/9, CLI contract 10/10, path policy 5/5, and operations 53/53.
- GREEN: `cargo +1.96.0 xtask test default` passed, including 3,020 extractor tests with 7 ignored, artifact writer 38/38, writer performance 2/2, CLI operations 53/53, and resolution 16/16.
- GREEN: `cargo +1.96.0 fmt --all -- --check` and `git diff --check`.
- Final follow-up state before commit: path `/Users/murphy/source/julie-extractors/.worktrees/cross-repo-dogfood-repair`, branch `codex/cross-repo-dogfood-repair`, HEAD `f1e0c9561f2e0de47b770a672920110724c46de7`; dirty only the golden contract repair, this report, and `.memories/2026-07-26/215306_2654.md`.

## Version-boundary follow-up

- Starting HEAD: `66e1f74a5247a9110ef4b062cdc47575a9572939`.
- Finding: all three shipped crates still claimed released version `2.17.0` after SQLite schema 5, extract contract 4, and JSONL 4 made the producer incompatible with the published schema-4/contract-3/JSONL-3 release.
- Architecture quality: no module or runtime behavior impact. This is a release-identity correction across existing manifests, lockfile records, versioned notes, and repository release guards.
- Miller `workspace list filter=cross-repo-dogfood-repair` identified the Julie task workspace as `2f441104f02d09df8d78551fd259a48dd17571c83f9541a241c0643bac64855b`; refresh confirmed revision 35.
- Miller `content search query=2.17.0 content_kind=config` returned exactly three live package manifests: `julie-extract-artifact`, `julie-extract-cli`, and `julie-extractors`.
- Miller all-text/docs evidence separated those live claims from the real `v2.17.0` release notes, release evidence, prior work reports, and `docs/release.md` current-published statement. Historical and published facts remain unchanged.
- Miller source search for `CARGO_PKG_VERSION` proved the CLI writes package identity into artifact metadata and reports. `inspect preflight_release_from_root` proved release preflight requires exact manifest equality and a versioned `docs/release-notes/v{version}.md` input.
- RED: `cargo +1.96.0 xtask release preflight --version 2.18.0` rejected `julie-extract-artifact/Cargo.toml` at `2.17.0`.
- Fix: bumped all three crate manifests and all three lockfile package records to `2.18.0`; added `docs/release-notes/v2.18.0.md`; added it to the release-notes index.
- Boundary decision: source identity is `2.18.0`; `v2.17.0` remains the current published release until a separate approval-gated release. No alias, version range, tag, push, publication, or release mutation was added.
- GREEN: `cargo +1.96.0 check --workspace` built all three shipped crates as `2.18.0`.
- GREEN: `cargo +1.96.0 run -p julie-extract-cli --bin julie-extract -- --version` reported `julie-extract 2.18.0`.
- GREEN: `cargo +1.96.0 xtask release preflight --version 2.18.0` validated 4 targets and 26 inputs.
- GREEN: `cargo +1.96.0 test -p xtask --test release_contract` passed 14/14.
- GREEN: `cargo +1.96.0 xtask test default` passed, including 3,020 extractor tests with 7 ignored, artifact writer 38/38, operations 53/53, and resolution 16/16.
- GREEN: `cargo +1.96.0 xtask test contract` passed all tiers: golden 3/3, capability matrix 39/39, pending shape 1/1, downstream smoke 1/1, artifact schema 15/15, reports 8/8, JSONL 9/9, CLI contract 10/10, path policy 5/5, and operations 53/53.
- GREEN: `cargo +1.96.0 fmt --all -- --check` and `git diff --check`.
- Final pre-commit state: path `/Users/murphy/source/julie-extractors/.worktrees/cross-repo-dogfood-repair`, branch `codex/cross-repo-dogfood-repair`, HEAD `66e1f74a5247a9110ef4b062cdc47575a9572939`; dirty only the version slice, this report, and `.memories/2026-07-26/220704_a625.md`.

## Remaining concerns

- Providers not explicitly migrated to target-token constructors intentionally emit spanless, non-exact sites. Their language/provider gaps remain visible in the capability snapshot instead of being hidden by consumer inference.
- No blocker remains for the Miller and Eros consumer tasks.
