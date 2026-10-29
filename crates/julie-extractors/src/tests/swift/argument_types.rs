use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::{ExtractionLevel, IdentifierKind, extract_canonical_for_language_at};

const SOURCE: &str = r#"
struct Point { let x: Int; init(x: Int) { self.x = x } }
struct Maybe { init?(s: String) { return nil } }

class Box {
  var size: Int = 0
  var label = "x"

  func go(d: Double, p: Point, values: [Int], _ text: String, maybe: Int?, more: Int...) {
    let n: Int = 1
    let q = Point(x: 2)
    let m = Maybe(s: "x")
    var a, b: Float
    let c: Int = 1, e = 2.5
    observe(1, "x", true, nil, 1.5, 0x1F, -1, (2))
    observe(a: n, b: d, c: p, q, values, text, maybe, more)
    observe(m, a, b, c, e, size, label, self)
    observe(Point(x: 1), d as Double, d as? Int, d as! Int, Maybe(s: "y"))
    observe(unknown, n + 1)
    observe(1) { x in x }
    observe { $0 }
    observe(1) { $0 } other: { $0 }
    observe()
  }
}
"#;

const SCOPES: &str = r#"
class Store {
  var item: String = ""

  static func make() { observe(self) }

  func run(items: [Int], maybe: Int?) {
    for item in items { observe(item) }
    observe(item)
    if let item = maybe { observe(item) }
    guard let other = maybe else { return }
    observe(other)
    items.forEach { item in observe(item) }
    items.forEach { (item: Int) in observe(item) }
    do { try work() } catch { observe(error) }
    switch maybe { case .some(let item): observe(item) default: break }
    func local(item: Double) { observe(item) }
    let (first, second) = (1, 2)
    observe(first)
    observe(missing)
  }
}

extension Store {
  func more() { observe(self, item) }
}

let global: Int = 1

func free() { observe(global) }
"#;

fn observed(source: &str, level: ExtractionLevel) -> Vec<(Value, Value, Value)> {
    extract_canonical_for_language_at("swift", "Test.swift", source, Path::new("."), level)
        .unwrap()
        .identifiers
        .into_iter()
        .filter(|identifier| {
            identifier.kind == IdentifierKind::Call && identifier.name == "observe"
        })
        .map(|identifier| {
            let metadata = identifier.metadata.unwrap_or_default();
            let key = |name: &str| metadata.get(name).cloned().unwrap_or(Value::Null);
            (key("arg_count"), key("arg_labels"), key("arg_types"))
        })
        .collect()
}

#[test]
fn swift_calls_record_argument_counts_labels_and_types() {
    assert_eq!(
        observed(SOURCE, ExtractionLevel::References),
        vec![
            (
                json!(8),
                json!([null, null, null, null, null, null, null, null]),
                json!(["Int", "String", "Bool", null, "Double", "Int", "Int", "Int"]),
            ),
            (
                json!(8),
                json!(["a", "b", "c", null, null, null, null, null]),
                json!([
                    "Int", "Double", "Point", "Point", "[Int]", "String", "Int?", null
                ]),
            ),
            (
                json!(8),
                json!([null, null, null, null, null, null, null, null]),
                json!([
                    null, "Float", "Float", "Int", "Double", "Int", "String", "Box"
                ]),
            ),
            (
                json!(5),
                json!([null, null, null, null, null]),
                json!(["Point", "Double", "Int?", "Int", null]),
            ),
            (json!(2), json!([null, null]), json!([null, null])),
            (json!(2), json!([null, null]), json!(["Int", null])),
            (json!(1), json!([null]), json!([null])),
            (
                json!(3),
                json!([null, null, "other"]),
                json!(["Int", null, null])
            ),
            (json!(0), json!([]), json!([])),
        ]
    );
}

#[test]
fn swift_argument_metadata_needs_the_references_level() {
    assert!(observed(SOURCE, ExtractionLevel::Facts).is_empty());
    assert_eq!(
        observed(SOURCE, ExtractionLevel::References),
        observed(SOURCE, ExtractionLevel::Full)
    );
}

#[test]
fn swift_argument_types_follow_scopes_and_pattern_bindings() {
    let types: Vec<Value> = observed(SCOPES, ExtractionLevel::References)
        .into_iter()
        .map(|(_, _, types)| types)
        .collect();
    assert_eq!(
        types,
        vec![
            json!([null]),
            json!([null]),
            json!(["String"]),
            json!([null]),
            json!([null]),
            json!([null]),
            json!(["Int"]),
            json!([null]),
            json!([null]),
            json!(["Double"]),
            json!([null]),
            json!([null]),
            json!(["Store", null]),
            json!(["Int"]),
        ]
    );
}

#[test]
fn swift_argument_types_of_many_calls_in_one_block_stay_fast() {
    let calls: String = (0..5_000)
        .map(|index| format!("  observe(a)\n  observe(b{index})\n"))
        .collect();
    let source = format!("func run() {{\n  let a = 1\n{calls}}}\n");
    let started = Instant::now();
    let found = observed(&source, ExtractionLevel::References);
    let elapsed = started.elapsed();
    assert_eq!(found.len(), 10_000);
    assert_eq!(found[9_998].2, json!(["Int"]));
    assert_eq!(found[9_999].2, json!([null]));
    assert!(
        elapsed < Duration::from_secs(10),
        "{elapsed:?} for 10000 calls"
    );
}
