# Index Store Ph2b Implementation Evidence

Status: historical Ph2b slice evidence, captured 2026-08-08. The versioned family store later
shipped in v2.31.0 and was hardened through v2.31.3. Miller still does not use this store in
production (Ph3).

## Revisions and isolation

- Required Task 10 base: `20c77aa11be8d6cc339c1a1bbbc97f65bfd07b95`.
- Runtime and Julie Extractors archive: `6a61b6e8832ab935830cd8bd0e1a19aa6f57f7a6`.
- Miller archive: `b7df7db2f775657912c90df5067ceb7fee985db0`.
- Branch: `codex/index-store-ph2`.
- Raw evidence: `target/task10/`; no generated database or log is tracked.

The two input roots were made with `git archive`, then changed only inside
`target/task10/dogfood/run-6a61b6e/roots`. The original Julie Extractors and Miller repositories
remained clean. Both disposable roots received the same `dogfood/shared.rs` path and content so
cross-view immutable-version reuse could be measured without changing either source repository.

## Branch and feature gates

The exact gate was:

```bash
RUSTUP_TOOLCHAIN=1.97.1 cargo fmt --check
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p xtask
RUSTUP_TOOLCHAIN=1.97.1 cargo xtask test default
RUSTUP_TOOLCHAIN=1.97.1 cargo xtask test contract
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-artifact --features test-store-crash --test store_crash_contract -- --nocapture
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-cli --features test-store-contract --test store_equivalence -- --nocapture
```

All commands passed on the final tree. The first cold default run completed every test but exceeded
the tier's 90-second wall-clock budget; its immediate warm rerun passed. The initial exact
default-parallel crash command exposed a test-fixture path collision. Commit
`6a61b6e8832ab935830cd8bd0e1a19aa6f57f7a6` made those test roots collision-proof; the exact
command then passed 11/11 three consecutive local runs and six independent review runs. The CLI
equivalence feature command passed 6/6.

## Dogfood commands

The release binary and disposable roots were prepared as follows:

```bash
RUSTUP_TOOLCHAIN=1.97.1 cargo build --release -p julie-extract-cli --bin julie-extract
DOGFOOD_RUN=/Users/murphy/source/julie-extractors/.claude/worktrees/index-store-ph2/target/task10/dogfood/run-6a61b6e
JULIE_SOURCE=/Users/murphy/source/julie-extractors/.claude/worktrees/index-store-ph2
MILLER_SOURCE=/Users/murphy/source/miller
mkdir -p "$DOGFOOD_RUN/roots/julie" "$DOGFOOD_RUN/roots/miller"
git -C "$JULIE_SOURCE" archive 6a61b6e8832ab935830cd8bd0e1a19aa6f57f7a6 | tar -x -C "$DOGFOOD_RUN/roots/julie"
git -C "$MILLER_SOURCE" archive b7df7db2f775657912c90df5067ceb7fee985db0 | tar -x -C "$DOGFOOD_RUN/roots/miller"
```

These were the public command shapes; each request used the same value for `--request-id` and
`--idempotency-key`, and every report used `--json`:

```bash
target/release/julie-extract store import --store "$DOGFOOD_RUN/store" --family c095f60c-5655-47a4-8af6-c24e85b15d00 --root "$ROOT" --view "$VIEW" --level "$LEVEL" --request-id "$REQUEST" --idempotency-key "$REQUEST" --json
target/release/julie-extract store update --store "$DOGFOOD_RUN/store" --family c095f60c-5655-47a4-8af6-c24e85b15d00 --root "$ROOT" --view "$VIEW" --file dogfood/shared.rs --level full --request-id "$REQUEST" --idempotency-key "$REQUEST" --json
target/release/julie-extract store delete --store "$DOGFOOD_RUN/store" --family c095f60c-5655-47a4-8af6-c24e85b15d00 --root "$ROOT" --view "$VIEW" --file "$FILE" --request-id "$REQUEST" --idempotency-key "$REQUEST" --json
```

The killed request, its exact replay, and the fresh imports used these commands:

```bash
MILLER_STORE_CHUNK_VERSIONS=0 target/release/julie-extract store import --store "$DOGFOOD_RUN/store" --family c095f60c-5655-47a4-8af6-c24e85b15d00 --root "$DOGFOOD_RUN/roots/julie" --view julie --level full --jobs 1 --request-id dogfood-killed-batch --idempotency-key dogfood-killed-batch --json
kill -9 95846
MILLER_STORE_CHUNK_VERSIONS=0 target/release/julie-extract store import --store "$DOGFOOD_RUN/store" --family c095f60c-5655-47a4-8af6-c24e85b15d00 --root "$DOGFOOD_RUN/roots/julie" --view julie --level full --jobs 1 --request-id dogfood-killed-batch --idempotency-key dogfood-killed-batch --json
MILLER_STORE_CHUNK_VERSIONS=8 target/release/julie-extract store import --store "$DOGFOOD_RUN/fresh-store" --family c095f60c-5655-47a4-8af6-c24e85b15d01 --root "$DOGFOOD_RUN/roots/julie" --view julie --level full --request-id dogfood-fresh-julie-full --idempotency-key dogfood-fresh-julie-full --json
MILLER_STORE_CHUNK_VERSIONS=8 target/release/julie-extract store import --store "$DOGFOOD_RUN/fresh-store" --family c095f60c-5655-47a4-8af6-c24e85b15d01 --root "$DOGFOOD_RUN/roots/miller" --view miller --level full --request-id dogfood-fresh-miller-full --idempotency-key dogfood-fresh-miller-full --json
```

Initial requests were `dogfood-julie-l1`, `dogfood-miller-l1`,
`dogfood-julie-full`, and `dogfood-miller-full`. The first Miller Full request honestly failed
with `busy` after its scheduling quantum exceeded the lease-safe bound. Retrying as the new request
`dogfood-miller-full-retry` with `MILLER_STORE_CHUNK_VERSIONS=8` committed the retained L1 work.

The pre-merge review connected that honest failure to two recovery defects: an overrun was terminal
instead of resumable, and each successor rebuilt the chunk schedule from its own environment. The
repair requeues an overrun without committing its transaction, freezes chunk limits in each durable
request, and makes 8 versions the default Full-deepening quantum. Re-running an untuned Full import
against the same disposable Miller root committed 1,534 versions through L1, L2, and L3 in 207
request chunks, with one terminal effect, no coordinator error, no remaining lease, and clean
`quick_check`/foreign-key results for both databases.

The 20 mixed requests were:

| View | Requests | Paths | Resulting generations |
|---|---|---|---:|
| Julie | `dogfood-update-julie-v2` through `-v6` | `dogfood/shared.rs`, content v2-v6 | 2-6 |
| Miller | `dogfood-update-miller-v2` through `-v6` | `dogfood/shared.rs`, identical content v2-v6 | 2-6 |
| Julie | `dogfood-delete-julie-01` through `-05` | `TODO.md`, `RAZORBACK.md`, `deny.toml`, `languages/gdscript.toml`, `xtask/tests/python_example_contract.rs` | 7-11 |
| Miller | `dogfood-delete-miller-01` through `-05` | `README.md`, `TODO.md`, `src/Miller.Indexing/MarkerFactReader.cs`, `src/Miller.Indexing/CloneGroupReader.cs`, `src/Miller.Indexing/TestLinkageReader.cs` | 7-11 |

All 20 commands committed. The store retained six immutable `dogfood/shared.rs` versions, all six
hashes were used by both views, and both current manifests reference version `3417`. Delete removed
only current manifest entries; 11 targeted immutable path versions remained retained.

## Kill, takeover, and reconciliation

Twenty-four `dogfood/batch_00.rs` through `dogfood/batch_23.rs` fixtures were added to the disposable
Julie root. The public process ran a Full import with request/idempotency key
`dogfood-killed-batch`, `MILLER_STORE_CHUNK_VERSIONS=0`, and one extraction job. An external monitor
polled the durable manifest effect and WAL size, then sent `kill -9` to PID `95846`.

The child exited `137` after a manifest flip. Store and coordinator `quick_check` returned `ok` and
both foreign-key checks returned zero rows. Generation 12 and one manifest effect were durable,
while the terminal row was absent; 1,895 chunks had no duplicate index. The coordinator still
recorded owner `cli-95846`, PID `95846`, and fencing token `1786189268661`.

Repeating the exact public request with the same idempotency key took over the dead holder,
reconciled/resumed, and committed in 20.42 seconds. The final facts were one manifest effect, one
terminal row, 3,789 distinct chunks, no duplicate chunk index, valid completion ordering, committed
coordinator state, and no surviving writer lease. The killed process's WAL peak was 980,616 bytes.
The successor completed before a second lease snapshot could capture its holder PID or fencing
token, so no successor-token value is claimed. Durable takeover proof is the dead owner's claimed
coordinator row before replay, followed by the same request's single terminal store row, committed
coordinator row with cleared claim owner, continued chunk sequence, and absent lease after replay.

## Timings, generations, and equivalence

| Operation | Real time | Manifest result |
|---|---:|---|
| Julie L1 import | 13.07s | generation 1 created |
| Miller L1 import | 13.65s | generation 1 created |
| Julie Full deepen | 28.47s | generation 1 reused |
| Miller Full first attempt | 29.18s | honest `busy`, generation 1 retained |
| Miller Full retry, chunk size 8 | 18.57s | generation 1 reused |
| Killed-batch takeover/reconcile | 20.42s | Julie generation 12 committed |
| Fresh Julie Full | 66s | generation 1 created |
| Fresh Miller Full | 89s | generation 1 created |

The largest observed WAL was 82,432,992 bytes during the fresh Miller Full import (fresh Julie:
31,645,752 bytes). Final incremental generations were Julie 12 and Miller 11. The incremental store
contained 3,441 immutable versions.

The fresh comparator included the current manifest hash; current manifest path/status/observed
hash/error payload; current indexed file payload with completion-presence booleans; all 14 child
tables; and four extraction-global tables. It excluded generation, request, timestamp, surrogate,
and log-sequence bookkeeping. All 21 groups matched for each view: zero mismatches across 42
view-groups. Representative current counts were 1,895 files and 203,190 symbols for Julie, and
1,534 files and 217,927 symbols for Miller.

Final integrity facts:

- Incremental and fresh `store.db`/`coord.db`: `quick_check=ok`, zero foreign-key violations.
- Incremental coordinator: 26 total requests, 25 committed, one honest failed request, zero
  nonterminal requests.
- Zero duplicate terminal requests, zero duplicate chunk indexes, zero invalid completion order.
- Fresh coordinator: two requests, both committed; zero duplicate terminal requests.
- Zero visible-row equivalence mismatches.
