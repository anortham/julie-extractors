# Release-Blocker Review Fixes Plan

**Goal:** Resolve the validated release-blocking and high-priority findings from
`docs/findings/CC_REVIEW.md` before deciding whether to publish `v0.1.0`.

**Product boundary:** Keep this repo scoped to `source tree -> versioned
extraction artifact`. Do not add Julie MCP, daemon, search, embedding, watcher,
dashboard, or editing behavior.

## Scope

1. Fix the shared scan correctness root cause behind F1, F3, F11, and F28.
2. Fix symlink discovery policy from F9.
3. Replace static parser/capability fingerprints from F12/F13.
4. Make lint guardrails real enough for the release candidate: workspace lint
   inheritance from F19 and a CI clippy gate from F20.

## Design

### Partial scan model

- `scan` should keep processing other files when one supported file cannot be
  read or extracted.
- Successfully extracted files must be committed into the artifact.
- Failed files must be represented with `FileStatus::FailedPreserved`, at least
  one parse diagnostic/error row, and report errors.
- A mixed scan returns `status=partial`, exit code `1`, and non-zero
  `counts.files_failed`.
- A clean, intentionally empty supported file remains `FileStatus::Indexed` and
  is allowed to replace old rows with zero symbols.
- `failed` remains for pre-write failures where no useful artifact update can
  be committed.

### Discovery symlink policy

- Discovery must not follow directory symlinks outside the scan root.
- Symlink loops must not recurse indefinitely.
- If a symlink path is skipped, the scan should remain deterministic and avoid
  indexing out-of-root files.

### Metadata fingerprints

- `parser_inventory_fingerprint` must be derived from the parser inventory rows
  that are written to the artifact.
- `capability_snapshot_fingerprint` must be derived from the capability snapshot
  rows written to the artifact.
- Fingerprints must be deterministic and contract-shaped as `sha256:<hex>`.

### Lint guardrails

- Member crates must inherit workspace Rust lints so `unsafe_code = "forbid"`
  is active.
- CI must run a clippy gate. If the existing test warning baseline is too large
  for `--all-targets -D warnings`, the release-candidate gate may start with
  production library/binary targets and document the deliberate exclusion.
- The current release-candidate clippy gate is deliberately scoped to
  `julie-extract-artifact`, `julie-extract-cli`, and `xtask` with `--no-deps`.
  The extractor engine crate still inherits workspace Rust lints, but its
  broader clippy cleanup remains outside this release-blocker slice because the
  ported extractor modules currently produce hundreds of style warnings.

## Acceptance Criteria

- [x] CLI test proves an intentionally empty changed file replaces stale rows.
- [x] CLI test proves one invalid UTF-8 supported file yields partial scan while
      committing another valid file.
- [x] CLI or discovery test proves out-of-root symlinked files are not indexed.
- [x] Metadata fingerprint tests prove the values are not static placeholders
      and change when their input rows change.
- [x] Workspace lint inheritance is enforced by manifest or convention test.
- [x] CI includes a clippy gate that can pass on the current tree.
- [x] `cargo xtask test default` passes.
- [x] `cargo xtask test contract` passes.
- [x] `scripts/check-agent-doc-sync.sh` passes if guidance docs are touched.

## Non-Goals

- Do not fix every medium/low CC review item in this slice.
- Do not change JSONL/SQLite schema versions unless a contract fix makes that
  unavoidable.
- Do not make dogfood, certification, real-world, or release packaging part of
  the default suite.
