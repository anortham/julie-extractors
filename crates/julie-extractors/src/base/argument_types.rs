//! Shared parts of the `arg_types` and `arg_names` call metadata of Java,
//! Kotlin, and Scala: which call identifiers get them, the walk from a name to
//! the declaration that binds it, and the declared type of that declaration.

use std::collections::HashMap;

use serde_json::Value;
use tree_sitter::{Node, Tree};

use crate::base::{Identifier, IdentifierKind, Symbol, SymbolKind, TypeInfo};

const VALUE_KINDS: [SymbolKind; 4] = [
    SymbolKind::Variable,
    SymbolKind::Field,
    SymbolKind::Property,
    SymbolKind::Constant,
];

/// The arguments of one call in source order: the type each one states, and
/// when one of them is named, the name of each (`None` for a positional one).
pub(crate) struct CallArguments {
    pub(crate) types: Vec<Option<String>>,
    pub(crate) names: Option<Vec<Option<String>>>,
}

/// Adds `arg_types`, and `arg_names` when `describe` gives names, to each
/// call identifier of `language` that has `arg_count` and no `arg_spread`.
/// `describe` gets the identifier's node; an answer whose length is not the
/// count adds nothing, since its arguments are not the ones counted.
pub(crate) fn annotate_calls<'tree>(
    language: &str,
    tree: &'tree Tree,
    identifiers: &mut [Identifier],
    mut describe: impl FnMut(Node<'tree>) -> Option<CallArguments>,
) {
    let root = tree.root_node();
    for identifier in identifiers.iter_mut() {
        if identifier.kind != IdentifierKind::Call || identifier.language != language {
            continue;
        }
        let Some(metadata) = identifier.metadata.as_mut() else {
            continue;
        };
        if metadata.contains_key("arg_spread") {
            continue;
        }
        let Some(count) = metadata.get("arg_count").and_then(Value::as_u64) else {
            continue;
        };
        let Some(node) = root.named_descendant_for_byte_range(
            identifier.start_byte as usize,
            identifier.end_byte as usize,
        ) else {
            continue;
        };
        let Some(arguments) = describe(node) else {
            continue;
        };
        if arguments.types.len() as u64 != count
            || arguments
                .names
                .as_ref()
                .is_some_and(|names| names.len() as u64 != count)
        {
            continue;
        }
        metadata.insert("arg_types".to_string(), arguments.types.into());
        if let Some(names) = arguments.names {
            metadata.insert("arg_names".to_string(), names.into());
        }
    }
}

/// The nearest ancestor of `node`, or `node` itself, whose kind is in `kinds`.
pub(crate) fn enclosing<'tree>(node: Node<'tree>, kinds: &[&str]) -> Option<Node<'tree>> {
    std::iter::successors(Some(node), Node::parent).find(|node| kinds.contains(&node.kind()))
}

/// What a scope says about a name used below it.
pub(crate) enum Found<'tree> {
    /// A declaration binds the name: its name node and its initializer.
    Declaration {
        name: Node<'tree>,
        initializer: Option<Node<'tree>>,
    },
    /// The name has this type here.
    Type(String),
    /// Something binds or may bind the name with a type nothing states.
    Unknown,
}

/// Walks from `at` to the root and returns what the first scope that
/// answers says about the name. `scope` gets each ancestor and the child of
/// it that holds `at`, and answers `None` to pass the name outward.
pub(crate) fn nearest_binding<'tree>(
    at: Node<'tree>,
    mut scope: impl FnMut(Node<'tree>, Node<'tree>) -> Option<Found<'tree>>,
) -> Found<'tree> {
    let mut child = at;
    while let Some(parent) = child.parent() {
        if let Some(found) = scope(parent, child) {
            return found;
        }
        child = parent;
    }
    Found::Unknown
}

/// The named children of `scope` that end before `child` starts.
pub(crate) fn before<'tree>(scope: Node<'tree>, child: Node<'tree>) -> Vec<Node<'tree>> {
    let mut cursor = scope.walk();
    scope
        .named_children(&mut cursor)
        .filter(|node| node.end_byte() <= child.start_byte())
        .collect()
}

pub(crate) fn node_text<'a>(source: &'a str, node: Node) -> &'a str {
    source.get(node.byte_range()).unwrap_or("")
}

/// Type facts of value symbols, looked up by the name node of a declaration.
pub(crate) struct Declarations<'a> {
    by_name: HashMap<&'a str, Vec<&'a Symbol>>,
    types: &'a HashMap<String, TypeInfo>,
}

impl<'a> Declarations<'a> {
    pub(crate) fn new(symbols: &'a [Symbol], types: &'a HashMap<String, TypeInfo>) -> Self {
        let mut by_name: HashMap<&str, Vec<&Symbol>> = HashMap::new();
        for symbol in symbols {
            if VALUE_KINDS.contains(&symbol.kind) {
                by_name
                    .entry(symbol.name.as_str())
                    .or_default()
                    .push(symbol);
            }
        }
        Self { by_name, types }
    }

    /// The type fact of the innermost value symbol named `name` whose span
    /// holds `name_node`, with whitespace removed.
    pub(crate) fn declared_type(&self, name: &str, name_node: Node) -> Option<String> {
        let start = name_node.start_byte() as u32;
        let end = name_node.end_byte() as u32;
        let symbol = self
            .by_name
            .get(name)?
            .iter()
            .filter(|symbol| symbol.start_byte <= start && end <= symbol.end_byte)
            .min_by_key(|symbol| symbol.end_byte - symbol.start_byte)?;
        self.types
            .get(&symbol.id)
            .map(|info| info.resolved_type.split_whitespace().collect())
    }

    /// The type a [`Found`] gives: a declaration's type fact, else the type
    /// `initializer_type` reads from its initializer.
    pub(crate) fn type_of_found(
        &self,
        name: &str,
        found: Found,
        initializer_type: impl FnOnce(Node) -> Option<String>,
    ) -> Option<String> {
        match found {
            Found::Declaration {
                name: node,
                initializer,
            } => self
                .declared_type(name, node)
                .or_else(|| initializer.and_then(initializer_type)),
            Found::Type(type_name) => Some(type_name),
            Found::Unknown => None,
        }
    }
}

/// The type of an integer literal written with an optional `l` or `L` suffix
/// (Java and Scala): `long` or `int` in the language's spelling.
pub(crate) fn suffixed_integer_type(
    text: &str,
    long: &'static str,
    int: &'static str,
) -> &'static str {
    if text.ends_with(['l', 'L']) {
        long
    } else {
        int
    }
}

/// The type of a floating-point literal written with an optional `f`, `F`,
/// `d`, or `D` suffix (Java, Kotlin, and Scala).
pub(crate) fn suffixed_float_type(
    text: &str,
    float: &'static str,
    double: &'static str,
) -> &'static str {
    if text.ends_with(['f', 'F']) {
        float
    } else {
        double
    }
}
