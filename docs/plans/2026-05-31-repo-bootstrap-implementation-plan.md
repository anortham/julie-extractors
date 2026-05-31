# Repo Bootstrap Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Bootstrap `/Users/murphy/source/julie-extractors` as the standalone extraction product repo with clear contracts, migration scope, and test discipline before code migration starts.

**Architecture:** The repo owns extraction from source tree to versioned artifact. SQLite is the primary output, JSONL is a secondary export, and `julie-extract` is the main cross-runtime integration surface.

**Tech Stack:** Rust, tree-sitter, SQLite, JSONL, CLI binaries, fixture-based tests, parser certification tooling.

**Architecture Quality:** Medium-high risk. The module boundary is clear, but schema design, CLI stability, release packaging, and test-suite discipline are product-level concerns.

---

## Verification Strategy

**Project source of truth:** This repo's `AGENTS.md`, `docs/testing-strategy.md`, and future CI config.

**Worker red/green scope:** For docs-only bootstrap work, verify links and file presence with shell checks. For future code work, run the narrowest test for the touched command, schema module, or language extractor.

**Worker ceiling:** Workers may run docs checks and narrow command/language tests. Workers must not run full certification, real-world corpus, or release packaging unless explicitly assigned.

**Worker gate invariant:** Each worker-owned gate proves one concrete interface: CLI report shape, one schema behavior, one language extractor behavior, or one fixture invariant.

**Lead affected-change scope:** Run the changed-area tier once it exists. Until then, review file diffs and run docs/file-presence checks after a coherent batch.

**Branch gate:** Default tier plus contract tier once code exists. Release tier is not a branch gate unless preparing a release.

**Replay/metric evidence:** Default-suite wall time is a hard gate once the budget tripwire exists. Real-world extraction metrics are report-only until a release plan marks a specific metric as hard.

**Escalation triggers:** Schema changes, CLI exit code changes, parser dependency upgrades, cross-language capability changes, and release packaging changes require strategy-tier review.

**Assigned verification failure:** Workers stop and report when assigned verification fails, unless the plan explicitly says to update that gate.

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, and timestamp. For replay or metric evidence, also record hard-gate metrics and report-only metrics.

## Model Routing

**Project source of truth:** Until this repo has a `RAZORBACK.md`, inherit the current Julie routing policy for planning and implementation.

**Strategy tier:** Planning, architecture, schema design, CLI contract design, release design.
- Harness mapping: `gpt-5.5` medium/high in Codex when selectable.

**Implementation tier:** Bounded worker tasks after a clear plan.
- Harness mapping: `gpt-5.5` low/medium in Codex when selectable.

**Mechanical tier:** Docs, fixtures, manifests, formatting with no gate ownership.
- Harness mapping: `gpt-5.4-mini` or equivalent when selectable.

**Gate-interpretation reviewer:** Review of failing tests, schema contract failures, or release evidence.
- Harness mapping: `gpt-5.5` high or coding-focused review model when terminal-heavy.

**Escalation tier:** Public schema/API changes, repeated failures, parser dependency issues, subtle cross-language correctness.
- Harness mapping: strongest available reasoning model.

**Worker eligibility:** Workers are eligible only when file ownership is narrow, the interface is named, and the verification ceiling is explicit.

**Escalation triggers:** Any change that modifies a public artifact schema, command semantics, parser inventory, or test-tier policy.

**Mechanical exclusion:** Mechanical workers cannot own failing tests, replay evidence, metrics, or acceptance gates.

**Unsupported harness behavior:** If a harness cannot choose models per agent, inherit and note the limitation.

## File Structure

Initial docs:

- `README.md`: product entry point and current status.
- `AGENTS.md`: repo-specific development rules.
- `docs/product/vision.md`: product promise and audience.
- `docs/architecture/product-boundary.md`: module/interface boundary.
- `docs/architecture/cli-contract.md`: command and status draft.
- `docs/architecture/schema-principles.md`: SQLite/JSONL design principles.
- `docs/testing-strategy.md`: test tier policy.
- `RAZORBACK.md`: project-specific model/verification ownership policy.
- `docs/decisions/0001-standalone-extraction-product.md`: accepted product boundary.
- `docs/plans/2026-05-31-product-bootstrap-design.md`: current design state.
- `docs/plans/2026-05-31-repo-bootstrap-implementation-plan.md`: execution plan.
- `docs/plans/2026-05-31-migration-inventory.md`: Julie source inventory.

Future code shape:

- `crates/julie-extractors/`: extractor crate, or equivalent root crate after naming decision.
- `crates/julie-extract-cli/`: CLI if split from engine.
- `fixtures/extraction/`: golden and capability fixtures.
- `xtask/`: test tiers, certification, release helper commands.
- `docs/contracts/`: CLI, SQLite, JSONL, and report contracts.

## Tasks

### Task 1: Planning Baseline

**Files:**
- Create: `README.md`
- Create: `AGENTS.md`
- Create: `RAZORBACK.md`
- Create: `docs/product/vision.md`
- Create: `docs/architecture/product-boundary.md`
- Create: `docs/architecture/cli-contract.md`
- Create: `docs/architecture/schema-principles.md`
- Create: `docs/testing-strategy.md`
- Create: `docs/decisions/0001-standalone-extraction-product.md`
- Create: `docs/plans/2026-05-31-product-bootstrap-design.md`
- Create: `docs/plans/2026-05-31-repo-bootstrap-implementation-plan.md`
- Create: `docs/plans/2026-05-31-migration-inventory.md`

**What to build:** Seed the new repo with product and architecture docs that make the extraction boundary explicit before code moves.

**Approach:** Keep Julie intact. Capture SQLite-primary, JSONL-secondary, CLI-first, and no-compatibility-mode decisions.

**Acceptance criteria:**
- [ ] The new folder exists at `/Users/murphy/source/julie-extractors`.
- [ ] Docs capture the product boundary and rejected alternatives.
- [ ] Test discipline is documented before code migration.
- [ ] Migration inventory captures move/redesign/leave-behind source paths.
- [ ] Git baseline commit exists.

### Task 2: Migration Inventory

**Files:**
- Create: `docs/plans/2026-05-31-migration-inventory.md`

**What to build:** Classify Julie files as move, redesign, reference only, or leave behind.

**Approach:** Inventory extraction crate, external extract CLI, schema/report code, fixtures, parser certification, release workflow, and docs. Keep MCP/server/daemon/search out.

**Acceptance criteria:**
- [ ] Every moved area has source and target path.
- [ ] Every redesign area names the new owner.
- [ ] Leave-behind areas are explicit.

### Task 3: Contract Design

**Files:**
- Create: `docs/contracts/cli.md`
- Create: `docs/contracts/sqlite-schema-v1.md`
- Create: `docs/contracts/jsonl-v1.md`
- Create: `docs/contracts/reports.md`

**What to build:** Define the first clean product contracts for downstream users.

**Approach:** Design around Miller/Eros usage patterns without copying Julie's internal schema as the product model.

**Acceptance criteria:**
- [ ] CLI commands have inputs, statuses, exit codes, and error code shape.
- [ ] SQLite schema domains and metadata are specified.
- [ ] JSONL envelope and record kinds are specified.
- [ ] Reports are machine-readable and stable.

### Task 4: Test Tier Scaffold

**Files:**
- Create: `docs/testing-strategy.md` updates as needed.
- Future create: test runner config and convention tests.

**What to build:** Establish the fast-default test contract before code arrives.

**Approach:** Copy the useful lesson from Miller: default suite excludes scale/certification work, and a budget tripwire catches regressions.

**Acceptance criteria:**
- [ ] Default, language, contract, certification, real-world, and release tiers are defined.
- [ ] Slow tests have an explicit non-default home.
- [ ] Budget and convention-test requirements are documented.
