/// Lua class pattern detection
///
/// Post-processes symbols to detect Lua class patterns. A form that only
/// declares classes is enough on its own:
/// - `Base:extend()` / `Base:subclass("Name")` (classic, rxi, 30log, middleclass)
/// - `class("Name", Base)` (middleclass)
/// - module-scope `setmetatable({}, { __index = Base })`
/// - a `---@class Name : Base` LuaLS annotation
///
/// A form that also builds instances needs corroboration from the class body:
/// an `__index` field, a `new` method, or colon methods on the name.
/// - `setmetatable({}, Base)` and `Base:new(...)` / `Base.new(...)` values
/// - plain tables need `__index`, or both `new` and colon methods
use crate::base::{Symbol, SymbolKind};
use regex::Regex;
use std::sync::LazyLock;

static SETMETATABLE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"setmetatable\(\s*\{\s*\}\s*,\s*(\w+)\s*\)").unwrap());

static SETMETATABLE_INDEX_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"setmetatable\(\s*\{\s*\}\s*,\s*\{\s*__index\s*=\s*(\w+)\s*,?\s*\}\s*\)").unwrap()
});

static EXTEND_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"=\s*(\w+):(?:extend|subclass)\(").unwrap());

static CLASS_CALL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"=\s*class\(\s*["'][\w.]+["']\s*(?:,\s*(\w+)\s*)?\)"#).unwrap());

static NEW_CALL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"=\s*(\w+)[.:]new\s*[({]").unwrap());

static ANNOTATED_CLASS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*---\s*@class\s+(?:\([^)]*\)\s*)?([\w.]+)\s*(?::\s*([\w.]+))?").unwrap()
});

/// The class a function-local `local x = setmetatable({}, Class)` instance belongs to.
pub(super) fn instance_metatable_name(symbol: &Symbol) -> Option<&str> {
    if symbol.kind != SymbolKind::Variable {
        return None;
    }
    SETMETATABLE_RE
        .captures(symbol.signature.as_deref()?)
        .and_then(|captures| captures.get(1))
        .map(|name| name.as_str())
}

/// Detect and upgrade Lua class patterns, recording `baseClass` metadata.
pub(crate) fn detect_lua_classes(symbols: &mut [Symbol]) {
    let upgrades: Vec<(usize, Option<String>)> = symbols
        .iter()
        .enumerate()
        .filter(|(_, symbol)| is_class_candidate(symbol))
        .filter_map(|(index, symbol)| class_evidence(symbols, symbol).map(|base| (index, base)))
        .collect();

    for (index, base_class) in upgrades {
        let symbol = &mut symbols[index];
        symbol.kind = SymbolKind::Class;
        if let Some(base_class) = base_class {
            symbol
                .metadata
                .get_or_insert_with(Default::default)
                .insert("baseClass".to_string(), base_class.into());
        }
    }
}

fn is_class_candidate(symbol: &Symbol) -> bool {
    matches!(symbol.kind, SymbolKind::Variable | SymbolKind::Field)
        && symbol
            .metadata
            .as_ref()
            .is_none_or(|metadata| !metadata.contains_key("role"))
}

/// `Some(base)` when `symbol` declares a class, with its base class if one is named.
fn class_evidence(symbols: &[Symbol], symbol: &Symbol) -> Option<Option<String>> {
    if let Some(base) = annotated_class(symbol) {
        return Some(base);
    }
    if symbol.kind != SymbolKind::Variable {
        return None;
    }
    let signature = symbol.signature.as_deref().unwrap_or_default();
    let capture = |re: &Regex| {
        re.captures(signature)
            .map(|captures| captures.get(1).map(|base| base.as_str().to_string()))
    };

    if let Some(base) = capture(&EXTEND_RE).or_else(|| capture(&CLASS_CALL_RE)) {
        return Some(base);
    }
    if symbol.parent_id.is_none()
        && let Some(base) = capture(&SETMETATABLE_INDEX_RE)
    {
        return Some(base);
    }

    let has_index = has_member(symbols, symbol, |member| member.name == "__index");
    let has_new = has_member(symbols, symbol, |member| {
        member.kind == SymbolKind::Method && member.name == "new"
    });
    let colon_prefix = format!("function {}:", symbol.name);
    let has_colon_methods = has_member(symbols, symbol, |member| {
        member.kind == SymbolKind::Method
            && member
                .signature
                .as_deref()
                .is_some_and(|signature| signature.starts_with(&colon_prefix))
    });

    if let Some(base) = capture(&SETMETATABLE_RE).or_else(|| capture(&NEW_CALL_RE)) {
        return (has_index || has_new || has_colon_methods).then_some(base);
    }
    let is_table = symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("dataType"))
        .and_then(|data_type| data_type.as_str())
        == Some("table");
    (is_table && (has_index || (has_new && has_colon_methods))).then_some(None)
}

fn has_member(symbols: &[Symbol], owner: &Symbol, predicate: impl Fn(&Symbol) -> bool) -> bool {
    symbols
        .iter()
        .any(|member| member.parent_id.as_deref() == Some(owner.id.as_str()) && predicate(member))
}

/// `Some(base)` when a `---@class Name` annotation above `symbol` names it.
fn annotated_class(symbol: &Symbol) -> Option<Option<String>> {
    let doc = symbol.doc_comment.as_deref()?;
    ANNOTATED_CLASS_RE
        .captures_iter(doc)
        .find(|captures| {
            captures
                .get(1)
                .is_some_and(|name| name.as_str().rsplit('.').next() == Some(symbol.name.as_str()))
        })
        .map(|captures| captures.get(2).map(|base| base.as_str().to_string()))
}
