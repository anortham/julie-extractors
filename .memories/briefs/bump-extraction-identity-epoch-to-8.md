---
id: bump-extraction-identity-epoch-to-8
title: Bump extraction identity epoch to 8
status: active
created: 2026-08-31T18:21:28.864Z
updated: 2026-08-31T20:01:52.227Z
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

BRE-16 already maps explicit C# `internal` to `Visibility::Internal`. It did not bump the identity epoch, so miller `workspace full` reused epoch-7 rows.

## Status

Published. julie-extract v2.38.2 is live at epoch 8 from `da93ea68`. Remaining work is miller's pin of 2.38.2. Miller 1.26.0 still pins 2.38.1 / epoch 7.

## Constraints

- Do not add `record_struct_declaration` to C# `extract_symbol`.
- Do not delete live `file_versions`.
- Do not pin miller from this repo.

## Success criteria

- `EXTRACTION_IDENTITY_EPOCH == 8` in published 2.38.2.
- Epoch 7 rows stay immutable.
- Miller pin is a separate miller change.

## References

- https://github.com/anortham/julie-extractors/releases/tag/v2.38.2
- docs/release-evidence/2026-08-31-v2-38-2-release.md
