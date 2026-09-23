//! Relationship extraction for Erlang.
//!
//! Erlang's edges split cleanly by whether the target can live in the same
//! file. A `.erl` file declares exactly one module, so only an unqualified call
//! to a function defined here is resolvable in-file; every other edge names
//! another module or another file and is emitted as a structured pending row
//! carrying the module/path context a resolver needs.
//!
//! | Source form | Edge |
//! | --- | --- |
//! | `helper(X)` or `?MODULE:helper(X)` with `helper/1` defined here | resolved `Calls` |
//! | `fun helper/1` / `fun ?MODULE:helper/1` with `helper/1` defined here | resolved `References` |
//! | `fun lists:reverse/1` | pending `References`, namespace `["lists"]` |
//! | `?DOUBLE(X)` with `-define(DOUBLE(X), ...)` here | resolved `Calls` to the macro |
//! | `ledger:record(X)` | pending `Calls`, namespace `["ledger"]` |
//! | `reverse(X)` under `-import(lists, [reverse/1])` | pending `Calls`, namespace `["lists"]`, import context `import` |
//! | `-behaviour(gen_server)` | pending `Implements` from the module symbol |
//! | `-include("x.hrl")` / `-include_lib("app/include/x.hrl")` | pending `Imports` from the module symbol |
//! | `-import(lists, [...])` | pending `Imports` from the module symbol |
//!
//! An unqualified call that resolves to neither a same-file function nor an
//! `-import` is an auto-imported BIF (`length/1`, `self/0`) or a function from
//! an included header. Emitting a pending edge for it would ask a resolver to
//! bind `length` to whatever workspace function happens to share the name, so
//! it emits nothing. Type signatures spell type names with the same `call` node
//! a real call uses, so the walk starts from the two executable declaration
//! kinds — `fun_decl` and `pp_define` — exactly as the identifier layer does.

use std::collections::HashMap;

use tree_sitter::Node;

use super::definition_forms;
use super::helpers::{
    NameArity, arg_count, child_named_kinds, find_child_by_type, named_children, unquote_atom,
};
use super::identifiers::{ImportedFunctions, imported_functions};
use super::{Bounded, ErlangExtractor};
use crate::base::{Relationship, RelationshipKind, Symbol, SymbolKind, UnresolvedTarget};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

const LOCAL_CALL_CONFIDENCE: f32 = 0.9;
const REMOTE_CALL_CONFIDENCE: f32 = 0.7;
const ATTRIBUTE_TARGET_CONFIDENCE: f32 = 0.9;

/// Function symbols keyed by Erlang's `(name, arity)` identity, so a call to
/// `helper/2` never binds to `helper/1`.
type FunctionIndex<'a> = HashMap<NameArity, &'a Symbol>;

pub(super) fn extract_relationships(
    extractor: &mut ErlangExtractor,
    declarations: &[Bounded],
    symbols: &[Symbol],
) -> Vec<Relationship> {
    let nodes: Vec<Node> = declarations.iter().map(|d| d.node).collect();
    let imports = imported_functions(extractor, &nodes);
    let containing_symbols = extractor.base.containing_symbol_index(symbols);
    let module = symbols
        .iter()
        .find(|symbol| symbol.kind == SymbolKind::Module);
    let module_id = module.map(|symbol| symbol.id.clone());
    let targets = CallTargets {
        functions: function_index(symbols),
        macros: macro_index(symbols),
        imports,
        module_name: module.map(|symbol| symbol.name.clone()),
    };

    let mut relationships = Vec::new();
    for &Bounded {
        node: declaration,
        end,
    } in declarations
    {
        let declaration = &declaration;
        match declaration.kind() {
            "behaviour_attribute" => {
                emit_behaviour(extractor, declaration, module_id.as_deref());
            }
            "pp_include" => emit_include(extractor, declaration, module_id.as_deref(), "include"),
            "pp_include_lib" => {
                emit_include(extractor, declaration, module_id.as_deref(), "include_lib")
            }
            "import_attribute" | "import_record_attribute" => {
                emit_import(extractor, declaration, module_id.as_deref())
            }
            "record_decl" => {
                let scope = containing_symbols
                    .find(*declaration)
                    .map(|symbol| symbol.id.clone());
                let mut walk = CallWalk {
                    scope: scope.as_deref(),
                    end,
                    targets: &targets,
                    relationships: &mut relationships,
                };
                for field in child_named_kinds(declaration, "record_field") {
                    if let Some(default) = field.child_by_field_name("expr") {
                        walk.visit(extractor, default, 0);
                    }
                }
            }
            "fun_decl" => {
                let scope = clause_scope(extractor, declaration, &targets.functions);
                let mut walk = CallWalk {
                    scope: scope.as_deref(),
                    end,
                    targets: &targets,
                    relationships: &mut relationships,
                };
                walk.visit(extractor, *declaration, 0);
            }
            "pp_define" => {
                let scope = containing_symbols
                    .find(*declaration)
                    .map(|symbol| symbol.id.clone());
                let mut walk = CallWalk {
                    scope: scope.as_deref(),
                    end,
                    targets: &targets,
                    relationships: &mut relationships,
                };
                for child in named_children(declaration) {
                    if child.kind() == "macro_lhs" {
                        continue;
                    }
                    walk.visit(extractor, child, 0);
                }
            }
            _ => {}
        }
    }

    relationships
}

/// Same-file targets a call can bind to, plus the context that names them.
struct CallTargets<'a> {
    functions: FunctionIndex<'a>,
    macros: FunctionIndex<'a>,
    imports: ImportedFunctions,
    /// The `-module` name, which `?MODULE` expands to.
    module_name: Option<String>,
}

fn function_index(symbols: &[Symbol]) -> FunctionIndex<'_> {
    symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Function)
        .filter_map(|symbol| {
            Some((
                (symbol.name.clone(), symbol_arity(symbol, "arity")?),
                symbol,
            ))
        })
        .collect()
}

/// `-define` macros that take arguments, keyed by name and macro arity.
fn macro_index(symbols: &[Symbol]) -> FunctionIndex<'_> {
    symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Constant)
        .filter_map(|symbol| {
            Some((
                (symbol.name.clone(), symbol_arity(symbol, "macro_arity")?),
                symbol,
            ))
        })
        .collect()
}

fn symbol_arity(symbol: &Symbol, key: &str) -> Option<u32> {
    symbol
        .metadata
        .as_ref()?
        .get(key)?
        .as_u64()
        .map(|arity| arity as u32)
}

/// A multi-clause function is a run of sibling `fun_decl` nodes but a single
/// symbol, so every clause resolves its scope through the shared name/arity
/// identity rather than through span containment.
fn clause_scope(
    extractor: &ErlangExtractor,
    declaration: &Node,
    functions: &FunctionIndex,
) -> Option<String> {
    let clause = definition_forms::function_clause(extractor, declaration)?;
    functions
        .get(&clause.identity)
        .map(|symbol| symbol.id.clone())
}

struct CallWalk<'w, 'a> {
    scope: Option<&'w str>,
    /// Bytes from here on belong to a later, recovered declaration.
    end: usize,
    targets: &'w CallTargets<'a>,
    relationships: &'w mut Vec<Relationship>,
}

impl CallWalk<'_, '_> {
    fn visit(&mut self, extractor: &mut ErlangExtractor, node: Node, depth: u32) {
        if !should_visit_tree_depth(depth) || node.start_byte() >= self.end {
            return;
        }

        match node.kind() {
            "remote" => self.remote_call(extractor, node),
            "call" if node.parent().map(|parent| parent.kind()) != Some("remote") => {
                self.local_call(extractor, node)
            }
            "internal_fun" => self.local_fun_reference(extractor, node),
            "external_fun" => self.remote_fun_reference(extractor, node),
            "macro_call_expr" => self.macro_call(extractor, node),
            _ => {}
        }
        for (function, module, arity) in
            super::mfa::targets(extractor, node, self.targets.module_name.as_deref())
        {
            let name = unquote_atom(&extractor.base.get_node_text(&function));
            self.qualified_target(
                extractor,
                RelationshipKind::Calls,
                module,
                name,
                arity,
                &function,
            );
        }

        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        for child in named_children(&node) {
            self.visit(extractor, child, child_depth);
        }
    }

    fn resolved(
        &mut self,
        extractor: &ErlangExtractor,
        kind: RelationshipKind,
        target: &Symbol,
        anchor: &Node,
    ) {
        let Some(scope) = self.scope else {
            return;
        };
        self.relationships
            .push(extractor.base.create_relationship_at_target(
                scope.to_string(),
                target.id.clone(),
                kind,
                anchor,
                Some(LOCAL_CALL_CONFIDENCE),
                None,
            ));
    }

    fn pending(
        &self,
        extractor: &mut ErlangExtractor,
        kind: RelationshipKind,
        target: UnresolvedTarget,
        arity: u32,
        anchor: &Node,
    ) {
        let Some(scope) = self.scope else {
            return;
        };
        let pending = extractor
            .base
            .create_pending_relationship_at_target(
                scope.to_string(),
                target,
                kind,
                anchor,
                Some(scope.to_string()),
                Some(REMOTE_CALL_CONFIDENCE),
            )
            .with_target_arity(Some(arity));
        extractor.base.add_structured_pending_relationship(pending);
    }

    /// `m:f(...)`. A call qualified by `?MODULE` or by this file's own module
    /// name is a local call and resolves like one; anything else is pending.
    fn remote_call(&mut self, extractor: &mut ErlangExtractor, node: Node) {
        let Some(module) = find_child_by_type(&node, "remote_module")
            .and_then(|wrapper| module_name(extractor, self.targets, &wrapper))
        else {
            return;
        };
        let Some(call) = find_child_by_type(&node, "call") else {
            return;
        };
        let Some(callee) = find_child_by_type(&call, "atom") else {
            return;
        };
        let name = unquote_atom(&extractor.base.get_node_text(&callee));
        let arity = call_arity(&call);
        self.qualified_target(
            extractor,
            RelationshipKind::Calls,
            module,
            name,
            arity,
            &callee,
        );
    }

    fn qualified_target(
        &mut self,
        extractor: &mut ErlangExtractor,
        kind: RelationshipKind,
        module: String,
        name: String,
        arity: u32,
        anchor: &Node,
    ) {
        if self.targets.module_name.as_deref() == Some(module.as_str()) {
            if let Some(target) = self.targets.functions.get(&(name, arity)) {
                self.resolved(extractor, kind, target, anchor);
            }
            return;
        }
        let target = UnresolvedTarget {
            display_name: format!("{module}:{name}"),
            terminal_name: name,
            receiver: None,
            namespace_path: vec![module],
            import_context: None,
        };
        self.pending(extractor, kind, target, arity, anchor);
    }

    fn local_call(&mut self, extractor: &mut ErlangExtractor, node: Node) {
        let Some(callee) = find_child_by_type(&node, "atom") else {
            return;
        };
        let name = unquote_atom(&extractor.base.get_node_text(&callee));
        self.local_target(
            extractor,
            RelationshipKind::Calls,
            name,
            call_arity(&node),
            &callee,
        );
    }

    fn local_target(
        &mut self,
        extractor: &mut ErlangExtractor,
        kind: RelationshipKind,
        name: String,
        arity: u32,
        anchor: &Node,
    ) {
        let identity = (name.clone(), arity);
        if let Some(target) = self.targets.functions.get(&identity) {
            self.resolved(extractor, kind, target, anchor);
            return;
        }
        let Some(module) = self.targets.imports.get(&identity).cloned() else {
            return;
        };
        let target = UnresolvedTarget {
            display_name: name.clone(),
            terminal_name: name,
            receiver: None,
            namespace_path: vec![module],
            import_context: Some("import".to_string()),
        };
        self.pending(extractor, kind, target, arity, anchor);
    }

    /// `fun f/N` names a function as a value rather than calling it, so it
    /// records a `References` edge to that function.
    fn local_fun_reference(&mut self, extractor: &mut ErlangExtractor, node: Node) {
        let Some((callee, arity)) = fun_reference_parts(extractor, &node) else {
            return;
        };
        let name = unquote_atom(&extractor.base.get_node_text(&callee));
        self.local_target(
            extractor,
            RelationshipKind::References,
            name,
            arity,
            &callee,
        );
    }

    fn remote_fun_reference(&mut self, extractor: &mut ErlangExtractor, node: Node) {
        let Some(module) = find_child_by_type(&node, "module")
            .and_then(|wrapper| module_name(extractor, self.targets, &wrapper))
        else {
            return;
        };
        let Some((callee, arity)) = fun_reference_parts(extractor, &node) else {
            return;
        };
        let name = unquote_atom(&extractor.base.get_node_text(&callee));
        self.qualified_target(
            extractor,
            RelationshipKind::References,
            module,
            name,
            arity,
            &callee,
        );
    }

    /// `?NAME(Args)` expands a same-file `-define(NAME(...), ...)`; the edge
    /// to that macro keeps function -> macro -> callee chains connected.
    fn macro_call(&mut self, extractor: &mut ErlangExtractor, node: Node) {
        let Some(args) = find_child_by_type(&node, "macro_call_args") else {
            return;
        };
        let Some(name_node) = node.child_by_field_name("name") else {
            return;
        };
        let name = unquote_atom(&extractor.base.get_node_text(&name_node));
        let arity = named_children(&args).len() as u32;
        if let Some(target) = self.targets.macros.get(&(name, arity)) {
            self.resolved(extractor, RelationshipKind::Calls, target, &name_node);
        }
    }
}

fn call_arity(call: &Node) -> u32 {
    find_child_by_type(call, "expr_args")
        .map(|args| arg_count(&args))
        .unwrap_or(0)
}

/// The module a `remote_module` / `module` wrapper names: an atom, or
/// `?MODULE`, which always expands to this file's `-module` name. A variable
/// module names nothing.
fn module_name(
    extractor: &ErlangExtractor,
    targets: &CallTargets,
    wrapper: &Node,
) -> Option<String> {
    let module = wrapper
        .child_by_field_name("module")
        .or_else(|| wrapper.child_by_field_name("name"))?;
    match module.kind() {
        "atom" => Some(unquote_atom(&extractor.base.get_node_text(&module))),
        "macro_call_expr" if is_module_macro(extractor, &module) => targets.module_name.clone(),
        _ => None,
    }
}

pub(super) fn is_module_macro(extractor: &ErlangExtractor, node: &Node) -> bool {
    find_child_by_type(node, "macro_call_args").is_none()
        && node
            .child_by_field_name("name")
            .is_some_and(|name| extractor.base.get_node_text(&name) == "MODULE")
}

fn fun_reference_parts<'tree>(
    extractor: &ErlangExtractor,
    node: &Node<'tree>,
) -> Option<(Node<'tree>, u32)> {
    let callee = node
        .child_by_field_name("fun")
        .filter(|fun| fun.kind() == "atom")?;
    let value = node
        .child_by_field_name("arity")?
        .child_by_field_name("value")?;
    let arity = extractor.base.get_node_text(&value).trim().parse().ok()?;
    Some((callee, arity))
}

/// A `.erl` file declares one module, so a `-behaviour` target is always in
/// another file: matching the behaviour name against a same-file function or
/// type symbol would invent an edge the source never declared.
fn emit_behaviour(extractor: &mut ErlangExtractor, node: &Node, module_id: Option<&str>) {
    let Some(module_id) = module_id else {
        return;
    };
    let Some(atom) = find_child_by_type(node, "atom") else {
        return;
    };
    let name = unquote_atom(&extractor.base.get_node_text(&atom));

    let pending = extractor.base.create_pending_relationship_at_target(
        module_id.to_string(),
        UnresolvedTarget::simple(name),
        RelationshipKind::Implements,
        &atom,
        Some(module_id.to_string()),
        Some(ATTRIBUTE_TARGET_CONFIDENCE),
    );
    extractor.base.add_structured_pending_relationship(pending);
}

/// `-include_lib` resolves through an application's lib directory while
/// `-include` resolves against the include path, so the attribute name is
/// recorded as the target's import context.
fn emit_include(
    extractor: &mut ErlangExtractor,
    node: &Node,
    module_id: Option<&str>,
    attribute: &str,
) {
    let Some(module_id) = module_id else {
        return;
    };
    let Some(string) = find_child_by_type(node, "string") else {
        return;
    };
    let path = unquote_string(&extractor.base.get_node_text(&string));
    if path.is_empty() {
        return;
    }

    let mut segments: Vec<String> = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
        .collect();
    let Some(terminal_name) = segments.pop() else {
        return;
    };

    let target = UnresolvedTarget {
        display_name: path,
        terminal_name,
        receiver: None,
        namespace_path: segments,
        import_context: Some(attribute.to_string()),
    };
    let pending = extractor.base.create_pending_relationship_at_target(
        module_id.to_string(),
        target,
        RelationshipKind::Imports,
        &string,
        Some(module_id.to_string()),
        Some(ATTRIBUTE_TARGET_CONFIDENCE),
    );
    extractor.base.add_structured_pending_relationship(pending);
}

fn emit_import(extractor: &mut ErlangExtractor, node: &Node, module_id: Option<&str>) {
    let Some(module_id) = module_id else {
        return;
    };
    let Some(atom) = find_child_by_type(node, "atom") else {
        return;
    };
    let name = unquote_atom(&extractor.base.get_node_text(&atom));

    let target = UnresolvedTarget {
        display_name: name.clone(),
        terminal_name: name,
        receiver: None,
        namespace_path: Vec::new(),
        import_context: Some("import".to_string()),
    };
    let pending = extractor.base.create_pending_relationship_at_target(
        module_id.to_string(),
        target,
        RelationshipKind::Imports,
        &atom,
        Some(module_id.to_string()),
        Some(ATTRIBUTE_TARGET_CONFIDENCE),
    );
    extractor.base.add_structured_pending_relationship(pending);
}

fn unquote_string(text: &str) -> String {
    text.trim().trim_matches('"').to_string()
}
