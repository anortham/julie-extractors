use crate::ExtractionResults;
use crate::base::SymbolKind;
use crate::pipeline::extract_canonical;
use crate::tests::helpers::import_rows_with_metadata;
use serde_json::json;
use std::path::Path;

fn extract(source: &str) -> ExtractionResults {
    extract_canonical("lib/app.rb", source, Path::new("/workspace")).expect("extract")
}

#[test]
fn require_forms_record_their_source_and_form() {
    let results = extract(
        "require_relative \"lib/helper\"\nrequire 'json'\nload \"tasks/setup.rb\"\nautoload :Parser, \"app/parser\"\n",
    );
    assert_eq!(
        import_rows_with_metadata(&results),
        vec![
            (
                "helper".to_string(),
                json!({"source": "lib/helper", "form": "require_relative"})
            ),
            (
                "json".to_string(),
                json!({"source": "json", "form": "require"})
            ),
            (
                "setup.rb".to_string(),
                json!({"source": "tasks/setup.rb", "form": "load"})
            ),
            (
                "Parser".to_string(),
                json!({"source": "app/parser", "form": "autoload"})
            ),
        ]
    );
}

#[test]
fn interpolated_require_keeps_its_row_without_a_source() {
    let results = extract("require \"#{root}/lib/helper\"\n");
    assert_eq!(
        import_rows_with_metadata(&results),
        vec![("helper".to_string(), json!({"form": "require"}))]
    );
}

#[test]
fn autoload_inside_a_module_belongs_to_the_module() {
    let results = extract("module App\n  autoload :Parser, \"app/parser\"\nend\n");
    let module = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "App")
        .expect("module App");
    let parser = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "Parser" && symbol.kind == SymbolKind::Import)
        .expect("autoload import");
    assert_eq!(parser.parent_id.as_deref(), Some(module.id.as_str()));
}

#[test]
fn load_and_autoload_calls_that_are_not_file_imports_write_no_import() {
    let results = extract(
        "class Store\n  def restore(data)\n    config = YAML.load(data)\n    load(data)\n    autoload\n  end\nend\n",
    );
    assert_eq!(import_rows_with_metadata(&results), vec![]);
}
