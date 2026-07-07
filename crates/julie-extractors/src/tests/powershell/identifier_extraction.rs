//! PowerShell `variable_ref` identifier emission (locked contract in
//! csharp/identifiers.rs). PowerShell was GREENFIELD for variable_ref:
//! `variables.rs::extract_variable_reference` creates Symbols for
//! automatic/environment variables only and never emits identifiers, so
//! `$var` reads previously produced NO identifier row at all.

use crate::base::IdentifierKind;
use crate::powershell::PowerShellExtractor;
use std::path::PathBuf;
use tree_sitter::Parser;

fn extract_all(code: &str) -> (Vec<crate::base::Symbol>, Vec<crate::base::Identifier>) {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_powershell::LANGUAGE.into())
        .expect("load PowerShell grammar");
    let tree = parser.parse(code, None).expect("parse PowerShell");
    let mut ext = PowerShellExtractor::new(
        "powershell".to_string(),
        "test.ps1".to_string(),
        code.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = ext.extract_symbols(&tree);
    let identifiers = ext.extract_identifiers(&tree, &symbols);
    (symbols, identifiers)
}

#[test]
fn test_powershell_variable_ref_emission() {
    let code = r#"
# GhostToken appears only in this comment
function Get-Total {
    param([int]$seed, [int]$unusedParam)
    $count += 1
    $x = 5
    $x = 7
    $total = $seed
    $g = $graphUtil.Reach()
    Write-Output $total
    foreach ($item in $items) { $total += $item }
    $msg = "prefix $interpolated suffix"
    [hashtable]$typed = @{}
    $s2 = $Script:counter
    $p = $env:PATH
    $rows | ForEach-Object { $_.Name }
    return $VisibilityUnknown
}

class Worker {
    [int]$Slot

    Worker([int]$ctorParam) {
        $this.Slot = $ctorParam
    }
}
"#;

    let (symbols, identifiers) = extract_all(code);

    let var_refs: Vec<&str> = identifiers
        .iter()
        .filter(|id| id.kind == IdentifierKind::VariableRef)
        .map(|id| id.name.as_str())
        .collect();

    // --- Positive cases (rules 1/4) ---
    for expected in [
        "graphUtil",         // member-invocation receiver
        "seed",              // RHS read (param name matches symbol naming: no `$`)
        "count",             // compound-assignment target
        "total",             // compound target + command argument + RHS reads
        "items",             // foreach collection
        "item",              // compound-assignment RHS inside loop body
        "interpolated",      // expandable-string interpolation hole is a runtime read
        "rows",              // pipeline source
        "VisibilityUnknown", // bare return read (case preserved)
        "ctorParam",         // constructor-body RHS read (the declaration is not)
        "counter",           // scope qualifier stripped like symbol naming ($Script:counter)
    ] {
        assert!(
            var_refs.contains(&expected),
            "expected variable_ref for {expected}; got {var_refs:?}"
        );
    }

    // Receiver read coexists with the method name staying OUT of variable_ref.
    // (Method names are `member_name > simple_name` nodes, never `variable`.
    // NOTE: the pre-existing Call arm matches "invocation_expression", but this
    // grammar spells the node "invokation_expression", so `$obj.Reach()` emits
    // no Call identifier today — a pre-existing gap outside variable_ref scope,
    // reported as an open gap for Task 8.)
    assert!(
        !identifiers
            .iter()
            .any(|id| id.name == "Reach" && id.kind == IdentifierKind::VariableRef),
        "method name Reach must not be a variable_ref"
    );

    // Names follow the PowerShell symbol-naming convention: `$` sigil stripped
    // (Miller's dead-code name-match compares identifiers.name = symbols.name).
    assert!(
        !var_refs.iter().any(|name| name.starts_with('$')),
        "variable_ref names must not carry the $ sigil; got {var_refs:?}"
    );

    // --- Negative cases (rules 2/3/4/5) ---
    for forbidden in [
        "x",              // plain-write LHS only (also its implicit declaration)
        "unusedParam",    // parameter declaration only
        "GhostToken",     // comment-only mention
        "_",              // automatic pipeline variable
        "PSItem",         // automatic pipeline variable alias
        "PATH",           // environment variable ($env:PATH)
        "env:PATH",       // no qualifier-carrying spelling either
        "Get-Total",      // function declaration name
        "Reach",          // call callee, owned by the Call arm
        "Slot",           // class property declaration + member name, never a bare read
        "this",           // engine constant ($this)
        "typed",          // typed plain-write LHS ([hashtable]$typed = @{})
        "Script:counter", // qualifier must be stripped, not carried
    ] {
        assert!(
            !var_refs.contains(&forbidden),
            "{forbidden} must NOT be a variable_ref; got {var_refs:?}"
        );
    }

    // The constructor parameter declaration is excluded; only its body read emits.
    assert_eq!(
        var_refs.iter().filter(|n| **n == "ctorParam").count(),
        1,
        "ctorParam must emit exactly one variable_ref (the RHS read); got {var_refs:?}"
    );

    assert!(
        !identifiers.iter().any(|id| id.name == "GhostToken"),
        "comment-only GhostToken must not be extracted at all"
    );

    // No duplicate rows: each (name, kind, span) is unique.
    let mut keys: Vec<(String, String, u32, u32)> = identifiers
        .iter()
        .map(|id| {
            (
                id.name.clone(),
                id.kind.to_string(),
                id.start_byte,
                id.end_byte,
            )
        })
        .collect();
    let before = keys.len();
    keys.sort();
    keys.dedup();
    assert_eq!(before, keys.len(), "duplicate identifier rows detected");

    // containing_symbol_id is populated on a variable_ref.
    let get_total = symbols
        .iter()
        .find(|s| s.name == "Get-Total")
        .expect("Get-Total function extracted");
    let recv_ref = identifiers
        .iter()
        .find(|id| id.name == "graphUtil" && id.kind == IdentifierKind::VariableRef)
        .expect("graphUtil variable_ref");
    assert_eq!(
        recv_ref.containing_symbol_id.as_deref(),
        Some(get_total.id.as_str()),
        "receiver variable_ref should be contained in Get-Total"
    );
}
