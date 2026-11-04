use crate::base::{NormalizedSpan, SymbolKind};
use crate::extract_canonical;
use std::path::Path;

#[test]
fn erlang_type_body_hashes_ignore_headers_and_comments_but_keep_rhs_tokens() {
    let sources = [
        "-module(shapes).\n-type pair(T) :: {ok, T}.\n",
        "-module(shapes).\r\n-type renamed(T) :: {\r\n  % café\r\n  ok, T\r\n}.\r\n",
        "-module(shapes).\n-type pair(T) :: {error, T}.\n",
    ];
    let hashes: Vec<_> = sources
        .into_iter()
        .map(|source| {
            let result = extract_canonical("shapes.erl", source, Path::new("/repo"))
                .expect("Erlang extraction");
            assert!(
                result.parse_diagnostics.is_empty(),
                "{:?}",
                result.parse_diagnostics
            );
            let types: Vec<_> = result
                .symbols
                .iter()
                .filter(|symbol| symbol.kind == SymbolKind::Type)
                .collect();
            assert_eq!(types.len(), 1);
            assert!(types[0].body_span.is_some());
            types[0].body_hash.clone().expect("type definition hash")
        })
        .collect();
    assert_eq!(hashes[0], hashes[1]);
    assert_ne!(hashes[0], hashes[2]);
}

#[test]
fn erlang_type_bodies_cover_the_exact_definition_rhs() {
    let source = concat!(
        "%% café\r\n",
        "-module(shapes).\r\n",
        "-record(account, {id}).\r\n",
        "-type account() :: #account{}.\r\n",
        "-opaque token() :: binary().\r\n",
        "-type pair(T) :: {T, [T]}.\r\n",
        "-type handler() :: fun((integer()) -> binary()).\r\n",
        "-nominal label() :: atom().\r\n",
        "-type(wrapped() :: {ok, integer()}).\r\n",
    );
    let result =
        extract_canonical("shapes.erl", source, Path::new("/repo")).expect("Erlang extraction");
    assert!(
        result.parse_diagnostics.is_empty(),
        "{:?}",
        result.parse_diagnostics
    );
    for (name, rhs) in [
        ("account", "#account{}"),
        ("token", "binary()"),
        ("pair", "{T, [T]}"),
        ("handler", "fun((integer()) -> binary())"),
        ("label", "atom()"),
        ("wrapped", "{ok, integer()}"),
    ] {
        let symbol = result
            .symbols
            .iter()
            .find(|symbol| symbol.name == name && symbol.kind == SymbolKind::Type)
            .expect(name);
        let body = symbol.body_span.expect(name);
        let start = source.find(rhs).expect(rhs);
        assert_eq!(
            source.get(body.start_byte as usize..body.end_byte as usize),
            Some(rhs),
            "{name}"
        );
        assert_eq!(
            Some(body),
            NormalizedSpan::from_content_range(source, start, start + rhs.len()),
            "{name}"
        );
        assert_eq!(
            symbol.body_hash,
            crate::base::body::body_hash(source, body, "erlang"),
            "{name}"
        );
    }
}
