use super::ElixirExtractor;
use super::helpers;
use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions, TestRole, Visibility};
use crate::test_detection::{apply_test_role, is_test_path};
use crate::tree_traversal::child_tree_depth;
use std::collections::HashMap;
use tree_sitter::Node;

/// Extract an ExUnit `test "description" do ... end` block, or a StreamData
/// `property "description" do ... end` block, as a Function symbol.
pub(super) fn extract_test(
    extractor: &mut ElixirExtractor,
    node: &Node,
    keyword: &str,
    parent_id: Option<&str>,
) -> Option<(Symbol, bool)> {
    let description = helpers::extract_first_string_arg(&extractor.base, node)?;
    let signature = format!("{keyword} \"{description}\"");

    let mut metadata = HashMap::new();
    apply_test_role(&mut metadata, TestRole::TestCase);

    let symbol = extractor.base.create_symbol(
        node,
        description,
        SymbolKind::Function,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Private),
            parent_id: parent_id.map(String::from),
            metadata: Some(metadata),
            doc_comment: None,
            annotations: Vec::new(),
        },
    );

    Some((symbol, false))
}

/// Extract an ExUnit `describe "context" do ... end` block as a Namespace symbol.
/// Traverses child nodes to extract nested test definitions.
pub(super) fn extract_describe(
    extractor: &mut ElixirExtractor,
    node: &Node,
    symbols: &mut Vec<Symbol>,
    parent_id: Option<&str>,
    depth: u32,
) -> Option<(Symbol, bool)> {
    let description = helpers::extract_first_string_arg(&extractor.base, node)?;
    let signature = format!("describe \"{}\"", description);

    let mut metadata = HashMap::new();
    apply_test_role(&mut metadata, TestRole::TestContainer);

    let symbol = extractor.base.create_symbol(
        node,
        description,
        SymbolKind::Namespace,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Private),
            parent_id: parent_id.map(String::from),
            metadata: Some(metadata),
            doc_comment: None,
            annotations: Vec::new(),
        },
    );

    let sym_id = symbol.id.clone();
    if let Some(do_block) = helpers::extract_do_block(node)
        && let Some(child_depth) = child_tree_depth(depth)
    {
        extractor.traverse_children(&do_block, symbols, Some(&sym_id), child_depth);
    }

    Some((symbol, true))
}

/// Extract an ExUnit `setup` / `setup_all` lifecycle hook as a Function symbol.
pub(super) fn extract_setup(
    extractor: &mut ElixirExtractor,
    node: &Node,
    name: &str,
    parent_id: Option<&str>,
) -> Option<(Symbol, bool)> {
    let signature = format!("{}()", name);

    let mut metadata = HashMap::new();
    apply_test_role(&mut metadata, TestRole::FixtureSetup);

    let symbol = extractor.base.create_symbol(
        node,
        name.to_string(),
        SymbolKind::Function,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Private),
            parent_id: parent_id.map(String::from),
            metadata: Some(metadata),
            doc_comment: None,
            annotations: Vec::new(),
        },
    );

    Some((symbol, false))
}

/// Extract an ExUnit `doctest Module` as a test case that runs the module's
/// documentation examples.
pub(super) fn extract_doctest(
    extractor: &mut ElixirExtractor,
    node: &Node,
    parent_id: Option<&str>,
) -> Option<(Symbol, bool)> {
    let args = helpers::find_child_by_type(node, "arguments")?;
    let module = args
        .named_child(0)
        .filter(|arg| arg.kind() == "alias")
        .map(|arg| extractor.base.get_node_text(&arg))?;
    let name = format!("doctest {module}");

    let mut metadata = HashMap::new();
    apply_test_role(&mut metadata, TestRole::TestCase);

    let symbol = extractor.base.create_symbol(
        node,
        name.clone(),
        SymbolKind::Function,
        SymbolOptions {
            signature: Some(name),
            visibility: Some(Visibility::Private),
            parent_id: parent_id.map(String::from),
            metadata: Some(metadata),
            doc_comment: None,
            annotations: Vec::new(),
        },
    );
    Some((symbol, false))
}

/// True when a module body `use`s `ExUnit.Case`, or a `*Case` template
/// (`MyAppWeb.ConnCase`) in a test file.
pub(super) fn is_exunit_case_module(base: &BaseExtractor, node: &Node) -> bool {
    let Some(do_block) = helpers::extract_do_block(node) else {
        return false;
    };
    let mut cursor = do_block.walk();
    do_block.named_children(&mut cursor).any(|child| {
        helpers::extract_call_target_name(base, &child).as_deref() == Some("use")
            && helpers::directive_modules(base, &child)
                .iter()
                .any(|module| {
                    module == "ExUnit.Case"
                        || (module.ends_with("Case") && is_test_path(&base.file_path))
                })
    })
}
