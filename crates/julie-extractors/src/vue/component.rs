//! Component name of a Vue SFC.

use super::parsing::ParsedVueSfc;
use tree_sitter::Node;

/// The `name` of the component options object (`export default {...}`,
/// `export default defineComponent({...})`, or `defineOptions({...})`), else
/// the file stem in PascalCase.
pub(super) fn extract_component_name(file_path: &str, sfc: &ParsedVueSfc) -> Option<String> {
    let declared = sfc.sections.iter().enumerate().find_map(|(idx, section)| {
        let tree = sfc.script_tree(idx)?;
        options_objects(tree.root_node(), &section.content)
            .into_iter()
            .find_map(|object| name_pair_value(object, &section.content))
    });
    if declared.is_some() {
        return declared;
    }

    let stem = std::path::Path::new(file_path).file_stem()?.to_str()?;
    Some(
        stem.split('-')
            .map(|part| {
                let mut chars = part.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                }
            })
            .collect(),
    )
}

/// The component options objects of a script block.
pub(super) fn options_objects<'t>(root: Node<'t>, source: &str) -> Vec<Node<'t>> {
    let mut objects = Vec::new();
    let mut cursor = root.walk();
    for statement in root.named_children(&mut cursor) {
        match statement.kind() {
            "export_statement" => {
                if let Some(value) = statement.child_by_field_name("value") {
                    objects.extend(options_object(value, source, "defineComponent"));
                }
            }
            "expression_statement" => {
                if let Some(call) = statement.named_child(0) {
                    objects.extend(options_object(call, source, "defineOptions"));
                }
            }
            _ => {}
        }
    }
    objects
}

fn options_object<'t>(value: Node<'t>, source: &str, wrapper: &str) -> Option<Node<'t>> {
    match value.kind() {
        "object" => Some(value),
        "call_expression" => {
            let callee = value.child_by_field_name("function")?;
            if source.get(callee.byte_range())? != wrapper {
                return None;
            }
            let arguments = value.child_by_field_name("arguments")?;
            arguments
                .named_child(0)
                .filter(|argument| argument.kind() == "object")
        }
        _ => None,
    }
}

fn name_pair_value(object: Node<'_>, source: &str) -> Option<String> {
    let mut cursor = object.walk();
    object.named_children(&mut cursor).find_map(|pair| {
        if pair.kind() != "pair" {
            return None;
        }
        let key = pair.child_by_field_name("key")?;
        let key_text = source.get(key.byte_range())?.trim_matches(['"', '\'']);
        if key_text != "name" {
            return None;
        }
        let value = pair.child_by_field_name("value")?;
        if value.kind() != "string" {
            return None;
        }
        let text = source.get(value.byte_range())?;
        let name = text.trim_matches(['"', '\'']);
        (!name.is_empty()).then(|| name.to_string())
    })
}
