// JavaScript Extractor Tests
//
// Direct Implementation of JavaScript extractor tests (TDD RED phase)

// Submodule declarations
pub mod cross_file_pending;
pub mod cross_file_relationships;
pub mod error_handling;
pub mod identifier_extraction;
pub mod jsdoc_comments;
pub mod jsx_complexity;
pub mod jsx_cross_file_pending;
pub mod legacy_patterns;
pub mod literals;
pub mod modern_features;
pub mod relationships;
pub mod scoping;
pub mod types;

#[cfg(test)]
mod traversal_depth {
    use crate::pipeline::extract_canonical;
    use crate::tree_traversal::TREE_TRAVERSAL_DEPTH_LIMIT;
    use std::path::Path;

    #[test]
    fn javascript_symbol_extraction_stops_at_traversal_depth_budget() {
        std::thread::Builder::new()
            .name("deep-js-traversal-budget".to_string())
            .stack_size(64 * 1024 * 1024)
            .spawn(|| {
                let mut source = String::from("function main() {\n");
                for _ in 0..(TREE_TRAVERSAL_DEPTH_LIMIT + 16) {
                    source.push_str("{\n");
                }
                source.push_str("function tooDeep() {}\n");
                for _ in 0..(TREE_TRAVERSAL_DEPTH_LIMIT + 16) {
                    source.push_str("}\n");
                }
                source.push_str("}\n");

                let results = extract_canonical("src/deep.js", &source, Path::new("/repo"))
                    .expect("deep JavaScript source should parse and extract");

                assert!(
                    results.symbols.iter().any(|symbol| symbol.name == "main"),
                    "shallow function should still be extracted"
                );
                assert!(
                    !results
                        .symbols
                        .iter()
                        .any(|symbol| symbol.name == "tooDeep"),
                    "symbol walker should not visit function_declaration nodes beyond the traversal budget"
                );
            })
            .expect("deep JavaScript traversal test thread should spawn")
            .join()
            .expect("deep JavaScript traversal test thread should pass");
    }
}
