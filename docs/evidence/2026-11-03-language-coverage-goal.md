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
