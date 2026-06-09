# Complexity Metrics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Add a versioned `complexity_metrics` extraction fact contract for conservative file and symbol complexity metrics.

**Architecture:** Compute parser-backed metrics once inside the extractor base layer, then persist normalized primitive rows through the existing artifact, JSONL, report, and capability surfaces. Downstream tools receive facts; they own ranking, thresholds, dashboards, and any quality score.

**Tech Stack:** Rust 2024, tree-sitter, `julie-extractors`, `julie-extract-artifact`, `julie-extract` CLI contracts, SQLite, JSONL v2, fixture-backed capability evidence.

**Architecture Quality:** Approved shape is a new public row domain patterned after `structural_facts` and `source_regions`. Architecture risk is medium because SQLite, JSONL, reports, and capability evidence change, but complexity policy stays local to `crates/julie-extractors/src/base/complexity_metrics.rs`.

---

## Contract Shape

Add a new extraction row domain:

```text
complexity_metrics
```

Each row stores one measured scope, not one opaque score:

- stable `complexity_metric_id`
- `file_id`, `path`, and `language`
- `scope` with values `file` or `symbol`
- optional `symbol_id` for symbol scope
- `algorithm_id`, initially `julie-ast-complexity-v1`
- `covered_lines` and `covered_bytes`
- `decision_count`
- `loop_count`
- `max_nesting_depth`
- nullable `parameter_count`
- normalized line, column, and byte span
- optional `metadata_json`

File rows use the root node span and no `symbol_id`. Symbol rows use the
symbol body span when available, otherwise the symbol declaration span.

## Metric Semantics

The first algorithm is intentionally primitive:

- **decision count:** conditional, switch/match/case, catch/rescue, and ternary-like decision nodes.
- **loop count:** loop node count.
- **max nesting depth:** maximum nested decision or loop depth inside the scope.
- **parameter count:** direct parameter nodes for callable symbol scopes when the parser shape is clear; otherwise `NULL`.
- **covered lines/bytes:** source coverage for the measured scope.

The extractor does not emit maintainability index, cyclomatic score labels,
severity, risk, or ranking.

## Initial Language Matrix

The first slice proves the contract on a representative parser-backed matrix:

| Language | Scope Evidence | Initial Notes |
| --- | --- | --- |
| `rust` | file and symbol | `if_expression`, `match_expression`, loop expressions, function parameters |
| `go` | file and symbol | `if_statement`, `for_statement`, `switch_statement`, `select_statement`, function parameters |
| `python` | file and symbol | `if_statement`, `for_statement`, `while_statement`, `try_statement`, `match_statement`, function parameters |
| `javascript` | file and symbol | `if_statement`, loops, `switch_statement`, `catch_clause`, ternary, function parameters |
| `typescript` | file and symbol | JavaScript rules plus TypeScript parser variants |
| `c` | file and symbol | `if_statement`, loops, `switch_statement`, ternary, function parameters |
| `cpp` | file and symbol | C rules plus C++ parser variants |

Other languages publish explicit capability gaps or empty metric claims until
fixture evidence exists.

## Extraction Flow

1. Language extractors emit symbols and existing facts.
2. `registry::extract_for_language` invokes `collect_complexity_metrics(...)`.
3. The collector walks the syntax tree with a language config.
4. It emits one file-scope row and one symbol-scope row per measurable symbol.
5. The CLI maps `ComplexityMetric` to `ArtifactComplexityMetric`.
6. The writer persists rows, JSONL exports them, reports count them, and the
   capability snapshot publishes exact metric coverage claims.

## Verification Strategy

**Project source of truth:** `AGENTS.md`, `CLAUDE.md`, `RAZORBACK.md`,
`docs/testing-strategy.md`, `.github/workflows/ci.yml`, and
`.github/workflows/specialist-gates.yml`.

**Worker red/green scope:** Use focused package tests:

```bash
cargo test -p julie-extractors complexity_metrics
cargo test -p julie-extract-artifact schema_contract
cargo test -p julie-extract-artifact writer_contract
cargo test -p julie-extract-artifact jsonl_contract
cargo test -p julie-extract-artifact report_contract
cargo test -p julie-extract-cli operations_contract
```

**Worker ceiling:** Focused extractor, artifact, CLI contract, and capability
matrix tests. Workers do not own real-repo dogfood interpretation, release
gates, or broad certification.

**Worker gate invariant:** The focused tests prove that complexity metrics are
computed from parser facts, persisted to SQLite, exported to JSONL, counted in
reports, surfaced through CLI operations, and advertised only when fixture
evidence exists.

**Lead affected-change scope:** Run the focused commands above plus
`cargo test -p julie-extractors --features test-capability-matrix capability_matrix`
after the coherent implementation batch.

**Branch gate:** Before closing TODO item 8, run:

```bash
cargo xtask test default
cargo xtask test contract
```

**Replay/metric evidence:** A real-repo dogfood report should show row counts by
language and metric scope. Row presence is hard evidence; timing and size
changes are report-only unless a documented budget gate fails.

**Escalation triggers:** Escalate if implementation requires parser dependency
changes, a CLI exit/status-code change, a schema version bump beyond the current
v2 contract, default-suite runtime growth, or language parity claims without
fixture evidence.

**Assigned verification failure:** Investigate and fix focused failures within
the approved shape. Stop only for a true plan mismatch.

**Verification ledger:** Record invariant, command, scope label, commit SHA,
result, and timestamp. For dogfood evidence, also record row counts by language
and metric scope.

## Model Routing

**Project source of truth:** `RAZORBACK.md`.

**Strategy tier:** planning, architecture, decomposition, lead review, finding
triage.
- Harness mapping: inherit current Codex session model.

**Implementation tier:** bounded worker tasks from this approved plan.
- Harness mapping: inherit current Codex session model.

**Mechanical tier:** docs, fixtures, formatting, and rote edits with no metric
or acceptance-gate ownership.
- Harness mapping: inherit current Codex session model.

**Gate-interpretation reviewer:** use lead/strategy tier for failing tests,
weak evidence, public schema interpretation, or capability claim disputes.
- Harness mapping: inherit current Codex session model.

**Escalation tier:** use strategy tier for public artifact schema changes,
capability claim uncertainty, hidden Julie coupling, or default-suite runtime
growth.
- Harness mapping: inherit current Codex session model.

**Worker eligibility:** Bounded implementation is eligible because the public
shape is approved and follows the existing row-family pattern.

**Mechanical exclusion:** Mechanical workers cannot own failing tests, fixture
evidence interpretation, metric acceptance, or contract gates.

**Unsupported harness behavior:** If the harness cannot choose models per
agent, use `inherit` and continue.

## Acceptance Criteria

- [x] `ExtractionResults` includes `complexity_metrics`.
- [x] SQLite creates `complexity_metrics` with required columns and indexes.
- [x] `ArtifactWriter` inserts, replaces, deletes, and counts complexity rows.
- [x] JSONL includes `complexity_metric` records in the contract order.
- [x] Reports include `complexity_metrics` in row-domain counts.
- [x] CLI scan persists non-empty complexity rows for a supported fixture.
- [x] Capability evidence advertises supported metric coverage only with
      fixture-backed rows.
- [x] Contract docs define SQLite, JSONL, and report shapes.
- [x] Current-schema writer performance workload includes complexity rows.
- [x] TODO item 8 is marked complete after focused and branch verification.

## Out Of Scope

- A single extractor-owned quality score.
- Miller or Eros ranking, thresholds, dashboards, or risk labels.
- Raw AST serialization.
- Exhaustive language parity.
- Parser dependency changes.
