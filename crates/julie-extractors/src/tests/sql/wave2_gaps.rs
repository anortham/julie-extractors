use std::path::Path;

use crate::ExtractionResults;
use crate::base::{IdentifierKind, RelationshipKind, SourceRegionKind, StructuralFact, Symbol};
use crate::tests::helpers::{facts_with_pattern, metadata_str};

fn extract(source: &str) -> ExtractionResults {
    crate::pipeline::extract_canonical("schema.sql", source, Path::new("/repo"))
        .expect("canonical SQL extraction should succeed")
}

fn symbol<'a>(results: &'a ExtractionResults, name: &str) -> &'a Symbol {
    results
        .symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap_or_else(|| {
            let names: Vec<_> = results.symbols.iter().map(|s| s.name.as_str()).collect();
            panic!("missing symbol {name}: {names:?}")
        })
}

fn child<'a>(results: &'a ExtractionResults, parent: &str, name: &str) -> &'a Symbol {
    let parent_id = &symbol(results, parent).id;
    results
        .symbols
        .iter()
        .find(|s| s.name == name && s.parent_id.as_ref() == Some(parent_id))
        .unwrap_or_else(|| panic!("missing {parent}.{name}"))
}

fn doc(symbol: &Symbol) -> Option<&str> {
    symbol.doc_comment.as_deref()
}

fn meta_bool(symbol: &Symbol, key: &str) -> Option<bool> {
    symbol.metadata.as_ref()?.get(key)?.as_bool()
}

fn type_of<'a>(results: &'a ExtractionResults, symbol: &Symbol) -> Option<&'a str> {
    results
        .types
        .get(&symbol.id)
        .map(|info| info.resolved_type.as_str())
}

fn declared_type<'a>(results: &'a ExtractionResults, symbol: &Symbol) -> Option<&'a str> {
    results
        .types
        .get(&symbol.id)?
        .metadata
        .as_ref()?
        .get("declared")?
        .as_str()
}

fn fact_strings(fact: &StructuralFact, key: &str) -> Vec<String> {
    fact.metadata
        .as_ref()
        .and_then(|m| m.get(key))
        .map_or_else(Vec::new, |value| {
            value
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_string())
                .collect()
        })
}

fn fact_u64(fact: &StructuralFact, key: &str) -> Option<u64> {
    fact.metadata.as_ref()?.get(key)?.as_u64()
}

fn fact_bool(fact: &StructuralFact, key: &str) -> Option<bool> {
    fact.metadata.as_ref()?.get(key)?.as_bool()
}

fn identifiers<'a>(results: &'a ExtractionResults, name: &str) -> Vec<&'a crate::base::Identifier> {
    results
        .identifiers
        .iter()
        .filter(|identifier| identifier.name == name)
        .collect()
}

#[test]
fn container_doc_comments_do_not_reach_columns_or_parameters() {
    let results = extract(
        "-- Registered users of the portal.\nCREATE TABLE users (\n    id INTEGER PRIMARY KEY,\n    email TEXT NOT NULL, -- Login address.\n    -- Display name shown in the header.\n    display_name TEXT\n);\n-- Looks up a user by email.\nCREATE FUNCTION find_user(p_email TEXT) RETURNS INTEGER LANGUAGE sql AS $$ SELECT 1 $$;\n",
    );

    assert_eq!(
        doc(symbol(&results, "users")),
        Some("-- Registered users of the portal.")
    );
    assert_eq!(doc(child(&results, "users", "id")), None);
    assert_eq!(
        doc(child(&results, "users", "email")),
        Some("-- Login address.")
    );
    assert_eq!(
        doc(child(&results, "users", "display_name")),
        Some("-- Display name shown in the header.")
    );
    assert_eq!(
        doc(symbol(&results, "find_user")),
        Some("-- Looks up a user by email.")
    );
    assert_eq!(doc(symbol(&results, "p_email")), None);

    let email = child(&results, "users", "email");
    let trailing = results
        .source_regions
        .iter()
        .find(|region| region.kind == SourceRegionKind::DocComment && region.start_line == 4)
        .expect("trailing doc region");
    assert_eq!(trailing.containing_symbol_id.as_ref(), Some(&email.id));
}

#[test]
fn comment_on_statements_and_mysql_comment_attributes_are_docs() {
    let results = extract(
        "CREATE TABLE invoices (id BIGINT PRIMARY KEY, due_date DATE NOT NULL);\nCOMMENT ON TABLE invoices IS 'Customer invoices issued by billing';\nCOMMENT ON COLUMN invoices.due_date IS 'Date payment is due';\nCREATE FUNCTION overdue_count() RETURNS BIGINT LANGUAGE sql AS $$ SELECT 1 $$;\nCOMMENT ON FUNCTION overdue_count() IS 'Number of overdue invoices';\nCREATE TABLE app_users (id INT COMMENT 'Primary key') COMMENT='Application users';\n",
    );

    assert_eq!(
        doc(symbol(&results, "invoices")),
        Some("Customer invoices issued by billing")
    );
    assert_eq!(
        doc(child(&results, "invoices", "due_date")),
        Some("Date payment is due")
    );
    assert_eq!(
        doc(symbol(&results, "overdue_count")),
        Some("Number of overdue invoices")
    );
    assert_eq!(
        doc(symbol(&results, "app_users")),
        Some("Application users")
    );
    assert_eq!(doc(child(&results, "app_users", "id")), Some("Primary key"));
}

#[test]
fn type_facts_come_from_the_grammar_type_nodes() {
    let results = extract(
        "CREATE TABLE t (\n  a INT, b int, c NVARCHAR(50), d TIMESTAMPTZ, h order_status, m TEXT[]\n);\nCREATE FUNCTION f(x INT, IN y text DEFAULT 'a') RETURNS TEXT LANGUAGE sql AS $$ SELECT 'a' $$;\nCREATE FUNCTION recent_orders(since DATE) RETURNS SETOF orders LANGUAGE sql AS $$ SELECT 1 $$;\nCREATE FUNCTION compute_balance(p BIGINT) RETURNS NUMERIC(12,2) LANGUAGE sql AS $$ SELECT 0 $$;\n",
    );

    for (name, expected) in [
        ("a", "INT"),
        ("b", "int"),
        ("c", "NVARCHAR"),
        ("d", "TIMESTAMPTZ"),
        ("h", "order_status"),
        ("m", "TEXT"),
        ("x", "INT"),
        ("y", "text"),
        ("f", "TEXT"),
        ("recent_orders", "orders"),
        ("compute_balance", "NUMERIC"),
    ] {
        assert_eq!(
            type_of(&results, symbol(&results, name)),
            Some(expected),
            "{name}"
        );
    }
    assert_eq!(
        declared_type(&results, symbol(&results, "c")),
        Some("NVARCHAR(50)")
    );
    assert_eq!(
        declared_type(&results, symbol(&results, "recent_orders")),
        Some("SETOF orders")
    );
    assert_eq!(type_of(&results, symbol(&results, "t")), Some("TABLE"));

    let balance = symbol(&results, "compute_balance");
    assert_eq!(
        balance.signature.as_deref(),
        Some("CREATE FUNCTION compute_balance(p BIGINT) RETURNS NUMERIC(12,2) LANGUAGE sql")
    );
    assert_eq!(meta_bool(balance, "isStoredProcedure"), Some(false));
}

#[test]
fn materialized_views_are_views_with_sources_facts_and_columns() {
    let results = extract(
        "CREATE TABLE orders (id INT PRIMARY KEY, user_id INT, total NUMERIC);\n-- Daily revenue rollup.\nCREATE MATERIALIZED VIEW daily_revenue AS\nSELECT user_id, sum(total) AS revenue\nFROM orders\nGROUP BY user_id\nWITH DATA;\n",
    );

    let view = symbol(&results, "daily_revenue");
    assert_eq!(meta_bool(view, "isView"), Some(true));
    assert_eq!(meta_bool(view, "isMaterialized"), Some(true));
    assert_eq!(doc(view), Some("-- Daily revenue rollup."));
    assert_eq!(
        child(&results, "daily_revenue", "revenue")
            .parent_id
            .as_ref(),
        Some(&view.id)
    );
    let orders = symbol(&results, "orders");
    assert!(results.relationships.iter().any(|relationship| {
        relationship.kind == RelationshipKind::References
            && relationship.from_symbol_id == view.id
            && relationship.to_symbol_id == orders.id
    }));
    let facts = facts_with_pattern(&results, "sql.view_definition.v1");
    assert_eq!(facts.len(), 1);
    assert_eq!(fact_bool(facts[0], "materialized"), Some(true));
    assert_eq!(fact_strings(facts[0], "source_tables"), vec!["orders"]);
}

#[test]
fn postgres_ddl_objects_emit_facts() {
    let results = extract(
        "CREATE POLICY tenant_isolation ON invoices USING (true);\nCREATE EXTENSION IF NOT EXISTS pgcrypto;\nCREATE SEQUENCE invoice_seq START 1000;\nCREATE TYPE order_status AS ENUM ('new', 'paid');\nCREATE ROLE reporting;\n",
    );

    let policy = facts_with_pattern(&results, "sql.policy_definition.v1");
    assert_eq!(
        metadata_str(policy[0], "policy_name"),
        Some("tenant_isolation")
    );
    assert_eq!(metadata_str(policy[0], "table_name"), Some("invoices"));
    let extension = facts_with_pattern(&results, "sql.extension.v1");
    assert_eq!(
        metadata_str(extension[0], "extension_name"),
        Some("pgcrypto")
    );
    assert_eq!(fact_bool(extension[0], "if_not_exists"), Some(true));
    let sequence = facts_with_pattern(&results, "sql.sequence_definition.v1");
    assert_eq!(
        metadata_str(sequence[0], "sequence_name"),
        Some("invoice_seq")
    );
    assert_eq!(fact_u64(sequence[0], "start"), Some(1000));
    let enum_type = facts_with_pattern(&results, "sql.type_definition.v1");
    assert_eq!(metadata_str(enum_type[0], "type_kind"), Some("enum"));
    assert_eq!(
        fact_strings(enum_type[0], "enum_values"),
        vec!["new", "paid"]
    );
    let role = facts_with_pattern(&results, "sql.role_definition.v1");
    assert_eq!(metadata_str(role[0], "role_name"), Some("reporting"));
}

#[test]
fn tsqlt_test_classes_mark_tests_and_setup() {
    let results = extract(
        "EXEC tSQLt.NewTestClass 'OrderTests';\nGO\nCREATE PROCEDURE OrderTests.SetUp AS BEGIN EXEC tSQLt.FakeTable 'dbo.Orders'; END;\nGO\nCREATE PROCEDURE OrderTests.[test total is zero] AS BEGIN EXEC tSQLt.AssertEquals 0, 0; END;\nGO\nCREATE PROCEDURE OrderTests.helper_build_order AS BEGIN SELECT 1; END;\nGO\nCREATE PROCEDURE dbo.usp_TestConnection AS BEGIN SELECT 1; END;\nGO\n",
    );

    let role = |name: &str| {
        let metadata = symbol(&results, name).metadata.clone().unwrap_or_default();
        (
            metadata
                .get("is_test")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            metadata
                .get("test_role")
                .and_then(|v| v.as_str())
                .map(str::to_string),
        )
    };
    assert_eq!(role("test total is zero"), (true, Some("test_case".into())));
    assert_eq!(role("SetUp").1.as_deref(), Some("fixture_setup"));
    assert_eq!(role("helper_build_order"), (false, None));
    assert_eq!(role("usp_TestConnection"), (false, None));
}

#[test]
fn pgtap_runner_with_name_cast_marks_the_schema_container() {
    let results = extract(
        "CREATE SCHEMA tests;\nCREATE FUNCTION tests.test_sum() RETURNS SETOF TEXT LANGUAGE plpgsql AS $$ BEGIN RETURN NEXT ok(true); END; $$;\nSELECT * FROM runtests('tests'::name);\n",
    );

    let schema = symbol(&results, "tests");
    assert_eq!(
        schema
            .metadata
            .as_ref()
            .and_then(|m| m.get("test_container"))
            .and_then(|v| v.as_bool()),
        Some(true)
    );
}

#[test]
fn sql_literals_survive_classification_and_decode_prefixed_strings() {
    let results = extract(
        "CREATE TABLE dbo.Settings ([Value] nvarchar(400) NOT NULL DEFAULT N'unset');\nINSERT INTO dbo.Settings ([Value]) VALUES (N'it''s');\nIF SCHEMA_ID(N'edr') IS NULL EXEC(N'CREATE SCHEMA [edr];');\nEXEC sp_executesql N'SELECT 1 FROM dbo.T';\n",
    );

    let mut literals = results.literals.clone();
    crate::classify_literals_by_carrier(&mut literals);
    let summary: Vec<_> = literals
        .iter()
        .map(|l| (l.literal_text.as_str(), l.kind.as_str()))
        .collect();
    assert_eq!(
        summary,
        vec![
            ("unset", "other"),
            ("it's", "other"),
            ("edr", "other"),
            ("CREATE SCHEMA [edr];", "sql"),
            ("SELECT 1 FROM dbo.T", "sql"),
        ]
    );
    let string_regions = results
        .source_regions
        .iter()
        .filter(|region| region.kind == SourceRegionKind::StringLiteral)
        .count();
    assert_eq!(string_regions, 5);
}

#[test]
fn parameters_and_variables_are_variable_refs() {
    let results = extract(
        "CREATE PROCEDURE dbo.GetUser (@UserId INT)\nAS\nBEGIN\n    DECLARE @total INT, @label NVARCHAR(50);\n    SELECT Id, Name FROM dbo.Users WHERE Id = @UserId;\n    EXEC dbo.usp_Audit @Entity = N'User', @Id = @UserId;\nEND;\nCREATE FUNCTION archive(p_days INT) RETURNS INT AS $$\nDECLARE\n    v_count INT;\nBEGIN\n    DELETE FROM orders WHERE age > p_days;\n    RETURN v_count;\nEND;\n$$ LANGUAGE plpgsql;\n",
    );

    let kinds = |name: &str| -> Vec<IdentifierKind> {
        identifiers(&results, name)
            .iter()
            .map(|identifier| identifier.kind.clone())
            .collect()
    };
    assert_eq!(
        kinds("@UserId"),
        vec![IdentifierKind::VariableRef, IdentifierKind::VariableRef]
    );
    assert!(kinds("@Entity").is_empty());
    assert_eq!(kinds("p_days"), vec![IdentifierKind::VariableRef]);
    assert_eq!(kinds("v_count"), vec![IdentifierKind::VariableRef]);
    assert_eq!(kinds("Id"), vec![IdentifierKind::MemberAccess; 2]);

    let variables: Vec<_> = results
        .symbols
        .iter()
        .filter(|s| meta_bool(s, "isDeclaredVariable") == Some(true))
        .map(|s| (s.name.as_str(), s.signature.as_deref().unwrap_or("")))
        .collect();
    assert_eq!(
        variables,
        vec![
            ("@total", "DECLARE @total INT"),
            ("@label", "DECLARE @label NVARCHAR(50)"),
            ("v_count", "DECLARE v_count INT"),
        ]
    );
}

#[test]
fn insert_and_merge_target_columns_are_member_accesses() {
    let results = extract(
        "INSERT INTO dbo.AccessLog (UserId, At) VALUES (@UserId, 1);\nMERGE INTO inventory AS t USING incoming AS s ON t.sku = s.sku\nWHEN NOT MATCHED THEN INSERT (sku, qty) VALUES (s.sku, s.qty);\n",
    );

    let receiver = |name: &str| -> Vec<Option<String>> {
        identifiers(&results, name)
            .iter()
            .filter(|i| i.kind == IdentifierKind::MemberAccess)
            .map(|i| i.receiver_type.clone())
            .collect()
    };
    assert_eq!(receiver("UserId"), vec![Some("AccessLog".to_string())]);
    assert_eq!(receiver("At"), vec![Some("AccessLog".to_string())]);
    assert_eq!(
        receiver("qty"),
        vec![Some("inventory".to_string()), Some("incoming".to_string())]
    );
}

#[test]
fn column_references_resolve_table_aliases() {
    let results = extract(
        "SELECT o.id, u.name, total FROM orders o JOIN users AS u ON u.id = o.user_id;\nSELECT balance FROM accounts WHERE accounts.id = 1;\nUPDATE a SET balance = 0 FROM accounts a WHERE a.id = 2;\n",
    );

    let receivers: Vec<_> = results
        .identifiers
        .iter()
        .filter(|i| i.kind == IdentifierKind::MemberAccess)
        .map(|i| (i.name.as_str(), i.receiver_type.as_deref()))
        .collect();
    assert_eq!(
        receivers,
        vec![
            ("id", Some("orders")),
            ("name", Some("users")),
            ("total", None),
            ("id", Some("users")),
            ("user_id", Some("orders")),
            ("balance", Some("accounts")),
            ("id", Some("accounts")),
            ("balance", None),
            ("id", Some("accounts")),
        ]
    );
}

#[test]
fn view_source_tables_hold_only_relations() {
    let results = extract(
        "CREATE VIEW customer_summary AS\nSELECT c.id, customer_total(c.id) AS total, CAST(c.score AS my_score) AS s, COUNT(*) AS n\nFROM customers c JOIN refunds r ON r.cid = c.id\nWHERE c.active;\n",
    );
    let facts = facts_with_pattern(&results, "sql.view_definition.v1");
    assert_eq!(
        fact_strings(facts[0], "source_tables"),
        vec!["customers", "refunds"]
    );
    assert_eq!(fact_u64(facts[0], "source_table_count"), Some(2));
}

#[test]
fn routine_facts_count_declared_parameters() {
    let results = extract(
        "CREATE OR REPLACE FUNCTION transfer(from_id INT, to_id INT, amount NUMERIC) RETURNS BOOLEAN LANGUAGE plpgsql AS $$ BEGIN RETURN TRUE; END; $$;\nCREATE FUNCTION recent_orders(since DATE) RETURNS SETOF orders LANGUAGE sql AS $$ SELECT 1 $$;\n",
    );
    let counts: Vec<_> = facts_with_pattern(&results, "sql.function_definition.v1")
        .iter()
        .map(|f| {
            (
                metadata_str(f, "routine_name").unwrap(),
                fact_u64(f, "parameter_count").unwrap(),
            )
        })
        .collect();
    assert_eq!(counts, vec![("transfer", 3), ("recent_orders", 1)]);

    let params: Vec<_> = results
        .complexity_metrics
        .iter()
        .filter(|metric| metric.scope == "symbol")
        .map(|metric| metric.parameter_count)
        .collect();
    assert_eq!(params, vec![Some(3), Some(1)]);
}

#[test]
fn merge_with_a_table_or_query_source_emits_a_merge_fact() {
    let results = extract(
        "MERGE INTO inventory AS t USING incoming AS s ON t.sku = s.sku\nWHEN MATCHED THEN UPDATE SET qty = t.qty + s.qty;\nMERGE INTO inventory AS t USING (SELECT sku FROM staging) AS s ON t.sku = s.sku\nWHEN NOT MATCHED THEN DELETE;\n",
    );
    let facts = facts_with_pattern(&results, "sql.merge_statement.v1");
    let summary: Vec<_> = facts
        .iter()
        .map(|f| {
            (
                metadata_str(f, "source_kind").unwrap(),
                metadata_str(f, "source_table"),
                fact_bool(f, "has_when_matched").unwrap(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        vec![("table", Some("incoming"), true), ("query", None, false)]
    );
}

#[test]
fn trigger_facts_keep_every_event_timing_and_function() {
    let results = extract(
        "CREATE TRIGGER v_ins INSTEAD OF INSERT ON v_users FOR EACH ROW EXECUTE FUNCTION v_users_insert();\nCREATE TRIGGER audit AFTER INSERT OR UPDATE OR DELETE ON users FOR EACH ROW EXECUTE FUNCTION audit_row();\n",
    );
    let facts = facts_with_pattern(&results, "sql.trigger_definition.v1");
    let summary: Vec<_> = facts
        .iter()
        .map(|f| {
            (
                metadata_str(f, "timing").unwrap(),
                fact_strings(f, "events"),
                metadata_str(f, "function_name").unwrap(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        vec![
            ("instead_of", vec!["insert".to_string()], "v_users_insert"),
            (
                "after",
                vec![
                    "insert".to_string(),
                    "update".to_string(),
                    "delete".to_string()
                ],
                "audit_row"
            ),
        ]
    );
}

#[test]
fn unnamed_constraints_get_no_symbol() {
    let results = extract(
        "CREATE TABLE team_members (\n    team_id INTEGER NOT NULL,\n    user_id INTEGER NOT NULL,\n    PRIMARY KEY (team_id, user_id),\n    CONSTRAINT fk_team FOREIGN KEY (team_id) REFERENCES teams (id),\n    FOREIGN KEY (user_id) REFERENCES users (id)\n);\n",
    );
    let names: Vec<_> = results
        .symbols
        .iter()
        .filter(|s| s.parent_id.is_some())
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(names, vec!["team_id", "user_id", "fk_team"]);
    assert_eq!(facts_with_pattern(&results, "sql.foreign_key.v1").len(), 2);
}

#[test]
fn complexity_counts_control_flow_case_branches_and_boolean_operators() {
    let results = extract(
        "CREATE VIEW tiers AS SELECT id, CASE WHEN total > 1000 THEN 'gold' WHEN total > 100 THEN 'silver' ELSE 'bronze' END AS tier FROM customers WHERE active AND (region = 'EU' OR region = 'US');\nCREATE PROCEDURE dbo.usp_Drain (@batch INT)\nAS\nBEGIN\n    WHILE EXISTS (SELECT 1 FROM dbo.Queue)\n    BEGIN\n        DELETE FROM dbo.Queue WHERE Id < 10\n    END\nEND;\nGO\nIF OBJECT_ID(N'dbo.Queue', N'U') IS NULL\nBEGIN\n    SELECT 1\nEND\nGO\n",
    );
    let metric = |name: Option<&str>| {
        let id = name.map(|name| symbol(&results, name).id.clone());
        results
            .complexity_metrics
            .iter()
            .find(|m| m.symbol_id == id)
            .unwrap_or_else(|| panic!("metric for {name:?}"))
    };
    let tiers = metric(Some("tiers"));
    assert_eq!(tiers.decision_count, 5);
    let drain = metric(Some("usp_Drain"));
    assert_eq!((drain.decision_count, drain.loop_count), (1, 1));
    assert_eq!(drain.parameter_count, Some(1));
    assert_eq!(metric(None).decision_count, 7);
}

#[test]
fn top_level_select_aliases_are_not_symbols() {
    let results = extract(
        "SELECT 'Identities' AS [Will delete], count(*) AS total_users FROM dbo.Identities;\nWITH big AS (SELECT id, total * 2 AS doubled FROM orders) SELECT doubled AS d FROM big;\n",
    );
    let fields: Vec<_> = results
        .symbols
        .iter()
        .filter(|s| meta_bool(s, "isSelectAlias") == Some(true))
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(fields, vec!["doubled"]);
}

#[test]
fn indexes_link_to_their_table_with_grammar_signatures() {
    let results = extract(
        "create table users (id int primary key, email text not null);\ncreate unique index ux_users_email on users (email);\nCREATE INDEX ix_orders_customer ON sales.orders (customer_id) WHERE customer_id > 0;\n",
    );

    let unique = symbol(&results, "ux_users_email");
    assert_eq!(meta_bool(unique, "isUnique"), Some(true));
    assert_eq!(
        unique.signature.as_deref(),
        Some("CREATE UNIQUE INDEX ux_users_email ON users (email)")
    );
    let users = symbol(&results, "users");
    assert!(results.relationships.iter().any(|r| {
        r.from_symbol_id == unique.id
            && r.to_symbol_id == users.id
            && r.metadata.as_ref().unwrap()["relationshipType"] == "index_table"
    }));

    let customer = symbol(&results, "ix_orders_customer");
    assert_eq!(
        customer.signature.as_deref(),
        Some("CREATE INDEX ix_orders_customer ON sales.orders (customer_id) WHERE customer_id > 0")
    );
    assert!(
        results
            .structured_pending_relationships
            .iter()
            .any(|pending| {
                pending.pending.from_symbol_id == customer.id
                    && pending.target.display_name == "sales.orders"
            })
    );
}

#[test]
fn recovered_tsql_trigger_is_named_and_targets_its_table() {
    let results = extract(
        "CREATE TABLE dbo.Orders (Id INT NOT NULL);\nGO\nCREATE TRIGGER dbo.trg_Orders_Audit ON dbo.Orders\nAFTER INSERT, UPDATE\nAS\nBEGIN\n    EXEC dbo.usp_WriteAudit @Entity = N'Order';\nEND;\nGO\n",
    );

    let trigger = symbol(&results, "trg_Orders_Audit");
    assert!(!results.symbols.iter().any(|s| s.name == "dbo"));
    let orders = symbol(&results, "Orders");
    assert!(results.relationships.iter().any(|r| {
        r.from_symbol_id == trigger.id
            && r.to_symbol_id == orders.id
            && r.metadata.as_ref().unwrap()["relationshipType"] == "trigger_target"
    }));
    let fact = facts_with_pattern(&results, "sql.trigger_definition.v1");
    assert_eq!(fact_strings(fact[0], "events"), vec!["insert", "update"]);
    assert_eq!(metadata_str(fact[0], "target_table"), Some("Orders"));
}
