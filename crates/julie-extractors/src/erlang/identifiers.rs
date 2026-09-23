//! Identifier extraction for Erlang: call sites, fun references, macro usage,
//! and record/field references.
//!
//! Node kinds come from `tree-sitter-erlang` 0.20.0 parse trees. Executable code
//! lives in `fun_decl` clauses and in `pp_define` macro bodies; every other
//! top-level form is a declaration. Type signatures matter here: `-spec`,
//! `-type`, `-opaque`, `-callback`, and record field types spell `integer()`
//! with the very same `call` node a real call uses, so the walk starts from the
//! two executable declaration kinds. A separate type-only walk handles declared
//! specifications and aliases without manufacturing executable calls.
//!
//! Kind assignment:
//! - a call — local, remote, imported, or a parameterized macro — is `Call`
//! - a module qualifier (`lists:`, `fun lists:reverse/1`) is `TypeUsage`,
//!   mirroring how the Elixir layer records a remote receiver as a separate row
//! - a fun reference (`fun g/1`) and a bare macro read (`?LIMIT`) name a value
//!   rather than invoking it, so they are `VariableRef` and stay distinguishable
//!   from the `Call` row a real invocation of the same name produces
//! - a record name is `TypeUsage` and a record field is `MemberAccess`
//!
//! The same walk captures string-literal call arguments as `Literal` rows,
//! carried by the verbatim callee (`io:format` for a remote call, the bare
//! atom otherwise).

use std::collections::HashMap;

use tree_sitter::Node;

use super::helpers::{
    self, NameArity, arg_count, child_named_kinds, find_child_by_type, first_atom_text,
    function_arity_entries, named_children, unquote_atom,
};
use super::{Bounded, ErlangExtractor};
use super::{definition_forms, mfa};
use crate::base::{Identifier, IdentifierKind, Symbol, SymbolKind, extract_type_arguments};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

/// `(name, arity)` pairs made local by `-import(Module, [...])`, keyed to the
/// module that owns them.
pub(super) type ImportedFunctions = HashMap<NameArity, String>;

pub(super) fn extract_identifiers(
    extractor: &mut ErlangExtractor,
    declarations: &[Bounded],
    symbols: &[Symbol],
) -> Vec<Identifier> {
    let nodes: Vec<Node> = declarations.iter().map(|d| d.node).collect();
    let imports = imported_functions(extractor, &nodes);
    let containing_symbols = extractor.base.containing_symbol_index(symbols);
    let mut clause_scopes: HashMap<NameArity, Option<String>> = HashMap::new();
    let module_name = symbols
        .iter()
        .find(|symbol| symbol.kind == SymbolKind::Module)
        .map(|symbol| symbol.name.clone());
    let spec_scopes = function_ids_by_identity(symbols);

    for &Bounded {
        node: declaration,
        end,
    } in declarations
    {
        let declaration = &declaration;
        let context = WalkContext {
            imports: &imports,
            module_name: module_name.as_deref(),
            end,
        };
        match declaration.kind() {
            "fun_decl" if helpers::doc_macro_name(&extractor.base, declaration).is_some() => {}
            "fun_decl" => {
                let scope = function_scope(
                    extractor,
                    declaration,
                    &containing_symbols,
                    &mut clause_scopes,
                );
                walk(extractor, *declaration, scope.as_deref(), &context, 0);
            }
            "spec" => {
                let scope = spec_identity(extractor, declaration)
                    .and_then(|identity| spec_scopes.get(&identity).cloned());
                walk_type_identifiers(extractor, *declaration, scope.as_deref(), end, 0);
            }
            "callback" | "type_alias" | "opaque" | "nominal" => {
                let scope = containing_symbol_id(declaration, &containing_symbols);
                walk_type_identifiers(extractor, *declaration, scope.as_deref(), end, 0);
            }
            "record_decl" => {
                let scope = containing_symbol_id(declaration, &containing_symbols);
                for field in child_named_kinds(declaration, "record_field") {
                    if let Some(field_type) = field.child_by_field_name("ty") {
                        walk_type_identifiers(extractor, field_type, scope.as_deref(), end, 0);
                    }
                    if let Some(default) = field.child_by_field_name("expr") {
                        walk(extractor, default, scope.as_deref(), &context, 0);
                    }
                }
            }
            "pp_define" => {
                let scope = containing_symbol_id(declaration, &containing_symbols);
                for child in named_children(declaration) {
                    if child.kind() == "macro_lhs" {
                        continue;
                    }
                    walk(extractor, child, scope.as_deref(), &context, 0);
                }
            }
            _ => {}
        }
    }

    extractor.base.identifiers.clone()
}

/// Function symbol ids keyed by `(name, arity)`, the identity a `-spec`
/// annotates. Callbacks are declarations of their own, not spec targets.
fn function_ids_by_identity(symbols: &[Symbol]) -> HashMap<NameArity, String> {
    symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Function)
        .filter(|symbol| {
            symbol
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("callback"))
                .is_none()
        })
        .filter_map(|symbol| {
            let arity = symbol.metadata.as_ref()?.get("arity")?.as_u64()? as u32;
            Some(((symbol.name.clone(), arity), symbol.id.clone()))
        })
        .collect()
}

fn spec_identity(extractor: &ErlangExtractor, spec: &Node) -> Option<NameArity> {
    let name = first_atom_text(&extractor.base, spec)?;
    let arguments = find_child_by_type(spec, "type_sig")?.child_by_field_name("args")?;
    Some((name, arg_count(&arguments)))
}

struct WalkContext<'a> {
    imports: &'a ImportedFunctions,
    /// The `-module` name, which `?MODULE` expands to.
    module_name: Option<&'a str>,
    /// Bytes from here on belong to a later, recovered declaration.
    end: usize,
}

/// A multi-clause function is a run of sibling `fun_decl` nodes but a single
/// symbol, so later clauses reuse the scope resolved for the first clause of
/// the same name/arity.
fn function_scope(
    extractor: &ErlangExtractor,
    declaration: &Node,
    containing_symbols: &crate::base::ContainingSymbolIndex<'_>,
    clause_scopes: &mut HashMap<NameArity, Option<String>>,
) -> Option<String> {
    let Some(clause) = definition_forms::function_clause(extractor, declaration) else {
        return containing_symbol_id(declaration, containing_symbols);
    };
    clause_scopes
        .entry(clause.identity)
        .or_insert_with(|| containing_symbol_id(declaration, containing_symbols))
        .clone()
}

fn containing_symbol_id(
    node: &Node,
    containing_symbols: &crate::base::ContainingSymbolIndex<'_>,
) -> Option<String> {
    containing_symbols
        .find(*node)
        .map(|symbol| symbol.id.clone())
}

fn walk(
    extractor: &mut ErlangExtractor,
    node: Node,
    scope: Option<&str>,
    context: &WalkContext,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) || node.start_byte() >= context.end {
        return;
    }

    emit_identifiers(extractor, node, scope, context);

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    for child in named_children(&node) {
        walk(extractor, child, scope, context, child_depth);
    }
}

fn walk_type_identifiers(
    extractor: &mut ErlangExtractor,
    node: Node,
    scope: Option<&str>,
    end: usize,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) || node.start_byte() >= end {
        return;
    }
    if node.kind() == "call"
        && let Some(atom) = node
            .child_by_field_name("expr")
            .filter(|expr| expr.kind() == "atom")
    {
        let name = unquote_atom(&extractor.base.get_node_text(&atom));
        let identifier = extractor.base.create_identifier(
            &atom,
            name,
            IdentifierKind::TypeUsage,
            scope.map(String::from),
        );
        let nested_in_application = node
            .parent()
            .filter(|parent| parent.kind() == "expr_args")
            .and_then(|args| args.parent())
            .is_some_and(|parent| parent.kind() == "call");
        if let Some(args) = node
            .child_by_field_name("args")
            .filter(|args| args.named_child_count() > 0 && !nested_in_application)
        {
            let arguments = extract_type_arguments(&extractor.base, args, decompose_type_arg);
            extractor.base.record_type_arguments(&identifier, arguments);
        }
    }
    match node.kind() {
        "record_expr" => emit_record_reference(extractor, node, scope),
        "remote" => {
            emit_module_qualifier(extractor, find_child_by_type(&node, "remote_module"), scope)
        }
        _ => {}
    }
    let Some(next_depth) = child_tree_depth(depth) else {
        return;
    };
    for child in named_children(&node) {
        walk_type_identifiers(extractor, child, scope, end, next_depth);
    }
}

fn emit_identifiers(
    extractor: &mut ErlangExtractor,
    node: Node,
    scope: Option<&str>,
    context: &WalkContext,
) {
    match node.kind() {
        "call" => emit_call(extractor, node, scope, context),
        "remote" => {
            emit_module_qualifier(extractor, find_child_by_type(&node, "remote_module"), scope)
        }
        "internal_fun" => emit_fun_reference(extractor, node, scope),
        "external_fun" => {
            emit_module_qualifier(extractor, find_child_by_type(&node, "module"), scope);
            emit_fun_reference(extractor, node, scope);
        }
        "macro_call_expr" => emit_macro_usage(extractor, node, scope),
        "record_expr" | "record_update_expr" | "record_index_expr" | "record_field_expr" => {
            emit_record_reference(extractor, node, scope)
        }
        "qualified_record_expr"
        | "qualified_record_update_expr"
        | "qualified_record_field_expr" => emit_qualified_record_reference(extractor, node, scope),
        _ => {}
    }
    for (function, module, _) in mfa::targets(extractor, node, context.module_name) {
        let name = unquote_atom(&extractor.base.get_node_text(&function));
        extractor.base.create_identifier_with_metadata(
            &function,
            name,
            IdentifierKind::Call,
            scope.map(String::from),
            receiver_metadata(&module),
        );
    }
}

/// `#geo:line{from = P}` (OTP 29): the record name and its module are type
/// usages, and each field is a member access on the record.
fn emit_qualified_record_reference(
    extractor: &mut ErlangExtractor,
    node: Node,
    scope: Option<&str>,
) {
    let Some(name) = node.child_by_field_name("name") else {
        return;
    };
    emit_module_qualifier(extractor, name.child_by_field_name("module"), scope);
    let Some(record) = name
        .child_by_field_name("name")
        .filter(|n| n.kind() == "atom")
    else {
        return;
    };
    let record_name = unquote_atom(&extractor.base.get_node_text(&record));
    extractor.base.create_identifier(
        &record,
        record_name.clone(),
        IdentifierKind::TypeUsage,
        scope.map(String::from),
    );
    let fields = node
        .child_by_field_name("field")
        .into_iter()
        .chain(child_named_kinds(&node, "record_field"));
    for field in fields {
        emit_wrapped_atom(
            extractor,
            Some(field),
            IdentifierKind::MemberAccess,
            scope,
            Some(&record_name),
        );
    }
}

fn decompose_type_arg<'a>(
    base: &crate::base::BaseExtractor,
    node: Node<'a>,
) -> Option<(String, Option<Node<'a>>)> {
    if !node.is_named() {
        return None;
    }
    match node.kind() {
        "call" => {
            let atom = node
                .child_by_field_name("expr")
                .filter(|e| e.kind() == "atom")?;
            let nested = node
                .child_by_field_name("args")
                .filter(|args| args.named_child_count() > 0);
            Some((unquote_atom(&base.get_node_text(&atom)), nested))
        }
        _ => Some((base.get_node_text(&node), None)),
    }
}

/// A `call` whose callee is not an `atom` is a dynamic call through a variable
/// (`Fun(X)`) and names no symbol. An auto-imported BIF (`length/1`, `self/0`)
/// is left as a bare call: attributing it to `erlang` would invent a module
/// reference the source never wrote and no workspace symbol can resolve.
fn emit_call(
    extractor: &mut ErlangExtractor,
    node: Node,
    scope: Option<&str>,
    context: &WalkContext,
) {
    let Some(atom) = find_child_by_type(&node, "atom") else {
        return;
    };
    let name = unquote_atom(&extractor.base.get_node_text(&atom));
    let is_remote = node.parent().map(|parent| parent.kind()) == Some("remote");
    let module = is_remote
        .then(|| remote_module_name(extractor, node, context.module_name))
        .flatten();
    match &module {
        Some(module) => {
            extractor.base.create_identifier_with_metadata(
                &atom,
                name.clone(),
                IdentifierKind::Call,
                scope.map(String::from),
                receiver_metadata(module),
            );
        }
        None => {
            extractor.base.create_identifier(
                &atom,
                name.clone(),
                IdentifierKind::Call,
                scope.map(String::from),
            );
        }
    }

    let carrier = match &module {
        Some(module) => format!("{module}:{name}"),
        None => name.clone(),
    };
    record_call_arg_literals(extractor, node, &carrier, scope);

    if is_remote {
        return;
    }
    let arity = find_child_by_type(&node, "expr_args")
        .map(|args| arg_count(&args))
        .unwrap_or(0);
    if let Some(module) = context.imports.get(&(name, arity)).cloned() {
        extractor.base.create_identifier(
            &atom,
            module,
            IdentifierKind::TypeUsage,
            scope.map(String::from),
        );
    }
}

fn receiver_metadata(receiver: &str) -> HashMap<String, serde_json::Value> {
    HashMap::from([(
        "receiver".to_string(),
        serde_json::Value::String(receiver.to_string()),
    )])
}

/// The module a remote call names: its atom, or this file's module for
/// `?MODULE`. A variable module (`Mod:run(...)`) names none.
fn remote_module_name(
    extractor: &ErlangExtractor,
    call: Node,
    module_name: Option<&str>,
) -> Option<String> {
    let module = call
        .parent()
        .and_then(|remote| find_child_by_type(&remote, "remote_module"))?
        .child_by_field_name("module")?;
    match module.kind() {
        "atom" => Some(unquote_atom(&extractor.base.get_node_text(&module))),
        "macro_call_expr" if super::relationships::is_module_macro(extractor, &module) => {
            module_name.map(str::to_string)
        }
        _ => None,
    }
}

/// Capture string-literal arguments of a call as `Literal` records.
///
/// Config-free: `kind` stays `Other` and `arg_position` counts over the whole
/// argument list; the artifact language-policy pass reclassifies and gates by
/// carrier. Only direct arguments are captured, so a string nested inside a
/// list or tuple argument is not a call-argument literal.
fn record_call_arg_literals(
    extractor: &mut ErlangExtractor,
    node: Node,
    carrier: &str,
    scope: Option<&str>,
) {
    let Some(args) = find_child_by_type(&node, "expr_args") else {
        return;
    };
    for (position, argument) in named_children(&args).into_iter().enumerate() {
        let literal = binary_string(&argument).unwrap_or(argument);
        let Some(text) = extractor.base.decode_string_literal(&literal) else {
            continue;
        };
        extractor.base.record_literal(
            &literal,
            text,
            Some(carrier.to_string()),
            position as u32,
            scope.map(String::from),
        );
    }
}

/// The string of a one-segment binary `<<"text">>`, which Erlang code uses
/// for UTF-8 text such as URLs and SQL.
fn binary_string<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    if node.kind() != "binary" {
        return None;
    }
    let elements = named_children(node);
    let [element] = elements.as_slice() else {
        return None;
    };
    if element.named_child_count() != 1 {
        return None;
    }
    element
        .child_by_field_name("element")
        .filter(|string| string.kind() == "string")
}

/// Records the module of `lists:reverse(X)`, `fun lists:reverse/1`, and an
/// `-import`-ed call as its own row, anchored on the module atom where the
/// source spells one and on the callee atom where `-import` leaves it implicit.
fn emit_module_qualifier(
    extractor: &mut ErlangExtractor,
    wrapper: Option<Node>,
    scope: Option<&str>,
) {
    let Some(wrapper) = wrapper else {
        return;
    };
    let Some(atom) = find_child_by_type(&wrapper, "atom") else {
        return;
    };
    let name = unquote_atom(&extractor.base.get_node_text(&atom));
    extractor.base.create_identifier(
        &atom,
        name,
        IdentifierKind::TypeUsage,
        scope.map(String::from),
    );
}

fn emit_fun_reference(extractor: &mut ErlangExtractor, node: Node, scope: Option<&str>) {
    let Some(atom) = find_child_by_type(&node, "atom") else {
        return;
    };
    let name = unquote_atom(&extractor.base.get_node_text(&atom));
    extractor.base.create_identifier(
        &atom,
        name,
        IdentifierKind::VariableRef,
        scope.map(String::from),
    );
}

/// `?NAME` reads a macro and `?NAME(...)` invokes one. The name is an atom
/// for a lowercase macro (`?assertEqual`) and a variable for an uppercase one.
fn emit_macro_usage(extractor: &mut ErlangExtractor, node: Node, scope: Option<&str>) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let kind = if find_child_by_type(&node, "macro_call_args").is_some() {
        IdentifierKind::Call
    } else {
        IdentifierKind::VariableRef
    };
    let name = unquote_atom(&extractor.base.get_node_text(&name_node));
    extractor
        .base
        .create_identifier(&name_node, name, kind, scope.map(String::from));
}

/// Record names are type usages and fields are member accesses whose
/// receiver is the record (`id` in `#user{id = X}` has receiver `user`).
fn emit_record_reference(extractor: &mut ErlangExtractor, node: Node, scope: Option<&str>) {
    let record = find_child_by_type(&node, "record_name");
    emit_wrapped_atom(extractor, record, IdentifierKind::TypeUsage, scope, None);
    let record = record
        .and_then(|wrapper| find_child_by_type(&wrapper, "atom"))
        .map(|atom| unquote_atom(&extractor.base.get_node_text(&atom)));
    emit_wrapped_atom(
        extractor,
        find_child_by_type(&node, "record_field_name"),
        IdentifierKind::MemberAccess,
        scope,
        record.as_deref(),
    );
    for field in child_named_kinds(&node, "record_field") {
        emit_wrapped_atom(
            extractor,
            Some(field),
            IdentifierKind::MemberAccess,
            scope,
            record.as_deref(),
        );
    }
}

fn emit_wrapped_atom(
    extractor: &mut ErlangExtractor,
    wrapper: Option<Node>,
    kind: IdentifierKind,
    scope: Option<&str>,
    receiver: Option<&str>,
) {
    let Some(wrapper) = wrapper else {
        return;
    };
    let Some(atom) = find_child_by_type(&wrapper, "atom") else {
        return;
    };
    let name = unquote_atom(&extractor.base.get_node_text(&atom));
    let scope = scope.map(String::from);
    match receiver {
        Some(receiver) => {
            extractor.base.create_identifier_with_metadata(
                &atom,
                name,
                kind,
                scope,
                receiver_metadata(receiver),
            );
        }
        None => {
            extractor.base.create_identifier(&atom, name, kind, scope);
        }
    }
}

pub(super) fn imported_functions(
    extractor: &ErlangExtractor,
    declarations: &[Node],
) -> ImportedFunctions {
    let mut imports = ImportedFunctions::new();
    for declaration in declarations {
        if declaration.kind() != "import_attribute" {
            continue;
        }
        let Some(module) = first_atom_text(&extractor.base, declaration) else {
            continue;
        };
        for entry in function_arity_entries(&extractor.base, declaration) {
            imports.insert(entry, module.clone());
        }
    }
    imports
}
