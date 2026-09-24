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
/// call: first the constructor patterns of a same-file class, then a
/// same-file function with an annotated `---@return` type.
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
        if let Some(type_name) = constructor_type_name(base, expression)
            .filter(|type_name| scope.class_names.contains(type_name))
        {
            base.record_declared_type_fact(&symbol.id, &type_name, &TYPE_NAME_RULES, true);
        } else if let Some(returned) = scope.call_return_type(base, expression) {
            base.record_declared_type_fact_with_declared(
                &symbol.id,
                &returned.name,
                &returned.declared,
                &ANNOTATION_TYPE_RULES,
                true,
            );
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
        let (name, owner, root) = match callee.kind() {
            "identifier" => {
                let name = base.get_node_text(&callee);
                if self.bindings.contains_key(&name) {
                    return None;
                }
                (name, None, callee)
            }
            "dot_index_expression" | "method_index_expression" => {
                let member_field = if callee.kind() == "dot_index_expression" {
                    "field"
                } else {
                    "method"
                };
                let member = base.get_node_text(&callee.child_by_field_name(member_field)?);
                let table = callee.child_by_field_name("table")?;
                let owner_table =
                    if table.kind() == "identifier" && base.get_node_text(&table) == "self" {
                        if scope::is_local_binding_in_scope(base, table, "self") {
                            return None;
                        }
                        scope::enclosing_colon_owner_table(table)?
                    } else {
                        table
                    };
                let root = owner_root(owner_table)?;
                let root_name = base.get_node_text(&root);
                if root_name == "self"
                    || self
                        .bindings
                        .get(&root_name)
                        .is_some_and(|count| *count > 1)
                {
                    return None;
                }
                (member, Some(base.get_node_text(&owner_table)), root)
            }
            _ => return None,
        };
        let binding = root_binding(base, root);
        self.return_types.lookup(&name, owner.as_deref(), binding)
    }
}

/// The identifier at the root of an `a.b.c` owner table.
fn owner_root(table: Node) -> Option<Node> {
    match table.kind() {
        "identifier" => Some(table),
        "dot_index_expression" => owner_root(table.child_by_field_name("table")?),
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
/// assignment sites that give a name a value that is not a function.
#[derive(Debug, Default)]
struct ReturnTypeIndex {
    entries: HashMap<String, Vec<ReturnEntry>>,
    /// Keyed by target text: `M`, `M.get`.
    value_sites: HashMap<String, usize>,
}

#[derive(Debug)]
struct ReturnEntry {
    /// The table text of `function M.name` / `function M:name` / `M.name = function`;
    /// `None` for a free function.
    owner: Option<String>,
    /// Start byte of the declaration that binds the free name, or the owner's
    /// root name, at the definition; `None` when that name is global.
    binding: Option<usize>,
    /// `None` when the function has no usable `---@return` annotation.
    returns: Option<ReturnType>,
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
                if let Some(target) = node.child_by_field_name("name") {
                    let doc = helpers::doc_comment(base, &node);
                    self.insert(base, target, None, doc.as_deref(), generics);
                }
            }
            "variable_declaration"
                if helpers::find_child_by_type(&node, "assignment_statement").is_none() =>
            {
                if let Some(variable_list) = helpers::find_child_by_type(&node, "variable_list") {
                    for target in
                        variable_list.children_by_field_name("name", &mut variable_list.walk())
                    {
                        self.add_value_site(base, target);
                    }
                }
            }
            "assignment_statement" => self.collect_assignment(base, node, generics),
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

    /// Index `name = function ... end` targets and count every other target
    /// as a value site; the doc comment applies only when the statement binds
    /// exactly one name.
    fn collect_assignment(
        &mut self,
        base: &BaseExtractor,
        assignment: Node,
        generics: &HashSet<String>,
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
            if expressions
                .get(index)
                .is_some_and(|expression| expression.kind() == "function_definition")
            {
                self.insert(base, target, declaration, doc.as_deref(), generics);
            } else {
                self.add_value_site(base, target);
            }
        }
    }

    fn add_value_site(&mut self, base: &BaseExtractor, target: Node) {
        *self
            .value_sites
            .entry(base.get_node_text(&target))
            .or_insert(0) += 1;
    }

    /// `declaration` is the `local` statement that declares an identifier target.
    fn insert(
        &mut self,
        base: &BaseExtractor,
        target: Node,
        declaration: Option<Node>,
        doc: Option<&str>,
        generics: &HashSet<String>,
    ) {
        let (name, owner, root) = match target.kind() {
            "identifier" => (base.get_node_text(&target), None, target),
            "dot_index_expression" | "method_index_expression" => {
                let member_field = if target.kind() == "dot_index_expression" {
                    "field"
                } else {
                    "method"
                };
                let Some(member) = target.child_by_field_name(member_field) else {
                    return;
                };
                let Some(table) = target.child_by_field_name("table") else {
                    return;
                };
                let Some(root) = owner_root(table) else {
                    return;
                };
                (
                    base.get_node_text(&member),
                    Some(base.get_node_text(&table)),
                    root,
                )
            }
            _ => return,
        };
        let binding = match declaration {
            Some(declaration) if owner.is_none() => Some(declaration.start_byte()),
            _ => root_binding(base, root),
        };
        let returns = doc.and_then(|doc| declared_return(doc, owner.as_deref(), generics));
        self.entries.entry(name).or_default().push(ReturnEntry {
            owner,
            binding,
            returns,
        });
    }

    /// The return type every same-named function with this owner and root
    /// binding agrees on. A target or owner path that the file also assigns
    /// a non-function value (more than the one owner table) records nothing.
    fn lookup(
        &self,
        name: &str,
        owner: Option<&str>,
        binding: Option<usize>,
    ) -> Option<&ReturnType> {
        let target = owner.map_or_else(|| name.to_string(), |owner| format!("{owner}.{name}"));
        if self.value_sites.contains_key(&target) {
            return None;
        }
        if let Some(owner) = owner {
            let mut paths = owner
                .match_indices('.')
                .map(|(end, _)| &owner[..end])
                .chain(std::iter::once(owner));
            if paths.any(|path| self.value_sites.get(path).is_some_and(|count| *count > 1)) {
                return None;
            }
        }
        let mut returns = self
            .entries
            .get(name)?
            .iter()
            .filter(|entry| entry.owner.as_deref() == owner && entry.binding == binding)
            .map(|entry| entry.returns.as_ref());
        let first = returns.next()??;
        returns.all(|other| other == Some(first)).then_some(first)
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
