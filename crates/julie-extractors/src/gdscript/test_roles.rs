//! Test roles for GUT (`extends GutTest`) and gdUnit4 (`extends GdUnitTestSuite`) suites.

use crate::base::{Symbol, SymbolKind, TestRole};
use crate::test_detection::apply_test_role;
use std::collections::HashMap;

#[derive(Clone, Copy)]
enum Framework {
    Gut,
    GdUnit,
}

/// Marks suite classes as test containers and classifies their methods by
/// the framework's naming rules, whatever directory the script lives in.
pub(super) fn apply_gdscript_test_roles(symbols: &mut [Symbol]) {
    let suites: HashMap<String, Framework> = symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Class)
        .filter_map(|symbol| Some((symbol.id.clone(), suite_framework(symbol)?)))
        .collect();
    if suites.is_empty() {
        return;
    }

    for symbol in symbols.iter_mut() {
        let role = if suites.contains_key(&symbol.id) {
            Some(TestRole::TestContainer)
        } else if symbol.kind == SymbolKind::Method {
            symbol
                .parent_id
                .as_ref()
                .and_then(|parent| suites.get(parent))
                .and_then(|framework| method_role(*framework, &symbol.name))
        } else {
            None
        };
        if let Some(role) = role {
            apply_test_role(symbol.metadata.get_or_insert_with(Default::default), role);
        }
    }
}

fn suite_framework(symbol: &Symbol) -> Option<Framework> {
    let base_types = symbol.metadata.as_ref()?.get("base_types")?.as_array()?;
    base_types.iter().find_map(|base| match base.as_str()? {
        "GutTest" => Some(Framework::Gut),
        "GdUnitTestSuite" => Some(Framework::GdUnit),
        _ => None,
    })
}

fn method_role(framework: Framework, name: &str) -> Option<TestRole> {
    let lifecycle = match framework {
        Framework::Gut => match name {
            "before_all" | "before_each" => Some(TestRole::FixtureSetup),
            "after_all" | "after_each" => Some(TestRole::FixtureTeardown),
            _ => None,
        },
        Framework::GdUnit => match name {
            "before" | "before_test" => Some(TestRole::FixtureSetup),
            "after" | "after_test" => Some(TestRole::FixtureTeardown),
            _ => None,
        },
    };
    lifecycle.or_else(|| name.starts_with("test").then_some(TestRole::TestCase))
}
