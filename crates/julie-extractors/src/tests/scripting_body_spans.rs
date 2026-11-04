use crate::base::{NormalizedSpan, Symbol, SymbolKind};
use crate::tests::helpers::init_parser;
use std::path::PathBuf;

fn body_text<'a>(source: &'a str, symbol: &Symbol) -> Option<&'a str> {
    let span = symbol.body_span?;
    source.get(span.start_byte as usize..span.end_byte as usize)
}

fn symbol<'a>(symbols: &'a [Symbol], name: &str, kind: SymbolKind) -> &'a Symbol {
    symbols
        .iter()
        .find(|symbol| symbol.name == name && symbol.kind == kind)
        .unwrap_or_else(|| panic!("missing {kind:?} {name}"))
}

fn extract_elixir(source: &str) -> (Vec<Symbol>, crate::elixir::ElixirExtractor) {
    let tree = init_parser(source, "elixir");
    let mut extractor = crate::elixir::ElixirExtractor::new(
        "elixir".to_string(),
        "test.ex".to_string(),
        source.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    (symbols, extractor)
}

fn extract_gdscript(source: &str) -> Vec<Symbol> {
    let tree = init_parser(source, "gdscript");
    let mut extractor = crate::gdscript::GDScriptExtractor::new(
        "gdscript".to_string(),
        "test.gd".to_string(),
        source.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    extractor.extract_symbols(&tree)
}

fn assigned_call_arguments_span(source: &str, assignment_name: &str) -> Option<NormalizedSpan> {
    fn find(
        node: tree_sitter::Node,
        source: &str,
        assignment_name: &str,
    ) -> Option<NormalizedSpan> {
        if node.kind() == "binary_operator"
            && node
                .child_by_field_name("lhs")
                .and_then(|lhs| lhs.utf8_text(source.as_bytes()).ok())
                == Some(assignment_name)
            && let Some(call) = node.child_by_field_name("rhs")
        {
            return call
                .child_by_field_name("arguments")
                .map(|arguments| NormalizedSpan::from_node(&arguments));
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(span) = find(child, source, assignment_name) {
                return Some(span);
            }
        }
        None
    }

    find(
        init_parser(source, "r").root_node(),
        source,
        assignment_name,
    )
}

fn extract_r(source: &str) -> Vec<Symbol> {
    crate::tests::r::extract_symbols(source)
}

fn class_for_binding<'a>(symbols: &'a [Symbol], name: &str) -> &'a Symbol {
    symbols
        .iter()
        .find(|symbol| {
            symbol.kind == SymbolKind::Class
                && (symbol.name == name
                    || symbol
                        .signature
                        .as_deref()
                        .is_some_and(|signature| signature.starts_with(&format!("{name} <-"))))
        })
        .unwrap_or_else(|| panic!("missing class for binding {name}"))
}

fn normalize_line_endings(source: String) -> String {
    source.replace("\r\n", "\n").replace('\r', "\n")
}

fn assert_fixture_body_fields(
    expected_json: &str,
    symbols: &[Symbol],
    include: impl Fn(&Symbol) -> bool,
) {
    let expected: serde_json::Value = serde_json::from_str(expected_json).expect("expected JSON");
    let rows = expected["symbols"].as_array().expect("expected symbols");

    for symbol in symbols.iter().filter(|symbol| include(symbol)) {
        let kind = serde_json::to_value(&symbol.kind).expect("symbol kind");
        let row = rows
            .iter()
            .find(|row| {
                row["name"] == symbol.name
                    && row["kind"] == kind
                    && row["start_line"] == symbol.start_line
                    && row["start_column"] == symbol.start_column
            })
            .unwrap_or_else(|| panic!("missing golden row for {}:{:?}", symbol.name, symbol.kind));
        assert_eq!(
            row["body_span"],
            serde_json::to_value(symbol.body_span).expect("body span"),
            "body span golden mismatch for {}:{:?}",
            symbol.name,
            symbol.kind
        );
        assert_eq!(
            row["body_hash"],
            serde_json::to_value(&symbol.body_hash).expect("body hash"),
            "body hash golden mismatch for {}:{:?}",
            symbol.name,
            symbol.kind
        );
    }
}

#[test]
fn elixir_pattern_bindings_do_not_own_their_match_pattern() {
    let source = "defmodule App do\n  def bind(%Worker{} = w, n), do: {w, n}\nend\n";
    let (symbols, extractor) = extract_elixir(source);
    let parameter = symbols
        .iter()
        .find(|symbol| {
            symbol.name == "w"
                && symbol.kind == SymbolKind::Variable
                && symbol
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("role"))
                    .and_then(serde_json::Value::as_str)
                    == Some("parameter")
        })
        .expect("missing `w` parameter");

    assert_eq!(parameter.body_span, None);
    assert_eq!(parameter.body_hash, None);
    assert_eq!(
        extractor.base.type_info[&parameter.id].resolved_type,
        "Worker"
    );
    assert_eq!(
        body_text(source, symbol(&symbols, "bind", SymbolKind::Function)),
        Some("{w, n}")
    );
}

#[test]
fn gdscript_signal_parameters_do_not_own_event_bodies() {
    let source = "signal activated(value)\nsignal _health_changed(value: int)\nfunc refresh(value):\n    return value\n";
    let symbols = extract_gdscript(source);

    for name in ["activated", "_health_changed"] {
        let event = symbol(&symbols, name, SymbolKind::Event);
        assert_eq!(event.body_span, None);
        assert_eq!(event.body_hash, None);
    }

    assert_eq!(
        body_text(source, symbol(&symbols, "refresh", SymbolKind::Function)),
        Some("\n    return value")
    );
}

#[test]
fn r_imports_and_value_assignments_do_not_own_call_arguments() {
    let source = "library(dplyr)\ntotal <- switch(mode, one = 1, 0)\ncache <- list()\ncompute <- function(value) value + 1\n";
    let symbols = extract_r(source);

    let import = symbol(&symbols, "dplyr", SymbolKind::Import);
    assert_eq!(import.body_span, None);
    assert_eq!(import.body_hash, None);
    for name in ["total", "cache"] {
        let variable = symbol(&symbols, name, SymbolKind::Variable);
        assert_eq!(variable.body_span, None);
        assert_eq!(variable.body_hash, None);
    }
    assert_eq!(
        body_text(source, symbol(&symbols, "compute", SymbolKind::Function)),
        Some("value + 1")
    );
}

#[test]
fn r_class_factories_own_their_complete_argument_nodes() {
    let source = r#"empty <- R6::R6Class("Empty")
fields <- setRefClass("Fields", fields = list(value = "numeric"))
s4 <- setClass("S4", representation(value = "numeric"))
s7 <- S7::new_class("S7", properties = list(value = S7::class_integer))
complex <- R6::R6Class("Complex", inherit = Base, public = list(
  first = function(x) { inner <- function() x; inner() },
  second = function(y) y
), active = list(total = function(value) self$total * 2))
"#;
    let symbols = extract_r(source);

    for (binding, name) in [
        ("empty", "empty"),
        ("fields", "fields"),
        ("s4", "S4"),
        ("s7", "s7"),
        ("complex", "complex"),
    ] {
        let class = symbols
            .iter()
            .find(|symbol| symbol.kind == SymbolKind::Class && symbol.name == name)
            .unwrap_or_else(|| panic!("missing class {name}"));
        assert_eq!(
            class.body_span,
            assigned_call_arguments_span(source, binding)
        );
        assert!(class.body_hash.is_some());
    }

    assert!(
        symbol(&symbols, "first", SymbolKind::Method)
            .body_span
            .is_some()
    );
    assert!(
        symbol(&symbols, "second", SymbolKind::Method)
            .body_span
            .is_some()
    );
    assert_eq!(
        body_text(source, symbol(&symbols, "total", SymbolKind::Property)),
        Some("self$total * 2")
    );
}

#[test]
fn r_class_argument_hashes_track_values_but_ignore_comments() {
    let source = "object <- R6::R6Class(\"Object\", inherit = Base, public = list(run = function() 1), private = list(cache = 2))\n";
    let changed_member = source.replace("cache = 2", "cache = 3");
    let changed_method = source.replace("function() 1", "function() 4");
    let commented = source.replace("inherit = Base, ", "inherit = Base, # stable comment\n  ");
    let reordered = source.replace(
        "inherit = Base, public = list(run = function() 1)",
        "public = list(run = function() 1), inherit = Base",
    );
    let hash = |source: &str| {
        class_for_binding(&extract_r(source), "object")
            .body_hash
            .clone()
            .expect("class factory argument hash")
    };

    assert_ne!(hash(source), hash(&changed_member));
    assert_ne!(hash(source), hash(&changed_method));
    assert_eq!(hash(source), hash(&commented));
    assert_ne!(hash(source), hash(&reordered));
}

#[test]
fn r_class_argument_spans_keep_unicode_and_crlf_byte_offsets() {
    let source = "# café class\nobject <- R6::R6Class(\"Objet\", public = list(label = function() \"café\"))\n";
    let crlf = source.replace('\n', "\r\n");
    let symbols = extract_r(&crlf);
    let class = class_for_binding(&symbols, "object");

    assert_eq!(
        class.body_span,
        assigned_call_arguments_span(&crlf, "object")
    );
    assert!(body_text(&crlf, class).is_some_and(|text| text.contains("café")));
    assert_eq!(
        class.body_hash,
        class_for_binding(&extract_r(source), "object").body_hash
    );
}

#[test]
fn fsharp_record_value_bindings_own_the_complete_right_hand_side() {
    let source =
        "type Profile = { Name: string; Age: int }\nlet profile = { Name = \"Ada\"; Age = 37 }\n";
    let tree = init_parser(source, "fsharp");
    let mut extractor = crate::fsharp::FSharpExtractor::new(
        "fsharp".to_string(),
        "test.fs".to_string(),
        source.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    let profile = symbol(&symbols, "profile", SymbolKind::Variable);

    assert_eq!(
        body_text(source, profile),
        Some("{ Name = \"Ada\"; Age = 37 }")
    );
    assert!(profile.body_hash.is_some());
}

#[test]
fn registered_scripting_body_goldens_match_extraction() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let elixir_source = include_str!("../../../../fixtures/extraction/elixir/basic/source.ex");
    let elixir_expected =
        include_str!("../../../../fixtures/extraction/elixir/basic/expected.json");
    let (elixir_symbols, _) = extract_elixir(elixir_source);
    assert_fixture_body_fields(elixir_expected, &elixir_symbols, |symbol| {
        symbol.kind == SymbolKind::Variable
            && symbol
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("role"))
                .and_then(serde_json::Value::as_str)
                == Some("parameter")
    });

    for fixture in ["basic", "godot", "structure"] {
        let source_path = format!("fixtures/extraction/gdscript/{fixture}/source.gd");
        let expected_path = format!("fixtures/extraction/gdscript/{fixture}/expected.json");
        let source = std::fs::read_to_string(root.join(&source_path)).expect("GDScript source");
        let expected = std::fs::read_to_string(root.join(&expected_path)).expect("GDScript golden");
        let symbols = extract_gdscript(&source);
        assert_fixture_body_fields(&expected, &symbols, |symbol| {
            symbol.kind == SymbolKind::Event
        });
    }

    for fixture in [
        "basic",
        "cross_file",
        "current_syntax",
        "frameworks",
        "idioms",
        "namespace",
    ] {
        let source_name = if fixture == "namespace" {
            "NAMESPACE"
        } else if fixture == "basic" {
            "source.r"
        } else {
            "source.R"
        };
        let source_path = format!("fixtures/extraction/r/{fixture}/{source_name}");
        let expected_path = format!("fixtures/extraction/r/{fixture}/expected.json");
        let source = normalize_line_endings(
            std::fs::read_to_string(root.join(&source_path)).expect("R source"),
        );
        let expected = std::fs::read_to_string(root.join(&expected_path)).expect("R golden");
        let symbols = extract_r(&source);
        assert_fixture_body_fields(&expected, &symbols, |symbol| {
            matches!(
                symbol.kind,
                SymbolKind::Class | SymbolKind::Import | SymbolKind::Variable
            )
        });
    }
}
