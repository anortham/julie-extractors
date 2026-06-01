# Incremental Scan Hash Skip Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Make incremental `julie-extract scan` skip parser and extraction work for files whose current content hash already matches the existing SQLite artifact.

**Architecture:** Keep SQLite as the primary artifact and keep `ArtifactWriter::write_scan` as the transactional writer. Move the optimization into the CLI scan preparation path: discover supported files, read source bytes as UTF-8, compute the same BLAKE3 content hash used by extracted rows, and pass a minimal unchanged `ArtifactFile` to the writer when the existing artifact hash matches. Do not change the SQLite schema, JSONL contract, report contract, or Rust extractor crate API for this slice.

**Tech Stack:** Rust, `julie-extract` CLI, `rusqlite`, BLAKE3 content hashes, existing `ArtifactWriter` SQLite transaction path.

**Architecture Quality:** Low to medium risk. The caller-facing contract stays the same, but the CLI scan preparation path now has a performance-sensitive boundary between source snapshotting and parser extraction.

---

## Source Documents

- `AGENTS.md`: product boundary, SQLite-first output, CLI-first integration, Julie read-only rule, and default-suite discipline.
- `RAZORBACK.md`: strategy-tier areas, worker eligibility, and verification ownership.
- `docs/contracts/cli.md`: `julie-extract scan` report and mode contract.
- `docs/contracts/sqlite-schema-v1.md`: stable file content hash and indexing contract.
- `docs/contracts/reports.md`: counts and no-change reporting.
- `docs/testing-strategy.md`: default and contract gate routing.
- `docs/plans/2026-05-31-julie-code-migration-implementation-plan.md`: completed migration baseline.
- `docs/plans/2026-06-01-post-bootstrap-stabilization-release-readiness.md`: release-readiness baseline and performance tripwire stance.

## Current Baseline

- Incremental `scan` discovers supported files, parses/extracts every supported file, then calls `ArtifactWriter::write_scan`.
- `ArtifactWriter::write_scan` already skips row rewrites when `files.content_hash` matches an existing row.
- This preserves artifact stability but still pays parser and extraction cost for unchanged files.
- The existing writer performance test proves one transaction for a tiny fixture batch; it does not prove the CLI avoids parser work before the writer.

## Architecture Quality

**Affected modules:** `crates/julie-extract-cli/src/commands.rs`, `crates/julie-extract-cli/src/extraction.rs`, CLI-focused tests, and this plan.

**Caller-facing interface:** The public CLI, SQLite v1, JSONL v1, and report schemas remain unchanged. The behavior improvement is internal: unchanged files are read and hashed, but not parsed.

**Depth/locality check:** Keep hash-first scan planning in the CLI crate because it depends on existing artifact rows and scan mode. Keep artifact row construction helpers in `extraction.rs` because they own file IDs, content hashes, line counts, and `ArtifactFile` shape.

**Test surface:** Prove the behavior through the CLI scan preparation interface with a test that fails when an unchanged file still invokes the extractor. Keep writer tests focused on transactional row writing.

**Seams/adapters:** Add only a small testable helper around supported scan targets and extraction callbacks if needed. Do not create a new service, daemon, cache layer, or artifact schema.

**Rejected shortcuts:** Do not add mtime-based skipping without schema support. Do not skip files based only on path or size. Do not make unchanged files disappear from the scan snapshot, because deletion detection depends on the snapshot paths. Do not push parsing decisions into `ArtifactWriter`, because the writer should remain a durable artifact writer, not a source-tree reader.

**Architecture risk:** Low to medium. The main risk is returning incomplete placeholder rows for changed files; tests must prove placeholders are only used when the hash matches the existing artifact.

## Verification Strategy

**Project source of truth:** `AGENTS.md`, `RAZORBACK.md`, `docs/testing-strategy.md`, and the contracts under `docs/contracts/`.

**Worker red/green scope:** `cargo test -p julie-extract-cli incremental_scan_reuses_existing_hash_without_parser_work`

**Worker ceiling:** `cargo test -p julie-extract-cli` and `cargo test -p julie-extract-artifact writer_performance`.

**Worker gate invariant:** Incremental scan reads and hashes discovered supported files, reuses existing hashes to bypass parser extraction for unchanged files, still sends every current supported path to `ArtifactWriter`, and keeps force scan parsing all files.

**Lead affected-change scope:** `cargo test -p julie-extract-cli`, `cargo test -p julie-extract-artifact writer_contract`, and `cargo test -p julie-extract-artifact writer_performance`.

**Branch gate:** `cargo xtask test default` and `cargo xtask test contract` before merge, push, or handoff.

**Replay/metric evidence:** No new hard timing threshold in this slice. Report test command durations if they regress noticeably; keep the existing writer tripwire as the hard performance gate.

**Escalation triggers:** Public schema/report/CLI changes, parser dependency changes, default-suite runtime growth, or a design that requires old Julie internals.

**Assigned verification failure:** Workers stop and report when assigned verification fails unless this plan explicitly says to update that gate.

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, and timestamp in the final report. Reuse a passing ledger entry only when the same HEAD and same command already passed in this run.

## Model Routing

**Project source of truth:** `RAZORBACK.md`.

**Strategy tier:** Planning, architecture, performance acceptance, and public contract interpretation.
- Harness mapping: inherit in this Codex session.

**Implementation tier:** Bounded edits to CLI scan preparation and extraction row helpers after the public contract stays unchanged.
- Harness mapping: inherit.

**Mechanical tier:** Formatting and wording-only documentation edits.
- Harness mapping: inherit.

**Gate-interpretation reviewer:** Lead interprets any failed parser-skip, report-count, or performance-tripwire result.
- Harness mapping: inherit.

**Escalation tier:** Public artifact contract changes, CLI status/count changes, parser dependency issues, weak test evidence, or repeated verification failures.
- Harness mapping: inherit.

**Worker eligibility:** Workers are eligible only for bounded, non-overlapping implementation or verification tasks after this plan fixes the public interface and verification ceiling.

**Escalation triggers:** Any change to public artifact schema, CLI status, exit code, error code, language capability claim, parser dependency version, or default-suite runtime.

**Mechanical exclusion:** Mechanical workers cannot own failing tests, performance evidence, or acceptance gates.

**Unsupported harness behavior:** If the harness cannot choose models per agent, use `inherit` and continue.

## File Structure

- Create: `docs/plans/2026-06-01-incremental-scan-hash-skip.md` - this plan and progress ledger.
- Modify: `crates/julie-extract-cli/src/extraction.rs` - expose source snapshotting and unchanged-row construction used by scan preparation.
- Modify: `crates/julie-extract-cli/src/commands.rs` - load existing file hashes for incremental scan and skip parser extraction for unchanged hashes.
- Modify/Create tests in `crates/julie-extract-cli/src/commands.rs` or CLI integration tests - prove unchanged hash skip and force-scan behavior.

## Open Decisions

- **Future mtime/size cache:** Rejected for this slice. It requires a schema-backed contract for file stat validity and invalidation behavior.
- **Progress metrics:** Rejected for this slice. Reports keep their existing schema; performance evidence comes from tests and final command timing.
- **SQLite index changes:** Rejected for this slice. Existing `files(path)` lookup supports writer skipping, and this change loads path/hash rows once for the scan. Add indexes only with measured evidence or a schema-contract task.

## Progress

- [x] Task 0: Plan baseline
- [x] Task 1: Add red parser-skip test
- [x] Task 2: Add source snapshot and unchanged file row helper
- [x] Task 3: Use existing SQLite hashes during incremental scan
- [x] Task 4: Verify focused and branch gates

## Tasks

### Task 0: Plan Baseline

**Files:**
- Create: `docs/plans/2026-06-01-incremental-scan-hash-skip.md`

**What to build:** Capture the optimization plan so scan performance work does not drift into schema churn, old Julie coupling, or non-product behavior.

**Acceptance criteria:**
- [x] Plan uses the required Razorback header.
- [x] Plan names the unchanged public contract.
- [x] Plan records rejected shortcuts and open decisions.
- [x] Plan defines focused and branch verification gates.

### Task 1: Add Red Parser-Skip Test

**Files:**
- Modify/Create tests near the CLI scan preparation code.

**What to build:** A failing test that proves unchanged files do not invoke parser extraction during incremental scan preparation.

**Approach:** Test a small supported-target helper with an injected extraction callback. Seed an existing hash for one current file and verify the callback is not invoked for that file, while a changed file is still extracted. Add a force-mode case proving force scan ignores the hash cache and extracts all files.

**Acceptance criteria:**
- [x] Test fails before production code because unchanged files are still parsed or the helper does not exist.
- [x] Test asserts actual callback counts and returned file paths, not only success.
- [x] Test covers incremental and force behavior.

### Task 2: Add Source Snapshot And Unchanged Row Helper

**Files:**
- Modify: `crates/julie-extract-cli/src/extraction.rs`

**What to build:** A source snapshot helper that reads UTF-8 content once, computes the canonical content hash, content byte count, and line count, plus an unchanged `ArtifactFile` constructor that uses the same stable file ID and row metadata as a parsed file.

**Approach:** Refactor `extract_artifact_file` to read a source snapshot, parse from the snapshot content, and map results. Construct unchanged rows with empty child vectors only when the caller has already verified the hash match.

**Acceptance criteria:**
- [x] Parsed files retain identical `file_id`, `content_hash`, `content_bytes`, `line_count`, `status`, and metadata behavior.
- [x] Unchanged rows include only file-level data and empty child rows.
- [x] Read errors still map to `ExtractFileErrorKind::Read`.

### Task 3: Use Existing SQLite Hashes During Incremental Scan

**Files:**
- Modify: `crates/julie-extract-cli/src/commands.rs`

**What to build:** Load existing `files.path` and `files.content_hash` rows from the current artifact for incremental scan, then skip parser extraction for discovered files whose current snapshot hash matches.

**Approach:** Keep existing metadata/root validation through `open_artifact_for_root`. Read hash rows from that validated connection. During normal incremental scan, pass the hash map into scan preparation. During force scan, do not use the hash map. Keep the writer call unchanged so snapshot deletion, no-change reporting, and single-transaction behavior stay owned by `ArtifactWriter`.

**Acceptance criteria:**
- [x] Existing root/schema validation behavior is unchanged.
- [x] Incremental scan still errors on unreadable current source files.
- [x] Missing files are still detected as deletions by the writer because current supported paths are preserved.
- [x] Force scan parses all current supported files.
- [x] Report counts remain compatible with existing contract tests.

### Task 4: Verify Focused And Branch Gates

**Files:**
- No planned production changes.

**What to verify:**
- `cargo test -p julie-extract-cli incremental_scan_reuses_existing_hash_without_parser_work`
- `cargo test -p julie-extract-cli`
- `cargo test -p julie-extract-artifact writer_contract`
- `cargo test -p julie-extract-artifact writer_performance`
- `cargo xtask test default`
- `cargo xtask test contract`

**Acceptance criteria:**
- [x] Focused parser-skip test passes.
- [x] Existing CLI scan/update/delete contracts pass.
- [x] Writer contract and writer performance gates pass.
- [x] Default and contract branch gates pass before merge, push, or handoff.
