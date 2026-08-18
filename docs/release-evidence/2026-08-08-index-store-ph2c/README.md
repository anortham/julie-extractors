# Index Store Ph2c Resolution Evidence

> **Historical release evidence.** The resolution write path this evidence
> records is retired. See [2026-08-18-resolution-write-path-retirement.md](../../decisions/2026-08-18-resolution-write-path-retirement.md).

Status: historical Ph2c slice evidence, captured 2026-08-08. Store resolution shipped in v2.31.0
and was hardened through v2.31.3. Miller still does not use the family-store resolution path in
production (Ph3).

## Revisions and isolation

- Task 12 runtime archive and dogfood base: `f8d8ca0`.
- Runtime-gap implementation: `4d52161`.
- Rust 1.95 Clippy compatibility cleanup: `4da5fda`.
- Performance rerun isolation: `21f5a8b`.
- Branch: `codex/index-store-ph2c`.
- Raw generated evidence: `target/task12/dogfood/run-f8d8ca0/` and
  `target/performance/store-resolution/`; neither is tracked.

Every source input was a disposable archive under `target/task12`. The Julie Extractors, Miller,
and comparison source checkouts remained unchanged.

## Branch gate

The final gate used the plan's exact commands, with the nonexistent artifact target
`store_resolution_contract` corrected to the three actual artifact contracts:

```bash
RUSTUP_TOOLCHAIN=1.97.1 cargo fmt --all -- --check
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p xtask
RUSTUP_TOOLCHAIN=1.97.1 cargo xtask test default
RUSTUP_TOOLCHAIN=1.97.1 cargo xtask test contract
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-artifact --features test-store-resolution --test store_resolution_schema_contract -- --test-threads=1
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-artifact --features test-store-resolution --test store_resolution_base_contract -- --test-threads=1
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-artifact --features test-store-resolution --test store_resolution_binding_contract -- --test-threads=1
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-cli --features test-store-resolution-contract --test store_resolution_contract -- --test-threads=1
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-cli --features test-store-resolution-contract --test store_resolution_adapters -- --test-threads=1
RUSTUP_TOOLCHAIN=1.97.1 cargo xtask performance store-resolution --runs 3
RUSTUP_TOOLCHAIN=1.97.1 cargo clippy -p julie-extract-artifact --all-targets --all-features -- -D warnings
RUSTUP_TOOLCHAIN=1.97.1 cargo clippy -p julie-extract-cli --all-targets --all-features -- -D warnings
cargo deny check
git diff --check
```

All commands passed. The first cold default run completed its tests but crossed the 90-second tier
budget; the immediate warm rerun passed. `cargo deny check` retained only the repository's existing
duplicate/wildcard notices. The performance command was deliberately rerun against its populated
default output directory. That exposed stale worker databases; the harness now resets only its
owned fixture/run children, preserves sibling files, and passes both clean and populated reruns.

Focused final counts included CLI resolution 10/10, adapters 18/18, coordinator 48/48, binding
4/4, base 8/8, schema/diff 18/18, and the exact hard-kill recovery case. The contract tier also
passed store crash 13/13 and all registered store import, operation, equivalence, mixed-version,
and resolution session contracts.

## Actual-store G3 matrix

`delta_write_ms` measures the real fenced store publication transaction. G3b uses
`(diff_ms + delta_write_ms) / resolution_compute_ms`; every sample must pass independently.

| Pair | Run | Resolve ms | Diff ms | Real store write ms | G3b ratio | RSS bytes | Exact ms |
|---|---:|---:|---:|---:|---:|---:|---:|
| mutated | 1 | 7,181 | 1,169 | 552 | 0.2397 | 185,286,656 | 13,609 |
| mutated | 2 | 7,353 | 1,197 | 570 | 0.2403 | 186,679,296 | 13,815 |
| mutated | 3 | 7,140 | 1,183 | 572 | 0.2458 | 186,597,376 | 13,631 |
| unchanged | 1 | 7,249 | 383 | 2 | 0.0531 | 20,709,376 | 12,848 |
| unchanged | 2 | 7,380 | 365 | 2 | 0.0497 | 21,331,968 | 12,503 |
| unchanged | 3 | 7,182 | 383 | 2 | 0.0536 | 21,331,968 | 12,318 |

All six rows passed G1, G2, G3a, G3b, G3c, G4, and G5; the G3b ceiling is 0.50.

## Public dogfood

The public binary ran one family with two views through Full import, resolve, content update,
multi-delete, failed-preserved input, path reuse, exactness invalidation, and reconvergence. A
second fresh family independently imported and resolved the final roots.

Representative public commands were:

```bash
target/release/julie-extract store import --store "$STORE" --family "$FAMILY" --root "$ROOT" --view "$VIEW" --level full --request-id "$REQUEST" --idempotency-key "$REQUEST" --json
target/release/julie-extract store update --store "$STORE" --family "$FAMILY" --root "$ROOT" --view "$VIEW" --file "$FILE" --level full --request-id "$REQUEST" --idempotency-key "$REQUEST" --json
target/release/julie-extract store delete --store "$STORE" --family "$FAMILY" --root "$ROOT" --view "$VIEW" --file "$FILE" --request-id "$REQUEST" --idempotency-key "$REQUEST" --json
target/release/julie-extract store resolve --store "$STORE" --family "$FAMILY" --view "$VIEW" --request-id "$REQUEST" --idempotency-key "$REQUEST" --json
target/release/julie-extract store export --store "$STORE" --family "$FAMILY" --view "$VIEW" --output "$OUTPUT" --json
```

The final normalized comparator covered 21 visible groups per view, including current manifest
identity and entries, file/version payload, all extraction child/global families, and both
resolution tables. Both incremental views matched their independently extracted and resolved fresh
views with zero mismatches.

The larger Miller-shaped fixture contained 1,914 files, 418,269 identifiers, and 4,658 pending
rows. Its first Full import took 82.07 seconds with 266,174,464 peak RSS; the second view reused all
versions in 6.71 seconds. The first resolve produced one 93,298,688-byte base in 370.94 seconds with
21,692,416 peak RSS; the second view reused that base in 0.95 seconds. The real-repository lookup
time is accepted correctness evidence, not a Ph2c performance claim; reducing it is a Ph3 task.

## Crash, takeover, and integrity

The resolver was killed externally after durable progress. Replay of the same request and
idempotency key removed stale `.work`/WAL/SHM state, reclaimed the dead claim, and committed exactly
one terminal resolution effect. The recovered request had one terminal, one effect, no duplicate
chunks, a committed coordinator row, and no active lease. Scratch directories were empty after
completion.

The incremental coordinator contained 26 committed requests, one intentionally failed request from
the pre-fix crash attempt, and zero nonterminal requests. Duplicate terminal, chunk, manifest, and
resolution-effect counts were zero. Completion ordering was valid. Every incremental and fresh
`store.db` and `coord.db` returned `quick_check=ok` and zero foreign-key violations. The largest
observed import WAL was 82,432,992 bytes; the largest resolution scratch WAL was 43,997,512 bytes.

## Scope boundary

This evidence closes Ph2c implementation and verification only. It does not publish a release,
change a package version, push a branch, or claim Miller adoption. Ph2d remains responsible for
base-root retention, pins, claims, GC, repair, and promotion. Ph3 remains responsible for the
measured large-repository resolver optimization.
