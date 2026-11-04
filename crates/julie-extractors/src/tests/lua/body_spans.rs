use crate::base::{ExtractionResults, Symbol, SymbolKind};
use crate::extract_canonical;
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("lua extraction")
}

fn symbol<'a>(result: &'a ExtractionResults, name: &str, kind: SymbolKind) -> &'a Symbol {
    result
        .symbols
        .iter()
        .find(|candidate| candidate.name == name && candidate.kind == kind)
        .unwrap_or_else(|| panic!("missing {kind:?} {name}: {:#?}", result.symbols))
}

fn body_text<'a>(symbol: &Symbol, source: &'a str) -> Option<&'a str> {
    let span = symbol.body_span?;
    source.get(span.start_byte as usize..span.end_byte as usize)
}

#[test]
fn empty_function_declarations_and_values_have_no_body_span() {
    let source = "function global_empty() end\nlocal function local_empty() end\nlocal Container = {}\nfunction Container:method_empty() end\nlocal assigned_empty = function() end\n";
    let result = extract("empty.lua", source);
    assert!(
        result.parse_diagnostics.is_empty(),
        "{:#?}",
        result.parse_diagnostics
    );
    for symbol in [
        symbol(&result, "global_empty", SymbolKind::Function),
        symbol(&result, "local_empty", SymbolKind::Function),
        symbol(&result, "method_empty", SymbolKind::Method),
        symbol(&result, "assigned_empty", SymbolKind::Function),
    ] {
        assert!(symbol.body_span.is_none(), "{symbol:#?}");
        assert!(symbol.body_hash.is_none(), "{symbol:#?}");
    }
}

#[test]
fn function_body_spans_follow_ast_blocks_and_hash_ignores_comment_edits() {
    let source = "-- 雪\nlocal function outer(value)\n    local function inner()\n        local answer = value\n        -- first comment\n        return answer .. \"é\"\n    end\n    return inner()\nend\nlocal assigned = function(input)\n    if input then return outer(input) end\nend\n".replace('\n', "\r\n");
    let edited_comment = source.replace("first comment", "edited comment with more text");
    let result = extract("nested.lua", &source);
    let edited = extract("nested.lua", &edited_comment);
    assert!(
        result.parse_diagnostics.is_empty(),
        "{:#?}",
        result.parse_diagnostics
    );
    assert!(
        edited.parse_diagnostics.is_empty(),
        "{:#?}",
        edited.parse_diagnostics
    );

    let outer = symbol(&result, "outer", SymbolKind::Function);
    let inner = symbol(&result, "inner", SymbolKind::Function);
    let assigned = symbol(&result, "assigned", SymbolKind::Function);
    let edited_outer = symbol(&edited, "outer", SymbolKind::Function);
    let edited_inner = symbol(&edited, "inner", SymbolKind::Function);
    let edited_assigned = symbol(&edited, "assigned", SymbolKind::Function);

    assert_eq!(inner.parent_id.as_deref(), Some(outer.id.as_str()));
    assert_eq!(
        body_text(outer, &source),
        Some(
            "local function inner()\r\n        local answer = value\r\n        -- first comment\r\n        return answer .. \"é\"\r\n    end\r\n    return inner()"
        )
    );
    assert_eq!(
        body_text(inner, &source),
        Some("local answer = value\r\n        -- first comment\r\n        return answer .. \"é\"")
    );
    assert_eq!(
        body_text(assigned, &source),
        Some("if input then return outer(input) end")
    );
    assert!(outer.body_hash.is_some(), "{outer:#?}");
    assert!(inner.body_hash.is_some(), "{inner:#?}");
    assert!(assigned.body_hash.is_some(), "{assigned:#?}");
    assert_eq!(
        inner.body_hash, edited_inner.body_hash,
        "{inner:#?}\n{edited_inner:#?}"
    );
    assert_eq!(
        outer.body_hash, edited_outer.body_hash,
        "{outer:#?}\n{edited_outer:#?}"
    );
    assert_eq!(
        assigned.body_hash, edited_assigned.body_hash,
        "{assigned:#?}\n{edited_assigned:#?}"
    );
    let input = symbol(&result, "input", SymbolKind::Variable);
    assert_eq!(input.parent_id.as_deref(), Some(assigned.id.as_str()));
}

#[test]
fn busted_symbols_use_callback_blocks_without_call_arguments() {
    let source = r#"describe("suite", function()
    before_each(function() end)
    it("empty", function() end)
    it("nonempty", function()
        local function nested()
            return 1
        end
        assert.equal(1, nested())
    end)
    pending("no callback")
end)
"#;
    let result = extract("spec/example_spec.lua", source);
    assert!(
        result.parse_diagnostics.is_empty(),
        "{:#?}",
        result.parse_diagnostics
    );

    let suite = symbol(&result, "suite", SymbolKind::Function);
    let before_each = symbol(&result, "before_each", SymbolKind::Function);
    let empty = symbol(&result, "empty", SymbolKind::Function);
    let nonempty = symbol(&result, "nonempty", SymbolKind::Function);
    let pending = symbol(&result, "no callback", SymbolKind::Function);
    let nested = symbol(&result, "nested", SymbolKind::Function);

    assert!(before_each.body_span.is_none(), "{before_each:#?}");
    assert!(empty.body_span.is_none(), "{empty:#?}");
    assert!(pending.body_span.is_none(), "{pending:#?}");
    assert_eq!(
        body_text(nonempty, source),
        Some(
            "local function nested()\n            return 1\n        end\n        assert.equal(1, nested())"
        )
    );
    assert_eq!(nonempty.parent_id.as_deref(), Some(suite.id.as_str()));
    assert_eq!(nested.parent_id.as_deref(), Some(nonempty.id.as_str()));
    assert!(suite.body_span.is_some(), "{suite:#?}");
    assert!(nonempty.body_hash.is_some(), "{nonempty:#?}");
}
