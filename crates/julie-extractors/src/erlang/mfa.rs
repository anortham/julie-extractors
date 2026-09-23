//! Calls in MFA form. `spawn(M, F, Args)`, `apply/3`, `rpc:call/4`,
//! `timer:apply_after/4`, and supervisor child specs name the function they
//! run as a module atom, a function atom, and a literal argument list; xref
//! treats them as calls, and so does this extractor.

use tree_sitter::Node;

use super::ErlangExtractor;
use super::helpers::{find_child_by_type, named_children, unquote_atom};

/// `(module, function, arity, index of the module argument)` for each carrier.
/// `None` as the module is an unqualified (auto-imported) call.
const MFA_CARRIERS: &[(Option<&str>, &str, u32, usize)] = &[
    (None, "spawn", 3, 0),
    (None, "spawn", 4, 1),
    (None, "spawn_link", 3, 0),
    (None, "spawn_link", 4, 1),
    (None, "spawn_monitor", 3, 0),
    (None, "spawn_opt", 4, 0),
    (None, "apply", 3, 0),
    (Some("erlang"), "spawn", 3, 0),
    (Some("erlang"), "spawn", 4, 1),
    (Some("erlang"), "spawn_link", 3, 0),
    (Some("erlang"), "spawn_link", 4, 1),
    (Some("erlang"), "spawn_monitor", 3, 0),
    (Some("erlang"), "spawn_opt", 4, 0),
    (Some("erlang"), "apply", 3, 0),
    (Some("proc_lib"), "spawn", 3, 0),
    (Some("proc_lib"), "spawn_link", 3, 0),
    (Some("proc_lib"), "start", 3, 0),
    (Some("proc_lib"), "start_link", 3, 0),
    (Some("timer"), "apply_after", 4, 1),
    (Some("timer"), "apply_interval", 4, 1),
    (Some("timer"), "apply_repeatedly", 4, 1),
    (Some("rpc"), "call", 4, 1),
    (Some("rpc"), "call", 5, 1),
    (Some("rpc"), "cast", 4, 1),
    (Some("rpc"), "async_call", 4, 1),
    (Some("erpc"), "call", 4, 1),
    (Some("erpc"), "call", 5, 1),
    (Some("erpc"), "cast", 4, 1),
];

const CHILD_RESTART_TYPES: &[&str] = &["permanent", "transient", "temporary"];

/// The functions `node` runs in MFA form, each as its function atom, the
/// module that owns it, and the call arity (the length of the argument list).
pub(super) fn targets<'tree>(
    extractor: &ErlangExtractor,
    node: Node<'tree>,
    module_name: Option<&str>,
) -> Vec<(Node<'tree>, String, u32)> {
    let found = match node.kind() {
        "call" if node.parent().map(|parent| parent.kind()) != Some("remote") => {
            carrier_target(extractor, None, &node, module_name)
        }
        "remote" => remote_carrier_target(extractor, &node, module_name),
        "map_expr" => child_spec_map_target(extractor, &node, module_name),
        "tuple" => child_spec_tuple_target(extractor, &node, module_name),
        _ => None,
    };
    found.into_iter().collect()
}

fn remote_carrier_target<'tree>(
    extractor: &ErlangExtractor,
    remote: &Node<'tree>,
    module_name: Option<&str>,
) -> Option<(Node<'tree>, String, u32)> {
    let module = find_child_by_type(remote, "remote_module")?
        .child_by_field_name("module")
        .filter(|module| module.kind() == "atom")?;
    let module = unquote_atom(&extractor.base.get_node_text(&module));
    let call = find_child_by_type(remote, "call")?;
    carrier_target(extractor, Some(&module), &call, module_name)
}

fn carrier_target<'tree>(
    extractor: &ErlangExtractor,
    carrier_module: Option<&str>,
    call: &Node<'tree>,
    module_name: Option<&str>,
) -> Option<(Node<'tree>, String, u32)> {
    let callee = call
        .child_by_field_name("expr")
        .filter(|e| e.kind() == "atom")?;
    let name = unquote_atom(&extractor.base.get_node_text(&callee));
    let args = named_children(&call.child_by_field_name("args")?);
    let &(_, _, _, module_index) = MFA_CARRIERS.iter().find(|(module, function, arity, _)| {
        *module == carrier_module && *function == name && *arity as usize == args.len()
    })?;
    mfa(
        extractor,
        &args[module_index..module_index + 3],
        module_name,
    )
}

/// `#{start => {M, F, A}}` in a supervisor child spec map.
fn child_spec_map_target<'tree>(
    extractor: &ErlangExtractor,
    map: &Node<'tree>,
    module_name: Option<&str>,
) -> Option<(Node<'tree>, String, u32)> {
    let start = named_children(map).into_iter().find(|field| {
        field.kind() == "map_field"
            && field.child_by_field_name("key").is_some_and(|key| {
                key.kind() == "atom" && unquote_atom(&extractor.base.get_node_text(&key)) == "start"
            })
    })?;
    mfa_tuple(extractor, &start.child_by_field_name("value")?, module_name)
}

/// `{Id, {M, F, A}, Restart, Shutdown, Type, Modules}`, the tuple child spec.
fn child_spec_tuple_target<'tree>(
    extractor: &ErlangExtractor,
    tuple: &Node<'tree>,
    module_name: Option<&str>,
) -> Option<(Node<'tree>, String, u32)> {
    let elements = named_children(tuple);
    if elements.len() != 6 {
        return None;
    }
    let restart = elements[2];
    if restart.kind() != "atom"
        || !CHILD_RESTART_TYPES
            .contains(&unquote_atom(&extractor.base.get_node_text(&restart)).as_str())
    {
        return None;
    }
    mfa_tuple(extractor, &elements[1], module_name)
}

fn mfa_tuple<'tree>(
    extractor: &ErlangExtractor,
    tuple: &Node<'tree>,
    module_name: Option<&str>,
) -> Option<(Node<'tree>, String, u32)> {
    if tuple.kind() != "tuple" {
        return None;
    }
    let elements = named_children(tuple);
    if elements.len() != 3 {
        return None;
    }
    mfa(extractor, &elements, module_name)
}

/// A literal `Module, Function, [Args]` triple: an atom or `?MODULE`, an atom,
/// and a list.
fn mfa<'tree>(
    extractor: &ErlangExtractor,
    triple: &[Node<'tree>],
    module_name: Option<&str>,
) -> Option<(Node<'tree>, String, u32)> {
    let [module, function, args] = triple else {
        return None;
    };
    let module = match module.kind() {
        "atom" => unquote_atom(&extractor.base.get_node_text(module)),
        "macro_call_expr" if super::relationships::is_module_macro(extractor, module) => {
            module_name?.to_string()
        }
        _ => return None,
    };
    if function.kind() != "atom" || args.kind() != "list" {
        return None;
    }
    Some((*function, module, args.named_child_count() as u32))
}
