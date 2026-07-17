# Structural Fact Registry Module Split — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Split the ~5k-line `base/structural_fact_registry.rs` SPECS god-module into focused per-family modules with zero behavior change, so HTTP/DSL pattern work no longer requires shotgun edits in one file.

**Architecture:** Convert `structural_fact_registry.rs` into a directory module `base/structural_fact_registry/` whose `mod.rs` keeps the public types, `SPECS` concatenation (or `const` slice merge), serializers, and re-exports. Spec literals move into private family modules that mirror collector ownership. This follows the completed web-structural-facts and writer splits, including convention tests that pin module layout.

**Tech Stack:** Rust, existing structural-fact registry tests, checked-in `docs/contracts/structural-fact-patterns.json`.

**Architecture Quality:** Pure code motion. Public surface (`StructuralFactPatternSpec`, pattern lookup helpers, JSON sync test, CLI `languages --json` section) stays identical via re-exports. Risk is low-medium: large file move with many `const` references; golden and registry sync tests must stay byte-identical. If code reality contradicts this shape, workers report a plan mismatch rather than redesigning locally.

---

## Global Constraints

- Zero behavior change: registry JSON export byte-identical without `UPDATE_CONTRACT_JSON=1` unless a documented typo fix is lead-approved.
- No contract version bump; no capability changes; no emission changes.
- Default suite stays under the 90s tripwire.
- `AGENTS.md` / `CLAUDE.md` untouched.
- Do not merge family modules that collectors have already split differently without an explicit reason (prefer registry families ≈ collector families).

---

## Current Friction

- `crates/julie-extractors/src/base/structural_fact_registry.rs` ≈ 5000+ LOC after Symfony/Ktor/DSL pattern growth.
- Nearly every new `pattern_id` requires editing the same file as unrelated language builtins.
- Review finding: [`docs/findings/2026-07-17-project-review.md`](../findings/2026-07-17-project-review.md) §4 ranked this as the strongest architecture candidate.

**Deletion test:** After the split, callers still import `crate::base::structural_fact_registry::{...}` (or existing re-exports from `base/mod.rs`). No caller needs to know which family module owns a spec.

---

## Target File Structure

```text
crates/julie-extractors/src/base/structural_fact_registry/
  mod.rs                 # types, SPECS merge, public helpers, JSON sync hooks
  builtins.rs            # language-local builtins (rust.unsafe_block, go.goroutine, …)
  framework.rs           # HTTP/framework pattern SPECS (or thin re-exports of subfiles)
  web.rs                 # web/CSS/HTML/Vue/React/Next pattern SPECS
  data.rs                # json/yaml/toml/markdown/… data pattern SPECS
  sql.rs                 # sql.*.v1 pattern SPECS
  http_client.rs         # http.client_request.v1 (+ shared client metadata) if clearer as own file
```

Exact submodule boundaries may be adjusted if a family is tiny; do not create one-file-per-pattern. Prefer ≤8 submodules.

`mod.rs` builds:

```rust
const SPECS: &[StructuralFactPatternSpec] = &{
    // concatenate family slices — pick the idiomatic const approach already
    // used elsewhere in the crate (static once_cell, concat macros, or
    // explicit array of references flattened at runtime if const concat is awkward).
};
```

If const-slice concat is painful, an accepted alternative is `fn all_specs() -> &'static [StructuralFactPatternSpec]` backed by `OnceLock` — only if existing call sites already tolerate a function. Prefer keeping `SPECS` as a `const` if currently so.

---

## Task 1: Directory Module Skeleton + Move Types

**Files:**
- Create: `structural_fact_registry/mod.rs` (move types + helpers from current file)
- Create: empty family modules with `pub(super) const FAMILY_SPECS: &[StructuralFactPatternSpec] = &[];`
- Modify: `base/mod.rs` if path form changes (`mod structural_fact_registry;`)
- Delete: old single-file `structural_fact_registry.rs` after move

**Acceptance:**
- [x] Crate compiles
- [x] Existing ungated registry tests pass
- [x] No SPECS content moved yet beyond types/helpers (or move all SPECS into one `legacy.rs` temporarily)

---

## Task 2: Partition SPECS By Family

**Files:** family modules listed above

**Approach:** Move SPECS blocks in mechanical chunks matching `query_family` / pattern-id prefix. Keep alphabetical or existing order stable within each family so JSON export order stays identical (the sync test may require deterministic ordering — preserve the current serializer order contract).

**Acceptance:**
- [x] `cargo test -p julie-extractors structural_fact_registry` (ungated + gated as applicable) passes
- [x] `docs/contracts/structural-fact-patterns.json` byte-identical without regen
- [x] `cargo xtask test golden` green
- [x] Capability matrix unaffected

---

## Task 3: Convention Tests + Docs Pointer

**Files:**
- Modify: `crates/julie-extractors/src/tests/structural_fact_registry.rs` (or sibling) — assert module layout: `mod.rs` exists, no giant single file above a LOC ceiling (e.g. 800 lines per family module; `mod.rs` under 400)
- Modify: `docs/findings/2026-07-17-project-review.md` or architecture note — mark candidate completed when done

**Acceptance:**
- [x] Convention test fails if SPECS are re-collapsed into one file
- [x] `cargo fmt` + clippy clean for touched crate

---

## Verification (branch gate)

```bash
cargo test -p julie-extractors --lib structural_fact_registry
cargo xtask test golden
cargo xtask test capability
node scripts/language-data-quality-report.mjs --strict
cargo fmt --check
cargo clippy -p julie-extractors --all-targets --all-features -- -D warnings
```

---

## Out Of Scope

- Changing metadata keys, pattern ids, or emission
- Splitting collectors further (`framework_structural_facts` already modular)
- Miller consumer changes
- Version bumps / releases

---

## Recommended Timing

Execute after the 2026-07-17 review parity batches (test roles, Symfony/Ktor, DSL depth) have landed, so the SPECS set is stable for the move. Do not fold this split into small pattern-add PRs.
