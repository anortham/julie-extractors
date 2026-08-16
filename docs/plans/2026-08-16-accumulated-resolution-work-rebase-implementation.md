# Accumulated Resolution Work Rebase Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Compact the store base-plus-delta overlay after a broad multi-transition scoped resolve while preserving existing scope admission, public contracts, and exact output.

**Architecture:** Store scope admission keeps its current inclusive 70% duplicated-read rule and separately derives a strict >25% unique-identifier signal guarded by `validated_transition_count > 1`. The crate-private decision travels through `StoreResolutionDecisionTelemetry`; `resolve_claimed` always evaluates the artifact crate's existing validated rebase decision before ORing the ephemeral CLI signal and entering the existing rebase path.

**Tech Stack:** Rust, rusqlite/SQLite, `julie-extract-cli`, `julie-extract-artifact`, Cargo integration tests.

**Architecture Quality:** Medium risk. The change stays inside the store CLI decision/session/orchestration path, leaves public Rust APIs and durable artifacts unchanged, and reuses the existing fenced atomic rebase lifecycle.

## Global Constraints

- Preserve `scope_crosses_over` duplicated identifier-query reads and its inclusive `f64 >= 70%` decision byte-for-byte, including the zero-identifier file/version fallback.
- The new trigger is scoped mode, `validated_transition_count > 1`, `total_unique_identifiers > 0`, and `unique_selected_identifiers * 4 > total_unique_identifiers` using widened integer arithmetic.
- Unique identifiers are current-manifest `(version_id, identifier_id)` keys admitted by the selected-version, name, or receiver arms; duplicate query-arm reads count once.
- One-transition, zero-identifier, full, and fallback decisions carry `rebase_after_exact=false`.
- `rebase_after_exact` remains crate-private and must not enter `ResolutionWorklists`, `ResolutionExactPublish`, `ResolutionExecutionTelemetry`, durable payloads, reports, SQLite schema, CLI arguments, Store Contract v1, Miller APIs, or legacy `resolution::delta_scope_crosses_over`.
- The artifact rebase check must run and succeed before the CLI folds in `rebase_after_exact`, preserving stale proof, malformed telemetry, and fence-loss error ordering.
- Rebase must use the existing ready-base materialization, pinning, cleanup, and fenced atomic publication path on Linux, macOS, and Windows.
- Follow strict TDD: write each behavior test, observe the expected failure, implement the minimum change, then rerun the same worker scope green.

---

## Verification Strategy

**Project source of truth:** `AGENTS.md`, `.github/workflows/ci.yml`, and the approved design `docs/plans/2026-08-16-accumulated-resolution-work-rebase-design.md`.

**Worker red/green scope:** Task 1 uses `cargo test -p julie-extract-cli --test store_delta_scope_contract` and `cargo check -p julie-extract-cli --features test-store-resolution-contract`. Task 2 uses `cargo test -p julie-extract-cli --features test-store-resolution-contract --test store_resolution_contract` and `cargo test -p julie-extract-cli --features test-store-resolution-contract --test store_resolution_scope_equivalence`. Task 3 uses `JULIE_ACCUMULATED_REBASE_PERF_OUT_DIR="$PWD/target/accumulated-resolution-work-rebase-perf" JULIE_ACCUMULATED_REBASE_PERF_RUNS=3 cargo test -p julie-extract-cli --features test-store-resolution-contract --test store_resolution_performance accumulated_resolution_work_rebase_performance_gate -- --exact --nocapture --test-threads=1`.

**Worker ceiling:** Workers may run only their owned integration-test targets and `cargo fmt --check`; they do not run xtask tiers, workspace Clippy, or unrelated crates.

**Worker gate invariant:** Task 1 proves the two cost models retain distinct units and boundaries and that only a scoped multi-transition decision carries the private bit. Task 2 proves validation ordering, contract non-leakage, base rotation, empty delta, crash/replay behavior, and exact-output equality. Task 3 proves the reproducible 79-transition shape, one-time rebase, bounded following update, exact digest, and recorded timings/RSS.

**Lead affected-change scope:** `cargo test -p julie-extract-cli --test store_delta_scope_contract`; `cargo check -p julie-extract-cli --features test-store-resolution-contract`; `cargo test -p julie-extract-cli --features test-store-resolution-contract --test store_resolution_contract`; `cargo test -p julie-extract-cli --features test-store-resolution-contract --test store_resolution_scope_equivalence`; the Task 3 accumulated-work performance command with `--nocapture --test-threads=1`; `cargo fmt --check`; `git diff --check`.

**Branch gate:** `cargo fmt --check`; `cargo clippy --workspace --all-targets --all-features`; `cargo test -p xtask`; `cargo xtask test default`; `cargo xtask test contract`; `git diff --check`.

**Security scope:** none declared.

**Replay/metric evidence:** Strict threshold boundaries, transition count, changed base id, empty delta, scope retirement, exact digest, and zero semantic differences are hard gates. Automated wall time and peak RSS are report-only; the same-machine live dogfood gate requires three warm following-update samples with p95 at most 2 seconds and records the before number.

**Escalation triggers:** Any public/durable contract change, artifact rebase API change, legacy-path change, non-identical exact digest, Windows file-rotation failure, or following-update p95 above 2 seconds requires lead review and broader specialist verification before completion.

**Assigned verification failure:** Workers stop and report when assigned verification fails, unless this plan explicitly says to update that gate.

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, and timestamp. For replay or metric evidence, also record hard-gate metrics and report-only metrics. If the same HEAD already has a passing ledger entry for the required scope, reuse that evidence instead of rerunning the same expensive gate.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: Compute and carry accumulated scope policy | None - serial | `crates/julie-extract-cli/src/store/delta_scope.rs`; `crates/julie-extract-cli/src/store/resolution_session.rs`; `crates/julie-extract-cli/tests/store_delta_scope_contract.rs` | Yes | Task 2 consumes the crate-private decision shape and telemetry field produced here. |
| Task 2: Fold the policy into existing rebase publication | None - serial | `crates/julie-extract-cli/src/store/resolve.rs`; `crates/julie-extract-cli/tests/store_resolution_contract.rs`; `crates/julie-extract-cli/tests/store_resolution_scope_equivalence.rs` | Yes | Requires Task 1's compiled decision telemetry and must establish lifecycle correctness before performance evidence. |
| Task 3: Add reproducible accumulated-work performance evidence | None - serial | `crates/julie-extract-cli/tests/store_resolution_performance.rs` | Yes | The fixture asserts the production behavior and publication invariants implemented by Tasks 1 and 2. |

### Task 1: Compute and carry accumulated scope policy

**Files:**
- Modify: `crates/julie-extract-cli/src/store/delta_scope.rs:19-254,530-591,709-718`
- Modify: `crates/julie-extract-cli/src/store/resolution_session.rs:406-410,4109-4207`
- Test: `crates/julie-extract-cli/tests/store_delta_scope_contract.rs:20-430,901-1148`

**Interfaces:**
- Consumes: `StoreDeltaScopeRequest`, validated resolution-scope batches, `ResolutionWorklists`, and the current `StoreDeltaScopeDecision`/`scope_crosses_over` behavior.
- Produces: a crate-private scoped decision carrying `worklists` plus `rebase_after_exact: bool`, and `StoreResolutionDecisionTelemetry.rebase_after_exact` for Task 2. Public `ResolutionWorklists` remains unchanged.

**Contract inputs:** The approved unique-key definition, strict 25% integer boundary, multi-transition guard, unchanged inclusive 70% duplicated-read rule, and false value for full/fallback/one-transition/zero-identifier paths.

**File ownership:** `crates/julie-extract-cli/src/store/delta_scope.rs`; `crates/julie-extract-cli/src/store/resolution_session.rs`; `crates/julie-extract-cli/tests/store_delta_scope_contract.rs`

**Serialization required:** Yes.

**Dependency reason:** Task 2 consumes the crate-private decision shape and telemetry field produced here.

**What to build:** Return both validated changes and the number of validated transition batches from the journal walk. Preserve the existing duplicated-read crossover helper unchanged, add a separate unique current-manifest identifier-set calculation for the three admitted arms, and set the bit only for still-scoped multi-transition work strictly above one quarter.

**Approach:** Keep cost models named and structurally separate so the 70% rule cannot accidentally switch units. Carry the bit through the crate-private `StoreDeltaScopeDecision::Scoped` shape into `StoreResolutionDecisionTelemetry`; explicitly assign false on requested-full, full-crossover, invalid-scope fallback, prior-overlay fallback, one-transition, and zero-identifier paths.

**Acceptance criteria:**
- [ ] Existing 70% and empty-identifier crossover tests pass without changed expectations.
- [ ] Multi-transition unique coverage at exactly one quarter carries false; one unique key over carries true.
- [ ] Duplicate selected-version/name/receiver hits count once for the 25% rule but retain duplicate cost for 70% admission.
- [ ] One transition and zero identifiers carry false even when their existing admission fallback selects scoped work.
- [ ] Full and fallback scope decisions carry false; the compiled store session carries the computed bit on crate-private telemetry without modifying `ResolutionWorklists`.
- [ ] Worker-scope verification passes and the worker commits the owned files using `serial-worker-commit`.

### Task 2: Fold the policy into existing rebase publication

**Files:**
- Modify: `crates/julie-extract-cli/src/store/resolve.rs:47-111,303-819`
- Test: `crates/julie-extract-cli/tests/store_resolution_contract.rs:1330-2235,3246-3340`
- Test: `crates/julie-extract-cli/tests/store_resolution_scope_equivalence.rs`

**Interfaces:**
- Consumes: `StoreResolutionDecisionTelemetry.rebase_after_exact` from Task 1 and `ResolutionBindingStore::exact_rebase_required_with_proof` unchanged.
- Produces: `artifact_rebase_required || decision.rebase_after_exact` after successful artifact validation, routed into the existing `materialize_exact_for_rebase`/`prepare_rebased_base`/publication sequence.

**Contract inputs:** No public artifact method signature, durable telemetry field, Store Contract field, schema column, CLI option, or Miller change is permitted. Existing validation must execute even when the new bit is true.

**File ownership:** `crates/julie-extract-cli/src/store/resolve.rs`; `crates/julie-extract-cli/tests/store_resolution_contract.rs`; `crates/julie-extract-cli/tests/store_resolution_scope_equivalence.rs`

**Serialization required:** Yes.

**Dependency reason:** Requires Task 1's compiled decision telemetry and must establish lifecycle correctness before performance evidence.

**What to build:** Read the private bit from the exact session decision, always evaluate `exact_rebase_required_with_proof`, then combine the booleans without short-circuiting validation. Reuse the current rebase pin, materialization, publication, terminal reporting, retry, and cleanup code exactly as the durable triggers do.

**Approach:** Add no field to `ResolutionExecutionTelemetry`; assert its generated durable JSON contains exactly the existing keys and omits `rebase_after_exact`. Assert requested-full, full-crossover, invalid-scope, and prior-overlay fallback session telemetry carry false while scoped session telemetry carries Task 1's value. Build a multi-transition integration fixture whose replacement/gap metrics stay below existing thresholds so the test proves the new trigger alone rotates `resolution_base_id`, publishes an empty delta, retires scope, and preserves exact output. Exercise stale-proof/malformed-telemetry/fence-loss ordering through existing seams rather than adding a new public test hook.

**Acceptance criteria:**
- [ ] The artifact rebase check is evaluated before the private bit and existing error classifications/order remain unchanged.
- [ ] The new bit alone changes the base id, publishes an empty delta, and emits only existing rebase report values.
- [ ] Generated durable resolution telemetry omits `rebase_after_exact` and retains its existing field set.
- [ ] Ordinary exact and rebased exact publication both retire scope state/batches.
- [ ] Full/fallback and exact-quarter paths retain existing non-rebase behavior; existing replacement and 64 MiB gap threshold tests remain green.
- [ ] Scoped and full oracles have identical identifiers, pending rows, tombstones, manifest hash, resolver epoch, and canonical exact digest.
- [ ] Crash/replay cases converge to one ready base and one empty delta without changing idempotent terminal behavior.
- [ ] Worker-scope verification passes and the worker commits the owned files using `serial-worker-commit`.

### Task 3: Add reproducible accumulated-work performance evidence

**Files:**
- Modify/Test: `crates/julie-extract-cli/tests/store_resolution_performance.rs:1-3450`

**Interfaces:**
- Consumes: the existing `store_resolution_performance_gate`, worker subprocess, `Sample`, fixture builders, canonical semantic digest, peak-RSS parser, and Task 2's published rebase behavior.
- Produces: `accumulated_resolution_work_rebase_performance_gate`, an environment-driven deterministic performance mode that emits per-run JSON and summary p95 for the broad resolve and following one-file updates.

**Contract inputs:** Exactly 79 sequential validated transition batches, final batch changing one file, unique coverage in the strict 25–70% band, at least three warm following-update samples, exact digest equality, base rotation, empty delta, and no second accumulated-work rebase on the one-transition update.

**File ownership:** `crates/julie-extract-cli/tests/store_resolution_performance.rs`

**Serialization required:** Yes.

**Dependency reason:** The fixture asserts the production behavior and publication invariants implemented by Tasks 1 and 2.

**What to build:** Extend the existing environment-driven performance gate rather than adding a product CLI flag. Generate 79 sequential store transitions, run the crossing resolve once, then apply and measure repeated one-file updates through the existing worker subprocess and JSON sample path.

**Approach:** Make transition count, unique-coverage numerator/denominator, resolution mode, base/delta identities, rebase count, exact digest, phase timings, wall time, and peak RSS visible in fixture output. Assert structural and exactness invariants in the test; calculate and print p95 from three or more warm samples while leaving wall-clock/RSS assertions report-only in automation.

**Acceptance criteria:**
- [ ] The fixture proves 79 sequential validated transitions rather than one multi-file transition.
- [ ] Unique coverage is strictly above 25% and below the unchanged 70% full crossover.
- [ ] The broad resolve rebases exactly once to a changed base id with an empty delta and exact digest equal to the full oracle.
- [ ] Each following update contains one validated transition and does not trigger another accumulated-work rebase.
- [ ] Per-run JSON records phase timings, wall time, peak RSS, scope counts, base/delta state, and digest; summary output reports warm p95 and the before value.
- [ ] The automated test treats timing/RSS as report-only; live same-machine dogfood records three warm samples and requires following-update p95 at most 2 seconds.
- [ ] Worker-scope verification passes and the worker commits the owned file using `serial-worker-commit`.
