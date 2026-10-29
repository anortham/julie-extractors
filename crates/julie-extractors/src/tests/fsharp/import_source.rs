use crate::ExtractionResults;
use crate::pipeline::extract_canonical;
use crate::tests::helpers::import_rows_with_metadata;
use serde_json::json;
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/workspace")).expect("extract")
}

#[test]
fn script_directives_and_opens_record_their_source_and_form() {
    let results = extract(
        "scripts/build.fsx",
        "#load \"Lib/Util.fsx\"\n#r \"nuget: Newtonsoft.Json, 13.0.1\"\n#r \"lib/Foo.dll\"\nopen System.IO\nopen MyApp.Blog\n",
    );
    assert_eq!(
        import_rows_with_metadata(&results),
        vec![
            (
                "Lib/Util.fsx".to_string(),
                json!({"directive": "load", "form": "#load", "source": "Lib/Util.fsx"})
            ),
            (
                "Newtonsoft.Json".to_string(),
                json!({"directive": "r", "form": "#r", "source": "nuget", "version": "13.0.1"})
            ),
            (
                "lib/Foo.dll".to_string(),
                json!({"directive": "r", "form": "#r", "source": "lib/Foo.dll"})
            ),
            (
                "IO".to_string(),
                json!({"form": "open", "source": "System.IO"})
            ),
            (
                "Blog".to_string(),
                json!({"form": "open", "source": "MyApp.Blog"})
            ),
        ]
    );
}

#[test]
fn open_inside_a_module_records_its_source() {
    let results = extract(
        "src/App.fs",
        "module App\n\nopen System\n\nlet run () = Console.WriteLine \"hi\"\n",
    );
    assert_eq!(
        import_rows_with_metadata(&results),
        vec![(
            "System".to_string(),
            json!({"form": "open", "source": "System"})
        )]
    );
}
