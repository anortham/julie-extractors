//! Qt's C++ macros are blanked to same-length spaces before the grammar sees them.

use crate::base::{ExtractionLevel, ExtractionResults, SymbolKind};
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
