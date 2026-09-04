use crate::ExtractionResults;
use crate::base::{ParseDiagnosticKind, SymbolKind};
use crate::factory::extract_symbols_and_relationships;
use crate::tests::helpers::init_parser;
use std::path::{Path, PathBuf};

fn extract_legacy(
    file_path: &str,
    language: &str,
    content: &str,
    workspace_root: &Path,
) -> ExtractionResults {
    let tree = init_parser(content, language);
    extract_symbols_and_relationships(&tree, file_path, content, language, workspace_root)
        .expect("legacy extraction should succeed")
}

fn assert_paths_are_normalized(results: &ExtractionResults, expected_file_path: &str) {
    assert!(!results.symbols.is_empty(), "expected extracted symbols");
    assert!(
        results
            .symbols
            .iter()
            .all(|symbol| symbol.file_path == expected_file_path),
        "expected all symbols to use normalized path {expected_file_path:?}, got {:?}",
        results
            .symbols
            .iter()
            .map(|symbol| symbol.file_path.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_extract_canonical_matches_legacy_factory_for_representative_languages() {
    let workspace_root = PathBuf::from("/test/workspace");
    let cases = [
        (
            "rust",
            "src/lib.rs",
            r#"
use crate::external::external_helper;

pub fn local_helper(input: i32) -> i32 {
    input + 1
}

pub fn process_data() -> Result<Vec<u8>, std::io::Error> {
    let value = local_helper(external_helper(41));
    Ok(vec![value as u8])
}
"#,
            vec!["local_helper", "process_data"],
            true,
            true,
            true,
            true,
        ),
        (
            "typescript",
            "src/app.ts",
            r#"
import { externalHelper } from "./external";

export function localHelper(input: number): number {
    return input + 1;
}

export function caller(): number {
    return localHelper(externalHelper(41));
}
"#,
            vec!["localHelper", "caller"],
            true,
            true,
            true,
            true,
        ),
        (
            "python",
            "src/app.py",
            r#"
from external import external_helper

def local_helper(input: int) -> int:
    return input + 1

def caller() -> int:
    return local_helper(external_helper(41))
"#,
            vec!["local_helper", "caller"],
            true,
            true,
            true,
            true,
        ),
    ];

    for (
        language,
        file_path,
        content,
        expected_symbol_names,
        expect_identifiers,
        expect_relationships,
        expect_pending_relationships,
        expect_types,
    ) in cases
    {
        let legacy = extract_legacy(file_path, language, content, &workspace_root);
        let canonical = crate::pipeline::extract_canonical(file_path, content, &workspace_root)
            .expect("canonical extraction should succeed");

        let legacy_names: Vec<_> = legacy
            .symbols
            .iter()
            .map(|symbol| symbol.name.as_str())
            .collect();
        let canonical_names: Vec<_> = canonical
            .symbols
            .iter()
            .map(|symbol| symbol.name.as_str())
            .collect();

        assert_eq!(
            canonical_names, legacy_names,
            "symbol names should stay in parity for {language}"
        );
        for expected_symbol_name in expected_symbol_names {
            assert!(
                canonical_names.contains(&expected_symbol_name),
                "expected symbol {expected_symbol_name:?} for {language}, got {:?}",
                canonical_names
            );
        }

        assert_paths_are_normalized(&canonical, file_path);

        assert_eq!(
            !canonical.identifiers.is_empty(),
            expect_identifiers,
            "identifier presence mismatch for {language}"
        );
        assert_eq!(
            !canonical.relationships.is_empty(),
            expect_relationships,
            "relationship presence mismatch for {language}"
        );
        assert_eq!(
            !canonical.pending_relationships.is_empty(),
            expect_pending_relationships,
            "pending relationship presence mismatch for {language}"
        );
        assert_eq!(
            !canonical.types.is_empty(),
            expect_types,
            "type presence mismatch for {language}"
        );

        assert_eq!(
            !legacy.identifiers.is_empty(),
            !canonical.identifiers.is_empty(),
            "legacy and canonical identifiers presence should match for {language}"
        );
        assert_eq!(
            !legacy.relationships.is_empty(),
            !canonical.relationships.is_empty(),
            "legacy and canonical relationships presence should match for {language}"
        );
        assert_eq!(
            !legacy.pending_relationships.is_empty(),
            !canonical.pending_relationships.is_empty(),
            "legacy and canonical pending relationship presence should match for {language}"
        );
        assert_eq!(
            !legacy.types.is_empty(),
            !canonical.types.is_empty(),
            "legacy and canonical types presence should match for {language}"
        );
    }
}

#[test]
fn test_extract_canonical_records_parse_diagnostics_without_dropping_recovered_symbols() {
    let workspace_root = PathBuf::from("/test/workspace");
    let content = r#"
package main

type Empty struct{}

type EmbeddedStruct struct {
    Empty
    value int
}

type MissingBrace struct {
    field int

func VariadicFunction(format string, args ...interface{}) {
    fmt.Printf(format, args...)
}
"#;

    let results = crate::pipeline::extract_canonical("src/recovery.go", content, &workspace_root)
        .expect("canonical extraction should recover partial Go syntax");

    assert!(
        !results.parse_diagnostics.is_empty(),
        "malformed recovered parse should record parse diagnostics"
    );
    assert!(
        results
            .parse_diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == ParseDiagnosticKind::Error),
        "malformed recovered parse should include an error diagnostic: {:?}",
        results.parse_diagnostics
    );

    let names: Vec<_> = results
        .symbols
        .iter()
        .map(|symbol| symbol.name.as_str())
        .collect();
    for expected_name in ["Empty", "EmbeddedStruct", "VariadicFunction"] {
        assert!(
            names.contains(&expected_name),
            "recovered parse should keep symbol {expected_name:?}; got {names:?}"
        );
    }
}

#[test]
fn test_extract_canonical_parse_none_returns_degraded_result_with_diagnostic() {
    let workspace_root = PathBuf::from("/test/workspace");
    let content = "fn main() {\n    println!(\"unterminated parse\")";

    let results = crate::pipeline::extract_canonical_with_parse(
        "src/broken.rs",
        content,
        &workspace_root,
        crate::base::ExtractionLevel::Full,
        |_language, _file_path, _content| Ok(None),
    )
    .expect("parser None should return a degraded extraction result");

    assert!(results.symbols.is_empty());
    assert!(results.relationships.is_empty());
    assert!(results.identifiers.is_empty());
    assert_eq!(results.parse_diagnostics.len(), 1);

    let diagnostic = &results.parse_diagnostics[0];
    assert_eq!(diagnostic.kind, ParseDiagnosticKind::Error);
    assert_eq!(diagnostic.start_line, 1);
    assert_eq!(diagnostic.start_column, 0);
    assert_eq!(diagnostic.start_byte, 0);
    assert_eq!(diagnostic.end_byte, content.len() as u32);
    assert_eq!(diagnostic.end_line, 2);
    assert_eq!(
        diagnostic.end_column,
        "    println!(\"unterminated parse\")".len() as u32
    );
}

#[test]
fn test_h_header_with_cpp_syntax_routes_to_cpp_extractor() {
    let workspace_root = PathBuf::from("/test/workspace");
    let content = r#"
#pragma once

namespace app {
class Widget {
public:
    void run() const;
};
}
"#;

    let results = crate::pipeline::extract_canonical("include/widget.h", content, &workspace_root)
        .expect("C++ header extraction should succeed");

    let widget = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "Widget" && symbol.kind == SymbolKind::Class)
        .unwrap_or_else(|| {
            panic!(
                "expected C++ class symbol in .h header: {:?}",
                results.symbols
            )
        });
    assert_eq!(widget.language, "cpp");
    assert!(
        results.symbols.iter().any(|symbol| {
            symbol.name == "run" && symbol.kind == SymbolKind::Method && symbol.language == "cpp"
        }),
        "expected C++ method symbol in .h header: {:?}",
        results.symbols
    );
}

#[test]
fn test_h_header_with_c_syntax_stays_c_extractor() {
    let workspace_root = PathBuf::from("/test/workspace");
    let content = r#"
#ifndef WIDGET_H
#define WIDGET_H

typedef struct widget {
    int id;
} widget_t;

void widget_init(widget_t *widget);

#endif
"#;

    let results = crate::pipeline::extract_canonical("include/widget.h", content, &workspace_root)
        .expect("C header extraction should succeed");

    assert!(
        results.symbols.iter().any(|symbol| {
            symbol.name == "widget_t" && symbol.kind == SymbolKind::Struct && symbol.language == "c"
        }),
        "expected C struct typedef symbol in .h header: {:?}",
        results.symbols
    );
    assert!(
        results.symbols.iter().any(|symbol| {
            symbol.name == "widget_init"
                && symbol.kind == SymbolKind::Function
                && symbol.language == "c"
        }),
        "expected C function declaration symbol in .h header: {:?}",
        results.symbols
    );
}

#[test]
fn test_detect_language_for_source_routes_cpp_h_header_and_preserves_c_header() {
    let cpp_header = r#"
#pragma once

namespace app {
class Widget {
public:
    void run() const;
};
}
"#;
    let c_header = r#"
#ifndef WIDGET_H
#define WIDGET_H

typedef struct widget {
    int id;
} widget_t;

void widget_init(widget_t *widget);

#endif
"#;

    assert_eq!(
        crate::language::detect_language_for_source("include/widget.h", cpp_header),
        Some("cpp"),
        "public source-aware language detection should route C++ .h headers to cpp"
    );
    assert_eq!(
        crate::language::detect_language_for_source("include/widget.h", c_header),
        Some("c"),
        "public source-aware language detection should preserve path-only C default for C .h headers"
    );
}

#[test]
fn test_detect_language_for_path_uses_source_contract_for_qmldir_basename() {
    assert_eq!(
        crate::pipeline::detect_language_for_path("modules/Example/qmldir").unwrap(),
        "qmldir"
    );
    assert!(crate::pipeline::detect_language_for_path("README").is_err());
    assert_eq!(
        crate::pipeline::detect_language_for_path("include/widget.h").unwrap(),
        "c"
    );
}

#[cfg(unix)]
#[test]
fn test_detect_language_for_path_ignores_invalid_utf8_parent_components() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let parent = PathBuf::from(OsString::from_vec(vec![b'm', 0xff, b'd']));
    assert_eq!(
        crate::language::detect_language_for_path(&parent.join("Widget.QML"), ""),
        Some("qml")
    );
    assert_eq!(
        crate::language::detect_language_for_path(&parent.join("Widget.qmltypes"), ""),
        Some("qml")
    );
    assert_eq!(
        crate::language::detect_language_for_path(&parent.join("qmldir"), ""),
        Some("qmldir")
    );
    assert_eq!(
        crate::language::detect_language_for_path(&parent.join("QMLDIR"), ""),
        Some("qmldir")
    );
    assert_eq!(
        crate::language::detect_language_for_path(
            &parent.join("widget.H"),
            "#pragma once\nnamespace app { class Widget { public: void run() const; }; }",
        ),
        Some("cpp")
    );
    assert_eq!(
        crate::language::detect_language_for_path(&parent.join("README"), ""),
        None
    );
}

#[test]
fn test_detect_language_for_source_preserves_c_headers_with_cpp_keyword_identifiers() {
    let c_header = r#"
#ifndef KEYWORDS_H
#define KEYWORDS_H

typedef struct namespace {
    int template;
    int requires;
} namespace_t;

void namespace_init(namespace_t *namespace);

#endif
"#;

    assert_eq!(
        crate::language::detect_language_for_source("include/keywords.h", c_header),
        Some("c"),
        "C identifiers named like C++ keywords must not force .h detection to cpp"
    );
}

#[test]
fn test_detect_language_for_source_preserves_c_headers_when_parser_prefers_c() {
    let c_header = r#"
#ifndef TEMPLATE_EXPR_H
#define TEMPLATE_EXPR_H

static inline int less_than_limit(int value) {
    int template = value;
    return template < 5;
}

#endif
"#;

    assert_eq!(
        crate::language::detect_language_for_source("include/labels.h", c_header),
        Some("c"),
        "parser comparison that prefers C must not fall through to C++ token heuristics"
    );
}

#[test]
fn test_detect_language_with_tree_returns_winning_tree_for_cpp_header() {
    let cpp_header = r#"
#pragma once
namespace app {
class Widget {
public:
    void run();
};
}
"#;

    let (language, tree) =
        crate::language::detect_language_with_tree(Path::new("include/widget.h"), cpp_header)
            .expect("detection should succeed");

    assert_eq!(language, "cpp");
    let tree = tree.expect("header disambiguation should produce a pre-parsed tree");
    assert!(!tree.root_node().has_error());
}

#[test]
fn test_detect_language_with_tree_returns_winning_tree_for_c_header() {
    let c_header = r#"
#ifndef C_HEADER_H
#define C_HEADER_H

typedef struct namespace {
    int template;
    int requires;
} namespace_t;

void init(namespace_t *n);

#endif
"#;

    let (language, tree) =
        crate::language::detect_language_with_tree(Path::new("include/c_header.h"), c_header)
            .expect("detection should succeed");

    assert_eq!(language, "c");
    let tree = tree.expect("header disambiguation should produce a pre-parsed tree");
    assert!(!tree.root_node().has_error());
}

#[test]
fn test_detect_language_with_tree_returns_none_tree_for_non_headers_and_empty() {
    let cpp_code = "class Widget {};";
    let (lang_cpp, tree_cpp) =
        crate::language::detect_language_with_tree(Path::new("src/widget.cpp"), cpp_code)
            .expect("cpp detection should succeed");
    assert_eq!(lang_cpp, "cpp");
    assert!(
        tree_cpp.is_none(),
        "non-header files should return None for tree"
    );

    let (lang_rust, tree_rust) =
        crate::language::detect_language_with_tree(Path::new("src/lib.rs"), "fn main() {}")
            .expect("rust detection should succeed");
    assert_eq!(lang_rust, "rust");
    assert!(
        tree_rust.is_none(),
        "non-header files should return None for tree"
    );

    let (lang_empty, tree_empty) =
        crate::language::detect_language_with_tree(Path::new("include/empty.h"), "   \n\t  ")
            .expect("empty header detection should succeed");
    assert_eq!(lang_empty, "c");
    assert!(
        tree_empty.is_none(),
        "empty headers should return None for tree"
    );
}

#[test]
fn test_header_extraction_reuses_pre_parsed_tree_and_parses_at_most_twice() {
    let cpp_header = r#"
#pragma once
namespace app {
class Widget {
public:
    void run();
};
}
"#;

    // 1. C++ header: disambiguation probes C and C++, and the pipeline reuses the C++ tree.
    crate::language_spec::reset_header_probe_parse_count();
    crate::pipeline::reset_parse_for_language_call_count();

    let results =
        crate::pipeline::extract_canonical("include/widget.h", cpp_header, Path::new("."))
            .expect("C++ header extraction should succeed");

    let probe_count = crate::language_spec::header_probe_parse_count();
    let pipeline_parse_count = crate::pipeline::parse_for_language_call_count();

    assert_eq!(
        probe_count, 2,
        "Header disambiguation must probe C and C++ exactly once each"
    );
    assert_eq!(
        pipeline_parse_count, 0,
        "Pipeline must reuse the pre-parsed tree without calling parse_for_language"
    );
    assert!(
        probe_count + pipeline_parse_count <= 2,
        "Invariant: non-empty .h file parsed at most twice end-to-end, never 3 times"
    );
    assert!(
        results.symbols.iter().any(|s| s.name == "Widget"),
        "Extracted symbols should include Widget class"
    );
    assert!(
        results.symbols.iter().any(|s| s.name == "run"),
        "Extracted symbols should include run method"
    );

    // 2. C header where C is preferred: pipeline reuses the C tree.
    let c_header = r#"
#ifndef C_HEADER_H
#define C_HEADER_H

typedef struct namespace {
    int template;
} namespace_t;

#endif
"#;

    crate::language_spec::reset_header_probe_parse_count();
    crate::pipeline::reset_parse_for_language_call_count();

    let c_results =
        crate::pipeline::extract_canonical("include/c_header.h", c_header, Path::new("."))
            .expect("C header extraction should succeed");

    let probe_count_c = crate::language_spec::header_probe_parse_count();
    let pipeline_parse_count_c = crate::pipeline::parse_for_language_call_count();

    assert_eq!(probe_count_c, 2, "Header disambiguation probes C and C++");
    assert_eq!(
        pipeline_parse_count_c, 0,
        "Pipeline must reuse the pre-parsed C tree without calling parse_for_language"
    );
    assert!(
        probe_count_c + pipeline_parse_count_c <= 2,
        "Invariant: non-empty .h file parsed at most twice end-to-end, never 3 times"
    );
    assert!(
        c_results.symbols.iter().any(|s| s.name == "namespace_t"),
        "Extracted symbols should include namespace_t"
    );

    // 3. Non-header C++ file: normal one-parse path in pipeline, zero header probes.
    crate::language_spec::reset_header_probe_parse_count();
    crate::pipeline::reset_parse_for_language_call_count();

    let non_header_results =
        crate::pipeline::extract_canonical("src/widget.cpp", cpp_header, Path::new("."))
            .expect("non-header extraction should succeed");

    let probe_count_non_header = crate::language_spec::header_probe_parse_count();
    let pipeline_parse_count_non_header = crate::pipeline::parse_for_language_call_count();

    assert_eq!(
        probe_count_non_header, 0,
        "Non-header file must not trigger header disambiguation probes"
    );
    assert_eq!(
        pipeline_parse_count_non_header, 1,
        "Non-header file must parse exactly once in pipeline"
    );
    assert!(
        non_header_results
            .symbols
            .iter()
            .any(|s| s.name == "Widget"),
        "Extracted symbols should include Widget class"
    );
}
