# Documentation

Julie Extractors owns parser-backed extraction and the standalone `julie-extract` process. Start
with the [CLI contract](contracts/cli.md), [report contract](contracts/reports.md), and
[testing strategy](testing-strategy.md).

## Current contracts

- [Extraction contract v4](contracts/extracted-data-v4.md)
- [SQLite artifact schema v6](contracts/sqlite-schema-v6.md)
- [JSONL v4](contracts/jsonl-v4.md)
- [Progress file v1](contracts/progress-file-v1.md)
- [Test evidence v1](contracts/test-evidence-v1.md)

## Versioned store

Ph2b, Ph2c, and Ph2d implement the [store v1 contract](contracts/store-v1.md),
[SQLite store schema v2](contracts/sqlite-store-schema-v2.md), and
[versioned-store architecture](architecture/versioned-index-store.md). The implementation is
published in v2.31.0 and patched through v2.31.4. Version 2.32.0 makes validated scoped resolution
the default while retaining an explicit forced-full escape hatch. The implementation
plans and dogfood records are:

- [Ph2b store-kernel plan](plans/2026-08-07-index-store-ph2b-store-kernel-plan.md)
- [Ph2b implementation evidence](release-evidence/2026-08-07-index-store-ph2b/README.md)
- [Ph2c resolution plan](plans/2026-08-08-index-store-ph2c-resolution-plan.md)
- [Ph2c implementation evidence](release-evidence/2026-08-08-index-store-ph2c/README.md)
- [Ph2d lifecycle design](plans/2026-08-08-index-store-ph2d-lifecycle-design.md)
- [Ph2d lifecycle plan](plans/2026-08-08-index-store-ph2d-lifecycle-plan.md)
- [Ph2d dogfood evidence](findings/2026-08-08-index-store-ph2d-dogfood.md)
- [v2.31.4 release notes](release-notes/v2.31.4.md)
- [v2.31.4 release evidence](release-evidence/2026-08-11-v2-31-4-release.md)
- [v2.31.3 release notes](release-notes/v2.31.3.md)
- [v2.31.2 release notes](release-notes/v2.31.2.md)
- [v2.31.1 release notes](release-notes/v2.31.1.md)
- [v2.31.0 release notes](release-notes/v2.31.0.md)
- [Concurrent fencing plan](plans/2026-08-10-store-concurrent-fencing-hardening.md)
- [Concurrent fencing evidence](evidence/2026-08-10-store-concurrent-fencing.md)
- [Store resolution performance repair plan](plans/2026-08-10-store-resolution-performance-repair.md)
- [Store resolution performance evidence](findings/2026-08-10-store-resolution-performance-repair.md)
- [Incremental-resolution dogfood and verification ledger](findings/2026-08-11-store-incremental-resolution-dogfood.md)
- [v2.32.0 release notes](release-notes/v2.32.0.md)
- [v2.32.0 release evidence](release-evidence/2026-08-11-v2-32-0-release.md)

## Evidence and historical material

- [Release-evidence index](release-evidence/README.md)
- `docs/plans/` records implementation plans; a completed plan is historical evidence, not the
  current public contract.
- `docs/archive/` and dated findings are historical context unless a current contract links them
  as authority.
