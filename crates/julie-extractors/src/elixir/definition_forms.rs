use super::{ElixirExtractor, attributes, helpers};
use crate::base::{
    BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility, normalize_annotations,
};
use serde_json::Value;
use std::collections::HashMap;
use tree_sitter::Node;

pub(super) fn extract_defguard(
    extractor: &mut ElixirExtractor,
    node: &Node,
    parent_id: Option<&str>,
    visibility: Visibility,
) -> Option<(Symbol, bool)> {
    let (fn_name, params) = helpers::extract_function_head(&extractor.base, node)?;
    let keyword = if visibility == Visibility::Private {
        "defguardp"
    } else {
        "defguard"
    };
    let signature = match &params {
        Some(p) => format!("{} {}{}", keyword, fn_name, p),
        None => format!("{} {}", keyword, fn_name),
    };
    let annotations = normalize_annotations(
        &attributes::collect_preceding_annotations(&extractor.base, node, &["doc", "spec", "impl"]),
        "elixir",
    );
    let mut metadata = HashMap::new();
    metadata.insert("guard".to_string(), Value::Bool(true));

    let mut symbol = extractor.base.create_symbol(
        node,
        fn_name,
        SymbolKind::Function,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(visibility),
            parent_id: parent_id.map(String::from),
            metadata: Some(metadata),
            doc_comment: attributes::extract_doc_comment_for_node(&extractor.base, node, "doc"),
            annotations,
        },
    );
    helpers::set_body_span(
        &extractor.base,
        &mut symbol,
        helpers::definition_body(&extractor.base, node),
    );
    Some((symbol, false))
}

pub(super) fn extract_defdelegate(
    extractor: &mut ElixirExtractor,
    node: &Node,
    parent_id: Option<&str>,
) -> Option<(Symbol, bool)> {
    let (fn_name, params) = helpers::extract_function_head(&extractor.base, node)?;
    let signature = match &params {
        Some(p) => format!("defdelegate {}{}", fn_name, p),
        None => format!("defdelegate {}", fn_name),
    };
    let annotations = normalize_annotations(
        &attributes::collect_preceding_annotations(&extractor.base, node, &["doc", "spec"]),
        "elixir",
    );
    let mut metadata = HashMap::new();
    metadata.insert("delegate".to_string(), Value::Bool(true));
    if let Some(target) = helpers::extract_keyword_value(&extractor.base, node, "to") {
        metadata.insert("delegate_to".to_string(), Value::String(target));
    }
    if let Some(renamed) = helpers::extract_keyword_value(&extractor.base, node, "as") {
        let renamed = renamed.trim_start_matches(':').to_string();
        metadata.insert("delegate_as".to_string(), Value::String(renamed));
    }

    let mut symbol = extractor.base.create_symbol(
        node,
        fn_name,
        SymbolKind::Delegate,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(String::from),
            metadata: Some(metadata),
            doc_comment: attributes::extract_doc_comment_for_node(&extractor.base, node, "doc"),
            annotations,
        },
    );
    helpers::set_body_span(&extractor.base, &mut symbol, None);
    Some((symbol, false))
}

pub(super) fn extract_defexception(
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
        .unwrap_or_else(|| "Exception".to_string());
    let field_names: Vec<&str> = fields.iter().map(|(n, _)| n.as_str()).collect();
    let signature = format!("defexception [{}]", field_names.join(", "));
    let annotations = normalize_annotations(
        &attributes::collect_preceding_annotations(&extractor.base, node, &["doc"]),
        "elixir",
    );
    let mut metadata = HashMap::new();
    metadata.insert("exception".to_string(), Value::Bool(true));

    let symbol = extractor.base.create_symbol(
        node,
        struct_name,
        SymbolKind::Struct,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(String::from),
            metadata: Some(metadata),
            doc_comment: attributes::extract_doc_comment_for_node(&extractor.base, node, "doc"),
            annotations,
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

    Some((symbol, true))
}

pub(super) fn extract_defoverridable(
    extractor: &mut ElixirExtractor,
    node: &Node,
    parent_id: Option<&str>,
) -> Option<(Symbol, bool)> {
    let name = extract_overridable_name(&extractor.base.get_node_text(node))?;
    let mut metadata = HashMap::new();
    metadata.insert("overridable".to_string(), Value::Bool(true));
    let signature = format!("defoverridable {}", name.replace('/', ": "));
    let symbol = extractor.base.create_symbol(
        node,
        name,
        SymbolKind::Method,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(String::from),
            metadata: Some(metadata),
            doc_comment: attributes::extract_doc_comment_for_node(&extractor.base, node, "doc"),
            annotations: Vec::new(),
        },
    );
    Some((symbol, false))
}

fn extract_overridable_name(text: &str) -> Option<String> {
    let body = text.trim().strip_prefix("defoverridable")?.trim();
    let (name, arity) = body.split_once(':')?;
    let arity = arity
        .trim()
        .split(|ch: char| !ch.is_ascii_digit())
        .next()
        .unwrap_or_default();
    if name.trim().is_empty() || arity.is_empty() {
        None
    } else {
        Some(format!("{}/{}", name.trim(), arity))
    }
}

/// Extract an Ecto `schema "table" do ... end` or `embedded_schema do ... end`
/// block as the struct it defines, with one field per `field`, association,
/// embed, and `timestamps()` declaration.
pub(super) fn extract_ecto_schema(
    extractor: &mut ElixirExtractor,
    node: &Node,
    keyword: &str,
    symbols: &mut Vec<Symbol>,
    parent_id: Option<&str>,
) -> Option<(Symbol, bool)> {
    let do_block = helpers::extract_do_block(node)?;
    let table = match keyword {
        "schema" => {
            let args = helpers::find_child_by_type(node, "arguments")?;
            Some(helpers::string_literal_content(
                &extractor.base,
                &args.named_child(0)?,
            )?)
        }
        _ => None,
    };
    let struct_name = extractor.module_stack.last()?.clone();
    let signature = match &table {
        Some(table) => format!("schema \"{table}\""),
        None => keyword.to_string(),
    };
    let mut metadata = HashMap::from([("ecto_schema".to_string(), Value::Bool(true))]);
    if let Some(table) = table {
        metadata.insert("table".to_string(), Value::String(table));
    }

    let mut symbol = extractor.base.create_symbol(
        node,
        struct_name,
        SymbolKind::Struct,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(String::from),
            metadata: Some(metadata),
            doc_comment: None,
            annotations: Vec::new(),
        },
    );
    helpers::set_body_span(&extractor.base, &mut symbol, Some(do_block));

    let mut cursor = do_block.walk();
    for declaration in do_block.named_children(&mut cursor) {
        for (name, name_node) in ecto_field_names(&extractor.base, &declaration) {
            let signature = extractor
                .base
                .get_node_text(&declaration)
                .lines()
                .next()
                .unwrap_or_default()
                .trim()
                .to_string();
            let mut field = extractor.base.create_symbol(
                &name_node,
                name,
                SymbolKind::Field,
                SymbolOptions {
                    signature: Some(signature),
                    visibility: Some(Visibility::Public),
                    parent_id: Some(symbol.id.clone()),
                    metadata: None,
                    doc_comment: None,
                    annotations: Vec::new(),
                },
            );
            helpers::set_body_span(&extractor.base, &mut field, None);
            symbols.push(field);
        }
    }
    Some((symbol, true))
}

/// Struct fields one schema declaration adds, each with the node that names it.
/// `belongs_to` also adds its foreign key, and `timestamps()` adds the two
/// timestamp columns.
fn ecto_field_names<'a>(base: &BaseExtractor, declaration: &Node<'a>) -> Vec<(String, Node<'a>)> {
    let Some(macro_name) = helpers::extract_call_target_name(base, declaration) else {
        return Vec::new();
    };
    let keyword = |key: &str| helpers::extract_keyword_value(base, declaration, key);
    let atom_name = |text: String| text.trim_start_matches(':').to_string();
    match macro_name.as_str() {
        "field" | "belongs_to" | "has_one" | "has_many" | "many_to_many" | "embeds_one"
        | "embeds_many" => {
            let Some(name_node) = helpers::find_child_by_type(declaration, "arguments")
                .and_then(|args| args.named_child(0))
                .filter(|arg| arg.kind() == "atom")
            else {
                return Vec::new();
            };
            let name = atom_name(base.get_node_text(&name_node));
            let mut fields = vec![(name.clone(), name_node)];
            if macro_name == "belongs_to" && keyword("define_field").as_deref() != Some("false") {
                let foreign_key = keyword("foreign_key")
                    .map(atom_name)
                    .unwrap_or_else(|| format!("{name}_id"));
                fields.push((foreign_key, name_node));
            }
            fields
        }
        "timestamps" => {
            let anchor =
                helpers::find_child_by_type(declaration, "arguments").unwrap_or(*declaration);
            [("inserted_at", "inserted_at"), ("updated_at", "updated_at")]
                .into_iter()
                .filter_map(|(key, default)| match keyword(key).as_deref() {
                    Some("false") => None,
                    Some(renamed) => Some((atom_name(renamed.to_string()), anchor)),
                    None => Some((default.to_string(), anchor)),
                })
                .collect()
        }
        _ => Vec::new(),
    }
}
