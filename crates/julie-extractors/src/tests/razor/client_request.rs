use std::path::Path;

use crate::base::StructuralFact;
use crate::tests::helpers::metadata_str;

const PATTERN_ID: &str = "http.client_request.v1";

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical extraction should succeed")
}

fn client_requests(results: &crate::ExtractionResults) -> Vec<&StructuralFact> {
    results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == PATTERN_ID)
        .collect()
}

#[test]
fn razor_code_httpclient_call_emits_with_absolute_source_span() {
    let source = r#"@page "/users"
@inject HttpClient Http

<h1>👋 Users</h1>

@code {
    private async Task Load()
    {
        var user = await Http.GetFromJsonAsync<Foo>("/api/foo");
    }
}
"#;

    let results = extract("Pages/Users.razor", source);
    let facts = client_requests(&results);
    assert_eq!(facts.len(), 1, "{facts:#?}");
    let fact = facts[0];
    assert_eq!(metadata_str(fact, "client"), Some("httpclient"));
    assert_eq!(metadata_str(fact, "verb"), Some("GET"));
    assert_eq!(metadata_str(fact, "target_path"), Some("/api/foo"));
    assert_eq!(metadata_str(fact, "url_kind"), Some("path"));
    assert_eq!(metadata_str(fact, "verb_source"), Some("attested"));
    assert_eq!(
        &source[fact.start_byte as usize..fact.end_byte as usize],
        "GetFromJsonAsync<Foo>(\"/api/foo\")"
    );
}

#[test]
fn razor_functions_supports_existing_httpclient_method_families() {
    let source = r#"@inject HttpClient Api

@functions {
    private async Task Save()
    {
        await Api.PostAsJsonAsync("https://api.example.com/items", payload);
        await Api.DeleteAsync("/api/items/1");
    }
}
"#;

    let results = extract("Pages/Editor.razor", source);
    let facts = client_requests(&results);
    assert_eq!(facts.len(), 2, "{facts:#?}");
    assert!(facts.iter().any(|fact| {
        metadata_str(fact, "verb") == Some("POST")
            && metadata_str(fact, "target_path") == Some("https://api.example.com/items")
    }));
    assert!(facts.iter().any(|fact| {
        metadata_str(fact, "verb") == Some("DELETE")
            && metadata_str(fact, "target_path") == Some("/api/items/1")
    }));
}

#[test]
fn razor_httpclient_text_outside_code_and_in_non_code_regions_stays_silent() {
    let source = r#"@inject HttpClient Http

Http.GetAsync("/markup")
<div title='Http.GetAsync("/attribute")'>
    Http.GetAsync("/element")
</div>
@("Http.GetAsync(\"/razor-string\")")
@* Http.GetAsync("/razor-comment") *@
<!-- Http.GetAsync("/html-comment") -->

@code {
    private const string Example = "Http.GetAsync(\"/csharp-string\")";
    // Http.GetAsync("/line-comment");
    /* Http.GetAsync("/block-comment"); */
}
"#;

    let results = extract("Pages/Quiet.razor", source);
    let facts = client_requests(&results);
    assert!(facts.is_empty(), "{facts:#?}");
}

#[test]
fn razor_bare_block_and_unproven_receivers_stay_silent() {
    let source = r#"@inject HttpClient Http

<p>HttpClient Cache</p>

@{
    await Http.GetAsync("/bare-block");
}

@code {
    await Cache.GetAsync("/cache");
}
"#;

    let results = extract("Pages/Quiet.razor", source);
    let facts = client_requests(&results);
    assert!(facts.is_empty(), "{facts:#?}");
}

#[test]
fn csharp_httpclient_path_remains_single_emission() {
    let source = r#"using System.Net.Http;

public sealed class Api
{
    public async Task Load(HttpClient client)
    {
        await client.GetAsync("/api/users");
    }
}
"#;

    let results = extract("src/Api.cs", source);
    let facts = client_requests(&results);
    assert_eq!(facts.len(), 1, "{facts:#?}");
    assert_eq!(metadata_str(facts[0], "target_path"), Some("/api/users"));
}
