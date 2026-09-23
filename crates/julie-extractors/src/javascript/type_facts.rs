//! Declared-type fact recording for JavaScript.

use crate::base::types::TypeNameRules;
use crate::base::{BaseExtractor, Symbol, SymbolKind};
use regex::Regex;
use std::sync::LazyLock;
use tree_sitter::Node;

pub(crate) const TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &[],
    generic_open: &['<'],
};

/// Record an inferred type fact for `symbol_id` when `value_node` is a plain
/// `new Identifier(...)` expression. Qualified constructors such as
/// `new ns.Foo()` record nothing.
pub(crate) fn record_new_expression_fact(
    base: &mut BaseExtractor,
    symbol_id: &str,
    value_node: Node,
    rules: &TypeNameRules,
) {
    if value_node.kind() != "new_expression" {
        return;
    }
    let Some(constructor) = value_node.child_by_field_name("constructor") else {
        return;
    };
    if constructor.kind() != "identifier" {
        return;
    }
    let declared = base.get_node_text(&constructor);
    base.record_declared_type_fact(symbol_id, &declared, rules, true);
}

/// JSDoc type text decorations: `?T`/`!T` nullability, `T=` optional
/// parameters, `...T` rest parameters, and generic arguments.
const JSDOC_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &["=", "?"],
    reference_prefixes: &["...", "?", "!"],
    generic_open: &['<'],
};

static JSDOC_RETURNS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"@returns?\s*\{([^}]+)\}").unwrap());
static JSDOC_TYPE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"@type\s*\{([^}]+)\}").unwrap());
static JSDOC_PARAM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"@(?:param|arg|argument)\s*\{([^}]+)\}\s*\[?([A-Za-z_$][\w$]*)").unwrap()
});

/// Record the JSDoc-declared type of a symbol: `@returns {T}` for callables,
/// `@type {T}` for variables, properties, and fields.
pub(crate) fn record_jsdoc_symbol_fact(base: &mut BaseExtractor, symbol: &Symbol) {
    let Some(doc) = symbol.doc_comment.as_deref() else {
        return;
    };
    let pattern = match symbol.kind {
        SymbolKind::Function | SymbolKind::Method => &*JSDOC_RETURNS_RE,
        SymbolKind::Variable | SymbolKind::Property | SymbolKind::Field => &*JSDOC_TYPE_RE,
        _ => return,
    };
    let Some(declared) = pattern
        .captures(doc)
        .map(|captures| captures[1].trim().to_string())
    else {
        return;
    };
    if matches!(declared.as_str(), "void" | "undefined" | "*") {
        return;
    }
    base.record_declared_type_fact(&symbol.id, &declared, &JSDOC_TYPE_NAME_RULES, false);
}

/// Record a parameter's type from its callable's `@param {T} name` tag.
pub(crate) fn record_jsdoc_param_fact(
    base: &mut BaseExtractor,
    parameter: &Symbol,
    callable_doc: Option<&str>,
) {
    let Some(doc) = callable_doc else {
        return;
    };
    let Some(declared) = JSDOC_PARAM_RE
        .captures_iter(doc)
        .find(|captures| captures[2] == *parameter.name)
        .map(|captures| captures[1].trim().to_string())
    else {
        return;
    };
    if declared == "*" {
        return;
    }
    base.record_declared_type_fact(&parameter.id, &declared, &JSDOC_TYPE_NAME_RULES, false);
}
