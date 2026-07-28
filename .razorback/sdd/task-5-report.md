# Task 5 Report: Capability hygiene, docs, coverage artifact

## Status

**complete**

## Worktree

| Field | Value |
|---|---|
| Path | `/Users/murphy/.config/razorback/worktrees/julie-extractors/static-tier-ts-js` |
| Branch | `feature/static-tier-ts-js` |
| Base commit | `0194055c` (Task 4) |
| Commit SHA | `ba226c677462e524d6b820328d6f01335d587b6b` |

## What changed

### `fixtures/extraction/capabilities.json`

- `reference_resolution.version` **3 → 4**
- `metadata_keys.reference_resolution_version` documents contract **(4)**
- `tiers.tier3_static_type.fixture_proven_languages` → `csharp`, `typescript`, `javascript`
- Visibility notes: TS export → `public` (fixture-proven); C# `internal` still open
- Static reachability notes: prefer `isStatic` metadata, signature fallback; TS/JS proven
- Evidence paths expanded to TS/JS fixtures + negatives
- Hygiene: csharp `kind_coverage.identifiers.supported` adds `member_access` (present in goldens)

### Coverage artifact

- Regenerated `fixtures/extraction/reference-resolution-coverage.json` via
  `node scripts/reference-resolution-coverage-report.mjs --write --strict`
- `source_digest` refreshed to match capabilities

### Contract docs

- `docs/contracts/sqlite-schema-v4.md`: resolution version **4**; static-tier proven
  languages csharp/typescript/javascript; `isStatic` + signature reachability
- `docs/contracts/jsonl-v3.md`: example `reference_resolution_version` **4**

### TODO / plan

- `TODO.md` §16: multi-language static certification done (`RESOLUTION_VERSION = 4`);
  slice 4 C# locals still open; debt notes updated for TS visibility + allowlist
- `docs/plans/2026-07-28-static-tier-ts-js-certification.md`: tasks 1–5 acceptance ticks

## Files modified (ownership)

```
fixtures/extraction/capabilities.json
fixtures/extraction/reference-resolution-coverage.json
docs/contracts/sqlite-schema-v4.md
docs/contracts/jsonl-v3.md
TODO.md
docs/plans/2026-07-28-static-tier-ts-js-certification.md
.razorback/sdd/task-5-report.md
.memories/**
```

Not modified: resolver/extractor Rust sources.

## Verification

```bash
node scripts/language-data-quality-report.mjs --strict
# → silent_cells=0, quality_bar_debts=0

node scripts/reference-resolution-coverage-report.mjs --write --strict
node scripts/reference-resolution-coverage-report.mjs --strict
# → pass
```

## Acceptance criteria

| Criterion | Result |
|---|---|
| fixture_proven_languages includes csharp, typescript, javascript | pass |
| language-data-quality-report --strict silent_cells=0, quality_bar_debts=0 | pass |
| reference-resolution-coverage-report --strict | pass |
| TODO §16: static multi-lang done; slice 4 open | pass |
| Branch gate commands (above) | pass |

## Concerns

- Historical release notes (`v2.19.0`, release evidence) still say C#-only static tier;
  left alone per brief (no rewrite of historical release notes).
- Pre-existing dirty `.razorback/sdd/task-1-report.md` and `task-4-report.md` were
  not included in this commit.
- C# locals/params (slice 4) and C# `internal` remain open debt.
