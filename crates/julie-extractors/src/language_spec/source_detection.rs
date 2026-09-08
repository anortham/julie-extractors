use anyhow::Result;
use std::path::Path;

pub(crate) type DetectedTree = (&'static str, Option<tree_sitter::Tree>);

#[cfg(test)]
thread_local! {
    static HEADER_PROBE_PARSE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn header_probe_parse_count() -> usize {
    HEADER_PROBE_PARSE_COUNT.with(|c| c.get())
}

#[cfg(test)]
pub(crate) fn reset_header_probe_parse_count() {
    HEADER_PROBE_PARSE_COUNT.with(|c| c.set(0));
}

pub(crate) fn detect_with_probe<E, F>(
    file_path: &Path,
    source: &str,
    mut header_probe: F,
) -> Result<Option<DetectedTree>, E>
where
    F: FnMut(&str) -> Result<Option<(&'static str, tree_sitter::Tree)>, E>,
{
    if file_path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("qmldir"))
    {
        return Ok(Some(("qmldir", None)));
    }

    let extension = file_path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("");

    if extension.eq_ignore_ascii_case("h") {
        if let Some((language, tree)) = header_probe(source)? {
            return Ok(Some((language, Some(tree))));
        }
        return Ok(Some(("c", None)));
    }

    Ok(crate::language_spec::detect_language_from_extension(extension).map(|lang| (lang, None)))
}

fn header_has_content_legacy(source: &str) -> bool {
    source.chars().any(|c| !c.is_whitespace())
}

pub(crate) fn detect_legacy(path: &Path, source: &str) -> Option<DetectedTree> {
    detect_with_probe(path, source, |source| {
        if header_has_content_legacy(source) {
            Ok::<_, std::convert::Infallible>(strict_header_probe(source).ok())
        } else {
            Ok(None)
        }
    })
    .expect("legacy header adapter is infallible")
}

pub(crate) fn strict_header_probe(content: &str) -> Result<(&'static str, tree_sitter::Tree)> {
    let (c_tree, c_errors) = parse_probe_tree_and_errors(crate::language_spec::parser_c(), content)
        .ok_or_else(|| anyhow::anyhow!("failed to probe C header"))?;
    let (cpp_tree, cpp_errors) =
        parse_probe_tree_and_errors(crate::language_spec::parser_cpp(), content)
            .ok_or_else(|| anyhow::anyhow!("failed to probe C++ header"))?;
    if cpp_errors < c_errors {
        Ok(("cpp", cpp_tree))
    } else if c_errors < cpp_errors {
        Ok(("c", c_tree))
    } else {
        let code = c_family_code_without_comments_and_strings(content);
        if code.contains("::")
            || code.contains("template <")
            || code.contains("public:")
            || code.contains("private:")
            || code.contains("protected:")
        {
            Ok(("cpp", cpp_tree))
        } else {
            Ok(("c", c_tree))
        }
    }
}

fn parse_probe_tree_and_errors(
    language: tree_sitter::Language,
    content: &str,
) -> Option<(tree_sitter::Tree, usize)> {
    #[cfg(test)]
    HEADER_PROBE_PARSE_COUNT.with(|c| c.set(c.get() + 1));

    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&language).ok()?;
    let tree = parser.parse(content, None)?;
    let error_count = count_parse_errors(tree.root_node(), 0);
    Some((tree, error_count))
}

fn count_parse_errors(node: tree_sitter::Node<'_>, depth: u32) -> usize {
    if !crate::tree_traversal::should_visit_tree_depth(depth) {
        return 0;
    }

    let mut count = usize::from(node.is_error()) + usize::from(node.is_missing());
    if !node.has_error() {
        return count;
    }

    let Some(child_depth) = crate::tree_traversal::child_tree_depth(depth) else {
        return count;
    };

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        count += count_parse_errors(child, child_depth);
    }
    count
}

fn c_family_code_without_comments_and_strings(content: &str) -> String {
    let mut code = String::with_capacity(content.len());
    let mut chars = content.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '/' if chars.peek() == Some(&'/') => {
                chars.next();
                for comment_ch in chars.by_ref() {
                    if comment_ch == '\n' {
                        code.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut previous = '\0';
                for comment_ch in chars.by_ref() {
                    if comment_ch == '\n' {
                        code.push('\n');
                    } else {
                        code.push(' ');
                    }
                    if previous == '*' && comment_ch == '/' {
                        break;
                    }
                    previous = comment_ch;
                }
            }
            '"' | '\'' => {
                code.push(' ');
                let mut escaped = false;
                for literal_ch in chars.by_ref() {
                    if literal_ch == '\n' {
                        code.push('\n');
                    } else {
                        code.push(' ');
                    }
                    if escaped {
                        escaped = false;
                        continue;
                    }
                    if literal_ch == '\\' {
                        escaped = true;
                        continue;
                    }
                    if literal_ch == ch {
                        break;
                    }
                }
            }
            _ => code.push(ch),
        }
    }

    code
}

#[cfg(feature = "syntax-api")]
fn header_has_content_bounded(
    source: &str,
    options: &crate::syntax::SyntaxOptions<'_>,
) -> Result<bool, crate::syntax::SyntaxError> {
    let mut chars_scanned = 0usize;
    for ch in source.chars() {
        if !ch.is_whitespace() {
            return Ok(true);
        }
        chars_scanned += 1;
        if chars_scanned.is_multiple_of(256) {
            options.check()?;
        }
    }
    options.check()?;
    Ok(false)
}

#[cfg(feature = "syntax-api")]
pub(crate) fn detect_strict(
    path: &Path,
    source: &str,
    options: &crate::syntax::SyntaxOptions<'_>,
) -> Result<Option<DetectedTree>, crate::syntax::SyntaxError> {
    detect_with_probe(path, source, |source| {
        if header_has_content_bounded(source, options)? {
            bounded_header_probe(source, options).map(Some)
        } else {
            Ok(None)
        }
    })
}

#[cfg(all(test, feature = "syntax-api"))]
thread_local! {
    pub(crate) static TEST_HEADER_BETWEEN_PROBES_HOOK: std::cell::RefCell<Option<Box<dyn FnMut()>>> = const { std::cell::RefCell::new(None) };
    pub(crate) static TEST_HEADER_SCORING_HOOK: std::cell::RefCell<Option<Box<dyn FnMut()>>> = const { std::cell::RefCell::new(None) };
}

#[cfg(all(test, feature = "syntax-api"))]
pub(crate) fn set_test_header_between_probes_hook(hook: Option<Box<dyn FnMut()>>) {
    TEST_HEADER_BETWEEN_PROBES_HOOK.with(|h| *h.borrow_mut() = hook);
}

#[cfg(all(test, feature = "syntax-api"))]
pub(crate) fn set_test_header_scoring_hook(hook: Option<Box<dyn FnMut()>>) {
    TEST_HEADER_SCORING_HOOK.with(|h| *h.borrow_mut() = hook);
}

#[cfg(feature = "syntax-api")]
pub(crate) fn bounded_header_probe(
    source: &str,
    options: &crate::syntax::SyntaxOptions<'_>,
) -> Result<(&'static str, tree_sitter::Tree), crate::syntax::SyntaxError> {
    options.check()?;

    let c_lang = crate::language_spec::parser_c();
    #[cfg(test)]
    HEADER_PROBE_PARSE_COUNT.with(|c| c.set(c.get() + 1));
    let c_tree = crate::syntax::parse_tree_with_options(&c_lang, source, options)?;
    let c_errors = count_parse_errors_bounded(c_tree.root_node(), 0, options)?;

    #[cfg(test)]
    TEST_HEADER_BETWEEN_PROBES_HOOK.with(|hook| {
        if let Some(hook) = hook.borrow_mut().as_mut() {
            hook();
        }
    });
    options.check()?;

    let cpp_lang = crate::language_spec::parser_cpp();
    #[cfg(test)]
    HEADER_PROBE_PARSE_COUNT.with(|c| c.set(c.get() + 1));
    let cpp_tree = crate::syntax::parse_tree_with_options(&cpp_lang, source, options)?;
    let cpp_errors = count_parse_errors_bounded(cpp_tree.root_node(), 0, options)?;

    #[cfg(test)]
    TEST_HEADER_SCORING_HOOK.with(|hook| {
        if let Some(hook) = hook.borrow_mut().as_mut() {
            hook();
        }
    });
    options.check()?;

    if cpp_errors < c_errors {
        Ok(("cpp", cpp_tree))
    } else if c_errors < cpp_errors {
        Ok(("c", c_tree))
    } else {
        let code = c_family_code_without_comments_and_strings_bounded(source, options)?;
        options.check()?;
        if tie_break_scan_prefers_cpp_bounded(&code, options)? {
            Ok(("cpp", cpp_tree))
        } else {
            Ok(("c", c_tree))
        }
    }
}

#[cfg(feature = "syntax-api")]
fn tie_break_scan_prefers_cpp_bounded(
    code: &str,
    options: &crate::syntax::SyntaxOptions<'_>,
) -> Result<bool, crate::syntax::SyntaxError> {
    const PATTERNS: &[&str] = &["::", "template <", "public:", "private:", "protected:"];

    let mut chars_scanned: usize = 0;
    for (byte_idx, _) in code.char_indices() {
        chars_scanned += 1;
        if chars_scanned.is_multiple_of(256) {
            options.check()?;
        }
        let remainder = &code[byte_idx..];
        for pattern in PATTERNS {
            if remainder.starts_with(pattern) {
                return Ok(true);
            }
        }
    }
    options.check()?;
    Ok(false)
}

#[cfg(feature = "syntax-api")]
fn count_parse_errors_bounded(
    node: tree_sitter::Node<'_>,
    depth: u32,
    options: &crate::syntax::SyntaxOptions<'_>,
) -> Result<usize, crate::syntax::SyntaxError> {
    options.check()?;
    if !crate::tree_traversal::should_visit_tree_depth(depth) {
        return Ok(0);
    }

    let mut count = usize::from(node.is_error()) + usize::from(node.is_missing());
    if !node.has_error() {
        return Ok(count);
    }

    let Some(child_depth) = crate::tree_traversal::child_tree_depth(depth) else {
        return Ok(count);
    };

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        count += count_parse_errors_bounded(child, child_depth, options)?;
    }
    Ok(count)
}

#[cfg(feature = "syntax-api")]
fn c_family_code_without_comments_and_strings_bounded(
    content: &str,
    options: &crate::syntax::SyntaxOptions<'_>,
) -> Result<String, crate::syntax::SyntaxError> {
    let mut code = String::with_capacity(content.len());
    let mut chars = content.chars().peekable();
    let mut chars_processed: usize = 0;

    while let Some(ch) = chars.next() {
        chars_processed += 1;
        if chars_processed.is_multiple_of(256) {
            options.check()?;
        }
        match ch {
            '/' if chars.peek() == Some(&'/') => {
                chars.next();
                chars_processed += 1;
                for comment_ch in chars.by_ref() {
                    chars_processed += 1;
                    if chars_processed.is_multiple_of(256) {
                        options.check()?;
                    }
                    if comment_ch == '\n' {
                        code.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                chars_processed += 1;
                let mut previous = '\0';
                for comment_ch in chars.by_ref() {
                    chars_processed += 1;
                    if chars_processed.is_multiple_of(256) {
                        options.check()?;
                    }
                    if comment_ch == '\n' {
                        code.push('\n');
                    } else {
                        code.push(' ');
                    }
                    if previous == '*' && comment_ch == '/' {
                        break;
                    }
                    previous = comment_ch;
                }
            }
            '"' | '\'' => {
                code.push(' ');
                let mut escaped = false;
                for literal_ch in chars.by_ref() {
                    chars_processed += 1;
                    if chars_processed.is_multiple_of(256) {
                        options.check()?;
                    }
                    if literal_ch == '\n' {
                        code.push('\n');
                    } else {
                        code.push(' ');
                    }
                    if escaped {
                        escaped = false;
                        continue;
                    }
                    if literal_ch == '\\' {
                        escaped = true;
                        continue;
                    }
                    if literal_ch == ch {
                        break;
                    }
                }
            }
            _ => code.push(ch),
        }
    }
    options.check()?;
    Ok(code)
}
