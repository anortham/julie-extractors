# Language coverage verification evidence

Implementation begins from `837b9796` in `.worktrees/language-coverage-plan` on `docs/language-coverage-plan`. Baseline product source is `f9a1b6cf`. All fourteen worktrees were clean before implementation. The goal authorizes local changes and commits; push and publication remain outside scope.

| Scope | Invariant | Command | Source | Result | Evidence |
|---|---|---|---|---|---|
| baseline | Existing strict quality contract | `node scripts/language-data-quality-report.mjs --strict` | 837b9796 clean | Passed | `/tmp/julie-coverage-baseline.txt` |

## Ownership

Tasks 1, 2 and 4 run in parallel under the plan ownership table. Shared manifests, command registration, decisions, evidence, and commits belong to the lead. Workers return reviewed diffs without staging.

## Remaining acceptance

Overall goal acceptance remains open. This file records the gates that passed and the work that is still pending; the latest summary is below.

## Current body policy

Definition and value bodies need not be executable. Preserve Bash assignment
RHS values, F# let-binding RHS values, C# property accessor lists, recognized
R class-factory argument payloads, QML type-info descriptor interiors, Erlang
config values and empty CSS braced rules. Programming parameters identified by
`metadata.role=parameter` have no body; QML descriptors use `typeinfo_kind`
and remain distinct. Code module headers, import operands, signal signatures
and accidentally selected nested expressions do not gain bodies from textual
delimiter inference. This policy supersedes contrary candidate notes below.

The central parameter regression reproduced a TypeScript callback body of
`()` before the fix. The factory now skips body inference for parameter rows.
The canonical TypeScript/TSX function/object type and decorator cases, Kotlin
function type and Zig generic type argument cases retain callable bodies and
clear parameter spans/hashes. Focused body tests pass 10/10 in
`target/coverage-parameter-final.log`; the marker red check fails as expected
in `target/coverage-parameter-marker-red.log`. The marker green check selects
one passing test (`target/coverage-parameter-marker-green.log`). Kotlin passes
194 unit tests plus its golden comparison; Zig passes 137 plus its golden
comparison (`target/coverage-parameter-{kotlin,zig}-final.log`). Their six
corrected parameter rows change only body spans and hashes.

## Baseline binary

The preserved binary is `target/coverage-baseline/julie-extract-3.11.0`, copied before candidate builds. It reports `julie-extract 3.11.0`; SHA-256 is `79eb21fa95d39445e78b7237a0300fe071649d5671db0fbc4b3baff0ee2fd78d`, matching `docs/release-evidence/v3.11.0-access-facts.md`. This is the measured local final 3.11.0 candidate, not the public Linux asset (SHA-256 `008372ec7a61f0921ef687d20e24c67547255f603d7dbca08710f8e27bef28bc`). The publication evidence records identical comparable fact tables for those binaries. The historical invocation is unknown. An isolated clean-source rebuild at `f9a1b6cf` with the recorded locked offline release recipe reproduced this exact executable hash; the hash-bound source, toolchain, feature graph and recipe are archived in `2026-11-03-language-coverage/baseline-build.json` and `baseline-reproduction.md`. The runner toolchain is recorded separately from binary provenance. ELF `.comment` inspection confirms the baseline was built with Rust `1.97.1 (8bab26f4f 2026-07-14)` and LLD 22.1.6, matching the current toolchain. Candidate measurements must record their own hash and build environment.

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

This was the audit's initial inventory, not its final disposition. The lead-owned canonical matrix and the current validation result are recorded in "Integrated verification status" below.

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

The independent checker/renderer component was reviewed before the real language audit finished. Its worker tests cover missing/duplicate/unknown cells; malformed/foreign-family proof; named tests hidden in strings/comments; source bytes/coordinates/ownership; observed-only families; exact retained IDs; unresolved defects; and safe deterministic Markdown output. The checker shares the report's 15-domain export. The lead later integrated the canonical matrix; its final status is below.

Lead review additionally reproduced an invalid access proof: a `call`/`type_usage` row carrying `access=read` was accepted as variable access. The new selected mutation failed before the fix; `hasVariableAccess` now requires `variable_ref` or `member_access`. The same test preserves positive variable/member cases. Focused and final whole checks pass: 1 and 31 tests respectively, zero failures. Final logs are `/tmp/julie-matrix-access-red.log`, `/tmp/julie-matrix-access-green.log`, and `/tmp/julie-matrix-final-tests.log`; worker lexical-declaration logs are in SDD `logs/`. Reviewed checker/test source digest is `cfad95ab0326275e39fb74ccc2c9ecc9d72f6859650bd49b7179de4f87e82ca7` on top of `59c8df54`. `check_edit` reports no changed callable shapes.

The initial validator handoff intentionally failed closed because `validation-matrix.json` did not exist yet. The lead has since integrated the 630-cell matrix, and the current checker passes. The final matrix status and log are recorded below.

### Task 3 native assignment: C-TYPEDEF-BODY-SPAN

The native audit reproduced an extra body kind in the registered C declarator golden: pointer alias `FooPtr` (`type`) inherits the inline struct's body bytes 380..390 and hash. The source helper chooses the type-definition body's span without checking whether the declarator owns it. Assigned exact production/test/fixture ownership to the native worker: `crates/julie-extractors/src/c/mod.rs`, `crates/julie-extractors/src/tests/c/declarators.rs`, and `fixtures/extraction/c/declarators/expected.json`. Prove the no-body pointer-alias regression before fixing it, preserve real named/anonymous struct/union/enum declaration bodies, and regenerate only the assigned golden. Lead owns marker/ledger declaration.

### Task 3 scripting diagnostic reproductions

The initial scripting audit replayed PowerShell variable-path argument diagnostics and Erlang documentation/catch macro diagnostics. The PowerShell worker later vendored the 0.26.6 grammar with adjacent-variable path parsing and corrected import/body ownership; its focused import suite passed 16/16 and its language tier passed 165 tests plus a golden comparison. The Erlang worker later added the macro catch-head grammar branch and variable/body corrections; the vendored 231-case grammar corpus passed, and the feature-gated Telemetry extraction test passed with no parse diagnostics. The broader Erlang release profile remains under review because the final symbol total differs from the old expected count, as recorded below.

The web worker also owns `crates/julie-extractors/src/tests/qml/wave2_gaps.rs` for canonical annotation-level characterization. Existing direct QML attachment tests remain; the new test must assert exact keys at all levels, zero diagnostics and declaration source slices. No production QML correction is assumed.

### Task 3 shared annotation-level contract assignment

Assigned `crates/julie-extractors/src/tests/extraction_levels.rs` to the scripting audit worker for one capability-gated all-language annotation-level contract. It must cover all 28 current annotation-claiming languages, select registered diagnostic-free positive fixture sources, compare exact declaration-owner/annotation-key expectations from versioned goldens at references/full, and require absence at symbols/facts. No language may be silently skipped; missing clean proof fails. Name the test `capability_matrix_preserves_annotations_at_every_extraction_level` and gate it with `test-capability-matrix` so the existing capability runner selects it while default stays fast. Existing direct-language and web wrapper span tests remain. This adds required characterization, not new extraction behavior.

### Task 3 SQL correction ownership

Current SQL docs/ledger advertise routines, RETURNS types, parameter/column counts, body spans and PK/FK facts without the old July plan's test-scope boundary. Fresh probes demonstrated missing/misnamed routines, bogus/missing columns and lost constraints. The SQL worker fixed SQL-PROCEDURE-HEADER, SQL-TEMPORAL-COLUMNS, SQL-INLINE-TVF, SQL-BRACKETED-CONSTRAINTS and the related SQL-TRANSACTION-FACTS and SQL-TSQL-COMPOUND-ASSIGNMENT defects. The separate SQL-DECLARE-AS-TYPE correction preserves the declared type in `DECLARE @AreaCode AS NVARCHAR(4)`; `tsql_declare_as_keeps_the_declared_type` covers it. Older certification coverage did not excuse these defects.

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

Root declares `declaration-body-ownership-v1` in the extraction contract and output ledger; its API assertion failed before the marker and passed afterward. Captured logs `/tmp/julie-native-body-marker-red.log`, `/tmp/julie-native-body-marker-green.log`; final recheck is `/tmp/julie-native-body-marker-final.log`. At that slice's handoff, the matrix and parser corrections were still open. The matrix and source corrections are now integrated; remaining acceptance is listed below.

### Additional source-owned body corrections

The scripting auditor verified two false body families from exact source slices. Bash readonly constants (`readonly MAX=3`) claim the initializer `3` as a body. Assign `src/bash/variables.rs`, `tests/bash/wave2_gaps.rs` and the Bash basic/scripts/commands goldens to that auditor. Clear non-owning variable/constant bodies while preserving real function bodies and useful facts.

PowerShell `#Requires -Modules Az.Accounts, @{ ModuleName = 'Pester'; ModuleVersion = '5.0' }` gives both imports the options hashtable as a body. Assign the dedicated PowerShell worker its already-owned imports.rs/import_source.rs plus `fixtures/extraction/powershell/module/expected.json` for the correction. Nonempty bytes and hashes alone do not justify new body support claims.

The HTML annotation fixture exposed JSX field `@Input() name;` inheriting decorator parentheses `()` as its body. Assign the web auditor new `tests/web_body_spans.rs`, `src/typescript/interfaces.rs::extract_property` and `src/javascript/types.rs::extract_property` for source-local property corrections and exact web body/hash controls. Root registered the test module. Preserve genuine field function bodies. The existing TypeScript language_gaps and JavaScript/JSX agent_usefulness goldens are assigned to the web worker; root coordinates the new HTML annotations golden with the domain auditor. New HTML claims are class/field/method annotation support and method symbols/bodies, backed by the registered fixture. Canonical symbols keep inner JSX language attribution and host source coordinates.

Assign `tests/powershell/wave2_gaps.rs` to the scripting auditor for nested function body exact-slice/hash boundaries, separate from the dedicated PowerShell import tests. Root wired the reviewed PowerShell and Erlang vendor paths, exclusions, dependency status and fixture registrations. Cargo resolved only the two intended parser package changes; consumer builds use checked-in C inputs. Build commands use `target/coverage-tmp` as TMPDIR after the shared system tmpfs hit its quota while writing generated-parser assembly.

The XML auditor challenged the current complexity N/A classification with an already-supported XSLT template function containing conditions and loops. A read-only architecture review is checking the product boundary and the smallest honest metric implementation before any shared complexity code is assigned.

### XML XSLT metric implementation boundary

Astra confirmed this is a false blanket N/A under the already-approved XSLT vocabulary. Assign the domain auditor new `src/xml/complexity_metrics.rs`, XML source/test module registration, `tests/xml/complexity_metrics.rs`, a dedicated `xml/complexity/source.xslt` fixture and affected existing XML expected goldens. Root keeps registry dispatch, capability scope advertisement, manifests, applicability, docs and marker changes. Reuse `julie-ast-complexity-v1` counts: XSLT if/when decisions, for-each loops, decision/loop nesting and direct callable param declarations. Exclude with-param, duplicate choose/otherwise decisions and XPath-expression internals. Resolve the XSLT namespace URI in one traversal, including prefix rebinding; plain/foreign XML does not become code. Cover anonymous match templates in file totals and named template/function ownership, full-level-only output and deterministic operation-count scaling.

### Native access characterization assignment

Assign a separate native access worker the following exact test paths beneath `crates/julie-extractors/src/tests/`: `rust/identifiers.rs`, `c/identifier_extraction.rs`, `cpp/identifier_extraction.rs`, `go/identifiers.rs`, `zig/identifiers.rs`, `swift/identifiers.rs`, `kotlin/identifiers.rs`, `scala/mod.rs`, `java/identifier_extraction.rs`, `dart/identifiers.rs`, `csharp/identifier_extraction.rs`, `vbnet/identifiers.rs`. Existing read/write/member/compound tests stay. Add exact outer/inner same-name occurrences and non-vacuous Call/TypeUsage no-access assertions where those kinds are claimed. The original native auditor keeps matrix/VB work and does not edit these test files. Production access fixes require a separate source-backed ownership assignment.

Bash golden ownership additionally includes `bash/bats/expected.json`: TMP command substitution is another literal initializer incorrectly treated as a constant body. Preserve its source and clear only non-owning body fields through the same variable extractor correction.

### ECMAScript import and variable body corrections

The web auditor verified false bodies in all four dialects: ES import specifier lists `{ Foo }`/`{ foo }` and ordinary initializer argument lists `()`/`(routes)`. Extend its source ownership to `typescript/imports_exports.rs::extract_import`, `typescript/functions.rs::extract_variable`, `javascript/imports.rs::create_import_symbol` and `javascript/variables.rs::extract_variable`. Add four-dialect regressions in the already-owned web_body_spans tests, preserving real callable variable/function bodies. This worker owns affected registered expected goldens throughout `fixtures/extraction/{typescript,tsx,javascript,jsx}/`; review changes as body-only deltas and request ownership for any other language. Root keeps capability N/A decisions, marker and output ledger changes; HTML remains coordinated with the domain auditor. Export wrappers owning actual function/class/interface definitions and type definition payloads remain separate from external references and ordinary initializer arguments.

Bash source ownership also includes `bash/helpers.rs::body_span` so its incorrect assignment-value body rule can be removed at the producer. Verify a deliberate no-body language hook cannot fall through to generic delimiter inference; remove redundant constructor clearing when unnecessary.

### Scripting access characterization assignment

Assign an independent worker the following exact test paths beneath `crates/julie-extractors/src/tests/`: `elixir/identifiers.rs`, `fsharp/wave2.rs`, `erlang/identifiers.rs`, `r/superassignment.rs`, `bash/wave1_gaps.rs`, `powershell/identifier_extraction.rs`, and `gdscript/identifier_extraction.rs`. Close missing source-specific shadowing and non-vacuous call/type no-access boundaries while reusing real receiver/member/compound tests. Do not fabricate TypeUsage proof for a language that does not claim that identifier kind. The scripting auditor keeps its matrix, Bash producer correction, Phoenix and annotation contract paths; these seven files belong to the independent access worker.

### VB parameter annotation ownership correction

Parser-side repeated attributes exposed the next local defect: `attach_vbnet_attribute_owners` rejects every Variable symbol, including the existing parameter symbol at the attribute owner's exact start byte. Assign the native auditor only `src/base/code_structural_facts.rs::attach_vbnet_attribute_owners` plus necessary private indexing helpers/imports in that file. Permit Variable owners only for actual parameter declarations; preserve non-variable declaration owners and file-targeted assembly/module attributes without invented owners. Build symbol-owner lookup once per invocation rather than scanning all symbols for each attribute. The already-owned VB test/fixture must prove adjacent attributes, preserved parameter types/owners, ordinary/single-block/malformed controls and non-vacuous attribution. Root keeps capability/snapshot/marker integration. These are structural attribute facts; do not count them as generic Symbol.annotations support without separate emitted-marker evidence.

### Additional source-local body implementation ownership

Assign a dedicated scripting-body worker `src/elixir/parameters.rs`, `src/fsharp/declarations.rs (read-only policy context)`, `src/r/mod.rs`, `src/r/idioms.rs`, and `src/gdscript/signals.rs` for the proven parameter-pattern, ordinary value/import and signal-signature false bodies. It also owns new `tests/scripting_body_spans.rs` and affected registered Elixir/F#/R/GDScript expected goldens. Root registers the test module and reconciles capability claims. Preserve actual callable/lambda/active-binding/type bodies and all useful facts; no shared generic fallback removal.

Astra's R decision preserves recognized R6/S4/reference/S7 class definitions but replaces accidental delimiter inference with the factory's actual argument-node span. These DSL payload bodies include class-name/configuration arguments; no rename-insensitivity is claimed. Prove nested methods, fields-only/empty classes, argument ordering, member/method-change hashes and comment/Unicode boundaries.

Assign the Erlang correction worker `src/erlang/identifiers.rs`, optional local `variable_access.rs` plus module registration, `src/erlang/{mod,attributes,definition_forms,term_config,helpers}.rs` and local body production as needed for ordinary-variable occurrence facts and proven false module/macro/field/config body spans. Tests `tests/erlang/identifiers.rs` transfer after the access worker's stable handoff, plus dedicated synthetic body tests/fixtures; affected registered Erlang goldens are owned by this worker. Root retains shared access-policy changes, markers and capability decisions. Use syntax-local occurrence binding state, not type inference's function-wide BodyBindings aggregation; preserve single assignment, clause/fun/branch/generator scope and unresolved dynamic calls.

### PowerShell computed dot-source assertion correction

The old module-command test expects a static Common import from computed `Join-Path` output. Assign only `tests/powershell/wave2_gaps.rs::module_commands_emit_one_row_per_member_and_narrow_unexported_functions` to the PowerShell correction worker to remove that fabricated-target expectation and assert retained Join-Path call/literal evidence and no invented static import. The scripting auditor's newly added nested-body test remains intact. Do not format this shared file concurrently.

Assign `tests/elixir/literals.rs` to the scripting auditor for a genuine non-call string negative literal assertion, retaining the existing argument-position boundary test.

The real Telemetry test is feature-gated outside default. Root added it to real-world-release routing; the route expectation failed before wiring and all24 test-tier contracts passed afterward (target/coverage-erlang-routing-red.log and -green.log).

### Body-policy correction supersedes the Bash/F# candidates

The September Bash decision explicitly defines assignment values as bodies; the F# adapter explicitly returns a binding's RHS expression; C# explicitly owns a property's accessor list. Non-executability alone does not invalidate a body. Restore Bash production behavior and goldens, assert exact RHS spans and hashes, and advertise the already-emitted Constant body kind. Keep F# record-valued bindings and C# auto-property accessor lists. Earlier Bash/F# false-body candidate statements in this chronological ledger are superseded, not accepted defects. The R class DSL payload boundary is explicitly its factory argument node; imports, signatures, parameter fragments and accidentally selected nested bodies remain distinct.

The scripting-body worker may read F# declarations and add positive body-policy tests, but does not clear or change its producer. It owns Elixir parameter, R import/value/class-body and GDScript signal corrections plus new scripting_body_spans tests and a dedicated R body_semantics fixture when needed. Exact source ownership is Elixir parameters.rs, R mod.rs/idioms.rs, GDScript signals.rs. Retain real callable/type/R6 active-binding bodies.

The confirmed access claimants are33 languages, including XML; CSS access is N/A. Assign the domain auditor new HTML/SQL/XML identifier_access_coverage test files for source-specific exact access/shadowing/no-access proof, and a CSS negative characterization file. Root registers HTML/CSS/SQL modules; the worker already owns XML test-module registration. Do not invent runtime assignment/lexical-scoping semantics for declarative XML or CSS.

### Web body follow-up ownership

Extend the web auditor's production ownership to `javascript/exports.rs::extract_export_rows`, `qml/imports.rs`, the signal and ordinary-local-variable branches of `qml/mod.rs::traverse_node`, `lua/core.rs::extract_bare_require_import`, and the Ruby-only assignment handling in `ruby/assignments.rs::extract_assignment`. Preserve Python behavior in the shared Ruby assignment helper and real lambda/proc/function bodies. Export lists and re-exports reference external definitions; inline function/class/interface/arrow declarations retain their actual definition body, including expression-bodied arrows. QML reactive property binding semantics are outside the local-variable cleanup. The web worker owns affected registered expected goldens throughout its eleven audited languages, except the new HTML annotations fixture retained by the domain worker. Root retains claims and output declarations.

The web auditor has finished `tests/source_regions.rs`. Transfer only `supported_languages_with_source_region_syntax_emit_regions` and its necessary local exact-region helpers to the native auditor. Preserve the eleven completed web cases and strengthen native/remaining existing table cases to exact semantic source slices/byte spans, with explicit absence rather than kind presence. No production changes follow from this test ownership.

### Narrow follow-up production ownership

Assign the PowerShell worker `src/powershell/variables.rs` and reuse/visibility changes only for `type_facts.rs::value_core`: ordinary hash/array/call initializer fragments must not become implementation bodies, while directly owned scriptblock bodies remain exact. The worker's existing import_source tests and registered PowerShell goldens cover the correction; root reconciles the real Variable/Constructor body claims.

Add `src/qml/locals.rs::extract_declaration` to the web worker's ordinary-local-variable body correction; do not conflate reactive property bindings with local initializers.

Allow the Erlang access worker only the visibility change to `type_facts.rs::pattern_fields` so the occurrence pass reuses the existing grammar vocabulary without importing function-wide type-binding state.

The native exact-region test reproduced Swift string text `/* not a comment */` also emitted as Comment. Assign the native auditor `src/base/source_regions.rs::collect_node` plus necessary local context helpers for a source-backed, Swift-specific correction. Preserve comments inside genuine interpolation code and nested literal handling; do not stop traversal of all strings across languages. Inspect actual AST kinds first. Root owns affected capability/marker/golden coordination.

### Swift literal and Razor comment-hash boundaries

The native worker also owns new Swift literal/interpolation/Unicode tests in source_regions.rs and `fixtures/extraction/swift/source_regions/{source.swift,expected.json}`. The pinned Swift grammar exposes comment-looking literal text as comment nodes, while actual interpolation code has distinct ancestors. Keep genuine interpolation comments and nested strings with a Swift-specific traversal context; do not suppress all string descendants.

Assign the web worker `razor/mod.rs::extract_symbols` and a reusable crate-private AST comment-masking helper. Collect actual comment-node ranges once, mask only those bytes in one full-source copy (retain CR/LF and byte length), then rehash body spans against that same masked source. Do not copy/reparse the full file for each symbol or globally treat markup URLs as C# line comments. Prove component and method comment-only stability plus meaningful markup/string/code changes. Root updates body-evidence recomputation to use the same grammar-derived normalization context once per Razor fixture, retaining original source bytes for span validation.

### Remaining named evidence and Erlang body ownership

Assign the domain auditor HTML complexity_metrics.rs, CSS body_spans.rs/literals.rs, qmldir source_regions.rs, and additions in HTML types.rs/literals.rs for source-backed positive/confusable/boundary assertions. Root registers new modules. Empty braced rule/accessor/member-list bodies remain valid; negative assertions must follow actual language policy.

The native auditor owns only the PowerShell branch of source_regions.rs::config_for_language for wrapper/child string-region deduplication and its five granted PowerShell goldens (cross_file, test_roles, script_features, module, reference_targets). Preserve here/verbatim/interpolated/nested literals.

Assign the Erlang worker attributes.rs::extract_module/extract_macro/extract_record_field: no code module-header or record-field type-call bodies; macro bodies use the actual replacement field. Preserve explicit term-config VALUE bodies and record/type definition payloads. The separate body negative test in tests/erlang/symbols.rs belongs to the scripting auditor; macro positive coverage belongs to the Erlang worker.

Canonical local IDs: ECMASCRIPT-NONCALLABLE-FIELD-BODY is recorded once in html.body_spans; web variant cells cross-reference it. VBNET-PARAMETER-ATTRIBUTE-GRAMMAR belongs to vbnet.symbols, VBNET-PARAMETER-ATTRIBUTE-OWNER to vbnet.structural_facts, XML-XSLT-COMPLEXITY to xml.complexity_metrics, SQL-TSQL-COMPOUND-ASSIGNMENT to sql.identifier_metadata, SWIFT-LITERAL-COMMENT-REGION to swift.source_regions and POWERSHELL-DUPLICATE-STRING-REGION to powershell.source_regions.

Task5's one-pass methodology review found unverified intermediate update states/fresh equivalence, prebuilt-binary provenance attributed to the runner, incompatible baseline/candidate fact expectations, disjoint allowlist-scope rejection and missing normalized metrics. Root verified these against current source and returned the complete component for correction before timing. The pinned79eb baseline is the measured local final3.11 binary; the public3.11 binary is008372ec, with18-table equivalence recorded in publication evidence. Do not relabel or silently replace the selected baseline.

### SQL access producer ownership

The compound-assignment parser regression now reaches the source-fact stage. Assign the SQL worker `src/sql/identifiers.rs` and only SQL-specific rules/helpers in `src/base/variable_access/wave2b.rs` for UPDATE-column write and SET-variable read/write facts. Verify expected identifier kind against the existing SQL contract; preserve unrelated language rules and all Call/TypeUsage access exclusions. The domain auditor retains its standalone access test; the SQL worker owns producer tests in its already-assigned files. The generated SQL corpus is500/500 and focused T-SQL tests47/47 before this final access correction.

### Literal negative and body-policy evidence tests

Assign a separate web-literal evidence worker existing `tests/{typescript,javascript,vue,qml,python,php,ruby,lua}/literals.rs` and new `tests/javascript/jsx_literals.rs`/`tests/typescript/tsx_literals.rs`. Source-verify each producer's literal contract before choosing negatives; do not exclude useful native numeric/data literals by assumption. Keep TSX/JSX direct variant extraction evidence, exact carriers/positions/spans, and nonempty positives. Root registers new modules; production gaps require a new exact source assignment.

The scripting auditor also owns R/GDScript literals.rs no-call string negatives, R identifiers.rs member/VariableRef exclusion, F# gap_closure.rs/semantic_facts.rs/mod.rs documentation/type-argument/abstract-member controls, and GDScript wave1_gaps.rs exact callable-body boundaries. Their existing Erlang symbols.rs negative uses the module and record fields, while keeping the defined open/1 body positive.


## Frozen corpus and original-baseline expectations

The manifest SHA-256 is `fb6e6a8a286acd8900094023d5d8c2b110695eb459c35b52bdfb642fd1a58ec6` after setting the corrected SQL candidate diagnostic count to zero. Erlang Telemetry's diagnostic probe now points to its actual pinned revision `8d8af76720856bcf26641ee1307658ad8f25e466`; its exact source hash remains unchanged. The manifest contract regression rejects revisions borrowed from unrelated registered projects.

The untimed original-baseline capture produced ten snapshots across references/full levels and original/updated/restored source states. Every snapshot scanned 44 files with zero failed/unsupported files; original-baseline diagnostics remain one malformed Rust diagnostic and nine SQL diagnostics. The candidate scan and unresolved release-profile comparison are recorded below. The receipt is archived at `2026-11-03-language-coverage/baseline-receipt/baseline-expectations.json`, SHA-256 `ae5b1e5ec8500d5b4e7c46b25a6961a9ce56128becfe4e4474f314d90dbf404e`. Four compressed raw semantic dumps and their compressed/uncompressed checksums are archived alongside it. No timings are claimed by this capture.

## Source-local C diagnostic boundary

All twenty diagnostic probes index without failed or unsupported files. SQL now has zero diagnostics. Four C diagnostics occur at raw `TS_PUBLIC` prefixes in the pinned tree-sitter `alloc.c`. The same revision's `alloc.h` defines that macro as empty on Windows/hidden builds and otherwise as `__attribute__((visibility("default")))`; header SHA-256 is `2ad7cc5391be8043ffdcc111b9b21b7767f592790fc8389acb6fcee7fd1acb61`. Explicit synthetic controls replacing the prefixes with each actual branch both parse without diagnostics. These controls establish the external-header dependency; production does not read hidden headers, preprocess text or suppress diagnostics. Raw source diagnostics remain visible.

## Additional exact producer ownership

The web worker owns `csharp/members.rs::extract_method`, its `tests/web_body_spans.rs` regression and Razor `members/expected.json` for `RAZOR-METHOD-SIGNATURE-BODY`: `IValidatable.Validate();` must not own `()`, while `FormModel.Validate() => true` retains its expression body. The earlier shared delegate correction and C#/Razor interpolation helper are separately tested. The SQL `DECLARE @AreaCode AS NVARCHAR(4)` correction is `SQL-DECLARE-AS-TYPE`; it advances past the optional grammar `AS` token and preserves the actual type.

## Shared body-hash correction evidence

The 139-source probe first reproduced comment-only hash changes in interpolation, F# and PowerShell comments, nested Rust block comments, and Lua long-bracket comments. Those findings are now covered by canonical regressions. The shared `SyntaxBodyHashIndex` normalizes syntax-node comments and interpolation code from one cached source/grammar index while retaining literal payloads, delimiters and format text. `body_hash_comments` passes all 11 tests in `target/coverage-tmp/body-hash-comments-final-wrapper-run.log`; the composed Vue host/script regression passes in `.razorback/sdd/2026-11-03-language-coverage-goal-54245db20e09/audit-web-vue-hash-final.log`. Tests cover comment-only stability and meaningful literal, format, markup and code changes.

The output contract records `syntax-aware-body-hash-v1`, `html-external-script-body-v1`, `declaration-body-ownership-v1`, `programming-parameter-body-v1`, and `language-coverage-corrections-v1` in `docs/contracts/extraction-output-changes.md`. The markers preserve SQLite schema 8, report schema 3, extraction contract v4 and identity epoch 10. The final canonical golden comparison passed in `target/coverage-final-golden-update.log`. Its reviewed delta inventory is archived in the [golden field delta file](2026-11-03-language-coverage/golden-field-deltas.json) and captured at `target/coverage-final-golden-field-deltas.json`. It contains 24 expected files and 40 changed fields, all under `/symbols/.../body_hash`; SHA-256 `f3c35b99198882726c38e6956f1fdd3ac5cec505de38d3a892cda9402abf8fe6`.

The earlier Vue body-ownership refresh changed 48 `body_hash` values: 11 were recalculated and 37 were removed with their false body spans. The removed bodies belonged to imports, ordinary variables, export references, macro call sites, and signature-only event/model declarations. This was reviewed separately from the final hash-only delta inventory; `.razorback/sdd/2026-11-03-language-coverage-goal-54245db20e09/audit-web-report.md` records the distinction and the passing Vue regression.

## Integrated verification status

The current lead-owned matrix has 630 cells and 46 local defects, all recorded as fixed. `node scripts/check-language-coverage-matrix.mjs` reports `language coverage matrix valid: 630 cells` in `target/coverage-final-matrix.log`. The Node test run passes 50/50 in `target/coverage-final-node-tests.log`. The strict quality report covers 42 languages with `silent_cells: 0`, `quality_bar_debts: 0` and 11 open structural-fact gaps in `target/coverage-final-quality.log`.

The latest default tier passes in 33 seconds (`target/coverage-final-default-rerun.log`). The fresh candidate CLI corpus scan covers 42 language samples and two controls, with zero failed or unsupported files (`target/coverage-final-corpus.log`); the malformed Rust control retains its expected diagnostic. The full contract rerun completes all 13 commands successfully, including the three reference-site identity tests (`target/coverage-final-contract-rerun.log`). The xtask suite, dependency deny check, and exact Clippy run pass in `target/coverage-final-xtask.log`, `target/coverage-final-deny.log` and `target/coverage-final-clippy-exact-clean.log`. The all-target format check exits 0; its empty log is `target/coverage-final-fmt-all-clean.log`.

The focused certification run and real-world smoke run pass in 62 and 92 seconds, respectively (`target/coverage-final-certification.log` and `target/coverage-final-real-world-smoke.log`). The Linux rebind-equivalence tests pass 4/4 in 2.70 seconds (`target/coverage-final-rebind.log`). `compat-check` against the pinned `79eb21fa` baseline emits the declared NOTICE from the 3.11.0 output ledger (`target/coverage-final-compat.log`). Full accounting of all table differences and the performance allowlist remain pending.

The real-world release profile passes all six commands after the Erlang baseline review (`target/coverage-final-real-world-release-after-update.log`, 13.67 seconds). Only Telemetry changes its exact expected counts: 52 to 60 symbols and 45 to zero diagnostics. Eight added parameter/local facts are source-backed; the other nine corpus files retain their expected counts. The focused three-test Erlang corpus rerun also passes. The original failed log remains historical evidence. No controlled baseline/candidate performance comparison or Windows run has been completed. The raw gate logs are archived with deterministic gzip and checksums in the [gate artifact index](2026-11-03-language-coverage/integration-gates/artifacts.json). Keep final release and coverage-goal acceptance open until those gates and the remaining performance/default-runtime acceptance checks are complete.

The authored changes pass the staged whitespace check. The byte-pinned baseline feature-tree capture and vendored upstream SQL/VB files retain upstream whitespace; they are excluded only from this formatting check so their recorded source hashes stay valid. No fixture assertion or extraction check is excluded.
