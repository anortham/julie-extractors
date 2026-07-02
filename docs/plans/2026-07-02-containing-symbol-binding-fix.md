# Containing-Symbol Binding Fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Fix the two containing-symbol binding defects reported from Miller bridge integration (2026-07-02, both repro-confirmed against the shipped 2.6.0 binary):

1. `export const VERB = async () =>` Next.js route handlers emit `nextjs.route_handler.v1` facts with `containing_symbol_id = NULL` — the fact anchors on the export-statement head (`export const POST`, cols 0–17) while the TS extractor's `POST` symbol spans only the arrow-function value (col 20 → end), so strict byte containment finds nothing. `export async function VERB` binds only because function declarations also get a whole-statement `export`-kind symbol.
2. `const res = await fetch(...)` emits `http.client_request.v1` bound to the `res` variable symbol (the narrowest byte-containing symbol) instead of the enclosing function — useless for call-graph joining.

**Root cause:** every collector's `attach_containing_symbols` uses strict byte-range containment + narrowest-symbol-wins with no symbol-kind filtering. Six near-identical copies exist (`code_structural_facts.rs:1690`, `data_structural_facts.rs:1330`, `framework_structural_facts.rs:1754`, `sql_structural_facts.rs:969`, `structural_facts.rs:218`, `web_structural_facts/fact_builders.rs:84`).

**Architecture:** One shared binder in `base/` with the corrected semantics; all six collectors call it. The `source_regions.rs:159` copy is EXCLUDED on purpose: source regions (comments, string literals) legitimately attach to value-holder symbols (a doc comment or literal on a variable belongs to that variable); structural facts describe code actions and need scope-bearing anchors.

**Decided binder semantics (lead, strategy tier):**
- Candidate filter: exclude non-scope-bearing symbol kinds from containment candidacy. Denylist authored from the actual kind vocabulary (`base/kinds.rs` / symbol kinds observed in goldens): at minimum `variable`, `constant`, `field`, `property`, `enum_member`, `import`. `export` symbols remain candidates (they are the whole-statement containers that make function-declaration binding work). The Task 1 worker verifies the exact vocabulary and lists the final denylist in the report; additions/removals to the denylist are lead adjudications.
- Primary pass unchanged otherwise: narrowest byte-containing candidate wins.
- Fallback pass (new): when the primary pass finds no candidate, retry with line containment (`symbol.start_line <= fact.start_line && symbol.end_line >= fact.end_line`), same kind filter, narrowest line span wins; ties broken by narrowest byte span, then earliest start_byte (fully deterministic). Still nothing → `None` (unchanged).

**Contract impact:** emitted `containing_symbol_id` values change for shipped families → golden regeneration (containing_symbol_id-only diffs expected; any other field diff is a stop), `EXTRACTION_CONTRACT_VERSION` bump with marker `.containing-symbol-binding-v2`, `api_surface.rs` list update, and semantics prose in `docs/contracts/sqlite-schema-v3.md` / `jsonl-v3.md`. Pattern registry untouched (metadata keys unchanged). Release is a separate user decision.

## Global Constraints

- Zero changes to symbol extraction, fact gating, spans, or metadata — only which symbol a fact binds to.
- Golden regeneration diffs must be `containing_symbol_id` (and its JSONL counterpart) ONLY; anything else escalates to the lead.
- Default suite stays under the 90s tripwire.
- Strict data-quality report stays 0/0.

## Task 1: Shared Binder with Kind Filter + Line Fallback

**Files:**
- Create: `crates/julie-extractors/src/base/containing_symbol.rs` (shared `attach_containing_symbols(facts: &mut [StructuralFact], symbols: &[Symbol])` + the kind predicate; unit tests in-module)
- Modify: `crates/julie-extractors/src/base/mod.rs` (register; `pub(crate)` re-export)
- Modify: the six collector files to delete their local copies and call the shared binder (`code_structural_facts.rs`, `data_structural_facts.rs`, `framework_structural_facts.rs`, `sql_structural_facts.rs`, `structural_facts.rs`, `web_structural_facts/fact_builders.rs` + its `mod.rs` re-export if needed)
- Test: repro regression tests — a `nextjs.route_handler.v1` const-arrow handler binds its `function`-kind symbol (was NULL); a `http.client_request.v1` fetch assigned to a const binds the enclosing function (was the variable); a bare-call fetch still binds the enclosing function (lock). Place them with the existing nextjs/http_client tests (find with Miller, follow conventions).
- Do NOT touch `source_regions.rs`.

**TDD:** repro tests first (RED against current binder), then the shared binder to GREEN.

**Acceptance criteria:**
- [ ] Six collector copies replaced by one shared binder; `source_regions.rs` untouched.
- [ ] Both Miller repro cases fixed and locked by named tests; bare-call binding locked.
- [ ] Final kind denylist reported with vocabulary evidence.
- [ ] Worker-scope verification passes, committed.

## Task 2: Contract Sweep — Goldens, Marker, Docs

**Files:**
- Regenerate: golden fixtures under `fixtures/extraction/` (existing `UPDATE_GOLDEN` path)
- Modify: the `EXTRACTION_CONTRACT_VERSION` marker (find with Miller; bump with marker `.containing-symbol-binding-v2`) + `crates/julie-extractors/src/tests/api_surface.rs`
- Modify: `docs/contracts/sqlite-schema-v3.md`, `docs/contracts/jsonl-v3.md` (containing_symbol_id semantics prose: kind filter + line fallback)

**Acceptance criteria:**
- [ ] Golden diffs are containing_symbol_id-only (verified and stated with counts per language; anything else stops the task).
- [ ] Marker bumped, api_surface updated, docs state the new semantics.
- [ ] `node scripts/language-data-quality-report.mjs --strict` clean; golden suite green.
- [ ] Worker-scope verification passes, committed.

## Verification Strategy

**Worker red/green scope:** Task 1: the named repro tests + `cargo test -p julie-extractors structural_facts`; Task 2: `cargo test -p julie-extractors --features test-golden golden_fixtures_match_canonical_extraction -- --nocapture` + the strict report.

**Worker ceiling:** `cargo test -p julie-extractors`.

**Lead affected-change scope:** gated registry conformance (`--features test-golden structural_fact_registry`), capability matrix (`--features test-capability-matrix`), CLI repro re-run against the two scratch cases.

**Branch gate:** `cargo test --workspace`; `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings`; `cargo xtask test default`; `cargo xtask test contract`; `node scripts/language-data-quality-report.mjs --strict`.

**Escalation triggers:** any golden diff beyond containing_symbol_id; any denylist judgment beyond the authored list; any change in which facts are emitted.

**Assigned verification failure:** workers stop and report unless the plan explicitly says to update that gate (Task 2's golden regen is the sanctioned update path).

**Verification ledger:** invariant, command, scope label, commit SHA, result, timestamp per task.

## Model Routing

**Strategy tier:** binder semantics (decided above), denylist adjudications, golden-diff interpretation. **Implementation tier:** Tasks 1 and 2 mechanics. **Gate-interpretation reviewer:** lead. **Worker eligibility:** both tasks (decided interfaces, narrow ownership, explicit ceilings). **Unsupported harness behavior:** inherit.
