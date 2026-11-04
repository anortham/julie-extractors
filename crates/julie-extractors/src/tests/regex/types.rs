#[cfg(test)]
mod tests {
    use crate::factory::extract_symbols_and_relationships;
    use crate::pipeline::extract_canonical;
    use std::path::Path;
    use std::path::PathBuf;
    use tree_sitter::Parser;

    #[test]
    fn test_factory_extracts_regex_types() {
        let code = r#"
/^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$/
/^\d{3}-\d{2}-\d{4}$/
/^(https?:\/\/)?([\da-z\.-]+)\.([a-z\.]{2,6})([\/\w \.-]*)*\/?$/
"#;

        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_regex::LANGUAGE.into())
            .expect("Error loading Regex grammar");
        let tree = parser.parse(code, None).expect("Error parsing code");

        let workspace_root = PathBuf::from("/tmp/test");
        let results =
            extract_symbols_and_relationships(&tree, "test.regex", code, "regex", &workspace_root)
                .expect("Extraction failed");

        assert!(
            !results.types.is_empty(),
            "Regex type extraction returned EMPTY types HashMap!"
        );

        println!("Extracted {} types from Regex code", results.types.len());
        for (symbol_id, type_info) in &results.types {
            println!(
                "  {} -> {} (inferred: {})",
                symbol_id, type_info.resolved_type, type_info.is_inferred
            );
        }

        assert!(!results.types.is_empty());
        for type_info in results.types.values() {
            assert_eq!(type_info.language, "regex");
            assert!(type_info.is_inferred);
        }
    }

    #[test]
    fn regex_types_follow_semantic_symbol_categories_without_construct_rows() {
        let source = "(?<name>[A-Z]+)(?=x)\\p{Lu}\n";
        let results =
            extract_canonical("rules.regex", source, Path::new("/repo")).expect("regex extraction");

        assert!(results.parse_diagnostics.is_empty());
        let pattern = results
            .symbols
            .iter()
            .find(|symbol| symbol.kind == crate::base::SymbolKind::Variable)
            .expect("pattern symbol");
        let named_group = results
            .symbols
            .iter()
            .find(|symbol| symbol.name == "name")
            .expect("named capture symbol");
        assert_eq!(named_group.parent_id.as_deref(), Some(pattern.id.as_str()));

        let expected = [
            (pattern, "regex:regex-pattern"),
            (named_group, "regex:group"),
            (
                results
                    .symbols
                    .iter()
                    .find(|symbol| symbol.kind == crate::base::SymbolKind::Class)
                    .expect("character class symbol"),
                "regex:character-class",
            ),
            (
                results
                    .symbols
                    .iter()
                    .find(|symbol| symbol.kind == crate::base::SymbolKind::Method)
                    .expect("lookaround symbol"),
                "regex:lookaround",
            ),
            (
                results
                    .symbols
                    .iter()
                    .find(|symbol| symbol.kind == crate::base::SymbolKind::Constant)
                    .expect("Unicode property symbol"),
                "regex:unicode-property",
            ),
        ];

        assert_eq!(results.symbols.len(), expected.len());
        assert_eq!(results.types.len(), expected.len());
        for (symbol, resolved_type) in expected {
            let type_info = results.types.get(&symbol.id).expect("symbol type");
            assert_eq!(type_info.resolved_type, resolved_type);
            assert_eq!(type_info.language, "regex");
            assert!(type_info.is_inferred);
        }
        assert!(
            results
                .types
                .keys()
                .all(|symbol_id| results.symbols.iter().any(|symbol| &symbol.id == symbol_id))
        );
        assert!(
            results
                .symbols
                .iter()
                .all(|symbol| { !["+", "x", "Lu"].contains(&symbol.name.as_str()) })
        );
    }
}
