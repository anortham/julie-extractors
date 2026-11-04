use crate::base::{ExtractionResults, Identifier, IdentifierKind};
use crate::extract_canonical;
use std::path::Path;

fn extract(source: &str) -> ExtractionResults {
    extract_canonical("sql/access.sql", source, Path::new("/repo")).expect("SQL extraction")
}

fn occurrence(source: &str, needle: &str, index: usize) -> usize {
    source
        .match_indices(needle)
        .nth(index)
        .map(|(start, _)| start)
        .unwrap_or_else(|| panic!("missing occurrence {index} of {needle:?}"))
}

fn identifier_at(results: &ExtractionResults, start: usize, kind: IdentifierKind) -> &Identifier {
    results
        .identifiers
        .iter()
        .find(|identifier| identifier.start_byte as usize == start && identifier.kind == kind)
        .unwrap_or_else(|| panic!("missing {kind:?} identifier at byte {start}"))
}

fn owner<'a>(results: &'a ExtractionResults, identifier: &Identifier) -> Option<&'a str> {
    let id = identifier.containing_symbol_id.as_deref()?;
    results
        .symbols
        .iter()
        .find(|symbol| symbol.id == id)
        .map(|symbol| symbol.name.as_str())
}

fn access(identifier: &Identifier) -> Option<&str> {
    identifier
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("access"))
        .and_then(serde_json::Value::as_str)
}

#[test]
fn procedure_variables_and_columns_distinguish_reads_writes_compounds_and_owners() {
    let source = concat!(
        "CREATE TABLE ledger (balance INT);\n",
        "GO\n",
        "CREATE PROCEDURE ChangeBalance @balance INT AS\n",
        "BEGIN\n",
        "  SET @balance = @balance + 1;\n",
        "  SET @balance += 1;\n",
        "  UPDATE ledger SET balance = @balance;\n",
        "  EXEC Helper @balance;\n",
        "END;\n",
        "GO\n",
        "CREATE PROCEDURE Helper @balance INT AS\n",
        "BEGIN\n",
        "  SELECT @balance;\n",
        "END;\n",
    );
    let results = extract(source);

    assert!(
        results.parse_diagnostics.is_empty(),
        "{:?}",
        results.parse_diagnostics
    );

    let write = identifier_at(
        &results,
        occurrence(source, "@balance =", 0),
        IdentifierKind::VariableRef,
    );
    let right_read = identifier_at(
        &results,
        occurrence(source, "@balance + 1", 0),
        IdentifierKind::VariableRef,
    );
    let compound = identifier_at(
        &results,
        occurrence(source, "@balance +=", 0),
        IdentifierKind::VariableRef,
    );
    let column_write = identifier_at(
        &results,
        occurrence(source, "UPDATE ledger SET balance", 0) + "UPDATE ledger SET ".len(),
        IdentifierKind::MemberAccess,
    );
    let helper_read = identifier_at(
        &results,
        occurrence(source, "@balance;", 2),
        IdentifierKind::VariableRef,
    );

    assert_eq!(access(write), Some("write"));
    assert_eq!(access(right_read), Some("read"));
    assert_eq!(access(compound), Some("read_write"));
    assert_eq!(access(column_write), Some("write"));
    assert_eq!(access(helper_read), Some("read"));
    assert_eq!(owner(&results, write), Some("ChangeBalance"));
    assert_eq!(owner(&results, right_read), Some("ChangeBalance"));
    assert_eq!(owner(&results, compound), Some("ChangeBalance"));
    assert_eq!(owner(&results, column_write), Some("ChangeBalance"));
    assert_eq!(owner(&results, helper_read), Some("Helper"));

    let call = identifier_at(
        &results,
        occurrence(source, "EXEC Helper", 0) + "EXEC ".len(),
        IdentifierKind::Call,
    );
    let type_usage = identifier_at(
        &results,
        occurrence(source, "UPDATE ledger", 0) + "UPDATE ".len(),
        IdentifierKind::TypeUsage,
    );
    assert_eq!(access(call), None);
    assert_eq!(access(type_usage), None);

    for identifier in [
        write,
        right_read,
        compound,
        column_write,
        helper_read,
        call,
        type_usage,
    ] {
        assert_eq!(
            &source[identifier.start_byte as usize..identifier.end_byte as usize],
            identifier.name
        );
    }
}
