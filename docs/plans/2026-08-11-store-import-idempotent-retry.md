# Store Import Idempotent Retry Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Complete byte-identical `--from-artifact` retries without rematerializing artifact files while preserving validation, exactness, and crash recovery.

**Architecture:** After normal artifact preflight, match the current exact manifest to its committed originating coordinator request and require byte-identical payload JSON. Carry that private reuse identity into the request executor, which rechecks the current generation/hash transactionally before completing the new request.

**Tech Stack:** Rust, rusqlite, Julie store coordinator and manifest contracts.

**Architecture Quality:** Behavior stays local to the from-artifact adapter and executor; the caller-facing CLI and durable schemas remain unchanged. Risk is medium because the shortcut must not admit changed artifacts or incomplete prior requests.

## Global Constraints

- Identical retry must avoid `load_artifact_file` and L1/L2/L3 writes.
- Changed or corrupt artifacts and incomplete prior requests must execute or fail through the existing safe path.
- Canonical store output and crash recovery remain exact.
- Do not change release metadata, push, publish, or run an unbounded replay.

## Verification Strategy

**Project source of truth:** `AGENTS.md` and existing store-resolution contract tests.

**Worker red/green scope:** Exact tests in `store_resolution_adapters` covering cross-key retry and safety branches.

**Worker ceiling:** Focused adapter and affected store contract targets only.

**Worker gate invariant:** Identical payload produces no version-materialization event; changed and incomplete inputs do not take the shortcut.

**Lead affected-change scope:** Store adapter, canonical equivalence, and crash recovery targets.

**Branch gate:** Formatting and strict Clippy for `julie-extract-cli`, plus one bounded timed replay if focused gates pass.

**Security scope:** none declared; no dependency files change.

**Replay/metric evidence:** Event/row counts are hard gates; wall/RSS/CPU are report-only.

**Escalation triggers:** Any schema change, canonical mismatch, or crash recovery failure requires renewed root-cause analysis.

**Assigned verification failure:** Investigate focused failures; do not weaken exactness assertions.

**Verification ledger:** Record command, invariant, result, counts, and timing in the final checkpoint/report.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: Safe cross-key retry | None - serial | `from_artifact.rs`, `executor.rs`, `store_resolution_adapters.rs` | Not applicable - single task. | Not applicable - single task. |

### Task 1: Safe cross-key retry

**Files:**
- Modify: `crates/julie-extract-cli/src/store/from_artifact.rs`
- Modify: `crates/julie-extract-cli/src/store/executor.rs`
- Test: `crates/julie-extract-cli/tests/store_resolution_adapters.rs`

**Interfaces:**
- Consumes: validated `FromArtifactRequestPayload`, current exact view, committed coordinator request.
- Produces: private transactional reuse identity and durable reused completion event.

**Contract inputs:** Exact payload JSON equality, committed/acknowledged prior state, current exact generation/hash.

**File ownership:** `from_artifact.rs`, `executor.rs`, `store_resolution_adapters.rs`

**Serialization required:** Not applicable - single task.

**Dependency reason:** Not applicable - single task.

**Acceptance criteria:**
- [x] Cross-key identical retry is red before production changes and green afterward.
- [x] Changed artifact content takes normal materialization.
- [x] Incomplete prior request cannot authorize reuse.
- [x] Canonical equality and crash recovery remain green.
- [x] Focused telemetry proves zero retry materialization chunks.
