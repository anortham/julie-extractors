use super::helpers;
use super::type_facts;
use crate::base::{
    AnnotationMarker, BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility,
    normalize_annotations,
};
use crate::test_detection::apply_callable_test_metadata;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::Node;

pub fn extract_method(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = base.get_node_text(&name_node);
    let modifiers = helpers::extract_modifiers(base, &node);
    let visibility = helpers::determine_visibility(&modifiers, "public");

    let is_function = node.child_by_field_name("return_type").is_some();
    let keyword = if is_function { "Function" } else { "Sub" };
    let params = helpers::extract_parameters(base, &node);
    let type_params = helpers::extract_type_parameters(base, &node).unwrap_or_default();

    let mut signature = format!(
        "{}{} {}{}{}",
        helpers::modifier_prefix(&modifiers),
        keyword,
        name,
        type_params,
        params
    );

    if is_function && let Some(rt) = helpers::extract_return_type(base, &node) {
        signature.push_str(&format!(" As {}", rt));
    }

    let doc_comment = helpers::find_vbnet_doc_comment(base, &node);
    let annotations = normalize_annotations(&helpers::extract_attributes(base, &node), "vbnet");
    let annotation_keys = annotations
        .iter()
        .map(|annotation| annotation.annotation_key.clone())
        .collect::<Vec<_>>();

    let mut metadata = helpers::vb_visibility_metadata(&modifiers, "public");
    apply_callable_test_metadata(
        "vbnet",
        &name,
        &base.file_path,
        &SymbolKind::Method,
        &annotation_keys,
        doc_comment.as_deref(),
        &mut metadata,
    );

    let options = SymbolOptions {
        signature: Some(signature),
        visibility: Some(visibility),
        parent_id,
        doc_comment,
        metadata: if metadata.is_empty() {
            None
        } else {
            Some(metadata)
        },
        annotations,
    };

    let symbol = base.create_symbol(&node, name, SymbolKind::Method, options);
    record_return_type(base, &symbol, node);
    Some(symbol)
}

fn record_return_type(base: &mut BaseExtractor, symbol: &Symbol, node: Node) {
    if let Some(return_type) = node.child_by_field_name("return_type") {
        type_facts::record_declared_type(base, &symbol.id, return_type, None);
    }
}

pub fn extract_abstract_method(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    extract_method(base, node, parent_id)
}

pub fn extract_constructor(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let name = "New".to_string();
    let modifiers = helpers::extract_modifiers(base, &node);
    let visibility = helpers::determine_visibility(&modifiers, "public");
    let params = helpers::extract_parameters(base, &node);

    let signature = format!("{}Sub New{}", helpers::modifier_prefix(&modifiers), params);
    let doc_comment = helpers::find_vbnet_doc_comment(base, &node);
    let metadata = helpers::vb_visibility_metadata(&modifiers, "public");
    let annotations = normalize_annotations(&helpers::extract_attributes(base, &node), "vbnet");

    let options = SymbolOptions {
        signature: Some(signature),
        visibility: Some(visibility),
        parent_id,
        metadata: Some(metadata),
        doc_comment,
        annotations,
    };

    Some(base.create_symbol(&node, name, SymbolKind::Constructor, options))
}

#[allow(clippy::too_many_arguments)]
pub fn extract_property(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = base.get_node_text(&name_node);
    let modifiers = helpers::extract_modifiers(base, &node);
    let visibility = helpers::determine_visibility(&modifiers, "public");

    let indexed_params = node
        .child_by_field_name("parameters")
        .map(|p| base.get_node_text(&p));

    let mut signature = format!("{}Property {}", helpers::modifier_prefix(&modifiers), name);

    if let Some(params) = indexed_params {
        signature.push_str(&params);
    }

    if let Some(prop_type) = helpers::extract_as_clause_type(base, &node) {
        signature.push_str(&format!(" As {}", prop_type));
    }

    let doc_comment = helpers::find_vbnet_doc_comment(base, &node);
    let metadata = helpers::vb_visibility_metadata(&modifiers, "public");
    let annotations = normalize_annotations(&helpers::extract_attributes(base, &node), "vbnet");

    let options = SymbolOptions {
        signature: Some(signature),
        visibility: Some(visibility),
        parent_id,
        metadata: Some(metadata),
        doc_comment,
        annotations,
    };

    let symbol = base.create_symbol(&node, name, SymbolKind::Property, options);
    if let Some(type_node) = type_facts::declared_type_node(node) {
        type_facts::record_declared_type(
            base,
            &symbol.id,
            type_node,
            type_facts::declarator_rank_node(node),
        );
    }
    Some(symbol)
}

pub fn extract_field(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    extract_fields(base, node, parent_id).into_iter().next()
}

pub fn extract_fields(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Vec<Symbol> {
    let modifiers = helpers::extract_modifiers(base, &node);
    let default_visibility = if node
        .parent()
        .is_some_and(|parent| parent.kind() == "structure_block")
    {
        "public"
    } else {
        "private"
    };
    let visibility = helpers::determine_visibility(&modifiers, default_visibility);

    let mut declarators = Vec::new();
    collect_descendants_of_kind(node, "variable_declarator", &mut declarators);

    let doc_comment = helpers::find_vbnet_doc_comment(base, &node);
    let metadata = helpers::vb_visibility_metadata(&modifiers, default_visibility);
    let annotations = normalize_annotations(&helpers::extract_attributes(base, &node), "vbnet");

    let shared_types = shared_declarator_types(&declarators);
    declarators
        .into_iter()
        .zip(shared_types)
        .filter_map(|(declarator, type_node)| {
            let name_node = declarator.child_by_field_name("name")?;
            let name = base.get_node_text(&name_node);
            let mut signature = format!("{}Dim {}", helpers::modifier_prefix(&modifiers), name);
            if let Some(rank) = type_facts::declarator_rank_node(declarator) {
                signature.push_str(&base.get_node_text(&rank));
            }
            if let Some(type_node) = type_node {
                signature.push_str(&type_facts::as_clause_suffix(base, type_node));
            }

            let options = SymbolOptions {
                signature: Some(signature),
                visibility: Some(visibility.clone()),
                parent_id: parent_id.clone(),
                metadata: Some(metadata.clone()),
                doc_comment: doc_comment.clone(),
                annotations: annotations.clone(),
            };

            let symbol = base.create_symbol(&node, name, SymbolKind::Field, options);
            if let Some(type_node) = type_node {
                type_facts::record_declared_type(
                    base,
                    &symbol.id,
                    type_node,
                    type_facts::declarator_rank_node(declarator),
                );
            }
            Some(symbol)
        })
        .collect()
}

/// The declared type of each field declarator. `Private a, b As Integer`
/// shares the trailing `As` clause with every bare name before it.
fn shared_declarator_types<'a>(declarators: &[Node<'a>]) -> Vec<Option<Node<'a>>> {
    let mut types: Vec<Option<Node<'a>>> = declarators
        .iter()
        .map(|declarator| type_facts::declared_type_node(*declarator))
        .collect();
    let mut shared = None;
    for (declarator, type_node) in declarators.iter().zip(types.iter_mut()).rev() {
        let has_initializer = declarator.named_child_count()
            > 1 + usize::from(type_facts::declarator_rank_node(*declarator).is_some());
        if type_node.is_some() {
            shared = *type_node;
        } else if has_initializer {
            shared = None;
        } else {
            *type_node = shared;
        }
    }
    types
}

pub fn extract_event(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = base.get_node_text(&name_node);
    let modifiers = helpers::extract_modifiers(base, &node);
    let visibility = helpers::determine_visibility(&modifiers, "public");

    let mut signature = format!("{}Event {}", helpers::modifier_prefix(&modifiers), name);

    if let Some(event_type) = helpers::extract_as_clause_type(base, &node) {
        signature.push_str(&format!(" As {}", event_type));
    }

    let doc_comment = helpers::find_vbnet_doc_comment(base, &node);
    let metadata = helpers::vb_visibility_metadata(&modifiers, "public");
    let annotations = normalize_annotations(&helpers::extract_attributes(base, &node), "vbnet");

    let options = SymbolOptions {
        signature: Some(signature),
        visibility: Some(visibility),
        parent_id,
        metadata: Some(metadata),
        doc_comment,
        annotations,
    };

    Some(base.create_symbol(&node, name, SymbolKind::Event, options))
}

pub fn extract_operator(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let (conversion, op) = operator_header(base, node)?;
    let name = format!("operator {}", op);
    let modifiers = helpers::extract_modifiers(base, &node);
    let visibility = helpers::determine_visibility(&modifiers, "public");
    let params = helpers::extract_parameters(base, &node);

    let mut signature = format!(
        "{}{}Operator {}{}",
        helpers::modifier_prefix(&modifiers),
        conversion.map(|c| format!("{c} ")).unwrap_or_default(),
        op,
        params
    );

    if let Some(rt) = helpers::extract_return_type(base, &node) {
        signature.push_str(&format!(" As {}", rt));
    }

    let doc_comment = helpers::find_vbnet_doc_comment(base, &node);
    let metadata = helpers::vb_visibility_metadata(&modifiers, "public");
    let annotations = normalize_annotations(&helpers::extract_attributes(base, &node), "vbnet");

    let options = SymbolOptions {
        signature: Some(signature),
        visibility: Some(visibility),
        parent_id,
        metadata: Some(metadata),
        doc_comment,
        annotations,
    };

    let symbol = base.create_symbol(&node, name, SymbolKind::Operator, options);
    record_return_type(base, &symbol, node);
    Some(symbol)
}

/// The conversion keyword (`Widening`/`Narrowing`) and operator token of an
/// operator declaration. The grammar exposes symbolic operators as the
/// `operator` field but keeps keyword operators (`Not`, `Mod`, `CType`,
/// `IsTrue`) anonymous, so those are read from the header text.
fn operator_header(base: &BaseExtractor, node: Node) -> Option<(Option<String>, String)> {
    let header_start = node
        .child_by_field_name("modifiers")
        .map_or(node.start_byte(), |modifiers| modifiers.end_byte());
    let header_end = node.child_by_field_name("parameters")?.start_byte();
    let header = base.content.get(header_start..header_end)?;
    let words: Vec<&str> = header.split_whitespace().collect();
    let keyword = words
        .iter()
        .position(|word| word.eq_ignore_ascii_case("Operator"))?;
    let conversion = words[..keyword]
        .iter()
        .find(|word| {
            word.eq_ignore_ascii_case("Widening") || word.eq_ignore_ascii_case("Narrowing")
        })
        .map(|word| word.to_string());
    let op = match node.child_by_field_name("operator") {
        Some(op_node) => base.get_node_text(&op_node),
        None => words.get(keyword + 1)?.to_string(),
    };
    (!op.is_empty()).then_some((conversion, op))
}

pub fn extract_const(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    extract_consts(base, node, parent_id).into_iter().next()
}

pub fn extract_consts(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Vec<Symbol> {
    let modifiers = helpers::extract_modifiers(base, &node);
    let visibility = helpers::determine_visibility(&modifiers, "public");

    let mut declarators = Vec::new();
    collect_descendants_of_kind(node, "variable_declarator", &mut declarators);

    let doc_comment = helpers::find_vbnet_doc_comment(base, &node);
    let metadata = helpers::vb_visibility_metadata(&modifiers, "public");
    let annotations = normalize_annotations(&helpers::extract_attributes(base, &node), "vbnet");

    if declarators.is_empty() {
        return extract_flat_consts(
            base,
            node,
            parent_id,
            &modifiers,
            visibility,
            metadata,
            doc_comment,
            annotations,
        );
    }

    declarators
        .into_iter()
        .filter_map(|declarator| {
            let name_node = declarator.child_by_field_name("name")?;
            let name = base.get_node_text(&name_node);
            let mut signature = format!("{}Const {}", helpers::modifier_prefix(&modifiers), name);
            append_as_clause(base, declarator, &mut signature);

            let options = SymbolOptions {
                signature: Some(signature),
                visibility: Some(visibility.clone()),
                parent_id: parent_id.clone(),
                metadata: Some(metadata.clone()),
                doc_comment: doc_comment.clone(),
                annotations: annotations.clone(),
            };

            Some(base.create_symbol(&node, name, SymbolKind::Constant, options))
        })
        .collect()
}

fn append_as_clause(base: &BaseExtractor, node: Node, signature: &mut String) {
    if let Some(type_name) = as_clause_type(base, node) {
        signature.push_str(&format!(" As {}", type_name));
    }
}

fn as_clause_type(base: &BaseExtractor, node: Node) -> Option<String> {
    if node.kind() == "as_clause" {
        return node
            .child_by_field_name("type")
            .map(|type_node| base.get_node_text(&type_node));
    }

    let mut cursor = node.walk();
    let as_clause = node.children(&mut cursor).find(|c| c.kind() == "as_clause");
    if let Some(as_clause) = as_clause
        && let Some(type_node) = as_clause.child_by_field_name("type")
    {
        return Some(base.get_node_text(&type_node));
    }

    None
}

#[allow(clippy::too_many_arguments)]
fn extract_flat_consts(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
    modifiers: &[String],
    visibility: Visibility,
    metadata: std::collections::HashMap<String, serde_json::Value>,
    doc_comment: Option<String>,
    annotations: Vec<AnnotationMarker>,
) -> Vec<Symbol> {
    let children: Vec<Node> = node.children(&mut node.walk()).collect();
    let mut constants = Vec::new();
    let mut index = 0;

    while index < children.len() {
        if children[index].kind() != "identifier" {
            index += 1;
            continue;
        }

        let name_node = children[index];
        let mut type_name = None;
        let mut next = index + 1;
        while next < children.len() && children[next].kind() != "identifier" {
            if children[next].kind() == "as_clause" {
                type_name = as_clause_type(base, children[next]);
            }
            next += 1;
        }

        let name = base.get_node_text(&name_node);
        let mut signature = format!("{}Const {}", helpers::modifier_prefix(modifiers), name);
        if let Some(type_name) = type_name {
            signature.push_str(&format!(" As {}", type_name));
        }

        let options = SymbolOptions {
            signature: Some(signature),
            visibility: Some(visibility.clone()),
            parent_id: parent_id.clone(),
            metadata: Some(metadata.clone()),
            doc_comment: doc_comment.clone(),
            annotations: annotations.clone(),
        };

        constants.push(base.create_symbol(&node, name, SymbolKind::Constant, options));
        index = next;
    }

    constants
}

fn collect_descendants_of_kind<'a>(node: Node<'a>, kind: &str, matches: &mut Vec<Node<'a>>) {
    collect_descendants_of_kind_at_depth(node, kind, matches, 0);
}

fn collect_descendants_of_kind_at_depth<'a>(
    node: Node<'a>,
    kind: &str,
    matches: &mut Vec<Node<'a>>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == kind {
            matches.push(child);
        }
        collect_descendants_of_kind_at_depth(child, kind, matches, child_depth);
    }
}

pub fn extract_declare(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = base.get_node_text(&name_node);
    let modifiers = helpers::extract_modifiers(base, &node);
    let default_visibility = if parent_id.is_some() {
        "public"
    } else {
        "friend"
    };
    let visibility = helpers::determine_visibility(&modifiers, default_visibility);

    let is_function = node.child_by_field_name("return_type").is_some();
    let keyword = if is_function { "Function" } else { "Sub" };
    let params = helpers::extract_parameters(base, &node);

    let lib = node
        .child_by_field_name("library")
        .map(|l| base.get_node_text(&l))
        .unwrap_or_default();

    let mut signature = format!(
        "{}Declare {} {} Lib {}{}",
        helpers::modifier_prefix(&modifiers),
        keyword,
        name,
        lib,
        params
    );

    if is_function && let Some(rt) = helpers::extract_return_type(base, &node) {
        signature.push_str(&format!(" As {}", rt));
    }

    let doc_comment = helpers::find_vbnet_doc_comment(base, &node);
    let metadata = helpers::vb_visibility_metadata(&modifiers, default_visibility);
    let annotations = normalize_annotations(&helpers::extract_attributes(base, &node), "vbnet");

    let options = SymbolOptions {
        signature: Some(signature),
        visibility: Some(visibility),
        parent_id,
        metadata: Some(metadata),
        doc_comment,
        annotations,
    };

    let symbol = base.create_symbol(&node, name, SymbolKind::Function, options);
    record_return_type(base, &symbol, node);
    Some(symbol)
}
