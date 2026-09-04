//! Function and method extraction for C++
//! Handles extraction of functions, methods, constructors, destructors, and operators

use crate::base::{
    AnnotationMarker, BaseExtractor, Symbol, SymbolKind, SymbolOptions, normalize_annotations,
};
use crate::test_detection::{
    apply_callable_test_metadata, apply_test_role, cpp_fixture_lifecycle_role,
    cpp_googletest_case_role,
};
use std::collections::HashMap;
use tree_sitter::Node;

pub(super) use super::function_signature_parts::{
    extract_basic_return_type, extract_function_modifiers, extract_function_parameters,
    extract_noexcept_specifier,
};
use super::function_signature_parts::{
    extract_const_qualifier, extract_method_modifiers, extract_trailing_return_type,
};
use super::{declarations, function_declarators, helpers};

/// Extract function (definition or declaration)
pub(super) fn extract_function(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&str>,
    symbols: &[Symbol],
) -> Option<Symbol> {
    let mut func_node = node;
    if node.kind() == "function_definition" {
        // The declarator of a function_definition is wrapped in
        // `pointer_declarator`/`reference_declarator` nodes when the return type is
        // a pointer or reference (`const char *f()`, `int& g()`, `char **h()`).
        // Descend through those wrappers to the inner `function_declarator` so the
        // name, parameters, const-qualifier, and noexcept-spec all resolve and the
        // symbol still spans the whole `node` (definition incl. body). Without this,
        // pointer/reference-return free functions fall through to the bare
        // `function_declarator` dispatch and get a declarator-only span. Mirrors the
        // C extractor's pointer_declarator handling (c/helpers.rs).
        if let Some(declarator) = node.child_by_field_name("declarator").or_else(|| {
            node.children(&mut node.walk()).find(|c| {
                matches!(
                    c.kind(),
                    "function_declarator" | "pointer_declarator" | "reference_declarator"
                )
            })
        }) {
            func_node = function_declarators::unwrap_to_function_declarator(declarator)
                .unwrap_or(declarator);
        }
    }

    let name_node = extract_function_name(func_node)?;
    let name = base.get_node_text(&name_node);

    let google_test_lifecycle =
        is_google_test_fixture_lifecycle(base, node, &name, parent_id, symbols);

    // Skip if it's a field_identifier (should be handled as method)
    if name_node.kind() == "field_identifier" {
        return extract_method(base, node, func_node, &name, parent_id, symbols);
    }

    // GoogleTest macros (`TEST(Suite, Name) { ... }`, `TEST_F`, `TEST_P`,
    // `TYPED_TEST`, `TYPED_TEST_P`) parse as function_definitions whose declarator
    // identifier IS the macro keyword and whose two "parameters" are the suite/
    // fixture and the test name. When the rebuild succeeds we rename the symbol to
    // `Suite.Name` AND remember the keyword so we can attach a synthetic annotation
    // below. Standalone artifact v1 preserves that annotation key as extracted
    // evidence, but it does not ship old Julie's test-role TOML classifier.
    // Structural `is_test` remains the artifact-visible test marker here. No
    // detect_cpp arm needed.
    let googletest_macro: Option<(String, String)> =
        if function_declarators::GTEST_MACROS.contains(&name.as_str()) {
            function_declarators::googletest_suite_dot_name(base, func_node, &name)
                .map(|suite_dot_name| (name.clone(), suite_dot_name))
        } else {
            None
        };
    let name = match &googletest_macro {
        Some((_, suite_dot_name)) => suite_dot_name.clone(),
        None => name,
    };

    // Check if this is a constructor or destructor
    let is_constructor_flag = is_constructor(base, &name, node);
    let is_destructor = name.starts_with('~');
    let is_operator = name.starts_with("operator");

    let kind = if is_constructor_flag {
        SymbolKind::Constructor
    } else if is_destructor {
        SymbolKind::Destructor
    } else if is_operator {
        SymbolKind::Operator
    } else {
        SymbolKind::Function
    };

    // Build signature from proven approach
    let modifiers = extract_function_modifiers(base, node);
    let return_type = if is_constructor_flag || is_destructor {
        String::new()
    } else {
        extract_basic_return_type(base, node)
    };
    let trailing_return_type = extract_trailing_return_type(base, node);
    let parameters = extract_function_parameters(base, func_node);
    let const_qualifier = extract_const_qualifier(func_node);
    let noexcept_spec = extract_noexcept_specifier(base, func_node);

    let mut signature = String::new();

    // Add template parameters if present
    if let Some(template_params) = helpers::extract_template_parameters(base, node.parent()) {
        signature.push_str(&template_params);
        signature.push('\n');
    }

    // Add modifiers
    if !modifiers.is_empty() {
        signature.push_str(&modifiers.join(" "));
        signature.push(' ');
    }

    // Add return type
    if !return_type.is_empty() {
        signature.push_str(&return_type);
        signature.push(' ');
    }

    // Add function name and parameters
    signature.push_str(&name);
    signature.push_str(&parameters);

    // Add const qualifier
    if const_qualifier {
        signature.push_str(" const");
    }

    // Add noexcept
    if !noexcept_spec.is_empty() {
        signature.push(' ');
        signature.push_str(&noexcept_spec);
    }

    // Add trailing return type
    if !trailing_return_type.is_empty() {
        if trailing_return_type.starts_with("->") {
            signature.push(' ');
            signature.push_str(&trailing_return_type);
        } else {
            signature.push_str(" -> ");
            signature.push_str(&trailing_return_type);
        }
    }

    // Check for = delete, = default (for function_definition nodes)
    if node.kind() == "function_definition" {
        let children: Vec<Node> = node.children(&mut node.walk()).collect();
        for child in &children {
            if child.kind() == "delete_method_clause" {
                signature.push_str(" = delete");
                break;
            } else if child.kind() == "default_method_clause" {
                signature.push_str(" = default");
                break;
            }
        }
    }

    // Extract visibility based on access specifiers (private:/protected:/public:)
    let visibility = declarations::extract_cpp_visibility(base, node);

    let doc_comment = base.find_doc_comment(&node);
    let mut annotations = normalize_annotations(&extract_standard_attributes(base, node), "cpp");
    // GoogleTest test macros carry no source attribute, so synthesize an annotation
    // whose key is the lowercased macro keyword (`test`, `test_f`, `test_p`,
    // `typed_test`, `typed_test_p`). The artifact preserves this evidence without
    // assigning old Julie test roles in v1.
    if let Some((macro_keyword, _)) = &googletest_macro {
        annotations.push(AnnotationMarker {
            annotation: macro_keyword.clone(),
            annotation_key: macro_keyword.to_ascii_lowercase(),
            raw_text: None,
            carrier: None,
        });
    }
    let annotation_keys = annotations
        .iter()
        .map(|annotation| annotation.annotation_key.clone())
        .collect::<Vec<_>>();

    // Test detection. A fixture hook recognized from the enclosing fixture's base
    // type outranks the case roles, because a hook also satisfies the macro test.
    // GoogleTest macros then take their role from the synthetic annotation above
    // (preserving test_p/typed_test_p → parameterized_test); everything else routes
    // through the shared name/annotation/path detector.
    let mut metadata = HashMap::new();
    if google_test_lifecycle {
        apply_test_role(&mut metadata, cpp_fixture_lifecycle_role(&name));
    } else if googletest_macro.is_some() {
        apply_test_role(&mut metadata, cpp_googletest_case_role(&annotation_keys));
    } else {
        apply_callable_test_metadata(
            "cpp",
            &name,
            &base.file_path,
            &kind,
            &annotation_keys,
            doc_comment.as_deref(),
            &mut metadata,
        );
    }

    Some(base.create_symbol(
        &node,
        name,
        kind,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(visibility),
            parent_id: parent_id.map(String::from),
            metadata: if metadata.is_empty() {
                None
            } else {
                Some(metadata)
            },
            doc_comment,
            annotations,
        },
    ))
}

/// Extract method (function inside a class)
fn extract_method(
    base: &mut BaseExtractor,
    node: Node,
    func_node: Node,
    name: &str,
    parent_id: Option<&str>,
    symbols: &[Symbol],
) -> Option<Symbol> {
    let is_constructor = is_constructor(base, name, node);
    let is_destructor = name.starts_with('~');
    let is_operator = name.starts_with("operator");

    let kind = if is_constructor {
        SymbolKind::Constructor
    } else if is_destructor {
        SymbolKind::Destructor
    } else if is_operator {
        SymbolKind::Operator
    } else {
        SymbolKind::Method
    };

    // For methods in classes, look for modifiers in the parent declaration node as well
    let modifiers = extract_method_modifiers(base, node, func_node);
    let return_type = if is_constructor || is_destructor {
        String::new()
    } else {
        extract_basic_return_type(base, node)
    };
    let parameters = extract_function_parameters(base, func_node);
    let const_qualifier = extract_const_qualifier(func_node);

    let mut signature = String::new();
    if !modifiers.is_empty() {
        signature.push_str(&modifiers.join(" "));
        signature.push(' ');
    }
    if !return_type.is_empty() {
        signature.push_str(&return_type);
        signature.push(' ');
    }
    signature.push_str(name);
    signature.push_str(&parameters);
    if const_qualifier {
        signature.push_str(" const");
    }

    // Extract visibility based on access specifiers (private:/protected:/public:)
    let visibility = declarations::extract_cpp_visibility(base, node);

    let doc_comment = base.find_doc_comment(&node);
    let annotations = normalize_annotations(&extract_standard_attributes(base, node), "cpp");
    let annotation_keys = annotations
        .iter()
        .map(|annotation| annotation.annotation_key.clone())
        .collect::<Vec<_>>();

    // Test detection
    let mut metadata = HashMap::new();
    if is_google_test_fixture_lifecycle(base, node, name, parent_id, symbols) {
        apply_test_role(&mut metadata, cpp_fixture_lifecycle_role(name));
    } else {
        apply_callable_test_metadata(
            "cpp",
            name,
            &base.file_path,
            &kind,
            &annotation_keys,
            doc_comment.as_deref(),
            &mut metadata,
        );
    }

    Some(base.create_symbol(
        &node,
        name.to_string(),
        kind,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(visibility),
            parent_id: parent_id.map(String::from),
            metadata: if metadata.is_empty() {
                None
            } else {
                Some(metadata)
            },
            doc_comment,
            annotations,
        },
    ))
}

fn extract_standard_attributes(base: &mut BaseExtractor, node: Node) -> Vec<String> {
    let mut attributes = Vec::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if !matches!(
            child.kind(),
            "attribute_specifier" | "attribute_declaration"
        ) {
            continue;
        }

        let text = base.get_node_text(&child);
        if text.trim_start().starts_with("[[") {
            collect_standard_attributes_from_text(&text, &mut attributes);
        }
    }

    let mut current = node.prev_sibling();
    while let Some(sibling) = current {
        if !matches!(
            sibling.kind(),
            "attribute_specifier" | "attribute_declaration"
        ) {
            break;
        }

        let sibling_text = base.get_node_text(&sibling);
        if !sibling_text.trim_start().starts_with("[[") {
            break;
        }
        let mut sibling_attributes = Vec::new();
        collect_standard_attributes_from_text(&sibling_text, &mut sibling_attributes);
        sibling_attributes.extend(attributes);
        attributes = sibling_attributes;
        current = sibling.prev_sibling();
    }

    attributes
}

fn is_google_test_fixture_lifecycle(
    base: &BaseExtractor,
    node: Node,
    name: &str,
    _parent_id: Option<&str>,
    symbols: &[Symbol],
) -> bool {
    let Some(method_name) = name.rsplit("::").next() else {
        return false;
    };
    if !matches!(
        method_name,
        "SetUp" | "TearDown" | "SetUpTestSuite" | "TearDownTestSuite"
    ) {
        return false;
    }

    if let Some((qualifier, _)) = name.rsplit_once("::") {
        let absolute = qualifier.starts_with("::");
        let qualifier = qualifier.trim_start_matches("::");
        let expected_qualified_name = if absolute {
            qualifier.to_string()
        } else {
            let scope = ast_enclosing_scope(base, node);
            if scope.is_empty()
                || qualifier == scope
                || qualifier.starts_with(&format!("{scope}::"))
            {
                qualifier.to_string()
            } else {
                format!("{scope}::{qualifier}")
            }
        };

        return has_matching_fixture_symbol(symbols, &expected_qualified_name);
    }

    let mut current = node.parent();
    while let Some(parent) = current {
        if matches!(parent.kind(), "class_specifier" | "struct_specifier") {
            let Some(base_clause) = parent
                .children(&mut parent.walk())
                .find(|child| child.kind() == "base_class_clause")
            else {
                return false;
            };
            return helpers::extract_base_type_names(base, base_clause)
                .iter()
                .any(|base_type| {
                    matches!(
                        base_type.trim_start_matches(':'),
                        "testing::Test" | "testing::TestWithParam"
                    )
                });
        }
        current = parent.parent();
    }
    false
}

fn class_specifier_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut cursor = node.walk();
    let name_node = node
        .children(&mut cursor)
        .find(|c| c.kind() == "type_identifier" || c.kind() == "template_type")?;
    if name_node.kind() == "template_type" {
        let mut inner_cursor = name_node.walk();
        let type_id = name_node
            .children(&mut inner_cursor)
            .find(|c| c.kind() == "type_identifier")
            .map(|n| base.get_node_text(&n))
            .unwrap_or_else(|| base.get_node_text(&name_node));
        Some(type_id)
    } else {
        Some(base.get_node_text(&name_node))
    }
}

fn ast_enclosing_scope(base: &BaseExtractor, node: Node) -> String {
    let mut segments = Vec::new();
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            "namespace_definition" => {
                let mut cursor = parent.walk();
                if let Some(name_node) = parent
                    .children(&mut cursor)
                    .find(|c| c.kind() == "namespace_identifier")
                {
                    segments.push(base.get_node_text(&name_node));
                }
            }
            "class_specifier" | "struct_specifier" => {
                if let Some(name) = class_specifier_name(base, parent) {
                    segments.push(name);
                }
            }
            _ => {}
        }
        current = parent.parent();
    }
    segments.reverse();
    segments.join("::")
}

fn has_google_test_fixture_base(symbol: &Symbol) -> bool {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("base_types"))
        .and_then(|value| value.as_array())
        .is_some_and(|base_types| {
            base_types
                .iter()
                .filter_map(|value| value.as_str())
                .any(|base_type| {
                    matches!(
                        base_type.trim_start_matches(':'),
                        "testing::Test" | "testing::TestWithParam"
                    )
                })
        })
}

fn has_matching_fixture_symbol(symbols: &[Symbol], expected_qualified_name: &str) -> bool {
    let id_map: HashMap<&str, &Symbol> = symbols.iter().map(|s| (s.id.as_str(), s)).collect();
    symbols.iter().any(|symbol| {
        matches!(symbol.kind, SymbolKind::Class | SymbolKind::Struct)
            && qualified_symbol_name(symbol, &id_map) == expected_qualified_name
            && has_google_test_fixture_base(symbol)
    })
}

fn qualified_symbol_name(symbol: &Symbol, id_map: &HashMap<&str, &Symbol>) -> String {
    let mut segments = vec![symbol.name.as_str()];
    let mut parent_id = symbol.parent_id.as_deref();
    while let Some(id) = parent_id {
        let Some(parent) = id_map.get(id) else {
            break;
        };
        if matches!(
            parent.kind,
            SymbolKind::Namespace | SymbolKind::Class | SymbolKind::Struct
        ) {
            segments.push(parent.name.as_str());
        }
        parent_id = parent.parent_id.as_deref();
    }
    segments.reverse();
    segments.join("::")
}

fn collect_standard_attributes_from_text(text: &str, attributes: &mut Vec<String>) {
    let mut remaining = text;
    while let Some(start) = remaining.find("[[") {
        let after_start = &remaining[start + 2..];
        let Some(end) = after_start.find("]]") else {
            break;
        };
        attributes.push(format!("[[{}]]", after_start[..end].trim()));
        remaining = &after_start[end + 2..];
    }
}

/// Extract function name from function declarator
pub(super) fn extract_function_name(func_node: Node) -> Option<Node> {
    // operator_name (operator overloading)
    if let Some(operator_node) = func_node
        .children(&mut func_node.walk())
        .find(|c| c.kind() == "operator_name")
    {
        return Some(operator_node);
    }

    // destructor_name
    if let Some(destructor_node) = func_node
        .children(&mut func_node.walk())
        .find(|c| c.kind() == "destructor_name")
    {
        return Some(destructor_node);
    }

    // field_identifier (methods)
    if let Some(field_id_node) = func_node
        .children(&mut func_node.walk())
        .find(|c| c.kind() == "field_identifier")
    {
        return Some(field_id_node);
    }

    // identifier (regular functions)
    if let Some(identifier_node) = func_node
        .children(&mut func_node.walk())
        .find(|c| c.kind() == "identifier")
    {
        return Some(identifier_node);
    }

    // qualified_identifier (e.g., ClassName::method)
    if let Some(qualified_node) = func_node
        .children(&mut func_node.walk())
        .find(|c| c.kind() == "qualified_identifier")
    {
        return Some(qualified_node);
    }

    None
}

/// Check if a function name matches a containing class name (is constructor)
pub(super) fn is_constructor(base: &BaseExtractor, name: &str, node: Node) -> bool {
    let mut current = Some(node);
    while let Some(parent) = current {
        if matches!(parent.kind(), "class_specifier" | "struct_specifier")
            && let Some(class_name_node) = parent
                .children(&mut parent.walk())
                .find(|c| c.kind() == "type_identifier")
        {
            let class_name = base.get_node_text(&class_name_node);
            if class_name == name {
                return true;
            }
        }
        current = parent.parent();
    }
    false
}
