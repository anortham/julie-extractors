//! Declared return types of the file's methods, read from the annotations a
//! Ruby type checker enforces: a Sorbet `sig { returns(T) }` or an RBS inline
//! `#: (..) -> T` comment directly above the `def`. YARD `@return` tags are
//! unchecked documentation and are not read.

use super::helpers::declared_name;
use crate::base::BaseExtractor;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

/// What `self` is at a point in the file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct SelfScope {
    /// The enclosing class or module name; `None` at the top level.
    owner: Option<String>,
    owner_is_class: bool,
    /// `self` is the class or module object, not an instance of it.
    singleton: bool,
}

impl SelfScope {
    /// The class object `Name` as a call receiver.
    pub(super) fn class_object(name: String) -> Self {
        Self {
            owner: Some(name),
            owner_is_class: true,
            singleton: true,
        }
    }

    fn key(&self) -> (Option<String>, bool) {
        (self.owner.clone(), self.singleton)
    }

    /// RBS `self`, Sorbet `T.self_type`: an instance of the enclosing class.
    fn self_type(&self) -> Option<String> {
        self.owner
            .clone()
            .filter(|_| self.owner_is_class && !self.singleton)
    }

    /// RBS `instance`, Sorbet `T.attached_class`: an instance of the enclosing class.
    fn instance_type(&self) -> Option<String> {
        self.owner.clone().filter(|_| self.owner_is_class)
    }
}

/// A declared type reduced to its base name, with the written text.
#[derive(Debug, Clone)]
pub(super) struct DeclaredType {
    pub(super) name: String,
    pub(super) declared: String,
}

/// Declared return types of the file's methods, by method name.
#[derive(Debug, Default)]
pub(super) struct ReturnTypeIndex {
    entries: HashMap<String, Vec<ReturnEntry>>,
    generics: HashSet<String>,
}

#[derive(Debug)]
struct ReturnEntry {
    /// `None` for a `def` whose `self` the file does not settle, such as one
    /// in a block. Such an entry never matches a lookup's owner, so it blocks
    /// every lookup of its name.
    key: Option<(Option<String>, bool)>,
    return_type: Option<DeclaredType>,
}

impl ReturnTypeIndex {
    pub(super) fn build(base: &BaseExtractor, root: Node) -> Self {
        let mut definitions = Vec::new();
        let mut index = Self::default();
        collect(base, root, 0, &mut definitions, &mut index.generics);
        for definition in definitions {
            let Some(name) = definition.child_by_field_name("name") else {
                continue;
            };
            let scope = definition_scope(base, definition);
            let return_type = scope
                .as_ref()
                .and_then(|scope| index.annotated_return(base, definition, scope));
            index
                .entries
                .entry(base.get_node_text(&name))
                .or_default()
                .push(ReturnEntry {
                    key: scope.map(|scope| scope.key()),
                    return_type,
                });
        }
        index
    }

    /// The return type every same-named method on this `self` agrees on.
    pub(super) fn lookup(&self, name: &str, scope: &SelfScope) -> Option<DeclaredType> {
        let key = scope.key();
        let mut candidates = self
            .entries
            .get(name)?
            .iter()
            .filter(|entry| entry.key.as_ref().is_none_or(|entry_key| *entry_key == key))
            .map(|entry| entry.return_type.as_ref());
        let first = candidates.next()??;
        candidates
            .all(|candidate| candidate.is_some_and(|candidate| candidate.name == first.name))
            .then(|| first.clone())
    }

    fn annotated_return(
        &self,
        base: &BaseExtractor,
        definition: Node,
        scope: &SelfScope,
    ) -> Option<DeclaredType> {
        let anchor = visibility_call_around(definition).unwrap_or(definition);
        let mut previous = previous_named(anchor);
        while let Some(node) = previous.filter(|node| node.kind() == "comment") {
            previous = previous_named(node);
        }
        if let Some(sig) = previous.filter(|node| is_sig(base, *node)) {
            return self.sig_return(base, sig, scope);
        }
        self.rbs_return(base, anchor, scope)
    }

    fn sig_return(
        &self,
        base: &BaseExtractor,
        sig: Node,
        scope: &SelfScope,
    ) -> Option<DeclaredType> {
        let body = sig
            .child_by_field_name("block")?
            .child_by_field_name("body")?;
        let mut chain = body
            .named_children(&mut body.walk())
            .filter(|node| node.kind() != "comment")
            .last()?;
        while chain.kind() == "call" {
            match base
                .get_node_text(&chain.child_by_field_name("method")?)
                .as_str()
            {
                "returns" => {
                    let arguments = chain.child_by_field_name("arguments")?;
                    let returned = arguments.named_child(0)?;
                    return self.sorbet_type(base, returned, scope);
                }
                "void" => return None,
                _ => chain = chain.child_by_field_name("receiver")?,
            }
        }
        None
    }

    /// The return type of an RBS inline method type in the comment block
    /// directly above `anchor`. Each `#:` line is one overload and `#|` lines
    /// continue it; every overload must return the same type.
    fn rbs_return(
        &self,
        base: &BaseExtractor,
        anchor: Node,
        scope: &SelfScope,
    ) -> Option<DeclaredType> {
        let mut comments = Vec::new();
        let mut next_row = anchor.start_position().row;
        let mut previous = previous_named(anchor);
        while let Some(comment) = previous
            .filter(|node| node.kind() == "comment" && node.end_position().row + 1 == next_row)
        {
            comments.push(base.get_node_text(&comment));
            next_row = comment.start_position().row;
            previous = previous_named(comment);
        }
        comments.reverse();
        let mut overloads: Vec<String> = Vec::new();
        for comment in &comments {
            let comment = comment.trim();
            if let Some(method_type) = comment.strip_prefix("#:") {
                overloads.push(method_type.to_string());
            } else if let Some(continued) = comment.strip_prefix("#|") {
                overloads.last_mut()?.push_str(continued);
            } else if let Some(returned) = comment
                .strip_prefix('#')
                .map(str::trim_start)
                .and_then(|text| text.strip_prefix("@rbs return:"))
            {
                let returned = returned.split(" -- ").next().unwrap_or_default();
                overloads.push(format!("-> {returned}"));
            }
        }
        let mut returns = overloads
            .iter()
            .map(|method_type| self.rbs_type(method_return_text(method_type)?, scope));
        let first = returns.next()??;
        returns
            .all(|other| other.is_some_and(|other| other.name == first.name))
            .then_some(first)
    }

    /// An RBS type's base name: a class name, optionally with type arguments,
    /// `?`, or a `| nil` member. Unions, literals, and keyword types other
    /// than `self` and `instance` have none.
    pub(super) fn rbs_type(&self, text: &str, scope: &SelfScope) -> Option<DeclaredType> {
        let declared = text.trim();
        let name = self.rbs_base_name(declared, scope, 0)?;
        Some(DeclaredType {
            name,
            declared: declared.to_string(),
        })
    }

    fn rbs_base_name(&self, text: &str, scope: &SelfScope, depth: u32) -> Option<String> {
        let child_depth = child_tree_depth(depth)?;
        let text = text.trim();
        let members = split_top_level(text, '|');
        if members.len() > 1 {
            let mut non_nil = members.into_iter().filter(|member| member.trim() != "nil");
            let only = non_nil.next()?;
            return non_nil
                .next()
                .is_none()
                .then(|| self.rbs_base_name(only, scope, child_depth))?;
        }
        if let Some(inner) = text.strip_suffix('?') {
            return self.rbs_base_name(inner, scope, child_depth);
        }
        if let Some(inner) = text
            .strip_prefix('(')
            .and_then(|rest| rest.strip_suffix(')'))
            && split_top_level(inner, ')').len() == 1
        {
            return self.rbs_base_name(inner, scope, child_depth);
        }
        match text {
            "self" => return scope.self_type(),
            "instance" => return scope.instance_type(),
            _ => {}
        }
        let path_end = text
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == ':'))
            .unwrap_or(text.len());
        let (path, arguments) = text.split_at(path_end);
        let arguments_ok =
            arguments.is_empty() || (arguments.starts_with('[') && arguments.ends_with(']'));
        (arguments_ok && split_top_level(arguments, '&').len() == 1)
            .then(|| self.class_path(path))?
    }

    /// A Sorbet type expression's base name: a class name, `T.nilable(X)`,
    /// `T.self_type`, `T.attached_class`, or a user generic `Box[X]`.
    pub(super) fn sorbet_type(
        &self,
        base: &BaseExtractor,
        node: Node,
        scope: &SelfScope,
    ) -> Option<DeclaredType> {
        let name = self.sorbet_base_name(base, node, scope, 0)?;
        Some(DeclaredType {
            name,
            declared: base.get_node_text(&node),
        })
    }

    fn sorbet_base_name(
        &self,
        base: &BaseExtractor,
        node: Node,
        scope: &SelfScope,
        depth: u32,
    ) -> Option<String> {
        let child_depth = child_tree_depth(depth)?;
        match node.kind() {
            "constant" | "scope_resolution" => {
                let path = base.get_node_text(&node);
                if path == "T::Boolean" {
                    return None;
                }
                self.class_path(&path)
            }
            "element_reference" => {
                let object = node.child_by_field_name("object")?;
                if base.get_node_text(&object).starts_with("T::") {
                    return None;
                }
                self.sorbet_base_name(base, object, scope, child_depth)
            }
            "call" => {
                let receiver = node.child_by_field_name("receiver")?;
                if receiver.kind() != "constant" || base.get_node_text(&receiver) != "T" {
                    return None;
                }
                match base
                    .get_node_text(&node.child_by_field_name("method")?)
                    .as_str()
                {
                    "nilable" => {
                        let arguments = node.child_by_field_name("arguments")?;
                        self.sorbet_base_name(base, arguments.named_child(0)?, scope, child_depth)
                    }
                    "self_type" => scope.self_type(),
                    "attached_class" => scope.instance_type(),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn class_path(&self, path: &str) -> Option<String> {
        let path = path.strip_prefix("::").unwrap_or(path);
        let is_constant_path = path.split("::").all(|segment| {
            segment.starts_with(|c: char| c.is_ascii_uppercase())
                && segment
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_')
        });
        (is_constant_path && !self.generics.contains(path)).then(|| path.to_string())
    }
}

/// What `self` is at `node`, or `None` where the file does not settle it:
/// in a `class << self` body, or in a block that rebinds `self`.
pub(super) fn self_scope(base: &BaseExtractor, node: Node) -> Option<SelfScope> {
    let mut current = node.parent();
    while let Some(ancestor) = current {
        match ancestor.kind() {
            "method" | "singleton_method" => return definition_scope(base, ancestor),
            "class" | "module" => {
                return Some(SelfScope {
                    owner: declared_name(base, ancestor),
                    owner_is_class: ancestor.kind() == "class",
                    singleton: true,
                });
            }
            "singleton_class" => return None,
            "block" | "do_block" if rebinds_self(base, ancestor) => return None,
            _ => {}
        }
        current = ancestor.parent();
    }
    Some(SelfScope::default())
}

/// What `self` is inside the body of a `def`.
fn definition_scope(base: &BaseExtractor, definition: Node) -> Option<SelfScope> {
    let mut singleton = definition.kind() == "singleton_method";
    if singleton && definition.child_by_field_name("object")?.kind() != "self" {
        return None;
    }
    let mut current = definition.parent();
    while let Some(ancestor) = current {
        match ancestor.kind() {
            "class" | "module" => {
                return Some(SelfScope {
                    owner: declared_name(base, ancestor),
                    owner_is_class: ancestor.kind() == "class",
                    singleton,
                });
            }
            "singleton_class"
                if !singleton && ancestor.child_by_field_name("value")?.kind() == "self" =>
            {
                singleton = true;
            }
            "singleton_class" | "method" | "singleton_method" | "block" | "do_block" | "lambda" => {
                return None;
            }
            _ => {}
        }
        current = ancestor.parent();
    }
    (!singleton).then(SelfScope::default)
}

fn rebinds_self(base: &BaseExtractor, block: Node) -> bool {
    block
        .parent()
        .filter(|call| call.kind() == "call")
        .and_then(|call| call.child_by_field_name("method"))
        .is_some_and(|method| {
            matches!(
                base.get_node_text(&method).as_str(),
                "instance_eval"
                    | "instance_exec"
                    | "class_eval"
                    | "class_exec"
                    | "module_eval"
                    | "module_exec"
                    | "new"
                    | "define"
            )
        })
}

/// The named node before `node`. Comments above the first statement of a
/// class or module body are children of the class node, not of its body.
fn previous_named(node: Node) -> Option<Node> {
    node.prev_named_sibling().or_else(|| {
        node.parent()
            .filter(|parent| parent.kind() == "body_statement")?
            .prev_named_sibling()
    })
}

/// The `private def x` style call a `def` is the argument of.
fn visibility_call_around(definition: Node) -> Option<Node> {
    let arguments = definition
        .parent()
        .filter(|node| node.kind() == "argument_list")?;
    arguments.parent().filter(|node| node.kind() == "call")
}

fn is_sig(base: &BaseExtractor, node: Node) -> bool {
    node.kind() == "call"
        && node.child_by_field_name("block").is_some()
        && node
            .child_by_field_name("method")
            .is_some_and(|method| base.get_node_text(&method) == "sig")
}

/// The return type text of an RBS method type `[T] (params) { block } -> R`.
fn method_return_text(method_type: &str) -> Option<&str> {
    let mut depth = 0usize;
    let bytes = method_type.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.checked_sub(1)?,
            b'-' if depth == 0 && bytes.get(index + 1) == Some(&b'>') => {
                return Some(&method_type[index + 2..]);
            }
            _ => {}
        }
    }
    None
}

/// `text` split on `separator` outside brackets.
fn split_top_level(text: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (index, c) in text.char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            _ if c == separator && depth == 0 => {
                parts.push(&text[start..index]);
                start = index + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts
}

fn collect<'tree>(
    base: &BaseExtractor,
    node: Node<'tree>,
    depth: u32,
    definitions: &mut Vec<Node<'tree>>,
    generics: &mut HashSet<String>,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    match node.kind() {
        "method" | "singleton_method" => definitions.push(node),
        "comment" => generics.extend(comment_generics(&base.get_node_text(&node))),
        "assignment" => generics.extend(sorbet_type_member(base, node)),
        _ => {}
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    for child in node.named_children(&mut node.walk()) {
        collect(base, child, child_depth, definitions, generics);
    }
}

/// Type parameter names an RBS comment declares: `#: [T, U] ...` or
/// `# @rbs generic T`.
fn comment_generics(comment: &str) -> Vec<String> {
    let text = comment.trim();
    let declared = if let Some(rest) = text.strip_prefix("#:") {
        rest.trim_start()
            .strip_prefix('[')
            .and_then(|rest| rest.split_once(']'))
            .map(|(parameters, _)| parameters.split(',').collect::<Vec<_>>())
            .unwrap_or_default()
    } else if let Some(rest) = text
        .strip_prefix('#')
        .map(str::trim_start)
        .and_then(|rest| rest.strip_prefix("@rbs generic"))
    {
        vec![rest]
    } else {
        Vec::new()
    };
    declared
        .into_iter()
        .filter_map(|parameter| {
            parameter
                .split_whitespace()
                .find(|word| !matches!(*word, "in" | "out" | "unchecked"))
        })
        .map(|word| {
            word.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .next()
                .unwrap_or_default()
                .to_string()
        })
        .filter(|name| !name.is_empty())
        .collect()
}

/// The constant a Sorbet `Elem = type_member` or `type_template` declares.
fn sorbet_type_member(base: &BaseExtractor, assignment: Node) -> Option<String> {
    let left = assignment.child_by_field_name("left")?;
    let right = assignment.child_by_field_name("right")?;
    let method = match right.kind() {
        "identifier" => right,
        "call" if right.child_by_field_name("receiver").is_none() => {
            right.child_by_field_name("method")?
        }
        _ => return None,
    };
    let is_type_member = matches!(
        base.get_node_text(&method).as_str(),
        "type_member" | "type_template"
    );
    (left.kind() == "constant" && is_type_member).then(|| base.get_node_text(&left))
}
