use crate::ExtractionResults;
use crate::base::{IdentifierKind, RelationshipKind, Symbol, SymbolKind};
use std::path::Path;

fn extract(source: &str) -> ExtractionResults {
    crate::pipeline::extract_canonical("db/schema.sql", source, Path::new("/repo"))
        .expect("canonical sql extraction should succeed")
}

fn named<'a>(results: &'a ExtractionResults, name: &str) -> &'a Symbol {
    results
        .symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap_or_else(|| panic!("missing {name}: {:#?}", results.symbols))
}

fn has_edge(results: &ExtractionResults, kind: RelationshipKind, from: &str, to: &str) -> bool {
    let (from, to) = (named(results, from), named(results, to));
    results.relationships.iter().any(|relationship| {
        relationship.kind == kind
            && relationship.from_symbol_id == from.id
            && relationship.to_symbol_id == to.id
    })
}

fn pending_namespace(
    results: &ExtractionResults,
    kind: RelationshipKind,
    from: &str,
    target: &str,
) -> Option<Vec<String>> {
    let from = named(results, from);
    results
        .structured_pending_relationships
        .iter()
        .find(|pending| {
            pending.pending.kind == kind
                && pending.pending.from_symbol_id == from.id
                && pending.target.terminal_name == target
        })
        .map(|pending| pending.target.namespace_path.clone())
}

fn metadata_str<'a>(symbol: &'a Symbol, key: &str) -> Option<&'a str> {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(serde_json::Value::as_str)
}

#[test]
fn routine_view_trigger_and_ctas_table_references_emit_rows() {
    let source = r#"CREATE TABLE dbo.Users (Id INT PRIMARY KEY, Name NVARCHAR(100));
CREATE PROCEDURE dbo.GetUser (@UserId INT)
AS
BEGIN
    SELECT Id, Name FROM dbo.Users WHERE Id = @UserId;
    UPDATE dbo.Users SET Name = N'x' WHERE Id = @UserId;
    DELETE FROM dbo.Sessions WHERE UserId = @UserId;
    INSERT INTO audit.AccessLog (UserId) VALUES (@UserId);
END;
CREATE VIEW active_users AS SELECT Id FROM dbo.Users;
CREATE VIEW external_view AS SELECT * FROM billing.invoices;
CREATE TABLE users_backup AS SELECT * FROM dbo.Users;
CREATE TRIGGER users_touch BEFORE UPDATE ON orders FOR EACH ROW EXECUTE FUNCTION touch();
"#;
    let results = extract(source);
    assert!(has_edge(
        &results,
        RelationshipKind::References,
        "GetUser",
        "Users"
    ));
    assert!(has_edge(
        &results,
        RelationshipKind::References,
        "active_users",
        "Users"
    ));
    assert!(has_edge(
        &results,
        RelationshipKind::References,
        "users_backup",
        "Users"
    ));
    assert_eq!(
        pending_namespace(
            &results,
            RelationshipKind::References,
            "GetUser",
            "Sessions"
        ),
        Some(vec!["dbo".to_string()])
    );
    assert_eq!(
        pending_namespace(
            &results,
            RelationshipKind::References,
            "GetUser",
            "AccessLog"
        ),
        Some(vec!["audit".to_string()])
    );
    assert_eq!(
        pending_namespace(
            &results,
            RelationshipKind::References,
            "external_view",
            "invoices"
        ),
        Some(vec!["billing".to_string()])
    );
    assert_eq!(
        pending_namespace(
            &results,
            RelationshipKind::References,
            "users_touch",
            "orders"
        ),
        Some(Vec::new())
    );
    let get_user = named(&results, "GetUser");
    let table_uses: Vec<_> = results
        .identifiers
        .iter()
        .filter(|identifier| {
            identifier.kind == IdentifierKind::TypeUsage
                && identifier.containing_symbol_id.as_deref() == Some(get_user.id.as_str())
        })
        .map(|identifier| identifier.name.as_str())
        .collect();
    assert_eq!(table_uses, ["Users", "Users", "Sessions", "AccessLog"]);
}

#[test]
fn exec_function_invocation_and_trigger_execute_emit_calls() {
    let source = r#"CREATE FUNCTION recent_orders(since DATE) RETURNS SETOF orders LANGUAGE sql AS $$ SELECT * FROM orders WHERE created > since $$;
CREATE FUNCTION caller() RETURNS BIGINT LANGUAGE sql AS $$ SELECT count(*) FROM recent_orders(now()::date) $$;
CREATE FUNCTION set_updated_at() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NEW; END; $$;
CREATE TRIGGER users_set_updated_at BEFORE UPDATE ON users FOR EACH ROW EXECUTE FUNCTION set_updated_at();
CREATE PROCEDURE dbo.LogAccess (@UserId INT)
AS
BEGIN
    SELECT 1;
END;
CREATE PROCEDURE dbo.GetUser (@UserId INT)
AS
BEGIN
    EXEC dbo.LogAccess @UserId;
    EXECUTE dbo.ExternalAudit @UserId;
END;
"#;
    let results = extract(source);
    assert!(has_edge(
        &results,
        RelationshipKind::Calls,
        "caller",
        "recent_orders"
    ));
    assert!(has_edge(
        &results,
        RelationshipKind::Calls,
        "users_set_updated_at",
        "set_updated_at"
    ));
    assert!(has_edge(
        &results,
        RelationshipKind::Calls,
        "GetUser",
        "LogAccess"
    ));
    assert_eq!(
        pending_namespace(
            &results,
            RelationshipKind::Calls,
            "GetUser",
            "ExternalAudit"
        ),
        Some(vec!["dbo".to_string()])
    );
    let get_user = named(&results, "GetUser");
    assert!(results.identifiers.iter().any(|identifier| {
        identifier.kind == IdentifierKind::Call
            && identifier.name == "LogAccess"
            && identifier.containing_symbol_id.as_deref() == Some(get_user.id.as_str())
    }));
    assert!(
        !results
            .relationships
            .iter()
            .any(
                |relationship| relationship.kind == RelationshipKind::References
                    && relationship.to_symbol_id == named(&results, "recent_orders").id
            ),
        "a table-valued function in FROM is a call, not a table reference"
    );
}

#[test]
fn alter_table_constraints_and_columns_attach_to_the_table() {
    let source = r#"CREATE TABLE public.users (id bigint NOT NULL, email text NOT NULL);
CREATE TABLE public.orders (id bigint NOT NULL, user_id bigint NOT NULL);
ALTER TABLE ONLY public.orders
    ADD CONSTRAINT orders_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;
ALTER TABLE ONLY public.orders
    ADD CONSTRAINT orders_account_fkey FOREIGN KEY (user_id) REFERENCES billing.accounts(id);
ALTER TABLE public.orders ADD COLUMN status text DEFAULT 'new';
ALTER TABLE [dbo].[Orders] ADD CONSTRAINT [FK_Orders_Customers] FOREIGN KEY ([CustomerId]) REFERENCES [dbo].[Customers] ([Id]);
"#;
    let results = extract(source);
    let orders = named(&results, "orders");
    assert!(has_edge(
        &results,
        RelationshipKind::References,
        "orders",
        "users"
    ));
    assert_eq!(
        pending_namespace(&results, RelationshipKind::References, "orders", "accounts"),
        Some(vec!["billing".to_string()])
    );
    let fkey = named(&results, "orders_user_id_fkey");
    assert_eq!(fkey.kind, SymbolKind::Interface);
    assert_eq!(fkey.parent_id.as_deref(), Some(orders.id.as_str()));
    let status = named(&results, "status");
    assert_eq!(status.kind, SymbolKind::Field);
    assert_eq!(status.parent_id.as_deref(), Some(orders.id.as_str()));
    let bracketed = named(&results, "FK_Orders_Customers");
    assert_eq!(bracketed.kind, SymbolKind::Interface);
    for pattern in ["sql.column_definition.v1", "sql.foreign_key.v1"] {
        for fact in results
            .structural_facts
            .iter()
            .filter(|fact| fact.pattern_id == pattern)
        {
            assert!(
                fact.metadata
                    .as_ref()
                    .is_some_and(|metadata| metadata.contains_key("table_name")),
                "{pattern} fact without table_name: {fact:#?}"
            );
        }
    }
}

#[test]
fn routine_body_span_is_the_function_body_node() {
    let source = r#"CREATE FUNCTION active_count(min_score INT) RETURNS BIGINT
LANGUAGE sql STABLE
AS $$
  SELECT count(*)
  FROM users
  WHERE score >= min_score
    AND lower(email) LIKE '%@example.com'
$$;
CREATE PROCEDURE usp_Unqualified (@CustomerId INT)
AS
BEGIN
    SELECT COUNT(*) AS n FROM Customers WHERE IsActive = 1;
END;
CREATE TABLE t (CustomerId INT IDENTITY(1,1), CreatedAt DATETIME DEFAULT SYSUTCDATETIME());
"#;
    let results = extract(source);
    let body = |name: &str| {
        let span = named(&results, name).body_span.expect("body span");
        &source[span.start_byte as usize..span.end_byte as usize]
    };
    assert!(body("active_count").starts_with("AS $$"));
    assert!(body("active_count").ends_with("'%@example.com'\n$$"));
    assert!(body("usp_Unqualified").starts_with("AS\nBEGIN"));
    assert!(body("usp_Unqualified").ends_with("END"));
    for column in ["CustomerId", "CreatedAt"] {
        let symbol = results
            .symbols
            .iter()
            .find(|symbol| symbol.name == column && symbol.kind == SymbolKind::Field)
            .unwrap();
        assert_eq!(symbol.body_span, None, "{column}");
    }
}

#[test]
fn tsql_recovery_names_routines_and_triggers_correctly() {
    let source = r#"CREATE PROCEDURE dbo.RenameUser
    @UserId INT,
    @Name NVARCHAR(100)
AS
BEGIN
    UPDATE dbo.Users SET Name = @Name WHERE Id = @UserId;
END
GO
CREATE PROC dbo.ShortForm AS SELECT 1;
GO
CREATE TRIGGER dbo.trg_Users_Audit ON dbo.Users AFTER INSERT AS BEGIN SELECT 1; END
GO
"#;
    let results = extract(source);
    let names: Vec<_> = results
        .symbols
        .iter()
        .map(|symbol| symbol.name.as_str())
        .collect();
    for bad in ["NVARCHAR", "INT", "dbo"] {
        assert!(!names.contains(&bad), "{bad} in {names:?}");
    }
    let rename = named(&results, "RenameUser");
    for parameter in ["@UserId", "@Name"] {
        assert!(
            results.symbols.iter().any(|symbol| symbol.name == parameter
                && symbol.parent_id.as_deref() == Some(rename.id.as_str())),
            "{parameter} missing: {names:?}"
        );
    }
    named(&results, "ShortForm");
    named(&results, "trg_Users_Audit");
    let routine_names: Vec<_> = results
        .structural_facts
        .iter()
        .filter_map(|fact| fact.metadata.as_ref()?.get("routine_name")?.as_str())
        .collect();
    assert!(routine_names.contains(&"RenameUser"), "{routine_names:?}");
}

#[test]
fn alter_procedure_and_function_are_routine_symbols() {
    let source = r#"-- Recomputes nightly totals.
ALTER PROCEDURE dbo.usp_NightlyTotals (@Day DATE)
AS
BEGIN
    DELETE FROM dbo.DailyTotals WHERE Day = @Day;
END;
GO
ALTER FUNCTION dbo.fn_Label (@x INT)
RETURNS NVARCHAR(20)
AS
BEGIN
    RETURN N'label'
END;
GO
"#;
    let results = extract(source);
    let procedure = named(&results, "usp_NightlyTotals");
    assert_eq!(procedure.kind, SymbolKind::Function);
    assert!(results.symbols.iter().any(|symbol| symbol.name == "@Day"
        && symbol.parent_id.as_deref() == Some(procedure.id.as_str())));
    let function = named(&results, "fn_Label");
    assert_eq!(
        function
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("isFunction"))
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    let patterns: Vec<_> = results
        .structural_facts
        .iter()
        .map(|fact| fact.pattern_id.as_str())
        .collect();
    assert!(patterns.contains(&"sql.procedure_definition.v1"));
    assert!(patterns.contains(&"sql.function_definition.v1"));
}

#[test]
fn schema_qualifiers_are_kept_and_matched() {
    let source = r#"CREATE TABLE dbo.Users (
    Id INT PRIMARY KEY
);
CREATE TABLE audit.Events (
    Id INT PRIMARY KEY,
    UserId INT NOT NULL,
    CONSTRAINT FK_Events_Users FOREIGN KEY (UserId) REFERENCES identity.Users (Id)
);
"#;
    let results = extract(source);
    assert_eq!(
        metadata_str(named(&results, "Users"), "schema"),
        Some("dbo")
    );
    assert_eq!(
        metadata_str(named(&results, "Events"), "schema"),
        Some("audit")
    );
    assert!(!has_edge(
        &results,
        RelationshipKind::References,
        "Events",
        "Users"
    ));
    assert_eq!(
        pending_namespace(&results, RelationshipKind::References, "Events", "Users"),
        Some(vec!["identity".to_string()])
    );
}
