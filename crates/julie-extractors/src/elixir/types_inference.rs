use crate::base::BaseExtractor;
use tree_sitter::Node;

/// Type inference for Elixir from @spec annotations.
///
/// Only a return type that reduces to a single base name is recorded: `integer()`
/// becomes `integer`, `GenServer.on_start()` becomes `GenServer.on_start`, and
/// tuples, lists, unions, maps, and type variables record nothing.
pub(super) fn spec_base_type_name(return_type: &str) -> Option<String> {
    let text = return_type.trim();
    let name = text.strip_suffix("()").unwrap_or(text);
    let is_dotted_name = !name.is_empty()
        && name.split('.').all(|segment| {
            segment
                .chars()
                .next()
                .is_some_and(|c| c.is_alphabetic() || c == '_')
                && segment.chars().all(|c| c.is_alphanumeric() || c == '_')
        });
    is_dotted_name.then(|| name.to_string())
}

/// The return type of one `@spec`, reduced to what type facts need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SpecReturn {
    text: String,
    /// The base name of the whole return type.
    pub(crate) base: Option<String>,
    /// The base name of `T` when `{:ok, x} = call()` can only bind `x` to a
    /// `T` from a `{:ok, T}` alternative.
    pub(crate) ok_payload: Option<String>,
}

impl SpecReturn {
    pub(super) fn from_node(base: &BaseExtractor, return_type: Node) -> Self {
        let text = base.get_node_text(&return_type);
        Self {
            base: (return_type.kind() != "identifier")
                .then(|| spec_base_type_name(&text))
                .flatten(),
            ok_payload: ok_payload(base, return_type),
            text,
        }
    }
}

/// The agreed payload of the `{:ok, T}` alternatives of a union. Atom and
/// other tuple alternatives cannot match `{:ok, x}`; any other alternative
/// (a named type, a type variable) might, so it records nothing.
fn ok_payload(base: &BaseExtractor, return_type: Node) -> Option<String> {
    let mut payload: Option<String> = None;
    let mut alternatives = vec![return_type];
    while let Some(node) = alternatives.pop() {
        match node.kind() {
            "binary_operator"
                if node
                    .child_by_field_name("operator")
                    .is_some_and(|operator| operator.kind() == "|") =>
            {
                alternatives.push(node.child_by_field_name("left")?);
                alternatives.push(node.child_by_field_name("right")?);
            }
            "atom" | "nil" | "boolean" => {}
            "tuple" => {
                let elements: Vec<Node> = node.named_children(&mut node.walk()).collect();
                match elements.as_slice() {
                    [tag, value] if base.get_node_text(tag) == ":ok" => {
                        let name = (value.kind() == "call")
                            .then(|| spec_base_type_name(&base.get_node_text(value)))
                            .flatten()?;
                        if payload.get_or_insert_with(|| name.clone()) != &name {
                            return None;
                        }
                    }
                    [tag, ..] if tag.kind() == "atom" => {}
                    _ => return None,
                }
            }
            _ => return None,
        }
    }
    payload
}
