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
    RESOLUTION_VERSION, finalize_resolution_metadata, resolve_workspace,
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
//   measured Full  : release 1212ms, debug 3813ms
//   measured Delta : release ~110ms, debug  380ms
//
// The Delta release ceiling (175ms) sits ABOVE the 100ms design target on purpose:
// the target is currently MISSED (~110ms) because the delta pass builds the whole
// workspace index/locator/covered-set every pass — a Task 5 optimization concern
// reported as a FINDING, NOT papered over by relabeling 175ms as "the budget".
const FULL_CEIL_RELEASE: Duration = Duration::from_millis(2_000);
const FULL_CEIL_DEBUG: Duration = Duration::from_millis(8_000);
const DELTA_CEIL_RELEASE: Duration = Duration::from_millis(175);
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
    };

    let tx = conn.transaction().unwrap();
    let started = Instant::now();
    let (resolution_counts, report) = resolve_workspace(&tx, &scope).expect("full resolve");
    let elapsed = started.elapsed();
    tx.commit().unwrap();

    let outcomes = OutcomeTotals::from_rows(&report.rows);
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

    // Soft check against the design target. As of 2026-07-06 the single-file delta
    // measures ~110ms in release — a marginal MISS of the 100ms target. It is NOT
    // delta-size-driven: the pass rebuilds the whole-workspace candidate index,
    // identifier locator (all 92k identifiers) and covered-set on every invocation,
    // so the fixed O(workspace) build already exceeds the budget before any
    // delta-specific work runs. Reported as a FINDING for Task 5 optimization
    // (scope the locator/covered load to changed files, or lazy-build), NOT hidden
    // by relabeling the ceiling as the budget.
    if elapsed >= DELTA_DESIGN_TARGET {
        println!(
            "resolution_perf: FINDING — DELTA {elapsed:?} exceeds the {DELTA_DESIGN_TARGET:?} \
             design target (run profile: {}). Root cause: workspace-wide index/locator/covered \
             build is O(workspace) per delta, not O(delta). Task 5 optimization concern.",
            if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
        );
    }

    // Hard gate: measurement-derived regression ceiling (profile-aware).
    let ceil = ceiling(DELTA_CEIL_RELEASE, DELTA_CEIL_DEBUG);
    assert!(
        elapsed < ceil,
        "DELTA resolve {elapsed:?} blew the regression ceiling {ceil:?} — real perf regression, \
         do NOT relax the ceiling to make it pass",
    );
}

/// Probe for Task 5 concern #2: the by-names / by-files delta worklists are NOT
/// chunked, so a large touched-name set binds `2 * N` SQLite variables (the
/// pending queries bind terminal + receiver names). This exercises escalating
/// scales and reports where — if anywhere — the pass stops returning `Ok`.
///
/// The pass maps any storage error to a non-fatal `ResolutionHookError`, so hitting
/// the bound-variable ceiling degrades resolution rather than crashing. This test
/// asserts **no panic** and records the boundary for the lead. If a future SQLite /
/// rusqlite bump raises the ceiling above every probed scale, the test still passes
/// and prints that no boundary was hit.
#[test]
fn delta_with_huge_touched_name_set_does_not_panic() {
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
        };
        let tx = conn.transaction().unwrap();
        let (_c, report) = resolve_workspace(&tx, &full_scope).expect("warm full resolve");
        tx.commit().unwrap();
        finalize_resolution_metadata(&conn, &clean_write_result(), Some(&report));
    }

    // 2 * N bound variables on the pending queries; SQLite's compiled-in default
    // ceiling (SQLITE_MAX_VARIABLE_NUMBER) is 32766 on modern builds, so the flip
    // is expected around N ≈ 16.4k.
    let probe_sizes = [8_000usize, 16_000, 16_384, 20_000, 40_000, 80_000];
    let mut first_error_at: Option<usize> = None;

    for &n in &probe_sizes {
        let names: HashSet<String> = (0..n).map(|i| format!("probe_name_{i}")).collect();
        let scope = ResolutionScopeInput {
            changed_file_ids: Vec::new(),
            touched_symbol_names: names,
            is_full_scan: false,
        };

        // catch_unwind guards against a panic being a *real* bug (the brief's
        // "if it DOES panic, that is a real bug"). A returned Err is the graceful,
        // by-design degradation and is recorded, not asserted against.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let tx = conn.transaction().unwrap();
            let result = resolve_workspace(&tx, &scope);
            // Roll back so each probe size starts from the same steady state.
            tx.rollback().unwrap();
            result
        }));

        match outcome {
            Err(_) => panic!(
                "resolve_workspace PANICKED at touched_names={n} (2*N bound vars) — this is a \
                 REQUIRED-LEAD-FIX: the delta worklists must chunk their IN(...) binds"
            ),
            Ok(Ok(_)) => {
                println!("resolution_perf: variable-limit probe N={n:>6} -> Ok");
            }
            Ok(Err(err)) => {
                println!(
                    "resolution_perf: variable-limit probe N={n:>6} -> Err (graceful, non-fatal): {}",
                    err.message()
                );
                if first_error_at.is_none() {
                    first_error_at = Some(n);
                }
            }
        }
    }

    match first_error_at {
        Some(n) => println!(
            "resolution_perf: FINDING — delta resolution degrades to a non-fatal error at \
             touched_names >= {n}. The by-names/by-files worklists bind 2*N (pending) SQLite \
             variables and are NOT chunked (Task 5 concern #2). REQUIRED-LEAD-FIX if real deltas \
             can touch that many distinct names; not a panic/crash.",
        ),
        None => println!(
            "resolution_perf: variable-limit probe hit no ceiling up to N={} (SQLite bound-var \
             limit not reached at probed scales)",
            probe_sizes.last().unwrap()
        ),
    }
    // Contract of this probe: never a panic. The Err-vs-Ok boundary is reported,
    // not gated, because graceful degradation is the current (by-design) behavior.
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
    if index % 10 == 0 {
        "typescript"
    } else {
        "rust"
    }
}

/// Seed a v4 artifact with a realistic-mix corpus. Everything is deterministic
/// (no RNG) so two runs at the same size produce byte-identical inputs.
fn seed_artifact(conn: &mut Connection, files: usize) -> SeedCounts {
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
        let mut ins_ident = tx
            .prepare(
                "INSERT INTO identifiers \
                 (identifier_id, file_id, path, language, name, kind, containing_symbol_id, \
                  target_symbol_id, start_line, start_column, end_line, end_column, \
                  start_byte, end_byte, confidence) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, ?8, 0, ?8, 8, ?9, ?10, 1.0)",
            )
            .unwrap();
        let mut ins_rel = tx
            .prepare(
                "INSERT INTO relationships \
                 (relationship_id, from_symbol_id, to_symbol_id, file_id, path, kind, \
                  start_line, start_byte, end_byte, confidence) \
                 VALUES (?1, ?2, ?3, ?4, ?5, 'calls', ?6, ?7, ?8, 0.95)",
            )
            .unwrap();
        let mut ins_pending = tx
            .prepare(
                "INSERT INTO pending_relationships \
                 (pending_relationship_id, from_symbol_id, caller_scope_symbol_id, file_id, path, \
                  kind, target_display_name, target_terminal_name, target_receiver, \
                  target_namespace_json, start_line, start_byte, end_byte, confidence) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, '[]', ?10, ?11, ?12, 0.5)",
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
                let id = format!("id-{i}-{ident_seq}");
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
            drop(push_ident);

            // Tier-1 relationships (covered-set + propagation cost).
            for r in 0..RELATIONSHIPS_PER_FILE {
                let rid = format!("rel-{i}-{r}");
                let to = format!("gfn-{i}-{r}");
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
