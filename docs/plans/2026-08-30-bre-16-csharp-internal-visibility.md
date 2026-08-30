# BRE-16 C# internal visibility implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Persist C# `internal` declarations as `Visibility::Internal` and remove the capability note that says they are stored as private.

**Architecture:** Correct the existing shared C# visibility helper. Keep the public enum, artifact schema, and `metadata_json.csharp_visibility` contract unchanged; prove the corrected value through normal extraction output.

**Tech Stack:** Rust, tree-sitter-c-sharp, Julie canonical fixtures, capability matrix.

**Architecture Quality:** Behavior-local change in `csharp::helpers::determine_visibility`; architecture risk is low and no new seam is introduced.

## Global Constraints

- Follow `docs/plans/2026-08-30-extractor-gap-closure-design.md` and Linear BRE-16.
- Do not change Miller reachability policy in this repository.
- Preserve existing defaults for declarations without an explicit visibility modifier.
- Keep `metadata_json.csharp_visibility` and the enum-backed artifact visibility column semantically aligned.
- Append `.csharp-visibility-v2` to `EXTRACTION_CONTRACT_VERSION` because canonical output changes.
- Golden diffs must contain only the intended visibility and contract evidence changes.
- `node scripts/language-data-quality-report.mjs --strict` must report `silent_cells = 0` and `quality_bar_debts = 0`.

---

## Verification Strategy

**Project source of truth:** `AGENTS.md`, `docs/testing-strategy.md`, `fixtures/extraction/capabilities.json`, and `docs/plans/2026-08-30-extractor-gap-closure-design.md`.

**Worker red/green scope:** Add focused assertions in `tests::csharp::core`, then run `cargo test -p julie-extractors tests::csharp::core -- --nocapture`. Run `cargo xtask test language csharp` after fixture updates.

**Worker ceiling:** `cargo xtask test language csharp`, `cargo xtask test golden`, and `cargo xtask test capability`.

**Worker gate invariant:** Explicit `internal` declarations emit `Visibility::Internal`; explicit private and default-private declarations do not change.

**Lead affected-change scope:** `cargo xtask test language csharp`; `cargo xtask test golden`; `cargo xtask test capability`; `node scripts/language-data-quality-report.mjs --strict`.

**Branch gate:** `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings`; `cargo test -p xtask`; `cargo xtask test default`; `cargo xtask test contract`; `git diff --check`.

**Security scope:** none declared.

**Replay/metric evidence:** The canonical C# fixture and focused unit assertions are hard gates. The historical estimate of recovered Miller call sites is report-only.

**Escalation triggers:** Any change outside C# visibility values, capability prose, or the extraction contract marker requires lead review before acceptance.

**Assigned verification failure:** Workers stop and report when assigned verification fails, unless this plan explicitly says to update that gate.

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, and timestamp. Reuse a passing entry for the same HEAD and scope.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: Correct and prove C# internal visibility | None - serial | `crates/julie-extractors/src/csharp/helpers.rs`, `crates/julie-extractors/src/tests/csharp/core.rs`, `fixtures/extraction/csharp/basic/**`, `fixtures/extraction/capabilities.json`, `docs/languages/csharp.md`, `crates/julie-extractors/src/lib.rs`, `crates/julie-extractors/src/tests/api_surface.rs` | Not applicable - single task. | Not applicable - single task. |

### Task 1: Correct and prove C# internal visibility

**Files:**
- Modify: `crates/julie-extractors/src/csharp/helpers.rs:45-66`
- Modify: `crates/julie-extractors/src/tests/csharp/core.rs:55-716`
- Modify: `fixtures/extraction/csharp/basic/source.cs`
- Modify: `fixtures/extraction/csharp/basic/expected.json`
- Modify: `fixtures/extraction/capabilities.json`
- Modify: `docs/languages/csharp.md`
- Modify: `crates/julie-extractors/src/lib.rs:130`
- Modify: `crates/julie-extractors/src/tests/api_surface.rs:14-51`

**Interfaces:**
- Consumes: `determine_visibility(modifiers: &[String], node_type: Option<&str>) -> Visibility`, existing C# metadata emission, and the canonical fixture contract.
- Produces: `Visibility::Internal` for explicit `internal`, plus extraction contract marker `csharp-visibility-v2`.

**Contract inputs:** BRE-16 acceptance criteria; the existing `Visibility::Internal` enum variant and serialized `internal` spelling; unchanged C# default-visibility rules.

**File ownership:** `crates/julie-extractors/src/csharp/helpers.rs`, `crates/julie-extractors/src/tests/csharp/core.rs`, `fixtures/extraction/csharp/basic/**`, `fixtures/extraction/capabilities.json`, `docs/languages/csharp.md`, `crates/julie-extractors/src/lib.rs`, `crates/julie-extractors/src/tests/api_surface.rs`

**Serialization required:** Not applicable - single task.

**Dependency reason:** Not applicable - single task.

**What to build:** Change the `internal` branch in `determine_visibility` from `Visibility::Private` to `Visibility::Internal`. Expand unit and golden coverage across a type and representative member kinds so every shared-helper path is proven while default and explicit private controls stay fixed.

**Approach:** Write failing assertions first. Add `internal` and private controls to the canonical C# fixture, regenerate the golden once, inspect the diff, update capability and language documentation, then append the contract marker and its API-surface assertion. Remove the obsolete narration comment on the changed helper line.

**Acceptance criteria:**
- [ ] Focused tests fail before the helper correction and pass afterward.
- [ ] Internal types, methods, properties, fields, and constructors covered by the shared helper persist `visibility = "internal"`.
- [ ] Explicit private and default-private controls retain `visibility = "private"`.
- [ ] `metadata_json.csharp_visibility` agrees with the enum-backed visibility column.
- [ ] `fixtures/extraction/capabilities.json` and `docs/languages/csharp.md` no longer claim that internal maps to private.
- [ ] `EXTRACTION_CONTRACT_VERSION` contains `csharp-visibility-v2` and its API-surface test passes.
- [ ] Golden, capability, strict data-quality, affected-change, and branch gates pass.
- [ ] Worker-scope verification passes and the change is committed per `serial-worker-commit`.
