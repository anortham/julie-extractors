use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

pub(super) const TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &[],
    generic_open: &['['],
};

pub(super) fn record_statement_type_facts(
    base: &mut BaseExtractor,
    symbol_id: &str,
    statement: Node,
    same_file_types: &SameFileTypes,
) {
    if let Some(type_node) = statement.child_by_field_name("type")
        && type_node.kind() == "type"
    {
        record_declared_type_node(base, symbol_id, type_node);
        return;
    }
    let Some(value) = statement.child_by_field_name("value") else {
        return;
    };
    if let Some(cast_type) = cast_type(base, value) {
        base.record_declared_type_fact(symbol_id, &cast_type, &TYPE_NAME_RULES, true);
        return;
    }
    let scope = InitializerScope {
        base,
        types: same_file_types,
        owner: enclosing_class_name(base, statement),
    };
    if let Some(inferred) = scope.type_of(value, 0) {
        base.record_declared_type_fact(symbol_id, &inferred, &TYPE_NAME_RULES, true);
    }
}

/// The target type of an `expr as T` initializer.
pub(super) fn cast_type(base: &BaseExtractor, value: Node) -> Option<String> {
    let operand = super::identifiers::type_test_operand(value)?;
    let is_cast = value
        .child_by_field_name("op")
        .is_some_and(|operator| operator.kind() == "as");
    is_cast.then(|| base.get_node_text(&operand))
}

pub(super) fn record_declared_type_node(
    base: &mut BaseExtractor,
    symbol_id: &str,
    type_node: Node,
) {
    if type_node.kind() != "type" {
        return;
    }
    let declared = base.get_node_text(&type_node);
    base.record_declared_type_fact(symbol_id, &declared, &TYPE_NAME_RULES, false);
}

/// What initializer inference needs to know about the file: its class names
/// and the declared return types of its functions, keyed by owning class.
#[derive(Debug, Default)]
pub(super) struct SameFileTypes {
    class_names: HashSet<String>,
    script_class: Option<String>,
    /// `(owner, function name)` to each same-named declaration's return type.
    /// The owner is the inner class name, or `None` for the script class.
    return_types: HashMap<(Option<String>, String), Vec<Option<String>>>,
}

impl SameFileTypes {
    pub(super) fn build(base: &BaseExtractor, root: Node) -> Self {
        let mut types = Self::default();
        types.collect(base, root, None, 0);
        types
    }

    fn collect(&mut self, base: &BaseExtractor, node: Node, owner: Option<&str>, depth: u32) {
        if !should_visit_tree_depth(depth) {
            return;
        }
        let name = node
            .child_by_field_name("name")
            .map(|name| base.get_node_text(&name));
        match (node.kind(), name) {
            ("class_name_statement", Some(name)) => {
                self.class_names.insert(name.clone());
                if owner.is_none() {
                    self.script_class = Some(name);
                }
            }
            ("class_definition", Some(name)) => {
                self.class_names.insert(name.clone());
                self.collect_children(base, node, Some(&name), depth);
            }
            ("class_definition", None) => {}
            ("function_definition", Some(name)) => {
                let return_type = node
                    .child_by_field_name("return_type")
                    .map(|return_type| base.get_node_text(&return_type))
                    .filter(|return_type| return_type != "void");
                self.return_types
                    .entry((owner.map(str::to_string), name))
                    .or_default()
                    .push(return_type);
            }
            _ => self.collect_children(base, node, owner, depth),
        }
    }

    fn collect_children(
        &mut self,
        base: &BaseExtractor,
        node: Node,
        owner: Option<&str>,
        depth: u32,
    ) {
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect(base, child, owner, child_depth);
        }
    }

    /// The return type every same-named function of this owner agrees on.
    fn return_type(&self, owner: Option<&str>, name: &str) -> Option<String> {
        let mut candidates = self
            .return_types
            .get(&(owner.map(str::to_string), name.to_string()))?
            .iter();
        let first = candidates.next()?.as_ref()?;
        candidates
            .all(|candidate| candidate.as_ref() == Some(first))
            .then(|| first.clone())
    }

    fn declares(&self, owner: Option<&str>, name: &str) -> bool {
        self.return_types
            .contains_key(&(owner.map(str::to_string), name.to_string()))
    }

    /// The owner key of a same-file class named by a bare identifier.
    fn class_owner(&self, name: &str) -> Option<Option<String>> {
        if self.script_class.as_deref() == Some(name) {
            Some(None)
        } else if self.class_names.contains(name) {
            Some(Some(name.to_string()))
        } else {
            None
        }
    }
}

fn enclosing_class_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut current = node.parent();
    while let Some(ancestor) = current {
        if ancestor.kind() == "class_definition" {
            return ancestor
                .child_by_field_name("name")
                .map(|name| base.get_node_text(&name));
        }
        current = ancestor.parent();
    }
    None
}

/// Resolves the type an initializer produces from same-file declarations:
/// `Foo.new()`, a bare or `self.` call to a function of the enclosing class,
/// or `Foo.func()` on a same-file class, each with a declared return type.
/// `await` and parentheses pass the type through, except that awaiting a
/// `Signal` yields the signal's arguments, so it records nothing.
struct InitializerScope<'a> {
    base: &'a BaseExtractor,
    types: &'a SameFileTypes,
    owner: Option<String>,
}

impl InitializerScope<'_> {
    fn type_of(&self, value: Node, depth: u32) -> Option<String> {
        if !should_visit_tree_depth(depth) {
            return None;
        }
        match value.kind() {
            "parenthesized_expression" => {
                self.type_of(value.named_child(0)?, child_tree_depth(depth)?)
            }
            "await_expression" => self
                .type_of(value.named_child(0)?, child_tree_depth(depth)?)
                .filter(|awaited| awaited != "Signal"),
            "call" => {
                let callee = value.named_child(0)?;
                if callee.kind() != "identifier" {
                    return None;
                }
                self.types
                    .return_type(self.owner.as_deref(), &self.base.get_node_text(&callee))
            }
            "attribute" => self.attribute_call(value),
            _ => None,
        }
    }

    fn attribute_call(&self, value: Node) -> Option<String> {
        let mut cursor = value.walk();
        let children: Vec<Node> = value.named_children(&mut cursor).collect();
        let [receiver, call] = children.as_slice() else {
            return None;
        };
        if receiver.kind() != "identifier" || call.kind() != "attribute_call" {
            return None;
        }
        let method = call
            .named_child(0)
            .filter(|method| method.kind() == "identifier")
            .map(|method| self.base.get_node_text(&method))?;
        let receiver = self.base.get_node_text(receiver);
        if receiver == "self" {
            return self.types.return_type(self.owner.as_deref(), &method);
        }
        let owner = self.types.class_owner(&receiver)?;
        if self.types.declares(owner.as_deref(), &method) {
            self.types.return_type(owner.as_deref(), &method)
        } else {
            (method == "new").then_some(receiver)
        }
    }
}
