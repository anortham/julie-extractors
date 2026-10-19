use std::path::Path;
use std::time::{Duration, Instant};

use julie_extractors::{ExtractionLevel, IdentifierKind, extract_canonical_for_language_at};
use serde_json::Value;

type Expected = (&'static str, Option<u64>, bool);

fn assert_calls(language: &str, path: &str, source: &str, expected: &[Expected]) {
    let results = extract_canonical_for_language_at(
        language,
        path,
        source,
        Path::new("."),
        ExtractionLevel::Full,
    )
    .unwrap();

    for (name, count, spread) in expected {
        let calls: Vec<_> = results
            .identifiers
            .iter()
            .filter(|identifier| {
                identifier.kind == IdentifierKind::Call && identifier.name == *name
            })
            .collect();
        assert_eq!(
            calls.len(),
            1,
            "{language}: expected one call identifier named {name:?}"
        );
        let metadata = calls[0].metadata.as_ref();
        let actual_count = metadata
            .and_then(|metadata| metadata.get("arg_count"))
            .map(|value| value.as_u64().unwrap_or_else(|| panic!("{name}: {value}")));
        let actual_spread = metadata.and_then(|metadata| metadata.get("arg_spread"));
        assert_eq!(actual_count, *count, "{language}: arg_count of {name:?}");
        assert_eq!(
            actual_spread,
            spread.then_some(&Value::Bool(true)),
            "{language}: arg_spread of {name:?}"
        );
    }
}

fn argument_counts_of(language: &str, path: &str, source: &str, name: &str) -> Vec<Option<u64>> {
    extract_canonical_for_language_at(
        language,
        path,
        source,
        Path::new("."),
        ExtractionLevel::Full,
    )
    .unwrap()
    .identifiers
    .iter()
    .filter(|identifier| identifier.kind == IdentifierKind::Call && identifier.name == name)
    .map(|identifier| {
        identifier
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("arg_count"))
            .and_then(Value::as_u64)
    })
    .collect()
}

fn assert_argument_lists_carry_no_keys(
    language: &str,
    path: &str,
    lists: &[&str],
    wrap: fn(&str) -> String,
) {
    let statements: Vec<_> = lists.iter().map(|list| format!("q({list});")).collect();
    let keys = argument_counts_of(language, path, &wrap(&statements.join("\n")), "q");
    assert_eq!(keys.len(), lists.len(), "{language}: one call per list");
    for (list, key) in lists.iter().zip(keys) {
        assert_eq!(key, None, "{language}: arg_count of q({list})");
    }
}

fn assert_each_argument_list_carries_no_key(
    language: &str,
    path: &str,
    lists: &[&str],
    wrap: fn(&str) -> String,
) {
    for list in lists {
        let keys = argument_counts_of(language, path, &wrap(&format!("q({list});")), "q");
        assert_eq!(keys, [None], "{language}: arg_count of q({list})");
    }
}

fn assert_argument_lists_have_counts(
    language: &str,
    path: &str,
    lists: &[(&str, u64)],
    wrap: fn(&str) -> String,
) {
    let statements: Vec<_> = lists
        .iter()
        .map(|(list, _)| format!("q({list});"))
        .collect();
    let keys = argument_counts_of(language, path, &wrap(&statements.join("\n")), "q");
    assert_eq!(keys.len(), lists.len(), "{language}: one call per list");
    for ((list, count), key) in lists.iter().zip(keys) {
        assert_eq!(key, Some(*count), "{language}: arg_count of q({list})");
    }
}

fn in_kotlin_function(statements: &str) -> String {
    format!("fun run(a: Int, b: Int, c: Int, d: Int, x: Any) {{\n{statements}\n}}\n")
}

fn in_typescript_function(statements: &str) -> String {
    format!(
        "declare const a: any, b: any, c: any, d: any, x: any;\nfunction run() {{\n{statements}\n}}\n"
    )
}

fn in_csharp_method(statements: &str) -> String {
    format!("class Z {{ void Run(int a, int b, int c, int d, object x) {{\n{statements}\n}} }}\n")
}

fn in_c_function(statements: &str) -> String {
    format!("static inline void run(long x, long y) {{\n{statements}\n}}\n")
}

fn in_cpp_function(statements: &str) -> String {
    format!(
        "long a, b, c, d;\ntemplate<class... T> long q(T&&...);\nvoid run() {{\n{statements}\n}}\n"
    )
}

#[test]
fn python_calls_count_arguments() {
    let source = "def run(items, opts):
    zero()
    one(1)
    three(1, 2, 3)
    keyword(1, key=2, other=3)
    splat(*items, **opts)
    outer(inner(1, 2), 3)
    obj.method(1, 2)
    Widget(1, 2)
    generator(x for x in items)
    commented(1,  # note
              2)
    chained(1)(2, 3)
    print(f\"{interpolated(1, 2)}\")
";
    assert_calls(
        "python",
        "sample.py",
        source,
        &[
            ("zero", Some(0), false),
            ("one", Some(1), false),
            ("three", Some(3), false),
            ("keyword", Some(3), false),
            ("splat", Some(2), true),
            ("outer", Some(2), false),
            ("inner", Some(2), false),
            ("method", Some(2), false),
            ("Widget", Some(2), false),
            ("generator", Some(1), false),
            ("commented", Some(2), false),
            ("chained", Some(1), false),
            ("interpolated", Some(2), false),
        ],
    );
}

#[test]
fn javascript_calls_count_arguments() {
    let source = "function run(items) {
  zero();
  one(1);
  three(1, 2, 3);
  splat(...items, 2);
  outer(inner(1, 2), 3);
  obj.method(1, 2);
  new Widget(1, 2);
  new Bare;
  tagged`x ${1}`;
  commented(/* note */ 1, 2);
  optional?.chain(1);
  spaced /* note */ (1, 2);
  const view = <Component a={1} />;
}
";
    assert_calls(
        "javascript",
        "sample.js",
        source,
        &[
            ("zero", Some(0), false),
            ("one", Some(1), false),
            ("three", Some(3), false),
            ("splat", Some(2), true),
            ("outer", Some(2), false),
            ("inner", Some(2), false),
            ("method", Some(2), false),
            ("Widget", Some(2), false),
            ("Bare", None, false),
            ("tagged", None, false),
            ("commented", Some(2), false),
            ("chain", Some(1), false),
            ("spaced", Some(2), false),
            ("Component", None, false),
        ],
    );
}

#[test]
fn typescript_calls_count_arguments() {
    let source = "function run(items: number[]) {
  zero();
  one(1);
  three(1, 2, 3);
  splat(...items, 2);
  outer(inner(1, 2), 3);
  obj.method(1, 2);
  new Widget(1, 2);
  generic<number>(1);
  new Box<string>(1, 2);
  f<T>(1);
  nested<Map<string, number[]>>(1, 2);
}
";
    assert_calls(
        "typescript",
        "sample.ts",
        source,
        &[
            ("zero", Some(0), false),
            ("one", Some(1), false),
            ("three", Some(3), false),
            ("splat", Some(2), true),
            ("outer", Some(2), false),
            ("inner", Some(2), false),
            ("method", Some(2), false),
            ("Widget", Some(2), false),
            ("generic", Some(1), false),
            ("Box", Some(2), false),
            ("f", Some(1), false),
            ("nested", Some(2), false),
        ],
    );
}

#[test]
fn tsx_calls_count_arguments() {
    let source = "function run(items: number[]) {
  zero();
  one(1);
  three(1, 2, 3);
  splat(...items, 2);
  new Widget(1, 2);
  return <Component a={inner(1)} />;
}
";
    assert_calls(
        "tsx",
        "sample.tsx",
        source,
        &[
            ("zero", Some(0), false),
            ("one", Some(1), false),
            ("three", Some(3), false),
            ("splat", Some(2), true),
            ("Widget", Some(2), false),
            ("inner", Some(1), false),
            ("Component", None, false),
        ],
    );
}

#[test]
fn jsx_components_inside_a_callee_carry_no_argument_keys() {
    let source = "const view = (() => <Component />)(1, 2, 3);\n";
    assert_calls("tsx", "sample.tsx", source, &[("Component", None, false)]);
}

#[test]
fn java_calls_count_arguments() {
    let source = "class A {
  void run() {
    zero();
    one(1);
    three(1, 2, 3);
    outer(inner(1, 2), 3);
    obj.method(1, 2);
    this.own(1);
    commented(1, /* note */ 2);
    Foo.<String>generic(1);
    obj.<T>m(1);
    Runnable reference = this::run;
    ((Runnable) this::target).invoke();
  }
}
";
    assert_calls(
        "java",
        "A.java",
        source,
        &[
            ("zero", Some(0), false),
            ("one", Some(1), false),
            ("three", Some(3), false),
            ("outer", Some(2), false),
            ("inner", Some(2), false),
            ("method", Some(2), false),
            ("own", Some(1), false),
            ("commented", Some(2), false),
            ("generic", Some(1), false),
            ("m", Some(1), false),
            ("run", None, false),
            ("target", None, false),
            ("invoke", Some(0), false),
        ],
    );
}

#[test]
fn csharp_calls_count_arguments() {
    let source = "class A {
  void Run() {
    Zero();
    One(1);
    Three(1, 2, 3);
    Named(1, key: 2, other: 3);
    Outer(Inner(1, 2), 3);
    obj.Method(1, 2);
    new Widget(1, 2);
    new Empty();
    Gadget gadget = new(1);
    Generic<int>(1);
    M<int>(1, 2);
    maybe?.Conditional(1);
    Passed(out var result, ref other);
    new Seeded(1) { Value = 1 };
    new Bare { Value = 1 };
  }
}
";
    assert_calls(
        "csharp",
        "sample.cs",
        source,
        &[
            ("Zero", Some(0), false),
            ("One", Some(1), false),
            ("Three", Some(3), false),
            ("Named", Some(3), false),
            ("Outer", Some(2), false),
            ("Inner", Some(2), false),
            ("Method", Some(2), false),
            ("Widget", Some(2), false),
            ("Empty", Some(0), false),
            ("Gadget", Some(1), false),
            ("Generic", Some(1), false),
            ("M", Some(2), false),
            ("Conditional", Some(1), false),
            ("Passed", Some(2), false),
            ("Seeded", Some(1), false),
            ("Bare", None, false),
        ],
    );
}

#[test]
fn go_calls_count_arguments() {
    let source = "package main

func run(items []int) {
	zero()
	one(1)
	three(1, 2, 3)
	splat(items...)
	mixed(1, items...)
	outer(inner(1, 2), 3)
	obj.Method(1, 2)
	pkg.Func(1)
	make([]int, 3)
	defer cleanup(1)
	go worker(2, 3)
	/* note */ commented(1 /* note */, 2)
}
";
    assert_calls(
        "go",
        "sample.go",
        source,
        &[
            ("zero", Some(0), false),
            ("one", Some(1), false),
            ("three", Some(3), false),
            ("splat", Some(1), true),
            ("mixed", Some(2), true),
            ("outer", Some(2), false),
            ("inner", Some(2), false),
            ("Method", Some(2), false),
            ("Func", Some(1), false),
            ("make", Some(2), false),
            ("cleanup", Some(1), false),
            ("worker", Some(2), false),
            ("commented", Some(2), false),
        ],
    );
}

#[test]
fn rust_calls_count_arguments() {
    let source = "fn run() {
    zero();
    one(1);
    three(1, 2, 3);
    outer(inner(1, 2), 3);
    obj.method(1, 2);
    Widget::build(1);
    mod_a::mod_b::deep(1);
    generic::<u32>(1);
    turbofish::<u8>(1, 2);
    padded /* note */ ::<u8> /* note */ (1, 2);
    arrowed::<fn(u8) -> u8>(1);
    Wrapper(1, 2);
    chained(1)(2, 3);
    commented(1, /* note */ 2);
    attributed(#[allow(unused)] 1);
}
";
    assert_calls(
        "rust",
        "sample.rs",
        source,
        &[
            ("zero", Some(0), false),
            ("one", Some(1), false),
            ("three", Some(3), false),
            ("outer", Some(2), false),
            ("inner", Some(2), false),
            ("method", Some(2), false),
            ("build", Some(1), false),
            ("deep", Some(1), false),
            ("generic", Some(1), false),
            ("turbofish", Some(2), false),
            ("padded", Some(2), false),
            ("arrowed", Some(1), false),
            ("Wrapper", Some(2), false),
            ("chained", Some(1), false),
            ("commented", Some(2), false),
            ("attributed", Some(1), false),
        ],
    );
}

#[test]
fn rust_macro_calls_and_calls_inside_macro_arguments_carry_no_argument_keys() {
    let source = "fn run() {
    println!(\"{}\", compute(1, 2));
    wrap(log!(\"x\", nested(1)));
    make_handler!()(1);
}
";
    assert_calls(
        "rust",
        "sample.rs",
        source,
        &[
            ("println", None, false),
            ("compute", None, false),
            ("wrap", Some(1), false),
            ("log", None, false),
            ("nested", None, false),
            ("make_handler", None, false),
        ],
    );
}

#[test]
fn kotlin_calls_count_arguments() {
    let source = "fun run(items: List<Int>) {
    zero()
    one(1)
    three(1, 2, 3)
    named(1, key = 2, other = 3)
    splat(*items.toIntArray(), 2)
    outer(inner(1, 2), 3)
    obj.method(1, 2)
    Widget(1, 2)
    items.mapped { it + 1 }
    withLambda(1) { it }
    onlyLambda { }
    chained(1)(2, 3)
}
";
    assert_calls(
        "kotlin",
        "sample.kt",
        source,
        &[
            ("zero", Some(0), false),
            ("one", Some(1), false),
            ("three", Some(3), false),
            ("named", Some(3), false),
            ("splat", Some(2), true),
            ("toIntArray", Some(0), false),
            ("outer", Some(2), false),
            ("inner", Some(2), false),
            ("method", Some(2), false),
            ("Widget", Some(2), false),
            ("mapped", Some(1), false),
            ("withLambda", Some(2), false),
            ("onlyLambda", Some(1), false),
            ("chained", Some(1), false),
        ],
    );
}

#[test]
fn c_calls_count_arguments() {
    let source = "void run(void) {
    zero();
    one(1);
    three(1, 2, 3);
    outer(inner(1, 2), 3);
    obj.apply(1, 2);
    ptr->step(1);
    MACRO(a, b);
    commented(1, /* note */ 2);
}
";
    assert_calls(
        "c",
        "sample.c",
        source,
        &[
            ("zero", Some(0), false),
            ("one", Some(1), false),
            ("three", Some(3), false),
            ("outer", Some(2), false),
            ("inner", Some(2), false),
            ("apply", Some(2), false),
            ("step", Some(1), false),
            ("MACRO", Some(2), false),
            ("commented", Some(2), false),
        ],
    );
}

#[test]
fn cpp_calls_count_arguments() {
    let source = "void run() {
    zero();
    one(1);
    three(1, 2, 3);
    outer(inner(1, 2), 3);
    obj.apply(1, 2);
    ptr->step(1);
    ns::scoped(1);
    generic<int>(1);
    pack(args...);
    Widget(1);
}

void qualified() {
    a::b::c::d1(1);
    a::b::c::d2<int>(1);
    a::b::c::d::e1(1);
    a::b::c::d::e::six(1, 2);
    boost::asio::ip::tcp::resolver::query(1, 2);
    a::b::c::d::template e2<int>(1);
    a::b::c<int>::d3(1);
    ns::Cls<int>::stat(1, 2, 3);
    this->ns::Base::method(1);
    Foo<Bar>::create(1, 2);
    (void)g(1, 2);
    ((Foo*)p)->m(1);
}
";
    assert_calls(
        "cpp",
        "sample.cpp",
        source,
        &[
            ("zero", Some(0), false),
            ("one", Some(1), false),
            ("three", Some(3), false),
            ("outer", Some(2), false),
            ("inner", Some(2), false),
            ("apply", Some(2), false),
            ("step", Some(1), false),
            ("scoped", Some(1), false),
            ("generic", Some(1), false),
            ("pack", Some(1), true),
            ("Widget", Some(1), false),
            ("d1", Some(1), false),
            ("d2", Some(1), false),
            ("e1", Some(1), false),
            ("six", Some(2), false),
            ("query", Some(2), false),
            ("e2", Some(1), false),
            ("d3", Some(1), false),
            ("stat", Some(3), false),
            ("method", Some(1), false),
            ("create", Some(2), false),
            ("g", Some(2), false),
            ("m", Some(1), false),
        ],
    );
}

#[test]
fn argument_counts_keep_receiver_metadata() {
    let results = extract_canonical_for_language_at(
        "python",
        "sample.py",
        "def run():\n    obj.method(1, 2)\n",
        Path::new("."),
        ExtractionLevel::Full,
    )
    .unwrap();
    let call = results
        .identifiers
        .iter()
        .find(|identifier| identifier.kind == IdentifierKind::Call && identifier.name == "method")
        .unwrap();
    let metadata = call.metadata.as_ref().unwrap();

    assert_eq!(metadata.get("receiver"), Some(&Value::from("obj")));
    assert_eq!(metadata.get("arg_count"), Some(&Value::from(2)));
}

#[test]
fn languages_outside_the_argument_count_set_carry_no_argument_keys() {
    let cases = [
        ("ruby", "sample.rb", "def run\n  helper(1, 2)\nend\n"),
        (
            "php",
            "sample.php",
            "<?php\nfunction run() { helper(1, 2); $o->method(1, 2); new Thing(1); }\n",
        ),
        (
            "swift",
            "sample.swift",
            "func run() { helper(1, 2); o.method(1, 2) }\n",
        ),
        (
            "scala",
            "sample.scala",
            "object A { def run() = { helper(1, 2); o.method(1, 2) } }\n",
        ),
        (
            "dart",
            "sample.dart",
            "void run() { helper(1, 2); o.method(1, 2); }\n",
        ),
        (
            "lua",
            "sample.lua",
            "function run() helper(1, 2) o:method(1, 2) end\n",
        ),
        (
            "zig",
            "sample.zig",
            "fn run() void { helper(1, 2); o.method(1, 2); }\n",
        ),
        (
            "qml",
            "sample.qml",
            "import QtQuick 2.0\nItem { onClicked: { helper(1, 2); other(1) } }\n",
        ),
        ("gdscript", "sample.gd", "func run():\n\thelper(1, 2)\n"),
    ];
    for (language, path, source) in cases {
        let results = extract_canonical_for_language_at(
            language,
            path,
            source,
            Path::new("."),
            ExtractionLevel::Full,
        )
        .unwrap();
        let calls: Vec<_> = results
            .identifiers
            .iter()
            .filter(|identifier| identifier.kind == IdentifierKind::Call)
            .collect();

        assert!(!calls.is_empty(), "{language}: expected call identifiers");
        assert!(
            calls.iter().all(|identifier| identifier
                .metadata
                .as_ref()
                .is_none_or(|metadata| !metadata.contains_key("arg_count")
                    && !metadata.contains_key("arg_spread"))),
            "{language}: unexpected argument keys"
        );
    }
}

#[test]
fn kotlin_identifiers_inside_a_callee_that_are_not_its_terminal_name_carry_no_argument_keys() {
    let source = "fun run(n: Int, a: Int, b: Int, p: Int, q: Int) {
    (0 until n step 2).toList()
    (a infixop b).after(1, 2)
    (p infix3 q)(1, 2)
    (::ref).invoke(1, 2, 3)
    x.also { y infix2 z }.then(1, 2, 3, 4)
    val s = a infix4 b
    foo(a infix5 b, 2)
}
";
    assert_calls(
        "kotlin",
        "sample.kt",
        source,
        &[
            ("step", None, false),
            ("toList", Some(0), false),
            ("infixop", None, false),
            ("after", Some(2), false),
            ("infix3", None, false),
            ("ref", None, false),
            ("invoke", Some(3), false),
            ("infix2", None, false),
            ("also", Some(1), false),
            ("then", Some(4), false),
            ("infix4", None, false),
            ("infix5", None, false),
            ("foo", Some(2), false),
        ],
    );
}

#[test]
fn kotlin_infix_receiver_of_a_trailing_lambda_call_carries_no_argument_keys() {
    let source = "fun run(n: Int) {
    (0 until n).forEach { println(it) }
}
";
    assert_calls(
        "kotlin",
        "sample.kt",
        source,
        &[
            ("until", None, false),
            ("forEach", Some(1), false),
            ("println", Some(1), false),
        ],
    );
}

#[test]
fn kotlin_type_arguments_and_trailing_lambdas_keep_their_counts() {
    let source = "fun run() {
    listOf<Int>(1, 2)
    withLambda(1) { }
    onlyLambda { }
}
";
    assert_calls(
        "kotlin",
        "sample.kt",
        source,
        &[
            ("listOf", Some(2), false),
            ("withLambda", Some(2), false),
            ("onlyLambda", Some(1), false),
        ],
    );
}

#[test]
fn go_generic_type_conversions_count_their_operand() {
    let source = "package main

func run() {
	s := NewStack[int](10)
	NewQueue[int](10)
	foo(NewList[int](10), 2)
	t := Wrap[string](x).Unwrap()
	Pair[int, string](x, y)
	gen2[int](1).Next(5, 6, 7, 8)
	Single[int](x).Get(1, 2)
	Complex[map[string]int, []int](x, y)
}
";
    assert_calls(
        "go",
        "sample.go",
        source,
        &[
            ("NewStack", Some(1), false),
            ("NewQueue", Some(1), false),
            ("foo", Some(2), false),
            ("NewList", Some(1), false),
            ("Wrap", Some(1), false),
            ("Unwrap", Some(0), false),
            ("Pair", Some(2), false),
            ("gen2", Some(1), false),
            ("Next", Some(4), false),
            ("Single", Some(1), false),
            ("Get", Some(2), false),
            ("Complex", Some(2), false),
        ],
    );
}

#[test]
fn c_dereferenced_function_pointer_names_carry_no_argument_keys() {
    let source = "void run(void) {
    (*callback)(1, 2);
    (*(*cb))(1);
    ((fn_t)ptr)(1, 2, 3);
    ((a)->b)(7, 8);
    (void)f(1);
}
";
    assert_calls(
        "c",
        "sample.c",
        source,
        &[
            ("callback", None, false),
            ("cb", None, false),
            ("((fn_t)ptr)", Some(3), false),
            ("b", None, false),
            ("f", Some(1), false),
        ],
    );
}

#[test]
fn cpp_parenthesized_callees_keep_their_counts() {
    let source = "void run() {
    (*callback)(1, 2);
    (*(*cb))(1);
    ((fn_t)ptr)(1, 2, 3);
    ((a)->b)(7, 8);
    (void)f(1);
}
";
    assert_calls(
        "cpp",
        "sample.cpp",
        source,
        &[
            ("(*callback)", Some(2), false),
            ("(*(*cb))", Some(1), false),
            ("((fn_t)ptr)", Some(3), false),
            ("((a)->b)", Some(2), false),
            ("f", Some(1), false),
        ],
    );
}

#[test]
fn call_result_callees_and_immediately_invoked_lambdas_carry_no_argument_keys() {
    let source = "void run() {
    o->get()(1, 2);
    f(1)(2, 3);
    [&]{ inner(1); }();
    [](int a, int b) { return a; }(1, 2);
}
";
    assert_calls(
        "cpp",
        "sample.cpp",
        source,
        &[
            ("o->get()", None, false),
            ("get", Some(0), false),
            ("f", Some(1), false),
            ("[&]{ inner(1); }", None, false),
            ("inner", Some(1), false),
            ("[](int a, int b) { return a; }", None, false),
        ],
    );
}

#[test]
fn chained_calls_count_their_own_argument_lists() {
    let cases = [
        ("python", "sample.py", "def run():\n    a(1).b(2, 3)\n"),
        (
            "javascript",
            "sample.js",
            "function run() { a(1).b(2, 3); }\n",
        ),
        (
            "typescript",
            "sample.ts",
            "function run() { a(1).b(2, 3); }\n",
        ),
        ("rust", "sample.rs", "fn run() { a(1).b(2, 3); }\n"),
        (
            "go",
            "sample.go",
            "package main\nfunc run() { a(1).b(2, 3) }\n",
        ),
        ("kotlin", "sample.kt", "fun run() {\n    a(1).b(2, 3)\n}\n"),
        (
            "java",
            "A.java",
            "class A { void run() { a(1).b(2, 3); } }\n",
        ),
        (
            "csharp",
            "A.cs",
            "class A { void Run() { a(1).b(2, 3); } }\n",
        ),
        ("cpp", "sample.cpp", "void run() { a(1).b(2, 3); }\n"),
    ];
    for (language, path, source) in cases {
        assert_calls(
            language,
            path,
            source,
            &[("a", Some(1), false), ("b", Some(2), false)],
        );
    }
}

fn assert_syntax_error_carries_no_argument_keys(
    language: &str,
    path: &str,
    source: &str,
    broken: &[&'static str],
) {
    let mut expected: Vec<Expected> = vec![("clean", Some(2), false)];
    expected.extend(broken.iter().map(|name| (*name, None, false)));
    assert_calls(language, path, source, &expected);
}

#[test]
fn python_calls_with_a_syntax_error_in_the_arguments_carry_no_argument_keys() {
    assert_syntax_error_carries_no_argument_keys(
        "python",
        "sample.py",
        "def run():\n    clean(1, 2)\n    broken(1, $, 3)\n    unclosed(1, 2 3)\n",
        &["broken", "unclosed"],
    );
}

#[test]
fn javascript_calls_with_a_syntax_error_in_the_arguments_carry_no_argument_keys() {
    assert_syntax_error_carries_no_argument_keys(
        "javascript",
        "sample.js",
        "function run() { clean(1, 2); broken(1, @@@, 3); }\n",
        &["broken"],
    );
}

#[test]
fn typescript_calls_with_a_syntax_error_in_the_arguments_carry_no_argument_keys() {
    assert_syntax_error_carries_no_argument_keys(
        "typescript",
        "sample.ts",
        "function run() { clean(1, 2); broken(1, @@@, 3); }\n",
        &["broken"],
    );
}

#[test]
fn tsx_calls_with_a_syntax_error_in_the_arguments_carry_no_argument_keys() {
    assert_syntax_error_carries_no_argument_keys(
        "tsx",
        "sample.tsx",
        "function run() { clean(1, 2); broken(1, @@@, 3); }\n",
        &["broken"],
    );
}

#[test]
fn java_calls_with_a_syntax_error_in_the_arguments_carry_no_argument_keys() {
    assert_syntax_error_carries_no_argument_keys(
        "java",
        "A.java",
        "class A { void run() { clean(1, 2); broken(1, @@@, 3); } }\n",
        &["broken"],
    );
}

#[test]
fn csharp_calls_with_a_syntax_error_in_the_arguments_carry_no_argument_keys() {
    assert_syntax_error_carries_no_argument_keys(
        "csharp",
        "A.cs",
        "class A { void Run() { clean(1, 2); broken(1, @@@, 3); } }\n",
        &["broken"],
    );
}

#[test]
fn go_calls_with_a_syntax_error_in_the_arguments_carry_no_argument_keys() {
    assert_syntax_error_carries_no_argument_keys(
        "go",
        "sample.go",
        "package main\nfunc run() {\n\tclean(1, 2)\n\tbroken(1, @@@, 3)\n\tConv[int](x y)\n}\n",
        &["broken", "Conv"],
    );
}

#[test]
fn rust_calls_with_a_syntax_error_in_the_arguments_carry_no_argument_keys() {
    assert_syntax_error_carries_no_argument_keys(
        "rust",
        "sample.rs",
        "fn run() { clean(1, 2); broken(1, $, 3); }\n",
        &["broken"],
    );
}

#[test]
fn kotlin_calls_with_a_syntax_error_in_the_arguments_carry_no_argument_keys() {
    assert_syntax_error_carries_no_argument_keys(
        "kotlin",
        "sample.kt",
        "fun run(x: Int) {\n    clean(1, 2)\n    broken(1, 2 +, 3)\n}\n",
        &["broken"],
    );
}

#[test]
fn c_macro_calls_with_a_syntax_error_in_the_arguments_carry_no_argument_keys() {
    assert_syntax_error_carries_no_argument_keys(
        "c",
        "sample.c",
        "void run(void) { clean(1, 2); N4_OP(1, ==, 3, 4); }\n",
        &["N4_OP"],
    );
}

#[test]
fn cpp_macro_calls_with_a_syntax_error_in_the_arguments_carry_no_argument_keys() {
    assert_syntax_error_carries_no_argument_keys(
        "cpp",
        "sample.cpp",
        "void run() { clean(1, 2); N4_OP(1, ==, 3, 4); }\n",
        &["N4_OP"],
    );
}

#[test]
fn typescript_function_types_inside_type_arguments_keep_their_counts() {
    let source = "function run(cb: any, fn: any) {
  useState<() => void>(null, 1);
  withEvent<(e: Event) => void, string>(cb);
  returning<() => Promise<void>>(cb);
  nested<Array<() => void>>(1);
  useCallback<(e: Event) => void>(fn, []);
  twoFunctions<() => void, () => string>(1, 2);
  unionMember<string | (() => void)>(1);
}
";
    assert_calls(
        "typescript",
        "sample.ts",
        source,
        &[
            ("useState", Some(2), false),
            ("withEvent", Some(1), false),
            ("returning", Some(1), false),
            ("nested", Some(1), false),
            ("useCallback", Some(2), false),
            ("twoFunctions", Some(2), false),
            ("unionMember", Some(1), false),
        ],
    );
}

#[test]
fn tsx_function_types_inside_type_arguments_keep_their_counts() {
    let source = "function Comp(fn: any) {
  const cb = useCallback<(e: Event) => void>(fn, []);
  const state = useState<() => void>(null, 1);
}
";
    assert_calls(
        "tsx",
        "sample.tsx",
        source,
        &[
            ("useCallback", Some(2), false),
            ("useState", Some(2), false),
        ],
    );
}

#[test]
fn typescript_literals_inside_type_arguments_keep_their_counts() {
    let source = "function run(cb: any) {
  quoteChar<'>' | 'x'>(1);
  quoteDouble<\"a>b\">(1);
  escaped<'\\'>'>(1);
  template<`a${string}>`>(cb);
  templateUnion<`${'a' | 'b'}>` | 1>(cb, 2);
}
";
    assert_calls(
        "typescript",
        "sample.ts",
        source,
        &[
            ("quoteChar", Some(1), false),
            ("quoteDouble", Some(1), false),
            ("escaped", Some(1), false),
            ("template", Some(1), false),
            ("templateUnion", Some(2), false),
        ],
    );
}

#[test]
fn cpp_literals_and_shifts_inside_template_arguments_keep_their_counts() {
    let source = "void run() {
    chararg<'>'>(1);
    shifted<1 << 2>(1);
    separated<1'000>(1);
    prefixed<u8'>', 2>(1, 2);
    nestedshift<A<(1 << 2)>>(1);
}
";
    assert_calls(
        "cpp",
        "sample.cpp",
        source,
        &[
            ("chararg", Some(1), false),
            ("shifted", Some(1), false),
            ("separated", Some(1), false),
            ("prefixed", Some(2), false),
            ("nestedshift", Some(1), false),
        ],
    );
}

#[test]
fn cpp_template_calls_misparsed_as_comparisons_carry_no_argument_keys() {
    let source = "void run() {
    guarded(template_call<A, B<C>, D>(1));
    plain(template_call2<A, B>(1));
    int v = compared(a < b, c > d);
}
";
    assert_calls(
        "cpp",
        "sample.cpp",
        source,
        &[
            ("guarded", None, false),
            ("plain", None, false),
            ("compared", None, false),
        ],
    );
}

#[test]
fn javascript_optional_calls_count_arguments() {
    let source = "function run(f, obj, a) {
  f?.(1);
  obj.method?.(1);
  a?.b?.(1, 2);
  a?.[0]?.indexed(1);
  a.deep?.(1, 2, 3);
}
class K {
  m() { this.#own?.(1); }
}
";
    assert_calls(
        "javascript",
        "sample.js",
        source,
        &[
            ("f", Some(1), false),
            ("method", Some(1), false),
            ("b", Some(2), false),
            ("indexed", Some(1), false),
            ("deep", Some(3), false),
            ("#own", Some(1), false),
        ],
    );
}

#[test]
fn typescript_optional_calls_count_arguments() {
    let source = "function run(f: any, obj: any, a: any) {
  f?.(1);
  obj.method?.(1);
  a?.b?.(1, 2);
  typed?.<string>(1, 2);
  after /* note */ ?. /* note */ (1);
}
class K {
  m() { this.#own?.(1); }
}
";
    assert_calls(
        "typescript",
        "sample.ts",
        source,
        &[
            ("f", Some(1), false),
            ("method", Some(1), false),
            ("b", Some(2), false),
            ("typed", Some(2), false),
            ("after", Some(1), false),
            ("#own", Some(1), false),
        ],
    );
}

#[test]
fn tsx_optional_calls_count_arguments() {
    let source = "function Comp(props: any) {
  props.onClick?.(1);
  return <div onClick={() => props.onSelect?.(1, 2)} />;
}
";
    assert_calls(
        "tsx",
        "sample.tsx",
        source,
        &[("onClick", Some(1), false), ("onSelect", Some(2), false)],
    );
}

#[test]
fn typescript_non_null_callees_carry_no_argument_keys() {
    let source = "function run(a: any, props: any) {
  nonnull!(1);
  props.handler!(a);
  a!.member(1);
  a!.b!.chained(1, 2);
}
";
    let results = extract_canonical_for_language_at(
        "typescript",
        "sample.ts",
        source,
        Path::new("."),
        ExtractionLevel::Full,
    )
    .unwrap();
    let keyed_names: Vec<_> = results
        .identifiers
        .iter()
        .filter(|identifier| {
            identifier.kind == IdentifierKind::Call
                && identifier
                    .metadata
                    .as_ref()
                    .is_some_and(|metadata| metadata.contains_key("arg_count"))
        })
        .map(|identifier| identifier.name.as_str())
        .collect();

    assert_eq!(keyed_names, ["member", "chained"]);
}

#[test]
fn python_backslash_continuations_between_the_name_and_the_arguments_keep_the_count() {
    let source =
        "def run():\n    lf \\\n        (1)\n    crlf \\\r\n        (1, 2)\n    plain (3)\n";
    assert_calls(
        "python",
        "sample.py",
        source,
        &[
            ("lf", Some(1), false),
            ("crlf", Some(2), false),
            ("plain", Some(1), false),
        ],
    );
}

#[test]
fn vue_template_and_script_calls_carry_argument_keys_but_bare_handlers_do_not() {
    let source = "<template>
  <button @click=\"onclick_call(1)\" :title=\"titled(1, 2)\" @input=\"bare_handler\">{{ interpolated(1, 2, 3) }}</button>
</template>

<script setup lang=\"ts\">
const state = scripted<number>(1);
function run() { inner(1, 2); optional?.(1); }
typed<() => void>(1, 2);
</script>
";
    assert_calls(
        "vue",
        "App.vue",
        source,
        &[
            ("onclick_call", Some(1), false),
            ("titled", Some(2), false),
            ("bare_handler", None, false),
            ("interpolated", Some(3), false),
            ("scripted", Some(1), false),
            ("inner", Some(2), false),
            ("optional", Some(1), false),
            ("typed", Some(2), false),
        ],
    );
}

#[test]
fn html_script_blocks_and_inline_handlers_carry_argument_keys() {
    let source = "<html><body>
<button onclick=\"inline_handler(1)\">go</button>
<script>
function run() { script_call(1, 2); }
script_top(3);
</script>
</body></html>
";
    assert_calls(
        "html",
        "index.html",
        source,
        &[
            ("inline_handler", Some(1), false),
            ("script_call", Some(2), false),
            ("script_top", Some(1), false),
        ],
    );
}

#[test]
fn kotlin_trailing_lambdas_next_to_a_syntax_error_carry_no_argument_keys() {
    for source in [
        "fun run() {\n    clean(1, 2)\n    broken(a, b)) { c }\n}\n",
        "fun run() {\n    clean(1, 2)\n    broken(a,) b) { c }\n}\n",
        "fun run() {\n    clean(1, 2)\n    broken(a, b) {@@@ c }\n}\n",
        "fun run() {\n    clean(1, 2)\n    broken(a, b) { @@@ }\n}\n",
        "fun run() {\n    clean(1, 2)\n    outer(broken(a){, other(b))\n}\n",
    ] {
        assert_syntax_error_carries_no_argument_keys("kotlin", "sample.kt", source, &["broken"]);
    }
}

#[test]
fn kotlin_lambda_only_calls_keep_their_count_next_to_a_stray_brace_or_a_second_lambda() {
    let source = "fun run() {
    clean(1, 2)
    only { a } }
}
";
    assert_calls(
        "kotlin",
        "sample.kt",
        source,
        &[("clean", Some(2), false), ("only", Some(1), false)],
    );

    let source = "fun run() {
    clean(1, 2)
    only { a, b -> a }{
}
";
    assert_calls(
        "kotlin",
        "sample.kt",
        source,
        &[("clean", Some(2), false), ("only", Some(1), false)],
    );

    let source = "fun run() {
    chain { a } { b }
    withArgs(1) { a } { b }
}
";
    assert_calls(
        "kotlin",
        "sample.kt",
        source,
        &[("chain", Some(1), false), ("withArgs", Some(2), false)],
    );
}

#[test]
fn javascript_family_calls_with_an_empty_argument_slot_carry_no_argument_keys() {
    let source = "function run(a, b, c) {
  leading(,a, b, c);
  doubled(a,, b, c);
  spaced(a, ,b, c);
  lone(,);
  trailing(a, b,);
}
";
    for (language, path) in [
        ("javascript", "sample.js"),
        ("typescript", "sample.ts"),
        ("tsx", "sample.tsx"),
    ] {
        assert_calls(
            language,
            path,
            source,
            &[
                ("leading", None, false),
                ("doubled", None, false),
                ("spaced", None, false),
                ("lone", None, false),
                ("trailing", Some(2), false),
            ],
        );
    }
}

#[test]
fn python_and_rust_calls_with_an_empty_argument_slot_carry_no_argument_keys() {
    assert_calls(
        "python",
        "sample.py",
        "def run(a, b):\n    lone(,)\n    trailing(a, b,)\n",
        &[("lone", None, false), ("trailing", Some(2), false)],
    );
    assert_calls(
        "rust",
        "sample.rs",
        "fn run(a: i32) {\n    lone(,);\n    trailing(a, a,);\n}\n",
        &[("lone", None, false), ("trailing", Some(2), false)],
    );
}

#[test]
fn comments_inside_type_arguments_keep_their_counts() {
    assert_calls(
        "typescript",
        "sample.ts",
        "function run(x: any) {
  blockComment<() => void /* > */>(x);
  lineComment<() => void // >
  >(x, x);
}
",
        &[
            ("blockComment", Some(1), false),
            ("lineComment", Some(2), false),
        ],
    );
    assert_calls(
        "cpp",
        "sample.cpp",
        "void run() {
    blockComment<A /* > */>(1);
    lineComment<A // >
    >(1, 2);
}
",
        &[
            ("blockComment", Some(1), false),
            ("lineComment", Some(2), false),
        ],
    );
}

#[test]
fn kotlin_class_delegation_calls_never_count_the_class_body_as_a_lambda() {
    let source = "interface I
class First : I by firstDelegate(1, 2) { }
class Second : I by secondDelegate(1)
";
    assert_calls(
        "kotlin",
        "sample.kt",
        source,
        &[
            ("firstDelegate", Some(2), false),
            ("secondDelegate", Some(1), false),
        ],
    );
    assert_calls(
        "kotlin",
        "sample.kt",
        "class Only : I by onlyDelegate(1, 2) { }\n",
        &[("onlyDelegate", None, false)],
    );
}

const TYPESCRIPT_COMMA_BETWEEN_ANGLE_LISTS: &[&str] = &[
    "a < b, c > -d",
    "a < b, c > +d",
    "a < b, c >> d",
    "a < b, c >>> d",
    "a < b, c >= d",
    "a < b, c >>= d",
    "a < b, c > [d]",
    "a < b, c > `s`",
    "a < b, c > d",
    "a < b, c > !d",
    "a < b, c > .5",
    "a < b, c > {}",
    "a < b, c > d as T",
    "a < b, c > is",
    "a < b, c >",
    "t!<A, B>`s`, c",
    "a! < b, c > [d], e",
    "x, a < b, c > -d, x",
    "a < b, c > -d, a < b, c > +d",
    "a < b, c >\n-d",
    "t<A, B>`s`",
    "x, t<A, B>`s`, x",
    "t<A, B>(x) + 1, t<A, B>`s`",
    "t<A, B>(x).y, a < b, c >= d",
    "a < b, c > (d)",
    "x, a < b, c > (d)",
    "a < b, c > (d), x",
    "a < b, c || d > (e)",
    "x++ < b, c > (d)",
    "a < b, super.b > (d)",
    "/\\//, /* c */ t<A, B>(x)",
    "t<A, B>(x), x",
    "t<A, B>(x), t<A, B>(x)",
    "t<A, B>(x), x, t<A, B<C>>(x)",
    "t<A, B>(x), a < b",
    "innerPlus<A, B>(x) + 1, 2",
    "innerAnd<A, B>(x) && x, x",
    "innerThree<A, B, C>(1) + 2, x",
    "innerNested<A, B<C>>(x) + 1, 2",
    "innerLess<A, B>(1) < 2, x",
    "memberCall<A, B>(x).y, 2",
    "generic<A, B>(1)",
    "x, new Map<A, B>()",
    "x as Map<A, B>, y",
];

const ANGLE_FREE_CONTROLS: &[(&str, u64)] = &[
    ("a, b", 2),
    ("x < y, z", 2),
    ("a > b, c < d", 2),
    ("g<A>(x), y", 2),
    ("(a < b), c > d", 2),
    ("\"<\", \",\", \">\"", 3),
    ("a < b, c < d, x", 3),
    ("g<A>(x), h<B>(y)", 2),
    ("g<A>(x), h<B>(y), k<C>(z)", 3),
    ("g<A<B>>(x), h<C>(y)", 2),
    ("g<A>(x), h<B<C>>(y)", 2),
    ("x, g<A>(x), y, h<B>(y)", 4),
    ("a <= b, c > d", 2),
    ("", 0),
];

#[test]
fn typescript_argument_lists_with_a_comma_between_angle_tokens_carry_no_argument_keys() {
    assert_argument_lists_carry_no_keys(
        "typescript",
        "sample.ts",
        TYPESCRIPT_COMMA_BETWEEN_ANGLE_LISTS,
        in_typescript_function,
    );
}

#[test]
fn tsx_argument_lists_with_a_comma_between_angle_tokens_carry_no_argument_keys() {
    assert_argument_lists_carry_no_keys(
        "tsx",
        "sample.tsx",
        TYPESCRIPT_COMMA_BETWEEN_ANGLE_LISTS,
        in_typescript_function,
    );
    assert_argument_lists_carry_no_keys(
        "tsx",
        "sample.tsx",
        &[
            "<p>it's</p>, /* c */ t<A, B>(x), <p>it's</p>",
            "<p>it's</p>, a < b, c > (d)",
        ],
        in_typescript_function,
    );
}

#[test]
fn csharp_argument_lists_with_a_comma_between_angle_tokens_carry_no_argument_keys() {
    assert_argument_lists_carry_no_keys(
        "csharp",
        "sample.cs",
        &[
            "a < b, c > -d",
            "a < b, c > +d",
            "a < b, c >> d",
            "a < b, c >= d",
            "a < b, c >>= d",
            "a < b, c > d",
            "a < b, c > !d",
            "a < b, c > .5",
            "a < b, c > d is Z",
            "a < b, c > global::d",
            "a < b, c > d as Z",
            "a < b, c >",
            "t<A= B<C, D>>(x), x",
            "t<A+ B<C, D>>(x), x",
            "t<A- B<C, D>>(x), x",
            "t<A.. B<C, D>>(x), x",
            "x is Dictionary<Foo, Bar> d3, x",
            "x is KeyValuePair<Foo, Bar> d6, x",
            "x is Dictionary<Foo, Bar> d3",
            "x, a < b, c > -d, x",
            "a < b, c > -d, a < b, c > +d",
            "a < b, c >\n-d",
            "x is Dictionary<Foo, Bar> d3, x is Dictionary<Foo, Bar> d4",
            "out Dictionary<Foo, Bar> d5",
            "x < this.Max, y > (z)",
            "i < arr[0], j > (k)",
            "x < null, y > (z)",
            "@\"C:\\dir\\\", a < b, c > (d)",
            "a < b, c > (d)",
            "t<Foo, Bar>(x), x",
            "new Dictionary<Foo, Bar>(), a < b, c > (d)",
            "a < b, new Dictionary<Foo, Bar>(), c > (d)",
            "t<Foo, Bar>(x), x, t<Foo, Dictionary<Foo, Bar>>(x)",
            "innerPlus<A, B>(x) + 1, 2",
            "memberCall<A, B>(x).y, 2",
            "x, indexCall<A, B>(x)[0]",
            "x, out Dictionary<A, B> map",
            "a < b, c > b.c",
        ],
        in_csharp_method,
    );
}

#[test]
fn cpp_argument_lists_with_a_comma_between_angle_tokens_carry_no_argument_keys() {
    assert_argument_lists_carry_no_keys(
        "cpp",
        "sample.cpp",
        &[
            "a < b, c > (d)",
            "a < b, c > -d",
            "a < b, c > +d",
            "a < b, c >> d",
            "a < b, c >= d",
            "a < b, c > !d",
            "a < b, c > .5",
            "a < b, c > ::d",
            "a<b,c>(d)",
            "x, a < b, c > (d), x",
            "a < b, c > (d), a < b, c > (d)",
            "a < b, c >\n(d)",
            "t<A, B>(x)",
            "x, t<A, B>(x), x",
            "t<A, B<C>, D>(x)",
            "a < b, u<A>(x)",
            "a < b, static_cast<long>(x)",
            "a < b, c, u<A>(x)",
            "R\"(,)\", a < b, c > (d), \"x\"",
            "t1<A, B<C>, D>(1) + 2",
            "t2<A, B<C>, D>::value",
            "t3<A, B<C>, D>(1).foo()",
            "template_call<A, B<C>, D>(1)",
            "template_call2<A, B>(1)",
        ],
        in_cpp_function,
    );
}

#[test]
fn kotlin_argument_lists_with_a_comma_between_angle_tokens_carry_no_argument_keys() {
    assert_argument_lists_carry_no_keys(
        "kotlin",
        "sample.kt",
        &[
            "f(x) < b, c > (d)",
            "a < b, c > (d)",
            "a < b, x > b",
            "innerSingle<String, Int>(16)",
            "innerFirst<String, Int>(16), x",
            "x, innerSecond<String, Int>(16)",
            "HashMap<String, Int>(16)",
            "Triple<A, B, C>(x)",
            "Pair<A, B>(x).first",
            "mapOf<String, Int>()",
        ],
        in_kotlin_function,
    );
}

#[test]
fn c_header_argument_lists_with_a_comma_between_angle_tokens_carry_no_argument_keys() {
    assert_argument_lists_carry_no_keys(
        "c",
        "sample.h",
        &[
            "t<A, B>(x), y",
            "a < b, c > (d)",
            "a < b, c > d",
            "x, a < b, c >> d",
            "t<A, B<C>, D>(x)",
        ],
        in_c_function,
    );
}

#[test]
fn c_and_cpp_macro_arguments_count_as_written() {
    let source = "#define ARGS a, b
void run(long a, long b, long c) {
    object_like(ARGS);
    object_like_then_c(ARGS, c);
}
";
    for (language, path) in [("c", "sample.c"), ("cpp", "sample.cpp")] {
        assert_calls(
            language,
            path,
            source,
            &[
                ("object_like", Some(1), false),
                ("object_like_then_c", Some(2), false),
            ],
        );
    }
}

#[test]
fn csharp_argument_lists_that_hold_a_preprocessor_directive_carry_no_argument_keys() {
    assert_each_argument_list_carries_no_key(
        "csharp",
        "sample.cs",
        &[
            "a,\n#if X\nb\n#endif\n",
            "a,\n#if false\nc,\n#endif\nb",
            "a\n#if X\n, b\n#endif\n",
            "a,\n#if X\nb,\n#else\nc, d,\n#endif\ne",
            "a,\n#if X\nb,\n#elif Y\nc,\n#endif\nd",
            "#if X\na\n#else\nb\n#endif\n, c",
            "a,\n#region r\nb,\n#endregion\nc",
            "a,\n#pragma warning disable CS0618\nb",
            "a,\n#nullable enable\nb",
            "a,\n#line 10\nb",
            "a,\n#warning w\nb",
            "a,\n#error e\nb",
            "a,\n#define FOO\nb",
            "a,\n#undef FOO\nb",
            "log.Contains(\"x\"\n#if NETCOREAPP\n, StringComparison.Ordinal\n#endif\n)",
            "@\"x\n#if X\ny\", b",
            "\"\"\"\n#if X\n\"\"\", b",
            "$@\"{a}\n#if X\n\", b",
            "$\"\"\"\n#if X {a}\n\"\"\", b",
            "a, /*\n#if X\n*/ b",
            "@\"a\n#region\n#pragma\n\", b",
            "@\"\n#if DEBUG\ndebug\", @\"\n#else\nrelease\", @\"\n#endif\nend\"",
            "@\"\n#if A\na\", b, @\"\n#elif B\nc\", d, @\"\n#endif\n\"",
            "a, /*\n#if X\n*/ b, /*\n#else\n*/ c, /*\n#endif\n*/ d",
        ],
        in_csharp_method,
    );
}

#[test]
fn csharp_directive_inside_a_nested_call_removes_the_key_of_every_enclosing_list() {
    let source = "class Z { void Run() {
    outer(inner(a,
#if X
        b,
#endif
        c), d);
    clean(a, b);
} }
";
    assert_calls(
        "csharp",
        "sample.cs",
        source,
        &[
            ("outer", None, false),
            ("inner", None, false),
            ("clean", Some(2), false),
        ],
    );
}

#[test]
fn csharp_hash_text_that_does_not_start_a_line_keeps_the_count() {
    assert_argument_lists_have_counts(
        "csharp",
        "sample.cs",
        &[
            ("a, // #if X\nb", 2),
            ("\"#if X\", b", 2),
            ("a, /* #else */ b", 2),
        ],
        in_csharp_method,
    );
}

#[test]
fn csharp_directive_between_the_callee_and_its_argument_list_removes_the_key() {
    let source = "class Z { void Run() {
    q
#if X
        (a, b)
#else
        (a)
#endif
        ;
    var o = new Q2
#if X
        (a, b)
#else
        (a)
#endif
        ;
    clean(a, b);
} }
";
    assert_calls(
        "csharp",
        "sample.cs",
        source,
        &[
            ("q", None, false),
            ("Q2", None, false),
            ("clean", Some(2), false),
        ],
    );
}

#[test]
fn csharp_object_creation_types_never_count_as_angle_brackets() {
    assert_argument_lists_have_counts(
        "csharp",
        "sample.cs",
        &[
            ("new KeyValuePair<string, string>(a, b)", 1),
            ("new Dictionary<string, X>(a)", 1),
            ("x, new Dictionary<string, X>(), y", 3),
            (
                "new KeyValuePair<A, B>(a, b), new KeyValuePair<C, D>(c, d)",
                2,
            ),
            ("new KeyValuePair<A, B>(new KeyValuePair<C, D>(a, b), c)", 1),
            ("new T<A, B>[3]", 1),
            ("new T<A, B>[] { x, y }", 1),
            ("new T<A, B> { x, y }", 1),
            ("new T<A, B>(1) { P = 1 }", 1),
            ("new a < b, c > (d)", 1),
            ("new a < b, c > (d), e", 2),
            ("new Dictionary<A, B>(), x", 2),
            ("Arg.Any<T>(), Arg.Any<U>()", 2),
        ],
        in_csharp_method,
    );
}

#[test]
fn argument_lists_with_operators_next_to_angle_tokens_follow_the_depth_rule() {
    let ambiguous = [
        "a <= b, c < d, e > (f)",
        "a >= b < c, d > (e)",
        "a < b >= c, d < e, f > (g)",
        "g<A>(x), h<B, C>(y)",
        "g<A, B>(x), h<C>(y)",
        "g<A>(x), a < b, c > (d)",
        "a < b, c > (d), g<A>(x)",
    ];
    let ambiguous_with_shifts = [
        &ambiguous[..],
        &["a << b, c < d, e > (f)", "a >>= b < c, d > (e)"],
    ]
    .concat();
    assert_argument_lists_carry_no_keys("kotlin", "sample.kt", &ambiguous, in_kotlin_function);
    for (language, path, wrap) in [
        (
            "typescript",
            "sample.ts",
            in_typescript_function as WrapStatements,
        ),
        ("tsx", "sample.tsx", in_typescript_function),
        ("csharp", "sample.cs", in_csharp_method),
        ("cpp", "sample.cpp", in_cpp_function),
        ("c", "sample.h", in_c_function),
    ] {
        assert_argument_lists_carry_no_keys(language, path, &ambiguous_with_shifts, wrap);
    }
    let shifts_after_the_angle: &[(&str, u64)] =
        &[("a < b >> c, d > (e)", 2), ("a < b >>> c, d > (e)", 2)];
    assert_argument_lists_have_counts(
        "csharp",
        "sample.cs",
        &[shifts_after_the_angle, &[("a << b, c > d", 2)]].concat(),
        in_csharp_method,
    );
    assert_argument_lists_have_counts(
        "cpp",
        "sample.cpp",
        &[("a < b >> c, d > (e)", 2), ("a << b, c > d", 2)],
        in_cpp_function,
    );
    assert_argument_lists_have_counts(
        "typescript",
        "sample.ts",
        shifts_after_the_angle,
        in_typescript_function,
    );
    for (language, path) in [("typescript", "sample.ts"), ("tsx", "sample.tsx")] {
        assert_argument_lists_carry_no_keys(
            language,
            path,
            &["a << b, c > d", "f<<T>(x: T) => void, B>(y)"],
            in_typescript_function,
        );
    }
}

#[test]
fn argument_lists_without_a_comma_between_angle_tokens_keep_their_counts() {
    let lambda_controls: &[(&str, u64)] = &[("x => x < 1, y", 2), ("x => x > 1, y < 2", 2)];
    let controls = |extra: &[(&'static str, u64)]| [ANGLE_FREE_CONTROLS, extra].concat();
    let trailing_comma: &[(&str, u64)] = &[("a, b,", 2)];
    assert_argument_lists_have_counts(
        "typescript",
        "sample.ts",
        &controls(&[lambda_controls, trailing_comma].concat()),
        in_typescript_function,
    );
    assert_argument_lists_have_counts(
        "tsx",
        "sample.tsx",
        &controls(&[lambda_controls, &[("<p>a</p>, <p>b</p>", 2)]].concat()),
        in_typescript_function,
    );
    assert_argument_lists_have_counts(
        "csharp",
        "sample.cs",
        &controls(lambda_controls),
        in_csharp_method,
    );
    assert_argument_lists_have_counts("cpp", "sample.cpp", &controls(&[]), in_cpp_function);
    assert_argument_lists_have_counts("c", "sample.h", &controls(&[]), in_c_function);
    assert_argument_lists_have_counts(
        "kotlin",
        "sample.kt",
        &controls(trailing_comma),
        in_kotlin_function,
    );
}

#[test]
fn argument_lists_with_a_top_level_comma_the_tree_does_not_split_carry_no_argument_keys() {
    assert_argument_lists_carry_no_keys(
        "typescript",
        "sample.ts",
        &[
            "class implements B, C {}",
            "class extends A implements B, C {}",
        ],
        in_typescript_function,
    );
    assert_argument_lists_carry_no_keys(
        "kotlin",
        "sample.kt",
        &["object : A, B { }", "x, object : A(), B { }"],
        in_kotlin_function,
    );
}

#[test]
fn kotlin_trailing_lambdas_keep_their_counts_next_to_comma_checks() {
    let source = "fun run(a: Int, b: Int, c: Int, d: Int) {
    ambiguous(a < b, c > (d)) { e }
    withLambda(a, b) { p, q -> p }
    comparing(a < b, c) { e }
    onlyLambda { p, q -> p }
}
";
    assert_calls(
        "kotlin",
        "sample.kt",
        source,
        &[
            ("ambiguous", None, false),
            ("withLambda", Some(3), false),
            ("comparing", Some(3), false),
            ("onlyLambda", Some(1), false),
        ],
    );
}

#[test]
fn embedded_typescript_follows_the_angle_rule_and_embedded_javascript_does_not() {
    let vue = "<script setup lang=\"ts\">
ambiguous(a < b, c > (d));
twoPlain(a, b);
</script>
";
    assert_calls(
        "vue",
        "App.vue",
        vue,
        &[("ambiguous", None, false), ("twoPlain", Some(2), false)],
    );
    let html = "<html><body><script>
ambiguous(a < b, c > (d));
twoPlain(a, b);
</script></body></html>
";
    assert_calls(
        "html",
        "index.html",
        html,
        &[("ambiguous", Some(2), false), ("twoPlain", Some(2), false)],
    );
    assert_calls(
        "javascript",
        "sample.js",
        "function run(a, b, c, d) { ambiguous(a < b, c > (d)); }\n",
        &[("ambiguous", Some(2), false)],
    );
}

#[test]
fn type_arguments_before_the_argument_list_keep_the_count_across_literals_and_nesting() {
    assert_calls(
        "cpp",
        "sample.cpp",
        "void run() {
    rawString<R\"x(>)x\">(1);
    nested<A<B<C>>>(1, 2);
}
",
        &[("rawString", Some(1), false), ("nested", Some(2), false)],
    );
    assert_calls(
        "typescript",
        "sample.ts",
        "function run(cb: any) {
  nestedTemplate<`a${`b${string}>`}>`>(cb);
  regexLikeUnion<'/' | '/'>(cb, 1);
  deep<A<B<C<D>>>>(1, 2, 3);
}
",
        &[
            ("nestedTemplate", Some(1), false),
            ("regexLikeUnion", Some(2), false),
            ("deep", Some(3), false),
        ],
    );
    assert_calls(
        "csharp",
        "sample.cs",
        "class K { void Run() {
    nested<List<int>>(1, 2);
} }
",
        &[("nested", Some(2), false)],
    );
    assert_calls(
        "kotlin",
        "sample.kt",
        "fun run() {
    nested<List<Int>>(1, 2)
}
",
        &[("nested", Some(2), false)],
    );
}

#[test]
fn comparisons_without_a_closing_angle_keep_their_counts() {
    let comparisons = "q(a < b, c < d, x);";
    assert_eq!(
        argument_counts_of(
            "typescript",
            "sample.ts",
            &in_typescript_function(comparisons),
            "q"
        ),
        [Some(3)]
    );
    assert_eq!(
        argument_counts_of("csharp", "sample.cs", &in_csharp_method(comparisons), "q"),
        [Some(3)]
    );
    assert_eq!(
        argument_counts_of("cpp", "sample.cpp", &in_cpp_function(comparisons), "q"),
        [Some(3)]
    );
}

fn timed_argument_counts_of(
    language: &str,
    path: &str,
    source: &str,
) -> (Vec<Option<u64>>, Duration) {
    let started = Instant::now();
    let keys = argument_counts_of(language, path, source, "q");
    (keys, started.elapsed())
}

type WrapStatements = fn(&str) -> String;

const ANGLE_RULE_LANGUAGES: [(&str, &str, WrapStatements); 4] = [
    ("typescript", "sample.ts", in_typescript_function),
    ("csharp", "sample.cs", in_csharp_method),
    ("cpp", "sample.cpp", in_cpp_function),
    ("kotlin", "sample.kt", in_kotlin_function),
];

fn assert_long_comparison_list_stays_fast(language: &str, path: &str, wrap: fn(&str) -> String) {
    let comparisons: Vec<_> = (0..20_000).map(|i| format!("a{i} < b{i}")).collect();
    let call = format!("q({});", comparisons.join(", "));
    let (keys, elapsed) = timed_argument_counts_of(language, path, &wrap(&call));
    assert_eq!(keys, [Some(20_000)], "{language}: 20000 comparisons");
    assert!(
        elapsed < Duration::from_secs(10),
        "{language}: {elapsed:?} for one call with 20000 comparisons"
    );
}

#[test]
fn typescript_call_with_twenty_thousand_comparisons_stays_fast() {
    assert_long_comparison_list_stays_fast("typescript", "sample.ts", in_typescript_function);
}

#[test]
fn csharp_call_with_twenty_thousand_comparisons_stays_fast() {
    assert_long_comparison_list_stays_fast("csharp", "sample.cs", in_csharp_method);
}

#[test]
fn cpp_call_with_twenty_thousand_comparisons_stays_fast() {
    assert_long_comparison_list_stays_fast("cpp", "sample.cpp", in_cpp_function);
}

#[test]
fn kotlin_call_with_twenty_thousand_comparisons_stays_fast() {
    assert_long_comparison_list_stays_fast("kotlin", "sample.kt", in_kotlin_function);
}

#[test]
fn nested_calls_keep_their_counts() {
    let depth = 100;
    let nested = format!("{}1{};", "q(".repeat(depth), ")".repeat(depth));
    for (language, path, wrap) in ANGLE_RULE_LANGUAGES {
        let (keys, elapsed) = timed_argument_counts_of(language, path, &wrap(&nested));
        assert_eq!(keys, vec![Some(1); depth], "{language}: nested calls");
        assert!(
            elapsed < Duration::from_secs(10),
            "{language}: {elapsed:?} for calls nested {depth} deep"
        );
    }
}
