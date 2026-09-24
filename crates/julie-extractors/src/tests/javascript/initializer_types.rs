use crate::javascript::JavaScriptExtractor;
use std::path::PathBuf;

fn inferred_type(source: &str, local: &str) -> Option<(String, bool)> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let mut extractor = JavaScriptExtractor::new(
        "javascript".to_string(),
        "initializer_types.js".to_string(),
        source.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    let local = symbols
        .iter()
        .find(|s| s.name == local && s.kind == crate::base::SymbolKind::Variable)
        .unwrap_or_else(|| panic!("missing local {local}"));
    extractor
        .base
        .type_info
        .get(&local.id)
        .map(|fact| (fact.resolved_type.clone(), fact.is_inferred))
}

fn inferred(name: &str) -> Option<(String, bool)> {
    Some((name.to_string(), true))
}

const LOADERS: &str = r#"
class Workspace {
    /** @returns {Folder} */
    child() {}

    /** @returns {Folder} */
    get root() {}

    /** @returns {Workspace} */
    static open() {}
}

/** @returns {Workspace} */
function loadWorkspace() {}

/** @returns {Promise<Workspace>} */
function fetchWorkspace() {}

/** @returns {Promise.<Workspace>} */
function legacyFetch() {}

/** @returns {?Workspace} */
function findWorkspace() {}

/** @returns {Workspace|null} */
function maybeWorkspace() {}

/** @returns {*} */
function anything() {}

function undocumented() {}

/**
 * @template T
 * @param {T} value
 * @returns {T}
 */
function identity(value) {}

/**
 * @template T
 * @returns {Promise<T>}
 */
function fetchAny() {}

/** @returns {Promise<Workspace>} */
async function fetchAsync() {}

/** @returns {Workspace} */
async function loadAsync() {}

/**
 * @returns {Workspace}
 * @returns {Folder}
 */
function overloaded() {}

/** @returns {Workspace} */
const loadArrow = () => {};
"#;

fn workspace_type(body: &str) -> Option<(String, bool)> {
    inferred_type(
        &format!("{LOADERS}\nasync function run() {{\n    {body}\n}}\n"),
        "workspace",
    )
}

#[test]
fn same_file_function_call_records_jsdoc_return_type_as_inferred() {
    assert_eq!(
        workspace_type("const workspace = loadWorkspace();"),
        inferred("Workspace")
    );
}

#[test]
fn every_declaration_keyword_records_the_call_type() {
    for keyword in ["let", "var"] {
        assert_eq!(
            workspace_type(&format!("{keyword} workspace = loadWorkspace();")),
            inferred("Workspace"),
            "{keyword}"
        );
    }
}

#[test]
fn function_declared_after_the_call_records_its_type() {
    let source = r#"
const workspace = later();

/** @returns {Workspace} */
function later() {}
"#;
    assert_eq!(inferred_type(source, "workspace"), inferred("Workspace"));
}

#[test]
fn arrow_function_const_with_jsdoc_records_its_type() {
    assert_eq!(
        workspace_type("const workspace = loadArrow();"),
        inferred("Workspace")
    );
}

#[test]
fn nullable_return_type_records_the_base_type() {
    assert_eq!(
        workspace_type("const workspace = findWorkspace();"),
        inferred("Workspace")
    );
}

#[test]
fn awaited_promise_call_records_the_resolved_type() {
    for callee in ["fetchWorkspace", "legacyFetch", "fetchAsync"] {
        assert_eq!(
            workspace_type(&format!("const workspace = await {callee}();")),
            inferred("Workspace"),
            "{callee}"
        );
    }
}

#[test]
fn unawaited_promise_call_records_promise() {
    assert_eq!(
        workspace_type("const workspace = fetchWorkspace();"),
        inferred("Promise")
    );
}

#[test]
fn parenthesized_call_records_its_type() {
    assert_eq!(
        workspace_type("const workspace = (await (fetchWorkspace()));"),
        inferred("Workspace")
    );
}

#[test]
fn static_call_on_same_file_class_records_its_type() {
    assert_eq!(
        workspace_type("const workspace = Workspace.open();"),
        inferred("Workspace")
    );
}

#[test]
fn chained_instance_method_of_a_same_file_class_records_its_type() {
    assert_eq!(
        workspace_type("const workspace = Workspace.open().child();"),
        inferred("Folder")
    );
    assert_eq!(
        workspace_type("const workspace = loadWorkspace().child();"),
        inferred("Folder")
    );
    assert_eq!(
        workspace_type("const workspace = new Workspace().child();"),
        inferred("Folder")
    );
}

#[test]
fn this_method_call_in_a_class_method_records_its_type() {
    let source = r#"
class Loader {
    /** @returns {Workspace} */
    load() {}

    /** @returns {Workspace} */
    static create() {}

    run() {
        const workspace = this.load();
        const later = () => {
            const nested = this.load();
        };
    }

    static build() {
        const built = this.create();
    }

    handler = () => {
        const field = this.load();
    };
}
"#;
    for local in ["workspace", "nested", "built", "field"] {
        assert_eq!(
            inferred_type(source, local),
            inferred("Workspace"),
            "{local}"
        );
    }
}

#[test]
fn private_method_call_records_its_type() {
    let source = r#"
class Loader {
    /** @returns {Workspace} */
    #load() {}

    run() {
        const workspace = this.#load();
    }
}
"#;
    assert_eq!(inferred_type(source, "workspace"), inferred("Workspace"));
}

#[test]
fn agreeing_same_named_functions_record_their_type() {
    let source = r#"
/** @returns {Workspace} */
function load() {}

function run() {
    /** @returns {Workspace} */
    function load() {}
    const workspace = load();
}
"#;
    assert_eq!(inferred_type(source, "workspace"), inferred("Workspace"));
}

#[test]
fn same_named_function_in_another_scope_does_not_block_the_call() {
    let source = r#"
/** @returns {Workspace} */
function load() {}

function other() {
    /** @returns {Folder} */
    function load() {}
}

function run() {
    const workspace = load();
}
"#;
    assert_eq!(inferred_type(source, "workspace"), inferred("Workspace"));
}

#[test]
fn written_jsdoc_type_wins_over_call_inference() {
    assert_eq!(
        workspace_type("/** @type {Folder} */\n    const workspace = loadWorkspace();"),
        Some(("Folder".to_string(), false))
    );
}

#[test]
fn written_jsdoc_type_wins_over_constructor_inference() {
    assert_eq!(
        workspace_type("/** @type {Folder} */\n    const workspace = new Workspace();"),
        Some(("Folder".to_string(), false))
    );
}

#[test]
fn constructor_call_still_records_its_type() {
    assert_eq!(
        workspace_type("const workspace = new Elsewhere();"),
        inferred("Elsewhere")
    );
}

#[test]
fn call_to_a_function_from_another_file_records_nothing() {
    let source = r#"
import { load } from "./load";
const workspace = load();
"#;
    assert_eq!(inferred_type(source, "workspace"), None);
    assert_eq!(workspace_type("const workspace = elsewhere();"), None);
}

#[test]
fn import_with_the_same_name_as_a_same_file_function_records_nothing() {
    let source = r#"
import { load } from "./load";

function run() {
    /** @returns {Workspace} */
    function load() {}
}

const workspace = load();
"#;
    assert_eq!(inferred_type(source, "workspace"), None);
}

#[test]
fn parameter_shadowing_a_same_file_function_records_nothing() {
    let source = r#"
/** @returns {Workspace} */
function load() {}

function run(load) {
    const workspace = load();
}
"#;
    assert_eq!(inferred_type(source, "workspace"), None);
}

#[test]
fn disagreeing_same_named_functions_record_nothing() {
    let source = r#"
/** @returns {Workspace} */
function load() {}

function run() {
    /** @returns {Folder} */
    function load() {}
    const workspace = load();
}
"#;
    assert_eq!(inferred_type(source, "workspace"), None);
}

#[test]
fn conflicting_return_tags_record_nothing() {
    assert_eq!(workspace_type("const workspace = overloaded();"), None);
}

#[test]
fn template_return_types_record_nothing() {
    assert_eq!(workspace_type("const workspace = identity(value);"), None);
    assert_eq!(workspace_type("const workspace = await fetchAny();"), None);
}

#[test]
fn class_template_return_type_records_nothing() {
    let source = r#"
/** @template T */
class Box {
    /** @returns {T} */
    get() {}

    run() {
        const workspace = this.get();
    }
}
"#;
    assert_eq!(inferred_type(source, "workspace"), None);
}

#[test]
fn unsupported_return_types_record_nothing() {
    for callee in ["maybeWorkspace", "anything", "undocumented"] {
        assert_eq!(
            workspace_type(&format!("const workspace = {callee}();")),
            None,
            "{callee}"
        );
    }
}

#[test]
fn awaited_non_promise_call_records_nothing() {
    assert_eq!(
        workspace_type("const workspace = await loadWorkspace();"),
        None
    );
}

#[test]
fn async_function_without_a_promise_return_type_records_nothing() {
    assert_eq!(workspace_type("const workspace = loadAsync();"), None);
    assert_eq!(workspace_type("const workspace = await loadAsync();"), None);
}

#[test]
fn unknown_method_at_the_end_of_a_chain_records_nothing() {
    for chain in [
        "loadWorkspace().toString()",
        "fetchWorkspace().then(done)",
        "Workspace.open().missing()",
        "elsewhere().child()",
    ] {
        assert_eq!(
            workspace_type(&format!("const workspace = {chain};")),
            None,
            "{chain}"
        );
    }
}

#[test]
fn getter_called_as_a_method_records_nothing() {
    assert_eq!(
        workspace_type("const workspace = loadWorkspace().root();"),
        None
    );
}

#[test]
fn static_and_instance_mismatches_record_nothing() {
    assert_eq!(workspace_type("const workspace = Workspace.child();"), None);
    assert_eq!(
        workspace_type("const workspace = loadWorkspace().open();"),
        None
    );
}

#[test]
fn optional_calls_record_nothing() {
    assert_eq!(workspace_type("const workspace = loadWorkspace?.();"), None);
    assert_eq!(workspace_type("const workspace = Workspace?.open();"), None);
}

#[test]
fn this_call_where_this_is_rebound_records_nothing() {
    let source = r#"
class Loader {
    /** @returns {Workspace} */
    load() {}

    run() {
        function inner() {
            const workspace = this.load();
        }
    }
}
"#;
    assert_eq!(inferred_type(source, "workspace"), None);
}

#[test]
fn this_call_in_an_object_literal_method_records_nothing() {
    let source = r#"
const api = {
    /** @returns {Workspace} */
    load() {},

    run() {
        const workspace = this.load();
    },
};
"#;
    assert_eq!(inferred_type(source, "workspace"), None);
}

#[test]
fn this_call_to_an_inherited_method_records_nothing() {
    let source = r#"
class Loader extends Base {
    run() {
        const workspace = this.load();
    }
}
"#;
    assert_eq!(inferred_type(source, "workspace"), None);
}

#[test]
fn class_member_with_the_same_name_that_is_not_a_method_records_nothing() {
    let source = r#"
class Loader {
    /** @returns {Workspace} */
    static load() {}

    static load = 3;

    static run() {
        const workspace = this.load();
    }
}
"#;
    assert_eq!(inferred_type(source, "workspace"), None);
}

#[test]
fn static_call_on_a_name_that_is_not_a_same_file_class_records_nothing() {
    let source = r#"
import { Workspace } from "./workspace";

const workspace = Workspace.open();
"#;
    assert_eq!(inferred_type(source, "workspace"), None);
}

#[test]
fn loop_catch_and_callback_bindings_shadowing_a_same_file_function_record_nothing() {
    for body in [
        "for (const loadWorkspace of loaders) { const workspace = loadWorkspace(); }",
        "for (let loadWorkspace in loaders) { const workspace = loadWorkspace(); }",
        "try {} catch (loadWorkspace) { const workspace = loadWorkspace(); }",
        "loaders.map(loadWorkspace => { const workspace = loadWorkspace(); });",
        "loaders.map(function (loadWorkspace) { const workspace = loadWorkspace(); });",
        "loaders.map(({ loadWorkspace }) => { const workspace = loadWorkspace(); });",
    ] {
        assert_eq!(workspace_type(body), None, "{body}");
    }
}

#[test]
fn callback_binding_shadowing_a_same_file_class_records_nothing() {
    assert_eq!(
        workspace_type("classes.map((Workspace) => { const workspace = Workspace.open(); });"),
        None
    );
}

#[test]
fn exported_function_expression_assignment_records_nothing_for_a_bare_call() {
    let source = r#"
/** @returns {Workspace} */
exports.load = function () {};

const workspace = load();
"#;
    assert_eq!(inferred_type(source, "workspace"), None);
}

#[test]
fn inner_name_of_a_class_expression_records_nothing_outside_the_class() {
    let source = r#"
const Loader = class Inner {
    /** @returns {Workspace} */
    static open() {}
};

const workspace = Inner.open();
"#;
    assert_eq!(inferred_type(source, "workspace"), None);
}

#[test]
fn exported_and_expression_bound_callables_record_their_type() {
    let source = r#"
/** @returns {Workspace} */
export function exported() {}

/** @returns {Workspace} */
const expression = function () {};

const Loader = class {
    /** @returns {Workspace} */
    static open() {}
};

export class Opener {
    /** @returns {Workspace} */
    static open() {}
}

const first = exported();
const second = expression();
const third = Loader.open();
const fourth = Opener.open();
"#;
    for local in ["first", "second", "third", "fourth"] {
        assert_eq!(
            inferred_type(source, local),
            inferred("Workspace"),
            "{local}"
        );
    }
}

#[test]
fn this_return_type_records_nothing() {
    let source = r#"
class Builder {
    /** @returns {this} */
    static create() {}
}

const workspace = Builder.create();
"#;
    assert_eq!(inferred_type(source, "workspace"), None);
}

#[test]
fn named_function_expression_own_name_shadows_a_same_file_function() {
    let source = r#"
/** @returns {Foo} */
function load() {}

const outer = function load(n) {
    if (n) {
        const inner = load(0);
        return inner;
    }
    return 42;
};
"#;
    assert_eq!(inferred_type(source, "inner"), None);
}

#[test]
fn named_class_expression_own_name_shadows_a_same_file_class() {
    let source = r#"
class Loader {
    /** @returns {Foo} */
    static open() {}
}

const Other = class Loader {
    static open() {}

    run() {
        const inner = Loader.open();
    }
};
"#;
    assert_eq!(inferred_type(source, "inner"), None);
}

#[test]
fn generator_call_with_an_element_return_type_records_nothing() {
    let source = r#"
/** @returns {Foo} */
function* gen() {}

/** @returns {Promise<Foo>} */
async function* asyncGen() {}

class G {
    /** @returns {Foo} */
    *items() {}

    run() {
        const method = this.items();
    }
}

const plain = gen();
const awaited = asyncGen();
"#;
    for local in ["plain", "awaited", "method"] {
        assert_eq!(inferred_type(source, local), None, "{local}");
    }
}

#[test]
fn generator_call_with_a_generator_return_type_records_it() {
    let source = r#"
/** @returns {Generator<Foo>} */
function* gen() {}

/** @returns {AsyncGenerator<Foo>} */
async function* asyncGen() {}

const plain = gen();
const later = asyncGen();
"#;
    assert_eq!(inferred_type(source, "plain"), inferred("Generator"));
    assert_eq!(inferred_type(source, "later"), inferred("AsyncGenerator"));
}

#[test]
fn declaration_doc_does_not_type_later_declarators() {
    let source = r#"
/** @returns {Foo} */
const mkFoo = () => new Foo(), mkBar = () => new Bar();

function main() {
    const first = mkFoo();
    const second = mkBar();
}
"#;
    assert_eq!(inferred_type(source, "first"), inferred("Foo"));
    assert_eq!(inferred_type(source, "second"), None);
}

#[test]
fn declarations_in_callbacks_iifes_and_blocks_are_not_visible_outside() {
    let source = r#"
[1].forEach(() => {
    /** @returns {Foo} */
    function load() {}
});
(function () {
    /** @returns {Foo} */
    function load2() {}
})();
if (true) {
    /** @returns {Foo} */
    const load3 = () => new Foo();
}
describe("x", function () {
    class Workspace {
        /** @returns {Foo} */
        static open() {}
    }
});

const m1 = load();
const m2 = load2();
const m3 = load3();
const n1 = Workspace.open();
"#;
    for local in ["m1", "m2", "m3", "n1"] {
        assert_eq!(inferred_type(source, local), None, "{local}");
    }
}

#[test]
fn block_function_called_outside_its_block_in_the_same_function_records_nothing() {
    let source = r#"
/** @returns {Bar} */
function load() {}

function main() {
    if (ready) {
        /** @returns {Foo} */
        function load() {}
    }
    const outside = load();
}
"#;
    assert_eq!(inferred_type(source, "outside"), None);
}

#[test]
fn declarations_in_the_calling_scope_record_their_type() {
    let source = r#"
function main() {
    if (ready) {
        /** @returns {Foo} */
        function load() {}
        const inBlock = load();
        /** @returns {Bar} */
        var loadBar = function () {};
    }
    const hoisted = loadBar();
}
"#;
    assert_eq!(inferred_type(source, "inBlock"), inferred("Foo"));
    assert_eq!(inferred_type(source, "hoisted"), inferred("Bar"));
}

#[test]
fn callback_or_typedef_block_before_a_function_does_not_type_its_calls() {
    for detached in [
        "@callback Loader\n * @param {string} p\n * @returns {Bar}",
        "@typedef {Object} Options\n * @returns {Bar}",
    ] {
        for own_doc in ["/**\n * Loads it.\n */\n", ""] {
            let source = format!(
                "class Foo {{}} class Bar {{}}\n/**\n * {detached}\n */\n\n{own_doc}function load() {{ return new Foo(); }}\nconst loaded = load();\n"
            );
            assert_eq!(inferred_type(&source, "loaded"), None, "{source}");
        }
    }
}

#[test]
fn own_jsdoc_block_after_a_callback_block_types_the_call() {
    let source = r#"
class Foo {} class Bar {}
/**
 * @callback Loader
 * @returns {Bar}
 */

/** @returns {Foo} */
function load() { return new Foo(); }
const loaded = load();
"#;
    assert_eq!(inferred_type(source, "loaded"), inferred("Foo"));
}

#[test]
fn overloaded_function_records_nothing() {
    let source = r#"
class Foo {} class Bar {}
/**
 * @overload
 * @param {string} x
 * @returns {Bar}
 */
/**
 * @param {string | number} x
 * @returns {Foo}
 */
function load(x) { return new Foo(); }
const loaded = load("a");
"#;
    assert_eq!(inferred_type(source, "loaded"), None);
}

#[test]
fn chained_call_resolves_the_returned_class_where_the_callee_is_declared() {
    let source = r#"
/** @returns {Node} */
function makeNode() { return document.createElement("div"); }
function build() {
  class Node { /** @returns {Leaf} */ child() { return null; } }
  const chained = makeNode().child();
}
"#;
    assert_eq!(inferred_type(source, "chained"), None);
}

#[test]
fn this_in_a_computed_member_name_records_nothing() {
    let source = r#"
class Foo {} class Bar {}
class Outer {
  /** @returns {Foo} */ make() { return new Foo(); }
  go() {
    class Inner {
      /** @returns {Bar} */ make() { return new Bar(); }
      [(() => { const computedKey = this.make(); return "k"; })()]() {}
    }
  }
}
"#;
    assert_eq!(inferred_type(source, "computedKey"), None);
}

#[test]
fn same_named_rest_parameter_does_not_block_a_top_level_call() {
    let source = r#"
class Foo {}
/** @returns {Foo} */
function load() { return new Foo(); }
function f6(...load) { return load; }
const loaded = load();
"#;
    assert_eq!(inferred_type(source, "loaded"), inferred("Foo"));
}

#[test]
fn callback_block_before_a_function_does_not_type_the_function() {
    let source =
        "class Bar {}\n/**\n * @callback Loader\n * @returns {Bar}\n */\n\nfunction load() {}\n";
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let mut extractor = JavaScriptExtractor::new(
        "javascript".to_string(),
        "initializer_types.js".to_string(),
        source.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    let load = symbols.iter().find(|s| s.name == "load").unwrap();
    assert!(
        load.doc_comment
            .as_deref()
            .unwrap_or("")
            .contains("@callback")
    );
    assert_eq!(extractor.base.type_info.get(&load.id), None);
}
