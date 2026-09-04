//! Shared containing-symbol binder for structural facts.
//!
//! Every structural-fact collector binds each emitted fact to the scope-bearing
//! symbol that encloses it (`StructuralFact::containing_symbol_id`). This module
//! is the single source of truth for that logic; the six collectors
//! (`code`/`data`/`framework`/`sql`/`structural`/`web` structural facts) all call
//! [`attach_containing_symbols`].
//!
//! Binding runs in two passes:
//!
//! 1. **Primary (byte containment).** The narrowest scope-bearing symbol whose
//!    byte span strictly contains the fact wins.
//! 2. **Fallback (line containment).** Only when the primary pass finds nothing:
//!    the narrowest scope-bearing symbol whose *line* span contains the fact
//!    wins, ties broken by narrowest byte span then earliest `start_byte`. This
//!    catches facts whose anchor byte span is not strictly inside its owning
//!    symbol — e.g. a `export const POST = async () => ...` route handler whose
//!    fact anchors on the `export const POST` head while the `POST` symbol spans
//!    only the arrow value.
//!
//! Both passes apply the same [`is_scope_bearing`] kind filter, so a fact
//! describing a code action binds to the enclosing scope (function/method/class/…)
//! rather than to the narrowest local value-holder (variable/constant/…) it
//! happens to sit inside. Fields and properties are NOT filtered: they are
//! first-class graph members (and property accessors are genuine scopes), so a
//! fact inside a field/property span binds that member.
//!
//! `source_regions.rs` deliberately keeps its own binder with the unfiltered
//! semantics: source regions (comments, string literals) legitimately attach to
//! value-holder symbols (a doc comment or literal on a variable belongs to that
//! variable).

use super::kinds::SymbolKind;
use super::types::{StructuralFact, Symbol};

/// Returns `true` when `symbol` may own a structural fact.
///
/// Local, non-scope-bearing "value holder" kinds are excluded from containment
/// candidacy: `Variable`, `Constant`, `EnumMember`, and `Import`. Everything else
/// stays a candidate — notably:
/// - `Field`/`Property`: first-class graph members whose spans (property accessor
///   bodies especially) are genuine scopes; a fact inside them binds the member,
///   and class binding stays recoverable via member parentage.
/// - `Export`: exported declarations get a whole-statement `export`-kind symbol
///   that is the correct owner for facts anchored on the export head.
fn is_scope_bearing(symbol: &Symbol) -> bool {
    !matches!(
        symbol.kind,
        SymbolKind::Variable | SymbolKind::Constant | SymbolKind::EnumMember | SymbolKind::Import
    )
}

/// Bind each fact in `facts` to its containing scope-bearing symbol.
pub(crate) fn attach_containing_symbols(facts: &mut [StructuralFact], symbols: &[Symbol]) {
    for fact in facts {
        fact.containing_symbol_id = containing_symbol_id(fact, symbols);
    }
}

fn containing_symbol_id(fact: &StructuralFact, symbols: &[Symbol]) -> Option<String> {
    byte_containing_symbol(fact, symbols)
        .or_else(|| line_containing_symbol(fact, symbols))
        .map(|symbol| symbol.id.clone())
}

/// Primary pass: narrowest scope-bearing symbol whose byte span contains the fact.
fn byte_containing_symbol<'a>(fact: &StructuralFact, symbols: &'a [Symbol]) -> Option<&'a Symbol> {
    symbols
        .iter()
        .filter(|symbol| is_scope_bearing(symbol))
        .filter(|symbol| symbol.start_byte <= fact.start_byte && symbol.end_byte >= fact.end_byte)
        .min_by_key(|symbol| byte_span(symbol))
}

/// Fallback pass: narrowest scope-bearing symbol whose line span contains the
/// fact and whose byte span is not contained by the fact. Ties broken by
/// narrowest byte span, then earliest `start_byte`.
fn line_containing_symbol<'a>(fact: &StructuralFact, symbols: &'a [Symbol]) -> Option<&'a Symbol> {
    symbols
        .iter()
        .filter(|symbol| is_scope_bearing(symbol))
        .filter(|symbol| symbol.start_line <= fact.start_line && symbol.end_line >= fact.end_line)
        .filter(|symbol| {
            !(symbol.start_byte >= fact.start_byte && symbol.end_byte <= fact.end_byte)
        })
        .min_by(|left, right| {
            line_span(left)
                .cmp(&line_span(right))
                .then_with(|| byte_span(left).cmp(&byte_span(right)))
                .then_with(|| left.start_byte.cmp(&right.start_byte))
        })
}

fn byte_span(symbol: &Symbol) -> u32 {
    symbol.end_byte.saturating_sub(symbol.start_byte)
}

fn line_span(symbol: &Symbol) -> u32 {
    symbol.end_line.saturating_sub(symbol.start_line)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_symbol(
        id: &str,
        kind: SymbolKind,
        start_byte: u32,
        end_byte: u32,
        start_line: u32,
        end_line: u32,
    ) -> Symbol {
        Symbol {
            id: id.to_string(),
            name: id.to_string(),
            kind,
            language: "typescript".to_string(),
            file_path: "app/route.ts".to_string(),
            start_line,
            start_column: 0,
            end_line,
            end_column: 0,
            start_byte,
            end_byte,
            body_span: None,
            body_hash: None,
            signature: None,
            doc_comment: None,
            visibility: None,
            parent_id: None,
            metadata: None,
            annotations: Vec::new(),
            semantic_group: None,
            confidence: None,
            content_type: None,
        }
    }

    fn make_fact(start_byte: u32, end_byte: u32, start_line: u32, end_line: u32) -> StructuralFact {
        StructuralFact {
            id: "fact".to_string(),
            file_path: "app/route.ts".to_string(),
            language: "typescript".to_string(),
            pattern_id: "test.fact.v1".to_string(),
            capture_name: "capture".to_string(),
            node_kind: "node".to_string(),
            containing_symbol_id: None,
            start_line,
            start_column: 0,
            end_line,
            end_column: 0,
            start_byte,
            end_byte,
            confidence: 1.0,
            metadata: None,
        }
    }

    fn bind(fact: &StructuralFact, symbols: &[Symbol]) -> Option<String> {
        containing_symbol_id(fact, symbols)
    }

    #[test]
    fn kind_filter_skips_value_holders_and_binds_enclosing_scope() {
        // A `variable` and a `function` both byte-contain the fact; the variable
        // is filtered so the function wins, even though it is the wider span.
        let symbols = vec![
            make_symbol("load", SymbolKind::Function, 0, 100, 1, 5),
            make_symbol("res", SymbolKind::Variable, 20, 60, 2, 2),
        ];
        let fact = make_fact(30, 45, 2, 2);
        assert_eq!(bind(&fact, &symbols).as_deref(), Some("load"));
    }

    #[test]
    fn every_filtered_kind_is_excluded() {
        for kind in [
            SymbolKind::Variable,
            SymbolKind::Constant,
            SymbolKind::EnumMember,
            SymbolKind::Import,
        ] {
            let symbols = vec![make_symbol("holder", kind.clone(), 0, 100, 1, 5)];
            let fact = make_fact(10, 20, 2, 2);
            assert_eq!(
                bind(&fact, &symbols),
                None,
                "kind {kind:?} must not be a containment candidate"
            );
        }
    }

    #[test]
    fn field_and_property_stay_candidates_and_bind_over_enclosing_type() {
        // Fields and properties are first-class members (property accessors are
        // genuine scopes), so a fact inside a member span binds the member, not
        // the enclosing type. Class binding stays recoverable via parentage.
        for member_kind in [SymbolKind::Field, SymbolKind::Property] {
            let symbols = vec![
                make_symbol("Widget", SymbolKind::Class, 0, 400, 1, 40),
                make_symbol("member", member_kind.clone(), 100, 200, 10, 18),
            ];
            let fact = make_fact(120, 150, 12, 14);
            assert_eq!(
                bind(&fact, &symbols).as_deref(),
                Some("member"),
                "a fact inside a {member_kind:?} span must bind the member, not the enclosing type"
            );
        }
    }

    #[test]
    fn export_symbol_stays_a_candidate() {
        // `export`-kind whole-statement symbols must remain eligible owners.
        let symbols = vec![make_symbol("GET", SymbolKind::Export, 0, 100, 1, 5)];
        let fact = make_fact(0, 17, 1, 1);
        assert_eq!(bind(&fact, &symbols).as_deref(), Some("GET"));
    }

    #[test]
    fn narrowest_byte_containing_symbol_wins() {
        let symbols = vec![
            make_symbol("outer", SymbolKind::Class, 0, 200, 1, 20),
            make_symbol("inner", SymbolKind::Method, 40, 120, 5, 12),
        ];
        let fact = make_fact(60, 80, 6, 7);
        assert_eq!(bind(&fact, &symbols).as_deref(), Some("inner"));
    }

    #[test]
    fn line_fallback_binds_when_byte_containment_fails() {
        // Repro shape: fact anchors on the `export const POST` head (bytes 0..17)
        // while the POST symbol spans only the arrow value (byte 20 onward) but
        // covers the same lines. Byte containment fails; line fallback binds POST.
        let symbols = vec![make_symbol("POST", SymbolKind::Function, 20, 90, 1, 3)];
        let fact = make_fact(0, 17, 1, 1);
        assert_eq!(
            byte_containing_symbol(&fact, &symbols).map(|s| s.id.as_str()),
            None
        );
        assert_eq!(bind(&fact, &symbols).as_deref(), Some("POST"));
    }

    #[test]
    fn line_fallback_prefers_narrowest_line_span() {
        let symbols = vec![
            make_symbol("wide", SymbolKind::Function, 200, 260, 1, 10),
            make_symbol("narrow", SymbolKind::Function, 300, 360, 2, 4),
        ];
        // No symbol byte-contains the fact; both line-contain it.
        let fact = make_fact(0, 5, 3, 3);
        assert_eq!(bind(&fact, &symbols).as_deref(), Some("narrow"));
    }

    #[test]
    fn line_fallback_tie_breaks_by_byte_span_then_start_byte() {
        // Equal line span (both cover lines 2..4): narrower byte span wins.
        let symbols = vec![
            make_symbol("byte_wide", SymbolKind::Function, 100, 200, 2, 4),
            make_symbol("byte_narrow", SymbolKind::Function, 300, 340, 2, 4),
        ];
        let fact = make_fact(0, 5, 3, 3);
        assert_eq!(bind(&fact, &symbols).as_deref(), Some("byte_narrow"));

        // Equal line span and equal byte span: earliest start_byte wins.
        let symbols = vec![
            make_symbol("later", SymbolKind::Function, 300, 340, 2, 4),
            make_symbol("earlier", SymbolKind::Function, 100, 140, 2, 4),
        ];
        let fact = make_fact(0, 5, 3, 3);
        assert_eq!(bind(&fact, &symbols).as_deref(), Some("earlier"));
    }

    #[test]
    fn line_fallback_rejects_child_symbol_byte_contained_by_fact() {
        let symbols = vec![make_symbol(
            "child-property",
            SymbolKind::Property,
            150,
            170,
            10,
            10,
        )];
        let fact = make_fact(100, 200, 10, 10);
        assert_eq!(bind(&fact, &symbols), None);
    }

    #[test]
    fn none_when_nothing_contains_the_fact() {
        let symbols = vec![make_symbol("far", SymbolKind::Function, 500, 600, 40, 50)];
        let fact = make_fact(0, 5, 1, 1);
        assert_eq!(bind(&fact, &symbols), None);
    }
}
