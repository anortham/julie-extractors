> **Superseded 2026-08-18.** The resolution write path is retired. See [2026-08-18-resolution-write-path-retirement.md](../decisions/2026-08-18-resolution-write-path-retirement.md).

# Index Store Ph2d Dogfood

Date: 2026-08-09
Branch: `codex/index-store-ph2d`
Candidate: `julie-extract 2.31.0`
Status: local release preparation; no push, tag, publication, or Miller pin change

## Workload

The disposable run used one family with two views over independent copies of the full 38-language
extraction fixture. Both roots carried an identical same-path source to prove cross-view immutable
version reuse. The run exercised L1 and Full import, ten paired content-changing Full updates, ten
deletes, delete/re-add reuse, classified `failed` and `failed_preserved` entries, exact resolution,
reader pins, request receipts, consumer cursor advance/release, historical churn, all GC levels,
repair, killed promotion/retry, forward rollback, and post-rollback convergence.

Generated databases and logs remain ignored under `target/ph2d-dogfood/run-BatqR9`. Source fixtures
and both repository checkouts remained clean.

## Timing and resource evidence

| Operation | Result | Time | Peak RSS / WAL |
| --- | --- | ---: | ---: |
| view A L1 import | committed | 13.07 s | recorded in run log |
| view B L1 import | committed, shared versions reused | 13.65 s | recorded in run log |
| view A Full | committed | 28.47 s | 182,059,008 B max observed import RSS |
| view B first Full | honest lease-safe quantum refusal | 29.18 s | no corrupt state |
| view B Full retry, 8 versions/quantum | committed | 18.57 s | retained L1 reused |
| killed promotion retry | recovered `gen-002` | 20.42 s | 24,444,928 B RSS |
| GC apply | bounded cohorts | 1.74 s | 34,635,776 B RSS; 3,015,872 B GC WAL |

The workload-wide observed WAL peak was 82,432,992 bytes. The externally killed promotion exited
137 after a durable manifest flip. Both databases reopened cleanly; retry reused the same durable
request, completed exactly one terminal effect, and published one recovered generation.

## Lifecycle facts

- GC removed historical manifests/deltas/bases only after their roots expired, demoted L3 before
  L2, purged ten whole unrooted versions in the initial run, and preserved current/shared versions.
- A live reader pin protected its manifest/base/delta until expiry. A consumer cursor advanced to a
  valid generation/log sequence, blocked unsafe pruning while present, and released cleanly.
- An aged terminal no-op request was archived into one immutable receipt before its live request and
  two owned log rows were pruned.
- Public healthy repair completed through checkpoint recovery. A dangling `CURRENT` naming a missing
  generation refused rather than fabricating an empty store; the pointer was restored without
  mutation.
- Promotion was killed after the destination reached about 298,860,544 bytes. Immediate retry took
  over the dead maintenance owner before TTL expiry and recovered the partial generation.
- Forward rollback selected retained `gen-001` while preserving the latest 509 immutable versions,
  1,947 log identities, receipts/cursors, and allocator state in new `gen-003`. Re-importing view A
  then reused its already-retained current manifest and restored fresh-store equivalence.

Two dogfood defects were fixed with RED/GREEN contracts: GC now deletes a retired manifest's child
entries before its restrictive parent row, and a confirmed-dead maintenance owner can be replaced
before expiry while unknown PID liveness remains conservative. A third release-blocking defect was
found after promotion: ordinary request commits did not advance root-owned allocator marks. The
coordinator now advances all four identity families with progress/terminal reconciliation; a public
post-promotion update regression and a public resolution-delta regression cover it.

## Final persisted evidence

After forward rollback and source reconvergence:

- `CURRENT = gen-003`; all `gen-001`, `gen-002`, and `gen-003` store catalogs plus `coord.db` report
  `quick_check=ok` and zero foreign-key violations.
- 509 immutable file versions remain; highest issued `version_id` is 519.
- view A points to manifest 42 with allocator high-water 43; view B points to manifest 4 with
  high-water 4. Reusing older immutable manifest 42 after rollback does not lower the allocator.
- store-log high-water is 2,014. Before receipt pruning there were 53 distinct terminal requests,
  zero duplicate terminal rows, and 351 unique request chunks with zero duplicates.
- the public resolve path advanced view A's resolution-delta high-water to 1.
- one aged request became a receipt before request/log deletion; the final disposable cursor was
  released, leaving 52 committed requests and 52 distinct terminal rows. No nonterminal request,
  duplicate request chunk, writer lease, maintenance intent, or consumer cursor remained.
- six revisions of the shared same-path source were retained and referenced by both views; both
  current views reused the same current immutable version.
- current visible rows equal independent fresh Full imports across 21 normalized groups per view:
  616,159 rows for view A, 616,147 for view B, `mismatch_count = 0`.
- retained store sizes were 296,068 KiB (`gen-001`), 295,636 KiB (`gen-002`), and 295,320 KiB
  (`gen-003`); the fresh comparator store was 295,936 KiB.

## Verification boundary

Focused generation, maintenance, crash, public promotion, coordinator, and resolution contracts are
green. The warm default tier, contract tier, standalone crash/equivalence gates, strict all-feature
Clippy, formatting, and diff checks are green. The first cold default run completed every test but
tripped only its 90-second wall-clock guard; the immediate warm rerun passed.

The local aarch64 Apple candidate package contains the store contract, schema, architecture, and
release note. Its archive is 13,708,190 bytes with SHA-256
`aa7881b145ef2aa7fcaef70716c4cf31fb078291758696e3e43a1295f7fbff84`; the packaged binary reports
2.31.0 and has SHA-256 `606ba4711f2189c23600530ddf99d4abf913c988b1b723d19d78eb6b8b8e12b6`.
GitHub publication and downloaded public release-asset verification are not claimed by this local
dogfood run.
