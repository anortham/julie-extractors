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

## 11. Per-file extraction cost attribution in reports — complete

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
- **Completed slice:** Added stable `counts.file_rows` entries with `path`,
  `language`, `status`, `total_rows`, and exhaustive per-row-family `rows`
  counts. Successful scan reports include a bounded largest-file summary with
  `counts.file_rows_truncated`; `info --json` includes the full persisted
  per-file breakdown. Attribution is computed as a read-side SQLite view over
  existing artifact tables, so the writer and SQLite schema stay unchanged.
- **Verification:** RED tests failed for the missing report type/field and
  missing `info --json` attribution before implementation. After implementation,
  `cargo test -p julie-extract-artifact --test report_contract`, `cargo test -p
  julie-extract-cli --test operations_contract`, `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets --all-features --no-deps -- -D
  warnings`, `cargo xtask test contract`, and `cargo xtask test default` pass.

## 12. Depth-bounded tree traversal — complete

- **Where:** Shared traversal helpers in
  `crates/julie-extractors/src/base/tree_methods.rs`; parse-diagnostic walks in
  `crates/julie-extractors/src/pipeline.rs`; parser-comparison walks in
  `crates/julie-extractors/src/language_spec/mod.rs`; follow-up audit for raw
  per-language recursive walkers under `crates/julie-extractors/src/**`.
- **Why it matters:** Many extractor paths recurse through tree-sitter nodes.
  Rust stack overflow can abort the process, so CLI panic isolation is not a
  complete safety boundary for adversarially deep source trees. Shared helpers
  should enforce a fixed traversal depth budget before language-specific
  cleanup happens.
- **Completed slices:** Added a shared internal traversal depth budget, enforced
  it in the base traversal helpers, parse-diagnostic walks, and C/C++ header
  parser-comparison error walks. Focused tests prove `walk_tree`,
  `traverse_tree`, and `find_nodes_by_type` do not visit nodes beyond the
  budget. A follow-up slice guarded the direct recursive symbol walkers in
  Rust, Go, Java, C#, VB.NET, and C++; the Rust regression first reproduced a
  real stack overflow from a deeply nested `function_item`.
- **Completed follow-up:** The main per-language extraction phases now use the
  shared depth budget across direct symbol, identifier, relationship, pending
  relationship, and data-language relationship walks. This includes the
  JavaScript/TypeScript canonical passes, Dart/QML pending-call walks, Vue
  script/template relationship and symbol walks, HTML identifier/resource
  pending walks, YAML alias walks, Zig relationships, and the supported
  code/data language identifier and relationship visitors.
- **Completed helper pass:** Shared structural-fact, source-region, complexity,
  type-argument, string-literal, SQL/web/framework/data structural helpers, and
  remaining language helper recursion now route through the same traversal
  budget. A stricter recursive-function audit reports zero unguarded true
  child-recursive tree-sitter walkers.
- **Verification:** The deeply nested Rust and JavaScript regressions pass.
  `cargo xtask test language` passes for Rust, JavaScript, TypeScript, C, C++,
  C#, Go, Java, Kotlin, Scala, Swift, Python, Ruby, PHP, Dart, GDScript, QML, R,
  Lua, Elixir, Bash, PowerShell, SQL, JSON, TOML, YAML, Markdown, Regex, Razor,
  CSS, HTML, VB.NET, Zig, and Vue. `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings`,
  and `cargo xtask test default` pass.

## 13. Split CLI command orchestration — complete

- **Where:** `crates/julie-extract-cli/src/commands.rs`.
- **Why it matters:** The command module is over 3,200 lines and `scan()`
  mixes path handling, existing artifact checks, discovery, extraction spooling,
  writing, report shaping, profiling, and exit-code mapping. That makes CLI
  contract changes harder to localize and review.
- **What changed:** Split stable helper families into focused internal modules:
  `capability_snapshot.rs` owns capability/parser fingerprint and snapshot
  mapping, `reports.rs` owns report/output/error mapping, and
  `artifact_access.rs` owns read-only artifact opening, metadata/version
  checks, root checks, content-hash loading, artifact report assembly, row
  totals, and JSONL count mapping. `commands.rs` now keeps command dispatch,
  high-level scan/update/delete orchestration, extraction spooling, and
  write-flow decisions.
- **Guardrail:** Added CLI convention tests that fail if capability snapshot,
  report/error, or artifact-access helpers drift back into `commands.rs`.
- **Verification:** Focused red/green convention tests passed after each move.
  `cargo test -p julie-extract-cli --test cli_contract`,
  `cargo test -p julie-extract-cli --test operations_contract`, and
  `cargo test -p julie-extract-cli --test path_policy` pass locally.
  `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings`,
  and `cargo xtask test default` also pass.

## 14. Modularize artifact writer internals — complete

- **Where:** `crates/julie-extract-artifact/src/writer.rs`.
- **Why it matters:** `ArtifactWriter` earns its public interface, but the file
  combines capability snapshot sync, revision semantics, row-family insertion,
  deletion, prepared statements, and the data-loss guard. New row families have
  to touch several distant sections.
- **What changed:** Kept the public `ArtifactWriter` API and transaction
  orchestration in `writer.rs`, and moved stable private helper families into
  focused submodules. `writer/capabilities.rs` owns capability snapshot key
  loading, deletions, upserts, and JSON/boolean mapping. `writer/rows.rs` owns
  file/child row inserters, row-family insert functions, preserved-failure row
  updates, parse-diagnostic replacement, and symbol/identifier/type-argument
  lookup helpers.
- **Guardrail:** Added writer convention tests that fail if capability sync or
  row-inserter helper definitions drift back into `writer.rs`.
- **Verification:** Focused red/green convention tests passed. `cargo test -p
  julie-extract-artifact`, `cargo test -p julie-extract-artifact --test
  writer_performance`, `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings`,
  and `cargo xtask test default` pass locally.

## 15. Reuse tree-sitter parsers per scan worker — complete

- **Where:** `crates/julie-extractors/src/pipeline.rs`.
- **Why it matters:** `parse_for_language` creates and configures a fresh
  `tree_sitter::Parser` for every file. That is a Rust-specific cold-scan cost
  on large repositories with many files of the same language.
- **Decision:** Do not add a thread-local parser cache now. The profile did not
  confirm meaningful scan-time savings, and a cache would add mutable
  thread-local state to a hot parser path for a very small measured win.
- **Evidence:** A focused local profiler compared 2,000 tiny Rust parses through
  the current `parse_for_language` path against one configured parser reused for
  all parses. Debug mode measured 41 ms vs. 38 ms; release mode measured 10 ms
  vs. 8 ms. That is at most a few milliseconds across 2,000 tiny files, so
  parser setup is not currently a top bottleneck.
- **Verification:** The profiling was run with:
  `cargo test -p julie-extractors pipeline::tests::parser_setup_profile_reports_reuse_baseline -- --ignored --nocapture`
  and
  `cargo test -p julie-extractors --release pipeline::tests::parser_setup_profile_reports_reuse_baseline -- --ignored --nocapture`.
  The temporary profiler/test scaffolding was removed after recording the
  evidence.

## 16. Identifier target resolution leaves unambiguous same-artifact symbols unresolved — open

- **Where:** the resolution pass that populates `identifiers.target_symbol_id`
  (consumed by Miller as `reference_sites.is_exact` / `provenance='target_token'`).
- **Why it matters:** span emission is healthy, but *target* resolution is not, and
  the two are easy to conflate. On a real 400k-site C# corpus (Miller's own
  workspace, julie-extract 2.18.0, schema 5), `reference_sites` carries exact spans
  for **78.4%** of C# sites — yet only **11.1%** of C# `identifiers` rows resolve to
  a `target_symbol_id`. Downstream, anything gated on exact target resolution
  (reference counts, callers/callees, rename coverage, dead-code candidates) sees a
  small fraction of the real graph.

- **Measured breakdown (C#, 308,933 identifiers):**

  | kind | rows | resolved | % |
  |---|---|---|---|
  | `variable_ref` | 151,426 | 7,440 | 4.9 |
  | `call` | 85,410 | 16,605 | 19.4 |
  | `member_access` | 44,938 | 96 | **0.2** |
  | `type_usage` | 27,159 | 10,086 | 37.1 |

- **This is not C#-specific.** Same corpus, `target_symbol_id` resolution rate by
  language: powershell 49.5%, bash 40.9%, javascript 28.2%, python 23.0%,
  csharp 11.1%, razor 9.8%, css/html 0.0%. C# is simply the largest corpus here,
  so it surfaces the gap most visibly. Treat this as a general resolution-pass
  item, not a C# extractor item.

- **The tractable subset.** Many unresolved rows are legitimately unresolvable —
  locals, parameters, and calls into the BCL/LINQ (`ThrowIfNull`, `Select`,
  `ToList` all appear in the unresolved sample and correctly have no
  same-artifact target). But a large subset is *not* explainable that way. Counting
  only unresolved C# rows whose `name` matches **exactly one** symbol in the same
  artifact (unambiguous by name alone, no type inference required):

  | kind | unresolved despite a unique same-artifact symbol |
  |---|---|
  | `variable_ref` | 17,414 |
  | `call` | 17,112 |
  | `member_access` | 8,705 |
  | `type_usage` | 527 |

  Relaxing to "name exists as any symbol" gives 64,661 / 36,198 / 28,563 / 8,362.
  The unique-name column is the interesting one: those need no overload or
  receiver-type reasoning to resolve correctly.

- **Proposed fix:** treat `member_access` as the first target — 0.2% is low enough
  to suggest the receiver path is not being walked at all, rather than being walked
  and failing. Then add a conservative unique-name fallback tier for `call` and
  `type_usage`: when exactly one symbol in the artifact bears the name and no
  in-scope binding shadows it, resolve to it and mark the tier so consumers can
  distinguish it from a type-checked resolution. Keep it strictly opt-out-able —
  Miller already separates exact from fallback evidence and must keep being able to.

- **Caveat before acting:** confirm the unique-name subset is not dominated by
  workspace symbols that coincidentally share a BCL name (e.g. a local `Select`).
  Sample it per kind first; if a meaningful share is coincidental, the fallback tier
  needs a shadowing/import check rather than a bare name match.

- **Verification:** re-run the two queries above against a fresh extract of a large
  C# workspace and compare the resolved percentages and the unique-name residual.
  The numbers above were measured 2026-07-27 on Miller's `.miller/symbols.db`
  (`artifact-1785123621446275000`, binary_version 2.18.0, sqlite schema 5).
