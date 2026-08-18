> **Superseded 2026-08-18.** The resolution write path is retired. See [2026-08-18-resolution-write-path-retirement.md](../decisions/2026-08-18-resolution-write-path-retirement.md).

# Accumulated Resolution Work Rebase Design

**Status:** Approved 2026-08-16; implementation plan pending approval.

**Owner:** `julie-extractors` / `julie-extract`

## Problem

A scoped store resolve can inherit a broad transition chain even when the newest
request changes one file. In the Miller dogfood incident, 79 accumulated paths
expanded through names, aliases, receivers, and module repoints to 227,249
reported scoped rows. The resolve took 100.3 seconds.

The result was correct. The rebase policy made the cost repeatable:

- `scope_crosses_over` promotes scoped work to full resolution only at 70% of
  corpus identifier cost.
- `exact_rebase_required_inner` rebases only when replacement and tombstone rows
  are strictly greater than 25% of bound-base semantic rows, or cumulative exact
  gap JSON is greater than 64 MiB.
- Exact publication retires the processed scope journal, so the following request
  does not inherit the same 79-path chain. It does retain the published
  base-plus-delta overlay unless the existing replacement or gap trigger rebases.
- Scope admission computes a duplicated identifier-read estimate for the 70%
  full crossover. The reported `scope_row_count` is a different unit, and neither
  value currently carries an accumulated-work compaction decision forward.

The incident's `227,249 / 416,361` ratio is only a scale signal. The numerator
includes identifier, pending, and relationship rows, while the 70% crossover
counts identifier query-arm reads and deliberately includes duplicates. It must
not be used as the new threshold's proof.

## Goals

- Compact the base-plus-delta overlay after a genuinely accumulated, broad scoped
  resolve, without making a single high-fanout edit rotate the base repeatedly.
- Measure the new trigger in unique current-manifest identifier keys, separate
  from the existing duplicated-read estimate and report row count.
- Preserve exact resolution, atomic publication, fencing, crash recovery, and
  current Store Contract v1 output.
- Keep the policy inside `julie-extractors`; Miller remains a process-contract
  consumer and does not acquire store-write or resolver policy.
- Prove the policy with operation counts and exact-output equivalence, then
  report wall-clock before and after on the same workload and machine.

## Non-goals

- Making a genuinely unresolved 79-file backlog require zero work.
- Replacing scoped resolution with unconditional full resolution.
- Optimizing phase SQL or prior-overlay materialization before profiling shows
  which phase owns the remaining time.
- Adding an MCP tool, Store Contract field, SQLite schema field, CLI flag, or
  environment variable.
- Changing Miller timeouts, freshness semantics, or background indexing.

## Decision

Extend the existing CLI rebase decision with an accumulated scoped-work trigger:

```text
accumulated_scope = validated_transition_count > 1
unique_scope_cost = unique_selected_identifiers / total_unique_identifiers

scoped_work_rebase =
    resolution_mode == scoped
    and accumulated_scope
    and total_unique_identifiers > 0
    and unique_selected_identifiers * 4 > total_unique_identifiers

rebase_required =
    existing_replacement_trigger
    or existing_cumulative_gap_trigger
    or scoped_work_rebase
```

The unique key is `(version_id, identifier_id)` in the current manifest. The
selected set is the union of identifiers admitted through selected-version,
name, and receiver arms; duplicate query-arm reads count once. The comparison is
strict: exactly one quarter does not rebase; one unique key over one quarter does.
Arithmetic uses widened integers.

The existing `scope_crosses_over` behavior remains byte-for-byte compatible: its
duplicated-read numerator, `f64 >= 70%` predicate, and zero-identifier
file/version fallback do not change. The new unique-key calculation is separate.
A still-scoped decision with more than one validated transition and strictly more
than 25% unique coverage marks `rebase_after_exact`. A zero-identifier store and
a one-transition scope always carry `false` for the new trigger.

The marker travels only on crate-private `StoreResolutionDecisionTelemetry` and
the CLI's in-process result. It never enters `ResolutionExecutionTelemetry`, its
`durable_payload`, the Store Contract report, or the family-store schema. Public
`ResolutionWorklists`, `ResolutionExactPublish`, and artifact rebase method
signatures remain unchanged. Full and fallback decisions carry `false`.

Full-mode resolution does not use the new scoped-work trigger. Existing full
behavior and both existing rebase triggers remain unchanged.

The CLI always calls the artifact's existing rebase check first so current
publication validation and error ordering run unchanged, then ORs the validated
result with `rebase_after_exact`. When true, the existing rebase path materializes
the exact result as a new ready base and atomically publishes it with an empty
delta. There is no second publication algorithm or background file lifecycle.

## Why One Quarter

The repository already treats changes above one quarter of base semantic rows as
large enough to justify rebase write amplification. The new trigger uses the same
strict ratio, but not the same units: it measures unique corpus coverage and also
requires multiple validated transitions so one high-fanout edit cannot cause
repeated base rotation.

The incident fixture must prove that its unique coverage crosses the new boundary;
the reported 54.6% ratio is not substituted for that measurement. Full resolution
remains a correctness fallback and is not assumed to be faster.

## Data Flow

1. Manifest publication records immutable resolution-scope transition batches.
2. `build_store_delta_scope` validates and reconstructs the chain, preserving its
   validated transition count.
3. The existing duplicated-read estimate retains the inclusive 70% full
   crossover. Separately, a still-scoped multi-transition decision with strictly
   more than 25% unique identifier coverage carries `rebase_after_exact=true` on
   crate-private decision telemetry.
4. Resolution produces and validates the exact scratch result.
5. The CLI invokes the existing artifact rebase check, which validates ready-base
   identity, proof, and semantic counts exactly as it does today.
6. Only after that call succeeds does the CLI fold `rebase_after_exact` beside
   the existing replacement and cumulative-gap result.
7. A triggered rebase uses `materialize_exact_for_rebase` and
   `prepare_rebased_base`, then publishes through the existing fenced atomic
   rebase path.
8. Scope retirement still occurs on every winning exact publication. The
   triggered path additionally makes the following import resolve against the
   new base with no prior delta to copy forward.

## Interface Shape

The caller-facing interfaces remain:

- `julie-extract store import|resolve` and Store Contract v1 reports.
- The versioned family-store manifest, view, base, and delta contracts.
- Miller's existing `StoreWorkspaceCoordinator` import/resolve orchestration and
  status projection.

The CLI's store scope builder owns transition count and unique-coverage policy.
The artifact crate retains its existing public API and owns durable replacement,
gap, publication-validation, and rebase-publication behavior. The CLI combines
the two successful decisions before entering the existing rebase path.

No new public type or adapter is justified. `ResolutionWorklists` remains
unchanged; the new bit belongs to the existing crate-private store decision
telemetry, not a serialized or public execution type.

## Error and Recovery Behavior

- Full resolution and scope fallbacks carry `rebase_after_exact=false` and use
  the existing replacement and gap triggers.
- Malformed execution telemetry retains the existing invalid-publication error
  behavior; the new policy does not weaken telemetry validation.
- Stale base/proof, malformed telemetry, and fence loss fail in the existing
  order because the artifact check is always evaluated before the CLI-only bit.
- A rebase materialization or publication failure leaves the previously
  published exact base-plus-delta view intact.
- Fencing, heartbeat, pin, retry, and cleanup behavior remain on the existing
  rebase path.
- Replayed requests retain their current idempotent terminal behavior.
- Windows handle closure and rename behavior must remain covered because rebase
  rotates a live base file.

## Architecture Quality

**Affected modules:**

- `crates/julie-extract-artifact/src/store/resolution.rs` retains the current
  durable rebase decision and publication validation without public API changes.
- `crates/julie-extract-cli/src/store/delta_scope.rs` preserves the existing 70%
  admission calculation and adds the separate unique 25% accumulated-work rule.
- `crates/julie-extract-cli/src/store/resolution_session.rs` and
  `crates/julie-extract-cli/src/store/resolve.rs` carry the internal decision to
  the existing rebase path.
- Store resolution binding, scope-equivalence, report, and performance tests
  prove behavior through the store resolve interface.

**Caller-facing interface:** Existing CLI, Store Contract v1, and family-store
artifacts are unchanged.

**Depth/locality check:** The CLI keeps the new ephemeral policy beside store
scope admission and combines it only after the artifact's durable check succeeds.
Miller and other process consumers learn nothing about scope telemetry or
thresholds.

**Test surface:** Tests submit store resolution work and assert the published
base/delta identity, exact rows, report mode/counts, and next-request overlay.
Private arithmetic boundaries and store publication are both required test
surfaces because the two policies intentionally use different units.

**Seams/adapters:** No new adapter. Crate-private store decision telemetry carries
one additional bit; the CLI-to-artifact public boundary stays unchanged.

**Rejected shortcuts:** Raising timeouts; lowering the 70% crossover without a
full-vs-scoped measurement; always rebasing; adding an operator knob; moving the
policy into Miller; changing report or schema contracts; optimizing SQL without
phase evidence.

**Legacy boundary:** This design changes only store-mode
`store::delta_scope::scope_crosses_over`. Legacy
`resolution::delta_scope_crosses_over`, including its single-file exemption,
remains unchanged.

**Architecture risk:** Medium. The change is local, but it exercises atomic base
rotation, publication fencing, and Windows file-lifecycle behavior.

## Alternatives Rejected

### Force full resolution earlier

Changing `scope_crosses_over` to promote the incident workload would name the
work as full but would not prove it is faster. Full resolution may do more work
and does not compact future deltas by itself more safely than the existing
rebase path.

### Rebase every exact publication

This bounds overlay age but converts ordinary one-file edits into repeated base
materialization and file rotation. It discards the intended write-amplification
tradeoff.

### Optimize scoped SQL first

The current evidence proves admitted work is too broad, not which SQL phase
dominates it. Processor changes require a phase profile and identical workload
before and after; they are a separate follow-up only if rebasing does not keep
subsequent requests within the target.

## Verification

### Deterministic contract tests

- Exactly one-quarter of unique selected identifiers across multiple validated
  transitions does not trigger rebase.
- One unique identifier over one quarter across multiple transitions triggers
  rebase.
- One transition does not trigger the new rule even when name/receiver expansion
  exceeds one quarter.
- A zero-identifier or pending-only store does not trigger the new rule.
- The inclusive 70% duplicated-read crossover remains unchanged.
- Full-mode telemetry does not trigger the new scoped-work rule.
- Full and fallback scope decisions preserve the existing policy.
- Malformed telemetry, stale base/proof, and fence-loss cases preserve their
  existing errors when `rebase_after_exact=true`.
- Generated durable telemetry contains exactly the existing resolution fields
  and omits `rebase_after_exact`, proving the bit did not leak into serialization.
- Existing strict replacement and 64 MiB gap threshold tests remain unchanged.
- Triggered publication changes `resolution_base_id`, publishes an empty delta,
  and preserves manifest hash, resolver epoch, exact identifiers, pending rows,
  tombstones, and exact digest.
- A multi-transition broad scoped resolve rebases. Scope-journal retirement is
  asserted separately for both ordinary exact and rebased exact publication.
- Scoped output and a full-resolution oracle produce identical exact digests.

### Performance evidence

Use one fixed realistic workload with the same repository snapshot and machine:

1. Add a deterministic fixture that publishes 79 sequential transition batches,
   with the final batch changing one file, and calibrate its unique identifier
   coverage into the strict 25–70% band. Do not approximate it with one
   98-file transition.
2. Record resolution mode, file/name/row counts, phase timings, base identity,
   total wall time, and peak RSS.
3. Run the threshold-crossing resolve.
4. Apply one additional single-file change and record the same fields.
5. Repeat the post-rebase single-file update three warm times and report p95.

The performance gate is:

- The broad resolve rebases once.
- The broad resolve changes base identity and leaves an empty delta.
- The following single-file update has one validated transition and does not
  trigger another accumulated-work rebase.
- Its exact digest matches the full oracle.
- Its same-machine warm p95 is at most 2 seconds.

Wall-clock is report-only in automated tests. Add a reproducible fixture runner
that prints each sample and computes p95 from the three warm post-rebase runs.
The durable regression guards are validated transition count, unique identifier
coverage, empty-delta/base-rotation assertions, and exact-output equality.

If the post-rebase request misses the 2-second target, profile the phase timings
and open a separate scoped-processor design. Do not weaken this design's
correctness criteria or add speculative SQL changes.

## Focused Verification Commands

Implementation planning must confirm the exact cargo feature flags, then run the
narrowest targets covering:

- `store_resolution_binding_contract`
- `store_delta_scope_contract`
- `store_resolution_scope_equivalence`
- `resolution_report_scope`
- `store_resolution_performance`

The branch gate must include the repository's normal default suite and its
explicit slow/store integration tier, with Windows coverage for base rotation.

Miller requires no code change. After rebuilding Miller against the local
`julie-extract` binary, rerun Miller's focused store round-trip/rebase Scale
tests and the live 79-to-1 dogfood workload.

## Acceptance Criteria

- [ ] The existing replacement and cumulative-gap triggers retain their exact
      strict boundaries.
- [ ] Multi-transition scoped work strictly above one quarter of unique current-
      manifest identifiers requires rebase.
- [ ] Exactly one quarter does not rebase.
- [ ] One-transition and zero-identifier scopes do not trigger the new rule.
- [ ] The existing inclusive 70% duplicated-read crossover is unchanged.
- [ ] Full-mode and fallback-scope behavior remain compatible.
- [ ] No CLI, JSON report, SQLite schema, Store Contract, MCP, or Miller API
      changes are introduced.
- [ ] Rebase publication remains fenced, atomic, idempotent, and recoverable on
      Linux, macOS, and Windows.
- [ ] The broad-chain fixture rebases to a changed base id with an empty delta;
      scope retirement remains correct with and without rebase.
- [ ] Scoped and full exact digests match, including pending tombstones.
- [ ] Same-machine warm p95 for the post-rebase one-file update is at most 2
      seconds, reported with the before number.
- [ ] If the target is missed, phase profiling—not an unmeasured optimization—
      determines the next design.
