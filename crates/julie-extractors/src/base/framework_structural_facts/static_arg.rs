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
    Java,
    CSharp,
    Ruby,
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
        StaticArgLang::Java => java_static_arg(node, content),
        StaticArgLang::CSharp => csharp_static_arg(node, content),
        StaticArgLang::Ruby => ruby_static_arg(node, content),
        StaticArgLang::Rust => rust_static_arg(node, content),
        StaticArgLang::Kotlin => kotlin_static_arg(node, content),
        StaticArgLang::Php => php_static_arg(node, content),
        StaticArgLang::Elixir => elixir_static_arg(node, content),
    }
}

/// Elixir arm: accept a lone `string` / `charlist` / `sigil` that carries no
/// interpolation; for a `sigil`, additionally require `sigil_name ∈ {s, S}`
/// (rejects `~r` regex and every other sigil). Reject every wrapper —
/// `binary_operator` (`<>` concat), `unary_operator` (`@attr` module-attribute
/// reference), a bare `identifier`, `alias` (module reference), `atom`,
/// `keywords`, and calls — by node kind before any value is read.
///
/// Interpolation guard (grammar-verified against tree-sitter-elixir 0.3, which
/// under-specifies design §4.4): a `"/u/#{id}"` string embeds a distinct
/// `interpolation` child, so `~s"/x"` / `"/x"` / `'/x'` are static only when no
/// `interpolation` child is present. **But `~S` (and any capital sigil) does not
/// interpolate**, so tree-sitter leaves a literal `#{id}` verbatim inside the
/// `quoted_content` node with NO `interpolation` child — an interpolation-child
/// check alone would leak `~S"/u/#{id}"` as the false-static route `/u/#{id}`.
/// The guard therefore ALSO fails closed on any `quoted_content` containing the
/// `#{` interpolation marker (M2 silence — a false static is worse than a miss).
/// String content lives in `quoted_content` / `escape_sequence` children (the
/// `quoted_start`/`quoted_end`/`~`/`sigil_name` delimiters are unnamed tokens or
/// metadata, not content); the inner text spans the content children as a raw
/// source slice. A heredoc (`"""…"""`) is the same `string` node kind, so it is
/// covered by the `string` arm.
fn elixir_static_arg<'a>(node: Node<'_>, content: &'a str) -> Option<&'a str> {
    match node.kind() {
        "string" | "charlist" => elixir_static_quoted(node, content),
        "sigil" => {
            // Require an interpolating-safe sigil name (`~s`/`~S`); reject `~r`
            // (regex) and every other sigil before reading any content.
            let name = elixir_child_of_kind(node, "sigil_name")
                .and_then(|name_node| content.get(name_node.start_byte()..name_node.end_byte()))?;
            if name != "s" && name != "S" {
                return None;
            }
            elixir_static_quoted(node, content)
        }
        _ => None,
    }
}

/// Inner text of an Elixir `string` / `charlist` / `sigil` as the raw source
/// slice spanning its `quoted_content` / `escape_sequence` children. Fails closed
/// to `None` on an `interpolation` child, on a `quoted_content` carrying the `#{`
/// marker (the non-interpolating `~S` literal-hash leak), and on any unexpected
/// named child. The `sigil_name` child is sigil metadata (already gated by the
/// caller) and is skipped. An empty literal has no content child → `""`.
fn elixir_static_quoted<'a>(node: Node<'_>, content: &'a str) -> Option<&'a str> {
    let mut cursor = node.walk();
    let mut content_nodes = Vec::new();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "interpolation" => return None,
            // Sigil name (`s`/`S`) is metadata gated by the caller, not content.
            "sigil_name" => {}
            "quoted_content" => {
                if content
                    .get(child.start_byte()..child.end_byte())?
                    .contains("#{")
                {
                    // A `~S`/capital sigil leaves a literal `#{…}` in the content
                    // with no interpolation node — fail closed to silence.
                    return None;
                }
                content_nodes.push(child);
            }
            "escape_sequence" => content_nodes.push(child),
            // Any other named child (e.g. `sigil_modifiers`) → fail closed.
            _ => return None,
        }
    }
    match (content_nodes.first(), content_nodes.last()) {
        (Some(first), Some(last)) => content.get(first.start_byte()..last.end_byte()),
        _ => Some(""),
    }
}

fn elixir_child_of_kind<'t>(node: Node<'t>, kind: &str) -> Option<Node<'t>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| child.kind() == kind)
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
    node.children(&mut cursor)
        .find(|child| child.kind() == kind)
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
                if content
                    .get(child.start_byte()..child.end_byte())?
                    .contains('$')
                {
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

fn java_static_arg<'a>(node: Node<'_>, content: &'a str) -> Option<&'a str> {
    match node.kind() {
        "string_literal" => literal_inner_text(node, content),
        _ => None,
    }
}

fn csharp_static_arg<'a>(node: Node<'_>, content: &'a str) -> Option<&'a str> {
    match node.kind() {
        "string_literal" | "verbatim_string_literal" | "raw_string_literal" => {
            csharp_literal_inner_text(node, content)
        }
        _ => None,
    }
}

fn csharp_literal_inner_text<'a>(node: Node<'_>, content: &'a str) -> Option<&'a str> {
    let text = content.get(node.start_byte()..node.end_byte())?;
    match node.kind() {
        "string_literal" => content.get(node.start_byte() + 1..node.end_byte().saturating_sub(1)),
        "verbatim_string_literal" => {
            content.get(node.start_byte() + 2..node.end_byte().saturating_sub(1))
        }
        "raw_string_literal" => {
            let quote_count = text.bytes().take_while(|byte| *byte == b'"').count();
            if quote_count < 3 || !text.ends_with(&"\"".repeat(quote_count)) {
                return None;
            }
            content.get(node.start_byte() + quote_count..node.end_byte() - quote_count)
        }
        _ => None,
    }
}

fn ruby_static_arg<'a>(node: Node<'_>, content: &'a str) -> Option<&'a str> {
    match node.kind() {
        "string" => ruby_static_string(node, content),
        _ => None,
    }
}

fn ruby_static_string<'a>(node: Node<'_>, content: &'a str) -> Option<&'a str> {
    let mut cursor = node.walk();
    let mut content_nodes = Vec::new();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "interpolation" => return None,
            "string_content" | "escape_sequence" => content_nodes.push(child),
            _ => return None,
        }
    }
    match (content_nodes.first(), content_nodes.last()) {
        (Some(first), Some(last)) => content.get(first.start_byte()..last.end_byte()),
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
    fn arms_are_grammar_isolated() {
        // A Rust `string_literal` node is accepted by the Rust arm. The PHP arm
        // (`string`/`encapsed_string`/…) and the Elixir arm (`string`/`charlist`/
        // `sigil`) name disjoint node kinds, so they fail closed on a Rust
        // `string_literal` node — each arm is an allowlist of its own grammar's
        // kinds (design §4.4). (The Kotlin grammar also names its literal
        // `string_literal`, so that pair is not node-kind-disjoint; the
        // per-language collector dispatch, not this arm, keeps them apart.)
        with_rust_arg(r#""/x""#, |node, content| {
            assert_eq!(
                static_route_arg(node, content, StaticArgLang::Rust),
                Some("/x"),
                "rust arm accepts its own string_literal"
            );
            for foreign in [StaticArgLang::Php, StaticArgLang::Elixir] {
                assert_eq!(
                    static_route_arg(node, content, foreign),
                    None,
                    "arm {foreign:?} must reject a Rust string_literal node kind"
                );
            }
        });
    }

    /// Parse `expr` as the first positional argument of an Elixir `f(<expr>)`
    /// call and hand its whole argument-expression node (plus the source) to
    /// `assertion` — exactly the node a Phoenix/Req collector passes to
    /// [`static_route_arg`].
    fn with_elixir_arg(expr: &str, assertion: impl FnOnce(Node<'_>, &str)) {
        let src = format!("f({expr})");
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_elixir::LANGUAGE.into())
            .expect("load elixir grammar");
        let tree = parser.parse(&src, None).expect("parse elixir source");
        let value = find_elixir_first_call_arg(tree.root_node())
            .unwrap_or_else(|| panic!("no call argument node for `{expr}`"));
        assertion(value, &src);
    }

    /// The first named child of the first `call` node's `arguments` — a pre-order
    /// walk returns the outer `f(<expr>)` call, so its first positional argument
    /// is `<expr>`'s whole expression node.
    fn find_elixir_first_call_arg(node: Node<'_>) -> Option<Node<'_>> {
        if node.kind() == "call" {
            let mut cursor = node.walk();
            let arguments = node
                .children(&mut cursor)
                .find(|child| child.kind() == "arguments")?;
            let mut arg_cursor = arguments.walk();
            return arguments.named_children(&mut arg_cursor).next();
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(found) = find_elixir_first_call_arg(child) {
                return Some(found);
            }
        }
        None
    }

    #[test]
    fn elixir_accepts_plain_static_string_sigil_and_charlist() {
        // Static strings, `~s`/`~S` sigils (any delimiter), and charlists. Phoenix
        // `:id` colon params are ordinary static content, so a route with them
        // stays fully static.
        for (expr, expected, kind) in [
            (r#""/x""#, "/x", "string"),
            (r#""/users/:id""#, "/users/:id", "string"),
            (r#""""#, "", "string"),
            (r#""/a\tb""#, r"/a\tb", "string"), // escapes stay raw in the slice
            (r#"~s"/x""#, "/x", "sigil"),
            (r#"~S"/x""#, "/x", "sigil"),
            (r#"~s(/users/:id)"#, "/users/:id", "sigil"), // paren delimiter
            (r#"'/x'"#, "/x", "charlist"),
        ] {
            with_elixir_arg(expr, |node, content| {
                assert_eq!(node.kind(), kind, "node kind for `{expr}`");
                assert_eq!(
                    static_route_arg(node, content, StaticArgLang::Elixir),
                    Some(expected),
                    "accepted static literal `{expr}`"
                );
            });
        }
    }

    #[test]
    fn elixir_accepts_static_heredoc() {
        // A heredoc is the same `string` node kind; its content sits between
        // `quoted_start`/`quoted_end` with surrounding newlines, so assert
        // acceptance + content rather than exact (newline-fragile) text.
        with_elixir_arg("\"\"\"\n/x\n\"\"\"", |node, content| {
            assert_eq!(node.kind(), "string", "heredoc is a string node");
            let value = static_route_arg(node, content, StaticArgLang::Elixir);
            assert!(
                value.is_some_and(|v| v.contains("/x")),
                "static heredoc accepted, got {value:?}"
            );
        });
    }

    #[test]
    fn elixir_rejects_dynamic_and_wrapped_forms() {
        // Every dynamic/wrapped form must be silent (M2). The label names the
        // form the whole-argument allowlist rejects.
        for (expr, why) in [
            (r#""/u/#{id}""#, "string interpolation child"),
            (r#"~s"/u/#{id}""#, "~s sigil interpolation child"),
            (
                r#"~S"/u/#{id}""#,
                "~S non-interpolating sigil with literal #{} in quoted_content (no interpolation node)",
            ),
            (r#"'/u/#{id}'"#, "charlist interpolation child"),
            (r#"~r"/x""#, "~r regex sigil (sigil_name r)"),
            (r#""/a/" <> id"#, "binary_operator <> concat"),
            (
                r#"prefix <> "/x""#,
                "binary_operator <> concat (literal second)",
            ),
            (
                r#""/a/" <> "/b""#,
                "binary_operator <> concat (two literals)",
            ),
            ("@path", "unary_operator module-attribute reference"),
            ("path", "identifier reference"),
            ("PathModule", "alias module reference"),
            (":show", "atom (non-string)"),
            ("123", "integer (non-string)"),
        ] {
            with_elixir_arg(expr, |node, content| {
                assert_eq!(
                    static_route_arg(node, content, StaticArgLang::Elixir),
                    None,
                    "must stay silent for {why}: `{expr}`"
                );
            });
        }
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
            (
                r#"$prefix . '/x'"#,
                "binary_expression `.` concat (literal second)",
            ),
            (
                r#"self::PREFIX . '/x'"#,
                "class_constant_access_expression concat",
            ),
            ("self::PREFIX", "class_constant_access_expression const ref"),
            ("Foo::BAR", "class_constant_access_expression const ref"),
            ("PREFIX", "bare name constant reference"),
            (r#""/u/$id""#, "encapsed variable_name interpolation ($id)"),
            (
                r#""/u/{$id}""#,
                "encapsed variable_name interpolation ({$id})",
            ),
            (
                r#""/u/${id}""#,
                "encapsed dynamic_variable_name interpolation (${id})",
            ),
            (
                r#""/u/{$user->id}""#,
                "encapsed member_access_expression interpolation",
            ),
            (
                r#""/u/$arr[0]""#,
                "encapsed subscript_expression interpolation",
            ),
            (r#""$base""#, "whole-string variable interpolation"),
            (
                "<<<EOT\n/u/$id\nEOT",
                "heredoc body variable_name interpolation",
            ),
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
            (
                r#""""/plain/multi""""#,
                "/plain/multi",
                "multiline_string_literal",
            ),
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
            (
                r#""$base/x""#,
                "braceless $id interpolation (split content, no node)",
            ),
            (r#""$base""#, "whole-string braceless interpolation"),
            (r#""a$b/c""#, "mid-string braceless interpolation"),
            (r#""""/a/${x}""""#, "multiline ${...} interpolation"),
            (r#""""$base/x""""#, "multiline braceless interpolation node"),
            (r#""/a/" + suffix"#, "binary_expression concat"),
            (
                r#"suffix + "/a""#,
                "binary_expression concat (literal second)",
            ),
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

    fn with_java_arg(expr: &str, assertion: impl FnOnce(Node<'_>, &str)) {
        let src = format!("class T {{ void f() {{ var x = {expr}; }} }}");
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_java::LANGUAGE.into())
            .expect("load java grammar");
        let tree = parser.parse(&src, None).expect("parse java source");
        let value = find_java_variable_value(tree.root_node())
            .unwrap_or_else(|| panic!("no java variable value node for `{expr}`"));
        assertion(value, &src);
    }

    fn find_java_variable_value(node: Node<'_>) -> Option<Node<'_>> {
        if node.kind() == "variable_declarator" {
            return node.child_by_field_name("value");
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(found) = find_java_variable_value(child) {
                return Some(found);
            }
        }
        None
    }

    #[test]
    fn java_accepts_plain_static_string_literals() {
        for (expr, expected, kind) in [
            (r#""/x""#, "/x", "string_literal"),
            (r#""/users/{id}""#, "/users/{id}", "string_literal"),
            (r#""""#, "", "string_literal"),
            (r#""/a\tb""#, r"/a\tb", "string_literal"),
        ] {
            with_java_arg(expr, |node, content| {
                assert_eq!(node.kind(), kind, "node kind for `{expr}`");
                assert_eq!(
                    static_route_arg(node, content, StaticArgLang::Java),
                    Some(expected),
                    "accepted static literal `{expr}`"
                );
            });
        }
    }

    #[test]
    fn java_rejects_dynamic_and_wrapped_forms() {
        for (expr, why) in [
            (r#""/u/" + id"#, "binary_expression concat"),
            (
                r#"prefix + "/x""#,
                "binary_expression concat literal second",
            ),
            ("PATHS.USER", "field_access member reference"),
            ("PATH", "identifier reference"),
            (r#"String.format("/%s", id)"#, "method_invocation"),
            (r#"new String("/x")"#, "object_creation_expression"),
            (r#"new String[] {"/a", "/b"}"#, "array_creation_expression"),
            ("42", "decimal_integer_literal"),
        ] {
            with_java_arg(expr, |node, content| {
                assert_eq!(
                    static_route_arg(node, content, StaticArgLang::Java),
                    None,
                    "must stay silent for {why}: `{expr}`"
                );
            });
        }
    }

    fn with_csharp_arg(expr: &str, assertion: impl FnOnce(Node<'_>, &str)) {
        let src = format!("class C {{ void M() {{ var x = {expr}; }} }}");
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_c_sharp::LANGUAGE.into())
            .expect("load csharp grammar");
        let tree = parser.parse(&src, None).expect("parse csharp source");
        let value = find_csharp_variable_value(tree.root_node())
            .unwrap_or_else(|| panic!("no csharp variable value node for `{expr}`"));
        assertion(value, &src);
    }

    fn find_csharp_variable_value(node: Node<'_>) -> Option<Node<'_>> {
        if node.kind() == "variable_declarator" {
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
            if let Some(found) = find_csharp_variable_value(child) {
                return Some(found);
            }
        }
        None
    }

    #[test]
    fn csharp_accepts_plain_static_string_literals() {
        for (expr, expected, kind) in [
            (r#""/x""#, "/x", "string_literal"),
            (r#""/users/{id}""#, "/users/{id}", "string_literal"),
            (r#""""#, "", "string_literal"),
            (r#""/a\tb""#, r"/a\tb", "string_literal"),
            (
                r#"@"/verbatim/{id}""#,
                "/verbatim/{id}",
                "verbatim_string_literal",
            ),
        ] {
            with_csharp_arg(expr, |node, content| {
                assert_eq!(node.kind(), kind, "node kind for `{expr}`");
                assert_eq!(
                    static_route_arg(node, content, StaticArgLang::CSharp),
                    Some(expected),
                    "accepted static literal `{expr}`"
                );
            });
        }
    }

    #[test]
    fn csharp_rejects_dynamic_and_wrapped_forms() {
        for (expr, why) in [
            (r#"$"/u/{id}""#, "interpolated_string_expression"),
            (r#""/u/" + id"#, "binary_expression concat"),
            (
                r#"prefix + "/x""#,
                "binary_expression concat literal second",
            ),
            ("Routes.User", "member_access_expression"),
            ("PATH", "identifier reference"),
            (r#"string.Format("/{0}", id)"#, "invocation_expression"),
            (r#"new[] {"/a", "/b"}"#, "array_creation_expression"),
            ("42", "integer_literal"),
        ] {
            with_csharp_arg(expr, |node, content| {
                assert_eq!(
                    static_route_arg(node, content, StaticArgLang::CSharp),
                    None,
                    "must stay silent for {why}: `{expr}`"
                );
            });
        }
    }

    fn with_ruby_arg(expr: &str, assertion: impl FnOnce(Node<'_>, &str)) {
        let src = format!("x = {expr}\n");
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_ruby::LANGUAGE.into())
            .expect("load ruby grammar");
        let tree = parser.parse(&src, None).expect("parse ruby source");
        let value = find_ruby_assignment_value(tree.root_node())
            .unwrap_or_else(|| panic!("no ruby assignment value node for `{expr}`"));
        assertion(value, &src);
    }

    fn find_ruby_assignment_value(node: Node<'_>) -> Option<Node<'_>> {
        if node.kind() == "assignment" {
            return node.child_by_field_name("right");
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(found) = find_ruby_assignment_value(child) {
                return Some(found);
            }
        }
        None
    }

    #[test]
    fn ruby_accepts_plain_static_string_literals() {
        for (expr, expected, kind) in [
            (r#""/x""#, "/x", "string"),
            (r#""/users/:id""#, "/users/:id", "string"),
            (r#""""#, "", "string"),
            (r#""/a\tb""#, r"/a\tb", "string"),
            (r#"'/single/:id'"#, "/single/:id", "string"),
        ] {
            with_ruby_arg(expr, |node, content| {
                assert_eq!(node.kind(), kind, "node kind for `{expr}`");
                assert_eq!(
                    static_route_arg(node, content, StaticArgLang::Ruby),
                    Some(expected),
                    "accepted static literal `{expr}`"
                );
            });
        }
    }

    #[test]
    fn ruby_rejects_dynamic_and_wrapped_forms() {
        for (expr, why) in [
            (r#""/u/#{id}""#, "interpolation child"),
            (r#""/u/" + id"#, "binary + concat"),
            (r#"prefix + "/x""#, "binary + concat literal second"),
            ("Routes::USER", "constant path reference"),
            ("PATH", "constant reference"),
            (r#"format("/%s", id)"#, "method call"),
            (r#"["/a", "/b"]"#, "array"),
            ("42", "integer"),
        ] {
            with_ruby_arg(expr, |node, content| {
                assert_eq!(
                    static_route_arg(node, content, StaticArgLang::Ruby),
                    None,
                    "must stay silent for {why}: `{expr}`"
                );
            });
        }
    }
}
