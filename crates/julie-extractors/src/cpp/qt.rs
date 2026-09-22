//! Qt's C++ macros carry the property, signal, slot and QML element facts that
//! the grammar never sees, because the pre-pass blanked them before the parse.
//! The macro sites are applied to the tree-driven symbols after the walk.

use std::collections::HashMap;

use serde_json::{Value, json};
use tree_sitter::{Node, Tree};

use super::qt_macros::{self, MacroKind, MacroSite, SIGNALS, SLOTS};
use crate::base::structural_fact_builders::{base_metadata, fact_for_span};
use crate::base::types::stable_location_id;
use crate::base::{BaseExtractor, NormalizedSpan, StructuralFact, Symbol, SymbolKind, Visibility};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

pub(crate) const LANGUAGE: &str = "cpp";
pub(crate) const PATTERN_ID: &str = "cpp.qt_property.v1";

const QUERY_FAMILY: &str = "properties";
const PROPERTY_MACRO: &str = "Q_PROPERTY";

const STRING_KEYWORDS: &[(&str, &str)] = &[
    ("READ", "read"),
    ("WRITE", "write"),
    ("MEMBER", "member"),
    ("NOTIFY", "notify"),
    ("RESET", "reset"),
    ("BINDABLE", "bindable"),
];

const FLAG_KEYWORDS: &[(&str, &str)] = &[
    ("CONSTANT", "constant"),
    ("FINAL", "final"),
    ("REQUIRED", "required"),
];

const BOUNDARY_KEYWORDS: &[&str] = &[
    "READ",
    "WRITE",
    "MEMBER",
    "NOTIFY",
    "RESET",
    "BINDABLE",
    "DESIGNABLE",
    "SCRIPTABLE",
    "STORED",
    "USER",
    "CONSTANT",
    "FINAL",
    "REQUIRED",
    "REVISION",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Section {
    Plain,
    Signals,
    Slots,
}

struct Property {
    name: String,
    property_type: String,
    strings: Vec<(&'static str, String)>,
    flags: Vec<&'static str>,
    signature: String,
}

/// Turns the Qt macro sites into property symbols, promotes signals to events
/// and records the slot, invokable and QML element metadata.
pub(crate) fn apply(base: &BaseExtractor, tree: &Tree, symbols: &mut Vec<Symbol>) {
    let sites = qt_macros::scan(&base.content);
    if sites.is_empty() {
        return;
    }

    apply_class_metadata(&sites, symbols);
    apply_member_sections(&sites, tree, symbols);
    let properties = property_symbols(base, &sites, symbols);
    symbols.extend(properties);
}

pub(crate) fn property_facts(
    file_path: &str,
    content: &str,
    symbols: &[Symbol],
) -> Vec<StructuralFact> {
    qt_macros::scan(content)
        .iter()
        .filter_map(|site| property_fact(file_path, content, site, symbols))
        .collect()
}

fn apply_class_metadata(sites: &[MacroSite], symbols: &mut [Symbol]) {
    for site in sites {
        if site.kind != MacroKind::Statement {
            continue;
        }
        let Some(index) = containing_class(symbols, site.start_byte) else {
            continue;
        };
        let class_name = symbols[index].name.clone();
        let Some((key, value)) = class_metadata_entry(site, &class_name) else {
            continue;
        };
        symbols[index]
            .metadata
            .get_or_insert_with(HashMap::new)
            .insert(key.to_string(), value);
    }
}

fn class_metadata_entry(site: &MacroSite, class_name: &str) -> Option<(&'static str, Value)> {
    let argument = || site.arguments.as_deref().map(unquote);
    match site.name.as_str() {
        "Q_OBJECT" => Some(("qt_object", json!(true))),
        "Q_GADGET" => Some(("qt_gadget", json!(true))),
        "QML_ELEMENT" => Some(("qml_element", json!(class_name))),
        "QML_SINGLETON" => Some(("qml_singleton", json!(true))),
        "QML_ANONYMOUS" => Some(("qml_anonymous", json!(true))),
        "QML_NAMED_ELEMENT" => Some(("qml_element", json!(argument()?))),
        "QML_UNCREATABLE" => Some(("qml_uncreatable", json!(argument()?))),
        "QML_ATTACHED" => Some(("qml_attached", json!(argument()?))),
        _ => None,
    }
}

/// A member belongs to the section opened by the last section site or access
/// label before it inside the same class body.
fn apply_member_sections(sites: &[MacroSite], tree: &Tree, symbols: &mut [Symbol]) {
    let mut boundaries = access_labels(tree)
        .into_iter()
        .map(|at| (at, 0u8, Section::Plain))
        .collect::<Vec<_>>();
    boundaries.extend(
        sites
            .iter()
            .filter(|site| site.kind == MacroKind::Section)
            .map(|site| (site.start_byte, 1u8, section_of(site))),
    );
    boundaries.sort_unstable();

    let class_ranges = symbols
        .iter()
        .filter(|symbol| is_class_like(symbol))
        .map(|symbol| (symbol.start_byte as usize, symbol.end_byte as usize))
        .collect::<Vec<_>>();

    for symbol in symbols.iter_mut() {
        if symbol.kind != SymbolKind::Method {
            continue;
        }
        let start = symbol.start_byte as usize;
        match innermost_range(&class_ranges, start)
            .and_then(|class| section_at(&boundaries, &class_ranges, class, start))
        {
            Some(Section::Signals) => symbol.kind = SymbolKind::Event,
            Some(Section::Slots) => insert_flag(symbol, "qt_slot"),
            _ => {}
        }
        let line = symbol.start_line as usize;
        for site in sites
            .iter()
            .filter(|site| site.kind == MacroKind::Prefix && site.line == line)
        {
            match site.name.as_str() {
                "Q_INVOKABLE" => insert_flag(symbol, "qt_invokable"),
                "Q_SIGNAL" => symbol.kind = SymbolKind::Event,
                "Q_SLOT" => insert_flag(symbol, "qt_slot"),
                _ => {}
            }
        }
    }
}

fn section_of(site: &MacroSite) -> Section {
    if site.name == SIGNALS {
        Section::Signals
    } else if site.name == SLOTS {
        Section::Slots
    } else {
        Section::Plain
    }
}

/// A boundary inside a nested class body belongs to that class, so it never ends
/// the section the outer class is in.
fn section_at(
    boundaries: &[(usize, u8, Section)],
    class_ranges: &[(usize, usize)],
    class: (usize, usize),
    member_start: usize,
) -> Option<Section> {
    boundaries
        .iter()
        .rev()
        .find(|(at, _, _)| {
            (class.0..=member_start).contains(at)
                && innermost_range(class_ranges, *at) == Some(class)
        })
        .map(|(_, _, section)| *section)
}

fn innermost_range(ranges: &[(usize, usize)], byte: usize) -> Option<(usize, usize)> {
    ranges
        .iter()
        .filter(|(start, end)| *start <= byte && byte < *end)
        .max_by_key(|(start, _)| *start)
        .copied()
}

fn containing_class(symbols: &[Symbol], byte: usize) -> Option<usize> {
    symbols
        .iter()
        .enumerate()
        .filter(|(_, symbol)| {
            is_class_like(symbol)
                && (symbol.start_byte as usize) <= byte
                && byte < symbol.end_byte as usize
        })
        .max_by_key(|(_, symbol)| symbol.start_byte)
        .map(|(index, _)| index)
}

fn is_class_like(symbol: &Symbol) -> bool {
    matches!(symbol.kind, SymbolKind::Class | SymbolKind::Struct)
}

fn insert_flag(symbol: &mut Symbol, key: &str) {
    symbol
        .metadata
        .get_or_insert_with(HashMap::new)
        .insert(key.to_string(), json!(true));
}

fn access_labels(tree: &Tree) -> Vec<usize> {
    let mut labels = Vec::new();
    collect_access_labels(tree.root_node(), &mut labels, 0);
    labels
}

fn collect_access_labels(node: Node<'_>, labels: &mut Vec<usize>, depth: u32) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if node.kind() == "access_specifier" {
        labels.push(node.start_byte());
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_access_labels(child, labels, child_depth);
    }
}

fn property_symbols(base: &BaseExtractor, sites: &[MacroSite], symbols: &[Symbol]) -> Vec<Symbol> {
    sites
        .iter()
        .filter_map(|site| property_symbol(base, site, symbols))
        .collect()
}

fn property_symbol(base: &BaseExtractor, site: &MacroSite, symbols: &[Symbol]) -> Option<Symbol> {
    let property = property_at(site)?;
    let class = containing_class(symbols, site.start_byte)?;
    let span = NormalizedSpan::from_content_range(&base.content, site.start_byte, site.end_byte)?;

    Some(Symbol {
        id: stable_location_id(&base.file_path, &property.name, span),
        name: property.name.clone(),
        kind: SymbolKind::Property,
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
        signature: Some(property.signature.clone()),
        doc_comment: None,
        visibility: Some(Visibility::Public),
        parent_id: Some(symbols[class].id.clone()),
        metadata: Some(property_metadata(&property)),
        annotations: Vec::new(),
        semantic_group: None,
        confidence: None,
        content_type: None,
    })
}

fn property_fact(
    file_path: &str,
    content: &str,
    site: &MacroSite,
    symbols: &[Symbol],
) -> Option<StructuralFact> {
    let property = property_at(site)?;
    let class = containing_class(symbols, site.start_byte)?;
    let span = NormalizedSpan::from_content_range(content, site.start_byte, site.end_byte)?;

    let mut metadata = base_metadata(QUERY_FAMILY);
    metadata.insert("name".to_string(), json!(property.name));
    metadata.extend(property_metadata(&property));

    let mut fact = fact_for_span(
        file_path,
        LANGUAGE,
        PATTERN_ID,
        "qt_property",
        "macro",
        span,
        metadata,
    );
    fact.containing_symbol_id = Some(symbols[class].id.clone());
    Some(fact)
}

fn property_at(site: &MacroSite) -> Option<Property> {
    if site.kind != MacroKind::Statement || site.name != PROPERTY_MACRO {
        return None;
    }
    parse_property(site.arguments.as_deref()?)
}

fn property_metadata(property: &Property) -> HashMap<String, Value> {
    let mut metadata = HashMap::new();
    if !property.property_type.is_empty() {
        metadata.insert("property_type".to_string(), json!(property.property_type));
    }
    for (key, value) in &property.strings {
        metadata.insert((*key).to_string(), json!(value));
    }
    for key in &property.flags {
        metadata.insert((*key).to_string(), json!(true));
    }
    metadata
}

fn parse_property(arguments: &str) -> Option<Property> {
    let collapsed = arguments.split_whitespace().collect::<Vec<_>>().join(" ");
    let tokens = collapsed.split(' ').filter(|token| !token.is_empty());
    let tokens = tokens.collect::<Vec<_>>();
    let boundary = tokens
        .iter()
        .position(|token| BOUNDARY_KEYWORDS.contains(token))
        .unwrap_or(tokens.len());

    let declaration = tokens[..boundary].join(" ");
    let name = trailing_identifier(&declaration)?;
    let property_type = declaration[..declaration.len() - name.len()]
        .trim()
        .to_string();

    let mut strings = Vec::new();
    let mut flags = Vec::new();
    let mut index = boundary;
    while index < tokens.len() {
        if let Some((_, key)) = STRING_KEYWORDS
            .iter()
            .find(|(keyword, _)| *keyword == tokens[index])
            && let Some(value) = tokens.get(index + 1)
        {
            strings.push((*key, (*value).to_string()));
            index += 2;
            continue;
        }
        if let Some((_, key)) = FLAG_KEYWORDS
            .iter()
            .find(|(keyword, _)| *keyword == tokens[index])
        {
            flags.push(*key);
        }
        index += 1;
    }

    Some(Property {
        name: name.to_string(),
        property_type,
        strings,
        flags,
        signature: format!("{PROPERTY_MACRO}({collapsed})"),
    })
}

fn trailing_identifier(declaration: &str) -> Option<&str> {
    let start = declaration
        .char_indices()
        .rev()
        .find(|(_, character)| !(character.is_alphanumeric() || *character == '_'))
        .map_or(0, |(at, character)| at + character.len_utf8());
    let name = &declaration[start..];
    (!name.is_empty() && !name.starts_with(|character: char| character.is_numeric()))
        .then_some(name)
}

fn unquote(text: &str) -> &str {
    let text = text.trim();
    text.strip_prefix('"')
        .and_then(|text| text.strip_suffix('"'))
        .unwrap_or(text)
}
