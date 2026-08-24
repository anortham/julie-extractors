# Language detection correction report

## Status

Green focused correction; parallel-lead-commit. No commit was created in this
packet. The parent agent must stage only the owned paths and the checkpoint
listed below, leaving concurrent QML extractor changes untouched.

## Root cause

- CLI discovery converted the entire absolute `Path` to UTF-8 before calling
  source detection, so a valid ASCII basename under an invalid-UTF8 parent was
  rejected.
- Store planned-file language fallback and extraction looked only at an
  extension, so extensionless `qmldir` failures were labeled `unknown`.

## Changes

- Added public `detect_language_for_path(&Path, content)` in
  `crates/julie-extractors/src/language_spec/mod.rs` and re-exported it through
  `language` and the crate root.
- Kept `detect_language_for_source(&str, content)` as a compatibility wrapper
  delegating to the Path API.
- Updated the pipeline helper and CLI discovery to use the Path API directly.
- Updated `PlannedImportFile::language()` and
  `StoreRequestExecutor::extract()` to use the same basename/source-aware
  contract.
- Added Unix path tests for invalid-UTF8 parents, mixed-case QML/QMLDIR and
  qmltypes, C++ `.h` content sniffing, and ordinary extensionless rejection.
- Added a store-contract test proving a failed `qmldir` manifest row records
  `language = qmldir`.

## Verification

- `cargo test -p julie-extractors test_detect_language_for_path_ignores_invalid_utf8_parent_components`
- `cargo test -p julie-extractors test_detect_language_for_path_uses_source_contract_for_qmldir_basename`
- `cargo test -p julie-extractors test_detect_language_for_source_routes_cpp_h_header_and_preserves_c_header`
- `cargo test -p julie-extract-cli discover_handles_supported_files_under_invalid_utf8_parent_components`
- `cargo test -p julie-extract-cli planned_extensionless_qmldir_uses_the_source_language_contract`
- `cargo test -p julie-extract-cli --features test-store-contract --test store_equivalence failed_extensionless_qmldir_manifest_row_preserves_language -- --nocapture`
- `cargo fmt --all -- --check`

## Worktree

- Path: `/home/murphy/source/julie-extractors/.worktrees/qml-first-class-extraction`
- Branch: `feat/qml-first-class-extraction`
- Base/HEAD at packet start: `e715b249`
- Dirty state: owned detection/CLI/store files and this report/checkpoint are
  modified, alongside concurrent QML extractor and fixture edits. No unrelated
  changes were reverted.
- Goldfish checkpoint: `.memories/2026-08-24/171634_3e21.md`

## Handoff

Parent should review and commit the explicit owned paths plus this report and
checkpoint. Broader QML/qmldir language gates, the full contract, Windows
verification, and the requested Grok review remain lead-owned.
