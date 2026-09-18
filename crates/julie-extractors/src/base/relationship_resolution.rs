use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::span::NormalizedSpan;
use super::types::{PendingRelationship, RelationshipKind, Symbol, SymbolKind};

/// Byte-accurate source span for a pending relationship's call site.
///
/// Reuses [`NormalizedSpan`] so a pending row's span is byte-identical to the
/// identifier extracted at the same site — the join key Task 4/5 relies on when
/// attaching resolved relationships back to identifiers by byte span.
pub type PendingSpan = NormalizedSpan;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct UnresolvedTarget {
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "terminalName")]
    pub terminal_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver: Option<String>,
    #[serde(
        rename = "namespacePath",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub namespace_path: Vec<String>,
    #[serde(
        rename = "importContext",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub import_context: Option<String>,
}

impl UnresolvedTarget {
    pub fn simple(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            display_name: name.clone(),
            terminal_name: name,
            receiver: None,
            namespace_path: Vec::new(),
            import_context: None,
        }
    }

    /// Builds the target for a qualified chain `Q1.Q2 ... Qn.t`: the last part
    /// is the terminal name, the part before it the receiver, and the rest the
    /// namespace path. One part or none behaves like [`Self::simple`].
    pub fn from_chain(mut parts: Vec<String>) -> Self {
        if parts.len() < 2 {
            return Self::simple(parts.pop().unwrap_or_default());
        }
        let display_name = parts.join(".");
        let terminal_name = parts.pop().unwrap_or_default();
        let receiver = parts.pop();
        Self {
            display_name,
            terminal_name,
            receiver,
            namespace_path: parts,
            import_context: None,
        }
    }

    /// Splits `text` on every separator and builds the chain target. Returns
    /// `None` when any trimmed part is empty or is not a plain identifier.
    pub fn from_qualified_text(text: &str, separators: &[&str]) -> Option<Self> {
        let mut parts = vec![text.trim().to_string()];
        for separator in separators {
            parts = parts
                .iter()
                .flat_map(|part| part.split(separator).map(|piece| piece.trim().to_string()))
                .collect();
        }
        if parts.iter().all(|part| is_plain_identifier(part)) {
            Some(Self::from_chain(parts))
        } else {
            None
        }
    }
}

fn is_plain_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    let is_identifier_start = |c: char| {
        c.is_ascii_digit() || unicode_ident::is_xid_start(c) || matches!(c, '_' | '$' | '@')
    };
    let is_identifier_continue =
        |c: char| unicode_ident::is_xid_continue(c) || matches!(c, '$' | '@');
    is_identifier_start(first) && chars.all(is_identifier_continue)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StructuredPendingRelationship {
    pub pending: PendingRelationship,
    pub target: UnresolvedTarget,
    #[serde(
        rename = "callerScopeSymbolId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub caller_scope_symbol_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<PendingSpan>,
    #[serde(default)]
    pub reference_site_is_exact: bool,
    /// Enclosing type name, recorded only when the call's receiver is the
    /// language's self reference (`this`/`base`). Rides into the artifact
    /// pending `metadata_json` under key `"receiver_type"`.
    #[serde(
        rename = "receiverType",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub receiver_type: Option<String>,
}

impl StructuredPendingRelationship {
    pub fn new(
        from_symbol_id: String,
        target: UnresolvedTarget,
        caller_scope_symbol_id: Option<String>,
        kind: RelationshipKind,
        file_path: String,
        line_number: u32,
        confidence: f32,
    ) -> Self {
        let display_name = target.display_name.clone();
        Self {
            target,
            caller_scope_symbol_id,
            span: None,
            reference_site_is_exact: false,
            receiver_type: None,
            pending: PendingRelationship {
                from_symbol_id,
                callee_name: display_name,
                kind,
                file_path,
                line_number,
                confidence,
            },
        }
    }

    pub fn with_context_span(mut self, span: PendingSpan) -> Self {
        self.span = Some(span);
        self
    }

    pub fn with_target_span(mut self, span: PendingSpan) -> Self {
        self.span = Some(span);
        self.reference_site_is_exact = true;
        self
    }

    pub fn with_receiver_type(mut self, receiver_type: Option<String>) -> Self {
        self.receiver_type = receiver_type;
        self
    }

    pub fn into_pending_relationship(self) -> PendingRelationship {
        self.pending
    }
}

impl PendingRelationship {
    pub fn legacy(
        from_symbol_id: String,
        callee_name: String,
        kind: RelationshipKind,
        file_path: String,
        line_number: u32,
        confidence: f32,
    ) -> Self {
        Self {
            from_symbol_id,
            callee_name,
            kind,
            file_path,
            line_number,
            confidence,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LocalTargetResolution<'a> {
    Resolved(&'a Symbol),
    Import(&'a Symbol),
    Ambiguous,
    ReceiverQualified,
    Missing,
}

impl<'a> LocalTargetResolution<'a> {
    pub fn as_symbol(self) -> Option<&'a Symbol> {
        match self {
            LocalTargetResolution::Resolved(symbol) | LocalTargetResolution::Import(symbol) => {
                Some(symbol)
            }
            LocalTargetResolution::Ambiguous
            | LocalTargetResolution::ReceiverQualified
            | LocalTargetResolution::Missing => None,
        }
    }
}

pub struct ScopedSymbolIndex<'a> {
    by_name: HashMap<&'a str, Vec<&'a Symbol>>,
}

impl<'a> ScopedSymbolIndex<'a> {
    pub fn new(symbols: &'a [Symbol]) -> Self {
        let mut by_name: HashMap<&'a str, Vec<&'a Symbol>> = HashMap::new();
        for symbol in symbols {
            by_name
                .entry(symbol.name.as_str())
                .or_default()
                .push(symbol);
        }
        Self { by_name }
    }

    pub fn unique_symbol_map(symbols: &'a [Symbol]) -> HashMap<String, &'a Symbol> {
        let index = Self::new(symbols);
        index
            .by_name
            .into_iter()
            .filter_map(|(name, candidates)| match candidates.as_slice() {
                [symbol] => Some((name.to_string(), *symbol)),
                _ => None,
            })
            .collect()
    }

    pub fn first_by_name(&self, name: &str) -> Option<&'a Symbol> {
        self.by_name
            .get(name)
            .and_then(|candidates| candidates.first().copied())
    }

    pub fn candidates_by_name(&self, name: &str) -> impl Iterator<Item = &'a Symbol> + '_ {
        self.by_name
            .get(name)
            .into_iter()
            .flat_map(|candidates| candidates.iter().copied())
    }

    pub fn resolve_call_target(
        &self,
        terminal_name: &str,
        caller: Option<&Symbol>,
        receiver: Option<&str>,
    ) -> LocalTargetResolution<'a> {
        let Some(candidates) = self.by_name.get(terminal_name) else {
            return LocalTargetResolution::Missing;
        };

        if receiver.is_some_and(|receiver| !is_self_receiver(receiver)) {
            return LocalTargetResolution::ReceiverQualified;
        }

        let callable: Vec<&Symbol> = candidates
            .iter()
            .copied()
            .filter(|symbol| is_callable_or_import(&symbol.kind))
            .collect();
        if callable.is_empty() {
            return LocalTargetResolution::Missing;
        }

        if receiver.is_some() {
            return resolve_self_receiver_target(&callable, caller);
        }

        unique_candidate(&callable)
    }
}

fn resolve_self_receiver_target<'a>(
    candidates: &[&'a Symbol],
    caller: Option<&Symbol>,
) -> LocalTargetResolution<'a> {
    let Some(caller_parent_id) = caller.and_then(|caller| caller.parent_id.as_deref()) else {
        return LocalTargetResolution::Missing;
    };

    let same_parent: Vec<&Symbol> = candidates
        .iter()
        .copied()
        .filter(|symbol| symbol.parent_id.as_deref() == Some(caller_parent_id))
        .collect();
    if same_parent.is_empty() {
        return LocalTargetResolution::Missing;
    }

    unique_candidate(&same_parent)
}

fn unique_candidate<'a>(candidates: &[&'a Symbol]) -> LocalTargetResolution<'a> {
    if let Some(symbol) = unique_concrete_definition(candidates) {
        return LocalTargetResolution::Resolved(symbol);
    }

    match candidates {
        [] => LocalTargetResolution::Missing,
        [symbol] if symbol.kind == SymbolKind::Import => LocalTargetResolution::Import(symbol),
        [symbol] => LocalTargetResolution::Resolved(symbol),
        _ => LocalTargetResolution::Ambiguous,
    }
}

fn unique_concrete_definition<'a>(candidates: &[&'a Symbol]) -> Option<&'a Symbol> {
    let mut definition = None;
    for symbol in candidates {
        match symbol_definition_status(symbol)? {
            true if definition.replace(*symbol).is_some() => return None,
            true => {}
            false => {}
        }
    }
    definition
}

fn symbol_definition_status(symbol: &Symbol) -> Option<bool> {
    let value = symbol.metadata.as_ref()?.get("isDefinition")?;
    match value {
        serde_json::Value::Bool(value) => Some(*value),
        serde_json::Value::String(value) => value.parse::<bool>().ok(),
        _ => None,
    }
}

fn is_self_receiver(receiver: &str) -> bool {
    matches!(receiver, "self" | "this" | "Self")
}

fn is_callable_or_import(kind: &SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor | SymbolKind::Import
    )
}

#[cfg(test)]
mod tests {
    use super::UnresolvedTarget;

    fn parts(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn from_chain_with_three_parts_keeps_receiver_and_namespace() {
        let target = UnresolvedTarget::from_chain(parts(&["Outer", "Inner", "Method"]));

        assert_eq!(target.terminal_name, "Method");
        assert_eq!(target.receiver.as_deref(), Some("Inner"));
        assert_eq!(target.namespace_path, parts(&["Outer"]));
        assert_eq!(target.display_name, "Outer.Inner.Method");
        assert_eq!(target.import_context, None);
    }

    #[test]
    fn from_chain_with_two_parts_has_empty_namespace() {
        let target = UnresolvedTarget::from_chain(parts(&["Helper", "Process"]));

        assert_eq!(target.terminal_name, "Process");
        assert_eq!(target.receiver.as_deref(), Some("Helper"));
        assert!(target.namespace_path.is_empty());
        assert_eq!(target.display_name, "Helper.Process");
    }

    #[test]
    fn from_chain_with_one_part_is_simple() {
        assert_eq!(
            UnresolvedTarget::from_chain(parts(&["Run"])),
            UnresolvedTarget::simple("Run")
        );
    }

    #[test]
    fn from_chain_with_no_parts_is_empty_simple() {
        assert_eq!(
            UnresolvedTarget::from_chain(Vec::new()),
            UnresolvedTarget::simple("")
        );
    }

    #[test]
    fn from_qualified_text_splits_on_dot() {
        let target = UnresolvedTarget::from_qualified_text("Outer.Inner", &["."]).unwrap();

        assert_eq!(target.terminal_name, "Inner");
        assert_eq!(target.receiver.as_deref(), Some("Outer"));
        assert!(target.namespace_path.is_empty());
        assert_eq!(target.display_name, "Outer.Inner");
    }

    #[test]
    fn from_qualified_text_splits_on_every_separator() {
        let target = UnresolvedTarget::from_qualified_text("a::b->c", &["::", "->"]).unwrap();

        assert_eq!(target.terminal_name, "c");
        assert_eq!(target.receiver.as_deref(), Some("b"));
        assert_eq!(target.namespace_path, parts(&["a"]));
        assert_eq!(target.display_name, "a.b.c");
    }

    #[test]
    fn from_qualified_text_trims_parts() {
        let target = UnresolvedTarget::from_qualified_text("Outer . Inner", &["."]).unwrap();

        assert_eq!(target.display_name, "Outer.Inner");
    }

    #[test]
    fn from_qualified_text_rejects_call_syntax() {
        assert_eq!(
            UnresolvedTarget::from_qualified_text("foo().bar", &["."]),
            None
        );
    }

    #[test]
    fn from_qualified_text_rejects_empty_part() {
        assert_eq!(UnresolvedTarget::from_qualified_text("a..b", &["."]), None);
        assert_eq!(UnresolvedTarget::from_qualified_text("", &["."]), None);
    }

    #[test]
    fn from_qualified_text_keeps_leading_dollar() {
        let target = UnresolvedTarget::from_qualified_text("$outer.inner", &["."]).unwrap();

        assert_eq!(target.terminal_name, "inner");
        assert_eq!(target.receiver.as_deref(), Some("$outer"));
        assert_eq!(target.display_name, "$outer.inner");
    }

    #[test]
    fn from_qualified_text_single_identifier_is_simple() {
        assert_eq!(
            UnresolvedTarget::from_qualified_text("Run", &["."]),
            Some(UnresolvedTarget::simple("Run"))
        );
    }
}
