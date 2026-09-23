// Tests for SQL type extraction through the factory

#[cfg(test)]
mod tests {
    use crate::factory::extract_symbols_and_relationships;
    use std::path::PathBuf;
    use tree_sitter::Parser;

    #[test]
    fn test_factory_extracts_sql_types() {
        let code = r#"
CREATE TABLE users (
    id INT PRIMARY KEY,
    name VARCHAR(100) NOT NULL,
    email VARCHAR(255) UNIQUE,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE FUNCTION get_user_count() RETURNS INTEGER AS $$
BEGIN
    RETURN (SELECT COUNT(*) FROM users);
END;
$$ LANGUAGE plpgsql;
"#;

        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_sequel::LANGUAGE.into())
            .expect("Error loading SQL grammar");
        let tree = parser.parse(code, None).expect("Error parsing code");

        let workspace_root = PathBuf::from("/tmp/test");
        let results =
            extract_symbols_and_relationships(&tree, "test.sql", code, "sql", &workspace_root)
                .expect("Extraction failed");

        let type_of = |name: &str| {
            let symbol = results
                .symbols
                .iter()
                .find(|symbol| symbol.name == name)
                .unwrap_or_else(|| panic!("missing symbol {name}"));
            let info = &results.types[&symbol.id];
            assert_eq!(info.language, "sql");
            (info.resolved_type.as_str(), info.is_inferred)
        };
        assert_eq!(type_of("id"), ("INT", false));
        assert_eq!(type_of("name"), ("VARCHAR", false));
        assert_eq!(type_of("created_at"), ("TIMESTAMP", false));
        assert_eq!(type_of("get_user_count"), ("INTEGER", false));
        assert_eq!(type_of("users"), ("TABLE", true));
    }
}
