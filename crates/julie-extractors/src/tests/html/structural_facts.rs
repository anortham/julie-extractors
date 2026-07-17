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

fn metadata_number(fact: &StructuralFact, key: &str) -> Option<u64> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_u64())
}

#[test]
fn html_element_signatures_order_non_priority_attributes_deterministically() {
    let source = r#"<div id="list" data-zeta="z" data-testid="panel" data-alpha="a"></div>"#;

    for _ in 0..64 {
        let results = extract(source);
        let signature = results
            .symbols
            .iter()
            .find(|symbol| symbol.name == "div")
            .and_then(|symbol| symbol.signature.as_deref());

        assert_eq!(
            signature,
            Some(r#"<div id="list" data-alpha="a" data-testid="panel" data-zeta="z">"#)
        );
    }
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
    assert_eq!(metadata_str(form, "method_source"), Some("explicit"));
    assert_eq!(metadata_str(form, "action_kind"), Some("static_path"));
    assert_eq!(metadata_str(form, "target_path"), Some("/workers"));
    assert_eq!(metadata_str(form, "name"), Some("workerForm"));
    assert_eq!(metadata_number(form, "control_count"), Some(2));

    let input = facts_with_pattern(&results, "html.form_control.v1")
        .into_iter()
        .find(|fact| metadata_str(fact, "name") == Some("worker"))
        .expect("expected input form-control fact");
    assert_eq!(metadata_str(input, "type"), Some("text"));
    assert_eq!(metadata_bool(input, "required"), Some(true));
    assert_eq!(metadata_str(input, "form_id"), Some("worker-form"));
    assert_eq!(metadata_str(input, "form_name"), Some("workerForm"));
    assert_eq!(metadata_str(input, "form_action"), Some("/workers"));
    assert_eq!(metadata_str(input, "form_method"), Some("post"));

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

#[test]
fn html_landmark_facts_require_semantic_landmark_roles() {
    let source = r#"
<div role="button" id="action">Action</div>
<div role="search" id="site-search">Search</div>
"#;
    let results = extract(source);
    let landmarks = facts_with_pattern(&results, "html.landmark.v1");

    assert_eq!(landmarks.len(), 1, "{landmarks:#?}");
    assert_eq!(metadata_str(landmarks[0], "role"), Some("search"));
    assert_eq!(metadata_str(landmarks[0], "id"), Some("site-search"));
}

#[test]
fn html_form_facts_default_method_and_rich_control_metadata() {
    let source = r#"<!doctype html>
<html>
  <body>
    <form action="/search" id="search-form" name="searchForm"
          ENCTYPE="multipart/form-data" TARGET="_blank" AUTocomplete="off" novalidate>
      <input type="checkbox" name="active" checked disabled>
      <input type="text" name="token" readonly>
      <select name="sort" multiple></select>
    </form>
    <input type="text" name="orphan" form="search-form" id="orphan-field">
  </body>
</html>"#;

    let results = extract(source);

    let form = facts_with_pattern(&results, "html.form.v1")
        .into_iter()
        .find(|fact| metadata_str(fact, "id") == Some("search-form"))
        .expect("expected search form fact");
    assert_eq!(metadata_str(form, "method"), Some("get"));
    assert_eq!(metadata_str(form, "method_source"), Some("default"));
    assert_eq!(metadata_str(form, "action_kind"), Some("static_path"));
    assert_eq!(metadata_str(form, "target_path"), Some("/search"));
    assert_eq!(metadata_str(form, "enctype"), Some("multipart/form-data"));
    assert_eq!(metadata_str(form, "target"), Some("_blank"));
    assert_eq!(metadata_str(form, "autocomplete"), Some("off"));
    assert_eq!(metadata_bool(form, "novalidate"), Some(true));
    assert_eq!(metadata_number(form, "control_count"), Some(3));

    let checkbox = facts_with_pattern(&results, "html.form_control.v1")
        .into_iter()
        .find(|fact| metadata_str(fact, "name") == Some("active"))
        .expect("expected checkbox control");
    assert_eq!(metadata_bool(checkbox, "checked"), Some(true));
    assert_eq!(metadata_bool(checkbox, "disabled"), Some(true));
    assert_eq!(metadata_str(checkbox, "form_method"), Some("get"));

    let readonly_input = facts_with_pattern(&results, "html.form_control.v1")
        .into_iter()
        .find(|fact| metadata_str(fact, "name") == Some("token"))
        .expect("expected readonly input control");
    assert_eq!(metadata_bool(readonly_input, "readonly"), Some(true));

    let select = facts_with_pattern(&results, "html.form_control.v1")
        .into_iter()
        .find(|fact| metadata_str(fact, "name") == Some("sort"))
        .expect("expected select control");
    assert_eq!(metadata_bool(select, "multiple"), Some(true));

    let orphan = facts_with_pattern(&results, "html.form_control.v1")
        .into_iter()
        .find(|fact| metadata_str(fact, "id") == Some("orphan-field"))
        .expect("expected orphan control");
    assert_eq!(metadata_str(orphan, "form_id"), Some("search-form"));
    assert_eq!(metadata_str(orphan, "form_action"), Some("/search"));
    assert_eq!(metadata_str(orphan, "form_method"), Some("get"));
}

#[test]
fn html_forms_emit_generic_data_attribute_facts() {
    let results = extract(r#"<form data-testid="checkout"><input name="email"></form>"#);
    let data_attrs = facts_with_pattern(&results, "html.data_attribute.v1");

    assert_eq!(data_attrs.len(), 1, "{data_attrs:#?}");
    assert_eq!(
        metadata_str(data_attrs[0], "attribute_name"),
        Some("data-testid")
    );
    assert_eq!(metadata_str(data_attrs[0], "value"), Some("checkout"));
    assert_eq!(metadata_str(data_attrs[0], "tag_name"), Some("form"));
}

#[test]
fn html_data_attribute_facts_have_distinct_deterministic_ids() {
    let source = r#"<div data-alpha="a" data-zeta="z"></div>"#;

    let mut previous_order: Option<Vec<String>> = None;
    for _ in 0..64 {
        let results = extract(source);
        let facts = facts_with_pattern(&results, "html.data_attribute.v1");
        assert_eq!(facts.len(), 2, "{facts:#?}");

        let ids = facts
            .iter()
            .map(|fact| fact.id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            ids.len(),
            2,
            "data-* facts must carry distinct ids: {facts:#?}"
        );

        let names = facts
            .iter()
            .map(|fact| metadata_str(fact, "attribute_name").unwrap().to_string())
            .collect::<Vec<_>>();
        if let Some(previous) = &previous_order {
            assert_eq!(&names, previous, "data-* fact order must be deterministic");
        } else {
            previous_order = Some(names.clone());
        }

        let name_set = names.into_iter().collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            name_set,
            std::collections::BTreeSet::from(["data-alpha".to_string(), "data-zeta".to_string()])
        );
    }
}

#[test]
fn html_form_and_role_bearing_elements_emit_landmark_facts() {
    let source = r#"<!doctype html>
<html>
  <body>
    <form role="search" action="/find"><input name="q"></form>
    <img src="/hero.png" role="banner" alt="Hero">
    <section role="tablist">Tabs</section>
  </body>
</html>"#;
    let results = extract(source);

    let forms = facts_with_pattern(&results, "html.form.v1");
    assert!(
        forms
            .iter()
            .any(|fact| metadata_str(fact, "action") == Some("/find")),
        "expected form fact: {forms:#?}"
    );

    let landmarks = facts_with_pattern(&results, "html.landmark.v1");
    assert!(
        landmarks.iter().any(|fact| {
            metadata_str(fact, "tag_name") == Some("form")
                && metadata_str(fact, "role") == Some("search")
        }),
        "expected landmark fact for role-bearing form: {landmarks:#?}"
    );
    assert!(
        landmarks.iter().any(|fact| {
            metadata_str(fact, "tag_name") == Some("img")
                && metadata_str(fact, "role") == Some("banner")
        }),
        "expected landmark fact for role-bearing img: {landmarks:#?}"
    );
    assert!(
        landmarks
            .iter()
            .all(|fact| metadata_str(fact, "role") != Some("tablist")),
        "tablist is not a landmark role: {landmarks:#?}"
    );

    let media = facts_with_pattern(&results, "html.media.v1");
    assert!(
        media
            .iter()
            .any(|fact| metadata_str(fact, "src") == Some("/hero.png")),
        "role-bearing img must still emit its media fact: {media:#?}"
    );
}

#[test]
fn html_style_blocks_emit_css_structural_facts_with_html_language() {
    let source = r#"<!doctype html>
<html>
  <head>
    <style>
      .btn { color: red; }
      @media (min-width: 40rem) { .wide { display: block; } }
    </style>
  </head>
</html>"#;
    let results = extract(source);

    let selector = facts_with_pattern(&results, "css.selector_rule.v1")
        .into_iter()
        .find(|fact| metadata_str(fact, "selector") == Some(".btn"))
        .expect("expected css selector rule fact from html style block");
    assert_eq!(selector.language, "html");
    assert!(selector.start_byte > 0);
    assert!(selector.start_byte < selector.end_byte);

    let media = facts_with_pattern(&results, "css.media_query.v1")
        .into_iter()
        .next()
        .expect("expected css media query fact from html style block");
    assert_eq!(media.language, "html");
}

#[test]
fn html_emits_area_media_landmark_and_data_attribute_facts() {
    let source = r##"<!doctype html>
<html>
  <body>
    <header role="banner" id="top">
      <nav aria-label="Primary">
        <a href="/home">Home</a>
      </nav>
    </header>
    <main>
      <img src="/logo.png" alt="Logo" id="logo">
      <video controls>
        <source src="/clip.mp4" type="video/mp4">
      </video>
      <map name="sites">
        <area href="/workers" shape="rect" coords="0,0,10,10" alt="Workers">
      </map>
      <div data-testid="panel" data-hx-get="/todos" hx-target="#list" x-data="{ open: true }">
        Panel
      </div>
    </main>
    <aside></aside>
    <footer></footer>
  </body>
</html>"##;
    let results = extract(source);

    let area = facts_with_pattern(&results, "html.area_link.v1")
        .into_iter()
        .next()
        .expect("expected area link fact");
    assert_eq!(metadata_str(area, "href"), Some("/workers"));
    assert_eq!(metadata_str(area, "tag_name"), Some("area"));

    let media = facts_with_pattern(&results, "html.media.v1");
    assert!(
        media.iter().any(|fact| {
            metadata_str(fact, "tag_name") == Some("img")
                && metadata_str(fact, "src") == Some("/logo.png")
        }),
        "{media:#?}"
    );
    assert!(
        media.iter().any(|fact| {
            metadata_str(fact, "tag_name") == Some("source")
                && metadata_str(fact, "src") == Some("/clip.mp4")
        }),
        "{media:#?}"
    );

    let landmarks = facts_with_pattern(&results, "html.landmark.v1");
    let landmark_tags = landmarks
        .iter()
        .filter_map(|fact| metadata_str(fact, "tag_name"))
        .collect::<std::collections::BTreeSet<_>>();
    assert!(landmark_tags.contains("header"));
    assert!(landmark_tags.contains("nav"));
    assert!(landmark_tags.contains("main"));
    assert!(landmark_tags.contains("aside"));
    assert!(landmark_tags.contains("footer"));
    assert!(
        landmarks
            .iter()
            .any(|fact| metadata_str(fact, "role") == Some("banner"))
    );

    let data_attrs = facts_with_pattern(&results, "html.data_attribute.v1");
    assert!(
        data_attrs.iter().any(|fact| {
            metadata_str(fact, "attribute_name") == Some("data-testid")
                && metadata_str(fact, "value") == Some("panel")
        }),
        "expected generic data-* fact: {data_attrs:#?}"
    );
    assert!(
        data_attrs
            .iter()
            .all(|fact| metadata_str(fact, "attribute_name") != Some("data-hx-get")),
        "data-hx-* must stay on htmx facts, not html.data_attribute"
    );
}
