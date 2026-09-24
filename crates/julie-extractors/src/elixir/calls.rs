/// Core call dispatch for Elixir extraction.
///
/// In tree-sitter-elixir, nearly every definition is a `call` node.
/// This module inspects the call target and dispatches to the appropriate handler.
use super::ElixirExtractor;
use super::attributes;
use super::definition_forms;
use super::helpers;
use super::parameters;
use super::test_calls;
use super::type_facts;
use crate::base::TestRole;
use crate::base::{Symbol, SymbolKind, SymbolOptions, Visibility, normalize_annotations};
use crate::test_detection::{apply_callable_test_metadata, apply_test_role};
use crate::tree_traversal::child_tree_depth;
use serde_json::Value;
use std::collections::HashMap;
use tree_sitter::Node;

/// Dispatch a call node to the appropriate extraction handler.
///
/// Returns `Some((symbol, children_visited))` if this call defines a symbol.
/// `children_visited` is true when the handler already traversed child nodes
/// (e.g., defmodule visits its do_block to extract nested definitions).
pub(super) fn dispatch_call(
    extractor: &mut ElixirExtractor,
    node: &Node,
    symbols: &mut Vec<Symbol>,
    parent_id: Option<&str>,
    depth: u32,
) -> Option<(Symbol, bool)> {
    let target_name = helpers::extract_call_target_name(&extractor.base, node)?;
    match target_name.as_str() {
        "defmodule" => extract_defmodule(extractor, node, symbols, parent_id, depth),
        "def" => extract_def(
            extractor,
            node,
            symbols,
            parent_id,
            depth,
            Visibility::Public,
        ),
        "defp" => extract_def(
            extractor,
            node,
            symbols,
            parent_id,
            depth,
            Visibility::Private,
        ),
        "defmacro" => extract_defmacro(
            extractor,
            node,
            symbols,
            parent_id,
            depth,
            Visibility::Public,
        ),
        "defmacrop" => extract_defmacro(
            extractor,
            node,
            symbols,
            parent_id,
            depth,
            Visibility::Private,
        ),
        "defguard" => {
            definition_forms::extract_defguard(extractor, node, parent_id, Visibility::Public)
        }
        "defguardp" => {
            definition_forms::extract_defguard(extractor, node, parent_id, Visibility::Private)
        }
        "defdelegate" => definition_forms::extract_defdelegate(extractor, node, parent_id),
        "defprotocol" => extract_defprotocol(extractor, node, symbols, parent_id, depth),
        "defimpl" => extract_defimpl(extractor, node, symbols, parent_id, depth),
        "defstruct" => extract_defstruct(extractor, node, symbols, parent_id),
        "defexception" => {
            definition_forms::extract_defexception(extractor, node, symbols, parent_id)
        }
        "defoverridable" => definition_forms::extract_defoverridable(extractor, node, parent_id),
        "import" => extract_import_call(extractor, node, symbols, parent_id),
        "use" => extract_use_call(extractor, node, symbols, parent_id),
        "alias" => extract_alias_call(extractor, node, symbols, parent_id),
        "require" => extract_require_call(extractor, node, symbols, parent_id),
        "test" | "property" => test_calls::extract_test(extractor, node, &target_name, parent_id),
        "doctest" => test_calls::extract_doctest(extractor, node, parent_id),
        "schema" | "embedded_schema" => {
            definition_forms::extract_ecto_schema(extractor, node, &target_name, symbols, parent_id)
        }
        "describe" => test_calls::extract_describe(extractor, node, symbols, parent_id, depth),
        "setup" | "setup_all" => {
            test_calls::extract_setup(extractor, node, &target_name, parent_id)
        }
        _ => None,
    }
}

// ========================================================================
// defmodule
// ========================================================================

fn extract_defmodule(
    extractor: &mut ElixirExtractor,
    node: &Node,
    symbols: &mut Vec<Symbol>,
    parent_id: Option<&str>,
    depth: u32,
) -> Option<(Symbol, bool)> {
    let declared_name = helpers::extract_module_name(&extractor.base, node)?;
    let module_name = match extractor.module_stack.last() {
        Some(parent) => format!("{parent}.{declared_name}"),
        None => declared_name,
    };

    let signature = format!("defmodule {}", module_name);
    let doc_comment = attributes::extract_moduledoc_for_module(&extractor.base, node);
    let annotations = normalize_annotations(
        &attributes::collect_module_annotations(&extractor.base, node),
        "elixir",
    );
    let metadata = test_calls::is_exunit_case_module(&extractor.base, node).then(|| {
        let mut metadata = HashMap::new();
        apply_test_role(&mut metadata, TestRole::TestContainer);
        metadata
    });

    let symbol = extractor.base.create_symbol(
        node,
        module_name.clone(),
        SymbolKind::Module,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(String::from),
            metadata,
            doc_comment,
            annotations,
        },
    );

    let sym_id = symbol.id.clone();

    // Push module name for qualified name building
    extractor.module_stack.push(module_name);

    // Visit do_block children to extract nested definitions
    if let Some(do_block) = helpers::extract_do_block(node)
        && let Some(child_depth) = child_tree_depth(depth)
    {
        extractor.traverse_children(&do_block, symbols, Some(&sym_id), child_depth);
    }

    extractor.module_stack.pop();

    Some((symbol, true)) // children already visited
}

// ========================================================================
// def / defp
// ========================================================================

fn extract_def(
    extractor: &mut ElixirExtractor,
    node: &Node,
    symbols: &mut Vec<Symbol>,
    parent_id: Option<&str>,
    depth: u32,
    visibility: Visibility,
) -> Option<(Symbol, bool)> {
    let (fn_name, params) = helpers::extract_function_head(&extractor.base, node)?;

    let signature = match &params {
        Some(p) => format!("def {}{}", fn_name, p),
        None => format!("def {}", fn_name),
    };
    let doc_comment = attributes::extract_doc_comment_for_node(&extractor.base, node, "doc");
    let annotations = normalize_annotations(
        &attributes::collect_preceding_annotations(&extractor.base, node, &["doc", "spec", "impl"]),
        "elixir",
    );
    let annotation_keys = annotations
        .iter()
        .map(|annotation| annotation.annotation_key.clone())
        .collect::<Vec<_>>();

    let mut test_metadata = HashMap::new();
    apply_callable_test_metadata(
        "elixir",
        &fn_name,
        &extractor.base.file_path,
        &SymbolKind::Function,
        &annotation_keys,
        doc_comment.as_deref(),
        &mut test_metadata,
    );
    let metadata = (!test_metadata.is_empty()).then_some(test_metadata);

    let mut symbol = extractor.base.create_symbol(
        node,
        fn_name,
        SymbolKind::Function,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(visibility),
            parent_id: parent_id.map(String::from),
            metadata,
            doc_comment,
            annotations,
        },
    );
    helpers::set_body_span(
        &extractor.base,
        &mut symbol,
        helpers::definition_body(&extractor.base, node),
    );
    record_spec_type(extractor, node, &symbol);
    extract_callable_bindings(extractor, node, &symbol.id, symbols, depth);
    Some((symbol, false))
}

// ========================================================================
// defmacro / defmacrop
// ========================================================================

fn extract_defmacro(
    extractor: &mut ElixirExtractor,
    node: &Node,
    symbols: &mut Vec<Symbol>,
    parent_id: Option<&str>,
    depth: u32,
    visibility: Visibility,
) -> Option<(Symbol, bool)> {
    let (macro_name, params) = helpers::extract_function_head(&extractor.base, node)?;

    let keyword = if visibility == Visibility::Private {
        "defmacrop"
    } else {
        "defmacro"
    };
    let signature = match &params {
        Some(p) => format!("{} {}{}", keyword, macro_name, p),
        None => format!("{} {}", keyword, macro_name),
    };
    let doc_comment = attributes::extract_doc_comment_for_node(&extractor.base, node, "doc");
    let annotations = normalize_annotations(
        &attributes::collect_preceding_annotations(&extractor.base, node, &["doc", "spec", "impl"]),
        "elixir",
    );

    let mut metadata = HashMap::new();
    metadata.insert("macro".to_string(), Value::Bool(true));

    let mut symbol = extractor.base.create_symbol(
        node,
        macro_name,
        SymbolKind::Function,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(visibility),
            parent_id: parent_id.map(String::from),
            metadata: Some(metadata),
            doc_comment,
            annotations,
        },
    );
    helpers::set_body_span(
        &extractor.base,
        &mut symbol,
        helpers::definition_body(&extractor.base, node),
    );
    record_spec_type(extractor, node, &symbol);
    extract_callable_bindings(extractor, node, &symbol.id, symbols, depth);
    Some((symbol, false))
}

/// Queue a definition for the `@spec` with its module, name, and full arity.
/// The match waits for the end of the file because a spec may follow the
/// definition it describes.
fn record_spec_type(extractor: &mut ElixirExtractor, node: &Node, symbol: &Symbol) {
    let arity = helpers::definition_arity(&extractor.base, node).1;
    let key = (
        extractor.module_stack.last().cloned(),
        symbol.name.clone(),
        arity,
    );
    extractor.spec_definitions.push((symbol.id.clone(), key));
}

fn extract_callable_bindings(
    extractor: &mut ElixirExtractor,
    node: &Node,
    callable_id: &str,
    symbols: &mut Vec<Symbol>,
    depth: u32,
) {
    symbols.extend(parameters::extract_parameter_symbols(
        &mut extractor.base,
        *node,
        callable_id,
    ));
    type_facts::extract_body_locals(&mut extractor.base, node, callable_id, symbols, depth);
}

// ========================================================================
// defprotocol
// ========================================================================

fn extract_defprotocol(
    extractor: &mut ElixirExtractor,
    node: &Node,
    symbols: &mut Vec<Symbol>,
    parent_id: Option<&str>,
    depth: u32,
) -> Option<(Symbol, bool)> {
    let protocol_name = helpers::extract_module_name(&extractor.base, node)?;

    let signature = format!("defprotocol {}", protocol_name);
    let doc_comment = attributes::extract_moduledoc_for_module(&extractor.base, node);

    let symbol = extractor.base.create_symbol(
        node,
        protocol_name.clone(),
        SymbolKind::Interface,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(String::from),
            metadata: None,
            doc_comment,
            annotations: Vec::new(),
        },
    );

    let sym_id = symbol.id.clone();

    extractor.module_stack.push(protocol_name);

    if let Some(do_block) = helpers::extract_do_block(node)
        && let Some(child_depth) = child_tree_depth(depth)
    {
        extractor.traverse_children(&do_block, symbols, Some(&sym_id), child_depth);
    }

    extractor.module_stack.pop();

    Some((symbol, true))
}

// ========================================================================
// defimpl
// ========================================================================

fn extract_defimpl(
    extractor: &mut ElixirExtractor,
    node: &Node,
    symbols: &mut Vec<Symbol>,
    parent_id: Option<&str>,
    depth: u32,
) -> Option<(Symbol, bool)> {
    let protocol_name = helpers::extract_impl_protocol_name(&extractor.base, node)?;
    let for_types = helpers::impl_for_types(&extractor.base, node)
        .or_else(|| extractor.module_stack.last().map(|m| vec![m.clone()]))
        .unwrap_or_default();
    let impl_name = helpers::impl_name(&protocol_name, &for_types);

    let signature = match helpers::extract_keyword_value(&extractor.base, node, "for") {
        Some(for_text) => format!("defimpl {protocol_name}, for: {for_text}"),
        None => format!("defimpl {protocol_name}"),
    };
    let doc_comment = attributes::extract_doc_comment_for_node(&extractor.base, node, "doc");

    let mut metadata = HashMap::new();
    metadata.insert("protocol_impl".to_string(), Value::Bool(true));
    match for_types.as_slice() {
        [] => {}
        [single] => {
            metadata.insert("for_type".to_string(), Value::String(single.clone()));
        }
        _ => {
            metadata.insert(
                "for_types".to_string(),
                Value::Array(for_types.iter().cloned().map(Value::String).collect()),
            );
        }
    }
    metadata.insert("protocol".to_string(), Value::String(protocol_name));

    let symbol = extractor.base.create_symbol(
        node,
        impl_name.clone(),
        SymbolKind::Class,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(String::from),
            metadata: Some(metadata),
            doc_comment,
            annotations: Vec::new(),
        },
    );

    let sym_id = symbol.id.clone();

    extractor.module_stack.push(impl_name);

    if let Some(do_block) = helpers::extract_do_block(node)
        && let Some(child_depth) = child_tree_depth(depth)
    {
        extractor.traverse_children(&do_block, symbols, Some(&sym_id), child_depth);
    }

    extractor.module_stack.pop();

    Some((symbol, true))
}

// ========================================================================
// defstruct
// ========================================================================

fn extract_defstruct(
    extractor: &mut ElixirExtractor,
    node: &Node,
    symbols: &mut Vec<Symbol>,
    parent_id: Option<&str>,
) -> Option<(Symbol, bool)> {
    let fields = helpers::extract_struct_fields(&extractor.base, node);

    let struct_name = extractor
        .module_stack
        .last()
        .cloned()
        .unwrap_or_else(|| "Struct".to_string());

    let field_names: Vec<&str> = fields.iter().map(|(n, _)| n.as_str()).collect();
    let signature = format!("defstruct [{}]", field_names.join(", "));

    let symbol = extractor.base.create_symbol(
        node,
        struct_name,
        SymbolKind::Struct,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(String::from),
            metadata: None,
            doc_comment: None,
            annotations: Vec::new(),
        },
    );

    let sym_id = symbol.id.clone();

    for (field_name, field_node) in &fields {
        let field_sym = extractor.base.create_symbol(
            field_node,
            field_name.clone(),
            SymbolKind::Field,
            SymbolOptions {
                signature: Some(format!(":{}", field_name)),
                visibility: Some(Visibility::Public),
                parent_id: Some(sym_id.clone()),
                metadata: None,
                doc_comment: None,
                annotations: Vec::new(),
            },
        );
        symbols.push(field_sym);
    }

    Some((symbol, true)) // fields already emitted
}

// ========================================================================
// Import calls: use, import, alias, require
// ========================================================================

fn extract_import_call(
    extractor: &mut ElixirExtractor,
    node: &Node,
    symbols: &mut Vec<Symbol>,
    parent_id: Option<&str>,
) -> Option<(Symbol, bool)> {
    extract_directive(extractor, node, symbols, parent_id, "import")
}

fn extract_use_call(
    extractor: &mut ElixirExtractor,
    node: &Node,
    symbols: &mut Vec<Symbol>,
    parent_id: Option<&str>,
) -> Option<(Symbol, bool)> {
    extract_directive(extractor, node, symbols, parent_id, "use")
}

fn extract_alias_call(
    extractor: &mut ElixirExtractor,
    node: &Node,
    symbols: &mut Vec<Symbol>,
    parent_id: Option<&str>,
) -> Option<(Symbol, bool)> {
    extract_directive(extractor, node, symbols, parent_id, "alias")
}

fn extract_require_call(
    extractor: &mut ElixirExtractor,
    node: &Node,
    symbols: &mut Vec<Symbol>,
    parent_id: Option<&str>,
) -> Option<(Symbol, bool)> {
    extract_directive(extractor, node, symbols, parent_id, "require")
}

fn extract_directive(
    extractor: &mut ElixirExtractor,
    node: &Node,
    symbols: &mut Vec<Symbol>,
    parent_id: Option<&str>,
    keyword: &str,
) -> Option<(Symbol, bool)> {
    let signature = extractor.base.get_node_text(node).trim().to_string();
    let explicit_alias = helpers::extract_keyword_value(&extractor.base, node, "as");
    let mut imports = helpers::directive_modules(&extractor.base, node)
        .into_iter()
        .map(|module| {
            let local_name = match (&explicit_alias, keyword) {
                (Some(alias), _) => Some(alias.clone()),
                (None, "alias") => module.rsplit('.').next().map(str::to_string),
                _ => None,
            };
            let metadata = local_name
                .map(|alias| HashMap::from([("alias".to_string(), Value::String(alias))]));
            let mut symbol = extractor.base.create_symbol(
                node,
                module,
                SymbolKind::Import,
                SymbolOptions {
                    signature: Some(signature.clone()),
                    visibility: Some(Visibility::Public),
                    parent_id: parent_id.map(String::from),
                    metadata,
                    doc_comment: None,
                    annotations: Vec::new(),
                },
            );
            helpers::set_body_span(&extractor.base, &mut symbol, None);
            symbol
        })
        .collect::<Vec<_>>();
    if imports.is_empty() {
        return None;
    }
    let first = imports.remove(0);
    symbols.extend(imports);
    Some((first, false))
}
