# Test Evidence Capability Contract Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Publish an honest, versioned capability dimension describing which test roles are golden-fixture-proven for each supported language, without adding continuous-testing runtime behavior to julie-extractors.

**Architecture:** Extend the existing `kind_coverage` object with `test_detection`, using the fixed units `test_case`, `test_container`, and `test_lifecycle`. The current `symbols.is_test`, `test_container`, and `test_lifecycle` fields remain the only emitted roles and the only classification authority. The capability matrix, artifact `language_capabilities.kind_coverage_json`, JSONL, and `languages --json` carry the additive data; existing file status and parse diagnostics define when consumers must treat absence as unknown.

**Tech Stack:** Rust, serde/serde_json, SQLite artifact contract, JSONL/CLI contracts, Node.js data-quality report, golden fixtures.

**Architecture Quality:** Affected modules are capability snapshot deserialization, CLI snapshot projection, the capability matrix/report, artifact JSON/JSONL serialization, and contract docs. The caller-facing interface is the existing `kind_coverage` JSON object, which is a deep additive seam already consumed across SQLite, JSONL, and CLI. Tests exercise those public artifacts. Rejected shortcuts: new top-level boolean columns, a second test classifier in Miller, inferring runner inventory, claiming support from unit tests alone, or adding watchers/runners/scheduling here. Architecture risk: medium because capability claims can be mistaken for exhaustive runner coverage.

## Global Constraints

- This repository continues to own only `source tree -> versioned extraction artifact`.
- Do not add an MCP server, daemon, watcher, scheduler, runner command, test result, duration, flakiness, dashboard, or editing behavior.
- `symbols.is_test`, `symbols.test_container`, and `symbols.test_lifecycle` remain first-class positive facts; absence is not runner-authoritative inventory.
- Add exactly one `kind_coverage` domain named `test_detection` with fixed units `test_case`, `test_container`, and `test_lifecycle`.
- A unit may be listed under `supported` only when a registered golden fixture emits the corresponding role.
- Unit tests for `is_test_symbol` are implementation evidence, not capability-claim evidence.
- Missing golden proof is an `open_gaps` row with reason, required closure, and planned closure task `docs/plans/2026-07-09-test-detection-golden-closure-implementation-plan.md`.
- Use `not_applicable` only after source verification establishes that the language genuinely lacks the role; uncertainty remains an open gap.
- This first contract slice adds `test_detection` to silent-cell validation but does not yet add it to the strict general-purpose-language quality expectation. The closure plan performs that promotion after goldens exist.
- `files.status=failed_preserved`, relevant `parse_diagnostics`, unsupported files, and missing/partial capability evidence prevent negative claims. Document that rule explicitly.
- Capability data is additive inside `kind_coverage_json`; do not bump the SQLite schema or extraction contract merely for the new object member.
- After capability changes, `node scripts/language-data-quality-report.mjs --strict` must report `silent_cells: 0` and `quality_bar_debts: 0`.
- Execution uses @razorback:test-driven-development for behavior changes and @razorback:verification-before-completion before each commit/handoff.
- No release, tag, push, Miller pin, or Eros runtime change is part of this plan.

---

## Public Contract

Each language row contains:

```json
"test_detection": {
  "supported": ["test_case"],
  "not_applicable": [],
  "open_gaps": [
    {
      "kind": "test_container",
      "reason": "No registered golden fixture currently emits test_container for this language.",
      "required_closure": "Add a language-native golden fixture that emits a test container and a negative non-test control.",
      "planned_closure_task": "docs/plans/2026-07-09-test-detection-golden-closure-implementation-plan.md"
    }
  ]
}
```

Consumer rule:

```text
positive test role = usable evidence
missing role + supported capability + clean relevant files = no matching extracted role, not no runner test
failed_preserved / parse diagnostic / unsupported / open gap = unknown
no combination of these facts proves semantic test-impact completeness
```

## Verification Strategy

**Project source of truth:** `AGENTS.md`, `fixtures/extraction/capabilities.json`, `docs/contracts/sqlite-schema-v4.md`, `docs/contracts/jsonl-v3.md`, and the `cargo xtask test` tiers.

**Worker red/green scope:** Run focused capability snapshot/matrix/artifact contract tests named per task. Follow TDD for deserialization, vocabulary validation, and public serialization.

**Worker ceiling:** Focused crate tests plus `cargo xtask test capability`. Workers do not run real-world corpora or certification.

**Worker gate invariant:** Snapshot tests prove typed round-trip; matrix/report tests prove every language classifies all three fixed units honestly; artifact/CLI tests prove downstream-visible additive data.

**Lead affected-change scope:** `cargo xtask test capability`, `cargo xtask test contract`, and `node scripts/language-data-quality-report.mjs --strict`.

**Branch gate:** `cargo xtask test default`, `cargo xtask test capability`, `cargo xtask test contract`, and the strict language data-quality report. Run `scripts/check-agent-doc-sync.sh` only if agent guidelines change; this plan does not require such a change.

**Replay/metric evidence:** Hard gates are all language rows present, fixed vocabulary only, no unsupported claim without golden evidence, exact SQLite/JSONL/CLI round-trip, and zero silent/debt report counts. Counts of supported/open units by language are report-only.

**Escalation triggers:** Any emitted symbol-role behavior change, schema column, contract version bump, or language parser change invokes the default + language-specific golden/contract gates and may require the closure plan instead.

**Assigned verification failure:** Workers stop and report when assigned verification fails, unless this plan explicitly says to update that gate.

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, and timestamp. Include strict report counts and the per-language support/open-gap summary.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: Add the typed capability domain | Batch A | `crates/julie-extractors/src/capability_snapshot.rs`, `crates/julie-extract-cli/src/capability_snapshot.rs`, `capability_snapshot_test.rs`, `cli_contract.rs` | No | None - safe parallel batch. |
| Task 2: Classify every language and extend quality reporting | Batch A | `fixtures/extraction/capabilities.json`, `language-data-quality-report.mjs`, `capability_matrix.rs` | No | None - safe parallel batch. |
| Task 3: Round-trip the public artifact and document consumer rules | None - serial | `writer_contract.rs`, `jsonl_contract.rs`, `schema_contract.rs`, `operations_contract.rs`, `sqlite-schema-v4.md`, `jsonl-v3.md`, `test-evidence-v1.md`, `continuous-testing-evidence-boundary.md` | Yes | Requires Tasks 1-2 final data shape. |

Batch A uses `parallel-lead-commit`. Task 3 uses `serial-worker-commit` after the branch gate.

### Task 1: Add the typed capability domain

**Files:**
- Modify: `crates/julie-extractors/src/capability_snapshot.rs:47`
- Modify: `crates/julie-extract-cli/src/capability_snapshot.rs:328`
- Modify: `crates/julie-extractors/src/tests/capability_snapshot_test.rs`
- Test: `crates/julie-extract-cli/tests/cli_contract.rs`

**Interfaces:**
- Consumes: existing `KindCoverage` and additive serde defaults.
- Produces: `CapabilityKindCoverage.test_detection` and exact CLI projection under `kind_coverage.test_detection`.

**Contract inputs:** Fixed vocabulary is validated separately in Task 2; legacy JSON without the field must deserialize to empty coverage for compatibility tests.

**File ownership:** `crates/julie-extractors/src/capability_snapshot.rs`, `crates/julie-extract-cli/src/capability_snapshot.rs`, `capability_snapshot_test.rs`, `cli_contract.rs`

**Serialization required:** No.

**Dependency reason:** None - safe parallel batch.

**Step 1: Write failing snapshot tests**

Extend the mixed legacy/new fixture so the new row contains:

```json
"test_detection": {
  "supported": ["test_case"],
  "not_applicable": [],
  "open_gaps": []
}
```

Assert legacy rows default empty and new rows preserve the value through `kind_coverage_json`.

**Step 2: Run tests to verify failure**

Run: `cargo test -p julie-extractors capability_snapshot_test && cargo test -p julie-extract-cli --test cli_contract`

Expected: FAIL because `CapabilityKindCoverage` and the CLI projection omit the domain.

**Step 3: Implement the additive field**

Add the field beside the other kind-coverage domains:

```rust
#[serde(default)]
pub test_detection: KindCoverage,
```

Add `"test_detection": kind_coverage_domain(&kind_coverage.test_detection)` to the CLI JSON projection. Do not add a top-level `ArtifactCapabilityFlags` boolean or SQLite column.

**Step 4: Run tests to verify pass**

Run the Step 2 commands.

Expected: PASS for legacy defaulting and new-field projection.

**Step 5: Apply commit mode**

Use `parallel-lead-commit`: hand the verified diff and ledger to the lead without committing.

**Acceptance criteria:**
- [x] Legacy capability JSON still deserializes.
- [x] New rows preserve all three coverage arrays.
- [x] CLI projection emits the exact domain.
- [x] No schema/top-level capability flag is added.
- [x] Worker-scope verification passes and the change is handed to the lead per commit mode.

### Task 2: Classify every language and extend quality reporting

**Files:**
- Modify: `fixtures/extraction/capabilities.json`
- Modify: `scripts/language-data-quality-report.mjs:8`
- Modify: `crates/julie-extractors/src/tests/capability_matrix.rs:1084`

**Interfaces:**
- Consumes: registered golden expected files and the fixed test-role vocabulary.
- Produces: one non-silent `test_detection` cell per supported language and report counts derived from golden symbols.

**Contract inputs:** Current registered goldens contain explicit test-role evidence for VB.NET `test_case`; every other claim must be proven from the live fixture tree during implementation rather than assumed from detector unit tests.

**File ownership:** `fixtures/extraction/capabilities.json`, `language-data-quality-report.mjs`, `capability_matrix.rs`

**Serialization required:** No.

**Dependency reason:** None - safe parallel batch.

**Step 1: Write failing vocabulary/evidence tests**

Add a validator that every language row contains `test_detection`, every one of the three units appears exactly once across supported/not-applicable/open-gaps, supported units occur in registered golden output, and every gap names the closure plan.

Derive observed counts from golden symbols:

```javascript
if (symbol.metadata?.is_test === true) counts.test_case += 1;
if (symbol.metadata?.test_container === true) counts.test_container += 1;
if (symbol.metadata?.test_lifecycle === true) counts.test_lifecycle += 1;
```

Account for first-class fields too if normalized expected artifacts expose them in the future.

**Step 2: Run tests to verify failure**

Run: `cargo xtask test capability && node scripts/language-data-quality-report.mjs --strict`

Expected: FAIL because the domain is absent from every row/report.

**Step 3: Populate honest rows**

For each language, inspect its registered golden outputs. Put observed roles in `supported`. Put unproven roles in `open_gaps` pointing to `docs/plans/2026-07-09-test-detection-golden-closure-implementation-plan.md`. Use `not_applicable` only with a source-backed language-semantic explanation; do not infer it from an empty fixture.

Add `test_detection` to report domains and silent-cell validation. Do not yet add it to `CODE_LANGUAGE_EXPECTATIONS`; open gaps are staged debt owned by the closure plan and must not make this contract-introduction branch fail the existing strict bar.

**Step 4: Run tests to verify pass**

Run the Step 2 commands.

Expected: PASS with `silent_cells: 0` and `quality_bar_debts: 0`; report output separately lists supported/open test-role coverage.

**Step 5: Apply commit mode**

Use `parallel-lead-commit`: hand the verified diff, strict report, and per-language classification ledger to the lead without committing.

**Acceptance criteria:**
- [x] Every supported language classifies all three fixed units exactly once.
- [x] Supported claims are registered-golden-backed.
- [x] All unproven roles point to the named closure plan.
- [x] No uncertain language is marked not applicable.
- [x] Strict report remains zero/zero.
- [x] Worker-scope verification passes and the change is handed to the lead per commit mode.

### Task 3: Round-trip the public artifact and document consumer rules

**Files:**
- Modify: `crates/julie-extract-artifact/tests/writer_contract.rs:1511`
- Modify: `crates/julie-extract-artifact/tests/jsonl_contract.rs:55`
- Modify: `crates/julie-extract-artifact/tests/schema_contract.rs:357`
- Modify: `crates/julie-extract-cli/tests/operations_contract.rs:111`
- Modify: `docs/contracts/sqlite-schema-v4.md`
- Modify: `docs/contracts/jsonl-v3.md`
- Create: `docs/contracts/test-evidence-v1.md`
- Create: `docs/architecture/continuous-testing-evidence-boundary.md`

**Interfaces:**
- Consumes: final `test_detection` capability rows from Tasks 1-2.
- Produces: exact SQLite `kind_coverage_json`, JSONL `language_capability`, `languages --json`, and consumer safety documentation.

**Contract inputs:** File `status`, parse diagnostics, and role columns already exist; this task documents their combined meaning without changing schema.

**File ownership:** `writer_contract.rs`, `jsonl_contract.rs`, `schema_contract.rs`, `operations_contract.rs`, `sqlite-schema-v4.md`, `jsonl-v3.md`, `test-evidence-v1.md`, `continuous-testing-evidence-boundary.md`

**Serialization required:** Yes.

**Dependency reason:** Requires Tasks 1-2 final data shape.

**Step 1: Write failing public round-trip tests**

Extend artifact fixtures with a `test_detection` object and assert exact equality through SQLite, JSONL, and `languages --json`. Extend the schema-doc convention test to require the new domain in the documented list.

**Step 2: Run tests to verify failure**

Run:

```bash
cargo test -p julie-extract-artifact --test writer_contract
cargo test -p julie-extract-artifact --test jsonl_contract
cargo test -p julie-extract-artifact --test schema_contract
cargo test -p julie-extract-cli --test operations_contract
```

Expected: FAIL until fixtures/docs include the additive domain.

**Step 3: Complete public serialization and docs**

The existing generic `kind_coverage_json` writer should require no production schema change; if tests reveal otherwise, fix only the generic projection. The new contract doc defines positive evidence, capability evidence, diagnostic gates, `failed_preserved`, and the prohibition on negative impact-completeness claims. The architecture note records repo ownership:

```text
julie-extractors: emitted test roles + capability/diagnostic evidence
Miller: deterministic graph candidates over those facts
Eros: runner inventory, scheduling, results, freshness, and verdicts
```

**Step 4: Run branch gates**

Run:

```bash
cargo xtask test default
cargo xtask test capability
cargo xtask test contract
node scripts/language-data-quality-report.mjs --strict
```

Expected: all pass; strict report remains zero/zero.

**Step 5: Apply commit mode**

Use `serial-worker-commit`: commit after focused and branch gates pass; record the SHA.

**Acceptance criteria:**
- [ ] SQLite, JSONL, and CLI round-trip exact capability data.
- [ ] Contract docs define diagnostic and failed-preserved gates.
- [ ] Docs distinguish extracted roles from runner inventory and semantic impact.
- [ ] No schema/contract version is bumped unnecessarily.
- [ ] Default, capability, contract, and strict report gates pass.
- [ ] Worker-scope verification passes and the change is committed per commit mode.

## Program Exit Criteria

- [ ] Every language has a non-silent, vocabulary-valid `test_detection` cell.
- [ ] No supported role lacks registered golden evidence.
- [ ] Missing evidence points to the explicit golden-closure plan.
- [ ] All public artifact forms expose the same additive object.
- [ ] Consumer docs prevent false negative/exhaustiveness claims.
- [ ] Default/capability/contract gates and strict quality report pass.
