use crate::ExtractionResults;
use crate::pipeline::extract_canonical;
use crate::tests::helpers::import_rows_with_metadata;
use serde_json::json;
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/workspace")).expect("extract")
}

#[test]
fn directives_record_their_uri_and_form() {
    let results = extract(
        "lib/d.dart",
        "import 'package:app/models.dart';\nimport 'util/strings.dart' as s show trim;\nimport 'heavy.dart' deferred as heavy;\nexport 'src/api.dart';\npart 'd.g.dart';\n",
    );
    assert_eq!(
        import_rows_with_metadata(&results),
        vec![
            (
                "package:app/models.dart".to_string(),
                json!({"type": "import", "form": "import", "source": "package:app/models.dart"})
            ),
            (
                "util/strings.dart".to_string(),
                json!({
                    "type": "import",
                    "form": "import",
                    "source": "util/strings.dart",
                    "alias": "s",
                    "show": ["trim"]
                })
            ),
            (
                "heavy.dart".to_string(),
                json!({
                    "type": "import",
                    "form": "import",
                    "source": "heavy.dart",
                    "alias": "heavy",
                    "deferred": true
                })
            ),
            (
                "src/api.dart".to_string(),
                json!({"type": "export", "form": "export", "source": "src/api.dart"})
            ),
            (
                "d.g.dart".to_string(),
                json!({"type": "part", "form": "part", "source": "d.g.dart"})
            ),
        ]
    );
}

#[test]
fn part_of_records_its_uri_or_library_name() {
    let by_uri = extract("lib/d.g.dart", "part of 'd.dart';\n");
    assert_eq!(
        import_rows_with_metadata(&by_uri),
        vec![(
            "d.dart".to_string(),
            json!({"type": "part_of", "form": "part of", "source": "d.dart"})
        )]
    );
    let by_name = extract("lib/e.dart", "part of blog.lib;\n");
    assert_eq!(
        import_rows_with_metadata(&by_name),
        vec![(
            "blog.lib".to_string(),
            json!({"type": "part_of", "form": "part of", "source": "blog.lib"})
        )]
    );
}
