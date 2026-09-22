//! Qt's QML JavaScript directives (`.pragma`, `.import`) at the head of a `.js`
//! file. They are not JavaScript, so both parse paths blank them to same-length
//! spaces while the original text stays the source of record.

use std::collections::HashMap;

use serde_json::json;

use crate::base::structural_fact_builders::{base_metadata, fact_for_span};
use crate::base::types::stable_location_id;
use crate::base::{BaseExtractor, NormalizedSpan, StructuralFact, Symbol, SymbolKind, Visibility};

pub(crate) const LANGUAGE: &str = "javascript";

pub(crate) const PATTERN_ID: &str = "javascript.qml_directive.v1";

enum Directive {
    Pragma {
        name: String,
    },
    Import {
        source: String,
        source_kind: &'static str,
        version: Option<String>,
        alias: String,
    },
}

struct Located {
    directive: Directive,
    text: String,
    start_byte: usize,
    end_byte: usize,
}

/// `None` when the source carries no directive, so the ordinary JavaScript path
/// parses the original text without an extra allocation.
pub(crate) fn blank_directives(content: &str) -> Option<String> {
    let directives = scan(content);
    if directives.is_empty() {
        return None;
    }

    let mut bytes = content.as_bytes().to_vec();
    for located in &directives {
        bytes[located.start_byte..located.end_byte].fill(b' ');
    }
    String::from_utf8(bytes).ok()
}

pub(crate) fn import_symbols(base: &BaseExtractor) -> Vec<Symbol> {
    if base.language != LANGUAGE {
        return Vec::new();
    }
    scan(&base.content)
        .iter()
        .filter_map(|located| import_symbol(base, located))
        .collect()
}

pub(crate) fn facts(file_path: &str, content: &str) -> Vec<StructuralFact> {
    scan(content)
        .iter()
        .filter_map(|located| pragma_fact(file_path, content, located))
        .collect()
}

fn import_symbol(base: &BaseExtractor, located: &Located) -> Option<Symbol> {
    let Directive::Import {
        source,
        source_kind,
        version,
        alias,
    } = &located.directive
    else {
        return None;
    };
    let span =
        NormalizedSpan::from_content_range(&base.content, located.start_byte, located.end_byte)?;

    let mut metadata = HashMap::from([
        ("source".to_string(), json!(source)),
        ("source_kind".to_string(), json!(source_kind)),
        (
            "import_kind".to_string(),
            json!(crate::qml::import_kind(source_kind, source)),
        ),
        ("alias".to_string(), json!(alias)),
        ("local_name".to_string(), json!(alias)),
        ("imported_name".to_string(), json!(source)),
        ("is_namespace".to_string(), json!(true)),
    ]);
    if let Some(version) = version {
        metadata.insert("version".to_string(), json!(version));
    }

    Some(Symbol {
        id: stable_location_id(&base.file_path, source, span),
        name: source.clone(),
        kind: SymbolKind::Import,
        language: base.language.clone(),
        file_path: base.file_path.clone(),
        start_line: span.start_line,
        start_column: span.start_column,
        end_line: span.end_line,
        end_column: span.end_column,
        start_byte: span.start_byte,
        end_byte: span.end_byte,
        body_span: None,
        body_hash: None,
        signature: Some(located.text.clone()),
        doc_comment: None,
        visibility: Some(Visibility::Public),
        parent_id: None,
        metadata: Some(metadata),
        annotations: Vec::new(),
        semantic_group: None,
        confidence: None,
        content_type: None,
    })
}

fn pragma_fact(file_path: &str, content: &str, located: &Located) -> Option<StructuralFact> {
    let Directive::Pragma { name } = &located.directive else {
        return None;
    };
    let span = NormalizedSpan::from_content_range(content, located.start_byte, located.end_byte)?;

    let mut metadata = base_metadata("directives");
    metadata.insert("directive".to_string(), json!("pragma"));
    metadata.insert("name".to_string(), json!(name));

    Some(fact_for_span(
        file_path,
        LANGUAGE,
        PATTERN_ID,
        "directive",
        "pragma",
        span,
        metadata,
    ))
}

/// Directives are only legal above the first statement, so the walk stops at the
/// first line that is neither blank, a comment, nor a directive.
fn scan(content: &str) -> Vec<Located> {
    let mut located = Vec::new();
    let mut offset = 0;
    let mut inside_block_comment = false;

    'lines: for line in content.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();

        let text = line.trim_end_matches('\n').trim_end_matches('\r');
        let mut cursor = 0;
        loop {
            if inside_block_comment {
                let Some(end) = text[cursor..].find("*/") else {
                    continue 'lines;
                };
                inside_block_comment = false;
                cursor += end + 2;
            }
            let rest = text[cursor..].trim_start();
            cursor = text.len() - rest.len();
            if rest.is_empty() || rest.starts_with("//") {
                continue 'lines;
            }
            if rest.starts_with("/*") {
                inside_block_comment = true;
                cursor += 2;
                continue;
            }
            break;
        }

        let line_rest = text[cursor..].trim_end();
        let directive_text = directive_without_comment(line_rest);
        let Some(directive) = parse_directive(directive_text) else {
            break;
        };
        located.push(Located {
            directive,
            text: directive_text.to_string(),
            start_byte: line_start + cursor,
            end_byte: line_start + cursor + line_rest.len(),
        });
    }

    located
}

/// A `//` comment ends a directive line. A `//` inside the quoted source of an
/// `.import` is part of the source, not a comment.
fn directive_without_comment(text: &str) -> &str {
    let bytes = text.as_bytes();
    let mut quote: Option<u8> = None;
    for index in 0..bytes.len() {
        match bytes[index] {
            byte @ (b'"' | b'\'') if quote.is_none() => quote = Some(byte),
            byte if quote == Some(byte) => quote = None,
            b'/' if quote.is_none() && bytes.get(index + 1) == Some(&b'/') => {
                return text[..index].trim_end();
            }
            _ => {}
        }
    }
    text
}

fn parse_directive(text: &str) -> Option<Directive> {
    let (keyword, arguments) = text.strip_prefix('.')?.split_once(char::is_whitespace)?;
    match keyword {
        "pragma" => {
            let name = arguments.trim();
            is_identifier(name).then(|| Directive::Pragma {
                name: name.to_string(),
            })
        }
        "import" => parse_import(arguments.trim()),
        _ => None,
    }
}

fn parse_import(arguments: &str) -> Option<Directive> {
    let quote = arguments.chars().next()?;
    let (source, source_kind, rest) = if quote == '"' || quote == '\'' {
        let body = &arguments[1..];
        let end = body.find(quote)?;
        (&body[..end], "quoted", &body[end + 1..])
    } else {
        let (uri, rest) = arguments.split_once(char::is_whitespace)?;
        if !is_module_uri(uri) {
            return None;
        }
        (uri, "uri", rest)
    };
    if source.is_empty() {
        return None;
    }

    let mut tokens = rest.split_whitespace();
    let version = if source_kind == "uri" {
        let version = tokens.next()?;
        if !is_version(version) {
            return None;
        }
        Some(version.to_string())
    } else {
        None
    };
    if tokens.next()? != "as" {
        return None;
    }
    let alias = tokens.next()?;
    if !is_identifier(alias) || tokens.next().is_some() {
        return None;
    }

    Some(Directive::Import {
        source: source.to_string(),
        source_kind,
        version,
        alias: alias.to_string(),
    })
}

fn is_identifier(text: &str) -> bool {
    let mut characters = text.chars();
    characters
        .next()
        .is_some_and(|first| first.is_alphabetic() || first == '_' || first == '$')
        && characters
            .all(|character| character.is_alphanumeric() || character == '_' || character == '$')
}

fn is_module_uri(text: &str) -> bool {
    !text.is_empty() && text.split('.').all(is_identifier)
}

fn is_version(text: &str) -> bool {
    let (major, minor) = match text.split_once('.') {
        Some((major, minor)) => (major, Some(minor)),
        None => (text, None),
    };
    let is_number = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    is_number(major) && minor.is_none_or(is_number)
}
