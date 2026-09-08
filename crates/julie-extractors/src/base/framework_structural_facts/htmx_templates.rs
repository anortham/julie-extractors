use std::collections::HashMap;

use serde_json::{Value, json};
use tree_sitter::{Node, Parser};

use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

pub(super) fn enrich_htmx_template(
    language: &str,
    raw: &str,
    metadata: &mut HashMap<String, Value>,
) {
    if !metadata.contains_key("verb") {
        return;
    }
    let dynamic_binding =
        metadata.get("value_source").and_then(Value::as_str) == Some("dynamic_expression");
    let raw = raw.trim();
    let expression = raw
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}'))
        .unwrap_or(raw)
        .trim();
    let (source, begin, end, grammar, kinds) = if language == "razor"
        && !metadata.contains_key("value_source")
        && raw.contains('@')
        && !raw.starts_with('$')
    {
        let prefix = "<a hx-get=\"";
        (
            format!("{prefix}{raw}\" />"),
            prefix.len(),
            prefix.len() + raw.len(),
            tree_sitter_razor::LANGUAGE.into(),
            vec!["razor_implicit_expression", "razor_explicit_expression"],
        )
    } else if language == "razor"
        && dynamic_binding
        && (raw.starts_with("$\"") || raw.starts_with("$@\""))
        && raw.ends_with('"')
    {
        let prefix = "class C { object Value = ";
        let quote = raw.find('"').unwrap();
        (
            format!("{prefix}{raw}; }}"),
            prefix.len() + quote + 1,
            prefix.len() + raw.len() - 1,
            tree_sitter_c_sharp::LANGUAGE.into(),
            vec!["interpolation"],
        )
    } else if matches!(
        language,
        "javascript" | "jsx" | "typescript" | "tsx" | "vue" | "html"
    ) && dynamic_binding
        && expression.starts_with('`')
        && expression.ends_with('`')
    {
        let prefix = "const value = ";
        (
            format!("{prefix}{expression};"),
            prefix.len() + 1,
            prefix.len() + expression.len() - 1,
            tree_sitter_javascript::LANGUAGE.into(),
            vec!["template_substitution"],
        )
    } else {
        return;
    };
    let mut parser = Parser::new();
    if parser.set_language(&grammar).is_err() {
        return;
    }
    let Some(tree) = parser.parse(&source, None) else {
        return;
    };
    if tree.root_node().has_error() && language != "razor" {
        metadata.insert("route_template_uncertainty".into(), json!("unknown"));
        return;
    }
    let mut spans = Vec::new();
    if !collect_spans(tree.root_node(), begin, end, &kinds, &mut spans, 0) {
        metadata.insert("route_template_uncertainty".into(), json!("unknown"));
        return;
    }
    if spans.is_empty() {
        if tree.root_node().has_error() {
            metadata.insert("route_template_uncertainty".into(), json!("unknown"));
        }
        return;
    }
    spans.sort_unstable();
    let mut segments = Vec::new();
    let mut template = String::new();
    let mut position = begin;
    for (start, finish) in spans {
        if start < position {
            continue;
        }
        if position < start {
            let literal = &source[position..start];
            segments.push(json!({"kind":"literal", "text":literal}));
            template.push_str(literal);
        }
        segments.push(json!({"kind":"dynamic", "text":&source[start..finish]}));
        template.push_str(":dynamic");
        position = finish;
    }
    if position < end {
        segments.push(json!({"kind":"literal", "text":&source[position..end]}));
        template.push_str(&source[position..end]);
    }
    metadata.insert("route_template_segments".into(), json!(segments));
    metadata.insert("route_template_uncertainty".into(), json!("partial"));
    if template.starts_with('/') {
        metadata.insert("normalized_route_template".into(), json!(template));
    }
}

fn collect_spans(
    node: Node<'_>,
    begin: usize,
    end: usize,
    kinds: &[&str],
    spans: &mut Vec<(usize, usize)>,
    depth: u32,
) -> bool {
    if !should_visit_tree_depth(depth) {
        return false;
    }
    if node.start_byte() >= begin
        && node.end_byte() <= end
        && kinds.contains(&node.kind())
        && !node.has_error()
    {
        let mut finish = node.end_byte();
        if node.kind() == "razor_implicit_expression" {
            let mut expression = node.named_child(0).unwrap_or(node);
            while expression.kind() == "binary_expression" {
                let Some(left) = expression.child_by_field_name("left") else {
                    break;
                };
                expression = left;
            }
            finish = expression.end_byte();
        }
        spans.push((node.start_byte(), finish));
        return true;
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.end_byte() > begin && child.start_byte() < end {
            let Some(child_depth) = child_tree_depth(depth) else {
                return false;
            };
            if !collect_spans(child, begin, end, kinds, spans, child_depth) {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_span_collection_reports_depth_exhaustion_without_partial_spans() {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_javascript::LANGUAGE.into())
            .unwrap();
        let source = "const value = `/items/${id}`;";
        let tree = parser.parse(source, None).unwrap();
        let mut spans = Vec::new();
        assert!(!collect_spans(
            tree.root_node(),
            0,
            source.len(),
            &["template_substitution"],
            &mut spans,
            crate::tree_traversal::TREE_TRAVERSAL_DEPTH_LIMIT,
        ));
        assert!(spans.is_empty());
        assert!(collect_spans(
            tree.root_node(),
            0,
            source.len(),
            &["template_substitution"],
            &mut spans,
            0
        ));
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn agent_usefulness_templates_preserve_suffix_and_mark_arbitrary_path_segments_partial() {
        for (language, value) in [
            ("razor", "/workspaces/@Esc(Model.Id)/tests/start"),
            ("razor", "/workspaces/@(Esc(Model.Id))/tests/start"),
            (
                "razor",
                r#"$"/workspaces/{Escape(Compute(Id))}/tests/start""#,
            ),
            (
                "javascript",
                "`/workspaces/${escape(compute(id))}/tests/start`",
            ),
            ("jsx", "{`/workspaces/${id}/tests/start`}"),
            ("typescript", "`/workspaces/${id}/tests/start`"),
            ("tsx", "{`/workspaces/${id}/tests/start`}"),
            ("vue", "`/workspaces/${id}/tests/start`"),
            ("html", "`/workspaces/${id}/tests/start`"),
        ] {
            let mut metadata = HashMap::from([("verb".into(), json!("POST"))]);
            if language != "razor" || value.starts_with('$') {
                metadata.insert("value_source".into(), json!("dynamic_expression"));
            }
            enrich_htmx_template(language, value, &mut metadata);
            assert_eq!(
                metadata.get("normalized_route_template"),
                Some(&json!("/workspaces/:dynamic/tests/start")),
                "{language}: {value}: {metadata:?}"
            );
            assert_eq!(
                metadata.get("route_template_uncertainty"),
                Some(&json!("partial"))
            );
        }
    }

    #[test]
    fn agent_usefulness_direct_jsx_template_uses_complete_parsed_expression() {
        let source = r#"export const view = <button hx-post={`/items/${get("a)/b", id)}/run`} />;"#;
        for path in ["View.js", "View.jsx", "View.tsx"] {
            let result =
                crate::pipeline::extract_canonical(path, source, std::path::Path::new("/repo"))
                    .unwrap();
            let facts: Vec<_> = result
                .structural_facts
                .iter()
                .filter(|fact| fact.pattern_id == "htmx.attribute.v1")
                .collect();
            assert_eq!(facts.len(), 1, "{path}: {:?}", result.structural_facts);
            let metadata = facts[0].metadata.as_ref().unwrap();
            assert_eq!(
                metadata.get("normalized_route_template"),
                Some(&json!("/items/:dynamic/run"))
            );
            assert_eq!(
                metadata.get("route_template_uncertainty"),
                Some(&json!("partial"))
            );
            assert_eq!(
                metadata.get("attribute_value"),
                Some(&json!(r#"{`/items/${get("a)/b", id)}/run`}"#))
            );
        }
    }

    #[test]
    fn agent_usefulness_templates_parse_quoted_parentheses_and_slashes_without_guessing() {
        for value in [
            r#"`/items/${get("a)/b")}/run`"#,
            r#"`/items/${id ? "a/b" : "c"}/run`"#,
        ] {
            let mut metadata = HashMap::from([
                ("verb".into(), json!("POST")),
                ("value_source".into(), json!("dynamic_expression")),
            ]);
            enrich_htmx_template("javascript", value, &mut metadata);
            assert_eq!(
                metadata.get("normalized_route_template"),
                Some(&json!("/items/:dynamic/run"))
            );
            assert_eq!(
                metadata.get("route_template_uncertainty"),
                Some(&json!("partial"))
            );
        }
        let mut metadata = HashMap::from([("verb".into(), json!("GET"))]);
        enrich_htmx_template("html", "/people/@name/start", &mut metadata);
        assert!(!metadata.contains_key("normalized_route_template"));
        enrich_htmx_template("html", "`/items/${id}/run`", &mut metadata);
        assert!(!metadata.contains_key("normalized_route_template"));
        metadata.insert("value_source".into(), json!("string_literal"));
        enrich_htmx_template("razor", "\"/items/@Esc(Id)/run\"", &mut metadata);
        assert!(!metadata.contains_key("normalized_route_template"));
    }
}
