/// Relationship extraction for Elixir symbols.
///
/// Handles: use (Uses), @behaviour (Implements), defimpl (Implements), function calls (Calls).
use super::{helpers, type_facts};
use crate::base::{
    BaseExtractor, ContainingSymbolIndex, Relationship, RelationshipKind, Symbol, SymbolKind,
    UnresolvedTarget,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::Node;

/// Extract all relationships from a parsed tree
pub(super) fn extract_relationships(
    extractor: &mut super::ElixirExtractor,
    tree: &tree_sitter::Tree,
    symbols: &[Symbol],
) -> Vec<Relationship> {
    let mut relationships = Vec::new();
    let scope = CallScope::new(&extractor.base, tree.root_node(), symbols);
    walk_for_relationships(
        extractor,
        tree.root_node(),
        symbols,
        &scope,
        &mut relationships,
        0,
    );
    relationships
}

/// Same-file lookup tables for Elixir call resolution. Elixir resolves a local
/// call inside the enclosing module only, and a remote call through the
/// module's `alias` directives, so both lookups are module-scoped.
struct CallScope<'a> {
    callers: ContainingSymbolIndex<'a>,
    modules: ContainingSymbolIndex<'a>,
    modules_by_name: HashMap<&'a str, &'a Symbol>,
    by_id: HashMap<&'a str, &'a Symbol>,
    callables: HashMap<(Option<&'a str>, &'a str), Vec<Callable<'a>>>,
    aliases: HashMap<Option<String>, HashMap<String, String>>,
    variables_by_start: HashMap<u32, &'a Symbol>,
}

struct Callable<'a> {
    symbol: &'a Symbol,
    min_arity: usize,
    max_arity: usize,
}

fn is_module_kind(symbol: &Symbol) -> bool {
    matches!(
        symbol.kind,
        SymbolKind::Module | SymbolKind::Class | SymbolKind::Interface
    )
}

/// def/defp/defmacro/defguard/defdelegate definitions: the only symbols a call
/// can target. Callbacks, tests, and setup hooks are not callable by name.
fn is_callable_definition(symbol: &Symbol) -> bool {
    matches!(symbol.kind, SymbolKind::Function | SymbolKind::Delegate)
        && symbol
            .signature
            .as_deref()
            .is_some_and(|signature| signature.starts_with("def"))
}

impl<'a> CallScope<'a> {
    fn new(base: &BaseExtractor, root: Node, symbols: &'a [Symbol]) -> Self {
        let by_id: HashMap<&str, &Symbol> = symbols.iter().map(|s| (s.id.as_str(), s)).collect();
        let module_symbols = || symbols.iter().filter(|s| is_module_kind(s));
        let mut scope = Self {
            callers: ContainingSymbolIndex::from_iter(symbols.iter().filter(|s| {
                s.kind == SymbolKind::Function
                    && !s
                        .signature
                        .as_deref()
                        .is_some_and(|signature| signature.starts_with('@'))
            })),
            modules: ContainingSymbolIndex::from_iter(module_symbols()),
            modules_by_name: module_symbols().map(|s| (s.name.as_str(), s)).collect(),
            by_id,
            callables: HashMap::new(),
            aliases: HashMap::new(),
            variables_by_start: symbols
                .iter()
                .filter(|s| s.kind == SymbolKind::Variable)
                .map(|s| (s.start_byte, s))
                .collect(),
        };
        for symbol in symbols.iter().filter(|s| is_callable_definition(s)) {
            let (min_arity, max_arity) = root
                .descendant_for_byte_range(symbol.start_byte as usize, symbol.end_byte as usize)
                .map(|node| helpers::definition_arity(base, &node))
                .unwrap_or((0, usize::MAX));
            let module = scope.module_of(symbol).map(|m| m.id.as_str());
            scope
                .callables
                .entry((module, symbol.name.as_str()))
                .or_default()
                .push(Callable {
                    symbol,
                    min_arity,
                    max_arity,
                });
        }
        scope.collect_aliases(base, root, 0);
        scope
    }

    fn module_of(&self, symbol: &Symbol) -> Option<&'a Symbol> {
        let mut current = symbol.parent_id.as_deref();
        while let Some(id) = current {
            let parent = self.by_id.get(id)?;
            if is_module_kind(parent) {
                return Some(parent);
            }
            current = parent.parent_id.as_deref();
        }
        None
    }

    fn enclosing_module(&self, node: &Node) -> Option<&'a Symbol> {
        self.modules.find(*node)
    }

    fn collect_aliases(&mut self, base: &BaseExtractor, node: Node, depth: u32) {
        if !should_visit_tree_depth(depth) {
            return;
        }
        if node.kind() == "call" {
            match helpers::extract_call_target_name(base, &node).as_deref() {
                Some("alias") | Some("require") => self.record_alias_directive(base, &node),
                Some("defmodule") => self.record_nested_module(base, &node),
                _ => {}
            }
        }
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect_aliases(base, child, child_depth);
        }
    }

    fn record_alias_directive(&mut self, base: &BaseExtractor, node: &Node) {
        let explicit = helpers::extract_keyword_value(base, node, "as");
        let is_alias = helpers::extract_call_target_name(base, node).as_deref() == Some("alias");
        if explicit.is_none() && !is_alias {
            return;
        }
        let module = self.enclosing_module(node);
        for target in helpers::directive_modules(base, node) {
            let full = self.expand_module(module, &target);
            let local = explicit
                .clone()
                .or_else(|| target.rsplit('.').next().map(str::to_string));
            if let Some(local) = local {
                self.aliases
                    .entry(module.map(|m| m.id.clone()))
                    .or_default()
                    .insert(local, full);
            }
        }
    }

    /// A nested `defmodule Child` is reachable as `Child` inside its parent.
    fn record_nested_module(&mut self, base: &BaseExtractor, node: &Node) {
        let Some(parent) = node
            .parent()
            .and_then(|parent| self.enclosing_module(&parent))
            .filter(|parent| {
                (parent.start_byte as usize) < node.start_byte()
                    && (parent.end_byte as usize) >= node.end_byte()
            })
        else {
            return;
        };
        let Some(declared) = helpers::extract_module_name(base, node) else {
            return;
        };
        let Some(first) = declared.split('.').next().map(str::to_string) else {
            return;
        };
        let full = format!("{}.{first}", parent.name);
        self.aliases
            .entry(Some(parent.id.clone()))
            .or_default()
            .entry(first)
            .or_insert(full);
    }

    /// Expand `__MODULE__` and the leading alias segment of a module reference
    /// to the full module name, searching the enclosing modules outward.
    fn expand_module(&self, module: Option<&'a Symbol>, reference: &str) -> String {
        let (head, rest) = match reference.split_once('.') {
            Some((head, rest)) => (head, Some(rest)),
            None => (reference, None),
        };
        let join = |full: &str| match rest {
            Some(rest) => format!("{full}.{rest}"),
            None => full.to_string(),
        };
        if head == "__MODULE__" {
            return module.map_or_else(|| reference.to_string(), |m| join(&m.name));
        }
        let mut current = module;
        loop {
            let key = current.map(|m| m.id.clone());
            if let Some(full) = self.aliases.get(&key).and_then(|map| map.get(head)) {
                return join(full);
            }
            match current {
                Some(m) => current = self.module_of(m),
                None => return reference.to_string(),
            }
        }
    }

    /// Every definition of `name` in `module` that accepts `arity` arguments.
    fn definitions(&self, module: Option<&Symbol>, name: &str, arity: usize) -> Vec<&'a Symbol> {
        self.callables
            .get(&(module.map(|m| m.id.as_str()), name))
            .into_iter()
            .flatten()
            .filter(|c| (c.min_arity..=c.max_arity).contains(&arity))
            .map(|c| c.symbol)
            .collect()
    }

    fn resolve(&self, module: Option<&Symbol>, name: &str, arity: usize) -> Option<&'a Symbol> {
        let candidates = self.callables.get(&(module.map(|m| m.id.as_str()), name))?;
        candidates
            .iter()
            .find(|c| (c.min_arity..=c.max_arity).contains(&arity))
            .or_else(|| candidates.first())
            .map(|c| c.symbol)
    }
}

fn call_arity(base: &BaseExtractor, node: &Node) -> usize {
    if let Some(arity) = helpers::capture_arity(base, node) {
        return arity;
    }
    let explicit = helpers::find_child_by_type(node, "arguments")
        .map(|args| args.named_child_count())
        .unwrap_or(0);
    let piped = node.parent().is_some_and(|parent| {
        parent.kind() == "binary_operator"
            && parent.child_by_field_name("right").map(|r| r.id()) == Some(node.id())
            && parent
                .child_by_field_name("operator")
                .is_some_and(|op| op.kind() == "|>")
    });
    explicit + usize::from(piped)
}

fn walk_for_relationships(
    extractor: &mut super::ElixirExtractor,
    node: Node,
    symbols: &[Symbol],
    scope: &CallScope<'_>,
    relationships: &mut Vec<Relationship>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    match node.kind() {
        "call" if !helpers::is_definition_head(&extractor.base, &node) => {
            if let Some(target_name) = helpers::extract_call_target_name(&extractor.base, &node) {
                match target_name.as_str() {
                    "use" => {
                        extract_use_relationship(extractor, &node, scope, relationships);
                    }
                    "defimpl" => {
                        extract_impl_relationship(extractor, &node, symbols, scope, relationships);
                    }
                    "defdelegate" => {
                        extract_delegate_relationship(
                            extractor,
                            &node,
                            symbols,
                            scope,
                            relationships,
                        );
                    }
                    "setup" | "setup_all" => {
                        for (atom, callback) in helpers::setup_callbacks(&extractor.base, &node) {
                            emit_call(
                                extractor,
                                &atom,
                                scope,
                                &callback,
                                1,
                                unresolved_elixir_alias(callback.clone()),
                                relationships,
                            );
                        }
                    }
                    "defmodule" | "def" | "defp" | "defmacro" | "defmacrop" | "defprotocol"
                    | "defstruct" | "defguard" | "defguardp" | "defexception"
                    | "defoverridable" | "import" | "alias" | "require" | "test" | "describe"
                    | "property" | "doctest" => {}
                    name if helpers::is_special_form(name) => {}
                    _ => {
                        extract_call_relationship(extractor, &node, scope, relationships);
                    }
                }
            }
        }
        "binary_operator" => record_initializer_type(extractor, node, scope),
        "unary_operator" if is_module_attribute(&extractor.base, &node) => {
            extract_behaviour_relationship(extractor, &node, scope, relationships);
            return;
        }
        "identifier" if helpers::is_local_capture(&extractor.base, &node) => {
            let name = extractor.base.get_node_text(&node);
            let arity = helpers::capture_arity(&extractor.base, &node).unwrap_or(0);
            emit_call(
                extractor,
                &node,
                scope,
                &name,
                arity,
                unresolved_elixir_alias(name.clone()),
                relationships,
            );
        }
        _ => {}
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_for_relationships(extractor, child, symbols, scope, relationships, child_depth);
    }
}

/// Give the local bound by `x = call()` the `@spec` return type of the
/// same-file definition the call reaches, and the local bound by
/// `{:ok, x} = call()` the `T` of its `{:ok, T}` return.
fn record_initializer_type(
    extractor: &mut super::ElixirExtractor,
    node: Node,
    scope: &CallScope<'_>,
) {
    let Some((binder, binds_ok_payload)) = type_facts::match_binder(&extractor.base, node) else {
        return;
    };
    let Some(local) = scope
        .variables_by_start
        .get(&(binder.start_byte() as u32))
        .filter(|local| local.name == extractor.base.get_node_text(&binder))
    else {
        return;
    };
    let Some(type_name) = node
        .child_by_field_name("right")
        .and_then(|value| initializer_type(extractor, value, scope, binds_ok_payload))
    else {
        return;
    };
    type_facts::record_type_fact(&mut extractor.base, &local.id, &type_name, true);
}

/// The return type every same-file definition a call can reach agrees on: a
/// local call in the enclosing module, or `Alias.fun()` / `__MODULE__.fun()`
/// on a module defined in this file. A piped call counts the piped argument.
/// A macro expands at compile time, so a macro call records nothing.
fn initializer_type(
    extractor: &super::ElixirExtractor,
    value: Node,
    scope: &CallScope<'_>,
    ok_payload: bool,
) -> Option<String> {
    let call = if value.kind() == "binary_operator"
        && value.child_by_field_name("operator")?.kind() == "|>"
    {
        value.child_by_field_name("right")?
    } else {
        value
    };
    if call.kind() != "call" {
        return None;
    }
    let base = &extractor.base;
    let target = call.child_by_field_name("target")?;
    let enclosing = scope.enclosing_module(&call);
    let (module, name) = match target.kind() {
        "identifier" => (enclosing, base.get_node_text(&target)),
        "dot" => {
            let left = target.child_by_field_name("left")?;
            let reference = base.get_node_text(&left);
            let module = scope.expand_module(enclosing, &reference);
            let module = *scope.modules_by_name.get(module.as_str())?;
            (
                Some(module),
                base.get_node_text(&target.child_by_field_name("right")?),
            )
        }
        _ => return None,
    };
    let mut types = scope
        .definitions(module, &name, call_arity(base, &call))
        .into_iter()
        .map(|definition| {
            if definition.metadata.as_ref().and_then(|m| m.get("macro"))
                == Some(&serde_json::Value::Bool(true))
            {
                return None;
            }
            let spec = extractor.spec_returns.get(&definition.id)?;
            if ok_payload {
                spec.ok_payload.clone()
            } else {
                spec.base.clone()
            }
        });
    let first = types.next()??;
    types
        .all(|other| other.as_ref() == Some(&first))
        .then_some(first)
}

fn is_module_attribute(base: &BaseExtractor, node: &Node) -> bool {
    node.child_by_field_name("operator")
        .is_some_and(|operator| base.get_node_text(&operator) == "@")
}

fn module_target(scope: &CallScope<'_>, node: &Node, name: &str) -> UnresolvedTarget {
    let full = scope.expand_module(scope.enclosing_module(node), name);
    UnresolvedTarget {
        display_name: full.clone(),
        terminal_name: full,
        receiver: None,
        namespace_path: Vec::new(),
        import_context: None,
    }
}

fn extract_use_relationship(
    extractor: &mut super::ElixirExtractor,
    node: &Node,
    scope: &CallScope<'_>,
    relationships: &mut Vec<Relationship>,
) {
    let Some(target) = extract_alias_argument(&extractor.base, node) else {
        return;
    };
    let Some(from_symbol) = scope.enclosing_module(node) else {
        return;
    };
    let target = module_target(scope, node, &target);

    if let Some(to_symbol) = scope.modules_by_name.get(target.terminal_name.as_str()) {
        relationships.push(Relationship {
            id: format!(
                "{}_{}_Uses_{}",
                from_symbol.id,
                to_symbol.id,
                node.start_position().row
            ),
            from_symbol_id: from_symbol.id.clone(),
            to_symbol_id: to_symbol.id.clone(),
            kind: RelationshipKind::Uses,
            file_path: extractor.base.file_path.clone(),
            line_number: (node.start_position().row + 1) as u32,
            span: Some(crate::base::NormalizedSpan::from_node(node)),
            reference_site_is_exact: false,
            confidence: 1.0,
            metadata: None,
        });
    } else {
        let pending = extractor.base.create_pending_relationship(
            from_symbol.id.clone(),
            target,
            RelationshipKind::Uses,
            node,
            Some(from_symbol.id.clone()),
            Some(0.8),
        );
        extractor.base.add_structured_pending_relationship(pending);
    }
}

/// A protocol implementation implements its protocol: the local protocol
/// symbol, or a pending row for a protocol defined elsewhere.
fn extract_impl_relationship(
    extractor: &mut super::ElixirExtractor,
    node: &Node,
    symbols: &[Symbol],
    scope: &CallScope<'_>,
    relationships: &mut Vec<Relationship>,
) {
    let Some(protocol_name) = helpers::extract_impl_protocol_name(&extractor.base, node) else {
        return;
    };
    let Some(from) = symbols
        .iter()
        .find(|s| s.kind == SymbolKind::Class && s.start_byte as usize == node.start_byte())
    else {
        return;
    };
    let target = module_target(scope, node, &protocol_name);
    match scope
        .modules_by_name
        .get(target.terminal_name.as_str())
        .filter(|to| to.kind == SymbolKind::Interface)
    {
        Some(to) => relationships.push(Relationship {
            id: format!(
                "{}_{}_Implements_{}",
                from.id,
                to.id,
                node.start_position().row
            ),
            from_symbol_id: from.id.clone(),
            to_symbol_id: to.id.clone(),
            kind: RelationshipKind::Implements,
            file_path: extractor.base.file_path.clone(),
            line_number: (node.start_position().row + 1) as u32,
            span: Some(crate::base::NormalizedSpan::from_node(node)),
            reference_site_is_exact: false,
            confidence: 1.0,
            metadata: None,
        }),
        None => {
            let pending = extractor.base.create_pending_relationship(
                from.id.clone(),
                target,
                RelationshipKind::Implements,
                node,
                Some(from.id.clone()),
                Some(0.9),
            );
            extractor.base.add_structured_pending_relationship(pending);
        }
    }
}

/// `defdelegate name(args), to: Mod, as: :other` calls `Mod.other` with the
/// same arity.
fn extract_delegate_relationship(
    extractor: &mut super::ElixirExtractor,
    node: &Node,
    symbols: &[Symbol],
    scope: &CallScope<'_>,
    relationships: &mut Vec<Relationship>,
) {
    let Some(delegate) = symbols
        .iter()
        .find(|s| s.kind == SymbolKind::Delegate && s.start_byte as usize == node.start_byte())
    else {
        return;
    };
    let Some(target_module) = helpers::extract_keyword_value(&extractor.base, node, "to") else {
        return;
    };
    let function = helpers::extract_keyword_value(&extractor.base, node, "as")
        .map(|renamed| renamed.trim_start_matches(':').to_string())
        .unwrap_or_else(|| delegate.name.clone());
    let arity = helpers::definition_arity(&extractor.base, node).1;
    let module = scope.expand_module(scope.enclosing_module(node), &target_module);
    let callee = scope
        .modules_by_name
        .get(module.as_str())
        .and_then(|m| scope.resolve(Some(m), &function, arity));
    let unresolved = UnresolvedTarget {
        display_name: format!("{module}.{function}"),
        terminal_name: function,
        receiver: None,
        namespace_path: vec![module],
        import_context: None,
    };
    push_call(extractor, node, delegate, callee, unresolved, relationships);
}

fn extract_behaviour_relationship(
    extractor: &mut super::ElixirExtractor,
    node: &Node,
    scope: &CallScope<'_>,
    relationships: &mut Vec<Relationship>,
) {
    let Some(operand) = node.child_by_field_name("operand") else {
        return;
    };
    if operand.kind() != "call" {
        return;
    }
    let Some(target) = operand.child_by_field_name("target") else {
        return;
    };
    let attr_name = extractor.base.get_node_text(&target);
    if attr_name != "behaviour" && attr_name != "behavior" {
        return;
    }

    let Some(behaviour_name) = extract_alias_argument(&extractor.base, &operand) else {
        return;
    };
    let Some(from) = scope.enclosing_module(node) else {
        return;
    };
    let target = module_target(scope, node, &behaviour_name);
    if let Some(to) = scope.modules_by_name.get(target.terminal_name.as_str()) {
        relationships.push(Relationship {
            id: format!(
                "{}_{}_Implements_{}",
                from.id,
                to.id,
                node.start_position().row
            ),
            from_symbol_id: from.id.clone(),
            to_symbol_id: to.id.clone(),
            kind: RelationshipKind::Implements,
            file_path: extractor.base.file_path.clone(),
            line_number: (node.start_position().row + 1) as u32,
            span: Some(crate::base::NormalizedSpan::from_node(node)),
            reference_site_is_exact: false,
            confidence: 1.0,
            metadata: None,
        });
    } else {
        let pending = extractor.base.create_pending_relationship(
            from.id.clone(),
            target,
            RelationshipKind::Implements,
            node,
            Some(from.id.clone()),
            Some(0.9),
        );
        extractor.base.add_structured_pending_relationship(pending);
    }
}

fn unresolved_elixir_alias(name: String) -> UnresolvedTarget {
    let parts: Vec<_> = name
        .split('.')
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect();

    let terminal_name = parts.last().cloned().unwrap_or_else(|| name.clone());
    let namespace_path = parts
        .get(..parts.len().saturating_sub(1))
        .unwrap_or(&[])
        .to_vec();

    UnresolvedTarget {
        display_name: name,
        terminal_name,
        receiver: None,
        namespace_path,
        import_context: None,
    }
}

fn extract_alias_argument(base: &BaseExtractor, node: &Node) -> Option<String> {
    let args = helpers::find_child_by_type(node, "arguments")?;
    find_alias_like_node(base, &args)
}

fn find_alias_like_node(base: &BaseExtractor, node: &Node) -> Option<String> {
    find_alias_like_node_at_depth(base, node, 0)
}

fn find_alias_like_node_at_depth(base: &BaseExtractor, node: &Node, depth: u32) -> Option<String> {
    if !should_visit_tree_depth(depth) {
        return None;
    }

    let child_depth = child_tree_depth(depth)?;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "alias" | "dot" => return Some(base.get_node_text(&child)),
            _ => {
                if let Some(name) = find_alias_like_node_at_depth(base, &child, child_depth) {
                    return Some(name);
                }
            }
        }
    }
    None
}

/// Emit a call edge from the enclosing callable. A local call resolves inside
/// the enclosing module; `Alias.fun()` resolves through the module's aliases
/// and, when that module is defined in this file, to its function. Anything
/// else is pending, with the full module name as the one namespace segment so
/// it joins to the module symbol's name.
fn extract_call_relationship(
    extractor: &mut super::ElixirExtractor,
    node: &Node,
    scope: &CallScope<'_>,
    relationships: &mut Vec<Relationship>,
) {
    let Some(caller) = scope.callers.find(*node) else {
        return;
    };
    let Some(target) = node.child_by_field_name("target") else {
        return;
    };
    let arity = call_arity(&extractor.base, node);
    let enclosing = scope.enclosing_module(node);

    let (callee, unresolved) = match target.kind() {
        "identifier" => {
            let name = extractor.base.get_node_text(&target);
            (
                scope.resolve(enclosing, &name, arity),
                unresolved_elixir_alias(name),
            )
        }
        "dot" => {
            let (Some(left), Some(right)) = (
                target.child_by_field_name("left"),
                target.child_by_field_name("right"),
            ) else {
                return;
            };
            let module_text = extractor.base.get_node_text(&left);
            let function = extractor.base.get_node_text(&right);
            let is_module =
                left.kind() == "alias" || module_text.split('.').next() == Some("__MODULE__");
            let has_arguments = helpers::find_child_by_type(node, "arguments").is_some();
            if !is_module
                && !has_arguments
                && helpers::capture_arity(&extractor.base, node).is_none()
            {
                return;
            }
            if left.kind() == "atom" {
                let erlang_module = module_text.trim_start_matches(':').to_string();
                (
                    None,
                    UnresolvedTarget {
                        display_name: format!("{module_text}.{function}"),
                        terminal_name: function,
                        receiver: None,
                        namespace_path: vec![erlang_module],
                        import_context: None,
                    },
                )
            } else if is_module {
                let module = scope.expand_module(enclosing, &module_text);
                let callee = scope
                    .modules_by_name
                    .get(module.as_str())
                    .and_then(|m| scope.resolve(Some(m), &function, arity));
                let unresolved = UnresolvedTarget {
                    display_name: format!("{module}.{function}"),
                    terminal_name: function,
                    receiver: None,
                    namespace_path: vec![module],
                    import_context: None,
                };
                (callee, unresolved)
            } else {
                (
                    None,
                    UnresolvedTarget {
                        display_name: format!("{module_text}.{function}"),
                        terminal_name: function,
                        receiver: Some(module_text),
                        namespace_path: Vec::new(),
                        import_context: None,
                    },
                )
            }
        }
        _ => return,
    };
    push_call(extractor, node, caller, callee, unresolved, relationships);
}

/// Emit a call from the callable around `site` to `name/arity`: a local edge
/// when the enclosing module defines it, otherwise a pending row.
#[allow(clippy::too_many_arguments)]
fn emit_call(
    extractor: &mut super::ElixirExtractor,
    site: &Node,
    scope: &CallScope<'_>,
    name: &str,
    arity: usize,
    unresolved: UnresolvedTarget,
    relationships: &mut Vec<Relationship>,
) {
    let Some(caller) = scope.callers.find(*site) else {
        return;
    };
    let callee = scope.resolve(scope.enclosing_module(site), name, arity);
    push_call(extractor, site, caller, callee, unresolved, relationships);
}

fn push_call(
    extractor: &mut super::ElixirExtractor,
    node: &Node,
    caller: &Symbol,
    callee: Option<&Symbol>,
    unresolved: UnresolvedTarget,
    relationships: &mut Vec<Relationship>,
) {
    if let Some(callee) = callee {
        relationships.push(Relationship {
            id: format!(
                "{}_{}_Calls_{}",
                caller.id,
                callee.id,
                node.start_position().row
            ),
            from_symbol_id: caller.id.clone(),
            to_symbol_id: callee.id.clone(),
            kind: RelationshipKind::Calls,
            file_path: extractor.base.file_path.clone(),
            line_number: (node.start_position().row + 1) as u32,
            span: Some(crate::base::NormalizedSpan::from_node(node)),
            reference_site_is_exact: false,
            confidence: 0.9,
            metadata: None,
        });
    } else {
        let pending = extractor.base.create_pending_relationship(
            caller.id.clone(),
            unresolved,
            RelationshipKind::Calls,
            node,
            Some(caller.id.clone()),
            Some(0.7),
        );
        extractor.base.add_structured_pending_relationship(pending);
    }
}
