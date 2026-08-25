/// Decorator extraction and handling
/// Supports @property, @staticmethod, @classmethod, and custom decorators
use super::PythonExtractor;
use tree_sitter::Node;

/// Extract decorators from a function or class definition
pub fn extract_decorators(extractor: &PythonExtractor, node: &Node) -> Vec<String> {
    extract_decorator_texts(extractor, node)
        .into_iter()
        .map(decorator_name_from_text)
        .collect()
}

/// Extract raw decorator text from a function or class definition.
pub fn extract_decorator_texts(extractor: &PythonExtractor, node: &Node) -> Vec<String> {
    let mut decorators = Vec::new();
    let base = extractor.base();

    if let Some(decorated_node) = find_decorated_node(node) {
        let mut cursor = decorated_node.walk();
        for child in decorated_node.children(&mut cursor) {
            if child.kind() == "decorator" {
                decorators.push(base.get_node_text(&child));
            }
        }
    }

    decorators
}

/// The `decorated_definition` whose decorators belong to `node`.
///
/// The walk stops at the first enclosing function or class. Without that stop a
/// nested `def` reaches its enclosing function's `decorated_definition` and
/// inherits decorators it never carried — a `@pytest.fixture` helper's inner
/// closure was reported as a fixture, and every method of a decorated class as
/// carrying the class decorator.
fn find_decorated_node<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    if node.kind() == "decorated_definition" {
        return Some(*node);
    }

    let mut current = *node;
    while let Some(parent) = current.parent() {
        match parent.kind() {
            "decorated_definition" => return Some(parent),
            "function_definition" | "class_definition" => return None,
            _ => current = parent,
        }
    }

    None
}

fn decorator_name_from_text(mut decorator_text: String) -> String {
    // Remove @ prefix (@ is ASCII, so this is safe)
    if decorator_text.starts_with('@') && decorator_text.is_char_boundary(1) {
        decorator_text = decorator_text[1..].to_string();
    }

    // Extract name without parameters: "lru_cache(maxsize=128)" -> "lru_cache"
    if let Some(paren_index) = decorator_text.find('(')
        && decorator_text.is_char_boundary(paren_index)
    {
        decorator_text = decorator_text[..paren_index].to_string();
    }

    decorator_text
}
