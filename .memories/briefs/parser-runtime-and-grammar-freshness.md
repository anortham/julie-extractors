---
id: parser-runtime-and-grammar-freshness
title: Parser runtime and grammar freshness
status: completed
created: 2026-07-14T11:02:13.140Z
updated: 2026-07-17T20:47:11.354Z
tags:
  - julie-extractors
  - tree-sitter
  - grammar-freshness
  - csharp14
  - swift
  - r
---

## Goal

Upgrade the shared Tree-sitter runtime and certify current C# 14, Swift, and R parser surfaces without changing Julie Extractors artifact contracts, then add a repeatable non-default grammar freshness report.

## Why Now

The T-SQL lane proved published grammar freshness is not a reliable proxy for live language support. Tree-sitter 0.26.11, unreleased upstream C# 14 support, Swift 0.7.3, and R 1.3.0 now require deliberate migration.

## Constraints

- Build on completed T-SQL commit `dbff11b8598e47eea867c1cc69484561b9877b3e`.
- Preserve zero Terraform SQL and Razor diagnostics and malformed T-SQL controls.
- Keep artifact contracts stable and network scans outside the default tier.
- Use an exact pushed owned C# grammar fork commit if upstream fixtures prove a grammar gap.
- Do not push, version, tag, publish, or release Julie Extractors without separate approval.

## Success Criteria

Runtime 0.26.11, C# 14/.NET 10 file apps, Swift 0.7.3, R 1.3.0, reviewed goldens/capabilities, deterministic freshness reporting, and all branch gates green.

## References

- `docs/plans/2026-07-14-parser-runtime-and-grammar-freshness-design.md`
- `docs/plans/2026-07-14-parser-runtime-and-grammar-freshness-implementation-plan.md`

## Status

Design locked; implementation plan and independent Claude review are next.
