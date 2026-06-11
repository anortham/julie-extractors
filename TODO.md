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
  structural facts, complexity metrics, parse diagnostics, and revision file
  changes. The default run wrote 10,000 files, 80,000 symbols, 240,000
  identifiers, 120,000 source regions, 10,000 structural facts, 90,000
  complexity metrics, and a 270,106,624-byte SQLite artifact. Elapsed write
  time, rows/sec, and artifact size are report-only metrics, not CI thresholds.
- **Verification:** Focused parser/runner tests and top-level routing tests pass.
  The manual evidence run completed:
  `cargo xtask performance writer-current-schema --out-dir target/performance/writer-current-schema`.
  Branch-gate verification passes with `cargo test -p xtask`,
  `cargo xtask test default`, and `cargo xtask test contract`.

## 7. Structural tree-sitter query facts for downstream tools — complete

- **Where:** New contract/design under `docs/contracts/` and/or `docs/plans/`;
  likely extractor surfaces under `crates/julie-extractors/src/base/`,
  `crates/julie-extractors/src/language_spec/`, and per-language modules;
  artifact surfaces under `crates/julie-extract-artifact/src/{schema.rs,writer.rs,jsonl.rs,model.rs}`.
- **Done:** The current v3 artifact has a `structural_facts` SQLite table,
  `structural_fact` JSONL records, report row counts, writer coverage, CLI scan
  coverage, and current-schema performance coverage. The extractor now emits a
  representative fixture-backed pattern set: Rust unsafe blocks, Go goroutine
  launches and defer statements, Python decorated definitions,
  JavaScript/JSX/TypeScript/TSX await expressions, and C/C++ preprocessor
  definitions.
- **Capability metadata:** `fixtures/extraction/capabilities.json`, Rust
  capability snapshot APIs, persisted SQLite `language_capabilities`, and
  `julie-extract languages --json` now publish exact
  `kind_coverage.structural_facts.supported` pattern ids.
- **Guardrail:** Do not add a generic downstream search engine or a Miller/Eros
  query DSL here. This repo should emit versioned extraction facts. Interactive
  querying, ranking, dashboards, and commercial workflows belong downstream.
- **Verification:** Contract tests prove SQLite/JSONL/report shape, writer tests
  prove row persistence and counts, extractor tests prove every advertised
  structural pattern, capability-matrix tests prove fixture-backed evidence,
  the CLI operations contract proves non-empty scan output and metadata
  publication, and `cargo xtask performance writer-current-schema` exercises
  the row family.

## 8. Cross-language AST/code complexity metrics — complete

- **Done:** The current v3 artifact has a `complexity_metrics` SQLite table,
  `complexity_metric` JSONL records, report row counts, writer coverage, CLI
  scan persistence, current-schema writer guard coverage, and contract docs.
- **Done:** Extractors now emit primitive parser-backed metrics with
  `algorithm_id = julie-ast-complexity-v1`: `file` and `symbol` scope,
  covered lines/bytes, decision count, loop count, max nesting depth, and
  parameter count where applicable.
- **Done:** The first fixture-backed matrix covers `c`, `cpp`, `go`,
  `javascript`, `python`, `rust`, and `typescript`. Capability evidence
  advertises supported scopes through
  `kind_coverage.complexity_metrics.supported`; unsupported languages publish
  no metric-scope claims.
- **Guardrail kept:** The extractor emits facts only. It does not emit a single
  opaque quality score, severity, ranking, threshold, or dashboard decision.
- **Evidence:** `docs/release-evidence/2026-06-09-complexity-metrics-dogfood.md`
  records a real-repo scan with 6943 `complexity_metrics` rows and counts by
  language and metric scope. Focused extractor, artifact, CLI, capability, and
  xtask tests pass for this slice.

## 9. Clone-ready body fingerprints beyond current `body_hash` — done

- **Where:** Existing normalized body hash logic in
  `crates/julie-extractors/src/base/body.rs`; symbol model fields in
  `crates/julie-extractors/src/base/types.rs` and
  `crates/julie-extract-artifact/src/model.rs`; artifact writers in
  `crates/julie-extract-artifact/src/{schema.rs,writer.rs,jsonl.rs}`;
  docs in `docs/contracts/{sqlite-schema-v3.md,jsonl-v3.md}`.
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
- **Decision:** Do not add a separate machine-readable fingerprint surface in
  the current v3 artifact. Token counts, SimHash, token n-grams, and other
  near-duplicate candidate signals need a named downstream requirement before
  they become public SQLite/JSONL contract surfaces.
- **Decision record:** `docs/decisions/0002-clone-fingerprint-scope.md`
  captures the clone-fingerprint scope boundary and future-entry criteria.
- **Guardrail:** Do not make the extractor decide product-level duplicate
  severity. Emit stable exact fingerprints; Miller/Eros can group, rank,
  threshold, and present duplicates downstream.
- **Verification:** Focused tests prove whitespace/comment-stable exact hashes,
  intentional non-matches when executable tokens differ, quoted string
  preservation, and SQLite/JSONL v3 contract stability. The remaining deferred
  surface was closed by Decision 0002, not by adding speculative artifact rows.

## 10. ASP.NET minimal API, htmx, and Alpine structural facts — done

- **Where:** `docs/plans/2026-06-09-aspnet-htmx-alpine-structural-facts.md`;
  planned extractor surfaces under
  `crates/julie-extractors/src/base/framework_structural_facts.rs`,
  `crates/julie-extractors/src/registry.rs`, C#/HTML/Razor fixture directories,
  and `fixtures/extraction/capabilities.json`.
- **Why it matters:** C#, Razor, HTML, and JavaScript parser support already
  covers the syntax, but downstream tools need durable framework facts for
  minimal API routes, htmx request attributes, and Alpine directives before
  Miller can reliably index and bridge this stack.
- **Completed slice:** Emits `aspnet.minimal_api.route.v1`,
  `htmx.attribute.v1`, and `alpine.directive.v1` through the existing
  `structural_facts` row family with fixture-backed capability metadata. The
  extractor records static ASP.NET minimal API route templates, htmx attributes
  with request metadata where applicable, and Alpine long-form/shorthand
  directive metadata. htmx-to-ASP.NET route linking remains downstream in
  Miller, not in this repo.
- **Verification:** Focused structural-fact tests pass; `cargo xtask test
  language csharp`, `cargo xtask test language html`, and `cargo xtask test
  language razor` pass; capability-matrix structural checks pass; golden
  fixture checks pass; `languages --json` capability snapshot test passes; a
  real three-file CLI smoke scan writes `aspnet.minimal_api.route.v1`,
  `htmx.attribute.v1`, and `alpine.directive.v1` rows; `cargo xtask test
  default` and `cargo xtask test contract` pass.

## 11. Per-file extraction cost attribution in reports — open

- **Where:** `crates/julie-extract-cli` (`info` command and scan report),
  `crates/julie-extract-artifact` report surfaces, `docs/contracts/cli.md`.
- **Why it matters:** The consumer-side vendor policy
  (`docs/decisions/2026-06-11-vendor-policy-consumer-side.md`) routes
  exclusions through `--ignore-file` and `.julieignore`, but consumers can
  only write good ignore rules if they can see which files dominate artifact
  rows. On openclaw, ten committed `*.tm.jsonl` i18n files produced ~39% of
  all structural facts — discoverable today only with ad-hoc SQL against
  schema internals. A stable report surface lets consumers such as Miller
  automate vendor detection by measured impact instead of name-pattern
  guesses, and makes artifact size regressions attributable.
- **Proposed fix:** Design pass first, then implement per-file row counts by
  row family as a report contract: a top-N offender summary in the scan
  report and/or a full per-file breakdown in `info --json`. Treat the JSON
  report shape as an API contract change with docs and focused tests. This is
  the named closure for the vendor-policy decision's follow-up debt.
