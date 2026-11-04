# Language coverage evidence and regression prevention

**Date / reviewed baseline:** 2026-11-03; `main` at `f9a1b6cfbd6c51e9aecad39a19610695f210ad05`, the recorded v3.11.0 publication.

**Status:** Implementation in progress under the active goal. Tasks 1, 2, and 4 began in parallel from `837b9796` in the existing plan worktree. Push and release remain outside the authorized scope.

**Goal:** Make the existing 42-language coverage claims demonstrably accurate, close every locally actionable defect found by the bounded audit below, and install repeatable correctness and performance checks that prevent those defects from returning.

**Architecture:** Keep source-local extraction in the existing language modules, capability evidence in the fixture contracts, and verification orchestration in `xtask`. SQLite and `julie-extract` remain the product interfaces. Reuse the existing baseline, golden, compatibility, and incremental-equivalence machinery.

**Tech stack:** Rust, tree-sitter, SQLite, Cargo/xtask, Node.js report scripts, GitHub Actions.

**Spec:** [AGENTS.md](../../AGENTS.md), [product boundary](../architecture/product-boundary.md), [capability policy](../decisions/2026-07-17-capability-ledger-policy.md), [reference-integrity dispositions](2026-09-27-reference-integrity.md), [testing strategy](../testing-strategy.md), and [extraction contract](../contracts/extracted-data-v4.md). This document defines the additional goal acceptance criteria.

**Architecture quality:** Preserve the existing module boundaries. Main risks are accepting empty evidence as support, introducing hidden project inputs, and multiplying AST/symbol work per emitted row. No new production service or resolver is needed.

## Current status and findings

The review ran the strict scorecard, inspected all registered golden files for body-span evidence, inspected the evidence predicates and applicability logic, and reviewed test routing, workflows, recent release evidence, and retained gap decisions. It did not rerun the Rust suite or benchmark binaries. Historical test results below are explicitly historical. This is a source-backed inventory and targeted audit, not a claim that every language construct has been exhaustively tested.

| Area | Evidence at the reviewed baseline | Remaining work |
| --- | --- | --- |
| Product | 42 registered languages; SQLite schema 8, extraction contract v4, report schema 3 | Preserve current interfaces; declare any extraction-output change |
| Fixtures | 457 registered fixture groups in `fixtures/extraction/capabilities.json` | Measure semantic assertions, not fixture count alone |
| Strict quality | Fresh `node scripts/language-data-quality-report.mjs --strict` exits 0: `silent_cells=0`, `quality_bar_debts=0`, `open_gap_backlog=11` | Passing strict mode does not mean all coverage claims are correct |
| Body claims | Five advertised language/kind pairs have no non-null span/hash evidence | Repair the validator and classify or implement each pair |
| Applicability | 31 unclassified language/domain cells, despite the green strict result | Reconcile report classifications against source and ledger evidence |
| Real-world tiers | Smoke covers JSON; release covers JSON, QML, R, Erlang: 4 of 42 language rows | Add a pinned, representative corpus for all 42; retain narrow selection |
| Performance | Manual writer-only floor; CLI baseline timings are report-only | Add an opt-in, controlled extractor comparison and scale checks |
| Recent delivery | 3.10.0 added imports, anonymous owners, argument facts, package facts and language fixes; 3.11.0 added access facts and annotations at references level | Requalify evidence instead of reimplementing released features |

Sources: [3.10.0 notes](../release-notes/v3.10.0.md), [3.11.0 notes](../release-notes/v3.11.0.md), [3.11.0 evidence](../release-evidence/v3.11.0-access-facts.md), `scripts/language-data-quality-report.mjs`, `crates/julie-extractors/src/tests/capability_matrix.rs`, `xtask/src/test_tiers.rs`, `.github/workflows/performance.yml`.

### Confirmed evidence defects

`observed_kind_coverage` checks `item.get("body_span").is_some()` and the equivalent hash expression. A present JSON key whose value is `null` passes. The scorecard, by contrast, counts truthy spans. All registered goldens were checked for a non-null span and hash:

| Language | Advertised body kinds without evidence | Required disposition |
| --- | --- | --- |
| HTML | `import` | Establish whether the construct has a body under the existing contract; correct the claim if it is bodiless |
| CSS | `import`, `variable` | Independently assess each construct; implement a missing valid body or document genuine non-applicability |
| qmldir | `export`, `module` | Inspect directive semantics; all four current symbol rows have null body fields |

Do not create artificial spans to satisfy a check. Do not move a real implementation gap to `not_applicable`. The null predicate is a proven validation defect; absence of a body on a bodiless declaration is not itself an extractor defect.

The 31 unclassified report cells are concentrated in four languages:

| Language | Count | Domains |
| --- | ---: | --- |
| gomod | 5 | identifiers, complexity_metrics, annotations, types, type_argument_usages |
| gosum | 12 | symbols, relationships, identifiers, body_spans, complexity_metrics, annotations, doc_comments, literals, source_regions, pending_relationships, types, type_argument_usages |
| qmldir | 10 | relationships, identifiers, body_spans, complexity_metrics, annotations, doc_comments, literals, pending_relationships, types, type_argument_usages |
| XML | 4 | complexity_metrics, annotations, types, type_argument_usages |

`DOMAIN_APPLICABILITY` is separate from the ledger and has not kept pace with these languages. Several core domains already have ledger N/A entries. Those entries require source verification; copying them blindly is not closure. XML schema type facts and qmldir dependency facts must be assessed using their native meaning, not programming-language expectations. The three observed-only domains (`types`, `type_argument_usages`, `pending_relationships`) remain outside `kind_coverage`, as the existing policy requires.

### The 11 retained gaps are explicit product boundaries

These are already-retained boundaries, not eleven newly discovered local extraction bugs. Keep the entries visible until their named closure actually happens.

| Language | Exact gap key | Disposition for this goal |
| --- | --- | --- |
| rust | `actix.scope_route_cross_file_registration` | Consumer joins mount sites to declarations; preserve multiple mounts |
| rust | `axum.cross_file_nest_join` | Consumer joins nest sites to router declarations |
| php | `laravel.route_service_provider_prefix` | Consumer joins provider prefixes and route files |
| elixir | `phoenix.cross_file_scope_prefix` | Consumer joins forward sites and target routers |
| rust | `axum.param_flavor_under_report` | Explicit version/dialect input is required to disambiguate colon syntax |
| rust | `rust.http_client.instance_receiver` | External type evidence belongs to consumer resolution unless an explicit extraction context contract is approved |
| kotlin | `kotlin.http_client.instance_receiver` | Same external type boundary |
| typescript, tsx, javascript, jsx | `nextjs.signal_free_pages_router_files` (four entries) | A `pages/` path alone does not establish Next.js; explicit framework context is required |

Acceptance here means proving useful local facts and honest uncertainty, not performing the consumer's join. No neighboring-manifest reads, guessed routes, or workspace resolver. A future project-context API needs its own design and freshness/invalidation contract. JSON5 remains outside scope by the [existing decision](../decisions/2026-09-23-json5-out-of-scope.md). Regex conditionals and branch-reset groups are already closed in the reference-integrity record; do not revive the obsolete September backlog.

### Performance and validation weaknesses

- The manual performance workflow runs `cargo xtask test perf`: synthetic SQLite writer tests, not parser/extractor throughput. Existing baseline summaries contain no old/new extractor threshold.
- The September comparison caught approximately 22% and 28% scan regressions in zod and Newtonsoft.Json. Causes included repeated parent walks, repeated member indexes, and per-fact symbol scans. See [wave-2 evidence](../evidence/2026-09-23-language-gap-wave2-real-world.md). Those historical numbers are not today's baseline.
- That comparison names 13 repositories without pinned revisions. QML upstream samples and the simplified R sample also lack complete revision/hash provenance. Erlang's vendored corpus has package versions, licenses, and checksums and is a useful model. Synthetic samples should be labeled as such rather than assigned invented upstream provenance.
- 3.11.0 records a 46-second default tier, Linux validation, and six-platform package verification in its release records; these are historical evidence, not fresh performance certification. Its Hermes scan retained 7 `slow_file_skipped` warnings and 33 payload-conflict warnings. A faster scan that skips more inputs must never pass a performance comparison.
- `docs/README.md` still links schema v7 as current, and the new-language checklist still names old v3/JSONL contracts. Update these during integration so future coverage work follows the SQLite-only contract.

## Definition of done and successful

The goal is complete only when **all** of the following are true on one identified final source state. A green scorecard, a plan document, or a partial audit is insufficient.

- [ ] All 42 baseline languages are accounted for across the 15 observed domains; supported kinds/patterns point to meaningful fixtures, and each absent domain has a source-backed classification. Test roles retain their per-role checks.
- [ ] All five unsupported body claims have correct, tested dispositions. Validators reject null/empty/malformed proof and detect removal of the last real supporting row.
- [ ] `silent_cells=0`, `quality_bar_debts=0`, unclassified applicability cells `=0`, unsupported advertised claims `=0`, and incomplete audit entries `=0`.
- [ ] Every reproducible local defect found by the bounded matrix below is fixed and verified. New defects may not be deferred merely because they are inconvenient. Genuine external blockers are recorded and prevent claiming the affected required outcome complete.
- [ ] Each of the 11 retained entries has positive local evidence, an uncertainty/negative control, and its existing owner/closure boundary. The backlog may remain 11; deleting or relabeling these entries does not count as implementation. No additional unexplained gap is permitted.
- [ ] The representative corpus covers 42/42 language rows with reproducible inputs and meaningful expected facts. Both native and embedded syntax are represented for wrappers/variants; no language is covered solely by another language's result.
- [ ] The new comparison detects injected correctness loss, extra skipped files, and performance regressions. The real final candidate passes the controlled budgets below, including approved row-growth accounting.
- [ ] Focused checks, the final branch gate, applicable Windows checks, and required expensive checks pass. The warm default-tier median remains at or below 180 seconds on the recorded reference host; any growth beyond that is repaired without weakening tests.
- [ ] A final evidence ledger records baseline/candidate SHAs, binary hashes, inputs, commands, results, metrics, and every disposition. All task boxes are checked and related worktrees are reconciled; changes are committed locally. Push, publication, release, and code-kb modifications are not required.

**Bounded audit:** Audit the current 42 languages and their currently advertised constructs, the five body claims, the 31 applicability cells, and the 11 retained gaps. For each applicable domain, examine a positive example, a confusable negative example, and a boundary example (nesting/shadowing, Unicode/CRLF offsets, malformed recovery, or embedded offsets as appropriate). Reuse existing assertions when they prove the behavior. New languages, speculative frameworks, and unlimited syntax enumeration are separate goals. N/A cells require a source-backed rationale rather than an artificial positive example. Fix local defects exposed by these checks before closing their matrix rows.

**Stop rules:** Do not mark the goal complete while a required gate fails, evidence is missing, or a required Windows/performance run is unavailable. Report the specific blocker and preserve the next action. Do not stop at a task boundary when the next authorized task can proceed.

## Global constraints

- SQLite is the only durable product output. Test manifests and verification summaries are development artifacts, not a new output format.
- No workspace-global reference resolution. Preserve source sites, uncertainty, and multiple registrations.
- Capability upgrades require registered goldens and correct spans/owners, not merely nonzero row counts. Body hashes must obey the existing normalization contract.
- Keep production schema/report versions unless an actual contract change requires otherwise. Extraction-output changes need the existing marker/ledger discipline; do not preselect a release version.
- Keep slow corpora, full certification, and timed performance runs out of default. Add routing/convention tests whenever adding a test target.
- Windows remains first class. Handle closure, basename selection, separators, canonical paths, and incremental file transitions follow existing shared helpers.
- Do not modify unrelated work or remove old worktrees. The review found all 12 pre-existing linked worktrees clean. Nine `gap/*` branches retain divergent ancestry, but the corresponding features are in 3.10.0 release evidence/current source; reconcile by patches and source, not ancestry alone.

## Verification strategy

**Project source of truth:** `docs/testing-strategy.md`, `xtask/src/test_tiers.rs`, `.github/workflows/ci.yml`, `.github/workflows/specialist-gates.yml`, and AGENTS.md.

**Worker red/green scope:** `cargo xtask test language <language>` for changed languages; `cargo xtask test capability` for evidence validators; `cargo test -p xtask --test performance_baseline_contract` for the existing performance harness. Add focused tests for new reporting/comparison behavior before implementation.

**Worker ceiling:** Owned language(s) or one verification component. Workers do not each run the full contract suite or corpus.

**Worker gate invariant:** Assert exact rows, source spans, owners, metadata, and intended absence. Show that the targeted defect fails before the fix and passes afterward; exclude no-op/zero-selected-test results.

**Lead affected-change scope:** Changed language tiers, then `cargo xtask test golden`, `cargo xtask test capability`, and `node scripts/language-data-quality-report.mjs --strict`. The proposed new applicability switch is only available after Task 2.

**Branch gate:** `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`; `cargo test -p xtask`; `cargo xtask test default`; `cargo xtask test contract`; strict quality plus the new classification gate; `scripts/check-agent-doc-sync.sh`. Run once on final integrated source, rerunning only affected checks after corrections.

**Security scope:** `cargo deny check` for dependency policy; no standalone secrets-scan command is declared. Preserve source licenses and provenance for added corpora.

**Replay/metric evidence:** Golden and SQLite assertions, fixture proof, classification completeness, skip/diagnostic limits, output-equivalence checks, and the dedicated comparison budgets are gates. Regular CI wall time remains informational. Cold-build time and general hosted-runner timings are report-only.

**Escalation triggers:** Parser/dependency changes require `cargo xtask test certification`. Shared extraction, owner lookup, body normalization, or high-volume fact changes require the full representative performance corpus. Changes to paths, manifests, CLI persistence, or fixture lifecycle require native Windows affected tests plus incremental equivalence. Final integration runs real-world smoke/release and the new all-language corpus.

**Assigned verification failure:** Investigate and fix failures inside ownership. Escalate ownership or contract conflicts to the lead; never silently weaken a gate.

**Verification ledger:** Create `docs/evidence/2026-11-03-language-coverage-goal.md`. Each entry records invariant, command, tier, SHA/dirty diff identity, timestamp, result, and evidence path. Reuse passing evidence only for an unchanged relevant source state. Distinguish historical results from fresh runs.

### Performance acceptance contract (new work in Task 5)

These are proposed goal budgets, not claims about existing CI or measured current performance.

1. Pin the corpus commit/content hashes, old/new release binary hashes, Rust toolchain/build profile, extraction level, jobs, OS/CPU/RAM/storage, and filesystem-cache procedure. Use the same host and input tree; run no concurrent benchmarks. Record both `references` and `full` because recent features differ by level.
2. Take one unmeasured warm-up and five alternating baseline/candidate measurements per workload. Calibrate small workloads into a fixed batch whose baseline sample lasts at least two seconds; use the identical operation count for the candidate and report both batch and per-operation times. Each operation still uses its required fresh database or prepared update/no-change state; preparation stays outside the measured interval. The absolute timing floor applies to the calibrated batch, so tiny individual operations cannot hide a large relative slowdown. Use a fresh database for a full scan; separately measure an immediate no-change scan and single-file update. Record min/median/max and all samples. A cold extraction here means a fresh artifact, not a claimed cold OS cache.
3. Compare semantic rows with the existing comparison rules: preserve row multiplicity, source identities, JSON arrays, owners and diagnostics; normalize only documented volatile metadata. New/changed rows require an explicit per-task allowlist tied to the output ledger. Blanket count tolerances or dropping whole fact tables are forbidden.
4. No new failed files, unsupported files, slow-file skips, payload conflicts, or unexplained parse diagnostics. Pinned inherited warnings need file/category-level baselines; their disappearance is checked for silent lost work. Every added clean fixture has zero diagnostics.
5. On the controlled host, a candidate fails a timing budget when its median exceeds baseline by **both 10% and 100 ms** for any scan/rescan/update workload. Peak resident memory fails when growth exceeds **both 15% and 32 MiB**. Artifact size fails when unexplained growth exceeds **both 10% and 1 MiB**. Keep total latency/size alongside time-per-file and bytes/row; normalization must not hide a slowdown.
6. Define sample spread as maximum minus minimum, separately for baseline and candidate elapsed-time samples. If it exceeds the greater of 10% of that median or 20 ms after small-workload batching, the measurement is inconclusive: remove contention and repeat once. Continued noise is a verification blocker, not a pass. A candidate crossing a budget gets one clean repeat before diagnosis; a repeat failure must be fixed.
7. Necessary additional facts can increase cost. Before accepting that increase, record exact row deltas, table allocation, profiles, and a workload-specific revised budget with explicit owner approval. Do not silently refresh the baseline or waive a regression by pointing at higher row counts. Unchanged-output workloads keep the original budget.
8. Add deterministic operation-count checks for newly changed traversal/index code where practical: flat and nested inputs at N/2N/4N, with N large enough to dominate setup, must remain within the chosen linear or N-log-N algorithm's bound (a doubling ratio no greater than 2.5 after fixed setup). Validate owners/spans and nonempty output at every size. Timed stress workloads remain opt-in.
9. Prove the comparator with controlled bad samples: +20% latency beyond the absolute floor, excess memory/size, lost rows, extra skipped files, and mismatched provenance each fail. Identical inputs and allowed row additions pass. Do not implement test-only sleeping in production extraction.
10. Preserve the writer batching one-transaction contract and run `cargo xtask test perf` and `cargo xtask performance writer-current-schema --out-dir <new-dir>` when persistence or row volume changes. Measure the default suite warm three times on the final source; retain every test and repair growth beyond the 180-second target.

Existing baseline command, reused by the new harness:

```sh
cargo build --release -p julie-extract-cli --bin julie-extract
cargo xtask performance baseline --root <pinned-root> --out-dir <new-dir> --binary <absolute-binary> --runs 5
```

The existing command does not implement alternating binaries, per-level selection, update timing, peak memory, or comparison budgets. Task 5 adds those explicitly rather than assuming the current summary proves them.

## Parallel execution contract

Commit mode: `parallel-lead-commit`. Workers hand verified diffs to the lead. The lead owns shared manifests, contract markers, evidence, integration, and commits. Register each new xtask module/command and test route as its task lands, before that task runs its acceptance commands; Task 6 verifies the integrated registration rather than delaying it. Source ownership is assigned before dispatch; two workers never regenerate the same golden or edit the same shared file simultaneously.

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
| --- | --- | --- | --- | --- |
| 1. Truthful body evidence | A | `crates/julie-extractors/src/tests/capability_matrix.rs`; lead merges `fixtures/extraction/capabilities.json` | No | Independent of Task 2 script work; shared manifest edits belong to lead |
| 2. Complete applicability | A | `scripts/language-data-quality-report.mjs`, new `scripts/language-data-quality-report.test.mjs`, `.github/workflows/ci.yml` | No | Independent code ownership; reconcile manifest through lead |
| 3. Requalify 42 languages | B | New `fixtures/extraction/validation-matrix.json`, `scripts/check-language-coverage-matrix.mjs`, `scripts/check-language-coverage-matrix.test.mjs`, and `docs/findings/2026-11-03-language-coverage-matrix.md`; lead assigns exact language source/test/fixture paths for reproduced defects | Yes | Requires trustworthy evidence checks from Tasks 1–2 |
| 4. Reproducible corpus | A | New `fixtures/real-world/coverage-manifest.json`, new `fixtures/real-world/coverage/`, new `xtask/src/coverage.rs`, new `xtask/tests/coverage_contract.rs` | No | New files are independent; lead serializes xtask registration |
| 5. Performance protection | C | `xtask/src/performance.rs`, `xtask/tests/performance_baseline_contract.rs`, new `xtask/tests/performance_comparison_contract.rs`, `.github/workflows/performance.yml` | Yes | Consumes pinned corpus from Task 4 and corrected candidate from Task 3 |
| 6. Integration and closure | D | `xtask/src/main.rs`, `xtask/src/lib.rs`, `xtask/src/commands.rs`, `xtask/src/test_tiers.rs`, `xtask/tests/test_tiers.rs`, `xtask/tests/integration_test_tiers.rs`, docs, shared contract/fixture files | Yes | Consumes Tasks 1–5 and owns all final gates |

### Task 1: Reject empty body evidence and resolve five claims

**Files / ownership:** The Task 1 row above; read `crates/julie-extractors/src/base/body.rs`, `crates/julie-extractors/src/tests/body_span.rs`, and existing HTML/CSS/qmldir fixtures before changing claims. Any required language-source fix is assigned explicitly by the lead.

**Interfaces:** Consumes registered expected JSON; produces an honest per-kind body-span/hash proof. **Contract inputs:** Existing body/hash semantics and the five pairs above. **Serialization required:** No. **Dependency reason:** Independent code; manifest changes serialize through the lead.

**What to build:** A validator that rejects missing, null, malformed and empty proof, plus correct dispositions for all five kinds. **Approach:** Tighten `observed_kind_coverage`, use the existing supported-kind test, and verify the affected constructs against actual syntax and extraction. Require valid in-source spans, not just an arbitrary object.

- [x] Null span/hash, malformed/reversed/out-of-source spans, empty hash, and removal of the last supporting row fail; a valid pair passes.
- [x] All five claims have a source-backed fixture or a documented genuine N/A classification; applicable missing bodies are implemented and tested.
- [x] Existing valid span/hash evidence survives; body-only edits change the hash while permitted formatting/comment edits preserve it.
- [x] Affected language tiers and capability checks pass; hand the verified diff to the lead.

### Task 2: Close applicability accounting and prevent drift

**Files / ownership:** The Task 2 row; lead owns any resulting capability/decision edits. **Interfaces:** Consumes capability rows, expected output and script-local observed-domain classifications; produces complete report classifications. **Contract inputs:** The 31 cells and the July capability policy. **Serialization required:** No. **Dependency reason:** No Task 1 code overlap.

**What to build:** Source-backed classifications and a completeness check without changing the established meaning of `--strict`. **Approach:** Derive core-domain state from the ledger when it fully accounts for the domain; keep the three observed-only domains in `DOMAIN_APPLICABILITY`. Add an explicit **new** `--check-applicability` switch which fails on unclassified cells or inconsistent classifications, and run it alongside strict mode in CI. Record this added gate in a decision document; do not turn every retained open gap into a strict-mode failure.

- [x] All 31 cells are resolved using source/grammar or real fixture evidence; qmldir is correctly treated as a domain language.
- [x] Empty output alone cannot establish N/A; useful XML/schema and manifest-native facts are not suppressed to satisfy classification.
- [x] Node regression tests reject unknown languages, duplicate/conflicting buckets, missing classifications, unsupported claims and stale fixture registrations; introducing a new language forces classification work.
- [x] `node --test scripts/language-data-quality-report.test.mjs` and `node scripts/language-data-quality-report.mjs --strict --check-applicability` pass; both commands become documented after implementation.
- [x] Hand the verified diff to the lead.

### Task 3: Requalify the current language claims and retained boundaries

**Files / ownership:** The Task 3 row; read-only access to all language source, tests, registered goldens and language docs. For each reproduced defect, the lead appends a concrete owned source/test/fixture list to the evidence ledger before dispatch; no two slices share a mutable file. **Interfaces:** Produces a 42-language evidence matrix and fixed local extraction behavior. **Contract inputs:** The bounded audit and 11-entry boundary table. **Serialization required:** Yes. **Dependency reason:** Tasks 1–2 establish reliable evidence predicates.

**What to build:** A machine-checked audit matrix with exact fixture/test pointers and dispositions, followed by fixes for every local omission it reproduces. Keep JSON as the source of truth and generate the Markdown view from it. **Approach:** Start with HTML/CSS/qmldir, gomod/gosum/XML, then recent access/annotation additions, wrappers/embedded languages, and the remaining languages. Reuse existing tests where their assertions are sufficient.

- [ ] Matrix format v1 has exactly one cell for each of the 42 registry languages × 15 observed domains (630 cells). Each cell requires `language`, `domain`, `classification`, `evidence`, `negative_or_boundary_evidence`, `rationale`, `local_defects`, and `retained_gap_ids`. Classification is `supported`, `not_applicable`, or `convention_only`; retained gaps coexist with support instead of replacing it.
- [ ] Supported cells have meaningful positive and negative/boundary evidence; every advertised kind/pattern has a named test assertion or golden JSON pointer. N/A and convention-only cells require rationale and source/contract pointers, not fabricated positive examples. Each local defect records its ID, status, fix and verification evidence; completion allows no unresolved local defect.
- [ ] The new `node scripts/check-language-coverage-matrix.mjs` rejects missing/duplicate/unknown cells, broken file or JSON pointers, nonexistent named tests, absent required rationale/proof, unresolved local defects, and unmatched retained-gap IDs. Fast mutation tests under `node --test scripts/check-language-coverage-matrix.test.mjs` prove those failures. The completion check validates the real matrix and enters regular CI at integration.
- [ ] Access-fact checks distinguish receiver reads, member writes, compound assignments, shadowing and unsupported syntax for all 33 claiming languages; call/type-usage identifiers never gain access metadata. Annotation checks cover all four extraction levels and applicable embedded formats.
- [ ] Body/hash checks include Unicode/CRLF boundaries, nested ownership and comment-only edits. Relationships distinguish lexical targets from unresolved calls; duplicate names and multiple mounts cannot fabricate a unique target.
- [ ] All 11 retained gaps have positive local-fact and negative/context evidence. Preserve every mount and refrain from inventing signal-free framework routes or unknown receiver types.
- [ ] Every discovered local defect has a failing reproduction, implementation, focused green check, and reviewed golden delta. Any output change is handed to the lead for contract declaration.
- [ ] All required language tiers pass; hand verified fixes and the complete matrix to the lead.

### Task 4: Make representative validation reproducible for all languages

**Files / ownership:** The Task 4 row; lead registers module/command/test routing. **Interfaces:** A development-only versioned corpus manifest and opt-in runner. **Contract inputs:** Current 42-language registry, existing Erlang provenance practice, and public CLI/SQLite contracts. **Serialization required:** No. **Dependency reason:** New files do not overlap other workers.

**What to build:** A small, pinned real-source sample for every language row and a runner that validates useful persisted facts. **Approach:** Preserve the existing specialized fixtures; add complementary samples with provenance instead of downloading live repository heads. Start with changed/risky languages and finish all 42. A purpose-built synthetic stress sample supplements real-source evidence; it does not replace it.

- [ ] Manifest records format version, language, local path, source URL/project, immutable revision/release, source content hash, license, expected domains and fact assertions. Locally authored controls are explicitly labeled and hashed.
- [ ] Coverage is 42/42, including TSX/JSX, Vue, Razor, QML, qmldir, gomod and gosum as separate rows. Each sample proves its advertised native facts through CLI-produced SQLite.
- [ ] Existing QML/R/JSON samples have truthful provenance or explicit synthetic status; do not invent historical source revisions. All added upstream files have license evidence.
- [ ] The proposed new `cargo xtask coverage --language <language> --binary <path>` and `cargo xtask coverage --all --binary <path>` commands select exact samples, reject missing/hash-mismatched inputs, run offline, and report skipped/failed/diagnostic counts and facts.
- [ ] Positive assertions, confusable negative controls, malformed recovery, and embedded source offsets are covered where applicable. Runs fail on missing expected rows rather than accepting nonzero totals alone.
- [ ] Fast runner contract tests use tiny local inputs; corpus extraction stays outside default. Hand the verified diff to the lead.

### Task 5: Add measured extractor regression protection

**Files / ownership:** The Task 5 row; lead owns test-tier wiring and testing-strategy documentation. **Interfaces:** Extends existing performance summaries and adds an explicit baseline/candidate comparison command; development-only JSON has a versioned schema. **Contract inputs:** Task 4 manifest, current baseline/writer helpers, and all ten performance rules above. **Serialization required:** Yes. **Dependency reason:** Requires stable inputs and the final intended output changes.

**What to build:** Reusable paired release-binary measurements, semantic checks, controlled budgets, and scaling guards. **Approach:** Extend `run_baseline`/summary machinery where appropriate; preserve the existing report-only baseline command. Add a proposed new `cargo xtask performance compare --baseline <path> --candidate <path> --manifest <path> --out-dir <new-dir>` entry point with per-language selection. Record peak memory with a documented native platform mechanism; unsupported collection is an explicit incomplete required measurement, not a fabricated zero.

- [ ] Comparison enforces provenance, row/diagnostic/skip checks, alternating samples, extraction levels, fresh scan/no-change/update scenarios, and the stated latency/memory/storage budgets.
- [ ] Controlled bad summary/input cases fail and valid cases pass; output changes require named evidence. There is no silent baseline refresh.
- [ ] Changed traversal/index code has meaningful N/2N/4N checks and no repeated whole-tree/symbol indexing per emitted row. Shared changes exercise representative large/nested files beyond tiny goldens.
- [ ] Dedicated workflow runs an opt-in comparison with explicit pinned baseline/candidate inputs and uploads summaries; ordinary PR CI runs comparator logic tests, not host-sensitive timing gates.
- [ ] Final candidate passes the pinned corpus comparison and applicable writer/batching checks; hand the verified diff and raw measurements to the lead.

### Task 6: Integrate contracts, prevent test-tier drift, and close the goal

**Files / ownership:** The Task 6 row; specifically `docs/testing-strategy.md`, `docs/README.md`, `docs/languages/new-language-checklist.md`, `docs/contracts/extraction-output-changes.md`, `crates/julie-extractors/src/lib.rs`, `crates/julie-extractors/src/tests/api_surface.rs`, capability/golden manifests, this plan and the final evidence ledger. Change contract markers only when the integrated output requires them. **Interfaces:** Final CLI/artifact evidence and correct development guidance. **Contract inputs:** All tasks and success criteria. **Serialization required:** Yes. **Dependency reason:** Final integration consumes all verified slices.

**What to build:** A locally integrated, verified change with an honest final report and durable prevention checks. **Approach:** Review golden deltas, run final gates once, fix any integration defects, then record the final source state and metrics.

- [ ] New fast contract tests are routed explicitly; convention tests prove corpus, certification and timed stress gates stay outside default. Retain narrow language commands.
- [ ] Public capability snapshots, docs and contract markers agree with the result; docs point to schema v8/extraction v4 and the SQLite-only product. No unsolicited version bump or release.
- [ ] Compatibility against the pinned 3.11.0 binary accounts for every intended row change. Reuse `crates/julie-extract-cli/tests/rebind_equivalence.rs` to check add/edit/delete/rename/malformed-to-valid/no-change against fresh scans with all fact tables and multiplicities.
- [ ] Run the final branch gate, real-world tiers, all-language corpus, controlled performance comparison and escalation gates. Native Windows runs cover changed platform-sensitive paths and incremental/file lifecycle behavior; log exact commands and source identity.
- [ ] Record three warm default runs with min/median/max and repair a median above 180 seconds. Retain broad coverage rather than hiding expensive tests in ad hoc exclusions.
- [ ] Review each success checkbox against evidence; record final counts and all 11 retained boundary dispositions. Reconcile worktree states and commit the scoped source/docs/memory locally.
- [ ] Mark this plan complete only after every required outcome is met. If a gate or access boundary blocks completion, leave it open and identify the smallest concrete missing action.

## Goal execution handoff

Use this document as the goal's acceptance contract, not merely its task list. On execution, first verify current source/worktrees and re-run the strict report: the reviewed baseline can drift. Preserve completed work, update evidence with the new baseline, and execute through final verification without stopping at intermediate tasks. The active goal authorizes local implementation and commits, not push or publication.

The plan itself is saved in `/home/murphy/source/julie-extractors/.worktrees/language-coverage-plan` on `docs/language-coverage-plan`, based on `f9a1b6cf`. Keep related edits together and reconcile this documentation branch when choosing the implementation worktree.
