use crate::base::RelationshipKind;
use crate::extract_canonical;
use crate::tests::helpers::{facts_with_pattern, metadata_str};

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    let workspace_root = std::path::PathBuf::from("/test/workspace");
    extract_canonical(file_path, source, &workspace_root).expect("extraction should succeed")
}

#[test]
fn test_review_regression_typescript_implements_keeps_namespace_context() {
    let workspace_root = std::path::PathBuf::from("/test/workspace");
    let code = r#"
class ServiceImpl implements Api.IService<Response> {
    serve() { }
}
"#;

    let results = extract_canonical("src/service-impl.ts", code, &workspace_root)
        .expect("typescript extraction should succeed");

    let pending = results
        .structured_pending_relationships
        .iter()
        .find(|pending| pending.pending.kind == RelationshipKind::Implements)
        .expect("should emit structured implements pending relationship");
    assert_eq!(pending.target.display_name, "Api.IService");
    assert_eq!(pending.target.terminal_name, "IService");
    assert_eq!(pending.target.namespace_path, vec!["Api"]);
}

#[test]
fn test_review_regression_python_hash_comments_are_not_docs() {
    let workspace_root = std::path::PathBuf::from("/test/workspace");
    let code = "# helper comment\ndef foo():\n    return 1\n";

    let results = extract_canonical("src/module.py", code, &workspace_root)
        .expect("python extraction should succeed");
    let foo = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "foo")
        .expect("should extract foo");
    assert_eq!(foo.doc_comment, None);
}

#[test]
fn test_review_regression_sql_trigger_keeps_trigger_and_target_table_distinct() {
    let source = r#"
CREATE TABLE users (id INTEGER);
CREATE TABLE audit_log (user_id INTEGER);

CREATE TRIGGER users_ai
AFTER INSERT ON users
BEGIN
  INSERT INTO audit_log(user_id) VALUES (NEW.id);
END;
"#;

    let results = extract("schema/triggers.sql", source);
    let facts = facts_with_pattern(&results, "sql.trigger_definition.v1");

    assert_eq!(facts.len(), 1, "expected one trigger definition fact");
    let fact = facts[0];
    assert_eq!(metadata_str(fact, "trigger_name"), Some("users_ai"));
    assert_eq!(metadata_str(fact, "target_table"), Some("users"));
}

#[test]
fn test_review_regression_react_index_route_requires_boolean_true_token() {
    let source = r#"
import { createBrowserRouter } from "react-router-dom";

const trueValue = false;
const routes = [
  { index: trueValue, Component: Home }
];

export const router = createBrowserRouter(routes);
"#;

    let results = extract("src/routes.jsx", source);

    assert!(
        facts_with_pattern(&results, "react.route_definition.v1").is_empty(),
        "non-literal `index: trueValue` must not emit an index route"
    );
}

#[test]
fn test_review_regression_html_comments_do_not_emit_htmx_facts() {
    let source = r#"
<main>
  <!-- <button hx-get="/commented">Hidden</button> -->
  <button id="plain">Visible</button>
</main>
"#;

    let results = extract("src/index.html", source);

    assert!(
        facts_with_pattern(&results, "htmx.attribute.v1").is_empty(),
        "commented htmx attributes must not emit artifact facts"
    );
}
