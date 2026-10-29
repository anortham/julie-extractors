//! The static type of each argument of a VB.NET call, so a reader can tell
//! which `Overloads` member the call binds: `Run(1)` binds
//! `Run(a As Integer)`, and `Run("x")` binds `Run(a As String)`.

use tree_sitter::{Node, Tree};

use crate::base::argument_types::{
    CallArguments, ScopeBindings, annotate_calls, compact_type_text,
};
use crate::base::call_arguments::vbnet_omits_argument;
use crate::base::{BaseExtractor, Identifier, IdentifierKind};

const TYPE_BLOCKS: [&str; 4] = [
    "class_block",
    "structure_block",
    "module_block",
    "interface_block",
];

/// Clauses whose statements do not see the statements written before the
/// clause in the same block: the `Then` part of an `If` is not in scope in its
/// `Else` part.
const SEPARATE_CLAUSES: [&str; 6] = [
    "elseif_clause",
    "else_clause",
    "catch_block",
    "finally_block",
    "case_block",
    "case_else_block",
];

/// The arguments of a construction `New T(...)`, which the grammar reads in
/// three ways: an argument list after the type, the rank of an array type
/// `T(1)` without an array initializer, or an index of a bare `New T`
/// (`Dim q = New T(2)`).
pub(super) struct Construction<'tree> {
    /// The bytes from `(` to `)`.
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) type_name: String,
    /// The `argument` nodes of an argument list, or the expressions of a rank
    /// or an index.
    arguments: Vec<Node<'tree>>,
}

pub(super) fn construction<'tree>(node: Node<'tree>, source: &str) -> Option<Construction<'tree>> {
    let (type_node, open, close, arguments) = match node.kind() {
        "new_expression" => {
            let type_node = node.child_by_field_name("type")?;
            let mut cursor = node.walk();
            let children: Vec<Node> = node.named_children(&mut cursor).collect();
            if let Some(list) = children
                .iter()
                .find(|child| child.kind() == "argument_list")
            {
                let mut cursor = list.walk();
                let arguments: Vec<Node> = list
                    .named_children(&mut cursor)
                    .filter(|child| !child.is_extra())
                    .collect();
                (type_node, *list, *list, arguments)
            } else {
                let has_initializer = children
                    .iter()
                    .any(|child| child.kind() == "object_initializers");
                if type_node.kind() != "array_type" || has_initializer {
                    return None;
                }
                let rank = type_node.child_by_field_name("rank")?;
                let element = type_node.child_by_field_name("element")?;
                let mut cursor = rank.walk();
                let arguments: Vec<Node> = rank
                    .named_children(&mut cursor)
                    .filter(|child| !child.is_extra())
                    .collect();
                (element, rank, rank, arguments)
            }
        }
        "element_access" => {
            let object = node.child_by_field_name("object")?;
            if object.kind() != "new_expression" || !is_bare_new(object) {
                return None;
            }
            let mut cursor = node.walk();
            let children: Vec<Node> = node.children(&mut cursor).collect();
            let open = *children.iter().find(|child| child.kind() == "(")?;
            let close = *children.iter().rev().find(|child| child.kind() == ")")?;
            let arguments = (0..node.child_count())
                .filter(|&index| node.field_name_for_child(index as u32) == Some("index"))
                .filter_map(|index| node.child(index as u32))
                .collect();
            (object.child_by_field_name("type")?, open, close, arguments)
        }
        _ => return None,
    };
    let text = source.get(type_node.byte_range())?;
    let without_arguments = text.split("(Of").next()?.split('(').next()?;
    let type_name = without_arguments
        .rsplit('.')
        .next()?
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();
    if type_name.is_empty() {
        return None;
    }
    Some(Construction {
        start: open.start_byte(),
        end: close.end_byte(),
        type_name,
        arguments,
    })
}

/// The construction whose argument bytes are `start..end`.
fn construction_at<'tree>(
    root: Node<'tree>,
    start: usize,
    end: usize,
    source: &str,
) -> Option<Construction<'tree>> {
    let mut node = root.descendant_for_byte_range(start, end)?;
    for _ in 0..3 {
        if let Some(found) = construction(node, source)
            && found.start == start
            && found.end == end
        {
            return Some(found);
        }
        node = node.parent()?;
    }
    None
}

/// Adds `arg_count` to each construction call identifier, then `arg_types` to
/// each VB.NET call identifier that has `arg_count`: one entry per argument,
/// the type its expression states, or `null`. A call with a named argument
/// (`Run(a:=1)`) gets none, since the name decides which parameter an
/// argument fills.
pub(crate) fn enrich(base: &BaseExtractor, tree: &Tree, identifiers: &mut [Identifier]) {
    let reader = Reader {
        base,
        bindings: ScopeBindings::new(true),
    };
    let root = tree.root_node();
    let source = base.content.as_str();
    for identifier in identifiers.iter_mut() {
        if identifier.kind != IdentifierKind::Call || identifier.language != "vbnet" {
            continue;
        }
        let Some(construction) = construction_at(
            root,
            identifier.start_byte as usize,
            identifier.end_byte as usize,
            source,
        ) else {
            continue;
        };
        if vbnet_omits_argument(
            construction.start + 1,
            construction.end.saturating_sub(1),
            &construction.arguments,
            source.as_bytes(),
        ) {
            continue;
        }
        let metadata = identifier.metadata.get_or_insert_with(Default::default);
        metadata.insert("arg_count".to_string(), construction.arguments.len().into());
        if let Some(arg_types) = reader.values_types(&construction.arguments) {
            metadata.insert("arg_types".to_string(), arg_types.into());
        }
    }
    annotate_calls("vbnet", tree, identifiers, |node, identifier| {
        Some(CallArguments {
            types: reader.argument_types(argument_list(node, identifier)?)?,
            names: None,
        })
    });
}

fn argument_list<'tree>(node: Node<'tree>, identifier: &Identifier) -> Option<Node<'tree>> {
    let mut current = node;
    while current.kind() != "invocation" {
        current = current.parent()?;
    }
    let list = current.child_by_field_name("arguments")?;
    (list.start_byte() >= identifier.end_byte as usize).then_some(list)
}

/// What a scope says about a name: it binds the name, with this type or an
/// unknown one, or the search goes on outward.
enum Lookup {
    Bound(Option<String>),
    Next,
}

/// The names one declaration binds, with the type each one gets.
struct Group<'tree> {
    names: Vec<Node<'tree>>,
    type_node: Option<Node<'tree>>,
    value: Option<Node<'tree>>,
    array: bool,
}

struct Reader<'a, 'tree> {
    base: &'a BaseExtractor,
    bindings: ScopeBindings<'tree, Option<String>>,
}

impl<'tree> Reader<'_, 'tree> {
    fn text(&self, node: Node<'tree>) -> String {
        self.base.get_node_text(&node)
    }

    fn same_name(&self, node: Node<'tree>, name: &str) -> bool {
        identifier_key(&self.text(node)) == name
    }

    fn argument_types(&self, list: Node<'tree>) -> Option<Vec<Option<String>>> {
        let mut cursor = list.walk();
        let arguments: Vec<Node> = list
            .named_children(&mut cursor)
            .filter(|child| child.kind() == "argument")
            .collect();
        self.values_types(&arguments)
    }

    /// The types of `argument` nodes, or of bare expressions.
    fn values_types(&self, arguments: &[Node<'tree>]) -> Option<Vec<Option<String>>> {
        let mut found = Vec::new();
        for argument in arguments {
            if argument.kind() != "argument" {
                found.push(self.type_of(*argument));
                continue;
            }
            if argument.child_by_field_name("name").is_some() {
                return None;
            }
            let value = argument
                .named_children(&mut argument.walk())
                .filter(|child| !child.is_extra())
                .last();
            found.push(value.and_then(|value| self.type_of(value)));
        }
        Some(found)
    }

    fn type_of(&self, node: Node<'tree>) -> Option<String> {
        let text = || self.text(node);
        let name = match node.kind() {
            "parenthesized_expression" => {
                let inner = node
                    .named_children(&mut node.walk())
                    .find(|child| !child.is_extra())?;
                return self.type_of(inner);
            }
            "integer_literal" => integer_type(&text())?,
            "floating_point_literal" => floating_type(&text())?,
            "string_literal" | "interpolated_string_literal" => "String",
            "character_literal" => "Char",
            "boolean_literal" => "Boolean",
            "date_literal" => "Date",
            "gettype_expression" => "System.Type",
            "nameof_expression" => "String",
            "conversion_expression" => conversion_type(text().split('(').next()?.trim())?,
            "cast_expression" => {
                return Some(compact_type_text(
                    &self.text(node.child_by_field_name("type")?),
                ));
            }
            "new_expression" => return self.constructed_type(node),
            "element_access" => {
                let object = node.child_by_field_name("object")?;
                return (object.kind() == "new_expression" && is_bare_new(object))
                    .then(|| self.constructed_type(object))?;
            }
            "unary_expression" => {
                let operator = self.text(node.child_by_field_name("operator")?);
                let operand = node.child_by_field_name("operand")?;
                let numeric =
                    matches!(operand.kind(), "integer_literal" | "floating_point_literal");
                return (numeric && matches!(operator.as_str(), "-" | "+"))
                    .then(|| self.type_of(operand))?;
            }
            "me_expression" => {
                if text().eq_ignore_ascii_case("MyBase") {
                    return None;
                }
                return self.enclosing_type_name(node);
            }
            "identifier" => return self.value_type(&identifier_key(&text()), node),
            _ => return None,
        };
        Some(name.to_string())
    }

    /// The type `New T(...)` builds. The grammar reads `New T(1)` as the array
    /// type `T(1)`; without an array initializer it is a constructor call.
    fn constructed_type(&self, node: Node<'tree>) -> Option<String> {
        let type_node = node.child_by_field_name("type")?;
        let has_initializer = node
            .named_children(&mut node.walk())
            .any(|child| child.kind() == "object_initializers");
        if type_node.kind() == "array_type" {
            if has_initializer {
                return None;
            }
            let element = type_node.child_by_field_name("element")?;
            return Some(compact_type_text(&self.text(element)));
        }
        Some(compact_type_text(&self.text(type_node)))
    }

    fn enclosing_type_name(&self, node: Node<'tree>) -> Option<String> {
        let mut current = node.parent();
        while let Some(scope) = current {
            if TYPE_BLOCKS.contains(&scope.kind()) {
                return scope
                    .child_by_field_name("name")
                    .map(|name| self.text(name));
            }
            current = scope.parent();
        }
        None
    }

    /// The declared type of the local, parameter, or member that `name`
    /// (lower case, without brackets) names at `at`. The search walks out
    /// from `at`, and the first scope that binds the name decides. A type
    /// block that does not declare the name ends the search: a base type or
    /// a module of the namespace can declare it.
    fn value_type(&self, name: &str, at: Node<'tree>) -> Option<String> {
        let mut child = at;
        while let Some(scope) = child.parent() {
            if let Lookup::Bound(found) = self.scope_lookup(scope, child, name) {
                return found;
            }
            if TYPE_BLOCKS.contains(&scope.kind()) {
                return None;
            }
            child = scope;
        }
        None
    }

    fn scope_lookup(&self, scope: Node<'tree>, child: Node<'tree>, name: &str) -> Lookup {
        if TYPE_BLOCKS.contains(&scope.kind()) {
            return self.scope_binding(scope, None, name);
        }
        if !SEPARATE_CLAUSES.contains(&child.kind())
            && let Lookup::Bound(found) = self.scope_binding(scope, Some(child.start_byte()), name)
        {
            return Lookup::Bound(found);
        }
        self.header_lookup(scope, name)
    }

    /// A name the head of a block binds: a parameter, a loop variable, a
    /// `Using` resource, a `Catch` variable, or a lambda parameter.
    fn header_lookup(&self, scope: Node<'tree>, name: &str) -> Lookup {
        if let Some(parameters) = scope.child_by_field_name("parameters") {
            let mut cursor = parameters.walk();
            for parameter in parameters.named_children(&mut cursor) {
                if let Lookup::Bound(found) = self.group_lookup(&groups(parameter), name) {
                    return Lookup::Bound(found);
                }
            }
        }
        match scope.kind() {
            "for_statement" | "for_each_statement" => {
                let Some(variable) = scope.child_by_field_name("variable") else {
                    return Lookup::Next;
                };
                if !self.same_name(variable, name) {
                    return Lookup::Next;
                }
                let type_node = scope
                    .named_children(&mut scope.walk())
                    .find(|child| child.kind() == "as_clause")
                    .and_then(|clause| clause.child_by_field_name("type"));
                Lookup::Bound(type_node.map(|node| compact_type_text(&self.text(node))))
            }
            "using_statement" => self.group_lookup(&groups(scope), name),
            "catch_block" => match scope.child_by_field_name("exception") {
                Some(exception) if self.same_name(exception, name) => Lookup::Bound(
                    scope
                        .child_by_field_name("type")
                        .map(|node| compact_type_text(&self.text(node))),
                ),
                _ => Lookup::Next,
            },
            "lambda_expression" => {
                let mut cursor = scope.walk();
                for parameter in scope.named_children(&mut cursor) {
                    if parameter.kind() == "lambda_parameter"
                        && let Lookup::Bound(found) = self.group_lookup(&groups(parameter), name)
                    {
                        return Lookup::Bound(found);
                    }
                }
                Lookup::Next
            }
            _ => Lookup::Next,
        }
    }

    /// The binding of `name` by the last child of `scope` that binds it: a
    /// `Dim` or `Const` statement that ends before the byte `before`, or any
    /// member of a type block when `before` is `None`.
    fn scope_binding(&self, scope: Node<'tree>, before: Option<usize>, name: &str) -> Lookup {
        let binding =
            self.bindings
                .last_binding(&self.base.content, scope, before, name, |statement| {
                    if before.is_some() && !is_statement(statement.kind()) {
                        return None;
                    }
                    match self.statement_binding(statement, name) {
                        Lookup::Bound(binding) => Some(binding),
                        Lookup::Next => None,
                    }
                });
        match binding {
            Some(binding) => Lookup::Bound(binding),
            None => Lookup::Next,
        }
    }

    fn statement_binding(&self, statement: Node<'tree>, name: &str) -> Lookup {
        match statement.kind() {
            "dim_statement" | "const_declaration" => self.group_lookup(&groups(statement), name),
            "field_declaration" => {
                let mut cursor = statement.walk();
                let declarators: Vec<Node> = statement
                    .named_children(&mut cursor)
                    .filter(|child| child.kind() == "variable_declarator")
                    .collect();
                let all: Vec<Group> = declarators.into_iter().flat_map(groups).collect();
                self.group_lookup(&all, name)
            }
            "property_declaration" => {
                let named = statement
                    .child_by_field_name("name")
                    .is_some_and(|node| self.same_name(node, name));
                if !named || statement.child_by_field_name("parameters").is_some() {
                    return if named {
                        Lookup::Bound(None)
                    } else {
                        Lookup::Next
                    };
                }
                Lookup::Bound(
                    statement
                        .named_children(&mut statement.walk())
                        .find(|child| child.kind() == "as_clause")
                        .and_then(|clause| clause.child_by_field_name("type"))
                        .map(|node| compact_type_text(&self.text(node))),
                )
            }
            _ => match statement.child_by_field_name("name") {
                Some(declared) if self.same_name(declared, name) => Lookup::Bound(None),
                _ => Lookup::Next,
            },
        }
    }

    fn group_lookup(&self, groups: &[Group<'tree>], name: &str) -> Lookup {
        for group in groups {
            if !group.names.iter().any(|node| self.same_name(*node, name)) {
                continue;
            }
            if group.array {
                return Lookup::Bound(None);
            }
            if let Some(type_node) = group.type_node {
                return Lookup::Bound(Some(compact_type_text(&self.text(type_node))));
            }
            return Lookup::Bound(
                group
                    .value
                    .filter(|_| group.names.len() == 1)
                    .and_then(|value| self.initializer_type(value)),
            );
        }
        Lookup::Next
    }

    /// The type an inferred local takes from a literal, a cast, a
    /// conversion, or a construction.
    fn initializer_type(&self, value: Node<'tree>) -> Option<String> {
        match value.kind() {
            "identifier" | "me_expression" => None,
            _ => self.type_of(value),
        }
    }
}

/// Splits a declaration into its name groups: `Dim a, b As Long` gives one
/// group of two names, `Dim c As Integer = 1, d = "x"` gives two.
fn groups(node: Node) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    let mut cursor = node.walk();
    for (index, child) in node.children(&mut cursor).enumerate() {
        let field = node.field_name_for_child(index as u32);
        let starts_name = matches!(field, Some("name" | "resource" | "exception"));
        if starts_name {
            let open = groups
                .last()
                .is_some_and(|group| group.type_node.is_none() && group.value.is_none());
            if !open {
                groups.push(Group {
                    names: Vec::new(),
                    type_node: None,
                    value: None,
                    array: false,
                });
            }
            if let Some(group) = groups.last_mut() {
                group.names.push(child);
            }
            continue;
        }
        let Some(group) = groups.last_mut() else {
            continue;
        };
        match (field, child.kind()) {
            (_, "as_clause") => group.type_node = child.child_by_field_name("type"),
            (_, "array_rank_specifier") => group.array = true,
            (Some("value" | "initializer"), _) => group.value = Some(child),
            _ => {}
        }
    }
    groups
}

fn is_statement(kind: &str) -> bool {
    matches!(kind, "dim_statement" | "const_declaration")
}

fn is_bare_new(node: Node) -> bool {
    let type_id = node.child_by_field_name("type").map(|ty| ty.id());
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .all(|child| Some(child.id()) == type_id)
}

/// A VB name as the language compares it: without the brackets of an escaped
/// name and without regard to case.
fn identifier_key(text: &str) -> String {
    text.trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase()
}

fn integer_type(text: &str) -> Option<&'static str> {
    let lower = text.to_ascii_lowercase().replace('_', "");
    let (radix, body) = if let Some(body) = lower.strip_prefix("&h") {
        (16, body)
    } else if let Some(body) = lower.strip_prefix("&o") {
        (8, body)
    } else if let Some(body) = lower.strip_prefix("&b") {
        (2, body)
    } else {
        (10, lower.as_str())
    };
    for (suffix, name) in [
        ("us", "UShort"),
        ("ui", "UInteger"),
        ("ul", "ULong"),
        ("s", "Short"),
        ("i", "Integer"),
        ("%", "Integer"),
        ("l", "Long"),
        ("&", "Long"),
    ] {
        if body.ends_with(suffix) {
            return Some(name);
        }
    }
    let value = u64::from_str_radix(body, radix).ok()?;
    if radix == 10 {
        if value <= i32::MAX as u64 {
            Some("Integer")
        } else if value <= i64::MAX as u64 {
            Some("Long")
        } else {
            None
        }
    } else if value <= u32::MAX as u64 {
        Some("Integer")
    } else {
        Some("Long")
    }
}

fn floating_type(text: &str) -> Option<&'static str> {
    match text.chars().last()?.to_ascii_lowercase() {
        'f' | '!' => Some("Single"),
        'r' | '#' => Some("Double"),
        'd' | '@' => Some("Decimal"),
        last if last.is_ascii_digit() => Some("Double"),
        _ => None,
    }
}

fn conversion_type(operator: &str) -> Option<&'static str> {
    let name = match operator.to_ascii_lowercase().as_str() {
        "cbool" => "Boolean",
        "cbyte" => "Byte",
        "cchar" => "Char",
        "cdate" => "Date",
        "cdec" => "Decimal",
        "cdbl" => "Double",
        "cint" => "Integer",
        "clng" => "Long",
        "cobj" => "Object",
        "csbyte" => "SByte",
        "cshort" => "Short",
        "csng" => "Single",
        "cstr" => "String",
        "cuint" => "UInteger",
        "culng" => "ULong",
        "cushort" => "UShort",
        _ => return None,
    };
    Some(name)
}
