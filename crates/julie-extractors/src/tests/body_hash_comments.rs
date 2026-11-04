use crate::base::{Symbol, SymbolKind};
use crate::pipeline::extract_canonical;
use std::path::Path;

const TEMPLATE_TICK: &str = "\u{60}";

struct CommentProbe {
    name: &'static str,
    path: &'static str,
    symbol: &'static str,
    template: &'static str,
    short: &'static str,
    long: &'static str,
}

const INTERPOLATION_PROBES: &[CommentProbe] = &[
    CommentProbe {
        name: "bash",
        path: "render.sh",
        symbol: "render",
        template: r#"render() {
  @@BODY_TOKEN@@ '%s' "$(printf %s value@@COMMENT@@
)" '@@PAYLOAD@@'
}
"#,
        short: " # short comment",
        long: " # long comment whose words must all disappear from normalized body tokens",
    },
    CommentProbe {
        name: "dart",
        path: "render.dart",
        symbol: "render",
        template: r#"String render(String value) {
  final result = "@@PAYLOAD@@ @@DOLLAR@@{@@BODY_TOKEN@@@@COMMENT@@}";
  return result;
}
"#,
        short: "/* short comment */",
        long: "/* long comment whose words must all disappear from normalized body tokens */",
    },
    CommentProbe {
        name: "javascript",
        path: "render.js",
        symbol: "render",
        template: r#"function render(value) {
  const result = @@TICK@@@@PAYLOAD@@ @@DOLLAR@@{@@BODY_TOKEN@@@@COMMENT@@}@@TICK@@;
  return result;
}
"#,
        short: "/* short comment */",
        long: "/* long comment whose words must all disappear from normalized body tokens */",
    },
    CommentProbe {
        name: "jsx",
        path: "render.jsx",
        symbol: "render",
        template: r#"function render(value) {
  const result = @@TICK@@@@PAYLOAD@@ @@DOLLAR@@{@@BODY_TOKEN@@@@COMMENT@@}@@TICK@@;
  return result;
}
"#,
        short: "/* short comment */",
        long: "/* long comment whose words must all disappear from normalized body tokens */",
    },
    CommentProbe {
        name: "kotlin",
        path: "Render.kt",
        symbol: "render",
        template: r#"fun render(value: String): String {
  val result = "@@PAYLOAD@@ @@DOLLAR@@{@@BODY_TOKEN@@@@COMMENT@@}"
  return result
}
"#,
        short: "/* short comment */",
        long: "/* long comment whose words must all disappear from normalized body tokens */",
    },
    CommentProbe {
        name: "powershell interpolation",
        path: "Render.ps1",
        symbol: "Render",
        template: r#"function Render($value) {
  $result = "@@PAYLOAD@@ $(@@BODY_TOKEN@@@@COMMENT@@)"
  return $result
}
"#,
        short: " <# short comment #>",
        long: " <# long comment whose words must all disappear from normalized body tokens #>",
    },
    CommentProbe {
        name: "python",
        path: "render.py",
        symbol: "render",
        template: r#"def render(value):
    result = f"""@@PAYLOAD@@ {@@BODY_TOKEN@@@@COMMENT@@
}"""
    return result
"#,
        short: " # short comment",
        long: " # long comment whose words must all disappear from normalized body tokens",
    },
    CommentProbe {
        name: "qml",
        path: "render.qml",
        symbol: "render",
        template: r#"Item {
  function render(value) {
    var result = @@TICK@@@@PAYLOAD@@ @@DOLLAR@@{@@BODY_TOKEN@@@@COMMENT@@}@@TICK@@
    return result
  }
}
"#,
        short: "/* short comment */",
        long: "/* long comment whose words must all disappear from normalized body tokens */",
    },
    CommentProbe {
        name: "ruby",
        path: "render.rb",
        symbol: "render",
        template: r#"def render(value)
  result = "@@PAYLOAD@@ #{@@BODY_TOKEN@@@@COMMENT@@
}"
  result
end
"#,
        short: " # short comment",
        long: " # long comment whose words must all disappear from normalized body tokens",
    },
    CommentProbe {
        name: "scala",
        path: "Render.scala",
        symbol: "render",
        template: r#"object Probe {
  def render(value: String): String = {
    val result = s"@@PAYLOAD@@ @@DOLLAR@@{@@BODY_TOKEN@@@@COMMENT@@}"
    result
  }
}
"#,
        short: "/* short comment */",
        long: "/* long comment whose words must all disappear from normalized body tokens */",
    },
    CommentProbe {
        name: "swift",
        path: "render.swift",
        symbol: "render",
        template: r#"func render(_ value: String) -> String {
  let result = "@@PAYLOAD@@ \(@@BODY_TOKEN@@@@COMMENT@@)"
  return result
}
"#,
        short: "/* short comment */",
        long: "/* long comment whose words must all disappear from normalized body tokens */",
    },
    CommentProbe {
        name: "tsx",
        path: "render.tsx",
        symbol: "render",
        template: r#"function render(value: string): string {
  const result = @@TICK@@@@PAYLOAD@@ @@DOLLAR@@{@@BODY_TOKEN@@@@COMMENT@@}@@TICK@@;
  return result;
}
"#,
        short: "/* short comment */",
        long: "/* long comment whose words must all disappear from normalized body tokens */",
    },
    CommentProbe {
        name: "typescript",
        path: "render.ts",
        symbol: "render",
        template: r#"function render(value: string): string {
  const result = @@TICK@@@@PAYLOAD@@ @@DOLLAR@@{@@BODY_TOKEN@@@@COMMENT@@}@@TICK@@;
  return result;
}
"#,
        short: "/* short comment */",
        long: "/* long comment whose words must all disappear from normalized body tokens */",
    },
    CommentProbe {
        name: "vue",
        path: "render.vue",
        symbol: "render",
        template: r#"<script>
export function render(value) {
  const result = @@TICK@@@@PAYLOAD@@ @@DOLLAR@@{@@BODY_TOKEN@@@@COMMENT@@}@@TICK@@;
  return result;
}
</script>
"#,
        short: "/* short comment */",
        long: "/* long comment whose words must all disappear from normalized body tokens */",
    },
];

const COMMENT_FORM_PROBES: &[CommentProbe] = &[
    CommentProbe {
        name: "fsharp line",
        path: "render.fs",
        symbol: "render",
        template: r#"let render value =
    let result = value + "!"
    @@COMMENT@@
    result
"#,
        short: "// short comment",
        long: "// long comment whose words must all disappear from normalized body tokens",
    },
    CommentProbe {
        name: "fsharp block",
        path: "render.fs",
        symbol: "render",
        template: r#"let render value =
    let result = value + "!"
    @@COMMENT@@
    result
"#,
        short: "(* short comment *)",
        long: "(* long comment whose words must all disappear from normalized body tokens *)",
    },
    CommentProbe {
        name: "powershell block",
        path: "Render.ps1",
        symbol: "Render",
        template: r#"function Render($value) {
  $result = "$value!"
  @@COMMENT@@
  return $result
}
"#,
        short: "<# short comment #>",
        long: "<# long comment whose words must all disappear from normalized body tokens #>",
    },
    CommentProbe {
        name: "rust nested block",
        path: "render.rs",
        symbol: "render",
        template: r#"fn render(value: &str) -> usize {
    let result = value.len();
    @@COMMENT@@
    result
}
"#,
        short: "/* short /* nested */ comment */",
        long: "/* long nested comment /* inner comment */ whose words must all disappear from normalized body tokens */",
    },
    CommentProbe {
        name: "lua long bracket",
        path: "render.lua",
        symbol: "render",
        template: r#"local function render(value)
  local result = value .. "!"
  @@COMMENT@@
  return result
end
"#,
        short: "--[=[ short comment ]=]",
        long: "--[=[ long comment whose words must all disappear from normalized body tokens
and whose continuation line must be discarded ]=]",
    },
];

#[test]
fn interpolation_comments_preserve_canonical_body_hashes() {
    let failures = INTERPOLATION_PROBES
        .iter()
        .filter_map(|probe| assert_comment_variants_match(probe).err())
        .collect::<Vec<_>>();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn language_specific_comment_forms_preserve_canonical_body_hashes() {
    let failures = COMMENT_FORM_PROBES
        .iter()
        .filter_map(|probe| assert_comment_variants_match(probe).err())
        .collect::<Vec<_>>();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn interpolation_body_hashes_track_code_tokens_and_literal_payloads() {
    let failures = INTERPOLATION_PROBES
        .iter()
        .filter_map(interpolation_change_failure)
        .collect::<Vec<_>>();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn direct_native_extractors_preserve_comment_invariance() {
    let cases = [
        (&INTERPOLATION_PROBES[2], "javascript"),
        (&INTERPOLATION_PROBES[4], "kotlin"),
        (&INTERPOLATION_PROBES[6], "python"),
        (&INTERPOLATION_PROBES[7], "qml"),
        (&COMMENT_FORM_PROBES[0], "fsharp"),
        (&COMMENT_FORM_PROBES[3], "rust"),
        (&COMMENT_FORM_PROBES[4], "lua"),
    ];
    let failures = cases
        .into_iter()
        .filter_map(|(probe, language)| direct_comment_failure(probe, language))
        .collect::<Vec<_>>();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn canonical_body_hash_changes_with_code_tokens_and_literal_payloads() {
    let baseline = r#"fn render(value: &str) -> String {
    format!("label: {value}")
}
"#;
    let changed_code = baseline.replace("{value}", "{input}");
    let changed_literal = baseline.replace("label:", "payload:");

    let baseline_hash = canonical_body_hash("render.rs", "render", baseline).expect("baseline");
    let changed_code_hash =
        canonical_body_hash("render.rs", "render", &changed_code).expect("changed code");
    let changed_literal_hash =
        canonical_body_hash("render.rs", "render", &changed_literal).expect("changed literal");

    assert_ne!(baseline_hash, changed_code_hash);
    assert_ne!(baseline_hash, changed_literal_hash);
}

#[test]
fn nested_lambda_body_inside_unicode_crlf_interpolation_preserves_comments() {
    let template = concat!(
        "function display(value) {\r\n",
        "  const result = @@TICK@@outer @@DOLLAR@@{({ lambda: (input) => {\r\n",
        "    const text = @@TICK@@inner λ @@DOLLAR@@{input@@COMMENT@@}@@TICK@@;\r\n",
        "    return text;\r\n",
        "  } }).lambda(value)}@@TICK@@;\r\n",
        "  return result;\r\n",
        "}\r\n"
    );
    let sources = [
        template.replace("@@COMMENT@@", ""),
        template.replace("@@COMMENT@@", "/* short comment */"),
        template.replace(
            "@@COMMENT@@",
            "/* long comment whose words must all disappear from normalized body tokens */",
        ),
    ]
    .map(|source| {
        source
            .replace("@@TICK@@", TEMPLATE_TICK)
            .replace("@@DOLLAR@@", "$")
    });
    let mut display_hashes = Vec::new();
    let mut lambda_hashes = Vec::new();

    for source in &sources {
        let display = canonical_body_symbol("display.js", "display", source).expect("display");
        let lambda = canonical_body_symbol("display.js", "lambda", source).expect("lambda");
        let body_span = lambda.body_span.expect("lambda body span");
        let lambda_body = source
            .get(body_span.start_byte as usize..body_span.end_byte as usize)
            .expect("lambda body must use UTF-8 source boundaries");
        let lambda_declaration = source
            .get(lambda.start_byte as usize..lambda.end_byte as usize)
            .expect("lambda declaration must use UTF-8 source boundaries");

        assert!(lambda_declaration.contains("lambda: (input) => {"));
        assert!(lambda_body.starts_with("{\r\n"));
        assert!(lambda_body.contains("inner λ "));
        assert!(lambda_body.ends_with('}'));
        display_hashes.push(display.body_hash.expect("display body hash"));
        lambda_hashes.push(lambda.body_hash.expect("lambda body hash"));
    }

    assert!(display_hashes.windows(2).all(|pair| pair[0] == pair[1]));
    assert!(lambda_hashes.windows(2).all(|pair| pair[0] == pair[1]));
}

#[test]
fn comment_markers_and_format_string_whitespace_remain_literal_data() {
    let comment_marker = r#"function render(value) {
  return "https://host/path/* literal */" + value;
}
"#;
    let changed_comment_marker = comment_marker.replace("/* literal */", "ordinary text");
    let format_spacing = r#"fn render(value: &str) -> String {
    format!("item: {value}")
}
"#;
    let changed_format_spacing = format_spacing.replace("item: ", "item:");

    assert_ne!(
        canonical_body_hash("render.js", "render", comment_marker).expect("marker"),
        canonical_body_hash("render.js", "render", &changed_comment_marker)
            .expect("changed marker")
    );
    assert_ne!(
        canonical_body_hash("render.rs", "render", format_spacing).expect("format spacing"),
        canonical_body_hash("render.rs", "render", &changed_format_spacing)
            .expect("changed format spacing")
    );
}

#[test]
fn python_nested_format_replacements_normalize_code_but_keep_format_text() {
    let compact = r#"def render(value, width):
    return f"""head {value:{width}d} tail"""
"#;
    let spaced = r#"def render(value, width):
    return f"""head { value :{ width }d} tail"""
"#;
    let short_comment = r#"def render(value, width):
    return f"""head {value:{width # short comment
}d} tail"""
"#;
    let long_comment = r#"def render(value, width):
    return f"""head {value:{width # long nested format comment whose words must disappear
}d} tail"""
"#;
    let changed_width = compact.replace("width", "precision");
    let changed_format_text = compact.replace("{width}d", "{width}x");
    let changed_literal_spacing = compact.replace("head ", "head  ");

    let compact_hash = canonical_body_hash("render.py", "render", compact).expect("compact");

    assert_eq!(
        compact_hash,
        canonical_body_hash("render.py", "render", spaced).expect("spaced")
    );
    assert_eq!(
        compact_hash,
        canonical_body_hash("render.py", "render", short_comment).expect("short comment")
    );
    assert_eq!(
        compact_hash,
        canonical_body_hash("render.py", "render", long_comment).expect("long comment")
    );
    assert_ne!(
        compact_hash,
        canonical_body_hash("render.py", "render", &changed_width).expect("changed width")
    );
    assert_ne!(
        compact_hash,
        canonical_body_hash("render.py", "render", &changed_format_text)
            .expect("changed format text")
    );
    assert_ne!(
        compact_hash,
        canonical_body_hash("render.py", "render", &changed_literal_spacing)
            .expect("changed literal spacing")
    );
}

#[test]
fn html_embedded_script_comments_preserve_host_body_hashes() {
    let template = "<div id=\"root\"><script>function render(value) { return `value ${value@@COMMENT@@}`; }</script></div>";
    let sources = [
        template.replace("@@COMMENT@@", ""),
        template.replace("@@COMMENT@@", " /* c */"),
        template.replace(
            "@@COMMENT@@",
            " /* long interpolation comment whose words must disappear */",
        ),
    ];
    let mut render_hashes = Vec::new();
    let mut root_hashes = Vec::new();

    for source in &sources {
        let result = extract_canonical("page.html", source, Path::new("/repo"))
            .expect("HTML canonical extraction");
        assert!(
            result.parse_diagnostics.is_empty(),
            "HTML parse diagnostics: {:?}",
            result.parse_diagnostics
        );

        let render = result
            .symbols
            .iter()
            .find(|symbol| {
                symbol.name == "render"
                    && symbol.language == "javascript"
                    && matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method)
            })
            .expect("embedded script callable");
        let render_span = render.body_span.expect("render body span");
        let render_body = source
            .get(render_span.start_byte as usize..render_span.end_byte as usize)
            .expect("render span must use host UTF-8 coordinates");
        assert!(!render_body.is_empty());
        let script_range = source.find("<script>").expect("script start") + "<script>".len()
            ..source.find("</script>").expect("script end");
        assert!(
            script_range.start <= render_span.start_byte as usize
                && render_span.end_byte as usize <= script_range.end,
            "render body span must stay inside the host script text"
        );
        render_hashes.push(render.body_hash.clone().expect("render body hash"));

        let root = result
            .symbols
            .iter()
            .find(|symbol| {
                symbol.language == "html"
                    && symbol
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("tagName"))
                        .and_then(serde_json::Value::as_str)
                        == Some("div")
                    && symbol
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("attributes"))
                        .and_then(|attributes| attributes.get("id"))
                        .and_then(serde_json::Value::as_str)
                        == Some("root")
            })
            .expect("root div element");
        let root_span = root.body_span.expect("root div body span");
        let root_body = source
            .get(root_span.start_byte as usize..root_span.end_byte as usize)
            .expect("root body span must use host UTF-8 coordinates");
        assert!(!root_body.is_empty());
        root_hashes.push(root.body_hash.clone().expect("root div body hash"));
    }

    assert!(render_hashes.windows(2).all(|pair| pair[0] == pair[1]));
    assert!(
        root_hashes.windows(2).all(|pair| pair[0] == pair[1]),
        "root element hashes changed across interpolation comments: {root_hashes:?}"
    );

    let changed_literal = sources[0].replace("return `value ", "return `payload ");
    let changed = extract_canonical("page.html", &changed_literal, Path::new("/repo"))
        .expect("changed HTML canonical extraction");
    assert!(changed.parse_diagnostics.is_empty());
    let changed_render = changed
        .symbols
        .iter()
        .find(|symbol| symbol.name == "render" && symbol.language == "javascript")
        .expect("changed embedded script callable");
    assert_ne!(
        render_hashes[0],
        changed_render
            .body_hash
            .clone()
            .expect("changed render hash")
    );
    let changed_root = changed
        .symbols
        .iter()
        .find(|symbol| {
            symbol.language == "html"
                && symbol
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("tagName"))
                    .and_then(serde_json::Value::as_str)
                    == Some("div")
                && symbol
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("attributes"))
                    .and_then(|attributes| attributes.get("id"))
                    .and_then(serde_json::Value::as_str)
                    == Some("root")
        })
        .expect("changed root div element");
    assert_ne!(
        root_hashes[0],
        changed_root
            .body_hash
            .clone()
            .expect("changed root div hash")
    );
}

#[test]
fn razor_literal_text_preserves_component_and_method_body_hashes() {
    let template = "<p>it's literal text</p>@code { string Render(int value) { return $\"{value@@COMMENT@@}\"; } }";
    let sources = [
        template.replace("@@COMMENT@@", ""),
        template.replace("@@COMMENT@@", " /* c */"),
        template.replace(
            "@@COMMENT@@",
            " /* long interpolation comment whose words must disappear */",
        ),
    ];
    let mut component_hashes = Vec::new();
    let mut method_hashes = Vec::new();

    for source in &sources {
        let result = extract_canonical("Page.razor", source, Path::new("/repo"))
            .expect("Razor canonical extraction");
        assert!(
            result.parse_diagnostics.is_empty(),
            "Razor parse diagnostics: {:?}",
            result.parse_diagnostics
        );

        let component = result
            .symbols
            .iter()
            .find(|symbol| {
                symbol.name == "Page"
                    && symbol.kind == SymbolKind::Class
                    && symbol.parent_id.is_none()
            })
            .expect("whole-file Razor component class");
        let component_span = component.body_span.expect("component body span");
        let component_body = source
            .get(component_span.start_byte as usize..component_span.end_byte as usize)
            .expect("component body span must use host UTF-8 coordinates");
        assert!(!component_body.is_empty());
        component_hashes.push(component.body_hash.clone().expect("component body hash"));

        let method = result
            .symbols
            .iter()
            .find(|symbol| symbol.name == "Render" && symbol.kind == SymbolKind::Method)
            .expect("Razor Render method");
        let method_span = method.body_span.expect("Render body span");
        let method_body = source
            .get(method_span.start_byte as usize..method_span.end_byte as usize)
            .expect("method body span must use host UTF-8 coordinates");
        assert!(!method_body.is_empty());
        method_hashes.push(method.body_hash.clone().expect("Render body hash"));
    }

    assert!(method_hashes.windows(2).all(|pair| pair[0] == pair[1]));
    assert!(
        component_hashes.windows(2).all(|pair| pair[0] == pair[1]),
        "Razor component hashes changed across C# interpolation comments: {component_hashes:?}"
    );

    let changed_text = sources[0].replace("it's literal text", "it's updated literal text");
    let changed = extract_canonical("Page.razor", &changed_text, Path::new("/repo"))
        .expect("changed Razor canonical extraction");
    assert!(changed.parse_diagnostics.is_empty());
    let changed_component = changed
        .symbols
        .iter()
        .find(|symbol| symbol.name == "Page" && symbol.kind == SymbolKind::Class)
        .expect("changed whole-file Razor component class");
    assert_ne!(
        component_hashes[0],
        changed_component
            .body_hash
            .clone()
            .expect("changed component body hash")
    );
}

#[test]
fn rust_labelled_loop_comments_preserve_body_hashes() {
    let template = "fn render() -> i32 { 'outer: loop { @@COMMENT@@ break 'outer 1; } }";
    let sources = [
        template.replace("@@COMMENT@@", ""),
        template.replace("@@COMMENT@@", "/* c */"),
        template.replace(
            "@@COMMENT@@",
            "/* long labelled-loop comment whose words must disappear */",
        ),
    ];
    let hashes = sources
        .iter()
        .map(|source| canonical_body_hash("render.rs", "render", source).expect("render"))
        .collect::<Vec<_>>();

    assert!(
        hashes.windows(2).all(|pair| pair[0] == pair[1]),
        "Rust labelled-loop hashes changed across comments: {hashes:?}"
    );

    let changed_label = sources[0].replace("'outer", "'inner");
    let changed_value = sources[0].replace("'outer 1", "'outer 2");
    assert_ne!(
        hashes[0],
        canonical_body_hash("render.rs", "render", &changed_label).expect("changed label")
    );
    assert_ne!(
        hashes[0],
        canonical_body_hash("render.rs", "render", &changed_value).expect("changed value")
    );
}

fn assert_comment_variants_match(probe: &CommentProbe) -> Result<(), String> {
    let baseline_source = render_source(probe, "", "value", body_token(probe, false));
    let short_source = render_source(probe, probe.short, "value", body_token(probe, false));
    let long_source = render_source(probe, probe.long, "value", body_token(probe, false));

    let baseline_hash = canonical_body_hash(probe.path, probe.symbol, &baseline_source)
        .map_err(|error| format!("{} baseline: {error}", probe.name))?;
    let short_hash = canonical_body_hash(probe.path, probe.symbol, &short_source)
        .map_err(|error| format!("{} short: {error}", probe.name))?;
    let long_hash = canonical_body_hash(probe.path, probe.symbol, &long_source)
        .map_err(|error| format!("{} long: {error}", probe.name))?;
    if baseline_hash != short_hash || baseline_hash != long_hash {
        return Err(format!(
            "{} changed across comment variants: {baseline_hash} {short_hash} {long_hash}",
            probe.name
        ));
    }
    Ok(())
}

fn direct_comment_failure(probe: &CommentProbe, language: &str) -> Option<String> {
    let hashes = [
        ("baseline", ""),
        ("short", probe.short),
        ("long", probe.long),
    ]
    .into_iter()
    .map(|(variant, comment)| {
        let source = render_source(probe, comment, "value", body_token(probe, false));
        direct_native_body_hash(language, probe.path, probe.symbol, &source)
            .map_err(|error| format!("{} {variant}: {error}", probe.name))
    })
    .collect::<Result<Vec<_>, _>>();
    match hashes {
        Ok(hashes) if hashes.windows(2).all(|pair| pair[0] == pair[1]) => None,
        Ok(hashes) => Some(format!("{} direct hashes differ: {hashes:?}", probe.name)),
        Err(error) => Some(error),
    }
}

fn interpolation_change_failure(probe: &CommentProbe) -> Option<String> {
    let baseline_source = render_source(probe, "", "value", body_token(probe, false));
    let changed_code_source = render_source(probe, "", "value", body_token(probe, true));
    let changed_literal_source = render_source(probe, "", "payload", body_token(probe, false));
    let baseline_hash = match canonical_body_hash(probe.path, probe.symbol, &baseline_source) {
        Ok(hash) => hash,
        Err(error) => return Some(format!("{} baseline: {error}", probe.name)),
    };
    let changed_code_hash =
        match canonical_body_hash(probe.path, probe.symbol, &changed_code_source) {
            Ok(hash) => hash,
            Err(error) => return Some(format!("{} changed code: {error}", probe.name)),
        };
    let changed_literal_hash =
        match canonical_body_hash(probe.path, probe.symbol, &changed_literal_source) {
            Ok(hash) => hash,
            Err(error) => return Some(format!("{} changed literal: {error}", probe.name)),
        };
    if baseline_hash == changed_code_hash || baseline_hash == changed_literal_hash {
        return Some(format!(
            "{} did not track code/literal changes: {baseline_hash} {changed_code_hash} {changed_literal_hash}",
            probe.name
        ));
    }
    None
}

fn direct_native_body_hash(
    language: &str,
    path: &str,
    name: &str,
    source: &str,
) -> Result<String, String> {
    let parser_language = crate::language::get_tree_sitter_language(language)
        .map_err(|error| format!("{path}: {error}"))?;
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&parser_language)
        .map_err(|error| format!("{path}: {error}"))?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| format!("{path}: parser returned no tree"))?;
    if tree.root_node().has_error() {
        return Err(format!("{path}: native parse tree contains errors"));
    }
    let symbols = match language {
        "javascript" => {
            let mut extractor = crate::javascript::JavaScriptExtractor::new(
                language.to_string(),
                path.to_string(),
                source.to_string(),
                Path::new("/repo"),
            );
            extractor.extract_symbols(&tree)
        }
        "python" => {
            let mut extractor = crate::python::PythonExtractor::new(
                path.to_string(),
                source.to_string(),
                Path::new("/repo"),
            );
            extractor.extract_symbols(&tree)
        }
        "rust" => {
            let mut extractor = crate::rust::RustExtractor::new(
                language.to_string(),
                path.to_string(),
                source.to_string(),
                Path::new("/repo"),
            );
            extractor.extract_symbols(&tree)
        }
        "fsharp" => {
            let mut extractor = crate::fsharp::FSharpExtractor::new(
                language.to_string(),
                path.to_string(),
                source.to_string(),
                Path::new("/repo"),
            );
            extractor.extract_symbols(&tree)
        }
        "lua" => {
            let mut extractor = crate::lua::LuaExtractor::new(
                language.to_string(),
                path.to_string(),
                source.to_string(),
                Path::new("/repo"),
            );
            extractor.extract_symbols(&tree)
        }
        "kotlin" => {
            let mut extractor = crate::kotlin::KotlinExtractor::new(
                language.to_string(),
                path.to_string(),
                source.to_string(),
                Path::new("/repo"),
            );
            extractor.extract_symbols(&tree)
        }
        "qml" => {
            let mut extractor = crate::qml::QmlExtractor::new(
                language.to_string(),
                path.to_string(),
                source.to_string(),
                Path::new("/repo"),
            );
            extractor.extract_symbols(&tree)
        }
        other => return Err(format!("{path}: no native extractor for {other}")),
    };
    let symbol = checked_body_symbol(&symbols, path, name, source)?;
    Ok(symbol.body_hash.expect("checked symbol has body hash"))
}

fn body_token(probe: &CommentProbe, changed: bool) -> &'static str {
    match (probe.name, changed) {
        ("bash", false) => "printf",
        ("bash", true) => "echo",
        ("powershell interpolation", false) => "$value",
        ("powershell interpolation", true) => "$input",
        (_, false) => "value",
        (_, true) => "input",
    }
}

fn render_source(probe: &CommentProbe, comment: &str, payload: &str, body_token: &str) -> String {
    probe
        .template
        .replace("@@COMMENT@@", comment)
        .replace("@@PAYLOAD@@", payload)
        .replace("@@BODY_TOKEN@@", body_token)
        .replace("@@TICK@@", TEMPLATE_TICK)
        .replace("@@DOLLAR@@", "$")
}

fn canonical_body_hash(path: &str, name: &str, source: &str) -> Result<String, String> {
    let symbol = canonical_body_symbol(path, name, source)?;
    Ok(symbol.body_hash.expect("checked symbol has body hash"))
}

fn canonical_body_symbol(path: &str, name: &str, source: &str) -> Result<Symbol, String> {
    let result = extract_canonical(path, source, Path::new("/repo"))
        .map_err(|error| format!("{path}: {error}"))?;
    if !result.parse_diagnostics.is_empty() {
        return Err(format!(
            "{path} parse diagnostics: {:?}",
            result.parse_diagnostics
        ));
    }
    checked_body_symbol(&result.symbols, path, name, source)
}

fn checked_body_symbol(
    symbols: &[Symbol],
    path: &str,
    name: &str,
    source: &str,
) -> Result<Symbol, String> {
    let symbol = symbols
        .iter()
        .find(|symbol| {
            symbol.name == name && matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method)
        })
        .ok_or_else(|| format!("{path}: missing callable {name}"))?;
    let body_span = symbol
        .body_span
        .ok_or_else(|| format!("{path}: callable has no body span"))?;
    if body_span.start_byte >= body_span.end_byte {
        return Err(format!("{path}: empty body span"));
    }
    let body_source = source
        .get(body_span.start_byte as usize..body_span.end_byte as usize)
        .ok_or_else(|| format!("{path}: body span is not on UTF-8 boundaries"))?;
    if body_source.is_empty() {
        return Err(format!("{path}: empty body source"));
    }
    let body_hash = symbol
        .body_hash
        .as_deref()
        .ok_or_else(|| format!("{path}: callable has no body hash"))?;
    if body_hash.is_empty() {
        return Err(format!("{path}: empty body hash"));
    }
    Ok(symbol.clone())
}
