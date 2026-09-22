//! Links rows written with their owner's name, such as the out-of-line
//! `void Widget::draw() {}`, `int Widget::count = 0;`, or `class Outer::Inner {}`,
//! to the owning class or namespace defined in the same file.

use crate::base::{Symbol, SymbolKind};
use std::collections::HashMap;

use super::helpers;

pub(super) fn link_scoped_definitions(symbols: &mut [Symbol]) {
    let links: Vec<(usize, usize)> = symbols
        .iter()
        .enumerate()
        .filter_map(|(index, symbol)| {
            let scope = symbol.metadata.as_ref()?.get("scope")?.as_str()?;
            let owner = find_owner(symbols, &helpers::strip_template_arguments(scope))?;
            Some((index, owner))
        })
        .collect();

    for (index, owner) in links {
        let owner_id = symbols[owner].id.clone();
        let owner_is_namespace = symbols[owner].kind == SymbolKind::Namespace;
        let member = member_name(&symbols[index].name).to_string();
        let declared_visibility = symbols
            .iter()
            .find(|declaration| {
                declaration.parent_id.as_deref() == Some(owner_id.as_str())
                    && declaration.id != symbols[index].id
                    && declaration.name == member
            })
            .and_then(|declaration| declaration.visibility.clone());

        let symbol = &mut symbols[index];
        symbol.parent_id = Some(owner_id);
        if owner_is_namespace && symbol.kind == SymbolKind::Method {
            symbol.kind = SymbolKind::Function;
        }
        if let Some(visibility) = declared_visibility {
            symbol.visibility = Some(visibility);
        }
    }
}

fn member_name(name: &str) -> &str {
    helpers::split_scope(name).map_or(name, |(_, member)| member)
}

/// The one class, struct, union, or namespace whose qualified name ends with `scope`.
fn find_owner(symbols: &[Symbol], scope: &str) -> Option<usize> {
    let by_id: HashMap<&str, &Symbol> = symbols.iter().map(|s| (s.id.as_str(), s)).collect();
    let suffix = format!("::{scope}");
    let owners: Vec<usize> = symbols
        .iter()
        .enumerate()
        .filter(|(_, symbol)| {
            matches!(
                symbol.kind,
                SymbolKind::Class | SymbolKind::Struct | SymbolKind::Union | SymbolKind::Namespace
            )
        })
        .filter(|(_, symbol)| {
            let qualified = qualified_name(symbol, &by_id);
            qualified == scope || qualified.ends_with(&suffix)
        })
        .map(|(index, _)| index)
        .collect();
    match owners.as_slice() {
        [owner] => Some(*owner),
        _ => None,
    }
}

fn qualified_name(symbol: &Symbol, by_id: &HashMap<&str, &Symbol>) -> String {
    let mut segments = vec![symbol.name.as_str()];
    let mut parent = symbol.parent_id.as_deref();
    while let Some(id) = parent {
        let Some(owner) = by_id.get(id) else {
            break;
        };
        segments.push(owner.name.as_str());
        parent = owner.parent_id.as_deref();
    }
    segments.reverse();
    segments.join("::")
}
