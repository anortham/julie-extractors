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
