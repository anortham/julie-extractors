// Symbol, Identifier, Relationship, and Visibility creation methods
//
// Extracted from extractor.rs to keep modules under 500 lines

use std::collections::HashMap;
use tree_sitter::Node;

use super::body::{body_hash, infer_body_span};
use super::extractor::BaseExtractor;
use super::relationship_resolution::{StructuredPendingRelationship, UnresolvedTarget};
use super::span::NormalizedSpan;
use super::types::IdentifierKind;
use super::types::{
    Identifier, Relationship, RelationshipKind, Symbol, SymbolKind, SymbolOptions, TypeInfo,
    TypeNameRules, Visibility, strip_type_decorations,
};

impl BaseExtractor {
    /// Create a symbol - exact port of createSymbol method
    pub fn create_symbol(
        &mut self,
        node: &Node,
        name: String,
        kind: SymbolKind,
        options: SymbolOptions,
    ) -> Symbol {
        self.create_symbol_from_span(node, NormalizedSpan::from_node(node), name, kind, options)
    }

    pub(crate) fn create_symbol_from_span(
        &mut self,
        node: &Node,
        span: NormalizedSpan,
        name: String,
        kind: SymbolKind,
        options: SymbolOptions,
    ) -> Symbol {
        let id = self.generate_id_for_span(&name, &span);
        let body_span = infer_body_span(node, &self.content, self.line_starts(), span);
        let body_hash = body_span.and_then(|span| body_hash(&self.content, span, &self.language));

        // Mark markdown symbols as documentation
        let content_type = if self.language == "markdown" {
            Some("documentation".to_string())
        } else {
            None
        };

        Symbol {
            id,
            name,
            kind,
            language: self.language.clone(),
            file_path: self.file_path.clone(),
            start_line: span.start_line,
            start_column: span.start_column,
            end_line: span.end_line,
            end_column: span.end_column,
            start_byte: span.start_byte,
            end_byte: span.end_byte,
            body_span,
            body_hash,
            signature: options.signature,
            doc_comment: options.doc_comment.or_else(|| self.find_doc_comment(node)),
            visibility: options.visibility,
            parent_id: options.parent_id,
            metadata: Some(options.metadata.unwrap_or_default()),
            annotations: options.annotations,
            semantic_group: None, // Will be populated during cross-language analysis
            confidence: None,     // Will be calculated based on parsing context
            content_type,
        }
    }

    /// Create an identifier (reference/usage) - NEW for LSP-quality reference tracking
    ///
    /// Unlike symbols (definitions), identifiers represent usage sites.
    /// They are stored unresolved (target_symbol_id = None) and resolved on-demand
    /// during queries for optimal incremental update performance.
    pub fn create_identifier(
        &mut self,
        node: &Node,
        name: String,
        kind: IdentifierKind,
        containing_symbol_id: Option<String>,
    ) -> Identifier {
        self.create_identifier_with_receiver_type(node, name, kind, containing_symbol_id, None)
    }

    /// Create an identifier that carries a `receiver_type`: the enclosing type
    /// name recorded when the call's receiver is the language's self reference
    /// (`this`/`base`).
    pub fn create_identifier_with_receiver_type(
        &mut self,
        node: &Node,
        name: String,
        kind: IdentifierKind,
        containing_symbol_id: Option<String>,
        receiver_type: Option<String>,
    ) -> Identifier {
        let span = NormalizedSpan::from_node(node);

        // Generate unique ID for this identifier
        let id = self.generate_id_for_span(&name, &span);

        let identifier = Identifier {
            id,
            name,
            kind,
            language: self.language.clone(),
            file_path: self.file_path.clone(),
            start_line: span.start_line,
            start_column: span.start_column,
            end_line: span.end_line,
            end_column: span.end_column,
            start_byte: span.start_byte,
            end_byte: span.end_byte,
            containing_symbol_id,
            target_symbol_id: None, // Unresolved - will be resolved on-demand in C#
            confidence: 1.0,        // Default high confidence for tree-sitter extractions
            receiver_type,
            code_context: None,
        };

        self.identifiers.push(identifier.clone());
        identifier
    }

    /// Create a relationship - exact port of createRelationship
    pub fn create_relationship(
        &self,
        from_symbol_id: String,
        to_symbol_id: String,
        kind: RelationshipKind,
        node: &Node,
        confidence: Option<f32>,
        metadata: Option<HashMap<String, serde_json::Value>>,
    ) -> Relationship {
        Relationship {
            id: format!(
                "{}_{}_{:?}_{}_{}_{}_{}",
                from_symbol_id,
                to_symbol_id,
                kind,
                node.start_position().row,
                node.start_position().column,
                node.start_byte(),
                node.end_byte()
            ),
            from_symbol_id,
            to_symbol_id,
            kind,
            file_path: self.file_path.clone(),
            line_number: (node.start_position().row + 1) as u32, // 1-based standard format
            span: Some(NormalizedSpan::from_node(node)),
            reference_site_is_exact: false,
            confidence: confidence.unwrap_or(1.0),
            metadata,
        }
    }

    pub fn create_relationship_at_target(
        &self,
        from_symbol_id: String,
        to_symbol_id: String,
        kind: RelationshipKind,
        target_token_node: &Node,
        confidence: Option<f32>,
        metadata: Option<HashMap<String, serde_json::Value>>,
    ) -> Relationship {
        let mut relationship = self.create_relationship(
            from_symbol_id,
            to_symbol_id,
            kind,
            target_token_node,
            confidence,
            metadata,
        );
        relationship.reference_site_is_exact = true;
        relationship
    }

    pub fn create_pending_relationship(
        &self,
        from_symbol_id: String,
        target: UnresolvedTarget,
        kind: RelationshipKind,
        node: &Node,
        caller_scope_symbol_id: Option<String>,
        confidence: Option<f32>,
    ) -> StructuredPendingRelationship {
        StructuredPendingRelationship::new(
            from_symbol_id,
            target,
            caller_scope_symbol_id,
            kind,
            self.file_path.clone(),
            node.start_position().row as u32 + 1,
            confidence.unwrap_or(1.0),
        )
        .with_context_span(NormalizedSpan::from_node(node))
    }

    pub fn create_pending_relationship_at_target(
        &self,
        from_symbol_id: String,
        target: UnresolvedTarget,
        kind: RelationshipKind,
        target_token_node: &Node,
        caller_scope_symbol_id: Option<String>,
        confidence: Option<f32>,
    ) -> StructuredPendingRelationship {
        StructuredPendingRelationship::new(
            from_symbol_id,
            target,
            caller_scope_symbol_id,
            kind,
            self.file_path.clone(),
            target_token_node.start_position().row as u32 + 1,
            confidence.unwrap_or(1.0),
        )
        .with_target_span(NormalizedSpan::from_node(target_token_node))
    }

    /// Find containing symbol - exact port of findContainingSymbol
    pub fn find_containing_symbol<'a>(
        &self,
        node: &Node,
        symbols: &'a [Symbol],
    ) -> Option<&'a Symbol> {
        Self::find_containing_symbol_from_iter(node, symbols.iter())
    }

    pub fn find_containing_symbol_from_map<'a>(
        &self,
        node: &Node,
        symbols_by_id: &HashMap<String, &'a Symbol>,
    ) -> Option<&'a Symbol> {
        self.find_containing_symbol_from_map_filtered(node, symbols_by_id, |_| true)
    }

    pub fn find_containing_symbol_from_map_filtered<'a>(
        &self,
        node: &Node,
        symbols_by_id: &HashMap<String, &'a Symbol>,
        include_symbol: impl Fn(&Symbol) -> bool,
    ) -> Option<&'a Symbol> {
        Self::find_containing_symbol_from_iter(
            node,
            symbols_by_id
                .values()
                .copied()
                .filter(|symbol| symbol.file_path == self.file_path && include_symbol(symbol)),
        )
    }

    fn find_containing_symbol_from_iter<'a>(
        node: &Node,
        symbols: impl IntoIterator<Item = &'a Symbol>,
    ) -> Option<&'a Symbol> {
        let position = node.start_position();

        // Find symbols that contain this position
        let mut containing_symbols: Vec<&Symbol> = symbols
            .into_iter()
            .filter(|s| {
                let pos_line = (position.row + 1) as u32;
                let pos_column = position.column as u32;

                let line_contains = s.start_line <= pos_line && s.end_line >= pos_line;

                // For column containment, handle multi-line spans exactly standard format
                let col_contains = if pos_line == s.start_line && pos_line == s.end_line {
                    // Single line span
                    s.start_column <= pos_column && s.end_column >= pos_column
                } else if pos_line == s.start_line {
                    // First line of multi-line span
                    s.start_column <= pos_column
                } else if pos_line == s.end_line {
                    // Last line of multi-line span
                    s.end_column >= pos_column
                } else {
                    // Middle line of multi-line span
                    true
                };

                line_contains && col_contains
            })
            .collect();

        if containing_symbols.is_empty() {
            return None;
        }

        // Priority order - reference implementation
        let get_priority = |kind: &SymbolKind| -> u32 {
            match kind {
                SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor => 1,
                SymbolKind::Class | SymbolKind::Interface => 2,
                SymbolKind::Namespace => 3,
                SymbolKind::Variable | SymbolKind::Constant | SymbolKind::Property => 10,
                _ => 5,
            }
        };

        containing_symbols.sort_by(|a, b| {
            // First, sort by priority (functions first)
            let priority_a = get_priority(&a.kind);
            let priority_b = get_priority(&b.kind);
            if priority_a != priority_b {
                return priority_a.cmp(&priority_b);
            }

            // Then by size (smaller first) — use byte range for accurate, overflow-safe comparison.
            // The old formula `(end_line - start_line) * 1000 + (end_column - start_column)` panicked
            // on multi-line symbols where end_column < start_column (columns refer to different lines).
            let size_a = a.end_byte - a.start_byte;
            let size_b = b.end_byte - b.start_byte;
            // start_byte and id complete the total order. Callers feed this from a
            // HashMap (identifier passes) or a Vec (relationship passes), and a tie
            // resolved by input order makes the two passes disagree about the same
            // token's containing symbol — which is exactly what equal-span symbols
            // such as C multi-declarator variables produce.
            size_a
                .cmp(&size_b)
                .then_with(|| a.start_byte.cmp(&b.start_byte))
                .then_with(|| a.id.cmp(&b.id))
        });

        Some(containing_symbols[0])
    }

    /// Extract visibility from explicit modifier nodes only.
    pub fn extract_visibility(&self, node: &Node) -> Option<Visibility> {
        // Look for visibility modifiers in child nodes
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                match child.kind() {
                    "public" => return Some(Visibility::Public),
                    "private" => return Some(Visibility::Private),
                    "protected" => return Some(Visibility::Protected),
                    _ => continue,
                }
            }
        }

        None
    }

    /// Record a declared-type fact for a symbol: `resolved_type` is the base
    /// type name per [`strip_type_decorations`], and the full declared text
    /// lands in `metadata["declared"]` when it differs. An existing row for
    /// the symbol wins; recorded rows in turn win over legacy inferred maps.
    pub fn record_declared_type_fact(
        &mut self,
        symbol_id: &str,
        declared_text: &str,
        rules: &TypeNameRules,
        is_inferred: bool,
    ) {
        self.record_declared_type_fact_with_declared(
            symbol_id,
            declared_text,
            declared_text,
            rules,
            is_inferred,
        );
    }

    /// Record a declared-type fact when the language already reduced the type
    /// node to a structural base name. `resolved_type` comes from
    /// [`strip_type_decorations`] on `base_text`. `declared_text` lands in
    /// `metadata["declared"]` when it differs from `resolved_type`. An
    /// existing row for the symbol wins; empty results record nothing.
    pub fn record_declared_type_fact_with_declared(
        &mut self,
        symbol_id: &str,
        base_text: &str,
        declared_text: &str,
        rules: &TypeNameRules,
        is_inferred: bool,
    ) {
        if self.type_info.contains_key(symbol_id) {
            return;
        }

        let declared = declared_text.trim();
        let resolved_type = strip_type_decorations(base_text, rules);
        if resolved_type.is_empty() {
            return;
        }

        let metadata = (resolved_type != declared).then(|| {
            HashMap::from([(
                "declared".to_string(),
                serde_json::Value::String(declared.to_string()),
            )])
        });

        self.type_info.insert(
            symbol_id.to_string(),
            TypeInfo {
                symbol_id: symbol_id.to_string(),
                resolved_type,
                generic_params: None,
                constraints: None,
                is_inferred,
                language: self.language.clone(),
                metadata,
            },
        );
    }
}

#[cfg(test)]
mod record_declared_type_fact_tests {
    use super::super::types::TypeNameRules;
    use crate::base::BaseExtractor;
    use std::path::Path;

    const RULES: TypeNameRules = TypeNameRules {
        nullable_suffixes: &["?"],
        reference_prefixes: &["ref"],
        generic_open: &['<'],
    };

    fn base() -> BaseExtractor {
        BaseExtractor::new(
            "csharp".to_string(),
            "/repo/src/App.cs".to_string(),
            "class App {}".to_string(),
            Path::new("/repo"),
        )
    }

    #[test]
    fn record_declared_type_fact_stores_base_name_and_declared_metadata() {
        let mut base = base();

        base.record_declared_type_fact("sym-1", "List<int>", &RULES, false);

        let fact = &base.type_info["sym-1"];
        assert_eq!(fact.symbol_id, "sym-1");
        assert_eq!(fact.resolved_type, "List");
        assert_eq!(fact.language, "csharp");
        assert!(!fact.is_inferred);
        assert_eq!(
            fact.metadata.as_ref().and_then(|m| m.get("declared")),
            Some(&serde_json::Value::String("List<int>".to_string()))
        );
    }

    #[test]
    fn record_declared_type_fact_omits_declared_metadata_for_undecorated_names() {
        let mut base = base();

        base.record_declared_type_fact("sym-1", "GraphTraversal", &RULES, true);

        let fact = &base.type_info["sym-1"];
        assert_eq!(fact.resolved_type, "GraphTraversal");
        assert!(fact.is_inferred);
        assert_eq!(fact.metadata, None);
    }

    #[test]
    fn record_declared_type_fact_keeps_the_first_row_for_a_symbol() {
        let mut base = base();

        base.record_declared_type_fact("sym-1", "GraphTraversal", &RULES, false);
        base.record_declared_type_fact("sym-1", "List<int>", &RULES, true);

        let fact = &base.type_info["sym-1"];
        assert_eq!(fact.resolved_type, "GraphTraversal");
        assert!(!fact.is_inferred);
    }

    #[test]
    fn record_declared_type_fact_skips_text_that_normalizes_to_nothing() {
        let mut base = base();

        base.record_declared_type_fact("sym-1", "<int>", &RULES, false);

        assert!(base.type_info.is_empty());
    }
}

#[cfg(test)]
mod record_declared_type_fact_with_declared_tests {
    use super::super::types::TypeNameRules;
    use crate::base::BaseExtractor;
    use std::path::Path;

    const RULES: TypeNameRules = TypeNameRules {
        nullable_suffixes: &[],
        reference_prefixes: &[],
        generic_open: &[],
    };

    fn base() -> BaseExtractor {
        BaseExtractor::new(
            "csharp".to_string(),
            "/repo/src/App.cs".to_string(),
            "class App {}".to_string(),
            Path::new("/repo"),
        )
    }

    #[test]
    fn record_declared_type_fact_with_declared_uses_base_text_for_resolved_type() {
        let cases: &[(&str, &str, &str, Option<&str>)] = &[
            ("foo", "struct foo *", "foo", Some("struct foo *")),
            ("list", "int list", "list", Some("int list")),
            ("Foo", "Foo", "Foo", None),
        ];

        for (i, (base_text, declared_text, resolved, declared_meta)) in cases.iter().enumerate() {
            let mut extractor = base();
            let symbol_id = format!("sym-{i}");
            extractor.record_declared_type_fact_with_declared(
                &symbol_id,
                base_text,
                declared_text,
                &RULES,
                false,
            );
            let fact = &extractor.type_info[&symbol_id];
            assert_eq!(fact.resolved_type, *resolved);
            assert_eq!(
                fact.metadata.as_ref().and_then(|m| m.get("declared")),
                declared_meta
                    .map(|text| serde_json::Value::String(text.to_string()))
                    .as_ref()
            );
        }
    }

    #[test]
    fn record_declared_type_fact_with_declared_keeps_the_first_row_for_a_symbol() {
        let mut extractor = base();
        extractor.record_declared_type_fact_with_declared(
            "sym-1",
            "foo",
            "struct foo *",
            &RULES,
            false,
        );
        extractor
            .record_declared_type_fact_with_declared("sym-1", "list", "int list", &RULES, true);

        let fact = &extractor.type_info["sym-1"];
        assert_eq!(fact.resolved_type, "foo");
        assert!(!fact.is_inferred);
        assert_eq!(
            fact.metadata.as_ref().and_then(|m| m.get("declared")),
            Some(&serde_json::Value::String("struct foo *".to_string()))
        );
    }

    #[test]
    fn record_declared_type_fact_with_declared_skips_empty_base() {
        let mut extractor = base();
        extractor.record_declared_type_fact_with_declared(
            "sym-1",
            "",
            "struct foo *",
            &RULES,
            false,
        );
        extractor.record_declared_type_fact_with_declared(
            "sym-1",
            "   ",
            "struct foo *",
            &RULES,
            false,
        );

        assert!(extractor.type_info.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tree_sitter::{Parser, Tree};

    const MULTI_DECLARATOR: &str = "long alpha = 1, beta = 2, gamma = ticks();\n";

    fn parse_c(source: &str) -> Tree {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_c::LANGUAGE.into())
            .expect("C grammar loads");
        parser.parse(source, None).expect("C source parses")
    }

    fn first_node_of_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
        if node.kind() == kind {
            return Some(node);
        }
        let mut cursor = node.walk();
        node.children(&mut cursor)
            .find_map(|child| first_node_of_kind(child, kind))
    }

    #[test]
    fn equal_span_containment_candidates_resolve_independently_of_input_order() {
        let tree = parse_c(MULTI_DECLARATOR);
        let declaration =
            first_node_of_kind(tree.root_node(), "declaration").expect("declaration node");
        let call = first_node_of_kind(declaration, "call_expression").expect("call node");

        let mut base = BaseExtractor::new(
            "c".to_string(),
            "/repo/a.c".to_string(),
            MULTI_DECLARATOR.to_string(),
            Path::new("/repo"),
        );
        let declarators: Vec<Symbol> = ["alpha", "beta", "gamma"]
            .into_iter()
            .map(|name| {
                base.create_symbol(
                    &declaration,
                    name.to_string(),
                    SymbolKind::Variable,
                    SymbolOptions::default(),
                )
            })
            .collect();

        let mut winners = std::collections::BTreeSet::new();
        for order in [
            [0, 1, 2],
            [0, 2, 1],
            [1, 0, 2],
            [1, 2, 0],
            [2, 0, 1],
            [2, 1, 0],
        ] {
            let shuffled: Vec<Symbol> = order
                .iter()
                .map(|index| declarators[*index].clone())
                .collect();
            let winner = base
                .find_containing_symbol(&call, &shuffled)
                .expect("an equal-span declarator contains the call");
            winners.insert(winner.name.clone());
        }

        assert_eq!(
            winners.len(),
            1,
            "input order changed the containment winner: {winners:?}"
        );
    }
}
