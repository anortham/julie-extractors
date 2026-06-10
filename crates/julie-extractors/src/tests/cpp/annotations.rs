use super::extract_symbols;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_standard_attributes_from_function_definition() {
        let code = r#"
            [[nodiscard, maybe_unused]]
            int stable_value() {
                return 1;
            }
        "#;

        let symbols = extract_symbols(code);
        let func = symbols
            .iter()
            .find(|s| s.name == "stable_value")
            .expect("Function not found");

        let annotation_keys: Vec<_> = func
            .annotations
            .iter()
            .map(|annotation| annotation.annotation_key.as_str())
            .collect();

        assert_eq!(annotation_keys, vec!["nodiscard", "maybe_unused"]);
    }

    #[test]
    fn test_attributes_do_not_bleed_to_following_function() {
        let code = r#"
            [[nodiscard]]
            int stable_value() {
                return 1;
            }

            int plain_value() {
                return 2;
            }
        "#;

        let symbols = extract_symbols(code);
        let plain = symbols
            .iter()
            .find(|s| s.name == "plain_value")
            .expect("Function not found");

        assert!(
            plain.annotations.is_empty(),
            "following function should not inherit previous attributes"
        );
    }
}
