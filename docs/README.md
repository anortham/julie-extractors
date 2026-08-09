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

## Versioned store release candidate

Ph2b, Ph2c, and Ph2d implement the [store v1 contract](contracts/store-v1.md),
[SQLite store schema v2](contracts/sqlite-store-schema-v2.md), and
[versioned-store architecture](architecture/versioned-index-store.md). The implementation is
prepared as the v2.31.0 release candidate. Miller does not use it yet; Ph3 owns consumer wiring,
admission, and sidecar integration. The implementation plans and dogfood records are:

- [Ph2b store-kernel plan](plans/2026-08-07-index-store-ph2b-store-kernel-plan.md)
- [Ph2b implementation evidence](release-evidence/2026-08-07-index-store-ph2b/README.md)
- [Ph2c resolution plan](plans/2026-08-08-index-store-ph2c-resolution-plan.md)
- [Ph2c implementation evidence](release-evidence/2026-08-08-index-store-ph2c/README.md)
- [Ph2d lifecycle design](plans/2026-08-08-index-store-ph2d-lifecycle-design.md)
- [Ph2d lifecycle plan](plans/2026-08-08-index-store-ph2d-lifecycle-plan.md)
- [Ph2d dogfood evidence](findings/2026-08-08-index-store-ph2d-dogfood.md)
- [v2.31.0 release notes](release-notes/v2.31.0.md)

## Evidence and historical material

- [Release-evidence index](release-evidence/README.md)
- `docs/plans/` records implementation plans; a completed plan is historical evidence, not the
  current public contract.
- `docs/archive/` and dated findings are historical context unless a current contract links them
  as authority.
