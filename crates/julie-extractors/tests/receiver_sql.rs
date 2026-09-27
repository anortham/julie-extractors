use julie_extractors::{
    ExtractionLevel, ExtractionResults, IdentifierKind, extract_canonical_for_language_at,
};
use std::path::Path;

#[test]
fn sql_qualified_calls_keep_receiver_parts_from_the_parsed_reference() {
    let source = r#"CREATE FUNCTION recent_orders(since DATE) RETURNS SETOF orders LANGUAGE sql AS $$ SELECT * FROM orders $$;
CREATE FUNCTION caller() RETURNS BIGINT LANGUAGE sql AS $$ SELECT count(*) FROM public.recent_orders(now()::date) $$;
CREATE FUNCTION "my.schema".lookup_user() RETURNS INT LANGUAGE sql AS $$ SELECT 1 $$;
CREATE FUNCTION quote_caller() RETURNS INT LANGUAGE sql AS $$ SELECT "my.schema".lookup_user() $$;
CREATE TABLE dbo.Orders (Id INT NOT NULL);
CREATE TRIGGER trg_Orders_Audit ON dbo.Orders AFTER INSERT AS BEGIN EXECUTE FUNCTION public.touch(); END;
CREATE PROCEDURE dbo.usp_WriteAudit @Entity NVARCHAR(100) AS BEGIN SELECT 1; END;
CREATE PROCEDURE dbo.usp_Drain @Batch INT AS BEGIN SELECT 1; END;
CREATE PROCEDURE dbo.GetUser @UserId INT AS BEGIN
    EXEC dbo.usp_WriteAudit N'order';
    EXECUTE billing.usp_Charge @UserId, @Total;
    EXEC dbo.audit.usp_Check @UserId;
    EXEC tSQLt.NewTestClass 'OrderTests';
    EXEC tSQLt.FakeTable 'dbo.Orders';
    EXEC dbo.usp_Drain @Batch = 10;
END;
CREATE TRIGGER trg_Order_Write ON dbo.Orders AFTER INSERT AS BEGIN EXEC dbo.usp_WriteAudit @Entity = N'Order'; END;
"#;
    let results = extract_canonical_for_language_at(
        "sql",
        "source.sql",
        source,
        Path::new("."),
        ExtractionLevel::Full,
    )
    .unwrap();

    for (context, name, receiver, qualifier) in [
        ("FROM public.recent_orders", "recent_orders", "public", None),
        ("FUNCTION public.touch", "touch", "public", None),
        (
            "EXEC dbo.usp_WriteAudit N'order'",
            "usp_WriteAudit",
            "dbo",
            None,
        ),
        ("EXECUTE billing.usp_Charge", "usp_Charge", "billing", None),
        (
            "EXEC dbo.audit.usp_Check",
            "usp_Check",
            "audit",
            Some("dbo"),
        ),
        ("EXEC tSQLt.NewTestClass", "NewTestClass", "tSQLt", None),
        ("EXEC tSQLt.FakeTable", "FakeTable", "tSQLt", None),
        ("EXEC dbo.usp_Drain", "usp_Drain", "dbo", None),
        (
            "EXEC dbo.usp_WriteAudit @Entity",
            "usp_WriteAudit",
            "dbo",
            None,
        ),
        (
            "SELECT \"my.schema\".lookup_user",
            "lookup_user",
            "my.schema",
            None,
        ),
    ] {
        assert_call_receiver(&results, source, context, name, receiver, qualifier);
    }
}

fn assert_call_receiver(
    results: &ExtractionResults,
    source: &str,
    context: &str,
    name: &str,
    expected_receiver: &str,
    expected_qualifier: Option<&str>,
) {
    let context_start = source
        .find(context)
        .unwrap_or_else(|| panic!("missing {context}"));
    let name_start = context_start + context.rfind(name).unwrap();
    let identifier = results
        .identifiers
        .iter()
        .find(|identifier| {
            identifier.kind == IdentifierKind::Call
                && identifier.name == name
                && identifier.start_byte as usize == name_start
        })
        .unwrap_or_else(|| panic!("missing {name} call at {name_start}"));
    let end = name_start + name.len();
    assert_eq!(identifier.end_byte as usize, end);
    assert_eq!(&source[name_start..end], name);
    let prefix = &source[..name_start];
    let expected_line = prefix.bytes().filter(|byte| *byte == b'\n').count() as u32 + 1;
    let expected_column = prefix.rsplit('\n').next().unwrap().len() as u32;
    assert_eq!(identifier.start_line, expected_line);
    assert_eq!(identifier.end_line, expected_line);
    assert_eq!(identifier.start_column, expected_column);
    assert_eq!(identifier.end_column, expected_column + name.len() as u32);
    let metadata = identifier.metadata.as_ref();
    assert_eq!(
        metadata
            .and_then(|metadata| metadata.get("receiver"))
            .and_then(|value| value.as_str()),
        Some(expected_receiver),
        "{context}"
    );
    assert_eq!(
        metadata
            .and_then(|metadata| metadata.get("receiver_qualifier"))
            .and_then(|value| value.as_str()),
        expected_qualifier,
        "{context}"
    );
}
