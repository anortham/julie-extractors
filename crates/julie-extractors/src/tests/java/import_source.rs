use crate::ExtractionResults;
use crate::pipeline::extract_canonical;
use crate::tests::helpers::import_rows_with_metadata;
use serde_json::json;
use std::path::Path;

fn extract(source: &str) -> ExtractionResults {
    extract_canonical("src/app/J.java", source, Path::new("/workspace")).expect("extract")
}

#[test]
fn imports_record_their_source_form_and_wildcard() {
    let results = extract(
        "package app;\nimport java.util.*;\nimport java.util.List;\nimport static org.junit.Assert.*;\nimport static org.junit.Assert.assertEquals;\nimport java.*;\nclass J {}\n",
    );
    assert_eq!(
        import_rows_with_metadata(&results),
        vec![
            (
                "util".to_string(),
                json!({"form": "import", "source": "java.util", "wildcard": true})
            ),
            (
                "List".to_string(),
                json!({"form": "import", "source": "java.util"})
            ),
            (
                "Assert".to_string(),
                json!({"form": "import static", "source": "org.junit.Assert", "wildcard": true})
            ),
            (
                "assertEquals".to_string(),
                json!({"form": "import static", "source": "org.junit.Assert"})
            ),
            (
                "java".to_string(),
                json!({"form": "import", "source": "java", "wildcard": true})
            ),
        ]
    );
}

#[test]
fn import_signatures_keep_their_written_form() {
    let results = extract(
        "package app;\nimport java.util.*;\nimport static org.junit.Assert.*;\nimport java.util.List;\nclass J {}\n",
    );
    let signatures: Vec<_> = results
        .symbols
        .iter()
        .filter_map(|symbol| symbol.signature.as_deref())
        .filter(|signature| signature.starts_with("import"))
        .collect();
    assert_eq!(
        signatures,
        vec![
            "import java.util.*",
            "import static org.junit.Assert.*",
            "import java.util.List"
        ]
    );
}
