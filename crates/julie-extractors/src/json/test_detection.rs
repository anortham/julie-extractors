use crate::base::BaseExtractor;
use tree_sitter::Node;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum JsonTestRole {
    Container,
    Case,
}

pub(super) fn role_for_description_pair(
    base: &BaseExtractor,
    pair: Node,
    key_name: &str,
) -> Option<JsonTestRole> {
    if key_name != "description" || pair_value(pair)?.kind() != "string" {
        return None;
    }

    let object = pair.parent()?;
    if object.kind() != "object" {
        return None;
    }
    if is_group_object(base, object) {
        return Some(JsonTestRole::Container);
    }
    if is_test_object(base, object) {
        return Some(JsonTestRole::Case);
    }
    None
}

fn is_group_object(base: &BaseExtractor, object: Node) -> bool {
    let Some(array) = object.parent() else {
        return false;
    };
    let Some(document) = array.parent() else {
        return false;
    };
    array.kind() == "array"
        && document.kind() == "document"
        && pair_value_for_key(base, object, "description")
            .is_some_and(|value| value.kind() == "string")
        && pair_value_for_key(base, object, "schema").is_some()
        && pair_value_for_key(base, object, "tests").is_some_and(|value| value.kind() == "array")
}

fn is_test_object(base: &BaseExtractor, object: Node) -> bool {
    let Some(array) = object.parent() else {
        return false;
    };
    let Some(tests_pair) = array.parent() else {
        return false;
    };
    let Some(group) = tests_pair.parent() else {
        return false;
    };
    array.kind() == "array"
        && tests_pair.kind() == "pair"
        && pair_key(base, tests_pair).as_deref() == Some("tests")
        && is_group_object(base, group)
        && pair_value_for_key(base, object, "description")
            .is_some_and(|value| value.kind() == "string")
        && pair_value_for_key(base, object, "data").is_some()
        && pair_value_for_key(base, object, "valid")
            .is_some_and(|value| matches!(value.kind(), "true" | "false"))
}

fn pair_value_for_key<'tree>(
    base: &BaseExtractor,
    object: Node<'tree>,
    key: &str,
) -> Option<Node<'tree>> {
    let mut cursor = object.walk();
    object.named_children(&mut cursor).find_map(|child| {
        (child.kind() == "pair" && pair_key(base, child).as_deref() == Some(key))
            .then(|| pair_value(child))
            .flatten()
    })
}

fn pair_key(base: &BaseExtractor, pair: Node) -> Option<String> {
    let key = pair.named_child(0)?;
    base.decode_string_literal(&key)
        .or_else(|| Some(base.get_node_text(&key).trim_matches('"').to_string()))
}

fn pair_value<'tree>(pair: Node<'tree>) -> Option<Node<'tree>> {
    let index = pair.named_child_count().checked_sub(1)? as u32;
    pair.named_child(index)
}
