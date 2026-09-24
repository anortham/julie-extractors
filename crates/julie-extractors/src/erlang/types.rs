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
//! function bodies. A declared shape with no single base name (`[t()]`,
//! tuple, union, fun, range, `#{..}`, binary, type variable) records nothing.
//! The named types `list(t())`, `nonempty_list(t())`, and `map()` record
//! their base name.

use std::collections::HashMap;

use tree_sitter::Node;

use super::helpers::{
    NameArity, arg_count, arguments, find_child_by_type, first_atom_text, unquote_atom,
};
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
    /// What the `-spec` of each function says a call returns; empty when two
    /// `-spec` attributes for one function disagree.
    spec_returns: HashMap<NameArity, SpecReturn>,
}

/// The types a call to a function with a `-spec` hands back.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SpecReturn {
    /// The type every spec clause returns: `X = load()` gets it.
    pub(super) value: Option<DeclaredType>,
    /// The `T` of every `{ok, T}` alternative: `{ok, X} = load()` gets it.
    pub(super) ok_payload: Option<DeclaredType>,
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

    /// Functions whose `-spec` returns a usable value or `{ok, T}` type.
    pub(super) fn spec_returns(&self) -> impl Iterator<Item = (&NameArity, &SpecReturn)> {
        self.spec_returns
            .iter()
            .filter(|(_, returned)| returned.value.is_some() || returned.ok_payload.is_some())
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
                                *agreed = SpecReturn::default();
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
    let args = find_child_by_type(declaration, "type_sig")?.child_by_field_name("args")?;
    let types: Vec<Option<String>> = arguments(&args)
        .iter()
        .map(|argument| base_type_name(base, argument))
        .collect();
    Some(((name, types.len() as u32), types))
}

/// What all clauses of one `-spec` return:
/// `-spec load(a) -> state(); (b) -> state().` gives the value type `state`,
/// and `-spec open() -> {ok, conn()} | {error, term()}.` gives the `{ok, T}`
/// payload `conn`.
fn spec_return(base: &BaseExtractor, declaration: &Node) -> Option<(NameArity, SpecReturn)> {
    let name = first_atom_text(base, declaration)?;
    let mut cursor = declaration.walk();
    let signatures: Vec<Node> = declaration
        .children_by_field_name("sigs", &mut cursor)
        .collect();
    let args = arguments(&signatures.first()?.child_by_field_name("args")?);
    if args
        .iter()
        .any(|argument| argument.kind() == "macro_call_expr")
    {
        return None;
    }
    let arity = args.len() as u32;
    let returns = signatures
        .iter()
        .map(|signature| signature.child_by_field_name("ty"))
        .collect::<Option<Vec<Node>>>()?;
    let returned = SpecReturn {
        value: agreed(returns.iter().map(|returned| value_type(base, *returned))),
        ok_payload: ok_payload(base, returns),
    };
    Some(((name, arity), returned))
}

/// The first type when every item declares the same type. The same base
/// name is not enough: `#state{}` and `state()` both name `state`.
fn agreed(mut types: impl Iterator<Item = Option<DeclaredType>>) -> Option<DeclaredType> {
    let first = types.next()??;
    types
        .all(|other| other.as_ref() == Some(&first))
        .then_some(first)
}

/// The type of the values a declared form describes. A bare atom (`-> ok`)
/// is one literal value, not a type, and `no_return()` and `none()` describe
/// no value at all, so these give `None`.
fn value_type(base: &BaseExtractor, declared: Node) -> Option<DeclaredType> {
    let form = strip_annotation(declared)?;
    if form.kind() == "atom" {
        return None;
    }
    let name = base_type_name(base, &form)
        .filter(|name| !matches!(name.as_str(), "no_return" | "none"))?;
    Some(DeclaredType {
        name,
        declared: base.get_node_text(&declared),
    })
}

/// `R :: t()` and `(t())` describe the same values as `t()`.
fn strip_annotation(mut form: Node) -> Option<Node> {
    loop {
        form = match form.kind() {
            "ann_type" => form.child_by_field_name("ty")?,
            "paren_expr" => form.child_by_field_name("expr")?,
            _ => return Some(form),
        };
    }
}

/// The `T` that every `{ok, T}` alternative of the returns agrees on. Atoms
/// and tuples with another tag or size cannot match `{ok, X}`. Any other
/// alternative (a named type, a type variable) might, so it gives `None`.
fn ok_payload(base: &BaseExtractor, returns: Vec<Node>) -> Option<DeclaredType> {
    let mut payload: Option<DeclaredType> = None;
    let mut alternatives = returns;
    while let Some(alternative) = alternatives.pop() {
        let form = strip_annotation(alternative)?;
        match form.kind() {
            "pipe" => {
                alternatives.push(form.child_by_field_name("lhs")?);
                alternatives.push(form.child_by_field_name("rhs")?);
            }
            "atom" => {}
            "tuple" => {
                let mut cursor = form.walk();
                let elements: Vec<Node> =
                    form.children_by_field_name("expr", &mut cursor).collect();
                match elements.as_slice() {
                    [tag, value] if is_ok_atom(base, tag) => {
                        let found = value_type(base, *value)?;
                        match &payload {
                            Some(agreed) if *agreed != found => return None,
                            Some(_) => {}
                            None => payload = Some(found),
                        }
                    }
                    [tag, ..] if tag.kind() == "atom" => {}
                    [] => {}
                    _ => return None,
                }
            }
            _ => return None,
        }
    }
    payload
}

pub(super) fn is_ok_atom(base: &BaseExtractor, node: &Node) -> bool {
    node.kind() == "atom" && unquote_atom(&base.get_node_text(node)) == "ok"
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
