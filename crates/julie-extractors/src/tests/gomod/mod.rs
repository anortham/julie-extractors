use std::path::Path;

use crate::base::{
    ExtractionResults, RelationshipKind, SourceRegionKind, StructuralFact, Symbol, SymbolKind,
};
use crate::extract_canonical;
use crate::tests::helpers::{facts_with_pattern, metadata_str};

fn extract(source: &str) -> ExtractionResults {
    extract_canonical("/repo/app/go.mod", source, Path::new("/repo")).unwrap()
}

fn symbol<'a>(results: &'a ExtractionResults, name: &str) -> &'a Symbol {
    results
        .symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap_or_else(|| panic!("missing symbol {name}: {:#?}", results.symbols))
}

fn fact<'a>(results: &'a ExtractionResults, pattern_id: &str) -> &'a StructuralFact {
    let facts = facts_with_pattern(results, pattern_id);
    assert_eq!(facts.len(), 1, "{pattern_id}: {facts:#?}");
    facts[0]
}

fn metadata_bool(fact: &StructuralFact, key: &str) -> Option<bool> {
    fact.metadata.as_ref()?.get(key)?.as_bool()
}

#[test]
fn go_mod_basename_selects_gomod_in_any_case() {
    for path in ["go.mod", "sub/go.mod", "sub/GO.MOD"] {
        assert_eq!(
            crate::language::detect_language_for_path(Path::new(path), ""),
            Some("gomod"),
            "{path}"
        );
    }
    for path in ["go.sum", "go.work", "vendor/modules.txt", "x.mod"] {
        assert_ne!(
            crate::language::detect_language_for_path(Path::new(path), ""),
            Some("gomod"),
            "{path}"
        );
    }
}

#[test]
fn declaring_lines_become_symbols_with_tight_spans_and_body_spans() {
    let source = "module example.com/app\n\ngo 1.22\n\ntoolchain go1.22.4\n\nrequire (\n\tgithub.com/a/b v1.2.3 // indirect\n)\n\ntool golang.org/x/tools/cmd/stringer\n";
    let results = extract(source);

    let kinds: Vec<(&str, SymbolKind)> = results
        .symbols
        .iter()
        .map(|symbol| (symbol.name.as_str(), symbol.kind.clone()))
        .collect();
    assert_eq!(
        kinds,
        vec![
            ("example.com/app", SymbolKind::Module),
            ("go", SymbolKind::Property),
            ("toolchain", SymbolKind::Property),
            ("github.com/a/b", SymbolKind::Import),
            ("golang.org/x/tools/cmd/stringer", SymbolKind::Import),
        ]
    );

    let required = symbol(&results, "github.com/a/b");
    assert_eq!((required.start_line, required.start_column), (8, 1));
    assert_eq!((required.end_line, required.end_column), (8, 22));
    assert_eq!(
        required.signature.as_deref(),
        Some("require github.com/a/b v1.2.3 // indirect")
    );
    let body = required.body_span.expect("require body span");
    assert_eq!(
        &source[body.start_byte as usize..body.end_byte as usize],
        "v1.2.3"
    );

    let go = symbol(&results, "go");
    let body = go.body_span.expect("go body span");
    assert_eq!(
        &source[body.start_byte as usize..body.end_byte as usize],
        "1.22"
    );
}

#[test]
fn body_hash_follows_the_required_version_and_ignores_comments() {
    let hash = |source: &str| {
        symbol(&extract(source), "github.com/a/b")
            .body_hash
            .clone()
            .expect("body hash")
    };
    let base = hash("module m.com/x\n\nrequire github.com/a/b v1.0.0\n");
    assert_eq!(
        base,
        hash("module m.com/x\n\n// A note.\nrequire github.com/a/b v1.0.0 // indirect\n")
    );
    assert_ne!(
        base,
        hash("module m.com/x\n\nrequire github.com/a/b v1.0.1\n")
    );
}

#[test]
fn leading_comments_are_doc_comments_and_suffix_comments_are_not() {
    let source = "// The service module.\nmodule example.com/app\n\n// Detached note.\n\nrequire (\n\t// Flags.\n\tgithub.com/spf13/pflag v1.0.5 // indirect\n\tgithub.com/spf13/cobra v1.8.0\n)\n";
    let results = extract(source);

    assert_eq!(
        symbol(&results, "example.com/app").doc_comment.as_deref(),
        Some("// The service module.")
    );
    assert_eq!(
        symbol(&results, "github.com/spf13/pflag")
            .doc_comment
            .as_deref(),
        Some("// Flags.")
    );
    assert_eq!(symbol(&results, "github.com/spf13/cobra").doc_comment, None);

    let region_kinds: Vec<(u32, SourceRegionKind)> = results
        .source_regions
        .iter()
        .map(|region| (region.start_line, region.kind.clone()))
        .collect();
    assert_eq!(
        region_kinds,
        vec![
            (1, SourceRegionKind::DocComment),
            (4, SourceRegionKind::Comment),
            (7, SourceRegionKind::DocComment),
            (8, SourceRegionKind::Comment),
        ]
    );
    assert_eq!(
        results.source_regions[2].containing_symbol_id.as_deref(),
        Some(symbol(&results, "github.com/spf13/pflag").id.as_str())
    );
}

#[test]
fn require_lines_are_go_dependency_facts_with_the_indirect_marker() {
    let source = "module example.com/app\n\nrequire (\n\tgithub.com/a/direct v1.0.0\n\tgithub.com/a/indirect v1.1.0 // indirect\n\tgithub.com/a/reasoned v1.2.0 // indirect; pulled in by x\n\tgithub.com/a/other v1.3.0 // indirectly used\n)\n";
    let results = extract(source);

    let facts = facts_with_pattern(&results, "manifest.dependency.v1");
    let rows: Vec<(&str, &str, bool)> = facts
        .iter()
        .map(|fact| {
            assert_eq!(metadata_str(fact, "ecosystem"), Some("go"));
            assert_eq!(metadata_str(fact, "group"), Some("require"));
            (
                metadata_str(fact, "name").unwrap(),
                metadata_str(fact, "version").unwrap(),
                metadata_bool(fact, "indirect").unwrap(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        vec![
            ("github.com/a/direct", "v1.0.0", false),
            ("github.com/a/indirect", "v1.1.0", true),
            ("github.com/a/reasoned", "v1.2.0", true),
            ("github.com/a/other", "v1.3.0", false),
        ]
    );
    assert_eq!(
        facts[0].containing_symbol_id.as_deref(),
        Some(symbol(&results, "github.com/a/direct").id.as_str())
    );
}

#[test]
fn required_modules_and_tools_are_imports_edges_from_the_module() {
    let source = "module example.com/app\n\nrequire github.com/a/b v1.0.0 // indirect\n\ntool (\n\texample.com/app/cmd/gen\n)\n\nexclude github.com/a/b v0.9.0\n";
    let results = extract(source);
    let module = symbol(&results, "example.com/app");

    let edges: Vec<(&str, &str, Option<bool>)> = results
        .relationships
        .iter()
        .map(|edge| {
            assert_eq!(edge.kind, RelationshipKind::Imports);
            assert_eq!(edge.from_symbol_id, module.id);
            let metadata = edge.metadata.as_ref().unwrap();
            (
                metadata["dependencyName"].as_str().unwrap(),
                metadata["dependencyKind"].as_str().unwrap(),
                metadata.get("indirect").and_then(|value| value.as_bool()),
            )
        })
        .collect();
    assert_eq!(
        edges,
        vec![
            ("github.com/a/b", "require", Some(true)),
            ("example.com/app/cmd/gen", "tool", None),
        ]
    );
}

#[test]
fn a_manifest_without_a_module_line_publishes_no_edges() {
    let results =
        extract("go 1.22\n\nrequire github.com/a/b v1.0.0\n\nreplace github.com/a/b => ../b\n");

    assert!(results.relationships.is_empty());
    assert!(results.structured_pending_relationships.is_empty());
    assert_eq!(
        facts_with_pattern(&results, "manifest.dependency.v1").len(),
        1
    );
}

#[test]
fn file_path_replacements_are_pending_imports_of_the_replacement_manifest() {
    let source = "module example.com/app\n\nreplace (\n\texample.com/lib => ../lib/\n\texample.com/win v1.0.0 => ..\\win\n\texample.com/old v1.0.0 => example.com/new v1.1.0\n)\n";
    let results = extract(source);
    let module = symbol(&results, "example.com/app");

    let pending: Vec<(&str, &str, Option<&str>, u32)> = results
        .structured_pending_relationships
        .iter()
        .map(|pending| {
            assert_eq!(pending.pending.kind, RelationshipKind::Imports);
            assert_eq!(pending.pending.from_symbol_id, module.id);
            assert_eq!(
                pending.caller_scope_symbol_id.as_deref(),
                Some(module.id.as_str())
            );
            (
                pending.target.display_name.as_str(),
                pending.target.terminal_name.as_str(),
                pending.target.import_context.as_deref(),
                pending.pending.line_number,
            )
        })
        .collect();
    assert_eq!(
        pending,
        vec![
            ("../lib/go.mod", "go.mod", Some("../lib/go.mod"), 4),
            ("../win/go.mod", "go.mod", Some("../win/go.mod"), 5),
        ]
    );

    let replacements: Vec<String> = facts_with_pattern(&results, "gomod.replace.v1")
        .iter()
        .map(|fact| {
            format!(
                "{} {} => {} {} local={}",
                metadata_str(fact, "module_path").unwrap(),
                metadata_str(fact, "version").unwrap_or("-"),
                metadata_str(fact, "replacement").unwrap(),
                metadata_str(fact, "replacement_version").unwrap_or("-"),
                metadata_bool(fact, "local").unwrap(),
            )
        })
        .collect();
    assert_eq!(
        replacements,
        vec![
            "example.com/lib - => ../lib/ - local=true",
            "example.com/win v1.0.0 => ..\\win - local=true",
            "example.com/old v1.0.0 => example.com/new v1.1.0 local=false",
        ]
    );
}

#[test]
fn settings_exclusions_tools_and_ignores_are_facts() {
    let source = "module example.com/app\n\ngo 1.24.0\n\ntoolchain default\n\nexclude example.com/bad v1.2.0\n\ntool example.com/app/cmd/gen\n\nignore (\n\t./node_modules\n\tstatic\n)\n";
    let results = extract(source);

    assert_eq!(
        metadata_str(fact(&results, "gomod.module.v1"), "module_path"),
        Some("example.com/app")
    );
    assert_eq!(
        metadata_str(fact(&results, "gomod.go.v1"), "version"),
        Some("1.24.0")
    );
    assert_eq!(
        metadata_str(fact(&results, "gomod.toolchain.v1"), "toolchain"),
        Some("default")
    );
    let exclude = fact(&results, "gomod.exclude.v1");
    assert_eq!(
        metadata_str(exclude, "module_path"),
        Some("example.com/bad")
    );
    assert_eq!(metadata_str(exclude, "version"), Some("v1.2.0"));
    assert_eq!(
        metadata_str(fact(&results, "gomod.tool.v1"), "package_path"),
        Some("example.com/app/cmd/gen")
    );
    let ignored: Vec<&str> = facts_with_pattern(&results, "gomod.ignore.v1")
        .iter()
        .map(|fact| metadata_str(fact, "path").unwrap())
        .collect();
    assert_eq!(ignored, vec!["./node_modules", "static"]);
}

#[test]
fn retractions_carry_interval_and_rationale_with_block_fallback() {
    let source = "module example.com/app\n\n// Crashes on start.\nretract v1.0.5\n\n// Wrong branch.\nretract (\n\tv1.0.0 // Published accidentally.\n\t[v1.0.1, v1.0.4]\n)\n\nretract v0.1.0\n";
    let results = extract(source);

    let rows: Vec<(&str, &str, bool, Option<&str>)> =
        facts_with_pattern(&results, "gomod.retract.v1")
            .iter()
            .map(|fact| {
                (
                    metadata_str(fact, "low").unwrap(),
                    metadata_str(fact, "high").unwrap(),
                    metadata_bool(fact, "range").unwrap(),
                    metadata_str(fact, "rationale"),
                )
            })
            .collect();
    assert_eq!(
        rows,
        vec![
            ("v1.0.5", "v1.0.5", false, Some("Crashes on start.")),
            ("v1.0.0", "v1.0.0", false, Some("Published accidentally.")),
            ("v1.0.1", "v1.0.4", true, Some("Wrong branch.")),
            ("v0.1.0", "v0.1.0", false, None),
        ]
    );
}

#[test]
fn module_deprecation_reads_the_deprecated_paragraph_of_leading_or_suffix_comments() {
    let leading = extract(
        "// Package app does things.\n//\n// Deprecated: use example.com/app/v2.\nmodule example.com/app\n",
    );
    assert_eq!(
        metadata_str(fact(&leading, "gomod.module.v1"), "deprecated"),
        Some("use example.com/app/v2.")
    );
    assert_eq!(
        symbol(&leading, "example.com/app")
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("deprecated"))
            .and_then(|value| value.as_str()),
        Some("use example.com/app/v2.")
    );

    let suffix = extract("module example.com/app // Deprecated: moved\n");
    assert_eq!(
        metadata_str(fact(&suffix, "gomod.module.v1"), "deprecated"),
        Some("moved")
    );

    let mentioned = extract("// This is not Deprecated: really.\nmodule example.com/app\n");
    assert_eq!(
        metadata_str(fact(&mentioned, "gomod.module.v1"), "deprecated"),
        None
    );
}

#[test]
fn quoted_values_decode_every_go_escape() {
    let source = "module \"example.com/\\u0061\\x62\\143\\U00000064\"\n\nreplace example.com/lib => \"../l\\x69b\"\n";
    let results = extract(source);

    assert_eq!(
        metadata_str(fact(&results, "gomod.module.v1"), "module_path"),
        Some("example.com/abcd")
    );
    assert_eq!(
        results.structured_pending_relationships[0]
            .target
            .display_name,
        "../lib/go.mod"
    );
}

#[test]
fn a_quoted_value_with_an_invalid_escape_keeps_its_written_text() {
    let source = "module \"example.com/\\x+f\\400\"\n";
    let results = extract(source);

    assert_eq!(
        metadata_str(fact(&results, "gomod.module.v1"), "module_path"),
        Some("example.com/\\x+f\\400")
    );
}

#[test]
fn quoted_values_are_unquoted_names_literals_and_string_regions() {
    let source = "module \"gopkg.in/yaml.v3\"\n\nrequire (\n\t\"gopkg.in/check.v1\" v0.0.0-20161208181325-20d25e280405\n\t`example.com/raw` v1.0.0\n)\n";
    let results = extract(source);

    assert_eq!(
        symbol(&results, "gopkg.in/yaml.v3").kind,
        SymbolKind::Module
    );
    let raw = symbol(&results, "example.com/raw");
    assert_eq!(
        metadata_str(fact(&results, "gomod.module.v1"), "module_path"),
        Some("gopkg.in/yaml.v3")
    );

    let literals: Vec<(&str, Option<&str>)> = results
        .literals
        .iter()
        .map(|literal| (literal.literal_text.as_str(), literal.carrier.as_deref()))
        .collect();
    assert_eq!(
        literals,
        vec![
            ("gopkg.in/yaml.v3", Some("module.module_path")),
            ("gopkg.in/check.v1", Some("require.module_path")),
            ("example.com/raw", Some("require.module_path")),
        ]
    );

    let strings: Vec<_> = results
        .source_regions
        .iter()
        .filter(|region| region.kind == SourceRegionKind::StringLiteral)
        .collect();
    assert_eq!(strings.len(), 3);
    assert_eq!(
        strings[2].containing_symbol_id.as_deref(),
        Some(raw.id.as_str())
    );
    assert_eq!(
        &source[strings[2].start_byte as usize..strings[2].end_byte as usize],
        "`example.com/raw`"
    );
}

#[test]
fn godebug_settings_are_property_symbols_and_facts() {
    let source = "module example.com/app\n\ngodebug default=go1.21\n\ngodebug (\n\tpanicnil=1\n\t// Timers keep the old channel semantics.\n\tasynctimerchan=0\n)\n";
    let results = extract(source);

    assert!(
        results.parse_diagnostics.is_empty(),
        "{:#?}",
        results.parse_diagnostics
    );
    let settings: Vec<(&str, &str)> = facts_with_pattern(&results, "gomod.godebug.v1")
        .iter()
        .map(|fact| {
            (
                metadata_str(fact, "key").unwrap(),
                metadata_str(fact, "value").unwrap(),
            )
        })
        .collect();
    assert_eq!(
        settings,
        vec![
            ("default", "go1.21"),
            ("panicnil", "1"),
            ("asynctimerchan", "0")
        ]
    );

    let default = symbol(&results, "default");
    assert_eq!(default.kind, SymbolKind::Property);
    assert_eq!(default.signature.as_deref(), Some("godebug default=go1.21"));
    assert_eq!(
        &source[default.start_byte as usize..default.end_byte as usize],
        "godebug default=go1.21"
    );
    let timers = symbol(&results, "asynctimerchan");
    assert_eq!(
        timers.signature.as_deref(),
        Some("godebug asynctimerchan=0")
    );
    assert_eq!(
        timers.doc_comment.as_deref(),
        Some("// Timers keep the old channel semantics.")
    );
    assert_eq!(symbol(&results, "panicnil").doc_comment, None);
}

#[test]
fn absolute_replacements_and_one_character_paths_extract() {
    let results = extract(
        "module example.com/app\n\nreplace example.com/a => /src/a\n\nignore x\n\nrequire example.com/b v1.0.0\n",
    );

    assert!(
        results.parse_diagnostics.is_empty(),
        "{:#?}",
        results.parse_diagnostics
    );
    let replace = fact(&results, "gomod.replace.v1");
    assert_eq!(metadata_str(replace, "replacement"), Some("/src/a"));
    assert_eq!(metadata_bool(replace, "local"), Some(true));
    assert_eq!(
        results
            .structured_pending_relationships
            .iter()
            .map(|pending| pending.target.display_name.as_str())
            .collect::<Vec<_>>(),
        vec!["/src/a/go.mod"]
    );
    assert_eq!(
        metadata_str(fact(&results, "gomod.ignore.v1"), "path"),
        Some("x")
    );
    assert_eq!(
        metadata_str(fact(&results, "manifest.dependency.v1"), "name"),
        Some("example.com/b")
    );
}

#[test]
fn a_last_line_without_a_newline_extracts_without_diagnostics() {
    for (source, pattern_id, key, value) in [
        (
            "module example.com/app\n\ngo 1.22",
            "gomod.go.v1",
            "version",
            "1.22",
        ),
        (
            "module example.com/app\n\ngodebug panicnil=1",
            "gomod.godebug.v1",
            "value",
            "1",
        ),
        (
            "module example.com/app\n\nrequire (\n\texample.com/a v1.0.0\n)",
            "manifest.dependency.v1",
            "version",
            "v1.0.0",
        ),
    ] {
        let results = extract(source);

        assert!(
            results.parse_diagnostics.is_empty(),
            "{source:?}: {:#?}",
            results.parse_diagnostics
        );
        assert_eq!(
            metadata_str(fact(&results, pattern_id), key),
            Some(value),
            "{source:?}"
        );
    }
}

#[test]
fn crlf_line_endings_keep_comments_and_the_indirect_marker() {
    let results = extract(
        "// Service.\r\nmodule example.com/app\r\n\r\nrequire (\r\n\texample.com/a v1.0.0 // indirect\r\n)\r\n",
    );

    assert!(results.parse_diagnostics.is_empty());
    assert_eq!(
        symbol(&results, "example.com/app").doc_comment.as_deref(),
        Some("// Service.")
    );
    assert_eq!(
        metadata_bool(fact(&results, "manifest.dependency.v1"), "indirect"),
        Some(true)
    );
}

#[test]
fn every_comment_of_a_long_block_documents_the_directive_below() {
    let block_lines = 20_000;
    let source = format!(
        "// stray\n\n{}module example.com/app\n",
        "//\n".repeat(block_lines)
    );
    let results = extract(&source);

    let kinds: Vec<SourceRegionKind> = results
        .source_regions
        .iter()
        .map(|region| region.kind.clone())
        .collect();
    assert_eq!(kinds.len(), block_lines + 1);
    assert_eq!(kinds[0], SourceRegionKind::Comment);
    assert!(
        kinds[1..]
            .iter()
            .all(|kind| *kind == SourceRegionKind::DocComment)
    );
}

#[test]
fn a_long_block_rationale_is_cut_once_and_shared_by_every_retraction() {
    let rationale = "é".repeat(400);
    let source = format!(
        "module example.com/app\n\n// {rationale}\nretract (\n\tv1.0.0\n\tv1.0.1\n\t// own reason\n\tv1.0.2\n)\n"
    );
    let results = extract(&source);

    let facts = facts_with_pattern(&results, "gomod.retract.v1");
    let rationales: Vec<(Option<&str>, Option<bool>)> = facts
        .iter()
        .map(|fact| {
            (
                metadata_str(fact, "rationale"),
                metadata_bool(fact, "rationale_truncated"),
            )
        })
        .collect();
    let cut = "é".repeat(250);
    assert_eq!(
        rationales,
        vec![
            (Some(cut.as_str()), Some(true)),
            (Some(cut.as_str()), Some(true)),
            (Some("own reason"), None),
        ]
    );
}
