> **Superseded 2026-08-18.** The resolution write path is retired. See [2026-08-18-resolution-write-path-retirement.md](../decisions/2026-08-18-resolution-write-path-retirement.md).

# Static-tier TS/JS certification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Certify TypeScript and JavaScript for `tier3_static_type` resolution by emitting the facts the tier requires (export-aware visibility, normalized static reachability) and adding fixture-proven language allowlist entries, without regressing C# static resolution.

**Architecture:** Keep the existing static-type receiver path in `crates/julie-extract-cli/src/resolution.rs`. Extend `CandidateSymbol` so static reachability can be proven from symbol metadata (`isStatic`) with signature-scan fallback for languages that only put `static` in the presentation signature. TypeScript records `Visibility::Public` for exported type declarations so cross-file static receivers can bind. JavaScript already defaults class visibility to public and already includes `static` in method signatures — it primarily needs fixtures and allowlist membership. Bump `RESOLUTION_VERSION` because resolution outcomes expand.

**Tech Stack:** Rust workspace, tree-sitter TypeScript/JavaScript/C#, SQLite artifact, resolution_contract fixtures, golden fixtures, cargo nextest / xtask.

**Architecture Quality:** Medium risk. Touches the resolution tier contract, symbol metadata shape (`isStatic`), TypeScript visibility semantics, and the public language allowlist. Must preserve C# precision refusals (non-public, nested, non-static, external homonyms). Do not invent a new symbol table column; load `isStatic` from existing `symbols.metadata_json`. Do not widen tier 4. Do not map all top-level TS types to public — only exported ones.

## Global Constraints

- Work only on branch `feature/static-tier-ts-js` in worktree `~/.config/razorback/worktrees/julie-extractors/static-tier-ts-js`.
- Product boundary: extraction artifact only — no Miller/MCP/search behavior.
- `RESOLUTION_VERSION` must bump from `3` → `4` in this plan (resolution outcomes expand for TS/JS).
- Metadata key for static reachability is exactly `isStatic` (boolean), matching existing TS/JS emission.
- Cross-file static type reachability still requires `visibility == "public"` in SQLite storage form (`Visibility::as_storage_str()`).
- `TIER3_STATIC_TYPE_LANGUAGES` may only include a language that has `fixtures/extraction/resolution_contract/<lang>/static_type_receiver/`.
- Precision rule: a wrong edge is worse than a missing one. Negative fixtures required for non-static members, non-exported TS types, and external-looking receivers.
- C# static-tier rates and refusal tests must remain green.
- Capability claims need golden/fixture evidence; run `node scripts/language-data-quality-report.mjs --strict` and `node scripts/reference-resolution-coverage-report.mjs --strict` after capability/coverage changes.
- Default suite must stay fast; resolution_contract tests already live in the CLI suite.

## Verification Strategy

**Project source of truth:** `AGENTS.md` / `CLAUDE.md`, `xtask` test tiers, `docs/contracts/sqlite-schema-v4.md` / v5 for resolution metadata, `TODO.md` §16 for residual resolution debt.

**Worker red/green scope:**
```bash
cargo test -p julie-extract-cli --test resolution_contract <focused_test_name>
cargo test -p julie-extract-cli resolution::tests::<focused_unit>
cargo nextest run -p julie-extractors --features test-golden -E 'test(typescript) | test(javascript) | test(csharp)'  # when extractor goldens change
```

**Worker ceiling:** `cargo xtask test default` (workers do not own contract/real-world/certification alone unless lead requests).

**Worker gate invariant:** Every new positive static-type binding is fixture-proven; every allowlisted language has `static_type_receiver`; C# unit refusal tests still pass; no silent capability cells.

**Lead affected-change scope:**
```bash
cargo xtask test language typescript
cargo xtask test language javascript
cargo xtask test language csharp
cargo test -p julie-extract-cli --test resolution_contract
cargo test -p julie-extractors --features test-golden golden  # if goldens regenerated
```

**Branch gate:**
```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo xtask test default
cargo xtask test contract
node scripts/language-data-quality-report.mjs --strict
node scripts/reference-resolution-coverage-report.mjs --strict
scripts/check-agent-doc-sync.sh
```

**Replay/metric evidence:** C# corpus re-measure is report-only after this slice if a Miller workspace is available; hard gate is fixture + unit tests. Report zero wrong edges on C# static refusal suite.

**Escalation triggers:** Resolution version / schema contract doc drift; golden mass-regeneration beyond intended languages; unexpected C# yield drop; proposal to parse new signature formats instead of metadata.

**Assigned verification failure:** Workers stop and report; do not weaken tests or mark gaps closed without fixtures.

**Verification ledger:** Record invariant, command, scope, commit SHA, result, timestamp per completed task batch.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: Normalized static reachability in resolver | Batch A | `crates/julie-extract-cli/src/resolution.rs`; unit tests in same file | No | None - safe parallel batch. Metadata key contract fixed in Global Constraints. |
| Task 2: C# emit `isStatic` metadata | Batch A | `crates/julie-extractors/src/csharp/members.rs`, `fields.rs`, related C# helpers/tests/goldens if required | No | None - safe parallel batch. Independent of TS/JS extractors. |
| Task 3: TS export-aware visibility + static signature | Batch A | `crates/julie-extractors/src/typescript/**`, TS tests/goldens | No | None - safe parallel batch. |
| Task 4: JS/TS resolution fixtures + allowlist + integration tests | Batch B | `fixtures/extraction/resolution_contract/{javascript,typescript}/**`, `crates/julie-extract-cli/tests/resolution_contract.rs`, `crates/julie-extract-cli/src/resolution.rs` allowlist only, `crates/julie-extract-cli/src/capability_snapshot.rs` only if comments/docs require, docs under `docs/release-notes/` deferred | Yes | Depends on Tasks 1–3 facts being present so fixtures can resolve. |
| Task 5: Capability hygiene + resolution docs + coverage regen | Batch C | `fixtures/extraction/capabilities.json`, `fixtures/extraction/reference-resolution-coverage.json`, resolution docs in `docs/contracts/*`, `TODO.md` note, optional capability matrix claims | Yes | Depends on Task 4 allowlist and fixture digests. |

**Commit mode:** `parallel-lead-commit` for Batch A; `serial-worker-commit` acceptable for Batch B/C if single-threaded, otherwise lead-commit after review.

---

### Task 1: Normalized static reachability in the resolver

**Files:**
- Modify: `crates/julie-extract-cli/src/resolution.rs` (`CandidateSymbol`, symbol load SQL, `is_statically_reachable`, unit tests)
- Modify: `RESOLUTION_VERSION` 3 → 4 in the same file

**Interfaces:**
- Consumes: `symbols.metadata_json` field already in SQLite; key `isStatic` boolean
- Produces: `CandidateSymbol.is_static: Option<bool>`; `is_statically_reachable` true when `is_static == Some(true)` OR (legacy) signature contains standalone `static` in modifier prefix; false when `is_static == Some(false)`; enum/constant/enum-member still always reachable

**Contract inputs:** Global Constraints metadata key `isStatic`; existing `contains_static_modifier` fallback must remain for C# until Task 2 lands and after as belt-and-suspenders.

**File ownership:** `crates/julie-extract-cli/src/resolution.rs`; unit tests in same file

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** Load `metadata_json` when building the candidate index; parse `isStatic` when present. Prefer explicit metadata over signature scanning. Keep signature fallback for rows without metadata. Bump `RESOLUTION_VERSION` to 4.

**Approach:**
- Extend `CandidateSymbol` with `is_static: Option<bool>` (default None).
- Change the symbols SELECT to also pull `metadata_json` and parse `isStatic` with serde_json (ignore non-bool).
- Update `is_statically_reachable`:
  1. EnumMember / Constant / Enum → true
  2. `is_static == Some(true)` → true
  3. `is_static == Some(false)` → false
  4. else → existing `contains_static_modifier(signature)`
- Unit tests: metadata-true without "static" in signature binds; metadata-false refuses even if signature lies; C# signature-only path still binds.
- Update any test helpers constructing `CandidateSymbol` to set `is_static: None`.

**Acceptance criteria:**
- [x] `is_statically_reachable` honors `isStatic` metadata with signature fallback
- [x] `RESOLUTION_VERSION == 4`
- [x] Existing C# static unit tests in `resolution.rs` pass
- [x] New unit tests cover metadata true/false and signature fallback
- [x] Change handed to lead for commit (parallel-lead-commit)

---

### Task 2: C# emits `isStatic` metadata

**Files:**
- Modify: `crates/julie-extractors/src/csharp/members.rs` (methods, constructors if relevant)
- Modify: `crates/julie-extractors/src/csharp/fields.rs` (static fields/constants as applicable)
- Modify: `crates/julie-extractors/src/csharp/operators.rs` / property extractors if they can be static
- Test: focused C# extractor tests and/or golden updates under `fixtures/extraction/csharp/` only if goldens assert full symbol metadata

**Interfaces:**
- Consumes: existing `helpers::extract_modifiers` which already sees `"static"`
- Produces: method/field/property symbols with `metadata["isStatic"] = true|false` (or true-only if false omission is the local convention — prefer always set boolean for methods to match TS)

**Contract inputs:** Key name `isStatic` boolean.

**File ownership:** C# extractor modules listed above + C# tests/goldens if required

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** When building C# member symbols, record `isStatic` from modifiers so the resolver does not depend only on presentation signatures. Keep existing signature text that already includes `static`.

**Approach:** After `extract_modifiers`, set `metadata.insert("isStatic", json!(modifiers.iter().any(|m| m == "static")))`. Mirror for fields (const/static). Avoid changing kind inventory claims in this task.

**Acceptance criteria:**
- [x] Static C# methods/fields carry `isStatic: true` in extraction results
- [x] Instance methods carry `isStatic: false` or omit only if proven equivalent for resolver (`Some(false)` path preferred)
- [x] C# language tests pass; goldens updated only if they freeze metadata
- [x] Handed to lead for commit

---

### Task 3: TypeScript export-aware visibility + static in method signatures

**Files:**
- Modify: `crates/julie-extractors/src/typescript/helpers.rs` (export detection + visibility helper)
- Modify: `crates/julie-extractors/src/typescript/classes.rs`, `interfaces.rs`, `functions.rs` (and enums if separate)
- Modify: `build_function_signature` / method signature path to include `static ` when `has_modifier(node, "static")`
- Test: `crates/julie-extractors/src/tests/typescript/**`
- Possibly regenerate: `fixtures/extraction/typescript/**/expected.json`, `tsx/**` if shared

**Interfaces:**
- Consumes: tree-sitter parent `export_statement`, accessibility modifiers, existing `has_modifier` / `isStatic` metadata
- Produces: exported classes/interfaces/enums/functions/types with `visibility = Public` when exported and no stronger private/protected modifier; method signatures that include standalone `static` for static methods

**Contract inputs:** Cross-file static tier requires storage visibility `"public"`. Non-exported types must remain non-public (None/Private) so cross-file static binding refuses.

**File ownership:** TypeScript extractor tree + TS tests/goldens

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:**
1. Helper `is_exported_declaration(node) -> bool` — true when parent (or relevant wrapper) is `export_statement`, including `export default`.
2. Visibility resolution: accessibility_modifier wins; else if exported → `Public`; else `None` (unchanged).
3. Apply to class, interface, enum, type-alias, and function declarations as appropriate for type-name receivers.
4. Prepend `static ` to method signatures when static (metadata already sets `isStatic`).

**Approach:** Prefer parent-walk one level (and export wrapper used elsewhere for decorators). Do not mark non-exported file-local classes public. Update goldens with UPDATE_GOLDEN only after intentional behavior change.

**Acceptance criteria:**
- [x] `export class Foo` yields visibility public in extraction
- [x] Non-exported class remains non-public
- [x] Static methods have `isStatic: true` and signature containing standalone `static`
- [x] TS language tests + affected goldens pass
- [x] Handed to lead for commit

---

### Task 4: JS/TS static_type_receiver fixtures, allowlist, integration tests

**Files:**
- Create: `fixtures/extraction/resolution_contract/typescript/static_type_receiver/{fixture.ts,consumer.ts}`
- Create: `fixtures/extraction/resolution_contract/javascript/static_type_receiver/{fixture.js,consumer.js}`
- Create (negatives, can share folders or sibling dirs): non-exported TS type cross-file refuse; instance member refuse
- Modify: `crates/julie-extract-cli/src/resolution.rs` — `TIER3_STATIC_TYPE_LANGUAGES = &["csharp", "typescript", "javascript"]`
- Modify: `crates/julie-extract-cli/tests/resolution_contract.rs` — tests mirroring C# static_type_receiver for TS/JS; optional negative tests

**Interfaces:**
- Consumes: Tasks 1–3 extractor + resolver behavior
- Produces: fixture-proven static bindings for TS/JS; allowlist membership; parity guard still green

**Contract inputs:** Fixture directory name must be exactly `static_type_receiver` (enforced by `every_static_type_language_ships_a_proving_fixture`). Positive cases: static method call + static field/const-like if language has it. Confidence band same as C# (between tier4 and concrete tier3).

**File ownership:** resolution_contract fixtures for ts/js; resolution allowlist; resolution_contract tests

**Serialization required:** Yes

**Dependency reason:** Depends on Tasks 1–3 facts being present so fixtures can resolve.

**What to build:**
- TS fixture: exported class with static method; exported enum or static field; consumer in another file calls `Fixture.create()` / reads static member; expect `tier3_static_type`.
- TS negative: non-exported class with static method referenced cross-file → unresolved.
- JS fixture: exported class with static method; consumer resolves.
- JS negative: instance method via type name → unresolved.
- Integration tests analogous to `static_type_receiver_resolves_csharp_across_files`.
- Expand allowlist only after fixtures exist and tests pass.

**Approach:** Copy structure from `csharp/static_type_receiver`. Use ESM `export` for JS so export surface is clear. Prefer simple names without namespace complexity for first fixtures.

**Acceptance criteria:**
- [x] TS and JS static_type_receiver fixtures resolve positive cases at method `tier3_static_type`
- [x] Negative cases do not resolve
- [x] `TIER3_STATIC_TYPE_LANGUAGES` includes csharp, typescript, javascript
- [x] `every_static_type_language_ships_a_proving_fixture` and `per_language_tier_parity_guard` pass
- [x] C# static integration tests still pass
- [x] Verified and committed (or lead-committed)

---

### Task 5: Capability hygiene, docs, coverage artifact

**Files:**
- Modify: `fixtures/extraction/capabilities.json` — `reference_resolution.tiers.tier3_static_type.fixture_proven_languages`; visibility/static notes; optional kind_coverage hygiene for csharp/ts/js observed kinds
- Modify: `fixtures/extraction/reference-resolution-coverage.json` via report `--write`
- Modify: resolution contract docs (`docs/contracts/sqlite-schema-v4.md` and/or current schema docs that mention resolution version 3)
- Modify: `TODO.md` §16 — note TS/JS static certification progress; C# locals still open
- Optional: short decision note only if policy changes (prefer updating existing capabilities comment block)

**Interfaces:**
- Consumes: Task 4 allowlist and fixtures
- Produces: honest capability ledger + strict gates green

**Contract inputs:** Do not claim TS/JS static tier without fixture_proven_languages update. Runtime gap emission already uses `tier3_static_type_proven`.

**File ownership:** capabilities, coverage JSON, contract docs, TODO.md

**Serialization required:** Yes

**Dependency reason:** Depends on Task 4 allowlist and fixture digests.

**What to build:** Sync documentation and capability surfaces with the new fixture-proven languages. Regenerate reference-resolution coverage with `--write --strict`. Hygiene-only kind_coverage claims for kinds already present in goldens (e.g. csharp `member_access`) if cheap and evidence-backed.

**Approach:** Prefer minimal doc edits: version number, fixture_proven list, static-modifier note that TS/JS are now proven. Avoid rewriting historical release notes; add forward notes only if a release note draft is in scope (out of scope unless releasing).

**Acceptance criteria:**
- [x] `capabilities.json` fixture_proven_languages includes csharp, typescript, javascript
- [x] `node scripts/language-data-quality-report.mjs --strict` → silent_cells=0, quality_bar_debts=0
- [x] `node scripts/reference-resolution-coverage-report.mjs --strict` passes
- [x] TODO §16 updated: static-tier multi-language done; slice 4 locals still open
- [x] Branch gate commands pass

---

## Out of scope (explicit)

- C# locals/params as symbols and real `infer_variable_type` (R2 / TODO slice 4)
- C# `internal` assembly-visible resolution
- C# tier-2 `using` import resolution
- Next.js signal-free pages-router open_gap
- Adding `tsx`/`jsx` to `TIER3_STATIC_TYPE_LANGUAGES` unless fixtures prove them (registry languages may still resolve if facts present; certification allowlist can stay on base languages)
- Schema version bump for a new `is_static` column

## Execution notes

- Worktree: `~/.config/razorback/worktrees/julie-extractors/static-tier-ts-js`
- Branch: `feature/static-tier-ts-js` @ base `02a207d3`
- Prefer Miller for orientation before edits; TDD for resolver and fixture tests
- After all tasks: lead runs branch gate, then `finishing-a-development-branch` only with user approval for push/PR/release
