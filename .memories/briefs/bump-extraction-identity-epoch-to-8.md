---
id: bump-extraction-identity-epoch-to-8
title: Bump extraction identity epoch to 8
status: active
created: 2026-08-31T18:21:28.864Z
updated: 2026-08-31T18:29:50.712Z
tags:
  - epoch
  - store
  - csharp
  - miller
  - bre-16
---

## Goal

Bump `EXTRACTION_IDENTITY_EPOCH` from 7 to 8 so family stores re-extract C# after BRE-16.

## Why now

BRE-16 already maps explicit C# `internal` to `Visibility::Internal` (julie-extract 2.38.0 / miller pin 2.38.1). It did not bump the identity epoch. Store import reuses a completed `(path, content_hash, extraction_epoch)` identity, so miller `workspace full` cannot rewrite L1. Live miller family stores still have `internal` types stored as `private`.

## Constraints

- Do not add `record_struct_declaration` to C# `extract_symbol`. `extract_record` already handles `record struct`.
- Do not treat miller's leftover `.miller/symbols.db` as source of truth.
- Do not delete live `file_versions` to force a rewrite (`ON DELETE RESTRICT`).
- Do not bump miller's pin from this repo. Miller 1.26.0 stays on 2.38.1 / epoch 7 until the next extract release.
- Do not push, tag, or publish without approval.
- Keep the capability snapshot keyed to the new epoch. Epoch 7 already collided once (39 languages vs 40 / F#).
- Candidate crate version is 2.38.2 so published v2.38.1 (epoch 7) and this source do not share a version number.
- Ledger heading is `## 2.38.2`.

## Success criteria

- `EXTRACTION_IDENTITY_EPOCH == 8` (done in source).
- Tests, fixtures, and store-meta assertions that name the current epoch pass at 8 (done).
- Epoch 7 rows stay immutable.
- A new family-store import of an unchanged C# file at epoch 8 allocates a new `file_versions` row (existing identity contract; proven by `extraction_epoch_change_creates_a_new_version_for_unchanged_content`).
- No miller pin, tag, or extract publication in this slice.

## Status

Source bump is on `bump/extraction-epoch-8`. Remaining work is ship v2.38.2 and pin miller.

## References

- TODO.md session brief (Miller dogfood 2026-08-31)
- docs/plans/2026-08-31-extraction-identity-epoch-8.md
- docs/contracts/extraction-output-changes.md (`## 2.38.2`)
- Prior bump: commit a3073cbd
