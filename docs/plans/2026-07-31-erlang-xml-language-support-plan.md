# Erlang + XML Language Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Add Erlang (`FULL_CAPABILITIES`, elixir-parity) and XML (`DATA_ONLY_CAPABILITIES`) as supported languages 37 and 38, plus an oversized-file transition fix, per `docs/plans/2026-07-31-erlang-xml-language-support-design.md`.

**Architecture:** Existing per-language seam, medium contract impact. Each language: grammar dep + `LanguageSpec` row + extractor module + registry dispatch + capability matrix row + golden fixtures + focused tests, per `docs/languages/new-language-checklist.md`. Erlang models `src/elixir/`; XML is a hybrid of `src/yaml/` (parent-chain nesting) and `src/html/elements.rs` (element filtering).

**Tech Stack:** Rust, tree-sitter `=0.26.11`, `tree-sitter-erlang 0.20.0` (crates.io, verified live 2026-07-31), `tree-sitter-xml 0.7.0`.

**Architecture Quality:** Approved shape recorded in the design doc (revised after Codex adversarial review, verdict SHIP WITH CHANGES). Main risk: `tree-sitter-xml` (2024 crate) ABI compatibility with tree-sitter 0.26.11 — Task 1 is the risk-first spike. If code reality contradicts the approved shape, workers report a plan mismatch rather than redesigning locally.

## Global Constraints

- **Toolchain:** the machine's `stable` rustup toolchain is 1.94.0 but the workspace requires rustc ≥1.95. Prefix every cargo command with `RUSTUP_TOOLCHAIN=1.97.1`. Do not change the global default.
- **Worktree:** all work happens in `.worktrees/erlang-xml-language-support` on branch `erlang-xml-language-support`. Every worker verifies `pwd` + branch before editing.
- **Capability honesty:** a capability flag in `fixtures/extraction/capabilities.json` is true only when useful rows are emitted AND fixture-verified. Erlang's `target_capabilities` is FULL from Task 2; `capabilities` ratchets up per task. Never set a flag true because a vector is nonempty.
- **`MAX_SOURCE_FILE_BYTES` stays 1MB** (`crates/julie-extract-cli/src/limits.rs:8`). No per-language override.
- **Language counts:** hard-coded 36-language assertions live at `crates/julie-extractors/src/registry.rs:676` (`supported_languages().len()`), `crates/julie-extractors/src/factory.rs:60-61`, `crates/julie-extractors/src/tests/capability_snapshot_test.rs:8`. Task 2 bumps them to 37; Task 3 to 38.
- **Contract version:** new downstream-visible outputs require reviewing `EXTRACTION_CONTRACT_VERSION` (`crates/julie-extractors/src/lib.rs:121`); the changed-path guard (`xtask/src/test_tiers.rs:385`) enforces the review. Adding languages within existing row domains is expected to be non-breaking — record the review conclusion in the task's commit message, bump only if a contract doc says so.
- **No new MCP/CLI surfaces.** This plan only extends extraction coverage.
- **Design doc is the spec:** `docs/plans/2026-07-31-erlang-xml-language-support-design.md`. Erlang coverage list and XML symbol/identifier rules there are binding acceptance criteria.

## Verification Strategy

**Project source of truth:** `docs/testing-strategy.md` + `docs/languages/new-language-checklist.md` §7.

**Worker red/green scope:** `RUSTUP_TOOLCHAIN=1.97.1 cargo xtask test language <erlang|xml>` plus the focused test files the task adds. For golden-fixture tasks add `cargo xtask test golden`. For CLI tasks: `RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-cli <named tests>`.

**Worker ceiling:** `cargo xtask test language <lang>` + `cargo xtask test golden` + `cargo xtask test capability`. Workers do not run certification, real-world, or default-wide tiers on their own.

**Worker gate invariant:** each task's gate proves its acceptance criteria rows exist with asserted values (no smoke-only tests, no no-assertion tests — repo rule).

**Lead affected-change scope:** `RUSTUP_TOOLCHAIN=1.97.1 cargo xtask test changed <touched paths>` after each merged batch (this triggers the contract-version guard when `specs.rs`/`capabilities.json` change).

**Branch gate (before handoff/PR):** `cargo xtask test default` + `golden` + `capability` + `cargo run -p julie-extract-cli -- languages --json` (inspect erlang/xml rows) + `node scripts/language-data-quality-report.mjs --strict` + `cargo fmt --check` + `cargo clippy` + `cargo deny check`.

**Expensive tiers:** `cargo xtask test certification` is REQUIRED (parser dependencies change in Task 1). The Erlang real-world corpus gate is Task 8's own harness, not the `real-world-smoke` tier.

**Replay/metric evidence:** Task 8's corpus baseline (file counts, symbol counts, diagnostic counts per file) is a hard gate once committed; scan wall-time is report-only.

**Escalation triggers:** any `Cargo.toml`/grammar change → certification; any `specs.rs`/`capabilities.json`/registry change → `cargo xtask test changed` on those paths; `commands.rs` update-path change → full `julie-extract-cli` test suite.

**Assigned verification failure:** workers stop and report when assigned verification fails, unless this plan explicitly says to update that gate.

**Verification ledger:** record invariant, command, scope label, commit SHA, result, timestamp in the task report. Reuse passing evidence for the same HEAD instead of rerunning expensive gates.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: Grammar spike | None - serial | Modify: `crates/julie-extractors/Cargo.toml`, `Cargo.lock`; Create: `crates/julie-extractors/src/tests/grammar_smoke.rs` (temporary), test-mod wiring | Yes | First task; proves the risk-first bet before anything else builds on the deps. |
| Task 2: Erlang registration + symbols | None - serial | Create: `crates/julie-extractors/src/erlang/*`, `crates/julie-extractors/src/tests/erlang/*`, `fixtures/extraction/erlang/basic/*`; Modify: `language_spec/specs.rs`, `language_spec/mod.rs`, `lib.rs`, `registry.rs`, `factory.rs`, `tests/capability_snapshot_test.rs`, `fixtures/extraction/capabilities.json` | Yes | Depends on Task 1 deps; owns shared registration files. |
| Task 3: XML registration + symbols + identifiers | None - serial | Create: `crates/julie-extractors/src/xml/*`, `crates/julie-extractors/src/tests/xml/*`, `fixtures/extraction/xml/{basic,xsd,wsdl,cardinality}/*`; Modify: same shared registration files as Task 2 | Yes | Conflicts with Task 2 on `specs.rs`, `registry.rs`, `lib.rs`, `factory.rs`, `capabilities.json`, count assertions. |
| Task 4: Erlang identifiers + calls | Batch A | Modify: `crates/julie-extractors/src/erlang/*`, `crates/julie-extractors/src/tests/erlang/*`, `fixtures/extraction/erlang/*`, erlang row of `fixtures/extraction/capabilities.json` | No | None - safe parallel batch. (Disjoint from Task 5's crate.) |
| Task 5: Oversized-transition policy | Batch A | Modify: `crates/julie-extract-cli/src/commands.rs`, `crates/julie-extract-cli/src/limits.rs` (comment only if needed), `crates/julie-extract-cli/tests/operations_contract.rs` | No | None - safe parallel batch. (Touches only `julie-extract-cli`; no shared files with Task 4.) |
| Task 6: Erlang relationships + pending | None - serial | Modify: `crates/julie-extractors/src/erlang/*`, `crates/julie-extractors/src/tests/erlang/*`; Create: `fixtures/extraction/erlang/cross_file/*`; Modify erlang row of `capabilities.json` | Yes | Same erlang files and capabilities row as Task 4; runs after Batch A. |
| Task 7: Erlang types + test roles → FULL | None - serial | Modify: `crates/julie-extractors/src/erlang/*`, `crates/julie-extractors/src/tests/erlang/*`; Create: `fixtures/extraction/erlang/test_roles/*`; Modify erlang row of `capabilities.json` | Yes | Sequential ratchet on the same erlang module and capabilities row. |
| Task 8: Erlang real-world corpus gate | None - serial | Create: `fixtures/real-world/erlang/**` (corpus + checksums + LICENSE notices), `crates/julie-extract-cli/tests/erlang_corpus.rs` | Yes | Needs Tasks 2/4/6/7 complete — the baseline asserts final extractor behavior. |
| Task 9: XML schema/WSDL structural facts | None - serial | Modify: `crates/julie-extractors/src/xml/*`, `crates/julie-extractors/src/base/data_structural_facts.rs`, `crates/julie-extractors/src/base/structural_fact_registry/*`, `docs/contracts/structural-fact-patterns.json` (regenerated), xml row of `capabilities.json`, `fixtures/extraction/xml/*` | Yes | Structural-fact registry files are shared repo-wide; runs alone after Task 3. |
| Task 10: Branch gates + repo docs | None - serial | Modify: `README.md`, any files fixed by gate failures | Yes | Final gate over everything; must run last. |
| Task 11: Cheap quality-bar debts | None - serial | Modify: erlang + xml extractor/test/fixture trees, both `capabilities.json` rows, migration-plan doc, corpus baseline | Yes | Added 2026-08-01 (user decision to close strict-gate debts on-branch); shares erlang goldens/capabilities/migration doc with Task 12. |
| Task 12: Erlang structural facts + strict green | None - serial | Create/Modify: structural_fact_registry erlang module + wiring, erlang extractor/fixture trees, contract JSON, erlang `capabilities.json` row, migration-plan doc, corpus baseline | Yes | Runs after Task 11 so the strict-gate exit criterion covers both closures. |

Commit mode: `serial-worker-commit` for serial tasks; `parallel-lead-commit` for Batch A (Tasks 4 and 5) — Batch A workers hand verified diffs to the lead.

Post-merge delivery chain (release 2.21.0 → Miller pin bump → Miller/site docs 36→38 → issue #8 reply) is **out of this plan's task list** — it runs in the lead session after merge, gated on explicit user approval per the design doc.

---

### Task 1: Grammar spike (risk-first)

**Files:**
- Modify: `crates/julie-extractors/Cargo.toml` (dep block near `tree-sitter = "=0.26.11"`, line 29), `Cargo.lock`
- Create: `crates/julie-extractors/src/tests/grammar_smoke.rs` + test-mod registration in `crates/julie-extractors/src/tests/` mod file

**Interfaces:**
- Consumes: nothing.
- Produces: `tree-sitter-erlang = "=0.20.0"` and `tree-sitter-xml = "=0.7.0"` as workspace deps; proof both load under runtime 0.26.11. Note: `tree-sitter-xml` exposes TWO grammars (XML and DTD) — record which entry point the XML extractor should use.

**Contract inputs:** design doc §Phase-0; `cargo deny check` must stay green (new deps).

**File ownership:** Modify: `crates/julie-extractors/Cargo.toml`, `Cargo.lock`; Create: `crates/julie-extractors/src/tests/grammar_smoke.rs` (temporary), test-mod wiring

**Serialization required:** Yes

**Dependency reason:** First task; proves the risk-first bet before anything else builds on the deps.

**What to build:** Add both grammar crates pinned exact. Write two smoke tests: parse a small `.erl` snippet (module + exported function + record) and a small XML snippet (nested elements + attributes); assert the root node kind and zero ERROR nodes. This is the Phase-0 gate from the design — if `tree-sitter-xml` fails to load under 0.26.11, STOP and report (vendoring/fork decision goes back to the user).

**Approach:** Follow how existing grammar deps are declared (exact-pin style in `Cargo.toml`). Smoke tests are temporary scaffolding — Tasks 2/3 replace them with real extractor tests; leave a note in the file saying so.

**Acceptance criteria:**
- [x] Both crates resolve, compile, and parse their smoke snippets with zero ERROR nodes under `RUSTUP_TOOLCHAIN=1.97.1`
- [x] `cargo deny check` passes with the new deps
- [x] XML grammar entry point (XML vs DTD) recorded in the task report
- [x] Worker-scope verification passes and the change is committed (serial-worker-commit)

### Task 2: Erlang registration + symbols

**Files:**
- Create: `crates/julie-extractors/src/erlang/` (mod.rs + helpers/attributes/definition_forms modules, modeled on `src/elixir/`), `crates/julie-extractors/src/tests/erlang/` (mod + symbol/doc/visibility/parse-error tests), `fixtures/extraction/erlang/basic/{source.erl,expected.json}`
- Modify: `crates/julie-extractors/src/language_spec/specs.rs` (new `spec("erlang", &["erl", "hrl"], "tree-sitter-erlang", …)` row), `crates/julie-extractors/src/language_spec/mod.rs` (parser fn, following `parser_elixir`), `crates/julie-extractors/src/lib.rs` (`pub mod erlang;`), `crates/julie-extractors/src/registry.rs` (dispatch entry per the `(extract_elixir, "elixir", crate::elixir::ElixirExtractor)` pattern at :185 and table at :528), `crates/julie-extractors/src/factory.rs:60-61` (36→37), `crates/julie-extractors/src/tests/capability_snapshot_test.rs:8` (36→37), `crates/julie-extractors/src/registry.rs:676` (36→37), `fixtures/extraction/capabilities.json` (new erlang row)

**Interfaces:**
- Consumes: Task 1's deps and Erlang grammar entry point.
- Produces: `ErlangExtractor` registered for `.erl`/`.hrl`; symbols: modules, functions grouped by name/arity across clauses (one symbol per name/arity, signature from the first clause head), records, macros (`-define`), types (`-type`/`-opaque`), behaviour callbacks. Visibility: exported iff in `-export`/`-export_type`, or `-compile(export_all)` present. Doc comments: EDoc `%% @doc` blocks and OTP `-doc`/`-moduledoc` attributes attached to functions, types, and callbacks. `.hrl` files extract standalone (records/macros/types) without failing. Parse errors degrade gracefully: emit what parses + `parse_diagnostics` rows.
- Capabilities row: `target_capabilities` FULL; `capabilities` symbols=true only; `kind_coverage.symbols` + `kind_coverage.body_spans` filled; other domains as honest `capability_gaps`/pending entries per matrix rules.

**Contract inputs:** design doc §Erlang; `docs/languages/new-language-checklist.md` §§1-6; LanguageSpec row must keep extensions/parser_crate identical to the capabilities.json row; capability flags declared in `LanguageSpec` must match the matrix row (spec row starts at the honest current tier — see checklist §1, and ratchet it alongside the matrix in Tasks 4/6/7).

**File ownership:** Create: `crates/julie-extractors/src/erlang/*`, `crates/julie-extractors/src/tests/erlang/*`, `fixtures/extraction/erlang/basic/*`; Modify: `language_spec/specs.rs`, `language_spec/mod.rs`, `lib.rs`, `registry.rs`, `factory.rs`, `tests/capability_snapshot_test.rs`, `fixtures/extraction/capabilities.json`

**Serialization required:** Yes

**Dependency reason:** Depends on Task 1 deps; owns shared registration files.

**What to build:** The Erlang extractor's symbol layer plus full registration. The basic golden proves: module symbol, exported + private functions with arity in signature, multi-clause function collapsing to one symbol, a record, a macro, a `-type`, EDoc and `-doc` attachment, visibility flags.

**Approach:** Mirror `src/elixir/` module decomposition but only build what symbols need now. Grammar node names come from the tree-sitter-erlang grammar — discover them by parsing fixtures, never from memory. `EXTRACTION_CONTRACT_VERSION` review: adding a language row is non-breaking; record that conclusion in the commit message.

**Acceptance criteria:**
- [x] `cargo xtask test language erlang` green; golden `erlang/basic` passes with zero parse diagnostics
- [x] Count assertions updated to 37; `cargo xtask test changed crates/julie-extractors/src/language_spec/specs.rs fixtures/extraction/capabilities.json` green
- [x] `languages --json` shows erlang with honest flags
- [x] `.hrl` standalone-extraction test asserts records/macros from a header
- [x] Worker-scope verification passes and the change is committed (serial-worker-commit)

### Task 3: XML registration + symbols + identifiers

**Files:**
- Create: `crates/julie-extractors/src/xml/` (mod.rs + elements/identifiers modules), `crates/julie-extractors/src/tests/xml/`, `fixtures/extraction/xml/basic/{source.xml,expected.json}`, `fixtures/extraction/xml/xsd/{source.xsd,expected.json}`, `fixtures/extraction/xml/wsdl/{source.wsdl,expected.json}`, `fixtures/extraction/xml/cardinality/{source.xml,expected.json}`
- Modify: same shared registration files as Task 2 (`specs.rs`, `language_spec/mod.rs`, `lib.rs`, `registry.rs`, `factory.rs:60-61` 37→38, `capability_snapshot_test.rs:8` 37→38, `registry.rs:676` 37→38, `capabilities.json` new xml row)

**Interfaces:**
- Consumes: Task 1's XML grammar entry point decision.
- Produces: `XmlExtractor` for `.xml`/`.xsd`/`.wsdl` at `DATA_ONLY_CAPABILITIES` (symbols + identifiers). Symbols: name-promoted elements ONLY — an element with a `name` or `id` attribute emits a symbol named by that attribute value (`<xs:complexType name="AddPhone">` → `AddPhone`), parent-chained like yaml keys; anonymous/generic elements (`<item>`, `<row>`) emit no symbol (html-style filtering, see `src/html/elements.rs:22`). Identifiers: attribute-value QName references from `type=`, `ref=`, `base=`, `element=` attributes.
- Cardinality fixture: a dense sub-1MB document with thousands of repeated anonymous elements asserting bounded symbol output (only the name-promoted handful).

**Contract inputs:** design doc §XML + §Large-XML; hybrid model decision (yaml parent-chain + html filtering); DATA_ONLY per `language_spec/mod.rs:129`.

**File ownership:** Create: `crates/julie-extractors/src/xml/*`, `crates/julie-extractors/src/tests/xml/*`, `fixtures/extraction/xml/{basic,xsd,wsdl,cardinality}/*`; Modify: same shared registration files as Task 2

**Serialization required:** Yes

**Dependency reason:** Conflicts with Task 2 on `specs.rs`, `registry.rs`, `lib.rs`, `factory.rs`, `capabilities.json`, count assertions.

**What to build:** Complete XML extraction for v1 (structural facts excepted — Task 9). Three separate goldens (.xml config-style, .xsd schema, .wsdl service) plus the cardinality fixture. Focused tests: name promotion, anonymous-element suppression, QName identifier rows, nested parent chains, malformed-XML graceful degradation with parse diagnostics.

**Approach:** Start from `src/yaml/mod.rs` for structure/parent chains; port the filtering discipline from `src/html/elements.rs`. Namespace prefixes stay in identifier text as written (`tns:AddPhone`) — no namespace resolution in v1.

**Acceptance criteria:**
- [x] `cargo xtask test language xml` + goldens for all four fixture dirs green, zero parse diagnostics on goldens
- [x] Cardinality fixture proves anonymous-element suppression (symbol count stays in the name-promoted handful)
- [x] Count assertions at 38; `cargo xtask test changed …` green; `languages --json` shows xml symbols+identifiers only
- [x] Worker-scope verification passes and the change is committed (serial-worker-commit)

### Task 4: Erlang identifiers + calls (Batch A)

**Files:**
- Modify: `crates/julie-extractors/src/erlang/` (identifiers/calls modules, modeled on `src/elixir/{identifiers,calls}.rs`), `crates/julie-extractors/src/tests/erlang/`, `fixtures/extraction/erlang/basic/expected.json` (add identifier assertions), erlang row of `fixtures/extraction/capabilities.json` (identifiers=true + kind_coverage.identifiers)

**Interfaces:**
- Consumes: Task 2's `ErlangExtractor` and symbol model.
- Produces: identifier rows for local calls, remote calls `M:F(Args)`, fun references `fun M:F/A` and `fun F/A` (kind distinct from calls), `-import`ed function usage, auto-imported BIF calls (`spawn/1`, `length/1`, …) attributed correctly, macro usage (`?MACRO`), record access (`#rec{}`/`X#rec.field`).

**Contract inputs:** design doc Erlang identifier list; Erlang expression semantics (remote call vs fun-reference distinction is load-bearing — verify against grammar nodes, not memory).

**File ownership:** Modify: `crates/julie-extractors/src/erlang/*`, `crates/julie-extractors/src/tests/erlang/*`, `fixtures/extraction/erlang/*`, erlang row of `fixtures/extraction/capabilities.json`

**Serialization required:** No

**Dependency reason:** None - safe parallel batch. (Disjoint from Task 5's crate.)

**What to build:** The identifier layer with per-kind focused tests asserting name, kind, span, and containing scope for each identifier class above.

**Acceptance criteria:**
- [x] `cargo xtask test language erlang` + golden green; identifiers=true honest in matrix
- [x] Remote call vs fun reference emit distinguishable rows (asserted)
- [x] BIF calls don't produce bogus unresolved module references (asserted)
- [x] Verified diff handed to lead (parallel-lead-commit)

### Task 5: Oversized-transition policy (Batch A)

**Files:**
- Modify: `crates/julie-extract-cli/src/commands.rs` (update path around :1445 where an oversized tracked file currently returns `no_change`), `crates/julie-extract-cli/tests/operations_contract.rs` (around :1850)

**Interfaces:**
- Consumes: existing `files` row lifecycle and skipped-too-large reporting in the CLI crate.
- Produces: policy change for ALL languages — when a previously indexed file is later over `MAX_SOURCE_FILE_BYTES`, `update` (and scan convergence) removes its artifact rows and records it as skipped-too-large instead of preserving stale rows via `no_change`.

**Contract inputs:** design doc §Large-XML corrections; `MAX_SOURCE_FILE_BYTES` at `crates/julie-extract-cli/src/limits.rs:8`; existing report schema (reuse the existing skipped-too-large disposition — no new report fields).

**File ownership:** Modify: `crates/julie-extract-cli/src/commands.rs`, `crates/julie-extract-cli/src/limits.rs` (comment only if needed), `crates/julie-extract-cli/tests/operations_contract.rs`

**Serialization required:** No

**Dependency reason:** None - safe parallel batch. (Touches only `julie-extract-cli`; no shared files with Task 4.)

**What to build:** The transition fix plus boundary tests: file at exactly 1MB (indexed), at 1MB+1 (skipped), and the indexed-then-grows-oversized update case asserting rows removed and disposition reported. Check the scan path handles the same transition, not just single-file `update`.

**Approach:** Follow the existing deleted-file row-removal path for the removal mechanics. If report schema docs (`docs/contracts/reports.md`) describe the `no_change` behavior, update them in this task.

**Acceptance criteria:**
- [x] Indexed-then-oversized file: rows removed, skipped-too-large recorded, on both `update` and `scan` paths (asserted)
- [x] 1MB / 1MB+1 boundary tests pass
- [x] Full `julie-extract-cli` test suite green (escalation trigger for `commands.rs`)
- [x] Verified diff handed to lead (parallel-lead-commit)

### Task 6: Erlang relationships + pending relationships

**Files:**
- Modify: `crates/julie-extractors/src/erlang/` (relationships module, modeled on `src/elixir/relationships.rs`), `crates/julie-extractors/src/tests/erlang/`, erlang row of `capabilities.json` (relationships=true, pending_relationships=true + kind_coverage)
- Create: `fixtures/extraction/erlang/cross_file/{sources,expected.json}`

**Interfaces:**
- Consumes: Tasks 2/4 symbol + identifier layers.
- Produces: resolved relationships — `-behaviour(gen_server)` → implements edge; same-file call edges. Pending relationships — cross-file remote calls (`other_mod:fun/2`) and `-include`/`-include_lib` with structured terminal name/namespace/import context per checklist §3.

**Contract inputs:** design doc Erlang relationships; cross_file golden must prove BOTH resolved and structured pending shapes (checklist §5).

**File ownership:** Modify: `crates/julie-extractors/src/erlang/*`, `crates/julie-extractors/src/tests/erlang/*`; Create: `fixtures/extraction/erlang/cross_file/*`; Modify erlang row of `capabilities.json`

**Serialization required:** Yes

**Dependency reason:** Same erlang files and capabilities row as Task 4; runs after Batch A.

**What to build:** Relationship extraction with the cross_file golden: two modules where A implements a behaviour, calls B remotely, and includes a header — asserting the implements edge, the pending remote-call shape, and the pending include shape.

**Acceptance criteria:**
- [x] `cargo xtask test language erlang` + goldens (basic, cross_file) green
- [x] relationships/pending flags honest in matrix with kind_coverage filled
- [x] Verification passes and the change is committed (serial-worker-commit)

### Task 7: Erlang types + test roles → FULL

**Files:**
- Modify: `crates/julie-extractors/src/erlang/` (types module, weight class of `src/elixir/types_inference.rs`; test-role wiring per `src/elixir/test_calls.rs` + `crates/julie-extractors/src/test_detection.rs`), `crates/julie-extractors/src/tests/erlang/`, `language_spec/specs.rs` (erlang row to `FULL_CAPABILITIES`), erlang row of `capabilities.json` (types=true → capabilities == target FULL)
- Create: `fixtures/extraction/erlang/test_roles/{sources,expected.json}`

**Interfaces:**
- Consumes: everything from Tasks 2/4/6.
- Produces: type facts from `-spec` (function argument/return types as declared text, minimal inference); test roles — EUnit modules (`*_tests.erl` or `eunit` include) as test containers with `_test`/`_test_` generators as cases; Common Test suites (`*_SUITE.erl`) with `all/0`-exported cases and `init_per_suite`/`end_per_suite`-style lifecycle hooks.

**Contract inputs:** design doc Erlang types/test-roles; existing test-role golden conventions (see `fixtures/extraction/elixir/test_roles/` for shape).

**File ownership:** Modify: `crates/julie-extractors/src/erlang/*`, `crates/julie-extractors/src/tests/erlang/*`; Create: `fixtures/extraction/erlang/test_roles/*`; Modify erlang row of `capabilities.json`

**Serialization required:** Yes

**Dependency reason:** Sequential ratchet on the same erlang module and capabilities row.

**What to build:** The last two capability domains, flipping Erlang to FULL honestly. Doc-attribute tests must cover functions, types, AND callbacks (design requirement) if not already proven in Task 2.

**Acceptance criteria:**
- [x] `-spec` type facts asserted; EUnit and CT containers/cases/lifecycle asserted in the test_roles golden
- [x] Erlang `capabilities == target_capabilities == FULL` in matrix; `cargo xtask test capability` green
- [x] Verification passes and the change is committed (serial-worker-commit)

### Task 8: Erlang real-world corpus gate

**Files:**
- Create: `fixtures/real-world/erlang/` (vendored sources of hex.pm `telemetry` 1.3.0, `certifi` 2.15.0, `unicode_util_compat` 0.7.1 — `.erl`/`.hrl` only — with per-package LICENSE files and a `CHECKSUMS.sha256`), `crates/julie-extract-cli/tests/erlang_corpus.rs` (feature-gated so default tests stay fast — follow how existing slow gates are feature-gated per `docs/testing-strategy.md`)

**Interfaces:**
- Consumes: the complete Erlang extractor (Tasks 2-7).
- Produces: the committed acceptance baseline: every corpus file extracts (0 unsupported, 0 failed), exact per-file symbol counts, exact parse-diagnostic baseline (target: zero; if any file legitimately produces diagnostics, the baseline records them explicitly with a comment).

**Contract inputs:** design doc gate 2 — this replaces "0 failed" hand-waving with checksummed inputs and exact assertions. Licenses: telemetry Apache-2.0, certifi BSD, unicode_util_compat Apache-2.0/Unicode — include license texts alongside vendored sources.

**File ownership:** Create: `fixtures/real-world/erlang/**`, `crates/julie-extract-cli/tests/erlang_corpus.rs`

**Serialization required:** Yes

**Dependency reason:** Needs Tasks 2/4/6/7 complete — the baseline asserts final extractor behavior.

**What to build:** Vendor the corpus (the issue's exact packages), write the gated test running a real scan over it and asserting the baseline. Record scan wall-time in the report (report-only).

**Acceptance criteria:**
- [x] All corpus `.erl`/`.hrl` files extract; exported functions, records, behaviours present for `telemetry.erl` (spot-asserted)
- [x] Checksums + licenses committed; baseline is exact, not thresholded
- [x] Gate excluded from default tier (default suite time unchanged)
- [x] Verification passes and the change is committed (serial-worker-commit)

### Task 9: XML schema/WSDL structural facts

**Files:**
- Modify: `crates/julie-extractors/src/xml/` (facts emission), `crates/julie-extractors/src/base/data_structural_facts.rs` (collector routing, see :98), `crates/julie-extractors/src/base/structural_fact_registry/` (new pattern specs following the document-family conventions used by json/yaml/toml/markdown), `docs/contracts/structural-fact-patterns.json` (regenerated, not hand-edited), xml row of `capabilities.json` (`kind_coverage.structural_facts.supported` lists exact pattern ids), `fixtures/extraction/xml/{basic,xsd,wsdl}/expected.json`

**Interfaces:**
- Consumes: Task 3's XML extractor.
- Produces: document-structure facts for generic XML plus schema-aware facts for `.xsd` (types, elements, imports/includes) and `.wsdl` (services, operations, messages, bindings). Exact pattern ids follow registry naming conventions — discover the convention from existing document-family specs, don't invent a new style.

**Contract inputs:** registry conventions in `base/structural_fact_registry/`; capability matrix requires advertised pattern ids to match registered specs (`tests/capability_matrix.rs`).

**File ownership:** Modify: `crates/julie-extractors/src/xml/*`, `crates/julie-extractors/src/base/data_structural_facts.rs`, `crates/julie-extractors/src/base/structural_fact_registry/*`, `docs/contracts/structural-fact-patterns.json`, xml row of `capabilities.json`, `fixtures/extraction/xml/*`

**Serialization required:** Yes

**Dependency reason:** Structural-fact registry files are shared repo-wide; runs alone after Task 3.

**What to build:** Registered pattern specs + emission + golden assertions for the three fixture types. Deferred schema-type relationship resolution stays a typed `open_gaps` entry with reason/closure/planned-task fields (matrix requires them — `tests/capability_matrix.rs:1095`).

**Acceptance criteria:**
- [x] Pattern specs registered; contract JSON regenerated; `cargo xtask test capability` green
- [x] XSD golden asserts type/element/import facts; WSDL golden asserts service/operation/message/binding facts
- [x] `open_gaps` entry for deferred schema relationships with all required fields
- [x] Verification passes and the change is committed (serial-worker-commit)

### Task 10: Branch gates + repo docs

**Files:**
- Modify: `README.md` (language count/list if it enumerates languages), any files fixed by gate failures

**Interfaces:**
- Consumes: all prior tasks.
- Produces: a branch that passes the full checklist §7 gate set, ready for merge and the post-merge delivery chain.

**Contract inputs:** Verification Strategy branch gate + expensive tiers; checklist §§7-9 review questions.

**File ownership:** Modify: `README.md`, any files fixed by gate failures

**Serialization required:** Yes

**Dependency reason:** Final gate over everything; must run last.

**What to build:** Run the full branch gate: `default`, `golden`, `capability`, `changed` (registration paths), `certification` (REQUIRED — parser deps changed), `languages --json` inspection, `node scripts/language-data-quality-report.mjs --strict`, `cargo fmt --check`, `clippy`, `cargo deny check`. Fix what fails. Update README language facts. Answer checklist §9 review questions in the task report.

**Acceptance criteria:**
- [x] Every gate listed above green, recorded in the verification ledger with SHAs — 13/14 at e2d39c0; the strict data-quality gate stayed red (4 quality-bar debts) and was escalated. User decision 2026-08-01: close all four on this branch via Tasks 11-12; the strict gate re-runs green as Task 12's exit criterion.
- [x] `languages --json` erlang row shows FULL, xml row shows symbols+identifiers (+structural facts coverage), both honest
- [x] README language facts current
- [x] Verification passes and the change is committed (serial-worker-commit)

### Task 11: Close the cheap quality-bar debts (erlang complexity + erlang literals + xml literals)

**Files:**
- Modify: `crates/julie-extractors/src/erlang/*` (complexity-metric config + string-literal call-argument capture), `crates/julie-extractors/src/xml/*` (attribute-value literal capture), `crates/julie-extractors/src/tests/erlang/*`, `crates/julie-extractors/src/tests/xml/*`, `fixtures/extraction/erlang/*` and `fixtures/extraction/xml/*` (golden regen), `fixtures/extraction/capabilities.json` (erlang + xml rows), `docs/plans/2026-05-31-julie-code-migration-implementation-plan.md` (Tasks 13/14 closure boxes), `crates/julie-extract-cli/tests/erlang_corpus.rs` (baseline update only if extraction output changes)

**Interfaces:**
- Consumes: the shipped erlang and xml extractors; `base/config_literals.rs::tag_attribute_carrier` (existing helper, used by html and vue); the repo's per-language complexity-metric configuration pattern (discover via Miller from an existing FULL language).
- Produces: erlang rows claim `complexity_metrics` and `literals` with golden evidence; xml row claims `literals`; the corresponding open gaps removed from `capabilities.json` and the migration-plan registry.

**Contract inputs:** Quality-bar semantics come from `scripts/language-data-quality-report.mjs` — read it to see exactly what makes a domain count as covered before implementing. Golden regen via `UPDATE_GOLDEN=1`. `RUSTUP_TOOLCHAIN=1.97.1` on every cargo command.

**File ownership:** As listed under Files (whole erlang/xml extractor + fixture trees, both capability rows, migration-plan doc, corpus baseline).

**Serialization required:** Yes

**Dependency reason:** Shares erlang goldens, `capabilities.json`, and the migration-plan doc with Task 12; runs first because it is the smaller slice.

**What to build:** Close three of the four strict-scorecard debts. (1) Erlang `complexity_metrics`: add the node-kind configuration so erlang functions get complexity scores like other FULL languages (~config-table change, no new walking). (2) Erlang `literals`: capture string-literal call arguments as literal identifiers following the established literal-capture pattern. (3) XML `literals`: capture attribute-value literals via the existing `tag_attribute_carrier` helper. Regenerate goldens; keep the cardinality golden bounded. If corpus counts shift, update the erlang corpus baseline (allowed by this plan) — diagnostics count must not regress from 45/2.

**Acceptance criteria:**
- [x] `node scripts/language-data-quality-report.mjs --strict` shows only `erlang.structural_facts` remaining as debt
- [x] Capability matrix: erlang claims complexity_metrics + literals, xml claims literals, all with golden evidence; matching open gaps removed; capability gate green
- [x] Golden, capability, language (erlang + xml), and default gates green; corpus gate green (baseline updated only if output legitimately changed, with the change explained in the report)
- [x] Migration-plan Tasks 13/14 updated to reflect the closures
- [x] Verification passes and the change is committed (serial-worker-commit)

### Task 12: Erlang structural facts + strict gate green

**Files:**
- Create: `crates/julie-extractors/src/base/structural_fact_registry/erlang.rs` (or per family-ceiling placement)
- Modify: `crates/julie-extractors/src/erlang/*` (fact emission), `crates/julie-extractors/src/base/structural_fact_registry/*` (registry wiring), `docs/contracts/structural-fact-patterns.json` (regenerated via `UPDATE_CONTRACT_JSON=1`), `fixtures/extraction/erlang/*` (golden regen), `fixtures/extraction/capabilities.json` (erlang row), `docs/plans/2026-05-31-julie-code-migration-implementation-plan.md` (Task 13 closure), `crates/julie-extract-cli/tests/erlang_corpus.rs` (baseline update only if output changes)

**Interfaces:**
- Consumes: Task 9's xml registry module as the reference implementation; Task 6's pending `behaviour_declaration` relationship work; the structural-fact registry family ceiling (700 lines forces module placement).
- Produces: erlang `structural_facts` claimed with golden evidence; strict scorecard exits 0; the branch's zero-debt invariant restored.

**Contract inputs:** Pattern specs must be generic `pattern_id` facts consumable by Miller's `patterns` surface. Registry regen via `UPDATE_CONTRACT_JSON=1`; golden regen via `UPDATE_GOLDEN=1`. `RUSTUP_TOOLCHAIN=1.97.1` on every cargo command.

**File ownership:** As listed under Files.

**Serialization required:** Yes

**Dependency reason:** Shares erlang goldens, `capabilities.json`, and the migration-plan doc with Task 11; must run after it so the strict-gate exit criterion covers both.

**What to build:** Erlang structural-fact pattern specs mirroring Task 9's xml slice: behaviour declarations (`-behaviour(...)`), include/include_lib dependencies, and OTP-callback/module-shape facts as warranted by what the grammar exposes — read `scripts/language-data-quality-report.mjs` first to confirm what satisfies the structural_facts quality bar, and scope the spec set to genuinely useful erlang shapes rather than padding. Regenerate the contract JSON and goldens. Then re-run the full Task 10 gate set including `node scripts/language-data-quality-report.mjs --strict`, which must exit 0.

**Acceptance criteria:**
- [ ] Erlang structural-fact specs registered and emitted with golden evidence; contract JSON regenerated
- [ ] `node scripts/language-data-quality-report.mjs --strict` exits 0 (zero quality-bar debts — the branch invariant restored)
- [ ] Full Task 10 gate set re-run green at the new HEAD, recorded in the report
- [ ] Migration-plan Task 13 updated to reflect closure
- [ ] Verification passes and the change is committed (serial-worker-commit)
