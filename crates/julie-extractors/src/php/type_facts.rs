use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

pub(super) const PHP_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &["?", "\\"],
    generic_open: &[],
};

pub(super) fn record_declared_type(base: &mut BaseExtractor, symbol_id: &str, type_node: Node) {
    record_type_node(base, symbol_id, type_node, false);
}

/// Record the type an assignment's right side produces (`is_inferred=true`):
/// the class of a `new` expression, or the declared return type of a call to
/// a same-file function (`load()`), a method of the enclosing class or enum
/// (`$this->load()`, `self::load()`, `static::load()`), or a static method of
/// a same-file class (`Type::create()`). Any other right side records nothing.
pub(super) fn record_initializer_type(
    base: &mut BaseExtractor,
    symbol_id: &str,
    value_node: Node,
    return_types: &ReturnTypeIndex,
) {
    if value_node.kind() == "object_creation_expression" {
        if let Some(type_node) = object_creation_type(value_node) {
            record_type_node(base, symbol_id, type_node, true);
        }
        return;
    }
    let Some(return_type) = return_types.call_return_type(base, value_node) else {
        return;
    };
    base.record_declared_type_fact_with_declared(
        symbol_id,
        &return_type.base_name,
        &return_type.declared,
        &PHP_TYPE_NAME_RULES,
        true,
    );
}

/// Declared return types of the file's functions and class or enum methods.
/// Keys are lowercase because PHP resolves function, method, and class names
/// without regard to case. Trait and anonymous-class methods are left out:
/// their `self` and `$this` are not a class this file names.
#[derive(Debug, Default)]
pub(super) struct ReturnTypeIndex {
    callees: HashMap<CalleeKey, Vec<Option<ReturnType>>>,
    blocks: NamespaceBlocks,
}

#[derive(Debug, PartialEq, Eq, Hash)]
struct CalleeKey {
    namespace: String,
    /// The declaring class or enum; `None` for a function.
    owner: Option<String>,
    name: String,
}

impl CalleeKey {
    fn new(namespace: &str, owner: Option<&str>, name: &str) -> Self {
        Self {
            namespace: namespace.to_ascii_lowercase(),
            owner: owner.map(str::to_ascii_lowercase),
            name: name.to_ascii_lowercase(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReturnType {
    base_name: String,
    declared: String,
    /// The namespace block whose imports give `base_name` its meaning, or
    /// `None` when the name means the same class in every block.
    block: Option<usize>,
}

impl ReturnTypeIndex {
    pub(super) fn build(base: &BaseExtractor, root: Node) -> Self {
        let blocks = NamespaceBlocks::of_program(base, root);
        let mut callees: HashMap<CalleeKey, Vec<Option<ReturnType>>> = HashMap::new();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if let Some((key, return_type)) = return_entry(base, &blocks, node) {
                callees.entry(key).or_default().push(return_type);
            }
            stack.extend(node.named_children(&mut node.walk()));
        }
        Self { callees, blocks }
    }

    /// The return type every same-named callee agrees on, when its name
    /// means the same class in the caller's block.
    fn lookup(&self, key: &CalleeKey, caller_block: usize) -> Option<&ReturnType> {
        let mut return_types = self.callees.get(key)?.iter();
        let first = return_types.next()?.as_ref()?;
        (return_types.all(|return_type| return_type.as_ref() == Some(first))
            && first.block.is_none_or(|block| block == caller_block))
        .then_some(first)
    }

    fn call_return_type(&self, base: &BaseExtractor, call: Node) -> Option<&ReturnType> {
        if is_first_class_callable(call) {
            return None;
        }
        let block_index = self.blocks.index_of(call);
        let block = &self.blocks.blocks[block_index];
        let (owner, name) = match call.kind() {
            "function_call_expression" => (None, call.child_by_field_name("function")?),
            "member_call_expression" => {
                let object = call.child_by_field_name("object")?;
                if base.get_node_text(&object) != "$this" {
                    return None;
                }
                (
                    Some(lexical_class_name(base, call)?),
                    call.child_by_field_name("name")?,
                )
            }
            "scoped_call_expression" => {
                let scope = call.child_by_field_name("scope")?;
                let scope_text = base.get_node_text(&scope);
                let owner = match scope.kind() {
                    "relative_scope" if is_self_or_static(&scope_text) => {
                        lexical_class_name(base, call)?
                    }
                    "name" if !block.imports(&scope_text, ImportKind::Class) => scope_text,
                    _ => return None,
                };
                (Some(owner), call.child_by_field_name("name")?)
            }
            _ => return None,
        };
        let name = base.get_node_text(&name);
        if owner.is_none() && block.imports(&name, ImportKind::Function) {
            return None;
        }
        self.lookup(
            &CalleeKey::new(&block.namespace, owner.as_deref(), &name),
            block_index,
        )
    }
}

/// `f(...)` builds a `Closure` from `f` and does not call it.
fn is_first_class_callable(call: Node) -> bool {
    call.child_by_field_name("arguments")
        .is_some_and(|arguments| {
            arguments
                .named_children(&mut arguments.walk())
                .any(|argument| argument.kind() == "variadic_placeholder")
        })
}

/// The file's namespace blocks: each braced `namespace X { }` body, and the
/// top-level statements from one `namespace X;` statement to the next. PHP
/// resets imports at each namespace declaration, so a name can mean a
/// different class in two blocks of the same namespace.
#[derive(Debug)]
struct NamespaceBlocks {
    blocks: Vec<NamespaceBlock>,
    /// Block index by the node id of a top-level statement.
    by_statement: HashMap<usize, usize>,
}

impl Default for NamespaceBlocks {
    fn default() -> Self {
        Self {
            blocks: vec![NamespaceBlock::default()],
            by_statement: HashMap::new(),
        }
    }
}

/// A namespace block's name and the lowercase local names its `use`
/// declarations bind.
#[derive(Debug, Default)]
struct NamespaceBlock {
    namespace: String,
    class_imports: HashSet<String>,
    function_imports: HashSet<String>,
}

impl NamespaceBlock {
    fn imports(&self, name: &str, kind: ImportKind) -> bool {
        let imports = match kind {
            ImportKind::Class => &self.class_imports,
            ImportKind::Function => &self.function_imports,
            ImportKind::Const => return false,
        };
        imports.contains(&name.to_ascii_lowercase())
    }
}

impl NamespaceBlocks {
    fn of_program(base: &BaseExtractor, root: Node) -> Self {
        let mut blocks = Self::default();
        let mut current = 0;
        for statement in root.named_children(&mut root.walk()) {
            if statement.kind() == "namespace_definition" {
                let namespace = statement
                    .child_by_field_name("name")
                    .map(|name| base.get_node_text(&name))
                    .unwrap_or_default();
                blocks.blocks.push(NamespaceBlock {
                    namespace,
                    ..NamespaceBlock::default()
                });
                current = blocks.blocks.len() - 1;
                if let Some(body) = statement.child_by_field_name("body") {
                    for inner in body.named_children(&mut body.walk()) {
                        blocks.add_imports(base, current, inner);
                    }
                }
            }
            blocks.by_statement.insert(statement.id(), current);
            blocks.add_imports(base, current, statement);
        }
        blocks
    }

    fn add_imports(&mut self, base: &BaseExtractor, block: usize, statement: Node) {
        if statement.kind() != "namespace_use_declaration" {
            return;
        }
        let declaration_kind = import_kind(statement).unwrap_or(ImportKind::Class);
        let mut clauses: Vec<Node> = statement
            .named_children(&mut statement.walk())
            .filter(|child| child.kind() == "namespace_use_clause")
            .collect();
        if let Some(group) = statement.child_by_field_name("body") {
            clauses.extend(group.named_children(&mut group.walk()));
        }
        let block = &mut self.blocks[block];
        for clause in clauses {
            let Some(alias) = import_alias(base, clause) else {
                continue;
            };
            let imports = match import_kind(clause).unwrap_or(declaration_kind) {
                ImportKind::Class => &mut block.class_imports,
                ImportKind::Function => &mut block.function_imports,
                ImportKind::Const => continue,
            };
            imports.insert(alias.to_ascii_lowercase());
        }
    }

    /// The index of the block that holds `node`. A braced namespace body
    /// maps through its `namespace_definition`; a top-level statement maps
    /// directly.
    fn index_of(&self, node: Node) -> usize {
        let mut statement = node;
        while let Some(parent) = statement.parent() {
            match parent.kind() {
                "namespace_definition" if parent.child_by_field_name("body").is_some() => {
                    statement = parent;
                    break;
                }
                "program" => break,
                _ => statement = parent,
            }
        }
        self.by_statement
            .get(&statement.id())
            .copied()
            .unwrap_or_default()
    }

    fn namespace_of(&self, node: Node) -> &str {
        &self.blocks[self.index_of(node)].namespace
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ImportKind {
    Class,
    Function,
    Const,
}

fn import_kind(node: Node) -> Option<ImportKind> {
    match node.child_by_field_name("type")?.kind() {
        "function" => Some(ImportKind::Function),
        "const" => Some(ImportKind::Const),
        _ => None,
    }
}

/// The local name a `use` clause binds: its `as` alias, or the last segment.
fn import_alias(base: &BaseExtractor, clause: Node) -> Option<String> {
    if let Some(alias) = clause.child_by_field_name("alias") {
        return Some(base.get_node_text(&alias));
    }
    let imported = clause
        .named_children(&mut clause.walk())
        .find(|child| matches!(child.kind(), "name" | "qualified_name"))?;
    let text = base.get_node_text(&imported);
    text.rsplit('\\').next().map(str::to_string)
}

fn return_entry(
    base: &BaseExtractor,
    blocks: &NamespaceBlocks,
    node: Node,
) -> Option<(CalleeKey, Option<ReturnType>)> {
    let owner = match node.kind() {
        "function_definition" => None,
        "method_declaration" => Some(method_owner_name(base, node)?),
        _ => return None,
    };
    let name = base.get_node_text(&node.child_by_field_name("name")?);
    let return_type = node
        .child_by_field_name("return_type")
        .and_then(|type_node| return_type(base, blocks, type_node, owner.as_deref()));
    let key = CalleeKey::new(blocks.namespace_of(node), owner.as_deref(), &name);
    Some((key, return_type))
}

/// A single named return type; `self` and `static` name the owner. `void`
/// and `parent` record nothing. A class name that is not fully qualified
/// resolves through the callee block's imports, so it is tied to that block.
fn return_type(
    base: &BaseExtractor,
    blocks: &NamespaceBlocks,
    type_node: Node,
    owner: Option<&str>,
) -> Option<ReturnType> {
    if !names_single_base_type(type_node) {
        return None;
    }
    let declared = base.get_node_text(&type_node);
    let bare = declared.trim_start_matches('?');
    let (base_name, block) = if is_self_or_static(bare) {
        (owner?.to_string(), None)
    } else if bare.eq_ignore_ascii_case("void") || bare.eq_ignore_ascii_case("parent") {
        return None;
    } else if bare.starts_with('\\') || names_primitive_type(type_node) {
        (bare.to_string(), None)
    } else {
        (bare.to_string(), Some(blocks.index_of(type_node)))
    };
    Some(ReturnType {
        base_name,
        declared,
        block,
    })
}

fn names_primitive_type(node: Node) -> bool {
    match node.kind() {
        "primitive_type" => true,
        "optional_type" => node.named_child(0).is_some_and(names_primitive_type),
        _ => false,
    }
}

fn is_self_or_static(text: &str) -> bool {
    text.eq_ignore_ascii_case("self") || text.eq_ignore_ascii_case("static")
}

/// The named class or enum that declares `method`.
fn method_owner_name(base: &BaseExtractor, method: Node) -> Option<String> {
    let owner = method.parent()?.parent()?;
    if !matches!(owner.kind(), "class_declaration" | "enum_declaration") {
        return None;
    }
    Some(base.get_node_text(&owner.child_by_field_name("name")?))
}

/// The class or enum whose method body holds `node`. A closure can be rebound
/// to another object, so `$this` and `self` inside one name nothing here.
fn lexical_class_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut current = node.parent();
    while let Some(ancestor) = current {
        match ancestor.kind() {
            "class_declaration" | "enum_declaration" => {
                return ancestor
                    .child_by_field_name("name")
                    .map(|name| base.get_node_text(&name));
            }
            "anonymous_class" | "anonymous_function" | "arrow_function" | "function_definition" => {
                return None;
            }
            _ => current = ancestor.parent(),
        }
    }
    None
}

fn record_type_node(base: &mut BaseExtractor, symbol_id: &str, type_node: Node, is_inferred: bool) {
    if !names_single_base_type(type_node) {
        return;
    }
    let declared = base.get_node_text(&type_node);
    let bare = declared.trim_start_matches('?');
    let base_text = if bare.eq_ignore_ascii_case("self") || bare.eq_ignore_ascii_case("static") {
        let Some(class_name) = enclosing_class_name(base, type_node) else {
            return;
        };
        class_name
    } else {
        bare.to_string()
    };
    base.record_declared_type_fact_with_declared(
        symbol_id,
        &base_text,
        &declared,
        &PHP_TYPE_NAME_RULES,
        is_inferred,
    );
}

/// `self` and `static` name the class, enum, trait, or interface that
/// declares the member. An anonymous class has no name to record.
fn enclosing_class_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut current = node.parent();
    while let Some(ancestor) = current {
        match ancestor.kind() {
            "class_declaration"
            | "enum_declaration"
            | "trait_declaration"
            | "interface_declaration" => {
                return ancestor
                    .child_by_field_name("name")
                    .map(|name| base.get_node_text(&name));
            }
            "anonymous_class" => return None,
            _ => current = ancestor.parent(),
        }
    }
    None
}

fn object_creation_type(node: Node<'_>) -> Option<Node<'_>> {
    if node
        .named_child(0)
        .is_some_and(|child| child.kind() == "anonymous_class")
    {
        return None;
    }
    node.child_by_field_name("type").or_else(|| {
        let child = node.named_child(0)?;
        match child.kind() {
            "name" | "qualified_name" | "named_type" | "primitive_type" => Some(child),
            _ => None,
        }
    })
}

fn names_single_base_type(node: Node) -> bool {
    match node.kind() {
        "name" | "qualified_name" | "named_type" | "primitive_type" => true,
        "optional_type" => node.named_child(0).is_some_and(names_single_base_type),
        "union_type" | "intersection_type" | "disjunctive_normal_form_type" => false,
        _ => false,
    }
}
