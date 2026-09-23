//! Source text that a tree-sitter grammar cannot parse, because a
//! preprocessor, a Qt macro, a C# directive, or an F# auto-property form owns
//! it, is blanked or rewritten at the same length before the parse.
//! Byte positions therefore still address the original text, which stays the
//! source of record for every extractor.

/// `None` when the language has nothing to blank or the source carries no site.
pub(crate) fn blanked_source(language: &str, content: &str) -> Option<String> {
    match language {
        crate::javascript::qml_directives::LANGUAGE => {
            crate::javascript::qml_directives::blank_directives(content)
        }
        "cpp" => crate::cpp::qt_macros::blank_macros(content),
        "csharp" => crate::csharp::preprocessor::blank_directives(content),
        "fsharp" => crate::fsharp::preprocessor::rewrite_auto_properties(content),
        _ => None,
    }
}
