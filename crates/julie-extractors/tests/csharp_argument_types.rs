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

const SCOPE_SOURCE: &str = r#"
class Error {}
class Resource {}

class Store
{
    private string value;
    private string each;
    private string index;
    private string error;
    private string item;

    void Run(int[] values)
    {
        {
            int value = 1;
            Observe(value);
        }
        Observe(value);
        foreach (int each in values)
        {
            Observe(each);
        }
        Observe(each);
        for (int index = 0; index < values.Length; index++)
        {
            Observe(index);
        }
        Observe(index);
        try
        {
            throw new Error();
        }
        catch (Error error)
        {
            Observe(error);
        }
        Observe(error);
        using (Resource item = new Resource())
        {
            Observe(item);
        }
        Observe(item);
        {
            var value = unknown;
            Observe(value);
        }
    }
}
"#;

const INTEGER_SOURCE: &str = r#"
class Store
{
    void Run()
    {
        Observe(2147483647, 2147483648, 4294967295, 4294967296, 9223372036854775807, 9223372036854775808, 18446744073709551615);
        Observe(1U, 4294967295u, 4294967296U, 1L, 9223372036854775807L, 9223372036854775808l, 18446744073709551615UL, 18446744073709551615lu, 18446744073709551616UL);
        Observe(0x7fffffff, 0x80000000, 0xFFFFFFFF, 0x100000000, 0x7fffffffffffffff, 0x8000000000000000, 0xffffffffffffffff);
        Observe(0b01111111111111111111111111111111, 0b10000000000000000000000000000000, 0b1_00000000000000000000000000000000, 0b1111111111111111111111111111111111111111111111111111111111111111);
        Observe(-2147483648, -2147483649, -9223372036854775808, -0x80000000, -18446744073709551616);
    }
}
"#;

const TYPEOF_SOURCE: &str = r#"
class Type {}

class Store
{
    void Run()
    {
        Observe(typeof(Type));
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

fn observe_arg_types(source: &str) -> Vec<Value> {
    extract_canonical_for_language_at(
        "csharp",
        "Store.cs",
        source,
        Path::new("."),
        ExtractionLevel::Full,
    )
    .unwrap()
    .identifiers
    .into_iter()
    .filter(|identifier| identifier.kind == IdentifierKind::Call && identifier.name == "Observe")
    .map(|identifier| {
        identifier
            .metadata
            .and_then(|metadata| metadata.get("arg_types").cloned())
            .unwrap()
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
                Some(json!([null, "string", "System.Type", "int", "string"]))
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

#[test]
fn csharp_argument_types_respect_lexical_binding_scopes() {
    assert_eq!(
        observe_arg_types(SCOPE_SOURCE),
        vec![
            json!(["int"]),
            json!(["string"]),
            json!(["int"]),
            json!(["string"]),
            json!(["int"]),
            json!(["string"]),
            json!(["Error"]),
            json!(["string"]),
            json!(["Resource"]),
            json!(["string"]),
            json!([null]),
        ]
    );
}

#[test]
fn csharp_integer_argument_types_follow_value_radix_and_suffix() {
    assert_eq!(
        observe_arg_types(INTEGER_SOURCE),
        vec![
            json!(["int", "uint", "uint", "long", "long", "ulong", "ulong"]),
            json!([
                "uint", "uint", "ulong", "long", "long", "ulong", "ulong", "ulong", null
            ]),
            json!(["int", "uint", "uint", "long", "long", "ulong", "ulong"]),
            json!(["int", "uint", "long", "ulong"]),
            json!([null, null, null, null, null]),
        ]
    );
}

#[test]
fn csharp_typeof_argument_type_is_system_type_when_a_project_type_is_named_type() {
    assert_eq!(
        observe_arg_types(TYPEOF_SOURCE),
        vec![json!(["System.Type"])]
    );
}
