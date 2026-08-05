#![cfg(feature = "test-perf")]
//! Feature-gated performance gate for the workspace reference-resolution pass
//! (`julie_extract_cli::resolution::resolve_workspace`).
//!
//! Kept out of the default suite by the test-tier convention (guarded by
//! `tests/perf_gate_convention.rs`) so the fast default gate stays cheap. Run on
//! demand with:
//!
//! ```text
//! cargo test -p julie-extract-cli --features test-perf \
//!     --test resolution_perf -- --nocapture
//! ```
//!
//! ## Why call `resolve_workspace` directly (not the binary)
//!
//! The design budgets (`docs/plans/2026-07-06-workspace-reference-resolution-design.md`,
//! §"Performance & determinism") are **Full < 2s** on a ~92k-identifier-scale
//! artifact and **single-file Delta < 100ms**. The real cost lives in the DB-bound
//! pass: the worklist queries, the once-per-pass index / locator / covered-set
//! loads, and one `record_*` round trip **per identifier** (92k of them on a Full
//! pass). Timing `resolve_one` in a loop would miss all of that. So the harness
//! seeds a v4 SQLite artifact at scale with raw inserts, opens a transaction, and
//! times the actual pass — the faithful mechanism.
//!
//! `resolve_workspace` lives in the CLI crate, which is bin-first; a thin
//! `src/lib.rs` re-exports the self-contained `resolution` module purely so this
//! integration test (which can only see a crate's public library API) can reach it.
//!
//! ## Synthetic seed shape (defaults; override with `JULIE_PERF_FILES`)
//!
//! ~2000 files → ~92k identifiers with a realistic outcome mix:
//! * 30 `member_access`/file (→ `NoContext`, the cheap-but-still-recorded majority),
//! * 12 `call`/file (6 resolvable cross-file via a globally-unique function name →
//!   tier-4 `Resolved`; 6 referencing a non-existent name → `Missing`),
//! * 4 `type_usage`/file (2 resolvable → tier-4, 2 `Missing`).
//!
//! Plus per-file symbols (module + unique target functions + a class), tier-1
//! `relationships`, `pending_relationships` (mostly tier-4 resolvable, some with a
//! receiver for tier-3), `type_facts`, and `kind='import'` symbols on the ~10% of
//! files marked `typescript` so tier-2's enabled path and the gated (`rust`) path
//! both run. Every table the pass reads or writes carries real volume so the
//! O(identifiers) record cost and the index/locator loads are genuine.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use julie_extract_artifact::model::WriteResult;
use julie_extract_artifact::resolution_store::{self, ResolutionReportRow};
use julie_extract_artifact::schema::create_schema;
use julie_extract_artifact::writer::ResolutionScopeInput;
use julie_extract_cli::resolution::{
    DELTA_SCOPE_CROSSOVER as CROSSOVER_THRESHOLD, RESOLUTION_VERSION, finalize_resolution_metadata,
    resolve_workspace, resolve_workspace_with_crossover,
};
use rusqlite::Connection;

// --- Design targets (design §"Performance & determinism"): Full < 2s on a
// ~92k-identifier artifact, single-file Delta < 100ms. These are the *release*
// product contract. They are printed and compared against as FINDINGS, but they
// are NOT the hard gate — the design's rule is "budgets move to measurement", so
// the hard gate is a measurement-derived regression ceiling (below).
const FULL_DESIGN_TARGET: Duration = Duration::from_secs(2);
const DELTA_DESIGN_TARGET: Duration = Duration::from_millis(100);

// --- Hard-gate regression ceilings, derived from measured numbers × headroom on
// 2026-07-06 (Apple Silicon, in-memory SQLite, --test-threads=1). Debug (`cargo
// test`) is ~3x slower than release, so the gate is profile-aware: the default
// debug invocation still runs and catches gross regressions without demanding
// release-only speed. Headroom is deliberately generous (CI hosts are noisy),
// mirroring `writer_perf.rs`'s soft-floor convention.
//
//   measured Full  : release  805ms (2026-08-05; was 1247ms on 2026-07-06)
//   measured Delta : release   51ms (2026-08-05; the historical 81ms figure was
//                    84% workspace-wide `resolution_report` — see
//                    docs/findings/2026-08-05-single-file-delta-172ms-attribution.md)
//
// The Delta pass scopes its identifier locator + covered-set load to the files the
// delta touches (an O(delta) load, since every co-location join is same-file) and
// skips the O(workspace) report aggregate entirely, so single-file Delta MEETS the
// 100ms design target (51ms release). The release ceiling (150ms) keeps generous
// headroom over the measured 51ms for noisy CI hosts.
const FULL_CEIL_RELEASE: Duration = Duration::from_millis(2_000);
const FULL_CEIL_DEBUG: Duration = Duration::from_millis(8_000);
const DELTA_CEIL_RELEASE: Duration = Duration::from_millis(150);
const DELTA_CEIL_DEBUG: Duration = Duration::from_millis(750);

/// Pick the profile-appropriate regression ceiling.
fn ceiling(release: Duration, debug: Duration) -> Duration {
    if cfg!(debug_assertions) {
        debug
    } else {
        release
    }
}

// --- Default seed sizing. ~2000 files * 46 identifiers/file ≈ 92k identifiers.
const DEFAULT_FILES: usize = 2_000;
const MEMBER_ACCESS_PER_FILE: usize = 30;
const CALLS_PER_FILE: usize = 12; // first half resolvable, second half missing
const TYPE_USAGES_PER_FILE: usize = 4; // first half resolvable, second half missing
const TARGET_FNS_PER_FILE: usize = 6; // globally-unique tier-4 call targets
const PENDING_PER_FILE: usize = 3; // 2 tier-4 resolvable, 1 tier-3 receiver
const RELATIONSHIPS_PER_FILE: usize = 2; // tier-1, for covered-set + propagation
const IMPORTS_PER_TS_FILE: usize = 5;

#[test]
fn full_resolve_is_within_budget_at_scale() {
    let files = env_usize("JULIE_PERF_FILES", DEFAULT_FILES);
    let mut conn = fresh_artifact();
    let counts = seed_artifact(&mut conn, files);
    print_seed("full", &counts);

    let scope = ResolutionScopeInput {
        changed_file_ids: Vec::new(),
        touched_symbol_names: HashSet::new(),
        is_full_scan: true,
        whole_corpus: true,
    };

    let tx = conn.transaction().unwrap();
    let started = Instant::now();
    let (resolution_counts, report) = resolve_workspace(&tx, &scope).expect("full resolve");
    let elapsed = started.elapsed();
    tx.commit().unwrap();

    let outcomes = OutcomeTotals::from_rows(
        report
            .rows
            .as_deref()
            .expect("a full pass carries the aggregate rows"),
    );
    println!(
        "resolution_perf: FULL resolve {} ms | identifiers={} pending={} | overlay writes: \
         identifier_resolutions={} pending_resolutions={} | outcomes: resolved={} ambiguous={} \
         missing={} no_context={} | status={:?} | budget={} ms (headroom {:.1}x)",
        elapsed.as_millis(),
        counts.identifiers,
        counts.pending,
        resolution_counts.identifier_resolutions,
        resolution_counts.pending_resolutions,
        outcomes.resolved,
        outcomes.ambiguous,
        outcomes.missing,
        outcomes.no_context,
        report.status,
        FULL_DESIGN_TARGET.as_millis(),
        FULL_DESIGN_TARGET.as_secs_f64() / elapsed.as_secs_f64().max(1e-9),
    );

    // Sanity: the pass must actually have done the O(identifiers) work — one overlay
    // row recorded per identifier.
    assert!(
        resolution_counts.identifier_resolutions as usize >= counts.identifiers,
        "full pass recorded {} identifier outcomes but seeded {} identifiers — the \
         O(identifiers) cost was not exercised",
        resolution_counts.identifier_resolutions,
        counts.identifiers,
    );
    assert!(
        outcomes.resolved > 0,
        "expected some tier-4 cross-file resolutions in the outcome mix"
    );

    // Soft check against the design target — a miss is a reported FINDING, not a
    // gate failure (the hard gate is the regression ceiling below).
    if elapsed >= FULL_DESIGN_TARGET {
        println!(
            "resolution_perf: FINDING — FULL {elapsed:?} exceeds the {FULL_DESIGN_TARGET:?} \
             design target at {} identifiers (release profile is the contract; this run is {}). \
             Flag Task 5's pass for optimization.",
            counts.identifiers,
            if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
        );
    }

    // Hard gate: measurement-derived regression ceiling (profile-aware).
    let ceil = ceiling(FULL_CEIL_RELEASE, FULL_CEIL_DEBUG);
    assert!(
        elapsed < ceil,
        "FULL resolve {elapsed:?} blew the regression ceiling {ceil:?} at {} identifiers — real \
         perf regression, do NOT relax the ceiling to make it pass",
        counts.identifiers,
    );
}

// --- Savepoint-seam gate. The other two tests call `resolve_workspace` directly
// on a bare transaction, which is the harness blind spot that let the v2.9.0
// quadratic ship: the pass only pays the `memjrnlTruncate` cost when it runs
// inside the writer's OPEN `SAVEPOINT resolution_hook` (a full scan of the
// julie-extractors repo went 6.5s -> 425s; a `sample` showed 11,789/11,797 stacks
// in `memjrnlTruncate`). Each statement-end truncates the savepoint sub-journal by
// walking its ever-growing chunk list from the head, so ~125k per-row statements
// inside the open savepoint is quadratic. This test reproduces the exact seam and
// gates wall-clock. The seed is scaled down (so the FIXED pass is fast) but large
// enough that the quadratic clearly blows the ceiling on the unfixed code.
// ~1000 files * 46 identifiers/file ≈ 46k identifier outcomes. At this scale the
// unfixed quadratic measures ~12s through the seam (RED), while the batched flush
// brings it to ~1s (GREEN) — a clear, noise-proof gap around the 5s ceiling.
const SEAM_FILES: usize = 1_000;
const SEAM_CEIL: Duration = Duration::from_secs(5);

#[test]
fn full_resolve_through_savepoint_seam_is_within_budget() {
    let files = env_usize("JULIE_PERF_SEAM_FILES", SEAM_FILES);
    // On-disk artifact (NOT in-memory): the quadratic `memjrnlTruncate` cost only
    // manifests when the savepoint sub-journal accumulates pages spread across a
    // real on-disk table — an in-memory DB stays linear and would hide the bug.
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("seam.db");
    let mut conn = Connection::open(&db_path).unwrap();
    // Mirror the production writer's connection PRAGMAs (writer.rs `open`). The
    // load-bearing one is `temp_store = MEMORY`: it keeps the savepoint sub-journal
    // as an in-memory `memjrnl` that GROWS instead of spilling to a cheap temp file,
    // so `memjrnlTruncate` walks an ever-longer chunk list at every statement end —
    // the exact production hot path (11,789/11,797 `sample` stacks). Without these
    // the sub-journal spills and the quadratic hides.
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    conn.pragma_update(None, "synchronous", "NORMAL").unwrap();
    conn.pragma_update(None, "temp_store", "MEMORY").unwrap();
    conn.pragma_update(None, "cache_size", -131_072i64).unwrap();
    create_schema(&conn).unwrap();
    // Scattered ids (scatter_ids = true) reproduce production's content-hash
    // identifier ids, which is what makes the open-savepoint sub-journal grow per
    // statement and exposes the quadratic.
    let counts = seed_artifact_with(&mut conn, files, true);
    print_seed("seam", &counts);

    let scope = ResolutionScopeInput {
        changed_file_ids: Vec::new(),
        touched_symbol_names: HashSet::new(),
        is_full_scan: true,
        whole_corpus: true,
    };

    // Mirror `run_resolution_hook` (writer.rs): open the same savepoint, run the
    // pass, release. This is the seam the two direct-call tests bypass.
    let tx = conn.transaction().unwrap();
    tx.execute_batch("SAVEPOINT resolution_hook").unwrap();
    let started = Instant::now();
    let (resolution_counts, _report) = resolve_workspace(&tx, &scope).expect("seam full resolve");
    let elapsed = started.elapsed();
    tx.execute_batch("RELEASE resolution_hook").unwrap();
    tx.commit().unwrap();

    println!(
        "resolution_perf: SEAM full resolve (through open SAVEPOINT) {} ms | identifiers={} \
         | overlay writes: identifier_resolutions={} pending_resolutions={} | ceiling={} ms",
        elapsed.as_millis(),
        counts.identifiers,
        resolution_counts.identifier_resolutions,
        resolution_counts.pending_resolutions,
        SEAM_CEIL.as_millis(),
    );

    // Sanity: the pass must have done the O(identifiers) write work inside the
    // savepoint (otherwise a trivially fast run would pass the gate for free).
    assert!(
        resolution_counts.identifier_resolutions as usize >= counts.identifiers,
        "seam pass recorded {} identifier outcomes but seeded {} identifiers — the \
         O(identifiers) write cost was not exercised inside the savepoint",
        resolution_counts.identifier_resolutions,
        counts.identifiers,
    );

    // Hard gate: through the savepoint seam the pass must stay linear. Unfixed
    // v2.9.0 blows this by ~10x+ (tens of seconds at this scale); the batched flush
    // brings it back under a second.
    assert!(
        elapsed < SEAM_CEIL,
        "FULL resolve THROUGH the open savepoint seam took {elapsed:?}, over the {SEAM_CEIL:?} \
         ceiling at {} identifiers — the quadratic memjrnl-truncate regression is back. The pass \
         must batch its overlay writes so only a FEW statements end inside the open savepoint, \
         not one per row. Do NOT relax the ceiling to make it pass.",
        counts.identifiers,
    );
}

#[test]
fn single_file_delta_is_within_budget() {
    let files = env_usize("JULIE_PERF_FILES", DEFAULT_FILES);
    let mut conn = fresh_artifact();
    let counts = seed_artifact(&mut conn, files);
    print_seed("delta", &counts);

    // Reach steady state: run a Full pass so the overlay is populated and durable
    // metadata exists, exactly as a real repo would be before an incremental edit.
    {
        let full_scope = ResolutionScopeInput {
            changed_file_ids: Vec::new(),
            touched_symbol_names: HashSet::new(),
            is_full_scan: true,
            whole_corpus: true,
        };
        let tx = conn.transaction().unwrap();
        let (_c, report) = resolve_workspace(&tx, &full_scope).expect("warm full resolve");
        tx.commit().unwrap();
        // Post-commit metadata write (mirrors the CLI) so the delta pass sees prior
        // metadata and takes the Delta branch instead of a v3 backfill Full.
        finalize_resolution_metadata(&conn, &clean_write_result(), Some(&report));
        let meta = resolution_store::read_resolution_metadata(&conn)
            .unwrap()
            .expect("resolution metadata present after warm full");
        assert_eq!(meta.version, RESOLUTION_VERSION);
    }

    // A single changed file with a handful of touched names — the incremental-edit
    // shape the < 100ms budget targets.
    let changed_file = format!("file-{}", files / 2);
    let touched: HashSet<String> = (0..TARGET_FNS_PER_FILE)
        .map(|k| format!("gfn_{}_{}", files / 2, k))
        .collect();
    let scope = ResolutionScopeInput {
        changed_file_ids: vec![changed_file.clone()],
        touched_symbol_names: touched.clone(),
        is_full_scan: false,
        whole_corpus: false,
    };

    let tx = conn.transaction().unwrap();
    let started = Instant::now();
    let _ = resolve_workspace(&tx, &scope).expect("delta resolve");
    let elapsed = started.elapsed();
    tx.commit().unwrap();

    println!(
        "resolution_perf: DELTA (1 file, {} touched names) {} ms | design target={} ms (headroom {:.1}x)",
        touched.len(),
        elapsed.as_millis(),
        DELTA_DESIGN_TARGET.as_millis(),
        DELTA_DESIGN_TARGET.as_secs_f64() / elapsed.as_secs_f64().max(1e-9),
    );

    // Soft check against the design target. The delta pass scopes its identifier
    // locator + covered-set load to the touched files (O(delta), not O(workspace))
    // and skips the workspace-wide report aggregate, so single-file delta measures
    // ~51ms release — it MEETS the 100ms target. Debug builds run ~3x slower and may
    // exceed it; that is expected and only noted. The hard gate is the regression
    // ceiling below.
    if elapsed < DELTA_DESIGN_TARGET {
        println!(
            "resolution_perf: DELTA {elapsed:?} MEETS the {DELTA_DESIGN_TARGET:?} design target \
             (run profile: {}).",
            if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
        );
    } else {
        println!(
            "resolution_perf: NOTE — DELTA {elapsed:?} exceeds the {DELTA_DESIGN_TARGET:?} design \
             target (run profile: {}); expected in debug, investigate if seen in release.",
            if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
        );
    }

    // Hard gate: measurement-derived regression ceiling (profile-aware).
    //
    // This gate was RED at 172–180 ms on 2026-08-05 after the fixture repair let it run
    // again (it had been dead since `reference_sites` became a NOT NULL FK the seed did
    // not satisfy). Bisected root cause — see
    // docs/findings/2026-08-05-single-file-delta-172ms-attribution.md: the timed pass
    // ended with the workspace-wide `resolution_report` aggregate (69 ms of the original
    // 82 ms figure, 131 ms of the red 180 ms); 6941e05's report rewrite over the base
    // tables was the single-commit 82 -> 146 ms jump, and the reference-sites era
    // doubled the real delta work (~18 -> ~49 ms). Deltas no longer compute the
    // aggregate (`ResolutionReport::rows` is `None` on a scoped pass), so this gate now
    // times actual delta resolution. If it goes red again, that is a real delta-path
    // regression — bisect it; do not relax the ceiling to get a green run.
    let ceil = ceiling(DELTA_CEIL_RELEASE, DELTA_CEIL_DEBUG);
    assert!(
        elapsed < ceil,
        "DELTA resolve {elapsed:?} blew the regression ceiling {ceil:?} — real perf regression, \
         do NOT relax the ceiling to make it pass",
    );
}

/// Regression guard for the by-names / by-files delta worklists: they bind up to
/// `2 * N` SQLite variables (the pending queries bind terminal + receiver names),
/// so a delta touching a large distinct-name set once overflowed SQLite's compiled
/// `SQLITE_MAX_VARIABLE_NUMBER` (32766, i.e. N ≈ 16.4k) and degraded to a non-fatal
/// error. The worklists now chunk their `IN (...)` binds, so a huge touched-name
/// set must RESOLVE, not degrade. This probes escalating scales well past the old
/// boundary and asserts the pass returns `Ok` (and never panics) at every one; a
/// returned `Err` now means the chunking regressed.
#[test]
fn delta_with_huge_touched_name_set_resolves_via_chunking() {
    let mut conn = fresh_artifact();
    // Small artifact — the probe is about the query's bound-variable count, driven
    // by the scope name-set size, not the corpus size.
    let counts = seed_artifact(&mut conn, 8);
    println!(
        "resolution_perf: variable-limit probe seeded files=8 identifiers={}",
        counts.identifiers
    );

    // Steady state so the Delta branch (with the by-names worklists) runs.
    {
        let full_scope = ResolutionScopeInput {
            changed_file_ids: Vec::new(),
            touched_symbol_names: HashSet::new(),
            is_full_scan: true,
            whole_corpus: true,
        };
        let tx = conn.transaction().unwrap();
        let (_c, report) = resolve_workspace(&tx, &full_scope).expect("warm full resolve");
        tx.commit().unwrap();
        finalize_resolution_metadata(&conn, &clean_write_result(), Some(&report));
    }

    // Probe scales straddling and well past the old ~16.4k boundary (2*N > 32766).
    // With chunked binds every one must resolve.
    let probe_sizes = [8_000usize, 16_000, 16_384, 20_000, 40_000, 80_000];

    for &n in &probe_sizes {
        let names: HashSet<String> = (0..n).map(|i| format!("probe_name_{i}")).collect();
        let scope = ResolutionScopeInput {
            changed_file_ids: Vec::new(),
            touched_symbol_names: names,
            is_full_scan: false,
            whole_corpus: false,
        };

        // catch_unwind so a panic is reported as the real bug it would be, distinct
        // from a returned Err.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let tx = conn.transaction().unwrap();
            let result = resolve_workspace(&tx, &scope);
            // Roll back so each probe size starts from the same steady state.
            tx.rollback().unwrap();
            result
        }));

        match outcome {
            Err(_) => panic!(
                "resolve_workspace PANICKED at touched_names={n} (2*N bound vars) — the delta \
                 worklists must chunk their IN(...) binds"
            ),
            Ok(Ok(_)) => {
                println!("resolution_perf: chunked delta N={n:>6} -> Ok");
            }
            Ok(Err(err)) => panic!(
                "resolve_workspace returned Err at touched_names={n} ({}) — the by-names/by-files \
                 worklists must chunk their IN(...) binds under SQLITE_MAX_VARIABLE_NUMBER so a \
                 huge delta RESOLVES instead of degrading",
                err.message()
            ),
        }
    }

    println!(
        "resolution_perf: chunked delta resolved at all probed scales up to N={}",
        probe_sizes.last().unwrap()
    );
}

// ---------------------------------------------------------------------------
// Seed
// ---------------------------------------------------------------------------

struct SeedCounts {
    files: usize,
    symbols: usize,
    identifiers: usize,
    pending: usize,
    relationships: usize,
    type_facts: usize,
    imports: usize,
    member_access: usize,
}

fn print_seed(tag: &str, c: &SeedCounts) {
    println!(
        "resolution_perf: [{tag}] seed files={} symbols={} identifiers={} \
         (member_access={}) pending={} relationships={} type_facts={} imports={}",
        c.files,
        c.symbols,
        c.identifiers,
        c.member_access,
        c.pending,
        c.relationships,
        c.type_facts,
        c.imports,
    );
}

fn fresh_artifact() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn).unwrap();
    conn
}

fn lang(index: usize) -> &'static str {
    if index.is_multiple_of(10) {
        "typescript"
    } else {
        "rust"
    }
}

/// Bijective scramble of a sequence counter (odd multiplier mod 2^32 is a
/// permutation), so `identifier_id` sort order is UNCORRELATED with insertion
/// (row) order. This reproduces production's content-hash ids: the resolution
/// worklists `ORDER BY identifier_id`, so scattered ids make every overlay INSERT
/// (`identifier_resolutions` PK b-tree) and denorm UPDATE (`idx_identifiers_target`
/// b-tree) dirty a fresh page — which is what makes the open-savepoint sub-journal
/// grow per statement and turns the pass quadratic. Sequential ids stay on the same
/// pages and hide the bug.
fn scramble_id(seq: usize) -> String {
    let key = (seq as u64).wrapping_mul(2_654_435_761).wrapping_add(1) & 0xFFFF_FFFF;
    format!("id-{key:010}")
}

/// Seed a v4 artifact with a realistic-mix corpus. Everything is deterministic
/// (no RNG) so two runs at the same size produce byte-identical inputs. When
/// `scatter_ids` is set, identifier ids are scrambled (see [`scramble_id`]) so the
/// savepoint-seam gate observes the real quadratic; the two direct-call tests use
/// sequential ids (`scatter_ids = false`) to keep their loads cheap.
fn seed_artifact(conn: &mut Connection, files: usize) -> SeedCounts {
    seed_artifact_with(conn, files, false)
}

fn seed_artifact_with(conn: &mut Connection, files: usize, scatter_ids: bool) -> SeedCounts {
    // Same-language peer file for cross-file tier-4 targets, so a resolvable call's
    // referent lives in a *different* file of the *same* language.
    let same_lang_peer = |i: usize| -> usize {
        let mut j = (i + 1) % files;
        while lang(j) != lang(i) && j != i {
            j = (j + 1) % files;
        }
        j
    };

    let tx = conn.transaction().unwrap();

    tx.execute(
        "INSERT INTO extraction_revisions \
         (revision_id, parent_revision_id, operation, mode, started_at, completed_at, \
          binary_version, extract_contract_version, sqlite_schema_version, input_root, counts_json) \
         VALUES (1, NULL, 'scan', 'full', '2026-07-06T00:00:00Z', '2026-07-06T00:00:01Z', \
                 'julie-extract perf', 3, 4, '/repo', '{}')",
        [],
    )
    .unwrap();

    let mut symbols = 0usize;
    let mut identifiers = 0usize;
    let mut member_access = 0usize;
    let mut pending = 0usize;
    let mut relationships = 0usize;
    let mut type_facts = 0usize;
    let mut imports = 0usize;
    // Global identifier counter (across files) so scrambled ids scatter over the
    // whole corpus, not just within a file.
    let mut global_ident_seq = 0usize;

    {
        let mut ins_file = tx
            .prepare(
                "INSERT INTO files \
                 (file_id, path, language, content_hash, content_bytes, line_count, \
                  indexed_at, last_revision_id, status) \
                 VALUES (?1, ?2, ?3, 'hash', 1024, 256, '2026-07-06T00:00:00Z', 1, 'indexed')",
            )
            .unwrap();
        let mut ins_symbol = tx
            .prepare(
                "INSERT INTO symbols \
                 (symbol_id, file_id, path, language, name, kind, parent_symbol_id, \
                  start_line, start_column, end_line, end_column, start_byte, end_byte, metadata_json) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, 0, ?8, 1, ?9, ?10, ?11)",
            )
            .unwrap();
        // `identifiers` and `relationships` both carry a NOT NULL FK to
        // `reference_sites`, so every row needs its site first. Exact sites here:
        // the schema's CHECK ties `is_exact = 1` to `provenance = 'target_token'`
        // and to all six span columns being present.
        let mut ins_site = tx
            .prepare(
                "INSERT INTO reference_sites \
                 (reference_site_id, file_id, path, language, containing_symbol_id, \
                  start_line, start_column, end_line, end_column, start_byte, end_byte, \
                  is_exact, provenance) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?6, 8, ?7, ?8, 1, 'target_token')",
            )
            .unwrap();
        let mut ins_ident = tx
            .prepare(
                "INSERT INTO identifiers \
                 (identifier_id, reference_site_id, file_id, path, language, name, kind, \
                  containing_symbol_id, target_symbol_id, start_line, start_column, end_line, \
                  end_column, start_byte, end_byte, confidence) \
                 VALUES (?1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, ?8, 0, ?8, 8, ?9, ?10, 1.0)",
            )
            .unwrap();
        let mut ins_rel = tx
            .prepare(
                "INSERT INTO relationships \
                 (relationship_id, reference_site_id, from_symbol_id, to_symbol_id, file_id, \
                  path, kind, start_line, start_byte, end_byte, confidence) \
                 VALUES (?1, ?1, ?2, ?3, ?4, ?5, 'calls', ?6, ?7, ?8, 0.95)",
            )
            .unwrap();
        let mut ins_pending = tx
            .prepare(
                "INSERT INTO pending_relationships \
                 (pending_relationship_id, reference_site_id, from_symbol_id, \
                  caller_scope_symbol_id, file_id, path, kind, target_display_name, \
                  target_terminal_name, target_receiver, target_namespace_json, start_line, \
                  start_byte, end_byte, confidence) \
                 VALUES (?1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, '[]', ?10, ?11, ?12, 0.5)",
            )
            .unwrap();
        let mut ins_type_fact = tx
            .prepare(
                "INSERT INTO type_facts \
                 (type_fact_id, symbol_id, language, resolved_type, is_inferred) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )
            .unwrap();

        for i in 0..files {
            let l = lang(i);
            let path = format!("src/file_{i}.rs");
            ins_file.execute((&format!("file-{i}"), &path, l)).unwrap();

            let file_id = format!("file-{i}");
            let module_id = format!("mod-{i}");
            let class_id = format!("cls-{i}");
            let class_name = format!("Cls_{i}");

            // Module container (parent/scope/containing symbol for this file).
            ins_symbol
                .execute((
                    &module_id,
                    &file_id,
                    &path,
                    l,
                    &format!("module_{i}"),
                    "module",
                    1i64,
                    1i64,
                    0i64,
                    16i64,
                    &Option::<String>::None,
                ))
                .unwrap();
            symbols += 1;

            // A unique class (tier-4 type_usage target).
            ins_symbol
                .execute((
                    &class_id,
                    &file_id,
                    &path,
                    l,
                    &class_name,
                    "class",
                    2i64,
                    2i64,
                    0i64,
                    32i64,
                    &Option::<String>::None,
                ))
                .unwrap();
            symbols += 1;

            // Globally-unique target functions (tier-4 call targets).
            for k in 0..TARGET_FNS_PER_FILE {
                let sid = format!("gfn-{i}-{k}");
                let name = format!("gfn_{i}_{k}");
                ins_symbol
                    .execute((
                        &sid,
                        &file_id,
                        &path,
                        l,
                        &name,
                        "function",
                        (10 + k) as i64,
                        (10 + k) as i64,
                        0i64,
                        (100 + k) as i64,
                        &Option::<String>::None,
                    ))
                    .unwrap();
                symbols += 1;
            }

            // A type fact on the module (index-load volume; tier-3 receiver fuel).
            ins_type_fact
                .execute((
                    &format!("tf-{i}"),
                    &module_id,
                    l,
                    &class_name,
                    (i % 2) as i64,
                ))
                .unwrap();
            type_facts += 1;

            // Import symbols on typescript files → tier-2 enabled path + import
            // index-load volume. Rust files exercise the tier-2 *gated* path.
            if l == "typescript" {
                for m in 0..IMPORTS_PER_TS_FILE {
                    let sid = format!("imp-{i}-{m}");
                    let peer = same_lang_peer(i);
                    // Local binding equals a peer file's target function name so a ts
                    // call keyed on it can resolve via tier 2.
                    let name = format!("gfn_{peer}_{m}");
                    let meta = format!("{{\"imported_name\":\"gfn_{peer}_{m}\"}}");
                    ins_symbol
                        .execute((
                            &sid,
                            &file_id,
                            &path,
                            l,
                            &name,
                            "import",
                            (50 + m) as i64,
                            (50 + m) as i64,
                            0i64,
                            (500 + m) as i64,
                            &Some(meta),
                        ))
                        .unwrap();
                    symbols += 1;
                    imports += 1;
                }
            }

            let peer = same_lang_peer(i);

            // Identifiers — the dominant volume. Distinct byte ranges keep them off
            // the pending/relationship spans so the generic chain owns them.
            let mut byte = 10_000i64;
            let mut line = 200i64;
            let mut ident_seq = 0usize;
            let mut push_ident = |kind: &str, name: &str| {
                let id = if scatter_ids {
                    scramble_id(global_ident_seq)
                } else {
                    format!("id-{i}-{ident_seq}")
                };
                global_ident_seq += 1;
                ins_site
                    .execute((&id, &file_id, &path, l, &module_id, line, byte, byte + 8))
                    .unwrap();
                ins_ident
                    .execute((
                        &id,
                        &file_id,
                        &path,
                        l,
                        name,
                        kind,
                        &module_id,
                        line,
                        byte,
                        byte + 8,
                    ))
                    .unwrap();
                ident_seq += 1;
                byte += 16;
                line += 1;
            };

            for j in 0..MEMBER_ACCESS_PER_FILE {
                push_ident("member_access", &format!("field_{i}_{j}"));
                member_access += 1;
                identifiers += 1;
            }
            for j in 0..CALLS_PER_FILE {
                if j < CALLS_PER_FILE / 2 {
                    // Resolvable cross-file: unique function name owned by a peer file.
                    push_ident("call", &format!("gfn_{peer}_{}", j % TARGET_FNS_PER_FILE));
                } else {
                    // No such symbol anywhere → Missing.
                    push_ident("call", &format!("nofn_{i}_{j}"));
                }
                identifiers += 1;
            }
            for j in 0..TYPE_USAGES_PER_FILE {
                if j < TYPE_USAGES_PER_FILE / 2 {
                    push_ident("type_usage", &format!("Cls_{peer}"));
                } else {
                    push_ident("type_usage", &format!("NoCls_{i}_{j}"));
                }
                identifiers += 1;
            }
            let _ = push_ident; // end the closure's &mut borrows before reusing the vectors

            // Tier-1 relationships (covered-set + propagation cost).
            for r in 0..RELATIONSHIPS_PER_FILE {
                let rid = format!("rel-{i}-{r}");
                let to = format!("gfn-{i}-{r}");
                ins_site
                    .execute((
                        &rid,
                        &file_id,
                        &path,
                        l,
                        &module_id,
                        (300 + r) as i64,
                        (20_000 + r * 16) as i64,
                        (20_008 + r * 16) as i64,
                    ))
                    .unwrap();
                ins_rel
                    .execute((
                        &rid,
                        &module_id,
                        &to,
                        &file_id,
                        &path,
                        (300 + r) as i64,
                        (20_000 + r * 16) as i64,
                        (20_008 + r * 16) as i64,
                    ))
                    .unwrap();
                relationships += 1;
            }

            // Pending relationships: mostly tier-4 resolvable, one tier-3 receiver.
            for p in 0..PENDING_PER_FILE {
                let pid = format!("pend-{i}-{p}");
                let (terminal, receiver): (String, Option<String>) = if p < PENDING_PER_FILE - 1 {
                    (format!("gfn_{peer}_{p}"), None)
                } else {
                    // Receiver-typed: caller scope's type_fact is `Cls_{i}`; terminal
                    // is a method-ish name (tier-3 will look up members of the type).
                    (format!("method_{i}_{p}"), Some(format!("recv_{i}")))
                };
                ins_site
                    .execute((
                        &pid,
                        &file_id,
                        &path,
                        l,
                        &module_id,
                        (400 + p) as i64,
                        (30_000 + p * 16) as i64,
                        (30_008 + p * 16) as i64,
                    ))
                    .unwrap();
                ins_pending
                    .execute((
                        &pid,
                        &module_id,
                        &module_id,
                        &file_id,
                        &path,
                        "calls",
                        &terminal,
                        &terminal,
                        &receiver,
                        (400 + p) as i64,
                        (30_000 + p * 16) as i64,
                        (30_008 + p * 16) as i64,
                    ))
                    .unwrap();
                pending += 1;
            }
        }
    }

    tx.commit().unwrap();

    SeedCounts {
        files,
        symbols,
        identifiers,
        pending,
        relationships,
        type_facts,
        imports,
        member_access,
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// A `WriteResult` with no resolution failure, for driving `finalize_resolution_metadata`
/// exactly as the CLI does after a successful commit (`resolution.failed == None`).
fn clean_write_result() -> WriteResult {
    WriteResult::default()
}

/// Per-outcome totals summed across the report's per-language/per-tier rows.
#[derive(Default)]
struct OutcomeTotals {
    resolved: i64,
    ambiguous: i64,
    missing: i64,
    no_context: i64,
}

impl OutcomeTotals {
    fn from_rows(rows: &[ResolutionReportRow]) -> Self {
        let mut totals = OutcomeTotals::default();
        for row in rows {
            match row.outcome.as_str() {
                "resolved" => totals.resolved += row.count,
                "ambiguous" => totals.ambiguous += row.count,
                "missing" => totals.missing += row.count,
                "no_context" => totals.no_context += row.count,
                _ => {}
            }
        }
        totals
    }
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

/// Where a widened delta scope stops being cheaper than one Full pass.
///
/// The crossover in `resolution.rs` decides this at runtime, and its threshold was a
/// placeholder until this arm existed. Sweeping N changed files against a Full pass on
/// the same artifact turns it into a measured number: below the crossing a scoped pass
/// wins, above it the scoped path does everything Full does and pays chunked `IN`
/// clauses and per-file bookkeeping on top.
///
/// Reports rather than asserting a ratio — the crossing moves with machine and corpus,
/// and a hard ceiling here is the wall-clock leak `test_tiers` exists to keep out. The
/// one thing it does assert is that the shipped threshold sits on the correct side of
/// what was just measured.
#[test]
fn delta_scope_crossover_sweep() {
    // Above any real fraction, so the scoped path stays scoped while being measured.
    const NO_PROMOTION: f64 = 2.0;

    let files = env_usize("JULIE_PERF_FILES", DEFAULT_FILES);
    let mut conn = fresh_artifact();
    let counts = seed_artifact(&mut conn, files);
    print_seed("crossover", &counts);

    let warm = |conn: &mut rusqlite::Connection| {
        let full_scope = ResolutionScopeInput {
            changed_file_ids: Vec::new(),
            touched_symbol_names: HashSet::new(),
            is_full_scan: true,
            whole_corpus: true,
        };
        let tx = conn.transaction().unwrap();
        let (_c, report) = resolve_workspace(&tx, &full_scope).expect("warm full resolve");
        tx.commit().unwrap();
        finalize_resolution_metadata(conn, &clean_write_result(), Some(&report));
    };
    warm(&mut conn);

    let full_elapsed = {
        let full_scope = ResolutionScopeInput {
            changed_file_ids: Vec::new(),
            touched_symbol_names: HashSet::new(),
            is_full_scan: true,
            whole_corpus: true,
        };
        let tx = conn.transaction().unwrap();
        let started = Instant::now();
        let _ = resolve_workspace(&tx, &full_scope).expect("full resolve");
        let elapsed = started.elapsed();
        tx.commit().unwrap();
        elapsed
    };
    println!(
        "resolution_perf: FULL baseline {} ms over {} files",
        full_elapsed.as_millis(),
        files,
    );

    // Changed-file counts spanning the decision, clamped to the corpus.
    let mut crossing: Option<f64> = None;
    // Dense between half and the whole corpus: the crossing sits in there, and coarse
    // points would leave the shipped threshold resting on an interpolation.
    for n in [
        1usize,
        50,
        500,
        files / 2,
        files * 3 / 5,
        files * 7 / 10,
        files * 4 / 5,
        files * 9 / 10,
        files,
    ]
    .into_iter()
    .filter(|n| *n <= files)
    {
        // Names are the file's own symbols, so the widening unions stay proportional
        // to N rather than pulling the workspace in through one hot shared name.
        let changed_file_ids: Vec<String> = (0..n).map(|i| format!("file-{i}")).collect();
        let touched: HashSet<String> = (0..n)
            .flat_map(|i| (0..TARGET_FNS_PER_FILE).map(move |k| format!("gfn_{i}_{k}")))
            .collect();
        let scope = ResolutionScopeInput {
            changed_file_ids,
            touched_symbol_names: touched,
            is_full_scan: false,
            whole_corpus: true,
        };
        let tx = conn.transaction().unwrap();
        let started = Instant::now();
        let _ = resolve_workspace_with_crossover(&tx, &scope, NO_PROMOTION).expect("delta resolve");
        let elapsed = started.elapsed();
        tx.commit().unwrap();

        let fraction = n as f64 / files as f64;
        let ratio = elapsed.as_secs_f64() / full_elapsed.as_secs_f64().max(1e-9);
        println!(
            "resolution_perf: DELTA n={n:<5} ({:>5.1}% of corpus) {:>6} ms | {:.2}x FULL",
            fraction * 100.0,
            elapsed.as_millis(),
            ratio,
        );
        if ratio >= 1.0 && crossing.is_none() {
            crossing = Some(fraction);
        }
    }

    match crossing {
        Some(fraction) => {
            println!(
                "resolution_perf: measured crossover at ~{:.0}% of the corpus; shipped threshold {:.0}%",
                fraction * 100.0,
                CROSSOVER_THRESHOLD * 100.0,
            );
            assert!(
                CROSSOVER_THRESHOLD <= fraction + f64::EPSILON,
                "the shipped crossover threshold ({:.2}) promotes to Full LATER than the measured \
                 crossing ({:.2}), so scopes between the two run the scoped path when Full is \
                 already cheaper. Lower DELTA_SCOPE_CROSSOVER to at most the measured value.",
                CROSSOVER_THRESHOLD,
                fraction,
            );
        }
        None => println!(
            "resolution_perf: no crossing up to the whole corpus — a scoped pass never lost, so \
             the shipped threshold only ever costs an unnecessary promotion to Full",
        ),
    }
}
