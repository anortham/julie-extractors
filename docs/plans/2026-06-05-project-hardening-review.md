# 2026-06-05 Project Hardening Review

## Scope

Deep review of `julie-extractors` for poor Rust practices, performance traps,
incomplete implementations, weak architecture, and leftover sloppy work.

Reviewed surfaces:

- `julie-extract` CLI behavior and report contract
- SQLite artifact writer and revision history
- JSONL export contract
- language detection and extractor hot paths
- release, CI, and specialist gate workflows
- repo metadata and docs drift

## Implementation Result

All F1-F19 findings are fixed in this pass.

- Artifact correctness fixes now cover partial discovery, no-op deletes,
  capability snapshot revisions, warning-only noncritical `info` metadata gaps,
  atomic JSONL path exports, and unsupported update history.
- Release and CI guardrails now include release preflight, target-specific
  specialist staging, strict workspace/all-target clippy, guidance sync in CI,
  and a default-suite wall-clock tripwire.
- Cleanup and contract fixes now cover parser inventory provenance,
  case-insensitive/content-aware language selection, SQL regex hoisting,
  repository metadata, route-literal wording, and clone-free extractor result
  handoff.
- Verification for the implemented fixes includes `cargo test -p xtask`,
  `cargo xtask test default`, `cargo xtask test contract`, strict workspace
  clippy, and focused CLI operation regressions.

## Second Review Follow-Up

The follow-up review found additional contract and hygiene issues after F1-F19.
They are fixed in the same hardening pass:

- Release package staging now uses the requested target's executable suffix and
  default binary path, so Windows packages stage `julie-extract.exe` even on
  non-Windows hosts.
- `.h` language detection now treats equal C/C++ parser diagnostics as
  inconclusive, but honors a parser result that clearly prefers C before running
  C++ token heuristics.
- SQLite revision `counts_json` now records the same row-domain counts as CLI
  reports, including parser inventory and language capability rows.
- Missing-row `delete` and unsupported `update` paths no longer create
  capability-only revisions, and unsupported updates only claim stale rows were
  removed when rows actually changed.
- Scan profiling no longer reports `capability_sync` as a separate phase now
  that capability writes are part of `artifact_write`.
- The broad extractor test clippy allow was removed; remaining test warnings
  are fixed or narrowed to exact helper functions.

## Highest Priority

Fix these first. They can create wrong durable artifacts or publish misleading
release evidence.

## F1. Unreadable discovery paths can delete valid artifact rows

- **Status:** fixed
- **Severity:** high
- **Where:** `crates/julie-extract-cli/src/discovery.rs:97`,
  `crates/julie-extract-cli/src/discovery.rs:102`,
  `crates/julie-extract-cli/src/discovery.rs:107`,
  `crates/julie-extract-artifact/src/writer.rs:771`
- **What exists:** `DiscoveryPolicy::discover_dir` silently returns on
  `read_dir` failure and silently skips entry/file-type errors. The scan writer
  later treats every existing file missing from `snapshot_paths` as deleted.
- **Why it matters:** a transient permission or IO failure can turn a partial
  discovery into an `ok` scan that deletes rows for still-existing files.
- **Evidence:** Reproduced locally by scanning a repo, `chmod 000` on the
  subdirectory that contains the file, and scanning again. The second scan
  returned `status=ok`, `errors=[]`, `files_deleted=1`, and `remaining_files=0`.
- **Fix:** make discovery return typed partial-discovery errors, track
  unreadable/unknown subtrees, and prevent snapshot deletion for paths under
  unknown subtrees. Add a contract regression that proves unreadable paths do
  not delete prior rows silently.

## F2. `delete` against a missing artifact creates a new SQLite artifact

- **Status:** fixed
- **Severity:** high
- **Where:** `crates/julie-extract-cli/src/commands.rs:602`,
  `crates/julie-extract-cli/src/commands.rs:1430`,
  `crates/julie-extract-cli/src/commands.rs:1501`,
  `crates/julie-extract-artifact/src/writer.rs:295`
- **What exists:** `existing_artifact_for_root` returns `Ok(None)` when the DB
  is absent, but `cleanup_delete` still opens an `ArtifactWriter`, which creates
  the DB and syncs capability rows before returning `not_found`.
- **Why it matters:** a no-op watcher delete can leave a durable artifact behind,
  with capability rows but no extraction revision.
- **Evidence:** Reproduced locally with `julie-extract delete --db
  missing.sqlite --file absent.rs --json`: exit `0`, `status=not_found`,
  `db_exists=yes`, `created_revision_id=null`, and capability row counts written.
- **Fix:** return `not_found` before opening the writer when there is no existing
  artifact. Do not sync capability rows for no-op delete paths.

## F3. Capability snapshot writes are outside command revisions

- **Status:** fixed
- **Severity:** high
- **Where:** `crates/julie-extract-cli/src/commands.rs:218`,
  `crates/julie-extract-cli/src/commands.rs:247`,
  `crates/julie-extract-cli/src/commands.rs:1508`,
  `crates/julie-extract-artifact/src/writer.rs:322`,
  `crates/julie-extract-artifact/src/writer.rs:404`
- **What exists:** capability snapshot sync starts and commits its own
  transaction before the scan/update/delete mutation transaction.
- **Why it matters:** failed or no-op commands can mutate durable capability
  tables without a matching `extraction_revisions` row. This also causes F2 to
  leave capability rows behind in a missing artifact.
- **Fix:** fold capability sync into the same artifact mutation transaction and
  revision count, or explicitly define capability-only revisions in the SQLite
  contract and report contract.

## F4. `info` missing-metadata handling violates the report contract

- **Status:** fixed
- **Severity:** high
- **Where:** `docs/contracts/reports.md:239`,
  `docs/contracts/reports.md:270`,
  `crates/julie-extract-cli/src/commands.rs:675`,
  `crates/julie-extract-cli/src/commands.rs:1870`
- **What exists:** the report contract says `info` must include missing metadata
  warnings, and lists `metadata_missing` as a warning code. The implementation
  treats any missing metadata key as `schema_incompatible` and returns exit code
  `3`.
- **Why it matters:** machine consumers cannot distinguish a recoverable legacy
  metadata gap from a hard schema incompatibility.
- **Evidence:** Reproduced locally by deleting `updated_at` from
  `artifact_metadata` and running `julie-extract info --json`. The report had
  `status=failed`, `errors=[schema_incompatible]`, `warnings=[]`, and exit `3`.
- **Fix:** either implement warning-only `metadata_missing` handling for
  non-critical metadata on `info`, or update the reports contract and tests to
  make missing metadata a hard schema error.

## F5. Failed JSONL path exports leave partial output files

- **Status:** fixed
- **Severity:** medium-high
- **Where:** `crates/julie-extract-cli/src/commands.rs:716`,
  `crates/julie-extract-cli/src/commands.rs:751`,
  `crates/julie-extract-artifact/src/jsonl.rs:142`
- **What exists:** the artifact crate has `export_jsonl_to_path`, which writes
  to a temp file and renames on success. The CLI bypasses it and writes directly
  with `File::create`.
- **Why it matters:** contract consumers can receive a truncated JSONL file even
  though the export report says `failed`.
- **Evidence:** Reproduced locally by corrupting one JSON column in a valid
  artifact and running CLI export to a path. The command returned `status=failed`
  and `error_code=export_failed`, but `out.jsonl` still existed with 169 partial
  lines.
- **Fix:** route path exports through atomic same-directory temp-file persistence.
  Use a unique temp name rather than the fixed `.tmp` helper if concurrent
  exports to the same destination are possible.

## F6. Unsupported `update` records a `delete` revision

- **Status:** fixed
- **Severity:** medium-high
- **Where:** `crates/julie-extract-cli/src/commands.rs:487`,
  `crates/julie-extract-cli/src/commands.rs:1353`,
  `crates/julie-extract-cli/src/commands.rs:1372`,
  `crates/julie-extract-artifact/src/writer.rs:437`
- **What exists:** unsupported or ignored update cleanup calls the shared delete
  row-removal path with `WriteOperation::Delete`.
- **Why it matters:** downstream revision history says a delete command happened
  even though the public CLI operation was `update`.
- **Evidence:** Reproduced locally by scanning `src/a.rs`, adding that file to
  `.gitignore`, then running `julie-extract update --file src/a.rs --json`.
  The report said `operation=update` and `status=unsupported`, but SQLite stored
  revision `(2, 'delete', 'single_file')`.
- **Fix:** add an update-specific cleanup operation or extend the revision/change
  contract with an explicit unsupported-cleanup change kind.

## F7. Language detection can disagree between discovery, extraction, and stored rows

- **Status:** fixed
- **Severity:** medium-high
- **Where:** `crates/julie-extract-cli/src/discovery.rs:225`,
  `crates/julie-extract-cli/src/commands.rs:1091`,
  `crates/julie-extract-cli/src/extraction.rs:82`,
  `crates/julie-extractors/src/language_spec/mod.rs:270`,
  `crates/julie-extractors/src/language_spec/mod.rs:280`
- **What exists:** discovery chooses a language from the extension and passes it
  into the artifact row. Extraction then independently re-detects language from
  path/content before parsing. Case-sensitive extension lookup also marks real
  uppercase extensions such as `.TS` unsupported.
- **Why it matters:** C/C++ `.h` files and uppercase extensions can produce
  misleading language profiles or skipped files. A C++ header can be parsed by
  the content-sensitive path while the artifact still records `language='c'`.
- **Evidence:** Local scan of `A.TS` produced `files_unsupported=1` and no file
  rows. Local scan of a C++-style `widget.h` recorded `profile_languages=c` and
  `files.language='c'`.
- **Fix:** create one language decision object for discovery/extraction/storage.
  Normalize extensions where appropriate, and make C/C++ header classification
  part of that single decision. Add fixtures for `.H`, `.TS`, `.CS`, and a C++
  `.h` header.

## F8. Release publish path lacks a hard preflight gate

- **Status:** fixed
- **Severity:** medium-high
- **Where:** `.github/workflows/release-binaries.yml:88`,
  `.github/workflows/release-binaries.yml:91`,
  `xtask/src/release.rs:172`
- **What exists:** release publishing builds, stages, archives, and uploads
  assets. The workflow does not run a single release-preflight command that
  verifies package versions, release notes, recent gates, target package layout,
  and tag/version consistency before upload.
- **Why it matters:** a manually dispatched release can publish assets from a
  commit whose documented release gate was not run or whose package version does
  not match crate metadata.
- **Fix:** add `cargo xtask release preflight --version <version>` and make
  `release-binaries.yml` run it before build/upload. The implemented preflight
  checks Cargo package versions, release-note presence, manifest safety, and the
  required target-specific package layout before assets are built and uploaded.

## F9. Contract-tier downstream smoke is not hermetic

- **Status:** fixed
- **Severity:** medium
- **Where:** `crates/julie-extractors/tests/downstream_smoke.rs:39`,
  `crates/julie-extractors/tests/downstream_smoke.rs:50`,
  `xtask/src/test_tiers.rs:265`
- **What exists:** the contract tier generates a temp downstream crate that path
  depends on `julie-extractors`, but it also adds external dependency
  `anyhow = "1.0"` and builds without a lockfile or offline/vendor guarantee.
- **Why it matters:** a public contract gate can depend on network/cache state
  rather than only the repo. That makes release evidence brittle.
- **Evidence:** `cargo xtask test contract` passed locally, but the downstream
  smoke step printed `Updating crates.io index`, refreshed git parser repos, and
  locked 71 packages for the generated temp crate.
- **Fix:** remove the generated consumer's external dependency, or use a
  checked-in locked fixture that can run with `--locked` and preferably offline.

## F10. Parser inventory provenance omits parser versions

- **Status:** fixed
- **Severity:** medium
- **Where:** `crates/julie-extract-cli/src/commands.rs:1990`,
  `crates/julie-extract-cli/src/commands.rs:2030`,
  `crates/julie-extract-cli/src/commands.rs:2050`
- **What exists:** artifact parser inventory rows set `parser_version` and
  `grammar_version` to `None`, and the parser inventory fingerprint hashes those
  rows.
- **Why it matters:** parser crate version or git revision changes can fail to
  appear in artifact provenance if package names and dependency status stay the
  same.
- **Evidence:** Local scan wrote 36 parser inventory rows; all 36 had
  `parser_version IS NULL` and `grammar_version IS NULL`.
- **Fix:** populate exact parser crate versions or git revisions, then add a
  regression proving parser dependency changes alter
  `parser_inventory_fingerprint`.

## F11. Specialist package staging can mislabel host binaries

- **Status:** fixed
- **Severity:** medium
- **Where:** `.github/workflows/specialist-gates.yml:83`,
  `.github/workflows/specialist-gates.yml:88`,
  `.github/workflows/specialist-gates.yml:91`
- **What exists:** the specialist release-package job runs on Ubuntu, builds
  `target/release/julie-extract`, then stages it under the arbitrary
  `${{ inputs.target }}` directory.
- **Why it matters:** manual evidence can claim a macOS or Windows target while
  packaging a Linux binary.
- **Fix:** make the specialist package job Linux-only, or use the same target
  matrix and target-specific binary path as `release-binaries.yml`.

## F12. Default-suite speed guardrails are documented but not enforced

- **Status:** fixed
- **Severity:** medium
- **Where:** `xtask/src/test_tiers.rs:141`,
  `crates/julie-extractors/src/tests/test_tiers.rs:5`,
  `docs/testing-strategy.md:237`
- **What exists:** the default tier runs plain package tests. The convention test
  checks known slow feature-gated modules, but there is no wall-clock tripwire or
  generic slow-test marker enforcement.
- **Why it matters:** a slow unmarked test can enter the default crate tests and
  CI will only notice after the suite becomes painful.
- **Evidence:** `cargo xtask test default` passed locally in about 7 seconds and
  ran 2,291 core extractor tests. That is acceptable today, but still unbudgeted.
- **Fix:** add a default-tier wall-clock budget gate and keep convention tests
  enforcing that known slow certification gates stay out of the default suite.

## F13. Clippy gates do not cover the whole workspace/test surface

- **Status:** fixed
- **Severity:** medium
- **Where:** `.github/workflows/ci.yml:27`,
  `TODO.md:37`
- **What exists:** CI clippy only gates `julie-extract-artifact`,
  `julie-extract-cli`, and `xtask` lib/bin targets. The legacy core extractor
  crate and test targets are outside the warning-as-error gate.
- **Why it matters:** low-quality Rust and test-support drift can accumulate
  while normal CI remains green.
- **Evidence:** `cargo clippy --workspace --all-targets --all-features --no-deps
  -- -D warnings` failed locally. Failures included core extractor warnings,
  test-target warnings in `writer_batching_contract`, and xtask test warnings.
- **Fix:** decide whether the full workspace is expected to be clippy-clean. If
  yes, run a one-time cleanup and add an all-target or staged clippy gate. If no,
  document exactly which targets are intentionally excluded and why.

## F14. SQL extraction still compiles regexes in hot paths

- **Status:** fixed
- **Severity:** medium
- **Where:** `crates/julie-extractors/src/sql/schemas.rs:142`,
  `crates/julie-extractors/src/sql/schemas.rs:176`,
  `crates/julie-extractors/src/sql/schemas.rs:449`,
  `crates/julie-extractors/src/sql/routines.rs:318`,
  `crates/julie-extractors/src/sql/routines.rs:359`
- **What exists:** shared SQL helpers already use `LazyLock<Regex>`, but several
  schema and error-recovery paths compile regexes inside extraction functions.
- **Why it matters:** SQL extraction is regex-heavy; compiling patterns per node
  adds avoidable CPU cost on SQL-heavy repositories.
- **Fix:** hoist stable patterns to `LazyLock<Regex>` and add a SQL-heavy perf
  fixture or tiny tripwire before broader SQL parser work.

## F15. Cargo repository metadata points at the wrong owner

- **Status:** fixed
- **Severity:** medium
- **Where:** `Cargo.toml:13`,
  `crates/julie-extractors/Cargo.toml:8`
- **What exists:** Cargo metadata uses `https://github.com/murphy/julie-extractors`
  while `origin` is `https://github.com/anortham/julie-extractors.git`.
- **Why it matters:** published crate metadata and downstream links can send
  consumers to the wrong repo.
- **Fix:** update workspace and crate repository metadata. Add a small metadata
  contract test if this repo is going to publish crates as well as binaries.

## F16. Stale docs can compete with current contracts

- **Status:** fixed
- **Severity:** low-medium
- **Where:** `docs/architecture/cli-contract.md:1`,
  `docs/contracts/cli.md:32`,
  `docs/release.md:123`
- **What exists:** `docs/architecture/cli-contract.md` is still labeled a draft,
  while the current CLI contract lives under `docs/contracts/cli.md`. Release
  docs currently point at v2.0.0 evidence even though newer release evidence is
  present.
- **Why it matters:** downstream readers and agents may follow stale docs instead
  of the current contract.
- **Fix:** either delete or clearly mark the architecture draft as superseded.
  Update `docs/release.md` to point at the current release evidence index.

## F17. Route literal lane is advertised but inactive

- **Status:** fixed
- **Severity:** low-medium
- **Where:** `languages/README.md:8`,
  `crates/julie-extractors/src/base/type_models.rs:126`,
  `crates/julie-extractors/src/language_policy.rs:118`,
  `languages/go.toml:26`
- **What exists:** docs and enum variants say literal carriers can produce
  `route` rows, but every language policy has `route = []`.
- **Why it matters:** endpoint route strings are captured by some extractors but
  cannot currently survive as route-classified rows.
- **Fix:** decide whether route literals are in scope. If yes, populate route
  carriers and add golden fixtures. If no, narrow the public wording and keep
  `Route` reserved/internal.

## F18. Extractor result handoff clones accumulated vectors

- **Status:** fixed
- **Severity:** low-medium
- **Where:** `crates/julie-extractors/src/base/extractor.rs:116`,
  `crates/julie-extractors/src/base/extractor.rs:158`,
  `crates/julie-extractors/src/base/extractor.rs:200`,
  `crates/julie-extractors/src/registry.rs:40`
- **What exists:** registry result construction calls getter methods that clone
  type-argument usages, literals, and pending relationships.
- **Why it matters:** large files with many captured literals or pending edges
  pay avoidable allocations at extraction result handoff.
- **Fix:** add consuming `take_*` or `into_results` APIs for result assembly and
  keep borrowing getters only for tests or read-only inspection.

## F19. Agent guidance sync is local-only

- **Status:** fixed
- **Severity:** low
- **Where:** `scripts/check-agent-doc-sync.sh:6`,
  `.github/workflows/ci.yml:21`
- **What exists:** the sync script exists, but CI does not run it.
- **Why it matters:** `AGENTS.md` and `CLAUDE.md` can drift in a future commit
  even though this repo treats byte-for-byte sync as a contract.
- **Fix:** add a CI step that runs `scripts/check-agent-doc-sync.sh`.

## Existing Items Still Valid

These were already tracked in `TODO.md` and remain valid:

- `cargo-deny` supply-chain/license/advisory gate is missing.
- standalone `md5` 0.7 needs an explicit keep-vs-migrate decision because it is
  used in production identity paths.

## Closure

The hardening findings from this review are closed. New follow-up work should
start from the remaining open TODO items or a fresh review pass.
