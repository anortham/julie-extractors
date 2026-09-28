use std::collections::HashMap;

use tree_sitter::{Node, Tree};

use super::helpers::{base_metadata, fact_for_node, insert_string, insert_string_array};
use crate::base::types::StructuralFact;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

const TABLE_CHECK_PATTERN_ID: &str = "kotest.table_check.v1";
const PROPERTY_CHECK_PATTERN_ID: &str = "kotest.property_check.v1";

#[derive(Clone, Copy, PartialEq, Eq)]
enum CheckKind {
    Table,
    Property,
}

struct LocalBinding {
    name: String,
    visible_from: usize,
}

type LocalBindings = HashMap<usize, Vec<LocalBinding>>;

#[derive(Default)]
struct CheckImports {
    explicit: HashMap<String, Option<CheckKind>>,
    table_wildcard: bool,
    property_wildcard: bool,
}

impl CheckImports {
    fn insert(&mut self, local_name: String, kind: CheckKind) {
        match self.explicit.entry(local_name) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(Some(kind));
            }
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                if entry.get() != &Some(kind) {
                    entry.insert(None);
                }
            }
        }
    }

    fn kind_for(&self, name: &str) -> Option<CheckKind> {
        if let Some(kind) = self.explicit.get(name) {
            return *kind;
        }
        let table = self.table_wildcard && matches!(name, "forAll" | "forNone");
        let property = self.property_wildcard && matches!(name, "forAll" | "checkAll");
        match (table, property) {
            (true, false) => Some(CheckKind::Table),
            (false, true) => Some(CheckKind::Property),
            _ => None,
        }
    }
}

pub(super) fn collect_kotest_testing_facts(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    if language != "kotlin" {
        return Vec::new();
    }
    let mut imports = CheckImports::default();
    collect_check_imports(tree.root_node(), content, &mut imports, 0);
    let mut local_bindings = HashMap::new();
    collect_local_bindings(tree.root_node(), content, &mut local_bindings, 0);
    let mut facts = Vec::new();
    collect_check_calls(
        tree.root_node(),
        language,
        file_path,
        content,
        &imports,
        &local_bindings,
        0,
        &mut facts,
    );
    facts
}

fn collect_check_imports(node: Node, content: &str, imports: &mut CheckImports, depth: u32) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if node.kind() == "import" {
        add_check_import(node, content, imports);
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_check_imports(child, content, imports, child_depth);
    }
}

fn add_check_import(import: Node, content: &str, imports: &mut CheckImports) {
    let mut cursor = import.walk();
    let children = import.children(&mut cursor).collect::<Vec<_>>();
    let Some(path_node) = children
        .iter()
        .find(|child| child.kind() == "qualified_identifier")
        .copied()
    else {
        return;
    };
    let Some(path) = node_text(content, path_node) else {
        return;
    };
    let wildcard = children.iter().any(|child| child.kind() == "*");
    if wildcard {
        match path {
            "io.kotest.data" | "io.kotest.datatest" => imports.table_wildcard = true,
            "io.kotest.property" => imports.property_wildcard = true,
            _ => {}
        }
        return;
    }
    let Some(kind) = check_import_kind(path) else {
        return;
    };
    let alias = children
        .iter()
        .skip_while(|child| child.kind() != "as")
        .find(|child| child.kind() == "identifier")
        .and_then(|child| node_text(content, *child));
    let local_name = alias.unwrap_or_else(|| path.rsplit('.').next().unwrap_or(path));
    imports.insert(local_name.to_string(), kind);
}

fn check_import_kind(path: &str) -> Option<CheckKind> {
    match path {
        "io.kotest.data.forAll"
        | "io.kotest.data.forNone"
        | "io.kotest.datatest.forAll"
        | "io.kotest.datatest.forNone" => Some(CheckKind::Table),
        "io.kotest.property.forAll" | "io.kotest.property.checkAll" => Some(CheckKind::Property),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_check_calls(
    node: Node,
    language: &str,
    file_path: &str,
    content: &str,
    imports: &CheckImports,
    local_bindings: &LocalBindings,
    depth: u32,
    facts: &mut Vec<StructuralFact>,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if node.kind() == "call_expression" && !is_callee_of_call(node) {
        if let Some((callee, kind)) = classify_check(node, content, imports)
            && !is_shadowed_import(node, &callee, local_bindings)
        {
            facts.push(fact_for_check(
                language, file_path, content, node, &callee, kind,
            ));
        }
    } else if node.kind() == "binary_expression"
        && let Some((callee, type_argument)) =
            classify_generic_property_check(node, content, imports)
        && !is_shadowed_import(node, &callee, local_bindings)
    {
        facts.push(fact_for_generic_property_check(
            language,
            file_path,
            node,
            &callee,
            type_argument,
        ));
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_check_calls(
            child,
            language,
            file_path,
            content,
            imports,
            local_bindings,
            child_depth,
            facts,
        );
    }
}

fn classify_check(
    call: Node,
    content: &str,
    imports: &CheckImports,
) -> Option<(String, CheckKind)> {
    let callee = direct_callee(call, content)?;
    check_kind_for_callee(&callee, imports).map(|kind| (callee, kind))
}

fn direct_callee(mut call: Node, content: &str) -> Option<String> {
    let mut depth = 0;
    loop {
        if !should_visit_tree_depth(depth) {
            return None;
        }
        let mut cursor = call.walk();
        let callee = call.named_children(&mut cursor).next()?;
        if callee.kind() == "call_expression" {
            depth = child_tree_depth(depth)?;
            call = callee;
            continue;
        }
        if !matches!(
            callee.kind(),
            "identifier" | "simple_identifier" | "qualified_identifier" | "navigation_expression"
        ) {
            return None;
        }
        return node_text(content, callee).map(str::to_string);
    }
}

fn check_kind_for_callee(callee: &str, imports: &CheckImports) -> Option<CheckKind> {
    check_import_kind(callee).or_else(|| {
        (!callee.contains('.'))
            .then(|| imports.kind_for(callee))
            .flatten()
    })
}

fn classify_generic_property_check(
    expression: Node,
    content: &str,
    imports: &CheckImports,
) -> Option<(String, String)> {
    let right = expression.child_by_field_name("right")?;
    if !matches!(right.kind(), "lambda_literal" | "annotated_lambda") {
        return None;
    }
    let generic = expression.child_by_field_name("left")?;
    if generic.kind() != "binary_expression" {
        return None;
    }
    let callee_node = generic.child_by_field_name("left")?;
    let type_argument_node = generic.child_by_field_name("right")?;
    let callee = node_text(content, callee_node)?.to_string();
    if check_kind_for_callee(&callee, imports) != Some(CheckKind::Property) {
        return None;
    }
    let type_argument = node_text(content, type_argument_node)?.to_string();
    Some((callee, type_argument))
}

fn is_callee_of_call(node: Node) -> bool {
    node.parent().is_some_and(|parent| {
        if parent.kind() != "call_expression" {
            return false;
        }
        let mut cursor = parent.walk();
        parent
            .named_children(&mut cursor)
            .next()
            .is_some_and(|callee| callee.id() == node.id())
    })
}

fn collect_local_bindings(node: Node, content: &str, bindings: &mut LocalBindings, depth: u32) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    match node.kind() {
        "function_declaration" => {
            add_function_binding(node, content, bindings);
            add_function_parameter_bindings(node, content, bindings);
        }
        "property_declaration" => add_property_bindings(node, content, bindings),
        "lambda_literal" => add_lambda_parameter_bindings(node, content, bindings),
        _ => {}
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_local_bindings(child, content, bindings, child_depth);
    }
}

fn add_function_binding(function: Node, content: &str, bindings: &mut LocalBindings) {
    if has_receiver_before_name(function) {
        return;
    }
    let Some(name) = function.child_by_field_name("name") else {
        return;
    };
    let Some(scope) = enclosing_scopes(function).first().copied() else {
        return;
    };
    let visible_from = if matches!(scope.kind(), "source_file" | "class_body") {
        0
    } else {
        name.end_byte()
    };
    add_local_binding(scope, node_text(content, name), visible_from, bindings);
}

fn add_function_parameter_bindings(function: Node, content: &str, bindings: &mut LocalBindings) {
    let mut cursor = function.walk();
    let parameters = function
        .named_children(&mut cursor)
        .find(|child| child.kind() == "function_value_parameters");
    let mut cursor = function.walk();
    let body = function
        .named_children(&mut cursor)
        .find(|child| child.kind() == "function_body");
    let (Some(parameters), Some(body)) = (parameters, body) else {
        return;
    };
    let mut cursor = parameters.walk();
    for parameter in parameters.named_children(&mut cursor) {
        if parameter.kind() != "parameter" {
            continue;
        }
        if let Some(name) = first_identifier(parameter) {
            add_local_binding(body, node_text(content, name), 0, bindings);
        }
    }
}

fn add_property_bindings(property: Node, content: &str, bindings: &mut LocalBindings) {
    let mut cursor = property.walk();
    let declaration = property.named_children(&mut cursor).find(|child| {
        matches!(
            child.kind(),
            "variable_declaration" | "multi_variable_declaration"
        )
    });
    let Some(declaration) = declaration else {
        return;
    };
    if has_receiver_before_declaration(property, declaration) {
        return;
    }
    let Some(scope) = enclosing_scopes(property).first().copied() else {
        return;
    };
    let visible_from = if matches!(scope.kind(), "source_file" | "class_body") {
        0
    } else {
        property.end_byte()
    };
    if declaration.kind() == "variable_declaration" {
        add_variable_binding(declaration, content, visible_from, scope, bindings);
    } else {
        let mut cursor = declaration.walk();
        for variable in declaration.named_children(&mut cursor) {
            if variable.kind() == "variable_declaration" {
                add_variable_binding(variable, content, visible_from, scope, bindings);
            }
        }
    }
}

fn add_lambda_parameter_bindings(lambda: Node, content: &str, bindings: &mut LocalBindings) {
    let mut cursor = lambda.walk();
    let Some(parameters) = lambda
        .named_children(&mut cursor)
        .find(|child| child.kind() == "lambda_parameters")
    else {
        return;
    };
    let mut cursor = parameters.walk();
    for parameter in parameters.named_children(&mut cursor) {
        if parameter.kind() == "variable_declaration" {
            add_variable_binding(parameter, content, 0, lambda, bindings);
        } else if parameter.kind() == "multi_variable_declaration" {
            let mut cursor = parameter.walk();
            for variable in parameter.named_children(&mut cursor) {
                if variable.kind() == "variable_declaration" {
                    add_variable_binding(variable, content, 0, lambda, bindings);
                }
            }
        }
    }
}

fn add_variable_binding(
    declaration: Node,
    content: &str,
    visible_from: usize,
    scope: Node,
    bindings: &mut LocalBindings,
) {
    if let Some(name) = first_identifier(declaration) {
        add_local_binding(scope, node_text(content, name), visible_from, bindings);
    }
}

fn first_identifier(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| matches!(child.kind(), "identifier" | "simple_identifier"))
}

fn add_local_binding(
    scope: Node,
    name: Option<&str>,
    visible_from: usize,
    bindings: &mut LocalBindings,
) {
    if let Some(name) = name {
        bindings.entry(scope.id()).or_default().push(LocalBinding {
            name: name.to_string(),
            visible_from,
        });
    }
}

fn has_receiver_before_declaration(property: Node, declaration: Node) -> bool {
    let mut cursor = property.walk();
    for child in property.named_children(&mut cursor) {
        if child.id() == declaration.id() {
            return false;
        }
        if !matches!(child.kind(), "modifiers" | "type_parameters") {
            return true;
        }
    }
    false
}

fn is_shadowed_import(node: Node, callee: &str, bindings: &LocalBindings) -> bool {
    if callee.contains('.') {
        return false;
    }
    let usage_scopes = enclosing_scopes(node);
    usage_scopes.iter().any(|scope| {
        bindings.get(&scope.id()).is_some_and(|candidates| {
            candidates
                .iter()
                .any(|binding| binding.name == callee && binding.visible_from <= node.start_byte())
        })
    })
}

fn has_receiver_before_name(function: Node) -> bool {
    let Some(name) = function.child_by_field_name("name") else {
        return false;
    };
    let mut cursor = function.walk();
    for child in function.named_children(&mut cursor) {
        if child.id() == name.id() {
            return false;
        }
        if !matches!(child.kind(), "modifiers" | "type_parameters") {
            return true;
        }
    }
    false
}

fn enclosing_scopes(node: Node) -> Vec<Node> {
    let mut scopes = Vec::new();
    let mut ancestor = Some(node);
    while let Some(current) = ancestor {
        if matches!(
            current.kind(),
            "source_file" | "block" | "class_body" | "function_body" | "lambda_literal"
        ) {
            scopes.push(current);
        }
        ancestor = current.parent();
    }
    scopes
}

fn call_type_arguments(mut call: Node, content: &str) -> Option<Vec<String>> {
    let mut depth = 0;
    loop {
        if !should_visit_tree_depth(depth) {
            return None;
        }
        let mut cursor = call.walk();
        if let Some(type_arguments) = call
            .children(&mut cursor)
            .find(|child| child.kind() == "type_arguments")
        {
            let mut cursor = type_arguments.walk();
            return Some(
                type_arguments
                    .named_children(&mut cursor)
                    .filter_map(|argument| node_text(content, argument).map(str::to_string))
                    .collect(),
            );
        }
        let mut cursor = call.walk();
        let callee = call.named_children(&mut cursor).next()?;
        if callee.kind() != "call_expression" {
            return None;
        }
        depth = child_tree_depth(depth)?;
        call = callee;
    }
}

fn call_arguments(mut call: Node) -> Option<Node> {
    let mut depth = 0;
    loop {
        if !should_visit_tree_depth(depth) {
            return None;
        }
        let mut cursor = call.walk();
        if let Some(arguments) = call
            .children(&mut cursor)
            .find(|child| child.kind() == "value_arguments")
        {
            return Some(arguments);
        }
        let mut cursor = call.walk();
        let callee = call.named_children(&mut cursor).next()?;
        if callee.kind() != "call_expression" {
            return None;
        }
        depth = child_tree_depth(depth)?;
        call = callee;
    }
}

fn argument_expressions(call: Node, content: &str) -> Vec<String> {
    let Some(arguments) = call_arguments(call) else {
        return Vec::new();
    };
    let mut cursor = arguments.walk();
    arguments
        .named_children(&mut cursor)
        .filter(|argument| argument.kind() == "value_argument")
        .filter_map(|argument| node_text(content, argument).map(str::to_string))
        .collect()
}

fn node_text<'a>(content: &'a str, node: Node<'_>) -> Option<&'a str> {
    content.get(node.start_byte()..node.end_byte())
}

fn fact_for_check(
    language: &str,
    file_path: &str,
    content: &str,
    call: Node,
    callee: &str,
    kind: CheckKind,
) -> StructuralFact {
    let (pattern_id, capture_name) = match kind {
        CheckKind::Table => (TABLE_CHECK_PATTERN_ID, "table_check"),
        CheckKind::Property => (PROPERTY_CHECK_PATTERN_ID, "property_check"),
    };
    let mut metadata = base_metadata("testing", "kotest");
    insert_string(&mut metadata, "callee", callee);
    let arguments = argument_expressions(call, content);
    if !arguments.is_empty() {
        insert_string_array(&mut metadata, "arguments", arguments);
    }
    if let Some(type_arguments) = call_type_arguments(call, content)
        && !type_arguments.is_empty()
    {
        insert_string_array(&mut metadata, "type_arguments", type_arguments);
    }
    fact_for_node(
        file_path,
        language,
        pattern_id,
        capture_name,
        call,
        metadata,
    )
}

fn fact_for_generic_property_check(
    language: &str,
    file_path: &str,
    expression: Node,
    callee: &str,
    type_argument: String,
) -> StructuralFact {
    let mut metadata = base_metadata("testing", "kotest");
    insert_string(&mut metadata, "callee", callee);
    insert_string_array(&mut metadata, "type_arguments", vec![type_argument]);
    fact_for_node(
        file_path,
        language,
        PROPERTY_CHECK_PATTERN_ID,
        "property_check",
        expression,
        metadata,
    )
}
