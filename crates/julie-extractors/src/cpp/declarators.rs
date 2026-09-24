//! Reads a C++ declarator the way the language binds it: the derivation applied
//! directly to the name decides whether the name is a function or an object.

use tree_sitter::Node;

pub(super) struct DeclaratorTarget<'a> {
    pub name: Node<'a>,
    /// The `function_declarator` applied directly to the name, when the name is a function.
    pub function: Option<Node<'a>>,
}

const NAME_KINDS: &[&str] = &[
    "identifier",
    "field_identifier",
    "qualified_identifier",
    "destructor_name",
    "operator_name",
    "operator_cast",
    "type_identifier",
    "template_function",
];

pub(super) fn declarator_target(declarator: Node) -> Option<DeclaratorTarget> {
    let mut current = declarator;
    let mut innermost: Option<Node> = None;
    for _ in 0..64 {
        if NAME_KINDS.contains(&current.kind()) {
            return Some(DeclaratorTarget {
                name: current,
                function: innermost.filter(|node| node.kind() == "function_declarator"),
            });
        }
        match current.kind() {
            "init_declarator" => current = current.child_by_field_name("declarator")?,
            "reference_declarator" | "parenthesized_declarator" | "attributed_declarator" => {
                current = first_declarator_child(current)?;
            }
            "pointer_declarator" | "array_declarator" | "function_declarator" => {
                innermost = Some(current);
                current = current.child_by_field_name("declarator")?;
            }
            _ => return None,
        }
    }
    None
}

fn first_declarator_child(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).find(|child| {
        !matches!(
            child.kind(),
            "comment" | "attribute_declaration" | "attribute_specifier" | "type_qualifier"
        )
    })
}

/// The `[a, b]` of a structured binding such as `auto [a, b] = ...` or
/// `const auto& [a, b] = ...`.
pub(super) fn structured_binding(declarator: Node) -> Option<Node> {
    let mut current = declarator;
    loop {
        current = match current.kind() {
            "structured_binding_declarator" => return Some(current),
            "init_declarator" => current.child_by_field_name("declarator")?,
            "reference_declarator" => first_declarator_child(current)?,
            _ => return None,
        };
    }
}

/// The names one declarator introduces: one name, or each name of a structured
/// binding such as `auto [key, value] = ...`.
pub(super) fn declared_names(declarator: Node) -> Vec<Node> {
    if let Some(binding) = structured_binding(declarator) {
        let mut cursor = binding.walk();
        return binding
            .named_children(&mut cursor)
            .filter(|child| child.kind() == "identifier")
            .collect();
    }
    declarator_target(declarator)
        .map(|target| vec![target.name])
        .unwrap_or_default()
}

/// `Writer writer(out, options_);` in a block reads as a prototype whose
/// parameters are bare types. In block scope, a parameter list of bare type
/// names, none of them primitive, is a direct initialization of an object.
pub(super) fn is_direct_initialization(declaration: Node, function: Node) -> bool {
    if declaration
        .parent()
        .is_none_or(|parent| parent.kind() != "compound_statement")
    {
        return false;
    }
    let Some(parameters) = function.child_by_field_name("parameters") else {
        return false;
    };
    let mut cursor = parameters.walk();
    let parameters: Vec<Node> = parameters.named_children(&mut cursor).collect();
    !parameters.is_empty()
        && parameters.iter().all(|parameter| {
            parameter.kind() == "parameter_declaration"
                && parameter.child_by_field_name("declarator").is_none()
                && parameter.child_by_field_name("type").is_some_and(|ty| {
                    matches!(ty.kind(), "type_identifier" | "qualified_identifier")
                })
                && parameter.named_child_count() == 1
        })
}

/// The declarators of a declaration or field declaration that declare a function.
pub(super) fn function_declarator_of(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    let declarators: Vec<Node> = node
        .children_by_field_name("declarator", &mut cursor)
        .collect();
    declarators.into_iter().find_map(|declarator| {
        let function = declarator_target(declarator)?.function?;
        (!is_direct_initialization(node, function)).then_some(function)
    })
}

/// Each name a declaration or field declaration introduces as an object, with
/// the declarator it comes from.
pub(super) fn object_names(node: Node) -> Vec<(Node, Node)> {
    let mut cursor = node.walk();
    let declarators: Vec<Node> = node
        .children_by_field_name("declarator", &mut cursor)
        .collect();
    declarators
        .into_iter()
        .filter(|declarator| {
            declarator_target(*declarator)
                .and_then(|target| target.function)
                .is_none_or(|function| is_direct_initialization(node, function))
        })
        .flat_map(|declarator| {
            declared_names(declarator)
                .into_iter()
                .filter(|name| {
                    matches!(
                        name.kind(),
                        "identifier" | "field_identifier" | "qualified_identifier"
                    )
                })
                .map(move |name| (declarator, name))
        })
        .collect()
}

/// Whether a `type_identifier` is an argument of a direct initialization such as
/// the `out` of `Writer writer(out);`.
pub(super) fn is_direct_initialization_argument(node: Node) -> bool {
    let Some(function) = node
        .parent()
        .filter(|parameter| parameter.kind() == "parameter_declaration")
        .and_then(|parameter| parameter.parent())
        .filter(|parameters| parameters.kind() == "parameter_list")
        .and_then(|parameters| parameters.parent())
        .filter(|function| function.kind() == "function_declarator")
    else {
        return false;
    };
    function
        .parent()
        .filter(|declaration| declaration.kind() == "declaration")
        .is_some_and(|declaration| is_direct_initialization(declaration, function))
}
