# julie-extractors Code Review (v0.1.0 Release Candidate)

**Scope:** Full review of the extraction product (`source tree -> versioned extraction artifact`): the `julie-extract` CLI, the SQLite/JSONL artifact contracts, the 34+ language extractor engine, capability/test signals, build/CI/release, and test discipline.

- **Date:** 2026-06-01
- **Commit:** `367af62` (main, clean working tree)
- **Release:** v0.1.0 release candidate
- **Method:** Multi-agent review across 9 dimensions, adversarially verified. Every finding cites `path:line` with quoted code or the exact contract clause. Absence claims were positively verified (grep the enum/module/test before asserting "missing"). Several bugs were traced through to the built binary's reachable paths; the lead independently ran `cargo xtask test default` (green) and `cargo clippy --workspace --all-targets` (exit 0, warning baseline). Positives were verified, not assumed, so the report is balanced.

---

## 1. Executive Summary

- **Ship-blocker class found:** a data-loss guard (F1) aborts the entire scan transaction when a tracked file is legitimately emptied (edited to comment-only). One such file bricks incremental scan repo-wide and leaves stale ghost symbols, directly violating `cli.md:160`. This is the single most important fix before release.
- **The whole-batch-abort architecture is the multiplier behind several findings:** scan uses `?` on the first failed/empty file, so there is no per-file failure path. `partial` status and `files_failed` are documented but never produced (F3), `FileStatus::FailedPreserved` is fully wired in schema/writer/guard but the CLI never emits it (F11), and one binary/non-UTF-8/parse-failed/empty file discards the entire artifact.
- **Untrusted-input robustness has two real holes:** unguarded recursion can stack-overflow on deeply nested source and abort the run with no per-file isolation (F8), and directory discovery follows symlinks with no cycle guard or depth cap, so a symlink loop crashes the scan and an out-of-root symlink silently injects foreign files (F9). The per-byte slicing paths, by contrast, are uniformly panic-safe (verified).
- **Two workspace guardrails are declared but inert:** `unsafe_code = "forbid"` is set at the workspace level but inherited by no member crate, so unsafe code would compile cleanly across a 202k-LOC parser (F19); and there is **no clippy gate** in any CI workflow, so the 757-warning baseline can grow freely (F20).
- **Metadata drift-detection is silently non-functional:** `parser_inventory_fingerprint` and `capability_snapshot_fingerprint` are hardcoded `sha256:...-v1` constants that never reflect parser/capability state, propagated into SQLite, JSONL, and the report contract (F12). A `*_fingerprint` field frozen to a constant is contract drift.
- **Contract conformance is otherwise strong and well-tested.** The SQLite schema (18 tables, FKs, 17 indexes), JSONL envelope/record-kind set, JSON report row-domain exhaustiveness, exit-code mapping, path policy, transaction/rollback model, and FK enforcement all match the docs and are pinned by executable contract tests that run in CI's contract tier. These are the strongest part of the codebase.
- **Cross-language inequality is mostly perf, not correctness:** Rust alone caches a containing-symbol index while TS/Python/C# re-filter+sort per identifier (F23); `VariableRef` is contract-listed but emitted by only 3 niche language modules, never the high-traffic ones (F24); several lower-traffic languages recompile regexes per node (F26/F41). The shared span/identifier foundations are correct and isomorphic.
- **Test discipline holds today:** the default tier is genuinely fast and slow gates are routed out, but the two stated guardrails are weaker than documented — the default-suite wall-clock budget tripwire does not exist (F25), and the tier-leak convention test checks a hardcoded module list rather than a structural rule (F26).
- **Net:** the durable-output contract surface is solid and well-guarded; the risk is concentrated in (a) the empty-file/parse-failure guard semantics, (b) the all-or-nothing batch model, and (c) untrusted-tree robustness (recursion + symlinks). F1 should block the release; F8/F9/F12/F19/F20 should be fixed in the RC window.

---

## 2. Lead-Verified Evidence

- **Default test tier is GREEN.** `cargo xtask test default` exits 0. Test execution is fast (cli_contract 1.36s, operations_contract 0.19s, path_policy 0.08s); the ~36.5s wall is dominated by compilation, not test runtime. The "default tier stays fast" claim holds.
- **The operations_contract tests assert real behavior, not smoke.** Verified: scan dedups duplicate identifiers before writing; scan skips relationships with missing symbol endpoints; scan no-change returns `no_change` with no new revision; delete returns `not_found` for a missing row; force scan reports force mode; update changes one file and preserves others; info is read-only. This corroborates the core artifact mutation contract.
  - **Caveat:** these tests do NOT cover the data-loss-guard empty-file case (F1), nor non-UTF-8/symlink/parse-failure cases (F8/F9/F11). Those gaps stand.
- **Doc-sync contract satisfied.** `AGENTS.md` and `CLAUDE.md` are byte-for-byte identical; `scripts/check-agent-doc-sync.sh` exits 0. No doc-sync drift finding.
- **Clippy baseline.** `cargo clippy --workspace --all-targets` exits 0 (compiles cleanly) but emits 757 warnings in the `julie-extractors` lib-test target (480 duplicates, 224 auto-fixable), almost all in TEST code (`assert!(x.len()>0)`, `for_kv_map`). Library/binary product code is materially cleaner than 757 implies. There is no clippy step in CI (F20). `unsafe_code = "forbid"` is declared at the workspace level but not inherited (F19).

---

## 3. Findings Table

| Severity | ID | Area | Title | File(s) |
|---|---|---|---|---|
| critical | F1 | data-loss guard / scan correctness | Data-loss guard blocks intentional empty files and rolls back the whole scan, leaving stale ghost symbols | writer.rs:739, writer.rs:359, extraction.rs:242, pipeline.rs:31, cli.md:160 |
| high | F2 | parse failure handling | True total parse failures are stored as successful `indexed` files with zero symbols (exit 0) | pipeline.rs:31, pipeline.rs:138, extraction.rs:242, commands.rs:823 |
| high | F3 | status values / scan semantics | `partial` status and `files_failed` count are documented but never produced; scan is all-or-nothing | commands.rs:151, commands.rs:809, commands.rs:1829, reports.rs:155, reports.md:130 |
| high | F8 | panic / data-loss on untrusted input | Unguarded recursion + no catch_unwind: deeply nested source can stack-overflow and abort the whole build | tree_methods.rs:11, tree_methods.rs:31, commands.rs:787, commands.rs:803, main.rs:7 |
| high | F9 | scan discovery / untrusted trees | Discovery follows symlinks with no cycle guard or depth cap: loop overflows the scan; out-of-root symlink injects foreign files | discovery.rs:90, discovery.rs:100, discovery.rs:109, discovery.rs:112, discovery.rs:188 |
| high | F10 | CLI export / stdout discipline | Failed `export --out -` writes the JSON report to stdout, interleaving with partial JSONL already streamed | commands.rs:676, commands.rs:630, commands.rs:654 |
| high | F11 | error handling / batch / contract drift | Per-file read/extract errors abort the whole batch via `?`; `FailedPreserved` is contract-defined but never produced | commands.rs:788, commands.rs:803, extraction.rs:78, pipeline.rs:31, model.rs:47 |
| high | F12 | metadata contract | `parser_inventory_fingerprint` / `capability_snapshot_fingerprint` are static placeholders, not content fingerprints | commands.rs:1653, commands.rs:1654, metadata.rs:47, jsonl.rs:189, sqlite-schema-v1.md:42 |
| high | F19 | Cargo workspace lints | `unsafe_code = "forbid"` declared at workspace level but inherited by NO member crate, so it is inert | Cargo.toml:15, artifact/Cargo.toml, cli/Cargo.toml, extractors/Cargo.toml, xtask/Cargo.toml |
| high | F20 | CI lint policy | No clippy gate in any CI workflow, alias, or script; 757 warnings can regress freely | ci.yml:21, specialist-gates.yml, release-binaries.yml, testing-strategy.md:204 |
| medium | F4 | error codes / concurrency | Stable error code `lock_timeout` is never emitted and no SQLite busy_timeout is configured | reports.rs:263, reports.rs:288, writer.rs:71, commands.rs:1034 |
| medium | F5 | scan discovery / silent failure | Unreadable directories are silently skipped during discovery with no warning or failure | discovery.rs:91, discovery.rs:97 |
| medium | F6 | test coverage gaps | No CLI-level test exercises the guard, intentional-empty, broken-parse, partial status, or export stream split | operations_contract.rs:398, cli_contract.rs:141, writer_contract.rs:453 |
| medium | F7 | writer perf contract / test quality | Per-row-commit tripwire asserts a hardcoded constant, not a measured commit count | writer.rs:205, writer.rs:439, writer_performance.rs:20, writer_contract.rs:28 |
| medium | F13 | metadata contract | `artifact_metadata` parser/capability fingerprints are hardcoded constants, defeating drift detection (SQLite-side view of F12) | commands.rs:1653, commands.rs:1654, metadata.rs:47, jsonl.rs:189 |
| medium | F16 | total parse failure | Total tree-sitter parse failure silently degrades to `Ok(empty)` instead of a typed error | pipeline.rs:31, pipeline.rs:138 |
| medium | F17 | force rebuild | Force-rebuild swallows artifact-file delete errors; can write a clean rebuild into a stale database | commands.rs:1695, commands.rs:166 |
| medium | F21 | determinism / relationship resolution | `containing_symbol_id` is nondeterministic on priority+size ties (HashMap iteration leaks into a persisted FK) | creation_methods.rs:261, creation_methods.rs:277, creation_methods.rs:205, python/identifiers.rs:17 |
| medium | F22 | test_detection cross-language | Scala test detection flags every callable in a test-path file as a test, with no name/annotation guard | test_detection.rs:129, test_detection.rs:137, scala/declarations.rs:90 |
| medium | F23 | cross-language perf | Containing-symbol lookup is O(symbols) per identifier in TS/Python/C#; Rust alone uses a cached index | base/creation_methods.rs, typescript/identifiers.rs, python/identifiers.rs, csharp/identifiers.rs, rust/identifiers.rs |
| medium | F24 | cross-language contract coverage | `IdentifierKind::VariableRef` is in the contract but emitted by only 3 of ~38 language modules | base/kinds.rs, sqlite-schema-v1.md, qml/identifiers.rs, r/identifiers.rs, yaml/mod.rs |
| medium | F25 | default-suite budget guardrail | Default-suite wall-clock budget tripwire does not exist (only an aspirational doc bullet) | testing-strategy.md:235, writer_performance.rs:27, xtask/dogfood.rs:537 |
| medium | F26 | tier-leak convention test | Tier-leak convention test enforces a hardcoded module list, not a structural rule | tests/test_tiers.rs:8, tests/test_tiers.rs:56, qml/mod.rs:84 |
| medium | F27 | offset correctness on BOM input | UTF-8 BOM is never stripped: byte/line offsets, content_bytes, and content_hash all include the 3 leading BOM bytes | extraction.rs:56, extraction.rs:65, pipeline.rs:29 |
| medium | F28 | untrusted-input / batch abort | Binary / invalid-UTF-8 files with a supported extension abort the whole batch instead of being recorded failed | extraction.rs:56 |
| low | F18 | revision.mode nullability | `revision.mode` nullability disagrees between SQLite schema (nullable) and jsonl-v1 (non-null enum) | jsonl.rs:436, jsonl-v1.md:247, sqlite-schema-v1.md:85 |
| low | F29 | export stream split (mid-stream) | `export --out -` mid-stream failure routes JSON report to stdout, conflicting with already-emitted JSONL | commands.rs:630, commands.rs:654, commands.rs:676, reports.md:241 |
| low | F30 | error codes / usage handling | Documented `usage_error` code is never emitted as structured JSON (clap text + exit 2 only) | commands.rs:41, reports.md:166, reports.md:253 |
| low | F31 | schema invariant | `body_hash` span invariant not enforced or tested at the writer boundary | writer.rs:867, extraction.rs:150, extraction.rs:174, creation_methods.rs:39 |
| low | F32 | scan --force replacement | Force rebuild on root mismatch deletes artifact in place before writing (non-atomic window) | commands.rs:149, commands.rs:165, commands.rs:1695 |
| low | F33 | report doc-internal drift | reports.md contradicts itself on export report `mode` (export vs jsonl); code emits jsonl | reports.md:106, reports.md:224, reports.rs:103, commands.rs:644 |
| low | F34 | silent failure / robustness | Export report totals silently report 0 if a COUNT query fails (swallowed Result) | commands.rs:1380, commands.rs:648 |
| low | F35 | export performance | JSONL export does a `json!`->Value->`to_writer` double pass plus per-row JSON re-parse | jsonl.rs:1288, jsonl.rs:1297, jsonl.rs:1458 |
| low | F36 | numeric overflow | u32 byte/line offsets truncate/overflow on >4GB files with no size guard at read time | span.rs:33, span.rs:64, embedded_span.rs:40, pipeline.rs:100, extraction.rs:56 |
| low | F37 | error messages | `read_source_snapshot` labels all read failures as "could not be read as UTF-8", masking true I/O errors | extraction.rs:56 |
| low | F38 | test quality | Pure smoke-only extractor test exists (asserts only `!is_empty()`) | zig/extractor.rs:27, typescript/extractor.rs:37 |
| low | F39 | test quality / dead code | Seven `#[ignore]` debug tests are dead AST-dump scaffolding with no assertions | scala/ast_debug.rs:30, scala/mod.rs:51, r/basics.rs:12, r/data_structures.rs:12 |
| low | F40 | literal carrier policy / capability loading | Embedded language policy and capabilities.json panic on malformed config (already covered by default-tier parse tests) | language_policy.rs:130, capability_snapshot.rs:121, tests/language_policy.rs:77, tests/capability_snapshot_test.rs:6 |
| low | F41 | cross-language perf | Per-call `Regex::new` recompilation in several lower-traffic languages vs LazyLock caching elsewhere | razor/relationships.rs, zig/variables.rs, zig/imports.rs, gdscript/mod.rs, go/functions.rs |
| low | F42 | robustness consistency | vue `get_node_text_from_content` slices without a bounds guard while its sibling helper guards | vue/identifiers.rs, vue/script_setup.rs |
| low | F43 | release packaging / docs | Release manifest ships only 2 of 3 architecture docs; release.md prose says "architecture docs" (plural) | xtask/release.rs:117, xtask/release.rs:121, release.md:53, docs/architecture/cli-contract.md |
| low | F44 | test tier routing / doc drift | Python downstream consumer is documented under Contract Tier but not run by `cargo xtask test contract` | testing-strategy.md:94, testing-strategy.md:98, xtask/test_tiers.rs:213, xtask/tests/python_example_contract.rs:54 |
| low | F45 | dogfood gate | Dogfood CI gate runs against a debug binary, so its perf timings reflect an unoptimized build | specialist-gates.yml:70, xtask/dogfood.rs:242, xtask/dogfood.rs:298, xtask/dogfood.rs:484 |

> Note: F12 and F13 are the same root defect (hardcoded metadata fingerprints) seen from two dimensions. F12 (high) is the canonical entry; F13 (medium) is the SQLite-writer-dimension view kept for the table but folds into F12's recommendation. Likewise F10 and F29 are the same export-stream-routing bug at two severities (F10 high is the mid-stream-failure interleave; F29 low is the precise restatement). Fixing the failure-arm stream choice resolves both.

---

## 4. Detailed Findings

### CRITICAL

#### F1 — Data-loss guard blocks intentional empty files and rolls back the whole scan, leaving stale ghost symbols
- **Severity:** critical
- **Area:** data-loss guard / scan & update correctness
- **Files:** `crates/julie-extract-artifact/src/writer.rs:739`, `crates/julie-extract-artifact/src/writer.rs:359`, `crates/julie-extract-cli/src/extraction.rs:242`, `crates/julie-extractors/src/pipeline.rs:31`, `docs/contracts/cli.md:160`
- **Claim:** When a supported, parser-backed file is edited so it legitimately becomes empty or comment-only, extraction succeeds with zero symbols and `FileStatus::Indexed`. The guard branch `FileStatus::Indexed if file.symbols.is_empty() => Some("parser returned zero symbols")` (writer.rs:739) then fires whenever the file previously had symbols, returning `data_loss_guard`/`failed`/exit 1 and refusing to replace the rows. The guard runs inside the scan transaction (`write_scan` → `write_scan_snapshot`, per-file loop at writer.rs:359), and the `?` propagation drops the `Transaction` without commit, rolling back the ENTIRE scan, so every other changed file in that scan is also discarded. The CLI only ever produces `FileStatus::Indexed` (extraction.rs:105/242, commands.rs:2002) and `FileStatus::Unsupported`, never `FailedPreserved`, so the `FailedPreserved` guard branch is dead and this empty-`Indexed` branch is the only one the CLI exercises.
- **Evidence:**
  - `writer.rs:739` → `FileStatus::Indexed if file.symbols.is_empty() => Some("parser returned zero symbols"),`
  - Scan path: `write_scan` (writer.rs:182) → `write_scan_snapshot`, which calls `ensure_data_loss_guard` at writer.rs:359 with `?` inside the per-file `for file in files` loop, before any commit (commit only at writer.rs:431).
  - `cli.md:160` → "An intentional empty supported file may still produce zero symbols when the file hash changed and extraction completed successfully." And `cli.md:156-158` says the guard should trip only on "parser failure, read failure, or extractor failure", not a clean empty parse.
- **Impact:** Editing a function down to a comment, or emptying any tracked file, bricks incremental scan repo-wide and leaves stale symbol/identifier/relationship rows pointing at code that no longer exists. Downstream consumers see ghost symbols. The whole-transaction rollback means a single such file discards all other changed files in the same scan. Directly violates the documented `cli.md:160` clause that anticipated exactly this case.
- **Recommendation:** Distinguish a real parser failure from a successful empty parse before tripping the guard. `extract_canonical` returns a whole-file `Error` parse diagnostic for true degraded failures (pipeline.rs:32 `degraded_parse_failure_result` → `total_parse_failure_diagnostic`) versus no diagnostics for a clean empty parse. Carry an explicit extraction-failed signal (e.g. set `FileStatus::FailedPreserved`) only when a total-failure diagnostic is present, and fire the zero-symbol guard only for that. A clean `Indexed` file with zero symbols and no error diagnostic must be allowed to replace existing rows. Separately, per-file failures inside a scan should not roll back the whole transaction (see F3).

---

### HIGH

#### F2 — True total parse failures are stored as successful `indexed` files with zero symbols (exit 0)
- **Severity:** high
- **Area:** parse failure handling / data-loss guard
- **Files:** `crates/julie-extractors/src/pipeline.rs:31`, `crates/julie-extractors/src/pipeline.rs:138`, `crates/julie-extract-cli/src/extraction.rs:242`, `crates/julie-extract-cli/src/commands.rs:823`
- **Claim:** `extract_canonical` returns `Ok(degraded_parse_failure_result(content))` when tree-sitter cannot build a tree (pipeline.rs:31-33) — a real parser failure is NOT an `Err`. The degraded result carries one whole-file `Error` parse diagnostic but zero symbols and flows through `map_results` as `FileStatus::Indexed` (extraction.rs:242). The CLI only maps the *Err* arm of extraction to `ReportCode::ParseFailed` (commands.rs:823, `ExtractFileErrorKind::Extract`), and that Err arm is reached only for unsupported extension / registry errors, never for a tree-sitter parse failure. So for a NEW file with no prior rows a total parse failure is written as `status=indexed`, `files_failed=0`, command status `ok`, exit 0 — a parse failure reported as success. For a file WITH prior rows the guard fires but labels it "parser returned zero symbols", identical to the intentional-empty case, so callers cannot tell a genuine parse failure from an empty file.
- **Evidence:**
  - `pipeline.rs:31-33` → `let Some(tree) = parse(language, file_path, content)? else { return Ok(degraded_parse_failure_result(content)); };`
  - `pipeline.rs:138-144` → `degraded_parse_failure_result` pushes only a `parse_diagnostic` (Error kind, full-file span) and no symbols.
  - `extraction.rs:242` → `status: FileStatus::Indexed`.
  - `commands.rs:821-825` maps only `ExtractFileErrorKind {Read->ReadFailed, Extract->ParseFailed, Serialize->InternalError}` — reached only via the `?` in `extract_supported_files` (commands.rs:809), which does not fire for the `Ok` degraded result.
- **Impact:** The data-loss-guard reason and the report status conflate three distinct outcomes (intentional empty, total parse failure, partial parse). Downstream consumers relying on `parse_failed`/`data_loss_guard` reason cannot reliably detect parser regressions. A parser failure on a brand-new file is silently recorded as a clean indexed file with exit 0.
- **Recommendation:** Detect a whole-file `Error` parse diagnostic (kind `Error` spanning the file with zero symbols) in the CLI mapping (`map_results` / scan path) and classify it as a parse failure: surface `ReportCode::ParseFailed` (or a distinct guard reason) and an appropriate status, rather than letting it pass as a clean `Indexed` file. This is the same signal needed to fix F1.

#### F3 — `partial` status and `files_failed` count are documented but never produced; scan is all-or-nothing
- **Severity:** high
- **Area:** status values / scan semantics / contract drift
- **Files:** `crates/julie-extract-cli/src/commands.rs:151`, `crates/julie-extract-cli/src/commands.rs:809`, `crates/julie-extract-cli/src/commands.rs:1829`, `crates/julie-extract-artifact/src/reports.rs:155`, `docs/contracts/reports.md:130`
- **Claim:** `ReportStatus::Partial` is defined (reports.rs:75) and serialized (commands.rs:1829) but is never constructed anywhere in the CLI. `extract_discovered_files` (commands.rs:151) extracts via `extract_supported_files` which uses `?` (commands.rs:788/809), returning `Err` on the first failed file; scan routes that straight to `extract_error_outcome` → `failed`, exit 1 (commands.rs:160-162, 814-839). There is no recoverable per-file failure path, so `partial` and the `files_failed` count (reports.rs:155, only ever the default 0) are dead contract surface.
- **Evidence:**
  - `reports.md:130` lists `partial` as a status; `reports.md:133-135` → "partial is an error status for exit-code purposes. It exists so callers can distinguish a consistent artifact with per-file extraction failures from a command that failed before producing useful rows."
  - `commands.rs:1829` → `ReportStatus::Partial => "partial",` is the only reference to `Partial` in the CLI (grep found no constructor).
  - `files_failed` (reports.rs:155) is set nowhere in the CLI or artifact src; `report_contract.rs` only references it as the default 0 (line 180).
- **Impact:** The contract promises callers can scan a large tree, tolerate a few unparseable files, and still get a committed artifact with `files_failed>0` and `status=partial`. Instead one bad/empty file fails the entire scan and commits nothing. This is the direct enabler of F1's repo-wide blast radius.
- **Recommendation:** Implement per-file failure accumulation in scan: collect failures instead of bailing on the first `?`, commit the successful files, set `counts.files_failed`, and emit `status=partial` with per-file error entries when at least one file failed but others committed. Reserve `failed` for failures before any useful rows (db open, capability sync).

#### F8 — Unguarded recursion + no catch_unwind on the batch path: deeply nested source can stack-overflow and abort the whole artifact build
- **Severity:** high
- **Area:** panic / data-loss safety on untrusted input
- **Files:** `crates/julie-extractors/src/base/tree_methods.rs:11`, `crates/julie-extractors/src/base/tree_methods.rs:31`, `crates/julie-extract-cli/src/commands.rs:787`, `crates/julie-extract-cli/src/commands.rs:803`, `crates/julie-extract-cli/src/main.rs:7`
- **Claim:** The engine parses arbitrary source files. `walk_tree` (tree_methods.rs:11) and `find_nodes_by_type_recursive` (tree_methods.rs:31) recurse once per tree level with no depth cap; tree-sitter imposes no nesting limit, so a pathologically deep file can produce a tree deep enough to overflow the default 8MB main-thread stack. `main()` (main.rs:7) calls `run_from_env()` directly on the main thread with no enlarged/worker stack, and the per-file batch loop (commands.rs:787-810) calls `extract(...)?` with no `std::panic::catch_unwind`, so a stack overflow (hard abort) OR any extractor panic aborts the entire artifact build, not just the offending file.
- **Evidence:**
  - `tree_methods.rs:17-21` `walk_tree`: `for i in 0..node.child_count(){ if let Some(child)=node.child(i as u32){ self.walk_tree(&child, visitor, depth+1); } }` (depth incremented but never compared).
  - `main.rs:7` → `fn main()->ExitCode{ commands::run_from_env() }` (no `thread::Builder.stack_size`).
  - `commands.rs:803` → `files.push(extract(root,&supported.target,...,snapshot)?);` (no catch_unwind).
  - `traverse_tree` (tree_methods.rs:87-117) DOES use `catch_unwind` but is a separate helper, and `catch_unwind` cannot recover a stack overflow anyway.
- **Impact:** One malformed/adversarial file in a scanned tree can crash the producer mid-run and lose the entire batch artifact. This is the worst-case failure mode for a batch artifact producer that parses untrusted source.
- **Recommendation:** Run extraction on a worker thread with a larger bounded stack AND wrap each file in `std::panic::catch_unwind` so a single file's panic degrades to that file (`status=FailedPreserved` + diagnostic) instead of aborting the run; or add a traversal depth cap that records a parse diagnostic and stops descending past N levels. Note: `catch_unwind` does not save a stack overflow, so the worker-thread-with-bounded-stack + depth-cap combination is the robust fix.

#### F9 — Directory discovery follows symlinks with no cycle guard or depth cap: a symlink loop stack-overflows the scan, and an out-of-root symlink silently injects external files
- **Severity:** high
- **Area:** scan discovery / panic & data-integrity on untrusted trees
- **Files:** `crates/julie-extract-cli/src/discovery.rs:90`, `crates/julie-extract-cli/src/discovery.rs:100`, `crates/julie-extract-cli/src/discovery.rs:109`, `crates/julie-extract-cli/src/discovery.rs:112`, `crates/julie-extract-cli/src/discovery.rs:188`
- **Claim:** `DiscoveryPolicy::discover_dir` recurses into every entry where `path.is_dir()` is true, with no `is_symlink()` check, no visited-inode set, and no recursion depth limit. `Path::is_dir()`/`is_file()` follow symlinks. A symlink cycle (e.g. `sub/loop -> ..`) therefore causes unbounded recursion until the process stack overflows and aborts the entire scan; a symlink pointing outside the scan root silently descends into and indexes files outside the product's stated `source tree -> artifact` boundary.
- **Evidence:**
  - `discovery.rs:90` → `fn discover_dir(&self, dir: &Path, summary: &mut DiscoverySummary)` takes no depth/visited args.
  - `discovery.rs:100` → `if path.is_dir()` (follows symlinks); `discovery.rs:109` → `self.discover_dir(&path, summary)` recurses unconditionally; `discovery.rs:112` → `if !path.is_file()` (also follows symlinks).
  - No `is_symlink`/`symlink_metadata`/`file_type()` anywhere in `crates/julie-extract-cli/src` (grep empty, verified). The crate does NOT use `ignore::WalkBuilder` (grep empty, verified), which would have handled symlinks safely.
  - The SIBLING helper `collect_nested_gitignore` (discovery.rs:188-189) DOES cap depth (`if depth == 0 { return; }`), so the omission in the main walk is an inconsistency, not a deliberate design choice.
- **Impact:** Crash (stack overflow, scan aborts with no artifact) on any repo containing a symlink cycle — common in `node_modules`-adjacent trees, vendored deps, or test fixtures. Separately, out-of-root symlinks violate the root boundary and inject foreign files into the artifact with root-relative paths that may collide. Both occur on normal, non-malicious input.
- **Recommendation:** Either switch discovery to `ignore::WalkBuilder` (which already handles symlink loops, depth, and gitignore in one pass and is the same crate already depended on), or add an `is_symlink()` skip (default: do not follow symlinks) plus a depth cap mirroring `collect_nested_gitignore`. Add a discovery unit test for a symlink cycle and an out-of-root symlink. There are currently zero `#[test]` in discovery.rs.
- **Confidence:** flagged by completeness pass, strong evidence (90). Symlink-following confirmed; the missing `is_symlink`/`WalkBuilder` was positively verified by grep; symlink cycle reproduced in `/tmp`.

#### F10 — Failed `export --out -` writes the JSON report to stdout, interleaving with partial JSONL already streamed there
- **Severity:** high
- **Area:** CLI export wiring / stdout-stderr discipline
- **Files:** `crates/julie-extract-cli/src/commands.rs:676`, `crates/julie-extract-cli/src/commands.rs:630`, `crates/julie-extract-cli/src/commands.rs:654`
- **Claim:** The success arm of `export()` routes the report to `ReportStream::Stderr` when `--out` is `-` (commands.rs:654-658), but the failure arm (commands.rs:661-677) unconditionally routes the failed report to `ReportStream::Stdout` with no branch on `args.out`. Because `export_jsonl` writes directly through a `BufWriter` wrapping the stdout lock (commands.rs:631-633), any records written before the failure are already on stdout. The failed JSON report then also goes to stdout, mixing a partial JSONL stream with a report object on the same stream.
- **Evidence:**
  - Success arm: `if args.out == Path::new("-") { ReportStream::Stderr } else { ReportStream::Stdout }` (commands.rs:654-658).
  - Failure arm: `outcome(report, 1, args.json, ReportStream::Stdout)` (commands.rs:676), no `args.out` branch.
  - stdout sink: `let stdout = io::stdout(); let mut lock = stdout.lock(); export_jsonl(&artifact.connection, &mut lock)` (commands.rs:631-633).
  - Mid-export failure path is reachable: a malformed `metadata_json` row produces a `JsonlExportError` mid-stream, proven by `failed_path_export_removes_incomplete_output_file` which UPDATEs `files.metadata_json` to `{` and gets an export error (jsonl_contract.rs:384-396).
- **Impact:** Violates `reports.md:241` ("`export --out - --json` writes JSONL to stdout and the final report to stderr") and `cli.md:116-117` ("JSONL uses stdout and the JSON report uses stderr"). A machine consumer streaming JSONL from stdout that hits a mid-export failure receives partial JSONL records followed by a failed JSON report on the SAME stream, corrupting the stream it is parsing. The documented completion signal (exit code + report on stderr) ends up partly on the data channel.
- **Recommendation:** In the failure arm at commands.rs:676, choose the report stream the same way the success arm does: `if args.out == Path::new("-") { ReportStream::Stderr } else { ReportStream::Stdout }`. Add an end-to-end test that triggers a mid-stream export failure with `--out -` and asserts stdout contains only (possibly partial) JSONL and the failed report is on stderr.

#### F11 — Per-file read/extract errors abort the entire batch via `?`; `FileStatus::FailedPreserved` is contract-defined but never produced
- **Severity:** high
- **Area:** error handling / batch robustness / contract drift
- **Files:** `crates/julie-extract-cli/src/commands.rs:788`, `crates/julie-extract-cli/src/commands.rs:803`, `crates/julie-extract-cli/src/extraction.rs:78`, `crates/julie-extractors/src/pipeline.rs:31`, `crates/julie-extract-artifact/src/model.rs:47`
- **Claim:** In `extract_supported_files`, both `read_source_snapshot(...)?` (commands.rs:788) and `extract(...)?` (commands.rs:803) propagate with `?`, so a single unreadable/non-UTF-8 file OR a file whose registry extractor returns `Err` fails the whole multi-file build instead of marking that file failed and continuing. The graceful per-file path (pipeline.rs:31-33 `degraded_parse_failure_result`) covers ONLY parser-returns-`None`; a registry extract `Err` (pipeline.rs:36) or a read `Err` is not degraded. Critically, the schema defines `FileStatus::FailedPreserved` (model.rs:47, writer maps it at writer.rs:738), but NO CLI code path ever sets that status — the producer only ever emits `FileStatus::Indexed` (extraction.rs:105, 242; commands.rs:2002). So the failed-file concept the contract anticipates is unreachable from production code.
- **Evidence:**
  - `commands.rs:788` → `let snapshot = read_source_snapshot(&supported.target)?;`
  - `commands.rs:803-809` → `files.push(extract(...,snapshot)?);`
  - `pipeline.rs:31-33` → `let Some(tree)=parse(...)? else { return Ok(degraded_parse_failure_result(content)); };` then line 36 `extract_for_language(...)?` (Err propagates, not degraded).
  - grep for `status: FileStatus::FailedPreserved` across crates returns only `writer_contract.rs:453` (a test); all CLI status assignments are `FileStatus::Indexed`.
- **Impact:** A directory containing one binary-ish/non-UTF-8 file that slips past discovery, or one file that trips an extractor error, aborts the entire scan rather than recording that single file as failed and continuing. The artifact captures nothing partial. The schema/writer infrastructure for per-file failure (`FailedPreserved` + "parser/read failure evidence" capability note at writer.rs:738) exists but is dead from the producer side — a real contract-vs-implementation gap.
- **Recommendation:** Catch per-file `Err` in `extract_supported_files`, record the file with `FileStatus::FailedPreserved` plus the error diagnostic, and continue the loop so the artifact captures partial-but-useful results and exercises the status the schema already defines. Cross-check `reports.md` for whether failed files must also appear in report counts. (This and F3 share the same fix; decide the model and align code + contract.)

#### F12 — `parser_inventory_fingerprint` and `capability_snapshot_fingerprint` are static placeholders, not content fingerprints
- **Severity:** high
- **Area:** capability snapshot / artifact metadata contract
- **Files:** `crates/julie-extract-cli/src/commands.rs:1653`, `crates/julie-extract-cli/src/commands.rs:1654`, `crates/julie-extract-artifact/src/metadata.rs:47`, `crates/julie-extract-artifact/src/jsonl.rs:189`, `docs/contracts/sqlite-schema-v1.md:42`, `docs/architecture/schema-principles.md:43`
- **Claim:** The two metadata fields the schema contract defines as "fingerprint of parser package inventory" and "fingerprint of language capabilities" are hardcoded constant strings emitted unconditionally for every artifact. They are not derived from `capabilities.json`, the parser inventory, or any content, so they never change when the capability set or parser inventory changes.
- **Evidence:**
  - `new_artifact_metadata()` sets `parser_inventory_fingerprint: "sha256:parser-inventory-v1".to_string()` and `capability_snapshot_fingerprint: "sha256:capability-snapshot-v1".to_string()` (commands.rs:1653-1654).
  - A whole-repo grep of set-sites confirms these are the ONLY two assignments of literal values; every other reference READS the field (metadata.rs:47-52 persists it, jsonl.rs:189-190 and reports.rs:132-133 propagate it into the JSONL/report contracts, commands.rs:1296-1315 reads it back).
  - `sqlite-schema-v1.md:42-43` define these as "fingerprint of parser package inventory" / "fingerprint of language capabilities"; `schema-principles.md:43-44` list them as required metadata.
  - The `sha256:` prefix is misleading: no hash of any content is computed, and `refreshed_metadata` (commands.rs:1660-1663) only re-stamps `updated_at`, never the fingerprints. The real inputs already exist: `artifact_capability_snapshot()` at commands.rs:1432 and `parser_inventory` built at commands.rs:1472.
- **Impact:** A downstream consumer (SQLite or JSONL) that compares these fingerprints to detect a capability-set or parser-inventory change will NEVER observe a change after `capabilities.json` is edited, a parser is upgraded, or a language is added/removed. The drift-detection contract is silently non-functional, and the lie propagates into the JSONL-v1 and reports contracts, not just SQLite.
- **Recommendation:** Compute both fingerprints deterministically from their source: a digest (blake3 is already the `hash_algorithm`) over a canonical sorted serialization of `artifact_capability_snapshot()` for the capability fingerprint, and over the sorted `(language, parser_crate)` inventory for the parser fingerprint. Re-stamp through `refreshed_metadata` so a parser/capability change on rescan updates it. Alternatively change all three contract docs to call these "version tags" and drop the `sha256:` prefix; a field named `*_fingerprint` frozen to a constant is contract drift either way. Add a test asserting two snapshots with different parser sets / capability flags produce different fingerprints.

#### F19 — `unsafe_code = "forbid"` is declared at workspace level but inherited by NO member crate, so it is inert
- **Severity:** high
- **Area:** Cargo workspace lint configuration
- **Files:** `Cargo.toml:15`, `crates/julie-extract-artifact/Cargo.toml`, `crates/julie-extract-cli/Cargo.toml`, `crates/julie-extractors/Cargo.toml`, `xtask/Cargo.toml`
- **Claim:** The workspace declares `[workspace.lints.rust] unsafe_code = "forbid"` (Cargo.toml:15-16), but no member crate opts in with `[lints] workspace = true`. Per Cargo semantics a `[workspace.lints]` table only applies to crates that explicitly inherit it; without that opt-in the table is completely inert. So the no-unsafe guardrail is declared but enforced nowhere.
- **Evidence:**
  - `Cargo.toml:15` → `[workspace.lints.rust]`, line 16 → `unsafe_code = "forbid"`.
  - `grep -rn lints --include=Cargo.toml .` (excluding target) returns ONLY the root workspace declaration.
  - Member manifests inherit edition/license/repository via `*.workspace = true` (e.g. artifact Cargo.toml:4-6) but contain no `[lints]` section and no `lints.workspace = true` (confirmed by grep on all four member manifests).
- **Impact:** The stated safety guardrail (no unsafe code) is not in effect. A contributor could add an `unsafe` block to any crate and it would compile cleanly with no lint/CI failure. Any future lint added to this table is equally inert, giving false confidence. The `unsafe_code=forbid` workspace positive is therefore unenforced across a 202k-LOC parser crate handling untrusted input.
- **Recommendation:** Add `[lints]\nworkspace = true` to each member crate's Cargo.toml (julie-extract-artifact, julie-extract-cli, julie-extractors, xtask). Verify by temporarily inserting an `unsafe {}` block and confirming the build fails. Add a convention test (like `crates/julie-extractors/src/tests/test_tiers.rs`) asserting every member inherits workspace lints so this cannot silently regress.

#### F20 — No clippy gate in any CI workflow, cargo alias, or script; 757 existing warnings can regress freely
- **Severity:** high
- **Area:** CI lint policy
- **Files:** `.github/workflows/ci.yml:21`, `.github/workflows/specialist-gates.yml`, `.github/workflows/release-binaries.yml`, `docs/testing-strategy.md:204`
- **Claim:** None of the three workflows, the cargo alias config (`.cargo/config.toml`), or any script runs `cargo clippy`. CI fast-gates runs only `fmt --check`, `cargo metadata`, `cargo test -p xtask`, `xtask test default`, `xtask test contract`. With 757 clippy warnings already present (lead signal), nothing prevents the count from growing and there is no path to driving it to zero.
- **Evidence:**
  - `ci.yml:21-34` lists every step (fmt --check, metadata, test -p xtask, xtask test default, xtask test contract) — no clippy.
  - `grep -rn clippy .github/ scripts/ xtask/` returns exit 1 (no matches). No `clippy.toml`/`.clippy.toml` exists. `.cargo/config.toml` contains only the `xtask` alias, no clippy alias.
  - `testing-strategy.md:204-214` documents exactly these 5 CI steps, confirming clippy is structurally absent, not a slip.
- **Impact:** Lint quality drifts unchecked. Because most of the 757 warnings are in test code (`assert!(x.len()>0)`, `for_kv_map`), a real correctness-adjacent lint introduced in non-test code can hide in the noise and never surface in review.
- **Recommendation:** Add a clippy step to `ci.yml`. Phased gate: after a one-time autofix sweep, `cargo clippy --workspace --all-targets -- -D warnings`; OR start with production-only enforcement `cargo clippy --workspace --lib --bins -- -D warnings` to lock in non-test cleanliness immediately while test-code lints are burned down. Pair with the F19 lints fix so clippy lints can be centralized in `[workspace.lints.clippy]`.

---

### MEDIUM

#### F4 — Stable error code `lock_timeout` is never emitted and no SQLite busy_timeout is configured
- **Severity:** medium
- **Area:** error codes / concurrency robustness
- **Files:** `crates/julie-extract-artifact/src/reports.rs:263`, `crates/julie-extract-artifact/src/reports.rs:288`, `crates/julie-extract-artifact/src/writer.rs:71`, `crates/julie-extract-cli/src/commands.rs:1034`
- **Claim:** `ReportCode::LockTimeout` is in the stable enum (reports.rs:263) and the `ERROR_CODES` list (reports.rs:288) but is emitted nowhere. `write_error_outcome` maps every `ArtifactWriteError::Sqlite` (which includes SQLITE_BUSY/locked) to `ReportCode::DbWriteFailed` (commands.rs:1034-1039), and the writer sets only `journal_mode=WAL` with no `busy_timeout` PRAGMA (writer.rs:71). So a concurrent writer contending for the artifact fails immediately as `db_write_failed` (or `db_open_failed` during open), and the `lock_timeout` code is unreachable.
- **Evidence:**
  - `reports.rs:263` → `LockTimeout,` and `reports.rs:288` → `Self::LockTimeout,` in `ERROR_CODES`.
  - `writer.rs:71` → `connection.pragma_update(None, "journal_mode", "WAL")?;` is the only pragma (no `busy_timeout`).
  - `commands.rs:1034-1039` maps `ArtifactWriteError::Sqlite` → `ReportCode::DbWriteFailed`. Grep for busy/timeout/LockTimeout across both src trees finds only the two enum references.
- **Impact:** Two `julie-extract` processes targeting the same artifact get an opaque `db_write_failed` instead of the documented `lock_timeout`, with no grace period. Callers cannot distinguish transient contention (retry) from a real write failure. A documented contract code is dead. (Related read-side risk: read connections open `SQLITE_OPEN_READ_ONLY` at commands.rs:1086, but with no busy_timeout a reader can still get an immediate SQLITE_BUSY while a scan holds the WAL write lock.)
- **Recommendation:** Set a `busy_timeout` PRAGMA on the writer connection and map SQLITE_BUSY/SQLITE_LOCKED (after the timeout) to `ReportCode::LockTimeout` with `recoverable=true`. Either wire the code up or, if single-writer is the intended invariant, remove `lock_timeout` from the stable list and document the assumption.

#### F5 — Unreadable directories are silently skipped during discovery with no warning or failure
- **Severity:** medium
- **Area:** scan discovery / silent failure
- **Files:** `crates/julie-extract-cli/src/discovery.rs:91`, `crates/julie-extract-cli/src/discovery.rs:97`
- **Claim:** `discover_dir` swallows `fs::read_dir` errors with `let Ok(entries) = fs::read_dir(dir) else { return; };` (discovery.rs:91-93). A directory that cannot be read (permissions, transient IO) is silently dropped along with its entire subtree. Separately, entries whose path cannot be made root-relative are silently `continue`'d (discovery.rs:97-99). The files inside are never extracted, never counted as unsupported or failed, and no warning is recorded, so the artifact silently under-covers the tree while reporting `status=ok`.
- **Evidence:**
  - `discovery.rs:91-93` → `let Ok(entries) = fs::read_dir(dir) else {\n    return;\n};` with no warning emission and no error propagation.
  - `discovery.rs:97-99` → `let Ok(relative) = crate::paths::root_relative_unix(&self.root, &path) else { continue; };` similarly silent.
- **Impact:** Silent incompleteness: a scan can report success while omitting an entire subtree. Downstream consumers treating the artifact as a complete snapshot of the root will have missing files with no signal. Contradicts the scan invariant that it covers the source tree.
- **Recommendation:** Surface `read_dir` failures as a typed warning (the report already supports warnings) or as a per-directory failure that contributes to `partial` status, rather than discarding silently. At minimum log the path and count it.

#### F6 — No CLI-level test exercises the data-loss guard, the intentional-empty case, the broken-parse case, the partial status, or the export stream split
- **Severity:** medium
- **Area:** test coverage gaps
- **Files:** `crates/julie-extract-cli/tests/operations_contract.rs:398`, `crates/julie-extract-cli/tests/cli_contract.rs:141`, `crates/julie-extract-artifact/tests/writer_contract.rs:453`
- **Claim:** The only data-loss-guard test (writer_contract.rs:443-468) uses `FileStatus::FailedPreserved` via `write_update`, which the CLI never produces, so the guard path the CLI actually hits (Indexed + empty + `write_scan`) is untested end to end. There is no test that empties a tracked file and asserts the resulting status/rows, which is why F1 ships green. The success-path export split for `--out -` (JSONL→stdout, report→stderr) is untested: the export success test (operations_contract.rs:398) writes to a file path, and the only `--out -` export test (cli_contract.rs:141-150) fails early on `unsupported_format` before any JSONL is written, so it does not exercise the stream split. There is also no CLI test asserting `status=partial` or `files_failed` produced by a real scan, and no broken-syntax-file test asserting `parse_failed`.
- **Evidence:**
  - `writer_contract.rs:453` → `status: FileStatus::FailedPreserved,` is the only guard test.
  - `operations_contract.rs:411-420` export uses `--out path_str(&out)` not `-`.
  - `cli_contract.rs:141-150` export `--out -` uses `--format xml` and asserts `unsupported_format` (early return at commands.rs:625, before `export_jsonl`), so the split path is not covered.
  - grep for partial/files_failed across the CLI tests dir returns nothing behavioral.
- **Impact:** The highest-value correctness behaviors (guard on Indexed+empty via scan, empty-file replacement, broken-parse classification, success stream split, partial) are not pinned by binary-level tests, so the contract violations above are invisible to CI.
- **Recommendation:** Add binary-level tests: (1) scan then empty a tracked file and assert rows are replaced and status is ok/partial per the fixed contract; (2) a broken-syntax file asserts `parse_failed`/`partial` not silent ok; (3) `export --out - --json` on a populated artifact asserts JSONL on stdout and report on stderr; (4) once implemented, a partial-scan test with one bad file among several good ones.

#### F7 — Per-row-commit tripwire asserts a hardcoded constant, not a measured commit count
- **Severity:** medium
- **Area:** writer performance contract / test quality
- **Files:** `crates/julie-extract-artifact/src/writer.rs:205`, `crates/julie-extract-artifact/src/writer.rs:232`, `crates/julie-extract-artifact/src/writer.rs:275`, `crates/julie-extract-artifact/src/writer.rs:326`, `crates/julie-extract-artifact/src/writer.rs:377`, `crates/julie-extract-artifact/src/writer.rs:439`, `crates/julie-extract-artifact/tests/writer_performance.rs:20`, `crates/julie-extract-artifact/tests/writer_contract.rs:28`
- **Claim:** `WriteResult.transactions_committed` is set to the literal `1` in every writer return path, and the tests that purport to guard against per-row commits assert on this literal. They cannot detect a real per-row-commit regression because the counter is decoupled from actual SQLite commit behavior.
- **Evidence:**
  - All six return paths hardcode the field: writer.rs:205 and :232 (delete_file), :275 and :326 (write_files), :377 and :439 (write_scan_snapshot) each return `transactions_committed: 1` as a literal.
  - `writer_performance.rs:20-23` asserts `result.transactions_committed, 1, "writer must not commit per file or per row"` and `writer_contract.rs:28` asserts `assert_eq!(result.transactions_committed, 1)`.
  - Contract: `sqlite-schema-v1.md:481` ("Avoid per-row commits and per-row schema or metadata reads.") and named gate at :532 ("tiny-fixture writer throughput in the default or contract tier"). The tripwire test passes today.
- **Impact:** The performance contract requires avoiding per-row commits and names a tiny-fixture writer throughput gate. The current commit-count tripwire gives false assurance: a future change that commits inside the per-file loop, or splits into multiple transactions, would still return the literal `1` and both named tests would still pass. The one-transaction guarantee currently rests solely on code structure plus the wall-clock budget (`elapsed < 750ms`), not on any executable check of commit count.
- **Recommendation:** Make the tripwire measure real commits. Install a rusqlite commit hook (`Connection::commit_hook`) for the duration of the write, count fired commits, and drive `transactions_committed` from that measured value instead of a literal so both the field and the assertion reflect reality. Keep the existing wall-clock budget as a secondary signal.

#### F13 — `artifact_metadata` parser/capability fingerprints are hardcoded constants, defeating drift detection (SQLite-writer view of F12)
- **Severity:** medium
- **Area:** metadata contract conformance
- **Files:** `crates/julie-extract-cli/src/commands.rs:1653`, `crates/julie-extract-cli/src/commands.rs:1654`, `crates/julie-extract-artifact/src/metadata.rs:47`, `crates/julie-extract-artifact/src/jsonl.rs:189`
- **Claim:** Same defect as F12, seen from the SQLite-writer dimension: the required metadata keys `parser_inventory_fingerprint` and `capability_snapshot_fingerprint` are written as fixed literal strings that never reflect the actual parser inventory or capability snapshot, so they cannot serve their contractual purpose of fingerprinting that state. The schema/writer store and round-trip the keys correctly; the producer never populates a true fingerprint.
- **Evidence:** `commands.rs:1653-1654` literals; stored verbatim through `metadata.rs:47-52 rows()` and echoed into JSONL at `jsonl.rs:189-190`. Contract keys at `sqlite-schema-v1.md:42-43`. The producer already materializes both inputs (`artifact_capability_snapshot()` at commands.rs:1432, `parser_inventory` at commands.rs:1472), so a real fingerprint is computable from data in hand.
- **Impact:** Write-path contract drift: a downstream consumer comparing fingerprints to decide whether parser packages or language capabilities changed will always see identical fingerprints even after a grammar/parser upgrade or capability change. The values are contract-shaped but semantically meaningless.
- **Recommendation:** Identical to F12 — compute both fingerprints from the data the CLI already builds and add a test asserting two snapshots with different parser sets / capability flags produce different fingerprints. (Fix F12 and F13 together; they are one defect.)

#### F16 — Total tree-sitter parse failure silently degrades to `Ok(empty)` instead of a typed error
- **Severity:** medium
- **Area:** pipeline / engine
- **Files:** `crates/julie-extractors/src/pipeline.rs:31`, `crates/julie-extractors/src/pipeline.rs:138`
- **Claim:** When tree-sitter returns no tree (`parse` returns `None`), `extract_canonical_with_parse` returns `Ok(degraded_parse_failure_result(content))` — an empty `ExtractionResults` carrying a single total-failure parse diagnostic — rather than `Err`. A file the parser cannot handle at all produces a "successful" extraction with zero symbols. This is the only silent failure-to-empty path in the extraction core; read errors, unsupported extensions, and parser-setup errors all return `Err`.
- **Evidence:** `pipeline.rs:31-33` → `let Some(tree) = parse(language, file_path, content)? else { return Ok(degraded_parse_failure_result(content)); };`; `pipeline.rs:138-144` builds `ExtractionResults::empty()` with one Error parse diagnostic.
- **Impact:** Mitigated, not benign. The parse diagnostic IS persisted (extraction.rs:255 → `parse_diagnostics` table → JSONL export) and the data-loss guard blocks the empty result from overwriting prior good rows, so no on-disk corruption today. But because the failure is `Ok`-typed, the CLI cannot surface it as a typed failure status; it shows up only as a `parse_diagnostics` row plus a zero-symbol `Indexed` file. On a first-time scan of an unparseable file, a consumer not joining `parse_diagnostics` reads it as "file has no symbols".
- **Recommendation:** Keep the deliberate degraded-mode behavior, but ensure status reflects it: when `degraded_parse_failure_result` is produced, the resulting `ArtifactFile` should carry `FailedPreserved` (ties to F11) so consumers distinguish "parser bailed" from "genuinely no symbols" without a join. At minimum, document that zero-symbol + non-empty `parse_diagnostics` means total parse failure.

#### F17 — Force-rebuild swallows artifact-file delete errors; can write a clean rebuild into a stale database
- **Severity:** medium
- **Area:** cli-commands / force rebuild
- **Files:** `crates/julie-extract-cli/src/commands.rs:1695`, `crates/julie-extract-cli/src/commands.rs:166`
- **Claim:** `remove_artifact_files` discards the result of every `fs::remove_file` via `let _ =` (commands.rs:1701-1703). It is called from `scan()` at :166 when `should_rebuild_db` is true (force scan whose existing DB belongs to a different root: `should_rebuild_db = args.force && db.exists() && force_existing_metadata.is_none()`, commands.rs:149). If a delete fails (locked file, permission, read-only dir), the error is swallowed and the scan proceeds to `ArtifactWriter::open_path` on the SAME old database (:175), producing a merged/stale artifact instead of the clean rebuild requested.
- **Evidence:** `commands.rs:1701-1703` → `if path.exists() { let _ = std::fs::remove_file(path); }` for db/-wal/-shm; caller `commands.rs:165-167` → `if should_rebuild_db { remove_artifact_files(&db); }` then :175 `ArtifactWriter::open_path(&db, metadata)`.
- **Impact:** A `--force` rebuild meant to discard a wrong-root artifact can silently keep old rows, yielding an artifact whose `root_path`/metadata may not match its contents, with exit code 0 and no error reported. Bounded blast radius (cross-root force-rebuild branch only) but a genuine silent failure that defeats the explicit force-rebuild intent.
- **Recommendation:** Have `remove_artifact_files` return `Result` and, on error, surface a typed report (e.g. `DbWriteFailed` or a new code) with exit 1 instead of continuing to write into the stale DB. At minimum, fail the scan if the primary db file still exists after the removal attempt.

#### F21 — `containing_symbol_id` is nondeterministic on priority+size ties (HashMap iteration order leaks into a persisted FK column)
- **Severity:** medium
- **Area:** determinism / relationship resolution
- **Files:** `crates/julie-extractors/src/base/creation_methods.rs:261`, `crates/julie-extractors/src/base/creation_methods.rs:277`, `crates/julie-extractors/src/base/creation_methods.rs:205`, `crates/julie-extractors/src/python/identifiers.rs:17`
- **Claim:** `find_containing_symbol_from_iter` sorts candidate symbols only by `(priority, end_byte-start_byte)` with no final stable tiebreaker, then returns `containing_symbols[0]`. The common caller `find_containing_symbol_from_map_filtered` feeds it `symbol_map.values()` from a std `HashMap<String,&Symbol>` built with the default randomized `RandomState` (no custom hasher). When two containing symbols share the same priority AND the same byte size at the same start position, the `[0]` pick depends on HashMap iteration order, which is randomized per process. The chosen `containing_symbol_id` is persisted as a SQLite FK and emitted as a JSONL field.
- **Evidence:**
  - `creation_methods.rs:261-277`: `containing_symbols.sort_by(|a,b|{ ...priority...; let size_a=a.end_byte-a.start_byte; let size_b=b.end_byte-b.start_byte; size_a.cmp(&size_b) }); Some(containing_symbols[0])`.
  - Fed (line 205) by `symbol_map.values().copied().filter(...)`.
  - Map built (python/identifiers.rs:17) as `HashMap<String,&Symbol> = symbols.iter().map(|s|(s.id.clone(),s)).collect()` with no hasher.
  - Persisted: schema.rs:128/229 `FOREIGN KEY (containing_symbol_id) REFERENCES symbols(symbol_id)`; emitted: jsonl.rs:803 `"containing_symbol_id": containing_symbol_id`.
- **Impact:** Two clean extractions of identical source can write different `containing_symbol_id` FK values into SQLite and JSONL, undermining the "stable opaque IDs" tradeoff (`sqlite-schema-v1.md:546`) and deterministic-export goal (`schema-principles.md:58`). 28 map-variant call sites across nearly all languages, so it is a cross-language hazard, not single-language. Note: the identifier's own primary id is `generate_id_for_span(name, span)` (creation_methods.rs:99) and does NOT fold in `containing_symbol_id`, so `identifier_id` / JSONL `record_id` themselves stay stable; only the `containing_symbol_id` field can drift.
- **Recommendation:** Add a deterministic final tiebreaker to the sort: `.then_with(|| a.start_byte.cmp(&b.start_byte)).then_with(|| a.id.cmp(&b.id))` so equal priority+size candidates resolve identically regardless of iteration order. Alternatively iterate a `BTreeMap` or sort values before selection. Worth a determinism test that re-extracts the same fixture twice and diffs the artifact.

#### F22 — Scala test detection flags every callable in a test-path file as a test, with no name/annotation guard
- **Severity:** medium
- **Area:** `test_detection` cross-language signal (`is_test`)
- **Files:** `crates/julie-extractors/src/test_detection.rs:129`, `crates/julie-extractors/src/test_detection.rs:137`, `crates/julie-extractors/src/scala/declarations.rs:90`
- **Claim:** `detect_scala` returns true for ANY callable symbol (Function/Method/Constructor) whose file is in a test path, with no name-prefix or annotation guard on that branch. This is materially broader than every other language arm: Java/Kotlin, Swift, PHP, Python all require a name prefix OR annotation IN ADDITION to the test path. A private helper like `def makeFixture(): User` inside a `*Spec.scala` file is incorrectly flagged `is_test=true`.
- **Evidence:**
  - `test_detection.rs:137-139` → `if is_test_path(file_path) { return true; }` with no name check; the only guard above it is the `@Test` annotation arm (line 131) and the only narrower arm below it (`name.starts_with("test")` at line 141) is unreachable because the path branch already returns.
  - `is_test_symbol` gates on `is_callable` (Function/Method/Constructor, line 70), and `scala/declarations.rs:90-99` calls `is_test_symbol` on real method/function declaration names.
  - Contrast `detect_swift` (line 293: `is_test_path && (name.starts_with("test") || lifecycle match)`), the java/kotlin path arm (line 82: `name.starts_with("test") && is_test_path`), and `detect_php` (line 219: `name.starts_with("test") && is_test_path`) which all add a name guard.
- **Impact:** Inflates the cross-language `is_test` signal (persisted in `symbols.metadata`, the authoritative test marker downstream consumers read) with false positives for Scala helper/utility/fixture methods in test files, skewing test-vs-production analysis for Scala only. No data loss; blast radius limited to Scala callable-declaration symbols. Scala's real call-style tests (`test()`/`it()`/FlatSpec clauses) are handled separately and correctly by `scala/test_calls.rs` via `classify_call_exact`.
- **Recommendation:** Tighten the path-only arm to match the swift/php/java pattern: keep the `@Test` annotation check, but for the path branch require a positive ScalaTest signal (name prefix `test`, recognized lifecycle names `beforeEach`/`afterEach`/`beforeAll`/`afterAll`, or the enclosing class extending a known ScalaTest base via `base_types`) instead of "any callable in a test dir". This puts Scala on equal footing with the other path-guarded languages.

#### F23 — Containing-symbol lookup is O(symbols) per identifier in TS/Python/C#; Rust alone uses a cached index
- **Severity:** medium
- **Area:** cross-language perf / extraction inequality
- **Files:** `crates/julie-extractors/src/base/creation_methods.rs`, `crates/julie-extractors/src/typescript/identifiers.rs`, `crates/julie-extractors/src/python/identifiers.rs`, `crates/julie-extractors/src/csharp/identifiers.rs`, `crates/julie-extractors/src/rust/identifiers/containing_symbols.rs`, `crates/julie-extractors/src/rust/identifiers.rs`
- **Claim:** TypeScript, Python, and C# resolve each identifier's containing symbol by calling `base.find_containing_symbol_from_map`, which on EVERY call re-filters the entire symbol map by file_path, collects into a `Vec`, and sorts it. Rust instead builds a `ContainingSymbolIndex` once per file (sorted by start position with precomputed priority/size and an early break) and queries it in a single pass. The result is equivalent, so this is a missing optimization in 3 of the 4 highest-traffic languages, not a correctness difference.
- **Evidence:**
  - `base/creation_methods.rs:197-210` `find_containing_symbol_from_map_filtered` → `find_containing_symbol_from_iter`, which at :219-244 does `symbol_map.values().copied().filter(file_path && include).collect::<Vec>()` then :261-275 `containing_symbols.sort_by(priority, size)` — executed once per identifier.
  - Per-node call sites: `typescript/identifiers.rs:303-312` invoked at :65,:80,:110,:134,:162,:196,:225,:336; `python/identifiers.rs:251-259` invoked at :59,:74,:103,:136,:152,:284; `csharp/identifiers.rs:324-331` invoked at :41,:53,:71,:90,:103,:356.
  - Contrast `rust/identifiers.rs:29` → `let containing_symbols = ContainingSymbolIndex::new(symbols, &file_path);` built ONCE, then `find()` called per node at :114,:165,:189,:212. `ContainingSymbolIndex` (`rust/identifiers/containing_symbols.rs:14-31`) sorts once by start_line/start_column; `find()` :39-51 iterates with `if candidate.symbol.start_line > pos_line { break; }` early-out.
- **Impact:** For a file with S symbols and I identifiers, TS/Python/C# do roughly O(I × S log S) work where Rust does O(S log S + I × S_window). On large source files (thousands of identifiers) this is a real multiplier on extraction cost, and extraction/export is already the slow surface (cold scan up to 18s, JSONL export 20-77s). Mainstream languages pay the penalty; the niche one (Rust) does not. The sort key differs (base sorts the post-filter candidate set by priority-then-size; Rust pre-sorts by position and selects best during the scan) but both return the same containing symbol, so it is purely a perf gap.
- **Recommendation:** Hoist the `ContainingSymbolIndex` (or an equivalent shared base helper that builds the sorted index once per file) and have TS/Python/C# (and ideally all languages still on `find_containing_symbol_from_map`) consume it, replacing the per-identifier filter+collect+sort. The selection logic is already proven equivalent, so this is a mechanical lift of a Rust-only optimization to the shared base.

#### F24 — `IdentifierKind::VariableRef` is in the contract but emitted by only 3 of ~38 language modules (none of rust/ts/python/csharp/go/java)
- **Severity:** medium
- **Area:** cross-language contract coverage
- **Files:** `crates/julie-extractors/src/base/kinds.rs`, `docs/contracts/sqlite-schema-v1.md`, `crates/julie-extractors/src/qml/identifiers.rs`, `crates/julie-extractors/src/r/identifiers.rs`, `crates/julie-extractors/src/yaml/mod.rs`
- **Claim:** The identifiers contract documents usage locations as "calls, variable references, type usages, and member accesses" (sqlite-schema-v1.md:209-210), and `IdentifierKind` defines `VariableRef` (base/kinds.rs:38, serialized as `variable_ref`). A full non-test grep shows `VariableRef` is produced ONLY by `qml/identifiers.rs:173`, `r/identifiers.rs:154` & `:167`, and `yaml/mod.rs:265` (YAML aliases). rust/typescript/python/csharp/go/java emit only `Call`, `MemberAccess`, and `TypeUsage` — never `VariableRef` — even though plain variable reads are pervasive in those languages. No per-language capability row in the docs distinguishes which identifier kinds a language emits.
- **Evidence:**
  - `base/kinds.rs:34-43` defines the four-variant `IdentifierKind` enum incl. `VariableRef`; :49,:61 serialize/deserialize it as `"variable_ref"`.
  - `sqlite-schema-v1.md:209-210` lists "calls, variable references, type usages, and member accesses".
  - Non-test grep for `IdentifierKind::VariableRef` returns only qml/identifiers.rs:173, r/identifiers.rs:154 & :167, yaml/mod.rs:265 (plus the enum def). Per-language tally for rust/typescript/python/csharp/go/java each yields exactly `{Call, MemberAccess, TypeUsage}`. There are 38 language directories under `src/`.
- **Impact:** A downstream consumer doing "find references to this variable" gets results for QML/R/YAML but nothing for Rust/TS/Python/C#/Go/Java. The capability is advertised by the schema and the enum but is implemented unevenly, so cross-language reference queries are silently incomplete for the most-used languages, and nothing in the contract or capability rows tells a consumer to expect that.
- **Recommendation:** Decide intent and make it uniform: either (a) emit `VariableRef` for variable reads in the high-traffic languages so the contract holds across the family, or (b) if `variable_ref` is deliberately out of scope for code languages, document that in `sqlite-schema-v1.md` and ideally surface it in a per-language capability row so consumers know not to expect it. Do not leave a contract-listed kind implemented in 3 niche modules only.

#### F25 — Default-suite wall-clock budget tripwire does not exist (only an aspirational doc bullet)
- **Severity:** medium
- **Area:** test-tiers / guardrails
- **Files:** `docs/testing-strategy.md:235`, `crates/julie-extract-artifact/tests/writer_performance.rs:27`, `xtask/src/dogfood.rs:537`
- **Claim:** `testing-strategy.md` lists two budget tripwires as Guardrails. The tiny-fixture WRITER budget is implemented, but the DEFAULT-SUITE wall-clock budget is not implemented anywhere: no aggregate timing assertion in any test crate, in xtask, or in CI. The only `Instant`/`elapsed` in xtask is dogfood's per-call report-only timing, which is not a tripwire.
- **Evidence:**
  - `testing-strategy.md:235` → "Add a default-suite wall-clock budget before implementation work grows." Grep for `suite.*budget|wall.?clock|SUITE_BUDGET|MAX_SUITE` across crates/ xtask/ .github/ returns zero matches.
  - By contrast `writer_performance.rs:27-30` implements the OTHER bullet: `assert!(elapsed < Duration::from_millis(750), "tiny fixture writer tripwire exceeded budget: {elapsed:?}")`.
  - `xtask/src/dogfood.rs:537-545` → `let started = Instant::now(); ... Ok((started.elapsed(), output))` just RETURNS the elapsed Duration; there is no comparison or assertion against any ceiling.
- **Impact:** The default tier runs the full `julie-extractors` lib test set plus two package suites with no wall-clock ceiling, so the suite can degrade incrementally with no automated signal. This is the exact failure mode `testing-strategy.md` opens by warning against. The doc presents this guardrail as existing policy when it is not enforced.
- **Recommendation:** Either implement the guardrail (time `cargo xtask test default` in xtask and fail above a documented ceiling, or add a `#[test]` asserting a representative timing/test-count stays under budget) or downgrade the doc bullet from a stated Guardrail to a TODO so the doc stops asserting an enforced contract that does not exist.

#### F26 — Tier-leak convention test enforces a hardcoded module list, not a structural rule, so a new ungated slow module would silently run in default
- **Severity:** medium
- **Area:** test-tiers / convention
- **Files:** `crates/julie-extractors/src/tests/test_tiers.rs:8`, `crates/julie-extractors/src/tests/test_tiers.rs:56`, `crates/julie-extractors/src/tests/qml/mod.rs:84`
- **Claim:** `test_tier_convention_keeps_slow_gates_out_of_default_suite` checks a fixed, hand-edited list of named modules (golden, capability_matrix, parser_upgrade, pending_shape_contract, qml::real_world, r::file_integration_bug, r::real_world, plus two literal-string fixture checks). `assert_module_is_feature_gated` only verifies that THOSE named modules carry the expected `#[cfg]` above their `pub mod` line. There is no positive/structural rule (e.g. "any module named real_world must be gated"). A developer adding e.g. `swift::real_world` without a gate would not trip this test and would run in the default tier.
- **Evidence:**
  - `test_tiers.rs:56-81` `assert_module_is_feature_gated` scans for the literal `pub mod {module};` line and checks the previous non-empty line equals the expected cfg; it is invoked only for the explicitly named modules at lines 15-29.
  - No filesystem walk exists: grep for `WalkDir|read_dir|fs::read_dir|walkdir` in this file (and `xtask/tests/test_tiers.rs`) returns zero matches.
  - The test relies on a static allowlist, so unlisted modules are simply never inspected. (Verified leak-free TODAY: the only `pub mod real_world`/`file_integration_bug` declarations are qml::real_world (qml/mod.rs:84), r::file_integration_bug (r/mod.rs:81), r::real_world (r/mod.rs:88) — all gated and all checked.)
- **Impact:** The convention test gives a false sense of completeness against its stated purpose (`testing-strategy.md:243-244`: "fails if known slow gates are no longer feature-gated out of the default suite"). It protects only the modules someone remembered to list, not future additions — which is the realistic regression path.
- **Recommendation:** Add a structural rule derived from the filesystem: walk `src/tests/**`, and fail if any module named `real_world`/`*file_integration*`, or any test source referencing `fixtures/real-world`, is reachable without the appropriate `#[cfg(feature = "test-real-world")]` gate. Keep the allowlist only as an extra cross-check, not the sole mechanism.

#### F27 — UTF-8 BOM is never stripped: byte/line offsets, content_bytes, and content_hash all include the 3 leading BOM bytes
- **Severity:** medium
- **Area:** robustness / offset correctness on BOM-prefixed source
- **Files:** `crates/julie-extract-cli/src/extraction.rs:56`, `crates/julie-extract-cli/src/extraction.rs:65`, `crates/julie-extractors/src/pipeline.rs:29`
- **Claim:** `read_source_snapshot` uses `fs::read_to_string`, which preserves a leading UTF-8 BOM (EF BB BF) as the first character of the content string. No extractor or the pipeline strips it (grep for `feff`/BOM across `crates/julie-extractors/src` and `crates/julie-extract-cli/src` returns nothing). The BOM-included string is what gets hashed, byte-counted, passed to tree-sitter, and byte-sliced. BOM-prefixed files are routine on Windows-authored C#, TS, and UTF-8-with-signature files.
- **Evidence:**
  - `extraction.rs:56` → `fs::read_to_string(&target.absolute_path)`.
  - `extraction.rs:65` → `content_bytes: content.len() as i64` (includes 3 BOM bytes); content_hash hashes `content.as_bytes()` (BOM included).
  - `pipeline.rs:29` passes content unmodified to `detect_language_for_source` and the parser.
- **Impact:** All byte offsets stored in symbols/identifiers/literals are internally consistent with the stored content (so a consumer slicing the same DB string is safe), but a consumer that re-reads the on-disk source with the BOM stripped (common) will be off by 3 bytes for every span in the file. tree-sitter may also emit a spurious leading ERROR diagnostic for the BOM in some grammars. `content_bytes` is inflated by 3. No test covers BOM input.
- **Recommendation:** Strip a leading U+FEFF in `read_source_snapshot` before hashing/counting/parsing (the `ignore` crate already does exactly this for `.gitignore` files — see ignore-0.4.25 gitignore.rs:411-414). Add a BOM-prefixed fixture test asserting byte offsets and `content_bytes` match the de-BOM'd content.
- **Confidence:** flagged by completeness pass, strong evidence (85).

#### F28 — Binary / invalid-UTF-8 files with a supported extension abort the whole batch instead of being recorded as failed
- **Severity:** medium
- **Area:** untrusted-input robustness / batch abort
- **Files:** `crates/julie-extract-cli/src/extraction.rs:56`
- **Claim:** Discovery selects files purely by extension (discovery.rs:212 `language_for_path`). A file with a supported extension (e.g. a corrupt or binary-but-`.py`/`.rs`/generated `.ts`) that is not valid UTF-8 makes `fs::read_to_string` return an `Err`, producing `ExtractFileErrorKind::Read`. Per F11, a single such file aborts the entire scan transaction — so one binary blob with a code extension blocks indexing of the whole repo.
- **Evidence:** `extraction.rs:56` → `fs::read_to_string(...).map_err(... ExtractFileErrorKind::Read ...)`. `read_to_string` fails on any non-UTF-8 byte sequence. `discovery.rs:212-215` selects by extension only, so binary files masquerading as source reach this read. This is the untrusted-input manifestation of F11; recorded separately because the trigger (binary file with a code extension) is common in real repos (generated fixtures, accidental commits) and was not called out.
- **Impact:** A normal repo containing e.g. a non-UTF-8 `.py` test fixture or a corrupt `.ts` file causes the entire scan to fail with exit 1 and no artifact, rather than skipping/flagging that one file. Couples to the `FailedPreserved`-never-emitted gap (F11).
- **Recommendation:** Read as bytes and lossily decode (or detect non-UTF-8 and emit `FileStatus::FailedPreserved` for that file) instead of failing the read with `?`. This is the same fix F3/F11 call for, with binary input as an explicit test case.
- **Confidence:** flagged by completeness pass (80).

---

### LOW

#### F18 — `revision.mode` nullability disagrees between SQLite schema (nullable) and jsonl-v1 contract (non-null enum)
- **Severity:** low
- **Area:** contract drift / JSONL record shape
- **Files:** `crates/julie-extract-artifact/src/jsonl.rs:436`, `docs/contracts/jsonl-v1.md:247`, `docs/contracts/sqlite-schema-v1.md:85`
- **Claim:** The SQLite schema declares `mode TEXT` (nullable, sqlite-schema-v1.md:85) and the exporter reads it as `Option<String>` (jsonl.rs:436), serializing JSON null when the column is NULL. But jsonl-v1.md:247 declares the revision record's `mode` as a required enum ("`mode`: `incremental`, `force`, or `single_file`") with no null allowance. A NULL mode row produces a JSONL record whose `mode` field violates the stated jsonl-v1 contract.
- **Evidence:** `jsonl.rs:436` → `row.get::<_, Option<String>>(3)?` for mode; serialized at jsonl.rs:464. `sqlite-schema-v1.md:85` → `mode TEXT,` (no NOT NULL; contrast `operation TEXT NOT NULL` on line 84). `jsonl-v1.md:247` → `- mode: incremental, force, or single_file` (no "or null", unlike `parent_revision_id` on line 245 which explicitly says "integer or null").
- **Impact:** A downstream consumer parsing `revision.mode` as a strict non-null enum (which the jsonl-v1 doc authorizes) would break on a JSON null. The code is defensively correct given the schema; the two contract docs are out of sync on nullability.
- **Recommendation:** Pick one source of truth: either change jsonl-v1.md:247 to say "or `null`" (matching SQLite and the code), or make the SQLite `mode` column NOT NULL and read it as required `String`. Given mode is normally always written by the writer, NOT NULL is the cleaner contract; if it must stay nullable, the JSONL doc must say so.

#### F29 — `export --out -` mid-stream failure routes the JSON report to stdout, conflicting with already-emitted JSONL
- **Severity:** low
- **Area:** export stream split
- **Files:** `crates/julie-extract-cli/src/commands.rs:630`, `crates/julie-extract-cli/src/commands.rs:654`, `crates/julie-extract-cli/src/commands.rs:676`, `docs/contracts/reports.md:241`
- **Claim:** Precise restatement of F10's stream-routing bug. On a successful export with `--out -`, the report is correctly routed to stderr (commands.rs:654-658). With `--out -`, JSONL is written directly to a locked stdout (commands.rs:630-633). But if `export_jsonl` fails partway, the failure outcome is hard-coded to `ReportStream::Stdout` (commands.rs:676) even when `--out` is `-`. Any JSONL bytes already written to stdout plus a JSON report on stdout means stdout is no longer cleanly either JSONL or a single report object.
- **Evidence:** `commands.rs:630-633` writes JSONL to stdout lock when `args.out == Path::new("-")`; `commands.rs:654-658` success arm selects Stderr when out is `-`; `commands.rs:676` → `outcome(report, 1, args.json, ReportStream::Stdout)` inside the `export_jsonl` Err arm with no out-aware branch. `reports.md:241` → "export --out - --json writes JSONL to stdout and the final report to stderr." (Note: the earlier `unsupported_format` check at commands.rs:625 and open_artifact failure also route to Stdout but occur before any JSONL is written, so they are harmless — only the mid-stream `export_jsonl` failure interleaves.)
- **Impact:** Low likelihood (mid-stream JSONL write failure is rare) but when it happens a machine consumer reading stdout as the report, or as pure JSONL, gets corrupted mixed output instead of the documented split. Same defect as F10; kept as a low-severity precise entry.
- **Recommendation:** Route the export failure report to `ReportStream::Stderr` whenever `args.out == "-"`, matching the success path at 654-658, so stdout stays JSONL-only in stdout-streaming mode. (Single fix resolves both F10 and F29.)

#### F30 — Documented `usage_error` code is never emitted as structured JSON (clap text + exit 2 only)
- **Severity:** low
- **Area:** error codes / usage handling
- **Files:** `crates/julie-extract-cli/src/commands.rs:41`, `docs/contracts/reports.md:166`, `docs/contracts/reports.md:253`
- **Claim:** Argument-parser failures return clap's exit code with clap's text printed (commands.rs:41-47); the stable code `usage_error` (reports.rs:253 enum, reports.md:166) is never emitted in a JSON report. This matches the explicit open-decision note in reports.md:253-256 (text+exit 2 acceptable before `--json` is recognized), so it is defensible, but the documented `usage_error` code currently has no producer in the CLI.
- **Evidence:** `commands.rs:44-46` → `let exit_code = error.exit_code(); let _ = error.print(); return ExitCode::from(exit_code as u8);`. grep for `UsageError` across `crates/julie-extract-cli/src` returns zero hits. `reports.md:253-256` documents this as an open decision: "argument-parser failures before that point may be text plus exit code 2."
- **Impact:** Minor: a consumer that branches on error code `usage_error` will never see it; usage failures are text-only. Acceptable per the contract's stated open decision, but worth tracking so the code is either wired up or its limited applicability documented.
- **Recommendation:** Either emit a `usage_error` JSON report when `--json` is present and recognized, or annotate reports.md that `usage_error` is reserved/not currently produced.

#### F31 — `body_hash` span invariant is not enforced or tested at the writer boundary
- **Severity:** low
- **Area:** schema invariant
- **Files:** `crates/julie-extract-artifact/src/writer.rs:867`, `crates/julie-extract-cli/src/extraction.rs:150`, `crates/julie-extract-cli/src/extraction.rs:174`, `crates/julie-extractors/src/base/creation_methods.rs:39`
- **Claim:** The contract invariant "body_hash is present only when all body span columns are present" holds today only by construction in the extractors; `insert_symbols` binds `symbol.body_hash` blindly with no validation, and there is no test asserting the invariant at the artifact boundary.
- **Evidence:** Contract invariant at `sqlite-schema-v1.md:186` ("body_hash is present only when all body span columns are present."). `insert_symbols` (writer.rs:856-896) binds `symbol.body_hash` at writer.rs:890 with no check that the six body span fields are `Some`. The invariant is satisfied upstream: `creation_methods.rs:38-39` computes `body_hash = body_span.and_then(|span| body_hash(...))` so `body_hash` is `Some` only when `body_span` is `Some`; the CLI derives all six body span columns from the same `symbol.body_span` (extraction.rs:150-173) and copies `body_hash` independently (extraction.rs:174). A grep of tests for `body_hash` found only column-presence checks (jsonl_contract.rs:205/554/566, schema_contract.rs:320) and no test asserting `body_hash.is_some()` implies spans present.
- **Impact:** Limited today: the CLI path is correct by construction. Risk is a silent contract violation if any of 34+ language extractors, a manual-symbol path, or a non-Rust caller using the crate API sets `body_hash` with null spans; the writer would persist a contract-violating row. Downstream readers relying on the invariant (a hash maps to a real body range) could be misled.
- **Recommendation:** Add a `debug_assert` or cheap validation in `insert_symbols` (or a model-level constructor) that `body_hash.is_some()` implies all six body span columns are `Some`, and add a writer_contract round-trip test that sets `body_hash` and verifies the spans are present.

#### F32 — Force rebuild on root mismatch deletes the artifact in place before writing a fresh one (non-atomic window)
- **Severity:** low
- **Area:** scan --force replacement
- **Files:** `crates/julie-extract-cli/src/commands.rs:149`, `crates/julie-extract-cli/src/commands.rs:165`, `crates/julie-extract-cli/src/commands.rs:1695`
- **Claim:** When `--force` targets an existing DB whose stored `root_path` does not match the requested root, the CLI removes the `.db`/`-wal`/`-shm` files in place and then opens a brand-new writer at the same path, rather than writing to a temp file and atomically renaming. It also ignores the result of each `remove_file`.
- **Evidence:** `commands.rs:149` → `let should_rebuild_db = args.force && db.exists() && force_existing_metadata.is_none();`. `force_existing_metadata` is None when the stored root mismatches the requested root (commands.rs:141 matches `Ok(artifact) if artifact.report.root_path == display_path(&root)` and falls through to None at :144 otherwise). `commands.rs:165-167` then calls `remove_artifact_files(&db)` before `ArtifactWriter::open_path` at :175. `remove_artifact_files` (1695-1705) deletes db/-wal/-shm with `let _ = std::fs::remove_file(path)`, discarding the Result. `open_path` (writer.rs:67-80) recreates the schema and re-inits metadata since the file no longer exists. Contract permits the atomic temp-and-swap optimization (`sqlite-schema-v1.md:488-489`) and requires readers never observe a successful artifact without required indexes (:491).
- **Impact:** Narrow: triggers only on `--force` with a root-path mismatch, an uncommon recovery case. During the window between deletion and the writer finishing `create_schema` + the scan transaction, a concurrent reader sees a missing file or an empty/partial DB. The normal `--force` path on a matching root is fully transactional. Secondary issue: swallowed `remove_file` errors (also F17) — an unlinkable stale file would let `open_path` reopen the old DB and produce mixed state with no error surfaced.
- **Recommendation:** For the rebuild-on-mismatch case, write the new artifact to a sibling temp path and atomically rename over the old one after the scan transaction commits, and surface remove/rename failures instead of discarding them. If single-writer/no-concurrent-reader is the intended assumption, document it and still propagate the unlink error.

#### F33 — reports.md internally contradicts itself on the export report `mode` value (export vs jsonl); code emits jsonl
- **Severity:** low
- **Area:** contract drift (doc-internal)
- **Files:** `docs/contracts/reports.md:106`, `docs/contracts/reports.md:224`, `crates/julie-extract-artifact/src/reports.rs:103`, `crates/julie-extract-cli/src/commands.rs:644`
- **Claim:** `reports.md:106-107` lists `export` among the valid mode string values ("operation-specific mode such as `incremental`, `force`, `single_file`, `read_only`, or `export`"), while reports.md:224 (the per-command export requirement) says the export mode is `jsonl`. The code emits `jsonl`: `ReportMode` derives `#[serde(rename_all = "snake_case")]` so `ReportMode::Jsonl` serializes to `"jsonl"`, and `export()` always builds the report with `ReportMode::Jsonl`. There is no `ReportMode::Export` variant.
- **Evidence:** `reports.md:106-107` lists `export`; `reports.md:224` → `mode: jsonl`. `reports.rs:102-110` → `#[serde(rename_all = "snake_case")] pub enum ReportMode { Incremental, Force, SingleFile, ReadOnly, Jsonl, CapabilitySnapshot }` (no Export variant). `commands.rs:644` and 665 use `ReportMode::Jsonl`. Pinned by `operations_contract.rs:426` → `assert_eq!(report["mode"], "jsonl")`.
- **Impact:** Doc-internal inconsistency only; the code and the authoritative per-command clause (reports.md:224) agree on `jsonl`. A reader scanning the mode list at reports.md:106 could wrongly branch on a mode value ("export") the binary never emits.
- **Recommendation:** Fix reports.md:106-107 to say `jsonl` instead of `export` (or add `jsonl` and drop `export`) so the prose mode list matches the per-command export requirement and the emitted value.

#### F34 — Export report totals silently report 0 if a COUNT query fails (swallowed Result)
- **Severity:** low
- **Area:** silent failure / robustness
- **Files:** `crates/julie-extract-cli/src/commands.rs:1380`, `crates/julie-extract-cli/src/commands.rs:648`
- **Claim:** `table_count()` maps any rusqlite error from `SELECT COUNT(*) FROM <table>` to 0 via `unwrap_or(0)`. `table_totals()` (used to fill `counts.totals` for the export report via `with_totals` at commands.rs:648) calls it for all 18 tables (commands.rs:1356-1377). If a count query fails for any reason (locked table, corruption mid-read), the report claims a total of 0 for that domain instead of surfacing an error, while the export itself may have succeeded.
- **Evidence:** `commands.rs:1380-1385` → `let sql = format!("SELECT COUNT(*) FROM {table}"); connection.query_row(&sql, [], |row| row.get(0)).unwrap_or(0)`. `commands.rs:648` → `.with_totals(table_totals(&artifact.connection))`. (`latest_revision_id` at 1387-1394 also uses `unwrap_or(None)`.)
- **Impact:** Low blast radius: the artifact was just opened successfully and totals are summary metadata, not the exported data. But a per-table read failure would be reported as a legitimate-looking total of 0, which a downstream consumer cannot distinguish from a genuinely empty table.
- **Recommendation:** Have `table_count` return `Result` and propagate failures into the report as a warning rather than silently coercing to 0, or at minimum log the swallowed error.

#### F35 — JSONL export serialization does a `json!` → Value → `to_writer` double pass plus per-row JSON re-parse
- **Severity:** low
- **Area:** performance
- **Files:** `crates/julie-extract-artifact/src/jsonl.rs:1288`, `crates/julie-extract-artifact/src/jsonl.rs:1297`, `crates/julie-extract-artifact/src/jsonl.rs:1458`
- **Claim:** Each record is built as a `serde_json::Value` tree via the `json!` macro (jsonl.rs:1288), then serialized with `serde_json::to_writer` (a second traversal, jsonl.rs:1297), and every JSON text column (`metadata_json`, `*_json` arrays, `kind_coverage`) is re-parsed from string into a Value per row via `parse_json` (jsonl.rs:1458-1459) before being re-serialized. For the largest tables this is many small allocations and two serialization passes. The architecture is otherwise sound: rows stream from SQLite via `query_map` and are written incrementally through a single `BufWriter`, with no full-table `Vec` materialization, so the cost is CPU/alloc in serialization, not memory or row-fetch.
- **Evidence:** `write_record` builds `let envelope = json!({...})` (jsonl.rs:1288) then `serde_json::to_writer(&mut *writer, &envelope)` (jsonl.rs:1297). `parse_json` re-parses each JSON column: `serde_json::from_str(value)` (jsonl.rs:1458-1459), called from `required_object`/`optional_object`/`required_array` (jsonl.rs:1420-1456) per row. Streaming confirmed: every `export_*` function uses `for row in rows { ... }` over `query_map` iterators (jsonl.rs:225,282,...,1242), no `Vec` collect.
- **Impact:** Export is the slowest surface (20-77s per the dogfood tracker). This is a performance characteristic, not a correctness bug; the streaming + buffering design is correct and memory-bounded. The addressable cost is the Value-tree round trip and per-row column re-parse. (The 20-77s number is report-only, not independently profiled, so "likely dominant cost" is a hypothesis.)
- **Recommendation:** If export latency becomes a priority, serialize directly to the writer with a serde `Serializer` or `write_all` of pre-validated fields instead of building an intermediate Value, and consider treating already-stored JSON columns as raw JSON using `serde_json::value::RawValue` to avoid the parse+reserialize round trip. Not a v0.1.0 blocker.

#### F36 — u32 byte/line offsets truncate or overflow on >4GB files with no size guard at read time
- **Severity:** low
- **Area:** robustness / numeric overflow
- **Files:** `crates/julie-extractors/src/base/span.rs:33`, `crates/julie-extractors/src/base/span.rs:64`, `crates/julie-extractors/src/base/embedded_span.rs:40`, `crates/julie-extractors/src/pipeline.rs:100`, `crates/julie-extract-cli/src/extraction.rs:56`
- **Claim:** All byte/line offsets are u32. `NormalizedSpan::from_node` casts `node.start_byte()`/`end_byte()` (usize) `as u32` (span.rs:33-34); `with_offset` (span.rs:64) and `EmbeddedSpanOffset::apply` (embedded_span.rs:40) do unchecked u32 additions; `jsonl_records` does `byte_offset += chunk.len() as u32` (pipeline.rs:100). On a >4GB source file these truncate to wrong positions and, in a debug build, the jsonl accumulator add would overflow-panic. No file-size limit is enforced at read time (`read_source_snapshot` just does `fs::read_to_string`, extraction.rs:56-68, with no size check).
- **Evidence:** `span.rs:33` → `start_byte: node.start_byte() as u32`; `span.rs:64-65` → `start_byte: self.start_byte + offset.byte_delta`; `embedded_span.rs:40` → `start_byte: span.start_byte + self.byte_delta`; `pipeline.rs:100` → `byte_offset += chunk.len() as u32`. grep for max-size/byte-limit guards in cli read path and discovery returns none.
- **Impact:** Practically unreachable (single 4GB+ source file), and tree-sitter itself uses u32 internally so the design choice is consistent with the parser. The realistic exposure is a missing guard, not a wrong-on-normal-input bug. (Related: the absence of any file-size cap also leaves the OOM/latency surface for large-but-sub-4GB files unbounded — see Coverage & Gaps.)
- **Recommendation:** Add an explicit max-file-size check at read time (reject or mark `FailedPreserved` above a configured byte ceiling well under `u32::MAX`) so truncation can never occur silently, and document the limit in the CLI contract.

#### F37 — `read_source_snapshot` labels all read failures as "could not be read as UTF-8", masking true I/O errors
- **Severity:** low
- **Area:** cli-extraction / error messages
- **Files:** `crates/julie-extract-cli/src/extraction.rs:56`
- **Claim:** `fs::read_to_string` fails both on non-UTF-8 content and on plain I/O errors (permission denied, file vanished, is-a-directory). The error mapper hardcodes "source file could not be read as UTF-8" for all of these, carrying the real `std::io::Error` only via `{error}` interpolation.
- **Evidence:** `extraction.rs:56-61` → `fs::read_to_string(&target.absolute_path).map_err(|error| ExtractFileError { kind: ExtractFileErrorKind::Read, ... message: format!("source file could not be read as UTF-8: {error}") })?`.
- **Impact:** Correct error kind (Read) and the underlying OS error is appended, so it fails loud and the detail is recoverable. But the leading message is misleading for permission/IO failures, which can confuse downstream consumers or users diagnosing a failed scan. Cosmetic, not a data-safety issue.
- **Recommendation:** Make the message generic ("source file could not be read") and let `{error}` convey the cause, or branch on `error.kind()` (`InvalidData` => UTF-8, else => I/O).

#### F38 — Pure smoke-only extractor test exists (asserts only `!is_empty()`, no symbol/kind check)
- **Severity:** low
- **Area:** test-quality / weak-assertions
- **Files:** `crates/julie-extractors/src/tests/zig/extractor.rs:27`, `crates/julie-extractors/src/tests/typescript/extractor.rs:37`
- **Claim:** A minority of extractor tests assert only that extraction returned something, never that the right symbol/name/kind was produced. `test_zig_basic_extraction` parses `pub fn main() void { var x: i32 = 5; }` and its sole assertion is `assert!(!symbols.is_empty())` (the banned smoke-only pattern). Most other existence assertions (e.g. typescript/extractor.rs:37) are immediately followed by a real value check and are preconditions, not the sole assertion.
- **Evidence:** `zig/extractor.rs:5-28`: the entire test body ends at line 27 with the only assertion `assert!(!symbols.is_empty());` and no name/kind check. Contrast `typescript/extractor.rs:35-38`: `let symbols = extractor.extract_symbols(&tree); assert!(!symbols.is_empty()); assert!(symbols.iter().any(|s| s.name == "getUserData"));` — here the weak assert is a precondition and a real value check follows.
- **Impact:** Smoke-only tests pass regardless of whether the extractor produced the correct symbol, so they would not catch a regression returning wrong-named or wrong-kind symbols. Limited blast radius: a clear minority, and most languages have stronger sibling tests covering the same surface. (The broad prevalence count of `len()>0`/`!is_empty()` lines was not independently re-counted; the concrete example is load-bearing here.)
- **Recommendation:** Tighten the genuinely smoke-only cases to assert the expected symbol name and kind (zig basic should assert `main` is a Function and `x` a Variable, mirroring the typescript pattern). Leave the many `!is_empty()` lines that already precede a value check as-is.

#### F39 — Seven `#[ignore]` debug tests are dead AST-dump scaffolding with no assertions
- **Severity:** low
- **Area:** test-quality / dead-code
- **Files:** `crates/julie-extractors/src/tests/scala/ast_debug.rs:30`, `crates/julie-extractors/src/tests/scala/mod.rs:51`, `crates/julie-extractors/src/tests/r/basics.rs:12`, `crates/julie-extractors/src/tests/r/data_structures.rs:12`
- **Claim:** All 7 `#[ignore]` tests are pretty-print-the-tree-sitter-AST helpers, not assertions. `scala/ast_debug.rs` holds `debug_scala_enum_ast` / `debug_scala_extends_ast` / `debug_scala_import_ast` / `debug_scala_package_ast`; `scala/mod.rs:52` `debug_scala_ast`; `r/basics.rs:13` `debug_r_ast`; `r/data_structures.rs:13` `debug_data_structures_ast`. Each only parses then calls a debug print routine with `println` and zero asserts. The `debug_print_tree` helper is duplicated across `scala/mod.rs` and `scala/ast_debug.rs`.
- **Evidence:** `scala/ast_debug.rs:1` → `//! AST exploration tests for Scala (run with --ignored --nocapture)`; bodies (e.g. lines 40-42) call only `debug_print_tree(tree.root_node(), code, 0);` with no assertion. `r/basics.rs:12` → `#[ignore] // Debug test to inspect AST`. Grep confirms exactly 7 `#[ignore]` in `src/tests`: 4 in scala/ast_debug.rs (lines 31,45,66,77), 1 scala/mod.rs:51, 1 r/basics.rs:12, 1 r/data_structures.rs:12. `grep -c fn debug_print_tree` returns 1 in scala/mod.rs and 1 in scala/ast_debug.rs (duplicated helper).
- **Impact:** Exploration scaffolding left in the committed suite. They never run in any tier and carry no value assertion, so they are dead test code that violates the project no-stub/no-decoration test rule, and the duplicated helper is real duplication. Harmless functionally.
- **Recommendation:** Remove the 7 debug tests and consolidate/remove the duplicated `debug_print_tree` helper. If deliberately kept as developer tooling, move AST exploration to an xtask subcommand rather than `#[ignore]`d tests in the suite.

#### F40 — Embedded language policy and capabilities.json panic on malformed config (already covered by default-tier parse tests)
- **Severity:** low
- **Area:** literal carrier policy / capability snapshot loading
- **Files:** `crates/julie-extractors/src/language_policy.rs:130`, `crates/julie-extractors/src/capability_snapshot.rs:121`, `crates/julie-extractors/src/tests/language_policy.rs:77`, `crates/julie-extractors/src/tests/capability_snapshot_test.rs:6`
- **Claim:** `load_embedded_literal_carrier_policies()` panics on a malformed TOML and `capability_snapshot()` `expect()`s on malformed `capabilities.json`. Both inputs are `include_str!`'d at compile time (developer-controlled repo files, not untrusted input) and loaded lazily via `OnceLock` on first use. The recommended belt-and-suspenders convention test ALREADY EXISTS for both: a default-tier (non-feature-gated) test triggers each parse.
- **Evidence:** `language_policy.rs:130-132` → `toml::from_str(content).unwrap_or_else(|err| panic!("failed to parse embedded language policy for {language}: {err}"))`; `capability_snapshot.rs:117-122` → `OnceLock` + `.expect("capabilities.json must be valid JSON ...")`. Both sources are `include_str!`'d. `tests/mod.rs:36` declares `pub mod language_policy;` ungated; `tests/language_policy.rs:77-78` `embedded_literal_carrier_policy_loads_and_aliases_jsx_tsx` parses every embedded TOML. `tests/mod.rs:13` declares `pub mod capability_snapshot_test;` ungated; `tests/capability_snapshot_test.rs:6-7` `test_capability_snapshot_loads_all_languages` parses `capabilities.json`. So a malformed embedded config fails the DEFAULT suite.
- **Impact:** Low. Inputs are compile-time-embedded repo files, so corruption is developer error caught by CI's default test run, not untrusted input. Panic messages are clear; no data-loss risk. The practical risk is lower than it might appear because default-tier coverage already exists for both files.
- **Recommendation:** No action required for v0.1.0. The recommended convention test is effectively redundant. If you want a single dedicated assertion that documents intent, add one explicit `toml::from_str`/`serde_json::from_str` loop test, but it would only formalize coverage that already exists.

#### F41 — Per-call `Regex::new` recompilation in several lower-traffic languages vs LazyLock caching in rust/python/java/go
- **Severity:** low
- **Area:** cross-language perf / extraction inequality
- **Files:** `crates/julie-extractors/src/razor/relationships.rs`, `crates/julie-extractors/src/zig/variables.rs`, `crates/julie-extractors/src/zig/imports.rs`, `crates/julie-extractors/src/gdscript/mod.rs`, `crates/julie-extractors/src/go/functions.rs`
- **Claim:** Several extractors compile regexes with `Regex::new(...)` inside per-node or per-call code paths rather than caching them in a `LazyLock` static. `razor/relationships.rs:271` and `:305` compile `@bind-(\w+)` and `@on(\w+)` inside `extract_element_relationships`, which runs per HTML element during the relationship walk (and `:215` compiles a third `<([A-Z]...)` per element); `zig/variables.rs:75` compiles a regex per generic-type-constructor node; `gdscript/mod.rs:165` & `:173` compile per type-inference call. Meanwhile go/functions.rs, rust, python, java cache identical-style regexes in `LazyLock` and compile once.
- **Evidence:** `razor/relationships.rs:271` → `if let Some(captures) = regex::Regex::new(r"@bind-(\w+)").unwrap().captures(&element_text)` and `:305` → `regex::Regex::new(r"@on(\w+)").unwrap()` both inside `fn extract_element_relationships` (`:205`) reached per element; `:215` also `regex::Regex::new(r"<([A-Z][A-Za-z0-9]*)\b")` per element. `zig/variables.rs:75` → `let param_match = Regex::new(r"\(([^)]+)\)").unwrap().find(&node_text);` inside `extract_generic_type_constructor`. `gdscript/mod.rs:165` → `regex::Regex::new(r"->\s*(\w+)").ok()?` and `:173` → `regex::Regex::new(r"(?:var|const)\s+\w+\s*:\s*(\w+)").ok()?` inside `infer_type_from_signature`. Contrast `go/functions.rs:11` → `static GO_FUNCTION_SIGNATURE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(...))`. Non-test grep: 76 per-call `Regex::new` sites vs 115 `LazyLock`-cached.
- **Impact:** Regex compilation is expensive relative to matching; recompiling per node multiplies extraction cost for the affected languages and is wasted work. Blast radius is bounded (mostly lower-traffic languages: razor/zig/gdscript), so severity is low, but it is the same class of cross-language inequality as F23.
- **Recommendation:** Move per-call `Regex::new` sites to module-level `LazyLock<Regex>` statics (the pattern already used by go/java/rust/python). Mechanical, low-risk, keeps every language on equal footing on the hot path. The 76-vs-115 split means this is a broad pattern, not just the three sampled languages.

#### F42 — vue `get_node_text_from_content` slices without a bounds guard while its sibling helper guards
- **Severity:** low
- **Area:** robustness consistency (vue embedded extraction)
- **Files:** `crates/julie-extractors/src/vue/identifiers.rs`, `crates/julie-extractors/src/vue/script_setup.rs`
- **Claim:** Two near-identical helpers in the same vue module disagree on safety. `vue/script_setup.rs:416-424` `get_node_text` guards with `if end <= content.len() { content[start..end] } else { String::new() }`. `vue/identifiers.rs:321-325` `get_node_text_from_content` does the unguarded `content[start_byte..end_byte].to_string()`. Both index a script-section content string with byte offsets from a tree parsed over that same string, so in normal operation offsets are in-bounds (tree-sitter invariant) and neither panics today; but the inconsistency is a latent foot-gun if the content/tree pairing ever drifts.
- **Evidence:** `vue/identifiers.rs:321-325` → `pub(super) fn get_node_text_from_content(node: &Node, content: &str) -> String { let start_byte = node.start_byte(); let end_byte = node.end_byte(); content[start_byte..end_byte].to_string() }` vs `vue/script_setup.rs:416-424` which checks `if end <= content.len()`. The tree is parsed from the same `section.content` passed to the walk (vue/identifiers.rs:44 passes `&section.content`; `parse_script_section` at :57-71 parses `&section.content`), so offsets are in-bounds in normal operation.
- **Impact:** No live crash (the tree is parsed from the same `section.content` that is passed in), but the unguarded slice would panic if a caller ever passed a mismatched content/node pair, and the divergence makes the safer pattern look optional. Note: neither helper checks char boundary — both rely on tree-sitter emitting valid UTF-8 boundary offsets; the only difference is the upper-bound length check.
- **Recommendation:** Make `get_node_text_from_content` delegate to the same bounds-checked path (or to base `get_node_text` at base/extractor.rs:207, which uses `from_utf8_lossy` + bounds check). One safe helper, not two with different safety contracts.

#### F43 — Release manifest ships only 2 of 3 architecture docs; release.md prose says "architecture docs" (plural, unqualified)
- **Severity:** low
- **Area:** release packaging / contract docs
- **Files:** `xtask/src/release.rs:117`, `xtask/src/release.rs:121`, `docs/release.md:53`, `docs/architecture/cli-contract.md`
- **Claim:** `release.md:53` states release packages contain "contract and architecture docs", but `release_package_items()` ships only `docs/architecture/product-boundary.md` (release.rs:117-119) and `schema-principles.md` (release.rs:121-123). `docs/architecture/cli-contract.md` exists on disk but is not in the manifest, so consumers unpacking a release package do not get the CLI architecture contract doc.
- **Evidence:** `release.rs:117-124` lists exactly product-boundary.md and schema-principles.md as the architecture Doc items. `ls docs/architecture/` shows three files: cli-contract.md, product-boundary.md, schema-principles.md. `release.md:53` → "contract and architecture docs". The contract docs (cli.md, sqlite-schema-v1.md, jsonl-v1.md, reports.md) ARE all shipped (release.rs:103-116), so CLI behavior documentation is not lost — only the architecture-tier CLI doc.
- **Impact:** Minor. CLI behavior is still documented via `docs/contracts/cli.md` (in the manifest). But the manifest is asserted as an exact, ordered contract (`release_contract.rs:74 release_package_list_is_exact_and_ordered`) and the prose claims all architecture docs, so this is documented-vs-actual drift.
- **Recommendation:** Either add `docs/architecture/cli-contract.md` to `release_package_items()` and update the `release_contract.rs` exact-list test, or tighten release.md:53 to "selected architecture docs (product boundary, schema principles)" so prose matches the curated manifest. Pick deliberately — the manifest is the API.

#### F44 — Python downstream consumer is documented under the Contract Tier but is not run by `cargo xtask test contract`
- **Severity:** low
- **Area:** test tier routing / doc drift
- **Files:** `docs/testing-strategy.md:94`, `docs/testing-strategy.md:98`, `xtask/src/test_tiers.rs:213`, `xtask/tests/python_example_contract.rs:54`
- **Claim:** `testing-strategy.md` Contract Tier lists "downstream smoke consumers" (line 79) and explicitly shows the Python SQLite consumer (lines 94-99), but `contract_plan()` (test_tiers.rs:213-278) only runs the Rust `downstream_smoke` test plus artifact/cli contract tests — it never invokes `examples/python/sqlite_consumer.py`. The Python consumer is exercised separately by `xtask/tests/python_example_contract.rs`.
- **Evidence:** `contract_plan()` (test_tiers.rs:213-278) contains no `python3` invocation; `grep python xtask/src/test_tiers.rs` returns exit 1. The Python consumer is tested in `python_example_contract.rs:54-60` via `Command::new("python3")`, which runs under `cargo test -p xtask` — a step that IS in ci.yml:28. So coverage is NOT lost, only the tier mapping in the doc is inaccurate. Secondary nuance: the xtask test fabricates its own SQLite fixture (python_example_contract.rs:77-115) rather than reading the dogfood artifact that testing-strategy.md:98 names, so the documented exact invocation is also not what runs.
- **Impact:** Low. CI does run the Python consumer (through the xtask test crate via ci.yml:28). The drift is purely that a reader following testing-strategy.md expects `cargo xtask test contract` to cover the Python reader and it does not.
- **Recommendation:** Either fold the `python_example_contract` assertions into the contract tier (run the Python consumer against the dogfood artifact), or clarify in testing-strategy.md that the Python downstream check runs via `cargo test -p xtask` rather than the contract tier. The honest record matters since this doc ships in the release package.

#### F45 — Dogfood CI gate runs against a debug binary, so its perf timings reflect an unoptimized build
- **Severity:** low
- **Area:** dogfood gate / release evidence
- **Files:** `.github/workflows/specialist-gates.yml:70`, `xtask/src/dogfood.rs:242`, `xtask/src/dogfood.rs:298`, `xtask/src/dogfood.rs:484`
- **Claim:** The dogfood specialist-gates job invokes `cargo xtask dogfood repo --root . --out-dir ...` with no `--binary` flag and no preceding release build. `dogfood.rs` falls through to `build_default_binary=true` and `default_binary_path()`, which targets `target/debug/julie-extract`, then runs a plain `cargo build` (no `--release`). So the dogfood gate measures and records timings from a debug binary, unlike `performance baseline` which mandates `--binary` against `target/release`.
- **Evidence:** `specialist-gates.yml:69-70` dogfood step has only `--root . --out-dir target/dogfood/julie-extractors`. `dogfood.rs:240-243` → `None => (default_binary_path(), true)`; `default_binary_path()` (484-489) joins `target/debug/<binary>`; the build at `dogfood.rs:297-299` runs `cargo build -p julie-extract-cli --bin julie-extract` with NO `--release` flag. The release-package job in the same file (specialist-gates.yml:83-91) DOES `cargo build --release` and pass `--binary target/release/julie-extract`, showing the asymmetry.
- **Impact:** Low. Dogfood timings are explicitly report-only, and the gate's hard evidence is correctness (no_change report, zero rows-written, schema versions), which is build-mode-independent. But `docs/release.md` frames dogfood metrics as release evidence; recording debug-build timings there is mildly misleading next to the release-binary performance baseline.
- **Recommendation:** In the dogfood CI job add a `cargo build --release -p julie-extract-cli` step and pass `--binary target/release/julie-extract`, matching the performance baseline command, so recorded dogfood timings are representative. Correctness validation is unaffected either way.

---

## 5. What's Solid (Verified Positives)

Each item below was opened and verified, not assumed.

**SQLite writer & schema**
- **Schema/columns/FKs/indexes match v1 exactly, guarded by an executable drift test.** 18 documented tables, correct FK ON DELETE behaviors, all 17 required indexes; `schema_contract.rs` asserts column drift and forbids old-Julie internal tables, and runs in CI's contract tier. (`schema.rs:10`, `schema_contract.rs:241/526`)
- **Writer transaction model is correct.** One explicit transaction + one commit per scan/update/delete, prepared statements for repeated inserts, replace-by-file_id in dependency order, content-hash skip before churn, full rollback on mid-batch error (proven by `failed_mid_batch_rolls_back_prior_file_writes`). Ran writer_contract: 14/14 pass. (`writer.rs:335/754/856`, `writer_contract.rs:119`)
- **FK enforcement is active on both in-memory AND file-backed WAL connections.** `PRAGMA foreign_keys = ON` persists past the `journal_mode=WAL` pragma; positively verified with a throwaway file-backed test (dangling file_id insert rejected). relationships carry triple-CASCADE. (`schema.rs:11`, `writer.rs:67`, `schema_contract.rs:131`)
- **SQLite writes + JSONL export are atomic at the file level.** One-transaction-rollback for SQLite; JSONL export writes a `.tmp` then `fs::rename`, removing the temp on any error so a failed export never leaves a complete output file. (`writer.rs:335`, `jsonl.rs:140`)
- **FailedPreserved and Unsupported writer paths are defensively complete but unreachable from the CLI** (informational; the writer is robust, the CLI just never produces those statuses — ties to F11). (`extraction.rs:105/242`, `commands.rs:2002`, `writer.rs:737`)

**JSONL export & JSON reports**
- **`rows_written`/`totals` exhaustive over all 18 row domains, locked by a set-equality test.** Every key always serializes (no serde skip), defaults to 0. (`reports.rs:161`, `report_contract.rs:114`)
- **JSONL envelope, record order, and all 18 payload kinds conform to jsonl-v1 with strong drift tests.** Exact 7-field envelope with `op='snapshot'`, 18 functions in contract order, JSON text columns decoded to real JSON values, per-kind BTreeSet key equality. (`jsonl.rs:1288/117`, `jsonl_contract.rs:87`)
- **Export is correctly buffered (64KB BufWriter, single flush) and atomic-on-failure for the file path, proven by a bounded-write-call test.** (`jsonl.rs:112/140`, `jsonl_contract.rs:355`)

**CLI surface**
- **Path policy is correct and well-aligned to the contract.** Canonicalize at boundary, typed outside-root/not-found errors, update requires existence, delete tolerates missing, stored paths root-relative Unix — all pinned by real-binary tests. (`paths.rs:84/90/111`, `path_policy.rs:82`)
- **Exit-code and report-stream mapping is correct and consistent.** 0 for ok/no_change/unsupported/not_found, 1 for op failures, 2 for usage (clap), 3 for version/contract incompatibility; export success splits JSONL→stdout, report→stderr. (`commands.rs:1234/654`, `cli_contract.rs:132`)

**Extractor engine core**
- **`types` HashMap is explicitly sorted before emit; `stable_location_id` is content-deterministic** — the obvious determinism trap is closed (and the right pattern F21 is missing). (`extraction.rs:203/204`, `types.rs:230`)
- **Embedded-language offset remapping (HTML `<script>`) is correct and panic-safe** — `get(..byte_offset)?`, line-1-only column delta, id refresh + parent remap. (`embedded_span.rs:11/26`, `html/scripts.rs:235/249`)
- **Node-text and byte-slice paths are panic-safe against malformed / non-UTF-8 input** — `from_utf8_lossy` + bounds checks, `content.get(range)?`, ASCII-anchored slicing, find/rfind guards; non-UTF-8 rejected at read time. (`extractor.rs:207`, `body.rs:41`, `string_literals.rs:24`, `annotations.rs:148`)
- **High-traffic languages share an isomorphic, careful identifier-extraction shape with consistent kinds.** rust/ts/python/csharp all walk the same way and emit `{Call, MemberAccess, TypeUsage}`; Rust's call/member-access boundary handling is notably careful (terminal-node span, call-function dedup skip). (`rust/identifiers.rs`, `typescript/identifiers.rs`, `python/identifiers.rs`, `csharp/identifiers.rs`)

**Error-handling / panic sweep**
- **All 5 `panic!` sites and the dangerous unwrap/expect set are unreachable on untrusted input** — the from_string panics are test-only; the artifact expect serializes in-code values; capability_snapshot parses embedded JSON. (`language_policy.rs:131`, `registry.rs:889`, `base/kinds.rs:70`, `writer.rs:651`)
- **Sampled per-language unwrap/expect/byte-slice sites are guarded or rely on tree-sitter invariants** — `is_none()`/length guards before unwraps, char-boundary checks before relative slices. This is a SAMPLE only and does not discharge the full ~169-unwrap audit. (`extractor.rs`, `dart/relationships.rs`, `go/relationships.rs`, `toml/mod.rs`, `regex/identifiers.rs`)
- **Base span computation and embedded-span remapping conform exactly to the position contract** — 1-based lines, 0-based byte columns, raw byte offsets; no char-count columns anywhere (verified by grep). Caveat: a few non-tree-sitter extractors (css/markdown/vue-manual) set columns directly but still byte-based. (`base/span.rs`, `base/embedded_span.rs`)

**Cross-language capability & test signals**
- **Single authoritative language source (LANGUAGE_SPECS, 36 entries) with compile-time + test-enforced no-drift invariant.** registry panics at startup if a spec lacks an extractor; `capability_matrix_matches_registry_entries` asserts capabilities.json equals language_spec one-for-one. Caveat: the strongest equality assertion is feature-gated to the contract tier, not default. (`specs.rs:3`, `mod.rs:419`, `registry.rs:878`, `capability_matrix.rs:160`)
- **Shared `test_calls` core isolates the JS-only dotted classifier from all other languages.** 12 per-language adapters delegate to one builder; the false-positive footgun (`classify_call`) is used only by JS/TS. (`test_calls.rs:50/71/164`, `dart/test_calls.rs:57`)
- **Capability claims are evidence-backed by golden fixtures, not hardcoded counts.** Boolean flags + kind_coverage; matrix test asserts each language has an on-disk fixture, relationship-advertising languages exercise relationships, and every claimed kind is actually emitted. (`capabilities.json:17`, `capability_matrix.rs:246/280/1199`)
- **`route` literal-carrier capability is wired end-to-end but empty for all 26 config TOMLs (reserved, equal across languages — not inequality).** (`language_policy.rs:118`, `type_models.rs:126`, `python.toml:29`)

**Test discipline**
- **Artifact and CLI contract tests assert real contract shape, not just non-error** — exact column lists, forbidden-table loop, full record-kind set in order, forbidden-behavior-term check in help. The strongest part of the suite. (`schema_contract.rs:32`, `jsonl_contract.rs:24`, `cli_contract.rs:53`)
- **Tier gating is enforced end-to-end and currently leak-free** — xtask routing test + crate-level gating convention test; the only real_world/file_integration modules are gated and checked. (`xtask/tests/test_tiers.rs:16`, `tests/test_tiers.rs:5`, `tests/mod.rs:11`)
- **xtask test tier routing matches testing-strategy.md, guarded by unit + convention tests.** (`xtask/test_tiers.rs:100`, `xtask/tests/test_tiers.rs:42/274`, `tests/test_tiers.rs:4`)

**Build, CI, release & xtask**
- **Dogfood rescan validation rigorously asserts the no_change contract** — schema version, status/operation/mode, empty errors, null created_revision_id (three-way null/present/missing match), every rows_written==0 (fails closed if the object is empty), correct file deltas. Model for how a gate should validate. (`dogfood.rs:709/762/783/797`)
- **Tree-sitter parser dependencies are cleanly pinned** — single core (0.26.9), no duplicate grammars, git grammars by immutable rev; changed-path tier triggers full certification on Cargo.lock change. (`extractors/Cargo.toml:25/49`, `Cargo.lock`)
- **Release staging is path-traversal guarded, checksummed deterministically (real SHA-256), and refuses dirty output dirs.** (`release.rs:371/288/387/456`)

**Untrusted-content storage (completeness pass)**
- **All SQLite writes use bound parameters; no dynamic SQL value interpolation.** The only `format!`-built SQL interpolates a run of `?N` bind markers, not values — so arbitrary source text cannot inject SQL or corrupt rows. (`writer.rs:859/1176/1293`)
- **Cross-platform path normalization and the "file becomes unsupported on update" edge are both handled correctly.** `root_relative_unix` builds from `Component::Normal` and joins with `/` (no Windows backslash bug); `cleanup_unsupported_update` deletes stale rows and returns `Unsupported`/exit 0, not orphaned rows. Nested-gitignore patterns anchor to their own dir. (`paths.rs`, `commands.rs:373/841`)

---

## 6. Coverage & Gaps (what was NOT deeply verified)

These areas were noted but not fully driven to a conclusion. Treat them as open verification work, not as findings.

- **Concurrent-writer behavior under contention** was only partially examined. Read paths open `SQLITE_OPEN_READ_ONLY` (commands.rs:1086, correct), but combined with the missing `busy_timeout` (F4), a reader (export/info) opening while a scan holds the WAL write lock can get an immediate SQLITE_BUSY rather than waiting. No test simulates a second process holding the DB. Extends F4 to the read side; not separately filed.
- **WAL checkpoint / `.db-wal`+`.db-shm` sidecar lifecycle** was not reviewed: whether the artifact ships as a single file after a scan (auto-checkpoint on close) or leaves `-wal`/`-shm` that a downstream non-Rust consumer copying only the `.db` would read as stale. Worth a dedicated check of writer connection close / `PRAGMA wal_checkpoint` before release. Unverified.
- **Extremely long single lines / pathological-but-valid source** (a 100MB minified non-`.min.js`, deeply nested JSON) was not stress-tested for memory blow-up. `fs::read_to_string` loads the whole file with no cap (extraction.rs:56) and tree-sitter builds a full CST; there is no max-file-size guard. F36 covers the >4GB overflow but not the OOM/latency surface for large-but-sub-4GB files.
- **Duplicate root-relative paths on case-insensitive filesystems** (macOS/Windows) or unicode-normalization-distinct names (NFC vs NFD) were not examined: `Foo.rs` and `foo.rs`, or NFC/NFD-distinct names, both map through `stable_id("file", [path])` and could collide or produce two `file_id`s the writer treats as distinct. Unverified whether the writer's path-keyed delete handles this.
- **JSONL export reader path under concurrent scan rewrite** (read_only connection + no busy_timeout) was reviewed for shape/buffering but not for behavior while a scan rewrites the artifact in another process. Interaction not tested.
- **`.julieignore` vs `.gitignore` precedence and negation (`!` re-include) semantics** are not exercised by any test; `build_ignore_matcher` loads both plus nested `.gitignore` files, but no test asserts a `!pattern` re-include or a `.julieignore` override behaves as documented. Coverage gap only (the nested-pattern anchoring was verified correct).
- **The ~90 `len()>0` + ~159 `!is_empty()` aggregate weak-assertion counts** (F38 context) were not independently re-derived; only the single concrete smoke-only test (zig) is load-bearing in F38.

---

## 7. Appendix — Corrected / Refuted Claims

No top-level findings were refuted outright. However, verifiers materially **narrowed or corrected** many initial claims; surfacing them documents what was checked and found NOT to be an issue (or was scoped down):

- **F1 file cite corrected.** The blast-radius path is `write_scan_snapshot` (writer.rs:359), not the `write_files`/update loop (writer.rs:257). Both have the guard, but scan is the repo-wide path; the cite was moved accordingly.
- **F2 mapping cite corrected.** The original placed the Err→`parse_failed` mapping at extraction.rs:78-84 (which is the `.map_err` constructing the error). The actual mapping is at commands.rs:821-825, and crucially that Err arm is NOT reached for tree-sitter parse failures (they return `Ok`). The bug stands; the cite was fixed.
- **F11 enum name corrected.** The schema enum is `FileStatus::FailedPreserved`, not `FileStatus::Failed` as initially written. Positively verified (grep) that NO CLI path sets it — which strengthened the finding from "loop doesn't exercise it" to "producer never emits the status at all" and upgraded severity medium→high.
- **F21 claim partially refuted/narrowed.** The initial claim that `containing_symbol_id` is "folded into identifier IDs on the JSONL rekey path" is FALSE: identifier id = `generate_id_for_span(name, span)` (creation_methods.rs:99), and JSONL `record_id` is just `identifier_id` — no composite. So `identifier_id`/`record_id` stay stable; only the `containing_symbol_id` field drifts. Map call-site count corrected to 28 (grep) vs the claimed 58. Severity kept medium.
- **F22 contrast set narrowed.** The original listed Dart/GDScript/Lua/R/Kotlin too; only swift/java-kotlin/php/python were explicitly verified to add a name guard, so the finding cites those.
- **F24 scope corrected.** Reframed from "3 of 34" to "3 of ~38 language modules" (38 src language dirs) and added go/java to the explicit no-VariableRef list after verifying them.
- **F26 verified leak-free TODAY.** Confirmed the only `real_world`/`file_integration_bug` modules (qml, r) are all gated and all checked by the convention test; the `markdown/mod.rs:918 test_yaml_frontmatter_real_world_blog_post` hit is a function name using inline data, not an ungated fixture — NOT a current leak. The finding is a future-regression gap, not a present bug.
- **F40 narrowed (risk → already-covered).** The original assumed the embedded-config parse happens lazily with no guarding test. Verified that `tests/mod.rs:36` (language_policy, ungated) and `tests/mod.rs:13` (capability_snapshot_test, ungated) both trigger the parse in the default suite, so a malformed edit already fails default CI. Severity kept low; impact/recommendation corrected.
- **`route` capability — NOT cross-language inequality.** Initially a candidate inequality; verified all 26 config TOMLs carry an empty `route = []` (equal across languages) and the variant is documented as reserved/annotation-sourced. Correctly recorded as info, not a bug.
- **Positive corrections.** Several positives were verified MORE complete than first claimed: the embedded-span column delta is applied to both start AND end endpoints (not just start); `stable_location_id` returns the md5 hex of the format string (still content-stable); the dogfood validation also checks `report_schema_version==1`; the JSONL atomic-cleanup guarantee covers the file path only, NOT `--out -` stdout (which is F10). The span-foundation positive was downgraded from "every span flows through NormalizedSpan" to "dominant path" after finding css/markdown/vue-manual extractors set columns directly (none with char counts, so still contract-consistent).
- **F25 / F26 / F44 / F45 — coverage/doc-drift, not lost coverage.** In each case the underlying behavior is actually exercised or correct; the finding is that the doc/guardrail asserts more than is enforced (no suite budget; hardcoded leak list; Python consumer runs via `cargo test -p xtask` not the contract tier; dogfood timings are debug-build). These are honest-record fixes, not functional regressions.
