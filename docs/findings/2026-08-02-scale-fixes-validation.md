# 2026-08-02 — Scale-fixes branch validation (dotnet/runtime, hermes-agent, Miller)

**Scope:** T6 of [`docs/plans/2026-08-02-scale-fixes-plan.md`](../plans/2026-08-02-scale-fixes-plan.md).
Validates branch `scale-fixes` @ `9a2fd23` against the three standing repro targets and runs the
branch gates. Evidence only — no code on this branch was changed by T6.

**Verdict in one line:** the branch turns dotnet/runtime from *cannot be indexed at all* into a
complete, committed 22.8 GiB artifact — the four aborts are gone — but the write phase is now a
**scaling wall** (3 h 51 m, 30.6 GiB peak RSS) and 28 C files still trip the demoted identity guard.
The scan is correct; it is not yet practical at 58k files.

**How to read that verdict.** Slow-but-correct is a finding, not a failure. 2.21.0 could not complete
this scan at all, at any stack size; this branch completes it and commits a consistent artifact. The
five planned defects are assessed on their own terms in
[Per-defect verdict](#per-defect-verdict), and four are cleanly fixed. The write-phase wall is a
**newly promoted successor target** (T7), not a regression introduced by this branch — no prior
version ever got far enough to measure it.

## Method

| Field | Value |
|---|---|
| Machine | Apple M2 Ultra, 24 cores, 64 GB, macOS 26.6 |
| Binary | release build of `scale-fixes` @ `9a2fd23`; `julie-extract --version` = `2.21.0` (crate versions deliberately not bumped on the branch) |
| Tree validated | `9a2fd23`. The branch has since advanced to `7f002f2`, which adds only the T7 section to the plan document — `git diff 9a2fd23..7f002f2` touches one `docs/plans/` file, so the validated binary is code-identical to branch HEAD |
| Toolchain | every cargo command run with `RUSTUP_TOOLCHAIN=1.97.1` |
| Scan argv | `scan --root <target> --db <scratch> --jobs 4 --json` (Miller's shape) |
| Stacks | `RUST_MIN_STACK` verified **unset** in the environment and explicitly `unset` in the runner before every scan |
| Isolation | every source root read-only; DB and `TMPDIR` under a session scratchpad; one scan at a time |
| Instrumentation | `/usr/bin/time -l` for wall/RSS; a 50 ms sampler over `$TMPDIR/julie-extract-scan-spool-*` for peak spool bytes (T4's methodology, since the spool is deleted on success) |

Targets:

- **V1** dotnet/runtime @ `a2f953fe266`, 58,500 tracked files, 913 MB checkout.
- **V2** `~/.hermes/hermes-agent` @ `f228e145b`, 7,700 tracked files; `scripts/install.ps1` is 3,830
  lines of one-line PowerShell functions — the exact T3 flavor-1 trigger.
- **V3** Miller repo @ `814f9ccb` (clean), 2,239 tracked files.

## Gate ledger

| Scope | Invariant it proves | Command | Result | Time |
|---|---|---|---|---|
| V1 dotnet/runtime | 58k-file repo produces a complete committed artifact at default stacks; no abort of any kind | `scan --root dotnet-runtime --jobs 4 --json` | **exit 1 / `status: partial`** — 8 per-file `read_failed` (non-UTF-8 sources) force exit 1 **by design**; no fatal error, artifact committed | 13,889.65 s (3 h 51 m 30 s) |
| V1 identity guard | T3's root fixes leave zero payload conflicts | same run, `warnings[]` | **FAIL vs plan** — 28 files, 4,237 conflicts, **all `language: c`**; import committed (first-write-wins held) | — |
| V1 recursion guard | GitHub_10215.cs no longer aborts; capping is visible | same run + `parse_diagnostics` | **PASS** — file indexed; 3 `depth_truncated` rows, all C# | — |
| V2 hermes-agent | PowerShell one-line-function flavor emits no conflict, at default stacks | `scan --root ~/.hermes/hermes-agent --jobs 4 --json` | **PASS** — exit 0, `status: ok`, 0 errors, **0 conflict warnings** | 173.10 s |
| V3 Miller repo | branch keeps the small-repo cold-start win | `scan --root ~/source/miller --jobs 4 --json` | **PASS** — exit 0, `status: ok`, 0 errors, 0 warnings | 22.26 s |
| Branch gate | whole workspace compiles and every test passes | `cargo test --workspace` | **PASS** — 3,709 passed / 0 failed / 7 ignored across 36 test binaries | — |
| Branch gate | end-to-end scan → rescan → info → export contract holds on this repo | `cargo xtask dogfood repo --root . --out-dir target/dogfood/julie-extractors` | **PASS** — exit 0; 1,676,716 JSONL records, rescan 860 ms with 1,794 unchanged / 0 changed | — |
| Branch gate | no lint regressions | `cargo clippy --workspace --all-targets --no-deps -- -D warnings` | **PASS** — exit 0 | — |
| Branch gate | formatting stable under 1.97 | `cargo fmt -- --check` | **PASS** — exit 0 | — |

`cargo test --workspace` is a superset of the `docs/release.md` branch gates `cargo test -p xtask`,
`cargo xtask test default` and `cargo xtask test contract` at the test-binary level. The specialist
tiers (`certification`, `real-world-smoke`, `real-world-release`) were not run — no parser pin,
`language_spec`, or `registry` path changed on this branch, which is the documented trigger for them.

The `cargo test`, clippy and dogfood gates ran while V1 was still in its write phase. They are
pass/fail gates, so contention cannot change their verdict; only their durations are unreliable, and
no duration from them is quoted here. V2 and V3 were run alone, after V1 finished, so their timings
are clean.

## V1 — dotnet/runtime, the decisive gate

### Outcome

| Field | Value |
|---|---|
| Exit / status | `1` / `partial` |
| Fatal errors | **none** |
| Errors | 8, all `read_failed`, all `recoverable: true` |
| Wall clock | 13,889.65 s real / 9,116.99 s user / 2,899.81 s sys (3 h 51 m 30 s) |
| Peak RSS | **32,855,687,168 B = 30.60 GiB** (peak memory footprint 39,270,399,752 B = 36.57 GiB) |
| Final DB | **24,524,300,288 B = 22.84 GiB** (5,987,378 pages × 4 KiB) |
| WAL at finish | **0 bytes**; `journal_mode` back to `wal` — the bulk-load pragma switch restored and checkpointed correctly |
| Peak spool | **3,408,895,037 B = 3.18 GiB** |
| Spool residue | none — `TMPDIR` empty at exit |
| Files | 58,366 scanned / 41,406 changed / 16,960 unsupported / **8 failed** |
| Instructions retired | 48.94 T |

Exit 1 is **not** a branch failure. `commands.rs:419` sets `exit_code = 1` whenever
`has_source_errors` is true, and `commands.rs:316` sets `status = partial` on the same condition.
dotnet/runtime ships 8 files that are not valid UTF-8, so **exit 0 is unreachable on this corpus for
any version of the extractor**. The plan's "exit 0, zero errors" wording should be read as "no fatal
error; per-file source errors only" — which is what this run delivers.

The 8 failures are exactly the per-language counts the 2.21.0 baseline recorded (1 C#, 2 PowerShell,
5 XML), and all 8 are `read_failed` on invalid UTF-8 — not parse failures, not panics:

| Language | File |
|---|---|
| csharp | `src/libraries/System.Memory/tests/ReadOnlySpan/Count.T.cs` (invalid UTF-8 at index 15120) |
| powershell | `.../System.Net.Sockets/tests/Scripts/ClearReuseUnicastPort.ps1`, `SetReuseUnicastPort.ps1` |
| xml | `Russian_problem_chars.xml`, `XQL_Orders_j1.xml`, `XQL_Orders_j3.xml`, `xql_orders-flat-200a.xml`, `ReaderWriter_C14N_BaselineXML_Binary.xml` |

**The "swallowed-panic C# file" question is answered and the premise was wrong.** The baseline's
single C# failure is `Count.T.cs`, and it fails because the file is not UTF-8 — it is **not** a
generated-JIT pathological shape and there was never a swallowed panic behind it. T1's rendering plus
the JSON report is what made this legible; on 2.21.0 the whole run printed the word `failed`.

### Phase profile

| Phase | ms | share of total |
|---|---|---|
| `discovery` | 5,128 | 0.04% |
| `extraction_spool` | 213,247 | 1.54% |
| **`artifact_write`** | **13,661,797** | **98.42%** |
| ├ `artifact_write_plan` | 10,686 | 0.08% |
| ├ `artifact_write_file_symbol_insert` | 3,823,466 | 27.55% |
| ├ `artifact_write_child_rows` | 1,031,208 | 7.43% |
| ├ `artifact_write_resolution` | 8,510,071 | 61.31% |
| ├ `artifact_write_index_build` | 285,787 | 2.06% |
| ├ `artifact_write_commit` | 74 | ~0 |
| └ `artifact_write_wal_checkpoint` | 35 | ~0 |
| total | 13,881,046 | |

The sub-phases partition `artifact_write` to within **470 ms of 13,661,797** — T5's partition
property holds at 58k files, which is what makes the rest of this section measurable at all.
`index_build` non-zero on all three targets confirms the fresh-artifact bulk-load path engaged.

Extraction is now a rounding error: 58,366 files spooled in 213 s = **274 files/s**, up from the
baseline's ~190 files/s, and 1.5% of the run. Everything below is the write phase.

### Phase timeline and progress trail

The sub-phase durations pin an exact wall-clock timeline, which the observed events confirm
independently (extraction end observed 16:28:23 vs derived 16:28:28; process exit observed 20:16:1x
vs derived 20:16:09):

| Phase | window | duration |
|---|---|---|
| discovery + extraction/spool | 16:24:50 → 16:28:28 | 3.6 min |
| `plan` | 16:28:28 → 16:28:39 | 0.2 min |
| `file_symbol_insert` | 16:28:39 → 17:32:22 | 63.7 min |
| `child_rows` | 17:32:22 → 17:49:33 | 17.2 min |
| `resolution` | 17:49:33 → 20:11:23 | 141.8 min |
| `index_build` | 20:11:23 → 20:16:09 | 4.8 min |
| `commit` + `wal_checkpoint` | 20:16:09 | 0.1 s |

A 3–5 s sampler recorded DB bytes and RSS throughout. Sampled every ~10 minutes:

| time | DB (GB) | ΔDB (MB/min) | phase |
|---|---|---|---|
| 16:29:21 | 0.38 | — | file_symbol_insert |
| 16:39:22 | 0.91 | 50.1 | file_symbol_insert |
| 16:49:26 | 0.93 | 1.9 | file_symbol_insert |
| 16:59:26 | 0.95 | 2.1 | file_symbol_insert |
| 17:09:26 | 0.97 | 2.1 | file_symbol_insert |
| 17:19:29 | 0.99 | 1.9 | file_symbol_insert |
| 17:29:29 | 1.04 | 4.6 | file_symbol_insert |
| 17:39:29 | 8.26 | 688.7 | child_rows |
| 17:49:31 | 13.13 | 462.6 | child_rows |
| 17:59:34 | 13.61 | 45.8 | resolution |
| 18:09:35 | 13.97 | 34.4 | resolution |
| 18:29:35 | 14.39 | 17.7 | resolution |
| 18:49:38 | 14.71 | 13.7 | resolution |
| 19:09:45 | 14.97 | 11.3 | resolution |
| 19:29:46 | 15.16 | 7.8 | resolution |
| 19:49:50 | 15.33 | 7.7 | resolution |
| 20:09:54 | 15.47 | 6.5 | resolution |
| 20:16:09 | 24.52 | ~1,900 | index_build (final) |

Three things fall out of this trail, and they are the most useful evidence in this document:

1. **Resolution throughput degrades monotonically** — 45.8 → 6.5 MB/min across 2 h 21 m, a **7x
   decay** with no plateau. This is the answer to "linear-but-big or superlinear": within a single
   phase, on a fixed workload, throughput falling 7x is **superlinear**, not merely large.
2. **`file_symbol_insert` writes almost nothing while it runs.** 63.7 minutes to insert 2.58M symbols
   grew the DB file by roughly 2 MB/min. A phase that spends an hour without producing pages is
   dominated by reading/probing, not writing — which is consistent with the sampled profile (a
   sustained B-tree scan) and with the re-dirtying model below.
3. **`child_rows` is not the problem.** 15.5M reference sites and 12.9M identifiers landed in 17.2
   minutes at 460–690 MB/min. The two slow phases are the ones that *don't* write much.

RSS moved in the opposite direction to the DB file: roughly flat at ~3.4 GiB through
`file_symbol_insert`, dropping to ~0.8 GiB early in `child_rows`, then climbing steadily through
`resolution` from ~13 GiB to ~26 GiB, peaking at 30.60 GiB during `index_build`. So the memory growth
lives in **resolution**, not in the insert phase — and it is not a clean 1:1 with the DB file: final
RSS exceeds the pre-`index_build` DB size (15.5 GB) by about 2x.

### Rows written

symbols 2,576,001 · reference_sites 15,515,252 · identifiers 12,856,606 · pending_relationships
2,493,581 · source_regions 2,189,454 · type_facts 1,925,243 · complexity_metrics 622,360 ·
type_arguments 672,554 · symbol_annotations 226,585 · structural_facts 127,216 · parse_diagnostics
131,551.

Resolution completed and committed its overlay: **identifier_resolutions 12,856,606** and
**pending_resolutions 276,237**.

### Identity conflicts — 28 files, 4,237 conflicts, all C

The plan asserted this would be zero after T3's root fixes. It is not. Per instruction I captured and
attributed it rather than fixing anything.

- Every conflicting file is `language: c`; **no PowerShell conflict anywhere** — T3's root fix 1 held.
- Every sampled site names exactly one diverging column: `containing_symbol_id`. No other column ever
  diverged, which matches T3's root-cause model precisely.
- No rollup warning was emitted, so all 28 affected files are individually named — 28 is the true
  file count, not a sample.
- The import **committed**: first-write-wins did its job, and this is now a recoverable warning
  instead of the non-recoverable abort that made the repo unindexable. Bug (c)'s blocker property is
  fixed even though its root cause is not fully eliminated.

Worst offenders: `src/coreclr/pal/prebuilt/inc/cordebug.h` (3,766), `src/mono/mono/metadata/marshal.h`
(143), `src/mono/mono/tests/libtest.c` (112), `src/mono/mono/sgen/sgen-gc.h` (40),
`src/mono/mono/component/hot_reload.h` (50). 25 of 28 are `.h` headers; the other three are `runtime.c`, `libtest.c`, `mono-threads.c`.

T3's own report predicted this class: its residual-risk audit lists the languages whose
relationship/pending pass computes containment by its own algorithm rather than the shared helper,
and it flagged that the fixture corpus does not exercise the C flavor. This run is that residual
showing up on a real corpus, concentrated in C headers. It is the same open item already tracked as
"route own-scope relationship passes through the shared containment helper".

### Recursion guards

`parse_diagnostics` by kind: `error` 91,475 · `missing` 40,073 · **`depth_truncated` 3**.

All 3 `depth_truncated` rows are C#, in the files you would predict:

- `src/tests/JIT/Regression/JitBlue/GitHub_10215/GitHub_10215.cs` — the T2 evidence anchor, the file
  that aborted 2.21.0 at default stacks. It is now **indexed**, with its truncation recorded instead
  of silent.
- `src/tests/JIT/Methodical/largeframes/skip6/skippage6.cs`
- `src/tests/JIT/Regression/JitBlue/Runtime_64125/Runtime_64125.cs`

Three truncations across 58,366 files is the guard biting exactly where the dossier said it would and
nowhere else — no collateral capping in any other language.

T2's concern #2 (the C# extractor is superlinear on long operator spines; the real GitHub_10215.cs
costs 5.9 s of extraction for one 75 KB file) is **not visible in the aggregate**: C# extraction over
32,602 files / 339 MB totals 410 s of worker time, so the JIT-test family costs seconds, not minutes,
of a 3 h 51 m run. The finding stands as written; it is simply not a scale problem at this ratio.

## V2 — hermes-agent (PowerShell identity flavor)

**PASS.** exit 0, `status: ok`, 7,534 scanned / 7,089 changed / 445 unsupported / **0 failed**, zero
errors, **zero `reference_site_payload_conflict` warnings**, at default stacks. The only warnings are
2 × `slow_file_skipped`. Wall 173.10 s, peak RSS 4.80 GiB, DB 3.28 GiB, peak spool 599 MiB, no spool
residue.

This is the decisive evidence for T3 root fix 1: `scripts/install.ps1` is the one-line-function file
that reproduced the abort on 2.21.0, and it now scans clean.

Phases (ms): discovery 251 · extraction_spool 20,157 · artifact_write 151,621 (resolution 103,590 ·
child_rows 30,095 · index_build 14,719 · file_symbol_insert 2,482 · plan 544 · commit 72 ·
wal_checkpoint 60).

## V3 — Miller repo (cold-start regression check)

**PASS.** exit 0, `status: ok`, 1,518 scanned / 1,331 changed / 0 failed, **zero warnings**. Wall
22.26 s, peak RSS 1.55 GiB, DB 740 MiB, peak spool 124.8 MiB.

Against the 2.21.0 pinned baseline recorded in Miller's
`docs/findings/2026-08-02-dotnet-runtime-scale-baseline.md`:

| Phase | 2.21.0 pinned | branch `9a2fd23` | improvement |
|---|---|---|---|
| total | 44.2 s | **22.09 s** (22.26 s wall) | **2.00x** |
| `artifact_write` | 39,630 ms | **17,765 ms** | **2.23x** |
| `extraction_spool` | 4,537 ms | 4,267 ms | 1.06x |
| `discovery` | 21 ms | 21 ms | — |

This lands inside T5's measured range (20.8 s total / 18.8–22.9 s write) rather than at its optimistic
end, which is the right way to quote it: **the small-repo cold start is halved**, and the write phase
drops from 89.7% of cold start to 80.4%.

## Per-defect verdict

| Defect | Verdict | Evidence |
|---|---|---|
| **(a)** stack-overflow abort at default stacks | **FIXED** | V1 and V2 both complete at default stacks with `RUST_MIN_STACK` unset; GitHub_10215.cs is indexed; 3 `depth_truncated` rows make the capping visible |
| **(b)** ~68x worst-case / ~10x aggregate spool amplification | **FIXED (7.3x smaller)** | peak spool 3.18 GiB for a full 58,366-file run, vs 15.4 GB leaked at 60% of files on 2.21.0 (~25 GB full-run projection). No spool residue on any of the three runs |
| **(c)** `reference_site identity conflict` aborts the whole import | **BLOCKER FIXED, ROOT CAUSE PARTIAL** | import commits; warning is recoverable; PowerShell flavor eliminated (V2 clean). 28 C files / 4,237 conflicts remain, all on `containing_symbol_id` |
| **(d)** bare-word `failed` with no diagnostics | **FIXED** | the 8 per-file failures are individually named with code, message, byte offset and path; this is what identified `Count.T.cs` |
| **(e)** artifact write dominates cold start | **IMPROVED at small scale, REGRESSED into a wall at large scale** | Miller write 39.6 s → 17.8 s (2.23x). dotnet/runtime write is 98.4% of a 3 h 51 m run |

## The scaling wall (new finding, not anticipated by the plan)

Write cost per unit of work grows sharply with artifact size. Normalising `artifact_write` by
reference sites (the dominant row domain) across the three targets:

| Target | files changed | reference_sites | `artifact_write` | ms per 1M ref-sites |
|---|---|---|---|---|
| Miller | 1,331 | 438,135 | 17,765 ms | 40,547 |
| hermes-agent | 7,089 | 1,778,987 | 151,621 ms | 85,229 |
| dotnet/runtime | 41,406 | 15,515,252 | 13,661,797 ms | **880,536** |

4.1x the rows costs 2.1x more per row (Miller → hermes); a further 8.7x the rows costs **10.3x** more
per row (hermes → dotnet). The two sub-phases that carry it:

| Sub-phase | Miller | hermes | dotnet | dotnet vs hermes, per row |
|---|---|---|---|---|
| `file_symbol_insert` per 1M symbols | 3,075 ms | 4,174 ms | **1,484,271 ms** | **356x** |
| `resolution` per 1M ref-sites | 22,509 ms | 58,229 ms | **548,504 ms** | **9.4x** |

Resolution's **share** of the write phase, placed next to T5's post-bulk-load numbers, is worth
recording precisely because it is *not* where the scaling signal lives:

| Target | files | artifact | `resolution` share of `artifact_write` | `resolution` per 1M ref-sites |
|---|---|---|---|---|
| cmov subtree (T5) | 80 | 753 MB | 81.2% | — |
| Miller (T5) | 1,518 | 776 MB | 55.9% | — |
| Miller (V3) | 1,518 | 740 MiB | 55.5% | 22,509 ms |
| hermes (V2) | 7,534 | 3.28 GiB | 68.3% | 58,229 ms |
| dotnet (V1) | 58,366 | 22.84 GiB | 62.3% | 548,504 ms |

The share bounces between 55% and 81% with no trend — an 80-file subtree shows the *highest*
resolution share in the set. **Share is not the scaling story; per-row cost and the in-phase
throughput decay are.** Reading the share alone would suggest dotnet/runtime is unremarkable, when
its per-row resolution cost is 9.4x hermes and its throughput falls 7x over the phase.

`file_symbol_insert` is the sharper break: 2.58M symbols took 63.7 minutes (674 symbols/s) where
hermes managed 594,628 symbols in 2.5 s (~240,000 symbols/s). Resolution is the bigger absolute
number (2 h 21 m, 61.3% of the run) but it degrades far more gently.

What I measured about the insert phase, and what I ruled out:

- Four live `sample`s were taken. Against the derived phase timeline above, **every one of them
  symbolized correctly**: 17:08 and 17:11 → `FileRowInserters::insert_symbols` during
  `file_symbol_insert` (16:28:39–17:32:22); 17:49 → `ChildRowInserters::insert_child_rows` during
  `child_rows` (17:32:22–17:49:33); 19:28 → `resolution::resolve_full` →
  `ResolutionWriteBuffer::flush` during `resolution`. Mid-run I told the lead the `insert_symbols`
  frame was a symbolization artifact and that the run was already in resolution — **that correction
  was itself wrong**, and the timeline disproves it. The original attribution stood.
- The two `file_symbol_insert` samples, three minutes apart, show a **single long-lived
  `sqlite3_step`** dominated by `sqlite3BtreeNext` → `moveToChild` → `getAndInitPage` → `pread`,
  with `vdbeColumnFromOverflow`/`getOverflowPage` and `binCollFunc`/`memcmp` — a B-tree **scan**
  comparing TEXT keys and reading overflow pages, not an insert seek. The ~2 MB/min DB growth across
  that hour corroborates it: the phase is read-dominated.
- **Ruled out: the symbol INSERT statement itself.** `FileRowInserters::prepare` builds a plain
  `INSERT INTO symbols (…) VALUES (…)`. I built a probe artifact, dropped the seven `symbols`
  secondary indexes to reproduce the bulk-load state, and ran `EXPLAIN` with `foreign_keys=ON` both
  with and without them, with `parent_symbol_id` NULL and non-NULL. **No `Next`/`Rewind` opcode
  appears in any variant** — only `Found` seeks and `IdxInsert`. So the scan is a *different
  statement inside the same phase*, not the symbol insert. That is the key open thread for T7: the
  phase is correctly attributed, and something in it scans.
- **Ruled out: the identity trigger.** `reference_sites_identity_guard` sub-selects on
  `reference_site_id`, which is that table's `TEXT PRIMARY KEY` — a seek, and it fires on
  `reference_sites`, not `symbols`.
- **Primary mechanism (the lead's T7 diagnosis, adopted here):** child-row tables key on random hash
  TEXT primary keys, so once the artifact is ~100x the 128 MB page cache every insert lands on a cold
  random B-tree leaf; and under T5's bulk-load `journal_mode=MEMORY`, mid-transaction cache spills
  write pages into the db file whose later re-modification journals pre-images **in RAM**, so the
  journal grows toward O(DB size). It explains why cmov (791 MB, fits in cache) never showed either
  effect, and the progress trail supports it: the two slow phases are the ones that barely grow the
  file, i.e. they re-touch pages rather than append them. T7 Stage 1 and Stage 2 are scoped against
  this in [`the plan`](../plans/2026-08-02-scale-fixes-plan.md).
  **Two refinements the trail forces on the "RSS tracks DB 1:1" formulation.** First, the memory
  growth is confined to `resolution` — RSS is flat at ~3.4 GiB through the 63.7-minute
  `file_symbol_insert` phase and only climbs (13 → 26 GiB) once resolution starts. Second, it is not
  1:1: peak RSS (30.60 GiB) is roughly **2x** the DB size at the moment resolution ends (15.5 GB).
  Whatever grows is therefore tied to resolution's own working set as well as to journalled
  pre-images, so Stage 1's disk-backed journal may bound the RAM without fixing the 7x throughput
  decay. Worth measuring both separately.
- **Secondary candidate, worth a cheap check during T7:**
  `load_symbol_lookup_for_requested_ids` (`writer/rows.rs:1580`) runs once before the insert loop
  (`writer.rs:1125`) and bulk-inserts every requested symbol id across the whole workspace into
  `temp.julie_symbol_lookup_requested`, a `WITHOUT ROWID` table keyed by a random TEXT id, held in RAM
  because `open_path` sets `temp_store=MEMORY`. At 58k files that is millions of random-key inserts
  into one in-memory B-tree, which would add to both the RSS curve and the scan-shaped profile. Cheap
  to confirm or kill with `temp_store=FILE` or an `EXPLAIN QUERY PLAN` on the join; I did not test it.

Memory tracks T5's concern #2 exactly, at the scale it warned about: peak RSS **30.6 GiB** and peak
footprint **36.6 GiB** on a 64 GB box. The 2.21.0 baseline peaked at 5.3 GB — but it also aborted at
313 s, so this is not a like-for-like regression, it is the first measurement of what a completed
58k-file write actually costs. T5's proposed scoped `temp_store=FILE` for the index-build window
would not by itself address this: `index_build` is only 285.8 s of 13,662 s, and the RSS climb
happens well before it.

I did not implement any fix, per the task's freeze on the prior tasks' code.

## Residuals

1. **The write-phase scaling wall** (above) — the dominant one, now scoped as **T7** in the plan
   (disk-backed bulk-load journal + scaled cache in Stage 1; PK-sorted child-row insertion in
   Stage 2). Four notes for T7's validation, in priority order:
   - This run's **13,661,797 ms** write phase is the before-number, and V1 is the re-validation gate.
   - `file_symbol_insert` degrades far more steeply per row (356x) than `resolution` (9.4x), but
     `resolution` is the larger absolute cost (2 h 21 m). Stage 1 should be measured against both.
   - Stage 2 targets **child-row insertion**, which the trail shows is already the *fast* phase
     (15.5M reference sites in 17.2 min at 460–690 MB/min). The two expensive phases,
     `file_symbol_insert` and `resolution`, both barely write. Confirm Stage 2 is aimed at the right
     table before building it.
   - Something inside `file_symbol_insert` performs a sustained B-tree scan that is **not** the
     symbol INSERT (ruled out by `EXPLAIN`). Identifying that statement is the cheapest available
     lead.
2. **28 C files still conflict on `containing_symbol_id`.** T3's residual-risk class, now confirmed on
   a real corpus. Recoverable, but the site-level column reflects one pass's opinion for 4,237 sites.
   Fix is the tracked follow-up: route own-scope relationship passes through the shared containment
   helper.
3. **Resolution is the next 2x of `artifact_write`** at every scale — as a share of the write phase:
   62.3% on dotnet, 68.3% on hermes, 55.5% on Miller. T5's concern #1, unchanged by this validation.
4. **Symbol `code_context`** is still computed and dropped at the artifact bridge (T4 residual 1) —
   pure CPU on every symbol in every language. Needs user approval; `Symbol` is a public serialized
   type.
5. **`resolution_store.rs` still SELECTs `i.code_context`** into `IdentifierWorkItem`; it is now always
   NULL and nothing reads it (T4 residual 2).
6. **Raw-byte hash ids in the spool** — T4's C2 residual, worth roughly another 46 B/row, would take
   the spool from 6.97x to ~4.9x of the original.
7. **C# superlinearity on long operator spines** (T2 concern #2, `GitHub_10215.cs` at 5.9 s for 75 KB)
   — real, but 32,602 C# files cost only 410 worker-seconds total, so it is not a scale problem today.
8. **Warnings are suppressed on the human (non-`--json`) path when `status == ok`** (T1's product
   decision). A successful scan with 4,237 identity conflicts prints only `ok` plus the counts line.
   Anything that needs to see conflicts must pass `--json`. Flagged as an observation, not a defect.

## Release prep

**This section is preparation only. Nothing here has been executed. No version file was edited, no
tag created, no release published, and Miller's pin was not touched.**

### Proposed version: `2.22.0` (minor)

Repo precedent, read from the release notes:

- `v2.12.1` is the patch precedent, and it explicitly says its rewrites "do not change extracted
  symbols, facts, contracts, parser pins, or release-package contents".
- `v2.21.0` is the minor precedent: new capability **plus** a consumer-facing behavior change
  (oversized-file transition policy, `update` status `no_change` → `unsupported`).

This branch changes consumer-visible behavior — `identifiers.code_context` is now always NULL, a new
warning code and a new `parse_diagnostics.kind` appear, and a fatal import abort became a recoverable
warning — so it is more than a patch. It adds no new language and breaks no contract version, so it
is not a major. **Minor, `2.22.0`.**

Contract constants verified unchanged on the branch: `SQLITE_SCHEMA_VERSION` 5,
`EXTRACT_CONTRACT_VERSION` 4, `JSONL_SCHEMA_VERSION` 4. Only
`docs/contracts/sqlite-schema-v5.catalog.sha256` moved, for the trigger DDL — T3's documented
decision, with the precedent that `87c52c0` changed the catalog fingerprint without bumping the
schema version.

### Release readiness: BLOCKED

The user has classified the write-phase scaling wall as a release blocker (T7). Recommendation: do
**not** cut `2.22.0` until T7 lands, then re-run V1 as the gate. Shipping today would advertise
dotnet/runtime support that takes 3 h 51 m and 30.6 GiB of RAM. The four abort fixes are genuinely
valuable and every other target improves, so if the blocker is later waived, the notes below are
ready as written.

### Draft release notes — `docs/release-notes/v2.22.0.md`

> # v2.22.0
>
> This release makes large real-world repositories indexable. julie-extract 2.21.0 could not index
> dotnet/runtime (58,500 files) at all: it aborted with a stack overflow at default thread stacks,
> leaked a 15.4 GB scan spool, and — at larger thread stacks — failed the entire artifact import
> non-recoverably on a reference-site identity conflict, writing zero rows and reporting the single
> word `failed`. All four of those are fixed.
>
> ## Recursion guards (stack-overflow abort)
>
> Every CST walker is now bounded by the shared traversal budget, including the Blazor navigation
> walkers that run on every C# file and the complexity-metrics walker. Hitting the cap records a
> `depth_truncated` row in `parse_diagnostics` (additive kind value) instead of capping silently. The
> rayon extraction pool takes an explicit 16 MiB worker stack so behavior no longer depends on the
> caller's `RUST_MIN_STACK`. A convention test fails the build on any new production walker that
> recurses over children without the guard.
>
> ## Reference-site identity (import abort)
>
> Two extraction passes could disagree about a reference site's `containing_symbol_id` and trip a
> BEFORE INSERT guard that aborted the whole single-transaction import. Two root fixes: PowerShell's
> identifier pass no longer filters containment to multi-line symbols, so a one-line
> `function F { G }` resolves the same way in every pass; and the shared containment helper now
> applies a deterministic total order (start_byte, then symbol_id) so input iteration order cannot
> change the winner. The guard itself is demoted from a fatal abort to **first write wins**: the
> import commits and the scan report carries a recoverable `reference_site_payload_conflict` warning
> per affected file, with the language, conflict count and a bounded sample of the diverging columns.
> Per-row attribution is unaffected — `identifiers` and `pending_relationships` carry their own
> containing/caller columns.
>
> ## Scan spool (disk amplification)
>
> Per-identifier `code_context` is no longer emitted: it was write-only weight that no consumer ever
> read. The spool is re-framed from one JSON object per line to length-prefixed binary frames with a
> spool-local intern table for repeated hash ids, and the import streams rows instead of re-parsing
> the whole spool three times. Measured 6.97x smaller on a dense subtree and 7.3x smaller on a full
> dotnet/runtime scan (3.2 GiB peak, against a ~25 GB projection for 2.21.0). **JSONL export emits
> `null` for `identifier.code_context`** — the field remains in the schema.
>
> ## Artifact write
>
> `artifact_write` is split into additive `artifact_write_*` JSON profile keys (`plan`,
> `file_symbol_insert`, `child_rows`, `resolution`, `index_build`, `commit`, `wal_checkpoint`) that
> partition the phase. A fresh-artifact bulk-load mode defers secondary-index creation and relaxes
> journal pragmas for the initial build only, restoring WAL and checkpointing before finishing; it is
> gated on an empty artifact and never activates on `update`, `delete`, or an incremental scan.
> Miller's cold scan drops from 44.2 s to 22.1 s, with `artifact_write` down 2.23x.
>
> ## Diagnostics
>
> The non-JSON report path now prints one `code: message (path)` line per error and warning plus a
> `files: scanned=N changed=N unchanged=N failed=N` counts line, instead of a bare status word. Exit
> codes and the `--json` payload are byte-identical to before.
>
> ## Contracts
>
> - SQLite schema version unchanged (5); only the catalog fingerprint moves, for the trigger DDL.
> - `extract_contract_version` unchanged (4); JSONL schema unchanged (4).
> - New additive values consumers should tolerate: `parse_diagnostics.kind = 'depth_truncated'` and
>   report warning code `reference_site_payload_conflict`.
> - Parser dependency pins unchanged.

### Release checklist (from `docs/release.md`)

1. `scripts/check-release-state.sh` — release-state tripwire, at session start.
2. Land T7 and re-run V1 as the scale gate (this release's specific blocker).
3. `chore(release): prepare v2.22.0` — bump `julie-extract-artifact`, `julie-extract-cli`,
   `julie-extractors` to `2.22.0`, update `Cargo.lock`, add `docs/release-notes/v2.22.0.md`, add the
   pointer to `docs/release-notes/README.md`, advance the "Current published release" line in
   `docs/release.md`.
4. Branch gates: `cargo fmt --check`, `cargo test -p xtask`, `cargo xtask test default`,
   `cargo xtask test contract`.
5. `cargo xtask dogfood repo --root . --out-dir target/dogfood/julie-extractors`.
6. `cargo xtask release preflight --version 2.22.0` — verifies crate versions, release-note
   availability, and package manifest paths.
7. Publish via tag push `v2.22.0` or `workflow_dispatch` with the explicit version input; the
   workflow builds the four targets and copies `docs/release-notes/v2.22.0.md` into the release body.
8. Source-control closeout: `git fetch origin --tags`, primary checkout on clean current `main`,
   `HEAD == origin/main == v2.22.0`.
9. Record evidence under `docs/release-evidence/`.
10. Only then: bump Miller's `scripts/julie-pins.json`.

Steps 3 and 7–10 are approval-gated with the user.

## Coordination notes for the fleet-safety session (W4–W6)

- **Spool filename glob is preserved.** `create_scan_spool` still writes
  `julie-extract-scan-spool-<pid>-<nanos>.jsonl` into `std::env::temp_dir()`, pinned by
  `commands.rs::tests::scan_spool_name_still_matches_the_fleet_reaper_glob`. The W4 reaper globs by
  name only, so it is unaffected.
- **Spool contents are now length-prefixed binary postcard frames**, despite the retained `.jsonl`
  suffix. Name-based reaping is fine; anything that parses spool *content* would not be. Nothing
  outside `writer.rs`/`commands.rs` reads it.
- **The W5 progress-hook insertion point is unchanged**: the serial per-chunk drain in
  `extract_supported_files_to_spool` (`commands.rs:1381`), chunked at
  `EXTRACT_SPOOL_CHUNK_SIZE = 512` (`commands.rs:1241`).
- **The new `artifact_write_*` keys give the W5 heartbeat real denominators.** This matters more than
  expected: on dotnet/runtime, extraction is 1.5% of the run and the write phase is 98.4%, so a
  progress design that only instruments extraction would show a full bar for 3 h 47 m of remaining
  work. The seven keys partition the write to within 470 ms of 3 h 47 m, so they are trustworthy as
  a denominator.
- **Two additive contract values for Miller**, both confirmed emitted on a real corpus:
  `parse_diagnostics.kind = 'depth_truncated'` (3 rows on dotnet/runtime) and report warning code
  `reference_site_payload_conflict` (28 warnings). Miller-side tolerance for both was verified by the
  lead.
- **The fresh-artifact bulk-load mode is invisible in the finished artifact** and never fires on
  live or incremental DBs — eligibility is `!artifact_has_files` at open (`writer.rs:214`) and is
  spent by any write (`writer.rs:232`), guarded by
  `writer_contract.rs::bulk_load_never_activates_on_update_delete_or_populated_scan`. All three
  validation runs finished with `journal_mode = wal` and a 0-byte WAL.
- **New number the fleet plan should carry:** a completed 58k-file scan currently needs ~31 GiB RSS
  and ~23 GiB of disk for the artifact, and takes ~4 h. Any fleet-level scheduling or timeout policy
  built on the old ~6–8 minute projection needs revising, and W4's disk accounting should budget for
  the artifact as well as the spool.

## Miller calls used

Workspace selector `julie-extractors-91c17adbdab9`. The index covers the **main** checkout, so both
calls returned no hits for branch-new identifiers — correctly, since `reference_site_payload_conflict`
and `depth_truncated` are introduced by this branch. Every code fact in this document was therefore
read directly from the worktree files, which is the required re-verification step anyway.

| Call | Result |
|---|---|
| `search query="reference_site_payload_conflict" mode=source` | no text hits (branch-new symbol, absent from the main-checkout index) — fell back to worktree grep, found `reports.rs:365/392/440`, `schema.rs:316` |
| `search query="depth_truncated parse diagnostics kind" mode=source` | no text hits (same reason) — worktree grep found `extraction.rs:894`, `tree_traversal.rs:33`, `base/types.rs:23` |

## Handoffs

- Miller's `docs/findings/2026-08-02-dotnet-runtime-scale-baseline.md` has an open item — "wall-clock
  for a full healthy extract + artifact write and final DB/WAL sizes — blocked on the reference-site
  identity fix". That item is now answered by this document (3 h 51 m 30 s; 22.84 GiB DB; 0-byte WAL;
  3.18 GiB peak spool). Updating that file is outside this task's file ownership and is left to the
  lead.
- The four bugs the baseline recommended filing can be closed as fixed on this branch, with (c)
  downgraded from blocker to the open C-header residual rather than closed outright.

---

# T7 re-validation — 2026-08-02 (appended)

T6 left `artifact_write` as a 3 h 51 m wall and attributed it to a random-PK B-tree problem
compounded by an in-RAM journal. Both attributions are wrong. This section records what the wall
actually was, the measurements that settled it, and the re-validation numbers after the fix.

## Root cause: a quadratic foreign-key scan that T5's index deferral introduced

The spooled writer sets `PRAGMA defer_foreign_keys=ON` (`writer.rs:1011`) so a symbol's parent may be
inserted later in the same transaction. SQLite enforces a deferred foreign key from the **parent**
side as well as the child side: every parent-row INSERT searches each referencing child table for rows
that point at the new row, so the deferred-constraint counter can be settled. With an index on the
child's foreign-key column that search is a seek; without one it is a full table scan.

T5's bulk load drops **every** secondary index, including `idx_symbols_parent`. Because
`symbols.parent_symbol_id` references `symbols`, each symbol insert then full-scans the table it is
filling — O(n²) in symbols.

`EXPLAIN` of the real symbol INSERT, counting full-scan opcodes:

| pragma state | opcodes | Rewind | Next | OpenRead |
|---|---|---|---|---|
| `defer_foreign_keys=OFF` | 77 | 0 | 0 | 2 |
| `defer_foreign_keys=ON` (what the writer sets) | 189 | **16** | **16** | 18 |
| `defer_foreign_keys=ON` + `idx_symbols_parent` | 194 | 15 | 16 | 18 |

16 is exactly the number of child foreign-key columns referencing `symbols`. T6's EXPLAIN probe
reported "no `Next`/`Rewind` opcode appears in any variant" and therefore cleared the insert path — it
did not set `defer_foreign_keys=ON`, which is the pragma that generates the scans.

Per-inserted-row full-scan opcodes across every row table:

| table | defer OFF | defer ON | defer ON + indexes present |
|---|---|---|---|
| `symbols` | 0 | **16** | **0** |
| `files` | 0 | **11** | **0** |
| `reference_sites` | 3 | 3 | **0** |
| `identifiers` | 0 | 1 | **0** |
| all other row tables | 0 | 0 | 0 |

With the indexes present there are zero scans anywhere: this is a regression the bulk load introduced,
not a property of the schema.

## What the T6 hypotheses actually measure

- **Random-PK B-tree wall — refuted.** 3,000,000 rows with random 32-hex TEXT PRIMARY KEYs, one
  transaction, the exact T5 bulk-load pragmas: **9.9 s**, 1.16 GB DB, peak RSS **273 MB**. T6's own
  numbers agree — `child_rows` wrote ~35.6M random-PK rows at 34,500 rows/s while the DB grew 1 → 13 GB,
  while `file_symbol_insert` wrote 2.58M rows at 675 rows/s while the DB was under 1 GB. The slow phase
  is the one with the small database.
- **MEMORY journal growing toward O(DB size) — refuted for the insert phase.** RSS is flat at 0.18 GiB
  across every reproduction, and T6's own sampler trail shows RSS flat through `file_symbol_insert`.
  The 13 → 26 GiB growth happens inside `resolution` and belongs to the resolver's in-memory
  structures.

Replaying all 2.58M real dotnet/runtime symbol rows through the real insert shape (real schema,
indexes dropped, foreign-key parents loaded, `defer_foreign_keys=ON`, single-row prepared statements,
source reads timed separately and excluded):

| variant | 600k rows | shape |
|---|---|---|
| baseline (today's bulk load) | **147.4 s** | quadratic — 4.8 / 33.2 / 109.4 s per 200k |
| `journal_mode=DELETE` (the proposed Stage 1) | slower than baseline | no effect on the mechanism |
| `cache_size=-4194304` (4 GiB; database never spilled a page) | ~40% better, still quadratic | not the mechanism |
| **`+ idx_symbols_parent`** | **3.5 s** | **linear** — 0.9 / 1.3 / 1.3 s per 200k |
| `defer_foreign_keys=OFF` | FOREIGN KEY constraint failed | deferral is genuinely required |

The 4 GiB-cache arm is decisive: the whole database stayed resident (16 MB on disk) and the collapse
still happened, so the cost is in-memory CPU that grows with rows already inserted — not I/O, not cache
residency, not B-tree shape.

## Fix

`drop_secondary_indexes` now preserves the narrowest index per foreign-key child column, derived from
`pragma_foreign_key_list` / `pragma_index_list` / `pragma_index_info` rather than a hand-maintained
list. 34 of 54 indexes stay through the bulk load; the wide search and export-order indexes
(`idx_symbols_name_kind`, `idx_symbols_path`, `idx_identifiers_name_kind`, the three `*_export_order`
indexes, …) remain deferred to the end-of-write build, so T5's win is preserved.

## Resolution is not a write wall

Every statement the resolution pass issues plans without a full scan in **all** index states —
the `identifier_resolutions` and `pending_resolutions` upserts, the `identifiers` target update, and
the overlay deletes all report 0 scan opcodes with no indexes, with the foreign-key subset, and with
the full catalog. `worklist_full_identifiers` performs one full `identifiers` scan, which is correct
and runs once. Resolution's remaining cost is therefore its own compute and its 12.86M-row in-memory
materialization, which is successor task #15's scope, not this task's.

## Fix and measured results

`begin_bulk_load` sets `PRAGMA foreign_keys=OFF` and `end_bulk_load` restores it; `verify_foreign_keys`
runs one whole-database `PRAGMA foreign_key_check` inside the write transaction, after the secondary
indexes are rebuilt and before `COMMIT`. A violation returns `ArtifactWriteError::ForeignKeyViolation`
and rolls back to the empty artifact. `drop_secondary_indexes` still drops every secondary index, so
T5's deferral win is kept in full.

This removes the parent-side searches AND the per-row child-side parent lookups. Enforcement is not
weakened: `foreign_key_check` validates every row in the artifact, where the deferred per-row checks
only covered rows the write touched. It is safe only on this path for the reason the bulk-load gate
already guarantees — a fresh artifact never deletes or rewrites a row during the write, so no
`ON DELETE CASCADE` / `SET NULL` action is owed.

An earlier attempt that instead PRESERVED the 34 foreign-key-backing indexes was measured and
rejected: it fixes the quadratic but forces those indexes to be maintained through every pass, and
`idx_identifiers_target` in particular only serves parent-side searches while `symbols` is being
inserted — when `identifiers` is still empty and the search is free anyway — while resolution updates
that column 12.86M times. On `src/coreclr` that build was still running at 8 m 18 s with 13.6 GiB RSS
against a 311 s / 6.29 GiB baseline.

Each arm is the same binary pair on the same input, back-to-back on the same box. The `old` arm
reproduces T5's published baselines (cmov write ~43.5 s, Miller write 18.8–22.9 s).

| target | files | `artifact_write` | wall | peak RSS | artifact bytes |
|---|---|---|---|---|---|
| cmov subtree | 80 | 45.9 → **25.0 s** (−46%) | 57.3 → 36.6 s | 2.37 → 1.60 GiB | 752,828,416 → 752,869,376 |
| Miller | 1,518 | 19.8 → **16.7 s** (−16%) | 24.2 → 21.1 s | 1.53 → 1.06 GiB | 776,019,968 → 776,003,584 |
| dotnet `src/coreclr` | 4,697 | 283.5 → **143.3 s** (−49%) | 311.0 → 173.1 s | 6.29 → 4.25 GiB | 3,828,793,344 → 3,828,822,016 |

`src/coreclr` sub-phase split:

| phase | old | new | change |
|---|---|---|---|
| `artifact_write` | 283,494 ms | 143,263 ms | **−49%** |
| `artifact_write_resolution` | 192,797 ms | 79,461 ms | −59% |
| `artifact_write_child_rows` | 45,223 ms | 35,688 ms | −21% |
| `artifact_write_file_symbol_insert` | 22,587 ms | **793 ms** | **−96.5%** |
| `artifact_write_index_build` | 22,278 ms | 25,987 ms | +17% |

Resolution falling 59% was not predicted by the insert diagnosis: it is the per-row child-side parent
lookups disappearing (each overlay insert probed `identifiers` and `symbols`, each identifier target
update probed `symbols`).

`index_build` rising 3,709 ms is the price of the fix, and it is worth stating precisely because it is
the one cost this change adds. Both arms drop and rebuild the same 54 indexes, so the index work is
identical; the delta is the new whole-database `foreign_key_check`, which `verify_foreign_keys` runs
just before `clock.lap(index_build)` and which is therefore folded into that phase.

Run standalone against the finished 3.57 GiB `coreclr` artifact with a cold cache, that check takes
**48.2 s** (13.5 s/GiB). In situ it costs **~3.7 s** on the same artifact, because it runs inside the
write transaction with the pages it just wrote still hot. The in-situ figure is the one that matters;
extrapolated to a 22.8 GiB artifact it is on the order of half a minute, against an `artifact_write`
budget measured in tens of minutes. It buys strictly stronger validation than the per-row deferred
checks it replaces: `foreign_key_check` verifies every row of every table, not only rows touched
during the write.

### Stage 1 (journal / cache_size) — measured, and not needed

- `journal_mode=DELETE` was *slower* than `MEMORY` on the insert path (110 s vs 101 s for the same
  200k-row slice) and does not touch the mechanism. Its purpose was bounding RSS, but insert-phase RSS
  is flat; the growth is resolution's own in-memory worklist.
- A 4 GiB `cache_size`, large enough that the database never spilled a page, still showed the collapse.

Both are recommended to stay as they are.

### Measurement caveat

A `julie-extract` from the sibling `fleet-safety` worktree ran concurrently on this box for part of
the session. A/B ratios are same-box and same-period so they hold; absolute wall-clock is noisy.

### Content equivalence on a real artifact

The two Miller artifacts (1,518 files, 24 tables) were diffed table by table: row counts, plus a
blake2b digest over every row sorted by all columns.

All 24 row counts are identical, and 18 of 24 tables digest-match exactly — including
`reference_sites` (438,135 rows), which is the direct evidence that the identity trigger's
first-write-wins semantics survive on a real workload and not only in the unit tests, plus
`identifiers` and `identifier_resolutions` (349,495 each), `pending_relationships` (79,487),
`type_facts` (45,174), `pending_resolutions`, `symbol_annotations`, `complexity_metrics`,
`type_arguments`, `type_argument_usages`, `revision_file_changes`, `literals`, `parse_diagnostics`,
`parser_inventory` and the `language_capabilit*` family.

The six that differ are accounted for and none is caused by this change:

- `files.indexed_at`, `extraction_revisions.started_at` / `completed_at` — wall-clock timestamps.
- `symbols`, `relationships`, `source_regions`, `structural_facts` differ **only** in `metadata_json`.
  Re-compared with JSON keys normalized, all four multisets are identical.

### Artifact `metadata_json` key order is not reproducible run-to-run

The key order comes from `pub metadata: Option<HashMap<String, serde_json::Value>>`
(`crates/julie-extractors/src/base/types.rs`, five sites). Rust randomizes `HashMap` iteration per
process, so `serde_json` emits a different key order on every run of the same binary. The artifact
writer binds the value through verbatim and only ever reads it via `json_extract`, so the writer is
not the source.

The operative consequence: **byte-level artifact comparison is not a valid equivalence method for
this project.** Compare content with normalized JSON instead. Making the order deterministic is a
small change (`BTreeMap`, or `serde_json/preserve_order`) but it belongs to the extractors crate.

### The milestone table — baseline vs fixed, same measurement method

Both runs sampled the artifact file size on a fixed interval, so "when did the database reach N GB"
is directly comparable between them. The baseline trail is T6's own `t6/v1-rss.log` (2 s interval);
the fixed run is `v1-fkoff-sampler.log` (30 s interval). T+ is measured from the start of each run.

| database reaches | baseline (T6) | fixed (T7) |
|---|---|---|
| 1 GB | **T+63.3 min** | ~T+4.0 min |
| 2 GB | T+63.6 min | ~T+4.5 min |
| 3 GB | T+64.1 min | ~T+7.0 min |
| 5 GB | T+65.3 min | ~T+20 min (contended, see below) |
| 10 GB | T+75.7 min | — |
| 13 GB | T+100.1 min | — |
| 20 GB | T+225.8 min | — |

Both runs spend their first ~3.5 minutes in extraction and spool, writing nothing to the artifact. So
the baseline's first gigabyte took **63 minutes of writing**, and the fixed run's took roughly **30
seconds**. That gap is the `file_symbol_insert` phase — 3,823 s in T6's profile — and it is the whole
of the quadratic foreign-key scan described above.

### Contention caveat on the T7 re-run's absolute numbers

A `julie-extract` from the sibling `fleet-safety` worktree (pid 55366, 22.9 GB RSS) ran throughout the
T7 V1 re-run, having started before it. Sampled over 40 s, it held ~85% of a core while the re-run's
single-threaded write phase got ~14%, on a 64 GB box whose swap was at 7.5 of 8 GB. **Ratios and the
milestone shape hold — both arms of every A/B ran same-box and same-period — but the T7 re-run's
absolute wall-clock is inflated and should be read as an upper bound.**

### Expected-unchanged residuals

These are T6 findings that this change does not address and must not alter: exit code 1 with 8
non-UTF-8 `read_failed` entries, ~4,237 C identity-conflict warnings, and 3 `depth_truncated` rows.
