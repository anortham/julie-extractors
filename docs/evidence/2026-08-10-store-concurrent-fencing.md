# Evidence: Store Concurrent Fencing Hardening

## Verdict

PASS. Branch gate commands listed below all passed on the final verified tree for this evidence
file. Code tasks 1–8 landed earlier on `fix/store-concurrent-fencing`; Task 9 records architecture
and contract notes, this evidence file, and the implementation plan.

## Scope

Close multi-worktree writer races so concurrent import/resolve/maintain against one family store
cannot mutate a frozen source generation or leave unfenced durable effects.

Implemented behaviors (Tasks 1–8):

1. Foreign live maintenance intent blocks ordinary writer lease acquire even with no lease row.
2. Maintenance owner is explicit (`run_id` + `owner` + token) via `try_acquire_for_maintenance`.
3. Temporary raised `min_writer_version` does not permanently land on a published destination;
   `maintenance_tmp_*` mirrors; restore-before-clear on finish/abort.
4. Resolve never writes `store.db` unfenced; exact publish pre-`BEGIN IMMEDIATE` heartbeat; wall-clock
   publish fence (near-wall production path; synthetic fence clock domain for injected test clocks).
5. Pin release-on-failure; expired pins are not GC roots for base protection.
6. Enqueue/claim/cursor intent recheck inside IMMEDIATE transactions.
7. Maintenance apply re-probes live free space before mutative steps.
8. Import bases use building→ready CAS.

## Environment

| Field | Value |
|---|---|
| Worktree | `/home/murphy/source/julie-extractors/.worktrees/fix/store-concurrent-fencing` |
| Branch | `fix/store-concurrent-fencing` |
| Host | Linux prax 7.1.7-200.fc44.x86_64 x86_64 |
| Toolchain | rustc 1.97.1 (8bab26f4f 2026-07-14), cargo 1.97.1 |
| Gate window (UTC) | `2026-08-10T17:42:34Z` → `2026-08-10T17:43:47Z` |
| Base HEAD before Task 9 | `4a9fc04d69799b9494b466064b979c257894ff1d` |
| Gate hygiene commit | `f20141fddad8f49cccdf102940269f1fadb0f29b` |

## Implementation commits

| SHA | Summary |
|---|---|
| `aba9324` | refuse leases under foreign maintenance intent |
| `798147b` | raise source writer floor and normalize destination meta |
| `f788fdf` | validate writer leases against wall time |
| `0920177` | adapt resolution perf publish_exact heartbeat hook |
| `6d0c6f4` | fence resolve terminal log writes |
| `0623e8a` | release resolve pins and honor pin expiry |
| `f461c59` | re-probe capacity before maintenance apply |
| `d702a6a` | recheck maintenance intent inside coordinator writes |
| `cf9e943` | CAS import resolution bases |
| `4a9fc04` | validate writer leases in the fence clock domain |
| `f20141f` | gate fixture + clippy/fmt hygiene for branch gate |

Task 9 also landed gate hygiene discovered while running the branch gate:

- generation contract expired-lease fixture aligned with fence clock domain (`checked_at == expires_at`)
- clippy `explicit_auto_deref` / `too_many_arguments` / `redundant_closure` and `cargo fmt` cleanups

## Architecture and contract notes

- [`docs/architecture/versioned-index-store.md`](../architecture/versioned-index-store.md) — Concurrent
  fencing section: intent authority, M1–M7 floor/mirror state machine, resolve wall-clock publish,
  pin expiry, capacity re-probe, import CAS.
- [`docs/contracts/store-v1.md`](../contracts/store-v1.md) — Caller-visible intent blocking, in-txn
  coordinator rechecks, fenced resolve writes, wall-clock exact publish, unexpired pin roots,
  building→ready import bases, live free-byte re-probe.
- [`docs/plans/2026-08-10-store-concurrent-fencing-hardening.md`](../plans/2026-08-10-store-concurrent-fencing-hardening.md)
  — Normative plan with acceptance checkboxes marked complete.

## Branch gate ledger

Commands used (Task 9 / worker brief; mixed_version / equivalence suite not required for this gate):

```bash
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-artifact \
  --features test-store-crash \
  --test store_generation_crash_contract --test store_maintenance_crash_contract \
  --test store_crash_contract -- --test-threads=1

RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-artifact \
  --test store_coordinator_contract --test store_connection_contract \
  --test store_generation_contract --test store_maintenance_contract \
  -- --test-threads=1

RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-artifact \
  --features test-store-resolution \
  --test store_resolution_binding_contract --test store_resolution_base_contract \
  -- --test-threads=1

RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-cli \
  --features test-store-resolution-contract \
  --test store_resolution_contract --test store_import_contract --test store_resolution_adapters \
  -- --test-threads=1

RUSTUP_TOOLCHAIN=1.97.1 cargo clippy -p julie-extract-artifact -p julie-extract-cli -- -D warnings

RUSTUP_TOOLCHAIN=1.97.1 cargo fmt --check
```

| Command group | Result | Counts |
|---|---|---|
| Crash contracts (`store_crash`, `store_generation_crash`, `store_maintenance_crash`) | PASS | 13 + 4 + 8 |
| Artifact store contracts (coordinator, connection, generation, maintenance) | PASS | 26 + 58 + 8 + 18 |
| Artifact resolution contracts (base, binding; `test-store-resolution`) | PASS | 9 + 6 |
| CLI contracts (import, adapters, resolution; `test-store-resolution-contract`) | PASS | 31 + 18 + 13 |
| Clippy `-D warnings` (artifact + cli) | PASS | clean |
| `cargo fmt --check` | PASS | clean |

**Skipped (not in Task 9 branch gate):** `store_equivalence`, `store_mixed_version`,
`store_maintenance_mixed_version`, full workspace, real-world corpora.

## Success definition check

| Invariant | Evidence |
|---|---|
| Foreign live intent blocks ordinary writers with no lease row | Task 1 contracts + generation foreign-intent contract |
| Maintenance owner requires full intent identity | `try_acquire_for_maintenance` + connection contracts |
| Destination does not inherit temporary raised floor / tmp mirrors | Task 2 promote generation contract; store-v1 meta keys |
| Resolve never unfenced; wall-clock publish; pin expiry | Tasks 3–5 resolution contracts |
| Enqueue/claim/cursor in-txn intent recheck | Task 7 coordinator contracts |
| Apply re-probes free space | Task 6 maintenance contract |
| Import building→ready CAS | Task 8 import + adapter contracts |

## Deferred follow-ups

- Cooperative cancel of off-lease resolve CPU loops
- Unique scratch nonces for concurrent resolve scratch isolation
- Optional fold of resolve terminal append into the same IMMEDIATE transaction as `publish_exact`
- Miller Ph3 integration (out of product boundary here)

## Notes

- Feature flag for CLI resolution/import contracts is `test-store-resolution-contract` (not
  `test-store-contract`, which leaves those integration tests empty).
- Dual lease clock domains: production near-wall fences validate against wall time; synthetic
  historical `checked_at` values far from wall validate in the fence clock domain so injected test
  clocks stay coherent with `expires_at`.
