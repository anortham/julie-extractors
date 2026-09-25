/// Helper utilities for Python symbol extraction
/// Includes AST navigation, argument extraction, and string handling
use super::PythonExtractor;
use tree_sitter::Node;

/// Walk up the AST tree to find the nearest enclosing class_definition
/// and return its generated symbol ID. Returns None if not inside a class.
pub fn find_parent_class_id(extractor: &PythonExtractor, node: &Node) -> Option<String> {
    let mut current = *node;
    while let Some(parent) = current.parent() {
        if parent.kind() == "class_definition" {
            let class_name = match parent.child_by_field_name("name") {
                Some(name_node) => extractor.base().get_node_text(&name_node),
                None => {
                    current = parent;
                    continue;
                }
            };

            let parent_id = extractor.base().generate_id_for_node(&class_name, &parent);

            return Some(parent_id);
        }
        current = parent;
    }
    None
}

/// Walk ancestors and return the id of the first function_definition or
/// async_function_definition. A class_definition reached first returns the class id.
pub fn find_enclosing_callable_id(extractor: &PythonExtractor, node: &Node) -> Option<String> {
    let mut current = *node;
    while let Some(parent) = current.parent() {
        match parent.kind() {
            "function_definition" | "async_function_definition" => {
                let name_text = match parent.child_by_field_name("name") {
                    Some(name_node) => extractor.base().get_node_text(&name_node),
                    None => {
                        current = parent;
                        continue;
                    }
                };
                return Some(extractor.base().generate_id_for_node(&name_text, &parent));
            }
            "class_definition" => {
                let class_name = match parent.child_by_field_name("name") {
                    Some(name_node) => extractor.base().get_node_text(&name_node),
                    None => {
                        current = parent;
                        continue;
                    }
                };
                return Some(extractor.base().generate_id_for_node(&class_name, &parent));
            }
            _ => current = parent,
        }
    }
    None
}

/// Drop the text-heuristic body span of a declaration that has no body:
/// imports, variables, constants, attributes, and parameters.
pub fn without_body(mut symbol: crate::base::Symbol) -> crate::base::Symbol {
    symbol.body_span = None;
    symbol.body_hash = None;
    symbol
}

pub fn enclosing_class_name(base: &crate::base::BaseExtractor, node: &Node) -> Option<String> {
    let mut current = *node;
    while let Some(parent) = current.parent() {
        if parent.kind() == "class_definition" {
            let name_node = parent.child_by_field_name("name")?;
            return Some(base.get_node_text(&name_node));
        }
        current = parent;
    }
    None
}

/// `cls` at `node` names the enclosing class: the nearest function that binds
/// `cls` takes it as a parameter. A function that assigns or imports `cls`
/// binds something else, and an unbound `cls` names nothing.
pub fn cls_names_enclosing_class(base: &crate::base::BaseExtractor, node: &Node) -> bool {
    let mut current = *node;
    while let Some(parent) = current.parent() {
        current = parent;
        match current.kind() {
            "class_definition" => return false,
            "function_definition" => {
                let takes_cls =
                    current
                        .child_by_field_name("parameters")
                        .is_some_and(|parameters| {
                            parameters
                                .named_children(&mut parameters.walk())
                                .any(|parameter| {
                                    parameter_name(base, parameter) == Some("cls".into())
                                })
                        });
                if takes_cls {
                    return true;
                }
                if current
                    .child_by_field_name("body")
                    .is_some_and(|body| binds_cls_locally(base, body))
                {
                    return false;
                }
            }
            _ => {}
        }
    }
    false
}

fn parameter_name(base: &crate::base::BaseExtractor, parameter: Node) -> Option<String> {
    match parameter.kind() {
        "identifier" => Some(base.get_node_text(&parameter)),
        _ => parameter
            .child_by_field_name("name")
            .or_else(|| {
                parameter
                    .named_children(&mut parameter.walk())
                    .find(|child| child.kind() == "identifier")
            })
            .map(|name| base.get_node_text(&name)),
    }
}

/// An assignment to `cls` or an import bound as `cls` in `body`, outside nested scopes.
fn binds_cls_locally(base: &crate::base::BaseExtractor, body: Node) -> bool {
    let mut stack = vec![body];
    while let Some(node) = stack.pop() {
        let binds = match node.kind() {
            "function_definition" | "class_definition" | "lambda" => continue,
            "assignment" => node.child_by_field_name("left").is_some_and(|left| {
                left.kind() == "identifier" && base.get_node_text(&left) == "cls"
            }),
            "aliased_import" => node
                .child_by_field_name("alias")
                .is_some_and(|alias| base.get_node_text(&alias) == "cls"),
            _ => false,
        };
        if binds {
            return true;
        }
        stack.extend(node.named_children(&mut node.walk()));
    }
    false
}

/// The type a method call's receiver names: the enclosing class for `self.m()`
/// and `cls.m()`, and the first declared base for `super().m()`.
pub fn self_or_cls_receiver_type(
    base: &crate::base::BaseExtractor,
    function_node: &Node,
) -> Option<String> {
    if function_node.kind() != "attribute" {
        return None;
    }
    let object = function_node.child_by_field_name("object")?;
    if is_super_call(base, &object) {
        return first_base_name(base, function_node);
    }
    let receiver = base.get_node_text(&object);
    if receiver == "self" || (receiver == "cls" && cls_names_enclosing_class(base, function_node)) {
        enclosing_class_name(base, function_node)
    } else {
        None
    }
}

/// `node` is a call of the `super` builtin: `super()` or `super(Cls, self)`.
pub fn is_super_call(base: &crate::base::BaseExtractor, node: &Node) -> bool {
    node.kind() == "call"
        && node
            .child_by_field_name("function")
            .is_some_and(|function| {
                function.kind() == "identifier" && base.get_node_text(&function) == "super"
            })
}

/// The simple name of the first positional base of the class enclosing `node`.
pub fn first_base_name(base: &crate::base::BaseExtractor, node: &Node) -> Option<String> {
    let mut current = *node;
    let class_node = loop {
        let parent = current.parent()?;
        if parent.kind() == "class_definition" {
            break parent;
        }
        current = parent;
    };
    let superclasses = class_node.child_by_field_name("superclasses")?;
    let mut cursor = superclasses.walk();
    let first = superclasses
        .named_children(&mut cursor)
        .find(|arg| matches!(arg.kind(), "identifier" | "attribute" | "subscript"))?;
    let head = if first.kind() == "subscript" {
        first.child_by_field_name("value")?
    } else {
        first
    };
    let text = base.get_node_text(&head);
    Some(text.rsplit('.').next().unwrap_or(&text).to_string())
}

/// The name node of the definition a decorator belongs to, when `node` sits
/// inside that decorator (and not inside a lambda within it). Calls and names
/// in a decorator are owned by the decorated function or class.
pub fn decorated_definition_name<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    let mut current = *node;
    while let Some(parent) = current.parent() {
        match parent.kind() {
            "decorator" => {
                return parent
                    .parent()
                    .filter(|owner| owner.kind() == "decorated_definition")?
                    .child_by_field_name("definition")?
                    .child_by_field_name("name");
            }
            "lambda" | "block" | "module" | "decorated_definition" => return None,
            _ => current = parent,
        }
    }
    None
}

/// Extract argument list from a superclasses node
pub fn extract_argument_list(extractor: &PythonExtractor, node: &Node) -> Vec<String> {
    let mut args = Vec::new();
    let base = extractor.base();

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "identifier" | "attribute" => {
                args.push(base.get_node_text(&child));
            }
            "subscript" => {
                // Handle generic types like Generic[K, V]
                args.push(base.get_node_text(&child));
            }
            "keyword_argument" => {
                // Handle keyword arguments like metaclass=SingletonMeta
                let mut child_cursor = child.walk();
                let children: Vec<_> = child.children(&mut child_cursor).collect();
                if let (Some(keyword_node), Some(value_node)) = (children.first(), children.last())
                    && keyword_node.kind() == "identifier"
                    && base.get_node_text(keyword_node) == "metaclass"
                {
                    args.push(format!(
                        "{}={}",
                        base.get_node_text(keyword_node),
                        base.get_node_text(value_node)
                    ));
                }
            }
            _ => {}
        }
    }

    args
}

/// Helper to strip string delimiters (quotes) from Python strings
/// Handles triple quotes (""" or '''), double quotes ("), and single quotes (')
pub fn strip_string_delimiters(s: &str) -> String {
    // Try delimiters in order: triple quotes first (3 chars), then single quotes (1 char)
    let delimiters = [("\"\"\"", 3), ("'''", 3), ("\"", 1), ("'", 1)];

    for (delimiter, strip_count) in &delimiters {
        if s.starts_with(delimiter) && s.ends_with(delimiter) && s.len() >= strip_count * 2 {
            return s[*strip_count..s.len() - strip_count].to_string();
        }
    }

    // No matching delimiter found, return as-is
    s.to_string()
}
