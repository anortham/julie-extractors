use crate::ExtractionResults;
use crate::extract_canonical;
use crate::tests::helpers::{
    call_owners, owner_rows, supertype_edges, symbol_metadata, symbol_named,
};
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("typescript extraction")
}

#[test]
fn unbound_class_expression_owns_its_members_and_records_its_heritage() {
    let result = extract(
        "src/anon.ts",
        "interface Task { run(): number }\n\
         export function make(): Task {\n\
         \x20 return new (class extends Base implements Task {\n\
         \x20   run() { return helper() }\n\
         \x20 })()\n\
         }\n\
         function helper(): number { return 1 }\n",
    );

    assert_eq!(
        owner_rows(&result),
        [
            "interface task < -",
            "method run < task",
            "function make < -",
            "class anonymous_class_l3 < make",
            "method run < anonymous_class_l3",
            "function helper < -",
        ]
    );
    let anonymous = symbol_named(&result, "anonymous_class_L3");
    assert_eq!(symbol_metadata(anonymous, "type"), Some("anonymous_class"));
    assert_eq!(symbol_metadata(anonymous, "extends"), Some("Base"));
    assert_eq!(
        anonymous.signature.as_deref(),
        Some("class extends Base implements Task")
    );
    assert_eq!(
        supertype_edges(&result),
        [
            "extends anonymous_class_l3 -> ?base",
            "implements anonymous_class_l3 -> task",
        ]
    );
    assert_eq!(call_owners(&result), ["helper in run"]);
}

#[test]
fn bound_and_default_exported_classes_keep_their_names() {
    let result = extract(
        "src/named.ts",
        "const K = class { go() {} }\n\
         const N = class Named { go() {} }\n\
         export const Cast = (class { go() {} }) as unknown\n\
         export default class { go() {} }\n",
    );

    let classes: Vec<String> = owner_rows(&result)
        .into_iter()
        .filter(|row| row.starts_with("class "))
        .collect();
    assert_eq!(
        classes,
        [
            "class k < -",
            "class named < -",
            "class cast < -",
            "class default < -",
        ]
    );
}

#[test]
fn tsx_class_expression_owns_its_members() {
    let result = extract(
        "src/view.tsx",
        "export function wrap(Inner: Component) {\n\
         \x20 return class extends React.Component {\n\
         \x20   render() { return <Inner /> }\n\
         \x20 }\n\
         }\n",
    );

    assert_eq!(
        owner_rows(&result),
        [
            "function wrap < -",
            "class anonymous_class_l2 < wrap",
            "method render < anonymous_class_l2",
        ]
    );
    assert_eq!(
        supertype_edges(&result),
        ["extends anonymous_class_l2 -> ?component"]
    );
}

#[test]
fn vue_script_class_expression_owns_its_members() {
    let result = extract(
        "src/Panel.vue",
        "<script setup lang=\"ts\">\n\
         function make() {\n\
         \x20 return new (class { run() { return 1 } })()\n\
         }\n\
         </script>\n",
    );

    let rows: Vec<String> = owner_rows(&result)
        .into_iter()
        .filter(|row| !row.ends_with("panel < -"))
        .collect();
    assert_eq!(
        rows,
        [
            "function make < -",
            "class anonymous_class_l3 < make",
            "method run < anonymous_class_l3",
        ]
    );
}
