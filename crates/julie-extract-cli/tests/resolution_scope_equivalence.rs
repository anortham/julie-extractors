//! The delta path's acceptance gate: an artifact converged by incremental steps
//! must carry the same resolution overlay a full pass would derive from the same
//! rows.
//!
//! ## Why re-derive in-process instead of comparing against a fresh scan
//!
//! A from-scratch scan of the final tree is a different artifact, not just a
//! differently-resolved one: a relationship row in an unchanged file whose target
//! symbol died is FK-cascaded away and never re-extracted, so the two artifacts
//! legitimately differ in rows that have nothing to do with resolution scope.
//! Clearing the overlay on a COPY and re-resolving at full scope holds the
//! extracted rows fixed and isolates the property under test.
//!
//! A failure here is a defect in the delta scope, never a test to relax: it means
//! the incremental artifact's answers depend on the order files were scanned.

use std::path::Path;
use std::process::{Command, Output};

use julie_extract_artifact::writer::ResolutionScopeInput;
use julie_extract_cli::resolution::resolve_workspace;
use rusqlite::Connection;
use tempfile::TempDir;

fn julie_extract(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args(args)
        .output()
        .unwrap_or_else(|err| panic!("failed to run julie-extract {args:?}: {err}"))
}

fn path_str(path: &Path) -> &str {
    path.to_str().expect("fixture paths are UTF-8")
}

fn assert_success(output: Output) {
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn scan(root: &Path, db: &Path) {
    assert_success(julie_extract(&[
        "scan",
        "--root",
        path_str(root),
        "--db",
        path_str(db),
        "--json",
    ]));
}

fn update(root: &Path, db: &Path, file: &str) {
    assert_success(julie_extract(&[
        "update",
        "--root",
        path_str(root),
        "--db",
        path_str(db),
        "--file",
        file,
        "--json",
    ]));
}

/// Removal is its own verb: `update` on a vanished path reports `file_not_found`
/// and writes nothing, so a deletion case driven through `update` tests nothing.
fn delete(root: &Path, db: &Path, file: &str) {
    assert_success(julie_extract(&[
        "delete",
        "--root",
        path_str(root),
        "--db",
        path_str(db),
        "--file",
        file,
        "--json",
    ]));
}

/// The overlay as ordered comparable rows. `resolved_at_revision` is excluded: it
/// records WHEN a row was written, which incremental and single-pass convergence
/// necessarily disagree on.
fn overlay(db: &Path) -> Vec<String> {
    let conn = Connection::open(db).expect("artifact opens");
    let mut rows = Vec::new();

    let mut pending = conn
        .prepare(
            "SELECT pending_relationship_id, target_symbol_id, tier, confidence, method \
             FROM pending_resolutions ORDER BY pending_relationship_id",
        )
        .unwrap();
    rows.extend(
        pending
            .query_map([], |row| {
                Ok(format!(
                    "pending|{}|{}|{}|{}|{}",
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, f64>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap(),
    );

    let mut identifiers = conn
        .prepare(
            "SELECT identifier_id, target_symbol_id, tier, confidence, method, outcome, candidates \
             FROM identifier_resolutions ORDER BY identifier_id",
        )
        .unwrap();
    rows.extend(
        identifiers
            .query_map([], |row| {
                Ok(format!(
                    "identifier|{}|{:?}|{:?}|{:?}|{:?}|{}|{:?}",
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<f64>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap(),
    );

    // The denormalized column consumers actually read.
    let mut targets = conn
        .prepare(
            "SELECT identifier_id, target_symbol_id FROM identifiers \
             WHERE target_symbol_id IS NOT NULL ORDER BY identifier_id",
        )
        .unwrap();
    rows.extend(
        targets
            .query_map([], |row| {
                Ok(format!(
                    "denormalized|{}|{}",
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap(),
    );

    rows
}

/// Copy `db`, wipe its overlay, re-resolve the copy at full scope, and assert the
/// original agrees with it.
fn assert_matches_full_rederivation(db: &Path, what: &str) {
    let oracle = db.with_extension("oracle.sqlite");
    std::fs::copy(db, &oracle).expect("artifact copies");

    let mut conn = Connection::open(&oracle).expect("oracle opens");
    conn.execute_batch(
        "DELETE FROM pending_resolutions; \
         DELETE FROM identifier_resolutions; \
         UPDATE identifiers SET target_symbol_id = NULL;",
    )
    .expect("overlay clears");

    let scope = ResolutionScopeInput {
        is_full_scan: true,
        ..Default::default()
    };
    let tx = conn.transaction().expect("oracle transaction opens");
    resolve_workspace(&tx, &scope).expect("full re-derivation runs");
    tx.commit().expect("oracle commits");
    drop(conn);

    let incremental = overlay(db);
    let rederived = overlay(&oracle);
    assert_eq!(
        incremental, rederived,
        "{what}: the incrementally converged overlay must equal a full re-derivation \
         over the same rows"
    );
}

struct Fixture {
    _temp: TempDir,
    root: std::path::PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("repo");
        std::fs::create_dir_all(root.join("src")).unwrap();
        Self { _temp: temp, root }
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    fn db(&self) -> std::path::PathBuf {
        self.root.join("artifact.sqlite")
    }
}

#[test]
fn aliased_import_filled_by_a_delta_matches_a_full_rederivation() {
    let fixture = Fixture::new();
    fixture.write("src/b.ts", "export function placeholder(): void {}\n");
    fixture.write(
        "src/a.ts",
        "import { realName as localName } from './b';\nexport function caller(): void { localName(); }\n",
    );
    let db = fixture.db();
    scan(&fixture.root, &db);

    fixture.write(
        "src/b.ts",
        "export function placeholder(): void {}\nexport function realName(): void {}\n",
    );
    update(&fixture.root, &db, "src/b.ts");

    assert_matches_full_rederivation(&db, "aliased import filled by a delta");
}

#[test]
fn receiver_type_ambiguity_demoted_by_a_delta_matches_a_full_rederivation() {
    let fixture = Fixture::new();
    fixture.write(
        "src/widget.cs",
        "namespace App { public class Widget { public int Render() { return 1; } } }\n",
    );
    fixture.write(
        "src/consumer.cs",
        "namespace App { public class Consumer { public int Run() { Widget w = new Widget(); return w.Render(); } } }\n",
    );
    let db = fixture.db();
    scan(&fixture.root, &db);

    fixture.write(
        "src/other.cs",
        "namespace Other { public class Widget { } }\n",
    );
    update(&fixture.root, &db, "src/other.cs");

    assert_matches_full_rederivation(&db, "receiver type made ambiguous by a delta");
}

#[test]
fn module_shadowing_applied_by_a_delta_matches_a_full_rederivation() {
    let fixture = Fixture::new();
    fixture.write("src/util/index.ts", "export function helper(): void {}\n");
    fixture.write(
        "src/consumer.ts",
        "import { helper as h } from './util';\nexport function run(): void { h(); }\n",
    );
    let db = fixture.db();
    scan(&fixture.root, &db);

    fixture.write("src/util.ts", "export function helper(): void {}\n");
    update(&fixture.root, &db, "src/util.ts");

    assert_matches_full_rederivation(&db, "module shadowed by a new file");
}

#[test]
fn restored_receiver_type_uniqueness_matches_a_full_rederivation() {
    // The inverse of the demotion case: the member starts AMBIGUOUS (two `Widget`
    // types), so the overlay holds a NULL-target row. Removing the rival makes it
    // resolvable again, which only a pass that retries already-attempted rows can
    // notice — a never-attempted-only worklist skips it forever.
    let fixture = Fixture::new();
    fixture.write(
        "src/widget.cs",
        "namespace App { public class Widget { public int Render() { return 1; } } }\n",
    );
    fixture.write(
        "src/rival.cs",
        "namespace Other { public class Widget { } }\n",
    );
    fixture.write(
        "src/consumer.cs",
        "namespace App { public class Consumer { public int Run() { Widget w = new Widget(); return w.Render(); } } }\n",
    );
    let db = fixture.db();
    scan(&fixture.root, &db);

    fixture.write(
        "src/rival.cs",
        "namespace Other { public class Unrelated { } }\n",
    );
    update(&fixture.root, &db, "src/rival.cs");

    assert_matches_full_rederivation(&db, "receiver type uniqueness restored by a delta");
}

#[test]
fn a_multi_step_edit_sequence_matches_a_full_rederivation() {
    // Order-dependence is the failure this whole gate exists to catch, so drive
    // several shapes through one artifact: an add, a rewrite that removes a symbol,
    // and a re-add that restores uniqueness.
    let fixture = Fixture::new();
    fixture.write("src/core.ts", "export function shared(): void {}\n");
    fixture.write(
        "src/one.ts",
        "import { shared } from './core';\nexport function useOne(): void { shared(); }\n",
    );
    let db = fixture.db();
    scan(&fixture.root, &db);

    fixture.write(
        "src/two.ts",
        "import { shared as alias } from './core';\nexport function useTwo(): void { alias(); }\n",
    );
    update(&fixture.root, &db, "src/two.ts");

    fixture.write("src/rival.ts", "export function shared(): void {}\n");
    update(&fixture.root, &db, "src/rival.ts");

    fixture.write("src/rival.ts", "export function unrelated(): void {}\n");
    update(&fixture.root, &db, "src/rival.ts");

    assert_matches_full_rederivation(&db, "multi-step edit sequence");
}

#[test]
fn a_shadow_file_with_disjoint_exports_matches_a_full_rederivation() {
    // The sibling shadowing case above defines `helper` in BOTH modules, so the
    // touched-name set carries the import's own binding and the name unions reach
    // the consumer on their own. Here the shadow file exports something else
    // entirely: no touched name matches the import, and the ONLY thing tying the
    // consumer to this write is that `src/util.ts` is a module-path candidate for
    // the specifier `./util`. Resolution moves from `helper` to nothing.
    let fixture = Fixture::new();
    fixture.write("src/util/index.ts", "export function helper(): void {}\n");
    fixture.write(
        "src/consumer.ts",
        "import { helper as h } from './util';\nexport function run(): void { h(); }\n",
    );
    let db = fixture.db();
    scan(&fixture.root, &db);

    fixture.write("src/util.ts", "export function unrelated(): void {}\n");
    update(&fixture.root, &db, "src/util.ts");

    assert_matches_full_rederivation(&db, "shadow file with disjoint exports");
}

#[test]
fn deleting_a_disjoint_shadow_file_matches_a_full_rederivation() {
    // The inverse direction, and the one a name-keyed scope cannot see at all: the
    // deleted file never shared a name with the import, so removing it restores the
    // directory module through a path change alone.
    let fixture = Fixture::new();
    fixture.write("src/util/index.ts", "export function helper(): void {}\n");
    fixture.write("src/util.ts", "export function unrelated(): void {}\n");
    fixture.write(
        "src/consumer.ts",
        "import { helper as h } from './util';\nexport function run(): void { h(); }\n",
    );
    let db = fixture.db();
    scan(&fixture.root, &db);

    std::fs::remove_file(fixture.root.join("src/util.ts")).unwrap();
    delete(&fixture.root, &db, "src/util.ts");

    assert_matches_full_rederivation(&db, "deleting a disjoint shadow file");
}

#[test]
fn deleting_a_same_name_shadow_file_matches_a_full_rederivation() {
    // Un-shadowing where both modules export the binding. This one the pre-delete
    // name collection already reaches; pinned so the module-path union is never
    // "simplified" into covering only the disjoint case.
    let fixture = Fixture::new();
    fixture.write("src/util/index.ts", "export function helper(): void {}\n");
    fixture.write("src/util.ts", "export function helper(): void {}\n");
    fixture.write(
        "src/consumer.ts",
        "import { helper as h } from './util';\nexport function run(): void { h(); }\n",
    );
    let db = fixture.db();
    scan(&fixture.root, &db);

    std::fs::remove_file(fixture.root.join("src/util.ts")).unwrap();
    delete(&fixture.root, &db, "src/util.ts");

    assert_matches_full_rederivation(&db, "deleting a same-name shadow file");
}

#[test]
fn shadowing_then_unshadowing_converges_to_a_full_rederivation() {
    // Round trip through the same artifact: the overlay after add-then-remove must
    // equal a single pass over the final rows, not merely differ from the stale one.
    let fixture = Fixture::new();
    fixture.write("src/util/index.ts", "export function helper(): void {}\n");
    fixture.write(
        "src/consumer.ts",
        "import { helper as h } from './util';\nexport function run(): void { h(); }\n",
    );
    let db = fixture.db();
    scan(&fixture.root, &db);

    fixture.write("src/util.ts", "export function unrelated(): void {}\n");
    update(&fixture.root, &db, "src/util.ts");

    std::fs::remove_file(fixture.root.join("src/util.ts")).unwrap();
    delete(&fixture.root, &db, "src/util.ts");

    assert_matches_full_rederivation(&db, "shadow added then removed");
}

#[test]
fn rename_rederives_old_name_rows_across_files() {
    let fixture = Fixture::new();
    fixture.write("src/a.cs", "namespace App { public class Foo { } }\n");
    fixture.write("src/c.cs", "namespace Other { public class Foo { } }\n");
    fixture.write(
        "src/b.cs",
        "namespace App { public class UseFoo { public Foo Make() { return new Foo(); } } }\n",
    );
    let db = fixture.db();
    scan(&fixture.root, &db);

    fixture.write("src/a.cs", "namespace App { public class Bar { } }\n");
    update(&fixture.root, &db, "src/a.cs");

    assert_matches_full_rederivation(
        &db,
        "a rename must re-derive the OLD name's rows in files the delta never touched",
    );
}

#[test]
fn rename_captures_new_name_rows_across_files() {
    let fixture = Fixture::new();
    fixture.write("src/a.cs", "namespace App { public class Foo { } }\n");
    fixture.write(
        "src/c.cs",
        "namespace Other { public class Unrelated { } }\n",
    );
    fixture.write(
        "src/b.cs",
        "namespace App { public class UseBar { public Bar Make() { return new Bar(); } } }\n",
    );
    let db = fixture.db();
    scan(&fixture.root, &db);

    fixture.write("src/a.cs", "namespace App { public class Bar { } }\n");
    update(&fixture.root, &db, "src/a.cs");

    assert_matches_full_rederivation(
        &db,
        "a rename must capture the NEW name's rows in files the delta never touched",
    );
}

#[test]
fn aliased_import_recheck_reaches_local_name_rows() {
    let fixture = Fixture::new();
    fixture.write("src/b.ts", "export function realName(): void {}\n");
    fixture.write(
        "src/a.ts",
        "import { realName as localName } from './b';\nexport function caller(): void { localName(); }\n",
    );
    fixture.write(
        "src/c.ts",
        "import { realName as alsoLocal } from './b';\nexport function otherCaller(): void { alsoLocal(); }\n",
    );
    let db = fixture.db();
    scan(&fixture.root, &db);

    fixture.write(
        "src/b.ts",
        "export function untouchedNeighbor(): void {}\n\nexport function realName(): void {}\n",
    );
    update(&fixture.root, &db, "src/b.ts");

    assert_matches_full_rederivation(
        &db,
        "moving an aliased export must re-point rows that carry only the LOCAL name",
    );
}

#[test]
fn receiver_type_touch_rechecks_member_rows_in_unchanged_files() {
    let fixture = Fixture::new();
    fixture.write(
        "src/widget.cs",
        "namespace App { public class Widget { public int Render() { return 1; } } }\n",
    );
    fixture.write(
        "src/rival.cs",
        "namespace Other { public class Placeholder { } }\n",
    );
    fixture.write(
        "src/consumer.cs",
        "namespace App { public class Consumer { public int Run() { Widget w = new Widget(); return w.Render(); } } }\n",
    );
    let db = fixture.db();
    scan(&fixture.root, &db);

    fixture.write(
        "src/rival.cs",
        "namespace Other { public class Widget { } }\n",
    );
    update(&fixture.root, &db, "src/rival.cs");

    assert_matches_full_rederivation(
        &db,
        "touching a receiver's TYPE must recheck member rows whose own name the delta never touched",
    );
}

#[test]
fn module_shadowing_repoint_survives_row_scoping() {
    let fixture = Fixture::new();
    fixture.write(
        "src/util/index.ts",
        "export function helper(): void {}\nexport function other(): void {}\n",
    );
    fixture.write(
        "src/consumer_a.ts",
        "import { helper as h } from './util';\nexport function runA(): void { h(); }\n",
    );
    fixture.write(
        "src/consumer_b.ts",
        "import { other as o } from './util';\nexport function runB(): void { o(); }\n",
    );
    let db = fixture.db();
    scan(&fixture.root, &db);

    fixture.write("src/util.ts", "export function helper(): void {}\n");
    update(&fixture.root, &db, "src/util.ts");

    assert_matches_full_rederivation(
        &db,
        "a shadowing module file must re-point every importer of the specifier, including the one \
         whose binding shares no touched name",
    );
}
