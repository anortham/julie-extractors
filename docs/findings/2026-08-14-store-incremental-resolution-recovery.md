> **Superseded 2026-08-18.** The resolution write path is retired. See [2026-08-18-resolution-write-path-retirement.md](../decisions/2026-08-18-resolution-write-path-retirement.md).

# Store incremental-resolution recovery

**Status:** Crossover routing fixed; the production 5-second one-file budget still fails.

## Root cause

A one-path shortcut in `store/delta_scope.rs` forced every single changed path to stay scoped before the existing 70% work estimate ran. On the frozen Miller store, changing `README.md` selected 776 versions and 386,163 prior identifier rows. The scoped `ResolvedIdentifiers` phase remained CPU-bound beyond a 600-second observation.

The fix removes only that shortcut and its dead path-count parameter. Every non-empty scope now uses the existing selected-version plus name/receiver identifier estimate. No threshold, schema, report, CLI, or public API changed.

## Correctness and bounded performance

- Delta-scope contracts: 7/7 passed.
- Full-versus-scoped equivalence: 8/8 passed.
- Resolution-report scope contracts: 6/6 passed.
- A bounded one-file fixture stayed scoped with no fallback, completed in 3,792 ms, and matched the full oracle digest exactly; the full oracle completed in 2,273 ms.
- Mutation checks proved the broad-collision test detects the removed exemption and the bounded test rejects an always-full shortcut.

## Production-volume replay

The required 60-second replay still timed out before JSON output. One permitted 600-second observation completed and supplied the decisive routing facts:

| Fact | Before fix | After fix |
|---|---:|---:|
| Route | Scoped `ResolvedIdentifiers` | Full crossover |
| Wall | More than 600,003 ms | 178,621 ms |
| CPU | 597,969 ms at termination | 163,517 ms |
| Peak RSS / PSS | 32,690,176 / 30,292,992 bytes | 195,280,896 / 192,740,352 bytes |
| Fallback | None; scoped path | `resolution_scope_crossover` |
| Exact state | Not emitted | Matched generation 2 |

The post-fix scope was 1,510 files and 566,545 rows. Resolution itself consumed 171,167 ms; diff consumed 1,335 ms and scope selection 647 ms. Routing is correct, but 178,621 ms remains far above the 5,000 ms development budget. Retained-history lifecycle work and post-rotation query work remain owned by the Miller recovery plan's Tasks 7A and 7B.

## Language evidence

The frozen corpus contained 11 languages. Eight emitted identifier rows; JSON, Markdown, and YAML emitted none. The producer advertised 38 supported languages, 27 of which were absent from this corpus. The crossover policy is language-neutral, but this replay does not claim all-language extraction coverage.

## Safety and evidence

The replay used copied stores and a frozen source tree. The protected live Miller process and semantic broker remained alive and were never signaled. Copied coordinator and store databases retained `quick_check=ok`; the protected `CURRENT` pointer stayed unchanged.

Detailed commands, hashes, reports, and external replay artifacts are recorded in the Miller repository's recovery evidence:

- `task-6-crossover-fix-report.md`
- `task-6-postfix-production-replay-report.md`
