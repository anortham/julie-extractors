use crate::base::{Symbol, SymbolKind};
use crate::pipeline::extract_canonical;

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    extract_canonical(file_path, source, std::path::Path::new("/repo"))
        .expect("canonical extraction should succeed")
}

fn symbol<'a>(results: &'a crate::ExtractionResults, name: &str, kind: SymbolKind) -> &'a Symbol {
    results
        .symbols
        .iter()
        .find(|symbol| symbol.name == name && symbol.kind == kind)
        .unwrap_or_else(|| panic!("missing {kind:?} symbol {name}"))
}

#[test]
fn typescript_decorator_parameter_list_is_not_a_property_body() {
    assert_decorator_field_is_bodiless("src/widget.ts", "name: string", SymbolKind::Property);
}

#[test]
fn tsx_decorator_parameter_list_is_not_a_property_body() {
    assert_decorator_field_is_bodiless("src/widget.tsx", "name: string", SymbolKind::Property);
}

#[test]
fn javascript_decorator_parameter_list_is_not_a_field_body() {
    assert_decorator_field_is_bodiless("src/widget.js", "name", SymbolKind::Field);
}

#[test]
fn jsx_decorator_parameter_list_is_not_a_field_body() {
    assert_decorator_field_is_bodiless("src/widget.jsx", "name", SymbolKind::Field);
}

#[test]
fn function_valued_fields_keep_callable_body_spans() {
    let source = "class Widget {\r\n  callback = () => { return \"ok\"; };\r\n}\r\n";
    for file_path in [
        "src/widget.ts",
        "src/widget.tsx",
        "src/widget.js",
        "src/widget.jsx",
    ] {
        let results = extract(file_path, source);
        assert!(
            results.parse_diagnostics.is_empty(),
            "{:#?}",
            results.parse_diagnostics
        );

        let callback = symbol(&results, "callback", SymbolKind::Method);
        let body = callback.body_span.expect(file_path);
        assert_eq!(
            source.get(body.start_byte as usize..body.end_byte as usize),
            Some("{ return \"ok\"; }")
        );
        assert!(callback.body_hash.is_some(), "{file_path}");
    }
}

#[test]
fn callable_variables_keep_their_implementation_bodies() {
    let source = "const callback = (value) => { return value + 1; };\r\n";
    for file_path in [
        "src/widget.ts",
        "src/widget.tsx",
        "src/widget.js",
        "src/widget.jsx",
    ] {
        let results = extract(file_path, source);
        assert!(
            results.parse_diagnostics.is_empty(),
            "{file_path}: {:#?}",
            results.parse_diagnostics
        );

        let callback = symbol(&results, "callback", SymbolKind::Function);
        let body = callback.body_span.expect(file_path);
        assert_eq!(
            source.get(body.start_byte as usize..body.end_byte as usize),
            Some("{ return value + 1; }")
        );
        assert!(callback.body_hash.is_some(), "{file_path}");
    }
}

#[test]
fn typescript_parameters_do_not_inherit_type_annotation_bodies() {
    for file_path in ["src/widget.ts", "src/widget.tsx"] {
        let source =
            "function run(callback: () => void, options: { enabled: boolean }) { callback(); }\n";
        let results = extract(file_path, source);
        assert!(
            results.parse_diagnostics.is_empty(),
            "{file_path}: {:#?}",
            results.parse_diagnostics
        );

        for name in ["callback", "options"] {
            let parameter = symbol(&results, name, SymbolKind::Variable);
            assert_eq!(parameter.body_span, None, "{file_path}: {name}");
            assert_eq!(parameter.body_hash, None, "{file_path}: {name}");
        }

        let run = symbol(&results, "run", SymbolKind::Function);
        let body = run.body_span.expect(file_path);
        assert_eq!(
            source.get(body.start_byte as usize..body.end_byte as usize),
            Some("{ callback(); }")
        );
        assert!(run.body_hash.is_some(), "{file_path}");
    }
}

#[test]
fn vue_script_setup_parameters_do_not_inherit_type_annotation_bodies() {
    let source = "<script setup lang=\"ts\">\nfunction run(callback: () => void) { callback(); }\n</script>\n";
    let results = extract("src/component.vue", source);
    assert!(
        results.parse_diagnostics.is_empty(),
        "{:#?}",
        results.parse_diagnostics
    );

    let callback = symbol(&results, "callback", SymbolKind::Variable);
    assert_eq!(callback.body_span, None);
    assert_eq!(callback.body_hash, None);

    let run = symbol(&results, "run", SymbolKind::Function);
    let body = run.body_span.expect("run");
    assert_eq!(
        source.get(body.start_byte as usize..body.end_byte as usize),
        Some("{ callback(); }")
    );
    assert!(run.body_hash.is_some());
}

#[test]
fn imports_and_non_callable_variables_have_no_body_spans() {
    let source = "import { Widget as ImportedWidget } from \"./widgets\";\nconst result = createResult();\nconst config = { nested: { enabled: true } };\nconst loaded = require(\"./loaded\");\nconst dynamic = import(\"./lazy\");\nexport function build() { return result; }\n";
    for file_path in [
        "src/widget.ts",
        "src/widget.tsx",
        "src/widget.js",
        "src/widget.jsx",
    ] {
        let results = extract(file_path, source);
        assert!(
            results.parse_diagnostics.is_empty(),
            "{file_path}: {:#?}",
            results.parse_diagnostics
        );

        for name in ["ImportedWidget", "loaded", "./lazy"] {
            let import = symbol(&results, name, SymbolKind::Import);
            assert_eq!(import.body_span, None, "{file_path}: {name}");
            assert_eq!(import.body_hash, None, "{file_path}: {name}");
        }

        for name in ["result", "config"] {
            let variable = symbol(&results, name, SymbolKind::Variable);
            assert_eq!(variable.body_span, None, "{file_path}: {name}");
            assert_eq!(variable.body_hash, None, "{file_path}: {name}");
        }

        let build = symbol(&results, "build", SymbolKind::Function);
        assert!(build.body_span.is_some(), "{file_path}");
        assert!(build.body_hash.is_some(), "{file_path}");
    }
}

#[test]
fn assigned_properties_have_no_initializer_bodies_and_callable_members_keep_theirs() {
    let source = "const context = {};\ncontext.options = {};\ncontext.logger = new Logger();\ncontext.render = () => { return context.options; };\n";
    let results = extract("src/context.js", source);
    assert!(
        results.parse_diagnostics.is_empty(),
        "{:#?}",
        results.parse_diagnostics
    );

    for name in ["options", "logger"] {
        let property = symbol(&results, name, SymbolKind::Property);
        assert_eq!(property.body_span, None, "{name}");
        assert_eq!(property.body_hash, None, "{name}");
    }

    let render = symbol(&results, "render", SymbolKind::Method);
    let body = render.body_span.expect("render");
    assert_eq!(
        source.get(body.start_byte as usize..body.end_byte as usize),
        Some("{ return context.options; }")
    );
    assert!(render.body_hash.is_some());
}

#[test]
fn constructor_assigned_properties_have_no_initializer_bodies() {
    let source = "class Service { constructor() { this.logger = new Logger(); } }\n";
    let results = extract("src/service.js", source);
    assert!(
        results.parse_diagnostics.is_empty(),
        "{:#?}",
        results.parse_diagnostics
    );

    let logger = results
        .symbols
        .iter()
        .find(|symbol| {
            symbol.name == "logger"
                && symbol.kind == SymbolKind::Property
                && symbol
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("isConstructorAssigned"))
                    .and_then(serde_json::Value::as_bool)
                    == Some(true)
        })
        .expect("constructor-assigned logger property");
    assert_eq!(logger.body_span, None);
    assert_eq!(logger.body_hash, None);
    assert_eq!(
        results
            .types
            .get(&logger.id)
            .map(|fact| fact.resolved_type.as_str()),
        Some("Logger")
    );
}

#[test]
fn vue_emits_type_signatures_have_no_event_bodies() {
    let source = "<script setup lang=\"ts\">\nconst emit = defineEmits<{ (event: 'close', reason: string): void }>()\n</script>\n";
    let results = extract("src/dialog.vue", source);
    assert!(
        results.parse_diagnostics.is_empty(),
        "{:#?}",
        results.parse_diagnostics
    );

    let close = symbol(&results, "close", SymbolKind::Event);
    assert_eq!(close.body_span, None);
    assert_eq!(close.body_hash, None);
}

#[test]
fn vue_bare_define_model_has_no_call_bodies_and_keeps_prop_payloads() {
    let source = "<script setup lang=\"ts\">\nconst props = defineProps({ config: { type: String, required: true } })\ndefineModel()\n</script>\n";
    let results = extract("src/dialog.vue", source);
    assert!(
        results.parse_diagnostics.is_empty(),
        "{:#?}",
        results.parse_diagnostics
    );

    for (name, kind) in [
        ("modelValue", SymbolKind::Property),
        ("update:modelValue", SymbolKind::Event),
    ] {
        let member = symbol(&results, name, kind);
        assert_eq!(member.body_span, None, "{name}");
        assert_eq!(member.body_hash, None, "{name}");
    }

    let macro_call = symbol(&results, "defineModel", SymbolKind::Function);
    assert_eq!(macro_call.body_span, None);
    assert_eq!(macro_call.body_hash, None);

    let config = symbol(&results, "config", SymbolKind::Property);
    let body = config.body_span.expect("prop option payload");
    assert_eq!(
        source.get(body.start_byte as usize..body.end_byte as usize),
        Some("{ type: String, required: true }")
    );
    assert!(config.body_hash.is_some());
}

#[test]
fn vue_composed_body_hashes_ignore_ast_comments_and_keep_literal_payloads() {
    let source = r#"<template>
  <button title="first payload"><!-- template note -->Go</button>
  <p>it's literal text {{ label }}</p>
</template>
<script>
const renderLabel = (label) => `host ${label /* first script note */}`;
</script>
<script setup lang="ts">
const props = defineProps({ config: { type: String, /* prop note */ required: true } })
const render = (name: string) => `value ${name /* interpolation note */}`;
</script>
<style>
.item { content: "style payload"; /* style note */ color: red; }
</style>"#;
    let results = extract("src/Counter.vue", source);
    assert!(
        results.parse_diagnostics.is_empty(),
        "{:#?}",
        results.parse_diagnostics
    );

    let component_hash = symbol(&results, "Counter", SymbolKind::Class)
        .body_hash
        .clone()
        .expect("component body hash");
    let config = symbol(&results, "config", SymbolKind::Property);
    let config_body = config.body_span.expect("prop configuration body");
    assert_eq!(
        source.get(config_body.start_byte as usize..config_body.end_byte as usize),
        Some("{ type: String, /* prop note */ required: true }")
    );
    let config_hash = config.body_hash.clone().expect("prop body hash");
    let vue_render_hash = symbol(&results, "render", SymbolKind::Function)
        .body_hash
        .clone()
        .expect("Vue native script body hash");
    let script_source =
        "const render = (name: string) => `value ${name /* interpolation note */}`;";
    let script_results = extract("src/component.ts", script_source);
    assert!(
        script_results.parse_diagnostics.is_empty(),
        "{:#?}",
        script_results.parse_diagnostics
    );
    assert_eq!(
        symbol(&script_results, "render", SymbolKind::Function).body_hash,
        Some(vue_render_hash)
    );

    let comment_variant = source
        .replace("<!-- template note -->", "<!-- expanded template note -->")
        .replace(
            "/* first script note */",
            "/* expanded first script note */",
        )
        .replace("/* prop note */", "/* expanded prop note */")
        .replace(
            "/* interpolation note */",
            "/* expanded interpolation note */",
        )
        .replace("/* style note */", "/* expanded style note */");
    let commented = extract("src/Counter.vue", &comment_variant);
    assert!(
        commented.parse_diagnostics.is_empty(),
        "{:#?}",
        commented.parse_diagnostics
    );
    assert_eq!(
        symbol(&commented, "Counter", SymbolKind::Class).body_hash,
        Some(component_hash.clone())
    );
    assert_eq!(
        symbol(&commented, "config", SymbolKind::Property).body_hash,
        Some(config_hash)
    );

    for (needle, replacement) in [
        ("title=\"first payload\"", "title=\"changed payload\""),
        ("`value ${name", "`different ${name"),
        ("`value ${name", "`value ${person"),
        ("it's literal text", "it's changed literal text"),
        ("content: \"style payload\"", "content: \"changed payload\""),
        ("required: true", "required: false"),
    ] {
        let changed_source = source.replace(needle, replacement);
        assert_ne!(
            symbol(
                &extract("src/Counter.vue", &changed_source),
                "Counter",
                SymbolKind::Class
            )
            .body_hash,
            Some(component_hash.clone()),
            "{needle}"
        );
    }
}

#[test]
fn csharp_and_razor_delegates_have_no_signature_bodies() {
    let cases = [
        (
            "src/Widget.cs",
            "public class Widget { [Obsolete] public delegate Task Handler<T>(int id, T value); public int Run() { return 1; } }",
        ),
        (
            "src/Widget.razor",
            "@code { [Obsolete] public delegate Task Handler<T>(int id, T value); public int Run() { return 1; } }",
        ),
    ];

    for (file_path, source) in cases {
        let results = extract(file_path, source);
        assert!(
            results.parse_diagnostics.is_empty(),
            "{file_path}: {:#?}",
            results.parse_diagnostics
        );

        let widget = symbol(&results, "Widget", SymbolKind::Class);
        let handler = symbol(&results, "Handler", SymbolKind::Delegate);
        assert_eq!(handler.body_span, None, "{file_path}");
        assert_eq!(handler.body_hash, None, "{file_path}");
        assert_eq!(handler.parent_id.as_deref(), Some(widget.id.as_str()));
        assert_eq!(
            handler.signature.as_deref(),
            Some("[Obsolete] public delegate Task Handler<T>(int id, T value)")
        );
        assert!(
            handler
                .annotations
                .iter()
                .any(|annotation| annotation.annotation_key == "obsolete"),
            "{file_path}"
        );

        let run = symbol(&results, "Run", SymbolKind::Method);
        let body = run.body_span.expect("Run body");
        assert_eq!(
            source.get(body.start_byte as usize..body.end_byte as usize),
            Some("{ return 1; }"),
            "{file_path}"
        );
        assert!(run.body_hash.is_some(), "{file_path}");
    }
}

#[test]
fn csharp_and_razor_interface_method_signatures_have_no_body_but_expression_methods_keep_theirs() {
    let csharp_source = "public interface IValidatable { bool Validate(); }\npublic class FormModel : IValidatable { public bool Validate() => true; }\n";
    let csharp = extract("src/Model.cs", csharp_source);
    assert!(
        csharp.parse_diagnostics.is_empty(),
        "{:#?}",
        csharp.parse_diagnostics
    );

    let csharp_interface = symbol(&csharp, "IValidatable", SymbolKind::Interface);
    let csharp_signature = csharp
        .symbols
        .iter()
        .find(|symbol| {
            symbol.name == "Validate"
                && symbol.kind == SymbolKind::Method
                && symbol.parent_id.as_deref() == Some(csharp_interface.id.as_str())
        })
        .expect("C# interface Validate signature");
    assert_eq!(csharp_signature.body_span, None);
    assert_eq!(csharp_signature.body_hash, None);

    let csharp_model = symbol(&csharp, "FormModel", SymbolKind::Class);
    let csharp_implementation = csharp
        .symbols
        .iter()
        .find(|symbol| {
            symbol.name == "Validate"
                && symbol.kind == SymbolKind::Method
                && symbol.parent_id.as_deref() == Some(csharp_model.id.as_str())
        })
        .expect("C# FormModel.Validate expression body");
    let csharp_body = csharp_implementation.body_span.expect("C# expression body");
    assert_eq!(
        csharp_source.get(csharp_body.start_byte as usize..csharp_body.end_byte as usize),
        Some("=> true")
    );
    assert!(csharp_implementation.body_hash.is_some());

    let source = include_str!("../../../../fixtures/extraction/razor/members/Counter.razor");
    let results = extract("fixtures/extraction/razor/members/Counter.razor", source);
    assert!(
        results.parse_diagnostics.is_empty(),
        "{:#?}",
        results.parse_diagnostics
    );

    let interface = symbol(&results, "IValidatable", SymbolKind::Interface);
    let signature = results
        .symbols
        .iter()
        .find(|symbol| {
            symbol.name == "Validate"
                && symbol.kind == SymbolKind::Method
                && symbol.parent_id.as_deref() == Some(interface.id.as_str())
        })
        .expect("interface Validate signature");
    assert_eq!(signature.body_span, None);
    assert_eq!(signature.body_hash, None);

    let model = symbol(&results, "FormModel", SymbolKind::Class);
    let implementation = results
        .symbols
        .iter()
        .find(|symbol| {
            symbol.name == "Validate"
                && symbol.kind == SymbolKind::Method
                && symbol.parent_id.as_deref() == Some(model.id.as_str())
        })
        .expect("FormModel.Validate expression body");
    let body = implementation.body_span.expect("expression body");
    assert_eq!(
        source.get(body.start_byte as usize..body.end_byte as usize),
        Some("=> true")
    );
    assert!(implementation.body_hash.is_some());
}

#[test]
fn export_references_and_non_callable_values_have_no_body_spans() {
    let source = "const payload = makePayload();\nexport { payload as publicPayload };\nexport { external as publicExternal } from \"./external\";\nexport const result = makeResult();\nexport const callback = value => value + 1;\nexport const blockCallback = value => { return value + 1; };\nexport function render(value) { return value; }\n";
    for file_path in [
        "src/widget.ts",
        "src/widget.tsx",
        "src/widget.js",
        "src/widget.jsx",
    ] {
        let results = extract(file_path, source);
        assert!(
            results.parse_diagnostics.is_empty(),
            "{file_path}: {:#?}",
            results.parse_diagnostics
        );

        for name in ["publicPayload", "publicExternal", "result"] {
            let export = symbol(&results, name, SymbolKind::Export);
            assert_eq!(export.body_span, None, "{file_path}: {name}");
            assert_eq!(export.body_hash, None, "{file_path}: {name}");
        }

        for (name, expected) in [
            ("callback", "value + 1"),
            ("blockCallback", "{ return value + 1; }"),
        ] {
            let export = symbol(&results, name, SymbolKind::Export);
            let body = export.body_span.expect(name);
            assert_eq!(
                source.get(body.start_byte as usize..body.end_byte as usize),
                Some(expected),
                "{file_path}: {name}"
            );
            assert!(export.body_hash.is_some(), "{file_path}: {name}");
        }

        let render = symbol(&results, "render", SymbolKind::Export);
        let body = render.body_span.expect(file_path);
        assert_eq!(
            source.get(body.start_byte as usize..body.end_byte as usize),
            Some("{ return value; }")
        );
        assert!(render.body_hash.is_some(), "{file_path}");
    }
}

#[test]
fn embedded_typescript_imports_and_variables_have_no_bodies() {
    let source = "<script lang=\"ts\">\nimport { api } from \"./api\";\nconst result = createResult();\nexport function run() { return result; }\n</script>\n";
    let results = extract("src/component.vue", source);
    assert!(
        results.parse_diagnostics.is_empty(),
        "{:#?}",
        results.parse_diagnostics
    );

    for (name, kind) in [
        ("api", SymbolKind::Import),
        ("result", SymbolKind::Variable),
    ] {
        let declaration = symbol(&results, name, kind);
        assert_eq!(declaration.body_span, None, "{name}");
        assert_eq!(declaration.body_hash, None, "{name}");
    }

    let run = symbol(&results, "run", SymbolKind::Function);
    assert!(run.body_span.is_some());
    assert!(run.body_hash.is_some());
}

#[test]
fn qml_imports_events_and_non_callable_variables_have_no_bodies() {
    let source = "import QtQuick 2.15\r\nimport QtQuick.Controls 2.15 as Controls\r\nItem {\r\n    signal activated(string value)\r\n    function update() {\r\n        let selected = chooseValue()\r\n        let d = new Date()\r\n        let graph = new ns.GraphTraversal()\r\n        return selected\r\n    }\r\n}\r\n";
    let results = extract("src/component.qml", source);
    assert!(
        results.parse_diagnostics.is_empty(),
        "{:#?}",
        results.parse_diagnostics
    );

    for (name, kind) in [
        ("QtQuick", SymbolKind::Import),
        ("selected", SymbolKind::Variable),
        ("d", SymbolKind::Variable),
        ("graph", SymbolKind::Variable),
        ("QtQuick.Controls", SymbolKind::Import),
        ("activated", SymbolKind::Event),
    ] {
        let declaration = symbol(&results, name, kind);
        assert_eq!(declaration.body_span, None, "{name}");
        assert_eq!(declaration.body_hash, None, "{name}");
    }
}

#[test]
fn lua_module_imports_have_no_bodies() {
    let source = "require(\"lazy.core.config\").setup({})\r\n";
    let results = extract("src/module.lua", source);
    assert!(
        results.parse_diagnostics.is_empty(),
        "{:#?}",
        results.parse_diagnostics
    );

    let config = symbol(&results, "config", SymbolKind::Import);
    assert_eq!(config.body_span, None);
    assert_eq!(config.body_hash, None);
}

#[test]
fn php_callable_variables_keep_their_implementation_bodies() {
    let source = "<?php\r\nfunction withMapper(int $value): int {\r\n    $mapper = function (int $input): int {\r\n        return $input + 1;\r\n    };\r\n    return $mapper($value);\r\n}\r\n";
    let results = extract("src/module.php", source);
    assert!(
        results.parse_diagnostics.is_empty(),
        "{:#?}",
        results.parse_diagnostics
    );

    let mapper = symbol(&results, "mapper", SymbolKind::Variable);
    let body = mapper.body_span.expect("$mapper");
    assert_eq!(
        source.get(body.start_byte as usize..body.end_byte as usize),
        Some("{\r\n        return $input + 1;\r\n    }")
    );
    assert!(mapper.body_hash.is_some());
}

#[test]
fn ruby_field_and_variable_initializers_have_no_bodies() {
    let source = "class Sample\r\n  def initialize\r\n    @user = users(:one)\r\n    connection = Faraday.new(url: \"https://service.example\")\r\n    @callback = ->(value) { value + 1 }\r\n    callback = ->(value) { value + 1 }\r\n    processor = proc { |value| value * 2 }\r\n  end\r\nend\r\n";
    let results = extract("src/sample.rb", source);
    assert!(
        results.parse_diagnostics.is_empty(),
        "{:#?}",
        results.parse_diagnostics
    );

    for (name, kind, expected) in [
        ("@callback", SymbolKind::Field, "{ value + 1 }"),
        ("callback", SymbolKind::Variable, "{ value + 1 }"),
        ("processor", SymbolKind::Variable, "{ |value| value * 2 }"),
    ] {
        let closure = symbol(&results, name, kind);
        let body = closure.body_span.expect(name);
        assert_eq!(
            source.get(body.start_byte as usize..body.end_byte as usize),
            Some(expected),
            "{name}"
        );
        assert!(closure.body_hash.is_some(), "{name}");
    }

    for (name, kind) in [
        ("connection", SymbolKind::Variable),
        ("@user", SymbolKind::Field),
    ] {
        let declaration = symbol(&results, name, kind);
        assert_eq!(declaration.body_span, None, "{name}");
        assert_eq!(declaration.body_hash, None, "{name}");
    }
}

#[test]
fn ruby_regex_constants_have_no_delimiter_bodies() {
    let source = "PATTERN = %r{\\d+}\nHANDLER = -> { 1 }\n";
    let results = extract("src/pattern.rb", source);
    assert!(
        results.parse_diagnostics.is_empty(),
        "{:#?}",
        results.parse_diagnostics
    );

    let pattern = symbol(&results, "PATTERN", SymbolKind::Constant);
    assert_eq!(pattern.body_span, None);
    assert_eq!(pattern.body_hash, None);

    let handler = symbol(&results, "HANDLER", SymbolKind::Constant);
    let body = handler.body_span.expect("callable constant body");
    assert_eq!(
        source.get(body.start_byte as usize..body.end_byte as usize),
        Some("{ 1 }")
    );
    assert!(handler.body_hash.is_some());
}

#[test]
fn razor_properties_keep_accessor_bodies_and_callable_locals_keep_their_bodies() {
    let source = "@code {\r\n    public string Title { get; set; }\r\n    public string Label { get => Format(Title); }\r\n    public void Run() {\r\n        Func<int, int> twice = value => value + 1;\r\n    }\r\n}\r\n";
    let results = extract("src/component.razor", source);
    assert!(
        results.parse_diagnostics.is_empty(),
        "{:#?}",
        results.parse_diagnostics
    );

    let title = symbol(&results, "Title", SymbolKind::Property);
    let title_body = title.body_span.expect("Title accessor list");
    assert_eq!(
        source.get(title_body.start_byte as usize..title_body.end_byte as usize),
        Some("{ get; set; }")
    );
    assert!(title.body_hash.is_some());

    let label = symbol(&results, "Label", SymbolKind::Property);
    let label_body = label.body_span.expect("Label getter expression");
    assert_eq!(
        source.get(label_body.start_byte as usize..label_body.end_byte as usize),
        Some("{ get => Format(Title); }")
    );
    assert!(label.body_hash.is_some());

    let twice = symbol(&results, "twice", SymbolKind::Variable);
    let body = twice.body_span.expect("twice");
    assert_eq!(
        source.get(body.start_byte as usize..body.end_byte as usize),
        Some("value + 1")
    );
    assert!(twice.body_hash.is_some());
}

#[test]
fn web_callable_bodies_keep_nested_unicode_crlf_ranges_and_comment_stable_hashes() {
    struct BodyCase<'a> {
        file_path: &'a str,
        source: &'a str,
        outer_name: &'a str,
        inner_name: &'a str,
        outer_body: &'a str,
        inner_body: &'a str,
        marker: &'a str,
        comment: &'a str,
    }

    let cases = [
        BodyCase {
            file_path: "src/widget.ts",
            source: "// λ prefix\r\nfunction outer() {\r\n  function inner() { return \"✓\"; }\r\n  return inner();\r\n}\r\n",
            outer_name: "outer",
            inner_name: "inner",
            outer_body: "{\r\n  function inner() { return \"✓\"; }\r\n  return inner();\r\n}",
            inner_body: "{ return \"✓\"; }",
            marker: "  function inner",
            comment: "  // body edit\r\n",
        },
        BodyCase {
            file_path: "src/widget.tsx",
            source: "// λ prefix\r\nfunction outer() {\r\n  function inner() { return <i>✓</i>; }\r\n  return inner();\r\n}\r\n",
            outer_name: "outer",
            inner_name: "inner",
            outer_body: "{\r\n  function inner() { return <i>✓</i>; }\r\n  return inner();\r\n}",
            inner_body: "{ return <i>✓</i>; }",
            marker: "  function inner",
            comment: "  // body edit\r\n",
        },
        BodyCase {
            file_path: "src/widget.js",
            source: "// λ prefix\r\nfunction outer() {\r\n  function inner() { return \"✓\"; }\r\n  return inner();\r\n}\r\n",
            outer_name: "outer",
            inner_name: "inner",
            outer_body: "{\r\n  function inner() { return \"✓\"; }\r\n  return inner();\r\n}",
            inner_body: "{ return \"✓\"; }",
            marker: "  function inner",
            comment: "  // body edit\r\n",
        },
        BodyCase {
            file_path: "src/widget.jsx",
            source: "// λ prefix\r\nfunction outer() {\r\n  function inner() { return <i>✓</i>; }\r\n  return inner();\r\n}\r\n",
            outer_name: "outer",
            inner_name: "inner",
            outer_body: "{\r\n  function inner() { return <i>✓</i>; }\r\n  return inner();\r\n}",
            inner_body: "{ return <i>✓</i>; }",
            marker: "  function inner",
            comment: "  // body edit\r\n",
        },
        BodyCase {
            file_path: "src/component.vue",
            source: "<!-- λ prefix -->\r\n<script lang=\"ts\">\r\nfunction outer() {\r\n  function inner() { return \"✓\"; }\r\n  return inner();\r\n}\r\n</script>\r\n",
            outer_name: "outer",
            inner_name: "inner",
            outer_body: "{\r\n  function inner() { return \"✓\"; }\r\n  return inner();\r\n}",
            inner_body: "{ return \"✓\"; }",
            marker: "  function inner",
            comment: "  // body edit\r\n",
        },
        BodyCase {
            file_path: "src/component.razor",
            source: "@* λ prefix *@\r\n@code {\r\n    string Outer() {\r\n        string Inner() { return \"✓\"; }\r\n        return Inner();\r\n    }\r\n}\r\n",
            outer_name: "Outer",
            inner_name: "Inner",
            outer_body: "{\r\n        string Inner() { return \"✓\"; }\r\n        return Inner();\r\n    }",
            inner_body: "{ return \"✓\"; }",
            marker: "        string Inner",
            comment: "        // body edit\r\n",
        },
        BodyCase {
            file_path: "src/component.qml",
            source: "// λ prefix\r\nItem {\r\n    function outer() {\r\n        function inner() { return \"✓\"; }\r\n        return inner();\r\n    }\r\n}\r\n",
            outer_name: "outer",
            inner_name: "inner",
            outer_body: "{\r\n        function inner() { return \"✓\"; }\r\n        return inner();\r\n    }",
            inner_body: "{ return \"✓\"; }",
            marker: "        function inner",
            comment: "        // body edit\r\n",
        },
        BodyCase {
            file_path: "src/module.py",
            source: "# λ prefix\r\ndef outer():\r\n    def inner():\r\n        return \"✓\"\r\n    return inner()\r\n",
            outer_name: "outer",
            inner_name: "inner",
            outer_body: "def inner():\r\n        return \"✓\"\r\n    return inner()",
            inner_body: "return \"✓\"",
            marker: "    return inner()",
            comment: "    # body edit\r\n",
        },
        BodyCase {
            file_path: "src/module.php",
            source: "<?php\r\n// λ prefix\r\nfunction outer() {\r\n  function inner() { return \"✓\"; }\r\n  return inner();\r\n}\r\n",
            outer_name: "outer",
            inner_name: "inner",
            outer_body: "{\r\n  function inner() { return \"✓\"; }\r\n  return inner();\r\n}",
            inner_body: "{ return \"✓\"; }",
            marker: "  function inner",
            comment: "  // body edit\r\n",
        },
        BodyCase {
            file_path: "src/module.rb",
            source: "# λ prefix\r\ndef outer()\r\n  def inner()\r\n    \"✓\"\r\n  end\r\n  inner()\r\nend\r\n",
            outer_name: "outer",
            inner_name: "inner",
            outer_body: "def inner()\r\n    \"✓\"\r\n  end\r\n  inner()\r",
            inner_body: "\"✓\"\r",
            marker: "  inner()",
            comment: "  # body edit\r\n",
        },
        BodyCase {
            file_path: "src/module.lua",
            source: "-- λ prefix\r\nfunction outer()\r\n  local function inner() return \"✓\" end\r\n  return inner()\r\nend\r\n",
            outer_name: "outer",
            inner_name: "inner",
            outer_body: "local function inner() return \"✓\" end\r\n  return inner()",
            inner_body: "return \"✓\"",
            marker: "  return inner()",
            comment: "  -- body edit\r\n",
        },
    ];

    for case in cases {
        assert!(case.source.contains("\r\n"), "{}", case.file_path);
        let results = extract(case.file_path, case.source);
        assert!(
            results.parse_diagnostics.is_empty(),
            "{}: {:#?}",
            case.file_path,
            results.parse_diagnostics
        );
        let outer = named_symbol(&results, case.outer_name);
        let inner = named_symbol(&results, case.inner_name);
        let outer_range = assert_body_matches(case.file_path, case.source, outer, case.outer_body);
        let inner_range = assert_body_matches(case.file_path, case.source, inner, case.inner_body);
        assert!(outer_range.start < inner_range.start, "{}", case.file_path);
        assert!(inner_range.end <= outer_range.end, "{}", case.file_path);
        assert!(outer.body_hash.is_some(), "{}", case.file_path);
        assert!(inner.body_hash.is_some(), "{}", case.file_path);

        let replacement = format!("{}{}", case.comment, case.marker);
        assert_eq!(
            case.source.matches(case.marker).count(),
            1,
            "{}",
            case.file_path
        );
        let edited_source = case.source.replacen(case.marker, &replacement, 1);
        let edited_outer_body = case.outer_body.replacen(case.marker, &replacement, 1);
        let edited_results = extract(case.file_path, &edited_source);
        assert!(
            edited_results.parse_diagnostics.is_empty(),
            "{}: {:#?}",
            case.file_path,
            edited_results.parse_diagnostics
        );
        let edited_outer = named_symbol(&edited_results, case.outer_name);
        let edited_inner = named_symbol(&edited_results, case.inner_name);
        assert_body_matches(
            case.file_path,
            &edited_source,
            edited_outer,
            &edited_outer_body,
        );
        assert_body_matches(
            case.file_path,
            &edited_source,
            edited_inner,
            case.inner_body,
        );
        assert_eq!(
            outer.body_hash, edited_outer.body_hash,
            "{}",
            case.file_path
        );
        assert_eq!(
            inner.body_hash, edited_inner.body_hash,
            "{}",
            case.file_path
        );
    }
}

#[test]
fn razor_comment_hashes_ignore_only_ast_comment_ranges() {
    let source = "<a href=http://cdn.example/assets>λ</a>\r\n@code {\r\n    private string Url = \"http://api.example/*literal*/\";\r\n    private string Render() {\r\n        return $\"path {Url /* interpolation comment */,10:C}\";\r\n    }\r\n}\r\n";
    let results = extract("src/Component.razor", source);
    assert!(
        results.parse_diagnostics.is_empty(),
        "{:#?}",
        results.parse_diagnostics
    );
    let component = symbol(&results, "Component", SymbolKind::Class);
    let render = symbol(&results, "Render", SymbolKind::Method);
    let component_body = component.body_span.expect("component body");
    let render_body = render.body_span.expect("Render body");
    assert_eq!(component_body.start_byte, 0);
    assert_eq!(component_body.end_byte as usize, source.len());
    assert_eq!(
        source.get(render_body.start_byte as usize..render_body.end_byte as usize),
        Some("{\r\n        return $\"path {Url /* interpolation comment */,10:C}\";\r\n    }")
    );
    let component_hash = component.body_hash.clone().expect("component body hash");
    let render_hash = render.body_hash.clone().expect("Render body hash");

    let commented_source = source
        .replacen(
            "<a href=",
            "@* λ Razor comment *@\r\n<!-- HTML comment -->\r\n<a href=",
            1,
        )
        .replacen(
            "        return $\"path {Url /* interpolation comment */,10:C}\";",
            "        // C# line comment\r\n        return $\"path { Url /* longer interpolation comment */ ,10:C}\"; /* C# block comment */",
            1,
        );
    let commented = extract("src/Component.razor", &commented_source);
    assert!(
        commented.parse_diagnostics.is_empty(),
        "{:#?}",
        commented.parse_diagnostics
    );
    let commented_component = symbol(&commented, "Component", SymbolKind::Class);
    let commented_render = symbol(&commented, "Render", SymbolKind::Method);
    let commented_component_body = commented_component
        .body_span
        .expect("commented component body");
    let commented_render_body = commented_render.body_span.expect("commented Render body");
    assert_eq!(commented_component_body.start_byte, 0);
    assert_eq!(
        commented_component_body.end_byte as usize,
        commented_source.len()
    );
    assert_eq!(
        commented_source.get(
            commented_render_body.start_byte as usize..commented_render_body.end_byte as usize
        ),
        Some(
            "{\r\n        // C# line comment\r\n        return $\"path { Url /* longer interpolation comment */ ,10:C}\"; /* C# block comment */\r\n    }"
        )
    );
    assert_eq!(
        commented_component.body_hash.as_deref(),
        Some(component_hash.as_str())
    );
    assert_eq!(
        commented_render.body_hash.as_deref(),
        Some(render_hash.as_str())
    );

    let changed_url = source.replace("http://cdn.example/assets", "http://cdn.example/other");
    let changed_url_results = extract("src/Component.razor", &changed_url);
    assert_ne!(
        symbol(&changed_url_results, "Component", SymbolKind::Class).body_hash,
        Some(component_hash.clone())
    );

    let changed_literal = source.replace(
        "http://api.example/*literal*/",
        "http://api.example/*changed*/",
    );
    let changed_literal_results = extract("src/Component.razor", &changed_literal);
    assert_ne!(
        symbol(&changed_literal_results, "Component", SymbolKind::Class).body_hash,
        Some(component_hash)
    );

    let changed_string_whitespace = source.replace("path {Url", "path  {Url");
    let changed_string_whitespace_results =
        extract("src/Component.razor", &changed_string_whitespace);
    assert_ne!(
        symbol(
            &changed_string_whitespace_results,
            "Render",
            SymbolKind::Method
        )
        .body_hash,
        Some(render_hash.clone())
    );

    let changed_formatting = source.replace(
        "Url /* interpolation comment */",
        " Url /* interpolation comment */ ",
    );
    let changed_formatting_results = extract("src/Component.razor", &changed_formatting);
    assert_eq!(
        symbol(&changed_formatting_results, "Render", SymbolKind::Method).body_hash,
        Some(render_hash.clone())
    );

    let changed_format = source.replace(",10:C", ",11:D");
    let changed_format_results = extract("src/Component.razor", &changed_format);
    assert_ne!(
        symbol(&changed_format_results, "Render", SymbolKind::Method).body_hash,
        Some(render_hash.clone())
    );

    let changed_method = source.replace(
        "Url /* interpolation comment */",
        "Url + \"!\" /* interpolation comment */",
    );
    let changed_method_results = extract("src/Component.razor", &changed_method);
    assert_ne!(
        symbol(&changed_method_results, "Render", SymbolKind::Method).body_hash,
        Some(render_hash)
    );
}

fn named_symbol<'a>(results: &'a crate::ExtractionResults, name: &str) -> &'a Symbol {
    results
        .symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap_or_else(|| panic!("missing symbol {name}"))
}

fn assert_body_matches(
    file_path: &str,
    source: &str,
    symbol: &Symbol,
    expected: &str,
) -> std::ops::Range<usize> {
    let body = symbol.body_span.expect(&symbol.name);
    let range = body.start_byte as usize..body.end_byte as usize;
    let expected_start = source.find(expected).expect("expected body source slice");
    let expected_end = expected_start + expected.len();
    assert_eq!(
        range,
        expected_start..expected_end,
        "{file_path}: {}",
        symbol.name
    );
    assert_eq!(
        source.get(range.clone()),
        Some(expected),
        "{file_path}: {}",
        symbol.name
    );
    let line_at = |byte: usize| source[..byte].bytes().filter(|byte| *byte == b'\n').count() + 1;
    assert_eq!(
        body.start_line as usize,
        line_at(range.start),
        "{}",
        file_path
    );
    assert_eq!(body.end_line as usize, line_at(range.end), "{}", file_path);
    assert!(
        range.start > source[..range.start].chars().count(),
        "{}",
        file_path
    );
    range
}

fn assert_decorator_field_is_bodiless(file_path: &str, declaration: &str, kind: SymbolKind) {
    let source = format!("class Widget {{\r\n  @Input()\r\n  {declaration};\r\n}}\r\n");
    let results = extract(file_path, &source);
    assert!(
        results.parse_diagnostics.is_empty(),
        "{:#?}",
        results.parse_diagnostics
    );

    let name = symbol(&results, "name", kind);
    assert_eq!(name.body_span, None, "{file_path}");
    assert_eq!(name.body_hash, None, "{file_path}");
}
