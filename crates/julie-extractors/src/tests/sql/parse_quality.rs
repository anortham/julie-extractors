use crate::base::ParseDiagnosticKind;
use crate::pipeline::parse_diagnostics_for_tree;
use tree_sitter::Parser;

fn parse_sql_diagnostics(source: &str) -> Vec<crate::base::ParseDiagnostic> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_sequel::LANGUAGE.into())
        .expect("Error loading SQL grammar");
    let tree = parser
        .parse(source, None)
        .expect("SQL parse should return a tree (possibly with ERROR nodes)");
    parse_diagnostics_for_tree(&tree)
}

fn assert_clean_parse(label: &str, source: &str) {
    let diagnostics = parse_sql_diagnostics(source);
    assert!(
        diagnostics.is_empty(),
        "{label}: expected zero parse diagnostics, got {} ({:?})",
        diagnostics.len(),
        diagnostics
    );
}

fn assert_has_parse_diagnostic(label: &str, source: &str) {
    let diagnostics = parse_sql_diagnostics(source);
    assert!(
        !diagnostics.is_empty(),
        "{label}: expected at least one parse diagnostic for malformed SQL"
    );
    assert!(
        diagnostics.iter().any(|d| matches!(
            d.kind,
            ParseDiagnosticKind::Error | ParseDiagnosticKind::Missing
        )),
        "{label}: expected error or missing diagnostic, got {:?}",
        diagnostics
    );
}

#[test]
fn tsql_bracketed_multipart_table_name_parses_cleanly() {
    assert_clean_parse(
        "bracketed multipart name",
        "CREATE TABLE [edr].[EdrForms] ([Id] int NOT NULL);",
    );
}

#[test]
fn tsql_go_batch_separator_parses_cleanly() {
    assert_clean_parse(
        "GO batch separator",
        "CREATE TABLE dbo.T (Id int);\nGO\nCREATE TABLE dbo.U (Id int);",
    );
}

#[test]
fn tsql_identity_bare_and_seeded_parses_cleanly() {
    assert_clean_parse(
        "IDENTITY bare",
        "CREATE TABLE dbo.T ([Id] int NOT NULL IDENTITY);",
    );
    assert_clean_parse(
        "IDENTITY seeded",
        "CREATE TABLE dbo.T ([Id] int NOT NULL IDENTITY(1, 1));",
    );
}

#[test]
fn tsql_max_length_types_parses_cleanly() {
    assert_clean_parse(
        "nvarchar(max)",
        "CREATE TABLE dbo.T ([Note] nvarchar(max) NOT NULL);",
    );
    assert_clean_parse(
        "varbinary(max)",
        "CREATE TABLE dbo.T ([Payload] varbinary(max) NULL);",
    );
}

#[test]
fn tsql_computed_persisted_column_parses_cleanly() {
    assert_clean_parse(
        "computed persisted column",
        "CREATE TABLE dbo.T (\n  [AdGroup] nvarchar(256) NOT NULL,\n  [AdGroupNormalized] AS UPPER([AdGroup]) PERSISTED\n);",
    );
}

#[test]
fn tsql_named_inline_constraints_parses_cleanly() {
    assert_clean_parse(
        "named inline PK",
        "CREATE TABLE dbo.T (\n  [A] int NOT NULL,\n  [B] int NOT NULL,\n  CONSTRAINT PK_T PRIMARY KEY ([A], [B])\n);",
    );
    assert_clean_parse(
        "named inline DEFAULT",
        "CREATE TABLE dbo.T (\n  [Status] nvarchar(50) NOT NULL CONSTRAINT DF_T_Status DEFAULT (N'open')\n);",
    );
}

#[test]
fn tsql_set_statements_parses_cleanly() {
    assert_clean_parse("SET NOCOUNT", "SET NOCOUNT ON;");
    assert_clean_parse("SET XACT_ABORT", "SET XACT_ABORT ON;");
}

#[test]
fn tsql_variable_assignments_support_compound_operators() {
    assert_clean_parse(
        "T-SQL variable assignments",
        "CREATE PROCEDURE dbo.p @Balance int AS BEGIN SET @Balance = @Balance + 1; SET @Balance += 1; END;",
    );
}

#[test]
fn tsql_malformed_compound_assignment_stays_diagnostic() {
    assert_has_parse_diagnostic(
        "malformed T-SQL compound assignment",
        "CREATE PROCEDURE dbo.p @Balance int AS BEGIN SET @Balance +== 1; END;",
    );
}

#[test]
fn tsql_if_object_id_and_exists_parses_cleanly() {
    assert_clean_parse(
        "IF OBJECT_ID",
        "IF OBJECT_ID(N'dbo.T', N'U') IS NOT NULL DROP TABLE dbo.T;",
    );
    assert_clean_parse(
        "IF SCHEMA_ID",
        "IF SCHEMA_ID(N'edr') IS NULL EXEC(N'CREATE SCHEMA [edr];');",
    );
    assert_clean_parse(
        "IF COL_LENGTH",
        "IF COL_LENGTH('dbo.T', 'Col') IS NULL ALTER TABLE dbo.T ADD Col int;",
    );
    assert_clean_parse(
        "IF NOT EXISTS",
        "IF NOT EXISTS (SELECT 1 FROM sys.tables WHERE name = 'T') CREATE TABLE dbo.T (Id int);",
    );
}

#[test]
fn tsql_begin_end_block_parses_cleanly() {
    assert_clean_parse(
        "BEGIN/END",
        "IF OBJECT_ID(N'dbo.T', N'U') IS NULL\nBEGIN\n  CREATE TABLE dbo.T (Id int NOT NULL);\nEND",
    );
}

#[test]
fn tsql_declare_with_initializer_parses_cleanly() {
    assert_clean_parse(
        "DECLARE with initializer",
        "DECLARE @AdGroup NVARCHAR(256) = N'example';",
    );
}

#[test]
fn tsql_table_variable_declaration_after_statement_parses_cleanly() {
    assert_clean_parse(
        "table variable after SET",
        "CREATE PROCEDURE dbo.p AS BEGIN SET NOCOUNT ON; DECLARE @Rows AS TABLE (Id int PRIMARY KEY); SELECT 1; END;",
    );
}

#[test]
fn tsql_declaration_initializer_accepts_function_calls() {
    assert_clean_parse(
        "variable initializer function call",
        "CREATE PROCEDURE dbo.p AS BEGIN SET NOCOUNT ON; DECLARE @StartedWhen datetime2(7) = SYSDATETIME(); SELECT @StartedWhen; END;",
    );
}

#[test]
fn tsql_insert_output_parses_cleanly() {
    assert_clean_parse(
        "INSERT OUTPUT",
        "INSERT dbo.T (Id) OUTPUT inserted.Id AS Id VALUES (1);",
    );
}

#[test]
fn tsql_system_variables_parse_cleanly() {
    assert_clean_parse("system variable", "SELECT @@ROWCOUNT;");
}

#[test]
fn tsql_select_assigns_into_a_variable() {
    assert_clean_parse(
        "SELECT TOP variable assignment",
        "CREATE PROCEDURE dbo.p @Value int AS BEGIN SELECT TOP 1 @Value = t.Value FROM dbo.T AS t; END;",
    );
}

#[test]
fn tsql_if_statement_inside_procedure_parses_cleanly() {
    assert_clean_parse(
        "IF inside procedure",
        "CREATE PROCEDURE dbo.p AS BEGIN IF @@ROWCOUNT = 0 SELECT 1; END;",
    );
}

#[test]
fn tsql_native_compilation_procedure_options_parse_cleanly() {
    assert_clean_parse(
        "native compilation procedure options",
        "CREATE PROCEDURE dbo.p @Batch AS dbo.MeterRows READONLY, @BatchSize int WITH NATIVE_COMPILATION, SCHEMABINDING AS BEGIN SELECT @BatchSize; END;",
    );
}

#[test]
fn tsql_atomic_block_options_parse_cleanly() {
    assert_clean_parse(
        "atomic procedure block options",
        "CREATE PROCEDURE dbo.p AS BEGIN ATOMIC WITH (TRANSACTION ISOLATION LEVEL=SNAPSHOT, LANGUAGE=N'English') SELECT 1; END;",
    );
}

#[test]
fn tsql_atomic_compilation_procedure_parses_cleanly() {
    assert_clean_parse(
        "native compiled atomic procedure",
        "CREATE PROCEDURE dbo.p @Batch AS dbo.MeterRows READONLY, @BatchSize int WITH NATIVE_COMPILATION, SCHEMABINDING AS BEGIN ATOMIC WITH (TRANSACTION ISOLATION LEVEL=SNAPSHOT, LANGUAGE=N'English') INSERT INTO dbo.MeterMeasurement (MeterID) SELECT MeterID FROM @Batch; END;",
    );
}

#[test]
fn tsql_atomic_block_allows_an_unterminated_final_statement() {
    assert_clean_parse(
        "atomic block without final statement terminator",
        "CREATE PROCEDURE dbo.p @Batch AS dbo.MeterRows READONLY WITH NATIVE_COMPILATION, SCHEMABINDING AS BEGIN ATOMIC WITH (TRANSACTION ISOLATION LEVEL=SNAPSHOT, LANGUAGE=N'English') INSERT INTO dbo.MeterMeasurement (MeterID) SELECT MeterID FROM @Batch END;",
    );
}

#[test]
fn tsql_atomic_block_without_end_stays_diagnostic() {
    assert_has_parse_diagnostic(
        "atomic block missing END",
        "CREATE PROCEDURE dbo.p AS BEGIN ATOMIC WITH (TRANSACTION ISOLATION LEVEL=SNAPSHOT, LANGUAGE=N'English') SELECT 1;",
    );
}

#[test]
fn tsql_utf8_bom_before_procedure_parses_cleanly() {
    assert_clean_parse(
        "UTF-8 BOM before T-SQL procedure",
        "\u{feff}CREATE PROCEDURE dbo.p @Id int AS BEGIN SELECT @Id; END;",
    );
}

#[test]
fn tsql_inline_select_with_where_clause_and_nested_role_query_parses_cleanly() {
    assert_clean_parse(
        "inline TVF WHERE and nested role query",
        "CREATE FUNCTION dbo.f(@CustomerID int) RETURNS TABLE AS RETURN (SELECT 1 AS AccessResult WHERE IS_ROLEMEMBER(N'db_owner') <> 0 OR IS_ROLEMEMBER((SELECT u.RoleName FROM dbo.Users AS u WHERE u.CustomerID = @CustomerID)) <> 0);",
    );
}

#[test]
fn tsql_if_not_exists_join_and_try_catch_parse_cleanly() {
    assert_clean_parse(
        "IF NOT EXISTS with joins and TRY/CATCH",
        "CREATE PROCEDURE dbo.p @RoleName sysname, @UserName sysname AS BEGIN IF NOT EXISTS (SELECT 1 FROM sys.database_role_members AS drm INNER JOIN sys.database_principals AS dpr ON drm.role_principal_id = dpr.principal_id AND dpr.type = N'R' INNER JOIN sys.database_principals AS dpu ON drm.member_principal_id = dpu.principal_id AND dpu.type = N'S' WHERE dpr.name = @RoleName AND dpu.name = @UserName) BEGIN BEGIN TRY EXECUTE (N'ALTER ROLE'); END TRY BEGIN CATCH THROW; END CATCH; END; END;",
    );
}

#[test]
fn tsql_throw_statement_parses_cleanly() {
    assert_clean_parse("THROW", "THROW 50001, N'validation failed', 1;");
}

#[test]
fn tsql_merge_using_values_parses_cleanly() {
    assert_clean_parse(
        "MERGE USING VALUES",
        r#"MERGE dbo.Target AS t
USING (VALUES (N'a', N'b')) AS s (ColA, ColB)
ON t.ColA = s.ColA
WHEN NOT MATCHED THEN INSERT (ColA, ColB) VALUES (s.ColA, s.ColB);"#,
    );
}

#[test]
fn tsql_supported_unicode_literal_parses_cleanly() {
    assert_clean_parse(
        "Unicode N' literal",
        "INSERT INTO dbo.T (Name) VALUES (N'example');",
    );
}

#[test]
fn tsql_supported_rowversion_parses_cleanly() {
    assert_clean_parse(
        "rowversion column",
        "ALTER TABLE dbo.T ADD RowVer rowversion NOT NULL;",
    );
}

#[test]
fn tsql_supported_alter_add_parses_cleanly() {
    assert_clean_parse(
        "ALTER TABLE ADD",
        "ALTER TABLE dbo.T ADD ExtraCol int NULL;",
    );
}

#[test]
fn tsql_supported_drop_create_index_parses_cleanly() {
    assert_clean_parse("DROP INDEX", "DROP INDEX IX_T_Col ON dbo.T;");
    assert_clean_parse(
        "CREATE UNIQUE INDEX",
        "CREATE UNIQUE INDEX IX_T_Col ON dbo.T (Col);",
    );
}

#[test]
fn tsql_nonclustered_index_parses_cleanly() {
    assert_clean_parse(
        "CREATE NONCLUSTERED INDEX",
        "CREATE NONCLUSTERED INDEX IX_T_Col ON dbo.T (Col ASC);",
    );
}

#[test]
fn tsql_supported_add_fk_parses_cleanly() {
    assert_clean_parse(
        "ADD CONSTRAINT FK",
        "ALTER TABLE dbo.Child ADD CONSTRAINT FK_Child_Parent FOREIGN KEY (ParentId) REFERENCES dbo.Parent (Id);",
    );
}

#[test]
fn tsql_supported_drop_schema_parses_cleanly() {
    assert_clean_parse("DROP SCHEMA", "DROP SCHEMA IF EXISTS scratch;");
}

#[test]
fn tsql_malformed_bracket_identifier_still_diagnostic() {
    assert_has_parse_diagnostic(
        "unclosed bracket identifier",
        "CREATE TABLE [dbo.T (Id int);",
    );
}

#[test]
fn tsql_malformed_unterminated_begin_still_diagnostic() {
    assert_has_parse_diagnostic("unterminated BEGIN", "BEGIN\n  SELECT 1;");
}

#[test]
fn tsql_malformed_identity_args_still_diagnostic() {
    assert_has_parse_diagnostic(
        "invalid IDENTITY args",
        "CREATE TABLE dbo.T (Id int IDENTITY(1,));",
    );
}

#[test]
fn tsql_malformed_merge_still_diagnostic() {
    assert_has_parse_diagnostic(
        "malformed MERGE",
        "MERGE dbo.T USING (VALUES (1)) AS s (Id) ON 1=1 WHEN;",
    );
}

#[test]
fn tsql_malformed_throw_still_diagnostic() {
    assert_has_parse_diagnostic("incomplete THROW", "THROW 50001, N'message';");
}

#[test]
fn tsql_coverage_regressions_fixture_parses_cleanly() {
    assert_clean_parse(
        "SQL coverage regressions",
        include_str!("../../../../../fixtures/extraction/sql/tsql_coverage_regressions/source.sql"),
    );
}

#[test]
fn tsql_procedure_try_catch_transactions_parse_cleanly() {
    assert_clean_parse(
        "procedure transaction control flow",
        r#"CREATE PROCEDURE dbo.p
@Input int
WITH EXECUTE AS OWNER
AS
BEGIN
    DECLARE @Rows AS TABLE (Id int PRIMARY KEY);
    BEGIN TRY
        BEGIN TRAN;
        SELECT @Input;
        COMMIT;
    END TRY
    BEGIN CATCH
        IF XACT_STATE() <> 0 ROLLBACK;
        PRINT N'failed';
        THROW;
        RETURN -1;
    END CATCH;
END;"#,
    );
}

#[test]
fn tsql_malformed_system_time_period_still_diagnostic() {
    assert_has_parse_diagnostic(
        "malformed system time period",
        "CREATE TABLE dbo.T (ValidFrom datetime2, PERIOD FOR SYSTEM_TIME (ValidFrom, ));",
    );
}

#[test]
fn tsql_incomplete_execute_as_owner_still_diagnostic() {
    assert_has_parse_diagnostic(
        "incomplete EXECUTE AS option",
        "CREATE PROCEDURE dbo.p @Id int WITH EXECUTE AS AS BEGIN SELECT @Id; END;",
    );
}

#[test]
fn tsql_incomplete_inline_tvf_case_still_diagnostic() {
    assert_has_parse_diagnostic(
        "incomplete inline TVF CASE",
        "CREATE FUNCTION dbo.f() RETURNS TABLE AS RETURN SELECT CASE WHEN THEN 1 END;",
    );
}
