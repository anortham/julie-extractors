use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use crate::base::SourceRegionKind;

struct SourceRegionFixture {
    language: &'static str,
    file_path: &'static str,
    source: &'static str,
    expected_kinds: &'static [SourceRegionKind],
}

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical extraction should succeed")
}

#[test]
fn swift_source_regions_preserve_interpolation_code_and_literal_text_boundaries() {
    let source = include_str!("../../../../fixtures/extraction/swift/source_regions/source.swift");
    let results = extract("source_regions/source.swift", source);
    let actual_regions = results
        .source_regions
        .iter()
        .map(|region| {
            (
                region.kind.as_str(),
                source
                    .get(region.start_byte as usize..region.end_byte as usize)
                    .expect("source region span must select valid source text"),
            )
        })
        .collect::<Vec<_>>();
    let expected_regions = [
        ("comment", "// region comment"),
        ("doc_comment", "/// region doc"),
        ("string_literal", "\"/* plain string content */\""),
        (
            "string_literal",
            "\"\"\"\n// multiline string content\n/* multiline block content */\n\"\"\"",
        ),
        (
            "string_literal",
            "\"value \\(/* interpolation comment */ café)\"",
        ),
        ("comment", "/* interpolation comment */"),
        (
            "string_literal",
            "#\"raw /* raw text */ \\#(/* raw interpolation comment */ café)\"#",
        ),
        ("comment", "/* raw interpolation comment */"),
    ];

    assert_eq!(actual_regions, expected_regions);
    for (region, (_, text)) in results.source_regions.iter().zip(expected_regions) {
        let start = source
            .find(text)
            .expect("expected source region text must appear in fixture");
        assert_eq!(region.start_byte as usize, start, "{text}");
        assert_eq!(region.end_byte as usize, start + text.len(), "{text}");
    }
}

fn syntax_node_count(node: tree_sitter::Node<'_>) -> usize {
    let mut count = 1;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        count += syntax_node_count(child);
    }
    count
}

#[test]
fn swift_source_region_visits_and_nested_literal_spans_scale_linearly() {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_swift::LANGUAGE.into())
        .expect("Error loading Swift grammar");
    let mut visits_by_size = Vec::new();

    for literal_count in [8, 16, 32] {
        let mut source = String::new();
        let mut expected = Vec::new();
        for index in 0..literal_count {
            source.push_str(&format!("let value_{index} = "));
            let outer = format!(
                r#""outer 💡 /* literal text */ \(render("inner 💡 /* nested literal text */", /* interpolation code */ value_{index}))""#
            );
            let outer_start = source.len();
            let nested = r#""inner 💡 /* nested literal text */""#;
            let code_comment = "/* interpolation code */";
            let nested_start = outer_start + outer.find(nested).expect("nested string");
            let code_comment_start =
                outer_start + outer.find(code_comment).expect("interpolation comment");
            source.push_str(&outer);
            source.push('\n');
            expected.push((
                "string_literal".to_string(),
                outer_start,
                outer_start + outer.len(),
                outer,
            ));
            expected.push((
                "string_literal".to_string(),
                nested_start,
                nested_start + nested.len(),
                nested.to_string(),
            ));
            expected.push((
                "comment".to_string(),
                code_comment_start,
                code_comment_start + code_comment.len(),
                code_comment.to_string(),
            ));
        }
        expected.sort_by_key(|region| region.1);

        let tree = parser.parse(&source, None).expect("parse Swift source");
        assert!(
            !tree.root_node().has_error(),
            "{}",
            tree.root_node().to_sexp()
        );
        let (regions, visits) = crate::base::source_regions::collect_source_regions_counted(
            "swift",
            &tree,
            "source_regions/scaling.swift",
            &source,
            &[],
        );
        let actual = regions
            .iter()
            .map(|region| {
                (
                    region.kind.as_str().to_string(),
                    region.start_byte as usize,
                    region.end_byte as usize,
                    source[region.start_byte as usize..region.end_byte as usize].to_string(),
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(actual, expected);
        assert_eq!(visits, syntax_node_count(tree.root_node()));
        visits_by_size.push(visits);
    }

    for window in visits_by_size.windows(2) {
        assert_eq!(window[1], window[0] * 2 - 1);
    }
}

#[test]
fn supported_languages_with_source_region_syntax_emit_regions() {
    use SourceRegionKind::{Comment, DocComment, Embedded, StringLiteral};

    let fixtures = [
        SourceRegionFixture {
            language: "rust",
            file_path: "src/lib.rs",
            source: r#"// "not a string"
/// doc
pub fn greet() {
    let name = "/* not a comment */";
}
"#,
            expected_kinds: &[Comment, DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "c",
            file_path: "src/main.c",
            source: r#"// "not a string"
/** doc */
const char *name = "/* not a comment */";
"#,
            expected_kinds: &[Comment, DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "cpp",
            file_path: "src/main.cpp",
            source: r#"// "not a string"
/** doc */
std::string name = "/* not a comment */";
"#,
            expected_kinds: &[Comment, DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "go",
            file_path: "main.go",
            source: r#"package main
// "not a string"
func main() {
    name := "// not a comment"
    _ = name
}
"#,
            expected_kinds: &[DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "zig",
            file_path: "src/main.zig",
            source: r#"// "not a string"
/// doc
const name = "/* not a comment */";
"#,
            expected_kinds: &[Comment, DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "typescript",
            file_path: "src/main.ts",
            source: r#"// "not a string"
/** doc */
const name: string = "/* not a comment */";
"#,
            expected_kinds: &[Comment, DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "tsx",
            file_path: "src/main.tsx",
            source: r#"// "not a string"
const node = <div title="/* not a comment */">text</div>;
"#,
            expected_kinds: &[Comment, StringLiteral],
        },
        SourceRegionFixture {
            language: "javascript",
            file_path: "src/main.js",
            source: r#"// "not a string"
/** doc */
const name = "/* not a comment */";
const templated = `value ${/* template expression comment */ name}`;
"#,
            expected_kinds: &[Comment, DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "jsx",
            file_path: "src/main.jsx",
            source: r#"// "not a string"
const node = <div title="/* not a comment */">text</div>;
"#,
            expected_kinds: &[Comment, StringLiteral],
        },
        SourceRegionFixture {
            language: "html",
            file_path: "index.html",
            source: r#"<!-- doc -->
<div title="hi"><script>const name = "hi";</script><style>body { color: red; }</style></div>
"#,
            expected_kinds: &[DocComment, StringLiteral, Embedded],
        },
        SourceRegionFixture {
            language: "css",
            file_path: "styles.css",
            source: r#"/* doc */
body::before { content: "hi"; }
"#,
            expected_kinds: &[DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "vue",
            file_path: "src/App.vue",
            source: r#"<!-- doc -->
<template><button title="/* not a comment */">Count</button></template>
<script>const name = "// not a comment";</script>
<style>button { color: red; }</style>
"#,
            expected_kinds: &[DocComment, StringLiteral, Embedded],
        },
        SourceRegionFixture {
            language: "python",
            file_path: "src/main.py",
            source: r#"# "not a string"
name = "not # a comment"
"#,
            expected_kinds: &[Comment, StringLiteral],
        },
        SourceRegionFixture {
            language: "java",
            file_path: "src/Main.java",
            source: r#"// "not a string"
/** doc */
class Worker {
    String name = "/* not a comment */";
}
"#,
            expected_kinds: &[Comment, DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "gomod",
            file_path: "go.mod",
            source: r#"// The module.
module "example.com/app"

require example.com/a v1.0.0 // indirect
"#,
            expected_kinds: &[Comment, DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "csharp",
            file_path: "src/Worker.cs",
            source: r#"// "not a string"
/// <summary>doc</summary>
public class Worker {
    string Name = "/* not a comment */";
}
"#,
            expected_kinds: &[Comment, DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "vbnet",
            file_path: "src/Worker.vb",
            source: r#"' "not a string"
''' doc
Module Worker
    Dim Name As String = "not ' a comment"
End Module
"#,
            expected_kinds: &[Comment, DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "php",
            file_path: "src/main.php",
            source: r#"<?php
// "not a string"
/** doc */
$name = "/* not a comment */";
"#,
            expected_kinds: &[Comment, DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "ruby",
            file_path: "src/main.rb",
            source: r#"# "not a string"
name = "not # a comment"
"#,
            expected_kinds: &[DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "swift",
            file_path: "src/main.swift",
            source: r##"// "not a string"
/// doc
let café = 1
let name = "/* not a comment */"
let rendered = "before \(/* interpolation block */ format("nested // not a comment")) after"
let raw = #"/* raw text */ \#(/* raw interpolation block */ value)"#
let multiline = """
/* multiline text */
"""
"##,
            expected_kinds: &[Comment, DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "kotlin",
            file_path: "src/main.kt",
            source: r#"// "not a string"
/** doc */
val name = "/* not a comment */"
"#,
            expected_kinds: &[Comment, DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "scala",
            file_path: "src/Main.scala",
            source: r#"// "not a string"
/** block */
object Worker { val name = "/* not a comment */" }
"#,
            expected_kinds: &[Comment, DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "dart",
            file_path: "lib/main.dart",
            source: r#"// "not a string"
/// doc
final name = "/* not a comment */";
"#,
            expected_kinds: &[Comment, DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "elixir",
            file_path: "lib/worker.ex",
            source: r#"defmodule Worker do
  # plain
  @name "hi"
end
"#,
            expected_kinds: &[Comment, StringLiteral],
        },
        SourceRegionFixture {
            language: "lua",
            file_path: "src/main.lua",
            source: r#"--- "not a string"
--- doc
local name = "-- not a comment"
"#,
            expected_kinds: &[DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "qml",
            file_path: "src/Main.qml",
            source: r#"// "not a string"
Item { property string name: "// not a comment" }
"#,
            expected_kinds: &[Comment, StringLiteral],
        },
        SourceRegionFixture {
            language: "r",
            file_path: "src/main.R",
            source: r#"#' doc
name <- "hi"
"#,
            expected_kinds: &[DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "bash",
            file_path: "scripts/main.sh",
            source: r#"# doc
name="hi"
"#,
            expected_kinds: &[DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "powershell",
            file_path: "scripts/main.ps1",
            source: r#"# plain
$Name = "hi"
$Literal = 'single'
$Here = @"
here $Name
"@
$LiteralHere = @'
literal here
'@
$Interpolated = "value $(Write-Output 'nested')"
"#,
            expected_kinds: &[Comment, StringLiteral],
        },
        SourceRegionFixture {
            language: "gdscript",
            file_path: "src/main.gd",
            source: r#"# plain
## doc
var name = "hi"
"#,
            expected_kinds: &[Comment, DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "razor",
            file_path: "Pages/Index.razor",
            source: r#"@* doc *@
<div title="markup">Hello</div>
@code {
    /// doc
    string Name = "hi";
    string Rendered = $"value { /* interpolation expression comment */ Name}";
}
"#,
            expected_kinds: &[DocComment, StringLiteral, Embedded],
        },
        SourceRegionFixture {
            language: "erlang",
            file_path: "src/main.erl",
            source: "% plain\n%% doc\n-module(main).\n-define(NAME, \"hi\").\n",
            expected_kinds: &[Comment, DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "fsharp",
            file_path: "src/Program.fs",
            source: r#"// plain
/// doc
module Worker
let message = "hi"
"#,
            expected_kinds: &[Comment, DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "sql",
            file_path: "schema.sql",
            source: r#"-- doc
/* doc */
SELECT 'hi' AS name;
"#,
            expected_kinds: &[DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "markdown",
            file_path: "README.md",
            source: r#"<!-- plain -->
# Title

```rust
fn main() {}
```
"#,
            expected_kinds: &[Comment, Embedded],
        },
        SourceRegionFixture {
            language: "json",
            file_path: "config.jsonc",
            source: r#"// plain
{"name":"hi"}
"#,
            expected_kinds: &[Comment, StringLiteral],
        },
        SourceRegionFixture {
            language: "toml",
            file_path: "config.toml",
            source: r#"# plain

# The display name.
name = "hi"
"#,
            expected_kinds: &[Comment, DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "yaml",
            file_path: "config.yaml",
            source: r#"# plain

# The display name.
name: "hi"
"#,
            expected_kinds: &[Comment, DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "xml",
            file_path: "config.xml",
            source: r#"<!-- plain -->

<!-- The configuration root. -->
<config name="hi"/>
"#,
            expected_kinds: &[Comment, DocComment, StringLiteral],
        },
    ];

    let fixture_languages = fixtures
        .iter()
        .map(|fixture| fixture.language)
        .collect::<BTreeSet<_>>();
    let domain_limited_languages = ["gosum", "qmldir", "regex"]
        .into_iter()
        .collect::<BTreeSet<_>>();
    let missing_languages = crate::language::supported_languages()
        .iter()
        .copied()
        .filter(|language| {
            !fixture_languages.contains(language) && !domain_limited_languages.contains(language)
        })
        .collect::<Vec<_>>();
    assert!(
        missing_languages.is_empty(),
        "source-region fixtures must cover every supported language with comment, string, or embedded syntax; missing {missing_languages:?}"
    );

    let exact_regions = [
        (
            "rust",
            &[
                ("comment", "// \"not a string\""),
                ("doc_comment", "/// doc\n"),
                ("string_literal", "\"/* not a comment */\""),
            ][..],
        ),
        (
            "c",
            &[
                ("comment", "// \"not a string\""),
                ("doc_comment", "/** doc */"),
                ("string_literal", "\"/* not a comment */\""),
            ][..],
        ),
        (
            "cpp",
            &[
                ("comment", "// \"not a string\""),
                ("doc_comment", "/** doc */"),
                ("string_literal", "\"/* not a comment */\""),
            ][..],
        ),
        (
            "go",
            &[
                ("doc_comment", "// \"not a string\""),
                ("string_literal", "\"// not a comment\""),
            ][..],
        ),
        (
            "html",
            &[
                ("doc_comment", "<!-- doc -->"),
                ("string_literal", "\"hi\""),
                (
                    "embedded",
                    "<script>const name = \"hi\";</script>",
                ),
                ("string_literal", "\"hi\""),
                ("embedded", "<style>body { color: red; }</style>"),
            ][..],
        ),
        (
            "css",
            &[
                ("doc_comment", "/* doc */"),
                ("string_literal", "\"hi\""),
            ][..],
        ),
        (
            "zig",
            &[
                ("comment", "// \"not a string\""),
                ("doc_comment", "/// doc"),
                ("string_literal", "\"/* not a comment */\""),
            ][..],
        ),
        (
            "swift",
            &[
                ("comment", "// \"not a string\""),
                ("doc_comment", "/// doc"),
                ("string_literal", "\"/* not a comment */\""),
                (
                    "string_literal",
                    "\"before \\(/* interpolation block */ format(\"nested // not a comment\")) after\"",
                ),
                ("comment", "/* interpolation block */"),
                ("string_literal", "\"nested // not a comment\""),
                (
                    "string_literal",
                    "#\"/* raw text */ \\#(/* raw interpolation block */ value)\"#",
                ),
                ("comment", "/* raw interpolation block */"),
                (
                    "string_literal",
                    "\"\"\"\n/* multiline text */\n\"\"\"",
                ),
            ][..],
        ),
        (
            "elixir",
            &[
                ("comment", "# plain"),
                ("string_literal", "\"hi\""),
            ][..],
        ),
        (
            "kotlin",
            &[
                ("comment", "// \"not a string\""),
                ("doc_comment", "/** doc */"),
                ("string_literal", "\"/* not a comment */\""),
            ][..],
        ),
        (
            "scala",
            &[
                ("comment", "// \"not a string\""),
                ("doc_comment", "/** block */"),
                ("string_literal", "\"/* not a comment */\""),
            ][..],
        ),
        (
            "java",
            &[
                ("comment", "// \"not a string\""),
                ("doc_comment", "/** doc */"),
                ("string_literal", "\"/* not a comment */\""),
            ][..],
        ),
        (
            "dart",
            &[
                ("comment", "// \"not a string\""),
                ("doc_comment", "/// doc"),
                ("string_literal", "\"/* not a comment */\""),
            ][..],
        ),
        (
            "csharp",
            &[
                ("comment", "// \"not a string\""),
                ("doc_comment", "/// <summary>doc</summary>"),
                ("string_literal", "\"/* not a comment */\""),
            ][..],
        ),
        (
            "vbnet",
            &[
                ("comment", "' \"not a string\""),
                ("doc_comment", "''' doc"),
                ("string_literal", "\"not ' a comment\""),
            ][..],
        ),
        (
            "gomod",
            &[
                ("doc_comment", "// The module."),
                ("string_literal", "\"example.com/app\""),
                ("comment", "// indirect"),
            ][..],
        ),
        (
            "erlang",
            &[
                ("comment", "% plain"),
                ("doc_comment", "%% doc"),
                ("string_literal", "\"hi\""),
            ][..],
        ),
        (
            "fsharp",
            &[
                ("comment", "// plain"),
                ("doc_comment", "/// doc"),
                ("string_literal", "\"hi\""),
            ][..],
        ),
        (
            "typescript",
            &[
                ("comment", "// \"not a string\""),
                ("doc_comment", "/** doc */"),
                ("string_literal", "\"/* not a comment */\""),
            ][..],
        ),
        (
            "tsx",
            &[
                ("comment", "// \"not a string\""),
                ("string_literal", "\"/* not a comment */\""),
            ][..],
        ),
        (
            "javascript",
            &[
                ("comment", "// \"not a string\""),
                ("doc_comment", "/** doc */"),
                ("string_literal", "\"/* not a comment */\""),
                (
                    "string_literal",
                    "`value ${/* template expression comment */ name}`",
                ),
                ("comment", "/* template expression comment */"),
            ][..],
        ),
        (
            "jsx",
            &[
                ("comment", "// \"not a string\""),
                ("string_literal", "\"/* not a comment */\""),
            ][..],
        ),
        (
            "vue",
            &[
                ("doc_comment", "<!-- doc -->"),
                ("string_literal", "\"/* not a comment */\""),
                ("embedded", "<script>const name = \"// not a comment\";</script>"),
                ("string_literal", "\"// not a comment\""),
                ("embedded", "<style>button { color: red; }</style>"),
            ][..],
        ),
        (
            "python",
            &[
                ("comment", "# \"not a string\""),
                ("string_literal", "\"not # a comment\""),
            ][..],
        ),
        (
            "php",
            &[
                ("comment", "// \"not a string\""),
                ("doc_comment", "/** doc */"),
                ("string_literal", "\"/* not a comment */\""),
            ][..],
        ),
        (
            "ruby",
            &[
                ("doc_comment", "# \"not a string\""),
                ("string_literal", "\"not # a comment\""),
            ][..],
        ),
        (
            "lua",
            &[
                ("doc_comment", "--- \"not a string\""),
                ("doc_comment", "--- doc"),
                ("string_literal", "\"-- not a comment\""),
            ][..],
        ),
        (
            "r",
            &[("doc_comment", "#' doc"), ("string_literal", "\"hi\"")][..],
        ),
        (
            "bash",
            &[("doc_comment", "# doc"), ("string_literal", "\"hi\"")][..],
        ),
        (
            "powershell",
            &[
                ("comment", "# plain"),
                ("string_literal", "\"hi\""),
                ("string_literal", "'single'"),
                ("string_literal", "@\"\nhere $Name\n\"@"),
                ("string_literal", "@'\nliteral here\n'@"),
                ("string_literal", "\"value $(Write-Output 'nested')\""),
                ("string_literal", "'nested'"),
            ][..],
        ),
        (
            "gdscript",
            &[
                ("comment", "# plain"),
                ("doc_comment", "## doc"),
                ("string_literal", "\"hi\""),
            ][..],
        ),
        (
            "sql",
            &[
                ("doc_comment", "-- doc"),
                ("doc_comment", "/* doc */"),
                ("string_literal", "'hi'"),
            ][..],
        ),
        (
            "markdown",
            &[
                ("comment", "<!-- plain -->\n"),
                ("embedded", "```rust\nfn main() {}\n```\n"),
            ][..],
        ),
        (
            "json",
            &[
                ("comment", "// plain"),
                ("string_literal", "\"name\""),
                ("string_literal", "\"hi\""),
            ][..],
        ),
        (
            "toml",
            &[
                ("comment", "# plain"),
                ("doc_comment", "# The display name."),
                ("string_literal", "\"hi\""),
            ][..],
        ),
        (
            "yaml",
            &[
                ("comment", "# plain"),
                ("doc_comment", "# The display name."),
                ("string_literal", "\"hi\""),
            ][..],
        ),
        (
            "xml",
            &[
                ("comment", "<!-- plain -->"),
                ("doc_comment", "<!-- The configuration root. -->"),
                ("string_literal", "\"hi\""),
            ][..],
        ),
        (
            "qml",
            &[
                ("comment", "// \"not a string\""),
                ("string_literal", "\"// not a comment\""),
            ][..],
        ),
        (
            "razor",
            &[
                ("doc_comment", "@* doc *@"),
                (
                    "embedded",
                    "@code {\n    /// doc\n    string Name = \"hi\";\n    string Rendered = $\"value { /* interpolation expression comment */ Name}\";\n}",
                ),
                ("doc_comment", "/// doc"),
                ("string_literal", "\"hi\""),
                (
                    "string_literal",
                    "$\"value { /* interpolation expression comment */ Name}\"",
                ),
                ("comment", "/* interpolation expression comment */"),
            ][..],
        ),
    ]
    .into_iter()
    .collect::<std::collections::BTreeMap<_, _>>();

    for fixture in fixtures {
        let results = extract(fixture.file_path, fixture.source);
        for expected_kind in fixture.expected_kinds {
            assert!(
                results
                    .source_regions
                    .iter()
                    .any(|region| region.kind == *expected_kind),
                "expected {} source region kind {:?} for {}, got {:?}",
                fixture.language,
                expected_kind,
                fixture.file_path,
                results.source_regions
            );
        }
        if let Some(expected_regions) = exact_regions.get(fixture.language) {
            let actual_regions = results
                .source_regions
                .iter()
                .map(|region| {
                    (
                        region.kind.as_str(),
                        fixture
                            .source
                            .get(region.start_byte as usize..region.end_byte as usize)
                            .expect("source region span must select valid source text"),
                    )
                })
                .collect::<Vec<_>>();
            assert_eq!(actual_regions, *expected_regions, "{}", fixture.language);
            let mut text_occurrences = HashMap::new();
            for (region, (_, text)) in results.source_regions.iter().zip(*expected_regions) {
                let occurrence = text_occurrences.entry(text).or_insert(0);
                let start = fixture
                    .source
                    .match_indices(text)
                    .nth(*occurrence)
                    .map(|(start, _)| start)
                    .expect("expected source region text must appear in fixture");
                *occurrence += 1;
                assert_eq!(
                    region.start_byte as usize, start,
                    "{}: {text}",
                    fixture.language
                );
                assert_eq!(
                    region.end_byte as usize,
                    start + text.len(),
                    "{}: {text}",
                    fixture.language
                );
            }
        }
    }
}

#[test]
fn rust_source_regions_capture_comments_doc_comments_and_string_literals() {
    let source = r#"// regular module comment
/// Explains greet.
pub fn greet() {
    let name = "Murphy";
    println!("{}", name);
}
"#;

    let results = extract("src/lib.rs", source);
    let greet = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "greet")
        .expect("expected greet symbol");

    let comment = results
        .source_regions
        .iter()
        .find(|region| region.kind == SourceRegionKind::Comment)
        .expect("expected regular comment source region");
    assert_eq!(comment.start_line, 1);
    assert_eq!(comment.start_byte, 0);

    let doc_comment = results
        .source_regions
        .iter()
        .find(|region| region.kind == SourceRegionKind::DocComment)
        .expect("expected doc comment source region");
    assert_eq!(
        doc_comment.containing_symbol_id.as_deref(),
        Some(greet.id.as_str())
    );

    let string_literal = results
        .source_regions
        .iter()
        .find(|region| region.kind == SourceRegionKind::StringLiteral)
        .expect("expected string literal source region");
    let literal_owner = results
        .symbols
        .iter()
        .find(|symbol| Some(symbol.id.as_str()) == string_literal.containing_symbol_id.as_deref())
        .expect("string literal containing symbol must exist");
    assert_eq!(literal_owner.name, "name");
    assert_eq!(literal_owner.parent_id.as_deref(), Some(greet.id.as_str()));
    assert!(string_literal.end_byte > string_literal.start_byte);
}

#[test]
fn vue_source_regions_capture_embedded_script_and_style_blocks() {
    let source = r#"<template>
  <button>{{ count }}</button>
</template>
<script>
export default {
  data() {
    return { count: 0 }
  }
}
</script>
<style>
button { color: red; }
</style>
"#;

    let results = extract("src/App.vue", source);
    let embedded = results
        .source_regions
        .iter()
        .filter(|region| region.kind == SourceRegionKind::Embedded)
        .collect::<Vec<_>>();

    assert!(
        embedded.len() >= 2,
        "expected script and style embedded regions, got {embedded:?}"
    );
    assert!(embedded.iter().any(|region| {
        region
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("embedded_language"))
            .and_then(|value| value.as_str())
            == Some("javascript")
    }));
    assert!(embedded.iter().any(|region| {
        region
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("embedded_language"))
            .and_then(|value| value.as_str())
            == Some("css")
    }));
}

#[test]
fn html_embedded_script_region_metadata_includes_host_tag_and_attributes() {
    let source = r#"<html><body>
<script TYPE="application/ld+json" SRC="/data.json">{"name":"worker"}</script>
</body></html>"#;

    let results = extract("index.html", source);
    let script = results
        .source_regions
        .iter()
        .find(|region| {
            region.kind == SourceRegionKind::Embedded
                && region
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("host_node_kind"))
                    .and_then(|value| value.as_str())
                    == Some("script_element")
        })
        .expect("expected embedded script region");

    let metadata = script.metadata.as_ref().expect("metadata");
    assert_eq!(
        metadata.get("host_tag").and_then(|value| value.as_str()),
        Some("script")
    );
    assert_eq!(
        metadata
            .get("embedded_language")
            .and_then(|value| value.as_str()),
        Some("json")
    );
    assert_eq!(
        metadata.get("type").and_then(|value| value.as_str()),
        Some("application/ld+json")
    );
    assert_eq!(
        metadata.get("src").and_then(|value| value.as_str()),
        Some("/data.json")
    );
}

#[test]
fn vue_embedded_regions_include_host_tag_and_script_lang() {
    let source = r#"<template><button title="hi">Run</button></template>
<script setup lang="ts">
const title = "Worker";
</script>
<style scoped lang="scss">
button { color: red; }
</style>
"#;

    let results = extract("src/App.vue", source);
    let embedded = results
        .source_regions
        .iter()
        .filter(|region| region.kind == SourceRegionKind::Embedded)
        .collect::<Vec<_>>();

    let script = embedded
        .iter()
        .find(|region| {
            region
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("host_node_kind"))
                .and_then(|value| value.as_str())
                == Some("script_element")
        })
        .expect("expected embedded script region");
    let script_metadata = script.metadata.as_ref().expect("script metadata");
    assert_eq!(
        script_metadata
            .get("host_tag")
            .and_then(|value| value.as_str()),
        Some("script")
    );
    assert_eq!(
        script_metadata
            .get("embedded_language")
            .and_then(|value| value.as_str()),
        Some("typescript")
    );
    assert_eq!(
        script_metadata
            .get("setup")
            .and_then(|value| value.as_bool()),
        Some(true)
    );
    assert_eq!(
        script_metadata.get("lang").and_then(|value| value.as_str()),
        Some("ts")
    );

    let style = embedded
        .iter()
        .find(|region| {
            region
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("host_node_kind"))
                .and_then(|value| value.as_str())
                == Some("style_element")
        })
        .expect("expected embedded style region");
    let style_metadata = style.metadata.as_ref().expect("style metadata");
    assert_eq!(
        style_metadata
            .get("host_tag")
            .and_then(|value| value.as_str()),
        Some("style")
    );
    assert_eq!(
        style_metadata
            .get("embedded_language")
            .and_then(|value| value.as_str()),
        Some("scss")
    );
    assert_eq!(
        style_metadata.get("lang").and_then(|value| value.as_str()),
        Some("scss")
    );
    assert_eq!(
        style_metadata
            .get("scoped")
            .and_then(|value| value.as_bool()),
        Some(true)
    );
}

#[test]
fn markdown_embedded_code_block_metadata_includes_info_string() {
    let source = r#"## Usage

```rust
fn main() {}
```
"#;

    let results = extract("README.md", source);
    let embedded = results
        .source_regions
        .iter()
        .find(|region| region.kind == SourceRegionKind::Embedded)
        .expect("expected fenced code embedded region");
    let metadata = embedded.metadata.as_ref().expect("metadata");
    assert_eq!(
        metadata
            .get("host_node_kind")
            .and_then(|value| value.as_str()),
        Some("fenced_code_block")
    );
    assert_eq!(
        metadata
            .get("embedded_language")
            .and_then(|value| value.as_str()),
        Some("rust")
    );
    assert_eq!(
        metadata.get("info_string").and_then(|value| value.as_str()),
        Some("rust")
    );
}

#[test]
fn razor_embedded_block_metadata_includes_block_type() {
    let source = r#"@page "/worker"

@code {
    public string Title { get; set; } = "Worker";
}
"#;

    let results = extract("Pages/Worker.razor", source);
    let embedded = results
        .source_regions
        .iter()
        .find(|region| region.kind == SourceRegionKind::Embedded)
        .expect("expected razor embedded block");
    let metadata = embedded.metadata.as_ref().expect("metadata");
    assert_eq!(
        metadata
            .get("host_node_kind")
            .and_then(|value| value.as_str()),
        Some("razor_block")
    );
    assert_eq!(
        metadata
            .get("embedded_language")
            .and_then(|value| value.as_str()),
        Some("csharp")
    );
    assert_eq!(
        metadata.get("block_type").and_then(|value| value.as_str()),
        Some("code")
    );
}
