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
the default while retaining an explicit forced-full escape hatch. Version 2.32.1 hardens long
artifact imports, bounds scope crossover before execution, reuses identical imports, and accelerates exact
resolution publication without changing public contracts. Version 2.33.0 keeps long batch work from being
rolled back at the quantum cap, frees a store wedged by an abandoned resolve claim, makes incremental vacuum
reclaim its whole page budget per call, and repairs three Windows defects: unknown process liveness, an
import that never retried a blocked drain, and a failed resolve that leaked its scratch database.
Version 2.33.1 repairs a fourth Windows defect that broke every scoped store resolution since 2.32.0: a
verbatim `\\?\` path prefix reached a SQLite URI and became an invalid authority, so a view could never
leave `converging`. Version 2.33.2 reused one coordinator
connection and retries transient `SQLITE_PROTOCOL` failures for the `coord.db`/`store.db` construction
reads plus the read-only reconcile, base-reader, scratch-reader, and `open_reader` paths; writer/lease
mutation opens remain non-retried. It samples lease time after `BEGIN IMMEDIATE`; renews leases with
transient-error retries and fencing-token checks; restores the single-changed-path scoped-delta
exemption; normalizes Windows diagnostic paths; and corrects the serial CI/zero-test guard. The
exact-tree release-prep gates were green. Version 2.33.3 is the current published release. It bounds
incremental resolution around changed state, reuses validated resolution proofs and fixed statements,
keeps unchanged imports on their no-op/idempotent paths, and hardens cross-platform root identity while
preserving the existing recovery and fencing boundaries. A cold full index remains expensive; the
release improves the bounded incremental path rather than promising fast first-time whole-repository
extraction. No public CLI, report, schema, or store contract version changes.
The implementation plans and dogfood records are:

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
- [Incremental-resolution crossover recovery](findings/2026-08-14-store-incremental-resolution-recovery.md)
- [v2.32.0 release notes](release-notes/v2.32.0.md)
- [v2.32.0 release evidence](release-evidence/2026-08-11-v2-32-0-release.md)
- [v2.32.1 release notes](release-notes/v2.32.1.md)
- [v2.32.1 release evidence](release-evidence/2026-08-12-v2-32-1-release.md)
- [v2.33.3 release notes](release-notes/v2.33.3.md)
- [v2.33.3 release evidence](release-evidence/2026-08-16-v2-33-3-release.md)
- [v2.33.2 release notes](release-notes/v2.33.2.md)
- [v2.33.2 coordinator and lease finding](findings/2026-08-13-coordinator-connection-reuse.md)
- [v2.33.3 incremental-resolution recovery finding](findings/2026-08-14-store-incremental-resolution-recovery.md)
- [v2.33.1 release notes](release-notes/v2.33.1.md)
- [v2.33.0 release notes](release-notes/v2.33.0.md)

## Evidence and historical material

- [Release-evidence index](release-evidence/README.md)
- `docs/plans/` records implementation plans; a completed plan is historical evidence, not the
  current public contract.
- `docs/archive/` and dated findings are historical context unless a current contract links them
  as authority.
