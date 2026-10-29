//! Shared parts of the `arg_types`, `arg_names`, and `arg_labels` call
//! metadata: which call identifiers get them, the walk from a name to the
//! declaration that binds it, the declared type of that declaration, and the
//! spelling of a written type.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

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
/// call identifier of `language` that has `arg_count`, no `arg_spread`, and
/// no `arg_types` yet. `describe` gets the identifier's node and the
/// identifier; an answer whose length is not the count adds nothing, since
/// its arguments are not the ones counted.
pub(crate) fn annotate_calls<'tree>(
    language: &str,
    tree: &'tree Tree,
    identifiers: &mut [Identifier],
    mut describe: impl FnMut(Node<'tree>, &Identifier) -> Option<CallArguments>,
) {
    let root = tree.root_node();
    for identifier in identifiers.iter_mut() {
        if identifier.kind != IdentifierKind::Call || identifier.language != language {
            continue;
        }
        let Some(metadata) = identifier.metadata.as_ref() else {
            continue;
        };
        if metadata.contains_key("arg_spread") || metadata.contains_key("arg_types") {
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
        let Some(arguments) = describe(node, identifier) else {
            continue;
        };
        let Some(metadata) = identifier.metadata.as_mut() else {
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
#[derive(Clone)]
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

/// The type text with its whitespace removed, except one space between two
/// word characters: `const char *` gives `const char*`, `std::map<int, int>`
/// gives `std::map<int,int>`, and `List(Of  Integer)` gives `List(Of Integer)`.
pub(crate) fn compact_type_text(text: &str) -> String {
    let mut compact = String::with_capacity(text.len());
    let mut after_space = false;
    for character in text.chars() {
        if character.is_whitespace() {
            after_space = true;
            continue;
        }
        if after_space
            && is_word_character(character)
            && compact.chars().last().is_some_and(is_word_character)
        {
            compact.push(' ');
        }
        after_space = false;
        compact.push(character);
    }
    compact
}

fn is_word_character(character: char) -> bool {
    character.is_alphanumeric() || character == '_' || character == '$'
}

/// The names that the nodes of a member give, such as the names its pattern
/// variables bind or its type checks narrow. It reads the nodes of a member
/// once, so the calls of one long member read each node once, not once per
/// name.
pub(crate) struct MemberNames {
    names: HashMap<usize, HashSet<String>>,
}

impl MemberNames {
    pub(crate) fn new() -> Self {
        Self {
            names: HashMap::new(),
        }
    }

    /// Whether a node at or below `member` gives `name`. `names_of` reads one
    /// node and adds the names that node gives.
    pub(crate) fn holds<'tree>(
        &mut self,
        member: Node<'tree>,
        name: &str,
        mut names_of: impl FnMut(Node<'tree>, &mut HashSet<String>),
    ) -> bool {
        self.names
            .entry(member.id())
            .or_insert_with(|| {
                let mut names = HashSet::new();
                let mut stack = vec![member];
                while let Some(current) = stack.pop() {
                    names_of(current, &mut names);
                    let mut cursor = current.walk();
                    stack.extend(current.named_children(&mut cursor));
                }
                names
            })
            .contains(name)
    }
}

/// Finds the child of a scope that binds a name. A child binds a name only
/// when its text holds the name as a word, so a search reads only those
/// children, and it remembers what each search of a scope for a name found.
/// The calls of one long block then read each statement at most once per
/// name, not once per call.
pub(crate) struct ScopeBindings<'tree, T> {
    /// Whether names ignore case, as in VB.NET.
    ignore_case: bool,
    children: RefCell<HashMap<usize, Rc<ScopeChildren<'tree>>>>,
    /// For a scope id and a name word, the search that read the most children.
    searched: RefCell<HashMap<(usize, String), Search<T>>>,
    /// For a scope id and a name word, the first two children that bind the
    /// name, by position.
    firsts: RefCell<HashMap<(usize, String), FirstBindings<T>>>,
}

/// The first children of a scope that bind a name: each one's position and
/// binding.
type FirstBindings<T> = Rc<Vec<(usize, T)>>;

/// How many of the first children of a scope a search read, and the binding
/// the last of them that binds the name gives.
#[derive(Clone)]
struct Search<T> {
    read: usize,
    binding: Option<T>,
}

struct ScopeChildren<'tree> {
    nodes: Vec<Node<'tree>>,
    /// For each word, the positions of the children whose text holds it.
    words: HashMap<String, Vec<usize>>,
}

impl ScopeChildren<'_> {
    /// The positions of the children whose text can bind `word`: those that
    /// hold it, or every child for a name that is not one word (a Kotlin
    /// name in backticks with a space).
    fn positions(&self, word: &str) -> std::borrow::Cow<'_, [usize]> {
        if word.is_empty() || !word.chars().all(is_word_character) {
            return (0..self.nodes.len()).collect::<Vec<_>>().into();
        }
        self.words.get(word).map_or(&[][..], Vec::as_slice).into()
    }
}

impl<'tree, T: Clone> ScopeBindings<'tree, T> {
    pub(crate) fn new(ignore_case: bool) -> Self {
        Self {
            ignore_case,
            children: RefCell::default(),
            searched: RefCell::default(),
            firsts: RefCell::default(),
        }
    }

    /// The binding of `name` by the last named child of `scope` that binds
    /// it, among the children that end before the byte `before`, or among
    /// all of them when `before` is `None`. `bind` reads one child.
    pub(crate) fn last_binding(
        &self,
        source: &str,
        scope: Node<'tree>,
        before: Option<usize>,
        name: &str,
        mut bind: impl FnMut(Node<'tree>) -> Option<T>,
    ) -> Option<T> {
        let children = self.children_of(source, scope);
        let count = match before {
            Some(limit) => children
                .nodes
                .partition_point(|child| child.end_byte() <= limit),
            None => children.nodes.len(),
        };
        let word = self.fold(name);
        let positions = children.positions(&word);
        let key = (scope.id(), word);
        let known = self.searched.borrow().get(&key).cloned();
        let (read, earlier) = match known {
            Some(search) if search.read <= count => (search.read, search.binding),
            _ => (0, None),
        };
        let first = positions.partition_point(|position| *position < read);
        let last = positions.partition_point(|position| *position < count);
        let found = positions[first..last]
            .iter()
            .rev()
            .find_map(|position| bind(children.nodes[*position]))
            .or(earlier);
        let further = self
            .searched
            .borrow()
            .get(&key)
            .is_some_and(|search| search.read > count);
        if !further {
            self.searched.borrow_mut().insert(
                key,
                Search {
                    read: count,
                    binding: found.clone(),
                },
            );
        }
        found
    }

    /// The binding of `name` by the first named child of `scope` that binds
    /// it, other than `except`, the child that holds the use. `bind` reads
    /// one child.
    pub(crate) fn first_binding_except(
        &self,
        source: &str,
        scope: Node<'tree>,
        except: Node<'tree>,
        name: &str,
        mut bind: impl FnMut(Node<'tree>) -> Option<T>,
    ) -> Option<T> {
        let children = self.children_of(source, scope);
        let word = self.fold(name);
        let key = (scope.id(), word);
        let known = self.firsts.borrow().get(&key).cloned();
        let firsts = match known {
            Some(firsts) => firsts,
            None => {
                let firsts: FirstBindings<T> = Rc::new(
                    children
                        .positions(&key.1)
                        .iter()
                        .filter_map(|position| {
                            bind(children.nodes[*position]).map(|binding| (*position, binding))
                        })
                        .take(2)
                        .collect(),
                );
                self.firsts.borrow_mut().insert(key, Rc::clone(&firsts));
                firsts
            }
        };
        firsts
            .iter()
            .find(|(position, _)| children.nodes[*position].id() != except.id())
            .map(|(_, binding)| binding.clone())
    }

    /// The word a name gives, without the quotes of an escaped name
    /// (`` `default` ``, `[Step]`).
    fn fold(&self, name: &str) -> String {
        let word = name.trim_matches(|character| matches!(character, '`' | '[' | ']'));
        if self.ignore_case {
            word.to_ascii_lowercase()
        } else {
            word.to_string()
        }
    }

    fn children_of(&self, source: &str, scope: Node<'tree>) -> Rc<ScopeChildren<'tree>> {
        if let Some(children) = self.children.borrow().get(&scope.id()) {
            return Rc::clone(children);
        }
        let mut cursor = scope.walk();
        let nodes: Vec<Node<'tree>> = scope.named_children(&mut cursor).collect();
        let mut words: HashMap<String, Vec<usize>> = HashMap::new();
        for (position, node) in nodes.iter().enumerate() {
            let text = source.get(node.byte_range()).unwrap_or_default();
            for word in text
                .split(|character: char| !is_word_character(character))
                .filter(|word| !word.is_empty())
            {
                let positions = words.entry(self.fold(word)).or_default();
                if positions.last() != Some(&position) {
                    positions.push(position);
                }
            }
        }
        let children = Rc::new(ScopeChildren { nodes, words });
        self.children
            .borrow_mut()
            .insert(scope.id(), Rc::clone(&children));
        children
    }
}
