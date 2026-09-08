use super::{extract, facts_with_pattern, metadata_str};
use std::collections::BTreeSet;

#[test]
fn map_methods_preserves_each_static_verb_group_and_handler() {
    let source = r#"
var group = app.MapGroup("/api");
group.MapMethods("/both", new[] { "GET", "POST" }, HandleBoth);
app.MapMethods("/collection", [HttpMethods.Head, "OPTIONS"], () => "ok");
app.MapHead("/head", HandleHead);
app.MapOptions("/options", HandleOptions);
"#;
    for (path, code) in [
        ("Program.cs", source.to_string()),
        ("View.razor", format!("@code {{ {source} }}")),
    ] {
        let result = extract(path, &code);
        let facts = facts_with_pattern(&result, "aspnet.minimal_api.route.v1");
        assert_eq!(facts.len(), 6, "{path}: {facts:?}");
        let both: Vec<_> = facts
            .iter()
            .filter(|f| metadata_str(f, "route_template") == Some("/both"))
            .collect();
        assert_eq!(
            both.iter()
                .filter_map(|f| metadata_str(f, "verb"))
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["GET", "POST"])
        );
        for fact in both {
            assert_eq!(
                metadata_str(fact, "effective_route_template"),
                Some("/api/both")
            );
            assert_eq!(metadata_str(fact, "handler_name"), Some("HandleBoth"));
        }
        assert_eq!(
            facts.iter().map(|f| &f.id).collect::<BTreeSet<_>>().len(),
            6
        );
    }
}

#[test]
fn map_methods_unknown_methods_remain_unknown_without_guessing_handler() {
    let result = extract(
        "Program.cs",
        r#"
app.MapMethods("/unknown", methods, HandleUnknown);
app.MapMethods("/mixed", new string[] { "GET", method }, HandleMixed);
app.MapMethods("/list", new List<string> { "POST", "POST" }, HandleList);
app.MapMethods("/empty", Array.Empty<string>(), HandleEmpty);
// app.MapMethods("/comment", new[] { "GET" }, Comment);
var text = "app.MapHead(\"/string\", Fake)";
"#,
    );
    let facts = facts_with_pattern(&result, "aspnet.minimal_api.route.v1");
    assert_eq!(facts.len(), 5, "{facts:?}");
    for fact in &facts {
        assert_ne!(metadata_str(fact, "route_template"), Some("/comment"));
        assert_ne!(metadata_str(fact, "route_template"), Some("/string"));
        assert!(
            metadata_str(fact, "handler_name")
                .unwrap()
                .starts_with("Handle")
        );
    }
    let unknown = facts
        .iter()
        .find(|f| metadata_str(f, "route_template") == Some("/unknown"))
        .unwrap();
    assert_eq!(metadata_str(unknown, "verb"), None);
    assert_eq!(metadata_str(unknown, "verb_source"), Some("unknown"));
}

#[test]
fn consumed_attribute_objects_cover_component_languages_without_logging_false_positives() {
    let cases = [
        (
            "View.razor",
            r#"<button @attributes="Attrs" />
@code { private Dictionary<string, object> Attrs => new() { ["hx-post"] = "/save", ["hx-trigger"] = "click" }; private Dictionary<string, object> Log = new() { ["hx-get"] = "/not-consumed" }; }"#,
        ),
        (
            "View.jsx",
            r#"const attrs = { 'hx-post': '/save', 'hx-trigger': 'click' }; const log = { 'hx-get': '/not-consumed' }; export const View = () => <button {...attrs} />;"#,
        ),
        (
            "View.tsx",
            r#"const attrs = { 'hx-post': '/save', 'hx-trigger': 'click' }; const log = { 'hx-get': '/not-consumed' }; export const View = () => <button {...attrs} />;"#,
        ),
        (
            "View.js",
            r#"const attrs = { 'hx-post': '/save', 'hx-trigger': 'click' }; const log = { 'hx-get': '/not-consumed' }; export const View = () => <button {...attrs} />;"#,
        ),
        (
            "View.vue",
            r#"<script setup>const attrs = { 'hx-post': '/save', 'hx-trigger': 'click' }; const log = { 'hx-get': '/not-consumed' };</script><template><button v-bind="attrs" /></template>"#,
        ),
        (
            "View.html",
            r#"<button x-bind="{ 'hx-post': '/save', 'hx-trigger': 'click' }"></button>"#,
        ),
    ];
    for (path, source) in cases {
        let result = extract(path, source);
        let facts = facts_with_pattern(&result, "htmx.attribute.v1");
        assert_eq!(
            facts.len(),
            2,
            "{path}: {facts:?}; symbols {:?}",
            result
                .symbols
                .iter()
                .map(|s| (&s.name, s.start_byte, s.end_byte))
                .collect::<Vec<_>>()
        );
        assert!(
            facts
                .iter()
                .all(|fact| metadata_str(fact, "attribute_value") != Some("/not-consumed"))
        );
        let post = facts
            .iter()
            .find(|fact| metadata_str(fact, "attribute_name") == Some("hx-post"))
            .unwrap();
        assert_eq!(metadata_str(post, "target_path"), Some("/save"));
    }
}

#[test]
fn consumed_attribute_dynamic_values_are_preserved_without_static_route_claim() {
    let result = extract(
        "View.razor",
        r#"<button @attributes="Attrs" />
@code { private Dictionary<string, object>? Attrs => Enabled ? new Dictionary<string, object> { ["hx-get"] = $"/items/{Id}", ["hx-trigger"] = "click" } : null; }"#,
    );
    let facts = facts_with_pattern(&result, "htmx.attribute.v1");
    assert_eq!(facts.len(), 2, "{facts:?}");
    let get = facts
        .iter()
        .find(|f| metadata_str(f, "attribute_name") == Some("hx-get"))
        .unwrap();
    assert_eq!(metadata_str(get, "target_path"), None);
    assert_eq!(
        metadata_str(get, "value_source"),
        Some("dynamic_expression")
    );
}

#[test]
fn consumed_attribute_inline_objects_work_and_comments_strings_stay_silent() {
    for (path, source) in [
        (
            "View.jsx",
            r#"const View = () => <button {...{'hx-get': '/inline'}} />; const text = "<button {...log} />"; const log = {'hx-get': '/fake'};"#,
        ),
        (
            "View.tsx",
            r#"const View = () => <button {...{'hx-get': '/inline'}} />; const text = "<button {...log} />"; const log = {'hx-get': '/fake'};"#,
        ),
        (
            "View.vue",
            r#"<script setup>const log = {'hx-get': '/fake'}; const text = '<button v-bind="log" />';</script><template><button v-bind="{'hx-get': '/inline'}" /><!-- <button v-bind="log" /> --></template>"#,
        ),
    ] {
        let result = extract(path, source);
        let facts = facts_with_pattern(&result, "htmx.attribute.v1");
        assert_eq!(facts.len(), 1, "{path}: {facts:?}");
        assert_eq!(metadata_str(facts[0], "target_path"), Some("/inline"));
    }
}

#[test]
fn consumed_attribute_facts_belong_to_each_markup_use_not_the_shared_dictionary() {
    let result = extract(
        "View.jsx",
        "const attrs = {'hx-get': '/read', 'hx-trigger': 'click'};\nconst One = () => <button {...attrs} />;\nconst Two = () => <button {...attrs} />;\n",
    );
    let facts = facts_with_pattern(&result, "htmx.attribute.v1");
    assert_eq!(facts.len(), 4, "{facts:?}");
    assert_eq!(
        facts.iter().map(|f| f.start_line).collect::<BTreeSet<_>>(),
        BTreeSet::from([2, 3])
    );
    for fact in facts {
        let owner = result
            .symbols
            .iter()
            .find(|s| Some(&s.id) == fact.containing_symbol_id.as_ref())
            .unwrap();
        assert!(matches!(owner.name.as_str(), "One" | "Two"));
    }
}
