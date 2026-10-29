//! The argument count, labels, and static types of a Swift call, so a reader
//! can tell which overload the call binds. Swift picks an overload by its
//! argument labels first: `run(a: 1)` binds `run(a:)`, `run(b: 1)` binds
//! `run(b:)`, and `run("x")` binds `run(_: String)` over `run(_: Int)` by type.

use std::collections::HashSet;

use serde_json::Value;
use tree_sitter::{Node, Tree};

use super::identifiers::call_callee;
use crate::base::argument_types::{ScopeBindings, compact_type_text};
use crate::base::{BaseExtractor, Identifier, IdentifierKind, Symbol, SymbolKind};

const TYPE_BODIES: [&str; 3] = ["class_body", "enum_class_body", "protocol_body"];
const CALLABLES: [&str; 4] = [
    "function_declaration",
    "init_declaration",
    "subscript_declaration",
    "deinit_declaration",
];

/// One argument as written: its label, or `None` when unlabeled, and its
/// value, which a `nil` argument does not have as a node.
struct Argument<'tree> {
    label: Option<Node<'tree>>,
    value: Option<Node<'tree>>,
}

/// Adds `arg_count`, `arg_labels`, and `arg_types` to each Swift call
/// identifier whose call has an argument list or a trailing closure. A
/// trailing closure is one argument; the first one has no label and each
/// later one has the label written before it.
pub(crate) fn enrich(
    base: &BaseExtractor,
    tree: &Tree,
    symbols: &[Symbol],
    identifiers: &mut [Identifier],
) {
    let root = tree.root_node();
    let reader = Reader {
        base,
        type_names: constructible_types(symbols),
        bindings: ScopeBindings::new(false),
    };
    for identifier in identifiers.iter_mut() {
        if identifier.kind != IdentifierKind::Call || identifier.language != "swift" {
            continue;
        }
        let Some(arguments) = root
            .named_descendant_for_byte_range(
                identifier.start_byte as usize,
                identifier.end_byte as usize,
            )
            .and_then(|name| called_by(base, name))
            .and_then(call_arguments)
        else {
            continue;
        };
        let labels: Vec<Value> = arguments
            .iter()
            .map(|argument| {
                argument
                    .label
                    .map(|label| base.get_node_text(&label))
                    .into()
            })
            .collect();
        let types: Vec<Value> = arguments
            .iter()
            .map(|argument| {
                argument
                    .value
                    .and_then(|value| reader.type_of(value))
                    .into()
            })
            .collect();
        let metadata = identifier.metadata.get_or_insert_with(Default::default);
        metadata.insert("arg_count".to_string(), arguments.len().into());
        metadata.insert("arg_labels".to_string(), labels.into());
        metadata.insert("arg_types".to_string(), types.into());
    }
}

/// The call whose callee name is `name`.
fn called_by<'tree>(base: &BaseExtractor, name: Node<'tree>) -> Option<Node<'tree>> {
    let mut current = name;
    for _ in 0..4 {
        current = current.parent()?;
        if current.kind() == "call_expression" {
            let callee = call_callee(base, current)?;
            return (callee.name.byte_range() == name.byte_range()).then_some(current);
        }
    }
    None
}

fn call_arguments(call: Node) -> Option<Vec<Argument>> {
    let suffix = call
        .named_children(&mut call.walk())
        .find(|child| child.kind() == "call_suffix")?;
    let mut arguments = Vec::new();
    let mut closure_label = None;
    let mut cursor = suffix.walk();
    for part in suffix.named_children(&mut cursor) {
        match part.kind() {
            "value_arguments" => {
                if part.child(0)?.kind() != "(" {
                    return None;
                }
                let mut cursor = part.walk();
                for argument in part.named_children(&mut cursor) {
                    match argument.kind() {
                        "value_argument" => arguments.push(Argument {
                            label: None,
                            value: argument.child_by_field_name("value"),
                        }),
                        "comment" | "multiline_comment" => {}
                        _ => return None,
                    }
                    if let (Some(last), Some(label)) =
                        (arguments.last_mut(), argument.child_by_field_name("name"))
                    {
                        last.label = Some(label);
                    }
                }
            }
            "simple_identifier" => closure_label = Some(part),
            "lambda_literal" => arguments.push(Argument {
                label: closure_label.take(),
                value: Some(part),
            }),
            "comment" | "multiline_comment" => {}
            _ => return None,
        }
    }
    Some(arguments)
}

/// The class, struct, enum, and actor names this file declares, without a
/// name a function of the file also has, and without a type with a failable
/// initializer, whose `T(...)` gives `T?`.
fn constructible_types(symbols: &[Symbol]) -> HashSet<String> {
    let functions: HashSet<&str> = symbols
        .iter()
        .filter(|symbol| matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method))
        .map(|symbol| symbol.name.as_str())
        .collect();
    let failable: HashSet<&str> = symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Constructor)
        .filter(|symbol| {
            symbol
                .signature
                .as_deref()
                .is_some_and(|signature| signature.contains("init?") || signature.contains("init!"))
        })
        .filter_map(|symbol| symbol.parent_id.as_deref())
        .filter_map(|parent| symbols.iter().find(|symbol| symbol.id == parent))
        .map(|symbol| symbol.name.as_str())
        .collect();
    symbols
        .iter()
        .filter(|symbol| {
            matches!(
                symbol.kind,
                SymbolKind::Class | SymbolKind::Struct | SymbolKind::Enum
            ) && !functions.contains(symbol.name.as_str())
                && !failable.contains(symbol.name.as_str())
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
    bindings: ScopeBindings<'tree, Option<String>>,
}

impl<'tree> Reader<'_, 'tree> {
    fn text(&self, node: Node<'tree>) -> String {
        self.base.get_node_text(&node)
    }

    fn type_of(&self, node: Node<'tree>) -> Option<String> {
        let name = match node.kind() {
            "integer_literal" | "hex_literal" | "oct_literal" | "bin_literal" => "Int",
            "real_literal" => "Double",
            "line_string_literal" | "multi_line_string_literal" | "raw_string_literal" => "String",
            "boolean_literal" => "Bool",
            "tuple_expression" => {
                let mut cursor = node.walk();
                let values: Vec<Node> = node.children_by_field_name("value", &mut cursor).collect();
                let has_label = node.child_by_field_name("name").is_some();
                return match values.as_slice() {
                    [value] if !has_label => self.type_of(*value),
                    _ => None,
                };
            }
            "prefix_expression" => {
                let operation = self.text(node.child_by_field_name("operation")?);
                let target = node.child_by_field_name("target")?;
                let numeric = matches!(
                    target.kind(),
                    "integer_literal"
                        | "hex_literal"
                        | "oct_literal"
                        | "bin_literal"
                        | "real_literal"
                );
                return (numeric && matches!(operation.as_str(), "-" | "+"))
                    .then(|| self.type_of(target))?;
            }
            "as_expression" => {
                let operator = node
                    .named_children(&mut node.walk())
                    .find(|child| child.kind() == "as_operator")?;
                let type_node = node.child_by_field_name("name")?;
                let written = compact_type_text(&self.text(type_node));
                return match self.text(operator).as_str() {
                    "as" | "as!" => Some(written),
                    "as?" => Some(format!("{written}?")),
                    _ => None,
                };
            }
            "self_expression" => return self.self_type(node),
            "call_expression" => {
                let callee = node.named_child(0)?;
                if callee.kind() != "simple_identifier" {
                    return None;
                }
                let name = self.text(callee);
                return self.type_names.contains(&name).then_some(name);
            }
            "simple_identifier" => return self.value_type(&self.text(node), node),
            _ => return None,
        };
        Some(name.to_string())
    }

    /// The type `self` names in an instance member: the enclosing type, or
    /// the type an extension extends. A static member's `self` is the
    /// metatype, which has no type here.
    fn self_type(&self, node: Node<'tree>) -> Option<String> {
        let mut current = node.parent();
        let mut in_static = false;
        while let Some(scope) = current {
            if CALLABLES.contains(&scope.kind()) || scope.kind() == "property_declaration" {
                in_static |= self.is_static(scope);
            }
            if scope.kind() == "class_declaration" || scope.kind() == "protocol_declaration" {
                if in_static {
                    return None;
                }
                let name = scope.child_by_field_name("name")?;
                let name = match name.kind() {
                    "user_type" => name
                        .named_children(&mut name.walk())
                        .find(|part| part.kind() == "type_identifier")
                        .unwrap_or(name),
                    _ => name,
                };
                return Some(self.text(name));
            }
            current = scope.parent();
        }
        None
    }

    fn is_static(&self, declaration: Node<'tree>) -> bool {
        declaration
            .named_children(&mut declaration.walk())
            .filter(|child| child.kind() == "modifiers")
            .any(|modifiers| {
                self.text(modifiers)
                    .split_whitespace()
                    .any(|word| matches!(word, "static" | "class"))
            })
    }

    /// The declared type of the local, parameter, property, or global that
    /// `name` names at `at`. The search walks out from `at`, and the first
    /// scope that binds the name decides. A binding whose type the reader
    /// cannot see (an optional binding, a loop variable, a pattern) gives
    /// none, and so does a type body without the name, since an extension
    /// or a base class can declare it.
    fn value_type(&self, name: &str, at: Node<'tree>) -> Option<String> {
        let mut child = at;
        while let Some(scope) = child.parent() {
            if let Lookup::Bound(found) = self.scope_lookup(scope, child, name) {
                return found;
            }
            if TYPE_BODIES.contains(&scope.kind()) {
                return None;
            }
            child = scope;
        }
        None
    }

    fn scope_lookup(&self, scope: Node<'tree>, child: Node<'tree>, name: &str) -> Lookup {
        match scope.kind() {
            "statements" => self.scope_binding(scope, Some(child.start_byte()), name),
            "source_file" => self.scope_binding(scope, None, name),
            kind if TYPE_BODIES.contains(&kind) => self.scope_binding(scope, None, name),
            kind if CALLABLES.contains(&kind) => self.parameter_lookup(scope, name),
            "lambda_literal" => self.closure_lookup(scope, name),
            "for_statement"
            | "if_statement"
            | "while_statement"
            | "guard_statement"
            | "catch_block"
            | "switch_entry"
            | "repeat_while_statement" => {
                if self.binds_in_head(scope, child, name) {
                    Lookup::Bound(None)
                } else {
                    Lookup::Next
                }
            }
            _ => Lookup::Next,
        }
    }

    /// Whether a pattern of the statement head binds `name`: an optional
    /// binding, a loop variable, a `catch` pattern, or a `case` pattern.
    /// A `catch` without a pattern binds `error`.
    fn binds_in_head(&self, statement: Node<'tree>, child: Node<'tree>, name: &str) -> bool {
        if statement.kind() == "catch_block" && name == "error" {
            let has_pattern = statement.child_by_field_name("error").is_some();
            if !has_pattern {
                return true;
            }
        }
        let mut cursor = statement.walk();
        let head: Vec<(Option<&str>, Node)> = statement
            .children(&mut cursor)
            .enumerate()
            .map(|(index, part)| (statement.field_name_for_child(index as u32), part))
            .filter(|(_, part)| {
                part.is_named() && part.id() != child.id() && part.kind() != "statements"
            })
            .collect();
        head.into_iter().any(|(field, part)| {
            if part.kind() == "simple_identifier" {
                field == Some("bound_identifier") && self.text(part) == name
            } else {
                self.pattern_binds(part, name, 0)
            }
        })
    }

    /// Whether a pattern below `node` binds `name`: a `bound_identifier`, or
    /// a name inside a pattern. An enum case name inside a pattern also
    /// counts, which only costs a type.
    fn pattern_binds(&self, node: Node<'tree>, name: &str, depth: u32) -> bool {
        if depth > 12 {
            return false;
        }
        let mut cursor = node.walk();
        let parts: Vec<(Option<&str>, Node)> = node
            .children(&mut cursor)
            .enumerate()
            .map(|(index, part)| (node.field_name_for_child(index as u32), part))
            .collect();
        parts.into_iter().any(|(field, part)| {
            if !part.is_named() {
                return false;
            }
            if part.kind() == "simple_identifier" {
                return (field == Some("bound_identifier") || node.kind() == "pattern")
                    && self.text(part) == name;
            }
            !matches!(
                part.kind(),
                "statements" | "lambda_literal" | "call_expression"
            ) && self.pattern_binds(part, name, depth + 1)
        })
    }

    /// The binding of `name` by the last child of `scope` that binds it,
    /// among the children that end before the byte `before`, or all of them.
    fn scope_binding(&self, scope: Node<'tree>, before: Option<usize>, name: &str) -> Lookup {
        let binding =
            self.bindings
                .last_binding(
                    &self.base.content,
                    scope,
                    before,
                    name,
                    |statement| match self.statement_binding(statement, name) {
                        Lookup::Bound(binding) => Some(binding),
                        Lookup::Next => None,
                    },
                );
        match binding {
            Some(binding) => Lookup::Bound(binding),
            None => Lookup::Next,
        }
    }

    fn statement_binding(&self, statement: Node<'tree>, name: &str) -> Lookup {
        match statement.kind() {
            "property_declaration" => self.property_binding(statement, name),
            "guard_statement" => {
                if self.binds_in_head(statement, statement, name) {
                    Lookup::Bound(None)
                } else {
                    Lookup::Next
                }
            }
            _ => match statement.child_by_field_name("name") {
                Some(declared) if self.text(declared) == name => Lookup::Bound(None),
                _ => Lookup::Next,
            },
        }
    }

    /// `let a: Int = 1, b = 2.5`: each name takes the type written after
    /// it, or before the next name that has one (`var a, b: Int`), or the
    /// type of its literal or construction initializer.
    fn property_binding(&self, declaration: Node<'tree>, name: &str) -> Lookup {
        let mut pending: Vec<Node> = Vec::new();
        let mut target_pending = false;
        let mut cursor = declaration.walk();
        let children: Vec<(Option<&str>, Node)> = declaration
            .children(&mut cursor)
            .enumerate()
            .map(|(index, child)| (declaration.field_name_for_child(index as u32), child))
            .collect();
        for (field, child) in children {
            match (field, child.kind()) {
                (Some("name"), _) => {
                    let simple = child
                        .child_by_field_name("bound_identifier")
                        .filter(|_| child.named_child_count() == 1);
                    match simple {
                        Some(bound) => {
                            pending.push(bound);
                            target_pending |= self.text(bound) == name;
                        }
                        None if self.pattern_binds(child, name, 0) => {
                            return Lookup::Bound(None);
                        }
                        None => {}
                    }
                }
                (_, "type_annotation") => {
                    if target_pending {
                        let type_node = child.child_by_field_name("name");
                        return Lookup::Bound(
                            type_node.map(|node| compact_type_text(&self.text(node))),
                        );
                    }
                    pending.clear();
                }
                (Some("value"), _) if child.is_named() => {
                    if target_pending {
                        return Lookup::Bound(
                            (pending.len() == 1)
                                .then(|| self.initializer_type(child))
                                .flatten(),
                        );
                    }
                    pending.clear();
                }
                (Some("computed_value"), _) | (_, "computed_property") if target_pending => {
                    return Lookup::Bound(None);
                }
                _ => {}
            }
        }
        if target_pending {
            Lookup::Bound(None)
        } else {
            Lookup::Next
        }
    }

    fn initializer_type(&self, value: Node<'tree>) -> Option<String> {
        match value.kind() {
            "simple_identifier" | "self_expression" => None,
            _ => self.type_of(value),
        }
    }

    fn parameter_lookup(&self, callable: Node<'tree>, name: &str) -> Lookup {
        let mut cursor = callable.walk();
        for parameter in callable.named_children(&mut cursor) {
            if parameter.kind() != "parameter" {
                continue;
            }
            let mut fields = parameter.walk();
            let names: Vec<Node> = parameter
                .children_by_field_name("name", &mut fields)
                .collect();
            let binding = names.iter().find(|node| node.kind() == "simple_identifier");
            if !binding.is_some_and(|node| self.text(*node) == name) {
                continue;
            }
            let variadic = parameter
                .children(&mut parameter.walk())
                .any(|child| child.kind() == "...");
            let modified = parameter
                .children(&mut parameter.walk())
                .any(|child| child.kind() == "parameter_modifiers")
                || self
                    .text(parameter)
                    .split_whitespace()
                    .any(|word| word == "inout");
            let type_node = names.iter().find(|node| node.kind() != "simple_identifier");
            return Lookup::Bound(
                type_node
                    .filter(|_| !variadic && !modified)
                    .map(|node| compact_type_text(&self.text(*node))),
            );
        }
        Lookup::Next
    }

    fn closure_lookup(&self, closure: Node<'tree>, name: &str) -> Lookup {
        let Some(signature) = closure.child_by_field_name("type") else {
            return self.capture_lookup(closure, name);
        };
        let mut stack = vec![signature];
        while let Some(node) = stack.pop() {
            if node.kind() == "lambda_parameter" {
                let mut fields = node.walk();
                let names: Vec<Node> = node.children_by_field_name("name", &mut fields).collect();
                let binds = names
                    .iter()
                    .any(|part| part.kind() == "simple_identifier" && self.text(*part) == name);
                if binds {
                    let type_node = names.iter().find(|part| part.kind() != "simple_identifier");
                    return Lookup::Bound(
                        type_node.map(|part| compact_type_text(&self.text(*part))),
                    );
                }
                continue;
            }
            if node.kind() == "simple_identifier" && self.text(node) == name {
                return Lookup::Bound(None);
            }
            let mut cursor = node.walk();
            stack.extend(node.named_children(&mut cursor));
        }
        self.capture_lookup(closure, name)
    }

    fn capture_lookup(&self, closure: Node<'tree>, name: &str) -> Lookup {
        let captures = closure
            .named_children(&mut closure.walk())
            .find(|child| child.kind() == "capture_list");
        match captures {
            Some(list) if self.pattern_binds_any(list, name) => Lookup::Bound(None),
            _ => Lookup::Next,
        }
    }

    fn pattern_binds_any(&self, node: Node<'tree>, name: &str) -> bool {
        let mut stack = vec![node];
        while let Some(current) = stack.pop() {
            if current.kind() == "simple_identifier" && self.text(current) == name {
                return true;
            }
            let mut cursor = current.walk();
            stack.extend(current.named_children(&mut cursor));
        }
        false
    }
}
