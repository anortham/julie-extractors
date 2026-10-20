use std::path::Path;

use julie_extractors::{ExtractionLevel, IdentifierKind, extract_canonical_for_language_at};
use serde_json::{Value, json};

const SOURCE: &str = r#"using System;

class JValue {}

class Store
{
    private JValue _cached;

    public void Run(JValue value, string label)
    {
        var local = new JValue();
        Equals(value);
        Equals(local);
        Equals(_cached);
        Equals((object)value);
        Equals(value as object);
        Equals(this);
        Take("a", 'b', true, 1, 2L, 3u, 4ul, 1.5, 2.5f, 3.5m);
        Take(null, nameof(label), typeof(Store), default(int), (label));
        Take(unknown, label.Length);
        Named(count: 1);
        ByRef(ref local);
        Equals(new JValue());
    }
}
"#;

fn arg_types(level: ExtractionLevel) -> Vec<(String, Option<Value>)> {
    extract_canonical_for_language_at("csharp", "Store.cs", SOURCE, Path::new("."), level)
        .unwrap()
        .identifiers
        .into_iter()
        .filter(|identifier| {
            identifier.kind == IdentifierKind::Call
                && ["Equals", "Take", "Named", "ByRef"].contains(&identifier.name.as_str())
        })
        .map(|identifier| {
            let types = identifier
                .metadata
                .and_then(|metadata| metadata.get("arg_types").cloned());
            (identifier.name, types)
        })
        .collect()
}

#[test]
fn csharp_calls_record_the_type_each_argument_states() {
    assert_eq!(
        arg_types(ExtractionLevel::Full),
        vec![
            ("Equals".to_string(), Some(json!(["JValue"]))),
            ("Equals".to_string(), Some(json!(["JValue"]))),
            ("Equals".to_string(), Some(json!(["JValue"]))),
            ("Equals".to_string(), Some(json!(["object"]))),
            ("Equals".to_string(), Some(json!(["object"]))),
            ("Equals".to_string(), Some(json!(["Store"]))),
            (
                "Take".to_string(),
                Some(json!([
                    "string", "char", "bool", "int", "long", "uint", "ulong", "double", "float",
                    "decimal"
                ]))
            ),
            (
                "Take".to_string(),
                Some(json!([null, "string", "Type", "int", "string"]))
            ),
            ("Take".to_string(), Some(json!([null, null]))),
            ("Named".to_string(), None),
            ("ByRef".to_string(), None),
            ("Equals".to_string(), Some(json!(["JValue"]))),
        ]
    );
}

#[test]
fn csharp_argument_types_need_the_references_level() {
    assert!(
        arg_types(ExtractionLevel::Facts)
            .iter()
            .all(|(_, types)| types.is_none())
    );
    assert_eq!(
        arg_types(ExtractionLevel::References),
        arg_types(ExtractionLevel::Full)
    );
}
