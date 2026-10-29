use crate::ExtractionResults;
use crate::extract_canonical;
use crate::tests::helpers::{
    call_owners, owner_rows, supertype_edges, symbol_metadata, symbol_named,
};
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("javascript extraction")
}

#[test]
fn unbound_class_expression_owns_its_members_under_the_function() {
    let result = extract(
        "src/anon.js",
        "function make() {\n\
         \x20 return new (class extends Base {\n\
         \x20   run() { return helper() }\n\
         \x20 })()\n\
         }\n\
         register(class { start() {} })\n\
         function helper() { return 1 }\n",
    );

    assert_eq!(
        owner_rows(&result),
        [
            "function make < -",
            "class anonymous_class_l2 < make",
            "method run < anonymous_class_l2",
            "class anonymous_class_l6 < -",
            "method start < anonymous_class_l6",
            "function helper < -",
        ]
    );
    let anonymous = symbol_named(&result, "anonymous_class_L2");
    assert_eq!(symbol_metadata(anonymous, "type"), Some("anonymous_class"));
    assert_eq!(anonymous.signature.as_deref(), Some("class extends Base"));
    assert_eq!(
        supertype_edges(&result),
        ["extends anonymous_class_l2 -> ?base"]
    );
    assert_eq!(call_owners(&result), ["helper in run", "register in -"]);
}

#[test]
fn bound_and_default_exported_classes_keep_their_names() {
    let result = extract(
        "src/named.js",
        "const Kx = class { go() {} }\n\
         const Ky = class Inner { go() {} }\n\
         module.exports.Kz = class { go() {} }\n\
         export default class { go() {} }\n",
    );

    let classes: Vec<String> = owner_rows(&result)
        .into_iter()
        .filter(|row| row.starts_with("class "))
        .collect();
    assert_eq!(
        classes,
        [
            "class kx < -",
            "class inner < -",
            "class kz < -",
            "class default < -",
        ]
    );
}

#[test]
fn jsx_class_expression_owns_its_members() {
    let result = extract(
        "src/view.jsx",
        "export function wrap(Inner) {\n\
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
}
