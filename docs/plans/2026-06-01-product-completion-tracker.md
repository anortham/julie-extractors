# Product Completion Tracker Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Keep the standalone extraction product work focused after the bootstrap and performance-baseline merges.

**Architecture:** Treat this as the current project tracker, not a replacement for the detailed slice plans. Keep the product boundary unchanged: source tree to versioned extraction artifact, SQLite primary, JSONL secondary, `julie-extract` CLI primary, Rust crate secondary.

**Tech Stack:** Rust workspace, `julie-extract`, SQLite, JSONL, `cargo xtask`, GitHub Actions, release evidence docs, Goldfish briefs.

**Architecture Quality:** No product architecture impact. This plan coordinates remaining slices and records which completed plans are now reference material.

---

## Current Status

- v2.0.0 is published:
  https://github.com/anortham/julie-extractors/releases/tag/v2.0.0.
- Release Binaries workflow run `26781742834` passed from `main` commit
  `a1f5069a36975e446c6a533e60bdcd3a9d3c11fa`.
- Remote tag `v2.0.0` points at
  `a1f5069a36975e446c6a533e60bdcd3a9d3c11fa`.
- Four GitHub Release assets were published for Linux x86_64, macOS Apple
  Silicon, macOS Intel, and Windows x86_64.
- PR #9: https://github.com/anortham/julie-extractors/pull/9.
- PR #9 Fast Gates passed before merge.
- No active product implementation branch remains.
- All migration and post-bootstrap plans below are complete and should be treated as historical evidence, not active task queues.
- Julie code intelligence is available again for this repo. Local Julie state is workspace tooling, not product code.
- The v2.0.0 release target was selected for continuity from the old Julie
  extractor crate line, which had reached v1.22.0.

## Completed Milestones

- **Standalone product contracts:** completed in `docs/contracts/` and architecture docs.
- **Repo bootstrap:** completed in `docs/plans/2026-05-31-repo-bootstrap-implementation-plan.md`.
- **Old Julie code migration:** completed in `docs/plans/2026-05-31-julie-code-migration-implementation-plan.md`.
- **Post-bootstrap release readiness:** completed in `docs/plans/2026-06-01-post-bootstrap-stabilization-release-readiness.md`.
- **Release binary workflow:** completed in `docs/plans/2026-06-01-release-binaries-workflow.md`.
- **Incremental scan hash skip:** completed in `docs/plans/2026-06-01-incremental-scan-hash-skip.md`.
- **Dogfood no-change rescan baseline:** completed in `docs/plans/2026-06-01-dogfood-incremental-rescan-baseline.md`.
- **JSONL export buffering:** completed in `docs/plans/2026-06-01-jsonl-export-performance.md`.
- **Repeatable performance baseline:** completed in `docs/plans/2026-06-01-repeatable-performance-baseline.md`.

## Current Evidence

- PR #3 dogfood evidence: `docs/release-evidence/v0.1.0-dogfood.md`.
- Latest dogfood hard evidence:
  - cold scan `status=ok`;
  - immediate rescan `status=no_change`;
  - `created_revision_id=null`;
  - every rescan `counts.rows_written` value is `0`;
  - report-only timing: cold scan `18189ms`, rescan `215ms`, export `76771ms`.
- Historical release binary workflow evidence: `docs/release-evidence/2026-06-01-release-binaries-workflow.md`.
  - original workflow staged three Actions artifacts only;
  - this evidence is superseded by the v2.0.0 GitHub Release evidence.
- v2.0.0 release publishing evidence:
  `docs/release-evidence/2026-06-01-v2-0-0-release.md`.
  - Release Binaries workflow run `26781742834` passed;
  - GitHub Release `v2.0.0` points at
    `a1f5069a36975e446c6a533e60bdcd3a9d3c11fa`;
  - release assets were uploaded for Linux x86_64, macOS Apple Silicon, macOS
    Intel, and Windows x86_64.
- Release-binary dogfood evidence: `docs/release-evidence/2026-06-01-release-binary-dogfood.md`.
  - cold scan `7607ms`;
  - immediate no-change rescan `52ms`;
  - export `68983ms`;
  - release binary SHA-256 `af51b3792e10eb54f6aab5d94cd04c257801b183be0fb23f08db96ba23f441ce`.
- JSONL export performance plan: `docs/plans/2026-06-01-jsonl-export-performance.md`.
  - local SQLite table counts across every JSONL row domain: `0.158s`;
  - local fetch of all exported SQLite rows: `213232` rows in `0.763s`;
  - release binary export to `/dev/null`: `20.79s` real, `3.83s` user, `15.66s` sys;
  - first implementation target: buffered JSONL writes, with no JSONL/SQLite/report/CLI contract changes.
- JSONL export buffering evidence: `docs/release-evidence/2026-06-01-jsonl-export-buffering.md`.
  - PR #6: https://github.com/anortham/julie-extractors/pull/6;
  - implementation commit `14da93e`;
  - bounded-write red test failed before buffering with `2853` downstream writes for an `8558` byte fixture export;
  - buffered release binary export to `/dev/null`: `2.43s` real, `1.06s` user, `0.21s` sys;
  - fallback per-record line buffer is not needed before the repeatable baseline slice.
- Repeatable performance baseline evidence:
  `docs/release-evidence/2026-06-01-repeatable-performance-baseline.md`.
  - PR #7: https://github.com/anortham/julie-extractors/pull/7;
  - implementation commit `844f1bb`;
  - command:
    `cargo xtask performance baseline --root . --out-dir target/performance/julie-extractors-baseline --binary target/release/julie-extract --runs 3`;
  - cold scan min/median/max: `6277ms` / `6387ms` / `7508ms`;
  - no-change rescan min/median/max: `51ms` / `51ms` / `52ms`;
  - JSONL export min/median/max: `1330ms` / `1330ms` / `1333ms`;
  - stable rows: `1018` files, `33019` symbols, `215388` JSONL records.
- v0.1.0 release candidate audit evidence:
  `docs/release-evidence/2026-06-01-v0-1-0-release-candidate-audit.md`.
  - PR #8 merged into `main` as `516a11b` on 2026-06-01 after Fast Gates passed;
  - package staging commit `c407cde`;
  - release binary SHA-256
    `c52b86f01c369088fad94da2ca013c9ddcfc840830e787c2f758a06724cf9237`;
  - staged target `aarch64-apple-darwin`;
  - package checksum verification passed;
  - audit blocker fixed: SQLite and JSONL artifacts now persist `36` parser
    inventory rows, `36` language capability rows, `76` fixture rows, and
    `17` gap rows;
  - refreshed repeatable baseline at `805da3b`: cold scan min/median/max
    `6485ms` / `6514ms` / `7550ms`, no-change rescan `56ms` / `62ms` /
    `62ms`, JSONL export `1318ms` / `1321ms` / `1328ms`, stable output
    `1020` files, `33099` symbols, `216253` JSONL records.
- PR #9 release-blocker and v2.0.0 alignment evidence:
  - merged into `main` as `94f1661` on 2026-06-01 after Fast Gates passed;
  - local verification included `cargo xtask test default` and
    `cargo xtask test contract`;
  - GitHub Fast Gates passed in run `26775855499`.
- Pre-release post-merge main evidence:
  - `e9d5601` recorded the PR #9 merge status;
  - GitHub Fast Gates passed for `e9d5601` in run `26776385538`;
  - `gh pr list --state open` returned no open pull requests;
  - `cargo xtask release package-list` passed and showed the v2.0.0 package
    includes the CLI, checksums, contracts, release docs, and release notes.
- Release workflow publishing upgrade:
  - `a1f5069` added four-platform GitHub Release asset publishing;
  - Release Binaries workflow run `26781742834` passed for `a1f5069`;
  - `gh release view v2.0.0` confirmed a published, non-draft,
    non-prerelease GitHub Release;
  - `git ls-remote --tags origin 'v2.0.0'` confirmed the remote tag points at
    `a1f5069a36975e446c6a533e60bdcd3a9d3c11fa`.
- v2.0.0 version/test-role alignment:
  `docs/plans/2026-06-01-v2-0-0-version-and-test-role-contract.md`.
  - package metadata and release workflow defaults now target v2.0.0;
  - SQLite `symbols` promotes `is_test`, `test_container`, and
    `test_lifecycle` to first-class indexed booleans;
  - JSONL `symbol` records expose the same booleans while preserving metadata
    keys for old metadata-oriented consumers.
- Autonomous run report for PR #3: `.memories/autonomous-run-2026-06-01-dogfood-rescan-baseline.md`.

## Non-Goals To Keep Out

- Do not add Julie MCP/server/daemon/search/embedding/watcher/dashboard/editing behavior.
- Do not back-port extractor product work into `/Users/murphy/source/julie` unless explicitly asked.
- Do not change SQLite, JSONL, report, or CLI contracts casually. Route those through a fresh strategy-tier plan.
- Do not turn dogfood, certification, real-world, or release package gates into the default suite.
- Do not promote one-machine dogfood timings into hard performance thresholds.

## Ordered Next Slices

### Slice 1: Release-Binary Dogfood Evidence

**Why now:** The current dogfood performance evidence uses the default debug binary. We need release-binary evidence before setting budgets or optimizing based on timings.

**Expected files:**
- Modify or create release evidence under `docs/release-evidence/`.
- Modify this tracker if the resulting evidence changes the next-slice order.

**Verification:**
- `cargo build --release -p julie-extract-cli --bin julie-extract`
- `cargo xtask dogfood repo --root . --out-dir target/dogfood/julie-extractors-release --binary target/release/julie-extract`
- `python3 examples/python/sqlite_consumer.py target/dogfood/julie-extractors-release/artifact.sqlite`
- Branch gate only when committing code or release docs: `cargo xtask test default` and `cargo xtask test contract`.

**Acceptance criteria:**
- [x] Evidence records the release binary path, commit, timestamp, hard-gate statuses, row totals, artifact sizes, and report-only timings.
- [x] Evidence makes clear whether timings are debug or release profile.
- [x] Generated artifacts remain under `target/`.

### Slice 2: JSONL Export Performance Plan

**Why next:** Current evidence shows JSONL export dominates runtime. Do not optimize scan again until export has been inspected.

**Expected files:**
- Create a fresh plan under `docs/plans/`.
- Likely inspect `crates/julie-extract-artifact` export code and CLI export path before deciding implementation.

**Verification:**
- Start with Julie orientation and a plan. Do not start by running broad tests.
- Worker red/green scope depends on the chosen export bottleneck.
- Branch gate before PR: `cargo xtask test default` and `cargo xtask test contract`.

**Acceptance criteria:**
- [x] Plan identifies whether bottleneck is SQLite reads, JSON serialization, file writes, or process/profile overhead.
- [x] Plan separates hard contract invariants from report-only performance evidence.
- [x] No JSONL contract changes unless explicitly approved.

### Slice 3: JSONL Export Buffered Writer Implementation

**Why next:** Slice 2 identified unbuffered output writes as the first verified export bottleneck. Fix that before collecting repeatable baseline numbers.

**Expected files:**
- Modify `crates/julie-extract-artifact/src/jsonl.rs`.
- Modify `crates/julie-extract-artifact/tests/jsonl_contract.rs`.
- Modify CLI export tests only if the CLI path needs extra coverage.
- Create release evidence for before/after JSONL export metrics.

**Verification:**
- Follow `docs/plans/2026-06-01-jsonl-export-performance.md`.
- Focused worker scope: `cargo test -p julie-extract-artifact --test jsonl_contract buffered_export_uses_bounded_write_calls`.
- Branch gate before PR: `cargo xtask test default` and `cargo xtask test contract`.

**Acceptance criteria:**
- [x] JSONL v1 output shape, order, and report counts are unchanged.
- [x] Export uses bounded downstream write calls for multi-record output.
- [x] Release-profile report-only export metrics are recorded.

### Slice 4: Repeatable Performance Baseline

**Why after release evidence/export inspection:** Thresholds need repeated same-machine runs and release-profile data, not one debug dogfood run.

**Expected files:**
- Plan under `docs/plans/`.
- Evidence under `docs/release-evidence/` or a dedicated performance evidence doc.

**Verification:**
- Use a repeatable command or documented script only after the evidence shape is planned.
- Keep metrics report-only until repeated data supports a threshold.

**Acceptance criteria:**
- [x] Captures cold scan, no-change rescan, export, artifact sizes, and row counts across repeated runs.
- [x] Records variance or at least min/median/max before proposing hard budgets.
- [x] Keeps default tests fast.

### Slice 5: v0.1.0 Release Candidate Audit

**Why later:** This is meaningful after release-binary dogfood evidence and any export performance decision.

**Expected files:**
- Update release evidence, release notes, this tracker, and active brief.
- Fix release-blocking contract drift found by the audit when it is directly on
  the package-readiness path.

**Verification:**
- `cargo xtask test default`
- `cargo xtask test contract`
- Specialist gates only where the audit touches their owned areas.

**Acceptance criteria:**
- [x] Release package staging evidence is current.
- [x] Release notes match actual shipped surfaces and known non-goals.
- [x] No generated artifacts are committed.

### Slice 6: Release-Blocker Review Fixes

**Why now:** `docs/findings/CC_REVIEW.md` identified release-blocking and
high-priority contract gaps after the release candidate audit. Resolve those
before making the v2.0.0 publish decision.

**Reference plan:** `docs/plans/2026-06-01-release-blocker-review-fixes.md`.

**Status:** Merged via PR #9 as `94f1661` after local gates and GitHub Fast
Gates passed.

**Acceptance criteria:**
- [x] Partial scans preserve prior good rows and commit valid files when another
  supported file fails.
- [x] Intentionally empty supported files can replace stale symbols.
- [x] Discovery does not index out-of-root symlinked files.
- [x] Parser inventory and capability snapshot fingerprints are computed
  `sha256:<hex>` values.
- [x] Workspace lint inheritance and a scoped CI clippy gate are enforced.
- [x] Fast branch gates pass before merge.

### Slice 7: v2.0.0 Version And Test-Role Contract Alignment

**Why now:** The old Julie crate was already at v1.22.0, and test-role metadata
is a downstream lookup path that should not depend on unindexed JSON extraction.

**Reference plan:**
`docs/plans/2026-06-01-v2-0-0-version-and-test-role-contract.md`.

**Status:** Merged via PR #9 as `94f1661` after local gates and GitHub Fast
Gates passed.

**Acceptance criteria:**

- [x] Package metadata and release workflow defaults target v2.0.0.
- [x] Artifact contract version numbers remain v1.
- [x] SQLite and JSONL contracts expose first-class test-role booleans.
- [x] SQLite has required indexes for test-role lookups.
- [x] CLI scan preserves old metadata keys and fills the first-class columns.

## Verification Discipline

- Reuse same-HEAD passing evidence from plan ledgers instead of rerunning broad gates for status updates.
- Run focused tests after implementation changes.
- Run `cargo xtask test default` and `cargo xtask test contract` before merge, push, or PR.
- Run dogfood only for evidence-producing slices or dogfood-affecting changes.
- Do not run certification, real-world, dogfood, or release package staging as a reflex.

## Progress

- [x] Tracker created after PR #3 merge.
- [x] Slice 1: Release-binary dogfood evidence.
- [x] Slice 2: JSONL export performance plan.
- [x] Slice 3: JSONL export buffered writer implementation.
- [x] Slice 4: Repeatable performance baseline.
- [x] Slice 5: v0.1.0 release candidate audit.
- [x] Slice 6: Release-blocker review fixes.
- [x] Slice 7: v2.0.0 version and test-role contract alignment.
- [x] PR #8 merged to `main`.
- [x] PR #9 merged to `main`.
- [x] Release workflow publishing upgrade: build four platforms and publish
  GitHub Release assets.
- [x] Release decision: triggered the Release Binaries workflow for `v2.0.0`.
- [x] v2.0.0 GitHub Release published with four platform assets.
