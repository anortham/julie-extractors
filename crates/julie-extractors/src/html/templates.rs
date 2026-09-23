//! Server-side template statements (Jinja, Django, Nunjucks, Twig): the
//! `{% extends %}`, `{% include %}`, `{% import %}`, and `{% from %}` imports,
//! and `{% block %}` / `{% macro %}` symbols.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;
use tree_sitter::Node;

use crate::base::relationship_resolution::{StructuredPendingRelationship, UnresolvedTarget};
use crate::base::{BaseExtractor, RelationshipKind, Symbol, SymbolKind, SymbolOptions, Visibility};

static TEMPLATE_TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)\{%-?\s*(\w+)(.*?)-?%\}").unwrap());
static QUOTED_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""([^"]+)"|'([^']+)'"#).unwrap());

struct TemplateTag<'c> {
    start: usize,
    end: usize,
    keyword: &'c str,
    arguments: &'c str,
}

fn template_tags(content: &str) -> Vec<TemplateTag<'_>> {
    if !content.contains("{%") {
        return Vec::new();
    }
    TEMPLATE_TAG_RE
        .captures_iter(content)
        .filter_map(|captures| {
            let whole = captures.get(0)?;
            Some(TemplateTag {
                start: whole.start(),
                end: whole.end(),
                keyword: captures.get(1)?.as_str(),
                arguments: captures.get(2)?.as_str().trim(),
            })
        })
        .collect()
}

/// Adds block and macro symbols and moves the markup symbols they enclose
/// under them.
pub(super) fn add_template_symbols(
    base: &mut BaseExtractor,
    root: Node,
    symbols: &mut Vec<Symbol>,
) {
    let content = base.content.clone();
    let mut open: Vec<TemplateTag<'_>> = Vec::new();
    let mut templates = Vec::new();
    for tag in template_tags(&content) {
        match tag.keyword {
            "block" | "macro" => open.push(tag),
            "endblock" | "endmacro" => {
                let opener = &tag.keyword[3..];
                if let Some(position) = open.iter().rposition(|start| start.keyword == opener) {
                    let start = open.remove(position);
                    templates.extend(template_symbol(base, root, &start, Some(&tag)));
                }
            }
            _ => {}
        }
    }
    for start in &open {
        templates.extend(template_symbol(base, root, start, None));
    }
    if templates.is_empty() {
        return;
    }

    for index in 0..templates.len() {
        templates[index].parent_id =
            innermost_enclosing(&templates[index], symbols.iter().chain(templates.iter()))
                .map(|parent| parent.id.clone());
    }
    let spans: HashMap<String, (u32, u32)> = symbols
        .iter()
        .chain(templates.iter())
        .map(|symbol| (symbol.id.clone(), (symbol.start_byte, symbol.end_byte)))
        .collect();
    for symbol in symbols
        .iter_mut()
        .filter(|symbol| symbol.language == "html")
    {
        let Some(block) = innermost_enclosing(symbol, templates.iter()) else {
            continue;
        };
        let parent_encloses_block = symbol.parent_id.as_ref().is_none_or(|parent_id| {
            spans
                .get(parent_id)
                .is_some_and(|(start, end)| *start <= block.start_byte && block.end_byte <= *end)
        });
        if parent_encloses_block {
            symbol.parent_id = Some(block.id.clone());
        }
    }
    symbols.extend(templates);
}

fn innermost_enclosing<'s>(
    symbol: &Symbol,
    candidates: impl Iterator<Item = &'s Symbol>,
) -> Option<&'s Symbol> {
    candidates
        .filter(|candidate| candidate.id != symbol.id)
        .filter(|candidate| {
            candidate.start_byte <= symbol.start_byte
                && symbol.end_byte <= candidate.end_byte
                && (candidate.end_byte - candidate.start_byte)
                    > (symbol.end_byte - symbol.start_byte)
        })
        .min_by_key(|candidate| candidate.end_byte - candidate.start_byte)
}

fn template_symbol(
    base: &mut BaseExtractor,
    root: Node,
    start: &TemplateTag<'_>,
    end: Option<&TemplateTag<'_>>,
) -> Option<Symbol> {
    let name_end = start
        .arguments
        .find(|c: char| c == '(' || c.is_whitespace())
        .unwrap_or(start.arguments.len());
    let name = &start.arguments[..name_end];
    if name.is_empty() {
        return None;
    }
    let kind = if start.keyword == "macro" {
        SymbolKind::Function
    } else {
        SymbolKind::Namespace
    };
    let span = base.span_for_byte_range(start.start, end.map_or(start.end, |end| end.end))?;
    let signature = base.content.get(start.start..start.end)?.to_string();
    let metadata = HashMap::from([
        (
            "type".to_string(),
            Value::String(format!("template-{}", start.keyword)),
        ),
        (
            "templateTag".to_string(),
            Value::String(start.keyword.to_string()),
        ),
    ]);
    let mut symbol = base.create_symbol_from_span(
        &root,
        span,
        name.to_string(),
        kind,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: None,
            metadata: Some(metadata),
            doc_comment: None,
            annotations: Vec::new(),
        },
    );
    let body = end
        .filter(|end| start.end < end.start)
        .and_then(|end| base.span_for_byte_range(start.end, end.start));
    base.set_body_span(&mut symbol, body);
    Some(symbol)
}

/// Pending imports for `{% extends %}`, `{% include %}`, `{% import %}`, and
/// `{% from ... import %}`, owned by the innermost symbol around the tag.
pub(super) fn template_imports(
    base: &BaseExtractor,
    symbols: &[Symbol],
) -> Vec<StructuredPendingRelationship> {
    let mut pending = Vec::new();
    for tag in template_tags(&base.content) {
        let context = match tag.keyword {
            "extends" => "jinja-extends",
            "include" => "jinja-include",
            "import" | "from" => "jinja-import",
            _ => continue,
        };
        let Some(target) = QUOTED_RE
            .captures(tag.arguments)
            .and_then(|captures| captures.get(1).or_else(|| captures.get(2)))
            .map(|target| target.as_str().to_string())
        else {
            continue;
        };
        let Some(span) = base.span_for_byte_range(tag.start, tag.end) else {
            continue;
        };
        let caller_id = symbols
            .iter()
            .filter(|symbol| symbol.file_path == base.file_path)
            .filter(|symbol| {
                symbol.start_byte as usize <= tag.start && tag.end <= symbol.end_byte as usize
            })
            .min_by_key(|symbol| symbol.end_byte - symbol.start_byte)
            .map(|symbol| symbol.id.clone())
            .unwrap_or_else(|| format!("file:{}", base.file_path));
        let mut unresolved = UnresolvedTarget::simple(target);
        unresolved.import_context = Some(context.to_string());
        pending.push(StructuredPendingRelationship::new(
            caller_id.clone(),
            unresolved,
            Some(caller_id),
            RelationshipKind::Imports,
            base.file_path.clone(),
            span.start_line,
            0.9,
        ));
    }
    pending
}
