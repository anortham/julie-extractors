use std::ops::Range;

use super::syntax_body_hash::{HashString, HashSyntax};
use tree_sitter::Node;

#[cfg(test)]
mod tests {
    use super::{HashString, collect_hash_syntax, supports};
    use crate::language::get_tree_sitter_language;
    use std::ops::Range;
    use tree_sitter::{Parser, Tree};

    fn parse(language: &str, source: &str) -> Tree {
        let grammar = get_tree_sitter_language(language).unwrap();
        let mut parser = Parser::new();
        parser.set_language(&grammar).unwrap();
        let tree = parser.parse(source, None).unwrap();
        assert!(
            !tree.root_node().has_error(),
            "{language}: {}",
            tree.root_node().to_sexp()
        );
        tree
    }

    fn source_range(source: &str, needle: &str) -> Range<usize> {
        let start = source.find(needle).unwrap();
        start..start + needle.len()
    }

    fn assert_single_interpolated_string(
        syntax_string: &HashString,
        range: Range<usize>,
        token_start: usize,
        code: Range<usize>,
    ) {
        assert_eq!(syntax_string.range, range);
        assert_eq!(syntax_string.token_start, token_start);
        assert_eq!(syntax_string.code, vec![code]);
    }

    #[test]
    fn supports_each_native_comment_or_interpolation_adapter() {
        for language in [
            "bash",
            "dart",
            "fsharp",
            "javascript",
            "jsx",
            "kotlin",
            "lua",
            "powershell",
            "python",
            "qml",
            "ruby",
            "rust",
            "scala",
            "swift",
            "tsx",
            "typescript",
        ] {
            assert!(supports(language), "{language}");
        }
        assert!(!supports("java"));
        assert!(!supports("csharp"));
    }

    #[test]
    fn javascript_template_comments_are_code_and_literals_keep_exact_ranges() {
        let source = "function render(value) { return `head ${value /* remove */} tail`; }";
        let tree = parse("javascript", source);
        let syntax = collect_hash_syntax(tree.root_node(), source, "javascript");
        let quote_start = source.find('`').unwrap();
        let quote_end = source.rfind('`').unwrap() + 1;
        let code_start = source.find("${").unwrap() + 2;
        let code_end = source[code_start..].find('}').unwrap() + code_start;

        assert_eq!(syntax.comments, vec![source_range(source, "/* remove */")]);
        assert_eq!(syntax.strings.len(), 1);
        assert_single_interpolated_string(
            &syntax.strings[0],
            quote_start..quote_end,
            quote_start,
            code_start..code_end,
        );
    }

    #[test]
    fn native_interpolation_adapters_select_comments_and_code_ranges() {
        let cases = [
            (
                "bash",
                "render() { local value=one; printf '%s' \"$(printf %s value # marker\n)\"; }",
                "# marker",
                "$(",
                ")",
            ),
            (
                "dart",
                "String render(String value) { return \"head ${value /* marker */} tail\"; }",
                "/* marker */",
                "${",
                "}",
            ),
            (
                "javascript",
                "function render(value) { return `head ${value /* marker */} tail`; }",
                "/* marker */",
                "${",
                "}",
            ),
            (
                "jsx",
                "function render(value) { return `head ${value /* marker */} tail`; }",
                "/* marker */",
                "${",
                "}",
            ),
            (
                "typescript",
                "function render(value: string): string { return `head ${value /* marker */} tail`; }",
                "/* marker */",
                "${",
                "}",
            ),
            (
                "tsx",
                "function render(value: string): string { return `head ${value /* marker */} tail`; }",
                "/* marker */",
                "${",
                "}",
            ),
            (
                "kotlin",
                "fun render(value: String): String { return \"head ${value /* marker */} tail\" }",
                "/* marker */",
                "${",
                "}",
            ),
            (
                "powershell",
                "function Render($value) { return \"head $($value <# marker #>) tail\" }",
                "<# marker #>",
                "$(",
                ")",
            ),
            (
                "python",
                "def render(value):\n    return f\"\"\"head {value # marker\n} tail\"\"\"\n",
                "# marker",
                "{",
                "}",
            ),
            (
                "qml",
                "Item { function render(value) { return `head ${value /* marker */} tail` } }",
                "/* marker */",
                "${",
                "}",
            ),
            (
                "ruby",
                "def render(value)\n  \"head #{value # marker\n} tail\"\nend\n",
                "# marker",
                "#{",
                "}",
            ),
            (
                "scala",
                "object Render { def render(value: String): String = s\"head ${value /* marker */} tail\" }",
                "/* marker */",
                "${",
                "}",
            ),
        ];

        for (language, source, marker, opening, closing) in cases {
            let tree = parse(language, source);
            let syntax = collect_hash_syntax(tree.root_node(), source, language);
            let code_start = source.find(opening).unwrap() + opening.len();
            let code_end = source[code_start..].find(closing).unwrap() + code_start;

            assert_eq!(
                syntax.comments,
                vec![source_range(source, marker)],
                "{language}"
            );
            assert_eq!(syntax.strings.len(), 1, "{language}");
            assert_eq!(
                syntax.strings[0].code,
                vec![code_start..code_end],
                "{language}"
            );
            assert!(syntax.strings[0].interpolated, "{language}");
        }
    }

    #[test]
    fn swift_literal_comment_text_is_excluded_but_interpolation_comment_is_code() {
        let source = "func render(_ value: String) -> String { let marker = \"/* literal */\"; return \"head \\(value /* code */) tail\" }";
        let tree = parse("swift", source);
        let syntax = collect_hash_syntax(tree.root_node(), source, "swift");
        let quote_start = source.find("\"head").unwrap();
        let quote_end = source.rfind("\" }").unwrap() + 1;
        let code_start = source.find("\\(").unwrap() + 2;
        let code_end = source.rfind(')').unwrap();

        assert_eq!(syntax.comments, vec![source_range(source, "/* code */")]);
        assert_eq!(syntax.strings.len(), 1);
        assert_single_interpolated_string(
            &syntax.strings[0],
            quote_start..quote_end,
            quote_start,
            code_start..code_end,
        );
    }

    #[test]
    fn python_interpolation_format_text_stays_outside_expression_ranges() {
        let source = "def render(value, width):\n    return f\"head { value :{ width }d} tail\"\n";
        let tree = parse("python", source);
        let syntax = collect_hash_syntax(tree.root_node(), source, "python");
        let outer_start = source.find('{').unwrap();
        let format_start = source[outer_start..].find(':').unwrap() + outer_start;
        let inner_start = source[format_start..].find('{').unwrap() + format_start;
        let inner_end = source[inner_start..].find('}').unwrap() + inner_start;

        assert_eq!(syntax.strings.len(), 1);
        assert_eq!(
            syntax.strings[0].code,
            vec![outer_start + 1..format_start, inner_start + 1..inner_end],
            "{}",
            tree.root_node().to_sexp()
        );
    }

    #[test]
    fn fsharp_line_and_block_comment_nodes_keep_their_full_ranges() {
        let source = "let render value =\n    let marker = \"(* literal *)\"\n    // line\n    (* block *)\n    marker\n";
        let tree = parse("fsharp", source);
        let syntax = collect_hash_syntax(tree.root_node(), source, "fsharp");

        assert_eq!(
            syntax.comments,
            vec![
                source_range(source, "// line"),
                source_range(source, "(* block *)")
            ]
        );
    }

    #[test]
    fn powershell_block_comment_node_keeps_its_full_range() {
        let source = "function Render { $marker = '# literal'; <# block #>; return $marker }";
        let tree = parse("powershell", source);
        let syntax = collect_hash_syntax(tree.root_node(), source, "powershell");

        assert_eq!(syntax.comments, vec![source_range(source, "<# block #>")]);
    }

    #[test]
    fn rust_nested_block_comment_node_keeps_its_full_range() {
        let source = "fn render() { let marker = \"/* literal */\"; /* outer /* nested */ trailing */ marker }";
        let tree = parse("rust", source);
        let syntax = collect_hash_syntax(tree.root_node(), source, "rust");

        assert_eq!(
            syntax.comments,
            vec![source_range(source, "/* outer /* nested */ trailing */")]
        );
    }

    #[test]
    fn rust_label_nodes_are_opaque_hash_tokens() {
        let source = "fn render() -> i32 { 'outer: loop { /* comment */ break 'outer 1; } }";
        let tree = parse("rust", source);
        let syntax = collect_hash_syntax(tree.root_node(), source, "rust");
        let mut expected = source
            .match_indices("'outer")
            .map(|(start, text)| start..start + text.len())
            .collect::<Vec<_>>();
        let mut stack = vec![tree.root_node()];
        let mut labels = Vec::new();

        while let Some(node) = stack.pop() {
            if node.kind() == "label" {
                labels.push(node.byte_range());
            }
            let mut cursor = node.walk();
            stack.extend(node.children(&mut cursor));
        }
        labels.sort_unstable_by_key(|range| range.start);
        expected.sort_unstable_by_key(|range| range.start);

        assert_eq!(labels, expected);
        assert_eq!(syntax.strings.len(), expected.len());
        for (descriptor, range) in syntax.strings.iter().zip(expected) {
            assert_eq!(descriptor.range, range);
            assert_eq!(descriptor.token_start, range.start);
            assert!(!descriptor.interpolated);
            assert!(descriptor.code.is_empty());
        }
    }

    #[test]
    fn rust_lifetime_nodes_shield_comments_and_preserve_literal_payloads() {
        let comments = [
            "",
            "/*x*/",
            "/* this comment is deliberately much longer */",
        ];
        let mut hashes = Vec::new();
        let mut lifetime_ranges = Vec::new();
        let mut descriptor_ranges_by_source = Vec::new();

        for comment in comments {
            let source = format!(
                "fn render<'a>(value: &'a str) -> &'a str {{ let marker = \"stable\"; let first: &'a str = value; {comment} let second: &'a str = first; second }}"
            );
            let tree = parse("rust", &source);
            let syntax = collect_hash_syntax(tree.root_node(), &source, "rust");
            let mut stack = vec![tree.root_node()];
            let mut node_ranges = Vec::new();

            while let Some(node) = stack.pop() {
                if node.kind() == "lifetime" {
                    node_ranges.push(node.byte_range());
                }
                let mut cursor = node.walk();
                stack.extend(node.children(&mut cursor));
            }
            node_ranges.sort_unstable_by_key(|range| range.start);
            let mut expected_ranges = source
                .match_indices("'a")
                .map(|(start, text)| start..start + text.len())
                .collect::<Vec<_>>();
            expected_ranges.sort_unstable_by_key(|range| range.start);
            assert_eq!(node_ranges, expected_ranges);
            lifetime_ranges.push(node_ranges.clone());

            let mut descriptor_ranges = syntax
                .strings
                .iter()
                .map(|descriptor| descriptor.range.clone())
                .collect::<Vec<_>>();
            descriptor_ranges.sort_unstable_by_key(|range| range.start);
            descriptor_ranges_by_source.push(descriptor_ranges);

            let body_start = source.find("{ let marker").unwrap();
            let body_end = source.rfind('}').unwrap() + 1;
            let body_span = crate::base::span::NormalizedSpan {
                start_line: 1,
                start_column: 1,
                end_line: 1,
                end_column: 1,
                start_byte: body_start as u32,
                end_byte: body_end as u32,
            };
            let index =
                super::super::syntax_body_hash::SyntaxBodyHashIndex::new(source.as_str(), syntax);
            hashes.push(index.hash(body_span).unwrap());
        }

        assert_eq!(hashes[0], hashes[1]);
        assert_eq!(hashes[1], hashes[2]);
        assert!(lifetime_ranges.iter().all(|ranges| ranges.len() == 5));
        assert_eq!(descriptor_ranges_by_source, lifetime_ranges);

        let source = "fn render<'a>(value: &'a str) -> &'a str { let marker = \"changed\"; let first: &'a str = value; let second: &'a str = first; second }";
        let tree = parse("rust", source);
        let index = super::super::syntax_body_hash::SyntaxBodyHashIndex::new(
            source,
            collect_hash_syntax(tree.root_node(), source, "rust"),
        );
        let body_start = source.find("{ let marker").unwrap();
        let body_end = source.rfind('}').unwrap() + 1;
        let changed_payload_span = crate::base::span::NormalizedSpan {
            start_line: 1,
            start_column: 1,
            end_line: 1,
            end_column: 1,
            start_byte: body_start as u32,
            end_byte: body_end as u32,
        };
        assert_ne!(hashes[0], index.hash(changed_payload_span).unwrap());
    }

    #[test]
    fn lua_extended_comment_node_keeps_its_full_range() {
        let source = "local function render() local marker = '--[=[ literal ]=]' --[=[ extended\ncomment ]=] return marker end";
        let tree = parse("lua", source);
        let syntax = collect_hash_syntax(tree.root_node(), source, "lua");

        assert_eq!(
            syntax.comments,
            vec![source_range(source, "--[=[ extended\ncomment ]=]")]
        );
    }

    fn flat_javascript_source(count: usize) -> String {
        let statements = (0..count)
            .map(|index| format!("  const item_{index} = `item ${{value}}`;"))
            .collect::<Vec<_>>()
            .join("\n");
        format!("function render(value) {{\n{statements}\n  return value;\n}}\n")
    }

    fn nested_javascript_source(depth: usize) -> String {
        let mut expression = "value".to_owned();
        for index in 0..depth {
            expression = format!("`layer_{index} ${{{expression}}}`");
        }
        format!("function render(value) {{ return {expression}; }}\n")
    }

    fn collection_work(source: &str) -> super::CollectionWork {
        let tree = parse("javascript", source);
        let (syntax, work) =
            super::collect_hash_syntax_counted(tree.root_node(), source, "javascript");
        assert!(!syntax.strings.is_empty());
        assert_eq!(
            work.node_visits,
            count_nodes(tree.root_node()),
            "the adapter visits each syntax node exactly once"
        );
        assert_eq!(work.source_range_lookups, syntax.strings.len() * 3);
        assert_eq!(work.source_bytes_scanned, syntax.strings.len() * 4);
        work
    }

    fn count_nodes(root: tree_sitter::Node<'_>) -> usize {
        let mut count = 0;
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            count += 1;
            let mut cursor = node.walk();
            stack.extend(node.children(&mut cursor));
        }
        count
    }

    fn assert_bounded_growth(first: usize, second: usize, third: usize, work: &str) {
        assert!(first <= second, "{work}: {first} > {second}");
        assert!(second <= third, "{work}: {second} > {third}");
        assert!(
            second.saturating_mul(2) <= first.saturating_mul(5),
            "{work}: {first}, {second}"
        );
        assert!(
            third.saturating_mul(2) <= second.saturating_mul(5),
            "{work}: {second}, {third}"
        );
    }

    #[test]
    fn flat_and_nested_ast_collection_work_scales_with_node_and_range_counts() {
        let flat = [32, 64, 128].map(|count| collection_work(&flat_javascript_source(count)));
        let nested = [8, 16, 32].map(|depth| collection_work(&nested_javascript_source(depth)));

        for (work_name, values) in [("flat", flat), ("nested", nested)] {
            assert_bounded_growth(
                values[0].node_visits,
                values[1].node_visits,
                values[2].node_visits,
                work_name,
            );
            assert_bounded_growth(
                values[0].source_range_lookups,
                values[1].source_range_lookups,
                values[2].source_range_lookups,
                work_name,
            );
            assert_bounded_growth(
                values[0].source_bytes_scanned,
                values[1].source_bytes_scanned,
                values[2].source_bytes_scanned,
                work_name,
            );
        }
    }
}

#[derive(Clone, Copy, Default)]
struct NodeContext {
    inside_literal_text: bool,
    string_index: Option<usize>,
}

#[derive(Default)]
struct CollectionWork {
    #[cfg(test)]
    node_visits: usize,
    #[cfg(test)]
    source_range_lookups: usize,
    #[cfg(test)]
    source_bytes_scanned: usize,
}

impl CollectionWork {
    fn visit_node(&mut self) {
        #[cfg(test)]
        {
            self.node_visits += 1;
        }
    }

    fn lookup_range(&mut self) {
        #[cfg(test)]
        {
            self.source_range_lookups += 1;
        }
    }

    fn scan_bytes(&mut self, _byte_count: usize) {
        #[cfg(test)]
        {
            self.source_bytes_scanned += _byte_count;
        }
    }
}

pub(crate) fn supports(language: &str) -> bool {
    matches!(
        language,
        "bash"
            | "dart"
            | "fsharp"
            | "javascript"
            | "jsx"
            | "kotlin"
            | "lua"
            | "powershell"
            | "python"
            | "qml"
            | "ruby"
            | "rust"
            | "scala"
            | "swift"
            | "tsx"
            | "typescript"
    )
}

pub(crate) fn collect_hash_syntax(root: Node<'_>, source: &str, language: &str) -> HashSyntax {
    let mut work = CollectionWork::default();
    collect_hash_syntax_with_work(root, source, language, &mut work)
}

fn collect_hash_syntax_with_work(
    root: Node<'_>,
    source: &str,
    language: &str,
    work: &mut CollectionWork,
) -> HashSyntax {
    if !supports(language) {
        return HashSyntax::default();
    }

    let mut syntax = HashSyntax::default();
    let mut opaque_strings = Vec::new();
    let mut stack = vec![(root, NodeContext::default())];

    while let Some((node, inherited)) = stack.pop() {
        work.visit_node();
        let kind = node.kind();
        let mut context = inherited;

        if is_literal_context(language, kind) {
            context.inside_literal_text = true;
        }

        if is_hash_string_root(language, kind)
            && let Some(range) = valid_node_range(source, node, work)
            && let Some(token_start) = token_start(source, range.clone(), work)
        {
            let index = syntax.strings.len();
            syntax.strings.push(HashString {
                range,
                token_start,
                interpolated: false,
                code: Vec::new(),
            });
            context.string_index = Some(index);
            context.inside_literal_text = true;
        }

        if language == "rust"
            && matches!(kind, "label" | "lifetime")
            && let Some(range) = valid_node_range(source, node, work)
            && let Some(token_start) = token_start(source, range.clone(), work)
        {
            opaque_strings.push(HashString {
                range,
                token_start,
                interpolated: false,
                code: Vec::new(),
            });
        }

        if language != "swift"
            && is_interpolation_node(language, kind)
            && let Some(index) = context.string_index
            && let Some(code) = interpolation_code_range(node, source, language, work)
            && code.start <= code.end
        {
            syntax.strings[index].code.push(code);
            syntax.strings[index].interpolated = true;
            context.inside_literal_text = false;
        }

        if is_comment_node(language, kind)
            && !context.inside_literal_text
            && let Some(range) = valid_node_range(source, node, work)
        {
            syntax.comments.push(range);
        }

        if language == "swift" && is_literal_context(language, kind) {
            let mut inside_literal_text = true;
            let mut interpolation_start = None;
            let child_count = node.child_count();
            for child_index in 0..child_count {
                let Some(child) = node.child(child_index as u32) else {
                    continue;
                };
                let child_kind = child.kind();
                let mut child_context = context;
                child_context.inside_literal_text = inside_literal_text;

                if is_swift_interpolation_open(child_kind) {
                    interpolation_start = Some(child.end_byte());
                    inside_literal_text = false;
                } else if child_kind == ")" {
                    if let Some(start) = interpolation_start.take()
                        && let Some(index) = context.string_index
                        && start <= child.start_byte()
                        && valid_range(source, &(start..child.start_byte()), work)
                    {
                        syntax.strings[index].code.push(start..child.start_byte());
                        syntax.strings[index].interpolated = true;
                    }
                    inside_literal_text = true;
                }

                stack.push((child, child_context));
            }
        } else {
            let mut children = Vec::with_capacity(node.child_count());
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                let mut child_context = context;
                if language != "swift"
                    && child_context.string_index.is_some()
                    && is_interpolation_node(language, child.kind())
                {
                    child_context.inside_literal_text = false;
                }
                children.push((child, child_context));
            }
            stack.extend(children.into_iter().rev());
        }
    }

    syntax.strings.retain(|string| string.interpolated);
    syntax.strings.extend(opaque_strings);
    syntax
}

fn is_comment_node(language: &str, kind: &str) -> bool {
    match language {
        "bash" | "dart" | "javascript" | "jsx" | "lua" | "powershell" | "python" | "qml"
        | "ruby" | "tsx" | "typescript" => kind == "comment",
        "fsharp" => matches!(kind, "line_comment" | "block_comment" | "xml_doc"),
        "kotlin" => matches!(kind, "line_comment" | "block_comment"),
        "rust" => matches!(kind, "line_comment" | "block_comment"),
        "scala" => matches!(kind, "comment" | "block_comment"),
        "swift" => matches!(kind, "comment" | "multiline_comment"),
        _ => false,
    }
}

fn is_literal_context(language: &str, kind: &str) -> bool {
    match language {
        "bash" => matches!(
            kind,
            "string" | "raw_string" | "ansi_c_string" | "heredoc_body"
        ),
        "dart" => kind == "string_literal",
        "fsharp" => matches!(
            kind,
            "string"
                | "triple_quoted_string"
                | "verbatim_string"
                | "bytearray"
                | "verbatim_bytearray"
        ),
        "javascript" | "jsx" | "typescript" | "tsx" | "qml" => {
            matches!(kind, "string" | "template_string")
        }
        "kotlin" => matches!(kind, "string_literal" | "multiline_string_literal"),
        "lua" => kind == "string",
        "powershell" => kind == "string_literal",
        "python" => matches!(kind, "string" | "format_specifier" | "type_conversion"),
        "ruby" => matches!(kind, "string" | "heredoc_body"),
        "rust" => matches!(kind, "string_literal" | "raw_string_literal"),
        "scala" => matches!(kind, "string" | "interpolated_string_expression"),
        "swift" => matches!(
            kind,
            "line_string_literal" | "multi_line_string_literal" | "raw_string_literal"
        ),
        _ => false,
    }
}

fn is_hash_string_root(language: &str, kind: &str) -> bool {
    match language {
        "bash" => kind == "string",
        "dart" => kind == "string_literal",
        "javascript" | "jsx" | "typescript" | "tsx" | "qml" => kind == "template_string",
        "kotlin" => matches!(kind, "string_literal" | "multiline_string_literal"),
        "powershell" => kind == "string_literal",
        "python" => kind == "string",
        "ruby" => kind == "string",
        "scala" => kind == "interpolated_string_expression",
        "swift" => matches!(
            kind,
            "line_string_literal" | "multi_line_string_literal" | "raw_string_literal"
        ),
        _ => false,
    }
}

fn is_interpolation_node(language: &str, kind: &str) -> bool {
    match language {
        "bash" => kind == "command_substitution",
        "dart" => kind == "template_substitution",
        "javascript" | "jsx" | "typescript" | "tsx" | "qml" => kind == "template_substitution",
        "kotlin" | "scala" | "ruby" => kind == "interpolation",
        "python" => matches!(kind, "interpolation" | "format_expression"),
        "powershell" => kind == "sub_expression",
        _ => false,
    }
}

fn is_swift_interpolation_open(kind: &str) -> bool {
    kind.starts_with('\\') && kind.ends_with('(')
}

fn valid_node_range(
    source: &str,
    node: Node<'_>,
    work: &mut CollectionWork,
) -> Option<Range<usize>> {
    let range = node.byte_range();
    valid_range(source, &range, work).then_some(range)
}

fn valid_range(source: &str, range: &Range<usize>, work: &mut CollectionWork) -> bool {
    work.lookup_range();
    range.start <= range.end && source.get(range.clone()).is_some()
}

fn source_slice<'a>(
    source: &'a str,
    range: Range<usize>,
    work: &mut CollectionWork,
) -> Option<&'a str> {
    work.lookup_range();
    source.get(range)
}

fn token_start(source: &str, range: Range<usize>, work: &mut CollectionWork) -> Option<usize> {
    let text = source_slice(source, range.clone(), work)?;
    let mut scanned = 0;
    let quote = text.as_bytes().iter().position(|byte| {
        scanned += 1;
        matches!(byte, b'\'' | b'"' | b'`')
    });
    work.scan_bytes(scanned);
    Some(range.start + quote.unwrap_or(0))
}

fn interpolation_code_range(
    node: Node<'_>,
    source: &str,
    language: &str,
    work: &mut CollectionWork,
) -> Option<Range<usize>> {
    let range = node.byte_range();
    let text = source_slice(source, range.clone(), work)?;

    match language {
        "bash" => delimited_code_range(&range, text, "$(", ")", work)
            .or_else(|| delimited_code_range(&range, text, "`", "`", work)),
        "dart" | "kotlin" | "scala" => {
            if text.starts_with("${") {
                delimited_code_range(&range, text, "${", "}", work)
            } else if text.starts_with('{') {
                delimited_code_range(&range, text, "{", "}", work)
            } else if text.starts_with('$') {
                work.scan_bytes(1);
                Some(range.start + 1..range.end)
            } else {
                Some(range)
            }
        }
        "javascript" | "jsx" | "typescript" | "tsx" | "qml" => {
            delimited_code_range(&range, text, "${", "}", work)
        }
        "powershell" => delimited_code_range(&range, text, "$(", ")", work),
        "python" => python_code_range(node, &range, text, work),
        "ruby" => delimited_code_range(&range, text, "#{", "}", work),
        _ => None,
    }
}

fn delimited_code_range(
    range: &Range<usize>,
    text: &str,
    opening: &str,
    closing: &str,
    work: &mut CollectionWork,
) -> Option<Range<usize>> {
    work.scan_bytes(opening.len());
    if !text.starts_with(opening) {
        return None;
    }
    work.scan_bytes(closing.len());
    if !text.ends_with(closing) {
        return None;
    }
    let start = range.start + opening.len();
    let end = range.end.checked_sub(closing.len())?;
    (start <= end).then_some(start..end)
}

fn python_code_range(
    node: Node<'_>,
    range: &Range<usize>,
    text: &str,
    work: &mut CollectionWork,
) -> Option<Range<usize>> {
    let (start, mut end) = if text.starts_with('{') && text.ends_with('}') {
        work.scan_bytes(2);
        (range.start + 1, range.end.checked_sub(1)?)
    } else {
        (range.start, range.end)
    };

    for index in 0..node.child_count() {
        work.lookup_range();
        if matches!(
            node.field_name_for_child(index as u32),
            Some("type_conversion" | "format_specifier")
        ) && let Some(child) = node.child(index as u32)
        {
            end = end.min(child.start_byte());
        }
    }

    (start <= end && valid_range(text, &(start - range.start..end - range.start), work))
        .then_some(start..end)
}

#[cfg(test)]
fn collect_hash_syntax_counted(
    root: Node<'_>,
    source: &str,
    language: &str,
) -> (HashSyntax, CollectionWork) {
    let mut work = CollectionWork::default();
    let syntax = collect_hash_syntax_with_work(root, source, language, &mut work);
    (syntax, work)
}
