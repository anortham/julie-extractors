# Canonical receiver facts

**Goal:** Make language-aware extraction own receiver facts, prove reference correctness across applicable languages, and measure incremental reporting cost.
**Spec:** The user's approval of the three next items in this conversation: consolidate CLI receiver detection into extraction, verify receiver types and exact locations across languages, and measure whole-artifact counting during incremental commands.
**Authority:** Local implementation and commits authorized by "ok do it". No push or release. External reviewer: none.
**Architecture:** Enrich existing identifier metadata while the language tree and source share coordinates. The CLI serializes those facts. Reuse the public extraction API and existing SQLite columns; no new resolver, schema, dependency, or configurable pipeline.

## Architecture quality

- Affected modules: extractor registry/shared identifier facts; CLI artifact mapping.
- Caller-facing interface: existing `extract_for_language_at`, `extract_canonical_for_language_at`, and CLI artifact output.
- Locality: an internal AST helper at the extraction boundary; language-specific rules remain explicit and small.
- Test surface: real source through public extraction and CLI, with literal expected receivers/types/spans.
- Dependencies: in-process tree-sitter and metadata types; local SQLite in CLI tests. No adapters needed.
- Rejected shortcuts: moving byte scanning unchanged; keeping a CLI fallback; guessing complex receivers or types; replacing all language extractors; adding persistent count caches before measurement.
- Risk: medium. Embedded trees, explicit null suppression, qualified names, exact spans, and all-language coverage require verification.

## Global constraints

- Work in `/home/murphy/source/julie-extractors/.worktrees/refactor-receiver-facts`, branch `refactor/receiver-facts`, base `e11fe30a`. Do not modify code-kb.
- Preserve authoritative extractor metadata and explicit absence; no false receiver on comments, paths, objectless members, or expression receivers that have no named receiver.
- Treat existing receiver and qualifier as an authoritative pair. For a wholly named chain `a.b.f()`, publish receiver `b` and qualifier `a`; do not invent partial named receivers for expression-rooted chains such as `factory().b.f()` or `items[0].f()`.
- Use AST evidence and support Unicode, comments between tokens, multiline access, self receivers, static qualification and existing language-specific member separators.
- Enrich embedded results before host-coordinate remapping. Do not interpret embedded identifiers against an unrelated host tree.
- Match canonical language aliases and skip embedded host regions. Vue relabels script identifiers as Vue after remapping, so language equality alone is insufficient.
- Do not alter identifier/reference-site span or identity merely to attach receiver metadata. Exact spans must select the source token they claim.
- Keep all applicable languages in scope. Unsupported constructs need evidence, not silent exclusion.
- Existing receiver types must survive both APIs. Do not infer workspace-global types or add language-specific consumer logic.
- Preserve relationship and pending-relationship receiver types even at levels without identifiers. Bound enrichment to byte-range node lookup or one traversal, never a whole-tree search for each identifier.
- Advance semantic extraction identity if emitted facts change; update and review intentional goldens. Capability checks must remain clean.
- No narrative inline comments or test comments. Workers do not stage, commit, or write memories.

## Verification strategy

Source: AGENTS.md and Cargo/xtask tiers. Baseline at `e11fe30a`: six CLI receiver unit tests pass; previous default 6,708 and Windows 390 tests pass on the same production source.
Worker red/green: new tests through public extraction or CLI only. Worker ceiling: assigned test module plus directly changed language tests. Workers fix failures in owned scope and report ownership conflicts.
Lead affected-change: CLI extraction/operations/reference-site tests, all-language goldens and strict quality report.
Branch gate: `cargo xtask test default`, formatting and diff checks. Windows CLI/artifact suites for final mapping/writes; no real-world corpus sweep unless a failure requires it.
Security scope: none declared. No dependency or security-boundary changes.
Performance evidence: one fixed update/delete workload, realistic extracted row volume, warm p95 wall time and actual count-query timings. Only measured improvements may remain. No wall-clock assertions in default tests.
Lead owns broad gates and records exact commands/results. Passing unchanged scopes are reused.

## Parallel execution contract

Commit mode: `parallel-lead-commit`.

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| 1: Canonical AST receiver facts | A | `crates/julie-extractors/src/base/receiver_metadata.rs` new, `base/mod.rs`, `registry.rs`, `crates/julie-extractors/tests/receiver_metadata.rs` new | No | Existing public extraction API is the interface |
| 2: Cross-language contract | A | `crates/julie-extract-cli/tests/receiver_contract.rs` new | No | Tests call existing public APIs; no production overlap |
| 3: CLI and integration | B | `crates/julie-extract-cli/src/extraction.rs`, `tests/receiver_type_contract.rs`, extractor `lib.rs`, changed golden expectations/capability evidence, contract/decision docs, this plan, memories | Yes | Canonical extraction must supply the facts before completing CLI integration; independent type-preservation checks can run during A |
| 4: Reporting measurement | A | `docs/findings/2026-09-27-incremental-reporting-cost.md`, ignored scratch measurements | No | Read-only production work; no reporting optimization without measured evidence |

### Task 1: Canonical AST receiver facts

**Interfaces:** `extract_for_language_at(language, tree, file_path, content, workspace_root, level)` and identifier metadata `receiver`, `receiver_qualifier`, plus existing `receiver_type`.
**Contract inputs:** Existing metadata wins. Explicit null receiver suppresses both receiver keys at artifact mapping. Embedded identifiers use their own parser coordinates.
**What to build:** Attach receiver facts during extraction from the actual AST, with minimal common logic and explicit grammar handling. Inspect existing language helpers first. Hook shared enrichment where normal/embedded/public calls receive it; report any embedded path needing a separate hook before editing outside ownership.
**Approach:** Start with public-API failures for ordinary, Unicode, commented/multiline and qualified member calls. Avoid raw backward source scanning. Keep source spans unchanged. The lead will remove CLI guessing after conformance passes.

- [ ] Public-API regression fails before the change and passes afterward.
- [ ] Named receiver and qualifier use AST evidence; authoritative metadata and type facts remain intact.
- [ ] Embedded coordinates and non-member negative cases are handled without false facts.
- [ ] Worker checks pass; lead reviews the implementation.

### Task 2: Cross-language receiver/reference contract

**Interfaces:** `extract_canonical_for_language_at` and real `julie-extract scan` output.
**Contract inputs:** Literal expected receiver/type values and target source tokens; current canonical language registry and existing grammar fixtures.
**What to build:** A compact contract matrix covering every applicable language, including embedded hosts and meaningful negative controls. Compare canonical facts with SQLite output, and independently assert expected receiver names/types and exact byte/line/column locations. Preserve multiplicity and precise sites through shared mappings.
**Approach:** Reuse valid source idioms from existing fixtures. Include comments, Unicode, multiline/chained calls, self/static receivers and explicit suppression where applicable. Report missing facts to the lead/core implementer rather than weakening expectations. One batched CLI fixture scan is preferable to one process per case.

- [ ] Matrix explicitly accounts for every registered language's applicability.
- [ ] Canonical and CLI receiver/type facts agree and match hand-derived expectations.
- [ ] Exact reference locations select the right source token, including Unicode and embedded offsets.
- [ ] Negative cases emit no false receiver; existing exact-site identity and repeated occurrences survive.

### Task 3: CLI and integration

**Interfaces:** `map_identifiers`, published metadata, semantic contract string and golden outputs.
**What to build:** Delete CLI source-text receiver detection and its private-helper tests once behavior is proven through extraction. Keep metadata serialization/explicit absence handling and existing type facts. Advance semantic identity and record the new ownership rule in a concise decision.

- [ ] CLI maps receiver facts without interpreting source syntax.
- [ ] Cross-language gaps found by Task 2 are corrected within extraction, with focused tests.
- [ ] Golden changes are reviewed and strict quality reports zero silent cells/quality debts.
- [ ] Relevant Linux/Windows gates pass, reviewed changes are committed, and all worktrees are reconciled.

### Task 4: Reporting measurement

**Interfaces:** Existing CLI update/delete reports and `table_totals`.
**What to build:** A reproducible baseline and attribution report. The report decides whether a reporting change is justified; preserve exact public totals and transaction consistency in any subsequent implementation.

- [x] Immutable baseline binary and fixed realistic workload are recorded.
- [x] Warm repeated command timings, row/query counts and count-query attribution are recorded.
- [x] Any retained optimization has a matching before/after measurement and correctness/count regression; otherwise document why no change is warranted.

**Measurement result:** On 1,688,470 extracted rows, all 22 count queries took 10.472 ms warm p95 versus 544.041 ms update and 534.918 ms delete. Keep exact totals; no reporting optimization is justified by this workload. See `docs/findings/2026-09-27-incremental-reporting-cost.md`.
