use crate::base::IdentifierKind;

#[test]
fn go_named_return_type_has_exact_identifier_span() {
    let source = include_str!("../../../../fixtures/extraction/go/return_types/source.go");
    let result =
        crate::pipeline::extract_canonical("go/source.go", source, std::path::Path::new("/repo"))
            .unwrap();
    let rows: Vec<_> = result
        .identifiers
        .iter()
        .filter(|id| {
            id.name == "Result" && id.start_byte == 54 && id.kind == IdentifierKind::TypeUsage
        })
        .collect();
    assert_eq!(
        rows.len(),
        1,
        "{:?} AST:{}",
        result.identifiers,
        crate::tests::helpers::init_parser(source, "go")
            .root_node()
            .to_sexp()
    );
    assert_eq!(rows[0].end_byte, 60);
}

#[test]
fn zig_named_return_type_has_exact_identifier_span() {
    let source = include_str!("../../../../fixtures/extraction/zig/return_types/source.zig");
    let result =
        crate::pipeline::extract_canonical("zig/source.zig", source, std::path::Path::new("/repo"))
            .unwrap();
    let rows: Vec<_> = result
        .identifiers
        .iter()
        .filter(|id| {
            id.name == "Result" && id.start_byte == 54 && id.kind == IdentifierKind::TypeUsage
        })
        .collect();
    assert_eq!(
        rows.len(),
        1,
        "{:?} AST:{}",
        result.identifiers,
        crate::tests::helpers::init_parser(source, "zig")
            .root_node()
            .to_sexp()
    );
    assert_eq!(rows[0].end_byte, 60);
}

#[test]
fn vbnet_named_return_type_has_exact_identifier_span() {
    let source = include_str!("../../../../fixtures/extraction/vbnet/return_types/source.vb");
    let result = crate::pipeline::extract_canonical(
        "vbnet/source.vb",
        source,
        std::path::Path::new("/repo"),
    )
    .unwrap();
    let rows: Vec<_> = result
        .identifiers
        .iter()
        .filter(|id| {
            id.name == "Result" && id.start_byte == 78 && id.kind == IdentifierKind::TypeUsage
        })
        .collect();
    assert_eq!(
        rows.len(),
        1,
        "{:?} AST:{}",
        result.identifiers,
        crate::tests::helpers::init_parser(source, "vbnet")
            .root_node()
            .to_sexp()
    );
    assert_eq!(rows[0].end_byte, 84);
}

#[test]
fn elixir_named_return_type_has_exact_identifier_span() {
    let source = include_str!("../../../../fixtures/extraction/elixir/return_types/source.ex");
    let result = crate::pipeline::extract_canonical(
        "elixir/source.ex",
        source,
        std::path::Path::new("/repo"),
    )
    .unwrap();
    let rows: Vec<_> = result
        .identifiers
        .iter()
        .filter(|id| {
            id.name == "result" && id.start_byte == 67 && id.kind == IdentifierKind::TypeUsage
        })
        .collect();
    assert_eq!(
        rows.len(),
        1,
        "{:?} AST:{}",
        result.identifiers,
        crate::tests::helpers::init_parser(source, "elixir")
            .root_node()
            .to_sexp()
    );
    assert_eq!(rows[0].end_byte, 73);
}

#[test]
fn erlang_named_return_type_has_exact_identifier_span() {
    let source = include_str!("../../../../fixtures/extraction/erlang/return_types/source.erl");
    let result = crate::pipeline::extract_canonical(
        "erlang/source.erl",
        source,
        std::path::Path::new("/repo"),
    )
    .unwrap();
    let rows: Vec<_> = result
        .identifiers
        .iter()
        .filter(|id| {
            id.name == "result" && id.start_byte == 63 && id.kind == IdentifierKind::TypeUsage
        })
        .collect();
    assert_eq!(
        rows.len(),
        1,
        "{:?} AST:{}",
        result.identifiers,
        crate::tests::helpers::init_parser(source, "erlang")
            .root_node()
            .to_sexp()
    );
    assert_eq!(rows[0].end_byte, 69);
}

#[test]
fn sql_named_return_type_has_exact_identifier_span() {
    let source = include_str!("../../../../fixtures/extraction/sql/return_types/source.sql");
    let result =
        crate::pipeline::extract_canonical("sql/source.sql", source, std::path::Path::new("/repo"))
            .unwrap();
    let rows: Vec<_> = result
        .identifiers
        .iter()
        .filter(|id| {
            id.name == "result" && id.start_byte == 62 && id.kind == IdentifierKind::TypeUsage
        })
        .collect();
    assert_eq!(
        rows.len(),
        1,
        "{:?} AST:{}",
        result.identifiers,
        crate::tests::helpers::init_parser(source, "sql")
            .root_node()
            .to_sexp()
    );
    assert_eq!(rows[0].end_byte, 68);
}

#[test]
fn rust_named_return_type_has_exact_identifier_span() {
    let source = include_str!("../../../../fixtures/extraction/rust/return_types/source.rs");
    let result =
        crate::pipeline::extract_canonical("rust/source.rs", source, std::path::Path::new("/repo"))
            .unwrap();
    let rows: Vec<_> = result
        .identifiers
        .iter()
        .filter(|id| {
            id.name == "Result" && id.start_byte == 28 && id.kind == IdentifierKind::TypeUsage
        })
        .collect();
    assert_eq!(rows.len(), 1, "{:?}", result.identifiers);
    assert_eq!(rows[0].end_byte, 34);
}

#[test]
fn c_named_return_type_has_exact_identifier_span() {
    let source = include_str!("../../../../fixtures/extraction/c/return_types/source.c");
    let result =
        crate::pipeline::extract_canonical("c/source.c", source, std::path::Path::new("/repo"))
            .unwrap();
    let rows: Vec<_> = result
        .identifiers
        .iter()
        .filter(|id| {
            id.name == "Result" && id.start_byte == 41 && id.kind == IdentifierKind::TypeUsage
        })
        .collect();
    assert_eq!(rows.len(), 1, "{:?}", result.identifiers);
    assert_eq!(rows[0].end_byte, 47);
}

#[test]
fn cpp_named_return_type_has_exact_identifier_span() {
    let source = include_str!("../../../../fixtures/extraction/cpp/return_types/source.cpp");
    let result =
        crate::pipeline::extract_canonical("cpp/source.cpp", source, std::path::Path::new("/repo"))
            .unwrap();
    let rows: Vec<_> = result
        .identifiers
        .iter()
        .filter(|id| {
            id.name == "Result" && id.start_byte == 17 && id.kind == IdentifierKind::TypeUsage
        })
        .collect();
    assert_eq!(rows.len(), 1, "{:?}", result.identifiers);
    assert_eq!(rows[0].end_byte, 23);
}

#[test]
fn typescript_named_return_type_has_exact_identifier_span() {
    let source = include_str!("../../../../fixtures/extraction/typescript/return_types/source.ts");
    let result = crate::pipeline::extract_canonical(
        "typescript/source.ts",
        source,
        std::path::Path::new("/repo"),
    )
    .unwrap();
    let rows: Vec<_> = result
        .identifiers
        .iter()
        .filter(|id| {
            id.name == "Result" && id.start_byte == 35 && id.kind == IdentifierKind::TypeUsage
        })
        .collect();
    assert_eq!(rows.len(), 1, "{:?}", result.identifiers);
    assert_eq!(rows[0].end_byte, 41);
}

#[test]
fn tsx_named_return_type_has_exact_identifier_span() {
    let source = include_str!("../../../../fixtures/extraction/tsx/return_types/source.tsx");
    let result =
        crate::pipeline::extract_canonical("tsx/source.tsx", source, std::path::Path::new("/repo"))
            .unwrap();
    let rows: Vec<_> = result
        .identifiers
        .iter()
        .filter(|id| {
            id.name == "Result" && id.start_byte == 35 && id.kind == IdentifierKind::TypeUsage
        })
        .collect();
    assert_eq!(rows.len(), 1, "{:?}", result.identifiers);
    assert_eq!(rows[0].end_byte, 41);
}

#[test]
fn vue_named_return_type_has_exact_identifier_span() {
    let source = include_str!("../../../../fixtures/extraction/vue/return_types/source.vue");
    let result =
        crate::pipeline::extract_canonical("vue/source.vue", source, std::path::Path::new("/repo"))
            .unwrap();
    let rows: Vec<_> = result
        .identifiers
        .iter()
        .filter(|id| {
            id.name == "Result" && id.start_byte == 54 && id.kind == IdentifierKind::TypeUsage
        })
        .collect();
    assert_eq!(rows.len(), 1, "{:?}", result.identifiers);
    assert_eq!(rows[0].end_byte, 60);
}

#[test]
fn python_named_return_type_has_exact_identifier_span() {
    let source = include_str!("../../../../fixtures/extraction/python/return_types/source.py");
    let result = crate::pipeline::extract_canonical(
        "python/source.py",
        source,
        std::path::Path::new("/repo"),
    )
    .unwrap();
    let rows: Vec<_> = result
        .identifiers
        .iter()
        .filter(|id| {
            id.name == "Result" && id.start_byte == 33 && id.kind == IdentifierKind::TypeUsage
        })
        .collect();
    assert_eq!(rows.len(), 1, "{:?}", result.identifiers);
    assert_eq!(rows[0].end_byte, 39);
}

#[test]
fn java_named_return_type_has_exact_identifier_span() {
    let source = include_str!("../../../../fixtures/extraction/java/return_types/source.java");
    let result = crate::pipeline::extract_canonical(
        "java/source.java",
        source,
        std::path::Path::new("/repo"),
    )
    .unwrap();
    let rows: Vec<_> = result
        .identifiers
        .iter()
        .filter(|id| {
            id.name == "Result" && id.start_byte == 32 && id.kind == IdentifierKind::TypeUsage
        })
        .collect();
    assert_eq!(rows.len(), 1, "{:?}", result.identifiers);
    assert_eq!(rows[0].end_byte, 38);
}

#[test]
fn php_named_return_type_has_exact_identifier_span() {
    let source = include_str!("../../../../fixtures/extraction/php/return_types/source.php");
    let result =
        crate::pipeline::extract_canonical("php/source.php", source, std::path::Path::new("/repo"))
            .unwrap();
    let rows: Vec<_> = result
        .identifiers
        .iter()
        .filter(|id| {
            id.name == "Result" && id.start_byte == 39 && id.kind == IdentifierKind::TypeUsage
        })
        .collect();
    assert_eq!(rows.len(), 1, "{:?}", result.identifiers);
    assert_eq!(rows[0].end_byte, 45);
}

#[test]
fn swift_named_return_type_has_exact_identifier_span() {
    let source = include_str!("../../../../fixtures/extraction/swift/return_types/source.swift");
    let result = crate::pipeline::extract_canonical(
        "swift/source.swift",
        source,
        std::path::Path::new("/repo"),
    )
    .unwrap();
    let rows: Vec<_> = result
        .identifiers
        .iter()
        .filter(|id| {
            id.name == "Result" && id.start_byte == 31 && id.kind == IdentifierKind::TypeUsage
        })
        .collect();
    assert_eq!(rows.len(), 1, "{:?}", result.identifiers);
    assert_eq!(rows[0].end_byte, 37);
}

#[test]
fn kotlin_named_return_type_has_exact_identifier_span() {
    let source = include_str!("../../../../fixtures/extraction/kotlin/return_types/source.kt");
    let result = crate::pipeline::extract_canonical(
        "kotlin/source.kt",
        source,
        std::path::Path::new("/repo"),
    )
    .unwrap();
    let rows: Vec<_> = result
        .identifiers
        .iter()
        .filter(|id| {
            id.name == "Result" && id.start_byte == 25 && id.kind == IdentifierKind::TypeUsage
        })
        .collect();
    assert_eq!(rows.len(), 1, "{:?}", result.identifiers);
    assert_eq!(rows[0].end_byte, 31);
}

#[test]
fn scala_named_return_type_has_exact_identifier_span() {
    let source = include_str!("../../../../fixtures/extraction/scala/return_types/source.scala");
    let result = crate::pipeline::extract_canonical(
        "scala/source.scala",
        source,
        std::path::Path::new("/repo"),
    )
    .unwrap();
    let rows: Vec<_> = result
        .identifiers
        .iter()
        .filter(|id| {
            id.name == "Result" && id.start_byte == 42 && id.kind == IdentifierKind::TypeUsage
        })
        .collect();
    assert_eq!(rows.len(), 1, "{:?}", result.identifiers);
    assert_eq!(rows[0].end_byte, 48);
}

#[test]
fn dart_named_return_type_has_exact_identifier_span() {
    let source = include_str!("../../../../fixtures/extraction/dart/return_types/source.dart");
    let result = crate::pipeline::extract_canonical(
        "dart/source.dart",
        source,
        std::path::Path::new("/repo"),
    )
    .unwrap();
    let rows: Vec<_> = result
        .identifiers
        .iter()
        .filter(|id| {
            id.name == "Result" && id.start_byte == 16 && id.kind == IdentifierKind::TypeUsage
        })
        .collect();
    assert_eq!(rows.len(), 1, "{:?}", result.identifiers);
    assert_eq!(rows[0].end_byte, 22);
}

#[test]
fn fsharp_named_return_type_has_exact_identifier_span() {
    let source = include_str!("../../../../fixtures/extraction/fsharp/return_types/source.fs");
    let result = crate::pipeline::extract_canonical(
        "fsharp/source.fs",
        source,
        std::path::Path::new("/repo"),
    )
    .unwrap();
    let rows: Vec<_> = result
        .identifiers
        .iter()
        .filter(|id| {
            id.name == "Result" && id.start_byte == 39 && id.kind == IdentifierKind::TypeUsage
        })
        .collect();
    assert_eq!(rows.len(), 1, "{:?}", result.identifiers);
    assert_eq!(rows[0].end_byte, 45);
}

#[test]
fn qml_named_return_type_has_exact_identifier_span() {
    let source = include_str!("../../../../fixtures/extraction/qml/return_types/source.qml");
    let result =
        crate::pipeline::extract_canonical("qml/source.qml", source, std::path::Path::new("/repo"))
            .unwrap();
    let rows: Vec<_> = result
        .identifiers
        .iter()
        .filter(|id| {
            id.name == "Result" && id.start_byte == 43 && id.kind == IdentifierKind::TypeUsage
        })
        .collect();
    assert_eq!(rows.len(), 1, "{:?}", result.identifiers);
    assert_eq!(rows[0].end_byte, 49);
}

#[test]
fn gdscript_named_return_type_has_exact_identifier_span() {
    let source = include_str!("../../../../fixtures/extraction/gdscript/return_types/source.gd");
    let result = crate::pipeline::extract_canonical(
        "gdscript/source.gd",
        source,
        std::path::Path::new("/repo"),
    )
    .unwrap();
    let rows: Vec<_> = result
        .identifiers
        .iter()
        .filter(|id| {
            id.name == "Result" && id.start_byte == 35 && id.kind == IdentifierKind::TypeUsage
        })
        .collect();
    assert_eq!(rows.len(), 1, "{:?}", result.identifiers);
    assert_eq!(rows[0].end_byte, 41);
}

#[test]
fn powershell_named_return_type_has_exact_identifier_span() {
    let source = include_str!("../../../../fixtures/extraction/powershell/return_types/source.ps1");
    let result = crate::pipeline::extract_canonical(
        "powershell/source.ps1",
        source,
        std::path::Path::new("/repo"),
    )
    .unwrap();
    let rows: Vec<_> = result
        .identifiers
        .iter()
        .filter(|id| {
            id.name == "Result" && id.start_byte == 34 && id.kind == IdentifierKind::TypeUsage
        })
        .collect();
    assert_eq!(
        rows.len(),
        1,
        "{:?} AST:{}",
        result.identifiers,
        crate::tests::helpers::init_parser(source, "powershell")
            .root_node()
            .to_sexp()
    );
    assert_eq!(rows[0].end_byte, 40);
}
