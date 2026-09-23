use crate::base::{ExtractionResults, IdentifierKind, RelationshipKind, Symbol, SymbolKind};
use crate::extract_canonical;
use crate::language_policy::classify_literals_by_carrier;
use std::path::Path;

fn extract(code: &str) -> ExtractionResults {
    extract_canonical("site/page.html", code, Path::new("/tmp/test"))
        .expect("canonical HTML extraction must succeed")
}

fn symbol<'a>(result: &'a ExtractionResults, name: &str) -> &'a Symbol {
    result
        .symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap_or_else(|| {
            let names: Vec<&str> = result.symbols.iter().map(|s| s.name.as_str()).collect();
            panic!("symbol {name} missing from {names:?}")
        })
}

fn owner(result: &ExtractionResults, id: Option<&String>) -> Option<String> {
    let id = id?;
    result
        .symbols
        .iter()
        .find(|symbol| &symbol.id == id)
        .map(|symbol| format!("{}@{}", symbol.name, symbol.start_line))
}

fn pending_rows(result: &ExtractionResults) -> Vec<(String, RelationshipKind, Option<String>)> {
    result
        .structured_pending_relationships
        .iter()
        .map(|pending| {
            (
                pending.target.display_name.clone(),
                pending.pending.kind.clone(),
                pending.target.import_context.clone(),
            )
        })
        .collect()
}

fn identifier_rows(result: &ExtractionResults) -> Vec<(String, IdentifierKind, Option<String>)> {
    result
        .identifiers
        .iter()
        .map(|identifier| {
            (
                identifier.name.clone(),
                identifier.kind.clone(),
                owner(result, identifier.containing_symbol_id.as_ref()),
            )
        })
        .collect()
}

fn pending(
    name: &str,
    kind: RelationshipKind,
    context: &str,
) -> (String, RelationshipKind, Option<String>) {
    (name.to_string(), kind, Some(context.to_string()))
}

#[test]
fn navigation_and_embedding_targets_become_structured_pending_rows() {
    let result = extract(
        "<body>\n<a href=\"/about.html\">About</a>\n<map name=\"m\"><area href=\"/map.html\" alt=\"Map\"></map>\n<iframe src=\"/embed/widget.html\" title=\"Widget\"></iframe>\n<object data=\"/docs/manual.pdf\" type=\"application/pdf\"></object>\n<embed src=\"/media/clip.mp4\">\n<form action=\"/todos\" method=\"post\"></form>\n<a href=\"#top\">Top</a> <a href=\"mailto:a@b.c\">Mail</a> <a href=\"https://example.com/\">Out</a>\n</body>\n",
    );
    assert_eq!(
        pending_rows(&result),
        [
            pending(
                "/about.html",
                RelationshipKind::References,
                "html-anchor-href"
            ),
            pending("/map.html", RelationshipKind::References, "html-area-href"),
            pending(
                "/embed/widget.html",
                RelationshipKind::Uses,
                "html-iframe-src"
            ),
            pending(
                "/docs/manual.pdf",
                RelationshipKind::Uses,
                "html-object-data"
            ),
            pending("/media/clip.mp4", RelationshipKind::Uses, "html-embed-src"),
            pending("/todos", RelationshipKind::Calls, "html-form-action"),
        ]
    );
    assert!(
        result.relationships.iter().all(|relationship| result
            .symbols
            .iter()
            .any(|symbol| symbol.id == relationship.to_symbol_id)),
        "relationships must target real symbols: {:#?}",
        result.relationships
    );
}

#[test]
fn pending_rows_come_from_the_element_that_carries_the_attribute() {
    let result = extract(
        "<head>\n  <script src=\"lib/jasmine.js\"></script>\n  <script src=\"lib/jasmine-html.js\"></script>\n  <script src=\"lib/boot0.js\"></script>\n  <link rel=\"modulepreload\" href=\"./app.js\">\n  <link rel=\"stylesheet\" href=\"./site.css\">\n</head>\n",
    );
    let lines: Vec<(u32, Option<String>)> = result
        .structured_pending_relationships
        .iter()
        .map(|pending| {
            (
                pending.pending.line_number,
                owner(&result, Some(&pending.pending.from_symbol_id)),
            )
        })
        .collect();
    assert_eq!(
        lines,
        [
            (2, Some("script@2".to_string())),
            (3, Some("script@3".to_string())),
            (4, Some("script@4".to_string())),
            (5, Some("link@5".to_string())),
            (6, Some("link@6".to_string())),
        ]
    );
}

#[test]
fn element_body_span_is_the_content_between_its_tags() {
    let code = "<div id=\"menu\" x-data=\"{ open: false }\">\n  <button>Toggle</button>\n</div>\n<input name=\"q\" oninput=\"search(this.value)\">\n<main id=\"content\"><p>Hello {{ user.name }}</p></main>\n";
    let result = extract(code);
    let body = |name: &str| {
        symbol(&result, name)
            .body_span
            .map(|span| &code[span.start_byte as usize..span.end_byte as usize])
    };
    assert_eq!(body("div"), Some("\n  <button>Toggle</button>\n"));
    assert_eq!(body("main"), Some("<p>Hello {{ user.name }}</p>"));
    assert_eq!(body("input"), None);
    assert!(symbol(&result, "input").body_hash.is_none());
}

#[test]
fn url_attributes_reach_the_artifact_as_url_literals() {
    let result = extract(
        "<section id=\"worker\">\n  <a href=\"/workers\">Workers</a>\n  <img src=\"/img/logo.png\" alt=\"Logo\">\n  <form action=\"/login\"></form>\n  <button hx-post=\"/items\" data-action=\"run\">Run</button>\n</section>\n",
    );
    let mut literals = result.literals.clone();
    classify_literals_by_carrier(&mut literals);
    let rows: Vec<(&str, &str)> = literals
        .iter()
        .map(|literal| (literal.literal_text.as_str(), literal.kind.as_str()))
        .collect();
    assert_eq!(
        rows,
        [
            ("/workers", "url"),
            ("/img/logo.png", "url"),
            ("/login", "url"),
            ("/items", "url"),
        ]
    );
}

#[test]
fn uppercase_tags_and_attributes_match_like_lowercase() {
    let result = extract(
        "<HTML>\n<HEAD><TITLE>Legacy</TITLE>\n<SCRIPT SRC=\"legacy.js\"></SCRIPT>\n<LINK REL=\"stylesheet\" HREF=\"legacy.css\">\n</HEAD>\n<BODY onLoad=\"init()\">\n<FORM NAME=\"login\" ACTION=\"/login.cgi\" METHOD=\"POST\"><INPUT TYPE=\"text\" NAME=\"user\"></FORM>\n<A HREF=\"page2.html\">Next</A>\n<DIV ID=\"footer\" CLASS=\"foot\">Bye</DIV>\n<svg><linearGradient id=\"g\"></linearGradient></svg>\n</BODY>\n</HTML>\n",
    );
    for name in [
        "html",
        "head",
        "title",
        "body",
        "form",
        "input",
        "a",
        "div",
        "linearGradient",
    ] {
        symbol(&result, name);
    }
    let script = symbol(&result, "script");
    assert_eq!(script.kind, SymbolKind::Import);
    let targets: Vec<String> = pending_rows(&result)
        .into_iter()
        .map(|(target, _, _)| target)
        .collect();
    assert!(targets.contains(&"legacy.js".to_string()), "{targets:?}");
    assert!(targets.contains(&"legacy.css".to_string()), "{targets:?}");
    let members: Vec<String> = identifier_rows(&result)
        .into_iter()
        .filter(|(_, kind, _)| *kind == IdentifierKind::MemberAccess)
        .map(|(name, _, _)| name)
        .collect();
    assert!(members.contains(&"footer".to_string()), "{members:?}");
    assert!(members.contains(&"foot".to_string()), "{members:?}");
}

#[test]
fn angular_bindings_produce_identifiers_inside_their_values() {
    let code = "<app-user-card *ngFor=\"let user of users; trackBy: trackById\" [user]=\"user\" (selected)=\"onSelect(user)\"></app-user-card>\n<button type=\"button\" (click)=\"loadMore()\" [disabled]=\"loading\">More</button>\n<input [(ngModel)]=\"filter\" #filterInput (keyup.enter)=\"search(filterInput.value)\">\n<p *ngIf=\"error\">{{ error.message | uppercase }}</p>\n";
    let result = extract(code);
    let rows: Vec<(String, IdentifierKind)> = result
        .identifiers
        .iter()
        .map(|identifier| (identifier.name.clone(), identifier.kind.clone()))
        .collect();
    for call in ["onSelect", "loadMore", "search"] {
        assert!(
            rows.contains(&(call.to_string(), IdentifierKind::Call)),
            "{call} call missing from {rows:?}"
        );
    }
    for name in [
        "users",
        "trackById",
        "user",
        "loading",
        "filter",
        "error",
        "message",
    ] {
        assert!(
            rows.iter()
                .any(|(row_name, kind)| row_name == name && *kind != IdentifierKind::Call),
            "{name} reference missing from {rows:?}"
        );
    }
    assert!(
        !rows.iter().any(|(name, _)| name == "uppercase"),
        "{rows:?}"
    );
    let users = result
        .identifiers
        .iter()
        .find(|identifier| identifier.name == "users")
        .unwrap();
    assert_eq!(
        &code[users.start_byte as usize..users.end_byte as usize],
        "users"
    );
}

#[test]
fn id_references_are_identifiers_named_by_the_target_id() {
    let code = "<a href=\"#pricing\">Pricing</a>\n<section id=\"pricing\">\n  <label for=\"email\">Email</label>\n  <input id=\"email\" list=\"domains\" aria-describedby=\"email-help email-note\">\n</section>\n<button popovertarget=\"menu-pop\">Menu</button>\n<button hx-get=\"/items\" hx-target=\"#results\">Load</button>\n<svg><use href=\"#icon-close\"></use></svg>\n";
    let result = extract(code);
    let references: Vec<(&str, &str)> = result
        .identifiers
        .iter()
        .filter(|identifier| identifier.kind == IdentifierKind::MemberAccess)
        .map(|identifier| {
            (
                identifier.name.as_str(),
                &code[identifier.start_byte as usize..identifier.end_byte as usize],
            )
        })
        .filter(|(name, text)| name == text)
        .collect();
    assert_eq!(
        references,
        [
            ("pricing", "pricing"),
            ("email", "email"),
            ("domains", "domains"),
            ("email-help", "email-help"),
            ("email-note", "email-note"),
            ("menu-pop", "menu-pop"),
            ("results", "results"),
            ("icon-close", "icon-close"),
        ]
    );
    let label_reference = result
        .identifiers
        .iter()
        .find(|identifier| {
            identifier.name == "email"
                && code[identifier.start_byte as usize..].starts_with("email\">Email")
        })
        .unwrap();
    assert_eq!(
        owner(&result, label_reference.containing_symbol_id.as_ref()).as_deref(),
        Some("label@3")
    );
}

#[test]
fn server_template_statements_become_imports_and_symbols() {
    let result = extract(
        "{% extends \"base.html\" %}\n{% import \"forms.html\" as forms %}\n{% from \"macros.html\" import field %}\n{% block content %}\n  {% include \"partials/order_table.html\" with orders=orders %}\n  <section id=\"orders\"><a href=\"{% url 'orders:detail' order.id %}\">Detail</a></section>\n{% endblock %}\n{% macro input(name, value='') %}<input name=\"{{ name }}\">{% endmacro %}\n",
    );
    let imports: Vec<(String, Option<String>)> = result
        .structured_pending_relationships
        .iter()
        .filter(|pending| pending.pending.kind == RelationshipKind::Imports)
        .map(|pending| {
            (
                pending.target.display_name.clone(),
                pending.target.import_context.clone(),
            )
        })
        .collect();
    assert_eq!(
        imports,
        [
            ("base.html".to_string(), Some("jinja-extends".to_string())),
            ("forms.html".to_string(), Some("jinja-import".to_string())),
            ("macros.html".to_string(), Some("jinja-import".to_string())),
            (
                "partials/order_table.html".to_string(),
                Some("jinja-include".to_string())
            ),
        ]
    );
    let block = symbol(&result, "content");
    assert_eq!(block.kind, SymbolKind::Namespace);
    assert_eq!((block.start_line, block.end_line), (4, 7));
    assert_eq!(
        symbol(&result, "section").parent_id.as_ref(),
        Some(&block.id)
    );
    let macro_symbol = result
        .symbols
        .iter()
        .find(|symbol| symbol.name == "input" && symbol.kind == SymbolKind::Function)
        .expect("macro symbol");
    assert_eq!(
        macro_symbol.signature.as_deref(),
        Some("{% macro input(name, value='') %}")
    );
    assert!(
        !pending_rows(&result)
            .iter()
            .any(|(target, _, _)| target.contains("{%")),
        "templated href must not become a pending target"
    );
}

#[test]
fn attribute_rows_belong_to_the_element_that_carries_them() {
    let result = extract(
        "<section id=\"panel\">\n  <button onclick=\"saveForm(event)\">Save</button>\n  <form onsubmit=\"return validate(this)\"><input name=\"q\" oninput=\"search(this.value)\"></form>\n  <button id=\"greet\">Greet</button>\n  <img src=\"/logo.png\" alt=\"Logo\">\n</section>\n",
    );
    let rows = identifier_rows(&result);
    let find = |name: &str| {
        rows.iter()
            .find(|(row_name, _, _)| row_name == name)
            .and_then(|(_, _, owner)| owner.clone())
    };
    assert_eq!(find("saveForm").as_deref(), Some("button@2"));
    assert_eq!(find("search").as_deref(), Some("input@3"));
    assert_eq!(find("greet").as_deref(), Some("button@4"));
    let media = result
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "html.media.v1")
        .unwrap();
    assert_eq!(
        owner(&result, media.containing_symbol_id.as_ref()).as_deref(),
        Some("img@5")
    );
}

#[test]
fn doctype_yields_one_symbol() {
    let result = extract("<!doctype html>\n<html><body></body></html>\n");
    let doctypes: Vec<&Symbol> = result
        .symbols
        .iter()
        .filter(|symbol| symbol.name == "DOCTYPE")
        .collect();
    assert_eq!(doctypes.len(), 1);
    assert_eq!(doctypes[0].signature.as_deref(), Some("<!doctype html>"));
}

#[test]
fn trailing_commented_out_and_conditional_comments_are_not_docs() {
    let result = extract(
        "<body>\n<header id=\"top\"></header><!-- /header -->\n<nav id=\"menu\"></nav>\n<!-- <div id=\"legacy-banner\">Old promo</div> -->\n<main id=\"content\"></main>\n<!--[if lt IE 9]><script src=\"html5shiv.js\"></script><![endif]-->\n<footer id=\"foot\"></footer>\n<!-- Search form -->\n<form id=\"search\"></form>\n</body>\n",
    );
    assert_eq!(symbol(&result, "nav").doc_comment, None);
    assert_eq!(symbol(&result, "main").doc_comment, None);
    assert_eq!(symbol(&result, "footer").doc_comment, None);
    let form = symbol(&result, "form");
    assert_eq!(form.doc_comment.as_deref(), Some("<!-- Search form -->"));
    let bound_to_form = result
        .source_regions
        .iter()
        .filter(|region| region.kind == crate::base::SourceRegionKind::DocComment)
        .filter(|region| region.containing_symbol_id.as_ref() == Some(&form.id))
        .count();
    assert_eq!(bound_to_form, 1, "{:#?}", result.source_regions);
}

#[test]
fn only_resource_links_import_and_assets_get_facts() {
    let result = extract(
        "<head>\n<link rel=\"canonical\" href=\"https://example.com/page\">\n<link rel=\"preconnect\" href=\"https://fonts.gstatic.com\">\n<link rel=\"modulepreload\" href=\"./app.js\">\n<link rel=\"stylesheet\" href=\"./site.css\">\n</head>\n<body>\n<iframe src=\"/embed/widget.html\"></iframe>\n<object data=\"/docs/manual.pdf\"></object>\n</body>\n",
    );
    let imports: Vec<String> = pending_rows(&result)
        .into_iter()
        .filter(|(_, kind, _)| *kind == RelationshipKind::Imports)
        .map(|(target, _, _)| target)
        .collect();
    assert_eq!(imports, ["./app.js", "./site.css"]);
    let facts: Vec<(&str, String)> = result
        .structural_facts
        .iter()
        .filter(|fact| {
            fact.pattern_id == "html.resource_link.v1" || fact.pattern_id == "html.embed.v1"
        })
        .map(|fact| {
            let metadata = fact.metadata.as_ref().unwrap();
            let target = metadata
                .get("href")
                .or_else(|| metadata.get("src"))
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string();
            (fact.pattern_id.as_str(), target)
        })
        .collect();
    assert_eq!(
        facts,
        [
            (
                "html.resource_link.v1",
                "https://example.com/page".to_string()
            ),
            (
                "html.resource_link.v1",
                "https://fonts.gstatic.com".to_string()
            ),
            ("html.resource_link.v1", "./app.js".to_string()),
            ("html.resource_link.v1", "./site.css".to_string()),
            ("html.embed.v1", "/embed/widget.html".to_string()),
            ("html.embed.v1", "/docs/manual.pdf".to_string()),
        ]
    );
}

#[test]
fn templated_class_and_id_values_emit_only_real_names() {
    let result = extract(
        "<div class=\"navbar-item {% if active_page == 'status' %}is-active{% endif %}\" id=\"langbar-{{ ws.workspace_id }}\">x</div>\n",
    );
    let members: Vec<String> = identifier_rows(&result)
        .into_iter()
        .filter(|(_, kind, _)| *kind == IdentifierKind::MemberAccess)
        .map(|(name, _, _)| name)
        .collect();
    assert_eq!(members, ["navbar-item", "is-active"]);
}

#[test]
fn attribute_values_keep_inner_quotes() {
    let result = extract("<button id=\"greet\" title=\"say 'hello'\">Greet</button>\n");
    let button = symbol(&result, "button");
    assert!(
        button
            .signature
            .as_deref()
            .is_some_and(|signature| signature.contains("title=\"say 'hello'\"")),
        "{:?}",
        button.signature
    );
    let title = button
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("attributes"))
        .and_then(|attributes| attributes.get("title"))
        .and_then(|value| value.as_str());
    assert_eq!(title, Some("say 'hello'"));
}

#[test]
fn babel_scripts_run_through_the_jsx_extractor() {
    let result = extract(
        "<script type=\"text/babel\">\n  function Greeting({ name }) { return <h1>Hello, {name}</h1>; }\n  const App = () => <Greeting name=\"World\" />;\n</script>\n",
    );
    assert_eq!(symbol(&result, "Greeting").kind, SymbolKind::Function);
    symbol(&result, "App");
}
