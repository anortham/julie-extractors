#[cfg(test)]
use super::syntax_body_hash::SyntaxBodyHashIndex;
use super::syntax_body_hash::{HashString, HashSyntax};
use std::ops::Range;
#[cfg(test)]
use std::sync::Arc;
use tree_sitter::Node;

pub(crate) fn collect_csharp_hash_syntax(root: Node<'_>, source: &str, razor: bool) -> HashSyntax {
    collect_csharp_hash_syntax_with_work(root, source, razor, |_| {})
}

fn collect_csharp_hash_syntax_with_work(
    root: Node<'_>,
    source: &str,
    razor: bool,
    mut record_work: impl FnMut(usize),
) -> HashSyntax {
    let mut comments = Vec::new();
    let mut strings = Vec::new();
    let mut stack = vec![(root, None)];

    while let Some((node, parent_string)) = stack.pop() {
        record_work(1);
        if is_comment_kind(node.kind(), razor) {
            comments.push(node.start_byte()..node.end_byte());
            continue;
        }

        if razor && matches!(node.kind(), "element" | "component_attribute_value") {
            collect_razor_attribute_hash_string(node, source, &mut strings, &mut record_work);
        }

        if razor && matches!(node.kind(), "compilation_unit" | "element") {
            collect_razor_host_text_hash_strings(node, source, &mut strings, &mut record_work);
        }

        let string_index = if node.kind() == "interpolated_string_expression" {
            let range = node.byte_range();
            let quote_offset = source.get(range.clone()).and_then(|text| text.find('"'));
            if let Some(offset) = quote_offset {
                record_work(offset + 1);
            }
            let token_start = quote_offset
                .map(|offset| range.start + offset)
                .unwrap_or(range.start);
            strings.push(HashString {
                range,
                token_start,
                interpolated: true,
                code: Vec::new(),
            });
            Some(strings.len() - 1)
        } else {
            parent_string
        };

        if node.kind() == "interpolation"
            && let Some(index) = parent_string
            && let Some((code, inspected_bytes)) = interpolation_code_ranges(node, source)
        {
            record_work(inspected_bytes);
            strings[index].code.extend(code);
        }

        for child_index in (0..node.child_count()).rev() {
            let child_index = u32::try_from(child_index)
                .expect("tree-sitter child indices fit the node child-index API");
            if let Some(child) = node.child(child_index) {
                stack.push((child, string_index));
            }
        }
    }

    HashSyntax { comments, strings }
}

fn collect_razor_host_text_hash_strings(
    node: Node<'_>,
    source: &str,
    strings: &mut Vec<HashString>,
    record_work: &mut impl FnMut(usize),
) {
    let mut cursor = node.start_byte();
    let end = node.end_byte();

    for child_index in 0..node.child_count() {
        record_work(1);
        let child_index = u32::try_from(child_index)
            .expect("tree-sitter child indices fit the node child-index API");
        let Some(child) = node.child(child_index) else {
            continue;
        };
        let child_start = child.start_byte().clamp(cursor, end);
        push_razor_quote_bearing_host_text(cursor..child_start, source, strings, record_work);
        cursor = cursor.max(child.end_byte()).min(end);
    }

    push_razor_quote_bearing_host_text(cursor..end, source, strings, record_work);
}

fn push_razor_quote_bearing_host_text(
    range: Range<usize>,
    source: &str,
    strings: &mut Vec<HashString>,
    record_work: &mut impl FnMut(usize),
) {
    if range.is_empty() {
        return;
    }

    let Some(text) = source.get(range.clone()) else {
        return;
    };
    let inspected_bytes = text.char_indices().find_map(|(index, character)| {
        matches!(character, '\'' | '"' | '`').then_some(index + character.len_utf8())
    });
    record_work(inspected_bytes.unwrap_or(text.len()));
    if inspected_bytes.is_none() {
        return;
    }

    strings.push(HashString {
        token_start: range.start,
        range,
        interpolated: false,
        code: Vec::new(),
    });
}

fn collect_razor_attribute_hash_string(
    node: Node<'_>,
    source: &str,
    strings: &mut Vec<HashString>,
    record_work: &mut impl FnMut(usize),
) {
    let mut opening_quote = None;
    let mut code = Vec::new();

    for child_index in 0..node.child_count() {
        record_work(1);
        let child_index = u32::try_from(child_index)
            .expect("tree-sitter child indices fit the node child-index API");
        let Some(child) = node.child(child_index) else {
            continue;
        };
        let range = child.byte_range();
        let quote = source.get(range.clone()).and_then(|text| match text {
            "\"" => Some('"'),
            "'" => Some('\''),
            _ => None,
        });

        if let Some(quote) = quote {
            if let Some((open_quote, start)) = opening_quote.take() {
                if open_quote == quote && !code.is_empty() {
                    strings.push(HashString {
                        range: start..child.end_byte(),
                        token_start: start,
                        interpolated: true,
                        code: std::mem::take(&mut code),
                    });
                }
                code.clear();
            } else {
                opening_quote = Some((quote, child.start_byte()));
                code.clear();
            }
        } else if opening_quote.is_some()
            && matches!(
                child.kind(),
                "razor_explicit_expression" | "razor_implicit_expression"
            )
        {
            code.push(range);
        }
    }
}

fn is_comment_kind(kind: &str, razor: bool) -> bool {
    if razor {
        matches!(
            kind,
            "comment" | "documentation_comment" | "razor_comment" | "html_comment"
        )
    } else {
        matches!(kind, "comment" | "documentation_comment")
    }
}

fn interpolation_code_ranges(node: Node<'_>, source: &str) -> Option<(Vec<Range<usize>>, usize)> {
    let mut braces = Vec::new();
    let mut alignment = None;
    let mut format = None;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "interpolation_brace" => braces.push(child.byte_range()),
            "interpolation_alignment_clause" => alignment = Some(child.byte_range()),
            "interpolation_format_clause" => format = Some(child.byte_range()),
            _ => {}
        }
    }
    let opening = braces.first()?.clone();
    let closing = braces.last()?.clone();
    let opening_text = source.get(opening.clone())?;
    let closing_text = source.get(closing.clone())?;
    let opening_end = opening_text
        .rfind('{')
        .map(|offset| opening.start + offset + 1)?;
    let closing_start = closing_text
        .find('}')
        .map(|offset| closing.start + offset)?;
    let expression_end = alignment
        .as_ref()
        .map(|range| range.start)
        .or_else(|| format.as_ref().map(|range| range.start))
        .unwrap_or(closing_start)
        .min(closing_start);
    if opening_end > expression_end {
        return None;
    }

    let mut code = Vec::with_capacity(2);
    code.push(opening_end..expression_end);
    if let Some(alignment) = alignment {
        code.push(alignment);
    }
    Some((code, opening_text.len() + closing_text.len()))
}

#[cfg(test)]
pub(crate) struct CSharpBodyHashIndex {
    index: SyntaxBodyHashIndex,
    collector_work: usize,
}

#[cfg(test)]
impl CSharpBodyHashIndex {
    pub(crate) fn for_csharp(root: Node<'_>, source: impl Into<Arc<str>>) -> Self {
        Self::new(root, source.into(), false)
    }

    pub(crate) fn for_razor(root: Node<'_>, source: impl Into<Arc<str>>) -> Self {
        Self::new(root, source.into(), true)
    }

    fn new(root: Node<'_>, source: Arc<str>, razor: bool) -> Self {
        let mut collector_work = 0;
        let syntax = collect_csharp_hash_syntax_with_work(root, &source, razor, |units| {
            collector_work += units;
        });
        let index = SyntaxBodyHashIndex::new(source, syntax);
        Self {
            index,
            collector_work,
        }
    }

    pub(crate) fn hash(&self, span: crate::base::BodySpan) -> Option<String> {
        self.index.hash(span)
    }

    fn hash_with_work(&self, span: crate::base::BodySpan) -> (Option<String>, usize) {
        self.index.hash_with_work(span)
    }

    fn build_work(&self) -> usize {
        self.collector_work + self.index.build_work()
    }

    fn token_count(&self) -> usize {
        self.index.token_count()
    }
}

#[cfg(test)]
mod tests {
    use super::{CSharpBodyHashIndex, collect_csharp_hash_syntax};
    use crate::base::{
        NormalizedSpan,
        body::body_hash,
        syntax_body_hash::{HashSyntax, SyntaxBodyHashIndex},
    };

    fn whole_file(source: &str) -> NormalizedSpan {
        NormalizedSpan::from_content_range(source, 0, source.len()).expect("whole file span")
    }

    fn method_statement_span(source: &str) -> NormalizedSpan {
        let start = source.find("return ").expect("return statement");
        let end = source[start..].find(';').expect("statement end") + start + 1;
        NormalizedSpan::from_content_range(source, start, end).expect("statement span")
    }

    fn nested_interpolation_source(depth: usize, expression: &str) -> String {
        let mut expression = expression.to_owned();
        for _ in 0..depth {
            expression = format!("string.Concat($\"segment {{{expression}}}\")");
        }
        format!(
            "class Sample {{ string Render(int value) {{ return $\"root {{{expression}}}\"; }} }}"
        )
    }

    #[test]
    fn csharp_hash_context_matches_existing_non_interpolated_hashes() {
        let source = "class Sample { string Render() { // comment\r\n return \"http://host/*literal*/\"; } }\r\n";
        let tree = crate::tests::helpers::init_parser(source, "csharp");
        let index = CSharpBodyHashIndex::for_csharp(tree.root_node(), source);
        assert_eq!(
            index.hash(whole_file(source)),
            body_hash(source, whole_file(source), "csharp")
        );
    }

    #[test]
    fn razor_hash_context_matches_existing_non_interpolated_hashes() {
        let source = "<a href=http://host/path>λ</a>\r\n@code { string Render() { return \"http://api/path\"; } }\r\n";
        let tree = crate::tests::helpers::init_parser(source, "razor");
        let index = CSharpBodyHashIndex::for_razor(tree.root_node(), source);
        assert_eq!(
            index.hash(whole_file(source)),
            body_hash(source, whole_file(source), "razor")
        );
    }

    #[test]
    fn csharp_interpolated_literal_without_holes_keeps_its_syntax_classification() {
        let source = "class Sample { string Render() { return $\"plain\"; } }";
        let tree = crate::tests::helpers::init_parser(source, "csharp");
        assert!(!tree.root_node().has_error());
        let syntax = collect_csharp_hash_syntax(tree.root_node(), source, false);
        assert_eq!(syntax.strings.len(), 1);
        assert!(syntax.strings[0].interpolated);
        assert!(syntax.strings[0].code.is_empty());

        let with_syntax =
            CSharpBodyHashIndex::for_csharp(tree.root_node(), source).hash(whole_file(source));
        let without_syntax =
            SyntaxBodyHashIndex::new(source, HashSyntax::default()).hash(whole_file(source));
        assert_ne!(with_syntax, without_syntax);
    }

    #[test]
    fn razor_host_quote_ranges_include_top_level_and_element_text_without_capturing_code() {
        let source = "it's top-level text\n<p>it's element text</p>@code { string Render(int value) { return $\"{value /* c */}\"; } }";
        let tree = crate::tests::helpers::init_parser(source, "razor");
        assert!(
            !tree.root_node().has_error(),
            "{}",
            tree.root_node().to_sexp()
        );

        let syntax = collect_csharp_hash_syntax(tree.root_node(), source, true);
        let host_text = syntax
            .strings
            .iter()
            .filter(|string| !string.interpolated)
            .filter_map(|string| source.get(string.range.clone()))
            .collect::<Vec<_>>();
        assert!(
            host_text
                .iter()
                .any(|text| text.contains("it's top-level text"))
        );
        assert!(
            host_text
                .iter()
                .any(|text| text.contains("it's element text"))
        );
        assert!(host_text.iter().all(|text| !text.contains("value")));

        let interpolation = syntax
            .strings
            .iter()
            .find(|string| string.interpolated)
            .expect("C# interpolation descriptor");
        assert!(interpolation.code.iter().any(|range| {
            source
                .get(range.clone())
                .is_some_and(|text| text.contains("value"))
        }));
        assert!(syntax.comments.iter().any(|range| {
            source
                .get(range.clone())
                .is_some_and(|text| text == "/* c */")
        }));
    }

    #[test]
    fn csharp_interpolation_hashes_ignore_code_formatting_and_comments() {
        let sources = [
            "class Sample { string Render(int a, int b) { return $\"value {a+b,10:N2}\"; } }",
            "class Sample { string Render(int a, int b) { return $\"value { a + b ,10:N2}\"; } }",
            "class Sample { string Render(int a, int b) { return $\"value { a /* comment */ + b ,10:N2}\"; } }",
        ];
        let hashes: Vec<_> = sources
            .iter()
            .map(|source| {
                let tree = crate::tests::helpers::init_parser(source, "csharp");
                assert!(!tree.root_node().has_error());
                CSharpBodyHashIndex::for_csharp(tree.root_node(), *source)
                    .hash(whole_file(source))
                    .expect("body hash")
            })
            .collect();
        assert!(
            hashes.windows(2).all(|pair| pair[0] == pair[1]),
            "{hashes:?}"
        );
    }

    #[test]
    fn csharp_multiple_interpolations_ignore_each_code_region_comment_and_spacing() {
        let sources = [
            "class Sample { string Render(int left, int right) { return $\"{left+right:N2} {left*right:N1}\"; } }",
            "class Sample { string Render(int left, int right) { return $\"{ left + right /* first */ :N2} { left * right /* second */ :N1}\"; } }",
        ];
        let hashes: Vec<_> = sources
            .iter()
            .map(|source| {
                let tree = crate::tests::helpers::init_parser(source, "csharp");
                assert!(
                    !tree.root_node().has_error(),
                    "{}",
                    tree.root_node().to_sexp()
                );
                CSharpBodyHashIndex::for_csharp(tree.root_node(), *source)
                    .hash(whole_file(source))
                    .expect("body hash")
            })
            .collect();
        assert_eq!(hashes[0], hashes[1]);

        let changed = sources[0].replace("left*right", "left/right");
        let tree = crate::tests::helpers::init_parser(&changed, "csharp");
        assert!(
            !tree.root_node().has_error(),
            "{}",
            tree.root_node().to_sexp()
        );
        assert_ne!(
            CSharpBodyHashIndex::for_csharp(tree.root_node(), changed.as_str())
                .hash(whole_file(&changed)),
            Some(hashes[0].clone())
        );
    }

    #[test]
    fn razor_interpolated_attribute_code_normalizes_comments_and_preserves_literal_text() {
        let plain = r#"<FluentCard Class="@($"area-card {area.SemanticClass}")" />"#;
        let commented = r#"<FluentCard Class="@($"area-card {area /* c */ .SemanticClass}")" />"#;
        let hash = |source: &str| {
            let tree = crate::tests::helpers::init_parser(source, "razor");
            assert!(
                !tree.root_node().has_error(),
                "{}",
                tree.root_node().to_sexp()
            );
            CSharpBodyHashIndex::for_razor(tree.root_node(), source)
                .hash(whole_file(source))
                .expect("Razor attribute source hash")
        };

        let plain_hash = hash(plain);
        assert_eq!(plain_hash, hash(commented));
        assert_ne!(plain_hash, hash(&plain.replace("area-card", "alert-card")));
        assert_ne!(plain_hash, hash(&plain.replace("area-card ", "area-card")));
    }

    #[test]
    fn csharp_nested_interpolation_preserves_literal_and_normalizes_expression_code() {
        let sources = [
            "class Sample { string Render(int value) { return $\"outer {string.Concat($\"inner {value}\")}\"; } }",
            "class Sample { string Render(int value) { return $\"outer { string.Concat($\"inner { value /* comment */ }\") }\"; } }",
        ];
        let hashes: Vec<_> = sources
            .iter()
            .map(|source| {
                let tree = crate::tests::helpers::init_parser(source, "csharp");
                assert!(
                    !tree.root_node().has_error(),
                    "{}",
                    tree.root_node().to_sexp()
                );
                let index = CSharpBodyHashIndex::for_csharp(tree.root_node(), *source);
                index.hash(whole_file(source)).expect("body hash")
            })
            .collect();
        assert_eq!(hashes[0], hashes[1]);

        let changed_literal = sources[0].replace("inner", "inner ");
        let tree = crate::tests::helpers::init_parser(&changed_literal, "csharp");
        assert_ne!(
            CSharpBodyHashIndex::for_csharp(tree.root_node(), changed_literal.as_str())
                .hash(whole_file(&changed_literal)),
            Some(hashes[0].clone())
        );
    }

    #[test]
    fn csharp_interpolation_literals_and_format_text_remain_significant() {
        let sources = [
            "class Sample { string Render(int value) { return $\"value {value,10:N2}\"; } }",
            "class Sample { string Render(int value) { return $\"value  {value,10:N2}\"; } }",
            "class Sample { string Render(int value) { return $\"value {value,11:N2}\"; } }",
            "class Sample { string Render(int value) { return $\"value {value,10:N3}\"; } }",
            "class Sample { string Render(int value) { return $\"value {value+1,10:N2}\"; } }",
        ];
        let hashes: Vec<_> = sources
            .iter()
            .map(|source| {
                let tree = crate::tests::helpers::init_parser(source, "csharp");
                CSharpBodyHashIndex::for_csharp(tree.root_node(), *source)
                    .hash(whole_file(source))
                    .expect("body hash")
            })
            .collect();
        assert!(
            hashes.windows(2).all(|pair| pair[0] != pair[1]),
            "{hashes:?}"
        );
    }

    #[test]
    fn csharp_alignment_code_spacing_normalizes_but_format_text_spacing_remains_significant() {
        let sources = [
            "class Sample { string Render(int value) { return $\"value {value,10:N2}\"; } }",
            "class Sample { string Render(int value) { return $\"value { value , 10:N2}\"; } }",
            "class Sample { string Render(int value) { return $\"value { value , 10: N2}\"; } }",
        ];
        let hashes: Vec<_> = sources
            .iter()
            .map(|source| {
                let tree = crate::tests::helpers::init_parser(source, "csharp");
                assert!(
                    !tree.root_node().has_error(),
                    "{}",
                    tree.root_node().to_sexp()
                );
                CSharpBodyHashIndex::for_csharp(tree.root_node(), *source)
                    .hash(whole_file(source))
                    .expect("body hash")
            })
            .collect();
        assert_eq!(hashes[0], hashes[1]);
        assert_ne!(hashes[0], hashes[2]);
    }

    #[test]
    fn csharp_hashes_partial_interpolation_lambda_bodies() {
        let hash = |comment: &str, value: &str| {
            let source = r#"class Sample { string Render() { return $"{((Func<int>)(() => { return COMMENTVALUE; }))()}"; } }"#
                .replace("COMMENT", comment)
                .replace("VALUE", value);
            let tree = crate::tests::helpers::init_parser(&source, "csharp");
            assert!(
                !tree.root_node().has_error(),
                "{}",
                tree.root_node().to_sexp()
            );
            let lambda_body_start = source.find("() => { return").expect("lambda body") + 6;
            let lambda_body_end = source[lambda_body_start..]
                .find('}')
                .expect("lambda body end")
                + lambda_body_start
                + 1;
            let lambda_body =
                NormalizedSpan::from_content_range(&source, lambda_body_start, lambda_body_end)
                    .expect("lambda span");
            CSharpBodyHashIndex::for_csharp(tree.root_node(), source.as_str())
                .hash(lambda_body)
                .expect("lambda body hash")
        };

        let plain = hash("", "1");
        let short_comment = hash("/* short */ ", "1");
        let long_comment = hash("/* a longer comment */ ", "1");
        let changed_value = hash("", "2");
        assert_eq!(plain, short_comment);
        assert_eq!(plain, long_comment);
        assert_ne!(plain, changed_value);
    }

    #[test]
    fn csharp_interpolated_verbatim_and_raw_strings_keep_literal_content() {
        let sources = [
            r#"class Sample { string Render(int value) { return @$"literal ""{value}"""; } }"#,
            r#"class Sample { string Render(int value) { return @$"literal ""{ value }"""; } }"#,
            r###"class Sample { string Render(int value) { return $$"""literal {{value}}"""; } }"###,
            r###"class Sample { string Render(int value) { return $$"""literal  {{value}}"""; } }"###,
        ];
        let hashes: Vec<_> = sources
            .iter()
            .map(|source| {
                let tree = crate::tests::helpers::init_parser(source, "csharp");
                assert!(
                    !tree.root_node().has_error(),
                    "{}",
                    tree.root_node().to_sexp()
                );
                CSharpBodyHashIndex::for_csharp(tree.root_node(), *source)
                    .hash(whole_file(source))
                    .expect("body hash")
            })
            .collect();
        assert_eq!(hashes[0], hashes[1]);
        assert_ne!(hashes[2], hashes[3]);
    }

    #[test]
    fn csharp_hash_index_build_work_scales_with_flat_and_nested_source() {
        let work = |count: usize, nested: bool| {
            let mut source = String::from("class Sample { void Render() {\n");
            for index in 0..count {
                if nested {
                    source.push_str(&format!("if (flag{index}) {{ value{index}++; }}\n"));
                } else {
                    source.push_str(&format!("value{index}++;\n"));
                }
            }
            source.push_str("}\n}");
            let tree = crate::tests::helpers::init_parser(&source, "csharp");
            let index = CSharpBodyHashIndex::for_csharp(tree.root_node(), source.as_str());
            assert!(index.hash(whole_file(&source)).is_some());
            index.build_work()
        };

        for nested in [false, true] {
            let sizes = [128, 256, 512].map(|count| work(count, nested));
            assert!(sizes[1] as f64 / sizes[0] as f64 <= 2.5, "{sizes:?}");
            assert!(sizes[2] as f64 / sizes[1] as f64 <= 2.5, "{sizes:?}");
        }
    }

    #[test]
    fn razor_quote_gap_scan_work_scales_with_flat_and_nested_markup() {
        let source = |count: usize, nested: bool| {
            if nested {
                let mut content = String::new();
                for index in 0..count {
                    content = format!("<div>it's nested {index} {content}</div>");
                }
                content
            } else {
                let mut content = String::new();
                for index in 0..count {
                    content.push_str(&format!("<p>it's flat {index}</p>"));
                }
                content
            }
        };
        let work = |source: &str| {
            let tree = crate::tests::helpers::init_parser(source, "razor");
            assert!(
                !tree.root_node().has_error(),
                "{}",
                tree.root_node().to_sexp()
            );
            let index = CSharpBodyHashIndex::for_razor(tree.root_node(), source);
            assert!(index.hash(whole_file(source)).is_some());
            index.build_work()
        };

        for nested in [false, true] {
            let sizes = [64, 128, 256].map(|count| work(&source(count, nested)));
            assert!(sizes[1] as f64 / sizes[0] as f64 <= 2.5, "{sizes:?}");
            assert!(sizes[2] as f64 / sizes[1] as f64 <= 2.5, "{sizes:?}");
        }
    }

    #[test]
    fn csharp_deep_interpolation_build_work_scales_with_actual_nested_strings() {
        let build = |depth: usize, expression: &str| {
            let source = nested_interpolation_source(depth, expression);
            let tree = crate::tests::helpers::init_parser(&source, "csharp");
            assert!(
                !tree.root_node().has_error(),
                "{}",
                tree.root_node().to_sexp()
            );
            let span = method_statement_span(&source);
            let statement = source
                .get(span.start_byte as usize..span.end_byte as usize)
                .expect("statement slice");
            assert!(statement.starts_with("return "));
            assert!(statement.ends_with(';'));
            assert!(statement.contains("root "));
            let index = CSharpBodyHashIndex::for_csharp(tree.root_node(), source.as_str());
            let hash = index.hash(span).expect("nested statement hash");
            assert!(!hash.is_empty());
            index.build_work()
        };

        let sizes = [8, 16, 32].map(|depth| build(depth, "value"));
        assert!(sizes[1] as f64 / sizes[0] as f64 <= 2.5, "{sizes:?}");
        assert!(sizes[2] as f64 / sizes[1] as f64 <= 2.5, "{sizes:?}");

        let plain = nested_interpolation_source(16, "value");
        let comment = nested_interpolation_source(16, " value /* comment */ ");
        let changed = nested_interpolation_source(16, "value + 1");
        let hash = |source: &str| {
            let tree = crate::tests::helpers::init_parser(source, "csharp");
            assert!(
                !tree.root_node().has_error(),
                "{}",
                tree.root_node().to_sexp()
            );
            CSharpBodyHashIndex::for_csharp(tree.root_node(), source)
                .hash(method_statement_span(source))
                .expect("statement hash")
        };
        assert_eq!(hash(&plain), hash(&comment));
        assert_ne!(hash(&plain), hash(&changed));
    }

    #[test]
    fn csharp_flat_interpolation_build_work_scales_with_string_count() {
        let build = |count: usize| {
            let mut source = String::from("class Sample { string Render() { return $\"");
            for index in 0..count {
                source.push_str(&format!("part {{{{value{index}}}}}"));
            }
            source.push_str("\"; } }");
            let tree = crate::tests::helpers::init_parser(&source, "csharp");
            assert!(
                !tree.root_node().has_error(),
                "{}",
                tree.root_node().to_sexp()
            );
            let index = CSharpBodyHashIndex::for_csharp(tree.root_node(), source.as_str());
            assert!(index.hash(method_statement_span(&source)).is_some());
            index.build_work()
        };

        let sizes = [64, 128, 256].map(build);
        assert!(sizes[1] as f64 / sizes[0] as f64 <= 2.5, "{sizes:?}");
        assert!(sizes[2] as f64 / sizes[1] as f64 <= 2.5, "{sizes:?}");
    }

    #[test]
    fn csharp_body_hash_queries_scale_with_selected_tokens_and_partial_ranges() {
        let query_work = |count: usize| {
            let mut source = String::from(
                "class Sample { string Focus(int value) { return $\"selected {value}\"; }\n",
            );
            for index in 0..count {
                source.push_str(&format!("void Ignore{index}() {{ value{index}++; }}\n"));
            }
            source.push('}');
            let tree = crate::tests::helpers::init_parser(&source, "csharp");
            assert!(
                !tree.root_node().has_error(),
                "{}",
                tree.root_node().to_sexp()
            );
            let span = method_statement_span(&source);
            let index = CSharpBodyHashIndex::for_csharp(tree.root_node(), source.as_str());
            let (hash, work) = index.hash_with_work(span);
            assert!(hash.is_some());
            (work, index.token_count())
        };

        let sizes = [128, 256, 512].map(query_work);
        assert!(sizes[1].0 as f64 / sizes[0].0 as f64 <= 1.5, "{sizes:?}");
        assert!(sizes[2].0 as f64 / sizes[1].0 as f64 <= 1.5, "{sizes:?}");

        let source = r#"class Sample { string Render() { return $"{((Func<int>)(() => { return 1; }))()}"; } }"#;
        let tree = crate::tests::helpers::init_parser(source, "csharp");
        assert!(
            !tree.root_node().has_error(),
            "{}",
            tree.root_node().to_sexp()
        );
        let start = source.find("() => {").expect("lambda") + 6;
        let end = source[start..].find("}").expect("lambda end") + start + 1;
        let span = NormalizedSpan::from_content_range(source, start, end).expect("lambda span");
        let index = CSharpBodyHashIndex::for_csharp(tree.root_node(), source);
        let (hash, work) = index.hash_with_work(span);
        assert!(hash.is_some());
        assert!(
            work <= (span.end_byte - span.start_byte) as usize * 3 + 128,
            "{work}"
        );
    }
}
