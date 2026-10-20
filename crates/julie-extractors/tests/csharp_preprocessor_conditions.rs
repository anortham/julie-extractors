use std::path::Path;

use julie_extractors::{ExtractionLevel, SymbolKind, extract_canonical_for_language_at};

const SOURCE: &str = r#"using System;
#if HAVE_CONCURRENT // the framework type
using System.Collections.Concurrent;
#endif

namespace App
{
#if !HAVE_CONCURRENT
    internal class ConcurrentDictionary
    {
#if HAVE_BARRIER
        void Barrier() {}
#endif
        void Add() {}
    }
#else
    internal class Other {}
#endif
    class Always {}
}
"#;

#[test]
fn csharp_symbols_in_a_kept_if_branch_carry_its_condition() {
    let results = extract_canonical_for_language_at(
        "csharp",
        "App.cs",
        SOURCE,
        Path::new("."),
        ExtractionLevel::Facts,
    )
    .unwrap();
    let condition = |name: &str, kind: SymbolKind| {
        let symbol = results
            .symbols
            .iter()
            .find(|symbol| symbol.name.ends_with(name) && symbol.kind == kind)
            .unwrap_or_else(|| panic!("{name} {kind:?}"));
        symbol
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("preprocessorIf"))
            .and_then(|value| value.as_str())
            .map(str::to_string)
    };
    assert_eq!(
        condition("Concurrent", SymbolKind::Import).as_deref(),
        Some("HAVE_CONCURRENT")
    );
    assert_eq!(condition("System", SymbolKind::Import), None);
    assert_eq!(
        condition("ConcurrentDictionary", SymbolKind::Class).as_deref(),
        Some("!HAVE_CONCURRENT")
    );
    assert_eq!(
        condition("Barrier", SymbolKind::Method).as_deref(),
        Some("!HAVE_CONCURRENT && HAVE_BARRIER")
    );
    assert_eq!(
        condition("Add", SymbolKind::Method).as_deref(),
        Some("!HAVE_CONCURRENT")
    );
    assert_eq!(condition("Always", SymbolKind::Class), None);
    assert!(results.symbols.iter().all(|symbol| symbol.name != "Other"));
}
