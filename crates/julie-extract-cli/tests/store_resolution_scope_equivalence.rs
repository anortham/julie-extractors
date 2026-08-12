#![cfg(feature = "test-store-resolution-contract")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use rusqlite::Connection;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

const FAMILY: &str = "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11";

struct StorePair {
    temp: TempDir,
    full_root: PathBuf,
    scoped_root: PathBuf,
    full_store: PathBuf,
    scoped_store: PathBuf,
}

impl StorePair {
    fn new(files: &[(&str, &str)]) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let full_root = temp.path().join("full-source");
        let scoped_root = temp.path().join("scoped-source");
        let full_store = temp.path().join("full-store");
        let scoped_store = temp.path().join("scoped-store");
        for root in [&full_root, &scoped_root] {
            for (path, content) in files {
                write_source(root, path, content);
            }
        }
        for (root, store) in [(&full_root, &full_store), (&scoped_root, &scoped_store)] {
            assert_success(&import(root, store));
            assert_success(&resolve(store, "off", "seed"));
        }
        Self {
            temp,
            full_root,
            scoped_root,
            full_store,
            scoped_store,
        }
    }

    fn roots(&self) -> [&Path; 2] {
        [&self.full_root, &self.scoped_root]
    }

    fn rescan(&self) {
        assert_success(&import(&self.full_root, &self.full_store));
        assert_success(&import(&self.scoped_root, &self.scoped_store));
    }

    fn resolve_and_compare(
        &self,
        expected_mode: &str,
        expected_fallback: Option<&str>,
    ) -> (PathBuf, Value) {
        let full = resolve(&self.full_store, "off", "oracle");
        let scoped = resolve(&self.scoped_store, "on", "candidate");
        assert_success(&full);
        assert_success(&scoped);
        let report: Value = serde_json::from_slice(&scoped.stdout).unwrap();
        assert_eq!(
            report["resolution"]["resolution_mode"], expected_mode,
            "{report}"
        );
        assert_eq!(
            report["resolution"]["fallback_reason"],
            expected_fallback.map_or(Value::Null, |reason| Value::String(reason.to_string()))
        );
        let full_artifact = self.temp.path().join("full.sqlite");
        let scoped_artifact = self.temp.path().join("scoped.sqlite");
        assert_success(&export(&self.full_store, &full_artifact));
        assert_success(&export(&self.scoped_store, &scoped_artifact));
        assert_eq!(
            semantic_digest(&full_artifact),
            semantic_digest(&scoped_artifact)
        );
        (scoped_artifact, report)
    }
}

fn run(args: &[&str], delta: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_julie-extract"));
    command.args(args);
    match delta {
        Some(value) => {
            command.env("JULIE_STORE_RESOLUTION_DELTA", value);
        }
        None => {
            command.env_remove("JULIE_STORE_RESOLUTION_DELTA");
        }
    }
    command.output().unwrap()
}

fn import(root: &Path, store: &Path) -> Output {
    run(
        &[
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            FAMILY,
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--level",
            "full",
            "--json",
        ],
        None,
    )
}

fn resolve(store: &Path, delta: &str, suffix: &str) -> Output {
    run(
        &[
            "store",
            "resolve",
            "--store",
            store.to_str().unwrap(),
            "--view",
            "view-main",
            "--request-id",
            &format!("resolve-{suffix}"),
            "--idempotency-key",
            &format!("resolve-{suffix}-key"),
            "--json",
        ],
        Some(delta),
    )
}

fn export(store: &Path, output: &Path) -> Output {
    run(
        &[
            "store",
            "export",
            "--store",
            store.to_str().unwrap(),
            "--view",
            "view-main",
            "--out",
            output.to_str().unwrap(),
            "--json",
        ],
        None,
    )
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_source(root: &Path, path: &str, content: &str) {
    let output = root.join(path);
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::write(output, content).unwrap();
}

fn semantic_digest(path: &Path) -> String {
    let connection = Connection::open(path).unwrap();
    let mut digest = Sha256::new();
    for query in [
        "SELECT f.path,f.language,f.content_hash
         FROM files AS f ORDER BY f.path COLLATE BINARY",
        "SELECT source.path,i.name,i.kind,i.start_byte,i.end_byte,r.outcome,
                target_file.path,target.name,r.tier,r.confidence,r.method,r.candidates
         FROM identifier_resolutions AS r
         JOIN identifiers AS i ON i.identifier_id=r.identifier_id
         JOIN files AS source ON source.file_id=i.file_id
         LEFT JOIN symbols AS target ON target.symbol_id=r.target_symbol_id
         LEFT JOIN files AS target_file ON target_file.file_id=target.file_id
         ORDER BY source.path COLLATE BINARY,i.start_byte,i.identifier_id COLLATE BINARY",
        "SELECT source.path,p.kind,p.target_terminal_name,p.start_byte,p.end_byte,
                target_file.path,target.name,r.tier,r.confidence,r.method
         FROM pending_resolutions AS r
         JOIN pending_relationships AS p
           ON p.pending_relationship_id=r.pending_relationship_id
         JOIN files AS source ON source.file_id=p.file_id
         JOIN symbols AS target ON target.symbol_id=r.target_symbol_id
         JOIN files AS target_file ON target_file.file_id=target.file_id
         ORDER BY source.path COLLATE BINARY,p.start_byte,p.pending_relationship_id COLLATE BINARY",
    ] {
        let mut statement = connection.prepare(query).unwrap();
        let column_count = statement.column_count();
        let mut rows = statement.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            for index in 0..column_count {
                let value = row.get_ref(index).unwrap();
                digest.update(format!("{value:?}\0").as_bytes());
            }
            digest.update(b"\n");
        }
        digest.update(b"\x1e");
    }
    format!("{:x}", digest.finalize())
}

#[test]
fn curated_add_delete_rename_module_repoint_and_sibling_rows_match_full() {
    let pair = StorePair::new(&[
        (
            "src/use.ts",
            "import { target } from './target';\nexport function run() { return target(); }\nexport function sibling() { return target(); }\n",
        ),
        ("src/target.ts", "export function target() { return 1; }\n"),
        (
            "src/stable.ts",
            "function helper() { return 7; } export function stable() { return helper(); }\n",
        ),
        (
            "src/stable2.ts",
            "function helper2() { return 7; } export function stable2() { return helper2(); }\n",
        ),
        (
            "src/stable3.ts",
            "function helper3() { return 7; } export function stable3() { return helper3(); }\n",
        ),
        (
            "src/stable4.ts",
            "function helper4() { return 7; } export function stable4() { return helper4(); }\n",
        ),
        (
            "src/stable5.ts",
            "function helper5() { return 7; } export function stable5() { return helper5(); }\n",
        ),
        (
            "src/stable6.ts",
            "function helper6() { return 7; } export function stable6() { return helper6(); }\n",
        ),
    ]);
    for root in pair.roots() {
        fs::remove_file(root.join("src/target.ts")).unwrap();
        write_source(
            root,
            "src/moved.ts",
            "export function target() { return 2; }\nexport function added() { return target(); }\n",
        );
        write_source(
            root,
            "src/use.ts",
            "import { target } from './moved';\nexport function run() { return target(); }\nexport function sibling() { return target(); }\n",
        );
    }
    pair.rescan();
    let _ = pair.resolve_and_compare("scoped", None);
}

#[test]
fn curated_receiver_and_tier4_uniqueness_flips_match_full() {
    let pair = StorePair::new(&[
        (
            "src/model.ts",
            "export class Model { ping() { return 1; } }\nexport function unique() { return 1; }\n",
        ),
        (
            "src/use.ts",
            "import { Model } from './model';\nexport function call(m: Model) { m.ping(); return unique(); }\n",
        ),
        ("src/stable.ts", "export function stable() { return 9; }\n"),
    ]);
    for root in pair.roots() {
        write_source(
            root,
            "src/competitor.ts",
            "export function unique() { return 2; }\nexport class Other { ping() { return 2; } }\n",
        );
    }
    pair.rescan();
    let _ = pair.resolve_and_compare("full", Some("resolution_scope_crossover"));
}

#[test]
fn unchanged_predecessor_version_carries_an_unselected_sibling_row() {
    let pair = StorePair::new(&[
        (
            "src/use.ts",
            "export function run() { return target() + stable(); }\n",
        ),
        ("src/target.ts", "export function target() { return 1; }\n"),
        ("src/stable.ts", "export function stable() { return 7; }\n"),
        (
            "src/padding1.ts",
            "function p1() { return 1; } export function q1() { return p1(); }\n",
        ),
        (
            "src/padding2.ts",
            "function p2() { return 2; } export function q2() { return p2(); }\n",
        ),
        (
            "src/padding3.ts",
            "function p3() { return 3; } export function q3() { return p3(); }\n",
        ),
    ]);
    for root in pair.roots() {
        write_source(
            root,
            "src/target.ts",
            "export function target() { return 2; }\n",
        );
    }
    pair.rescan();
    let (scoped_artifact, report) = pair.resolve_and_compare("scoped", None);
    assert!(report["resolution"]["scope_name_count"].as_u64().unwrap() >= 1);
    let connection = Connection::open(scoped_artifact).unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*)
                 FROM identifier_resolutions AS resolution
                 JOIN identifiers AS identifier
                   ON identifier.identifier_id=resolution.identifier_id
                 JOIN files AS source ON source.file_id=identifier.file_id
                 WHERE source.path='src/use.ts' AND identifier.name='stable'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*)
                 FROM identifier_resolutions AS resolution
                 JOIN identifiers AS identifier
                   ON identifier.identifier_id=resolution.identifier_id
                 JOIN files AS source ON source.file_id=identifier.file_id
                 WHERE source.path='src/use.ts' AND identifier.name='target'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
}

#[test]
fn crossover_fallback_matches_full() {
    let pair = StorePair::new(&[
        ("src/a.ts", "export function a() { return 1; }\n"),
        ("src/b.ts", "export function b() { return a(); }\n"),
        ("src/c.ts", "export function c() { return b(); }\n"),
    ]);
    for root in pair.roots() {
        write_source(
            root,
            "src/a.ts",
            "export function renamedA() { return 2; }\n",
        );
        write_source(
            root,
            "src/b.ts",
            "export function renamedB() { return renamedA(); }\n",
        );
        write_source(
            root,
            "src/c.ts",
            "export function c() { return renamedB(); }\n",
        );
    }
    pair.rescan();
    let _ = pair.resolve_and_compare("full", Some("resolution_scope_crossover"));
}

#[test]
fn one_file_broad_name_crossover_matches_full() {
    let mut files = vec![(
        "src/target.ts".to_string(),
        "export function shared() { return 1; }\n".to_string(),
    )];
    for index in 0..9 {
        files.push((
            format!("src/collision-{index}.ts"),
            format!(
                "export function collision{index}() {{ return shared() + shared() + shared(); }}\n"
            ),
        ));
    }
    let file_refs = files
        .iter()
        .map(|(path, content)| (path.as_str(), content.as_str()))
        .collect::<Vec<_>>();
    let pair = StorePair::new(&file_refs);
    for root in pair.roots() {
        write_source(
            root,
            "src/target.ts",
            "export function shared() { return 2; }\n",
        );
    }
    pair.rescan();
    let (_, report) = pair.resolve_and_compare("full", Some("resolution_scope_crossover"));
    assert_eq!(report["resolution"]["scope_file_count"], 10);
}
