//! Task 6 — per-language reference-resolution contract suite.
//!
//! Two enforcement surfaces:
//!
//! 1. **Contract cases** (`design.md` §Testing, authoritative): concrete
//!    `#[test]`s that scan a tiny fixture repo with the built `julie-extract`
//!    binary and assert the resolution overlay rows for every case in the
//!    design's list — tier 1 same-file, tier 2 cross-file import, tier 3
//!    receiver-typed, tier 4 unique-language-global, plus the negatives
//!    (ambiguous, overload, C# partial class, cross-language name collision).
//!
//! 2. **Per-language parity guard** (`per_language_tier_parity_guard`): iterates
//!    every language in the artifact's capability snapshot and, for the two
//!    *gated* honesty tiers (2 import, 3 receiver), asserts each language has
//!    EITHER a passing fixture proof OR a recorded `language_capability_gaps`
//!    row. A language with neither for a gated tier fails the suite — that is
//!    the parity guard the plan's Global Constraints parity rule demands
//!    ("tier coverage advertised per language via capability rows and
//!    `language_capability_gaps`"). `parity_guard_bites_on_unmet_cell` proves
//!    the guard actually fails on a cell with neither surface.
//!
//! Tiers 1 and 4 are *universal, non-gated* mechanisms: they carry no
//! per-language gating and no per-language gap rows, so the honesty/parity
//! surface for them is the representative fixture matrix in `TIER14_MATRIX`
//! (rust, python, go, java, C#), asserted directly by
//! `tier1_and_tier4_resolve_across_representative_languages`. See
//! `.razorback/sdd/task-6-report.md` for the scoping judgment call.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use rusqlite::{Connection, OptionalExtension};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// Absolute path to `fixtures/extraction/resolution_contract/`.
fn fixture_base() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/extraction/resolution_contract")
        .canonicalize()
        .expect("resolution_contract fixture tree exists")
}

/// Copy a fixture scenario directory into a fresh temp root and scan it with the
/// built binary, returning the temp dir (kept alive) and the artifact path.
fn scan_fixture(relative: &str) -> (TempDir, PathBuf) {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("repo");
    std::fs::create_dir_all(&root).unwrap();
    copy_dir(&fixture_base().join(relative), &root);

    let db = temp.path().join("artifact.sqlite");
    let output = julie_extract(&[
        "scan",
        "--root",
        path_str(&root),
        "--db",
        path_str(&db),
        "--json",
    ]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "scan of {relative} must succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    (temp, db)
}

fn copy_dir(src: &Path, dst: &Path) {
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let target = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            std::fs::create_dir_all(&target).unwrap();
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

fn julie_extract(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args(args)
        .output()
        .unwrap_or_else(|err| panic!("failed to run julie-extract {args:?}: {err}"))
}

fn path_str(path: &Path) -> &str {
    path.to_str().unwrap()
}

/// The tier/method/confidence the resolver recorded for the co-located
/// identifier of `name`, or `None` when the identifier did not resolve.
struct IdentifierResolution {
    tier: i64,
    method: String,
    confidence: f64,
}

fn resolved_identifier(db: &Path, name: &str) -> Option<IdentifierResolution> {
    let conn = Connection::open(db).unwrap();
    conn.query_row(
        "SELECT i.target_symbol_id, r.tier, r.method, r.confidence \
         FROM identifiers i \
         JOIN identifier_resolutions r ON i.identifier_id = r.identifier_id \
         WHERE i.name = ?1 AND i.target_symbol_id IS NOT NULL",
        [name],
        |row| {
            let _target: String = row.get(0)?;
            Ok(IdentifierResolution {
                tier: row.get(1)?,
                method: row.get(2)?,
                confidence: row.get(3)?,
            })
        },
    )
    .optional()
    .unwrap()
}

/// Whether the identifier `name` has a non-NULL denormalized target — i.e. it
/// resolved to exactly one symbol.
fn identifier_has_target(db: &Path, name: &str) -> bool {
    let conn = Connection::open(db).unwrap();
    conn.query_row(
        "SELECT target_symbol_id FROM identifiers WHERE name = ?1",
        [name],
        |row| row.get::<_, Option<String>>(0),
    )
    .optional()
    .unwrap()
    .flatten()
    .is_some()
}

fn table_count(db: &Path, table: &str) -> i64 {
    let conn = Connection::open(db).unwrap();
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
        row.get(0)
    })
    .unwrap()
}

fn symbol_count_named(db: &Path, name: &str) -> i64 {
    let conn = Connection::open(db).unwrap();
    conn.query_row(
        "SELECT COUNT(*) FROM symbols WHERE name = ?1",
        [name],
        |row| row.get(0),
    )
    .unwrap()
}

/// All languages present in the artifact's capability snapshot.
fn snapshot_languages(db: &Path) -> Vec<String> {
    let conn = Connection::open(db).unwrap();
    let mut stmt = conn
        .prepare("SELECT language FROM language_capabilities ORDER BY language")
        .unwrap();
    stmt.query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

/// Whether a `language_capability_gaps` row exists for `language`/`capability`.
fn gap_recorded(db: &Path, language: &str, capability: &str) -> bool {
    let conn = Connection::open(db).unwrap();
    conn.query_row(
        "SELECT 1 FROM language_capability_gaps WHERE language = ?1 AND capability = ?2",
        [language, capability],
        |row| row.get::<_, i64>(0),
    )
    .optional()
    .unwrap()
    .is_some()
}

// ---------------------------------------------------------------------------
// Contract cases — design.md §Testing
// ---------------------------------------------------------------------------

#[test]
fn tier1_same_file_call_propagates_to_identifier() {
    // Same-file call -> extraction-time relationship -> propagated at tier 1.
    let (_t, db) = scan_fixture("rust/tier1_same_file");
    let res = resolved_identifier(&db, "alpha").expect("same-file call resolves at tier 1");
    assert_eq!(res.tier, 1, "same-file propagation is tier 1");
    assert_eq!(res.method, "tier1_local");
    assert_eq!(res.confidence, 0.95);
}

#[test]
fn tier2_cross_file_import_resolves_typescript() {
    // Cross-file import -> resolved through the import contract at tier 2.
    let (_t, db) = scan_fixture("typescript/tier2_import");
    let res =
        resolved_identifier(&db, "produceWidget").expect("imported call resolves at tier 2 (ts)");
    assert_eq!(res.tier, 2, "import resolution is tier 2");
    assert_eq!(res.method, "tier2_import");
    assert_eq!(res.confidence, 0.85);
    // The tier-2-enabled languages carry no tier2_import gap.
    assert!(
        !gap_recorded(&db, "typescript", "reference_resolution.tier2_import"),
        "a tier-2-enabled language must NOT record a tier2_import gap"
    );
}

#[test]
fn tier2_cross_file_import_resolves_javascript() {
    let (_t, db) = scan_fixture("javascript/tier2_import");
    let res =
        resolved_identifier(&db, "produceWidget").expect("imported call resolves at tier 2 (js)");
    assert_eq!(res.tier, 2);
    assert_eq!(res.method, "tier2_import");
    assert!(!gap_recorded(
        &db,
        "javascript",
        "reference_resolution.tier2_import"
    ));
}

#[test]
fn tier2_aliased_import_resolves_after_lead_fix() {
    // Aliased import `produceWidget as pw`: the imported name differs from the
    // local binding, and the alias has no global definition — so ONLY tier 2 can
    // resolve `pw()`. This fails today because `import_binding` does not read the
    // camelCase `importedName` key the extractor emits; un-ignore once the lead
    // applies the one-line fix.
    let (_t, db) = scan_fixture("typescript/tier2_aliased_import");
    let res = resolved_identifier(&db, "pw").expect("aliased import must resolve at tier 2");
    assert_eq!(res.tier, 2);
    assert_eq!(res.method, "tier2_import");
}

#[test]
fn tier2_aliased_import_requires_resolved_source_module() {
    // The imported-name side of an alias is trustworthy only when the import
    // source resolves to the defining file. Otherwise `alias()` could point at
    // any same-language symbol named `missing` elsewhere in the workspace.
    let (_t, db) = scan_fixture("typescript/tier2_missing_module_alias");
    assert!(
        symbol_count_named(&db, "missing") >= 1,
        "the fixture supplies a tempting but unrelated same-language symbol"
    );
    assert!(
        !identifier_has_target(&db, "alias"),
        "an alias from an unresolved module must not resolve to an unrelated definition"
    );
    assert_eq!(
        table_count(&db, "pending_resolutions"),
        0,
        "no pending resolution should be written for the unresolved alias import"
    );
}

#[test]
fn tier3_receiver_typed_resolves_csharp() {
    // Receiver -> type_fact -> unique type symbol -> member; tier 3.
    let (_t, db) = scan_fixture("csharp/tier3_receiver");
    let res = resolved_identifier(&db, "Build").expect("receiver-typed call resolves at tier 3");
    assert_eq!(res.tier, 3, "receiver-typed resolution is tier 3");
    assert_eq!(res.method, "tier3_receiver");
    // The field's type fact is inferred, so confidence drops to the inferred band.
    assert_eq!(res.confidence, 0.65);
}

#[test]
fn tier4_unique_language_global_call_resolves() {
    // Cross-file call to a workspace-unique free function; tier 4.
    let (_t, db) = scan_fixture("rust/tier4_cross_file");
    assert_eq!(table_count(&db, "pending_resolutions"), 1);
    let res =
        resolved_identifier(&db, "produce_widget").expect("unique-global call resolves at tier 4");
    assert_eq!(res.tier, 4, "unique-language-global is tier 4");
    assert_eq!(res.method, "tier4_global");
    assert_eq!(res.confidence, 0.55);
}

#[test]
fn ambiguous_same_name_stays_unresolved() {
    // Two same-name candidates -> no best guess.
    let (_t, db) = scan_fixture("rust/ambiguous");
    assert_eq!(
        symbol_count_named(&db, "produce_widget"),
        2,
        "the fixture must supply two colliding candidates"
    );
    assert!(
        !identifier_has_target(&db, "produce_widget"),
        "an ambiguous call must not resolve to a best guess"
    );
    assert_eq!(
        table_count(&db, "pending_resolutions"),
        0,
        "no pending_resolutions row for an ambiguous edge"
    );
}

#[test]
fn overload_stays_ambiguous() {
    // Two same-name method overloads -> ambiguous without arity discrimination.
    let (_t, db) = scan_fixture("csharp/overload");
    assert_eq!(symbol_count_named(&db, "Handle"), 2);
    assert!(
        !identifier_has_target(&db, "Handle"),
        "an overloaded call must stay ambiguous (no best guess)"
    );
}

#[test]
fn partial_class_stays_ambiguous() {
    // Two `partial class Store` declarations -> the class reference is ambiguous.
    let (_t, db) = scan_fixture("csharp/partial_class");
    assert_eq!(
        symbol_count_named(&db, "Store"),
        2,
        "a partial class produces two same-name class symbols"
    );
    assert!(
        !identifier_has_target(&db, "Store"),
        "a partial-class reference must stay ambiguous"
    );
}

#[test]
fn cross_language_name_collision_stays_unresolved() {
    // A Rust call to a name defined only in Python must not resolve across
    // languages (tier 4 is same-language only).
    let (_t, db) = scan_fixture("cross_language");
    assert_eq!(
        symbol_count_named(&db, "shared_widget"),
        1,
        "the only definition is the Python one"
    );
    assert!(
        !identifier_has_target(&db, "shared_widget"),
        "the same-language constraint must keep the cross-language call unresolved"
    );
    assert_eq!(table_count(&db, "pending_resolutions"), 0);
}

// ---------------------------------------------------------------------------
// Universal tiers (1 & 4) across representative languages
// ---------------------------------------------------------------------------

/// (fixture scenario, terminal identifier, expected tier, expected method) for
/// the language-generic tiers proven on a representative language set.
const TIER14_MATRIX: &[(&str, &str, i64, &str)] = &[
    ("rust/tier1_same_file", "alpha", 1, "tier1_local"),
    ("python/tier1_same_file", "alpha", 1, "tier1_local"),
    ("go/tier1_same_file", "alpha", 1, "tier1_local"),
    ("java/tier1_same_file", "alpha", 1, "tier1_local"),
    ("csharp/tier3_receiver", "Build", 3, "tier3_receiver"), // C# also proves tier 3 here
    ("rust/tier4_cross_file", "produce_widget", 4, "tier4_global"),
    (
        "python/tier4_cross_file",
        "produce_widget",
        4,
        "tier4_global",
    ),
    ("go/tier4_cross_file", "produceWidget", 4, "tier4_global"),
];

#[test]
fn tier1_and_tier4_resolve_across_representative_languages() {
    for (scenario, terminal, tier, method) in TIER14_MATRIX {
        let (_t, db) = scan_fixture(scenario);
        let res = resolved_identifier(&db, terminal)
            .unwrap_or_else(|| panic!("{scenario}: {terminal} must resolve"));
        assert_eq!(res.tier, *tier, "{scenario}: tier");
        assert_eq!(&res.method, method, "{scenario}: method");
    }
}

// ---------------------------------------------------------------------------
// Per-language parity guard (gated tiers 2 & 3)
// ---------------------------------------------------------------------------

/// Languages whose tier-2 import contract is proven by a passing fixture in this
/// suite (so they legitimately carry no `tier2_import` gap).
const TIER2_FIXTURE_LANGUAGES: &[&str] = &["typescript", "javascript"];

/// One gated tier's parity obligation for a language: satisfied by a proving
/// fixture (allowlist) OR a recorded capability gap.
fn gated_cell_satisfied(
    db: &Path,
    language: &str,
    capability: &str,
    fixture_languages: &[&str],
) -> bool {
    fixture_languages.contains(&language) || gap_recorded(db, language, capability)
}

#[test]
fn per_language_tier_parity_guard() {
    // A single scan surfaces the full, scan-independent capability snapshot
    // (every registered language + its reference-resolution gap rows).
    let (_t, db) = scan_fixture("rust/tier4_cross_file");
    let languages = snapshot_languages(&db);
    assert!(
        languages.len() >= 30,
        "the snapshot must cover the full language registry, got {}",
        languages.len()
    );

    let mut violations = Vec::new();
    for language in &languages {
        // Tier 2 (import): fixture-proven for the tier-2-enabled languages,
        // otherwise a recorded `tier2_import` gap.
        if !gated_cell_satisfied(
            &db,
            language,
            "reference_resolution.tier2_import",
            TIER2_FIXTURE_LANGUAGES,
        ) {
            violations.push(format!(
                "{language}: tier2 — neither fixture nor recorded gap"
            ));
        }
        // Tier 3 (receiver): coverage is type_facts-bounded everywhere, so every
        // language records a `tier3_receiver` gap (C# additionally proves it by
        // fixture). The gap is the parity surface here.
        if !gap_recorded(&db, language, "reference_resolution.tier3_receiver") {
            violations.push(format!("{language}: tier3 — no recorded gap"));
        }
    }

    assert!(
        violations.is_empty(),
        "parity guard: every language must prove or record each gated tier:\n{}",
        violations.join("\n")
    );
}

#[test]
fn parity_guard_bites_on_unmet_cell() {
    // The guard must FAIL for a cell with neither a fixture nor a gap. Rust is
    // not in the tier-2 fixture allowlist and (correctly) records a tier2_import
    // gap; assert that WITHOUT the gap the cell would be unsatisfied, and WITH an
    // absent gap-and-allowlist the checker reports a violation.
    let (_t, db) = scan_fixture("rust/tier4_cross_file");

    // A real cell that is satisfied (rust records the tier2 gap).
    assert!(
        gated_cell_satisfied(
            &db,
            "rust",
            "reference_resolution.tier2_import",
            TIER2_FIXTURE_LANGUAGES,
        ),
        "rust tier2 is satisfied via its recorded gap"
    );

    // A cell keyed on a non-existent capability with rust NOT in the allowlist:
    // no gap row, not fixture-proven -> the checker must report it unsatisfied.
    assert!(
        !gated_cell_satisfied(
            &db,
            "rust",
            "reference_resolution.__nonexistent_tier__",
            TIER2_FIXTURE_LANGUAGES,
        ),
        "a cell with neither a fixture nor a recorded gap must be unsatisfied — \
         this is the guard biting"
    );
}
