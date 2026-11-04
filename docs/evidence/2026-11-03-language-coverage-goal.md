# Language coverage verification evidence

Implementation begins from `837b9796` in `.worktrees/language-coverage-plan` on `docs/language-coverage-plan`. Baseline product source is `f9a1b6cf`. All fourteen worktrees were clean before implementation. The goal authorizes local changes and commits; push and publication remain outside scope.

| Scope | Invariant | Command | Source | Result | Evidence |
|---|---|---|---|---|---|
| baseline | Existing strict quality contract | `node scripts/language-data-quality-report.mjs --strict` | 837b9796 clean | Passed | `/tmp/julie-coverage-baseline.txt` |

## Ownership

Tasks 1, 2 and 4 run in parallel under the plan ownership table. Shared manifests, command registration, decisions, evidence, and commits belong to the lead. Workers return reviewed diffs without staging.

## Remaining acceptance

All goal completion criteria remain open until verified against the integrated source. This file records execution evidence, not a completion claim.

## Baseline binary

The preserved binary is `target/coverage-baseline/julie-extract-3.11.0`, copied before candidate builds. It reports `julie-extract 3.11.0`; SHA-256 is `79eb21fa95d39445e78b7237a0300fe071649d5671db0fbc4b3baff0ee2fd78d`, matching `docs/release-evidence/v3.11.0-access-facts.md`. That establishes the published-source baseline without rebuilding from an unproven toolchain. ELF `.comment` inspection confirms the baseline was built with Rust `1.97.1 (8bab26f4f 2026-07-14)` and LLD 22.1.6, matching the current toolchain. Candidate measurements must record their own hash and build environment.

Native Windows execution is available through `win-test`; the guest was shut off during initial inspection. No Windows acceptance result has been claimed.

## Command registration red check

`cargo test -p xtask --test commands_contract commands_route_coverage_before_test_tier_parser` selected one test and failed because `coverage` reached the generic test-tier parser. Log: `/tmp/julie-coverage-route-red.txt`. This is the expected pre-implementation result for the Task 4 CLI registration. The test requires the corpus parser to reject a missing `--language` value, proving dispatch reaches that parser.

## Reference host

Initial host inventory: Linux prax `7.2.13-200.fc44.x86_64`, x86_64, Intel Core i9-12950HX (24 logical CPUs), 62 GiB RAM, Btrfs on `/dev/nvme0n1p3`. Rust `1.97.1 (8bab26f4f 2026-07-14)`, Cargo `1.97.1 (c980f4866 2026-06-30)`, LLVM 22.1.6. Final comparisons must capture the live host/build details again and run without concurrent builds or benchmarks.

## Embedded annotation characterization

The preserved 3.11.0 binary completed eight fresh CLI scans over Vue script-setup and Razor C# attribute controls, one for each extraction level. SQLite `symbol_annotations.annotation_key` matched exact independently specified sets: Vue `defineemits`, `defineoptions`, `defineprops` and Razor `authorize`, `parameter` at references/full; empty at symbols/facts. Every scan had zero parse diagnostics. Generated sources, artifacts and full reports: `target/annotation-level-probe/results.json`. This is baseline behavior evidence, not final-candidate acceptance.

## Retained-boundary focused checks

On the current product source (no extractor implementation changes), `cargo test -p julie-extractors --test framework_context_boundaries` passes both tests. The cases prove signal-free and misleading-string Next.js paths remain unclassified in all four ECMAScript language rows, actual framework signal produces the `/Home` route, and Axum colon literals remain distinct from `{id}` captures. Log: `/tmp/julie-context-boundaries.txt`.

`cargo test -p julie-extractors --test rust_framework_gaps` passes all eight tests. Exact Actix assertions retain one unprefixed local `/status` route and both `/api` and `/v2` mounts, exclude unrelated config types, and preserve shadowing/dynamic-prefix uncertainty. Reqwest cases require local same-impl field/import evidence. Log: `/tmp/julie-rust-boundaries.txt`. These focused checks do not certify the complete 630-cell audit.

## Reproduced local defect HTML-IMPORT-BODY

The direct canonical extraction of `<script src="external.js">inlineText()</script>` emits an Import named `script` with body bytes 26..38 and a body hash. `ScriptStyleExtractor::extract_script_element` chooses Import when `src` exists, then unconditionally applies the paired-tag interior as its body. An external reference does not own that inline implementation. Fix and test ownership is assigned to the Task 1 worker: `crates/julie-extractors/src/html/scripts.rs` and `crates/julie-extractors/src/tests/html/wave2_gaps.rs`, in addition to its validator file. The test must preserve real inline-script bodies. Status: fixed and reviewed under Task 1; final goal integration gates remain pending.

## HTML correction contract marker

`cargo test -p julie-extractors --lib test_public_contract_version_marks_current_fact_families` failed for the added `html-external-script-body-v1` requirement before the marker changed, then passed after the marker was appended. Both runs selected one test. Logs: `/tmp/julie-html-contract-marker-red.txt` and `/tmp/julie-html-contract-marker-green.txt`. Package version, schema 8, report schema 3, and identity epoch 10 remain unchanged. The output ledger distinguishes this unreleased correction from published 3.11.0 behavior.

## Audit preparation

A temporary candidate-proof inventory accounts for all 630 language/domain cells and every advertised supported unit. Relationship-kind evidence includes existing direct and pending relationship families, following the capability validator. No supported unit lacks a candidate positive golden pointer after the five body dispositions. This inventory is only a navigation aid; it does not establish semantic correctness, negative/boundary proof, or audit completion.

## Task 1 reviewed acceptance

The lead read the changed validator/extractor/tests and generated golden, resolved its source-ownership and declaration-containment review findings, inspected `check_edit`, and parsed every final test-result line from the captured logs. All results are successful, with no failed tests. The unchanged `printReport` caller reported by code-kb belongs to Task 2 and uses compatible default parameters; Task 1 introduced no out-of-diff caller mismatch.

Reviewed source digest (SHA-256 over ordered path + NUL + bytes + NUL for the extractor, its two tests, capability manifest, new fixture pair, contract constant and API test): `3b71cf4729f303488c6812508f6b6d517bf14c7b1b1f0b12ddcae7a1c126ff88`. Baseline HEAD is `837b9796` plus these identified changes.

| Scope | Invariant | Command | UTC | Result | Evidence |
| --- | --- | --- | --- | --- | --- |
| Task 1 | External-script negative cases and inline bodies | `cargo xtask test language html` | 2026-11-04 00:49:14 | 76 unit + 1 golden passed | SDD `task-1-verification-logs/html-tier.log` |
| Task 1 | CSS declaration dispositions | `cargo xtask test language css` | 2026-11-04 00:43:27 | 68 unit + 1 golden passed | SDD `task-1-verification-logs/css-tier.log` |
| Task 1 | qmldir directive dispositions | `cargo xtask test language qmldir` | 2026-11-04 00:43:31 | 6 unit + 1 golden passed | SDD `task-1-verification-logs/qmldir-tier.log` |
| Task 1 | Source-backed claims and removal of last proof | `cargo xtask test capability` | 2026-11-04 00:49:40 | 48 capability + 1 pending contract passed | SDD `task-1-verification-logs/capability.log` |
| Task 1 | Token-sensitive hashes and Unicode/CRLF offsets | `cargo test -p julie-extractors body_hash -- --nocapture`; `cargo test -p julie-extractors indexed_content_range_matches_prefix_scan -- --nocapture` | 2026-11-04 00:45:31 | 12 hash + 1 offset check passed | SDD `task-1-verification-logs/body-normalization.log` |

The SDD workspace is `.razorback/sdd/2026-11-03-language-coverage-goal-54245db20e09`. Captured negative mutations fail when case-insensitive source detection, registered-source checking, or declaration containment is removed; restored source passes the final scopes above. Task 1 is ready for its local commit. This does not close Task 3 or final Windows/performance acceptance.

## Source-byte preservation correction

Task 1 landed locally as `bdb5039c`. The staging warning exposed a real checkout defect: the verified HTML fixture had 354 bytes and one CRLF, while Git's `* text=auto eol=lf` stored 353 bytes without CRLF. An exact working-source/committed-blob assertion failed. The source and golden were correct together, but a fresh checkout would not retain that source identity.

Added `-text` for the exact HTML source and `fixtures/real-world/coverage/**` so pinned corpus hashes also survive staging/checkouts. The already-tracked fixture required `git add --renormalize` after the attribute change; an ordinary add reused its cached clean state. Exact staged-byte comparison now passes (354 bytes, one CRLF). `JULIE_GOLDEN_LANGUAGE=html cargo test -p julie-extractors --features test-golden --lib golden_fixtures_match_canonical_extraction` selects one test and passes; its source bytes equal the staged blob. Log: `/tmp/julie-html-preserved-bytes-golden.txt`. This follow-up corrects the local commit without rewriting history.

## Task 2 reviewed acceptance

The lead reviewed the report and CI changes, requested per-unit proof and useful access/annotation/doc-comment guards, then checked the resulting predicates and captured final outputs. All 31 original cells now have source-backed dispositions. Raw output-family counters retain their established meaning. The 14 language-level applicability views are complete; test detection retains per-role validation.

| Scope | Command | UTC | Source identity | Result | Evidence |
| --- | --- | --- | --- | --- | --- |
| Task 2 | `node --test scripts/language-data-quality-report.test.mjs` | 2026-11-04 00:58 | 4c5356c4 plus reviewed Task 2 digest `03e529f69fdb25e2c59bc1fdb45333164e743789c8c47e23f06d1b85236b74a2` | 19 passed, 0 failed | SDD `task-2-node-tests-final.log` |
| Task 2 | `node scripts/language-data-quality-report.mjs --strict --check-applicability` | 2026-11-04 00:58 | same source | 42 languages; silent cells 0; quality debts 0; retained gaps 11; unclassified views 0/14 | SDD `task-2-applicability-final.log` |
| Task 2 | `node scripts/language-data-quality-report.mjs --strict` | 2026-11-04 00:58 | same source | Existing strict checks pass | SDD `task-2-strict-final.log` |

The reviewed digest covers ordered paths and bytes of the report, its Node tests and CI workflow. Body hashes are recomputed by the Rust capability guard; Node additionally rejects malformed/mismatched spans and missing proof. No generic type/relationship rows are inferred from unrelated structural facts.

## Task 3 audit dispatch

Tasks 1–2 are locally committed and reviewed. The audit is divided into non-overlapping language groups, with source/test/fixture fixes assigned per reproduction before any edits. Each group owns a private JSON fragment and report; the lead owns canonical matrix integration, shared manifests and final verification.

Twenty pinned source diagnostic probes are retained in `fixtures/real-world/coverage/diagnostic-probes.json`. Candidate triage includes C macro-qualified function pointers, Roslyn VB syntax, PowerShell product scripts, existing Erlang telemetry, and SQL Server DDL/procedure syntax. Clean corpus alternatives do not dispose of these probes. Their current status is candidate investigation, not a verified local-defect classification.

### Task 3 web annotation-level evidence assignment

The web audit confirmed that four-level annotation assertions currently cover Rust only. Baseline CLI probes prove the intended Vue/Razor behavior, but those probes do not replace permanent assertions. Assigned the web worker exact test ownership of `crates/julie-extractors/src/tests/vue/annotations.rs` and `crates/julie-extractors/src/tests/razor/annotations.rs`. Add canonical extraction checks with exact annotation-key sets and zero diagnostics at all four levels, preserving existing direct-language tests. This is characterization of existing correct behavior; no production correction is indicated by the probes.

## Task 3 validator component

The independent checker/renderer component is reviewed; the real language audit remains open. Worker tests cover missing/duplicate/unknown cells; malformed/foreign-family proof; named tests hidden in strings/comments; source bytes/coordinates/ownership; observed-only families; exact retained IDs; unresolved defects; and safe deterministic Markdown output. The checker shares the report's 15-domain export.

Lead review additionally reproduced an invalid access proof: a `call`/`type_usage` row carrying `access=read` was accepted as variable access. The new selected mutation failed before the fix; `hasVariableAccess` now requires `variable_ref` or `member_access`. The same test preserves positive variable/member cases. Focused and final whole checks pass: 1 and 31 tests respectively, zero failures. Final logs are `/tmp/julie-matrix-access-red.log`, `/tmp/julie-matrix-access-green.log`, and `/tmp/julie-matrix-final-tests.log`; worker lexical-declaration logs are in SDD `logs/`. Reviewed checker/test source digest is `cfad95ab0326275e39fb74ccc2c9ecc9d72f6859650bd49b7179de4f87e82ca7` on top of `59c8df54`. `check_edit` reports no changed callable shapes.

The default real-matrix command still fails for a missing `validation-matrix.json`, intentionally. Only fast validator mutation tests are added to CI at this stage. Canonical data, final 630-cell validation and its CI gate remain Task 3 acceptance work.

### Task 3 native assignment: C-TYPEDEF-BODY-SPAN

The native audit reproduced an extra body kind in the registered C declarator golden: pointer alias `FooPtr` (`type`) inherits the inline struct's body bytes 380..390 and hash. The source helper chooses the type-definition body's span without checking whether the declarator owns it. Assigned exact production/test/fixture ownership to the native worker: `crates/julie-extractors/src/c/mod.rs`, `crates/julie-extractors/src/tests/c/declarators.rs`, and `fixtures/extraction/c/declarators/expected.json`. Prove the no-body pointer-alias regression before fixing it, preserve real named/anonymous struct/union/enum declaration bodies, and regenerate only the assigned golden. Lead owns marker/ledger declaration.

### Task 3 scripting diagnostic reproductions

The scripting audit replayed the PowerShell variable-path argument diagnostics and Erlang documentation/catch macro diagnostics. Assigned only `crates/julie-extractors/src/tests/powershell/import_source.rs` for a minimal valid argument-mode path reproduction. Production/parser ownership remains pending source and official-syntax analysis. Erlang macro cases require further triage before any new boundary or fix is asserted; preserve positive recovered facts and uncertainty.

The web worker also owns `crates/julie-extractors/src/tests/qml/wave2_gaps.rs` for canonical annotation-level characterization. Existing direct QML attachment tests remain; the new test must assert exact keys at all levels, zero diagnostics and declaration source slices. No production QML correction is assumed.

### Task 3 shared annotation-level contract assignment

Assigned `crates/julie-extractors/src/tests/extraction_levels.rs` to the scripting audit worker for one capability-gated all-language annotation-level contract. It must cover all 28 current annotation-claiming languages, select registered diagnostic-free positive fixture sources, compare exact declaration-owner/annotation-key expectations from versioned goldens at references/full, and require absence at symbols/facts. No language may be silently skipped; missing clean proof fails. Name the test `capability_matrix_preserves_annotations_at_every_extraction_level` and gate it with `test-capability-matrix` so the existing capability runner selects it while default stays fast. Existing direct-language and web wrapper span tests remain. This adds required characterization, not new extraction behavior.

### Task 3 SQL correction ownership

Current SQL docs/ledger advertise routines, RETURNS types, parameter/column counts, body spans and PK/FK facts without the old July plan's test-scope boundary. Fresh probes demonstrate missing/misnamed routines, bogus/missing columns and lost constraints. Recorded IDs: SQL-PROCEDURE-HEADER, SQL-TEMPORAL-COLUMNS, SQL-INLINE-TVF, SQL-BRACKETED-CONSTRAINTS. Assign a separate SQL implementer the exact production/test/new-fixture paths listed in SDD `task-3-sql-brief.md`; the domain auditor keeps matrix work. Parser edits require a source-backed proposal and explicit vendor/source assignment, while the lead retains manifests/lock/status/markers. Additional DECLARE-AS type loss remains a candidate requiring an exact source/test assignment. These defects are in scope; older certification coverage cannot be used to reclassify them away.

### Task 3 native body ownership assignments

Assigned GO-NONBODY-SPANS to the native worker: `crates/julie-extractors/src/go/parameters.rs`, `crates/julie-extractors/src/go/specs.rs`, `crates/julie-extractors/src/tests/go/wave2_gaps.rs`, and the exact expected JSON files under `go/test_roles`, `go/types_and_relations`, `go/gocheck_ginkgo`. Function-type parameter parentheses and ordinary composite-literal delimiters are not declaration bodies. Preserve any genuinely owned callable bodies; report additional useful body kinds for lead capability registration rather than inventing N/A.

Assigned SWIFT-PARAMETER-BODY to the same worker: `crates/julie-extractors/src/swift/parameters.rs`, `crates/julie-extractors/src/tests/swift/argument_types.rs`, and `fixtures/extraction/swift/declarations/expected.json`. The function-typed parameter p currently points at `(Element)` as a body; both fields must be absent for a parameter. Preserve its type facts and real function bodies.

### Task 3 web access-evidence assignments

Assigned the web worker exact assertion paths: `tests/typescript/identifiers.rs`, `tests/javascript/identifier_extraction.rs`, `tests/vue/wave2_gaps.rs`, `tests/razor/wave2_gaps.rs`, `tests/qml/identifiers.rs`, `tests/python/identifiers.rs`, `tests/php/identifiers.rs`, `tests/ruby/identifiers.rs`, `tests/lua/identifier_extraction.rs` under the extractor src tree. Add occurrence-by-byte shadowing/access checks and explicit non-vacuous no-access assertions for calls/type usages where those kinds apply. Add standalone variants in new `tests/javascript/jsx_access.rs` and `tests/typescript/tsx_access.rs`; lead owns module registration so existing per-variant filters select them. Existing callee/type-resolution shadowing assertions do not substitute for access classification. Production defects, if any, require exact follow-up ownership.

SQL-TRANSACTION-FACTS is an additional accepted reproduction: the pinned procedure has BEGIN TRAN, COMMIT and ROLLBACK sites but no advertised sql.transaction.v1 rows. The SQL worker's existing base/sql_structural_facts.rs and tests/sql/structural_facts.rs ownership covers its fix; distinguish transaction starts from ordinary BEGIN blocks.

### Parser correction decision and ownership

Astra approved source-local parser corrections under the existing licensed-vendor policy; no remote push is required. Prefer published crates, then unchanged pinned upstream, then a local correction. A full freshness report attempt failed HTTP403 on unrelated VB GitHub metadata; targeted PowerShell checks show published 0.26.6 and upstream 65ea6919 match, fix comma lists, and still lack variable-prefix path arguments. Assign a PowerShell implementer vendor/tree-sitter-powershell plus the exact language consumer/test/fixture/doc paths in SDD task-3-powershell-brief.md, preserving audit_other's existing red test. Root keeps manifests/lock/status/marker/gates.

Official [PowerShell parsing documentation](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_parsing) distinguishes argument-mode variable adjacency from expression mode. The correction must preserve those boundaries and actual variable facts, not merely erase diagnostics.

The SQL worker is also assigned `vendor/tree-sitter-sequel-tsql/` for a small licensed correction if targeted published/current-owned-upstream checks cannot preserve existing certified behavior and close the proven cases. Base source is the current owned fork63ea933e464813d01cab5d7febcb0f77409c247b (MIT), with distinct version/provenance and generator0.26.11. No source preprocessing or warning filtering. The same worker may edit `src/sql/types.rs` if the post-grammar DECLARE-AS type regression still needs local semantic adaptation; its tests remain in the already-owned SQL files. Root retains all shared dependency wiring.

### Task 3 native parameter-attribute reproduction

The Roslyn `IgnoreAccessibilityBinder.vb` probe diagnoses the valid `<[In]>` parameter attribute before `<Out> ByRef` at line 20. Assigned the native auditor `crates/julie-extractors/src/tests/vbnet/wave2.rs` and `fixtures/extraction/vbnet/parameter_attributes/{source.vb,expected.json}` for the exact failing reproduction. Grammar ownership follows targeted published/upstream inspection; root retains dependency and fixture registration. A clean alternate VB corpus file does not close this defect.

The C `alloc.c` probe instead depends on the included header's `TS_PUBLIC` macro definition before four function-pointer globals. The source-local extractor does not expand included headers. Record the actual macro definition, recovered facts and diagnostic spans as a context boundary; do not invent a definition or silently suppress the diagnostics.

### Task 5 implementation dispatch

Assigned the performance worker `xtask/src/performance.rs`, new submodules beneath `xtask/src/performance/`, comparison contract tests, affected baseline contract tests and the opt-in performance workflow. Preserve the existing baseline and writer commands. Implement all ten performance acceptance rules from the approved plan, including exact semantic row deltas, prepared update/no-change state, calibrated alternating batches, native memory collection and noise/repeat rules. Timed corpus measurements wait for the corrected candidate and an otherwise idle host. Root owns output allowlist contents, shared dependencies, final measurements and integration evidence.

### Task 4 lead review follow-up

The initial runner implements actual isolated CLI scans and exact SQLite assertions for all 42 samples plus two executed controls. Working-source hashes match every manifest entry. Lead requested offline copies of required license/NOTICE evidence, strict malformed-report handling and additional manifest misuse checks before integration. The selected SQL sample's nine inherited diagnostics remain a Task 3 defect; passing its current count assertion is not a healthy-corpus claim.

### Task 3 source-region assertion assignment

The web auditor found that `supported_languages_with_source_region_syntax_emit_regions` proves only presence of each expected kind and permits extra fabricated regions. Assigned that function in `crates/julie-extractors/src/tests/source_regions.rs` and necessary local helpers to the web worker. Add meaningful kind/span and confusable-absence assertions for the eleven audited web languages; preserve existing assertions for other languages. Any production defect requires separate ownership before edits.

### Task 3 Erlang macro catch correction assignment

The latest published `tree-sitter-erlang` 0.21.0 and upstream `62c82e67b73e13f3ef06e4c8c07ff035a8235d30` have identical grammar source and retain the catch-arrow gap. Assign a separate worker `vendor/tree-sitter-erlang/`, `crates/julie-extractors/src/tests/erlang/wave1_gaps.rs` and new `fixtures/extraction/erlang/macro_catch_head/{source.erl,expected.json}`. Base a distinct vendored version on 0.21.0; preserve actual upstream Apache LICENSE and document/correct its conflicting Cargo MIT metadata. Add a catch-specific macro-head/no-arrow-body branch, with distinct AST fields that cannot be mistaken for pattern bindings. No include expansion, fabricated bindings or diagnostic filtering. Source review indicates existing recursive extraction can preserve real calls and complete function bodies without production Rust changes; request exact ownership if that assumption fails. Root retains dependency/fixture registration, status, output marker, certification and final gates.

### Task 3 retained Phoenix boundary assertion assignment

Assign `crates/julie-extractors/src/tests/elixir/wave2_gaps.rs` to the scripting auditor for the retained `phoenix.cross_file_scope_prefix` context boundary. Prove local forwarding/mount facts and multiple registrations while keeping another file's router prefix unknown. Do not add project-wide resolution. Existing in-file nested routes remain positive proof.

### Task 3 HTML annotation evidence assignment

The existing HTML annotation N/A classification contradicts useful annotations already emitted from Babel script classes. Assign `crates/julie-extractors/src/tests/html/annotations.rs` and `fixtures/extraction/html/annotations/{source.html,expected.json}` to the domain auditor for exact positive, undecorated/confusable and host-offset proof. Inspect member kinds as well as classes before updating claims. Root owns test module and capability registration; the shared annotation contract must derive claiming languages from the ledger, including HTML after corrected proof lands.

### Task 3 VB parameter-attribute parser correction

The exact Roslyn syntax is two adjacent parameter attribute blocks, `<[In]> <Out> ByRef`. Pinned `LuisCarreras/tree-sitter-vb-dotnet` 25dca4ac1456c691e2381a2d76151ef432aefc9e accepts only one optional parameter attribute block. Published 0.1.0 and current CodeAnt-AI upstream retain that limitation and omit other required pinned-fork behavior. Assign the native auditor `vendor/tree-sitter-vb-dotnet/` in addition to the already-owned VB test/fixture paths. Base a distinctly versioned licensed correction on the current pinned fork, permit repeated adjacent parameter attribute blocks while preserving ordinary and single-block parameters, regenerate with CLI0.26.11 and retain malformed controls. Root owns Cargo/lock/workspace/status wiring, fixture registration, output declaration and final certification/Windows gates.

## Task 3 reviewed native body-ownership slice

C-TYPEDEF-BODY-SPAN, GO-NONBODY-SPANS and SWIFT-PARAMETER-BODY are reviewed. The C check preserves real aggregate bodies and covers pointer, array and function-pointer aliases. Go preserves a direct function literal's exact body/hash and the composite initializer's inferred type while removing false parameter/initializer bodies. Swift preserves the function body while clearing the function-type parameter's body. All five changed goldens differ only in body_span/body_hash fields, now null; no symbols or type facts are lost.

The source digest over the fourteen ordered source/test/golden/marker paths is `9c3c59d5d6dc47f287ca36283c7ee70f57403af902e5ff09ce9fce7a8c20b11a` on top of `7257ec1f`. Final focused checks pass: C typedef-related filter16 tests, Go1, Swift1. Logs: `/tmp/julie-native-c-reviewed-green.log`, `/tmp/julie-native-go-reviewed-final.log`, `/tmp/julie-native-swift-reviewed-green.log`. Earlier integration attempts encountered transient Rust syntax/helper errors in concurrently authored Erlang/Elixir tests; their owners corrected those before the successful runs. Native worker language tiers pass C141, Go173, Swift182 plus one selected golden each; captured SDD `task-3-native-*-language-final.log`. Final focused source has the same production changes and stronger preservation assertions. Scoped rustfmt and diff whitespace checks pass. `check_edit` reports no changed callable shapes.

Root declares `declaration-body-ownership-v1` in the extraction contract and output ledger; its API assertion failed before the marker and passed afterward. Captured logs `/tmp/julie-native-body-marker-red.log`, `/tmp/julie-native-body-marker-green.log`; final recheck is `/tmp/julie-native-body-marker-final.log`. This completes only the native body-ownership slice. Matrix, parser corrections and all goal integration gates remain open.
