# Architecture and Performance Audit Findings

Date: 2026-09-04
Commit: `536b22d9` on `main`. The source baseline was clean; this audit and
Goldfish memory files were untracked during validation.
Status: in remediation. Wave 1 closed: E1, E2, E3 (identifier lookup), C1, C2, A2, C4. Other findings remain open until their fixes land and pass their
own verification.
Mode: read-only source audit plus focused test and timing checks. No product
code changed. Four reviewers swept one area each; the lead reconciled every
finding against the source and current repository state.

Areas: `julie-extract-artifact`, `julie-extract-cli`, `julie-extractors` core, tests and build tooling.

## Validation log

Two independent review passes checked every finding against this tree on
2026-09-04. Original claims stay for traceability. Each item has a Validation
note with a final verdict:

- CONFIRMED: the claim matches the source.
- PARTIALLY CONFIRMED: the waste is real, but a count, path, or scope is off.
- REFUTED: the claim is wrong. Do not act on it as written.

The verdicts validate control flow, work counts, API use, and repository state.
Performance priority labels are static hypotheses unless a measurement is
named. Any optimization still needs the same-workload before and after numbers.

Largest corrections: T1 is refuted because CI already runs golden and
capability through `cargo xtask test contract`; T6 is only partial because one
merged worktree contains untracked files; C6 duplicates nine shared modules,
not every CLI module.

Line and repository counts come from `536b22d9`. The default tier was measured
with `hyperfine --warmup 1 --runs 3 'cargo xtask test default'`.

## Summary

The product boundary, durability design, and test tiering are sound. The main
problems are:

- Per-file work that repeats per-run work (capability snapshot, discovery,
  language detection, spool round trips).
- Per-symbol work whose output nobody reads (`code_context`, `symbol_map` clone).
- O(symbols) scans per identifier in about 30 languages, where Rust already
  has an index that should become the shared one.
- Two hand-maintained copies of the SQLite schema and row binders.
- Gates or checks promised by the agent docs but absent from automation
  (data-quality report and wall-clock budget tracking).

**Validation.** CONFIRMED after correcting the last bullet. CI runs golden and
capability inside the contract tier. The data-quality report and wall-clock
budget tracking are still missing.

## Top recommendation

Fix the hot-path waste in the extractors crate first (findings E1 to E4).
It touches all 40 languages, it is measurable, and each fix is local.
Second, add the targeted data-quality automation in T2 and reconcile the
wall-clock policy in T3.

**Validation.** PARTIALLY CONFIRMED. E1 to E3 are local. E4 is a planned
structural change and should not share their implementation slice. T1 needs no
work; the contract job already runs those tiers.

## Extractors crate (`crates/julie-extractors`)

### E1. `code_context` is built for every symbol and never persisted (high)

- `base/creation_methods.rs:43` formats about seven lines of source with line
  prefixes into a fresh `String` for every `create_symbol`. For a large class
  symbol that is the whole class body.
- No symbol column stores it. The only `code_context` column is on
  `identifiers`, and the CLI writes `None` there (`julie-extract-cli/src/extraction.rs:566`).
  JSONL emits `Null` (`julie-extract-artifact/src/jsonl.rs:915`).
- Direction: delete `code_context`, `ContextConfig`, and `extract_code_context`
  from the symbol hot path.

**Validation.** CONFIRMED.
- `create_symbol_from_span` still builds it, then clones it into `symbol_map`.
- The window is 3 lines before and after the whole symbol span, not a fixed
  7 lines. A large class gets the whole class body plus those lines.
- `Symbol.code_context` exists only in memory. `ArtifactSymbol` has no such
  field. The identifiers table owns the SQLite column. The CLI stores `None`.
  JSONL writes `Value::Null`. A golden test asserts no identifier context.
- Extra builders: `markdown/semantic_symbols.rs`, `go/functions.rs`,
  `vue/test_calls.rs`. Tests in `src/tests/base.rs` cover `ContextConfig`.
- `line_ranges` on `BaseExtractor` exists only for this path. Delete it with
  the context helper.
- Keep the identifiers column unless you bump the contract.

**Status: CLOSED.** Fixed in commit `840ef079` (Audit Wave 1, Task 1). Deleted `code_context`, `ContextConfig`, `extract_code_context`, and `line_ranges` from `BaseExtractor` and language extractors.

### E2. Every symbol is cloned into `symbol_map`; six languages read it (high)

- `base/creation_methods.rs:81` clones name, signature, doc comment, metadata
  map, annotations, and the context string from E1.
- Readers: cpp, ruby, elixir, erlang, go, php. The other ~30 languages pay for
  nothing.
- Direction: drop the map from `BaseExtractor`. The six readers build their own
  index from the returned `Vec<Symbol>`.

**Validation.** PARTIALLY CONFIRMED. "Six readers" overstates use.
- The insert is `self.symbol_map.insert(id, symbol.clone())`.
- Languages that **read** the map: cpp, ruby, php. Erlang reads it to rewrite
  a body hash.
- Elixir only `clear()`s. Go and C# only insert extra symbols. C# was missed.
- Many other languages build a local `HashMap` from `&[Symbol]`. Those are
  not this map.
- Drop the map from `BaseExtractor`. Keep local maps in cpp/ruby/php/erlang.
  Do not delete ruby's assignment path without a replacement.

**Status: CLOSED.** Fixed in commit `37e10b12` (Audit Wave 1, Task 2). Deleted `BaseExtractor::symbol_map` and eliminated symbol cloning on creation; languages that require symbol lookup now construct local maps.

### E3. Containing-symbol lookup is O(symbols) plus a sort, per identifier (high)

- `base/creation_methods.rs:267-329` filters all symbols, collects a `Vec`,
  sorts with a three-key comparator, and takes element 0. 63 call sites.
- Rust already has the right shape: `rust/identifiers/containing_symbols.rs`
  builds one sorted index and scans with early exit. TypeScript has a third
  variant that also clones the whole `Symbol` per call expression
  (`typescript/relationships.rs:65`).
- Direction: promote the Rust index into `base`, return `&Symbol`, delete the
  other two variants and the 33 per-language five-line wrappers.

**Validation.** PARTIALLY CONFIRMED.
- `find_containing_symbol_from_iter` still filters, collects, sorts, and takes
  `[0]`. The public wrapper is at line 236.
- `ContainingSymbolIndex` exists in `rust/identifiers/containing_symbols.rs`.
- TypeScript clones at `relationships.rs:65` via `.cloned()` on the containing
  callable. JavaScript copies the same pattern at `relationships.rs:63`.
- Wrapper defs named `find_containing_symbol_id`: 30 files, not 33.
- The "63 call sites" figure was not reproduced. Count real calls before
  planning.
- Safer first slice: identifier lookup only. Leave
  `attach_containing_symbols` for the E4 plan.
- The algorithmic cost is source-confirmed. Its share of extraction time has
  not been measured.

**Status: PARTIALLY CLOSED (identifier lookup closed).** Fixed in commit `faf20c51` (Audit Wave 1, Task 3). Promoted `ContainingSymbolIndex` to `base/containing_symbol_index.rs` and replaced O(symbols) filter-and-sort scans per identifier with binary search on the pre-sorted span index. Note: Single tree-walk and structural fact binder consolidation are deferred to Wave 5 / E4.

### E4. Ten or more full-tree walks per file, each with a linear binder (high)

- `registry.rs:786-880` runs nine fact-family collectors plus complexity after
  the language extractor. Each starts at the root and walks the whole tree.
  Each then calls `attach_containing_symbols` (linear scan per fact) and sorts.
  `registry.rs:867` sorts the merged result again.
- Language extractors add their own: typescript 10 root walks, javascript 9,
  vue 9, sql 7.
- Direction: one walk per file dispatching to per-node-kind emitters, and one
  shared containing-symbol index reused by every family. This is the largest
  change in the list and should have a plan.

**Validation.** PARTIALLY CONFIRMED.
- After the language extractor, `extract_for_language_at` calls source regions,
  structural facts, rust doc-test facts, framework, marker, web, code, data,
  sql, then complexity. That is nine collectors plus complexity.
- Several collectors return at once when the language has no patterns
  (`collect_structural_facts` is one). Marker walks source regions, not the
  tree. "Ten or more full-tree walks" is a peak, not the typical file.
- `attach_containing_symbols` exists in `source_regions.rs` and
  `containing_symbol.rs`. The extra sort of the merged list is real.
- Language-extractor `root_node()` walks proved: TypeScript 8, JavaScript 7.
  The cited 10 and 9 are high.
- Direction still needs a plan. Do not fold it into E1-E3. Promote the
  containing-symbol index first, then merge walks.

### E5. Vue re-parses the same script section up to eight times (high)

- `parse_script_section` is copy-pasted in `vue/script.rs`,
  `vue/script_setup.rs`, `vue/identifiers.rs` (two parsers), `vue/relationships.rs`,
  `vue/test_calls.rs`, `base/complexity_metrics.rs:1335`, and
  `base/web_structural_facts/vue.rs:110`. `vue/mod.rs:58` also clones the whole
  file content to pass a `&str`.
- Direction: parse each section once and pass the `Tree`.

**Validation.** PARTIALLY CONFIRMED. Repeated script parses are real, but eight
is not a proven upper bound and no runtime share was measured.
- Five `fn parse_script_section` copies exist: script, script_setup,
  identifiers, relationships, test_calls. `identifiers.rs` is one helper,
  not two parsers.
- `script_setup.rs` parses twice (symbols then annotations).
- `complexity_metrics.rs:1335` is `parse_vue_script_tree`. It parses the
  section once at file level, then again per callable symbol.
- `web_structural_facts/vue.rs:110` parses the style section as CSS. That is
  not the script section.
- `vue/mod.rs:58` still clones `self.base.content` into `parse_vue_sfc`.
  `parse_vue_sfc` also reruns from identifiers, relationships, and complexity.
- One script-setup SFC can hit about 8 parses. Complexity can push that
  higher. Cache both the SFC split and each section `Tree`.

### E6. Three `canonicalize` syscalls per file to build a relative path (medium)

- `base/span.rs:149-179` canonicalizes, then `utils/paths.rs:13` canonicalizes
  file and root again. The CLI already passes `root_relative_path`. On Windows
  this is also where the verbatim-prefix trap lives.
- Direction: accept a normalized relative path from the caller.

**Validation.** CONFIRMED.
- `span.rs` canonicalizes the file. `utils/paths.rs` canonicalizes file and
  root. That is three syscalls when both paths succeed.
- Keep the Windows verbatim-prefix warning when this is touched.

### E7. `.h` files parse three times (medium)

- `language_spec/mod.rs:345-372` parses with C and C++ to count errors, then
  `pipeline.rs:53` parses again with the winner.
- Direction: return the winning tree from detection.

**Validation.** CONFIRMED for non-empty `.h` files.
- `header_parse_prefers_cpp` parses C and C++. The winner tree is dropped.
  The winner parse is `pipeline.rs:49`, not line 53.
- Empty or whitespace-only `.h` files skip the C/C++ probe.
- Scan already detects in `commands.rs` and again in `extraction.rs` (C4).
  A non-empty `.h` file on that path can trial-parse more than three times.

### E8. `body_hash` tokenizes nested bodies once per nesting level through `Vec<char>` (medium)

- `base/body.rs:43-88`. A method inside a class inside a namespace is
  tokenized three times, each time through `Vec<char>` and `Vec<String>`.
  `body_hash` is persisted, so the hash stays. MD5 is used for every id; a
  change is a contract bump.
- Direction: stream bytes through the hasher with a whitespace-skipping
  iterator.

**Validation.** CONFIRMED.
- `normalized_body_tokens` still does `source.chars().collect()` into
  `Vec<char>`, then builds `Vec<String>`.
- Each nested symbol hashes its own span, so nested bodies are retokenized.
- `body_hash` and `generate_id` both use MD5. Changing either is a contract
  bump.

### E9. Orchestration layers that fail the deletion test (high confidence, low risk)

- `manager.rs`: wraps `pipeline::extract_canonical` and re-detects language
  for a debug log. No external caller.
- `routing_symbols.rs`, `routing_identifiers.rs`, `routing_relationships.rs`:
  eight lines each, return one field, used only by `manager.rs`.
- `factory.rs`: the factory function is `#[cfg(test)]`.
- `language.rs:17-174`: four `get_*_node_kinds` functions with zero callers.
- `registry.rs:132-560`: five near-identical macros plus ~12 hand-written
  `extract_*` functions. One trait with default methods replaces ~500 lines.
- Direction: delete manager, routing, factory, and the dead functions.

**Validation.** PARTIALLY CONFIRMED. Do not delete `factory.rs` wholesale.
- `ExtractorManager` has no production caller. Tests
  (`api_surface.rs`, `jsonl_pipeline.rs`) and the crate README still use it.
  Point those tests at `extract_canonical` first.
- Routing modules are public and exist only for the manager.
- `factory.rs` is mixed. `extract_symbols_and_relationships` is `#[cfg(test)]`.
  `convert_types_map` is production and is imported by `registry.rs`.
- Four dead helpers in `language.rs`: `get_function_node_kinds`,
  `get_import_node_kinds`, `get_symbol_node_kinds`, `get_symbol_name_field`.
  No callers.
- Registry macros: 4, not 5. Hand-written `extract_*`: 10 (lua, r, html, sql,
  toml, erlang, json, xml, qmldir, vue), not ~12.
- Move `convert_types_map` next to the registry. Delete manager, routing, and
  the dead language helpers. Leave the macros for a later plan.

### E10. Helpers duplicated per language that base already has (high)

- Doc comments: `go/helpers.rs:346` and `:218` are verbatim copies of
  `base/extractor.rs:450` and `:340`. `rust/helpers.rs:289` is a third version.
- Visibility: ~15 language-local `determine_visibility` variants map the same
  keyword set.
- `sql/mod.rs:102` rebuilds a symbol `HashMap` per string-literal node.
- Direction: make the base helpers `pub(crate)` and delete the copies.

**Validation.** PARTIALLY CONFIRMED.
- Go `preceding_comment_texts` matches `previous_comment_texts`. Go
  `select_go_doc_comment_block` matches `select_doc_comment_block`.
- `rust/helpers.rs:289` is a different algorithm (walk siblings, skip
  attributes). It is not a third copy of the Go/base helper.
- `fn determine_visibility` exists in 8 language files, not ~15: csharp,
  java, kotlin, php, razor, scala, swift, vbnet. Other names exist
  (`extract_visibility`, Go `is_public`). Do not force Go's uppercase rule
  into the shared helper.
- SQL still builds `HashMap<String, &Symbol>` inside the string-literal node
  arm. That is per node, not per file. Pass one map or the E3 index.

### E11. Base dispatches on language name strings in 12+ places (medium)

- `match language` blocks in `structural_facts.rs:257`,
  `code_structural_facts.rs:694`, `data_structural_facts.rs:131`,
  `framework_structural_facts/mod.rs:202-319`, `web_structural_facts/mod.rs:145`,
  `source_regions.rs:549`, `complexity_metrics.rs:595`, `registry.rs:869`,
  and string checks in `extractor.rs:324`, `creation_methods.rs:49`,
  `body.rs:98-150`, `annotations.rs:42-58`.
- Each new language touches six to eight match statements. `base/` is 43k
  lines, larger than all 40 language modules together.
- Direction: put the collector list on `LanguageSpec`, which is already the
  data table for everything else.

**Validation.** PARTIALLY CONFIRMED. The size comparison is wrong.
- `base/` is 42,548 lines. The 38 language modules are about 99,983 lines.
  Base is large. It is not larger than the language modules.
- `capabilities.json` lists 40 languages (JSX and TSX are aliases). There are
  38 language directories. `factory.rs` asserts `supported_languages().len() == 40`.
- Language-name `match language` in `base/` is at least 15 sites. "12+" is low.
- `LanguageSpec` already holds name, aliases, extensions, parser, and
  doc-comment style. It does not hold collector lists yet. Put collectors
  there. Do not add a second registry.

### E12. Public API wider than its two consumers (medium)

- Exported but unused outside the crate: `ExtractorManager`, `BaseExtractor`
  and its mutators, `pub mod base`, all 38 language modules,
  `get_tree_sitter_language`, `LanguageRegistryEntry`, and the legacy
  `pending_relationships` twin of the structured payload.
- Direction: `pub(crate)` for language modules and base; export the entry
  function, result types, detection, and capability snapshot.

**Validation.** CONFIRMED, with one correction.
- `lib.rs` still has `pub mod base`, `pub mod manager`, `pub mod` for 38
  languages, and `pub use manager::ExtractorManager`.
- CLI does not import `ExtractorManager`, language modules, or
  `LanguageRegistryEntry`.
- `pub mod base` is not unused. CLI imports `base::{NormalizedSpan,
  StructuredPendingRelationship}` because those types are not re-exported at
  crate root. Re-export them before shrinking `pub mod base`.
- Keep `pending_relationships` until you bump the extraction contract.
- "Unused outside the crate" is proven only for consumers in this workspace.
  Unknown downstream Rust callers may use the published public API.

## CLI crate (`crates/julie-extract-cli`)

### C1. Store import writes and re-reads a spool file per source file under a global mutex (high)

- `store/executor.rs:36` declares `IMPORT_SPOOL_IO: Mutex<()>`.
  `executor.rs:598-648` runs inside the rayon pool but, per file: lock, create
  a spool file, push one record, flush, reopen, decode, convert. Workers
  serialize on the lock. `StoreFileVersion::try_from_artifact_file` already
  takes the in-memory `ArtifactFile`.
- Direction: call the conversion directly. Delete the mutex and spool detour.

**Validation.** CONFIRMED.
- `extract` still locks `IMPORT_SPOOL_IO`, creates a spool, pushes one file,
  finishes, reopens, and then calls `try_from_artifact_file`.
- Parse and extract run before the lock. Only the spool round trip is
  serialized.
- `try_from_artifact_file` takes `&ArtifactFile`. The spool is not required
  for conversion.
- `from_artifact` already converts in memory. The mutex is this extract path
  only (import and update share it).

**Status: CLOSED.** Fixed in commit `54e30a0b` (Audit Wave 1, Task 4). Removed `IMPORT_SPOOL_IO` global mutex and the per-file spool write/re-read detour from store import and update paths, converting directly in memory via `StoreFileVersion::try_from_artifact_file`.

### C2. Capability snapshot rebuilt per file in the store write loop (high)

- `executor.rs:1723` and `executor.rs:1126` call `artifact_capability_snapshot()`
  per file. It re-parses the embedded `Cargo.lock` text and rebuilds every
  language row each call (`capability_snapshot.rs:24-26`). See A2 for what the
  writer then does with it.
- Direction: build once per request and pass a reference. Pass `None` after
  the first sync.

**Validation.** CONFIRMED, and the from-artifact path is worse.
- `artifact_capability_snapshot` still parses `include_str!("../../../Cargo.lock")`
  on every call. The inner extractor snapshot is cached in a `OnceLock`. The
  per-file cost is the lock parse plus a clone of every language row.
- Import L1 at `executor.rs:1723` builds one snapshot per file.
- From-artifact uses `(level == StoreLevel::L1).then_some(&artifact_capability_snapshot())`.
  `then_some` always builds the value, so L2 and L3 pay for it too.
- Scan builds it once per run (`commands.rs:464`). That is fine.

**Status: CLOSED.** Fixed in commit `154fad78` (Audit Wave 1, Task 5). Built capability snapshot once per quantum/chunk in store import, update, and from-artifact paths, passing `Some(&snapshot)` only for the first L1 file and `None` thereafter.

### C3. Discovery decisions re-run for every file (high)

- `commands.rs:1552-1556` calls `discovery.select_file` again for every file
  the walk already classified. That repeats hard-exclude matching, the full
  per-component gitignore walk, and a `metadata()` syscall, serially before
  the pool starts.
- Direction: carry the language on the discovered target.

**Validation.** CONFIRMED.
- `spool_discovered_files` already receives `targets` and
  `unsupported_targets`. It still calls `select_file` on every target.

### C4. Language detected twice per file in scan (medium)

- `commands.rs:1893` detects, then `extraction.rs:177` detects again.
- Direction: trust the language passed in.

**Validation.** CONFIRMED. The scan path detects three times, not two.
- Discovery uses extension only (`language_for_path` with empty content).
- The worker at `commands.rs:1882` then runs `detect_language_for_source`.
- `extraction.rs:177` runs it again. The passed language is only the fallback.
- Store extract also detects twice: `executor.rs:605` then `extraction.rs:177`.

**Status: CLOSED.** Fixed in commit `4a2ba80a` (Audit Wave 1, Task 6). Reused the language detected in the scan worker or store executor directly in `extract_artifact_file_from_snapshot_at`, removing redundant internal `detect_language_for_source` calls.

### C5. Store import reads and hashes every file twice, and extracts twice at `--level full` (medium)

- `store/import.rs:159-176` reads and hashes to plan; `executor.rs:604` reads
  and hashes again. Full level then extracts L1 then L3 per file
  (`executor.rs:1528-1540, 1660-1666`).
- Direction: plan from size and mtime; hash once at extraction.

**Validation.** CONFIRMED. L1 is stored, not thrown away.
- Plan path reads the file in `read_source_identity_or_missing`. Extract reads
  it again in `read_source_snapshot` (`executor.rs:597`).
- Full import builds L1 chunks, then L3 chunks. L1 extracts at
  `ExtractionLevel::Symbols` and writes those rows. L3 extracts at
  `ExtractionLevel::Full` and writes again. The waste is a second extract,
  not a discarded L1.

### C6. Nine shared CLI modules compile separately as bin and lib (high, quality)

- `src/lib.rs:11-30` declares nine private modules with `#[allow(dead_code)]`;
  `src/main.rs:1-13` declares them again. `commands.rs` uses the bin copies,
  `store/` uses the lib copies. `FileTarget` exists as two distinct types, and
  twelve `allow(dead_code)` markers hide real dead code.
- Direction: `main.rs` becomes a shim over the lib. Move `commands.rs` and
  `args.rs` into the lib. Remove every `allow(dead_code)`.

**Validation.** PARTIALLY CONFIRMED. The duplication is real, but not every
module is compiled twice.
- `lib.rs` has nine `#[allow(dead_code)]` modules. `watchdog.rs` and
  `artifact_access.rs` add three more. Total: 12.
- `FileTarget` has one struct in `paths.rs`. Bin and lib each compile that
  file, so the types are distinct. They do not meet at a call boundary today.
- `store/` lives only in the lib. `commands.rs` lives only in the bin.

### C7. `commands.rs` is a god module (medium)

- 2,486 non-test lines. `scan` is 480 lines with the same error-return block
  nine times. The parallel pipeline (lines 1323-2130) belongs in `extraction.rs`.

**Validation.** CONFIRMED as a structure problem.
- The file has 3,388 lines. `#[cfg(test)] mod tests` starts at 2486, which is
  2,485 lines before the test module, not 2,486.
- `scan_collecting_warnings` is about 481 lines. The same
  `path_error_outcome` form appears 8 times, not 9.
- The parallel drain is at 2036-2070.

### C8. Chunked pool with serial drain (medium)

- `commands.rs:2039-2070`: 512-file chunks alternate parallel extraction and a
  serial drain. Determinism is the goal and is worth keeping. Measure before
  changing; the spool is postcard, so the drain may be cheap.

**Validation.** CONFIRMED.
- `EXTRACT_SPOOL_CHUNK_SIZE` is 512 at `commands.rs:1812`.
- The comment still says the serial drain exists to keep scan order.
- The serialization is source-confirmed. Its wall-time cost has not been
  measured, so preserve determinism until a benchmark isolates the drain.

### C9. Fourteen `GROUP BY file_id` scans after every scan and update (medium)

- `artifact_access.rs:698-806` computes a top-25 attribution table over every
  child table on every run, including a one-file incremental update.
- Direction: compute only for `--json` output, or only for touched files.

**Validation.** PARTIALLY CONFIRMED. Update does not run this.
- The 14 `GROUP BY` queries are real.
- Scan always calls `file_row_attribution` when it builds the report,
  including non-JSON runs. The cap is 20 rows (`SCAN_REPORT_FILE_ROW_LIMIT`),
  not 25.
- Info calls it with no limit, so it attributes every file.
- No call from update. The "including a one-file incremental update" clause
  is wrong.
- `table_totals` also `COUNT(*)`s the same tables on the same scan.

### C10 to C12 (low)

- `--strict-schema` is a no-op on four of the seven commands that accept it
  (`artifact_access.rs:523`).
- Duplicate helpers across `store/*`: `quote_identifier`, `valid_blake3_hash`,
  `base_report`, and the coordinator-open block, each two to four copies.
- `MILLER_STORE_CHUNK_VERSIONS` (`executor.rs:1417-1432`) is an undocumented
  env var that changes durable output.

**Validation.**
- C10 PARTIALLY CONFIRMED. Six commands take the flag: scan, update, delete,
  rebind, info, export. Not seven. Write commands already refuse a schema
  mismatch, so the flag is redundant there by contract. Info and export honor
  it. The check is `strict_schema || access == Write`.
- C11 CONFIRMED. `quote_identifier` in from_artifact and export.
  `valid_blake3_hash` in from_artifact and executor. Store `base_report`
  copies: import, update, from_artifact, delete (plus `reports.rs`).
  Coordinator opening has five call sites across four modules.
- C12 PARTIALLY CONFIRMED. The env var freezes chunk sizes on the request, so
  it changes quanta, `indexed_at` per chunk, and log events. It should not
  change per-file fact rows. It is already documented in
  `docs/contracts/cli.md`. Do not write a new page for it.

Kept after the deletion test: `watchdog.rs`, `spool.rs`, `progress.rs`
(throttled to one write per second), `limits.rs`, `paths.rs`, the hand-written
discovery walk, and `ReportBuilder`. `sha2` alongside `blake3` is justified.

**Validation.** CONFIRMED for the keep list.
- Progress interval is `Duration::from_secs(1)`.
- CLI uses blake3. sha2 is still a direct dependency of the CLI and artifact
  crates.

## Artifact crate (`crates/julie-extract-artifact`)

### A1. Two schemas and two row-binder stacks for the same tables (high)

- `src/schema.rs` and `src/store/schema.rs` define the same tables by hand.
  Binders exist twice (`writer/rows.rs` and `store/rows.rs`); capability sync
  exists twice. Every column change lands in four places.
- Direction: derive the store DDL from the artifact DDL plus key columns;
  one binder per table parameterized on the key.

**Validation.** CONFIRMED as duplication. The files are not identical.
- Artifact schema: 555 lines. Store schema: 1,377 lines. Store adds
  `version_id` / `extraction_epoch`, `STRICT`, deferred FKs, and coordinator
  tables. Store uses `file_versions`, not `files`.
- Binders are not 1:1. Artifact has 16 `insert_*` functions. Store has three
  level inserters plus `insert_reference_sites`.
- A shared fact column still lands in both DDLs and both binders.

### A2. Store writer re-syncs the whole capability snapshot per file (high)

- `store/writer.rs:428-460` runs `sync_capability_snapshot`: 40 language
  upserts, fixture and gap upserts, four COUNT queries, and a SELECT per row.
  Over 100 statements per file before the first symbol row.
  `stage_capability_snapshot` exists to do it once but the static
  `write_level_in_transaction` skips it. Pairs with C2.

**Validation.** CONFIRMED. "Over 100 statements" is low.
- L1 with `Some(snapshot)` always calls `sync_capability_snapshot`, even when
  the epoch is already initialized. Later files hit `INSERT OR IGNORE` plus a
  full match: four COUNTs, then a SELECT per inventory, language, fixture, and
  gap row. Fixture rows alone are about 199.
- Instance `write_level` does use `stage_capability_snapshot` and drops it
  after the first matching L1. Tests use that path. The CLI store path uses
  the static function and does not.

**Status: CLOSED.** Fixed in commit `154fad78` (Audit Wave 1, Task 5). Optimized `write_level_in_transaction` to verify capability fingerprint against initialized epochs before re-running full snapshot synchronization, eliminating over 100 redundant SQL statements per file.

### A3. `copy_table` pages with LIMIT/OFFSET (high)

- `store/generation.rs:857`. Every page walks past all earlier rows, so a
  promote is O(n²/512) per table. `maintenance.rs:3326` already has the keyset
  form.
- Direction: keyset pagination.

**Validation.** CONFIRMED.
- `copy_table` still builds `LIMIT ?1 OFFSET ?2`. Default window is 512
  (`DEFAULT_COPY_WINDOW`). The O(n²/page) shape is real.
- `maintenance.rs:3326` is inspector paging of `file_versions`, not a drop-in
  replacement for `copy_table`. Copy also re-prepares the SELECT each page.

### A4. Store projection deep-clones every file (medium)

- `store/model.rs:236` clones the whole `ArtifactFile`; the caller drops the
  original right after. Direction: take by value.

**Validation.** CONFIRMED.
- `try_from_artifact_file` takes `&ArtifactFile` and does `let mut file = file.clone()`.

### A5. `structural_fact_ids` grows for the whole scan (medium-high)

- `writer/rows.rs:160`: a `HashSet<String>` lives for the whole transaction and
  clones every id. The column is already the primary key, and the store path
  dedupes per file. Direction: dedupe per file with `&str`, or `INSERT OR IGNORE`.

**Validation.** CONFIRMED.
- `ChildRowInserters` holds `structural_fact_ids: HashSet<String>` for the
  whole artifact write transaction and clones every id.
- Store PK is `(version_id, structural_fact_id)`, not the bare id. Store
  dedupes per file only.

### A6. Migration debris runs on every writer open (medium)

- `store/connection.rs:263-266`: index ensure, resolution-object retire,
  capability-gap reap, and two `read_dir` walks on every open. The coordinator
  also runs `DROP INDEX IF EXISTS` per open (`coordinator.rs:3067`).
- Direction: bump the store schema version once; do retirement in
  `create_store_schema`.

**Validation.** PARTIALLY CONFIRMED.
- Writer open still calls `ensure_read_symbol_indexes`,
  `retire_resolution_store_objects`, `reap_retired_resolution_capability_gaps`,
  and `reap_retired_resolution_files`.
- `DROP INDEX IF EXISTS uidx_coord_one_claimed_resolve` lives in
  `store/schema.rs:221`. Coordinator open at 3062 calls
  `retire_coordinator_resolution_objects`.
- After the first success, index create and `DROP INDEX IF EXISTS` are cheap.
  The gap `DELETE` and both `read_dir` walks still run every open.

### A7. `checkpoint_wal(TRUNCATE)` after every write, including single-file updates (medium)

- `writer.rs:358, 459, 495, 561, 619`. A stream of single-file updates pays a
  full checkpoint and fsync per file.
- Direction: leave autocheckpoint in charge for update and delete; TRUNCATE at
  scan end or close.

**Validation.** PARTIALLY CONFIRMED. The sites are the artifact writer, not
the store writer.
- Cited lines 358, 459, 495, 561, 619 all exist on
  `crates/julie-extract-artifact/src/writer.rs`. They cover snapshot sync,
  delete miss, delete, update skip, and update write. Empty commits still
  checkpoint.
- Scan uses the same TRUNCATE after commit via `finish_journal`.
- `store/writer.rs` has no `checkpoint_wal`. Store stays on WAL plus
  `synchronous=FULL`. Do not "fix" the store writer for this item.

### A8. JSONL export builds a `serde_json::Value` tree per row (medium)

- `jsonl.rs:604-716` and 18 siblings. One table already uses the streaming
  raw path (`jsonl.rs:1448`). Direction: one `Serialize` struct per record.

**Validation.** CONFIRMED.
- Identifiers still build `json!({...})` (see E1). Structural facts already
  use `write_record_raw_object` at 1448. There are 21 `json!` exporters and
  one raw path, not 18 siblings. `604-716` is `export_symbols`.

### A9. Store public API is ~90 re-exports for one consumer (medium)

- `store/mod.rs:20-63`. Direction: `pub(crate)` everything the CLI does not import.

**Validation.** PARTIALLY CONFIRMED.
- Ten `pub use` groups export about 110 names, not 90.
- The CLI is the only production crate that imports `store`. Artifact tests
  also use the public API, so it is not literally one consumer.

### A10 to A12

- `StatementPreparationCounter` (`store/rows.rs:20-38`) is a test metric in
  product API, asserted `== 21` in a test. Gate it with `#[cfg(test)]`.
- The lease heartbeat opens and fully configures a new connection every 1.67 s
  (`coordinator.rs:2855-2875`). Reuse one connection.
- `maintenance.rs` (3582 lines) and `coordinator.rs` (3302) mix planning,
  inspection, lease lifecycle, GC, and pid liveness. Split by concern.

**Validation.**
- A10 CONFIRMED. The counter type is `pub(super)`. The public leak is
  `StoreWriteResult.statement_preparations`. Tests still assert L1 `== 21`.
  The number counts `prepare_cached` calls, not SQLite compiles.
- A11 CONFIRMED. Interval is `lease_duration_ms / 3` with a 5,000 ms lease,
  so 1,666 ms. Each tick opens a new coordinator connection in
  `heartbeat_lease_at`. A failed tick can open a second one to reclaim.
- A12 CONFIRMED. Line counts match: 3,582 and 3,302.

Durability: no defect found. The bulk-load pragma trade is gated to an empty
artifact and closed by `foreign_key_check` before commit. Good.

**Validation.** CONFIRMED.
- Bulk-load pragmas (`journal_mode=MEMORY`, `synchronous=OFF`) run only when
  the artifact has no `files` and no `extraction_revisions`.
- Commit still runs `PRAGMA foreign_key_check`, then restores WAL.
- Store writer does not use this trade. It stays on WAL plus
  `synchronous=FULL`.

## Tests, build, hygiene

Measured: `cargo xtask test default` runs 4,750 tests. On this machine, three
warm runs took 33.457, 33.534, and 33.603 seconds. Tests are gated behind
`#[cfg(test)]` and never enter a plain build. Tier design is clean.

**Validation.** CONFIRMED. The command passed four times, including the warm-up,
and a fifth captured run also reported 4,750 passed and zero failed. The timing
is a local baseline, not a portable pass/fail threshold.

### T1. Golden and capability tiers never run in CI (high)

- `.github/workflows/ci.yml:47,50` runs only `default` and `contract`. The
  225 `expected.json` fixtures and `capabilities.json` are behind
  `test-golden` and `test-capability-matrix` features, enabled only by
  `xtask test golden|capability|language|changed`. Nothing guards them on push.

- Direction: add both tiers to `ci.yml`. They are `--lib` runs.

**Validation.** REFUTED.
- CI Fast Gates run `cargo xtask test contract`.
- `contract_plan()` starts with `golden_plan()` and `capability_plan()`.
  `xtask/tests/test_tiers.rs` asserts that.
- 225 `expected.json` files exist. The features are real. They already run
  on push through contract.
- Do not add duplicate golden/capability jobs. The original direction would
  double the work.
- Focused proof: `cargo test -p xtask --test test_tiers
  test_contract_tier_runs_golden_and_capability_gates_with_features -- --exact`
  passed.

### T2. `scripts/language-data-quality-report.mjs --strict` is not in CI or xtask (high)

CLAUDE.md requires it. No workflow or xtask references it.

**Validation.** CONFIRMED.
- CLAUDE.md and AGENTS.md require the script after capability or fixture
  changes. No hit in `.github/workflows`, `xtask/`, or
  `scripts/check-release-state.sh`.
- The requirement is not "run on every push". Wire it to fixture/capability
  CI or to preflight.

### T3. The wall-clock budget tripwire from CLAUDE.md does not exist (high)

What exists bans `Instant::now()` in default tests. Nothing measures suite
time. Add a threshold to the xtask default runner, or fix the CLAUDE.md line.

**Validation.** CONFIRMED that the CLAUDE.md tripwire is missing. A failing
CI timer is the wrong fix.
- CLAUDE.md and AGENTS.md line 69 ask for a wall-clock budget tripwire.
- The existing guard is
  `julie-extract-artifact/tests/test_tiers.rs::default_suite_tests_assert_no_wall_clock_budget`.
  It only scans that crate's `tests/*.rs` for `Instant::now()`. It does not
  time the suite.
- `docs/testing-strategy.md` says pass/fail must not depend on wall-clock
  time. The documents conflict. Reconcile both synced agent files with the
  testing strategy, or keep the budget local and informational.

### T4. Tombstone tests keep a dead fixture alive (high)

`crates/julie-extract-cli/tests/test_tiers.rs:8-36` asserts nine deleted
files still do not exist and that `fixtures/store-resolution/` (11 files, no
code reference) still exists. Delete both.

**Validation.** PARTIALLY CONFIRMED.
- The missing-file test checks 8 paths, not 9, plus two manifest strings.
  Those tombstones block resurrection. They do not keep files alive.
- `legacy_resolution_fixture_and_oracle_are_checked_in_together` asserts
  `fixtures/store-resolution/legacy-v3` still exists. That is 11 files.
- Production code does not read the fixture. The existence test and some
  `.razorback/` reports do.
- Delete the existence test and the fixture together. Treat the absence
  tombstones as a separate, optional cleanup.

### T5. Test helpers copy-pasted per language (medium)

`metadata_str` 15 copies, `extract` 15, `config` 15, `init_parser` 10,
`facts_with_pattern` 8. Move them into `src/tests/helpers.rs`.

**Validation.** PARTIALLY CONFIRMED. Duplication is larger than listed.
- Exact function-name counts under `src/tests`: `metadata_str` 54,
  `init_parser` 44, `facts_with_pattern` 24, `extract` 121, and `config` 15.
  These counts include different signatures and semantics, so they establish
  repetition, not that every body is interchangeable.
- `src/tests/helpers.rs` and `src/tests/test_utils.rs` already exist.
  Consolidate by behavior and retire one of those modules. Do not create a
  third helpers file. This is medium-priority hygiene, not a high defect.

### T6. 30 non-main worktree branches are merged; one tree is dirty (high)

All 30 non-main branches are merged. `.claude/worktrees/` is excluded only via
the local `.git/info/exclude`, not `.gitignore`. Remove the worktrees, delete
the branches, add the ignore rule.

**Validation.** PARTIALLY CONFIRMED.
- 31 worktrees. Every non-main branch has 0 commits that are not in `main`.
- `.gitignore` already ignores `.worktrees/`. `.claude/worktrees/` is only in
  `.git/info/exclude`.
- Three of the stale trees live under `.worktrees/`:
  `fix/store-writer-heartbeat`, `fix/test-detection-precision`,
  `release/2.32.1`.
- The `ct-language-audit-plan` worktree contains two untracked docs. It is not
  safe to remove until those files are reconciled. The other 29 non-main
  worktrees were clean during validation.

### T7. `.razorback/` is ignored but 13 files are tracked

Untrack them or drop the rule.

**Validation.** CONFIRMED.
- `.gitignore` has `.razorback/`. `git ls-files .razorback` lists 13 files.

### T8. Bookkeeping weight (medium)

- 683 of 1,106 commits touch `.memories/`. 196k words tracked; nothing in the
  build reads them.
- `docs/` is 486k words. `docs/plans/` has 103 files, 12 with a status marker.
  Release evidence and release notes duplicate each other per release, and the
  release preflight requires 20 contract docs.
- Direction: squash memory commits into work commits; archive plans without
  status; merge release evidence into release notes.

**Validation.** PARTIALLY CONFIRMED. Several counts are off.
- Commits: 1,106 total. 495 touch `.memories/`, not 683.
- Words: 196,008 in `.memories/` (630 files). 485,604 in `docs/` (376 files).
- Plans: 97 markdown files, not 103. Exactly 12 have an explicit `Status:`
  field or status heading, so the original status-marker count was right.
- Release preflight packages 31 `Doc` items plus a release note
  (`xtask/src/release.rs`), not 20 contract docs.
- Sixty-nine releases have both evidence and release-note markdown. The latest
  pair has different jobs: evidence records the source, gates, and live asset
  hashes; notes describe behavior and compatibility. Do not merge them based
  on file count alone.
- Do not rewrite the 495 published memory commits. Apply the existing Goldfish
  rule going forward: checkpoint before a work commit so the memory file rides
  with that commit. Archiving plans does not rewrite history.

### T9 to T11 (low)

- `operations_contract.rs` (4,291 lines) is fine at runtime today but should
  split by command when next touched.
- Three files named `writer_perf`, `writer_performance`, and
  `store_writer_performance` invite the confusion the guard test exists to stop.
- No release profile in `Cargo.toml`. The shipped binary with 40 parsers builds
  without LTO or codegen-unit tuning.

**Validation.**
- T9 CONFIRMED. The file is `crates/julie-extract-cli/tests/operations_contract.rs`,
  4,291 lines. It is a test, not runtime code.
- T10 CONFIRMED. All three live under
  `crates/julie-extract-artifact/tests/`.
- T11 CONFIRMED. No `[profile.release]` in the workspace or crate
  `Cargo.toml` files. Release CI is `cargo build --release` with default
  Rust settings (`lto=false`, `codegen-units=16`).
  `capabilities.json` lists 40 languages. That is 40 specs, not 40 unique
  parser crates (TSX/JSX share grammars). The defaults match the current
  [Cargo profile reference](https://doc.rust-lang.org/cargo/reference/profiles.html).
  Measure binary size, build time, and a fixed extraction workload before
  adding LTO.

## Suggested order

1. E1, E2, E3, C2/A2, C1: local deletions and one-time computation. One
   agent session each, measurable with the existing `xtask performance` tier.
2. T2, T4, T7, and the 29 clean worktrees from T6: CI and hygiene. Preserve
   the dirty `ct-language-audit-plan` worktree until its files are reconciled.
3. A3, A5, A7, C3, C4, C9: query and loop fixes. One session.
4. C6, E9, E10, E12, A9: dead code and API narrowing. One or two sessions.
5. A1 and E4/E11: the two structural refactors. Each needs a plan and a
   decision record.

**Validation.** CONFIRMED after correcting steps 1 and 2. Skip T1. Reconcile
T3 across AGENTS.md, CLAUDE.md, and `docs/testing-strategy.md` rather than
adding a flaky CI timer. Keep A7 aimed at `src/writer.rs`, not
`store/writer.rs`. Keep E4/E11 as planned work.
