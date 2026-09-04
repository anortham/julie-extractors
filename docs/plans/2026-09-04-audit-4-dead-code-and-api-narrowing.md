# Audit Wave 4: Dead Code, Duplication, and API Narrowing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Delete orchestration layers with no callers, collapse copy-pasted helpers, stop compiling CLI modules twice, and shrink the public API of the two library crates to what consumers use.

**Architecture:** Deletions and visibility changes. The one structural move is making the CLI binary a shim over its own library crate. Public-API changes are recorded in the extraction contract doc because downstream Rust callers outside this workspace may exist.

**Tech Stack:** Rust.

**Architecture Quality:** Affected modules: `julie-extractors/src/{lib,manager,factory,language,routing_*}.rs`, `julie-extractors/src/base/extractor.rs`, `julie-extractors/src/vue/*`, `julie-extract-cli/src/{lib,main}.rs`, `julie-extract-artifact/src/store/{mod,rows,writer}.rs`. Caller-facing interface: the crate roots export fewer items; every item a workspace consumer imports stays exported at the crate root. Test surface: `src/tests/api_surface.rs` is the executable contract for the extractors crate and must be updated in the same task that changes an export. Rejected shortcuts: keeping `ExtractorManager` as a deprecated alias (it duplicates detection work), and a third test-helper module. Architecture risk: medium for the API narrowing (unknown downstream callers); low elsewhere.

Source: findings C6, C11, E5, E9, E10, E12, A9, A10, T5.

## Global Constraints

- Every export removed from a crate root is listed in `docs/contracts/extraction-output-changes.md` under a new "Rust API" heading with the version that removes it.
- `EXTRACTION_CONTRACT_VERSION` does not change; it tracks output, not Rust API.
- `pending_relationships` on results stays; removing it is a contract bump and is out of scope.
- No golden fixture changes.
- Zero comments in tests. No narration comments.

## Verification Strategy

**Project source of truth:** `docs/testing-strategy.md`, `crates/julie-extractors/src/tests/api_surface.rs`.

**Worker red/green scope:** `cargo test -p julie-extractors --lib tests::api_surface` for export changes; `cargo xtask test language <name>` for each language touched; `cargo test -p julie-extract-cli` for the bin/lib merge; `cargo test -p julie-extract-artifact` for store visibility.

**Worker ceiling:** `cargo xtask test default`.

**Worker gate invariant:** the workspace builds with `cargo build --workspace --all-targets` and every consumer import resolves.

**Lead affected-change scope:** `cargo xtask test changed <paths>` plus `cargo clippy --workspace --all-targets -- -D dead_code` for the CLI crate after Task 1.

**Branch gate:** `cargo xtask test default`, `cargo xtask test contract`, `cargo fmt --check`, `cargo clippy --workspace --all-targets`, `cargo doc --workspace --no-deps` (catches broken intra-doc links after deletions).

**Security scope:** none declared.

**Replay/metric evidence:** none.

**Escalation triggers:** any consumer crate import that stops resolving; any README example that stops compiling (`cargo test --doc -p julie-extractors`).

**Assigned verification failure:** Workers stop and report when assigned verification fails, unless this plan explicitly says to update that gate.

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, and timestamp.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: CLI binary as a shim over the lib | None - serial | Modify `crates/julie-extract-cli/src/lib.rs`, `main.rs`; move `args.rs` and `commands.rs` into the lib module tree; remove all `#[allow(dead_code)]`; modify `artifact_access.rs:158`, `watchdog.rs:114,125` | Yes | Touches every CLI module declaration; nothing else in the CLI can run in parallel. |
| Task 2: Delete extractors orchestration leftovers | Batch A | Delete `crates/julie-extractors/src/manager.rs`, `routing_symbols.rs`, `routing_identifiers.rs`, `routing_relationships.rs`; modify `lib.rs`, `factory.rs`, `registry.rs`, `language.rs`, `src/tests/api_surface.rs`, `src/tests/jsonl_pipeline.rs`, `crates/julie-extractors/README.md` | No | None - safe parallel batch. |
| Task 3: Shared doc-comment and visibility helpers | Batch A | Modify `crates/julie-extractors/src/base/extractor.rs` (visibility of `previous_comment_texts`, `select_doc_comment_block`); create `base/visibility.rs`; modify `go/helpers.rs`, and `helpers.rs` in csharp, java, kotlin, php, scala, razor, vbnet, plus `swift/signatures.rs` | No | None - safe parallel batch. |
| Task 4: Vue parses each section once | Batch A | Modify `crates/julie-extractors/src/vue/parsing.rs`, `vue/mod.rs`, `vue/script.rs`, `vue/script_setup.rs`, `vue/identifiers.rs`, `vue/relationships.rs`, `vue/test_calls.rs`, `base/complexity_metrics.rs` (`parse_vue_script_tree` at 1335) | No | None - safe parallel batch. |
| Task 5: Narrow the extractors crate root | None - serial | Modify `crates/julie-extractors/src/lib.rs`, `src/tests/api_surface.rs`, `docs/contracts/extraction-output-changes.md` | Yes | Depends on Task 2 (manager exports gone) and Task 4 (vue module visibility). |
| Task 6: Narrow the store module and gate the preparation counter | Batch B | Modify `crates/julie-extract-artifact/src/store/mod.rs`, `store/rows.rs`, `store/writer.rs`, `tests/store_writer_batching_contract.rs` | No | None - safe parallel batch. |
| Task 7: Store CLI helper dedupe | Batch B | Create `crates/julie-extract-cli/src/store/common.rs`; modify `store/export.rs`, `store/from_artifact.rs`, `store/executor.rs`, `store/import.rs`, `store/update.rs`, `store/delete.rs` | Yes | Depends on Task 1 (module tree settled). |
| Task 8: Consolidate test helpers | None - serial | Modify `crates/julie-extractors/src/tests/helpers.rs`; delete `src/tests/test_utils.rs`; modify every `src/tests/<lang>/mod.rs` that defines a duplicate | Yes | Touches most test files; run alone after Batch A. |

Commit mode: `serial-worker-commit` for serial tasks; `parallel-lead-commit` inside Batch A and B.

---

## Task 1: CLI binary as a shim over the lib

**Files:** `crates/julie-extract-cli/src/lib.rs`, `main.rs`, `args.rs`, `commands.rs`, `artifact_access.rs`, `watchdog.rs`.

**What to build:** `main.rs` becomes:

```rust
fn main() -> std::process::ExitCode {
    julie_extract_cli::run_from_env()
}
```

`lib.rs` declares every module once, exposes `pub fn run_from_env()`, and keeps `pub mod limits` and `pub mod store` public. All nine `#[allow(dead_code)]` attributes in `lib.rs` and the three in `artifact_access.rs` and `watchdog.rs` go away. Whatever the compiler then reports as dead is deleted or wired.

**Approach:** Move `commands.rs` and `args.rs` under the lib as private modules. Integration tests in `tests/` that reach internals through the lib keep working because they already use the lib. Run `cargo clippy -p julie-extract-cli --all-targets -- -D dead_code` and fix every hit. Check `watchdog.rs:114-128` (`process_status`): it is reached from `store/import.rs:1151`, so it stays; the attribute was hiding a bin-only false positive.

**Acceptance criteria:**
- [x] `grep -rn 'allow(dead_code)' crates/julie-extract-cli/src` is empty.
- [x] `main.rs` is three lines.
- [x] `cargo test -p julie-extract-cli` passes; `CARGO_BIN_EXE_julie-extract` integration tests still spawn the binary.

## Task 2: Delete extractors orchestration leftovers

**Files:** Delete `manager.rs`, `routing_symbols.rs`, `routing_identifiers.rs`, `routing_relationships.rs`. Modify `lib.rs` (lines 51, 58-60, 115), `factory.rs`, `registry.rs`, `language.rs` (17, 50, 75, 166), `src/tests/api_surface.rs` (7, 76, 259), `src/tests/jsonl_pipeline.rs` (1, 36), `README.md` (10, 12, 64).

**What to build:** Nothing. Point the two tests and the README at `extract_canonical` / `extract_canonical_at`. Move `convert_types_map` from `factory.rs` to `registry.rs` (its only importer) and delete `factory.rs` if nothing but `#[cfg(test)]` items remain; otherwise keep the test items in a `src/tests/` file. Delete the four `language.rs` functions.

**Approach:** Miller `trace` on each deleted item before removing it. The README example at line 64 must compile as a doc test or be marked `no_run`; run `cargo test --doc -p julie-extractors`.

**Acceptance criteria:**
- [ ] `ExtractorManager` does not exist anywhere in the workspace.
- [ ] `cargo test -p julie-extractors --lib tests::api_surface` and `tests::jsonl_pipeline` pass.
- [ ] README compiles as a doc test.

## Task 3: Shared doc-comment and visibility helpers

**Files:** `base/extractor.rs` (`previous_comment_texts` 340, `select_doc_comment_block` 450 become `pub(crate)`); create `base/visibility.rs`; modify `go/helpers.rs` (delete `preceding_comment_texts` 218 and `select_go_doc_comment_block` 346); modify `determine_visibility` in csharp, java, kotlin, php, scala, razor, vbnet `helpers.rs` and `swift/signatures.rs`.

**What to build:** One `pub(crate) fn visibility_from_modifiers(modifiers: &[String]) -> Visibility` in `base/visibility.rs` that maps `public`, `private`, `protected`, `internal`, `fileprivate`, `Public`, `Private`, `Protected`, `Friend` and the default rule each language uses today. Each language keeps a one-line wrapper only if its default differs from the shared default; write the per-language default table into the test.

**Approach:** Before merging, write a table test that runs each language's current `determine_visibility` against the same modifier lists and the shared function, and asserts equality. Do not force Go's uppercase rule into the shared helper (`go/helpers.rs::is_public` stays). Then swap implementations.

**Acceptance criteria:**
- [ ] Go's doc-comment helpers are gone and Go uses the base ones; `cargo xtask test language go` passes.
- [ ] Eight `determine_visibility` bodies collapse to the shared function or a one-line default wrapper.
- [ ] Language tiers for all eight pass; golden passes with zero changes.

## Task 4: Vue parses each section once

**Files:** `vue/parsing.rs` (`parse_vue_sfc` 61), `vue/mod.rs` (58), the five `parse_script_section` copies (`script.rs:136`, `script_setup.rs:219`, `identifiers.rs:63`, `relationships.rs:467`, `test_calls.rs:41`), `base/complexity_metrics.rs:1335`.

**What to build:** A `ParsedVueSfc` in `vue/parsing.rs` that holds the sections and a lazily parsed `Tree` per script section, built once in `vue/mod.rs` and passed by reference to symbols, script-setup, identifiers, relationships, test calls, and complexity. `parse_vue_sfc` takes `&str` without the content clone.

**Approach:** Write a test that counts `Parser::parse` calls for one script-setup SFC through the existing test hooks (or by wrapping the parse in a counting function inside `vue/parsing.rs` under `#[cfg(test)]`). Expect one parse per script section after the change. Delete the five copies. Complexity's per-callable reparse must reuse the section tree and slice by byte range instead.

**Acceptance criteria:**
- [x] One `fn parse_script_section` (or equivalent) exists, in `vue/parsing.rs`.
- [x] One script-setup SFC parses each script section once.
- [x] `cargo xtask test language vue` and golden pass with zero fixture changes.

## Task 5: Narrow the extractors crate root

**Files:** `lib.rs`, `src/tests/api_surface.rs`, `docs/contracts/extraction-output-changes.md`.

**What to build:** The crate root exports: `extract_canonical`, `extract_canonical_at`, `ExtractionLevel`, `ExtractionResults` and its row types, `detect_language_for_path`, `detect_language_for_source`, `capability_snapshot` and its types, `language_policy::classify_literals_by_carrier`, `EXTRACTION_IDENTITY_EPOCH`, `EXTRACTION_CONTRACT_VERSION`, `registry::supported_languages`, and the row types the CLI imports from `base` today (`ComplexityMetric`, `NormalizedSpan`, `StructuralFact`, `StructuredPendingRelationship`), re-exported at the root. The full list is the union of every `use julie_extractors::` line in `crates/julie-extract-cli/src` and `crates/julie-extract-artifact/src`; build it with grep before editing. The 38 language modules, `base`, `registry`, `pipeline`, `test_detection`, `test_calls`, `utils` become `pub(crate)`.

**Approach:** Grep each workspace consumer's imports first (`use julie_extractors::` in both other crates) and confirm each still resolves. Update `api_surface.rs` to assert the new surface. Record every removed export in the contract doc with the release version that removes it.

**Acceptance criteria:**
- [ ] `cargo build --workspace --all-targets` passes.
- [ ] `api_surface.rs` lists exactly the exported items.
- [ ] Contract doc has the Rust API section.

## Task 6: Narrow the store module and gate the preparation counter

**Files:** `store/mod.rs` (the `pub use` groups at 20-63), `store/rows.rs` (`StatementPreparationCounter` 20-38), `store/writer.rs` (`StoreWriteResult.statement_preparations`), `tests/store_writer_batching_contract.rs`.

**What to build:** `store/mod.rs` exports only what `crates/julie-extract-cli/src` and the artifact crate's own `tests/` import. `statement_preparations` moves behind `#[cfg(any(test, feature = "test-store-contract"))]`, mirroring the artifact writer's `writer_prepare_metrics`. The `== 21` assertions stay under that gate.

**Approach:** Build the import list from `grep -rhn 'use julie_extract_artifact::store' crates/julie-extract-cli/src crates/julie-extract-artifact/tests`. Change visibility, build, fix.

**Acceptance criteria:**
- [ ] Every remaining `pub use` in `store/mod.rs` has at least one importer outside the module.
- [ ] `statement_preparations` is not on the non-test `StoreWriteResult`.
- [ ] `cargo test -p julie-extract-artifact` passes, including `--features test-store-crash`.

## Task 7: Store CLI helper dedupe

**Files:** Create `crates/julie-extract-cli/src/store/common.rs`. Modify `store/export.rs:649`, `store/from_artifact.rs:631,720,730`, `store/executor.rs:1316,1327`, and the `base_report` copies in `import.rs:913`, `update.rs:313`, `delete.rs:163`, `from_artifact.rs:675`, plus the coordinator-open block in `import.rs:340-353,494-506`, `update.rs:71-82`, `from_artifact.rs:654-673`.

**What to build:** `common.rs` with `quote_identifier`, `valid_blake3_hash`, `valid_root_relative_path`, `base_report`, and `open_cli_coordinator(layout) -> Result<StoreCoordinator, _>` that builds the `LeaseHolder` with the `cli-<pid>` name and the `ImportClock` / `ImportPidLiveness` runtime.

**Approach:** Diff the copies first; if two `base_report` bodies differ, keep the difference as a parameter, not a second function. Existing store CLI tests cover every call site.

**Acceptance criteria:**
- [ ] Each helper has exactly one definition in the CLI crate.
- [ ] `cargo test -p julie-extract-cli` passes.

## Task 8: Consolidate test helpers

**Files:** `crates/julie-extractors/src/tests/helpers.rs`, `src/tests/test_utils.rs`, and each `src/tests/<lang>/mod.rs` and framework test module that defines `metadata_str`, `init_parser`, `facts_with_pattern`, `extract`, or `config`.

**What to build:** One helpers module. Merge `test_utils.rs` into `helpers.rs`. For each duplicated name, group the definitions by body: identical bodies move to `helpers.rs` and the local copy is deleted; bodies that differ keep a local, differently named function. The `extract` name has 121 definitions with different signatures; move only the ones with the common `(source) -> ExtractionResults` shape.

**Approach:** Script the grouping (`grep -A8 'fn metadata_str'` per file, hash the bodies) and record the groups in the task report before editing. Run `cargo xtask test default` once at the end; this task is mechanical and touches too many files for per-language runs to be cheaper.

**Acceptance criteria:**
- [ ] `src/tests/test_utils.rs` is gone.
- [ ] `metadata_str`, `init_parser`, `facts_with_pattern` each have one definition under `src/tests/`.
- [ ] `cargo xtask test default` passes.
