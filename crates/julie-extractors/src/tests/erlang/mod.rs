mod complexity;
mod docs;
mod headers;
mod identifiers;
mod literals;
mod parse_errors;
mod relationships;
mod structural_facts;
mod symbols;
mod test_roles;
mod type_facts;
mod types;
mod visibility;

#[cfg(test)]
pub(crate) mod support {
    use crate::base::{
        Identifier, Relationship, StructuredPendingRelationship, Symbol, SymbolKind,
    };
    use crate::erlang::ErlangExtractor;
    use std::path::PathBuf;
    use tree_sitter::{Parser, Tree};

    pub(crate) fn parse(code: &str) -> Tree {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_erlang::LANGUAGE.into())
            .expect("Error loading Erlang grammar");
        parser.parse(code, None).expect("parse returned no tree")
    }

    pub(crate) fn extract_from(file_path: &str, code: &str) -> Vec<Symbol> {
        let tree = parse(code);
        let mut extractor = ErlangExtractor::new(
            "erlang".to_string(),
            file_path.to_string(),
            code.to_string(),
            &PathBuf::from("/tmp/test"),
        );
        extractor.extract_symbols(&tree)
    }

    pub(crate) fn extract(code: &str) -> Vec<Symbol> {
        extract_from("bank.erl", code)
    }

    pub(crate) fn find<'a>(symbols: &'a [Symbol], name: &str) -> &'a Symbol {
        symbols
            .iter()
            .find(|symbol| symbol.name == name)
            .unwrap_or_else(|| panic!("no symbol named {name}; got {}", inventory(symbols)))
    }

    pub(crate) fn find_kind<'a>(symbols: &'a [Symbol], name: &str, kind: SymbolKind) -> &'a Symbol {
        symbols
            .iter()
            .find(|symbol| symbol.name == name && symbol.kind == kind)
            .unwrap_or_else(|| panic!("no {kind:?} named {name}; got {}", inventory(symbols)))
    }

    pub(crate) fn extract_with_types(
        code: &str,
    ) -> (Vec<Symbol>, std::collections::HashMap<String, String>) {
        let tree = parse(code);
        let mut extractor = ErlangExtractor::new(
            "erlang".to_string(),
            "bank.erl".to_string(),
            code.to_string(),
            &PathBuf::from("/tmp/test"),
        );
        let symbols = extractor.extract_symbols(&tree);
        let types = extractor.infer_types(&symbols);
        (symbols, types)
    }

    pub(crate) fn extract_with_identifiers(code: &str) -> (Vec<Symbol>, Vec<Identifier>) {
        let tree = parse(code);
        let mut extractor = ErlangExtractor::new(
            "erlang".to_string(),
            "bank.erl".to_string(),
            code.to_string(),
            &PathBuf::from("/tmp/test"),
        );
        let symbols = extractor.extract_symbols(&tree);
        let identifiers = extractor.extract_identifiers(&tree, &symbols);
        (symbols, identifiers)
    }

    pub(crate) fn named<'a>(identifiers: &'a [Identifier], name: &str) -> Vec<&'a Identifier> {
        identifiers
            .iter()
            .filter(|identifier| identifier.name == name)
            .collect()
    }

    pub(crate) fn only<'a>(identifiers: &'a [Identifier], name: &str) -> &'a Identifier {
        let matches = named(identifiers, name);
        assert_eq!(
            matches.len(),
            1,
            "expected exactly one identifier named {name}; got {}",
            identifier_inventory(identifiers)
        );
        matches[0]
    }

    pub(crate) fn extract_with_relationships(
        code: &str,
    ) -> (
        Vec<Symbol>,
        Vec<Relationship>,
        Vec<StructuredPendingRelationship>,
    ) {
        extract_from_with_relationships("bank.erl", code)
    }

    pub(crate) fn extract_from_with_relationships(
        file_path: &str,
        code: &str,
    ) -> (
        Vec<Symbol>,
        Vec<Relationship>,
        Vec<StructuredPendingRelationship>,
    ) {
        let tree = parse(code);
        let mut extractor = ErlangExtractor::new(
            "erlang".to_string(),
            file_path.to_string(),
            code.to_string(),
            &PathBuf::from("/tmp/test"),
        );
        let symbols = extractor.extract_symbols(&tree);
        let relationships = extractor.extract_relationships(&tree, &symbols);
        let pending = extractor.get_structured_pending_relationships();
        (symbols, relationships, pending)
    }

    pub(crate) fn pending_named<'a>(
        pending: &'a [StructuredPendingRelationship],
        terminal_name: &str,
    ) -> &'a StructuredPendingRelationship {
        let matches: Vec<_> = pending
            .iter()
            .filter(|edge| edge.target.terminal_name == terminal_name)
            .collect();
        assert_eq!(
            matches.len(),
            1,
            "expected exactly one pending edge targeting {terminal_name}; got {}",
            pending_inventory(pending)
        );
        matches[0]
    }

    pub(crate) fn pending_inventory(pending: &[StructuredPendingRelationship]) -> String {
        format!(
            "{:?}",
            pending
                .iter()
                .map(|edge| (
                    edge.target.display_name.as_str(),
                    edge.pending.kind.to_string()
                ))
                .collect::<Vec<_>>()
        )
    }

    pub(crate) fn relationship_inventory(relationships: &[Relationship]) -> String {
        format!(
            "{:?}",
            relationships
                .iter()
                .map(|edge| (edge.kind.to_string(), edge.line_number))
                .collect::<Vec<_>>()
        )
    }

    pub(crate) fn identifier_inventory(identifiers: &[Identifier]) -> String {
        format!(
            "{:?}",
            identifiers
                .iter()
                .map(|identifier| (identifier.name.as_str(), identifier.kind.clone()))
                .collect::<Vec<_>>()
        )
    }

    fn inventory(symbols: &[Symbol]) -> String {
        format!(
            "{:?}",
            symbols
                .iter()
                .map(|symbol| (symbol.name.as_str(), symbol.kind.clone()))
                .collect::<Vec<_>>()
        )
    }
}
