use std::path::Path;

use serde_json::{Value, json};

use crate::{ExtractionLevel, IdentifierKind, extract_canonical_for_language_at};

fn call_metadata(source: &str, name: &str) -> Vec<(Value, Value, Value)> {
    extract_canonical_for_language_at(
        "scala",
        "Run.scala",
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
fn scala_calls_record_the_type_each_argument_states() {
    let source = r#"
class Box

class Run(val cached: Box) {
  def go(param: Box, n: Long, rest: Int*): Unit = {
    val local = 3
    val typed: Box = new Box()
    val built = new Box()
    var big = 3L
    observe(1, 2L, 1.0f, 1.0, 'c', "s", true, null)
    observe(new Box(), param.asInstanceOf[Any], this, local, cached, n, typed, built)
    observe(s"a $local", classOf[Box], unknown, rest, big, -1, (param: Any), "a" + 1)
    observe(this.cached, 0x1F, 1e3, sql"x", 1 + 2, make(), new Box { }, (n))
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
                "java.lang.Class",
                null,
                null,
                "Long",
                "Int",
                "Any",
                "String"
            ]),
            json!(["Box", "Int", "Double", null, null, null, null, "Long"]),
        ]
    );
}

#[test]
fn scala_named_arguments_record_their_names() {
    let source = r#"
object Run {
  def go(): Unit = {
    observe(first = 1, "x")
    observe(1, second = "x")
    observe(1, 2)
  }
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
fn scala_block_arguments_count_one_untyped_argument() {
    let source = r#"
object Run {
  def go(items: List[Int]): Unit = {
    observe { 1 }
    items.foreach { case x => observe(x) }
  }
}
"#;
    assert_eq!(
        call_metadata(source, "observe"),
        vec![
            (json!(1), json!([null]), Value::Null),
            (json!(1), json!([null]), Value::Null),
        ]
    );
    assert_eq!(
        call_metadata(source, "foreach"),
        vec![(json!(1), json!([null]), Value::Null)]
    );
}

#[test]
fn scala_applications_of_a_call_result_spreads_and_infix_calls_record_no_argument_keys() {
    let source = r#"
object Run {
  def go(items: List[Int]): Unit = {
    observe(1)(2)
    observe(1) { 2 }
    observe(items: _*)
    observe(items*)
    this observe 1
  }
}
"#;
    assert_eq!(
        call_metadata(source, "observe"),
        vec![
            (Value::Null, Value::Null, Value::Null),
            (Value::Null, Value::Null, Value::Null),
            (json!(1), Value::Null, Value::Null),
            (json!(1), Value::Null, Value::Null),
            (Value::Null, Value::Null, Value::Null),
        ]
    );
}

#[test]
fn scala_constructor_calls_are_call_identifiers_with_argument_types() {
    let source = r#"
class Box(size: Int)

object Run {
  def go(): Unit = {
    new Box(1)
    new a.b.Box[Int](2L)
    new Box
    new Box(3) { }
  }
}
"#;
    assert_eq!(
        call_metadata(source, "Box"),
        vec![
            (json!(1), json!(["Int"]), Value::Null),
            (json!(1), json!(["Long"]), Value::Null),
            (Value::Null, Value::Null, Value::Null),
        ]
    );
}

#[test]
fn scala_argument_names_follow_blocks_lambdas_patterns_and_members() {
    let source = r#"
class Scope(val value: String) {
  def go(values: List[Int]): Unit = {
    {
      val value = 1
      observe(value)
    }
    observe(value)
    values.foreach(value => observe(value))
    values.foreach { (value: Int) => observe(value) }
    values match { case value :: _ => observe(value) }
    for (value <- values) observe(value)
    observe(inherited)
  }
}

object Single {
  def go(): Unit = observe(this)
}
"#;
    assert_eq!(
        arg_types(source, "observe"),
        vec![
            json!(["Int"]),
            json!(["String"]),
            json!([null]),
            json!(["Int"]),
            json!([null]),
            json!([null]),
            json!([null]),
            json!(["Single"]),
        ]
    );
}
