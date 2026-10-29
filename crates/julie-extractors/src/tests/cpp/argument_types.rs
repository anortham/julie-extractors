use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::{ExtractionLevel, IdentifierKind, extract_canonical_for_language_at};

const LITERALS: &str = r#"
struct Point { int x; Point(int x) : x(x) {} };

class Box {
  int size;
  const char* label;

  void go(double d, const Point& p, unsigned long u) const {
    long n = 1;
    auto q = Point(2);
    auto r = 3u;
    observe(1, "x", 'c', true, nullptr, 1.5, 2.5f, 3.5L);
    observe(n, d, p, q, r, u);
    observe(size, label, this);
    observe(Point(1), Point{1}, new Point(1), static_cast<long>(d), (short)d, int(d));
    observe(L"w", u8"u", u"x", U"y", R"(raw)", "a" "b");
    observe(unknown, n + 1, NULL, make(), 'ab', -1, (n));
  }
};
"#;

const INTEGERS: &str = r#"
void go() {
  observe(2147483647, 2147483648, 0x80000000, 0xffffffff, 0x100000000, 017, 0b101, 1'000);
  observe(1u, 4294967296u, 1l, 2147483648l, 1ul, 4294967296ul, 1z);
  observe(1ll, 9223372036854775808ll, 0xffffffffffffffffll, 1ull, 0x1p3, 1e3, 1.0f16);
}
"#;

const SCOPES: &str = r#"
struct Error {};

class Store {
  const char* value;
  void later();

  void run(int* values, int count) {
    {
      int value = 1;
      observe(value);
    }
    observe(value);
    for (int index = 0; index < count; ++index) {
      observe(index);
    }
    for (long each : values) {
      observe(each);
    }
    for (auto item : values) {
      observe(item);
    }
    try {
    } catch (Error& error) {
      observe(error);
    }
    if (int found = count) {
      observe(found);
    }
    auto lambda = [](double value) { observe(value); };
    auto capture = [value = 1]() { observe(value); };
    switch (count) {
    case 1: {
      short small = 1;
      observe(small);
    }
    }
    int values2[3];
    observe(values2);
    auto [first, second] = make();
    observe(first);
    observe(count);
  }
};

void Store::later() { observe(value); }

void Other::later() { observe(value); }

class Derived : public Store {
  void more() { observe(value); }
};

int global = 1;

void free_function() { observe(global); }
"#;

const CONSTRUCTIONS: &str = r#"
struct Point { Point(int x); Point(const char* s); };

void make(Point p, int n) {
  Point a(1);
  Point b{"x"};
  Point c = {n};
  Point* d = new Point(2);
  auto e = Point{3};
  Point f(p);
  int g(5);
  Point h;
  Point i(1), j(2);
  ns::Point k(2.0);
}
"#;

fn calls(
    source: &str,
    name: &str,
    level: ExtractionLevel,
) -> Vec<(u32, Option<Value>, Option<Value>)> {
    extract_canonical_for_language_at("cpp", "test.cpp", source, Path::new("."), level)
        .unwrap()
        .identifiers
        .into_iter()
        .filter(|identifier| identifier.kind == IdentifierKind::Call && identifier.name == name)
        .map(|identifier| {
            let metadata = identifier.metadata.unwrap_or_default();
            (
                identifier.start_line,
                metadata.get("arg_count").cloned(),
                metadata.get("arg_types").cloned(),
            )
        })
        .collect()
}

fn observed(source: &str) -> Vec<Value> {
    calls(source, "observe", ExtractionLevel::References)
        .into_iter()
        .map(|(_, _, types)| types.unwrap_or(Value::Null))
        .collect()
}

#[test]
fn cpp_calls_record_the_type_each_argument_states() {
    assert_eq!(
        observed(LITERALS),
        vec![
            json!([
                "int",
                "const char*",
                "char",
                "bool",
                "std::nullptr_t",
                "double",
                "float",
                "long double"
            ]),
            json!([
                "long",
                "double",
                "const Point",
                "Point",
                "unsigned int",
                "unsigned long"
            ]),
            json!(["int", "const char*", "const Box*"]),
            json!(["Point", "Point", "Point*", "long", "short", "int"]),
            json!([
                "const wchar_t*",
                "const char8_t*",
                "const char16_t*",
                "const char32_t*",
                "const char*",
                "const char*"
            ]),
            json!([null, null, null, null, null, "int", "long"]),
        ]
    );
}

#[test]
fn cpp_integer_argument_types_need_one_answer_on_every_data_model() {
    assert_eq!(
        observed(INTEGERS),
        vec![
            json!([
                "int",
                null,
                "unsigned int",
                "unsigned int",
                null,
                "int",
                "int",
                "int"
            ]),
            json!([
                "unsigned int",
                null,
                "long",
                null,
                "unsigned long",
                null,
                null
            ]),
            json!([
                "long long",
                null,
                "unsigned long long",
                "unsigned long long",
                "double",
                "double",
                null
            ]),
        ]
    );
}

#[test]
fn cpp_argument_types_follow_block_scopes_and_class_members() {
    assert_eq!(
        observed(SCOPES),
        vec![
            json!(["int"]),
            json!(["const char*"]),
            json!(["int"]),
            json!(["long"]),
            json!([null]),
            json!(["Error"]),
            json!(["int"]),
            json!(["double"]),
            json!([null]),
            json!(["short"]),
            json!([null]),
            json!([null]),
            json!(["int"]),
            json!(["const char*"]),
            json!([null]),
            json!([null]),
            json!(["int"]),
        ]
    );
}

#[test]
fn cpp_constructions_are_calls_with_argument_counts_and_types() {
    assert_eq!(
        calls(CONSTRUCTIONS, "Point", ExtractionLevel::References),
        vec![
            (5, Some(json!(1)), Some(json!(["int"]))),
            (6, Some(json!(1)), Some(json!(["const char*"]))),
            (7, Some(json!(1)), Some(json!(["int"]))),
            (8, Some(json!(1)), Some(json!(["int"]))),
            (9, Some(json!(1)), Some(json!(["int"]))),
            (10, Some(json!(1)), Some(json!(["Point"]))),
            (14, Some(json!(1)), Some(json!(["double"]))),
        ]
    );
}

#[test]
fn cpp_argument_types_need_the_references_level() {
    assert!(calls(CONSTRUCTIONS, "Point", ExtractionLevel::Facts).is_empty());
    assert!(
        calls(LITERALS, "observe", ExtractionLevel::Facts)
            .iter()
            .all(|(_, _, types)| types.is_none())
    );
    assert_eq!(
        calls(LITERALS, "observe", ExtractionLevel::References),
        calls(LITERALS, "observe", ExtractionLevel::Full)
    );
}

#[test]
fn cpp_construction_calls_leave_type_usages_and_other_declarations_alone() {
    let identifiers = extract_canonical_for_language_at(
        "cpp",
        "test.cpp",
        CONSTRUCTIONS,
        Path::new("."),
        ExtractionLevel::Full,
    )
    .unwrap()
    .identifiers;
    let type_usages = identifiers
        .iter()
        .filter(|identifier| {
            identifier.kind == IdentifierKind::TypeUsage && identifier.name == "Point"
        })
        .count();
    assert_eq!(type_usages, 11);
    assert!(
        identifiers
            .iter()
            .filter(|identifier| identifier.kind == IdentifierKind::Call)
            .all(|identifier| identifier.name == "Point")
    );
}

#[test]
fn cpp_argument_types_of_many_calls_in_one_block_stay_fast() {
    let calls: String = (0..5_000)
        .map(|index| format!("  observe(a);\n  observe(b{index});\n"))
        .collect();
    let source = format!("long a = 1;\nvoid run() {{\n{calls}}}\n");
    let started = Instant::now();
    let types = observed(&source);
    let elapsed = started.elapsed();
    assert_eq!(types.len(), 10_000);
    assert_eq!(types[9_998], json!(["long"]));
    assert_eq!(types[9_999], json!([null]));
    assert!(
        elapsed < Duration::from_secs(10),
        "{elapsed:?} for 10000 calls"
    );
}
