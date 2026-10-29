use std::path::Path;

use serde_json::{Value, json};

use crate::{ExtractionLevel, IdentifierKind, extract_canonical_for_language_at};

fn call_metadata(source: &str, name: &str) -> Vec<(Value, Value, Value)> {
    extract_canonical_for_language_at(
        "kotlin",
        "run.kt",
        source,
        Path::new("."),
        ExtractionLevel::References,
    )
    .unwrap()
    .identifiers
    .into_iter()
    .filter(|identifier| identifier.kind == IdentifierKind::Call && identifier.name == name)
    .map(|identifier| {
        let metadata = identifier.metadata.unwrap_or_default();
        let key = |key: &str| metadata.get(key).cloned().unwrap_or(Value::Null);
        (key("arg_count"), key("arg_types"), key("arg_names"))
    })
    .collect()
}

fn arg_types(source: &str, name: &str) -> Vec<Value> {
    call_metadata(source, name)
        .into_iter()
        .map(|(_, types, _)| types)
        .collect()
}

#[test]
fn kotlin_calls_record_the_type_each_argument_states() {
    let source = r#"
class Box

class Run(val cached: Box) {
    fun go(param: Box, n: Long, vararg rest: Int) {
        val local = 3
        val typed: Box = Box()
        val built = Box()
        var big = 3000000000
        observe(1, 2L, 1.0f, 1.0, 'c', "s", true, null)
        observe(Box(), param as Any, this, local, cached, n, typed, built)
        observe("a $local", Box::class, unknown, rest, big, -1, 0xFFL, (n))
        observe(1u, 4294967296u, 1uL, 0x7FFFFFFF, 0x80000000, """raw""", "a" + 1, this.cached)
        observe(param as? Box, 1 + 2, make(), Run::go)
    }
}
"#;
    assert_eq!(
        arg_types(source, "observe"),
        vec![
            json!([
                "Int", "Long", "Float", "Double", "Char", "String", "Boolean", null
            ]),
            json!(["Box", "Any", "Run", "Int", "Box", "Long", "Box", "Box"]),
            json!([
                "String",
                "kotlin.reflect.KClass",
                null,
                null,
                "Long",
                "Int",
                "Long",
                "Long"
            ]),
            json!([
                "UInt", "ULong", "ULong", "Int", "Long", "String", "String", "Box"
            ]),
            json!(["Box", null, null, null]),
        ]
    );
    assert_eq!(arg_types(source, "Box")[2], json!([]));
}

#[test]
fn kotlin_named_arguments_record_their_names() {
    let source = r#"
fun go(items: IntArray) {
    observe(first = 1, "x")
    observe(1, `second` = "x")
    observe(1, 2)
}
"#;
    assert_eq!(
        call_metadata(source, "observe"),
        vec![
            (json!(2), json!(["Int", "String"]), json!(["first", null])),
            (json!(2), json!(["Int", "String"]), json!([null, "second"])),
            (json!(2), json!(["Int", "Int"]), Value::Null),
        ]
    );
}

#[test]
fn kotlin_trailing_lambdas_and_spreads_record_no_argument_types() {
    let source = r#"
fun go(items: IntArray) {
    observe(1) { it }
    observe { it }
    observe(*items, 2)
}
"#;
    assert_eq!(
        call_metadata(source, "observe"),
        vec![
            (json!(2), Value::Null, Value::Null),
            (json!(1), Value::Null, Value::Null),
            (json!(2), Value::Null, Value::Null),
        ]
    );
}

#[test]
fn kotlin_argument_names_follow_scopes_receivers_and_smart_casts() {
    let source = r#"
class Item

class Store(val value: String, plain: Int) {
    val fromPlain = observe(plain)

    fun go(values: List<Int>) {
        run {
            val value = 1
            observe(value)
        }
        observe(value)
        for (value in values) {
            observe(value)
        }
        values.forEach { observe(value, it, this) }
        values.forEach { value: Int -> observe(value) }
        observe(plain)
    }

    fun cast(item: Any, other: Any) {
        if (item is Item) observe(item)
        observe(other)
    }

    fun subject(item: Any) {
        when (item) {
            is Item -> observe(item)
            else -> observe(item)
        }
    }

    fun Item.extension() {
        observe(this, value)
    }
}

object Single {
    fun go() { observe(this) }
}
"#;
    assert_eq!(
        arg_types(source, "observe"),
        vec![
            json!(["Int"]),
            json!(["Int"]),
            json!(["String"]),
            json!([null]),
            json!([null, null, null]),
            json!(["Int"]),
            json!([null]),
            json!([null]),
            json!(["Any"]),
            json!([null]),
            json!([null]),
            json!([null, null]),
            json!(["Single"]),
        ]
    );
}
