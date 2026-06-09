# TODO

Lightweight tracker for open, agreed-but-not-yet-done work on the extraction
product. One section per item, each with a concrete file reference, why it
matters, and the proposed fix. No "later" placeholders.

Status legend: `open` (verified present), `partial` (partly done), `idea`
(proposed, not committed to), `done` (implemented and verified).

---

## 1. No `cargo-deny` supply-chain / license / advisory gate — done

- **Where:** `deny.toml`; `.github/workflows/ci.yml`.
- **What changed:** Added a cargo-deny policy covering advisories, SPDX license
  allow-listing, duplicate/wildcard warnings, exact git-source allow-listing,
  and an explicit compatibility decision for `md5@0.7.0`.
- **Verification:** `cargo deny check` passes locally. Duplicate versions and
  path/git wildcard requirements are warnings; advisories, license policy, and
  unknown dependency sources are hard gates. CI runs
  `EmbarkStudios/cargo-deny-action@v2` with `--all-features`.

## 2. Evaluate migrating standalone `md5` 0.7 to RustCrypto `md-5` — done

- **Where:** `crates/julie-extractors/Cargo.toml:74` (`md5 = "0.7"`); usages in
  production ID/hash paths:
  `crates/julie-extractors/src/base/{extractor.rs,types.rs,body.rs,results_normalization.rs}`;
  expected-value helpers also use it under `crates/julie-extractors/src/tests/`.
- **Decision:** Keep `md5@0.7.0` as an explicitly allowed compatibility
  dependency. The MD5 output is part of stable legacy extraction IDs and body
  hashes, so changing crates would be an artifact identity migration rather than
  a supply-chain cleanup.
- **Verification:** `deny.toml` explicitly allows only `md5@0.7.0` with the
  compatibility reason above; strict workspace clippy and the path-identity /
  contract gates pass locally.

## 3. Legacy `julie-extractors` clippy warnings not gated (residual of F20) — done

- **Where:** `.github/workflows/ci.yml`.
- **What changed:** CI now runs
  `cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings`,
  covering the core extractor crate and test targets.
- **Verification:** strict workspace clippy passes locally.

## 4. Execute the 2026-06-05 project hardening review plan — done

- **Where:** `docs/plans/2026-06-05-project-hardening-review.md`.
- **What changed:** F1-F19 are implemented and marked fixed in the plan.
  Regression coverage was added for artifact correctness, JSONL atomicity,
  parser provenance, language detection, release preflight, default-suite
  tripwires, and workflow guardrails.
- **Verification:** `cargo xtask test default`, `cargo xtask test contract`,
  `cargo test -p xtask`, and strict workspace clippy pass locally.

## 5. TypeScript generic client-call URL literals are missing — done

- **Where:** `crates/julie-extractors/src/typescript/identifiers.rs`;
  `crates/julie-extractors/src/tests/typescript/literals.rs`;
  `crates/julie-extract-cli/tests/operations_contract.rs`.
- **Root cause:** For `await axios.get<T>(...)`, tree-sitter TypeScript reports
  the call callee through an awaited node whose fallback text was
  `await axios.get`. The literal was captured, but carrier classification only
  recognizes `axios.get`, so the language-policy pass dropped the row before it
  reached SQLite/JSONL.
- **What changed:** TypeScript callee carrier normalization now strips the
  `await ` prefix before carrier classification. Regression coverage verifies
  raw extractor output and the real CLI SQLite `literals` table for generic
  `axios.get<T>` and `axios.put<T>` calls.
- **Verification:** Focused raw TypeScript literal test passes; focused CLI
  operations contract passes; a MyraNext services smoke scan now persists
  `/api/messages/active` with `kind = url`, `carrier = axios.get`, and
  containing symbol `getActiveMessages`.

## 6. OpenClaw-scale SQLite writer perf guard for current schema — done

- **Where:** `xtask/src/performance.rs`; `xtask/Cargo.toml`;
  `xtask/tests/performance_baseline_contract.rs`;
  `xtask/tests/commands_contract.rs`; `docs/testing-strategy.md`.
- **What changed:** Added a non-default
  `cargo xtask performance writer-current-schema` command that generates a
  deterministic current-schema artifact workload, writes it through the real
  `ArtifactWriter`, and records `artifact.sqlite` plus
  `writer-current-schema-summary.json` under `target/`.
- **Coverage:** The generated workload includes files, symbols,
  symbol annotations, identifiers, relationships, pending relationships,
  type facts, type argument usages, type arguments, literals, source regions,
  parse diagnostics, and revision file changes. The default run wrote 10,000
  files, 80,000 symbols, 240,000 identifiers, 120,000 source regions, and a
  235,573,248-byte SQLite artifact. Elapsed write time, rows/sec, and artifact
  size are report-only metrics, not CI thresholds.
- **Verification:** Focused parser/runner tests and top-level routing tests pass.
  The manual evidence run completed:
  `cargo xtask performance writer-current-schema --out-dir target/performance/writer-current-schema`.
  Branch-gate verification passes with `cargo test -p xtask`,
  `cargo xtask test default`, and `cargo xtask test contract`.

## 7. Structural tree-sitter query facts for downstream tools — partial

- **Where:** New contract/design under `docs/contracts/` and/or `docs/plans/`;
  likely extractor surfaces under `crates/julie-extractors/src/base/`,
  `crates/julie-extractors/src/language_spec/`, and per-language modules;
  artifact surfaces under `crates/julie-extract-artifact/src/{schema.rs,writer.rs,jsonl.rs,model.rs}`.
- **Implemented slice:** The current v2 artifact now has a `structural_facts`
  SQLite table, `structural_fact` JSONL records, report row counts, writer
  coverage, CLI scan coverage, and current-schema performance coverage. The
  extractor emits `rust.unsafe_block.v1` facts for Rust `unsafe { ... }`
  blocks with capture name, matched node kind, span, optional containing
  symbol, confidence, and pattern metadata.
- **Remaining scope:** Expand from the Rust unsafe-block starter pattern into a
  representative parser-backed language/pattern set, then add explicit
  capability metadata once the matrix is meaningful. Candidate next patterns
  should be chosen contract-first and fixture-backed.
- **Guardrail:** Do not add a generic downstream search engine or a Miller/Eros
  query DSL here. This repo should emit versioned extraction facts. Interactive
  querying, ranking, dashboards, and commercial workflows belong downstream.
- **Verification:** Contract tests prove SQLite/JSONL/report shape, writer tests
  prove row persistence and counts, extractor tests prove the Rust unsafe-block
  capture, the CLI operations contract proves non-empty scan output, and
  `cargo xtask performance writer-current-schema` exercises the row family.

## 8. Cross-language AST/code complexity metrics — open

- **Where:** New contract/design under `docs/contracts/` and/or `docs/plans/`;
  likely extractor surfaces under `crates/julie-extractors/src/base/` and
  per-language modules; artifact surfaces under
  `crates/julie-extract-artifact/src/{schema.rs,writer.rs,jsonl.rs,model.rs}`;
  capability evidence in `fixtures/extraction/capabilities.json` and
  `crates/julie-extractors/src/tests/capability_matrix.rs`.
- **Finding:** The repo has regex-pattern complexity, but not a stable
  cross-language code/AST complexity contract for symbols or files.
- **Why it matters:** Miller can rank context, flag hard-to-read areas, and feed
  Eros risk dashboards only if complexity metrics are emitted as extraction
  facts. Computing them in Miller would duplicate parser traversal and would
  silently miss languages.
- **Proposed fix:** Define a v1 `complexity_metrics` contract with conservative
  metrics that can be measured consistently across languages: symbol id or file
  id, metric scope, lines/bytes covered, branch/decision count, loop count,
  nesting depth, parameter count where applicable, and a versioned algorithm id.
  Start with parser-backed languages where the metric semantics are clear; mark
  unsupported or not-applicable language/kind gaps explicitly in the capability
  matrix.
- **Guardrail:** Avoid claiming language parity until fixture evidence exists.
  Avoid a single opaque "quality score" in the extractor. Emit primitive,
  versioned metrics; downstream tools can decide how to rank or present them.
- **Verification target:** Add fixture-backed metrics for at least a small
  cross-language matrix, contract tests for SQLite/JSONL output, capability
  rows for supported and open-gap languages, and one real-repo dogfood report
  showing row counts by language and metric scope.

## 9. Clone-ready body fingerprints beyond current `body_hash` — exact-hash contract complete

- **Where:** Existing normalized body hash logic in
  `crates/julie-extractors/src/base/body.rs`; symbol model fields in
  `crates/julie-extractors/src/base/types.rs` and
  `crates/julie-extract-artifact/src/model.rs`; artifact writers in
  `crates/julie-extract-artifact/src/{schema.rs,writer.rs,jsonl.rs}`;
  docs in `docs/contracts/{sqlite-schema-v2.md,jsonl-v2.md}`.
- **Finding:** Symbols already expose `body_hash` when body spans are available.
  That hash is useful for exact normalized-body matches. The exact-hash contract
  now has a documented algorithm id, normalization rules, and guardrails in the
  SQLite/JSONL contracts.
- **Why it matters:** Miller can consume exact duplicate facts cheaply, and Eros
  can build higher-level clone/risk workflows, but only if the extractor emits
  stable, documented fingerprints instead of forcing downstream tools to
  re-tokenize source.
- **Completed slice:** Kept the existing `body_hash` field, defined algorithm
  id `julie-normalized-body-md5-v1`, made normalization ignore whitespace and
  language-appropriate comments, and added tests for exact-match stability and
  contract wording.
- **Deferred slice:** Add a separate machine-readable fingerprint surface only
  if downstream consumers need token counts, SimHash, token n-grams, or other
  near-duplicate candidate signals beyond exact normalized-body matches.
- **Guardrail:** Do not make the extractor decide product-level duplicate
  severity. Emit stable fingerprints and counts; Miller/Eros can group, rank,
  threshold, and present duplicates downstream.
- **Verification:** Focused tests now prove whitespace/comment-stable exact
  hashes, intentional non-matches when executable tokens differ, quoted string
  preservation, and SQLite/JSONL v2 contract stability.
