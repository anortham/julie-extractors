//! Qt's C++ macros are blanked to same-length spaces before the grammar sees them.

use crate::base::{ExtractionLevel, ExtractionResults, IdentifierKind, SymbolKind};
use crate::cpp::qt_macros::{MacroKind, blank_macros, scan};
use crate::pipeline::extract_canonical_at;
use std::path::Path;

const QT_HEADER: &str = r#"#pragma once

#include <QQuickItem>

class KIRIGAMI2_EXPORT ColumnViewAttached : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_UNCREATABLE("Attached property")
    Q_PROPERTY(int index READ index WRITE setIndex NOTIFY indexChanged FINAL)

public:
    enum Mode {
        Fixed = 0,
        Dynamic,
    };
    Q_ENUM(Mode)

    int index() const;
    void setIndex(int index);
    Q_INVOKABLE QQuickItem *get(int index);

public Q_SLOTS:
    void refresh();

Q_SIGNALS:
    void indexChanged();
};
"#;

fn extract_header(source: &str) -> ExtractionResults {
    extract_canonical_at(
        "src/layouts/columnview.h",
        source,
        Path::new("/repo"),
        ExtractionLevel::Facts,
    )
    .expect("a Qt header should extract")
}

fn raw_cpp_parse_has_errors(source: &str) -> bool {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_cpp::LANGUAGE.into())
        .expect("the C++ grammar should load");
    parser
        .parse(source, None)
        .expect("the C++ grammar should parse")
        .root_node()
        .has_error()
}

#[test]
fn a_statement_macro_and_its_argument_list_become_spaces() {
    let source = "class Foo\n{\n    Q_OBJECT\n    Q_PROPERTY(int index READ index)\n};\n";

    let blanked = blank_macros(source).expect("Qt macros should be blanked");
    let sites = scan(source);

    assert_eq!(
        blanked,
        format!(
            "class Foo\n{{\n{}\n{}\n}};\n",
            " ".repeat(12),
            " ".repeat(36)
        )
    );
    assert_eq!(sites.len(), 2);
    assert_eq!(sites[0].kind, MacroKind::Statement);
    assert_eq!(sites[0].name, "Q_OBJECT");
    assert_eq!(sites[0].arguments, None);
    assert_eq!(sites[0].line, 3);
    assert_eq!((sites[0].start_byte, sites[0].end_byte), (16, 24));
    assert_eq!(sites[1].kind, MacroKind::Statement);
    assert_eq!(sites[1].name, "Q_PROPERTY");
    assert_eq!(sites[1].arguments.as_deref(), Some("int index READ index"));
    assert_eq!(sites[1].line, 4);
    assert_eq!((sites[1].start_byte, sites[1].end_byte), (29, 61));
}

#[test]
fn a_prefix_macro_becomes_spaces_and_the_rest_of_the_line_stays() {
    let source = "class Foo\n{\n    Q_INVOKABLE void run();\n};\n";

    let blanked = blank_macros(source).expect("Qt macros should be blanked");
    let sites = scan(source);

    assert_eq!(
        blanked,
        format!("class Foo\n{{\n{}void run();\n}};\n", " ".repeat(16))
    );
    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].kind, MacroKind::Prefix);
    assert_eq!(sites[0].name, "Q_INVOKABLE");
    assert_eq!(sites[0].arguments, None);
    assert_eq!(sites[0].line, 3);
    assert_eq!((sites[0].start_byte, sites[0].end_byte), (16, 27));
}

#[test]
fn a_bare_signals_label_becomes_a_padded_public_label() {
    let source = "class Foo\n{\nQ_SIGNALS:\n    void changed();\n};\n";

    let blanked = blank_macros(source).expect("Qt macros should be blanked");
    let sites = scan(source);

    assert_eq!(
        blanked,
        "class Foo\n{\npublic:   \n    void changed();\n};\n"
    );
    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].kind, MacroKind::Section);
    assert_eq!(sites[0].name, "signals");
    assert_eq!(sites[0].arguments, None);
    assert_eq!(sites[0].line, 3);
    assert_eq!((sites[0].start_byte, sites[0].end_byte), (12, 22));
}

#[test]
fn a_bare_lowercase_signals_label_becomes_a_padded_public_label() {
    let source = "class Foo\n{\nsignals:\n    void changed();\n};\n";

    let blanked = blank_macros(source).expect("Qt macros should be blanked");

    assert_eq!(blanked, "class Foo\n{\npublic: \n    void changed();\n};\n");
}

#[test]
fn ordinary_function_labels_are_not_rewritten_as_qt_sections() {
    let source = "void f()\n{\nsignals:\n    goto slots;\nslots:\n    return;\n}\n";

    assert!(scan(source).is_empty());
    assert!(blank_macros(source).is_none());
    assert!(!raw_cpp_parse_has_errors(source));
    let results = extract_canonical_at(
        "src/labels.cpp",
        source,
        Path::new("/repo"),
        ExtractionLevel::Full,
    )
    .expect("ordinary C++ labels should extract");
    assert!(results.parse_diagnostics.is_empty());
}

#[test]
fn lowercase_labels_in_methods_stay_ordinary_but_class_sections_are_rewritten() {
    let source = "class Foo {\n    void f()\n    {\n    signals:\n        goto slots;\n    slots:\n        return;\n    }\nsignals:\n    void changed();\n};\n";

    assert_eq!(
        scan(source)
            .iter()
            .map(|site| site.name.as_str())
            .collect::<Vec<_>>(),
        ["signals"]
    );
}

#[test]
fn lower_case_sections_work_after_template_and_namespace_class_headers() {
    let source = "namespace N { class Foo {\nsignals:\n    void changed();\n}; }\ntemplate<class T> class Bar {\nsignals:\n    void changed();\n};\n";

    assert_eq!(
        scan(source)
            .iter()
            .map(|site| site.name.as_str())
            .collect::<Vec<_>>(),
        ["signals", "signals"]
    );
}

#[test]
fn lower_case_sections_work_after_standard_class_declaration_prefixes() {
    for header in [
        "class Forward; class Widget {",
        "export class Widget {",
        "typedef class Widget {",
    ] {
        let source = format!("{header}\nsignals:\n    void changed();\n}};\n");
        assert_eq!(
            scan(&source)
                .iter()
                .map(|site| site.name.as_str())
                .collect::<Vec<_>>(),
            ["signals"]
        );
    }
}

#[test]
fn a_bare_slots_label_becomes_spaces() {
    let source = "class Foo\n{\npublic:\nQ_SLOTS:\n    void run();\n};\n";

    let blanked = blank_macros(source).expect("Qt macros should be blanked");
    let sites = scan(source);

    assert_eq!(
        blanked,
        format!(
            "class Foo\n{{\npublic:\n{}\n    void run();\n}};\n",
            " ".repeat(8)
        )
    );
    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].kind, MacroKind::Section);
    assert_eq!(sites[0].name, "slots");
    assert_eq!(sites[0].arguments, None);
    assert_eq!((sites[0].start_byte, sites[0].end_byte), (20, 28));
}

#[test]
fn a_prefixed_slots_label_keeps_its_access_word_and_colon() {
    let source = "class Foo\n{\npublic Q_SLOTS:\n    void run();\n};\n";

    let blanked = blank_macros(source).expect("Qt macros should be blanked");
    let sites = scan(source);

    assert_eq!(
        blanked,
        format!(
            "class Foo\n{{\npublic{}:\n    void run();\n}};\n",
            " ".repeat(8)
        )
    );
    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].kind, MacroKind::Section);
    assert_eq!(sites[0].name, "slots");
    assert_eq!(sites[0].arguments.as_deref(), Some("public"));
    assert_eq!(sites[0].line, 3);
    assert_eq!((sites[0].start_byte, sites[0].end_byte), (19, 26));
}

#[test]
fn an_export_macro_becomes_spaces_mid_line() {
    let source = "class KIRIGAMI2_EXPORT ColumnView : public QQuickItem\n{\n};\n";

    let blanked = blank_macros(source).expect("Qt macros should be blanked");
    let sites = scan(source);

    assert_eq!(
        blanked,
        format!(
            "class{}ColumnView : public QQuickItem\n{{\n}};\n",
            " ".repeat(18)
        )
    );
    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].kind, MacroKind::Export);
    assert_eq!(sites[0].name, "KIRIGAMI2_EXPORT");
    assert_eq!(sites[0].arguments, None);
    assert_eq!(sites[0].line, 1);
    assert_eq!((sites[0].start_byte, sites[0].end_byte), (6, 22));
}

#[test]
fn a_multi_line_property_macro_keeps_its_newlines_and_byte_length() {
    let source = "class Foo\n{\n    Q_PROPERTY(int index\n               READ index\n               NOTIFY indexChanged)\n};\n";

    let blanked = blank_macros(source).expect("Qt macros should be blanked");
    let sites = scan(source);

    assert_eq!(blanked.len(), source.len());
    assert_eq!(
        blanked,
        format!(
            "class Foo\n{{\n{}\n{}\n{}\n}};\n",
            " ".repeat(24),
            " ".repeat(25),
            " ".repeat(35)
        )
    );
    assert_eq!(sites.len(), 1);
    assert_eq!(
        sites[0].arguments.as_deref(),
        Some("int index\n               READ index\n               NOTIFY indexChanged")
    );
}

#[test]
fn literals_comments_and_preprocessor_lines_are_untouched() {
    let source = "#define Q_OBJECT\nconst char *name = \"Q_OBJECT\";\n// Q_PROPERTY(int index)\n/* Q_OBJECT */\nclass Foo\n{\n    Q_GADGET\n};\n";

    let blanked = blank_macros(source).expect("Qt macros should be blanked");
    let sites = scan(source);

    assert_eq!(
        blanked,
        format!(
            "#define Q_OBJECT\nconst char *name = \"Q_OBJECT\";\n// Q_PROPERTY(int index)\n/* Q_OBJECT */\nclass Foo\n{{\n{}\n}};\n",
            " ".repeat(12)
        )
    );
    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].name, "Q_GADGET");
}

#[test]
fn character_literals_and_digit_separators_do_not_hide_later_macros() {
    let source = "void f(int n) { switch (n) { case'a': break; } }\nclass Foo {\n    Q_CLASSINFO(\"n\", 1'000)\n    Q_OBJECT\n};\n";

    let sites = scan(source);

    assert_eq!(
        sites
            .iter()
            .map(|site| site.name.as_str())
            .collect::<Vec<_>>(),
        ["Q_CLASSINFO", "Q_OBJECT"]
    );
    assert_eq!(sites[0].arguments.as_deref(), Some("\"n\", 1'000"));
}

#[test]
fn a_continued_preprocessor_line_is_untouched() {
    let source = "#define WRAP(x) \\\n    Q_ASSERT(x);\nclass Foo\n{\n    Q_OBJECT\n};\n";

    let blanked = blank_macros(source).expect("Qt macros should be blanked");
    let sites = scan(source);

    assert_eq!(
        blanked,
        format!(
            "#define WRAP(x) \\\n    Q_ASSERT(x);\nclass Foo\n{{\n{}\n}};\n",
            " ".repeat(12)
        )
    );
    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].name, "Q_OBJECT");
}

#[test]
fn crlf_line_endings_keep_their_terminator() {
    let source = "class Foo\r\n{\r\n    Q_OBJECT\r\n};\r\n";

    let blanked = blank_macros(source).expect("Qt macros should be blanked");

    assert_eq!(blanked, "class Foo\r\n{\r\n            \r\n};\r\n");
}

#[test]
fn utf8_text_after_a_macro_keeps_its_bytes() {
    let source = "class Foo\n{\n    Q_OBJECT\n    const char *hint = \"héllo → ok\";\n};\n";

    let blanked = blank_macros(source).expect("Qt macros should be blanked");

    assert_eq!(blanked.len(), source.len());
    assert!(blanked.contains("\"héllo → ok\""));
}

#[test]
fn a_line_leading_emit_becomes_spaces_but_a_call_or_member_does_not() {
    let source = "void f()\n{\n    emit changed();\n    emit(x);\n    emitter.emit(x);\n}\n";

    let blanked = blank_macros(source).expect("Qt macros should be blanked");
    let sites = scan(source);

    assert_eq!(
        blanked,
        format!(
            "void f()\n{{\n{}changed();\n    emit(x);\n    emitter.emit(x);\n}}\n",
            " ".repeat(9)
        )
    );
    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].kind, MacroKind::Prefix);
    assert_eq!(sites[0].name, "emit");
    assert_eq!((sites[0].start_byte, sites[0].end_byte), (15, 19));
}

#[test]
fn mid_line_macros_are_untouched() {
    let source =
        "void f()\n{\n    invoke(obj, \"m\", Q_ARG(int, 5));\n    qDebug() << Q_FUNC_INFO;\n}\n";

    assert_eq!(blank_macros(source), None);
}

#[test]
fn a_source_without_a_macro_site_is_not_rewritten() {
    let source = "class Foo\n{\npublic:\n    void run();\n};\n";

    assert_eq!(blank_macros(source), None);
    assert!(scan(source).is_empty());
}

#[test]
#[cfg(feature = "syntax-api")]
fn the_check_path_reports_no_diagnostics_for_a_qt_header() {
    let parsed = crate::syntax::parse_source(Path::new("/repo/src/columnview.h"), QT_HEADER)
        .expect("a Qt header should parse");

    assert_eq!(parsed.language, "cpp");
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    assert!(raw_cpp_parse_has_errors(QT_HEADER));
}

#[test]
fn the_scan_path_emits_no_macro_named_or_empty_named_symbols() {
    let results = extract_header(QT_HEADER);

    assert!(raw_cpp_parse_has_errors(QT_HEADER));
    assert!(
        !results
            .symbols
            .iter()
            .any(|symbol| symbol.name == "Q_PROPERTY")
    );
    assert!(!results.symbols.iter().any(|symbol| symbol.name.is_empty()));
    let setter = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "setIndex")
        .expect("setIndex should be extracted");
    assert_eq!(setter.kind, SymbolKind::Method);
    assert!(
        setter
            .signature
            .as_deref()
            .is_some_and(|signature| signature.ends_with("setIndex(int index)")),
        "{:?}",
        setter.signature
    );
}

#[test]
fn the_scan_path_emits_no_emit_variable_rows_for_a_cpp_body() {
    let source = "#include \"columnview.h\"\n\nvoid ColumnView::setIndex(int index)\n{\n    m_index = index;\n    Q_EMIT indexChanged(Index(index));\n}\n";

    let results = extract_canonical_at(
        "src/layouts/columnview.cpp",
        source,
        Path::new("/repo"),
        ExtractionLevel::Facts,
    )
    .expect("a Qt body should extract");

    assert!(!results.symbols.iter().any(|symbol| {
        symbol
            .signature
            .as_deref()
            .is_some_and(|signature| signature.contains("Q_EMIT"))
    }));
}

#[test]
fn a_qt_namespace_enum_header_is_still_detected_as_cpp() {
    let source = "#pragma once\n\n#include <qobjectdefs.h>\n\nnamespace Style\n{\nQ_NAMESPACE\nQML_ELEMENT\n\nenum Mode {\n    Fixed = 0,\n    Dynamic,\n};\nQ_ENUM_NS(Mode)\n}\n";

    let results = extract_canonical_at(
        "src/controls/enums.h",
        source,
        Path::new("/repo"),
        ExtractionLevel::Facts,
    )
    .expect("a Qt namespace header should extract");

    assert!(
        results
            .symbols
            .iter()
            .all(|symbol| symbol.language == "cpp"),
        "{:?}",
        results
            .symbols
            .iter()
            .map(|symbol| symbol.language.as_str())
            .collect::<Vec<_>>()
    );
    assert!(
        results.parse_diagnostics.is_empty(),
        "{:?}",
        results.parse_diagnostics
    );
}

const ENUMERATOR_PROBE: &str = r#"enum Mode { MODE_EXPORT, MODE_IMPORT };

int f(int flags)
{
    return flags | MODE_EXPORT;
}
"#;

const SPECIFIER_PROBE: &str = r#"class Foo
{
public:
    virtual Q_INVOKABLE QIcon icon(const QString &name) const;
    static Q_INVOKABLE int count();
    inline Q_NOREPLY void go();
};
"#;

const VENDOR_PROBE: &str = r#"QT_BEGIN_NAMESPACE

class Foo
{
public:
    KIRIGAMIPLATFORM_DEPRECATED_VERSION(5, 80, "use y") void old();
    QT_DEPRECATED_VERSION_X_6_0("use z") void older();
};

K_PLUGIN_FACTORY_WITH_JSON(FooFactory, "foo.json", registerPlugin<Foo>();)
QUICK_TEST_MAIN(Kirigami)
"#;

#[cfg(feature = "syntax-api")]
fn check_diagnostics(name: &str, source: &str) -> Vec<String> {
    crate::syntax::parse_source(&Path::new("/repo/src").join(name), source)
        .expect("a C++ source should parse")
        .diagnostics
        .iter()
        .map(|diagnostic| format!("{diagnostic:?}"))
        .collect()
}

#[test]
fn an_export_suffixed_enumerator_outside_a_declaration_position_is_untouched() {
    assert_eq!(blank_macros(ENUMERATOR_PROBE), None);
    assert!(scan(ENUMERATOR_PROBE).is_empty());
}

#[test]
fn an_export_macro_after_a_declaration_specifier_becomes_spaces() {
    let source = "class Foo\n{\n    static KIRIGAMI2_EXPORT int count();\n};\n";

    let blanked = blank_macros(source).expect("an export macro should be blanked");
    let sites = scan(source);

    assert_eq!(
        blanked,
        format!(
            "class Foo\n{{\n    static{}int count();\n}};\n",
            " ".repeat(18)
        )
    );
    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].kind, MacroKind::Export);
    assert_eq!(sites[0].name, "KIRIGAMI2_EXPORT");
}

#[test]
fn a_prefix_macro_after_declaration_specifiers_becomes_spaces() {
    let blanked = blank_macros(SPECIFIER_PROBE).expect("prefix macros should be blanked");
    let sites = scan(SPECIFIER_PROBE);

    assert_eq!(
        blanked,
        format!(
            "class Foo\n{{\npublic:\n    virtual{}QIcon icon(const QString &name) const;\n    static{}int count();\n    inline{}void go();\n}};\n",
            " ".repeat(13),
            " ".repeat(13),
            " ".repeat(11)
        )
    );
    assert_eq!(sites.len(), 3);
    assert!(sites.iter().all(|site| site.kind == MacroKind::Prefix));
    assert_eq!(sites[2].name, "Q_NOREPLY");
}

#[test]
fn a_vendor_statement_macro_and_its_arguments_become_spaces() {
    let source = "QUICK_TEST_MAIN(Kirigami)\n";

    let blanked = blank_macros(source).expect("a vendor statement macro should be blanked");
    let sites = scan(source);

    assert_eq!(blanked, format!("{}\n", " ".repeat(25)));
    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].kind, MacroKind::Statement);
    assert_eq!(sites[0].name, "QUICK_TEST_MAIN");
    assert_eq!(sites[0].arguments.as_deref(), Some("Kirigami"));
}

#[test]
fn a_qt_prefixed_statement_macro_becomes_spaces() {
    let source = "QT_BEGIN_NAMESPACE\nclass Foo;\nQT_END_NAMESPACE\n";

    let blanked = blank_macros(source).expect("a QT_ macro should be blanked");
    let sites = scan(source);

    assert_eq!(
        blanked,
        format!("{}\nclass Foo;\n{}\n", " ".repeat(18), " ".repeat(16))
    );
    assert_eq!(sites.len(), 2);
    assert_eq!(sites[0].name, "QT_BEGIN_NAMESPACE");
    assert_eq!(sites[1].name, "QT_END_NAMESPACE");
}

#[test]
fn runtime_qt_expression_macros_keep_nested_calls() {
    let source = "void helper();\nvoid run()\n{\n    Q_ASSERT(helper());\n}\n";

    assert!(scan(source).is_empty());
    let results = extract_canonical_at(
        "src/layouts/columnview.cpp",
        source,
        Path::new("/repo"),
        ExtractionLevel::Full,
    )
    .expect("a Qt expression macro should extract");

    assert!(results.identifiers.iter().any(|identifier| {
        identifier.name == "helper"
            && identifier.kind == IdentifierKind::Call
            && identifier.start_line == 4
            && identifier.start_column == 13
    }));
}

#[test]
fn q_unused_without_a_semicolon_keeps_nested_calls() {
    let source = "void helper();\nvoid run()\n{\n    Q_UNUSED(helper())\n}\n";

    let results = extract_canonical_at(
        "src/layouts/columnview.cpp",
        source,
        Path::new("/repo"),
        ExtractionLevel::Full,
    )
    .expect("Q_UNUSED should extract");

    assert!(results.parse_diagnostics.is_empty());
    assert!(results.identifiers.iter().any(|identifier| {
        identifier.name == "helper"
            && identifier.kind == IdentifierKind::Call
            && identifier.start_line == 4
            && identifier.start_column == 13
    }));
}

#[test]
fn qt_declaration_macros_parse_typed_forms() {
    let source = "class FooPrivate {};\nclass Foo {\n    void run()\n    {\n        Q_D(const Foo);\n        Q_Q(const Foo);\n        Q_FOREACH(const FooPrivate &item, items()) {}\n    }\n};\n";

    let results = extract_canonical_at(
        "src/declarations.cpp",
        source,
        Path::new("/repo"),
        ExtractionLevel::Full,
    )
    .expect("Qt declaration macros should extract");

    assert!(results.parse_diagnostics.is_empty());
}

#[test]
fn declaration_macro_families_remain_supported() {
    let source = "class Foo {\n    Q_DISABLE_COPY_MOVE(Foo)\n    Q_DECLARE_PRIVATE(Foo)\n    Q_DECLARE_PUBLIC(Foo)\n    QML_FOREIGN(Foo)\n    QML_EXTENDED(Base)\n    QML_ADDED_IN_VERSION(1, 2)\n};\n";

    assert_eq!(
        scan(source)
            .into_iter()
            .map(|site| site.name)
            .collect::<Vec<_>>(),
        [
            "Q_DISABLE_COPY_MOVE",
            "Q_DECLARE_PRIVATE",
            "Q_DECLARE_PUBLIC",
            "QML_FOREIGN",
            "QML_EXTENDED",
            "QML_ADDED_IN_VERSION",
        ]
    );
}

#[test]
fn released_declaration_macros_remain_supported() {
    let source = "Q_ALWAYS_INLINE void fast();\nQ_NODISCARD_CTOR explicit Foo();\nQ_IMPLICIT Foo(int);\nQ_REVISION(2, 1) void revised();\nQ_ENUMS(Mode)\nQ_PRIVATE_SLOT(d, void changed())\nQ_OBJECT_BINDABLE_PROPERTY(Foo, int, value)\nQ_MOC_INCLUDE(\"private.h\")\nQT_WARNING_PUSH\nQT_WARNING_DISABLE_CLANG(\"-Wfoo\")\nQT_FORWARD_DECLARE_CLASS(Forward)\nQT_REQUIRE_CONFIG(feature)\nQ_LOGGING_CATEGORY(category, \"app\")\nQ_GLOBAL_STATIC_WITH_ARGS(Foo, instance, ())\n";

    assert_eq!(
        scan(source)
            .iter()
            .map(|site| site.name.as_str())
            .collect::<Vec<_>>(),
        [
            "Q_ALWAYS_INLINE",
            "Q_NODISCARD_CTOR",
            "Q_IMPLICIT",
            "Q_REVISION",
            "Q_ENUMS",
            "Q_PRIVATE_SLOT",
            "Q_OBJECT_BINDABLE_PROPERTY",
            "Q_MOC_INCLUDE",
            "QT_WARNING_PUSH",
            "QT_WARNING_DISABLE_CLANG",
            "QT_FORWARD_DECLARE_CLASS",
            "QT_REQUIRE_CONFIG",
            "Q_LOGGING_CATEGORY",
            "Q_GLOBAL_STATIC_WITH_ARGS",
        ]
    );
}

#[test]
fn long_ordinary_line_does_not_hide_the_next_qt_macro() {
    let source = format!("{}\nQ_OBJECT\n", "ordinary ".repeat(10_000));

    let blanked = blank_macros(&source).expect("Q_OBJECT should be blanked");

    assert_eq!(scan(&source).len(), 1);
    assert_eq!(blanked.len(), source.len());
}

#[test]
fn a_deprecated_vendor_macro_and_its_arguments_become_spaces() {
    let source =
        "class Foo\n{\n    KIRIGAMIPLATFORM_DEPRECATED_VERSION(5, 80, \"use y\") void old();\n};\n";

    let blanked = blank_macros(source).expect("a deprecation macro should be blanked");
    let sites = scan(source);

    assert_eq!(
        blanked,
        format!("class Foo\n{{\n    {}void old();\n}};\n", " ".repeat(52))
    );
    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].kind, MacroKind::Prefix);
    assert_eq!(sites[0].name, "KIRIGAMIPLATFORM_DEPRECATED_VERSION");
    assert_eq!(sites[0].arguments.as_deref(), Some("5, 80, \"use y\""));
}

#[test]
fn a_test_declaration_macro_is_untouched() {
    let source = "TEST(Suite, Name)\n{\n}\n\nTEST_CASE(\"a name\")\n{\n}\n";

    assert_eq!(blank_macros(source), None);
}

#[test]
fn an_enumerator_ending_in_deprecated_is_untouched() {
    let source = "enum Mode {\n    MODE_DEPRECATED = 1,\n    MODE_CURRENT = 2,\n};\n";

    assert_eq!(blank_macros(source), None);
}

#[test]
#[cfg(feature = "syntax-api")]
fn the_check_path_accepts_an_export_suffixed_enumerator() {
    let diagnostics = check_diagnostics("modes.cpp", ENUMERATOR_PROBE);

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
#[cfg(feature = "syntax-api")]
fn the_check_path_accepts_a_prefix_macro_after_declaration_specifiers() {
    let diagnostics = check_diagnostics("platformtheme.h", SPECIFIER_PROBE);

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert!(raw_cpp_parse_has_errors(SPECIFIER_PROBE));
}

#[test]
#[cfg(feature = "syntax-api")]
fn the_check_path_accepts_vendor_and_deprecation_macros() {
    let diagnostics = check_diagnostics("factory.cpp", VENDOR_PROBE);

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert!(raw_cpp_parse_has_errors(VENDOR_PROBE));
}

const INITIALIZER_PROBE: &str = r#"static const char *names[] = {
    QT_TR_NOOP("One"),
    QT_TRANSLATE_NOOP("Ctx", "Two"),
    Q_NULLPTR
};
"#;

const INVOKE_PROBE: &str = r#"void f(QObject *obj, int &out)
{
    QMetaObject::invokeMethod(obj, "m",
        Q_ARG(int, 5),
        Q_RETURN_ARG(int, out));
}
"#;

const DECL_ATTRIBUTE_PROBE: &str = r#"class Foo
{
public:
    Foo() Q_DECL_EQ_DELETE;
    QString h() const Q_DECL_NOEXCEPT;
    void f() Q_DECL_OVERRIDE;
};
"#;

#[test]
fn a_line_leading_macro_call_in_an_initializer_list_is_untouched() {
    let blanked = blank_macros(INITIALIZER_PROBE);
    let sites = scan(INITIALIZER_PROBE);

    assert!(blanked.is_none());
    assert!(!sites.iter().any(|site| site.name == "Q_NULLPTR"));
}

#[test]
fn q_nullptr_stays_in_ordinary_expression_operands() {
    let source = "struct QObject {};\nstruct Foo { Foo(QObject *parent = Q_NULLPTR); };\nvoid f(QObject *value) { value = Q_NULLPTR; if (value != Q_NULLPTR) {} }\n";

    let results = extract_canonical_at(
        "src/null.cpp",
        source,
        Path::new("/repo"),
        ExtractionLevel::Full,
    )
    .expect("Q_NULLPTR source should extract");

    assert!(blank_macros(source).is_none());
    assert!(results.parse_diagnostics.is_empty());
}

#[test]
fn line_leading_argument_macros_in_a_call_are_untouched() {
    assert_eq!(blank_macros(INVOKE_PROBE), None);
    assert!(scan(INVOKE_PROBE).is_empty());
}

#[test]
fn a_trailing_declaration_attribute_macro_becomes_spaces() {
    let blanked = blank_macros(DECL_ATTRIBUTE_PROBE).expect("Q_DECL_ macros should be blanked");
    let sites = scan(DECL_ATTRIBUTE_PROBE);

    assert_eq!(
        blanked,
        format!(
            "class Foo\n{{\npublic:\n    Foo(){};\n    QString h() const{};\n    void f(){};\n}};\n",
            " ".repeat(17),
            " ".repeat(16),
            " ".repeat(16)
        )
    );
    let names = sites
        .iter()
        .map(|site| site.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec!["Q_DECL_EQ_DELETE", "Q_DECL_NOEXCEPT", "Q_DECL_OVERRIDE"]
    );
}

#[test]
#[cfg(feature = "syntax-api")]
fn the_check_path_accepts_a_line_leading_macro_call_in_an_initializer_list() {
    let diagnostics = check_diagnostics("names.cpp", INITIALIZER_PROBE);

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
#[cfg(feature = "syntax-api")]
fn the_check_path_accepts_trailing_declaration_attribute_macros() {
    let diagnostics = check_diagnostics("foo.h", DECL_ATTRIBUTE_PROBE);

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

const RAW_STRING_PROBE: &str = r##"class Foo
{
    Q_OBJECT
    const char *named = R"tag(a )tag2 not it)tag";
    const char *empty = R"(
    Q_PROPERTY(int ghost READ ghost)
    )";
    const char *wide = LR"x(y)x";
    const char *utf = u8R"(z)";
    Q_PROPERTY(int index READ index)
};
"##;

#[test]
fn raw_string_literals_are_skipped_whole() {
    let sites = scan(RAW_STRING_PROBE);

    let names = sites
        .iter()
        .map(|site| site.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["Q_OBJECT", "Q_PROPERTY"]);
    assert_eq!(sites[1].arguments.as_deref(), Some("int index READ index"));
}

#[test]
fn a_truncated_raw_string_ends_the_scan() {
    let source = "class Foo\n{\n    Q_OBJECT\n    const char *broken = R\"";

    let sites = scan(source);

    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].name, "Q_OBJECT");
}

#[test]
fn a_raw_string_argument_does_not_close_a_macro_early() {
    let source = "class Foo\n{\n    QML_UNCREATABLE(R\"msg(Use \")\" carefully)msg\")\n};\n";

    let blanked = blank_macros(source).expect("the macro should be blanked");
    let sites = scan(source);

    assert_eq!(blanked, format!("class Foo\n{{\n{}\n}};\n", " ".repeat(49)));
    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].name, "QML_UNCREATABLE");
    assert_eq!(
        sites[0].arguments.as_deref(),
        Some("R\"msg(Use \")\" carefully)msg\"")
    );
}

#[test]
fn an_argument_list_after_a_newline_or_a_comment_becomes_spaces() {
    let after_newline = "class Foo\n{\n    Q_PROPERTY\n        (int value READ value)\n};\n";
    let after_comment = "class Foo\n{\n    Q_PROPERTY /* here */ (int value READ value)\n};\n";

    for source in [after_newline, after_comment] {
        let blanked = blank_macros(source).expect("the macro should be blanked");
        let sites = scan(source);

        assert_eq!(blanked.len(), source.len(), "{source}");
        assert!(!blanked.contains("READ"), "{blanked}");
        assert_eq!(sites.len(), 1, "{source}");
        assert_eq!(
            sites[0].arguments.as_deref(),
            Some("int value READ value"),
            "{source}"
        );
    }
}

#[test]
fn a_line_comment_continued_by_a_backslash_hides_the_next_line() {
    let source =
        "class Foo\n{\n    Q_OBJECT\n    // note \\\n    Q_PROPERTY(int ghost READ ghost)\n};\n";

    let sites = scan(source);

    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].name, "Q_OBJECT");
}

#[test]
#[cfg(feature = "syntax-api")]
fn the_check_path_accepts_raw_string_literals_between_macros() {
    let diagnostics = check_diagnostics("raw.h", RAW_STRING_PROBE);

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

const STACKED_PROBE: &str = r#"class Foo
{
public:
    Q_REQUIRED_RESULT Q_INVOKABLE int count();
    /* keep */ Q_INVOKABLE void run();
};
"#;

#[test]
fn a_run_of_declaration_macros_after_a_comment_becomes_spaces() {
    let blanked = blank_macros(STACKED_PROBE).expect("the macros should be blanked");
    let sites = scan(STACKED_PROBE);

    let names = sites
        .iter()
        .map(|site| site.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec!["Q_REQUIRED_RESULT", "Q_INVOKABLE", "Q_INVOKABLE"]
    );
    assert!(!blanked.contains("Q_INVOKABLE"), "{blanked}");
    assert!(blanked.contains("/* keep */"), "{blanked}");
}

#[test]
#[cfg(feature = "syntax-api")]
fn the_check_path_accepts_a_run_of_declaration_macros() {
    let diagnostics = check_diagnostics("counter.h", STACKED_PROBE);

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}
