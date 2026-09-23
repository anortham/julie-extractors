// R Relationship Extraction
// Extracts relationships between R symbols: function calls, library usage, pipes

use crate::base::{
    ContainingSymbolIndex, Relationship, RelationshipKind, ScopedSymbolIndex, Symbol, SymbolKind,
    UnresolvedTarget,
};
use crate::r::RExtractor;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::{Node, Tree};

/// Extract all relationships from R code
pub(super) fn extract_relationships(
    extractor: &mut RExtractor,
    tree: &Tree,
    symbols: &[Symbol],
) -> Vec<Relationship> {
    let call_targets: Vec<Symbol> = symbols
        .iter()
        .filter(|symbol| !is_test_symbol(symbol))
        .cloned()
        .collect();
    let symbol_index = ScopedSymbolIndex::new(&call_targets);
    let function_symbols = ContainingSymbolIndex::from_iter(symbols.iter().filter(|s| {
        matches!(
            s.kind,
            SymbolKind::Function | SymbolKind::Method | SymbolKind::Variable
        ) && !is_parameter(s)
    }));
    let mut relationships = Vec::new();
    extract_extends_relationships(extractor, tree, symbols, &mut relationships);
    extract_call_relationships(
        extractor,
        tree.root_node(),
        &symbol_index,
        &function_symbols,
        &mut relationships,
        0,
    );
    extract_pipe_relationships(
        extractor,
        tree.root_node(),
        &symbol_index,
        &function_symbols,
        &mut relationships,
        0,
    );
    extract_member_access_relationships(extractor, tree.root_node(), &function_symbols, 0);
    relationships
}

/// Extract function call relationships
fn extract_call_relationships<'a>(
    extractor: &mut RExtractor,
    node: Node,
    symbol_index: &ScopedSymbolIndex<'a>,
    function_symbols: &ContainingSymbolIndex<'a>,
    relationships: &mut Vec<Relationship>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    if let Some(operator) = user_operator(extractor, node)
        && let Some(caller_symbol) = find_containing_function(node, function_symbols)
    {
        let target = UnresolvedTarget::simple(operator.clone());
        match local_call_target(extractor, symbol_index, &target, caller_symbol, node) {
            Some(called_symbol) => relationships.push(call_relationship(
                extractor,
                caller_symbol,
                called_symbol,
                node,
            )),
            None => {
                let pending = extractor.base.create_pending_relationship(
                    caller_symbol.id.clone(),
                    target,
                    RelationshipKind::Calls,
                    &node,
                    Some(caller_symbol.id.clone()),
                    Some(0.7),
                );
                extractor.add_structured_pending_relationship(pending);
            }
        }
    }

    // R function calls are represented as "call" nodes
    if node.kind() == "call" {
        // The function being called is the first child
        if let Some(function_node) = node.child(0) {
            let function_name = match function_node.kind() {
                "identifier" => extractor.base.get_node_text(&function_node),
                "namespace_operator" => {
                    // Handle package::function syntax
                    if let Some(function_child) = function_node.child(2) {
                        extractor.base.get_node_text(&function_child)
                    } else {
                        extractor.base.get_node_text(&function_node)
                    }
                }
                "extract_operator" => {
                    // Handle $ operator: object$method()
                    if let Some(member) = function_node.child(2) {
                        extractor.base.get_node_text(&member)
                    } else {
                        extractor.base.get_node_text(&function_node)
                    }
                }
                _ => extractor.base.get_node_text(&function_node),
            };

            // A DSL call (`test_that`, `setMethod`) never calls itself from the symbol it declares.
            let caller = find_containing_function(node, function_symbols)
                .filter(|caller| caller.start_byte != node.start_byte() as u32);
            if let Some(caller_symbol) = caller {
                let target = unresolved_call_target(extractor, function_node, &function_name);
                let local_target =
                    local_call_target(extractor, symbol_index, &target, caller_symbol, node);

                if let Some(called_symbol) = local_target {
                    relationships.push(call_relationship(
                        extractor,
                        caller_symbol,
                        called_symbol,
                        node,
                    ));
                } else if !is_declaration_call(extractor, node, &function_name)
                    && (function_node.kind() != "identifier"
                        || !is_builtin_function(&function_name))
                {
                    let receiver_type =
                        super::type_facts::self_receiver_type(extractor, function_node);
                    let pending = extractor
                        .base
                        .create_pending_relationship(
                            caller_symbol.id.clone(),
                            target,
                            RelationshipKind::Calls,
                            &node,
                            Some(caller_symbol.id.clone()),
                            Some(0.7),
                        )
                        .with_receiver_type(receiver_type);
                    extractor.add_structured_pending_relationship(pending);
                }
                // Built-in functions (print, mean, length, etc.) are silently
                // dropped - they're known base R functions that don't need resolution
            }
        }
    }

    // Recursively process children
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        extract_call_relationships(
            extractor,
            child,
            symbol_index,
            function_symbols,
            relationships,
            child_depth,
        );
    }
}

/// Extract pipe operator relationships (%>%, |>, etc.)
fn extract_pipe_relationships<'a>(
    extractor: &mut RExtractor,
    node: Node,
    symbol_index: &ScopedSymbolIndex<'a>,
    function_symbols: &ContainingSymbolIndex<'a>,
    relationships: &mut Vec<Relationship>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    // Pipe operators in R are binary operators
    if node.kind() == "binary_operator"
        && let Some(operator) = node.child(1)
    {
        let op_text = extractor.base.get_node_text(&operator);

        // Piped calls (`x %>% f()`) are ordinary call nodes that the call pass
        // already covers; only magrittr's bare-function target needs an edge here.
        if op_text == "%>%"
            && let Some(right_child) = node.child(2)
            && right_child.kind() == "identifier"
        {
            let function_name = extractor.base.get_node_text(&right_child);
            if let Some(containing_symbol) = find_containing_function(node, function_symbols) {
                let target = UnresolvedTarget::simple(function_name.clone());
                match local_call_target(
                    extractor,
                    symbol_index,
                    &target,
                    containing_symbol,
                    right_child,
                ) {
                    Some(called_symbol) => relationships.push(call_relationship(
                        extractor,
                        containing_symbol,
                        called_symbol,
                        right_child,
                    )),
                    None if !is_builtin_function(&function_name) => {
                        let pending = extractor.base.create_pending_relationship(
                            containing_symbol.id.clone(),
                            target,
                            RelationshipKind::Calls,
                            &right_child,
                            Some(containing_symbol.id.clone()),
                            Some(0.7),
                        );
                        extractor.add_structured_pending_relationship(pending);
                    }
                    None => {}
                }
            }
        }
    }

    // Recursively process children
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        extract_pipe_relationships(
            extractor,
            child,
            symbol_index,
            function_symbols,
            relationships,
            child_depth,
        );
    }
}

/// Extract member access relationships ($ operator)
fn extract_member_access_relationships<'a>(
    extractor: &mut RExtractor,
    node: Node,
    function_symbols: &ContainingSymbolIndex<'a>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    let is_called = node.parent().is_some_and(|call| {
        call.kind() == "call" && call.child_by_field_name("function") == Some(node)
    });
    // R uses extract_operator for $ and @; a called member is covered by the call pass.
    if node.kind() == "extract_operator" && !is_called {
        // The member being accessed is the third child (index 2)
        if let Some(member_node) = node.child(2) {
            let member_name = extractor.base.get_node_text(&member_node);

            // Find containing function
            if let Some(containing_symbol) = find_containing_function(node, function_symbols) {
                // Member access targets can't be resolved locally (they're dynamic)
                // Use PendingRelationship for cross-file resolution
                let expression_text = extractor.base.get_node_text(&node);
                let target =
                    UnresolvedTarget::from_qualified_text(&expression_text, R_CHAIN_SEPARATORS)
                        .unwrap_or_else(|| {
                            let receiver = node
                                .child(0)
                                .map(|object| extractor.base.get_node_text(&object));
                            let display_name = receiver
                                .as_ref()
                                .map(|receiver| format!("{receiver}.{member_name}"))
                                .unwrap_or_else(|| member_name.clone());
                            UnresolvedTarget {
                                display_name,
                                terminal_name: member_name.clone(),
                                receiver,
                                namespace_path: Vec::new(),
                                import_context: None,
                            }
                        });
                let pending = extractor.base.create_pending_relationship(
                    containing_symbol.id.clone(),
                    target,
                    RelationshipKind::Uses,
                    &node,
                    Some(containing_symbol.id.clone()),
                    Some(0.6),
                );
                extractor.add_structured_pending_relationship(pending);
            }
        }
    }

    // Recursively process children
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        extract_member_access_relationships(extractor, child, function_symbols, child_depth);
    }
}

fn is_parameter(symbol: &Symbol) -> bool {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("role"))
        .and_then(|role| role.as_str())
        == Some("parameter")
}

fn is_test_symbol(symbol: &Symbol) -> bool {
    symbol
        .metadata
        .as_ref()
        .is_some_and(|metadata| metadata.contains_key("test_role"))
}

/// A same-file function or method a call resolves to. `self$m()` and
/// `private$m()` resolve within the caller's class; `super$m()` resolves in the
/// class named by `inherit`. Test DSL symbols are never call targets.
fn local_call_target<'a>(
    extractor: &RExtractor,
    symbol_index: &ScopedSymbolIndex<'a>,
    target: &UnresolvedTarget,
    caller: &'a Symbol,
    site: Node,
) -> Option<&'a Symbol> {
    if !target.namespace_path.is_empty() {
        return None;
    }
    let resolved = match target.receiver.as_deref() {
        Some("super") => {
            let base = super::type_facts::super_receiver_type(extractor, site)?;
            symbol_index
                .candidates_by_name(&target.terminal_name)
                .filter(|candidate| {
                    candidate.parent_id.as_deref().is_some_and(|parent_id| {
                        symbol_index
                            .candidates_by_name(&base)
                            .any(|class| class.id == parent_id && class.kind == SymbolKind::Class)
                    })
                })
                .fold(
                    None,
                    |found: Option<Option<&Symbol>>, candidate| match found {
                        None => Some(Some(candidate)),
                        Some(_) => Some(None),
                    },
                )
                .flatten()
        }
        Some("private") => symbol_index
            .resolve_call_target(&target.terminal_name, Some(caller), Some("self"))
            .as_symbol(),
        receiver => symbol_index
            .resolve_call_target(&target.terminal_name, Some(caller), receiver)
            .as_symbol(),
    };
    resolved.filter(|symbol| matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method))
}

fn call_relationship(
    extractor: &RExtractor,
    caller: &Symbol,
    callee: &Symbol,
    node: Node,
) -> Relationship {
    Relationship {
        id: format!(
            "{}_{}_{:?}_{}",
            caller.id,
            callee.id,
            RelationshipKind::Calls,
            node.start_position().row
        ),
        from_symbol_id: caller.id.clone(),
        to_symbol_id: callee.id.clone(),
        kind: RelationshipKind::Calls,
        file_path: extractor.base.file_path.clone(),
        line_number: (node.start_position().row + 1) as u32,
        span: Some(crate::base::NormalizedSpan::from_node(&node)),
        reference_site_is_exact: false,
        confidence: 1.0,
        metadata: None,
    }
}

/// The name of a user-defined `%op%` operator applied at `node`.
pub(super) fn user_operator(extractor: &RExtractor, node: Node) -> Option<String> {
    if node.kind() != "binary_operator" {
        return None;
    }
    let operator = extractor
        .base
        .get_node_text(&node.child_by_field_name("operator")?);
    let is_special = operator.len() > 2 && operator.starts_with('%') && operator.ends_with('%');
    let is_base = matches!(
        operator.as_str(),
        "%%" | "%/%" | "%*%" | "%o%" | "%x%" | "%in%" | "%>%" | "%<>%" | "%T>%" | "%$%" | "%||%"
    );
    (is_special && !is_base).then_some(operator)
}

/// Resolve `inherit =` / `contains =` declarations to `extends` edges.
fn extract_extends_relationships(
    extractor: &mut RExtractor,
    tree: &Tree,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let requests = std::mem::take(&mut extractor.extends_requests);
    for request in requests {
        let Some(site) = tree
            .root_node()
            .descendant_for_byte_range(request.start_byte, request.end_byte)
        else {
            continue;
        };
        let (namespace, base) = match request.base.rsplit_once("::") {
            Some((namespace, base)) => (Some(namespace.trim_end_matches(':')), base),
            None => (None, request.base.as_str()),
        };
        let local = namespace.is_none().then(|| {
            symbols
                .iter()
                .find(|symbol| symbol.kind == SymbolKind::Class && symbol.name == base)
        });
        match local.flatten() {
            Some(parent) => relationships.push(Relationship {
                id: format!(
                    "{}_{}_{:?}_{}",
                    request.class_id,
                    parent.id,
                    RelationshipKind::Extends,
                    site.start_position().row
                ),
                from_symbol_id: request.class_id.clone(),
                to_symbol_id: parent.id.clone(),
                kind: RelationshipKind::Extends,
                file_path: extractor.base.file_path.clone(),
                line_number: (site.start_position().row + 1) as u32,
                span: Some(crate::base::NormalizedSpan::from_node(&site)),
                reference_site_is_exact: false,
                confidence: 1.0,
                metadata: None,
            }),
            None => {
                let target = UnresolvedTarget {
                    display_name: request.base.clone(),
                    terminal_name: base.to_string(),
                    receiver: None,
                    namespace_path: namespace.map(str::to_string).into_iter().collect(),
                    import_context: None,
                };
                let pending = extractor.base.create_pending_relationship(
                    request.class_id.clone(),
                    target,
                    RelationshipKind::Extends,
                    &site,
                    Some(request.class_id.clone()),
                    Some(0.9),
                );
                extractor.add_structured_pending_relationship(pending);
            }
        }
    }
}

/// Find the containing function for a node using byte-range containment
fn find_containing_function<'a>(
    node: Node,
    function_symbols: &ContainingSymbolIndex<'a>,
) -> Option<&'a Symbol> {
    function_symbols.find(node)
}

/// Class and generic generators (bare or `pkg::`-qualified) and package or
/// file loads declare a symbol; they are not calls from the symbol they bind.
fn is_declaration_call(extractor: &RExtractor, call: Node, function_name: &str) -> bool {
    matches!(
        function_name,
        "R6Class" | "setRefClass" | "new_class" | "new_generic" | "new_property"
    ) || super::idioms::import_modules(extractor, call).is_some()
}

/// Check if a function name is a built-in R function
/// Built-in functions should not create pending relationships (they're known to be in base R)
fn is_builtin_function(name: &str) -> bool {
    matches!(
        name,
        // Core R functions
        "c" | "list" | "data.frame" | "matrix" | "array" | "factor" | "as.numeric" |
        "as.character" | "as.logical" | "as.integer" | "as.data.frame" | "as.matrix" |
        "as.list" | "as.vector" | "length" | "names" | "dim" | "nrow" | "ncol" | "class" |
        "typeof" | "is.na" | "is.null" | "is.numeric" | "is.character" | "is.logical" |
        // Math functions
        "mean" | "median" | "sum" | "min" | "max" | "abs" | "sqrt" | "exp" | "log" | "log10" |
        "sin" | "cos" | "tan" | "round" | "floor" | "ceiling" | "trunc" | "sign" |
        // Statistics functions
        "sd" | "var" | "cov" | "cor" | "lm" | "glm" | "summary" | "quantile" | "range" |
        // I/O functions
        "print" | "cat" | "paste" | "paste0" | "sprintf" | "format" | "write" | "read.csv" |
        "read.table" | "write.csv" | "write.table" | "readLines" | "writeLines" |
        // Control flow
        "if" | "else" | "for" | "while" | "repeat" | "break" | "next" | "return" | "stop" |
        "warning" | "message" | "invisible" | "do.call" | "lapply" | "sapply" | "mapply" |
        "tapply" | "Reduce" | "Filter" | "Map" | "Vectorize" |
        // Common utility functions
        "seq" | "rep" | "sort" | "order" | "rank" | "unique" | "duplicated" | "which" |
        "head" | "tail" | "str" | "View" | "rm" | "ls" | "exists" | "all" | "any" |
        "subset" | "merge" | "rbind" | "cbind" | "t" | "apply" |
        // String functions
        "nchar" | "substr" | "substring" | "strsplit" | "tolower" | "toupper" | "trimws" |
        "grep" | "grepl" | "sub" | "gsub" | "match" | "pmatch" | "charmatch" |
        // Type checking and conversion
        "is.data.frame" | "is.matrix" | "is.array" | "is.factor" | "is.ordered" |
        "is.function" | "is.list" | "is.atomic" | "is.recursive" |
        // Operators
        "+" | "-" | "*" | "/" | "^" | "%%" | "%/%" | "%*%" | ":" | "~" |
        // Base functions
        "Sys.time" | "Sys.Date" | "system" | "system2" | "shell" | "getwd" | "setwd" |
        "list.files" | "dir" | "file.exists" | "file.create" | "file.remove" | "file.rename" |
        "dir.create" | "tempdir" | "tempfile" | "library" | "require" | "source" |
        "eval" | "parse" | "deparse" | "substitute" | "quote" | "expression" |
        // Environment/Scope
        "parent.frame" | "parent.env" | "environment" | "new.env" | "with" | "within" |
        "attach" | "detach" | "search" | "get" | "assign" | "remove" |
        // Common functions from tidyverse-like operations
        "filter" | "select" | "mutate" | "arrange" | "group_by" | "summarize" | "summarise" |
        "join" | "left_join" | "right_join" | "inner_join" | "full_join" | "ggplot" | "aes" |
        // S3, S4, and R6 declaration machinery
        "UseMethod" | "NextMethod" | "standardGeneric" | "callNextMethod" | "signature" |
        "representation" | "setClass" | "setGeneric" | "setMethod" | "setReplaceMethod" |
        "setValidity" | "setRefClass" | "validObject" | "R6Class"
    )
}

const R_CHAIN_SEPARATORS: &[&str] = &["$", "@"];

fn unresolved_call_target(
    extractor: &RExtractor,
    function_node: Node,
    fallback_name: &str,
) -> UnresolvedTarget {
    match function_node.kind() {
        "namespace_operator" => {
            let namespace = function_node
                .child(0)
                .map(|node| extractor.base.get_node_text(&node));
            let terminal_name = function_node
                .child(2)
                .map(|node| extractor.base.get_node_text(&node))
                .unwrap_or_else(|| fallback_name.to_string());
            let display_name = namespace
                .as_ref()
                .map(|namespace| format!("{namespace}::{terminal_name}"))
                .unwrap_or_else(|| terminal_name.clone());
            UnresolvedTarget {
                display_name,
                terminal_name,
                receiver: None,
                namespace_path: namespace.into_iter().collect(),
                import_context: None,
            }
        }
        "extract_operator" => {
            let expression_text = extractor.base.get_node_text(&function_node);
            if let Some(chain) =
                UnresolvedTarget::from_qualified_text(&expression_text, R_CHAIN_SEPARATORS)
            {
                return chain;
            }
            let receiver = function_node
                .child(0)
                .map(|node| extractor.base.get_node_text(&node));
            let terminal_name = function_node
                .child(2)
                .map(|node| extractor.base.get_node_text(&node))
                .unwrap_or_else(|| fallback_name.to_string());
            let display_name = receiver
                .as_ref()
                .map(|receiver| format!("{receiver}.{terminal_name}"))
                .unwrap_or_else(|| terminal_name.clone());
            UnresolvedTarget {
                display_name,
                terminal_name,
                receiver,
                namespace_path: Vec::new(),
                import_context: None,
            }
        }
        _ => UnresolvedTarget::simple(fallback_name.to_string()),
    }
}
