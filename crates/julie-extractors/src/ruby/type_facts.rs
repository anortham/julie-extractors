use super::locals::LocalBindings;
use super::return_types::{DeclaredType, ReturnTypeIndex, SelfScope, self_scope};
use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashSet;
use tree_sitter::Node;

pub(super) const TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &[],
    generic_open: &[],
};

/// Per-file state that assignment type inference reads.
pub(super) struct InitializerContext<'a> {
    pub(super) same_file_class_names: &'a HashSet<String>,
    pub(super) return_types: &'a ReturnTypeIndex,
    pub(super) locals: &'a mut LocalBindings,
}

/// Record the type of an assigned variable or field. A written type wins: a
/// trailing RBS `#: T` or `#: as T` comment, or Sorbet `T.let(x, T)` /
/// `T.cast(x, T)`. Otherwise the type is inferred (`is_inferred=true`) from
/// same-file `Foo.new`, or from a call to a same-file method whose Sorbet or
/// RBS annotation declares its return type: a receiverless or `self.` call on
/// the current `self`, or `Foo.m` on a same-file class or module. `T.must(x)`
/// and a trailing `#: as !nil` keep the type of `x`.
pub(super) fn record_assignment_type(
    base: &mut BaseExtractor,
    symbol_id: &str,
    assignment: Node,
    value: Node,
    context: InitializerContext<'_>,
) {
    let scope =
        self_scope(base, assignment).filter(|_| !context.return_types.self_rebound(assignment));
    let written = match trailing_annotation(base, assignment) {
        TrailingAnnotation::Type(text) => Some(
            context
                .return_types
                .rbs_type(&text, &scope.clone().unwrap_or_default()),
        ),
        TrailingAnnotation::Unreadable => Some(None),
        TrailingAnnotation::NotNil | TrailingAnnotation::Absent => {
            sorbet_written_type(base, value, context.return_types, scope.as_ref())
        }
    };
    let (declared, is_inferred) = match written {
        Some(declared) => (declared, false),
        None => {
            let mut inference = Inference {
                base,
                context,
                scope: scope.as_ref(),
            };
            (inference.type_of(value, 0), true)
        }
    };
    if let Some(DeclaredType { name, declared }) = declared {
        base.record_declared_type_fact_with_declared(
            symbol_id,
            &name,
            &declared,
            &TYPE_NAME_RULES,
            is_inferred,
        );
    }
}

enum TrailingAnnotation {
    Absent,
    Type(String),
    NotNil,
    Unreadable,
}

/// Whether a trailing RBS comment writes a type for the assignment, so its
/// literal right-hand side does not state the type.
pub(super) fn has_trailing_written_type(base: &BaseExtractor, assignment: Node) -> bool {
    matches!(
        trailing_annotation(base, assignment),
        TrailingAnnotation::Type(_) | TrailingAnnotation::Unreadable
    )
}

/// The RBS comment after an assignment on its last line.
fn trailing_annotation(base: &BaseExtractor, assignment: Node) -> TrailingAnnotation {
    let rest = &base.content[assignment.end_byte()..];
    let line = rest.lines().next().unwrap_or_default().trim_start();
    let Some(annotation) = line.strip_prefix("#:").map(str::trim) else {
        return if line.contains("#:") {
            TrailingAnnotation::Unreadable
        } else {
            TrailingAnnotation::Absent
        };
    };
    match annotation.strip_prefix("as ").map(str::trim) {
        Some("!nil") => TrailingAnnotation::NotNil,
        Some(cast) => TrailingAnnotation::Type(cast.to_string()),
        None => TrailingAnnotation::Type(annotation.to_string()),
    }
}

/// The type a Sorbet `T.let(x, T)` or `T.cast(x, T)` states; `Some(None)`
/// when it states one with no base name.
fn sorbet_written_type(
    base: &BaseExtractor,
    value: Node,
    return_types: &ReturnTypeIndex,
    scope: Option<&SelfScope>,
) -> Option<Option<DeclaredType>> {
    let method = sorbet_t_call(base, value)?;
    if !matches!(method.as_str(), "let" | "cast") {
        return None;
    }
    let written = value.child_by_field_name("arguments")?.named_child(1)?;
    Some(return_types.sorbet_type(base, written, &scope.cloned().unwrap_or_default()))
}

/// The method name of a `T.method(..)` call.
fn sorbet_t_call(base: &BaseExtractor, value: Node) -> Option<String> {
    if value.kind() != "call" {
        return None;
    }
    let receiver = value.child_by_field_name("receiver")?;
    if receiver.kind() != "constant" || base.get_node_text(&receiver) != "T" {
        return None;
    }
    Some(base.get_node_text(&value.child_by_field_name("method")?))
}

struct Inference<'a, 'b> {
    base: &'a BaseExtractor,
    context: InitializerContext<'b>,
    scope: Option<&'a SelfScope>,
}

impl Inference<'_, '_> {
    fn type_of(&mut self, value: Node, depth: u32) -> Option<DeclaredType> {
        let child_depth = child_tree_depth(depth)?;
        match value.kind() {
            "identifier" => {
                if !self
                    .context
                    .locals
                    .is_method_call(&self.base.content, value)
                {
                    return None;
                }
                self.on_self(&self.base.get_node_text(&value))
            }
            "call" => {
                if sorbet_t_call(self.base, value).as_deref() == Some("must") {
                    let unwrapped = value.child_by_field_name("arguments")?.named_child(0)?;
                    return self.type_of(unwrapped, child_depth);
                }
                let method = self
                    .base
                    .get_node_text(&value.child_by_field_name("method")?);
                let Some(receiver) = value.child_by_field_name("receiver") else {
                    return self.on_self(&method);
                };
                match receiver.kind() {
                    "self" => self.on_self(&method),
                    "constant" => self.on_class(receiver, &method),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn on_self(&self, method: &str) -> Option<DeclaredType> {
        self.context.return_types.lookup(method, self.scope?)
    }

    fn on_class(&self, receiver: Node, method: &str) -> Option<DeclaredType> {
        let owner = self.base.get_node_text(&receiver);
        if method == "new" {
            return self
                .context
                .same_file_class_names
                .contains(&owner)
                .then(|| DeclaredType {
                    name: owner.clone(),
                    declared: owner,
                });
        }
        let return_types = self.context.return_types;
        let class_object = return_types.constant_receiver(self.base, receiver, &owner)?;
        return_types.lookup(method, &class_object)
    }
}

pub(super) fn collect_same_file_class_names(base: &BaseExtractor, root: Node) -> HashSet<String> {
    let mut names = HashSet::new();
    collect_class_names(base, root, 0, &mut names);
    names
}

pub(super) fn self_receiver_type(base: &BaseExtractor, call_node: Node) -> Option<String> {
    let receiver = call_node.child_by_field_name("receiver")?;
    if receiver.kind() != "self" {
        return None;
    }
    enclosing_type_name(base, call_node)
}

fn collect_class_names(base: &BaseExtractor, node: Node, depth: u32, names: &mut HashSet<String>) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if node.kind() == "class"
        && let Some(name) = super::helpers::declared_name(base, node)
    {
        names.insert(name);
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_class_names(base, child, child_depth, names);
    }
}

fn enclosing_type_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        if matches!(candidate.kind(), "class" | "module") {
            return super::helpers::declared_name(base, candidate);
        }
        current = candidate.parent();
    }
    None
}
