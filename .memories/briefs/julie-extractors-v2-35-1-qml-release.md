---
id: julie-extractors-v2-35-1-qml-release
title: Julie Extractors v2.35.1 QML release
status: completed
created: 2026-08-24T18:27:16.335Z
updated: 2026-08-24T20:10:33.245Z
tags:
  - release
  - qml
  - qmldir
  - v2.35.1
---

## Goal

Publish v2.35.1 from clean `main` with the first-class QML and `qmldir` extraction work integrated.

## Why Now

QML support is now a first-class product requirement and Miller needs its focused continuous-testing targets in a published Julie Extractors release.

## Constraints

- Follow `docs/release.md` and the repository worktree/release discipline.
- Treat Windows as a first-class release target.
- Classify extraction-output changes and advance the extraction identity epoch when required.
- Preserve schema 7, JSONL v5, family-store schema 2, and store format epoch 1 unless release evidence proves a contract change.
- Run secrets and dependency scans before push.
- Push, tag, publish, and verify all four release assets under the user’s explicit approval.

## Success Criteria

- `main`, `origin/main`, and `v2.35.1` resolve to the intended verified release commit.
- Release gates, compatibility, packaging, Linux and Windows verification pass.
- GitHub release assets for Linux x86_64, macOS arm64/x86_64, and Windows x86_64 are published and verified.
- Release notes, evidence, current-release pointers, and Goldfish state are reconciled.

## References

- `docs/plans/2026-08-24-qml-first-class-extraction-implementation-plan.md`
- `docs/release.md`
