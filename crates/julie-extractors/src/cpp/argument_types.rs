//! The static type of each argument of a C++ call, so a reader can tell which
//! overload the call binds: `run(1)` binds `run(int)`, and `run("x")` binds
//! `run(const char*)`. A construction (`new Point(1)`, `Point{1}`,
//! `Point p(1)`) gets the same keys on the call identifier that spans its
//! argument list.

use std::collections::{HashMap, HashSet};

use tree_sitter::{Node, Tree};

use super::declarators::{declarator_target, is_direct_initialization, structured_binding};
use super::name_bindings::declaration_names;
use crate::base::argument_types::{
    CallArguments, ScopeBindings, annotate_calls, compact_type_text,
};
use crate::base::call_arguments::list_summary;
use crate::base::{BaseExtractor, Identifier, IdentifierKind, Symbol, SymbolKind};

const NAMED_CASTS: [&str; 4] = [
    "static_cast",
    "dynamic_cast",
    "const_cast",
    "reinterpret_cast",
];

/// Adds `arg_count` (and `arg_spread`) to each construction identifier, then
/// `arg_types` to each C++ call identifier that has `arg_count`: one entry per
/// argument, the type its expression states, or `null`. A call with a pack
/// expansion or a designated initializer gets none.
pub(crate) fn enrich(
    base: &BaseExtractor,
    tree: &Tree,
    symbols: &[Symbol],
    identifiers: &mut [Identifier],
) {
    count_constructions(base, tree, identifiers);
    let reader = Reader {
        base,
        type_names: file_type_names(symbols),
        class_bodies: class_bodies(base, tree.root_node()),
        bindings: ScopeBindings::new(false),
    };
    annotate_calls("cpp", tree, identifiers, |node, identifier| {
        let arguments = call_arguments(node, identifier)?;
        Some(CallArguments {
            types: reader.argument_types(&arguments)?,
            names: None,
        })
    });
}

/// The argument list a construction identifier spans, with the node that
/// owns it: a `new` expression, a `T{...}` temporary, or a declaration.
pub(super) fn construction_list(node: Node) -> Option<Node> {
    if !matches!(
        node.kind(),
        "argument_list" | "initializer_list" | "parameter_list"
    ) {
        return None;
    }
    let parent = node.parent()?;
    let field = |name: &str| parent.child_by_field_name(name) == Some(node);
    let owned = match parent.kind() {
        "new_expression" => field("arguments"),
        "compound_literal_expression" => field("value"),
        "init_declarator" => field("value") && declaration_type(parent).is_some(),
        "function_declarator" => {
            field("parameters")
                && parent.parent().is_some_and(|declaration| {
                    declaration.kind() == "declaration"
                        && declaration.child_by_field_name("declarator") == Some(parent)
                        && is_direct_initialization(declaration, parent)
                })
        }
        _ => false,
    };
    owned.then_some(node)
}

/// The class-like type of the declaration that holds `init_declarator`, when
/// the declaration has that one declarator.
fn declaration_type(init_declarator: Node) -> Option<Node> {
    let declaration = init_declarator.parent()?;
    if declaration.kind() != "declaration" {
        return None;
    }
    let mut cursor = declaration.walk();
    let declarators = declaration
        .children_by_field_name("declarator", &mut cursor)
        .count();
    if declarators != 1 || declaration.child_by_field_name("declarator") != Some(init_declarator) {
        return None;
    }
    let type_node = declaration.child_by_field_name("type")?;
    constructed_type_name(type_node).map(|_| type_node)
}

/// The name a construction of `type_node` calls: the class name of a written
/// class type, without its scope or template arguments.
pub(super) fn constructed_type_name(type_node: Node) -> Option<Node> {
    let mut node = type_node;
    loop {
        node = match node.kind() {
            "type_identifier" => return Some(node),
            "qualified_identifier" => node.child_by_field_name("name")?,
            "template_type" => node.child_by_field_name("name")?,
            _ => return None,
        };
    }
}

/// The type a construction list builds: the `type` of its `new` expression,
/// temporary, or declaration.
pub(super) fn construction_type(list: Node) -> Option<Node> {
    let parent = list.parent()?;
    match parent.kind() {
        "new_expression" | "compound_literal_expression" => parent.child_by_field_name("type"),
        "init_declarator" => declaration_type(parent),
        "function_declarator" => parent.parent()?.child_by_field_name("type"),
        _ => None,
    }
}

fn count_constructions(base: &BaseExtractor, tree: &Tree, identifiers: &mut [Identifier]) {
    let root = tree.root_node();
    let source = base.content.as_bytes();
    for identifier in identifiers.iter_mut() {
        if identifier.kind != IdentifierKind::Call || identifier.language != "cpp" {
            continue;
        }
        let Some(list) = root
            .descendant_for_byte_range(identifier.start_byte as usize, identifier.end_byte as usize)
            .filter(|node| {
                node.start_byte() == identifier.start_byte as usize
                    && node.end_byte() == identifier.end_byte as usize
            })
            .and_then(construction_list)
        else {
            continue;
        };
        let Some(owner) = list.parent() else {
            continue;
        };
        let Some((count, spread)) = list_summary("cpp", owner, list, source) else {
            continue;
        };
        let metadata = identifier.metadata.get_or_insert_with(Default::default);
        metadata.insert("arg_count".to_string(), count.into());
        if spread {
            metadata.insert("arg_spread".to_string(), true.into());
        }
    }
}

/// The argument nodes of the call that `node` names, or of the construction
/// list that `node` is.
fn call_arguments<'tree>(node: Node<'tree>, identifier: &Identifier) -> Option<Vec<Node<'tree>>> {
    let list = match construction_list(node) {
        Some(list) => list,
        None => {
            let mut current = node;
            while current.kind() != "call_expression" {
                current = current.parent()?;
            }
            let list = current.child_by_field_name("arguments")?;
            if list.start_byte() < identifier.end_byte as usize {
                return None;
            }
            list
        }
    };
    let mut cursor = list.walk();
    let arguments: Vec<Node> = list
        .named_children(&mut cursor)
        .filter(|child| !child.is_extra())
        .collect();
    if arguments
        .iter()
        .any(|argument| argument.kind() == "initializer_pair")
    {
        return None;
    }
    Some(arguments)
}

/// Names of the classes, structs, unions, enums, and aliases this file
/// declares, without the names a function of the file also has.
fn file_type_names(symbols: &[Symbol]) -> HashSet<String> {
    let functions: HashSet<&str> = symbols
        .iter()
        .filter(|symbol| matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method))
        .map(|symbol| symbol.name.as_str())
        .collect();
    symbols
        .iter()
        .filter(|symbol| {
            matches!(
                symbol.kind,
                SymbolKind::Class
                    | SymbolKind::Struct
                    | SymbolKind::Union
                    | SymbolKind::Enum
                    | SymbolKind::Type
            ) && !functions.contains(symbol.name.as_str())
        })
        .map(|symbol| symbol.name.clone())
        .collect()
}

/// What a scope says about a name: it binds the name, with this type or an
/// unknown one, or the search goes on outward.
enum Lookup {
    Bound(Option<String>),
    Next,
}

struct Reader<'a, 'tree> {
    base: &'a BaseExtractor,
    type_names: HashSet<String>,
    /// The body of each class this file defines outside a function, by name;
    /// `None` for a name two classes share.
    class_bodies: HashMap<String, Option<Node<'tree>>>,
    bindings: ScopeBindings<'tree, Option<String>>,
}

impl<'tree> Reader<'_, 'tree> {
    fn text(&self, node: Node<'tree>) -> String {
        self.base.get_node_text(&node)
    }

    fn argument_types(&self, arguments: &[Node<'tree>]) -> Option<Vec<Option<String>>> {
        Some(
            arguments
                .iter()
                .map(|argument| self.type_of(*argument))
                .collect(),
        )
    }

    fn type_of(&self, node: Node<'tree>) -> Option<String> {
        match node.kind() {
            "parenthesized_expression" => self.type_of(node.named_child(0)?),
            "number_literal" => number_type(&self.text(node)),
            "char_literal" => char_type(node, &self.text(node)),
            "string_literal" | "raw_string_literal" => string_type(&self.text(node)),
            "concatenated_string" => self.concatenated_string_type(node),
            "true" | "false" => Some("bool".to_string()),
            "null" => (self.text(node) == "nullptr").then(|| "std::nullptr_t".to_string()),
            "this" => self.this_type(node),
            "identifier" => self.value_type(&self.text(node), node),
            "parameter_declaration" => {
                let type_node = node.child_by_field_name("type")?;
                (type_node.kind() == "type_identifier")
                    .then(|| self.value_type(&self.text(type_node), node))?
            }
            "cast_expression" => self.written_type(node.child_by_field_name("type")?),
            "call_expression" => self.call_type(node),
            "new_expression" => {
                let type_node = node.child_by_field_name("type")?;
                if is_placeholder(type_node) {
                    return None;
                }
                Some(format!("{}*", compact_type_text(&self.text(type_node))))
            }
            "compound_literal_expression" => {
                let type_node = node.child_by_field_name("type")?;
                if type_node.kind() == "type_descriptor" {
                    return self.written_type(type_node);
                }
                (!is_placeholder(type_node)).then(|| compact_type_text(&self.text(type_node)))
            }
            "unary_expression" => {
                let operator = node.child_by_field_name("operator")?;
                let argument = node.child_by_field_name("argument")?;
                (matches!(operator.kind(), "-" | "+") && argument.kind() == "number_literal")
                    .then(|| number_type(&self.text(argument)))?
            }
            _ => None,
        }
    }

    /// The type a `type_descriptor` names, without a top-level reference:
    /// an expression never has a reference type.
    fn written_type(&self, descriptor: Node<'tree>) -> Option<String> {
        let text = compact_type_text(&self.text(descriptor));
        let text = text
            .strip_suffix("&&")
            .or_else(|| text.strip_suffix('&'))
            .unwrap_or(&text);
        (!text.is_empty() && !text.contains("auto")).then(|| text.to_string())
    }

    fn concatenated_string_type(&self, node: Node<'tree>) -> Option<String> {
        let mut prefixes = HashSet::new();
        let mut cursor = node.walk();
        for part in node.named_children(&mut cursor) {
            if !matches!(part.kind(), "string_literal" | "raw_string_literal") {
                return None;
            }
            let text = self.text(part);
            let prefix = string_prefix(&text)?;
            if !prefix.is_empty() {
                prefixes.insert(prefix.to_string());
            }
        }
        match prefixes.len() {
            0 => string_type("\"\""),
            1 => string_type(&format!("{}\"\"", prefixes.into_iter().next()?)),
            _ => None,
        }
    }

    /// A named cast gives its target type; `T(x)` gives `T` when `T` is a
    /// built-in type or a type this file declares.
    fn call_type(&self, call: Node<'tree>) -> Option<String> {
        let function = call.child_by_field_name("function")?;
        match function.kind() {
            "template_function" => {
                let name = self.text(function.child_by_field_name("name")?);
                if NAMED_CASTS.contains(&name.as_str()) {
                    let arguments = function.child_by_field_name("arguments")?;
                    let target = arguments.named_child(0)?;
                    if arguments.named_child_count() != 1 || target.kind() != "type_descriptor" {
                        return None;
                    }
                    return self.written_type(target);
                }
                self.type_names
                    .contains(&name)
                    .then(|| compact_type_text(&self.text(function)))
            }
            "primitive_type" | "sized_type_specifier" => {
                Some(compact_type_text(&self.text(function)))
            }
            "identifier" => {
                let name = self.text(function);
                self.type_names.contains(&name).then_some(name)
            }
            "qualified_identifier" => {
                let mut name = function;
                while name.kind() == "qualified_identifier" {
                    name = name.child_by_field_name("name")?;
                }
                let terminal = match name.kind() {
                    "template_type" | "template_function" => name.child_by_field_name("name")?,
                    _ => name,
                };
                self.type_names
                    .contains(&self.text(terminal))
                    .then(|| compact_type_text(&self.text(function)))
            }
            _ => None,
        }
    }

    /// `X*` for the class whose member function holds `node`, or
    /// `const X*` in a `const` member function.
    fn this_type(&self, node: Node<'tree>) -> Option<String> {
        let mut current = node.parent();
        while let Some(scope) = current {
            if scope.kind() == "function_definition" {
                let declarator = scope.child_by_field_name("declarator")?;
                let function = declarator_target(declarator)?.function?;
                let class = self.member_class_name(scope, declarator)?;
                let constant = function
                    .children(&mut function.walk())
                    .any(|child| child.kind() == "type_qualifier" && self.text(child) == "const");
                return Some(if constant {
                    format!("const {class}*")
                } else {
                    format!("{class}*")
                });
            }
            current = scope.parent();
        }
        None
    }

    fn member_class_name(&self, function: Node<'tree>, declarator: Node<'tree>) -> Option<String> {
        if let Some(body) = enclosing_class_body(function) {
            let class = body.parent()?;
            let name = class.child_by_field_name("name")?;
            let name = match name.kind() {
                "template_type" => name.child_by_field_name("name")?,
                _ => name,
            };
            return Some(self.text(name));
        }
        let mut name = declarator_target(declarator)?.name;
        let mut scope = None;
        while name.kind() == "qualified_identifier" {
            scope = name.child_by_field_name("scope");
            name = name.child_by_field_name("name")?;
        }
        let scope = scope?;
        let scope = match scope.kind() {
            "template_type" => scope.child_by_field_name("name")?,
            _ => scope,
        };
        Some(self.text(scope))
    }

    /// The declared type of the local, parameter, member, or global that
    /// `name` names at `at`. The search walks out from `at` through the
    /// scopes C++ binds names in, and the first scope that binds the name
    /// decides.
    fn value_type(&self, name: &str, at: Node<'tree>) -> Option<String> {
        let mut child = at;
        while let Some(scope) = child.parent() {
            if let Lookup::Bound(found) = self.scope_lookup(scope, child, name) {
                return found;
            }
            child = scope;
        }
        None
    }

    fn scope_lookup(&self, scope: Node<'tree>, child: Node<'tree>, name: &str) -> Lookup {
        let before = |node: &Node| node.end_byte() <= child.start_byte();
        match scope.kind() {
            "compound_statement" | "case_statement" | "translation_unit" | "declaration_list"
            | "preproc_if" | "preproc_ifdef" | "preproc_else" | "preproc_elif" => {
                self.scope_binding(scope, Some(child.start_byte()), name)
            }
            "for_statement" => match scope.child_by_field_name("initializer") {
                Some(initializer) if before(&initializer) => {
                    self.last_binding(&[initializer], name)
                }
                _ => Lookup::Next,
            },
            "for_range_loop" => {
                let Some(declarator) = scope.child_by_field_name("declarator") else {
                    return Lookup::Next;
                };
                if !before(&declarator) || !self.binds(declarator, name) {
                    return Lookup::Next;
                }
                Lookup::Bound(self.declared_type(scope, declarator))
            }
            "condition_clause" => {
                let mut cursor = scope.walk();
                let parts: Vec<Node> = scope
                    .named_children(&mut cursor)
                    .filter(|part| before(part))
                    .collect();
                self.last_binding(&parts, name)
            }
            "if_statement" | "while_statement" | "switch_statement" => {
                match scope.child_by_field_name("condition") {
                    Some(condition) if before(&condition) => self.last_binding(&[condition], name),
                    _ => Lookup::Next,
                }
            }
            "catch_clause" => self.parameter_lookup(scope, name),
            "lambda_expression" => {
                if self.captures(scope, name) {
                    return Lookup::Bound(None);
                }
                match scope.child_by_field_name("declarator") {
                    Some(declarator) => self.parameter_lookup(declarator, name),
                    None => Lookup::Next,
                }
            }
            "template_declaration" => self.template_parameter_lookup(scope, name),
            "function_definition" => self.function_lookup(scope, name),
            "field_declaration_list" => self.class_lookup(scope, name),
            _ => Lookup::Next,
        }
    }

    /// The last binding of `name` among `statements`: a declaration, the
    /// statements of a `case` or a preprocessor branch, a condition variable,
    /// or any other name the statements declare (a function, a type, an
    /// enumerator), whose type stays unknown.
    fn last_binding(&self, statements: &[Node<'tree>], name: &str) -> Lookup {
        let mut found = Lookup::Next;
        for statement in statements {
            if let Lookup::Bound(binding) = self.statement_binding(*statement, name, 0) {
                found = Lookup::Bound(binding);
            }
        }
        found
    }

    /// `last_binding` over the children of `scope` that end before the
    /// byte `before`, or over all of them.
    fn scope_binding(&self, scope: Node<'tree>, before: Option<usize>, name: &str) -> Lookup {
        let binding =
            self.bindings
                .last_binding(
                    &self.base.content,
                    scope,
                    before,
                    name,
                    |statement| match self.statement_binding(statement, name, 0) {
                        Lookup::Bound(binding) => Some(binding),
                        Lookup::Next => None,
                    },
                );
        match binding {
            Some(binding) => Lookup::Bound(binding),
            None => Lookup::Next,
        }
    }

    fn statement_binding(&self, statement: Node<'tree>, name: &str, depth: u32) -> Lookup {
        if depth > 8 {
            return Lookup::Next;
        }
        match statement.kind() {
            "declaration" | "field_declaration" => {
                let mut cursor = statement.walk();
                let declarators: Vec<Node> = statement
                    .children_by_field_name("declarator", &mut cursor)
                    .collect();
                for declarator in declarators.into_iter().rev() {
                    if self.binds(declarator, name) {
                        return Lookup::Bound(self.declared_type(statement, declarator));
                    }
                }
                self.other_binding(statement, name)
            }
            "init_statement" | "case_statement" | "preproc_if" | "preproc_ifdef"
            | "preproc_else" | "preproc_elif" | "condition_clause" => {
                let mut cursor = statement.walk();
                let children: Vec<Node> = statement.named_children(&mut cursor).collect();
                let mut found = Lookup::Next;
                for child in children {
                    if let Lookup::Bound(binding) = self.statement_binding(child, name, depth + 1) {
                        found = Lookup::Bound(binding);
                    }
                }
                found
            }
            _ => self.other_binding(statement, name),
        }
    }

    fn other_binding(&self, statement: Node<'tree>, name: &str) -> Lookup {
        let mut names = Vec::new();
        declaration_names(self.base, statement, true, &mut names);
        if names.iter().any(|bound| bound == name) {
            Lookup::Bound(None)
        } else {
            Lookup::Next
        }
    }

    fn binds(&self, declarator: Node<'tree>, name: &str) -> bool {
        if let Some(binding) = structured_binding(declarator) {
            let mut cursor = binding.walk();
            return binding
                .named_children(&mut cursor)
                .any(|part| self.text(part) == name);
        }
        let declarator = match declarator.kind() {
            "variadic_declarator" => match declarator.named_child(0) {
                Some(inner) => inner,
                None => return false,
            },
            _ => declarator,
        };
        declarator_target(declarator).is_some_and(|target| self.text(target.name) == name)
    }

    /// The type `declarator` gives its name in `container`: the written type
    /// with its `const` and `volatile`, the pointers of the declarator, and no
    /// top-level reference. `auto` takes the type of a literal or
    /// construction initializer. A function, an array, a structured binding,
    /// or a function pointer has none.
    fn declared_type(&self, container: Node<'tree>, declarator: Node<'tree>) -> Option<String> {
        if structured_binding(declarator).is_some() {
            return None;
        }
        let target = declarator_target(declarator)?;
        if let Some(function) = target.function {
            if container.kind() == "declaration" && is_direct_initialization(container, function) {
                return self.stated_type(container, None);
            }
            return None;
        }
        let type_node = container.child_by_field_name("type")?;
        if is_placeholder(type_node) {
            if type_node.kind() != "placeholder_type_specifier"
                || self.text(type_node).trim() != "auto"
                || declarator.kind() != "init_declarator"
            {
                return None;
            }
            let value = declarator.child_by_field_name("value")?;
            return self.initializer_type(value);
        }
        self.stated_type(container, Some(declarator))
    }

    fn stated_type(
        &self,
        container: Node<'tree>,
        declarator: Option<Node<'tree>>,
    ) -> Option<String> {
        let type_node = container.child_by_field_name("type")?;
        let mut qualifiers = Vec::new();
        let mut cursor = container.walk();
        for child in container.children(&mut cursor) {
            if child.kind() != "type_qualifier" {
                continue;
            }
            let qualifier = match self.text(child).as_str() {
                "const" | "constexpr" => "const",
                "volatile" => "volatile",
                _ => continue,
            };
            if !qualifiers.contains(&qualifier) {
                qualifiers.push(qualifier);
            }
        }
        let mut decoration = String::new();
        let mut current = declarator;
        while let Some(node) = current {
            current = match node.kind() {
                "init_declarator" => node.child_by_field_name("declarator"),
                "pointer_declarator" => {
                    decoration.push('*');
                    node.child_by_field_name("declarator")
                }
                "reference_declarator" => {
                    let kind = if node
                        .children(&mut node.walk())
                        .any(|child| child.kind() == "&&")
                    {
                        "&&"
                    } else {
                        "&"
                    };
                    decoration.push_str(kind);
                    node.named_children(&mut node.walk())
                        .find(|child| child.kind() != "type_qualifier")
                }
                "identifier" | "field_identifier" | "qualified_identifier" | "type_identifier" => {
                    None
                }
                _ => return None,
            };
        }
        let decoration = decoration
            .strip_prefix("&&")
            .or_else(|| decoration.strip_prefix('&'))
            .unwrap_or(&decoration);
        if decoration.contains('&') {
            return None;
        }
        let mut text = qualifiers.join(" ");
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(&self.text(type_node));
        text.push_str(decoration);
        Some(compact_type_text(&text))
    }

    /// The type of an `auto` initializer: a literal, a cast, or a construction.
    fn initializer_type(&self, value: Node<'tree>) -> Option<String> {
        match value.kind() {
            "identifier" | "this" | "parameter_declaration" => None,
            "parenthesized_expression" => self.initializer_type(value.named_child(0)?),
            "call_expression" => {
                let function = value.child_by_field_name("function")?;
                let is_cast = function.kind() == "template_function"
                    && function
                        .child_by_field_name("name")
                        .is_some_and(|name| NAMED_CASTS.contains(&self.text(name).as_str()));
                let is_construction = matches!(
                    function.kind(),
                    "identifier" | "qualified_identifier" | "template_function"
                ) && self.call_type(value).is_some();
                (is_cast || is_construction).then(|| self.call_type(value))?
            }
            _ => self.type_of(value),
        }
    }

    fn parameter_lookup(&self, holder: Node<'tree>, name: &str) -> Lookup {
        let Some(parameters) = holder.child_by_field_name("parameters") else {
            return Lookup::Next;
        };
        let mut cursor = parameters.walk();
        for parameter in parameters.named_children(&mut cursor) {
            let Some(declarator) = parameter.child_by_field_name("declarator") else {
                continue;
            };
            if self.binds(declarator, name) {
                return Lookup::Bound(match parameter.kind() {
                    "parameter_declaration" | "optional_parameter_declaration" => {
                        self.declared_type(parameter, declarator)
                    }
                    _ => None,
                });
            }
        }
        Lookup::Next
    }

    fn template_parameter_lookup(&self, template: Node<'tree>, name: &str) -> Lookup {
        let Some(parameters) = template.child_by_field_name("parameters") else {
            return Lookup::Next;
        };
        let mut cursor = parameters.walk();
        for parameter in parameters.named_children(&mut cursor) {
            if let Some(declarator) = parameter.child_by_field_name("declarator") {
                if self.binds(declarator, name) {
                    return Lookup::Bound(self.declared_type(parameter, declarator));
                }
                continue;
            }
            let type_parameter = parameter
                .child_by_field_name("name")
                .into_iter()
                .chain(
                    parameter
                        .named_children(&mut parameter.walk())
                        .filter(|child| child.kind() == "type_identifier"),
                )
                .any(|type_name| self.text(type_name) == name);
            if type_parameter {
                return Lookup::Bound(None);
            }
        }
        Lookup::Next
    }

    fn captures(&self, lambda: Node<'tree>, name: &str) -> bool {
        let Some(captures) = lambda.child_by_field_name("captures") else {
            return false;
        };
        let mut cursor = captures.walk();
        captures
            .named_children(&mut cursor)
            .filter(|capture| capture.kind() == "lambda_capture_initializer")
            .filter_map(|capture| capture.child_by_field_name("left"))
            .any(|left| self.text(left) == name)
    }

    /// A function's parameters, then, for a member function defined outside
    /// its class, the members of that class when this file holds its body.
    /// A member function defined in its class body finds the members on the
    /// way out.
    fn function_lookup(&self, function: Node<'tree>, name: &str) -> Lookup {
        let Some(declarator) = function.child_by_field_name("declarator") else {
            return Lookup::Next;
        };
        if let Some(function_declarator) =
            declarator_target(declarator).and_then(|target| target.function)
            && let Lookup::Bound(found) = self.parameter_lookup(function_declarator, name)
        {
            return Lookup::Bound(found);
        }
        if enclosing_class_body(function).is_some() {
            return Lookup::Next;
        }
        let Some(target) = declarator_target(declarator) else {
            return Lookup::Next;
        };
        if target.name.kind() != "qualified_identifier" {
            return Lookup::Next;
        }
        let Some(class) = self.member_class_name(function, declarator) else {
            return Lookup::Bound(None);
        };
        match self.class_bodies.get(&class) {
            Some(Some(body)) => self.class_lookup(*body, name),
            _ => Lookup::Bound(None),
        }
    }

    /// The members a class body declares, wherever they stand in the body.
    /// A class with a base class can inherit the name, so the search stops.
    fn class_lookup(&self, body: Node<'tree>, name: &str) -> Lookup {
        if let Lookup::Bound(found) = self.scope_binding(body, None, name) {
            return Lookup::Bound(found);
        }
        let inherits = body.parent().is_some_and(|class| {
            class
                .children(&mut class.walk())
                .any(|child| child.kind() == "base_class_clause")
        });
        if inherits {
            Lookup::Bound(None)
        } else {
            Lookup::Next
        }
    }
}

fn class_bodies<'tree>(
    base: &BaseExtractor,
    root: Node<'tree>,
) -> HashMap<String, Option<Node<'tree>>> {
    let mut bodies: HashMap<String, Option<Node<'tree>>> = HashMap::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "class_specifier" | "struct_specifier" | "union_specifier" => {
                if let (Some(name), Some(body)) = (
                    node.child_by_field_name("name"),
                    node.child_by_field_name("body"),
                ) {
                    bodies
                        .entry(base.get_node_text(&name))
                        .and_modify(|found| *found = None)
                        .or_insert(Some(body));
                }
            }
            "compound_statement" => continue,
            _ => {}
        }
        let mut cursor = node.walk();
        stack.extend(node.named_children(&mut cursor));
    }
    bodies
}

fn enclosing_class_body(function: Node) -> Option<Node> {
    let mut parent = function.parent()?;
    while parent.kind() == "template_declaration" {
        parent = parent.parent()?;
    }
    (parent.kind() == "field_declaration_list").then_some(parent)
}

fn is_placeholder(type_node: Node) -> bool {
    matches!(
        type_node.kind(),
        "placeholder_type_specifier" | "auto" | "decltype"
    )
}

/// The type of an integer or floating literal, when it is the same on every
/// common data model: `int` and `long` are 32 bits on Windows and `long` is
/// 64 bits elsewhere, so a value that only a 64-bit `long` holds has none.
fn number_type(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase().replace('\'', "");
    let lower = lower.trim_start_matches(['-', '+']);
    let hex = lower.starts_with("0x");
    let floating = if hex {
        lower.contains('p')
    } else {
        lower.contains('.') || lower.contains('e')
    };
    if floating {
        if ["f16", "f32", "f64", "f128", "bf16"]
            .iter()
            .any(|suffix| lower.ends_with(suffix))
        {
            return None;
        }
        let name = match lower.chars().last()? {
            'f' => "float",
            'l' => "long double",
            last if last.is_ascii_digit() || last == '.' => "double",
            _ => return None,
        };
        return Some(name.to_string());
    }
    let digits_end = lower
        .rfind(|c: char| !matches!(c, 'u' | 'l' | 'z'))
        .map_or(0, |index| index + 1);
    let (number, suffix) = lower.split_at(digits_end);
    let (radix, digits) = if let Some(digits) = number.strip_prefix("0x") {
        (16, digits)
    } else if let Some(digits) = number.strip_prefix("0b") {
        (2, digits)
    } else if number.len() > 1 && number.starts_with('0') {
        (8, &number[1..])
    } else {
        (10, number)
    };
    if digits.is_empty() {
        return None;
    }
    let value = u64::from_str_radix(digits, radix).ok()?;
    let decimal = radix == 10;
    let fits_int = value <= i32::MAX as u64;
    let fits_unsigned = value <= u32::MAX as u64;
    let name = match suffix {
        "" if fits_int => "int",
        "" if !decimal && fits_unsigned => "unsigned int",
        "u" if fits_unsigned => "unsigned int",
        "l" if fits_int => "long",
        "ul" | "lu" if fits_unsigned => "unsigned long",
        "ll" if value <= i64::MAX as u64 => "long long",
        "ll" if !decimal => "unsigned long long",
        "ull" | "llu" => "unsigned long long",
        _ => return None,
    };
    Some(name.to_string())
}

fn char_type(node: Node, text: &str) -> Option<String> {
    let mut cursor = node.walk();
    let parts = node
        .named_children(&mut cursor)
        .filter(|part| matches!(part.kind(), "character" | "escape_sequence"))
        .count();
    if parts != 1 {
        return None;
    }
    let name = match text.split('\'').next()? {
        "" => "char",
        "L" => "wchar_t",
        "u8" => "char8_t",
        "u" => "char16_t",
        "U" => "char32_t",
        _ => return None,
    };
    Some(name.to_string())
}

/// The encoding prefix of a string literal (`L`, `u8`, `u`, `U`, or empty),
/// without the `R` of a raw string.
fn string_prefix(text: &str) -> Option<&str> {
    let prefix = text.split('"').next()?;
    let prefix = prefix.strip_suffix('R').unwrap_or(prefix);
    matches!(prefix, "" | "L" | "u8" | "u" | "U").then_some(prefix)
}

/// The pointer a string literal decays to.
fn string_type(text: &str) -> Option<String> {
    let character = match string_prefix(text)? {
        "" => "char",
        "L" => "wchar_t",
        "u8" => "char8_t",
        "u" => "char16_t",
        _ => "char32_t",
    };
    Some(format!("const {character}*"))
}
