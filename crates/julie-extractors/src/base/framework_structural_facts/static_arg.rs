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
//! Task 0 ships the Rust arm as the reference implementation. Task 2 adds the
//! Kotlin arm. The Elixir and PHP arms are added by their framework tasks; until
//! then they return `None` so no dynamic value can leak. Per-language accepted
//! node kinds (and their interpolation guards) are enumerated in design §4.4.
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
        StaticArgLang::Kotlin => kotlin_static_arg(node, content),
        StaticArgLang::Php => php_static_arg(node, content),
        // The Elixir arm lands with its framework task (design §4.4). Until then
        // it stays silent so no dynamic argument can leak.
        StaticArgLang::Elixir => None,
    }
}

/// PHP arm: accept a lone `string` (single-quote, never interpolates), an
/// `encapsed_string` (double-quote) whose children are only
/// `string_content`/`escape_sequence`, a `heredoc` whose body is likewise
/// interpolation-free, or a `nowdoc` (single-quote semantics, never
/// interpolates). Reject every wrapper — `binary_expression` (`.` concat),
/// `class_constant_access_expression` (`self::X` / `Foo::BAR`), a bare `name`
/// constant reference, `array_creation_expression`, `variable_name`, and calls —
/// by node kind before any value is read.
///
/// Interpolation guard (grammar-verified against tree-sitter-php 0.24.2, NOT
/// design §4.4 which under-specified the doc-string shapes): a double-quoted
/// `"/$id"` **and** `"/{$id}"` embed a `variable_name` child, `"/${id}"` a
/// `dynamic_variable_name`, `"/{$o->p}"` a `member_access_expression`, and
/// `"/$a[0]"` a `subscript_expression` — so an `encapsed_string` is static only
/// when EVERY child is `string_content`/`escape_sequence`. Heredoc/nowdoc wrap
/// their content in a `heredoc_body` / `nowdoc_body` node (with `heredoc_start`
/// / `heredoc_end` siblings), **not** as direct children of the `heredoc` /
/// `nowdoc` node — so a literal reading of design §4.4 ("heredoc uses the same
/// allowlist check" on the node's children) would fail closed on every heredoc.
/// The allowlist therefore runs on the *body* node's children.
fn php_static_arg<'a>(node: Node<'_>, content: &'a str) -> Option<&'a str> {
    match node.kind() {
        "string" | "encapsed_string" => php_static_string_children(node, content),
        // `heredoc` → `heredoc_body` holds the interpolation-checkable content.
        "heredoc" => php_static_string_children(php_child_of_kind(node, "heredoc_body")?, content),
        // `nowdoc` never interpolates; span its `nowdoc_body`'s `nowdoc_string`.
        "nowdoc" => php_nowdoc_inner_text(php_child_of_kind(node, "nowdoc_body")?, content),
        _ => None,
    }
}

/// Inner text of a PHP `string`/`encapsed_string`/`heredoc_body` as the raw
/// source slice spanning its content children, accepting only `string_content`
/// and `escape_sequence`. Any other named child is an interpolation node
/// (`variable_name`, `dynamic_variable_name`, `member_access_expression`,
/// `subscript_expression`, …) and fails closed to `None`. An empty literal has
/// no content child and yields `""`.
fn php_static_string_children<'a>(node: Node<'_>, content: &'a str) -> Option<&'a str> {
    let mut cursor = node.walk();
    let mut content_nodes = Vec::new();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "string_content" | "escape_sequence" => content_nodes.push(child),
            _ => return None,
        }
    }
    match (content_nodes.first(), content_nodes.last()) {
        (Some(first), Some(last)) => content.get(first.start_byte()..last.end_byte()),
        _ => Some(""),
    }
}

/// Inner text of a `nowdoc_body` (the source slice spanning its `nowdoc_string`
/// content). Nowdoc uses single-quote semantics and never interpolates, so no
/// interpolation guard is needed; an empty nowdoc has no content child → `""`.
fn php_nowdoc_inner_text<'a>(body: Node<'_>, content: &'a str) -> Option<&'a str> {
    let mut cursor = body.walk();
    let strings: Vec<Node> = body
        .named_children(&mut cursor)
        .filter(|child| child.kind() == "nowdoc_string")
        .collect();
    match (strings.first(), strings.last()) {
        (Some(first), Some(last)) => content.get(first.start_byte()..last.end_byte()),
        _ => Some(""),
    }
}

fn php_child_of_kind<'t>(node: Node<'t>, kind: &str) -> Option<Node<'t>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).find(|child| child.kind() == kind)
}

/// Kotlin arm: accept a lone `string_literal` / `multiline_string_literal` that
/// carries no interpolation; reject every wrapper (`+` concatenation,
/// identifier/`const` reference, `navigation_expression` member access,
/// `collection_literal` array, `call_expression`, non-string) by node kind
/// before any value is read.
///
/// Interpolation guard (grammar-verified against tree-sitter-kotlin-ng 1.1):
/// the `${…}` form (and the braceless `$id` form inside a *multiline* literal)
/// is a distinct `interpolation` child — but the grammar does **not** wrap the
/// braceless `$id` form in a single-line literal in an `interpolation` node.
/// Instead it splits the content so the bare `$` lands in its own
/// `string_content` child (`"$base/x"` → `string_content "$"` +
/// `string_content "base/x"`). Detecting only an `interpolation` child would
/// therefore leak `"$base/x"` as the false-static route `$base/x`. So the guard
/// rejects **both** an `interpolation` child and any `$` inside a `string_content`
/// child. A literal `$` (only legal in source as an escape) is rare in a route
/// and over-rejecting it is safe silence (M2).
fn kotlin_static_arg<'a>(node: Node<'_>, content: &'a str) -> Option<&'a str> {
    match node.kind() {
        "string_literal" | "multiline_string_literal" => kotlin_static_string(node, content),
        _ => None,
    }
}

fn kotlin_static_string<'a>(node: Node<'_>, content: &'a str) -> Option<&'a str> {
    let mut cursor = node.walk();
    let mut content_nodes = Vec::new();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            // `${…}` (any literal) or braceless `$id` (multiline) → dynamic.
            "interpolation" => return None,
            "string_content" => {
                // A braceless `$id` in a single-line literal is split so a bare
                // `$` lands here; reject it as (potential) interpolation.
                if content.get(child.start_byte()..child.end_byte())?.contains('$') {
                    return None;
                }
                content_nodes.push(child);
            }
            // Escape sequences (incl. an escaped `\$`, a genuine literal dollar)
            // are static content; keep them in the span.
            "escape_sequence" => content_nodes.push(child),
            // Any other named child is unexpected → fail closed to silence.
            _ => return None,
        }
    }
    match (content_nodes.first(), content_nodes.last()) {
        (Some(first), Some(last)) => content.get(first.start_byte()..last.end_byte()),
        // An empty literal (`""`, `""""""`) has no content child.
        _ => Some(""),
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
            // Elixir is the only remaining unimplemented arm (Php lands in Task 3).
            assert_eq!(
                static_route_arg(node, content, StaticArgLang::Elixir),
                None,
                "unimplemented arm Elixir must stay silent"
            );
        });
    }

    /// Parse `expr` as the RHS of a PHP `$x = …;` assignment and hand its whole
    /// value-expression node (plus the source) to `assertion` — exactly the
    /// argument node a PHP collector passes to [`static_route_arg`].
    fn with_php_arg(expr: &str, assertion: impl FnOnce(Node<'_>, &str)) {
        let src = format!("<?php\n$x = {expr};\n");
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_php::LANGUAGE_PHP.into())
            .expect("load php grammar");
        let tree = parser.parse(&src, None).expect("parse php source");
        let value = find_php_assignment_value(tree.root_node())
            .unwrap_or_else(|| panic!("no assignment value node for `{expr}`"));
        assertion(value, &src);
    }

    /// The `right` field of the first `assignment_expression`.
    fn find_php_assignment_value(node: Node<'_>) -> Option<Node<'_>> {
        if node.kind() == "assignment_expression" {
            return node.child_by_field_name("right");
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(found) = find_php_assignment_value(child) {
                return Some(found);
            }
        }
        None
    }

    #[test]
    fn php_accepts_plain_static_string_literals() {
        // Single- and double-quoted static strings, plus a static nowdoc/heredoc.
        // Laravel's `{id}` / `{id?}` param braces are NOT `{$…}` interpolation, so
        // a double-quoted route with braces stays fully static.
        for (expr, expected, kind) in [
            (r#"'/users'"#, "/users", "string"),
            (r#"'/users/{id}'"#, "/users/{id}", "string"),
            (r#"'/users/{id?}'"#, "/users/{id?}", "string"),
            (r#"''"#, "", "string"),
            (r#""/users""#, "/users", "encapsed_string"),
            (r#""/users/{id}""#, "/users/{id}", "encapsed_string"),
            (r#""/users/{id?}""#, "/users/{id?}", "encapsed_string"),
            (r#""""#, "", "encapsed_string"),
        ] {
            with_php_arg(expr, |node, content| {
                assert_eq!(node.kind(), kind, "node kind for `{expr}`");
                assert_eq!(
                    static_route_arg(node, content, StaticArgLang::Php),
                    Some(expected),
                    "accepted static literal `{expr}`"
                );
            });
        }
    }

    #[test]
    fn php_accepts_static_heredoc_and_nowdoc() {
        // Heredoc/nowdoc are rare as route args but the guard supports them per
        // design §4.4. Their content sits in a `heredoc_body`/`nowdoc_body`, so
        // exact text is newline-fragile — assert acceptance + content instead.
        with_php_arg("<<<'EOT'\n/users\nEOT", |node, content| {
            assert_eq!(node.kind(), "nowdoc", "nowdoc node kind");
            let value = static_route_arg(node, content, StaticArgLang::Php);
            assert!(
                value.is_some_and(|v| v.contains("/users")),
                "static nowdoc accepted, got {value:?}"
            );
        });
        with_php_arg("<<<EOT\n/users\nEOT", |node, content| {
            assert_eq!(node.kind(), "heredoc", "heredoc node kind");
            let value = static_route_arg(node, content, StaticArgLang::Php);
            assert!(
                value.is_some_and(|v| v.contains("/users")),
                "static heredoc accepted, got {value:?}"
            );
        });
    }

    #[test]
    fn php_rejects_dynamic_and_wrapped_forms() {
        // Every dynamic/wrapped form must be silent (M2). The label names the
        // form the whole-argument allowlist rejects.
        for (expr, why) in [
            (r#"'/u/' . $id"#, "binary_expression `.` concat"),
            (r#"$prefix . '/x'"#, "binary_expression `.` concat (literal second)"),
            (r#"self::PREFIX . '/x'"#, "class_constant_access_expression concat"),
            ("self::PREFIX", "class_constant_access_expression const ref"),
            ("Foo::BAR", "class_constant_access_expression const ref"),
            ("PREFIX", "bare name constant reference"),
            (r#""/u/$id""#, "encapsed variable_name interpolation ($id)"),
            (r#""/u/{$id}""#, "encapsed variable_name interpolation ({$id})"),
            (r#""/u/${id}""#, "encapsed dynamic_variable_name interpolation (${id})"),
            (r#""/u/{$user->id}""#, "encapsed member_access_expression interpolation"),
            (r#""/u/$arr[0]""#, "encapsed subscript_expression interpolation"),
            (r#""$base""#, "whole-string variable interpolation"),
            ("<<<EOT\n/u/$id\nEOT", "heredoc body variable_name interpolation"),
            ("$id", "variable_name reference"),
            (r#"['/a', '/b']"#, "array_creation_expression"),
            ("42", "integer (non-string)"),
        ] {
            with_php_arg(expr, |node, content| {
                assert_eq!(
                    static_route_arg(node, content, StaticArgLang::Php),
                    None,
                    "must stay silent for {why}: `{expr}`"
                );
            });
        }
    }

    /// Parse `expr` as the initializer of a Kotlin `val` binding and hand its
    /// whole value-expression node (plus the source) to `assertion` — exactly the
    /// argument node a Kotlin collector passes to [`static_route_arg`].
    fn with_kotlin_arg(expr: &str, assertion: impl FnOnce(Node<'_>, &str)) {
        let src = format!("val x = {expr}");
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_kotlin_ng::LANGUAGE.into())
            .expect("load kotlin grammar");
        let tree = parser.parse(&src, None).expect("parse kotlin source");
        let value = find_kotlin_property_value(tree.root_node())
            .unwrap_or_else(|| panic!("no property value node for `{expr}`"));
        assertion(value, &src);
    }

    /// The value expression of the first `property_declaration`: the first named
    /// child after the `=` token.
    fn find_kotlin_property_value(node: Node<'_>) -> Option<Node<'_>> {
        if node.kind() == "property_declaration" {
            let mut cursor = node.walk();
            let children: Vec<Node> = node.children(&mut cursor).collect();
            let eq = children.iter().position(|child| child.kind() == "=")?;
            return children[eq + 1..]
                .iter()
                .find(|child| child.is_named())
                .copied();
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(found) = find_kotlin_property_value(child) {
                return Some(found);
            }
        }
        None
    }

    #[test]
    fn kotlin_accepts_plain_static_string_literals() {
        for (expr, expected, kind) in [
            (r#""/x""#, "/x", "string_literal"),
            (r#""/users/{id}""#, "/users/{id}", "string_literal"),
            (r#""""#, "", "string_literal"),
            (r#""/a/b/""#, "/a/b/", "string_literal"),
            (r#""""/plain/multi""""#, "/plain/multi", "multiline_string_literal"),
            (r#""""/a/{id}""""#, "/a/{id}", "multiline_string_literal"),
        ] {
            with_kotlin_arg(expr, |node, content| {
                assert_eq!(node.kind(), kind, "node kind for `{expr}`");
                assert_eq!(
                    static_route_arg(node, content, StaticArgLang::Kotlin),
                    Some(expected),
                    "accepted static literal `{expr}`"
                );
            });
        }
    }

    #[test]
    fn kotlin_rejects_dynamic_and_wrapped_forms() {
        // Every dynamic/wrapped form must be silent (M2). The label names the
        // form the whole-argument allowlist rejects.
        for (expr, why) in [
            (r#""${base}/x""#, "${...} interpolation child"),
            (r#""$base/x""#, "braceless $id interpolation (split content, no node)"),
            (r#""$base""#, "whole-string braceless interpolation"),
            (r#""a$b/c""#, "mid-string braceless interpolation"),
            (r#""""/a/${x}""""#, "multiline ${...} interpolation"),
            (r#""""$base/x""""#, "multiline braceless interpolation node"),
            (r#""/a/" + suffix"#, "binary_expression concat"),
            (r#"suffix + "/a""#, "binary_expression concat (literal second)"),
            ("PATHS", "identifier / const reference"),
            ("PATHS.USER", "navigation_expression member access"),
            (r#"["/a", "/b"]"#, "collection_literal array"),
            (r#"listOf("/a")"#, "call_expression"),
            ("42", "integer_literal (non-string)"),
        ] {
            with_kotlin_arg(expr, |node, content| {
                assert_eq!(
                    static_route_arg(node, content, StaticArgLang::Kotlin),
                    None,
                    "must stay silent for {why}: `{expr}`"
                );
            });
        }
    }
}
