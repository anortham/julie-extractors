# Rebind Verb Implementation Plan (P2)

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when
> subagent delegation is available. Fall back to razorback:executing-plans for single-task,
> tightly-sequential, or no-delegation runs.

**Goal:** Implement the `julie-extract rebind` verb per the frozen P1 contract — a pure, atomic
metadata retarget that rewrites an artifact's recorded root and identity so a copied artifact can
serve a different checkout of the same repo.

**Architecture:** One new CLI verb following the existing clap/`CommandOutcome` conventions. All
writes in a single transaction on the WAL connection. No copying, no scanning, no sensitive-root
policy — those stay with the caller (Miller). The authoritative contract is
`/Users/murphy/source/miller/docs/plans/2026-08-05-rebind-contract-design.md` §3; every §3
requirement is restated here so workers need not read it.

**Tech Stack:** Rust (rustc 1.97.1 — the worktree has a directory override), clap, rusqlite,
serde. One new direct dependency: `getrandom` (already in `Cargo.lock` transitively).

**Architecture Quality:** Additive-only surface: new verb, new report section, new
`ReportOperation`/`ReportMode`/`ReportCode` variants, new optional metadata keys. No change to any
existing verb's argv, report shape, or SQLite schema (`SQLITE_SCHEMA_VERSION` stays 5). Risk
concentrates in transaction atomicity and in keeping the validation order exactly as contracted.

## Global Constraints

- **Worktree:** ALL work happens in
  `/Users/murphy/.config/razorback/worktrees/julie-extractors/rebind-verb` (branch `rebind-verb`).
  Every worker's step 1: `cd` there, verify `pwd`, `git branch --show-current` = `rebind-verb`.
- Invocation (verbatim from the contract):
  `julie-extract rebind --db <artifact.db> --root <new-root> [--json] [--strict-schema]`.
- Validation ORDER (before any write): (1) clap/path usage errors → exit 2, canonicalization via
  `paths.rs` helpers like every verb; (2) artifact open + `check_versions` gate (same as
  update/delete) → typed exit 3; (3) **fingerprint gate**: recorded
  `parser_inventory_fingerprint` and `capability_snapshot_fingerprint` must equal
  `current_capability_fingerprints()` (`crates/julie-extract-cli/src/capability_snapshot.rs:16`)
  → NEW typed exit-3 code; (4) **committed-revision gate**: `latest_revision_id(&connection)`
  must prove at least one committed extraction revision → NEW typed exit-3 code; (5) `--root`
  equal to recorded `root_path` → success no-op, exit 0, `changed: false`.
- Effects, ONE transaction: `root_path` ← `display_path(canonical new root)`; `artifact_id` ←
  `artifact-<32 lowercase hex>` from 16 `getrandom` bytes (NOT the clock-only
  `generated_artifact_id`); `updated_at` refreshed; `created_at` preserved; NEW additive keys
  `rebound_from_root`, `rebound_from_artifact_id`, `rebound_at` (RFC3339, same clock source as
  `updated_at`). `REQUIRED_METADATA_KEYS`
  (`crates/julie-extract-artifact/src/metadata.rs:7-19`) is NOT extended. `binary_version`, both
  fingerprints, `reference_resolution_*`, `index_level`, all data tables, and
  `extraction_revisions` are untouched.
- Report: JSON carries a new optional `rebind` section — `previous_root`, `new_root`,
  `previous_artifact_id`, `new_artifact_id`, `changed: bool` — present only for the rebind
  operation. `ReportOperation::Rebind`; new `ReportMode::Metadata` (rebind is neither read-only
  nor a scan). New `ReportCode` variants for the two refusals; the size-asserted `ALL`/
  `ERROR_CODES` arrays (`crates/julie-extract-artifact/src/reports.rs:391,420`) must be bumped.
- Exit codes stay coarse: 0 completed (incl. no-op), 1 failed, 2 usage, 3 incompatible.
- No changes to existing verbs. `cargo fmt` clean; clippy `-D warnings` clean (workspace, all
  targets).

## Verification Strategy

**Project source of truth:** `docs/release.md` (branch gates) + xtask test tiers.

**Worker red/green scope:** targeted test runs, e.g.
`cargo test -p julie-extract-cli --test rebind_contract` or
`cargo test -p julie-extract-artifact reports`, per task.

**Worker ceiling:** `cargo test -p julie-extract-cli` / `-p julie-extract-artifact`. Workers do
not run xtask tiers.

**Worker gate invariant:** each task's tests prove the contracted behavior stated in its
acceptance criteria; a failing assigned gate stops the worker (report, don't weaken).

**Lead affected-change scope:** `cargo xtask test default` and `cargo xtask test contract` after
each accepted task batch.

**Branch gate (before declaring the branch done):** `cargo fmt --check`; clippy `-D warnings`
(workspace, all targets, `test-perf` on); `cargo test -p xtask`; `cargo xtask test default`;
`cargo xtask test contract`; `cargo xtask test capability`; `cargo xtask dogfood repo`
(per `docs/release.md:33-53`).

**Escalation triggers:** any touched file under `crates/julie-extract-artifact/src/` beyond
`reports.rs`/`metadata.rs` ⇒ also run `cargo test -p julie-extract-artifact` in full at the
affected-change tier.

**Assigned verification failure:** Workers stop and report when assigned verification fails,
unless this plan explicitly says to update that gate.

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, and
timestamp per gate. Reuse passing same-HEAD evidence rather than rerunning expensive tiers.

**Out of scope for this plan:** release packaging, tag/publish, and the Miller pin bump — those
follow separately and require explicit user approval (program plan P2 note).

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: rebind verb core | None - serial | Modify: `crates/julie-extract-cli/src/{args.rs,commands.rs,artifact_access.rs,reports.rs}`, `crates/julie-extract-artifact/src/{reports.rs,metadata.rs}`, `crates/julie-extract-cli/Cargo.toml`, `Cargo.lock`. Create+Test: `crates/julie-extract-cli/tests/rebind_contract.rs` | Yes | Produces the verb, report shapes, and codes every other task consumes. |
| Task 2: row-level equivalence gate | Batch B | Create+Test: `crates/julie-extract-cli/tests/rebind_equivalence.rs` only | No | None - safe parallel batch (disjoint files from Task 3; consumes Task 1's committed verb). |
| Task 3: contract docs | Batch B | Modify: `docs/contracts/cli.md`, `docs/contracts/reports.md` only | No | None - safe parallel batch (docs only; consumes Task 1's committed shapes). |

Commit modes: Task 1 `serial-worker-commit`. Tasks 2 and 3 `parallel-lead-commit`.

---

### Task 1: rebind verb core

**Files:**
- Modify: `crates/julie-extract-cli/src/args.rs` (add `Rebind` to `Command` at :16-24; new
  `RebindArgs` — `root`, `db`, `strict_schema`, `json`, mirroring `DeleteArgs` at :113-130 minus
  `file`)
- Modify: `crates/julie-extract-cli/src/commands.rs` (dispatch arm at :83-92; new `fn rebind`
  modeled on `fn info` at :1028-1071; transaction body)
- Modify: `crates/julie-extract-cli/src/artifact_access.rs` (an open-for-rebind path that runs
  `check_versions` but deliberately NOT the `RootMismatch` gate — retargeting a different root is
  the verb's purpose; the `open_artifact_for_root` precedent is at :295-317)
- Modify: `crates/julie-extract-artifact/src/reports.rs` (`ReportOperation::Rebind` at :116-123;
  `ReportMode::Metadata` at :125-134; two new `ReportCode` variants + `ALL` 26→27+/`ERROR_CODES`
  18→19+ size-asserted arrays at :391,:420; new optional `rebind` report section struct)
- Modify: `crates/julie-extract-artifact/src/metadata.rs` (a targeted
  `apply_rebind(tx, new_root, new_artifact_id, now)`-style helper that UPDATEs/UPSERTs exactly the
  contracted keys; keep `REQUIRED_METADATA_KEYS` unchanged)
- Modify: `crates/julie-extract-cli/Cargo.toml` (+`getrandom`)
- Test: `crates/julie-extract-cli/tests/rebind_contract.rs` (new; follow the
  `tests/path_policy.rs` / `tests/operations_contract.rs` harness conventions — build a small
  fixture tree, run the binary, assert report JSON + exit codes + on-disk metadata)

**Interfaces:**
- Consumes: `current_capability_fingerprints()` (capability_snapshot.rs:16),
  `latest_revision_id`, `display_path`, `base_report`/`outcome` report builders, `check_versions`.
- Produces: the `rebind` verb; `ReportOperation::Rebind`; `ReportMode::Metadata`; the `rebind`
  report section `{previous_root, new_root, previous_artifact_id, new_artifact_id, changed}`;
  two new ReportCode names (worker chooses names in the existing naming style, e.g.
  `fingerprint_mismatch`, `no_committed_revision`); the exact metadata-key effects listed in
  Global Constraints. Tasks 2 and 3 depend on these exact names/shapes — record final names in
  the task report.

**Contract inputs:** Global Constraints block above (validation order, effects, report shape).

**File ownership:** Modify: `crates/julie-extract-cli/src/{args.rs,commands.rs,artifact_access.rs,reports.rs}`, `crates/julie-extract-artifact/src/{reports.rs,metadata.rs}`, `crates/julie-extract-cli/Cargo.toml`, `Cargo.lock`. Create+Test: `crates/julie-extract-cli/tests/rebind_contract.rs`

**Serialization required:** Yes

**Dependency reason:** Produces the verb, report shapes, and codes every other task consumes.

**What to build:** The verb end to end under TDD (razorback:test-driven-development): scan a
small fixture to a real artifact, then drive rebind through every contracted behavior.

**Approach:** Write the transaction with rusqlite on the writer connection (WAL is already set by
`ArtifactWriter::open_path`, writer.rs:301-324 — but rebind does not need the full writer: a
plain `Connection::open` + one `unchecked_transaction` over `artifact_metadata` is enough and
avoids `initialize_metadata`'s existed-check subtleties at writer.rs:314-316). Interrupted-rebind
atomicity: assert that a transaction that fails mid-flight (simulate by making one UPDATE violate
a constraint in a test double, or kill between open and commit) leaves all 11+ metadata rows
byte-identical.

**Acceptance criteria:**
- [x] `rebind --db --root --json` retargets: `root_path` new, `artifact_id` new
      `artifact-<32 hex>` ≠ old, `created_at` preserved, `updated_at` refreshed, three provenance
      keys present and correct, all other metadata + a sampled data-table row count unchanged.
- [x] Same-root invocation: exit 0, `changed: false`, zero metadata mutations (byte-compare).
- [x] Fingerprint mismatch (tamper the stored fingerprint in the fixture artifact): typed exit 3
      with the new code; artifact untouched.
- [x] No committed revision (metadata-only shell fixture): typed exit 3 with the new code.
- [x] `--strict-schema` and version gates behave exactly as they do for `update` (reuse the
      `check_versions` path; one test proving the wiring).
- [x] After rebind, a plain `scan --root <new-root> --db <db>` passes the root gate and runs
      incremental (one test: byte-identical tree ⇒ `no_change`).
- [x] `cargo test -p julie-extract-cli --test rebind_contract` and
      `cargo test -p julie-extract-artifact` pass; fmt + clippy clean on touched crates.
- [x] Committed by the worker (serial-worker-commit) with SHA recorded.

---

### Task 2: row-level equivalence gate

**Files:**
- Create+Test: `crates/julie-extract-cli/tests/rebind_equivalence.rs`

**Interfaces:**
- Consumes: the `rebind` verb binary surface from Task 1 (invocation + exit codes only — no
  internal symbols), and the row-comparison approach of
  `crates/julie-extract-cli/tests/resolution_scope_equivalence.rs` (reuse/adapt its
  full-vs-incremental comparison helpers).
- Produces: the P2 acceptance gate the program plan requires ("rebound artifact is
  indistinguishable from a fresh scan").

**Contract inputs:** Exclusion list for equivalence (from the frozen contract §9): `artifact_id`,
timestamp keys (`created_at`, `updated_at`, `rebound_at`), provenance keys, and
`extraction_revisions` history (incl. `revision_file_changes` bookkeeping tied to revision ids).
Everything else must be row-equivalent.

**File ownership:** Create+Test: `crates/julie-extract-cli/tests/rebind_equivalence.rs` only

**Serialization required:** No

**Dependency reason:** None - safe parallel batch (disjoint files from Task 3; consumes Task 1's
committed verb).

**What to build:** The contract's equivalence proof: scan tree A (a **multi-language fixture** —
at minimum Rust + C# + TypeScript files, per the language-parity rule) → copy the artifact with
`std::fs::copy` (the test is quiescent; online backup is Miller's side) → rebind to tree B →
`scan` → compare against a fresh from-scratch scan of tree B.

**Approach:** Three arms: (a) tree B byte-identical to A (delta must be `no_change`; equivalence
trivial but assert it); (b) modify-only delta (edit function bodies in ≥2 languages); (c)
add/delete delta (add one file, delete one file — this arm exercises the structure-changed
full-resolution path). Compare per-table row multisets over all twelve path-bearing tables plus
the resolution overlay, normalizing the excluded columns. Keep runtime test-suite-friendly
(small fixture, three scans + three rebind chains ≈ seconds).

**Acceptance criteria:**
- [ ] All three arms assert row-level equivalence with ONLY the contracted exclusions.
- [ ] Arm (a) additionally asserts the post-rebind scan reported `no_change`.
- [ ] `cargo test -p julie-extract-cli --test rebind_equivalence` passes.
- [ ] Verified diff handed to the lead (parallel-lead-commit).

---

### Task 3: contract docs

**Files:**
- Modify: `docs/contracts/cli.md` (new verb section alongside scan/update/delete/info/export/
  languages; the one-artifact-one-root paragraph at :117-146 gains the rebind exception; version
  pins at :12-22 updated per that file's own versioning policy for an additive verb)
- Modify: `docs/contracts/reports.md` (the `rebind` report section fields, the new operation and
  `metadata` mode values, the new codes in the code tables)

**Interfaces:**
- Consumes: Task 1's final report field names, code names, and mode value (from Task 1's task
  report / committed code — read the code, don't guess).
- Produces: the documented contract Miller P3 codes against.

**Contract inputs:** Global Constraints block; Task 1's recorded final names.

**File ownership:** Modify: `docs/contracts/cli.md`, `docs/contracts/reports.md` only

**Serialization required:** No

**Dependency reason:** None - safe parallel batch (docs only; consumes Task 1's committed
shapes).

**What to build:** Contract documentation exactly matching the shipped behavior, in each file's
existing voice and structure (tables/sections mirroring the other verbs).

**Acceptance criteria:**
- [x] `cli.md` documents invocation, validation order, exit codes, the same-root no-op, and the
      root-binding exception; version pins updated per the file's policy.
- [x] `reports.md` documents the `rebind` section, operation/mode values, and both new codes,
      consistent with the code as committed.
- [x] Every documented flag/field/code name grep-matches the implementation.
- [x] Verified diff handed to the lead (parallel-lead-commit).
