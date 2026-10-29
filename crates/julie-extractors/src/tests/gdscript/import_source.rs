use crate::ExtractionResults;
use crate::base::SymbolKind;
use crate::pipeline::extract_canonical;
use crate::tests::helpers::import_rows_with_metadata;
use serde_json::json;
use std::path::Path;

fn extract(source: &str) -> ExtractionResults {
    extract_canonical("scripts/player.gd", source, Path::new("/workspace")).expect("extract")
}

#[test]
fn preload_and_load_constants_and_fields_also_import_the_resource() {
    let results = extract(
        "extends Node\nconst Helper = preload(\"res://scripts/helper.gd\")\nvar Scene = load(\"res://scenes/main.tscn\")\nvar _icon := preload('res://icon.png')\n",
    );
    assert_eq!(
        import_rows_with_metadata(&results),
        vec![
            (
                "Helper".to_string(),
                json!({"source": "res://scripts/helper.gd", "form": "preload"})
            ),
            (
                "Scene".to_string(),
                json!({"source": "res://scenes/main.tscn", "form": "load"})
            ),
            (
                "_icon".to_string(),
                json!({"source": "res://icon.png", "form": "preload"})
            ),
        ]
    );
    for (name, kind) in [
        ("Helper", SymbolKind::Constant),
        ("Scene", SymbolKind::Field),
        ("_icon", SymbolKind::Field),
    ] {
        let first_named = results
            .symbols
            .iter()
            .find(|symbol| symbol.name == name)
            .expect("named row");
        assert_eq!(first_named.kind, kind);
        let binding = first_named;
        let import = results
            .symbols
            .iter()
            .find(|symbol| symbol.name == name && symbol.kind == SymbolKind::Import)
            .expect("import row");
        assert_ne!(binding.id, import.id);
        assert_eq!(binding.parent_id, import.parent_id);
        assert_eq!(binding.start_line, import.start_line);
    }
}

#[test]
fn preload_call_identifiers_stay_in_their_constant() {
    let results = extract("const Helper = preload(\"res://scripts/helper.gd\")\n");
    let constant = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "Helper" && symbol.kind == SymbolKind::Constant)
        .expect("constant");
    let preload = results
        .identifiers
        .iter()
        .find(|identifier| identifier.name == "preload")
        .expect("preload call identifier");
    assert_eq!(
        preload.containing_symbol_id.as_deref(),
        Some(constant.id.as_str())
    );
}

#[test]
fn other_initializers_and_local_loads_write_no_import() {
    let results = extract(
        "extends Node\nvar path = \"res://x.gd\"\nvar dynamic = load(path)\nvar node = Node.new()\nconst Two = preload(\"res://a.gd\", \"x\")\nfunc _ready():\n\tvar tex = load(\"res://icon.png\")\n",
    );
    assert_eq!(import_rows_with_metadata(&results), vec![]);
}
