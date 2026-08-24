use std::collections::BTreeSet;
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
fn supported_languages_with_source_region_syntax_emit_regions() {
    use SourceRegionKind::{Comment, DocComment, Embedded, StringLiteral};

    let fixtures = [
        SourceRegionFixture {
            language: "rust",
            file_path: "src/lib.rs",
            source: r#"// plain
/// doc
pub fn greet() {
    let name = "hi";
}
"#,
            expected_kinds: &[Comment, DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "c",
            file_path: "src/main.c",
            source: r#"// plain
/** doc */
const char *name = "hi";
"#,
            expected_kinds: &[Comment, DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "cpp",
            file_path: "src/main.cpp",
            source: r#"// plain
/** doc */
std::string name = "hi";
"#,
            expected_kinds: &[Comment, DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "go",
            file_path: "main.go",
            source: r#"package main
// doc
func main() {
    name := "hi"
    _ = name
}
"#,
            expected_kinds: &[DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "zig",
            file_path: "src/main.zig",
            source: r#"// plain
/// doc
const name = "hi";
"#,
            expected_kinds: &[Comment, DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "typescript",
            file_path: "src/main.ts",
            source: r#"// plain
/** doc */
const name: string = "hi";
"#,
            expected_kinds: &[Comment, DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "tsx",
            file_path: "src/main.tsx",
            source: r#"// plain
const node = <div title="hi">text</div>;
"#,
            expected_kinds: &[Comment, StringLiteral],
        },
        SourceRegionFixture {
            language: "javascript",
            file_path: "src/main.js",
            source: r#"// plain
/** doc */
const name = "hi";
"#,
            expected_kinds: &[Comment, DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "jsx",
            file_path: "src/main.jsx",
            source: r#"// plain
const node = <div title="hi">text</div>;
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
<template><button title="hi">Count</button></template>
<script>const name = "hi";</script>
<style>button { color: red; }</style>
"#,
            expected_kinds: &[DocComment, StringLiteral, Embedded],
        },
        SourceRegionFixture {
            language: "python",
            file_path: "src/main.py",
            source: r#"# plain
name = "hi"
"#,
            expected_kinds: &[Comment, StringLiteral],
        },
        SourceRegionFixture {
            language: "java",
            file_path: "src/Main.java",
            source: r#"// plain
/** doc */
class Worker {
    String name = "hi";
}
"#,
            expected_kinds: &[Comment, DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "csharp",
            file_path: "src/Worker.cs",
            source: r#"// plain
/// <summary>doc</summary>
public class Worker {
    string Name = "hi";
}
"#,
            expected_kinds: &[Comment, DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "vbnet",
            file_path: "src/Worker.vb",
            source: r#"' plain
''' doc
Module Worker
    Dim Name As String = "hi"
End Module
"#,
            expected_kinds: &[Comment, DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "php",
            file_path: "src/main.php",
            source: r#"<?php
// plain
/** doc */
$name = "hi";
"#,
            expected_kinds: &[Comment, DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "ruby",
            file_path: "src/main.rb",
            source: r#"# doc
name = "hi"
"#,
            expected_kinds: &[DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "swift",
            file_path: "src/main.swift",
            source: r#"// plain
/// doc
let name = "hi"
"#,
            expected_kinds: &[Comment, DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "kotlin",
            file_path: "src/main.kt",
            source: r#"// plain
/** doc */
val name = "hi"
"#,
            expected_kinds: &[Comment, DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "scala",
            file_path: "src/Main.scala",
            source: r#"// plain
/** block */
object Worker { val name = "hi" }
"#,
            expected_kinds: &[Comment, StringLiteral],
        },
        SourceRegionFixture {
            language: "dart",
            file_path: "lib/main.dart",
            source: r#"// plain
/// doc
final name = "hi";
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
            source: r#"-- plain
--- doc
local name = "hi"
"#,
            expected_kinds: &[DocComment, StringLiteral],
        },
        SourceRegionFixture {
            language: "qml",
            file_path: "src/Main.qml",
            source: r#"// plain
Item { property string name: "hi" }
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
<div title="hi">Hello</div>
@code {
    /// doc
    string Name = "hi";
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
name = "hi"
"#,
            expected_kinds: &[Comment, StringLiteral],
        },
        SourceRegionFixture {
            language: "yaml",
            file_path: "config.yaml",
            source: r#"# plain
name: "hi"
"#,
            expected_kinds: &[Comment, StringLiteral],
        },
        SourceRegionFixture {
            language: "xml",
            file_path: "config.xml",
            source: r#"<!-- plain -->
<config name="hi"/>
"#,
            expected_kinds: &[Comment, StringLiteral],
        },
    ];

    let fixture_languages = fixtures
        .iter()
        .map(|fixture| fixture.language)
        .collect::<BTreeSet<_>>();
    let domain_limited_languages = ["qmldir", "regex"].into_iter().collect::<BTreeSet<_>>();
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
    assert_eq!(
        string_literal.containing_symbol_id.as_deref(),
        Some(greet.id.as_str())
    );
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
