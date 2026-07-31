mod docs;
mod headers;
mod parse_errors;
mod symbols;
mod visibility;

#[cfg(test)]
pub(crate) mod support {
    use crate::base::{Symbol, SymbolKind};
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
