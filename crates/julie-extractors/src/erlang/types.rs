//! Erlang's declared type surface: `-spec`, `-callback`, `-type` and `-opaque`.
//!
//! These attributes are walked here and nowhere else. The relationship and
//! identifier walks visit executable forms only, because a `call` node inside a
//! type declaration is a type application — `integer()` in a `-spec` is not a
//! call site — and widening those walks to reach type attributes would emit
//! false call edges. This walk therefore reads the same declarations
//! independently and produces only type facts.
//!
//! Only the declared base type name is recorded; nothing is inferred from
//! function bodies. A declared shape with no single base name (list, tuple,
//! union, fun, range, map, binary, type variable) records nothing.

use std::collections::HashMap;

use tree_sitter::Node;

use super::helpers::{NameArity, arg_count, find_child_by_type, first_atom_text, unquote_atom};
use crate::base::{BaseExtractor, Symbol, SymbolKind};

const ARITY_METADATA_KEY: &str = "arity";
const CALLBACK_METADATA_KEY: &str = "callback";

/// Declared base type names keyed by the `(name, arity)` identity they
/// annotate. Specs, callbacks, and type aliases occupy separate Erlang
/// namespaces, so a module may declare `handle/1` in all three without
/// collision.
#[derive(Debug, Default, Clone)]
pub(crate) struct DeclaredTypes {
    specs: HashMap<NameArity, String>,
    callbacks: HashMap<NameArity, String>,
    aliases: HashMap<NameArity, String>,
}

/// Collect every declared type form from the top-level declarations.
pub(super) fn collect(base: &BaseExtractor, declarations: &[Node]) -> DeclaredTypes {
    let mut declared = DeclaredTypes::default();

    for declaration in declarations {
        match declaration.kind() {
            "spec" => insert(base, declaration, signature_form, &mut declared.specs),
            "callback" => insert(base, declaration, signature_form, &mut declared.callbacks),
            "type_alias" | "opaque" => insert(base, declaration, alias_form, &mut declared.aliases),
            _ => {}
        }
    }

    declared
}

/// Match declared forms to the symbols they annotate.
pub(super) fn infer_types(declared: &DeclaredTypes, symbols: &[Symbol]) -> HashMap<String, String> {
    let mut types = HashMap::new();

    for symbol in symbols {
        let Some(arity) = metadata_arity(symbol) else {
            continue;
        };
        let identity = (symbol.name.clone(), arity);
        let form = match symbol.kind {
            SymbolKind::Type => declared.aliases.get(&identity),
            SymbolKind::Function if is_callback(symbol) => declared.callbacks.get(&identity),
            SymbolKind::Function => declared.specs.get(&identity),
            _ => None,
        };
        if let Some(form) = form {
            types.insert(symbol.id.clone(), form.clone());
        }
    }

    types
}

fn insert(
    base: &BaseExtractor,
    declaration: &Node,
    form: fn(&BaseExtractor, &Node) -> Option<(NameArity, String)>,
    into: &mut HashMap<NameArity, String>,
) {
    if let Some((identity, declared)) = form(base, declaration) {
        into.entry(identity).or_insert(declared);
    }
}

/// `-spec open(integer()) -> account().` and `-callback init(term()) -> ok.`
/// share the `atom` + `type_sig` shape. A multi-clause spec carries one
/// `type_sig` per clause; the first is used, matching how a multi-clause
/// function takes its signature from the first clause head.
fn signature_form(base: &BaseExtractor, declaration: &Node) -> Option<(NameArity, String)> {
    let name = first_atom_text(base, declaration)?;
    let signature = find_child_by_type(declaration, "type_sig")?;
    let arguments = signature.child_by_field_name("args")?;
    let return_type = signature.child_by_field_name("ty")?;

    Some((
        (name, arg_count(&arguments)),
        base_type_name(base, &return_type)?,
    ))
}

/// `-type account() :: #account{}.` names the alias in a `type_name` child and
/// carries the declared form in the `ty` field.
fn alias_form(base: &BaseExtractor, declaration: &Node) -> Option<(NameArity, String)> {
    let type_name = find_child_by_type(declaration, "type_name")?;
    let name = first_atom_text(base, &type_name)?;
    let arity = find_child_by_type(&type_name, "var_args")
        .map(|parameters| arg_count(&parameters))
        .unwrap_or(0);
    let declared = declaration.child_by_field_name("ty")?;

    Some(((name, arity), base_type_name(base, &declared)?))
}

/// The single name a declared type node states: `foo()` and `mod:foo()` name
/// `foo` and `mod:foo`, `#foo{}` names `foo`, a bare atom names itself, and an
/// annotation (`Result :: foo()`) names its type.
fn base_type_name(base: &BaseExtractor, declared: &Node) -> Option<String> {
    match declared.kind() {
        "atom" => Some(unquote_atom(&base.get_node_text(declared))),
        "call" => atom_name(base, &declared.child_by_field_name("expr")?),
        "remote" => remote_type_name(base, declared),
        "record_expr" => first_atom_text(base, &declared.child_by_field_name("name")?),
        "record_name" => first_atom_text(base, declared),
        "ann_type" => base_type_name(base, &declared.child_by_field_name("ty")?),
        "paren_expr" => base_type_name(base, &declared.child_by_field_name("expr")?),
        _ => None,
    }
}

/// `mod:foo()` parses as `remote{module: remote_module{module}, fun: call}`.
fn remote_type_name(base: &BaseExtractor, declared: &Node) -> Option<String> {
    let module = declared
        .child_by_field_name("module")?
        .child_by_field_name("module")?;
    let module = atom_name(base, &module)?;
    let name = base_type_name(base, &declared.child_by_field_name("fun")?)?;
    Some(format!("{module}:{name}"))
}

fn atom_name(base: &BaseExtractor, node: &Node) -> Option<String> {
    (node.kind() == "atom").then(|| unquote_atom(&base.get_node_text(node)))
}

fn metadata_arity(symbol: &Symbol) -> Option<u32> {
    symbol
        .metadata
        .as_ref()?
        .get(ARITY_METADATA_KEY)?
        .as_u64()
        .map(|arity| arity as u32)
}

fn is_callback(symbol: &Symbol) -> bool {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get(CALLBACK_METADATA_KEY))
        .and_then(serde_json::Value::as_bool)
        == Some(true)
}
