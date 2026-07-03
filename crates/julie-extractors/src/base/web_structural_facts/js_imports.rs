use std::collections::HashMap;

use super::js_object_scan::{
    find_matching_brace, is_identifier_boundary, parse_js_identifier, parse_js_string_literal,
    skip_ascii_whitespace_until,
};

#[derive(Debug, Default)]
pub(super) struct JsImportIndex {
    pub(super) react_router_links: HashMap<String, String>,
    pub(super) react_router_routes: HashMap<String, String>,
    pub(super) react_router_route_apis: HashMap<String, String>,
    pub(super) next_links: HashMap<String, String>,
    pub(super) axios_clients: HashMap<String, String>,
}

pub(super) fn collect_js_imports(content: &str) -> JsImportIndex {
    let mut imports = JsImportIndex::default();
    let mut cursor = 0;

    while cursor < content.len() {
        let Some(relative_import) = content[cursor..].find("import") else {
            break;
        };
        let import_start = cursor + relative_import;
        cursor = import_start + "import".len();
        if !is_identifier_boundary(content, import_start, "import".len()) {
            continue;
        }
        if is_in_js_comment_or_string(content, import_start) {
            continue;
        }

        let statement_end = js_import_statement_end(content, import_start);
        let Some(statement) = content.get(import_start..statement_end) else {
            continue;
        };
        cursor = statement_end;

        let Some(source) = parse_import_source(statement) else {
            continue;
        };
        match source.as_str() {
            "react-router" | "react-router-dom" | "@remix-run/react" => {
                for (imported, local) in parse_named_imports(statement) {
                    match imported.as_str() {
                        "Link" | "NavLink" => {
                            imports.react_router_links.insert(local, source.clone());
                        }
                        "Route" => {
                            imports.react_router_routes.insert(local, source.clone());
                        }
                        "createBrowserRouter" | "useRoutes" | "createRoutesFromElements" => {
                            imports
                                .react_router_route_apis
                                .insert(local, source.clone());
                        }
                        _ => {}
                    }
                }
            }
            "next/link" => {
                if let Some(local) = parse_default_import(statement) {
                    imports.next_links.insert(local, source.clone());
                }
                for (imported, local) in parse_named_imports(statement) {
                    if imported == "Link" {
                        imports.next_links.insert(local, source.clone());
                    }
                }
            }
            "axios" => {
                // The callable client is the default export (or the namespace
                // object, whose call/method surface matches it). Named imports
                // such as `AxiosError` are not clients and stay out.
                if let Some(local) = parse_default_import(statement) {
                    imports.axios_clients.insert(local, source.clone());
                }
                if let Some(local) = parse_namespace_import(statement) {
                    imports.axios_clients.insert(local, source.clone());
                }
            }
            _ => {}
        }
    }

    imports
}

fn is_in_js_comment_or_string(content: &str, target: usize) -> bool {
    let bytes = content.as_bytes();
    let mut cursor = 0;
    let mut line_comment = false;
    let mut block_comment = false;
    let mut quote = None;
    let mut escaped = false;

    while cursor < target {
        let byte = bytes[cursor];
        let next = bytes.get(cursor + 1).copied();
        if line_comment {
            if byte == b'\n' {
                line_comment = false;
            }
        } else if block_comment {
            if byte == b'*' && next == Some(b'/') {
                block_comment = false;
                cursor += 1;
            }
        } else if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active_quote {
                quote = None;
            }
        } else if byte == b'/' && next == Some(b'/') {
            line_comment = true;
            cursor += 1;
        } else if byte == b'/' && next == Some(b'*') {
            block_comment = true;
            cursor += 1;
        } else if matches!(byte, b'\'' | b'"' | b'`') {
            quote = Some(byte);
        }
        cursor += 1;
    }

    line_comment || block_comment || quote.is_some()
}

pub(in crate::base) fn js_import_statement_end(content: &str, import_start: usize) -> usize {
    let bytes = content.as_bytes();
    let mut cursor = import_start;
    let mut brace_depth = 0usize;
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut quote = None;
    let mut escaped = false;

    while cursor < bytes.len() {
        let byte = bytes[cursor];
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active_quote {
                quote = None;
            }
        } else if byte == b'\'' || byte == b'"' {
            quote = Some(byte);
        } else if byte == b'{' {
            brace_depth += 1;
        } else if byte == b'}' {
            brace_depth = brace_depth.saturating_sub(1);
        } else if byte == b'(' {
            paren_depth += 1;
        } else if byte == b')' {
            paren_depth = paren_depth.saturating_sub(1);
        } else if byte == b'[' {
            bracket_depth += 1;
        } else if byte == b']' {
            bracket_depth = bracket_depth.saturating_sub(1);
        } else if byte == b';' && brace_depth == 0 && paren_depth == 0 && bracket_depth == 0 {
            return cursor + 1;
        } else if byte == b'\n' && brace_depth == 0 && paren_depth == 0 && bracket_depth == 0 {
            return cursor;
        }
        cursor += 1;
    }

    content.len()
}

pub(in crate::base) fn parse_import_source(statement: &str) -> Option<String> {
    let from_start = statement.rfind("from")?;
    if !is_identifier_boundary(statement, from_start, "from".len()) {
        return None;
    }
    let source_start =
        skip_ascii_whitespace_until(statement, from_start + "from".len(), statement.len());
    let (source, source_end) = parse_js_string_literal(statement, source_start)?;
    (source_end <= statement.len()).then_some(source)
}

pub(in crate::base) fn parse_named_imports(statement: &str) -> Vec<(String, String)> {
    let Some(open_brace) = statement.find('{') else {
        return Vec::new();
    };
    let Some(close_brace) = find_matching_brace(statement, open_brace, statement.len()) else {
        return Vec::new();
    };
    let Some(import_list) = statement.get(open_brace + 1..close_brace) else {
        return Vec::new();
    };

    import_list
        .split(',')
        .filter_map(|entry| {
            let entry = entry.trim().trim_start_matches("type ").trim();
            if entry.is_empty() {
                return None;
            }
            let mut parts = entry.split_whitespace();
            let imported = parts.next()?.trim().to_string();
            let local = if parts.next() == Some("as") {
                parts.next()?.trim().to_string()
            } else {
                imported.clone()
            };
            Some((imported, local))
        })
        .collect()
}

/// Parses `import * as local from "..."`, returning the local binding.
pub(in crate::base) fn parse_namespace_import(statement: &str) -> Option<String> {
    let after_import = skip_ascii_whitespace_until(statement, "import".len(), statement.len());
    if statement.as_bytes().get(after_import) != Some(&b'*') {
        return None;
    }
    let as_start = skip_ascii_whitespace_until(statement, after_import + 1, statement.len());
    if !statement[as_start..].starts_with("as")
        || !is_identifier_boundary(statement, as_start, "as".len())
    {
        return None;
    }
    let local_start =
        skip_ascii_whitespace_until(statement, as_start + "as".len(), statement.len());
    parse_js_identifier(statement, local_start, statement.len()).map(|(identifier, _)| identifier)
}

pub(in crate::base) fn parse_default_import(statement: &str) -> Option<String> {
    let after_import = skip_ascii_whitespace_until(statement, "import".len(), statement.len());
    if matches!(
        statement.as_bytes().get(after_import),
        Some(b'{') | Some(b'*')
    ) {
        return None;
    }
    parse_js_identifier(statement, after_import, statement.len()).map(|(identifier, _)| identifier)
}
