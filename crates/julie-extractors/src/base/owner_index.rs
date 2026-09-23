//! The symbol that owns a call or identifier, found through syntax ancestors.
//!
//! Used by extractors whose span-priority lookup would hand a field
//! initializer, property accessor, or destructor body to the enclosing type.

use crate::base::{BaseExtractor, ContainingSymbolIndex, Symbol, SymbolKind};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

/// Maps each owning declaration's byte range to its symbol, so a field
/// initializer belongs to its field, a constructor body to its constructor,
/// and a `test(...)` closure to its test. Locals and parameters never own
/// code. Nodes outside every declaration fall back to the span index.
pub(crate) struct OwnerIndex<'a> {
    by_range: HashMap<(u32, u32), &'a Symbol>,
    fallback: ContainingSymbolIndex<'a>,
}

impl<'a> OwnerIndex<'a> {
    pub(crate) fn new(base: &BaseExtractor, symbols: &'a [Symbol]) -> Self {
        Self::new_filtered(base, symbols, |_| true)
    }

    /// An index over the symbols `keep` accepts, so a language can exclude
    /// rows that name code without owning it (ECMAScript export rows).
    pub(crate) fn new_filtered(
        base: &BaseExtractor,
        symbols: &'a [Symbol],
        keep: impl Fn(&Symbol) -> bool,
    ) -> Self {
        let callable_ids: HashSet<&str> = symbols
            .iter()
            .filter(|symbol| keep(symbol) && is_callable(&symbol.kind))
            .map(|symbol| symbol.id.as_str())
            .collect();
        let mut by_range = HashMap::new();
        for symbol in symbols.iter().filter(|symbol| keep(symbol)) {
            let owns_code = match symbol.kind {
                SymbolKind::Function
                | SymbolKind::Method
                | SymbolKind::Constructor
                | SymbolKind::Destructor
                | SymbolKind::Operator
                | SymbolKind::Property
                | SymbolKind::Field
                | SymbolKind::Constant => true,
                SymbolKind::Variable => symbol
                    .parent_id
                    .as_deref()
                    .is_none_or(|parent| !callable_ids.contains(parent)),
                _ => false,
            };
            if owns_code {
                by_range
                    .entry((symbol.start_byte, symbol.end_byte))
                    .or_insert(symbol);
            }
        }
        Self {
            by_range,
            fallback: ContainingSymbolIndex::from_iter(
                symbols
                    .iter()
                    .filter(|symbol| symbol.file_path == base.file_path && keep(symbol)),
            ),
        }
    }

    pub(crate) fn find(&self, node: Node) -> Option<&'a Symbol> {
        let mut current = node.parent();
        while let Some(ancestor) = current {
            if let Some(symbol) = self
                .by_range
                .get(&(ancestor.start_byte() as u32, ancestor.end_byte() as u32))
            {
                return Some(symbol);
            }
            current = ancestor.parent();
        }
        self.fallback.find(node)
    }
}

fn is_callable(kind: &SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Function
            | SymbolKind::Method
            | SymbolKind::Constructor
            | SymbolKind::Destructor
            | SymbolKind::Operator
    )
}

/// A test DSL call symbol, such as Dart's `test('adds', ...)` or Quick's
/// `it("adds")`: it names a closure, so it is never the target of a call.
pub(crate) fn is_test_call_symbol(symbol: &Symbol) -> bool {
    symbol.kind == SymbolKind::Function
        && symbol.visibility.is_none()
        && symbol
            .metadata
            .as_ref()
            .is_some_and(|metadata| metadata.contains_key("test_role"))
}
