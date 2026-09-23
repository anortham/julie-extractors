use tree_sitter::{Node, Tree};

use super::{client_fact, verb_for_token};
use crate::base::types::StructuralFact;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

/// `Invoke-RestMethod` / `Invoke-WebRequest` (and their `irm` / `iwr`
/// aliases) with a static `-Uri` or first positional URL. `-Method` attests
/// the verb; without it the cmdlets send GET.
pub(super) fn collect_powershell_http_client_requests(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    collect(
        tree.root_node(),
        language,
        tree,
        file_path,
        content,
        &mut facts,
        0,
    );
    facts
}

fn collect(
    node: Node,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if node.kind() == "command"
        && let Some(fact) = client_request(node, language, tree, file_path, content)
    {
        facts.push(fact);
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect(
            child,
            language,
            tree,
            file_path,
            content,
            facts,
            child_depth,
        );
    }
}

fn client_request(
    command: Node,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Option<StructuralFact> {
    let text = |node: Node| content.get(node.byte_range()).unwrap_or_default();
    let name = command.child_by_field_name("command_name")?;
    let client = match text(name).to_ascii_lowercase().as_str() {
        "invoke-restmethod" | "irm" => "invoke-restmethod",
        "invoke-webrequest" | "iwr" => "invoke-webrequest",
        _ => return None,
    };
    let elements = command.child_by_field_name("command_elements")?;
    let mut url = None;
    let mut verb = None;
    let mut parameter: Option<String> = None;
    let mut cursor = elements.walk();
    for element in elements.named_children(&mut cursor) {
        match element.kind() {
            "command_argument_sep" => {}
            "command_parameter" => {
                parameter = Some(text(element).trim_start_matches('-').to_ascii_lowercase());
            }
            _ => match parameter.take().as_deref() {
                None | Some("uri") if url.is_none() => url = static_word(element, content),
                Some("method") => verb = static_word(element, content),
                _ => {}
            },
        }
    }
    let url = url?;
    let (verb, verb_source) = match verb {
        Some(token) => (verb_for_token(&token)?, "attested"),
        None => ("GET", "default"),
    };
    client_fact(
        language,
        tree,
        file_path,
        content,
        command.start_byte(),
        command.end_byte(),
        client,
        &url,
        verb,
        verb_source,
        None,
    )
}

/// A bare word or a string with no interpolation, unquoted.
fn static_word(value: Node, content: &str) -> Option<String> {
    let mut current = value;
    while matches!(
        current.kind(),
        "array_literal_expression" | "unary_expression"
    ) {
        if current.named_child_count() != 1 {
            return None;
        }
        current = current.named_child(0)?;
    }
    let raw = content.get(current.byte_range())?;
    match current.kind() {
        "generic_token" => Some(raw.to_string()),
        "string_literal" => {
            let inner = current.named_child(0)?;
            (inner.named_child_count() == 0).then(|| raw.trim_matches(['"', '\'']).to_string())
        }
        _ => None,
    }
}
