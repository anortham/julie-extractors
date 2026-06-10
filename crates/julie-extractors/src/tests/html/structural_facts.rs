use std::path::Path;

use crate::base::StructuralFact;

fn extract(source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical("source.html", source, Path::new("/repo"))
        .expect("canonical HTML extraction should succeed")
}

fn facts_with_pattern<'a>(
    results: &'a crate::ExtractionResults,
    pattern_id: &str,
) -> Vec<&'a StructuralFact> {
    results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == pattern_id)
        .collect()
}

fn metadata_str<'a>(fact: &'a StructuralFact, key: &str) -> Option<&'a str> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_str())
}

fn metadata_bool(fact: &StructuralFact, key: &str) -> Option<bool> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_bool())
}

#[test]
fn html_emits_link_script_and_form_control_structural_facts() {
    let source = r#"<!doctype html>
<html>
  <body>
    <a href="/workers" id="workers-link" class="nav">Workers</a>
    <form action="/workers" method="post" id="worker-form" name="workerForm">
      <input type="text" name="worker" required>
      <button type="button" data-action="run" id="run-btn">Run</button>
    </form>
    <script src="/app.js" type="module"></script>
    <script>function helper() { return 1; }</script>
  </body>
</html>"#;

    let results = extract(source);

    let link = facts_with_pattern(&results, "html.link.v1")
        .into_iter()
        .find(|fact| metadata_str(fact, "href") == Some("/workers"))
        .expect("expected link fact");
    assert_eq!(metadata_str(link, "tag_name"), Some("a"));
    assert_eq!(metadata_str(link, "id"), Some("workers-link"));
    assert_eq!(metadata_str(link, "class"), Some("nav"));

    let external_script = facts_with_pattern(&results, "html.script.v1")
        .into_iter()
        .find(|fact| metadata_str(fact, "src") == Some("/app.js"))
        .expect("expected external script fact");
    assert_eq!(metadata_str(external_script, "type"), Some("module"));
    assert_eq!(metadata_bool(external_script, "inline"), Some(false));

    let inline_script = facts_with_pattern(&results, "html.script.v1")
        .into_iter()
        .find(|fact| metadata_bool(fact, "inline") == Some(true))
        .expect("expected inline script fact");

    let form = facts_with_pattern(&results, "html.form.v1")
        .into_iter()
        .find(|fact| metadata_str(fact, "id") == Some("worker-form"))
        .expect("expected form fact");
    assert_eq!(metadata_str(form, "action"), Some("/workers"));
    assert_eq!(metadata_str(form, "method"), Some("post"));
    assert_eq!(metadata_str(form, "name"), Some("workerForm"));

    let input = facts_with_pattern(&results, "html.form_control.v1")
        .into_iter()
        .find(|fact| metadata_str(fact, "name") == Some("worker"))
        .expect("expected input form-control fact");
    assert_eq!(metadata_str(input, "type"), Some("text"));
    assert_eq!(metadata_bool(input, "required"), Some(true));

    let button = facts_with_pattern(&results, "html.form_control.v1")
        .into_iter()
        .find(|fact| metadata_str(fact, "id") == Some("run-btn"))
        .expect("expected button form-control fact");
    assert_eq!(metadata_str(button, "tag_name"), Some("button"));
    assert_eq!(metadata_str(button, "type"), Some("button"));

    assert!(link.start_byte < link.end_byte);
    assert!(external_script.start_byte < external_script.end_byte);
    assert!(inline_script.start_byte < inline_script.end_byte);
    assert!(form.start_byte < form.end_byte);
}

#[test]
fn html_structural_facts_normalize_case_insensitive_tags_and_attributes() {
    let source = r#"<A HREF="/upper" ID="upper-link">Upper</A>"#;
    let results = extract(source);

    let link = facts_with_pattern(&results, "html.link.v1")
        .into_iter()
        .next()
        .expect("expected uppercase link fact");
    assert_eq!(metadata_str(link, "tag_name"), Some("a"));
    assert_eq!(metadata_str(link, "href"), Some("/upper"));
    assert_eq!(metadata_str(link, "id"), Some("upper-link"));
}
