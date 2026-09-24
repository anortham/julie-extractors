use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use std::collections::HashMap;
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
pub(super) struct ReturnTypeIndex(HashMap<CalleeKey, Vec<Option<ReturnType>>>);

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
}

impl ReturnTypeIndex {
    pub(super) fn build(base: &BaseExtractor, root: Node) -> Self {
        let mut entries: HashMap<CalleeKey, Vec<Option<ReturnType>>> = HashMap::new();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if let Some((key, return_type)) = return_entry(base, node) {
                entries.entry(key).or_default().push(return_type);
            }
            stack.extend(node.named_children(&mut node.walk()));
        }
        Self(entries)
    }

    /// The return type every same-named callee agrees on.
    fn lookup(&self, key: &CalleeKey) -> Option<&ReturnType> {
        let mut return_types = self.0.get(key)?.iter();
        let first = return_types.next()?.as_ref()?;
        return_types
            .all(|return_type| return_type.as_ref() == Some(first))
            .then_some(first)
    }

    fn call_return_type(&self, base: &BaseExtractor, call: Node) -> Option<&ReturnType> {
        if is_first_class_callable(call) {
            return None;
        }
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
                    "name" if !imports_name(base, call, &scope_text, ImportKind::Class) => {
                        scope_text
                    }
                    _ => return None,
                };
                (Some(owner), call.child_by_field_name("name")?)
            }
            _ => return None,
        };
        let name = base.get_node_text(&name);
        if owner.is_none() && imports_name(base, call, &name, ImportKind::Function) {
            return None;
        }
        self.lookup(&CalleeKey::new(
            &namespace_of(base, call),
            owner.as_deref(),
            &name,
        ))
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum ImportKind {
    Class,
    Function,
    Const,
}

/// Whether a `use` declaration in `node`'s namespace block imports `name`
/// as `kind`. Imports reset at each namespace declaration, so an imported
/// name refers to another namespace even when this file declares it too.
fn imports_name(base: &BaseExtractor, node: Node, name: &str, kind: ImportKind) -> bool {
    namespace_block(node).1.into_iter().any(|statement| {
        statement.kind() == "namespace_use_declaration"
            && use_declaration_imports(base, statement, name, kind)
    })
}

fn use_declaration_imports(
    base: &BaseExtractor,
    declaration: Node,
    name: &str,
    kind: ImportKind,
) -> bool {
    let declaration_kind = import_kind(declaration).unwrap_or(ImportKind::Class);
    let mut clauses: Vec<Node> = declaration
        .named_children(&mut declaration.walk())
        .filter(|child| child.kind() == "namespace_use_clause")
        .collect();
    if let Some(group) = declaration.child_by_field_name("body") {
        clauses.extend(group.named_children(&mut group.walk()));
    }
    clauses.into_iter().any(|clause| {
        import_kind(clause).unwrap_or(declaration_kind) == kind
            && import_alias(base, clause).is_some_and(|alias| alias.eq_ignore_ascii_case(name))
    })
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

fn return_entry(base: &BaseExtractor, node: Node) -> Option<(CalleeKey, Option<ReturnType>)> {
    let owner = match node.kind() {
        "function_definition" => None,
        "method_declaration" => Some(method_owner_name(base, node)?),
        _ => return None,
    };
    let name = base.get_node_text(&node.child_by_field_name("name")?);
    let return_type = node
        .child_by_field_name("return_type")
        .and_then(|type_node| return_type(base, type_node, owner.as_deref()));
    let key = CalleeKey::new(&namespace_of(base, node), owner.as_deref(), &name);
    Some((key, return_type))
}

/// A single named return type; `self` and `static` name the owner. `void`
/// and `parent` record nothing.
fn return_type(base: &BaseExtractor, type_node: Node, owner: Option<&str>) -> Option<ReturnType> {
    if !names_single_base_type(type_node) {
        return None;
    }
    let declared = base.get_node_text(&type_node);
    let bare = declared.trim_start_matches('?');
    let base_name = if is_self_or_static(bare) {
        owner?.to_string()
    } else if bare.eq_ignore_ascii_case("void") || bare.eq_ignore_ascii_case("parent") {
        return None;
    } else {
        bare.to_string()
    };
    Some(ReturnType {
        base_name,
        declared,
    })
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

/// The namespace `node` is declared in: the enclosing braced `namespace { }`
/// block, or the last `namespace X;` statement before it.
fn namespace_of(base: &BaseExtractor, node: Node) -> String {
    namespace_block(node)
        .0
        .and_then(|namespace| namespace.child_by_field_name("name"))
        .map(|name| base.get_node_text(&name))
        .unwrap_or_default()
}

/// The namespace declaration that holds `node`, if any, and the top-level
/// statements of its block: the braced body, or the statements between the
/// surrounding `namespace X;` statements.
fn namespace_block(node: Node<'_>) -> (Option<Node<'_>>, Vec<Node<'_>>) {
    let mut statement = node;
    while let Some(parent) = statement.parent() {
        if parent.kind() == "namespace_definition" {
            return (
                Some(parent),
                statement.named_children(&mut statement.walk()).collect(),
            );
        }
        if parent.kind() == "program" {
            break;
        }
        statement = parent;
    }
    let is_boundary = |sibling: &Node| sibling.kind() == "namespace_definition";
    let mut statements = vec![statement];
    let mut namespace = None;
    let mut sibling = statement.prev_named_sibling();
    while let Some(previous) = sibling {
        if is_boundary(&previous) {
            namespace = Some(previous);
            break;
        }
        statements.push(previous);
        sibling = previous.prev_named_sibling();
    }
    let mut sibling = statement.next_named_sibling();
    while let Some(next) = sibling.filter(|next| !is_boundary(next)) {
        statements.push(next);
        sibling = next.next_named_sibling();
    }
    (namespace, statements)
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
