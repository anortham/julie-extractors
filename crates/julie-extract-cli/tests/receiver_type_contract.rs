use julie_extractors::{ExtractionLevel, extract_canonical_for_language_at};
use rusqlite::Connection;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const CASES: &[(&str, &str, &str, &str)] = &[
    ("rust", "rust/basic/source.rs", "mark", "Worker"),
    ("cpp", "cpp/basic/source.cpp", "helper", "Worker"),
    ("go", "go/basic/source.go", "Run", "Worker"),
    ("zig", "zig/basic/source.zig", "close", "Pool"),
    (
        "typescript",
        "typescript/language_gaps/source.ts",
        "area",
        "Shape",
    ),
    (
        "javascript",
        "javascript/language_gaps/source.js",
        "clear",
        "Queue",
    ),
    ("html", "html/basic/source.html", "render", "WorkerCard"),
    (
        "python",
        "python/django_tests/tests.py",
        "assertEqual",
        "AsyncTests",
    ),
    ("java", "java/basic/source.java", "recordRun", "Worker"),
    (
        "csharp",
        "csharp/base_lists_and_calls/source.cs",
        "Save",
        "RepositoryBase",
    ),
    ("vbnet", "vbnet/basic/source.vb", "Run", "Worker"),
    ("php", "php/basic/source.php", "missingWave2", "Worker"),
    ("ruby", "ruby/basic/source.rb", "helper", "Worker"),
    (
        "swift",
        "swift/basic/source.swift",
        "persist",
        "ReceiverBox",
    ),
    ("kotlin", "kotlin/basic/source.kt", "missingWave2", "Worker"),
    ("scala", "scala/basic/source.scala", "m", "Widget"),
    ("dart", "dart/basic/source.dart", "persist", "OrderService"),
    ("fsharp", "fsharp/basic/source.fs", "Helper", "Calculator"),
    ("lua", "lua/basic/source.lua", "missing_wave2", "Worker"),
    ("qml", "qml/basic/source.qml", "format", "source"),
    ("r", "r/basic/source.r", "missing_wave2", "Worker"),
    ("powershell", "powershell/basic/source.ps1", "Run", "Widget"),
    ("gdscript", "gdscript/basic/source.gd", "persist", "Worker"),
    ("razor", "razor/basic/source.razor", "Refresh", "WorkerPage"),
    ("sql", "sql/advanced_dml/source.sql", "Id", "Workers"),
];

fn scan(root: &Path, db: &Path, level: &str) {
    let output = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .arg("scan")
        .arg("--root")
        .arg(root)
        .arg("--db")
        .arg(db)
        .args(["--level", level, "--jobs", "1", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn receiver_types_survive_canonical_extraction_and_artifact_levels() {
    let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/extraction");
    let temp = tempfile::tempdir().unwrap();
    let source_root = temp.path().join("source");
    let mut identifier_types = BTreeMap::<&str, BTreeSet<_>>::new();
    let mut pending_types = BTreeSet::new();
    for &(language, path, target, expected_type) in CASES {
        let source = fs::read_to_string(fixture_root.join(path)).unwrap();
        let destination = source_root.join(path);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::write(destination, &source).unwrap();
        let mut full_pending = BTreeSet::new();
        let mut full_member_types = BTreeSet::new();
        for (level_name, level) in [
            ("full", ExtractionLevel::Full),
            ("facts", ExtractionLevel::Facts),
            ("symbols", ExtractionLevel::Symbols),
        ] {
            let results =
                extract_canonical_for_language_at(language, path, &source, &source_root, level)
                    .unwrap();
            if level_name == "full" {
                assert!(
                    results.identifiers.iter().any(|identifier| {
                        identifier.name == target
                            && identifier.receiver_type.as_deref() == Some(expected_type)
                    }),
                    "{language}: {target} must retain receiver type {expected_type}"
                );
                full_member_types.extend(
                    results
                        .identifiers
                        .iter()
                        .filter(|identifier| {
                            matches!(
                                identifier.kind,
                                julie_extractors::IdentifierKind::TypeUsage
                                    | julie_extractors::IdentifierKind::MemberAccess
                            )
                        })
                        .filter_map(|identifier| {
                            identifier
                                .receiver_type
                                .as_ref()
                                .map(|receiver_type| (identifier.id.clone(), receiver_type.clone()))
                        }),
                );
            } else if level_name == "facts" {
                let member_types: BTreeSet<_> = results
                    .identifiers
                    .iter()
                    .filter_map(|identifier| {
                        identifier
                            .receiver_type
                            .as_ref()
                            .map(|receiver_type| (identifier.id.clone(), receiver_type.clone()))
                    })
                    .collect();
                assert_eq!(member_types, full_member_types, "{language}: facts types");
            } else {
                assert!(results.identifiers.is_empty(), "{language}: symbols level");
            }
            identifier_types.entry(level_name).or_default().extend(
                results.identifiers.iter().filter_map(|identifier| {
                    identifier.receiver_type.as_ref().map(|receiver_type| {
                        (
                            path.to_string(),
                            identifier.name.clone(),
                            i64::from(identifier.start_byte),
                            i64::from(identifier.end_byte),
                            receiver_type.clone(),
                        )
                    })
                }),
            );
            let current_pending: BTreeSet<_> = results
                .structured_pending_relationships
                .iter()
                .filter_map(|pending| {
                    pending.receiver_type.as_ref().map(|receiver_type| {
                        (
                            path.to_string(),
                            pending.target.terminal_name.clone(),
                            i64::from(pending.pending.line_number),
                            receiver_type.clone(),
                        )
                    })
                })
                .collect();
            if level_name == "full" {
                full_pending = current_pending;
            } else {
                assert_eq!(
                    current_pending, full_pending,
                    "{language}: {level_name} pending types"
                );
            }
        }
        pending_types.extend(full_pending);
    }
    assert!(!pending_types.is_empty());

    for level in ["full", "facts", "symbols"] {
        let db = temp.path().join(format!("{level}.sqlite"));
        scan(&source_root, &db, level);
        let conn = Connection::open(db).unwrap();
        let actual_identifier_types: BTreeSet<_> = conn
            .prepare(
                "SELECT f.path, i.name, i.start_byte, i.end_byte,
                    json_extract(i.metadata_json, '$.receiver_type')
             FROM identifiers i JOIN files f USING(file_id)
             WHERE json_extract(i.metadata_json, '$.receiver_type') IS NOT NULL",
            )
            .unwrap()
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            actual_identifier_types, identifier_types[level],
            "{level}: identifier types"
        );
        let actual_pending_types: BTreeSet<_> = conn
            .prepare(
                "SELECT f.path, p.target_terminal_name, p.start_line,
                    json_extract(p.metadata_json, '$.receiver_type')
             FROM pending_relationships p JOIN files f USING(file_id)
             WHERE json_extract(p.metadata_json, '$.receiver_type') IS NOT NULL",
            )
            .unwrap()
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            actual_pending_types, pending_types,
            "{level}: pending types"
        );
    }
}

#[test]
fn receiver_types_survive_ecmascript_variants_and_embedded_hosts() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("source");
    fs::create_dir_all(&root).unwrap();
    let script = "class Cart { run() { this.persist(); } }";
    let mut expected = BTreeMap::new();
    for (language, extension) in [
        ("javascript", "js"),
        ("jsx", "jsx"),
        ("typescript", "ts"),
        ("tsx", "tsx"),
        ("html", "html"),
        ("vue", "vue"),
    ] {
        let path = format!("source.{extension}");
        let source = if matches!(language, "html" | "vue") {
            format!("<script>{script}</script>")
        } else {
            script.to_string()
        };
        fs::write(root.join(&path), &source).unwrap();
        let results = extract_canonical_for_language_at(
            language,
            &path,
            &source,
            &root,
            ExtractionLevel::Full,
        )
        .unwrap();
        let call = results
            .identifiers
            .iter()
            .find(|identifier| {
                identifier.name == "persist"
                    && identifier.kind == julie_extractors::IdentifierKind::Call
            })
            .unwrap_or_else(|| panic!("{language}: missing persist call"));
        assert_eq!(call.receiver_type.as_deref(), Some("Cart"), "{language}");
        expected.insert(path, "Cart".to_string());
    }
    let db = temp.path().join("artifact.sqlite");
    scan(&root, &db, "full");
    let conn = Connection::open(db).unwrap();
    let actual: BTreeMap<String, String> = conn
        .prepare(
            "SELECT f.path, json_extract(i.metadata_json, '$.receiver_type')
         FROM identifiers i JOIN files f USING(file_id)
         WHERE i.name = 'persist' AND i.kind = 'call'",
        )
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(actual, expected);
}
