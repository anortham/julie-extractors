mod cardinality;
mod identifiers;
mod parse_errors;
mod routing;
mod structural_facts;
mod symbols;

#[cfg(test)]
pub(crate) mod support {
    use crate::base::{Identifier, Symbol};
    use crate::xml::XmlExtractor;
    use std::path::PathBuf;
    use tree_sitter::{Parser, Tree};

    pub(crate) fn parse(code: &str) -> Tree {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_xml::LANGUAGE_XML.into())
            .expect("Error loading XML grammar");
        parser.parse(code, None).expect("parse returned no tree")
    }

    fn extractor(file_path: &str, code: &str) -> XmlExtractor {
        XmlExtractor::new(
            "xml".to_string(),
            file_path.to_string(),
            code.to_string(),
            &PathBuf::from("/tmp/test"),
        )
    }

    pub(crate) fn extract_from(file_path: &str, code: &str) -> Vec<Symbol> {
        let tree = parse(code);
        extractor(file_path, code).extract_symbols(&tree)
    }

    pub(crate) fn extract(code: &str) -> Vec<Symbol> {
        extract_from("schema.xml", code)
    }

    pub(crate) fn extract_identifiers(code: &str) -> (Vec<Symbol>, Vec<Identifier>) {
        let tree = parse(code);
        let mut extractor = extractor("schema.xml", code);
        let symbols = extractor.extract_symbols(&tree);
        let identifiers = extractor.extract_identifiers(&tree, &symbols);
        (symbols, identifiers)
    }

    pub(crate) fn names(symbols: &[Symbol]) -> Vec<&str> {
        symbols.iter().map(|symbol| symbol.name.as_str()).collect()
    }

    pub(crate) fn find<'a>(symbols: &'a [Symbol], name: &str) -> &'a Symbol {
        symbols
            .iter()
            .find(|symbol| symbol.name == name)
            .unwrap_or_else(|| panic!("no symbol named {name}; got {:?}", names(symbols)))
    }
}
