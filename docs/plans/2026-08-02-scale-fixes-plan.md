# Scale Fixes — dotnet/runtime baseline remediation plan

**Provenance:** the 2026-08-02 real-repo scale baseline
(miller repo: `docs/findings/2026-08-02-dotnet-runtime-scale-baseline.md`) found that julie-extract
2.21.0 cannot index dotnet/runtime (58,500 files) and attributed five defects. A five-track
read-only root-cause investigation (per-track reports preserved in the driving session; all
verdicts verified-in-code except write-throughput's cost split, which is strong-hypothesis pending
instrumentation) produced this plan. Worktree: `~/.config/razorback/worktrees/julie-extractors/scale-fixes`,
branch `scale-fixes` off `main` @ 173ae45. Baseline: 108 crate tests green (rustc 1.97.1 via
`RUSTUP_TOOLCHAIN`; workspace `rust-version = 1.95`, local default stable is 1.94).

**Standing repro targets:** dotnet/runtime @ `a2f953fe266` (clone in the driving session's
scratchpad) — currently fails deterministically; `~/.hermes/hermes-agent` — same failure via
`scripts/install.ps1` one-line PowerShell functions. Both must scan green at default stacks by the
end of this plan.

**Release shape:** one julie-extractors release bundling these fixes with the fleet-safety W4–W6
flags (built separately in the `fleet-safety-flags` worktree), then one Miller pin bump. No
release/pin bump without explicit user approval.

**Coordination notes (fleet-safety session):**
- Spool filename glob: their W4 reaper matches `julie-extract-scan-spool-*-*.jsonl`; T4 changes the
  spool encoding — keep the filename pattern (or hand them the new one before their W4 lands).
- W5 progress-file insertion point (investigated finding, hand over): there is NO existing progress
  plumbing; the natural hook is the serial per-chunk drain in `extract_supported_files_to_spool`
  (commands.rs:1368/1393, every ≤512 files via `EXTRACT_SPOOL_CHUNK_SIZE`), with phase labels from
  the existing `record_profile_phase` boundaries; artifact-write needs its own heartbeat inside the
  writer.

## Tasks

### T1 — Human-path error rendering (bug d). Smallest; do first (it unblocks debugging T2's swallowed panic).

- `crates/julie-extract-cli/src/reports.rs:397-411` (`write_outcome` non-JSON branch): after the
  status word, print one line per `report.errors`/`report.warnings` entry as `<code>: <message>`
  (append path when present) plus a `files: scanned=N changed=N unchanged=N failed=N` counts line
  for scan/update/delete; same stream the status word already uses. Add `ReportCode::as_str()`.
  Exit codes and the JSON branch untouched (verified: no caller parses human output — Miller and
  xtask always pass `--json`; no test asserts the bare word).
- Tests: failed scan without `--json` → stderr first line `failed` + code/message lines; ok scan →
  stdout starts `ok`; `--json` output byte-identical before/after; unit test for the formatter.

### T2 — Recursion guards + stack policy (bug a).

- Apply the existing `TREE_TRAVERSAL_DEPTH_LIMIT`/`should_visit_tree_depth` guard (tree_traversal.rs)
  to the four unguarded production walkers: `blazor_navigation.rs` `collect_receiver_declarations`
  (:42), `collect_navigation_calls` (:134), `collect_razor_hrefs` (:239) + `has_razor_expression_in_range`
  (:337), and `complexity_metrics.rs` `collect_stats` (:162; its `current_depth` is a metric, not a
  budget). Emit a truncation diagnostic (existing `parse_diagnostics` table; additive kind value)
  when the cap trips, so depth-capping stops being silent.
- Defense-in-depth: explicit `.stack_size(16 MiB)` on the rayon pool (commands.rs:1363) so behavior
  stops depending on the caller's `RUST_MIN_STACK`; confirm the single-file `update` path shares
  the policy.
- Tests (from the track report): ~4k-term generated `+`-chain run under a 512 KiB thread must
  complete (inverse of the existing 64 MB pattern at src/tests/javascript/mod.rs:28); registry-level
  deep-fixture test for csharp/razor asserting capped facts + diagnostic row; a convention
  source-scan test failing on any production walker that self-recurses over children without the
  guard (extend to catch mutual recursion); CLI regression: trimmed GitHub_10215-style fixture
  scans green at default stacks.
- Evidence anchor: dotnet/runtime `src/tests/JIT/Regression/JitBlue/GitHub_10215/GitHub_10215.cs`
  (17,602 `+` ops; crashes 2.21.0 alone at default stacks; macOS crash reports pin
  `collect_receiver_declarations`/`collect_navigation_calls`).

### T3 — Reference-site identity: root fixes + import hardening (bug c, THE BLOCKER).

Root cause (verified): three extraction passes share one site id (blake3(file_id, span)) by design,
but compute `containing_symbol_id` via different code paths; any disagreement trips the
`reference_sites_identity_guard` BEFORE INSERT trigger (schema.rs:260), aborting the whole
single-transaction import — zero rows. Two verified flavors: PowerShell single-line-function filter
(identifiers.rs:590 `symbol.start_line < symbol.end_line`) → NULL vs parent-walk; C multi-declarator
equal-span tie broken by HashMap-vs-Vec iteration order (creation_methods.rs:306 tie-break is
priority+size only).

- Root fixes: (1) remove the PowerShell multi-line filter — span containment decides; (2) add
  deterministic final tie-breaks (start_byte, then symbol_id) to the shared containment helper so
  input iteration order can never change the winner; (3) audit every language's relationship/pending
  pass to route containment through the same helper as its identifier pass (language-parity rule).
- Import hardening: demote the trigger from scan-fatal ABORT to **first-write-wins + observability**
  — keep the first site row, record a per-file recoverable warning with a conflict count in the
  JSON report. No change to `reference_site_id` derivation (rejected option (a): Miller's
  references-export-v2 + ADR-0004 depend on one-site-per-token identity; the bug is payload
  divergence, not under-discriminated ids). Note: per-row attribution is NOT lost — identifiers and
  pending_relationships carry their own containing/caller columns; only the denormalized site-level
  column had the disagreement.
- Tests (from the track report): PowerShell one-line `function F { G }` roundtrip; C multi-declarator
  roundtrip; shuffled-input determinism unit for the helper; hand-built divergent-spool import
  hardening test; cross-language fixture sweep asserting no site id maps to two payloads (all 38
  languages); scale regression: dotnet/runtime AND ~/.hermes/hermes-agent scan green.
- Open during work: bisect whether src/mono / eventpipe / external fail via the same C flavor;
  confirm whether trigger DDL change needs a `sqlite_schema_version` bump and note Miller's
  JulieSchemaGate tolerance.

### T4 — Spool diet (bug b). Two stages, coordinate with T5.

- Stage 1 (approved 2026-08-02): stop emitting per-identifier `code_context`
  (creation_methods.rs:102; ContextConfig types.rs:237). Verified write-only dead weight: julie's
  resolver loads-but-never-reads it; Miller never SELECTs it; symbols already dropped it in v1
  (precedent). ~47% of identifier spool bytes AND artifact bytes gone. JSONL export emits null —
  bump/document JSONL schema note. Parity: base-extractor change is language-uniform; assert
  per-language identifier row counts unchanged.
- Stage 2: re-frame the spool from one-JSON-object-per-file lines to length-prefixed binary frames
  (postcard via existing serde derives), header frame (path, file_id, content_hash, status, symbol
  ids) separate from child-row frames, spool-local integer interning for repeated hash IDs.
  Planning pass reads headers only and skips child bytes; import streams rows without materializing
  66 MB Strings. Spool is verified internal-only (created+consumed+deleted in one process; only
  writer.rs/commands.rs reference it). Keep the spool filename pattern for W4-reaper compatibility.
- Tests: frame roundtrip (byte-identical ArtifactFile); bytes-per-identifier-row ceiling regression
  (<200 B) on a dense fixture; writer contract suite green; scale assertion spool < ~2× source
  bytes on the cmov subtree; JSONL contract updated for null code_context.
- Projection: 66.6 MB worst entry → ~5-8 MB; 15.4 GB aggregate → ~1.5-2 GB; import parse CPU ~⅓.

### T5 — Artifact-write throughput (finding e). Instrument, then bulk-load mode.

- Stage 1 (must precede optimization): sub-phase timers inside artifact_write — spool passes,
  insert pass, resolution hook, wal_checkpoint — as additive `--json` profile keys (pattern:
  `record_profile_phase`). The insert-vs-resolution split is the one unverified attribution.
- Stage 2: fresh-artifact bulk-load mode, gated on the existing fresh-DB signal (writer.rs:326
  `open_path` existed/row-count check) or explicit rebuild flag: defer creation of the 54 secondary
  indexes (schema.rs:459-514) until after inserts + resolution (verified safe: fresh-path
  in-transaction reads use only PKs, UNIQUE table constraints, temp tables, full scans —
  resolution.rs:2578 full pass has no WHERE), plus rebuild journal pragmas (journal_mode=OFF/MEMORY,
  synchronous=OFF) — safe under promote-not-merge (torn .rebuild is discarded, never promoted);
  restore WAL + checkpoint before finishing. MUST never activate on live/incremental DBs (delta
  resolution needs file_id indexes — resolution.rs:2587; gate test required).
- Stage 3 (follow-up, only if Stage-1 numbers justify): multi-row batching for
  identifiers/reference_sites/pending_relationships/literals mirroring the existing chunked
  inserters (rows.rs:98); identity-guard trigger still fires per row in multi-row inserts — test.
- Tests: byte-equivalent artifact via bulk path vs current path; gate guard (never on
  write_update/delete/incremental); resolution equivalence without secondary indexes; profile keys
  sum to phase total; Scale tripwire on ms/file; crash-safety kill test mid-fresh-build.

### T6 — Scale validation + release prep. (DONE 2026-08-02 — `docs/findings/2026-08-02-scale-fixes-validation.md`)

- Full scan of dotnet/runtime @ pinned commit at default stacks: **no fatal error, per-file source
  errors only** (the corpus ships 8 non-UTF-8 files, so exit 0 is unreachable by design —
  original "exit 0, zero errors" wording corrected by T6), warnings only where designed, record
  phase timings + final DB/WAL/spool sizes in a findings doc here and update the miller-side
  baseline doc.
- **Identity-conflict assertion NOT met:** 4,237 conflicts across 28 files, all `language: c`
  (25/28 `.h`) — T3's predicted own-scope residual class. Recoverable (first-write-wins held,
  import committed); zero PowerShell conflicts. Fix is the tracked containment-helper follow-up
  (route all own-scope relationship passes through the shared helper), scheduled next cycle, and
  the release notes must record it as a known residual.
- `~/.hermes/hermes-agent` scans green. Miller repo write phase re-measured (target: severalfold
  improvement from T4+T5).
- Full `cargo test` + `xtask dogfood`; release checklist; hand coordination notes to the
  fleet-safety session (spool glob, W5 hook point). Release + pin bump gated on user approval.

### T7 — Large-artifact write scaling (added 2026-08-02 after T6's V1 exposed the wall; user-directed: multi-hour is not shippable).

T6's dotnet/runtime run proved the five original defects fixed but exposed artifact_write as a
multi-hour wall at 58,500 files (DB past 14.7 GB, RSS tracking it 1:1). Diagnosed mechanism (lead,
verified against schema + SQLite semantics): child-row tables key on random hash TEXT PRIMARY KEYs
(md5/blake3 hex), so at 100× the 128 MB page cache every insert lands on a cold random B-tree leaf;
and under T5's bulk-load `journal_mode=MEMORY`, mid-transaction cache spills put pages into the db
file whose later re-modification journals pre-images IN RAM — the MEMORY journal grows toward
O(DB size). cmov (791 MB) never showed either because it fits in cache.

- Stage 1 (mitigations): bulk-load journal goes disk-backed (DELETE/TRUNCATE rollback journal —
  keeps T5's error-rollback property without the RAM cost); bulk-load `cache_size` scaled for large
  builds (policy proposed by implementer, bounded by physical RAM; 128 MB stays the non-bulk
  default). Both scoped to the existing bulk-load path only.
- Stage 2 (the real fix, evidence-gated on Stage 1's numbers): PK-sorted child-row insertion —
  stream child rows into unindexed rowid staging tables (sequential appends), then
  `INSERT INTO <table> SELECT … ORDER BY <pk>` so the final B-tree build is a sorted sequential
  append (SQLite's canonical bulk-load pattern). Byte-equivalence + gate tests mirror T5's.
- Validation: re-run the T6 V1 dotnet/runtime scan on the new binary; today's bounded/completed
  multi-hour run is the before-number. Target: artifact_write in minutes, RSS bounded and
  independent of DB size.

## Sequencing

| Order | Task | Why |
|---|---|---|
| 1 | T1 error rendering | trivial, unblocks debugging the T2-adjacent swallowed panic |
| 2 (parallel) | T2 guards, T3 identity | independent files; both blockers for the repro targets |
| 3 | T4 spool diet | Stage 1 independent; Stage 2 shares import-pass code with T5 |
| 4 | T5 write throughput | Stage 1 anytime; Stage 2 after T4 Stage 2 (shared import passes) |
| 5 | T6 validation + release prep | needs all |
| 6 | T7 large-artifact write scaling | added after T6 V1; release holds until its re-validation |

Worker verification scope: targeted `cargo test -p julie-extract-cli` / `-p julie-extract-artifact`
(+ `-p julie-extractors` for T2/T3 extractor changes) per change; full `cargo test` + dogfood is
the lead's branch gate. All builds via `RUSTUP_TOOLCHAIN=1.97.1`.

## Contract decisions (user-approved 2026-08-02)

1. Drop per-identifier `code_context` (T4 Stage 1) — approved; JSONL export note required.
2. Demote identity trigger to first-write-wins + warning (T3) — approved; check schema-version bump.
