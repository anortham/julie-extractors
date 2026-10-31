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
    $array = @()
    $i = 0
    $array[$i] = $x
    Invoke-Thing ([ref]$x)
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
    let access_at = |start: usize| {
        identifiers
            .iter()
            .find(|identifier| {
                identifier.start_byte == start as u32
                    && identifier.kind == IdentifierKind::VariableRef
            })
            .and_then(|identifier| identifier.metadata.as_ref())
            .and_then(|metadata| metadata.get("access"))
            .and_then(serde_json::Value::as_str)
    };

    for expected in [
        "graphUtil",
        "seed",
        "count",
        "total",
        "array",
        "i",
        "items",
        "item",
        "interpolated",
        "rows",
        "VisibilityUnknown",
        "ctorParam",
        "counter",
    ] {
        assert!(
            var_refs.contains(&expected),
            "expected variable_ref for {expected}; got {var_refs:?}"
        );
    }

    assert!(
        !identifiers
            .iter()
            .any(|id| id.name == "Reach" && id.kind == IdentifierKind::VariableRef),
        "method name Reach must not be a variable_ref"
    );

    assert!(
        !var_refs.iter().any(|name| name.starts_with('$')),
        "variable_ref names must not carry the $ sigil; got {var_refs:?}"
    );

    for forbidden in [
        "unusedParam",
        "GhostToken",
        "_",
        "PSItem",
        "PATH",
        "env:PATH",
        "Get-Total",
        "Reach",
        "Slot",
        "this",
        "typed",
        "Script:counter",
    ] {
        assert!(
            !var_refs.contains(&forbidden),
            "{forbidden} must NOT be a variable_ref; got {var_refs:?}"
        );
    }

    assert_eq!(
        var_refs.iter().filter(|n| **n == "ctorParam").count(),
        1,
        "ctorParam must emit exactly one variable_ref (the RHS read); got {var_refs:?}"
    );
    assert_eq!(access_at(code.find("$x = 5").unwrap()), None);
    assert_eq!(access_at(code.find("$x = 7").unwrap()), Some("write"));
    assert_eq!(
        access_at(code.find("$count +=").unwrap()),
        Some("read_write")
    );
    assert_eq!(access_at(code.find("$item in").unwrap()), Some("write"));
    assert_eq!(access_at(code.find("$array[$i]").unwrap()), Some("read"));
    assert_eq!(access_at(code.find("$i]").unwrap()), Some("read"));
    let ref_target = code.find("[ref]$x").unwrap() + "[ref]".len();
    assert_eq!(access_at(ref_target), Some("write"));

    assert!(
        !identifiers.iter().any(|id| id.name == "GhostToken"),
        "comment-only GhostToken must not be extracted at all"
    );

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
