//! Whole-argument static-literal detection for the HTTP-boundary collectors.
//!
//! The silence guard (design §4.4, ADR-0005): a route/URL argument produces a
//! fact only when the *whole argument expression* is a plain, static string
//! literal. Concatenation, `format!`/`sprintf`/macro calls, const/identifier
//! references, member/subscript access, and arrays must emit **nothing** — a
//! false "static" promotes a computed path to a guessed route (M2 silence: a
//! false positive is worse than a miss). The decision is an **allowlist on the
//! argument node kind**, never a denylist: an unknown wrapper node fails closed
//! to `None`.
//!
//! Task 0 ships the Rust arm as the reference implementation. The Kotlin,
//! Elixir, and PHP arms are added by their framework tasks; until then they
//! return `None` so no dynamic value can leak. Per-language accepted node kinds
//! (and their interpolation guards) are enumerated in design §4.4.
#![allow(dead_code)] // Foundation API: per-framework collectors (v2.8.0 Tasks 2–6) are its callers.

use tree_sitter::Node;

/// Language selector for [`static_route_arg`]. Each arm owns the allowlist of
/// static string-literal node kinds (and the interpolation guard) for its
/// grammar, per design §4.4.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StaticArgLang {
    Kotlin,
    Elixir,
    Php,
    Rust,
}

/// Return the inner text of `node` **only** when `node` is itself an approved
/// static string-literal argument for `lang`; `None` for every dynamic or
/// wrapped form (concatenation, `format!`/`sprintf`/macro call, identifier or
/// `const` reference, member/subscript access, array, or a literal carrying an
/// interpolation child).
///
/// The check runs on the whole argument-expression node — not a pre-plucked
/// literal — so a collector cannot leak a false positive by extracting the
/// first string out of a larger expression (design §4.4).
pub(super) fn static_route_arg<'a>(
    node: Node<'_>,
    content: &'a str,
    lang: StaticArgLang,
) -> Option<&'a str> {
    match lang {
        StaticArgLang::Rust => rust_static_arg(node, content),
        // Kotlin/Elixir/PHP arms land with their framework tasks (design §4.4).
        // Until then they stay silent so no dynamic argument can leak.
        StaticArgLang::Kotlin | StaticArgLang::Elixir | StaticArgLang::Php => None,
    }
}

/// Rust arm: accept a lone `string_literal` / `raw_string_literal`; reject every
/// wrapper (`format!`/`concat!` macro invocation, `+` concatenation,
/// `.to_string()` call, identifier/`const` reference, `&`-reference, array,
/// non-string). Rust string grammar has no interpolation, so a lone literal is
/// always static — the real work is the whole-argument wrapper rejection.
fn rust_static_arg<'a>(node: Node<'_>, content: &'a str) -> Option<&'a str> {
    match node.kind() {
        "string_literal" | "raw_string_literal" => literal_inner_text(node, content),
        _ => None,
    }
}

/// Inner text of a string-literal node as the raw source slice between its
/// delimiters (escapes left unprocessed). Spans the literal's named content
/// children (`string_content` / `escape_sequence`), which is delimiter- and
/// `#`-count independent, so it handles `"x"`, `r"x"`, and `r#"x"#` uniformly;
/// an empty literal (`""`, `r#""#`) has no content child and yields `""`.
fn literal_inner_text<'a>(node: Node<'_>, content: &'a str) -> Option<&'a str> {
    let mut cursor = node.walk();
    let children: Vec<Node> = node.named_children(&mut cursor).collect();
    match (children.first(), children.last()) {
        (Some(first), Some(last)) => content.get(first.start_byte()..last.end_byte()),
        _ => Some(""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::Parser;

    /// Parse `expr` as the initializer of a `let` binding and hand its whole
    /// value-expression node (plus the backing source) to `assertion`. That
    /// node is exactly the argument-expression node a collector would pass to
    /// [`static_route_arg`].
    fn with_rust_arg(expr: &str, assertion: impl FnOnce(Node<'_>, &str)) {
        let src = format!("fn f() {{ let x = {expr}; }}");
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .expect("load rust grammar");
        let tree = parser.parse(&src, None).expect("parse rust source");
        let value = find_let_value(tree.root_node())
            .unwrap_or_else(|| panic!("no let value node for `{expr}`"));
        assertion(value, &src);
    }

    fn find_let_value(node: Node<'_>) -> Option<Node<'_>> {
        if node.kind() == "let_declaration" {
            return node.child_by_field_name("value");
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(found) = find_let_value(child) {
                return Some(found);
            }
        }
        None
    }

    #[test]
    fn rust_accepts_plain_and_raw_string_literals() {
        for (expr, expected, kind) in [
            (r#""/x""#, "/x", "string_literal"),
            (r#""/users/{id}""#, "/users/{id}", "string_literal"),
            (r#""""#, "", "string_literal"),
            (r#""/a\tb""#, r"/a\tb", "string_literal"), // escapes stay raw in the slice
            (r##"r"/x""##, "/x", "raw_string_literal"),
            (r###"r#"/x"#"###, "/x", "raw_string_literal"),
            (r####"r##"/a/{b}"##"####, "/a/{b}", "raw_string_literal"),
        ] {
            with_rust_arg(expr, |node, content| {
                assert_eq!(node.kind(), kind, "node kind for `{expr}`");
                assert_eq!(
                    static_route_arg(node, content, StaticArgLang::Rust),
                    Some(expected),
                    "accepted static literal `{expr}`"
                );
            });
        }
    }

    #[test]
    fn rust_rejects_dynamic_and_wrapped_forms() {
        // Every dynamic/wrapped form must be silent (M2). The label names the
        // wrapper node kind the whole-argument allowlist rejects.
        for (expr, why) in [
            (r#"format!("/u/{id}")"#, "format! macro_invocation"),
            (r#"concat!("/a", "/b")"#, "concat! macro_invocation"),
            (r#""/u/".to_owned() + &id"#, "binary_expression concat"),
            (r#""/a".to_string()"#, "call_expression"),
            ("PATHS", "identifier / const reference"),
            ("PATHS.USER", "field_expression member access"),
            (r#"&"/x""#, "reference_expression wrapper"),
            (r#"["/a", "/b"]"#, "array_expression"),
            ("41", "integer_literal (non-string)"),
        ] {
            with_rust_arg(expr, |node, content| {
                assert_eq!(
                    static_route_arg(node, content, StaticArgLang::Rust),
                    None,
                    "must stay silent for {why}: `{expr}`"
                );
            });
        }
    }

    #[test]
    fn unimplemented_arms_stay_silent() {
        // A Rust `string_literal` node is accepted by the Rust arm but MUST be
        // rejected by the not-yet-implemented arms — they emit nothing until
        // their framework task lands (design §4.4), even for a genuine literal.
        with_rust_arg(r#""/x""#, |node, content| {
            assert_eq!(
                static_route_arg(node, content, StaticArgLang::Rust),
                Some("/x"),
                "rust arm is implemented in Task 0"
            );
            for lang in [
                StaticArgLang::Kotlin,
                StaticArgLang::Elixir,
                StaticArgLang::Php,
            ] {
                assert_eq!(
                    static_route_arg(node, content, lang),
                    None,
                    "unimplemented arm {lang:?} must stay silent"
                );
            }
        });
    }
}
