use std::path::Path;

use serde_json::{Value, json};

use crate::{ExtractionLevel, IdentifierKind, extract_canonical_for_language_at};

fn call_metadata(source: &str, name: &str, level: ExtractionLevel) -> Vec<(Value, Value, Value)> {
    extract_canonical_for_language_at("java", "Run.java", source, Path::new("."), level)
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
    call_metadata(source, name, ExtractionLevel::Full)
        .into_iter()
        .map(|(_, types, _)| types)
        .collect()
}

const SOURCE: &str = r#"
class Box {}

class Run {
    private Box cached;
    static final int LIMIT = 3;

    void go(Box param, long n, String... rest) {
        int local = 3;
        var inferred = new Box();
        var count = 7L;
        observe(1, 2L, 1.0f, 1.0, 'c', "s", true, null);
        observe(new Box(), (Object) param, this, local, cached, n);
        observe(Box.class, "a" + local, local + "a", unknown, rest, inferred, count);
        observe(0x1F, 07, 0b1, 1_000, -1, (n), 2.5e3, 1.0d, this.cached, Run.this, LIMIT);
        observe(new Outer.Inner(), new java.util.ArrayList<String>(), (java.util.List<String>) null, 1 + 2);
        observe("""
            text""", 1 + 2 + "a", local - 1, call(), new int[3]);
    }
}
"#;

#[test]
fn java_calls_record_the_type_each_argument_states() {
    assert_eq!(
        arg_types(SOURCE, "observe"),
        vec![
            json!([
                "int", "long", "float", "double", "char", "String", "boolean", null
            ]),
            json!(["Box", "Object", "Run", "int", "Box", "long"]),
            json!([
                "java.lang.Class",
                "String",
                "String",
                null,
                null,
                "Box",
                "long"
            ]),
            json!([
                "int", "int", "int", "int", "int", "long", "double", "double", "Box", "Run", "int"
            ]),
            json!(["Outer.Inner", null, null, null]),
            json!(["String", "String", null, null, null]),
        ]
    );
}

#[test]
fn java_argument_types_need_the_references_level() {
    assert!(
        call_metadata(SOURCE, "observe", ExtractionLevel::Facts)
            .iter()
            .all(|(count, types, _)| count.is_null() && types.is_null())
    );
    assert_eq!(
        arg_types(SOURCE, "observe"),
        call_metadata(SOURCE, "observe", ExtractionLevel::References)
            .into_iter()
            .map(|(_, types, _)| types)
            .collect::<Vec<_>>()
    );
}

#[test]
fn java_constructor_calls_are_call_identifiers_with_argument_types() {
    let source = r#"
class Pair<A, B> {
    Pair(A a, B b) {}
}

class Box {
    Box(int size) {}
    Box(String name) {}
}

class Run {
    void go(Outer outer) {
        new Box(1);
        new Box("x");
        new Pair<String, Integer>("a", 1);
        outer.new Box(2);
        new Box(3) { };
    }
}
"#;
    assert_eq!(
        call_metadata(source, "Box", ExtractionLevel::Full),
        vec![
            (json!(1), json!(["int"]), Value::Null),
            (json!(1), json!(["String"]), Value::Null),
            (json!(1), json!(["int"]), Value::Null),
        ]
    );
    assert_eq!(
        call_metadata(source, "Pair", ExtractionLevel::Full),
        vec![(json!(2), json!(["String", "int"]), Value::Null)]
    );
    let anonymous = extract_canonical_for_language_at(
        "java",
        "Run.java",
        source,
        Path::new("."),
        ExtractionLevel::Full,
    )
    .unwrap()
    .identifiers
    .into_iter()
    .filter(|identifier| identifier.name == "Box" && identifier.start_line == 17)
    .map(|identifier| (identifier.kind, identifier.metadata))
    .collect::<Vec<_>>();
    assert_eq!(anonymous, vec![(IdentifierKind::TypeUsage, None)]);
}

#[test]
fn java_argument_names_follow_block_loop_resource_catch_and_lambda_scopes() {
    let source = r#"
class Item {}

class Store {
    private String value;

    void go(java.util.List<Item> items) {
        {
            int value = 1;
            observe(value);
        }
        observe(value);
        for (int index = 0; index < 3; index++) {
            observe(index);
        }
        for (Item each : items) {
            observe(each);
        }
        try (Item resource = open()) {
            observe(resource);
        } catch (IllegalStateException | IllegalArgumentException error) {
            observe(error);
        }
        Runnable task = () -> observe(value);
        java.util.function.Consumer<Long> sink = value -> observe(value);
        java.util.function.Consumer<Item> typed = (Item value) -> observe(value);
        new Object() {
            Item value;
            void m() { observe(value); }
        };
        new Object() {
            void m() { observe(value); }
        };
        switch (items.size()) {
            case 0:
                long size = 0;
                break;
            default:
                observe(size);
        }
    }
}
"#;
    assert_eq!(
        arg_types(source, "observe"),
        vec![
            json!(["int"]),
            json!(["String"]),
            json!(["int"]),
            json!(["Item"]),
            json!(["Item"]),
            json!([null]),
            json!(["String"]),
            json!([null]),
            json!(["Item"]),
            json!(["Item"]),
            json!([null]),
            json!(["long"]),
        ]
    );
}

#[test]
fn java_pattern_variables_and_inherited_names_have_no_argument_type() {
    let source = r#"
class Item {}

class Base {
    protected Item inherited;
}

class Store extends Base {
    void go(Object value, Item other) {
        if (value instanceof Item item) {
            observe(item);
        }
        observe(inherited);
        observe(value);
    }

    enum Color {
        RED;

        void paint() { observe(RED, this); }
    }

    record Point(int x) {
        void show() { observe(x, this); }
    }
}
"#;
    assert_eq!(
        arg_types(source, "observe"),
        vec![
            json!([null]),
            json!([null]),
            json!(["Object"]),
            json!(["Color", "Color"]),
            json!(["int", "Point"]),
        ]
    );
}
