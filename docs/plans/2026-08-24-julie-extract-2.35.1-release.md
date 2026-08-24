# Julie Extract v2.35.1 Release Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Publish the integrated first-class QML and `qmldir` extraction work as Julie Extract v2.35.1 and verify every live release asset.

**Architecture:** Runtime behavior is already integrated on `main`. Release preparation advances the three publishable crate versions and extraction identity epoch, classifies the output change, and feeds the existing four-target GitHub release workflow; post-publication work records independently verified asset and contract evidence.

**Tech Stack:** Rust 1.95, Cargo workspace, SQLite/JSONL extraction contracts, GitHub Actions, GitHub CLI, Windows NTFS verification through `win-test`.

**Architecture Quality:** No Architecture Impact — release work updates existing version, epoch, compatibility, documentation, and evidence contracts without changing schema or public API shape.

## Global Constraints

- Publish version `2.35.1` from clean, current `main` under the user's explicit approval.
- Keep artifact schema 7, JSONL v5, family-store schema 2, and store format epoch 1.
- Advance extraction identity epoch from 4 to 5 because QML, `.qmltypes`, and `qmldir` extraction output changes.
- Classify the extraction-output change as compatible; existing readers can read the unchanged tables, but consumers must re-extract or populate epoch-5 family-store versions.
- Keep v2.35.0 labeled as the current published release until v2.35.1 is live.
- Windows is a first-class target; verify default, QML, and `qmldir` gates on NTFS at the exact candidate commit.
- Do not modify Miller; its package pin and continuous-testing configuration remain separate work.
- The prior Grok campaign reviewed the complete QML runtime diff. Release-only metadata and evidence do not reopen that exhausted campaign.

## Verification Strategy

**Project source of truth:** `AGENTS.md`, `docs/release.md`, `docs/contracts/{store-v1,extraction-output-changes}.md`, `.github/workflows/{ci,release-binaries,specialist-gates}.yml`, and `xtask/src/{compat,release}.rs`.

**Worker red/green scope:** Focused version/epoch contract tests, `cargo check --workspace`, `cargo fmt --all -- --check`, `cargo xtask release package-list`, `cargo xtask release preflight --version 2.35.1`, agent-doc sync, and whitespace checks.

**Worker ceiling:** Metadata-sensitive version, epoch, compatibility, and release-preflight gates. Workers do not push, tag, publish, or accept broad branch gates.

**Worker gate invariant:** Every publishable version reports 2.35.1; current extraction identity reports epoch 5 with compatibility baseline/current 4/5; release notes and ledger classify QML output changes without claiming publication.

**Lead affected-change scope:** Review the complete release diff, refresh Miller, run post-edit impact, and verify exact version/epoch/pointer consistency.

**Branch gate:** `cargo fmt --check`; `cargo test -p xtask`; `cargo xtask test default`; `cargo xtask test contract`; `cargo xtask test certification`; `cargo xtask test real-world-smoke`; `cargo xtask test real-world-release`; `cargo xtask test language qml`; `cargo xtask test language qmldir`; `cargo xtask dogfood repo --root . --out-dir target/dogfood/julie-extractors`; `node scripts/language-data-quality-report.mjs --strict`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`; `cargo xtask release package-list`; `cargo xtask release preflight --version 2.35.1`; `cargo xtask compat-check --previous-binary <published-v2.35.0-linux-binary>`; `scripts/check-agent-doc-sync.sh`; `git diff --check`; plus clean-SHA Windows default/QML/`qmldir` gates.

**Security scope:** `gitleaks git v2.35.0..HEAD`; `cargo audit`; `cargo deny --all-features check`.

**Replay/metric evidence:** Version, epoch, schema, compatibility, default/contract/certification/real-world tiers, QML gates, package inputs, all four live archives, embedded checksums, and downloaded binary behavior are hard gates. Timing is report-only.

**Escalation triggers:** Any undeclared output difference, same-epoch output change, schema/JSONL/store drift, version mismatch, missing package input, Windows failure, failed CI/release job, asset checksum mismatch, or unstable/draft release blocks publication or closeout.

**Assigned verification failure:** Diagnose and repair the candidate or release process; do not weaken gates, move tags, force-push, or publish partial assets.

**Verification ledger:** Record invariant, command, scope, commit SHA, result, timestamp, workflow URLs, asset digests, and downloaded-binary checks in checkpoints and release evidence.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: Prepare candidate | None - serial | Modify three publishable manifests, `Cargo.lock`, workflow defaults, epoch source/tests/docs, compatibility constants, release-note index, and this plan; create `docs/release-notes/v2.35.1.md` and checkpoint. | Yes | Candidate metadata must be internally consistent before verification or publication. |
| Task 2: Verify and publish | None - serial | No source-file ownership; lead owns exact-HEAD gates, security scans, push, tag, workflow monitoring, and release publication. | Yes | Requires the committed Task 1 candidate and passing gates. |
| Task 3: Verify live release and close out | None - serial | Create release evidence and checkpoint; modify published-release pointers in `docs/release.md`, `README.md`, `docs/README.md`, `docs/release-notes/README.md`, `docs/release-notes/v2.35.1.md`, `docs/site/index.html`, this plan, and Goldfish brief state. | Yes | Requires a successful stable release with downloadable assets. |

### Task 1: Prepare the v2.35.1 candidate

**Files:**
- Create: `docs/release-notes/v2.35.1.md`
- Modify: `crates/julie-extractors/Cargo.toml`
- Modify: `crates/julie-extract-artifact/Cargo.toml`
- Modify: `crates/julie-extract-cli/Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `.github/workflows/release-binaries.yml`
- Modify: `.github/workflows/specialist-gates.yml`
- Modify: `crates/julie-extractors/src/lib.rs`
- Modify: `crates/julie-extractors/src/tests/api_surface.rs`
- Modify: `crates/julie-extract-artifact/src/store/layout.rs`
- Modify: `crates/julie-extract-artifact/tests/store_connection_contract.rs`
- Modify: `crates/julie-extract-cli/tests/store_equivalence.rs`
- Modify: `xtask/src/compat.rs`
- Modify: `docs/contracts/extraction-output-changes.md`
- Modify: `docs/contracts/store-v1.md`
- Modify: `docs/architecture/versioned-index-store.md`
- Modify: `docs/release-notes/README.md`
- Modify: `docs/plans/2026-08-24-julie-extract-2.35.1-release.md`
- Create: `.memories/<Goldfish checkpoint generated before commit>`

**Interfaces:**
- Consumes: integrated QML runtime commits, published v2.35.0, epoch-4 identity, and the four-target package workflow.
- Produces: a clean v2.35.1 candidate at epoch 5 whose output changes are compatibility-classified.

**Contract inputs:** QML/`.qmltypes`/`qmldir` facts and test targets, unchanged schema contracts, file-version identity `(path, content_hash, extraction_epoch)`, and published v2.35.0 as the compatibility baseline.

**File ownership:** Modify three publishable manifests, `Cargo.lock`, workflow defaults, epoch source/tests/docs, compatibility constants, release-note index, and this plan; create `docs/release-notes/v2.35.1.md` and checkpoint.

**Serialization required:** Yes.

**Dependency reason:** Candidate metadata must be internally consistent before verification or publication.

**What to build:** Bump 2.35.0 to 2.35.1, advance extraction identity 4 to 5, set compatibility baseline/current epochs to 4/5, and document the compatible QML output expansion and consumer re-extraction action. Keep all published-release pointers on v2.35.0 until Task 3.

**Approach:** Follow the v2.34.3/v2.34.4 epoch-bump pattern. Change only product-current epoch literals, not arbitrary historical or multi-epoch fixture values.

**Acceptance criteria:**
- [x] All publishable manifests and lockfile package entries report 2.35.1.
- [x] Product-current extraction identity is epoch 5 and compatibility baseline/current are 4/5.
- [x] Ledger and release notes classify the QML output change as compatible and preserve schema versions.
- [x] Workflow defaults report 2.35.1 while current-published pointers still report v2.35.0.
- [x] Worker-scope verification passes and the lead commits the reviewed candidate.

### Task 2: Verify and publish v2.35.1

**Files:** None.

**Interfaces:**
- Consumes: the committed Task 1 candidate and published v2.35.0 Linux binary.
- Produces: pushed `main`, tag `v2.35.1`, successful source CI, and a stable four-asset GitHub release.

**Contract inputs:** Branch gate, security scope, user approval to push/tag/publish, and `.github/workflows/release-binaries.yml`.

**File ownership:** No source-file ownership; lead owns exact-HEAD gates, security scans, push, tag, workflow monitoring, and release publication.

**Serialization required:** Yes.

**Dependency reason:** Requires the committed Task 1 candidate and passing gates.

**What to build:** Run every exact-HEAD Linux and Windows gate, scan the candidate, push `main`, wait for source CI, create and push `v2.35.1`, and wait for all release workflow jobs and the stable GitHub release.

**Approach:** Never force-push or move a tag. Stop publication on any failed hard gate; after tag push, repair through a superseding release rather than rewriting published history.

**Acceptance criteria:**
- [x] Linux, Windows, compatibility, release, and security gates pass on the exact candidate commit.
- [x] `main` is clean, pushed, and source CI succeeds.
- [x] Tag `v2.35.1` points to the candidate commit locally and on origin.
- [x] Release workflow succeeds for four targets and publishes a stable, non-draft, non-prerelease release.

### Task 3: Verify live assets and close out

**Files:**
- Create: `docs/release-evidence/2026-08-24-v2-35-1-release.md`
- Modify: `docs/release.md`
- Modify: `README.md`
- Modify: `docs/README.md`
- Modify: `docs/release-notes/README.md`
- Modify: `docs/release-notes/v2.35.1.md`
- Modify: `docs/site/index.html`
- Modify: `docs/plans/2026-08-24-julie-extract-2.35.1-release.md`
- Modify: `.memories/briefs/julie-extractors-v2-35-1-qml-release.md`
- Create: `.memories/<Goldfish checkpoint generated before commit>`

**Interfaces:**
- Consumes: live v2.35.1 release metadata and four downloadable archives.
- Produces: independently verified checksums/behavior, published-release pointers, a completed brief, and reconciled source control.

**Contract inputs:** `docs/release.md` closeout checks, embedded checksum format, release-note claims, and exact tag provenance.

**File ownership:** Create release evidence and checkpoint; modify published-release pointers in `docs/release.md`, `README.md`, `docs/README.md`, `docs/release-notes/README.md`, `docs/release-notes/v2.35.1.md`, `docs/site/index.html`, this plan, and Goldfish brief state.

**Serialization required:** Yes.

**Dependency reason:** Requires a successful stable release with downloadable assets.

**What to build:** Download all four archives, verify outer and embedded checksums, run the Windows binary version and QML scan/info smoke, record workflow/job URLs and asset digests, advance all current-release pointers, complete the plan and brief, commit the evidence-only closeout, push it, and reconcile `main`, origin, tag, and worktrees.

**Approach:** Keep generated archives and artifacts under ignored `target/`. Record the post-release documentation commit explicitly as an approved evidence-only follow-up whose source binaries remain the tagged candidate commit.

**Acceptance criteria:**
- [x] All four live archives and embedded checksums verify independently.
- [x] Downloaded Windows binary reports 2.35.1 and successfully extracts/inspects representative QML content.
- [x] Release evidence records exact source/tag/workflow/job URLs, sizes, digests, schemas, epoch, and consumer action.
- [x] All current-release pointers report v2.35.1 without contradicting tag provenance.
- [ ] Evidence-only closeout is committed and pushed; primary `main` is clean and all worktrees are reconciled or named.
