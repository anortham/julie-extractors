use std::collections::HashMap;

use serde_json::{Number, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParamFlavor {
    Colon,
    Braces,
    AngleBrackets,
    BracesWithDots,
    GinWildcard,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NormalizedTemplate {
    pub template: String,
    pub dynamic_segments: Vec<String>,
}

pub(crate) fn normalize_route_template(template: &str, flavor: ParamFlavor) -> NormalizedTemplate {
    let mut input = template.trim();
    if let Some(stripped) = input.strip_prefix("~/") {
        input = stripped;
    } else {
        input = input.trim_start_matches('/');
    }

    let normalized = match flavor {
        ParamFlavor::Colon => normalize_colon_template(input),
        ParamFlavor::Braces => normalize_brace_template(input),
        ParamFlavor::AngleBrackets => normalize_angle_template(input),
        ParamFlavor::BracesWithDots => normalize_braces_with_dots_template(input),
        ParamFlavor::GinWildcard => normalize_gin_wildcard_template(input),
    };

    NormalizedTemplate {
        template: format!("/{}", normalized.template),
        dynamic_segments: normalized.dynamic_segments,
    }
}

pub(crate) fn classify_url(literal: &str) -> &'static str {
    if literal.starts_with('/') {
        "path"
    } else if literal.contains("://") {
        "absolute"
    } else {
        "relative"
    }
}

pub(crate) fn join_route_templates(prefix: &str, route_template: &str) -> String {
    match (prefix.ends_with('/'), route_template.starts_with('/')) {
        (true, true) => format!("{}{}", prefix.trim_end_matches('/'), route_template),
        (false, false) => format!("{prefix}/{route_template}"),
        _ => format!("{prefix}{route_template}"),
    }
}

pub(crate) fn client_request_metadata(
    client: &str,
    target_path: &str,
    verb: &str,
    verb_source: &str,
    import_source: Option<&str>,
) -> HashMap<String, Value> {
    let mut metadata = HashMap::from([
        (
            "pattern_version".to_string(),
            Value::Number(Number::from(1)),
        ),
        (
            "query_family".to_string(),
            Value::String("web.http_client".to_string()),
        ),
        ("framework".to_string(), Value::String(client.to_string())),
        ("client".to_string(), Value::String(client.to_string())),
        (
            "target_path".to_string(),
            Value::String(target_path.to_string()),
        ),
        (
            "url_kind".to_string(),
            Value::String(classify_url(target_path).to_string()),
        ),
        ("verb".to_string(), Value::String(verb.to_string())),
        (
            "verb_source".to_string(),
            Value::String(verb_source.to_string()),
        ),
    ]);
    if let Some(import_source) = import_source {
        metadata.insert(
            "import_source".to_string(),
            Value::String(import_source.to_string()),
        );
    }
    metadata
}

fn normalize_colon_template(input: &str) -> NormalizedTemplate {
    let mut dynamic_segments = Vec::new();
    let template = input
        .split('/')
        .map(|segment| {
            collect_colon_dynamic_segments(segment, &mut dynamic_segments);
            segment.to_string()
        })
        .collect::<Vec<_>>()
        .join("/");
    NormalizedTemplate {
        template,
        dynamic_segments,
    }
}

fn collect_colon_dynamic_segments(segment: &str, dynamic_segments: &mut Vec<String>) {
    let bytes = segment.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] != b':' {
            cursor += 1;
            continue;
        }
        let name_start = cursor + 1;
        let mut name_end = name_start;
        while name_end < bytes.len()
            && (bytes[name_end].is_ascii_alphanumeric() || matches!(bytes[name_end], b'_' | b'$'))
        {
            name_end += 1;
        }
        if name_end > name_start {
            dynamic_segments.push(segment[name_start..name_end].to_string());
        }
        cursor = name_end.saturating_add(1);
    }
}

fn normalize_brace_template(input: &str) -> NormalizedTemplate {
    replace_delimited_params(input, '{', '}', |inner| {
        let mut name = inner.trim();
        name = name.trim_start_matches('*');
        name = name.trim_end_matches('?');
        if let Some((before, _)) = name.split_once(':') {
            name = before.trim();
        }
        (!name.is_empty()).then(|| name.to_string())
    })
}

fn normalize_angle_template(input: &str) -> NormalizedTemplate {
    replace_delimited_params(input, '<', '>', |inner| {
        let inner = inner.trim();
        let name = inner
            .rsplit_once(':')
            .map(|(_, name)| name)
            .unwrap_or(inner)
            .trim()
            .trim_end_matches('?');
        (!name.is_empty()).then(|| name.to_string())
    })
}

fn normalize_braces_with_dots_template(input: &str) -> NormalizedTemplate {
    if input == "{$}" {
        return NormalizedTemplate {
            template: String::new(),
            dynamic_segments: Vec::new(),
        };
    }
    replace_delimited_params(input, '{', '}', |inner| {
        let name = inner
            .trim()
            .trim_end_matches("...")
            .trim_start_matches('*')
            .trim_end_matches('?')
            .trim();
        (!name.is_empty()).then(|| name.to_string())
    })
}

fn normalize_gin_wildcard_template(input: &str) -> NormalizedTemplate {
    let mut dynamic_segments = Vec::new();
    let template = input
        .split('/')
        .map(|segment| {
            if let Some(name) = segment.strip_prefix(':').filter(|name| !name.is_empty()) {
                dynamic_segments.push(name.to_string());
                segment.to_string()
            } else if let Some(name) = segment.strip_prefix('*').filter(|name| !name.is_empty()) {
                dynamic_segments.push(name.to_string());
                format!(":{name}")
            } else {
                segment.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("/");
    NormalizedTemplate {
        template,
        dynamic_segments,
    }
}

fn replace_delimited_params(
    input: &str,
    open: char,
    close: char,
    extract_name: impl Fn(&str) -> Option<String>,
) -> NormalizedTemplate {
    let mut output = String::new();
    let mut dynamic_segments = Vec::new();
    let mut cursor = 0;
    while let Some(relative_open) = input[cursor..].find(open) {
        let open_index = cursor + relative_open;
        let inner_start = open_index + open.len_utf8();
        let Some(relative_close) = input[inner_start..].find(close) else {
            break;
        };
        let close_index = inner_start + relative_close;
        output.push_str(&input[cursor..open_index]);
        let inner = &input[inner_start..close_index];
        if let Some(name) = extract_name(inner) {
            output.push(':');
            output.push_str(&name);
            dynamic_segments.push(name);
        } else {
            output.push(open);
            output.push_str(inner);
            output.push(close);
        }
        cursor = close_index + close.len_utf8();
    }
    output.push_str(&input[cursor..]);
    NormalizedTemplate {
        template: output,
        dynamic_segments,
    }
}

#[cfg(test)]
mod tests {
    use super::{ParamFlavor, classify_url, join_route_templates, normalize_route_template};

    #[test]
    fn normalizes_parameter_flavors_to_colon_segments() {
        let cases = [
            ("users/:id", ParamFlavor::Colon, "/users/:id", vec!["id"]),
            (
                "users/{id:int}",
                ParamFlavor::Braces,
                "/users/:id",
                vec!["id"],
            ),
            (
                "users/<int:user_id>/",
                ParamFlavor::AngleBrackets,
                "/users/:user_id/",
                vec!["user_id"],
            ),
            (
                "files/{path...}",
                ParamFlavor::BracesWithDots,
                "/files/:path",
                vec!["path"],
            ),
            (
                "assets/*filepath",
                ParamFlavor::GinWildcard,
                "/assets/:filepath",
                vec!["filepath"],
            ),
        ];

        for (input, flavor, expected, segments) in cases {
            let normalized = normalize_route_template(input, flavor);
            assert_eq!(normalized.template, expected, "{input}");
            assert_eq!(normalized.dynamic_segments, segments, "{input}");
        }
    }

    #[test]
    fn preserves_trailing_slashes_and_absolute_markers() {
        assert_eq!(
            normalize_route_template("~/status/", ParamFlavor::Braces).template,
            "/status/"
        );
        assert_eq!(
            normalize_route_template("/users/{id}/", ParamFlavor::Braces).template,
            "/users/:id/"
        );
        assert_eq!(
            normalize_route_template("users/{id}", ParamFlavor::Braces).template,
            "/users/:id"
        );
    }

    #[test]
    fn classifies_http_client_url_literals() {
        assert_eq!(classify_url("/api/users"), "path");
        assert_eq!(classify_url("api/users"), "relative");
        assert_eq!(classify_url("./users"), "relative");
        assert_eq!(classify_url("../users"), "relative");
        assert_eq!(classify_url("https://api.example.com/users"), "absolute");
    }

    #[test]
    fn joins_route_prefixes_without_duplicate_slashes() {
        assert_eq!(join_route_templates("/api", "/users"), "/api/users");
        assert_eq!(join_route_templates("/api/", "/users"), "/api/users");
        assert_eq!(join_route_templates("/api", "users"), "/api/users");
        assert_eq!(join_route_templates("/api/", "users"), "/api/users");
    }
}
