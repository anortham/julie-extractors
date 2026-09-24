use crate::base::types::{TypeNameRules, strip_type_decorations};
use crate::base::{BaseExtractor, Symbol, SymbolKind};
use crate::lua::{helpers, scope};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

pub(super) const TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &[],
    generic_open: &[],
};

pub(super) fn record_declared_owner_fact(
    base: &mut BaseExtractor,
    symbol_id: &str,
    owner_name: &str,
) {
    base.record_declared_type_fact(symbol_id, owner_name, &TYPE_NAME_RULES, false);
}

/// Record inferred type facts for `local` declarations whose initializer is a
/// call: first a same-file function with an annotated `---@return` type, then
/// the constructor patterns of a same-file class.
pub(super) fn record_inferred_initializer_facts(
    base: &mut BaseExtractor,
    root: Node,
    symbols: &[Symbol],
) {
    let scope = InitializerScope {
        class_names: symbols
            .iter()
            .filter(|symbol| symbol.kind == SymbolKind::Class)
            .map(|symbol| symbol.name.clone())
            .collect(),
        bindings: binding_counts(symbols),
        return_types: ReturnTypeIndex::build(base, root, &generic_names(symbols)),
    };
    walk_initializer_facts(base, root, symbols, &scope, 0);
}

pub(super) fn colon_method_owner_name(base: &BaseExtractor, function_node: Node) -> Option<String> {
    let table = colon_method_index(function_node)?.child_by_field_name("table")?;
    let owner = match table.kind() {
        "identifier" => table,
        "dot_index_expression" => table.child_by_field_name("field")?,
        _ => return None,
    };
    Some(base.get_node_text(&owner))
}

pub(super) fn colon_method_name_node(function_node: Node) -> Option<Node> {
    colon_method_index(function_node)?.child_by_field_name("method")
}

fn colon_method_index(function_node: Node) -> Option<Node> {
    function_node
        .child_by_field_name("name")
        .filter(|name| name.kind() == "method_index_expression")
}

pub(super) fn enclosing_colon_owner_name(base: &BaseExtractor, mut node: Node) -> Option<String> {
    while let Some(parent) = node.parent() {
        if matches!(
            parent.kind(),
            "function_declaration" | "function_definition_statement"
        ) {
            return colon_method_owner_name(base, parent);
        }
        node = parent;
    }
    None
}

pub(super) fn call_receiver_type(base: &BaseExtractor, node: Node) -> Option<String> {
    let index = match node.kind() {
        "function_call" => node.child_by_field_name("name")?,
        "method_index_expression" | "dot_index_expression" => node,
        _ => return None,
    };
    if identifier_table_name(base, index)?.as_str() != "self" {
        return None;
    }
    enclosing_colon_owner_name(base, node)
}

fn identifier_table_name(base: &BaseExtractor, index: Node) -> Option<String> {
    let table = index.child_by_field_name("table")?;
    if table.kind() != "identifier" {
        return None;
    }
    Some(base.get_node_text(&table))
}

fn walk_initializer_facts(
    base: &mut BaseExtractor,
    node: Node,
    symbols: &[Symbol],
    scope: &InitializerScope,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if node.kind() == "variable_declaration" {
        record_declaration_initializer_facts(base, node, symbols, scope);
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_initializer_facts(base, child, symbols, scope, child_depth);
    }
}

fn record_declaration_initializer_facts(
    base: &mut BaseExtractor,
    node: Node,
    symbols: &[Symbol],
    scope: &InitializerScope,
) {
    let Some(assignment) = helpers::find_child_by_type(&node, "assignment_statement") else {
        return;
    };
    let Some(variable_list) = helpers::find_child_by_type(&assignment, "variable_list") else {
        return;
    };
    let expressions: Vec<Node> = helpers::find_child_by_type(&assignment, "expression_list")
        .map(super::variables::collect_expression_nodes)
        .unwrap_or_default();

    let mut cursor = variable_list.walk();
    let variables: Vec<Node> = variable_list
        .children(&mut cursor)
        .filter(|child| child.kind() == "variable" || child.kind() == "identifier")
        .collect();

    for (index, var_node) in variables.iter().enumerate() {
        let name_node = if var_node.kind() == "identifier" {
            Some(*var_node)
        } else {
            helpers::find_child_by_type(var_node, "identifier")
        };
        let Some(name_node) = name_node else {
            continue;
        };
        let Some(expression) = expressions.get(index).copied() else {
            continue;
        };
        let name = base.get_node_text(&name_node);
        let Some(symbol) = symbol_for_name_node(symbols, &name, name_node)
            .filter(|symbol| symbol.kind != SymbolKind::Class)
        else {
            continue;
        };
        if let Some(returned) = scope.call_return_type(base, expression) {
            base.record_declared_type_fact_with_declared(
                &symbol.id,
                &returned.name,
                &returned.declared,
                &ANNOTATION_TYPE_RULES,
                true,
            );
        } else if !scope.callee_has_return_annotation(base, expression)
            && let Some(type_name) = constructor_type_name(base, expression)
                .filter(|type_name| scope.class_names.contains(type_name))
        {
            base.record_declared_type_fact(&symbol.id, &type_name, &TYPE_NAME_RULES, true);
        }
    }
}

struct InitializerScope {
    class_names: HashSet<String>,
    bindings: HashMap<String, usize>,
    return_types: ReturnTypeIndex,
}

impl InitializerScope {
    /// The annotated return type of a call to a same-file function: `load()`,
    /// `M.load()`, `M:load()`, `a.b.load()`, or `self:load()` / `self.load()`
    /// inside a colon method of `M` where `self` is not a declared parameter or
    /// local. The free name or the owner's root name must resolve to the same
    /// declaration (or to the global) as at the function's definition. A free
    /// name that the file also binds as a variable, parameter, or import records
    /// nothing, as does an owner bound more than once.
    fn call_return_type(&self, base: &BaseExtractor, expression: Node) -> Option<&ReturnType> {
        if expression.kind() != "function_call" {
            return None;
        }
        let callee = expression.child_by_field_name("name")?;
        if let Some(table) = callee.child_by_field_name("table")
            && table.kind() != "identifier"
            && written_root(table).is_some_and(|root| base.get_node_text(&root) == "self")
        {
            return None;
        }
        let path = TargetPath::of(base, callee, 0)?;
        let rejected = match &path.owner {
            None => self.bindings.contains_key(&path.name),
            Some(_) => {
                let root_name = base.get_node_text(&path.root);
                root_name == "self"
                    || self
                        .bindings
                        .get(&root_name)
                        .is_some_and(|count| *count > 1)
            }
        };
        if rejected {
            return None;
        }
        let binding = root_binding(base, path.root);
        self.return_types
            .lookup(&path.name, path.owner.as_deref(), binding)
    }

    /// True when a same-file function with the callee's name and owner has a
    /// `---@return` or `---@overload` tag, usable or not, so the class
    /// constructor rule must not guess instead.
    fn callee_has_return_annotation(&self, base: &BaseExtractor, expression: Node) -> bool {
        expression
            .child_by_field_name("name")
            .and_then(|callee| TargetPath::of(base, callee, 0))
            .is_some_and(|path| {
                self.return_types
                    .entries
                    .get(&path.name)
                    .is_some_and(|entries| {
                        entries.iter().any(|entry| {
                            entry.annotated && entry.owner.as_deref() == path.owner.as_deref()
                        })
                    })
            })
    }
}

/// The identifier at the root of an `a.b["c"]` chain, as written.
fn written_root(mut table: Node) -> Option<Node> {
    loop {
        match table.kind() {
            "identifier" => return Some(table),
            "dot_index_expression" | "bracket_index_expression" => {
                table = table.child_by_field_name("table")?;
            }
            _ => return None,
        }
    }
}

/// A function name or assignment target read as an owner table path and a
/// member name. `self` inside a colon method reads as the owner table,
/// `X["name"]` reads as `X.name`, and a global `_G.name` reads as `name`.
struct TargetPath<'tree> {
    owner: Option<String>,
    name: String,
    /// The identifier that roots the path, after `self` is replaced.
    root: Node<'tree>,
}

impl<'tree> TargetPath<'tree> {
    /// `None` past the traversal depth limit, so a very deep chain records nothing.
    fn of(base: &BaseExtractor, target: Node<'tree>, depth: u32) -> Option<Self> {
        if !should_visit_tree_depth(depth) {
            return None;
        }
        let member = match target.kind() {
            "identifier" => {
                return Some(Self {
                    owner: None,
                    name: base.get_node_text(&target),
                    root: target,
                });
            }
            "dot_index_expression" => base.get_node_text(&target.child_by_field_name("field")?),
            "method_index_expression" => base.get_node_text(&target.child_by_field_name("method")?),
            "bracket_index_expression" => string_key(base, target.child_by_field_name("field")?)?,
            _ => return None,
        };
        let (owner, root) = table_path(base, target.child_by_field_name("table")?, depth)?;
        let owner = (owner != "_G" || root_binding(base, root).is_some()).then_some(owner);
        Some(Self {
            owner,
            name: member,
            root,
        })
    }

    fn text(&self) -> String {
        joined_path(self.owner.as_deref(), &self.name)
    }
}

fn joined_path(owner: Option<&str>, name: &str) -> String {
    owner.map_or_else(|| name.to_string(), |owner| format!("{owner}.{name}"))
}

fn table_path<'tree>(
    base: &BaseExtractor,
    table: Node<'tree>,
    depth: u32,
) -> Option<(String, Node<'tree>)> {
    let depth = child_tree_depth(depth)?;
    if table.kind() != "identifier" {
        let path = TargetPath::of(base, table, depth)?;
        return Some((path.text(), path.root));
    }
    let text = base.get_node_text(&table);
    if text == "self"
        && !scope::is_local_binding_in_scope(base, table, "self")
        && let Some(owner) = scope::outer_colon_owner_table(table).filter(|owner| {
            written_root(*owner).is_none_or(|root| base.get_node_text(&root) != "self")
        })
    {
        return table_path(base, owner, depth);
    }
    Some((text, table))
}

/// The text of a string key such as `"name"` in `X["name"]` or `{ ["name"] = .. }`.
fn string_key(base: &BaseExtractor, key: Node) -> Option<String> {
    match key.kind() {
        "identifier" => Some(base.get_node_text(&key)),
        "string" => Some(base.get_node_text(&key.child_by_field_name("content")?)),
        _ => None,
    }
}

/// Start byte of the declaration that binds the identifier `root` where it
/// appears; `None` when it is global there.
fn root_binding(base: &BaseExtractor, root: Node) -> Option<usize> {
    scope::innermost_local_binding(base, root, &base.get_node_text(&root))
        .map(|declaration| declaration.start_byte())
}

/// How many variable, parameter, import, and class symbols bind each name.
fn binding_counts(symbols: &[Symbol]) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for symbol in symbols.iter().filter(|symbol| {
        matches!(
            symbol.kind,
            SymbolKind::Variable | SymbolKind::Constant | SymbolKind::Import | SymbolKind::Class
        )
    }) {
        *counts.entry(symbol.name.clone()).or_insert(0) += 1;
    }
    counts
}

/// Type parameter names from every `---@generic` tag and `---@class Name<T>`
/// annotation in the file.
fn generic_names(symbols: &[Symbol]) -> HashSet<String> {
    let mut names = HashSet::new();
    for doc in symbols
        .iter()
        .filter_map(|symbol| symbol.doc_comment.as_deref())
    {
        for (tag, rest) in annotation_tags(doc) {
            let parameters = match tag {
                "generic" => rest,
                "class" => rest
                    .split_once('<')
                    .and_then(|(_, rest)| rest.split_once('>'))
                    .map_or("", |(parameters, _)| parameters),
                _ => continue,
            };
            names.extend(
                parameters
                    .split(',')
                    .filter_map(|parameter| parameter.split(':').next())
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_string),
            );
        }
    }
    names
}

/// A usable `---@return` type: the base name and the annotated text.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ReturnType {
    name: String,
    declared: String,
}

/// Annotated return types of the file's functions, keyed by name, and the
/// assignment sites that give a path a value that is not a function.
#[derive(Debug, Default)]
struct ReturnTypeIndex {
    entries: HashMap<String, Vec<ReturnEntry>>,
    /// Keyed by the target path: `M`, `M.get`, `View.update` for
    /// `self.update` in a colon method of `View`.
    value_sites: HashMap<String, Vec<ValueSite>>,
}

#[derive(Debug)]
struct ReturnEntry {
    /// The owner table path of `function M.name` / `function M:name` /
    /// `M.name = function` / `M = { name = function }`; `None` for a free function.
    owner: Option<String>,
    /// Start byte of the declaration that binds the free name, or the owner's
    /// root name, at the definition; `None` when that name is global.
    binding: Option<usize>,
    start: usize,
    /// `None` when the function has no usable `---@return` annotation.
    returns: Option<ReturnType>,
    /// True when the doc has a `---@return` or `---@overload` tag.
    annotated: bool,
}

#[derive(Debug)]
struct ValueSite {
    start: usize,
    /// End byte of the function that holds the site; `usize::MAX` at top level.
    scope_end: usize,
    new_table: bool,
}

/// One value that an assignment, `local` declaration, or table field gives to
/// the path `owner.name`.
struct AssignedValue<'tree> {
    owner: Option<String>,
    name: String,
    binding: Option<usize>,
    site: Node<'tree>,
    value: Option<Node<'tree>>,
    doc: Option<String>,
}

impl ReturnTypeIndex {
    fn build(base: &BaseExtractor, root: Node, generics: &HashSet<String>) -> Self {
        let mut index = Self::default();
        index.collect(base, root, generics, 0);
        index
    }

    fn collect(
        &mut self,
        base: &BaseExtractor,
        node: Node,
        generics: &HashSet<String>,
        depth: u32,
    ) {
        if !should_visit_tree_depth(depth) {
            return;
        }
        match node.kind() {
            "function_declaration" | "function_definition_statement" => {
                if let Some(path) = node
                    .child_by_field_name("name")
                    .and_then(|target| TargetPath::of(base, target, 0))
                {
                    let doc = helpers::doc_comment(base, &node);
                    let binding = root_binding(base, path.root);
                    self.insert(path.owner, path.name, binding, node, doc, generics);
                }
            }
            "variable_declaration"
                if helpers::find_child_by_type(&node, "assignment_statement").is_none() =>
            {
                if let Some(variable_list) = helpers::find_child_by_type(&node, "variable_list") {
                    for target in
                        variable_list.children_by_field_name("name", &mut variable_list.walk())
                    {
                        self.add_value_site(base.get_node_text(&target), target, false);
                    }
                }
            }
            "assignment_statement" => self.collect_assignment(base, node, generics, depth),
            _ => {}
        }
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect(base, child, generics, child_depth);
        }
    }

    /// Index each target of an assignment; the doc comment applies only when
    /// the statement binds exactly one name.
    fn collect_assignment(
        &mut self,
        base: &BaseExtractor,
        assignment: Node,
        generics: &HashSet<String>,
        depth: u32,
    ) {
        let Some(variable_list) = helpers::find_child_by_type(&assignment, "variable_list") else {
            return;
        };
        let targets: Vec<Node> = variable_list
            .children_by_field_name("name", &mut variable_list.walk())
            .collect();
        let expressions: Vec<Node> = helpers::find_child_by_type(&assignment, "expression_list")
            .map(super::variables::collect_expression_nodes)
            .unwrap_or_default();
        let declaration = assignment
            .parent()
            .filter(|parent| parent.kind() == "variable_declaration");
        let doc = (targets.len() == 1)
            .then(|| helpers::doc_comment(base, &declaration.unwrap_or(assignment)))
            .flatten();
        for (index, target) in targets.into_iter().enumerate() {
            let Some(path) = TargetPath::of(base, target, 0) else {
                continue;
            };
            let binding = match declaration {
                Some(declaration) => Some(declaration.start_byte()),
                None => root_binding(base, path.root),
            };
            let assigned = AssignedValue {
                owner: path.owner,
                name: path.name,
                binding,
                site: target,
                value: expressions.get(index).copied(),
                doc: doc.clone(),
            };
            self.add_value(base, assigned, generics, depth);
        }
    }

    /// Index a function value as an entry and any other value as a value site.
    /// The named fields of a table constructor are indexed under its path.
    fn add_value(
        &mut self,
        base: &BaseExtractor,
        assigned: AssignedValue,
        generics: &HashSet<String>,
        depth: u32,
    ) {
        if assigned
            .value
            .is_some_and(|value| value.kind() == "function_definition")
        {
            self.insert(
                assigned.owner,
                assigned.name,
                assigned.binding,
                assigned.site,
                assigned.doc,
                generics,
            );
            return;
        }
        let path = joined_path(assigned.owner.as_deref(), &assigned.name);
        let new_table = assigned
            .value
            .is_some_and(|value| is_new_table(base, value));
        self.add_value_site(path.clone(), assigned.site, new_table);
        let Some(table) = assigned
            .value
            .filter(|value| value.kind() == "table_constructor")
        else {
            return;
        };
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = table.walk();
        for field in table
            .named_children(&mut cursor)
            .filter(|child| child.kind() == "field")
        {
            let Some(name) = field
                .child_by_field_name("name")
                .and_then(|key| string_key(base, key))
            else {
                continue;
            };
            let field_value = AssignedValue {
                owner: Some(path.clone()),
                name,
                binding: assigned.binding,
                site: field,
                value: field.child_by_field_name("value"),
                doc: helpers::doc_comment(base, &field),
            };
            self.add_value(base, field_value, generics, child_depth);
        }
    }

    fn add_value_site(&mut self, path: String, site: Node, new_table: bool) {
        let scope_end = std::iter::successors(site.parent(), Node::parent)
            .find(|ancestor| {
                matches!(
                    ancestor.kind(),
                    "function_declaration" | "function_definition"
                )
            })
            .map_or(usize::MAX, |function| function.end_byte());
        self.value_sites.entry(path).or_default().push(ValueSite {
            start: site.start_byte(),
            scope_end,
            new_table,
        });
    }

    fn insert(
        &mut self,
        owner: Option<String>,
        name: String,
        binding: Option<usize>,
        definition: Node,
        doc: Option<String>,
        generics: &HashSet<String>,
    ) {
        let annotated = doc.as_deref().is_some_and(|doc| {
            annotation_tags(doc).any(|(tag, _)| matches!(tag, "return" | "overload"))
        });
        let returns = doc.and_then(|doc| declared_return(&doc, owner.as_deref(), generics));
        self.entries.entry(name).or_default().push(ReturnEntry {
            owner,
            binding,
            start: definition.start_byte(),
            returns,
            annotated,
        });
    }

    /// The return type every same-named function with this owner and root
    /// binding agrees on. A member that the file also assigns a value that is
    /// not a function records nothing, as does an owner path that is not
    /// [stable](Self::owner_path_is_stable).
    fn lookup(
        &self,
        name: &str,
        owner: Option<&str>,
        binding: Option<usize>,
    ) -> Option<&ReturnType> {
        if self.value_sites.contains_key(&joined_path(owner, name)) {
            return None;
        }
        let candidates: Vec<&ReturnEntry> = self
            .entries
            .get(name)?
            .iter()
            .filter(|entry| entry.owner.as_deref() == owner && entry.binding == binding)
            .collect();
        if let Some(owner) = owner {
            let mut paths = owner
                .match_indices('.')
                .map(|(end, _)| &owner[..end])
                .chain(std::iter::once(owner));
            if !paths.all(|path| self.owner_path_is_stable(path, &candidates)) {
                return None;
            }
        }
        let first = candidates.first()?.returns.as_ref()?;
        candidates
            .iter()
            .all(|entry| entry.returns.as_ref() == Some(first))
            .then_some(first)
    }

    /// True when the file never assigns the owner path, or assigns it once
    /// to a new table before every definition, in a scope that holds them.
    fn owner_path_is_stable(&self, path: &str, definitions: &[&ReturnEntry]) -> bool {
        match self.value_sites.get(path).map(Vec::as_slice) {
            None => true,
            Some([site]) => {
                site.new_table
                    && definitions
                        .iter()
                        .all(|entry| site.start < entry.start && entry.start < site.scope_end)
            }
            Some(_) => false,
        }
    }
}

/// `{ .. }` or `setmetatable({ .. }, mt)`, which returns its new first argument.
fn is_new_table(base: &BaseExtractor, value: Node) -> bool {
    match value.kind() {
        "table_constructor" => true,
        "function_call" => {
            let is_setmetatable = value.child_by_field_name("name").is_some_and(|name| {
                name.kind() == "identifier" && base.get_node_text(&name) == "setmetatable"
            });
            let first_argument = value
                .child_by_field_name("arguments")
                .and_then(|arguments| arguments.named_child(0));
            is_setmetatable
                && first_argument.is_some_and(|argument| argument.kind() == "table_constructor")
        }
        _ => false,
    }
}

/// The first `---@return` type of a function without `---@overload`. `self`
/// names the owner table; generic parameters, `any`, `unknown`, and types
/// that are not a plain (dotted) name record nothing.
fn declared_return(
    doc: &str,
    owner: Option<&str>,
    generics: &HashSet<String>,
) -> Option<ReturnType> {
    let mut returns = None;
    for (tag, rest) in annotation_tags(doc) {
        match tag {
            "overload" => return None,
            "return" if returns.is_none() => returns = Some(rest),
            _ => {}
        }
    }
    let declared = first_return_type(returns?)?;
    let base_text = annotated_base(declared)?;
    let name = strip_type_decorations(base_text, &ANNOTATION_TYPE_RULES);
    let name = match name.as_str() {
        "self" => owner?.rsplit('.').next()?.to_string(),
        "any" | "unknown" | "true" | "false" => return None,
        _ if generics.contains(&name) => return None,
        _ => name,
    };
    let is_plain_name = name.split('.').all(|segment| {
        segment
            .chars()
            .next()
            .is_some_and(|first| first.is_alphabetic() || first == '_')
            && segment.chars().all(|ch| ch.is_alphanumeric() || ch == '_')
    });
    is_plain_name.then(|| ReturnType {
        name,
        declared: declared.to_string(),
    })
}

fn constructor_type_name(base: &BaseExtractor, expression: Node) -> Option<String> {
    if expression.kind() != "function_call" {
        return None;
    }
    let name = expression.child_by_field_name("name")?;
    let member_field = match name.kind() {
        "dot_index_expression" => Some("field"),
        "method_index_expression" => Some("method"),
        _ => None,
    };
    if let Some(member_field) = member_field {
        let member = name.child_by_field_name(member_field)?;
        if base.get_node_text(&member) != "new" {
            return None;
        }
        return identifier_table_name(base, name);
    }
    if name.kind() == "identifier" && base.get_node_text(&name) != "setmetatable" {
        return Some(base.get_node_text(&name));
    }
    if name.kind() == "identifier" && base.get_node_text(&name) == "setmetatable" {
        let arguments = expression.child_by_field_name("arguments")?;
        let mut cursor = arguments.walk();
        let args: Vec<Node> = arguments.named_children(&mut cursor).collect();
        if args.len() >= 2
            && args[0].kind() == "table_constructor"
            && args[1].kind() == "identifier"
        {
            return Some(base.get_node_text(&args[1]));
        }
    }
    None
}

fn symbol_for_name_node<'a>(
    symbols: &'a [Symbol],
    name: &str,
    name_node: Node,
) -> Option<&'a Symbol> {
    let start_line = name_node.start_position().row as u32 + 1;
    let start_column = name_node.start_position().column as u32;
    symbols.iter().find(|symbol| {
        symbol.name == name
            && symbol.start_line == start_line
            && symbol.start_column == start_column
    })
}

const ANNOTATION_TYPE_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &["?"],
    reference_prefixes: &[],
    generic_open: &['<'],
};

/// Record declared type facts from LuaLS / LuaCATS doc annotations:
/// `---@param name Type` on parameters, `---@return Type` on the function, and
/// `---@type Type` on variables and fields.
pub(super) fn record_annotation_facts(base: &mut BaseExtractor, symbols: &[Symbol]) {
    for symbol in symbols {
        let Some(doc) = symbol.doc_comment.as_deref() else {
            continue;
        };
        for (tag, rest) in annotation_tags(doc) {
            match (tag, &symbol.kind) {
                ("param", SymbolKind::Function | SymbolKind::Method) => {
                    let Some((name, type_text)) = rest.split_once(char::is_whitespace) else {
                        continue;
                    };
                    let name = name.trim_end_matches('?');
                    let optional = rest
                        .split_whitespace()
                        .next()
                        .is_some_and(|n| n.ends_with('?'));
                    let parameter = symbols.iter().find(|candidate| {
                        candidate.name == name
                            && candidate.parent_id.as_deref() == Some(symbol.id.as_str())
                    });
                    if let (Some(parameter), Some(type_text)) =
                        (parameter, leading_type(type_text.trim_start()))
                    {
                        let declared = if optional {
                            format!("{type_text}?")
                        } else {
                            type_text.to_string()
                        };
                        record_annotated_type(base, &parameter.id, &declared);
                    }
                }
                ("return", SymbolKind::Function | SymbolKind::Method) => {
                    if let Some(type_text) = first_return_type(rest) {
                        record_annotated_type(base, &symbol.id, type_text);
                    }
                }
                ("type", SymbolKind::Variable | SymbolKind::Field | SymbolKind::Constant) => {
                    if let Some(type_text) = leading_type(rest) {
                        record_annotated_type(base, &symbol.id, type_text);
                    }
                }
                _ => {}
            }
        }
    }
}

fn annotation_tags(doc: &str) -> impl Iterator<Item = (&str, &str)> {
    doc.lines().filter_map(|line| {
        let rest = line.trim_start().trim_start_matches('-').trim_start();
        let rest = rest.strip_prefix('@')?;
        let (tag, rest) = rest.split_once(char::is_whitespace)?;
        Some((tag, rest.trim()))
    })
}

/// The type expression at the start of `text`: brackets and parentheses nest,
/// whitespace around a top-level `|`, `,`, or `:` stays inside it, and the
/// first other top-level whitespace (or `#` comment marker) ends it.
fn leading_type(text: &str) -> Option<&str> {
    let mut depth = 0i32;
    for (index, ch) in text.char_indices() {
        match ch {
            '(' | '<' | '[' | '{' => depth += 1,
            ')' | '>' | ']' | '}' => depth -= 1,
            '#' if depth == 0 => return non_empty(&text[..index]),
            c if c.is_whitespace() && depth == 0 => {
                if text[..index].trim_end().ends_with([',', ':', '|'])
                    || text[index..].trim_start().starts_with('|')
                {
                    continue;
                }
                return non_empty(&text[..index]);
            }
            _ => {}
        }
    }
    non_empty(text)
}

fn non_empty(text: &str) -> Option<&str> {
    let text = text.trim();
    (!text.is_empty()).then_some(text)
}

/// The first type of a `---@return` tag: the leading type expression, cut at
/// its first top-level comma (`---@return Foo, string` returns two values).
fn first_return_type(rest: &str) -> Option<&str> {
    let leading = leading_type(rest)?;
    let mut depth = 0i32;
    for (index, ch) in leading.char_indices() {
        match ch {
            '(' | '<' | '[' | '{' => depth += 1,
            ')' | '>' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => return non_empty(&leading[..index]),
            _ => {}
        }
    }
    Some(leading)
}

/// The single type an annotation names. A union drops `nil`; a union of
/// several other types names nothing, and a `fun(...)` type names `function`.
fn annotated_base(declared: &str) -> Option<&str> {
    let mut depth = 0i32;
    let mut members = Vec::new();
    let mut start = 0;
    for (index, ch) in declared.char_indices() {
        match ch {
            '(' | '<' | '[' | '{' => depth += 1,
            ')' | '>' | ']' | '}' => depth -= 1,
            '|' if depth == 0 => {
                members.push(declared[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    members.push(declared[start..].trim());
    let non_nil: Vec<&str> = members
        .into_iter()
        .filter(|member| !member.is_empty() && *member != "nil")
        .collect();
    let [single] = non_nil.as_slice() else {
        return None;
    };
    Some(if single.starts_with("fun(") {
        "function"
    } else {
        single
    })
}

fn record_annotated_type(base: &mut BaseExtractor, symbol_id: &str, declared: &str) {
    let Some(base_text) = annotated_base(declared) else {
        return;
    };
    base.record_declared_type_fact_with_declared(
        symbol_id,
        base_text,
        declared,
        &ANNOTATION_TYPE_RULES,
        false,
    );
}
