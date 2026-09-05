# Producer reader-retention qualification evidence

## Status

Producer branch qualification is complete for
`4ca16853ecb054f6989aafa1410381f41273adde`. Linux, Windows, changed-path, contract, crash, format,
clippy, and documentation gates are green. This evidence does not merge the branch, publish a
release, or authorize a Miller pin update.

The producer-retention implementation was reviewed and committed at
`f0d4014d9a4782162e938b669c380ae7ca5c40db`. Commit
`af3bd6857d38098a4e8b363ef4ee807fde9f08a6` adds diagnostic crash-checkpoint exposure only.
`a2fcee2cf8afb92409f292ba24edc3256d1b72e1` corrects only the crash-test recovery lease and adds the
held-reader rollback proof. Task 5 is committed at
`4ff0281d1d7c2d7eefcd2b66a9a1392be5b51191`. The frozen source/test candidate, including both Task 6
targets, advanced through `11de0176517caf40209ff327306471df07249d6c` for a private
fingerprint-helper lint correction and then
`f3f433e3bdbb10402d090ea5bae82615fa908375` for the test-only reader-catalog crash split. The public
API and fixed snapshot vector are unchanged. The final test-only expected-command correction is
`4ca16853ecb054f6989aafa1410381f41273adde`; this is the implementation/test identity for final
gates.

## Executable and fixture identities

| Identity | Kind | SHA-256 or source identity | Qualification |
|---|---|---|---|
| `/home/murphy/source/miller/.tools/julie-extract` | Real released Linux executable | `de5d6d93e353f395950b60fd22f5ee8b2656f5b4d91dea90a730a29857aaf0dc` | `julie-extract 2.39.0`; old writer baseline |
| `target/debug/julie-extract` at capture time | Real locally built Linux executable | `c76b95ddf8c4cb21d912b78410e8c04fa6b425130a09fa4bb51ab226aefa452f` | Reports `2.40.0`; default-feature test build at source `a2fcee2c`, whose CLI production is Task 5 commit `4ff0281d`; not a release artifact |
| `StoreConnectionFactory(..., "2.41.0")` | Deterministic in-process fixture | Current source implementation plus version argument `2.41.0` | Exercises newer-writer compatibility logic; it is not a separately built or released future executable |
| `C:/work/miller/.tools/julie-extract.exe` | Real released Windows executable | `14dbfc49b6f6b4bca2dedc925394f0d39d2dcb1bcb5877e541285f6418db9eae` | Binary version `2.39.0`; Windows old-writer baseline |

The first mixed-version target run used source commit `f0d4014d9a4782162e938b669c380ae7ca5c40db`
plus the new isolated test file. An intermediate exact-row run used `af3bd685` with Task 5 in flight.
After Task 5 committed, the complete target passed again against `4ff0281d` with the required
15-column registration comparison.

## Schema and snapshot digests

The checked-in catalog authority and `store_and_coordinator_catalogs_match_the_checked_in_authority`
agree on these SHA-256 digests:

| Catalog | SHA-256 |
|---|---|
| `store.db` schema catalog | `c3786c3d483dc554c6170efe7b5bb6d97360ca05f2713d1c04ed0f0c8111109c` |
| `coord.db` schema catalog | `6fc7a0a09cc81a623ba1514c0ceece35275896edc707c68efd2ad29e29641176` |

`reader_models_freeze_identity_and_derive_snapshot_facts` verifies the fixed
`julie-reader-snapshot-v1` vector as
`0fac79b573ab9eafc7a1fdd31198da0c51657c13d894b1d8cedb08387fed8450`. Its inputs are family
`family-a`, store instance `family-a:gen-000042`, view `view-a`, manifest generation `42`, manifest
hash `manifest-hash`, generation `gen-000042`, extraction epoch `9`, served sequence `800`, and
minimum retained sequence `700`.

The schema remains store schema version 2. The reader catalog is coordinator-owned. There is no
reader registration table or copied reader-version root table in `store.db`. Reader-capable families
have a permanent writer floor of `2.40.0`.

## Exact coordinator readback

The mixed-version target reads the complete live registration tuple before and after each
maintenance attempt using this parameter-free ordered query over its one-row disposable fixture:

```sql
SELECT pin_id,owner_nonce,view_id,manifest_generation,generation_name,
       owner_pid,owner_birth_identity,heartbeat_at,expires_at,
       store_instance_id,manifest_hash,extraction_identity_epoch,
       served_store_log_sequence,min_retained_store_log_sequence,
       snapshot_fingerprint
FROM reader_registrations ORDER BY pin_id;
```

The test compares typed values without `Debug`, so assertion failures cannot print the nonce or raw
birth identity. Published transcripts replace both with `[redacted]`. The mixed-version fixture row
is inserted with parameterized SQL, uses fixed synthetic identity values, is unexpired, and has a
synthetic snapshot fingerprint. It is maintenance-root evidence, not proof of the admission path.

## Required state transitions

The eight transitions are covered compositionally by focused producer tests. The literal sample IDs
in the plan do not require a duplicate monolithic fixture. The actual Task 5 wire transcript appears
below.

| Step | State change and authoritative assertion | Proof |
|---:|---|---|
| 1 | Acquire commits exactly one registration before returning. Real publication and committed-log fixtures prove the served sequence is the maximum retained committed row, the retained original flip is a lower floor when present, and pruned history falls back to served or zero. The 1k, 10k, and 100k fixtures each insert one registration. The fixed `800/700` vector proves fingerprint framing only. | `acquire_is_idempotent_by_nonce`, `acquire_preserves_retained_original_manifest_floor_below_served_high_water`, `acquire_uses_retained_committed_high_water_after_manifest_log_is_pruned`, `one_registration_roots_each_manifest_size`, `reader_models_freeze_identity_and_derive_snapshot_facts` |
| 2 | Publishing manifest generation 43 after acquiring generation 42 does not retarget the registration. Replay returns the original snapshot, generation 42, and original hash. Renew preserves the same snapshot. Separate physical-generation tests prove a registered non-current generation survives repeated promotion and rollback until release. | `acquire_is_idempotent_by_nonce`, `retired_cleanup_keeps_a_registered_non_current_generation`, `reader_held_historical_generation_survives_rollback_until_release` |
| 3 | Cursor advance and its regression, ahead-of-high-water, and foreign-generation refusals leave the authenticated reader structurally unchanged. | `active_reader_leaves_cursor_monotonic_bounded_and_generation_bound` |
| 4 | GC reports one protected reader, preserves the reader manifest and all reachable L1/L2/L3 versions, includes the reader root reason, and keeps the inclusive reader log floor. | `gc_keeps_registered_manifest_roots`, `gc_retains_the_stricter_cursor_or_live_reader_log_floor`, `newer_version_factory_fixture_marks_and_retains_registration_roots` |
| 5 | View retirement returns `maintenance_busy` while a reader is held. View, manifest, and registration counts remain unchanged. | `retire_view_refuses_a_registered_reader_for_that_view` |
| 6 | An existing row with the wrong nonce returns `ReaderOwnerMismatch`. Inspection and release reveal no stored snapshot, owner, nonce, or birth identity, and the row remains. | `renew_release_and_inspection_require_the_registered_owner` |
| 7 | Correct release removes one row. The second correct release and a later wrong-nonce release on the absent row return successful no-ops. | `renew_release_and_inspection_require_the_registered_owner` |
| 8 | A reader added after planning changes the coordinator fingerprint. Maintenance acquire returns `maintenance_plan_stale`, keeps one reader row, and runs no apply deletion. | `maintenance_acquire_rejects_a_reader_added_after_planning` |

## Actual reader CLI capture

The committed Task 5 target passed 23 tests. The actual reader namespace help is:

```text
Usage: julie-extract store reader <COMMAND>

Commands:
  acquire  Register one immutable manifest snapshot
  renew    Renew an authenticated reader registration
  release  Release an authenticated reader registration
  help     Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

The capture imported `fixtures/extraction/rust/basic` through the public CLI, kept the invoking shell
process alive as owner PID `2259503`, and used the committed Task 5 binary hash shown above. Only the
nonce is redacted. Birth identity is producer-internal and is absent from the wire.

Immediately before capture,
`RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-cli --test store_reader_mixed_version_contract -- --nocapture`
built the default-feature Cargo test binary and passed 2 of 2. Source HEAD was `a2fcee2c`; later
feature-bearing gates may replace `target/debug/julie-extract`, so the recorded hash applies only to
this capture.

```json
{"report_schema_version":1,"operation":"reader_acquire","state":"acquired","family_id":"9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11","view_id":"default","pin_id":"reader-3675c210d67dc24bd456885a04e45c65","generation_name":"gen-001","manifest_generation":1,"owner_nonce":"[redacted]","owner_pid":2259503,"store_instance_id":"9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11:gen-001","manifest_hash":"268e169684662f84763fff31e7d63e0c88db81f73a1427e0ec68247d7a6b5d3d","extraction_identity_epoch":9,"served_store_log_sequence":9,"min_retained_store_log_sequence":3,"snapshot_fingerprint":"b77722ab1fe03f69c932b3de2c8139726f17777cec31cb8e8ad178e21cd6b865","protected_manifest_count":1,"expires_at":1788582046907,"warning":null,"failure_class":null,"error":null}
{"report_schema_version":1,"operation":"reader_renew","state":"renewed","family_id":"9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11","view_id":"default","pin_id":"reader-3675c210d67dc24bd456885a04e45c65","generation_name":"gen-001","manifest_generation":1,"owner_nonce":"[redacted]","owner_pid":2259503,"store_instance_id":"9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11:gen-001","manifest_hash":"268e169684662f84763fff31e7d63e0c88db81f73a1427e0ec68247d7a6b5d3d","extraction_identity_epoch":9,"served_store_log_sequence":9,"min_retained_store_log_sequence":3,"snapshot_fingerprint":"b77722ab1fe03f69c932b3de2c8139726f17777cec31cb8e8ad178e21cd6b865","protected_manifest_count":1,"expires_at":1788582076925,"warning":null,"failure_class":null,"error":null}
{"report_schema_version":1,"operation":"reader_release","state":"released","family_id":"9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11","view_id":null,"pin_id":"reader-3675c210d67dc24bd456885a04e45c65","generation_name":null,"manifest_generation":null,"owner_pid":null,"store_instance_id":null,"manifest_hash":null,"extraction_identity_epoch":null,"served_store_log_sequence":null,"min_retained_store_log_sequence":null,"snapshot_fingerprint":null,"protected_manifest_count":null,"expires_at":null,"released":true,"warning":null,"failure_class":null,"error":null}
```

SQL registration counts were `0` before acquire, `1` after acquire, `1` after renew, and `0` after
release. Acquire and renew preserve the exact manifest, generation, store instance, sequence, and
snapshot fields. Renew changes only liveness time. Release returns no former snapshot or owner
fields. The lead independently recomputed the acquire and renew snapshot fingerprint with Node
SHA-256 using the specified length-prefix and signed-i64 framing; both match the captured
`b77722ab1fe03f69c932b3de2c8139726f17777cec31cb8e8ad178e21cd6b865` value.

## Cursor independence and log retention

`store_reader_cursor_contract` has four tests at commit `f0d4014d9a4782162e938b669c380ae7ca5c40db`.
The final unchanged run passed 4 of 4 in 0.29 seconds of test time.

- Cursor-only advance returns a consumer cursor and leaves `reader_registrations` at 0.
- Cursor release does not release an authenticated reader.
- Reader release does not advance or release a consumer cursor.
- A foreign maintenance intent refuses cursor advance, cursor release, reader renew, and reader
  release. Both rows remain unchanged.
- Reader floor 3 and cursor floor 4 retain log rows `[3, 4]`. Releasing the reader leaves the cursor
  at 4 and permits the next GC to leave `[]`.
- Cursor floor 1 and reader floor 3 prune through the cursor's inclusive acknowledgment and leave
  `[2, 3, 4]`. Releasing the cursor permits the next GC to leave `[3, 4]`.

A cursor call returns no reader admission or registration. Producer retention cannot prevent a
caller from reading arbitrary filesystem paths directly. Miller's M1 integration must enforce that
it opens no family-store read session before successful reader admission.

## Mixed-version maintenance

### Real 2.39.0 writer

Command:

```text
JULIE_EXTRACT_2_39_BIN=/home/murphy/source/miller/.tools/julie-extract \
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-artifact \
  --test store_reader_registration_contract \
  v239_maintenance_refuses_reader_registered_family_before_mutation \
  -- --ignored --exact --nocapture
```

Result: 1 passed, 0 failed, 0 ignored, 33 filtered out, 0.12 seconds test time. The test first
asserted exact version output `julie-extract 2.39.0`. Each real process invocation of `gc --apply`,
`repair --apply`, `promote --apply`, and `retire-view --apply` returned exit 3 with
`failure_class=incompatible_store`.

Linux fixture state before all four commands and after each command:

| Fact | Value |
|---|---|
| `store.db` SHA-256 | `0ae9d28fd5e23834d24c5f7a06b7d0d75b6d30024c4047a0da610f09fdb7ab40` |
| `coord.db` SHA-256 | `542890c63e69c151555c6213ee44f76e8a53b29cd144868f88d60294c664af85` |
| `CURRENT` | `gen-001` |
| `min_writer_version` | `2.40.0` |
| reader registrations | 1 |
| maintenance intents | 0 |
| writer leases | 0 |
| views | 1 |

The Windows old-binary run at commit `2f775cf2a389f9b49ef4c745c1434c5ca42c4c3f` also passed 1 of
1. It preserved store SHA-256
`a6770e7200a08f033693cf8e96a0ec2cda27f610fd2a8f23fc93e4902314a763` and coordinator SHA-256
`68a458445763f29f1cec8529295ef10b897928dfa2252e4760a1c00ab1042994` across all four refusals.

### Current binary and newer factory fixture

`store_reader_mixed_version_contract` passed 2 of 2. The current-binary case launches the real Cargo
test binary, asserts its `2.40.0` version output, runs `store maintain gc --apply --json`, and checks
one protected reader, zero dead readers, zero unknown readers, and zero removed readers. Before and
after SQL facts are registration 1, held manifest 1, held entry 1, held version 1, and current
manifest generation 2. The full 15-column registration tuple is unchanged.

The `2.41.0` case uses the current implementation through `StoreConnectionFactory`. It is not a
future binary. Its plan names `reader-mixed-version` as the L1, L2, and L3 root for version 101.
Apply preserves the same SQL facts and the same full registration tuple.

## Admission work and legacy activation

The actual admission observer recorded the same work at every adversarial manifest size:

| Manifest entries | Registration rows | Store-data statements | VM steps | Full-scan steps |
|---:|---:|---:|---:|---:|
| 1,000 | 1 | 10 | 134 | 0 |
| 10,000 | 1 | 10 | 134 | 0 |
| 100,000 | 1 | 10 | 134 | 0 |

The ten labels are `writer_floor_admission`, `snapshot_data_version_start`, `store_family`,
`extraction_identity_epoch`, `writer_floor_snapshot`, `current_manifest`,
`retained_store_log_high_water`, `original_manifest_flip`, `snapshot_data_version_end`, and
`current_manifest_revalidation`. Their query plans use point or index searches. No executed
statement or plan names `manifest_entries` or `file_versions`. Rusqlite 0.40 exposes VM-step and
full-scan-step counters here. It does not expose scanstatus row-visit counters, so this evidence makes
no rows-visited claim.

Coordinator mutations are reported separately from the ten measured store-data statements. Static
source inspection shows a first successful acquire executes one `INSERT` into
`reader_registrations`; an idempotent nonce replay writes zero rows. A successful renew executes one
authenticated `UPDATE` of `heartbeat_at` and `expires_at`. A successful release executes one
authenticated `DELETE`; an absent-row release writes zero rows. Tests assert row counts of 1 after
acquire, 1 after renew, 0 after first release, and 0 after the idempotent second release. These are
row-mutation counts from source and contract assertions, not observer-measured coordinator statement
or VM-step counts.

GC liveness-probe cost is also separate. Per call to `qualify_reader_owner`, static control-flow
inspection shows an unexpired reader performs zero OS process probes. An expired reader with a valid
stored identity performs one `ProcessIdentityProbe::inspect`. An absent or matching terminated
observation performs one additional identity-domain validation. Maintenance can call qualification
again during reinspection, plan validation, and cleanup preparation, including across bounded
windows. No per-operation probe total or probe timing was instrumented, so neither is reported.

`first_time_floor_activation_allows_bounded_admission` separately proved legacy activation on a real
100,000-entry fixture. Pre-activation admission returned the typed floor-required result. The fenced
activation installed the absent reader catalog and permanent 2.40 floor. The first admission then
used the same 10 statements, 134 VM steps, and 0 full-scan steps. Activation work is not included in
those admission counters.

Pruned committed-log behavior is also explicit. If the original `manifest_flipped` row survives, it
is the minimum reader floor. If receipt pruning removed it, admission uses the maximum retained
committed sequence, or zero when the committed log is empty. Allocator high-water is never reported
as a committed log position.

## Liveness, race, and cleanup matrix

`Protected delete count` means deletion of a registered manifest, entry, version, generation, or
reader row without authorized cleanup. It does not mean GC performed no unrelated maintenance.

| Case | Fixture and final SQL state | Disposition | Protected delete count |
|---|---|---|---:|
| Live unexpired reader | Real store fixture; registration 1, manifest 1, version roots retained | `protected` | 0 |
| Expired heartbeat, matching live process | Deterministic probe and maintenance fixture; registration 1 retained | `retained_alive` | 0 |
| Paused live reader past expiry | Real child process; registration 1 retained | `retained_alive` | 0 |
| Crashed reader with definitive same-instance death | Real child exits; registration becomes 0, report removal count 1 | `definitively_dead` | 0 |
| PID reused with different birth identity | Liveness policy returns unknown. The generic expired-unknown maintenance fixture separately proves SQL registration 1 and all roots retained. No isolated PID-reuse SQL fixture is claimed. | `retained_unknown_identity` with `reader_identity_unknown` | 0 in the generic integrated unknown fixture |
| Missing birth identity, domain mismatch, access denial, or probe error | Policy fixtures return unknown. The generic expired-unknown maintenance fixture proves SQL registration 1 and roots retained. | `retained_unknown_identity` with warning | 0 in the generic integrated unknown fixture |
| Acquire sees an existing maintenance intent | Real coordinator transaction; registration 0 | `busy` | 0 |
| Acquire wins before intent creation | Barrier inside admission; registration 1, competing intent serializes after commit | `acquired` | 0 |
| Acquire races manifest publication or view retirement | Barrier after the WAL snapshot; registration 0 | `stale_snapshot` | 0 |
| Renew or release sees maintenance ownership | `gc_fence_refuses_renewal_and_keeps_the_live_reader_roots` acquires the real maintenance executor first. Renew returns busy. Before and after: registration 1, held manifest 1, held entry 1, held version 1, current manifest generation 2. | `busy` | 0 |
| Renew commits before maintenance ownership | `renewal_commits_before_gc_and_the_renewed_reader_roots_survive` renews first, verifies the stored authenticated row equals the renewed tuple, then plans and applies GC. The same five SQL facts remain unchanged and L1/L2/L3 carry the reader root. | `renewed`, then protected | 0 |
| Renew after GC plan | `renewal_after_planning_makes_the_old_plan_stale_without_root_changes` renews after planning. Maintenance acquire refuses the old fingerprint. Registration 1, held manifest 1, held entry 1, held version 1, and current generation 2 remain. | `stale_plan` | 0 |
| Reader added after GC plan | Registration 1; executor rejects the old fingerprint before apply | `stale_plan` | 0 |
| Generation promoted repeatedly while old physical generation is held | Registration 1; old generation and its manifest remain | `protected` | 0 |
| Rollback with a reader-held historical generation | `reader_held_historical_generation_survives_rollback_until_release` checks the full registration digest, original manifest/version rows, and physical old generation through rollback. Explicit release permits later reclaim. | `protected`, then `released` | 0 |
| View retirement while reader is held | Registration 1, view 1, manifest 1 | `busy` | 0 |
| Reader catalog missing, partial, malformed, or unreadable after enablement | Maintenance refuses; existing roots are not treated as empty | fail closed | 0 |

Torn acquire leaves no row because insertion and admission commit share one coordinator transaction.
Torn release leaves the complete live row. Generation replacement preserves `coord.db` outside the
generation directories. Julie starts no reader renewal process. The caller renews and explicitly
releases; a crash remains protected until a maintenance probe proves definitive process-instance
death.

The three renew-versus-GC tests use the public live-process acquire and renew APIs on Linux and
Windows. Their store fixture seeds committed log sequence and allocator high-water `800` directly
with test SQL. That number checks snapshot and race setup only; production committed-log semantics
come from the real `ManifestStore`, `StoreLog`, reconciliation, and pruning tests listed in step 1.

## Crash matrix

| Boundary | Evidence | Status |
|---|---|---|
| Before reader-root scan | `reader_root_survives_every_pre_delete_crash_boundary` | Linux and Windows pass; row/root retained |
| After reader-root scan | same parent crash test | Linux and Windows pass; row/root retained |
| Before first delete | same parent crash test | Linux and Windows pass; row/root retained |
| Store demotion before and after commit | `store_demotions_are_atomic_on_both_sides_of_the_commit` | Linux pass; standalone final crash gate also passed 11 of 11 |
| Generation publication checkpoints | `every_promotion_boundary_recovers_the_same_generation_without_duplicates` | Linux pass; corrected Windows run at `a2fcee2c` passed 5 of 5 |
| Physical generation deletion lock | `generation_deletion_guard_blocks_takeover_through_physical_delete` | Linux and Windows pass; competing `BEGIN IMMEDIATE` remains busy through directory deletion |

The Windows generation-crash run at `f0d4014d` failed because the parent could not open the expected
`gen-002/store.db`. Diagnostic commit `af3bd685` proved the child exited 101 with
`Maintenance(MaintenanceFenceLost)` before the checkpoint. That first diagnostic exposed a distinct
100ms child-start lease problem. A later `5286ffb6` run reached the exact markers but passed 3 and
failed 2 because parent recovery operations exceeded their 5-second fixture leases. Commit
`a2fcee2c` raised only those recovery leases to 60 seconds. The full corrected
Windows target passed 5 of 5 in 56.34 seconds with a 0.93 second build. Production fencing did not
change.

## Report bounds and privacy

`maintenance_reports_a_bounded_sanitized_reader_summary` creates 25 retained-unknown reader rows.
The JSON report contains 20 warnings, reports `omitted_warning_count: 5`, and reports counts 25
protected, 0 definitively dead, 25 retained unknown, and 0 removed. Each warning contains only a
`pin_id` and `reader_identity_unknown`. JSON and human output contain neither nonce nor raw birth
identity.

Operator outcomes are distinct:

- `released`: authenticated explicit release removed one row, or the row was already absent.
- `definitively_dead`: maintenance proved same-process-instance death and committed cleanup.
- `retained_unknown_identity`: process identity could not authorize cleanup, so the row and all roots
  remain.

The Task 5 reader help, one-line report contracts, and actual redacted lifecycle transcript are
recorded above. The initial Windows target passed 21 behavior/privacy tests and failed only two help
expectations that used `julie-extract` instead of the correct Windows name `julie-extract.exe`.
The focused corrected help filter passed 2 of 2 at `8219d218`, so Windows CLI qualification is 23
covered cases without rerunning the unchanged 21.

## Platform results

| Platform and commit | Scope | Result |
|---|---|---|
| Linux `3e417ab5` | Liveness integration | 14 passed, 1 explicit helper ignored |
| Linux `5f7d8684` | Private liveness platform tests | 9 passed |
| Windows `3e417ab5` | Liveness integration | 14 passed, 1 explicit helper ignored; 0.01s tests, 5.47s build |
| Windows `5f7d8684` | Private Windows liveness tests | 4 passed |
| Windows `3e417ab5` | Legacy reader-catalog crash/recovery | 8 passed, 1 explicit child ignored; 5.90s tests, 7.11s build |
| Windows `b980d315` | Reader admission contract | 30 passed, 1 explicit old-binary test ignored; 52.32s tests, 7.43s build |
| Windows `f0d4014d` | Generation equivalence | 15 passed, 1 explicit child ignored; 23.41s |
| Windows `f0d4014d` | Maintenance contract | 38 passed, 1 explicit child ignored; 19.40s. The pre-existing Unix-only maintenance-owner case is not compiled on Windows |
| Windows `f0d4014d` | Cursor and reader contract | 4 passed; 4.25s |
| Windows `f0d4014d` | Deletion guard and debug privacy units | 2 passed; 0.69s tests, 5.75s build |
| Windows `f0d4014d` | Reader pre-delete crash boundaries | 1 passed; 6.29s tests, 7.77s build |
| Windows `af3bd685` | Future store/coordinator schema refusal | 2 passed; 0.96s tests, 1.22s build |
| Windows `af3bd685` | Permanent writer floor above compiled producer | 1 passed; 0.49s tests, 0.08s build |
| Windows `a2fcee2c` | Full generation crash matrix | 5 passed, 0 failed, 0 ignored; 56.34s tests, 0.93s build |
| Windows `a2fcee2c` | Held-reader rollback | 1 passed; 3.96s tests, 1.02s build |
| Windows `a2fcee2c`, correction `8219d218` | Reader CLI | Initial 21 behavior/privacy cases passed and 2 help-name cases failed; corrected focused help filter passed 2 of 2 in 0.03s with a 2.26s build |
| Windows `8219d218` | Renew versus GC | 3 passed; 2.72s tests, 0.93s build |
| Windows `8219d218` | Mixed-version current binary and newer factory | 2 passed; 1.46s tests, 1.16s build |
| Windows `f3f433e3` | Isolated reader-catalog crash recovery | 8 passed, 1 explicit child ignored; 8.27s tests, 2.49s build; exact crash marker observed |
| Windows `f3f433e3` | Fixed snapshot fingerprint vector | 1 passed; 0.00s test time, 2.39s build |
| Windows `f3f433e3` | Definitive-death reader helper | 1 passed; 1.25s tests, 1.24s build |
| Other unsupported platforms | Deterministic policy path | `UnsupportedPlatform` retains the reader with `reader_identity_unknown`; no real-platform run claimed |

## Verification ledger

| Scope | Command or immutable prior record | Source identity | Result |
|---|---|---|---|
| Task 6 mixed-version initial | `cargo test -p julie-extract-cli --test store_reader_mixed_version_contract -- --nocapture` | `f0d4014d` plus new test only | 2 passed, 0 failed; 0.06s test time |
| Task 6 exact registration readback | same command after Task 5 committed | `4ff0281d` plus new test | 2 passed, 0 failed; 0.07s test time |
| Real Linux old writer | exact ignored command above with required environment variable | `f0d4014d` plus new test only; external binary SHA above | 1 passed, 0 failed; four pre-mutation refusals |
| Schema catalog authority | `cargo test -p julie-extract-artifact --test store_schema_contract store_and_coordinator_catalogs_match_the_checked_in_authority -- --exact --nocapture` | schema unchanged from `f0d4014d` | 1 passed, 0 failed; 0.01s test time |
| Snapshot digest | `cargo test -p julie-extract-artifact --test store_reader_registration_contract reader_models_freeze_identity_and_derive_snapshot_facts -- --exact --nocapture` | reader model unchanged from `b980d315` | 1 passed, 0 failed; 0.00s test time |
| Renew versus GC serialization | `cargo test -p julie-extract-artifact --test store_reader_renew_gc_contract -- --nocapture` | committed in `8219d218` | Initial compile exposed a test-only `Debug` bound; first behavioral run exposed invalid zero-clock setup; final public live-process target passed 3, failed 0 in 0.12s |
| Cursor/reader independence | immutable `task-6-cursor-tests-report.md` final run | committed in `f0d4014d` | 4 passed, 0 failed; not rerun on an unchanged target |
| Linux Task 4 maintenance | immutable Task 4 report | committed in `f0d4014d` | 39 passed, 1 explicit helper ignored |
| Linux Task 4 generation | immutable Task 4 report | committed in `f0d4014d` | 15 passed, 1 explicit helper ignored |
| Linux Task 4 CLI maintenance | immutable Task 4 report | committed in `f0d4014d` | 19 passed |
| Windows main retention batch | lead verification record | `f0d4014d` | Generation, maintenance, and cursor targets passed as listed above |
| Windows generation crash | lead verification record | `a2fcee2c` | 5 passed, 0 failed, 0 ignored |
| Task 5 reader CLI | `store_reader_cli_contract` plus actual help/transcript capture | `4ff0281d`; Windows correction in `8219d218` | Linux 23 passed and transcript captured; Windows 21 unchanged behavior/privacy cases plus corrected help 2 passed |
| Windows Task 6 targets | focused `store_reader_renew_gc_contract` and `store_reader_mixed_version_contract` runs | `8219d218` | renew/GC 3 passed; mixed-version 2 passed |
| Lead changed-path gate, including default and certification | `git diff --name-only -z a87121c6 HEAD \| xargs -0 cargo xtask test changed` | `8219d218` | Failed: xargs exit 123, cargo exit 101. Extractor 3933 passed and 7 ignored plus 1 doc test; preceding artifact targets including reader 33, cursor 4, and renew/GC 3 passed. `test_tiers::default_suite_tests_assert_no_wall_clock_budget` flagged timing code in `store_reader_catalog_contract.rs`. CLI/default continuation and certification did not run. |
| Certification resumed after changed-gate abort | `cargo xtask test certification` | `8219d218` | passed: capability 39, pending-shape 1, parser-upgrade 2; reported test times 1.04s, 0.17s, 0.00s and rebuild times 32.28s, 31.89s |
| Linux CLI default scope resumed after changed-gate abort | `cargo test -p julie-extract-cli` | `8219d218` | passed; visible summaries include unit 162, store CLI 25, reader CLI 23, mixed-version 2, and all other CLI targets green |
| Changed-path gate after catalog split | same exact changed-path command | `f3f433e3` | Extractor, artifact, and CLI default stages passed, including extractor 3933 with 7 ignored, artifact guard 8, reader 33, renew/GC 3, and reader CLI 23. The run then failed in `xtask/tests/test_tiers` because its expected command vector omitted the new catalog crash target. Commands and production behavior were unchanged; `4ca16853` corrected this expected vector and the final run passed. |
| Xtask tier-plan correction | lead focused xtask tier-test record | `4ca16853` | 23 passed; runtime and tier commands unchanged from `f3f433e3` |
| Branch contract gate | `cargo xtask test contract` | started at `f3f433e3`; only the 14-line xtask expected-vector and memory data changed in `4ca16853`, with production and tier commands identical | passed, exit 0. Visible summaries: golden 7, capability 39, pending 1, downstream 1, maintenance 39 plus helper ignore, generation 16 plus helper ignore, maintenance crash 7, generation crash 5, serial store crash 11, catalog crash 8 plus child ignore, deep model-free CLI 1, references 3, store equivalence 7, mixed 6, import 39, operations 31, maintenance equivalence 2 in 91.54s, maintenance mixed 1, maintenance performance 3 |
| Final changed-path gate | `git diff --name-only -z a87121c6 HEAD \| xargs -0 cargo xtask test changed` | `4ca16853ecb054f6989aafa1410381f41273adde` | passed, exit 0; includes all three default packages, xtask 8 unit plus 23 tier tests, and certification 39/1/2 |
| Branch crash gate | `cargo test -p julie-extract-artifact --features test-store-crash --test store_crash_contract` | `4ca16853` | 11 passed; 0.49s tests, 0.03s build; separate from the serial 11-case run inside contract |
| Format | `cargo fmt --check` | `4ca16853` | passed |
| Clippy | `cargo clippy --workspace --all-targets` | `4ca16853` | passed, exit 0; zero warnings; 3.30s |
| Documentation | `cargo doc --workspace --no-deps` | `4ca16853` | passed, exit 0; zero warnings; 5.88s |

## Qualification disposition

- Task 6 and J1 have no remaining technical blocker on the qualified candidate.
- Earlier Linux and Windows failures remain in the ledger as corrected history, not current failures.
- Unsupported platforms have deterministic fail-closed policy coverage only; no real unsupported
  platform run is claimed.
- The branch is not merged, released, or pinned. Those actions retain their normal approval and
  release gates.
