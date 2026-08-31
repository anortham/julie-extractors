use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;

use crate::base::StructuralFact;

const PATTERN_ID: &str = "code.marker.v1";

struct MarkerFixture {
    language: &'static str,
    file_path: &'static str,
    source: &'static str,
}

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical extraction should succeed")
}

fn marker_facts(results: &crate::ExtractionResults) -> Vec<&StructuralFact> {
    results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == PATTERN_ID)
        .collect()
}

fn metadata_string<'a>(fact: &'a StructuralFact, key: &str) -> Option<&'a str> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(Value::as_str)
}

#[test]
fn marker_facts_are_line_oriented_and_preserve_exact_semantic_spans() {
    let source = r#"fn work() {
    // TODO(alice): ship this
    // This prose mentions FIXME later.
    /*
     * FIXME - repair this
     * HACK(bob) remove workaround
     */
}

/// XXX: document this
pub fn documented() {}
"#;
    let results = extract("src/lib.rs", source);
    let facts = marker_facts(&results);

    assert_eq!(facts.len(), 4, "{facts:#?}");

    let expected = [
        (
            "TODO",
            Some("alice"),
            Some("ship this"),
            "TODO(alice): ship this",
        ),
        ("FIXME", None, Some("repair this"), "FIXME - repair this"),
        (
            "HACK",
            Some("bob"),
            Some("remove workaround"),
            "HACK(bob) remove workaround",
        ),
        ("XXX", None, Some("document this"), "XXX: document this"),
    ];

    for (fact, (marker, owner, description, semantic_text)) in facts.iter().zip(expected) {
        let start = source
            .find(semantic_text)
            .expect("semantic text should exist");
        let end = start + semantic_text.len();
        let expected_line = source[..start]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count() as u32
            + 1;
        let line_start = source[..start].rfind('\n').map_or(0, |index| index + 1);

        assert_eq!(fact.capture_name, "marker");
        assert_eq!(fact.start_byte, start as u32);
        assert_eq!(fact.end_byte, end as u32);
        assert_eq!(fact.start_line, expected_line);
        assert_eq!(fact.end_line, expected_line);
        assert_eq!(fact.start_column, (start - line_start) as u32);
        assert_eq!(fact.end_column, (end - line_start) as u32);
        assert_eq!(fact.confidence, 1.0);
        assert_eq!(metadata_string(fact, "marker"), Some(marker));
        assert_eq!(metadata_string(fact, "owner"), owner);
        assert_eq!(metadata_string(fact, "description"), description);
        assert_eq!(
            metadata_string(fact, "source_region_kind"),
            Some(fact.node_kind.as_str())
        );
        assert!(matches!(fact.node_kind.as_str(), "comment" | "doc_comment"));
    }

    let inner_fact = facts
        .iter()
        .find(|fact| metadata_string(fact, "marker") == Some("TODO"))
        .expect("inner function marker should be emitted");
    assert!(
        inner_fact.containing_symbol_id.is_some(),
        "inner marker should retain its source-region owner"
    );
    let doc_fact = facts
        .iter()
        .find(|fact| metadata_string(fact, "marker") == Some("XXX"))
        .expect("doc marker should be emitted");
    assert!(
        doc_fact.containing_symbol_id.is_some(),
        "doc marker should retain its documented symbol owner"
    );
}

#[test]
fn marker_matching_is_case_insensitive_but_requires_the_first_semantic_token() {
    let source = r#"// todo: lower case
// note TODO: prose only
// prefixFIXME: not a token
// HACKING is not HACK
"#;
    let results = extract("src/main.rs", source);
    let facts = marker_facts(&results);

    assert_eq!(facts.len(), 1, "{facts:#?}");
    assert_eq!(metadata_string(facts[0], "marker"), Some("TODO"));
    assert_eq!(
        &source[facts[0].start_byte as usize..facts[0].end_byte as usize],
        "todo: lower case"
    );
}

#[test]
fn malformed_owner_does_not_suppress_marker_fact() {
    let source = "// TODO(alice: preserve this marker\n";
    let results = extract("src/main.rs", source);
    let facts = marker_facts(&results);

    assert_eq!(facts.len(), 1, "{facts:#?}");
    assert_eq!(metadata_string(facts[0], "marker"), Some("TODO"));
    assert_eq!(metadata_string(facts[0], "owner"), None);
    assert_eq!(
        metadata_string(facts[0], "description"),
        Some("(alice: preserve this marker")
    );
}

#[test]
fn marker_language_matrix_covers_every_supported_comment_language() {
    let fixtures = [
        MarkerFixture {
            language: "rust",
            file_path: "src/lib.rs",
            source: "// TODO: marker\n",
        },
        MarkerFixture {
            language: "c",
            file_path: "src/main.c",
            source: "// TODO: marker\nint main(void) { return 0; }\n",
        },
        MarkerFixture {
            language: "cpp",
            file_path: "src/main.cpp",
            source: "// TODO: marker\nint main() { return 0; }\n",
        },
        MarkerFixture {
            language: "go",
            file_path: "main.go",
            source: "package main\n// TODO: marker\nfunc main() {}\n",
        },
        MarkerFixture {
            language: "zig",
            file_path: "src/main.zig",
            source: "// TODO: marker\npub fn main() void {}\n",
        },
        MarkerFixture {
            language: "typescript",
            file_path: "src/main.ts",
            source: "// TODO: marker\nconst value = 1;\n",
        },
        MarkerFixture {
            language: "tsx",
            file_path: "src/main.tsx",
            source: "// TODO: marker\nconst node = <div />;\n",
        },
        MarkerFixture {
            language: "javascript",
            file_path: "src/main.js",
            source: "// TODO: marker\nconst value = 1;\n",
        },
        MarkerFixture {
            language: "jsx",
            file_path: "src/main.jsx",
            source: "// TODO: marker\nconst node = <div />;\n",
        },
        MarkerFixture {
            language: "html",
            file_path: "index.html",
            source: "<!-- TODO: marker -->\n<div></div>\n",
        },
        MarkerFixture {
            language: "css",
            file_path: "styles.css",
            source: "/* TODO: marker */\nbody {}\n",
        },
        MarkerFixture {
            language: "vue",
            file_path: "src/App.vue",
            source: "<!-- TODO: marker -->\n<template><div /></template>\n",
        },
        MarkerFixture {
            language: "python",
            file_path: "src/main.py",
            source: "# TODO: marker\nvalue = 1\n",
        },
        MarkerFixture {
            language: "java",
            file_path: "src/Main.java",
            source: "// TODO: marker\nclass Main {}\n",
        },
        MarkerFixture {
            language: "csharp",
            file_path: "src/Main.cs",
            source: "// TODO: marker\nclass Main {}\n",
        },
        MarkerFixture {
            language: "vbnet",
            file_path: "src/Main.vb",
            source: "' TODO: marker\nModule Main\nEnd Module\n",
        },
        MarkerFixture {
            language: "php",
            file_path: "src/main.php",
            source: "<?php\n// TODO: marker\n$value = 1;\n",
        },
        MarkerFixture {
            language: "ruby",
            file_path: "src/main.rb",
            source: "# TODO: marker\nvalue = 1\n",
        },
        MarkerFixture {
            language: "swift",
            file_path: "src/main.swift",
            source: "// TODO: marker\nlet value = 1\n",
        },
        MarkerFixture {
            language: "kotlin",
            file_path: "src/main.kt",
            source: "// TODO: marker\nval value = 1\n",
        },
        MarkerFixture {
            language: "scala",
            file_path: "src/Main.scala",
            source: "// TODO: marker\nobject Main\n",
        },
        MarkerFixture {
            language: "dart",
            file_path: "lib/main.dart",
            source: "// TODO: marker\nfinal value = 1;\n",
        },
        MarkerFixture {
            language: "elixir",
            file_path: "lib/main.ex",
            source: "# TODO: marker\ndefmodule Main do\nend\n",
        },
        MarkerFixture {
            language: "erlang",
            file_path: "src/main.erl",
            source: "%% TODO: marker\n-module(main).\n",
        },
        MarkerFixture {
            language: "fsharp",
            file_path: "src/Program.fs",
            source: "// TODO: marker\nmodule Main\nlet value = 1\n",
        },
        MarkerFixture {
            language: "lua",
            file_path: "src/main.lua",
            source: "-- TODO: marker\nlocal value = 1\n",
        },
        MarkerFixture {
            language: "qml",
            file_path: "src/Main.qml",
            source: "// TODO: marker\nItem {}\n",
        },
        MarkerFixture {
            language: "r",
            file_path: "src/main.R",
            source: "# TODO: marker\nvalue <- 1\n",
        },
        MarkerFixture {
            language: "bash",
            file_path: "scripts/main.sh",
            source: "# TODO: marker\nvalue=1\n",
        },
        MarkerFixture {
            language: "powershell",
            file_path: "scripts/main.ps1",
            source: "# TODO: marker\n$Value = 1\n",
        },
        MarkerFixture {
            language: "gdscript",
            file_path: "src/main.gd",
            source: "# TODO: marker\nvar value = 1\n",
        },
        MarkerFixture {
            language: "razor",
            file_path: "Pages/Index.razor",
            source: "@* TODO: marker *@\n<div></div>\n",
        },
        MarkerFixture {
            language: "sql",
            file_path: "schema.sql",
            source: "-- TODO: marker\nSELECT 1;\n",
        },
        MarkerFixture {
            language: "markdown",
            file_path: "README.md",
            source: "<!-- TODO: marker -->\n# Title\n",
        },
        MarkerFixture {
            language: "json",
            file_path: "config.jsonc",
            source: "// TODO: marker\n{\"value\":1}\n",
        },
        MarkerFixture {
            language: "toml",
            file_path: "config.toml",
            source: "# TODO: marker\nvalue = 1\n",
        },
        MarkerFixture {
            language: "yaml",
            file_path: "config.yaml",
            source: "# TODO: marker\nvalue: 1\n",
        },
        MarkerFixture {
            language: "xml",
            file_path: "config.xml",
            source: "<!-- TODO: marker -->\n<config name=\"value\"/>\n",
        },
    ];
    let applicable = fixtures
        .iter()
        .map(|fixture| fixture.language)
        .collect::<BTreeSet<_>>();
    let not_applicable = ["qmldir", "regex"].into_iter().collect::<BTreeSet<_>>();
    let covered = applicable
        .union(&not_applicable)
        .copied()
        .collect::<BTreeSet<_>>();
    let supported = crate::language::supported_languages()
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let registered = crate::base::structural_fact_pattern_specs()
        .iter()
        .find(|spec| spec.pattern_id == PATTERN_ID)
        .expect("marker pattern should be registered")
        .languages
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();

    assert_eq!(covered, supported);
    assert_eq!(registered, applicable);
    assert!(applicable.is_disjoint(&not_applicable));

    for fixture in fixtures {
        let results = extract(fixture.file_path, fixture.source);
        let facts = marker_facts(&results);
        assert_eq!(
            facts.len(),
            1,
            "{} fixture should emit one marker fact, got {facts:#?}",
            fixture.language
        );
        assert_eq!(facts[0].language, fixture.language);
        assert_eq!(metadata_string(facts[0], "marker"), Some("TODO"));
    }
}
