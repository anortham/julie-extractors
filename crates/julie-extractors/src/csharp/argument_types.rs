//! The static type of each argument of a C# call, so a reader can tell which
//! overload the call binds: `Equals(value)` with a `JValue` binds
//! `Equals(JValue)`, not `Equals(object)`.

use std::collections::HashMap;

use tree_sitter::{Node, Tree};

use crate::base::{Identifier, IdentifierKind, Symbol, SymbolKind, TypeInfo};

const CALL_KINDS: [&str; 2] = ["invocation_expression", "object_creation_expression"];
const TYPE_KINDS: [SymbolKind; 4] = [
    SymbolKind::Class,
    SymbolKind::Struct,
    SymbolKind::Interface,
    SymbolKind::Enum,
];
const VALUE_KINDS: [SymbolKind; 4] = [
    SymbolKind::Variable,
    SymbolKind::Field,
    SymbolKind::Property,
    SymbolKind::Constant,
];

/// Adds `arg_types` to each C# call identifier that has `arg_count`: one entry
/// per argument, the type its expression states, or `null`. A call with a
/// named argument or a `ref`, `out`, or `in` argument gets none, since those
/// change which parameter an argument fills.
pub(crate) fn enrich(
    tree: &Tree,
    source: &str,
    symbols: &[Symbol],
    types: &HashMap<String, TypeInfo>,
    identifiers: &mut [Identifier],
) {
    let root = tree.root_node();
    for identifier in identifiers.iter_mut() {
        if identifier.kind != IdentifierKind::Call || identifier.language != "csharp" {
            continue;
        }
        let Some(metadata) = identifier.metadata.as_mut() else {
            continue;
        };
        if !metadata.contains_key("arg_count") {
            continue;
        }
        let Some(list) = argument_list(root, identifier.start_byte, identifier.end_byte) else {
            continue;
        };
        let scope = Scope {
            source,
            symbols,
            types,
            at: identifier.start_byte,
            container: identifier.containing_symbol_id.as_deref(),
        };
        if let Some(arg_types) = scope.argument_types(list) {
            metadata.insert("arg_types".to_string(), arg_types.into());
        }
    }
}

fn argument_list(root: Node<'_>, start: u32, end: u32) -> Option<Node<'_>> {
    let mut node = root.named_descendant_for_byte_range(start as usize, end as usize)?;
    while !CALL_KINDS.contains(&node.kind()) {
        node = node.parent()?;
    }
    node.child_by_field_name("arguments")
}

struct Scope<'a> {
    source: &'a str,
    symbols: &'a [Symbol],
    types: &'a HashMap<String, TypeInfo>,
    at: u32,
    container: Option<&'a str>,
}

impl Scope<'_> {
    fn argument_types(&self, list: Node) -> Option<Vec<Option<String>>> {
        let mut found = Vec::new();
        for argument in list.named_children(&mut list.walk()) {
            if argument.kind() != "argument" {
                continue;
            }
            let passes_by_reference = argument
                .children(&mut argument.walk())
                .any(|child| matches!(child.kind(), "ref" | "out" | "in"));
            if argument.child_by_field_name("name").is_some() || passes_by_reference {
                return None;
            }
            let value = argument.children(&mut argument.walk()).last()?;
            found.push(self.type_of(value));
        }
        Some(found)
    }

    fn type_of(&self, node: Node) -> Option<String> {
        let text = |node: Node| {
            self.source
                .get(node.byte_range())
                .map(|text| text.split_whitespace().collect::<String>())
        };
        match node.kind() {
            "parenthesized_expression" => self.type_of(node.named_child(0)?),
            "string_literal"
            | "verbatim_string_literal"
            | "raw_string_literal"
            | "interpolated_string_expression" => Some("string".to_string()),
            "character_literal" => Some("char".to_string()),
            "boolean_literal" => Some("bool".to_string()),
            "integer_literal" => Some(integer_type(&text(node)?).to_string()),
            "real_literal" => Some(real_type(&text(node)?).to_string()),
            "typeof_expression" => Some("Type".to_string()),
            "object_creation_expression" | "cast_expression" | "default_expression" => {
                text(node.child_by_field_name("type")?)
            }
            "as_expression" => text(node.child_by_field_name("right")?),
            "invocation_expression" => {
                let function = node.child_by_field_name("function")?;
                (text(function)? == "nameof").then(|| "string".to_string())
            }
            "this" => self.enclosing_type().map(|symbol| symbol.name.clone()),
            "identifier" => self.value_type(&text(node)?),
            _ => None,
        }
    }

    fn symbol(&self, id: &str) -> Option<&Symbol> {
        self.symbols.iter().find(|symbol| symbol.id == id)
    }

    fn enclosing_type(&self) -> Option<&Symbol> {
        let mut current = self.symbol(self.container?)?;
        while !TYPE_KINDS.contains(&current.kind) {
            current = self.symbol(current.parent_id.as_deref()?)?;
        }
        Some(current)
    }

    /// The declared type of the local, parameter, field, or property that
    /// `name` names here: the nearest callable that declares it before the
    /// call, else a member of the enclosing type.
    fn value_type(&self, name: &str) -> Option<String> {
        let mut scope = self.container.and_then(|id| self.symbol(id));
        while let Some(owner) = scope {
            let declared = self
                .symbols
                .iter()
                .filter(|symbol| {
                    symbol.name == name
                        && VALUE_KINDS.contains(&symbol.kind)
                        && symbol.parent_id.as_deref() == Some(owner.id.as_str())
                        && (TYPE_KINDS.contains(&owner.kind) || symbol.start_byte < self.at)
                })
                .max_by_key(|symbol| symbol.start_byte);
            if let Some(symbol) = declared {
                return self
                    .types
                    .get(&symbol.id)
                    .map(|info| info.resolved_type.split_whitespace().collect());
            }
            if TYPE_KINDS.contains(&owner.kind) {
                return None;
            }
            scope = owner.parent_id.as_deref().and_then(|id| self.symbol(id));
        }
        None
    }
}

fn integer_type(text: &str) -> &'static str {
    let lower = text.to_ascii_lowercase();
    if lower.ends_with("ul") || lower.ends_with("lu") {
        "ulong"
    } else if lower.ends_with('l') {
        "long"
    } else if lower.ends_with('u') {
        "uint"
    } else {
        "int"
    }
}

fn real_type(text: &str) -> &'static str {
    match text.to_ascii_lowercase().chars().last() {
        Some('f') => "float",
        Some('m') => "decimal",
        _ => "double",
    }
}
