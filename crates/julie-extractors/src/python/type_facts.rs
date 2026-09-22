/// Declared-type fact recording for receiver-typed call resolution.
/// Records the one plainly named type an annotation binds: a bare identifier,
/// a plain dotted name, or a subscript whose base is one of those. Wrappers
/// that do not change the receiver type (`Optional[X]`, `X | None`,
/// `Annotated[X, ...]`, `ClassVar[X]`, `Final[X]`, `Mapped[X]`) and string
/// forward references (`"X"`) unwrap to `X`. Other unions and inline
/// callables record nothing.
use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use tree_sitter::Node;

pub(super) const PYTHON_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &[],
    generic_open: &['['],
};

const TRANSPARENT_WRAPPERS: &[&str] = &["Optional", "Annotated", "ClassVar", "Final", "Mapped"];

/// Record a syntactically stated annotation for a symbol (`is_inferred=false`).
pub(super) fn record_annotation_fact(base: &mut BaseExtractor, symbol_id: &str, type_node: Node) {
    if let Some(named) = plainly_named_annotation(base, type_node) {
        let declared = base.get_node_text(&type_node);
        base.record_declared_type_fact_with_declared(
            symbol_id,
            &named,
            &declared,
            &PYTHON_TYPE_NAME_RULES,
            false,
        );
    }
}

/// Record the class constructed by an `x = Foo()` initializer when `Foo` is a
/// class defined in the same file (`is_inferred=true`).
pub(super) fn record_constructor_fact(base: &mut BaseExtractor, symbol_id: &str, class_name: &str) {
    base.record_declared_type_fact(symbol_id, class_name, &PYTHON_TYPE_NAME_RULES, true);
}

fn plainly_named_annotation(base: &BaseExtractor, node: Node) -> Option<String> {
    plainly_named_annotation_at(base, node, 0)
}

fn plainly_named_annotation_at(base: &BaseExtractor, node: Node, depth: u32) -> Option<String> {
    let child_depth = crate::tree_traversal::child_tree_depth(depth)?;
    match node.kind() {
        "type" => plainly_named_annotation_at(base, node.named_child(0)?, child_depth),
        "identifier" | "none" => Some(base.get_node_text(&node)),
        "attribute" | "member_type" => is_plain_name(node).then(|| base.get_node_text(&node)),
        "string" => forward_reference(base, node),
        "generic_type" => {
            let mut cursor = node.walk();
            let head = node.named_children(&mut cursor).next()?;
            let arguments = type_arguments(node);
            unwrap_wrapper(base, head, &arguments, child_depth)
                .or_else(|| Some(base.get_node_text(&node)))
        }
        "subscript" => {
            let value = node.child_by_field_name("value")?;
            if !is_plain_name(value) {
                return None;
            }
            let mut cursor = node.walk();
            let arguments: Vec<Node> = node
                .children_by_field_name("subscript", &mut cursor)
                .collect();
            unwrap_wrapper(base, value, &arguments, child_depth)
                .or_else(|| Some(base.get_node_text(&node)))
        }
        "binary_operator" | "union_type" => {
            let mut members = Vec::new();
            collect_union_members(node, &mut members);
            let mut non_none = members.into_iter().filter(|member| !is_none(*member));
            let only = non_none.next()?;
            non_none
                .next()
                .is_none()
                .then(|| plainly_named_annotation_at(base, only, child_depth))?
        }
        _ => None,
    }
}

fn type_arguments(generic: Node) -> Vec<Node> {
    let mut cursor = generic.walk();
    generic
        .named_children(&mut cursor)
        .find(|child| child.kind() == "type_parameter")
        .map(|parameters| {
            let mut inner = parameters.walk();
            parameters.named_children(&mut inner).collect()
        })
        .unwrap_or_default()
}

fn unwrap_wrapper(
    base: &BaseExtractor,
    head: Node,
    arguments: &[Node],
    depth: u32,
) -> Option<String> {
    let head_text = base.get_node_text(&head);
    let wrapper = head_text.rsplit('.').next().unwrap_or(&head_text);
    if wrapper == "Union" {
        let mut non_none = arguments.iter().filter(|argument| !is_none(**argument));
        let only = non_none.next()?;
        return non_none
            .next()
            .is_none()
            .then(|| plainly_named_annotation_at(base, *only, depth))?;
    }
    if !TRANSPARENT_WRAPPERS.contains(&wrapper) {
        return None;
    }
    plainly_named_annotation_at(base, *arguments.first()?, depth)
}

fn collect_union_members<'a>(node: Node<'a>, members: &mut Vec<Node<'a>>) {
    collect_union_members_at(node, members, 0);
}

fn collect_union_members_at<'a>(node: Node<'a>, members: &mut Vec<Node<'a>>, depth: u32) {
    let Some(child_depth) = crate::tree_traversal::child_tree_depth(depth) else {
        members.push(node);
        return;
    };
    let is_pipe = node
        .child_by_field_name("operator")
        .is_none_or(|operator| operator.kind() == "|");
    if matches!(node.kind(), "binary_operator" | "union_type") && is_pipe {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            collect_union_members_at(child, members, child_depth);
        }
    } else if node.kind() == "type" && node.named_child_count() == 1 {
        collect_union_members_at(node.named_child(0).unwrap_or(node), members, child_depth);
    } else {
        members.push(node);
    }
}

fn is_none(node: Node) -> bool {
    match node.kind() {
        "none" => true,
        "type" => node.named_child(0).is_some_and(is_none),
        _ => false,
    }
}

fn forward_reference(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut cursor = node.walk();
    let content = node
        .named_children(&mut cursor)
        .find(|child| child.kind() == "string_content")?;
    let text = base.get_node_text(&content);
    let text = text.trim();
    let is_dotted_name = !text.is_empty()
        && text.split('.').all(|segment| {
            segment
                .chars()
                .next()
                .is_some_and(|c| c.is_alphabetic() || c == '_')
                && segment.chars().all(|c| c.is_alphanumeric() || c == '_')
        });
    is_dotted_name.then(|| text.to_string())
}

fn is_plain_name(node: Node) -> bool {
    match node.kind() {
        "type" | "member_type" => node.named_child(0).is_some_and(is_plain_name),
        "identifier" => true,
        "attribute" => node
            .child_by_field_name("object")
            .is_some_and(is_plain_name),
        _ => false,
    }
}
