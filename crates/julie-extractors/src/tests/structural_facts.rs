use std::collections::BTreeSet;
use std::path::Path;

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical extraction should succeed")
}

#[test]
fn rust_unsafe_blocks_emit_structural_facts_with_containing_symbol() {
    let source = r#"pub fn read_flag(value: &i32) -> i32 {
    unsafe {
        core::ptr::read_volatile(value)
    }
}
"#;

    let results = extract("src/lib.rs", source);
    let read_flag = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "read_flag")
        .expect("expected read_flag symbol");

    let fact = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "rust.unsafe_block.v1")
        .expect("expected unsafe-block structural fact");

    assert_eq!(fact.capture_name, "unsafe_block");
    assert_eq!(fact.node_kind, "unsafe_block");
    assert_eq!(
        fact.containing_symbol_id.as_deref(),
        Some(read_flag.id.as_str())
    );
    assert_eq!(fact.confidence, 1.0);
    assert_eq!(
        fact.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("query_family"))
            .and_then(|value| value.as_str()),
        Some("safety")
    );
    assert!(fact.end_byte > fact.start_byte);
}

#[derive(Debug)]
struct StructuralFactCase {
    file_path: &'static str,
    source: &'static str,
    expected: &'static [ExpectedStructuralFact],
}

#[derive(Debug)]
struct ExpectedStructuralFact {
    pattern_id: &'static str,
    capture_name: &'static str,
    query_family: &'static str,
    node_kinds: &'static [&'static str],
}

#[test]
fn supported_structural_patterns_emit_parser_backed_facts() {
    let cases = [
        StructuralFactCase {
            file_path: "src/lib.rs",
            source: r#"pub fn read_flag(value: &i32) -> i32 {
    unsafe {
        core::ptr::read_volatile(value)
    }
}
"#,
            expected: &[ExpectedStructuralFact {
                pattern_id: "rust.unsafe_block.v1",
                capture_name: "unsafe_block",
                query_family: "safety",
                node_kinds: &["unsafe_block"],
            }],
        },
        StructuralFactCase {
            file_path: "src/service.go",
            source: r#"package main

func worker() {}
func cleanup() {}

func run() {
    go worker()
    defer cleanup()
}
"#,
            expected: &[
                ExpectedStructuralFact {
                    pattern_id: "go.goroutine_launch.v1",
                    capture_name: "go_statement",
                    query_family: "concurrency",
                    node_kinds: &["go_statement"],
                },
                ExpectedStructuralFact {
                    pattern_id: "go.defer_statement.v1",
                    capture_name: "defer_statement",
                    query_family: "lifecycle",
                    node_kinds: &["defer_statement"],
                },
            ],
        },
        StructuralFactCase {
            file_path: "src/decorators.py",
            source: r#"def timed(fn):
    return fn

@timed
def run():
    return 1
"#,
            expected: &[ExpectedStructuralFact {
                pattern_id: "python.decorated_definition.v1",
                capture_name: "decorated_definition",
                query_family: "metadata",
                node_kinds: &["decorated_definition"],
            }],
        },
        StructuralFactCase {
            file_path: "src/load.js",
            source: r#"export async function load() {
    return await fetch("/api");
}
"#,
            expected: &[ExpectedStructuralFact {
                pattern_id: "javascript.await_expression.v1",
                capture_name: "await_expression",
                query_family: "async",
                node_kinds: &["await_expression"],
            }],
        },
        StructuralFactCase {
            file_path: "src/View.jsx",
            source: r#"export async function View() {
    const data = await load();
    return <div>{data}</div>;
}
"#,
            expected: &[ExpectedStructuralFact {
                pattern_id: "jsx.await_expression.v1",
                capture_name: "await_expression",
                query_family: "async",
                node_kinds: &["await_expression"],
            }],
        },
        StructuralFactCase {
            file_path: "src/load.ts",
            source: r#"export async function load(): Promise<Response> {
    return await fetch("/api");
}
"#,
            expected: &[ExpectedStructuralFact {
                pattern_id: "typescript.await_expression.v1",
                capture_name: "await_expression",
                query_family: "async",
                node_kinds: &["await_expression"],
            }],
        },
        StructuralFactCase {
            file_path: "src/View.tsx",
            source: r#"export async function View() {
    const data = await load();
    return <div>{data}</div>;
}
"#,
            expected: &[ExpectedStructuralFact {
                pattern_id: "tsx.await_expression.v1",
                capture_name: "await_expression",
                query_family: "async",
                node_kinds: &["await_expression"],
            }],
        },
        StructuralFactCase {
            file_path: "src/config.c",
            source: r#"#define LIMIT 4
#define DOUBLE(x) ((x) * 2)

int read_value(void) {
    return DOUBLE(LIMIT);
}
"#,
            expected: &[ExpectedStructuralFact {
                pattern_id: "c.preprocessor_definition.v1",
                capture_name: "preprocessor_definition",
                query_family: "preprocessor",
                node_kinds: &["preproc_def", "preproc_function_def"],
            }],
        },
        StructuralFactCase {
            file_path: "src/config.cpp",
            source: r#"#define LIMIT 4
#define DOUBLE(x) ((x) * 2)

int readValue() {
    return DOUBLE(LIMIT);
}
"#,
            expected: &[ExpectedStructuralFact {
                pattern_id: "cpp.preprocessor_definition.v1",
                capture_name: "preprocessor_definition",
                query_family: "preprocessor",
                node_kinds: &["preproc_def", "preproc_function_def"],
            }],
        },
    ];

    for case in cases {
        let results = extract(case.file_path, case.source);
        let expected_ids = case
            .expected
            .iter()
            .map(|expected| expected.pattern_id.to_string())
            .collect::<BTreeSet<_>>();
        let actual_ids = results
            .structural_facts
            .iter()
            .map(|fact| fact.pattern_id.clone())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            actual_ids, expected_ids,
            "{} emitted unexpected structural pattern ids",
            case.file_path
        );

        for expected in case.expected {
            let facts = results
                .structural_facts
                .iter()
                .filter(|fact| fact.pattern_id == expected.pattern_id)
                .collect::<Vec<_>>();
            let actual_node_kinds = facts
                .iter()
                .map(|fact| fact.node_kind.clone())
                .collect::<BTreeSet<_>>();
            let expected_node_kinds = expected
                .node_kinds
                .iter()
                .map(|kind| (*kind).to_string())
                .collect::<BTreeSet<_>>();
            assert_eq!(
                actual_node_kinds, expected_node_kinds,
                "{} emitted wrong node kinds for {}",
                case.file_path, expected.pattern_id
            );
            for fact in facts {
                assert_eq!(fact.capture_name, expected.capture_name);
                assert_eq!(fact.confidence, 1.0);
                assert_eq!(
                    fact.metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("query_family"))
                        .and_then(|value| value.as_str()),
                    Some(expected.query_family)
                );
                assert_eq!(
                    fact.metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("pattern_version"))
                        .and_then(|value| value.as_u64()),
                    Some(1)
                );
                assert!(fact.end_byte > fact.start_byte);
            }
        }
    }
}
