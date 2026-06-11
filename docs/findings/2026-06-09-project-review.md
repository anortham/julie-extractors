# Project Review Findings — 2026-06-09 (post-v2.2.1)

Full-project review run after the v2.2.1 release. Four parallel review passes
(extractor quality, architecture/contracts, performance/robustness,
tests/CI/release) with direct verification of every high-severity claim before
inclusion. Two raw findings were refuted during verification and are recorded
at the bottom so they are not re-reported later.

Overall assessment: no critical bugs. Core engineering (crash resilience,
incremental correctness, test guardrails) is production-grade and matches the
documented contracts. The items below are hygiene fixes, two cold-scan
performance opportunities, and structural debt that will slow future work if
left to compound.

## 1. Verified problems

### 1.1 No MSRV declared; workspace failed to build on rustc 1.90 (HIGH)

- Evidence: `libsqlite3-sys 0.38.0`'s build script uses unstable `cfg_select!`
  and fails with E0658 on rustc 1.90.0. Reproduced locally during this review.
  CI uses `dtolnay/rust-toolchain@stable`, which masks the requirement.
- Impact: contributors and downstream builders on slightly older toolchains
  get a confusing dependency compile error with no stated minimum.
- Fix: add `rust-version` to `[workspace.package]` in the root `Cargo.toml`;
  optionally add an MSRV check to CI.
- Status: local toolchain was updated to rustc 1.96.0 on 2026-06-09 and
  `cargo clippy --workspace --all-targets` now passes with a single lint
  (`manual_strip` at `crates/julie-extractors/src/utils/paths.rs:24`). The
  `rust-version` pin is still outstanding.

### 1.2 Stale release workflow version defaults (HIGH, trivial fix)

- Evidence: `.github/workflows/release-binaries.yml:9` and
  `.github/workflows/specialist-gates.yml:9` both default to `2.1.0`; current
  released version is 2.2.1.
- Root cause: a version bump touches 8+ files (3 crate `Cargo.toml`s, README,
  release-notes docs, 2 workflow defaults) with no bump automation, so the
  workflow defaults went stale two releases ago.
- Fix: update the defaults now; longer term add `cargo xtask release bump`
  (or derive workflow version from the tag) so one command updates every
  location and `release preflight` verifies them.

### 1.3 No cargo caching in CI (MEDIUM)

- Evidence: `.github/workflows/ci.yml` has no `Swatinem/rust-cache` or
  `actions/cache` step; every PR rebuilds the full dependency tree.
- Fix: add `Swatinem/rust-cache@v2` before the build steps.

### 1.4 Parsers recreated per file (MEDIUM, cold-scan performance)

- Evidence: `crates/julie-extractors/src/pipeline.rs:109-126` —
  `parse_for_language` calls `configured_parser_for_language`, which runs
  `Parser::new()` + `set_language()` for every file.
- Impact: the most promising lead on the deferred cold-scan optimization
  (openclaw cold scan ~88s for 12,781 files). Estimated single-digit-percent
  to ~15% gain; profile before and after to confirm.
- Fix: thread-local per-language parser cache inside the rayon workers.

### 1.5 Regexes compiled per call in extractor hot paths (MEDIUM)

- Evidence: 75 `Regex::new` call sites across 30+ extractor files. Some run
  per node or per doc comment, e.g.
  `crates/julie-extractors/src/javascript/mod.rs:634-647` recompiles two
  regexes on every doc comment it inspects. Others are already `LazyLock`
  statics, so the pattern is inconsistent rather than uniformly wrong.
- Impact: regex compilation is expensive; this stacks with 1.4 on cold-scan
  cost.
- Fix: audit the 75 sites and move per-call compilations to `LazyLock`
  statics.

### 1.6 Panic site in Dart extractor (LOW severity, cheap fix)

- Evidence: `crates/julie-extractors/src/dart/mod.rs:147` calls
  `node.parent().unwrap()` mid-extraction.
- Impact: contained product-wide by the `catch_unwind` wrapper
  (`crates/julie-extract-cli/src/extraction.rs:117-138`), so a trigger would
  not crash a scan — it would silently degrade that file to
  `FailedPreserved`.
- Fix: replace with a `let Some(parent)` guard.

### 1.7 Dart dart3 generic-modifier recovery path is dead code (LOW, discovered during fix execution)

- Evidence: `crates/julie-extractors/src/dart/mod.rs:137` (and the sibling
  check near `:247`) gate the recovery path on `parent.kind() == "program"`,
  but tree-sitter-dart 0.2.0 has no `program` node — its root is
  `source_file` (verified against the grammar's node-types.json and live
  parse dumps during the 2026-06-09 fix work; riverpod-style constructs now
  parse cleanly as `class_declaration`).
- Impact: the recovery path never executes. Either the guard should say
  `source_file` (re-enabling the recovery behavior) or the path should be
  deleted. Activating it is a behavior change and needs its own decision.

### 1.8 C# return-type inference is substring-fragile (LOW, discovered during fixture work)

- Evidence: `crates/julie-extractors/src/csharp/type_inference.rs:46`
  (`infer_method_return_type`) locates the method name in the signature via
  `part.contains(&symbol.name)`. Any earlier signature token containing the
  method name — e.g. an attribute argument like `[Obsolete("use NewHelper")]`
  on a method named `Helper` — corrupts `resolved_type` (observed: `int`
  became `[Obsolete("use`).
- Fix direction: match the name token by exact identifier (or walk the AST
  node for the return type) instead of substring containment.

## 2. Structural debt (not urgent, compounding)

### 2.1 `commands.rs` god module

- `crates/julie-extract-cli/src/commands.rs` is 3,228 lines / 77 functions.
  `scan()` alone is ~280 lines mixing discovery, extraction orchestration,
  artifact writing, and report generation. No sub-modules.
- Recommendation: split into discovery/orchestration/reporting/error-mapping
  modules; keep `commands.rs` as a thin dispatcher.

### 2.2 `writer.rs` organization

- `crates/julie-extract-artifact/src/writer.rs` is 2,419 lines with ~60
  insert/upsert helpers and capability-sync logic interleaved. Adding a new
  row domain requires finding insertion, deletion, and sync points scattered
  through the file.
- Recommendation: group insert helpers by domain and extract capability-sync
  into its own module.

### 2.3 `registry.rs` copy-paste wiring

- `crates/julie-extractors/src/registry.rs` (~1,046 lines) has 20 near-
  identical `extract_*()` functions (~620 lines of boilerplate). The existing
  extractor macros cover only 7 languages; the rest are hand-written.
- Recommendation: extend the macro (or a generic factory) to cover all
  languages so adding a language does not mean copying 30 lines of wiring.

### 2.4 Schema-version redundancy and contract doc drift

- `schema_version` and `sqlite_schema_version` are both written with the same
  value (`crates/julie-extract-artifact/src/metadata.rs:38-43`); only one is
  needed.
- `docs/contracts/cli.md` references extract contract v2 while code enforces
  v3 (`crates/julie-extract-artifact/src/schema.rs`).
- v1/v2 schema contract docs carry no deprecation notice and there is no
  migration path (artifacts are rejected, which is a valid policy but
  undocumented).
- Recommendation: one cleanup pass — drop the redundant key (schema bump or
  documented alias), fix the cli.md version reference, add deprecation
  notices to v1/v2 docs, and write a short
  `docs/architecture/versioning-strategy.md` covering when schema, contract,
  crate, and CLI versions bump.

## 3. Gaps vs. "best tree-sitter implementation"

### 3.1 Identifier extraction covers 23 of 34 languages

- Bash, JSON, YAML, TOML, Markdown, and QML lag the full-coverage languages.
  Bash in particular only extracts variable references, not function calls
  (`crates/julie-extractors/src/bash/mod.rs:179-184`, no dedicated
  `identifiers.rs`).
- Impact: downstream reference lookups are weakest exactly where script/config
  files are common. Biggest capability gap found.

### 3.2 No recursion depth guard in tree walks

- `crates/julie-extractors/src/base/tree_methods.rs:11-22` and similar
  recursive `walk_tree` patterns (~190 sites) have no depth limit.
  Pathologically nested or minified files could overflow the stack; the panic
  catcher contains the blast, but a depth counter would degrade gracefully
  instead of losing the file.

### 3.3 Grammar hygiene

- Four grammars pinned to forks: tree-sitter-qmljs, tree-sitter-razor,
  tree-sitter-powershell (Airbus fork), tree-sitter-vb-dotnet. Forks are
  sometimes necessary but limit upstream tracking.
- Version skew across grammars: e.g. tree-sitter-typescript 0.23.2 vs
  tree-sitter-javascript 0.25.0; oldest are tree-sitter-elixir 0.3 and
  tree-sitter-lua 0.5.0.
- Recommendation: periodic upgrade pass; the parser-certification gate already
  exists to validate upgrades.

### 3.4 Inconsistent visibility and doc-comment handling across languages

- Visibility extraction is strong for C++/C#/Java/Swift but missing or weak
  for Python, Dart, and Go. Doc-comment parsing is re-implemented per language
  with no unified normalization helper. A `base/` doc-comment helper would
  remove ~150 lines of duplication across 8+ languages and improve
  consistency.

## 4. Strengths confirmed (keep doing this)

All verified in code, not just documented:

- Per-file panic isolation via `catch_unwind`; one bad file cannot abort a
  scan (`crates/julie-extract-cli/src/extraction.rs:117-138`).
- blake3 content-hash incremental scans — immune to mtime/clock-skew issues.
- Single-transaction WAL writes with tuned pragmas
  (`journal_mode=WAL`, `synchronous=NORMAL`, 128 MiB cache) and prepared-
  statement reuse.
- Spool-to-disk extraction (512-file chunks) bounds peak memory on huge repos.
- Atomic temp-then-rename JSONL export.
- Data-loss guard prevents overwriting good symbols with failure rows
  (`crates/julie-extract-artifact/src/writer.rs:1268-1292`).
- 1 MiB source-file size cap, symlink skipping, UTF-8 validation with error
  rows.
- Test guardrails all real: 90s wall-clock tripwire (`xtask/src/test_tiers.rs`),
  convention test blocking slow-gate leakage into the default tier,
  per-language `cargo xtask test language <name>` commands, feature-gated
  golden/certification/real-world tiers.

## 5. Refuted findings (do not re-report)

- **`captures[1]` "panic risk" in javascript/python/razor/gdscript doc-comment
  regexes:** false alarm. The capture groups are non-optional, so they always
  participate when the regex matches; indexing cannot panic. The real issue at
  those sites is per-call regex compilation (finding 1.5).
- **Byte-based column arithmetic in `base/span.rs` "wrong for multi-byte
  characters":** not a bug. Both the tree-sitter path and the content-range
  path produce byte columns, consistent with tree-sitter's own column
  convention. At most worth one sentence in the schema contract documenting
  that columns are byte offsets.

## 6. Suggested order

1. Quick wins (~an afternoon): workflow version defaults, `rust-version` pin,
   CI cargo cache, Dart `unwrap` guard, LazyLock regex audit.
2. Profile and implement thread-local parser reuse (cold-scan optimization).
3. Split `commands.rs`; then `writer.rs` and `registry.rs` consolidation.
4. Identifier coverage for bash/config languages.
5. Schema-version cleanup + versioning-strategy doc.
6. Grammar upgrade pass through the certification gate.
