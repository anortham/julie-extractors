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
    let Some(owner) = enclosing_class_path(base, statement) else {
        return;
    };
    let scope = InitializerScope {
        base,
        types: same_file_types,
        owner,
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

/// The chain of inner-class names from the script down to a class. The
/// script class itself is the empty path.
type ClassPath = Vec<String>;

/// What initializer inference needs to know about the file: its classes and
/// the declared return types of its functions, keyed by owning class path.
#[derive(Debug, Default)]
pub(super) struct SameFileTypes {
    classes: HashSet<ClassPath>,
    script_class: Option<String>,
    /// `(owner path, function name)` to each same-named declaration's return
    /// type. More than one entry only comes from duplicate declarations.
    return_types: HashMap<(ClassPath, String), Vec<Option<String>>>,
}

impl SameFileTypes {
    pub(super) fn build(base: &BaseExtractor, root: Node) -> Self {
        let mut types = Self::default();
        types.collect(base, root, &[], 0);
        types
    }

    fn collect(&mut self, base: &BaseExtractor, node: Node, owner: &[String], depth: u32) {
        if !should_visit_tree_depth(depth) {
            return;
        }
        let name = node
            .child_by_field_name("name")
            .map(|name| base.get_node_text(&name));
        match (node.kind(), name) {
            ("class_name_statement", Some(name)) => {
                if owner.is_empty() {
                    self.script_class = Some(name);
                }
            }
            ("class_definition", Some(name)) => {
                let mut path = owner.to_vec();
                path.push(name);
                self.classes.insert(path.clone());
                self.collect_children(base, node, &path, depth);
            }
            ("class_definition", None) => {}
            ("function_definition", Some(name)) => {
                let return_type = node
                    .child_by_field_name("return_type")
                    .map(|return_type| base.get_node_text(&return_type))
                    .filter(|return_type| return_type != "void");
                self.return_types
                    .entry((owner.to_vec(), name))
                    .or_default()
                    .push(return_type);
            }
            _ => self.collect_children(base, node, owner, depth),
        }
    }

    fn collect_children(&mut self, base: &BaseExtractor, node: Node, owner: &[String], depth: u32) {
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect(base, child, owner, child_depth);
        }
    }

    /// The return type every same-named function of this owner agrees on.
    fn return_type(&self, owner: &[String], name: &str) -> Option<String> {
        let mut candidates = self
            .return_types
            .get(&(owner.to_vec(), name.to_string()))?
            .iter();
        let first = candidates.next()?.as_ref()?;
        candidates
            .all(|candidate| candidate.as_ref() == Some(first))
            .then(|| first.clone())
    }

    fn declares(&self, owner: &[String], name: &str) -> bool {
        self.return_types
            .contains_key(&(owner.to_vec(), name.to_string()))
    }

    fn declares_class_named(&self, name: &str) -> bool {
        self.script_class.as_deref() == Some(name)
            || self
                .classes
                .iter()
                .any(|path| path.last().is_some_and(|last| last == name))
    }

    /// Every same-file class that the bare name `name` can mean inside
    /// `scope`: an inner class of `scope` or of any class around it, or the
    /// script's `class_name`.
    fn visible_classes(&self, name: &str, scope: &[String]) -> Vec<ClassPath> {
        let mut found: Vec<ClassPath> = (0..=scope.len())
            .map(|depth| {
                let mut path = scope[..depth].to_vec();
                path.push(name.to_string());
                path
            })
            .filter(|path| self.classes.contains(path))
            .collect();
        if self.script_class.as_deref() == Some(name) {
            found.push(ClassPath::new());
        }
        found
    }

    /// Whether every class name in `type_text`, written in `callee`, names the
    /// same thing when read in `caller`. A name that is ambiguous in either
    /// scope fails.
    fn means_the_same(&self, type_text: &str, callee: &[String], caller: &[String]) -> bool {
        callee == caller
            || type_text
                .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.'))
                .filter_map(|name| name.split('.').next().filter(|head| !head.is_empty()))
                .all(|name| {
                    let written = self.visible_classes(name, callee);
                    written.len() <= 1 && written == self.visible_classes(name, caller)
                })
    }
}

/// The class path of the class that holds `node`, or `None` when a class
/// around it has no name.
fn enclosing_class_path(base: &BaseExtractor, node: Node) -> Option<ClassPath> {
    let mut path = ClassPath::new();
    let mut current = node.parent();
    while let Some(ancestor) = current {
        if ancestor.kind() == "class_definition" {
            path.push(base.get_node_text(&ancestor.child_by_field_name("name")?));
        }
        current = ancestor.parent();
    }
    path.reverse();
    Some(path)
}

/// Resolves the type an initializer produces from same-file declarations:
/// `Foo.new()`, a bare or `self.` call to a function of the enclosing class,
/// or `Foo.func()` on the one same-file class that `Foo` can mean here, each
/// with a declared return type. A return type from another class is kept only
/// when its class names mean the same classes at the call site. `await` and
/// parentheses pass the type through, except that awaiting a `Signal` yields
/// the signal's arguments, so it records nothing.
struct InitializerScope<'a> {
    base: &'a BaseExtractor,
    types: &'a SameFileTypes,
    owner: ClassPath,
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
                    .return_type(&self.owner, &self.base.get_node_text(&callee))
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
            return self.types.return_type(&self.owner, &method);
        }
        if let [target] = self
            .types
            .visible_classes(&receiver, &self.owner)
            .as_slice()
            && self.types.declares(target, &method)
        {
            return self
                .types
                .return_type(target, &method)
                .filter(|returned| self.types.means_the_same(returned, target, &self.owner));
        }
        (method == "new" && self.types.declares_class_named(&receiver)).then_some(receiver)
    }
}
