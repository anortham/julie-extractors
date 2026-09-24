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
    /// Base type of each `-spec` argument position, `None` where the declared
    /// argument has no single base name.
    spec_args: HashMap<NameArity, Vec<Option<String>>>,
    callbacks: HashMap<NameArity, String>,
    aliases: HashMap<NameArity, String>,
    /// The return type every clause of every `-spec` for a function agrees
    /// on; `None` when a clause states no single base name or two disagree.
    spec_returns: HashMap<NameArity, Option<DeclaredType>>,
}

/// A declared type as its base name and the text written for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeclaredType {
    pub(super) name: String,
    pub(super) declared: String,
}

impl DeclaredTypes {
    pub(super) fn spec_args(&self, identity: &NameArity) -> &[Option<String>] {
        self.spec_args.get(identity).map_or(&[], Vec::as_slice)
    }

    /// Functions whose `-spec` clauses all agree on one base return type.
    pub(super) fn agreed_spec_returns(&self) -> impl Iterator<Item = (&NameArity, &DeclaredType)> {
        self.spec_returns
            .iter()
            .filter_map(|(identity, agreed)| Some((identity, agreed.as_ref()?)))
    }
}

/// Collect every declared type form from the top-level declarations.
pub(super) fn collect(base: &BaseExtractor, declarations: &[Node]) -> DeclaredTypes {
    let mut declared = DeclaredTypes::default();

    for declaration in declarations {
        match declaration.kind() {
            "spec" => {
                insert(base, declaration, signature_form, &mut declared.specs);
                if let Some((identity, args)) = spec_argument_types(base, declaration) {
                    declared.spec_args.entry(identity).or_insert(args);
                }
                if let Some((identity, returned)) = spec_return(base, declaration) {
                    declared
                        .spec_returns
                        .entry(identity)
                        .and_modify(|agreed| {
                            if *agreed != returned {
                                *agreed = None;
                            }
                        })
                        .or_insert(returned);
                }
            }
            "callback" => insert(base, declaration, signature_form, &mut declared.callbacks),
            "type_alias" | "opaque" | "nominal" => {
                insert(base, declaration, alias_form, &mut declared.aliases)
            }
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

/// The base type of each argument of a spec's first clause:
/// `-spec c(Req :: cowboy_req:req(), role())` gives `cowboy_req:req`, `role`.
fn spec_argument_types(
    base: &BaseExtractor,
    declaration: &Node,
) -> Option<(NameArity, Vec<Option<String>>)> {
    let name = first_atom_text(base, declaration)?;
    let arguments = find_child_by_type(declaration, "type_sig")?.child_by_field_name("args")?;
    let mut cursor = arguments.walk();
    let types: Vec<Option<String>> = arguments
        .named_children(&mut cursor)
        .map(|argument| base_type_name(base, &argument))
        .collect();
    Some(((name, types.len() as u32), types))
}

/// The return type all clauses of `-spec` agree on:
/// `-spec load(a) -> state(); (b) -> state().` gives `state`, and clauses
/// with different or shapeless returns give `None`.
fn spec_return(
    base: &BaseExtractor,
    declaration: &Node,
) -> Option<(NameArity, Option<DeclaredType>)> {
    let name = first_atom_text(base, declaration)?;
    let mut cursor = declaration.walk();
    let signatures: Vec<Node> = declaration
        .children_by_field_name("sigs", &mut cursor)
        .collect();
    let first = signatures.first()?;
    let arity = arg_count(&first.child_by_field_name("args")?);
    let first_return = first.child_by_field_name("ty")?;
    let returned = base_type_name(base, &first_return)
        .filter(|name| {
            signatures[1..].iter().all(|signature| {
                signature
                    .child_by_field_name("ty")
                    .and_then(|ty| base_type_name(base, &ty))
                    .as_ref()
                    == Some(name)
            })
        })
        .map(|name| DeclaredType {
            name,
            declared: base.get_node_text(&first_return),
        });
    Some(((name, arity), returned))
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
pub(super) fn base_type_name(base: &BaseExtractor, declared: &Node) -> Option<String> {
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
