#![cfg(feature = "test-store-resolution-contract")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

const FAMILY: &str = "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11";

struct SequencePair {
    temp: TempDir,
    full_root: PathBuf,
    scoped_root: PathBuf,
    full_store: PathBuf,
    scoped_store: PathBuf,
}

impl SequencePair {
    fn new(seed: u64) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let full_root = temp.path().join("full-source");
        let scoped_root = temp.path().join("scoped-source");
        let full_store = temp.path().join("full-store");
        let scoped_store = temp.path().join("scoped-store");
        for root in [&full_root, &scoped_root] {
            write_source(
                root,
                "src/core.ts",
                &format!(
                    "export function core() {{ return {seed}; }}\nexport function local() {{ return core(); }}\n"
                ),
            );
            write_source(
                root,
                "src/use.ts",
                "import { core } from './core';\nexport function use() { return core(); }\n",
            );
            write_source(
                root,
                "src/model.ts",
                "export class Model { ping() { return 1; } }\nexport function receiver(m: Model) { return m.ping(); }\n",
            );
            for index in 0..5 {
                write_source(
                    root,
                    &format!("src/stable{index}.ts"),
                    &format!(
                        "function helper{index}() {{ return {index}; }} export function stable{index}() {{ return helper{index}(); }}\n"
                    ),
                );
            }
        }
        for (root, store) in [(&full_root, &full_store), (&scoped_root, &scoped_store)] {
            assert_success(&import(root, store));
            assert_success(&resolve(store, "off", &format!("seed-{seed}")));
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

    fn advance(&self, seed: u64, generation: u64) {
        for root in self.roots() {
            match generation {
                1 => write_source(
                    root,
                    "src/extra.ts",
                    &format!(
                        "export function extra() {{ return {seed}; }} export function extraUse() {{ return extra(); }}\n"
                    ),
                ),
                2 => {
                    write_source(
                        root,
                        "src/core.ts",
                        &format!(
                            "export function core() {{ return {}; }}\nexport function local() {{ return core(); }}\n",
                            seed + generation
                        ),
                    );
                    write_source(
                        root,
                        "src/use.ts",
                        "import { core } from './core'; import { extra } from './extra';\nexport function use() { return core() + extra(); }\n",
                    );
                }
                3 => {
                    fs::remove_file(root.join("src/extra.ts")).unwrap();
                    write_source(
                        root,
                        "src/moved.ts",
                        &format!(
                            "export function extra() {{ return {}; }} export function extraUse() {{ return extra(); }}\n",
                            seed + generation
                        ),
                    );
                    write_source(
                        root,
                        "src/use.ts",
                        "import { core } from './core'; import { extra } from './moved';\nexport function use() { return core() + extra(); }\n",
                    );
                }
                4 => {
                    fs::remove_file(root.join("src/moved.ts")).unwrap();
                    write_source(
                        root,
                        "src/use.ts",
                        "import { core } from './core';\nexport function use() { return core(); }\n",
                    );
                    write_source(
                        root,
                        "src/model.ts",
                        "export class Model { pong() { return 2; } }\nexport function receiver(m: Model) { return m.pong(); }\n",
                    );
                }
                _ => unreachable!(),
            }
        }
        assert_success(&import(&self.full_root, &self.full_store));
        assert_success(&import(&self.scoped_root, &self.scoped_store));
    }

    fn compare(&self, seed: u64, generation: u64) {
        assert_success(&resolve(
            &self.full_store,
            "off",
            &format!("full-{seed}-{generation}"),
        ));
        let scoped = resolve(
            &self.scoped_store,
            "on",
            &format!("scoped-{seed}-{generation}"),
        );
        assert_success(&scoped);
        let report: Value = serde_json::from_slice(&scoped.stdout).unwrap();
        if generation == 4 {
            assert_eq!(report["resolution"]["resolution_mode"], "full");
            assert_eq!(
                report["resolution"]["fallback_reason"], "resolution_scope_crossover",
                "seed={seed} generation={generation} report={report}"
            );
        } else {
            assert_eq!(report["resolution"]["resolution_mode"], "scoped");
            assert!(report["resolution"]["fallback_reason"].is_null());
        }
        let full_artifact = self
            .temp
            .path()
            .join(format!("full-{seed}-{generation}.sqlite"));
        let scoped_artifact = self
            .temp
            .path()
            .join(format!("scoped-{seed}-{generation}.sqlite"));
        assert_success(&export(&self.full_store, &full_artifact));
        assert_success(&export(&self.scoped_store, &scoped_artifact));
        assert_eq!(
            semantic_digest(&full_artifact),
            semantic_digest(&scoped_artifact),
            "seed={seed} generation={generation}"
        );
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

fn wait_for_path(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !path.exists() {
        assert!(Instant::now() < deadline, "{}", path.display());
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn semantic_digest(path: &Path) -> String {
    let connection = Connection::open(path).unwrap();
    let mut digest = Sha256::new();
    for query in [
        "SELECT f.path,f.language,f.content_hash FROM files AS f ORDER BY f.path COLLATE BINARY",
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
                digest.update(format!("{:?}\0", row.get_ref(index).unwrap()).as_bytes());
            }
            digest.update(b"\n");
        }
        digest.update(b"\x1e");
    }
    format!("{:x}", digest.finalize())
}

#[test]
fn deterministic_seeded_multi_generation_sequences_match_forced_full() {
    for seed in [7, 19, 41] {
        let pair = SequencePair::new(seed);
        for generation in 1..=4 {
            pair.advance(seed, generation);
            pair.compare(seed, generation);
        }
    }
}

#[test]
fn cas_loss_against_a_later_manifest_never_publishes_stale_exact_output() {
    let pair = SequencePair::new(73);
    for root in pair.roots() {
        write_source(
            root,
            "src/core.ts",
            "export function core() { return 74; } export function local() { return core(); }\n",
        );
    }
    assert_success(&import(&pair.scoped_root, &pair.scoped_store));

    let pause = pair.temp.path().join("before-exact.pause");
    let mut child = Command::new(env!("CARGO_BIN_EXE_julie-extract"));
    child
        .args([
            "store",
            "resolve",
            "--store",
            pair.scoped_store.to_str().unwrap(),
            "--view",
            "view-main",
            "--request-id",
            "resolve-stale",
            "--idempotency-key",
            "resolve-stale-key",
            "--json",
        ])
        .env("JULIE_STORE_RESOLUTION_DELTA", "on")
        .env(
            "JULIE_EXTRACT_STORE_RESOLUTION_PAUSE_BEFORE_EXACT_FILE",
            &pause,
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = child.spawn().unwrap();
    wait_for_path(&pause);

    write_source(
        &pair.scoped_root,
        "src/stable0.ts",
        "function changed() { return 75; } export function stable0() { return changed(); }\n",
    );
    assert_success(&import(&pair.scoped_root, &pair.scoped_store));
    fs::write(pause.with_extension("resume"), b"resume").unwrap();
    let stale = child.wait_with_output().unwrap();
    assert_eq!(stale.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&stale.stdout).unwrap();
    assert_eq!(report["failure_class"], "resolution_failed");

    let connection = Connection::open(pair.scoped_store.join("gen-001/store.db")).unwrap();
    let (generation, exact_at, state): (i64, Option<i64>, String) = connection
        .query_row(
            "SELECT current_generation,resolution_exact_at,resolution_state
             FROM views WHERE view_id='view-main'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(generation, 3);
    assert!(state != "exact" || exact_at != Some(generation));
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM store_log
                 WHERE request_id='resolve-stale' AND event_kind='resolution_exact_published'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0,
        "generation={generation} exact_at={exact_at:?} state={state} report={report}"
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM store_log
                 WHERE request_id='resolve-stale' AND terminal=1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );

    let recovered = resolve(&pair.scoped_store, "on", "after-cas-loss");
    assert_success(&recovered);
    let recovered: Value = serde_json::from_slice(&recovered.stdout).unwrap();
    assert_eq!(recovered["manifest"]["generation"], 3);
    assert_eq!(recovered["resolution"]["exact_at_generation"], 3);
}
