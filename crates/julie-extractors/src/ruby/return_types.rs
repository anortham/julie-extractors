//! Declared return types of the file's methods, read from the annotations a
//! Ruby type checker enforces: a Sorbet `sig { returns(T) }` or an RBS inline
//! `#: (..) -> T` comment directly above the `def`. YARD `@return` tags are
//! unchecked documentation and are not read.

use crate::base::BaseExtractor;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

/// A `self` by owner path and whether it is the class or module object.
type SelfKey = (Option<String>, bool);

/// What `self` is at a point in the file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct SelfScope {
    /// The full lexical path of the enclosing class or module, such as
    /// `A::Item`; `None` at the top level.
    owner: Option<String>,
    owner_is_class: bool,
    /// `self` is the class or module object, not an instance of it.
    pub(super) singleton: bool,
}

impl SelfScope {
    fn of_owner(base: &BaseExtractor, owner: Node, singleton: bool) -> Option<Self> {
        Some(Self {
            owner: Some(owner_path(base, owner)?),
            owner_is_class: owner.kind() == "class",
            singleton,
        })
    }

    fn key(&self) -> SelfKey {
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
    /// Names of type parameters and of Sorbet type aliases and members. A
    /// written path whose last segment is one of them names no class.
    generics: HashSet<String>,
    /// Full paths of the file's classes and modules; `true` for a class.
    owners: HashMap<String, bool>,
    /// Full paths of the file's other constants, such as `Holder::Widget` for
    /// `Widget = Other` in `class Holder`.
    constants: HashSet<String>,
    /// Owners, by `self` level, whose ancestors reach past `Object`: a
    /// superclass, `include` or `prepend`, or `extend` for the class level.
    /// Ruby looks a constant up in them before the top level.
    has_ancestors: HashSet<(String, bool)>,
    /// Owners, by `self` level, that `prepend` a module, whose methods then
    /// run before the owner's own.
    prepends: HashSet<SelfKey>,
    /// Last path segments of constants that `Foo.include`, `Foo.extend` or
    /// `Foo.prepend` give ancestors, and of those that `Foo.prepend` targets.
    named_ancestors: HashSet<String>,
    named_prepends: HashSet<String>,
    /// A `prepend` on a receiver the file does not name, such as
    /// `base.prepend(M)`, can reach any class.
    prepend_anywhere: bool,
    /// Owners that define methods under names the file does not spell out,
    /// such as `define_method(name)`.
    dynamic_owners: HashSet<Option<String>>,
    /// Methods that `alias`, `attr_reader`, `define_method` and similar
    /// statements define, with the `self` they define them on.
    redefinitions: Vec<(String, Option<SelfKey>)>,
    /// Start bytes of `T.bind(self, ..)` calls and Steep `# @type self:`
    /// comments, which rebind `self` for the rest of their method or block,
    /// and whether each is a Steep `instance:` or `module:` comment, which
    /// rebinds it for the rest of its class or module.
    self_rebinds: Vec<(usize, bool)>,
    /// The `self` levels at which each class uses each `@ivar`.
    ivar_levels: HashMap<(Option<String>, String), HashSet<Option<bool>>>,
}

#[derive(Debug)]
struct ReturnEntry {
    /// `None` for a `def` whose `self` the file does not settle, such as one
    /// in a block. Such an entry never matches a lookup's owner, so it blocks
    /// every lookup of its name.
    key: Option<SelfKey>,
    return_type: Option<DeclaredType>,
}

impl ReturnTypeIndex {
    pub(super) fn build(base: &BaseExtractor, root: Node) -> Self {
        let mut definitions = Vec::new();
        let mut module_bodies = HashMap::new();
        let mut index = Self::default();
        let mut walk = Walk {
            definitions: &mut definitions,
            definition_scopes: HashMap::new(),
        };
        collect(base, root, None, 0, &mut walk, &mut index);
        for (name, key) in std::mem::take(&mut index.redefinitions) {
            index.entries.entry(name).or_default().push(ReturnEntry {
                key,
                return_type: None,
            });
        }
        for definition in definitions {
            let Some(name) = definition.child_by_field_name("name") else {
                continue;
            };
            let scope = definition_scope(base, definition);
            let return_type = scope
                .as_ref()
                .and_then(|scope| index.annotated_return(base, definition, scope));
            let entries = index.entries.entry(base.get_node_text(&name)).or_default();
            if let Some(scope) = scope.as_ref().filter(|scope| {
                !scope.singleton
                    && scope.owner.is_some()
                    && is_module_function(base, definition, &mut module_bodies)
            }) {
                entries.push(ReturnEntry {
                    key: Some((scope.owner.clone(), true)),
                    return_type: return_type.clone(),
                });
            }
            entries.push(ReturnEntry {
                key: scope.map(|scope| scope.key()),
                return_type,
            });
        }
        index
    }

    /// Whether the class that owns the `@ivar` target of `assignment` uses
    /// that name where `self` is an instance and also where `self` is the
    /// class or is not settled. The field then stands for two variables, so
    /// it gets no type.
    pub(super) fn ivar_has_mixed_levels(&self, base: &BaseExtractor, assignment: Node) -> bool {
        assignment
            .child_by_field_name("left")
            .filter(|target| target.kind() == "instance_variable")
            .and_then(|target| self.ivar_levels.get(&ivar_key(base, target)))
            .is_some_and(|levels| levels.len() > 1)
    }

    /// The same-file class or module a constant receiver names at `node`.
    /// Ruby looks in each lexically enclosing class or module, innermost
    /// first, then in the ancestors of the innermost one, then at the top
    /// level. `None` when the first same-file constant found is not a class or
    /// module, or when the ancestors could hold the constant.
    pub(super) fn constant_receiver(
        &self,
        base: &BaseExtractor,
        node: Node,
        name: &str,
    ) -> Option<SelfScope> {
        let mut nesting = Vec::new();
        let mut innermost = None;
        let mut current = node.parent();
        while let Some(ancestor) = current {
            match ancestor.kind() {
                "class" | "module" => {
                    let path = owner_path(base, ancestor)?;
                    innermost.get_or_insert((path.clone(), false));
                    nesting.push(path);
                }
                "singleton_class" if innermost.is_none() => {
                    if ancestor.child_by_field_name("value")?.kind() != "self" {
                        return None;
                    }
                    let owner = ancestor.parent().and_then(enclosing_owner)?;
                    innermost = Some((owner_path(base, owner)?, true));
                }
                _ => {}
            }
            current = ancestor.parent();
        }
        let lexical = nesting.iter().map(|path| format!("{path}::{name}"));
        let top_level = innermost
            .is_none_or(|innermost| {
                !self.has_ancestors.contains(&innermost)
                    && !self.named_ancestors.contains(last_segment(&innermost.0))
            })
            .then(|| name.to_string());
        let path = lexical
            .chain(top_level)
            .find(|path| self.constants.contains(path) || self.owners.contains_key(path))?;
        if self.constants.contains(&path) {
            return None;
        }
        Some(SelfScope {
            owner_is_class: self.owners[&path],
            owner: Some(path),
            singleton: true,
        })
    }

    /// Whether a `T.bind(self, ..)` or a Steep `@type` annotation of `self`
    /// before `node` can change what `self` is at `node`.
    pub(super) fn self_rebound(&self, node: Node) -> bool {
        let start = node.start_byte();
        let reach = |stops: &[&str]| {
            let mut current = node.parent();
            while let Some(ancestor) = current {
                if stops.contains(&ancestor.kind()) {
                    return Some(ancestor.start_byte()..start);
                }
                current = ancestor.parent();
            }
            None
        };
        let body = reach(&[
            "method",
            "singleton_method",
            "class",
            "module",
            "singleton_class",
            "program",
        ]);
        let owner = reach(&["class", "module", "program"]);
        self.self_rebinds.iter().any(|(position, reaches_owner)| {
            let range = if *reaches_owner { &owner } else { &body };
            range.as_ref().is_some_and(|range| range.contains(position))
        })
    }

    /// The return type every same-named method on this `self` agrees on.
    pub(super) fn lookup(&self, name: &str, scope: &SelfScope) -> Option<DeclaredType> {
        let key = scope.key();
        let named_prepend = scope
            .owner
            .as_deref()
            .is_some_and(|owner| self.named_prepends.contains(last_segment(owner)));
        if self.prepend_anywhere
            || named_prepend
            || self.prepends.contains(&key)
            || self.dynamic_owners.contains(&scope.owner)
        {
            return None;
        }
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
        while let Some(comment) = previous.filter(|node| {
            node.kind() == "comment"
                && node.end_position().row + 1 == next_row
                && starts_its_line(base, *node)
        }) {
            comments.push(base.get_node_text(&comment));
            next_row = comment.start_position().row;
            previous = previous_named(comment);
        }
        comments.reverse();
        let mut overloads: Vec<String> = Vec::new();
        let mut rbs_method_type_seen = false;
        for comment in &comments {
            let comment = comment.trim();
            if let Some(method_type) = comment.strip_prefix("#:") {
                overloads.push(method_type.to_string());
                continue;
            }
            if let Some(continued) = comment.strip_prefix("#|") {
                overloads.last_mut()?.push_str(continued);
                continue;
            }
            let text = comment.strip_prefix('#').unwrap_or(comment).trim_start();
            if let Some(returned) = text.strip_prefix("@rbs return:") {
                let returned = returned.split(" -- ").next().unwrap_or_default();
                overloads.push(format!("-> {returned}"));
            } else if let Some(method_type) = rbs_tag_method_type(text) {
                overloads.push(method_type.to_string());
                rbs_method_type_seen = true;
            } else if rbs_method_type_seen && text.starts_with('|') {
                return None;
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
        (is_constant_path && !self.generics.contains(last_segment(path))).then(|| path.to_string())
    }
}

/// What `self` is at `node`, or `None` where the file does not settle it:
/// in a `class << self` body, or in a block that rebinds `self`.
///
/// A block or lambda directly in a class or module body, or at the top level,
/// settles nothing: `define_method`, `before_action`, `scope`, RSpec
/// `describe` and most other DSLs run their block with an instance or another
/// object as `self`.
pub(super) fn self_scope(base: &BaseExtractor, node: Node) -> Option<SelfScope> {
    self_scope_cached(base, node, &mut HashMap::new())
}

/// `self_scope`, with `definition_scopes` caching `definition_scope` by `def`
/// node id.
fn self_scope_cached(
    base: &BaseExtractor,
    node: Node,
    definition_scopes: &mut HashMap<usize, Option<SelfScope>>,
) -> Option<SelfScope> {
    let mut in_block = false;
    let mut current = node.parent();
    while let Some(ancestor) = current {
        match ancestor.kind() {
            "method" | "singleton_method" => {
                return definition_scopes
                    .entry(ancestor.id())
                    .or_insert_with(|| definition_scope(base, ancestor))
                    .clone();
            }
            "class" | "module" | "program" if in_block => return None,
            "class" | "module" => return SelfScope::of_owner(base, ancestor, true),
            "singleton_class" => return None,
            "block" | "do_block" if rebinds_self(base, ancestor) => return None,
            "block" | "do_block" | "lambda" => in_block = true,
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
            "class" | "module" => return SelfScope::of_owner(base, ancestor, singleton),
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
                    | "define_method"
                    | "define_singleton_method"
            )
        })
}

/// The full lexical path of a class or module node: `B::Item` for `class
/// Item` in `module B`. A `::Name` declaration starts from the top level.
fn owner_path(base: &BaseExtractor, owner: Node) -> Option<String> {
    let mut segments = Vec::new();
    let mut current = Some(owner);
    while let Some(node) = current {
        if matches!(node.kind(), "class" | "module") {
            let name = base.get_node_text(&node.child_by_field_name("name")?);
            if let Some(absolute) = name.strip_prefix("::") {
                segments.push(absolute.to_string());
                break;
            }
            segments.push(name);
        }
        current = node.parent();
    }
    segments.reverse();
    Some(segments.join("::"))
}

fn last_segment(path: &str) -> &str {
    path.rsplit("::").next().unwrap_or(path)
}

/// The class or module at or around `node`.
fn enclosing_owner(node: Node) -> Option<Node> {
    let mut current = Some(node);
    while let Some(candidate) = current {
        if matches!(candidate.kind(), "class" | "module") {
            return Some(candidate);
        }
        current = candidate.parent();
    }
    None
}

/// The full path of the class or module lexically around `node`.
fn lexical_owner(base: &BaseExtractor, node: Node) -> Option<String> {
    owner_path(base, enclosing_owner(node.parent()?)?)
}

/// An `@ivar` by the full path of its lexically enclosing class or module.
fn ivar_key(base: &BaseExtractor, ivar: Node) -> (Option<String>, String) {
    (lexical_owner(base, ivar), base.get_node_text(&ivar))
}

/// What a module body says about which of its `def`s are module methods.
#[derive(Default)]
struct ModuleBody {
    extends_self: bool,
    /// The `:name` arguments of `module_function :name` statements.
    named: HashSet<String>,
    /// Start byte of each bare `module_function`, `public`, `private`, or
    /// `protected` statement, in source order, and whether it is
    /// `module_function`.
    markers: Vec<(usize, bool)>,
}

impl ModuleBody {
    fn read(base: &BaseExtractor, body: Node) -> Self {
        let mut read = Self::default();
        for statement in body.named_children(&mut body.walk()) {
            if statement.kind() == "identifier" {
                match base.get_node_text(&statement).as_str() {
                    "module_function" => read.markers.push((statement.start_byte(), true)),
                    "public" | "private" | "protected" => {
                        read.markers.push((statement.start_byte(), false));
                    }
                    _ => {}
                }
            }
            let Some(arguments) = statement.child_by_field_name("arguments") else {
                continue;
            };
            let mut cursor = arguments.walk();
            let mut arguments = arguments.named_children(&mut cursor);
            match call_method(base, statement).as_deref() {
                Some("extend") => {
                    read.extends_self |= arguments.all(|argument| argument.kind() == "self");
                }
                Some("module_function") => {
                    read.named
                        .extend(arguments.map(|argument| base.get_node_text(&argument)));
                }
                _ => {}
            }
        }
        read
    }
}

/// Whether a `def` in a module body is also a module method: it follows a
/// bare `module_function`, is named by `module_function :name` or wrapped as
/// `module_function def`, or the body has `extend self`. `bodies` caches each
/// module body's reading by node id.
fn is_module_function(
    base: &BaseExtractor,
    definition: Node,
    bodies: &mut HashMap<usize, ModuleBody>,
) -> bool {
    let anchor = visibility_call_around(definition).unwrap_or(definition);
    if anchor != definition {
        return call_method(base, anchor).as_deref() == Some("module_function");
    }
    let Some(body) = anchor
        .parent()
        .filter(|node| node.kind() == "body_statement")
    else {
        return false;
    };
    let Some(name) = definition.child_by_field_name("name") else {
        return false;
    };
    let body = bodies
        .entry(body.id())
        .or_insert_with(|| ModuleBody::read(base, body));
    let follows_module_function = body
        .markers
        .iter()
        .rev()
        .find(|(start, _)| *start < anchor.start_byte())
        .is_some_and(|(_, module_function)| *module_function);
    follows_module_function
        || body.extends_self
        || body
            .named
            .contains(&format!(":{}", base.get_node_text(&name)))
}

/// The method name of a receiverless call.
fn call_method(base: &BaseExtractor, node: Node) -> Option<String> {
    if node.kind() != "call" || node.child_by_field_name("receiver").is_some() {
        return None;
    }
    Some(base.get_node_text(&node.child_by_field_name("method")?))
}

/// The method type of an RBS inline `# @rbs (..) -> T` annotation, given the
/// comment text after `#`.
fn rbs_tag_method_type(text: &str) -> Option<&str> {
    let method_type = text.strip_prefix("@rbs")?.trim_start();
    ["(", "[", "?{", "{", "->"]
        .iter()
        .any(|start| method_type.starts_with(start))
        .then_some(method_type)
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

/// Whether only whitespace comes before `node` on its first line. A trailing
/// comment, such as `attr_accessor :cb #: ^() -> String`, types its own line.
fn starts_its_line(base: &BaseExtractor, node: Node) -> bool {
    let before = &base.content[..node.start_byte()];
    before[before.rfind('\n').map_or(0, |newline| newline + 1)..]
        .trim()
        .is_empty()
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

/// State the index walk carries across the whole tree.
struct Walk<'tree, 'a> {
    definitions: &'a mut Vec<Node<'tree>>,
    definition_scopes: HashMap<usize, Option<SelfScope>>,
}

/// `lexical_owner` is what `lexical_owner` gives for the children of `node`'s
/// parent, carried down so an `@ivar` never climbs the tree for it.
fn collect<'tree>(
    base: &BaseExtractor,
    node: Node<'tree>,
    lexical_owner: Option<&str>,
    depth: u32,
    walk: &mut Walk<'tree, '_>,
    index: &mut ReturnTypeIndex,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    let mut own_path = None;
    match node.kind() {
        "method" | "singleton_method" => walk.definitions.push(node),
        "class" | "module" => {
            own_path = owner_path(base, node);
            if let Some(path) = own_path.clone() {
                if node.child_by_field_name("superclass").is_some() {
                    index.has_ancestors.insert((path.clone(), false));
                    index.has_ancestors.insert((path.clone(), true));
                }
                index.owners.insert(path, node.kind() == "class");
            }
        }
        "alias" => {
            if let Some(name) = node.child_by_field_name("name") {
                let name = base.get_node_text(&name);
                let key = definition_scope(base, node).map(|scope| scope.key());
                index
                    .redefinitions
                    .push((name.trim_start_matches(':').to_string(), key));
            }
        }
        "call" => {
            if is_self_bind(base, node) {
                index.self_rebinds.push((node.start_byte(), false));
            }
            collect_call(base, node, index);
        }
        "instance_variable" => {
            let level = self_scope_cached(base, node, &mut walk.definition_scopes)
                .map(|scope| scope.singleton);
            index
                .ivar_levels
                .entry((lexical_owner.map(str::to_string), base.get_node_text(&node)))
                .or_default()
                .insert(level);
        }
        "comment" => {
            let comment = base.get_node_text(&node);
            if let Some(reaches_owner) = steep_self_annotation(&comment) {
                index.self_rebinds.push((node.start_byte(), reaches_owner));
            }
            index.generics.extend(comment_generics(&comment));
        }
        "assignment" => {
            index.generics.extend(sorbet_type_constant(base, node));
            index.constants.extend(constant_target(base, node));
        }
        _ => {}
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let children_owner = if matches!(node.kind(), "class" | "module") {
        own_path.as_deref()
    } else {
        lexical_owner
    };
    for child in node.named_children(&mut node.walk()) {
        collect(base, child, children_owner, child_depth, walk, index);
    }
}

/// Whether a call is Sorbet `T.bind(self, ..)`.
fn is_self_bind(base: &BaseExtractor, call: Node) -> bool {
    call.child_by_field_name("receiver")
        .is_some_and(|receiver| base.get_node_text(&receiver) == "T")
        && call
            .child_by_field_name("method")
            .is_some_and(|method| base.get_node_text(&method) == "bind")
        && call
            .child_by_field_name("arguments")
            .and_then(|arguments| arguments.named_child(0))
            .is_some_and(|first| first.kind() == "self")
}

/// For a Steep `# @type self:`, `instance:` or `module:` comment, whether it
/// reaches the whole class or module (`instance:` and `module:`).
fn steep_self_annotation(comment: &str) -> Option<bool> {
    let text = comment.trim().strip_prefix('#')?.trim_start();
    let target = text.strip_prefix("@type")?.trim_start();
    if target.starts_with("self:") {
        Some(false)
    } else {
        (target.starts_with("instance:") || target.starts_with("module:")).then_some(true)
    }
}

/// Type parameter names an RBS comment declares: `#: [T, U] ...`,
/// `# @rbs [T] ...` or `# @rbs generic T`.
fn comment_generics(comment: &str) -> Vec<String> {
    let text = comment.trim();
    let tag = text.strip_prefix('#').unwrap_or(text).trim_start();
    let method_type = text.strip_prefix("#:").or_else(|| rbs_tag_method_type(tag));
    let declared = if let Some(rest) = method_type {
        rest.trim_start()
            .strip_prefix('[')
            .and_then(|rest| rest.split_once(']'))
            .map(|(parameters, _)| parameters.split(',').collect::<Vec<_>>())
            .unwrap_or_default()
    } else if let Some(rest) = tag.strip_prefix("@rbs generic") {
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

/// The constant a Sorbet `Elem = type_member`, `type_template` or
/// `T.type_alias { .. }` declares. None of them names one class.
fn sorbet_type_constant(base: &BaseExtractor, assignment: Node) -> Option<String> {
    let left = assignment.child_by_field_name("left")?;
    let right = assignment.child_by_field_name("right")?;
    let receiver = right
        .child_by_field_name("receiver")
        .map(|receiver| base.get_node_text(&receiver));
    let method = match right.kind() {
        "identifier" => right,
        "call" => right.child_by_field_name("method")?,
        _ => return None,
    };
    let is_type_constant = matches!(
        (receiver.as_deref(), base.get_node_text(&method).as_str()),
        (None, "type_member" | "type_template") | (Some("T"), "type_alias")
    );
    (matches!(left.kind(), "constant" | "scope_resolution") && is_type_constant)
        .then(|| last_segment(&base.get_node_text(&left)).to_string())
}

/// The full path of the constant an assignment such as `Widget = Other`
/// declares.
fn constant_target(base: &BaseExtractor, assignment: Node) -> Option<String> {
    let left = assignment.child_by_field_name("left")?;
    if !matches!(left.kind(), "constant" | "scope_resolution") {
        return None;
    }
    let name = base.get_node_text(&left);
    if let Some(absolute) = name.strip_prefix("::") {
        return Some(absolute.to_string());
    }
    Some(match lexical_owner(base, assignment) {
        Some(owner) => format!("{owner}::{name}"),
        None => name,
    })
}

/// Record what a call changes about method and constant lookup: `include`,
/// `extend` and `prepend` add ancestors, and `alias_method`, `attr_reader`,
/// `define_method` and similar calls define methods without a `def`.
fn collect_call(base: &BaseExtractor, call: Node, index: &mut ReturnTypeIndex) {
    let Some(method) = call.child_by_field_name("method") else {
        return;
    };
    let method = base.get_node_text(&method);
    if !matches!(
        method.as_str(),
        "include"
            | "extend"
            | "prepend"
            | "alias_method"
            | "define_method"
            | "define_singleton_method"
            | "attr_reader"
            | "attr_accessor"
            | "attr"
            | "delegate"
            | "def_delegator"
            | "def_delegators"
    ) {
        return;
    }
    let receiver = call.child_by_field_name("receiver");
    if matches!(method.as_str(), "include" | "extend" | "prepend") {
        collect_ancestor_call(base, call, receiver, &method, index);
        return;
    }
    let scope = definition_scope(base, call)
        .filter(|_| receiver.is_none_or(|receiver| receiver.kind() == "self"));
    let Some(arguments) = call.child_by_field_name("arguments") else {
        return;
    };
    let mut cursor = arguments.walk();
    let (arguments, options): (Vec<Node>, Vec<Node>) = arguments
        .named_children(&mut cursor)
        .partition(|argument| argument.kind() != "pair");
    let first = &arguments[..arguments.len().min(1)];
    let (defined, scope) = match method.as_str() {
        "alias_method" | "define_method" => (first, scope),
        "attr_reader" | "attr_accessor" => (&arguments[..], scope),
        "delegate" => {
            let prefixed = options.iter().any(|option| {
                option
                    .child_by_field_name("key")
                    .is_some_and(|key| base.get_node_text(&key).starts_with("prefix"))
            });
            if prefixed {
                index.dynamic_owners.insert(lexical_owner(base, call));
            }
            (&arguments[..], scope)
        }
        "attr" => (
            &arguments[..],
            scope.filter(|_| {
                !arguments
                    .iter()
                    .any(|argument| matches!(argument.kind(), "true" | "false"))
            }),
        ),
        "def_delegators" => (arguments.get(1..).unwrap_or_default(), scope),
        "def_delegator" => (
            arguments
                .get(2..3)
                .or_else(|| arguments.get(1..2))
                .unwrap_or_default(),
            scope,
        ),
        _ => (
            first,
            scope
                .filter(|scope| !scope.singleton)
                .map(|scope| SelfScope {
                    singleton: true,
                    ..scope
                }),
        ),
    };
    let key = scope.map(|scope| scope.key());
    for argument in defined {
        match literal_method_name(base, *argument) {
            Some(name) => index.redefinitions.push((name, key.clone())),
            None => {
                index.dynamic_owners.insert(lexical_owner(base, call));
            }
        }
    }
}

/// Record the ancestors an `include`, `extend` or `prepend` adds. An
/// `include` or `prepend` counts for both `self` levels, since a module's
/// `included` hook can `extend` the class too. One whose target the file does
/// not settle, such as one in a block or a method, counts for both levels of
/// its lexical owner.
fn collect_ancestor_call(
    base: &BaseExtractor,
    call: Node,
    receiver: Option<Node>,
    method: &str,
    index: &mut ReturnTypeIndex,
) {
    if let Some(receiver) = receiver.filter(|receiver| receiver.kind() != "self") {
        if matches!(receiver.kind(), "constant" | "scope_resolution") {
            let name = last_segment(&base.get_node_text(&receiver)).to_string();
            if method == "prepend" {
                index.named_prepends.insert(name.clone());
            }
            index.named_ancestors.insert(name);
        } else {
            index.prepend_anywhere |= method == "prepend";
        }
        return;
    }
    let scope = definition_scope(base, call);
    let ancestor_levels: &[bool] = match (&scope, method) {
        (Some(scope), _) if scope.singleton => &[true],
        (Some(_), "extend") => &[true],
        _ => &[false, true],
    };
    let prepend_levels: &[bool] = match &scope {
        Some(scope) if scope.singleton => &[true],
        Some(_) => &[false],
        None => &[false, true],
    };
    let owner = match scope {
        Some(scope) => scope.owner,
        None => lexical_owner(base, call),
    };
    if let Some(owner) = owner.clone() {
        for singleton in ancestor_levels {
            index.has_ancestors.insert((owner.clone(), *singleton));
        }
    }
    if method == "prepend" {
        for singleton in prepend_levels {
            index.prepends.insert((owner.clone(), *singleton));
        }
    }
}

/// The method name a `:name`, `:"name"` or `'name'` argument spells out.
fn literal_method_name(base: &BaseExtractor, argument: Node) -> Option<String> {
    match argument.kind() {
        "simple_symbol" => Some(base.get_node_text(&argument)[1..].to_string()),
        "string" | "delimited_symbol" => {
            let mut cursor = argument.walk();
            let mut parts = argument.named_children(&mut cursor);
            match (parts.next(), parts.next()) {
                (Some(content), None) if content.kind() == "string_content" => {
                    Some(base.get_node_text(&content))
                }
                _ => None,
            }
        }
        _ => None,
    }
}
