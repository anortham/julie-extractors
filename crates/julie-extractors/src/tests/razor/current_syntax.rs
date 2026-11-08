use crate::ExtractionResults;
use serde_json::Value;
use std::path::Path;

fn extract(name: &str, source: &str) -> ExtractionResults {
    crate::pipeline::extract_canonical(
        &format!("{name}.razor"),
        source,
        Path::new("/tmp/current-razor-syntax"),
    )
    .unwrap_or_else(|error| panic!("{name}: canonical Razor extraction failed: {error}"))
}

#[test]
fn current_razor_syntax_valid_cases_have_no_parse_diagnostics() {
    let cases = [
        ("doctype", "<!DOCTYPE html><html><body></body></html>"),
        (
            "qualified component",
            "<BlazorSample.AdminComponents.Pages.ProductDetail />",
        ),
        (
            "void and unquoted",
            "<head><base href=/CoolApp/></head><input disabled>",
        ),
        ("entities", "<p>Tom &amp; Jerry &#x1F63A;</p>"),
        ("single quoted", "<input class='form-control'>"),
        (
            "nested block",
            "<div>@{ var value = 1; }<span>@value</span></div>",
        ),
        (
            "nested conditional field",
            "<div>@if (condition) { @Field(\"x\", value) }</div>",
        ),
        (
            "arrow render fragment",
            r#"@code { private RenderFragment Fragment() => @<text>Hello</text>; }"#,
        ),
        (
            "explicit component attribute",
            r#"<ParameterChild Title="Set by @(panelData.Title)" />"#,
        ),
        ("bare page", "@page\n<h1>Page</h1>"),
        (
            "template",
            "@{ Func<dynamic, object> t = @<p>@item.Name</p>; }",
        ),
        ("escape", "<p>@@ @(DateTime.Now).</p>"),
        (
            "tag helper",
            "@addTagHelper My.TagHelpers.EmailTagHelper, My.Assembly",
        ),
        (
            "comment first in a block",
            "@if (shown)\n{\n    @* c *@\n    <p>a</p>\n}",
        ),
        (
            "comment first in an else block",
            "@if (shown)\n{\n    <p>a</p>\n}\nelse\n{\n    @* c *@\n    <p>b</p>\n}",
        ),
        (
            "comment first in a foreach block",
            "@foreach (var item in items)\n{\n    @* c *@\n    <p>@item</p>\n}",
        ),
        (
            "comment first in an element",
            "<div>\n    @* c *@\n    <p>a</p>\n</div>",
        ),
        (
            "comment first in a code block",
            "@code {\n    @* c *@\n    private int count;\n}",
        ),
        (
            "comment above the directives",
            "@* c *@\n@page \"/items\"\n@inherits LayoutComponentBase\n<p>a</p>",
        ),
        (
            "comment with stars and at signs",
            "@* a * b ** c @ d **@\n<p>a</p>",
        ),
    ];

    for (name, source) in cases {
        let results = extract(name, source);
        assert!(
            results.parse_diagnostics.is_empty(),
            "{name}: {:#?}",
            results.parse_diagnostics
        );
    }
}

#[test]
fn current_razor_syntax_malformed_quote_reports_diagnostic_and_recovers_following_fact() {
    let results = extract(
        "malformed quote recovery",
        "<div class=\"broken></div>\n<RecoveredComponent />",
    );

    assert!(
        !results.parse_diagnostics.is_empty(),
        "malformed input must not be labeled clean"
    );
    assert!(
        results.structural_facts.iter().any(|fact| {
            fact.pattern_id == "blazor.component_reference.v1"
                && fact
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("tag"))
                    .and_then(serde_json::Value::as_str)
                    == Some("RecoveredComponent")
        }),
        "expected following component fact after recovery: {:#?}",
        results.structural_facts
    );
}

#[test]
fn current_razor_syntax_fixture_matches_reviewed_evidence() {
    let source =
        include_str!("../../../../../fixtures/extraction/razor/current-syntax/source.razor");
    let evidence: Value = serde_json::from_str(include_str!(
        "../../../../../fixtures/extraction/razor/current-syntax/evidence.json"
    ))
    .expect("current-syntax evidence must be valid JSON");
    let results = extract("current-syntax/source", source);

    for expected in evidence["symbols"]
        .as_array()
        .expect("evidence symbols must be an array")
    {
        let name = expected["name"]
            .as_str()
            .expect("symbol name must be a string");
        let kind = expected["kind"]
            .as_str()
            .expect("symbol kind must be a string");
        let signature = expected["signature"]
            .as_str()
            .expect("symbol signature must be a string");
        assert!(
            results.symbols.iter().any(|symbol| {
                symbol.name == name
                    && format!("{:?}", symbol.kind).eq_ignore_ascii_case(kind)
                    && symbol.signature.as_deref() == Some(signature)
            }),
            "missing reviewed symbol evidence {expected:#?}"
        );
    }

    for expected in evidence["structural_facts"]
        .as_array()
        .expect("evidence structural_facts must be an array")
    {
        let pattern_id = expected["pattern_id"]
            .as_str()
            .expect("fact pattern_id must be a string");
        let node_kind = expected["node_kind"]
            .as_str()
            .expect("fact node_kind must be a string");
        let metadata = expected["metadata"]
            .as_object()
            .expect("fact metadata must be an object");
        assert!(
            results.structural_facts.iter().any(|fact| {
                fact.pattern_id == pattern_id
                    && fact.node_kind == node_kind
                    && metadata.iter().all(|(key, value)| {
                        fact.metadata.as_ref().and_then(|actual| actual.get(key)) == Some(value)
                    })
            }),
            "missing reviewed structural fact evidence {expected:#?}"
        );
    }

    assert_eq!(
        results.parse_diagnostics.len(),
        evidence["parse_diagnostic_count"]
            .as_u64()
            .expect("parse_diagnostic_count must be an integer") as usize
    );
}

#[test]
fn razor_comment_first_in_a_block_keeps_the_code_members() {
    let results = extract(
        "comment in block keeps members",
        "@page \"/items\"\n\n@if (shown)\n{\n    @* The Razor compiler accepts a comment here. *@\n    <p>@Count</p>\n}\n\n@code {\n    private bool shown;\n    private int Count { get; set; }\n    private async Task LoadAsync() => await Task.Yield();\n    private bool Invalid(string id) => id.Length == 0;\n}\n",
    );

    assert!(
        results.parse_diagnostics.is_empty(),
        "{:#?}",
        results.parse_diagnostics
    );
    for name in ["shown", "Count", "LoadAsync", "Invalid"] {
        assert!(
            results.symbols.iter().any(|symbol| symbol.name == name),
            "missing `{name}` in {:?}",
            results
                .symbols
                .iter()
                .map(|symbol| symbol.name.as_str())
                .collect::<Vec<_>>()
        );
    }
    assert!(
        results
            .symbols
            .iter()
            .any(|symbol| symbol.name == "@page" && symbol.start_line == 1),
        "the directive above the block stays a directive"
    );
}
