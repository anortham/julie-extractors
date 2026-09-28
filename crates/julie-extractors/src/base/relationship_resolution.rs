use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

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
    /// Argument count of the call site, for languages whose function identity
    /// includes arity (Erlang `name/arity`). Rides into the artifact pending
    /// `metadata_json` under key `"arity"`.
    #[serde(
        rename = "targetArity",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub target_arity: Option<u32>,
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
            target_arity: None,
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

    pub fn with_target_arity(mut self, arity: Option<u32>) -> Self {
        self.target_arity = arity;
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
    by_id: HashMap<&'a str, &'a Symbol>,
    by_inner_name: HashMap<&'a str, Vec<&'a Symbol>>,
    prototype_owner_ids: HashSet<&'a str>,
}

impl<'a> ScopedSymbolIndex<'a> {
    pub fn new(symbols: &'a [Symbol]) -> Self {
        let mut by_name: HashMap<&'a str, Vec<&'a Symbol>> = HashMap::new();
        let mut by_id = HashMap::new();
        let mut by_inner_name: HashMap<&'a str, Vec<&'a Symbol>> = HashMap::new();
        let mut prototype_owner_ids = HashSet::new();
        for symbol in symbols {
            by_name
                .entry(symbol.name.as_str())
                .or_default()
                .push(symbol);
            by_id.insert(symbol.id.as_str(), symbol);
            if let Some(name) = metadata_str(symbol, "innerName") {
                by_inner_name.entry(name).or_default().push(symbol);
            }
            if symbol.kind == SymbolKind::Method
                && symbol
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("isPrototypeMethod"))
                    .and_then(serde_json::Value::as_bool)
                    == Some(true)
                && let Some(parent_id) = symbol.parent_id.as_deref()
            {
                prototype_owner_ids.insert(parent_id);
            }
        }
        Self {
            by_name,
            by_id,
            by_inner_name,
            prototype_owner_ids,
        }
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
        call_site: Node<'_>,
    ) -> LocalTargetResolution<'a> {
        if receiver.is_some_and(|receiver| !is_self_receiver(receiver)) {
            return LocalTargetResolution::ReceiverQualified;
        }

        if receiver.is_some() {
            if receiver == Some("super") {
                return LocalTargetResolution::ReceiverQualified;
            }
            if is_ecmascript_language(caller) && ecmascript_this_is_unbound(self, call_site, caller)
            {
                return LocalTargetResolution::ReceiverQualified;
            }
            return self.resolve_self_receiver_target(terminal_name, caller);
        }

        let candidates: Vec<&'a Symbol> = self.candidates_by_name(terminal_name).collect();
        let lua_predeclared_targets: Vec<&'a Symbol> =
            if caller.is_some_and(|caller| caller.language == "lua") {
                candidates
                    .iter()
                    .copied()
                    .filter(|target| target.kind == SymbolKind::Function)
                    .filter(|target| {
                        candidates.iter().copied().any(|placeholder| {
                            lua_local_placeholder_precedes_function(placeholder, target, call_site)
                        })
                    })
                    .collect()
            } else {
                Vec::new()
            };
        let mut visible: Vec<(usize, &'a Symbol)> = candidates
            .into_iter()
            .filter_map(|symbol| {
                let lua_placeholder_superseded = symbol.kind == SymbolKind::Variable
                    && lua_predeclared_targets.iter().any(|target| {
                        lua_local_placeholder_precedes_function(symbol, target, call_site)
                    });
                self.lexical_distance(symbol, caller)
                    .filter(|_| is_bare_binding(symbol, caller, &self.by_id))
                    .filter(|_| !lua_placeholder_superseded)
                    .filter(|_| {
                        binding_is_visible(symbol, call_site)
                            || lua_predeclared_targets
                                .iter()
                                .any(|target| target.id == symbol.id)
                    })
                    .map(|distance| (distance, symbol))
            })
            .collect();

        if is_ecmascript_language(caller)
            && let Some(inner) = self
                .by_inner_name
                .get(terminal_name)
                .into_iter()
                .flatten()
                .copied()
                .filter(|symbol| self.ecmascript_inner_name_is_visible(symbol, caller, call_site))
                .filter_map(|symbol| {
                    self.lexical_distance_to_symbol(symbol, caller)
                        .map(|distance| (distance, symbol))
                })
                .min_by_key(|(distance, _)| *distance)
        {
            visible.push(inner);
        }

        resolve_nearest_binding(visible, |symbol| {
            is_callable_target(symbol, caller) && target_value_available(symbol, call_site)
        })
    }

    pub fn resolve_constructable_target(
        &self,
        terminal_name: &str,
        caller: Option<&Symbol>,
        call_site: Node<'_>,
    ) -> LocalTargetResolution<'a> {
        let visible = self
            .candidates_by_name(terminal_name)
            .filter_map(|symbol| {
                (symbol.kind != SymbolKind::Constructor)
                    .then_some(symbol)
                    .filter(|symbol| is_bare_binding(symbol, caller, &self.by_id))
                    .filter(|symbol| binding_is_visible(symbol, call_site))
                    .and_then(|symbol| {
                        self.lexical_distance(symbol, caller)
                            .map(|distance| (distance, symbol))
                    })
            })
            .collect();
        resolve_nearest_binding(visible, |symbol| {
            is_constructable_target(symbol, &self.by_id)
                && target_value_available(symbol, call_site)
        })
    }

    pub fn resolve_macro_target(
        &self,
        terminal_name: &str,
        caller: Option<&Symbol>,
        call_site: Node<'_>,
    ) -> Option<&'a Symbol> {
        self.candidates_by_name(terminal_name)
            .filter(|symbol| metadata_str(symbol, "rustSymbolKind") == Some("macro_rules"))
            .filter(|symbol| symbol.start_byte <= call_site.start_byte() as u32)
            .filter(|symbol| generic_block_binding_is_visible(symbol, call_site))
            .filter_map(|symbol| {
                self.lexical_distance(symbol, caller)
                    .map(|distance| (distance, symbol))
            })
            .min_by_key(|(distance, symbol)| (*distance, std::cmp::Reverse(symbol.start_byte)))
            .map(|(_, symbol)| symbol)
    }

    fn resolve_self_receiver_target(
        &self,
        terminal_name: &str,
        caller: Option<&Symbol>,
    ) -> LocalTargetResolution<'a> {
        let Some(owner_id) = self.enclosing_type_id(caller) else {
            return LocalTargetResolution::Missing;
        };
        let candidates: Vec<&Symbol> = self
            .candidates_by_name(terminal_name)
            .filter(|symbol| symbol.parent_id.as_deref() == Some(owner_id.as_str()))
            .filter(|symbol| is_callable_or_import(&symbol.kind))
            .collect();
        unique_candidate(&candidates)
    }

    fn lexical_distance(&self, symbol: &Symbol, caller: Option<&Symbol>) -> Option<usize> {
        if symbol.parent_id.is_none() {
            return Some(self.scope_chain(caller).len() + 1);
        }
        self.scope_chain(caller)
            .iter()
            .position(|scope| Some(scope.as_str()) == symbol.parent_id.as_deref())
    }

    fn lexical_distance_to_symbol(
        &self,
        symbol: &Symbol,
        caller: Option<&Symbol>,
    ) -> Option<usize> {
        self.scope_chain(caller)
            .iter()
            .position(|scope| *scope == symbol.id)
            .map(|distance| distance + 1)
    }

    fn scope_chain(&self, caller: Option<&Symbol>) -> Vec<String> {
        let mut current_id = caller.map(|caller| caller.id.clone());
        let mut seen = HashMap::<String, ()>::new();
        let mut chain = Vec::new();
        while let Some(id) = current_id {
            if seen.insert(id.clone(), ()).is_some() {
                break;
            }
            current_id = self
                .by_id
                .get(id.as_str())
                .and_then(|symbol| symbol.parent_id.clone());
            chain.push(id);
        }
        chain
    }

    fn enclosing_type_id(&self, caller: Option<&Symbol>) -> Option<String> {
        if caller.is_some_and(|caller| is_type_scope(&caller.kind)) {
            return caller.map(|caller| caller.id.clone());
        }
        let mut current_id = caller.map(|caller| caller.id.clone());
        while let Some(id) = current_id {
            let symbol = self.by_id.get(id.as_str()).copied()?;
            if is_type_scope(&symbol.kind) {
                return Some(symbol.id.clone());
            }
            if is_ecmascript_language_name(&symbol.language)
                && symbol.kind == SymbolKind::Method
                && symbol
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("isPrototypeMethod"))
                    .and_then(serde_json::Value::as_bool)
                    == Some(true)
            {
                return symbol.parent_id.clone();
            }
            if is_ecmascript_language_name(&symbol.language)
                && symbol.kind == SymbolKind::Function
                && self.prototype_owner_ids.contains(symbol.id.as_str())
            {
                return Some(symbol.id.clone());
            }
            if is_ecmascript_language_name(&symbol.language)
                && symbol.kind == SymbolKind::Function
                && symbol
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("isArrowFunction"))
                    .and_then(serde_json::Value::as_bool)
                    != Some(true)
            {
                return None;
            }
            current_id = symbol.parent_id.clone();
        }
        None
    }

    fn ecmascript_inner_name_is_visible(
        &self,
        symbol: &Symbol,
        caller: Option<&Symbol>,
        call_site: Node<'_>,
    ) -> bool {
        let Some(caller) = caller else {
            return false;
        };
        symbol.start_byte <= call_site.start_byte() as u32
            && call_site.end_byte() as u32 <= symbol.end_byte
            && self.scope_chain(Some(caller)).contains(&symbol.id)
    }
}

fn resolve_nearest_binding<'a>(
    visible: Vec<(usize, &'a Symbol)>,
    is_target: impl Fn(&Symbol) -> bool,
) -> LocalTargetResolution<'a> {
    let Some(nearest) = visible.iter().map(|(distance, _)| *distance).min() else {
        return LocalTargetResolution::Missing;
    };
    let bindings: Vec<&Symbol> = visible
        .into_iter()
        .filter_map(|(distance, symbol)| (distance == nearest).then_some(symbol))
        .collect();
    if bindings.iter().any(|symbol| !is_target(symbol)) {
        return LocalTargetResolution::Ambiguous;
    }
    unique_candidate(&bindings)
}

fn metadata_str<'a>(symbol: &'a Symbol, key: &str) -> Option<&'a str> {
    symbol.metadata.as_ref()?.get(key)?.as_str()
}

fn is_bare_binding(
    symbol: &Symbol,
    caller: Option<&Symbol>,
    by_id: &HashMap<&str, &Symbol>,
) -> bool {
    let language = caller
        .map(|caller| caller.language.as_str())
        .unwrap_or(symbol.language.as_str());
    if symbol.kind == SymbolKind::Method
        && (!allows_implicit_methods(language) || matches!(language, "python"))
    {
        return false;
    }
    if symbol.language == "ruby"
        && symbol.kind == SymbolKind::Method
        && ruby_method_is_static(symbol) != caller.is_some_and(ruby_method_is_static)
    {
        return false;
    }
    if is_ecmascript_language_name(language) && is_class_member(symbol, by_id) {
        return false;
    }
    !matches!(symbol.kind, SymbolKind::Export | SymbolKind::EnumMember)
}

fn is_callable_target(symbol: &Symbol, caller: Option<&Symbol>) -> bool {
    if symbol.language == "c"
        && symbol.kind == SymbolKind::Variable
        && metadata_str(symbol, "isFunctionPointer") == Some("true")
    {
        return true;
    }
    if !is_callable_or_import(&symbol.kind) {
        return false;
    }
    match symbol.kind {
        SymbolKind::Method | SymbolKind::Constructor => allows_implicit_methods(
            caller
                .map(|caller| caller.language.as_str())
                .unwrap_or(symbol.language.as_str()),
        ),
        _ => true,
    }
}

fn is_constructable_target(symbol: &Symbol, by_id: &HashMap<&str, &Symbol>) -> bool {
    if matches!(
        symbol.kind,
        SymbolKind::Class | SymbolKind::Struct | SymbolKind::Type | SymbolKind::Interface
    ) {
        return true;
    }
    symbol.kind == SymbolKind::Function
        && by_id.values().any(|child| {
            child.parent_id.as_deref() == Some(symbol.id.as_str())
                && child.kind == SymbolKind::Method
        })
}

fn allows_implicit_methods(language: &str) -> bool {
    matches!(
        language,
        "java"
            | "csharp"
            | "vbnet"
            | "cpp"
            | "dart"
            | "swift"
            | "kotlin"
            | "scala"
            | "gdscript"
            | "ruby"
            | "zig"
    )
}

fn ruby_method_is_static(symbol: &Symbol) -> bool {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("isStatic"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn is_type_scope(kind: &SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Class
            | SymbolKind::Interface
            | SymbolKind::Struct
            | SymbolKind::Trait
            | SymbolKind::Enum
            | SymbolKind::Module
    )
}

fn is_ecmascript_language(caller: Option<&Symbol>) -> bool {
    caller.is_some_and(|caller| is_ecmascript_language_name(&caller.language))
}

fn is_ecmascript_language_name(language: &str) -> bool {
    matches!(language, "javascript" | "typescript" | "jsx" | "tsx")
}

fn is_class_member(symbol: &Symbol, by_id: &HashMap<&str, &Symbol>) -> bool {
    let Some(parent_id) = symbol.parent_id.as_deref() else {
        return false;
    };
    by_id
        .get(parent_id)
        .copied()
        .is_some_and(|parent| matches!(parent.kind, SymbolKind::Class | SymbolKind::Interface))
}

fn binding_is_visible(symbol: &Symbol, call_site: Node<'_>) -> bool {
    if !is_ecmascript_language_name(&symbol.language) {
        return generic_block_binding_is_visible(symbol, call_site);
    }
    let Some(declaration) = root_node(call_site)
        .descendant_for_byte_range(symbol.start_byte as usize, symbol.end_byte as usize)
    else {
        return true;
    };
    let is_parameter = metadata_str(symbol, "role") == Some("parameter");
    let (block_scope, function_scope) = ecmascript_binding_scopes(declaration, is_parameter);
    if node_encloses(block_scope, call_site) {
        return true;
    }
    if node_encloses(function_scope, call_site)
        && (declaration.kind() == "variable_declaration"
            || matches!(
                declaration.kind(),
                "function_declaration" | "generator_function_declaration"
            ))
    {
        return true;
    }
    false
}

fn lua_local_placeholder_precedes_function(
    placeholder: &Symbol,
    function: &Symbol,
    call_site: Node<'_>,
) -> bool {
    if placeholder.language != "lua"
        || placeholder.kind != SymbolKind::Variable
        || function.language != "lua"
        || function.kind != SymbolKind::Function
        || placeholder.start_byte >= function.start_byte
        || placeholder.end_byte > call_site.start_byte() as u32
    {
        return false;
    }
    let root = root_node(call_site);
    let Some(placeholder_node) = root.descendant_for_byte_range(
        placeholder.start_byte as usize,
        placeholder.end_byte as usize,
    ) else {
        return false;
    };
    let Some(function_node) =
        root.descendant_for_byte_range(function.start_byte as usize, function.end_byte as usize)
    else {
        return false;
    };
    let Some(variable_declaration) = ancestor_including(placeholder_node, "variable_declaration")
    else {
        return false;
    };
    let Some(local_container) = variable_declaration.parent() else {
        return false;
    };
    if !has_child_field(local_container, variable_declaration, "local_declaration") {
        return false;
    }
    let Some(function_declaration) = ancestor_including(function_node, "function_declaration")
    else {
        return false;
    };
    let Some(name) = function_declaration.child_by_field_name("name") else {
        return false;
    };
    let Some(function_container) = function_declaration.parent() else {
        return false;
    };
    if name.kind() != "identifier"
        || !node_encloses(function_node, name)
        || has_child_field(
            function_container,
            function_declaration,
            "local_declaration",
        )
    {
        return false;
    }
    let Some(placeholder_block) = ancestor_including(local_container, "block")
        .or_else(|| ancestor_including(local_container, "chunk"))
    else {
        return false;
    };
    let Some(function_block) = ancestor_including(function_container, "block")
        .or_else(|| ancestor_including(function_container, "chunk"))
    else {
        return false;
    };
    placeholder_block.id() == function_block.id() && node_encloses(placeholder_block, call_site)
}

fn ancestor_including<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let mut current = Some(node);
    while let Some(node) = current {
        if node.kind() == kind {
            return Some(node);
        }
        current = node.parent();
    }
    None
}

fn has_child_field(parent: Node<'_>, child: Node<'_>, field: &str) -> bool {
    (0..parent.child_count()).any(|index| {
        parent.child(index as u32).is_some_and(|candidate| {
            candidate.id() == child.id() && parent.field_name_for_child(index as u32) == Some(field)
        })
    })
}

fn generic_block_binding_is_visible(symbol: &Symbol, call_site: Node<'_>) -> bool {
    if !uses_block_scopes(&symbol.language)
        || !matches!(
            symbol.kind,
            SymbolKind::Variable
                | SymbolKind::Constant
                | SymbolKind::Import
                | SymbolKind::Function
                | SymbolKind::Class
                | SymbolKind::Interface
                | SymbolKind::Struct
                | SymbolKind::Type
                | SymbolKind::Enum
                | SymbolKind::Namespace
                | SymbolKind::Module
        )
    {
        return true;
    }
    let Some(mut declaration) = root_node(call_site)
        .descendant_for_byte_range(symbol.start_byte as usize, symbol.end_byte as usize)
    else {
        return true;
    };
    if matches!(symbol.language.as_str(), "lua")
        && symbol.start_byte > call_site.start_byte() as u32
    {
        return false;
    }
    if matches!(
        symbol.kind,
        SymbolKind::Variable | SymbolKind::Constant | SymbolKind::Import
    ) && symbol.start_byte > call_site.start_byte() as u32
    {
        return false;
    }
    if metadata_str(symbol, "role") == Some("parameter") {
        let scope = enclosing_node(declaration, generic_function_scope);
        return node_encloses(scope, call_site);
    }
    if matches!(
        declaration.kind(),
        "function_definition"
            | "function_declaration"
            | "function_declaration_statement"
            | "function_item"
            | "method_declaration"
            | "class_declaration"
            | "struct_item"
            | "enum_item"
            | "type_declaration"
            | "interface_declaration"
    ) {
        declaration = declaration.parent().unwrap_or(declaration);
    }
    let scope = enclosing_node(declaration, is_generic_block_scope);
    node_encloses(scope, call_site)
}

fn uses_block_scopes(language: &str) -> bool {
    matches!(
        language,
        "c" | "cpp"
            | "csharp"
            | "dart"
            | "fsharp"
            | "gdscript"
            | "go"
            | "java"
            | "kotlin"
            | "lua"
            | "rust"
            | "scala"
            | "swift"
            | "vbnet"
            | "zig"
    )
}

fn is_generic_block_scope(kind: &str) -> bool {
    matches!(
        kind,
        "block"
            | "block_statement"
            | "block_stmt"
            | "compound_statement"
            | "statement_block"
            | "function_body"
            | "statements"
            | "switch_body"
            | "switch_block"
            | "case_block"
            | "for_statement"
            | "for_in_statement"
            | "for_expression"
            | "foreach_statement"
            | "catch_clause"
            | "catch_block"
            | "try_statement"
            | "if_statement"
            | "source_file"
            | "translation_unit"
            | "program"
            | "compilation_unit"
    )
}

fn generic_function_scope(kind: &str) -> bool {
    is_generic_block_scope(kind)
        || matches!(
            kind,
            "function_definition"
                | "function_declaration"
                | "function_declaration_statement"
                | "function_item"
                | "method_declaration"
                | "lambda_expression"
                | "arrow_function"
                | "function_expression"
                | "lambda"
        )
}

fn target_value_available(symbol: &Symbol, call_site: Node<'_>) -> bool {
    let Some(declaration) = root_node(call_site)
        .descendant_for_byte_range(symbol.start_byte as usize, symbol.end_byte as usize)
    else {
        return true;
    };
    if !is_ecmascript_language_name(&symbol.language)
        || matches!(
            declaration.kind(),
            "function_declaration" | "generator_function_declaration"
        )
    {
        return true;
    }
    symbol.start_byte <= call_site.start_byte() as u32
}

fn ecmascript_binding_scopes(declaration: Node<'_>, is_parameter: bool) -> (Node<'_>, Node<'_>) {
    if is_parameter {
        let function = enclosing_node(declaration, is_ecmascript_function_scope);
        return (function, function);
    }
    let binding = match declaration.kind() {
        "arrow_function" | "function_expression" | "generator_function" => declaration
            .parent()
            .filter(|parent| parent.kind() == "variable_declarator")
            .unwrap_or(declaration),
        _ => declaration,
    };
    let mut current = Some(binding);
    while let Some(node) = current {
        match node.kind() {
            "variable_declaration" => {
                let function = enclosing_node(node, is_ecmascript_function_scope);
                return (function, function);
            }
            "lexical_declaration" | "class_declaration" | "import_statement" => {
                let block = enclosing_node(node, is_ecmascript_block_scope);
                return (block, block);
            }
            "function_declaration" | "generator_function_declaration" => {
                let block = enclosing_node(node, is_ecmascript_block_scope);
                let function = enclosing_node(node, is_ecmascript_function_scope);
                return (block, function);
            }
            kind if is_ecmascript_block_scope(kind) => return (node, node),
            _ => current = node.parent(),
        }
    }
    (binding, binding)
}

fn is_ecmascript_function_scope(kind: &str) -> bool {
    matches!(
        kind,
        "program"
            | "function_declaration"
            | "generator_function_declaration"
            | "function_expression"
            | "generator_function"
            | "arrow_function"
            | "method_definition"
            | "class_static_block"
    )
}

fn is_ecmascript_block_scope(kind: &str) -> bool {
    matches!(
        kind,
        "statement_block" | "program" | "switch_body" | "for_statement" | "for_in_statement"
    )
}

fn enclosing_node<'tree>(node: Node<'tree>, keep: impl Fn(&str) -> bool) -> Node<'tree> {
    let mut current = node;
    while let Some(parent) = current.parent() {
        if keep(parent.kind()) {
            return parent;
        }
        current = parent;
    }
    current
}

fn node_encloses(scope: Node<'_>, node: Node<'_>) -> bool {
    scope.start_byte() <= node.start_byte() && node.end_byte() <= scope.end_byte()
}

fn root_node<'tree>(node: Node<'tree>) -> Node<'tree> {
    let mut root = node;
    while let Some(parent) = root.parent() {
        root = parent;
    }
    root
}

fn ecmascript_this_is_unbound(
    index: &ScopedSymbolIndex<'_>,
    call_site: Node<'_>,
    caller: Option<&Symbol>,
) -> bool {
    let nearest_function = {
        let mut current = call_site.parent();
        let mut nearest_function = None;
        while let Some(node) = current {
            if matches!(
                node.kind(),
                "function_declaration"
                    | "function_expression"
                    | "generator_function"
                    | "generator_function_declaration"
            ) {
                nearest_function = Some(node);
                break;
            }
            if matches!(node.kind(), "method_definition" | "program") {
                break;
            }
            current = node.parent();
        }
        nearest_function
    };

    let mut current_symbol = caller;
    let prototype_owner = loop {
        let Some(symbol) = current_symbol else {
            break None;
        };
        if symbol.kind == SymbolKind::Method
            && symbol
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("isPrototypeMethod"))
                .and_then(serde_json::Value::as_bool)
                == Some(true)
        {
            break Some((symbol, true));
        }
        if symbol.kind == SymbolKind::Function
            && index.prototype_owner_ids.contains(symbol.id.as_str())
        {
            break Some((symbol, false));
        }
        if symbol.kind != SymbolKind::Function
            || symbol
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("isArrowFunction"))
                .and_then(serde_json::Value::as_bool)
                != Some(true)
        {
            break None;
        }
        current_symbol = symbol
            .parent_id
            .as_deref()
            .and_then(|parent_id| index.by_id.get(parent_id).copied());
    };

    if let Some((owner, is_prototype_method)) = prototype_owner {
        if !is_prototype_method {
            let owner_node = root_node(call_site)
                .descendant_for_byte_range(owner.start_byte as usize, owner.end_byte as usize);
            return !owner_node.is_some_and(|node| {
                matches!(
                    node.kind(),
                    "function_declaration"
                        | "function_expression"
                        | "generator_function"
                        | "generator_function_declaration"
                ) && nearest_function.is_some_and(|nearest| nearest.id() == node.id())
                    && node
                        .child_by_field_name("body")
                        .is_some_and(|body| node_encloses(body, call_site))
            });
        }

        let assignment = root_node(call_site)
            .descendant_for_byte_range(owner.start_byte as usize, owner.end_byte as usize)
            .and_then(|node| ancestor_including(node, "assignment_expression"));
        let Some(assignment) = assignment.filter(|assignment| {
            assignment.start_byte() == owner.start_byte as usize
                && assignment.end_byte() == owner.end_byte as usize
        }) else {
            return true;
        };
        let Some(function) = assignment.child_by_field_name("right") else {
            return true;
        };
        if !matches!(
            function.kind(),
            "function_expression" | "generator_function" | "function"
        ) || nearest_function.is_none_or(|nearest| nearest.id() != function.id())
            || !function
                .child_by_field_name("body")
                .is_some_and(|body| node_encloses(body, call_site))
        {
            return true;
        }
        return false;
    }

    nearest_function.is_some()
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
