//! AWS CloudFormation / SAM intrinsic references between template entries.
//!
//! In a template (a root `AWSTemplateFormatVersion` key, or a root `Resources`
//! mapping whose entries declare `Type`), `!Ref X`, `Ref: X`, `!GetAtt X.Attr`,
//! `Fn::GetAtt: [X, Attr]`, `${X}` inside `!Sub` / `Fn::Sub`, and
//! `DependsOn: X | [X]` each name a resource or parameter of the same template.
//! Each site becomes a `variable_ref` identifier and a `References` edge from
//! the enclosing template entry (`Resources.Handler`, `Outputs.BucketArn`) to
//! the named `Resources` or `Parameters` entry. Pseudo parameters
//! (`AWS::Region`) name nothing in the template and are skipped.

use super::relationships::{pair_key, push_reference, scalar_list, scalar_value};
use crate::base::{BaseExtractor, IdentifierKind, Relationship, Symbol};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::{Node, Tree};

pub(super) fn extract_relationships(
    base: &BaseExtractor,
    tree: &Tree,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    for (node, name) in reference_sites(base, tree, symbols) {
        let (Some(target), Some(source)) = (
            template_entry(symbols, &name),
            base.find_containing_symbol(&node, symbols)
                .map(|symbol| entry_for(symbols, symbol)),
        ) else {
            continue;
        };
        if source.id != target.id {
            push_reference(
                base,
                source,
                target,
                &node,
                ("cloudFormationRef", &name),
                relationships,
            );
        }
    }
}

pub(super) fn extract_identifiers(base: &mut BaseExtractor, tree: &Tree, symbols: &[Symbol]) {
    let sites = reference_sites(base, tree, symbols);
    for (node, name) in sites {
        let containing = base
            .find_containing_symbol(&node, symbols)
            .map(|symbol| symbol.id.clone());
        let target = template_entry(symbols, &name).map(|symbol| symbol.id.clone());
        base.create_identifier(&node, name, IdentifierKind::VariableRef, containing);
        if let Some(last) = base.identifiers.last_mut() {
            last.target_symbol_id = target;
        }
    }
}

fn is_template(symbols: &[Symbol]) -> bool {
    root_symbol(symbols, "AWSTemplateFormatVersion").is_some()
        || root_symbol(symbols, "Resources").is_some_and(|resources| {
            children(symbols, resources)
                .any(|resource| children(symbols, resource).any(|field| field.name == "Type"))
        })
}

fn root_symbol<'a>(symbols: &'a [Symbol], name: &str) -> Option<&'a Symbol> {
    symbols
        .iter()
        .find(|symbol| symbol.parent_id.is_none() && symbol.name == name)
}

fn children<'a>(symbols: &'a [Symbol], parent: &'a Symbol) -> impl Iterator<Item = &'a Symbol> {
    symbols
        .iter()
        .filter(move |symbol| symbol.parent_id.as_deref() == Some(parent.id.as_str()))
}

fn template_entry<'a>(symbols: &'a [Symbol], name: &str) -> Option<&'a Symbol> {
    ["Resources", "Parameters"].iter().find_map(|section| {
        let section = root_symbol(symbols, section)?;
        children(symbols, section).find(|entry| entry.name == name)
    })
}

/// The second-level ancestor (`Resources.X`, `Outputs.Y`) of a symbol, or the
/// symbol itself when it is a root section.
fn entry_for<'a>(symbols: &'a [Symbol], symbol: &'a Symbol) -> &'a Symbol {
    let mut current = symbol;
    while let Some(parent) = current
        .parent_id
        .as_deref()
        .and_then(|id| symbols.iter().find(|s| s.id == id))
    {
        if parent.parent_id.is_none() {
            return current;
        }
        current = parent;
    }
    current
}

fn reference_sites<'tree>(
    base: &BaseExtractor,
    tree: &'tree Tree,
    symbols: &[Symbol],
) -> Vec<(Node<'tree>, String)> {
    let mut sites = Vec::new();
    if is_template(symbols) {
        collect_sites(base, tree.root_node(), &mut sites, 0);
    }
    sites.retain(|(_, name)| !name.is_empty() && !name.contains("::"));
    sites
}

fn collect_sites<'tree>(
    base: &BaseExtractor,
    node: Node<'tree>,
    sites: &mut Vec<(Node<'tree>, String)>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    match node.kind() {
        "flow_node" | "block_node" => {
            if let Some(tag) = first_named_child(node, "tag") {
                let tag = base.get_node_text(&tag);
                push_intrinsic(base, tag.trim_start_matches('!'), node, sites);
            }
        }
        "block_mapping_pair" | "flow_pair" => {
            if let (Some(key), Some(value)) = (
                pair_key(&base.content, node),
                node.child_by_field_name("value"),
            ) {
                push_intrinsic(base, key.trim_start_matches("Fn::"), value, sites);
            }
        }
        _ => {}
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_sites(base, child, sites, child_depth);
    }
}

fn push_intrinsic<'tree>(
    base: &BaseExtractor,
    function: &str,
    value: Node<'tree>,
    sites: &mut Vec<(Node<'tree>, String)>,
) {
    match function {
        "Ref" => {
            if let Some((node, name)) = scalar_value(&base.content, value) {
                sites.push((node, name));
            }
        }
        "DependsOn" => sites.extend(scalar_list(&base.content, value)),
        "GetAtt" => {
            if let Some((node, text)) = scalar_list(&base.content, value).into_iter().next() {
                let name = text.split('.').next().unwrap_or_default().to_string();
                sites.push((node, name));
            }
        }
        "Sub" => {
            if let Some((node, text)) = scalar_list(&base.content, value).into_iter().next() {
                for name in substitution_names(&text) {
                    sites.push((node, name));
                }
            }
        }
        _ => {}
    }
}

/// Names in `${X}` / `${X.Attr}` placeholders; `${!Literal}` is an escape.
fn substitution_names(template: &str) -> Vec<String> {
    template
        .split("${")
        .skip(1)
        .filter_map(|rest| rest.split_once('}'))
        .map(|(inner, _)| inner.trim())
        .filter(|inner| !inner.starts_with('!'))
        .map(|inner| inner.split('.').next().unwrap_or_default().to_string())
        .collect()
}

fn first_named_child<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| child.kind() == kind)
}
