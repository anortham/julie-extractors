use super::{SymbolKind, extract_symbols};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_stored_procedures_functions_and_triggers() {
        let sql_code = r#"
-- Stored procedure with parameters
DELIMITER $$
CREATE PROCEDURE GetUserAnalytics(
    IN p_user_id BIGINT,
    IN p_start_date DATE,
    IN p_end_date DATE,
    OUT p_total_events INT,
    OUT p_unique_sessions INT
)
BEGIN
    DECLARE EXIT HANDLER FOR SQLEXCEPTION
    BEGIN
        ROLLBACK;
        RESIGNAL;
    END;

    START TRANSACTION;

    SELECT
        COUNT(*) INTO p_total_events,
        COUNT(DISTINCT session_id) INTO p_unique_sessions
    FROM analytics_events
    WHERE user_id = p_user_id
      AND DATE(occurred_at) BETWEEN p_start_date AND p_end_date;

    COMMIT;
END$$
DELIMITER ;

-- User-defined function
CREATE FUNCTION CalculateUserScore(p_user_id BIGINT)
RETURNS DECIMAL(10,2)
READS SQL DATA
DETERMINISTIC
BEGIN
    DECLARE v_score DECIMAL(10,2) DEFAULT 0.0;
    DECLARE v_event_count INT;
    DECLARE v_account_age_days INT;

    SELECT COUNT(*), DATEDIFF(NOW(), created_at)
    INTO v_event_count, v_account_age_days
    FROM analytics_events ae
    JOIN users u ON ae.user_id = u.id
    WHERE ae.user_id = p_user_id;

    SET v_score = (v_event_count * 0.1) + (v_account_age_days * 0.01);

    RETURN COALESCE(v_score, 0.0);
END;

-- PostgreSQL function with JSON processing
CREATE OR REPLACE FUNCTION update_user_preferences(
    p_user_id BIGINT,
    p_preferences JSONB
) RETURNS BOOLEAN
LANGUAGE plpgsql
AS $$
DECLARE
    v_current_prefs JSONB;
    v_merged_prefs JSONB;
BEGIN
    -- Get current preferences
    SELECT preferences INTO v_current_prefs
    FROM user_profiles
    WHERE user_id = p_user_id;

    -- Merge with new preferences
    v_merged_prefs := COALESCE(v_current_prefs, '{}'::jsonb) || p_preferences;

    -- Update the user profile
    UPDATE user_profiles
    SET preferences = v_merged_prefs,
        updated_at = NOW()
    WHERE user_id = p_user_id;

    RETURN FOUND;
EXCEPTION
    WHEN OTHERS THEN
        RAISE LOG 'Error updating preferences for user %: %', p_user_id, SQLERRM;
        RETURN FALSE;
END;
$$;

-- Trigger for audit logging
CREATE TRIGGER audit_user_changes
    AFTER UPDATE ON users
    FOR EACH ROW
    WHEN (OLD.* IS DISTINCT FROM NEW.*)
BEGIN
    INSERT INTO audit_log (
        table_name,
        record_id,
        action,
        new_values,
        new_values,
        changed_by,
        changed_at
    ) VALUES (
        'users',
        NEW.id,
        'UPDATE',
        json_object(OLD.*),
        json_object(NEW.*),
        USER(),
        NOW()
    );
END;

-- View with complex aggregations
CREATE VIEW user_engagement_summary AS
SELECT
    u.id,
    u.username,
    u.email,
    COUNT(DISTINCT ae.session_id) as total_sessions,
    COUNT(ae.id) as total_events,
    MIN(ae.occurred_at) as first_event,
    MAX(ae.occurred_at) as last_event,
    AVG(EXTRACT(EPOCH FROM (ae.occurred_at - LAG(ae.occurred_at) OVER (PARTITION BY u.id ORDER BY ae.occurred_at)))) as avg_time_between_events,
    EXTRACT(DAYS FROM (MAX(ae.occurred_at) - MIN(ae.occurred_at))) as engagement_span_days,
    CalculateUserScore(u.id) as user_score
FROM users u
LEFT JOIN analytics_events ae ON u.id = ae.user_id
WHERE u.is_active = TRUE
GROUP BY u.id, u.username, u.email
HAVING COUNT(ae.id) > 0;
"#;

        let symbols = extract_symbols(sql_code);

        let get_user_analytics = symbols
            .iter()
            .find(|s| s.name == "GetUserAnalytics" && s.kind == SymbolKind::Function);
        assert!(get_user_analytics.is_some());
        assert!(
            get_user_analytics
                .unwrap()
                .signature
                .as_ref()
                .unwrap()
                .contains("CREATE PROCEDURE")
        );

        let user_id_param = symbols.iter().find(|s| s.name == "p_user_id");
        assert!(user_id_param.is_some());
        assert!(
            user_id_param
                .unwrap()
                .signature
                .as_ref()
                .unwrap()
                .contains("IN p_user_id BIGINT")
        );

        let total_events_param = symbols.iter().find(|s| s.name == "p_total_events");
        assert!(total_events_param.is_some());
        assert!(
            total_events_param
                .unwrap()
                .signature
                .as_ref()
                .unwrap()
                .contains("OUT")
        );

        let calculate_user_score = symbols.iter().find(|s| s.name == "CalculateUserScore");
        assert!(calculate_user_score.is_some());
        assert!(
            calculate_user_score
                .unwrap()
                .signature
                .as_ref()
                .unwrap()
                .contains("RETURNS DECIMAL(10,2)")
        );

        let update_user_prefs = symbols.iter().find(|s| s.name == "update_user_preferences");
        assert!(update_user_prefs.is_some());
        assert!(
            update_user_prefs
                .unwrap()
                .signature
                .as_ref()
                .unwrap()
                .contains("RETURNS BOOLEAN")
        );
        assert!(
            update_user_prefs
                .unwrap()
                .signature
                .as_ref()
                .unwrap()
                .contains("LANGUAGE plpgsql")
        );

        let score_var = symbols.iter().find(|s| s.name == "v_score");
        assert!(score_var.is_some());
        assert!(
            score_var
                .unwrap()
                .signature
                .as_ref()
                .unwrap()
                .contains("DECLARE v_score DECIMAL(10,2)")
        );

        let current_prefs_var = symbols.iter().find(|s| s.name == "v_current_prefs");
        assert!(current_prefs_var.is_some());
        assert!(
            current_prefs_var
                .unwrap()
                .signature
                .as_ref()
                .unwrap()
                .contains("JSONB")
        );

        let audit_trigger = symbols.iter().find(|s| s.name == "audit_user_changes");
        assert!(audit_trigger.is_some());
        assert!(
            audit_trigger
                .unwrap()
                .signature
                .as_ref()
                .unwrap()
                .contains("CREATE TRIGGER")
        );
        assert!(
            audit_trigger
                .unwrap()
                .signature
                .as_ref()
                .unwrap()
                .contains("AFTER UPDATE ON users")
        );

        let engagement_view = symbols.iter().find(|s| s.name == "user_engagement_summary");
        assert!(engagement_view.is_some());
        assert!(
            engagement_view
                .unwrap()
                .signature
                .as_ref()
                .unwrap()
                .contains("CREATE VIEW")
        );

        let total_sessions = symbols.iter().find(|s| s.name == "total_sessions");
        assert!(total_sessions.is_some());

        let user_score = symbols.iter().find(|s| s.name == "user_score");
        assert!(user_score.is_some());
    }

    #[test]
    fn clean_routine_signature_has_one_create_prefix() {
        let symbols = extract_symbols(
            "CREATE FUNCTION score(value BIGINT) RETURNS DECIMAL(10,2) RETURN value;",
        );
        let signature = symbols
            .iter()
            .find(|symbol| symbol.name == "score")
            .and_then(|symbol| symbol.signature.as_deref())
            .expect("score signature");

        assert_eq!(
            signature,
            "CREATE FUNCTION score(value BIGINT) RETURNS DECIMAL(10,2)"
        );
    }

    #[test]
    fn clean_routine_signature_without_argument_list_preserves_legacy_shape() {
        let symbols =
            extract_symbols("CREATE PROCEDURE refresh_cache AS BEGIN SELECT COALESCE($1, 0); END;");
        let signature = symbols
            .iter()
            .find(|symbol| symbol.name == "refresh_cache")
            .and_then(|symbol| symbol.signature.as_deref())
            .expect("refresh_cache signature");

        assert_eq!(signature, "CREATE PROCEDURE refresh_cache()");
    }

    #[test]
    fn clean_routine_parameters_exclude_nested_call_arguments_and_duplicates() {
        let symbols = extract_symbols(
            r#"CREATE PROCEDURE process_user(IN p_user_id BIGINT)
BEGIN
    SELECT COALESCE(p_user_id, 0);
END;"#,
        );
        let parameters = symbols
            .iter()
            .filter(|symbol| symbol.kind == SymbolKind::Variable)
            .collect::<Vec<_>>();

        assert_eq!(
            parameters
                .iter()
                .filter(|symbol| symbol.name == "p_user_id")
                .count(),
            1
        );
        assert!(parameters.iter().all(|symbol| symbol.name != "COALESCE"));
    }

    #[test]
    fn tsql_execute_as_option_stays_outside_bare_parameter_declaration() {
        let source = include_str!(
            "../../../../../fixtures/extraction/sql/tsql_coverage_regressions/source.sql"
        );
        let symbols = extract_symbols(source);
        let procedure = symbols
            .iter()
            .find(|symbol| symbol.name == "p" && symbol.kind == SymbolKind::Function)
            .expect("procedure p");
        let parameter = symbols
            .iter()
            .find(|symbol| {
                symbol.name == "@Id"
                    && symbol.kind == SymbolKind::Variable
                    && symbol.parent_id.as_deref() == Some(procedure.id.as_str())
            })
            .expect("procedure parameter");

        assert_eq!(parameter.signature.as_deref(), Some("@Id int"));
        assert!(
            procedure
                .signature
                .as_deref()
                .is_some_and(|signature| signature.contains("WITH EXECUTE AS OWNER"))
        );
        let body_span = procedure.body_span.expect("procedure body span");
        assert_eq!(
            &source[body_span.start_byte as usize..body_span.end_byte as usize],
            "AS BEGIN SELECT @Id; END"
        );
        assert!(
            procedure
                .body_hash
                .as_deref()
                .is_some_and(|hash| !hash.is_empty())
        );
    }

    #[test]
    fn tsql_native_compilation_option_stays_outside_parameter_declarations() {
        let source = "CREATE PROCEDURE dbo.p @Batch AS dbo.MeterRows READONLY, @BatchSize int WITH NATIVE_COMPILATION, SCHEMABINDING AS BEGIN SELECT @BatchSize; END;";
        let results =
            crate::pipeline::extract_canonical("source.sql", source, std::path::Path::new("/repo"))
                .expect("canonical SQL extraction");
        assert!(
            results.parse_diagnostics.is_empty(),
            "{:?}",
            results.parse_diagnostics
        );
        let procedure = results
            .symbols
            .iter()
            .find(|symbol| symbol.name == "p" && symbol.kind == SymbolKind::Function)
            .expect("procedure p");
        let batch_parameter = results
            .symbols
            .iter()
            .find(|symbol| {
                symbol.name == "@Batch"
                    && symbol.kind == SymbolKind::Variable
                    && symbol.parent_id.as_deref() == Some(procedure.id.as_str())
            })
            .expect("batch parameter");

        assert_eq!(
            batch_parameter.signature.as_deref(),
            Some("@Batch AS dbo.MeterRows READONLY")
        );
        assert!(
            procedure.signature.as_deref().is_some_and(
                |signature| signature.contains("WITH NATIVE_COMPILATION, SCHEMABINDING")
            )
        );
    }

    #[test]
    fn tsql_parameter_declarations_are_not_variable_references() {
        let source = "CREATE PROCEDURE dbo.p @Id int AS BEGIN SELECT @Id; END;";
        let results =
            crate::pipeline::extract_canonical("source.sql", source, std::path::Path::new("/repo"))
                .expect("canonical SQL extraction");
        let declaration_start = source.find("@Id").expect("parameter declaration") as u32;
        let use_start = source.rfind("@Id").expect("parameter use") as u32;

        assert!(
            !results
                .identifiers
                .iter()
                .any(|identifier| identifier.start_byte == declaration_start)
        );
        let parameter_use = results
            .identifiers
            .iter()
            .find(|identifier| identifier.start_byte == use_start)
            .expect("parameter use identifier");
        assert_eq!(parameter_use.kind, crate::base::IdentifierKind::VariableRef);
        assert_eq!(
            parameter_use
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("access"))
                .and_then(serde_json::Value::as_str),
            Some("read")
        );
    }

    #[test]
    fn tsql_try_catch_procedure_keeps_parameters_and_complete_body() {
        let source = r#"CREATE PROCEDURE Website.InsertCustomerOrders
@Orders Website.OrderList READONLY,
@OrderLines Website.OrderLineList READONLY,
@OrdersCreatedByPersonID int,
@SalespersonPersonID int
WITH EXECUTE AS OWNER
AS
BEGIN
    DECLARE @OrdersToGenerate AS TABLE (OrderReference int PRIMARY KEY, OrderID int);
    BEGIN TRY
        BEGIN TRAN;
        SELECT @SalespersonPersonID;
        COMMIT;
    END TRY
    BEGIN CATCH
        IF XACT_STATE() <> 0 ROLLBACK;
        PRINT N'Unable to create the customer orders.';
        THROW;
        RETURN -1;
    END CATCH;
    RETURN 0;
END;"#;
        let symbols = extract_symbols(source);
        let procedure = symbols
            .iter()
            .find(|symbol| {
                symbol.name == "InsertCustomerOrders" && symbol.kind == SymbolKind::Function
            })
            .expect("InsertCustomerOrders procedure");
        assert!(
            procedure
                .signature
                .as_deref()
                .is_some_and(|signature| signature.contains("WITH EXECUTE AS OWNER"))
        );

        for (name, declared_type) in [
            ("@Orders", "Website.OrderList READONLY"),
            ("@OrderLines", "Website.OrderLineList READONLY"),
            ("@OrdersCreatedByPersonID", "int"),
            ("@SalespersonPersonID", "int"),
        ] {
            let parameter = symbols
                .iter()
                .find(|symbol| {
                    symbol.name == name
                        && symbol.kind == SymbolKind::Variable
                        && symbol.parent_id.as_deref() == Some(procedure.id.as_str())
                })
                .unwrap_or_else(|| panic!("{name} procedure parameter"));
            assert_eq!(
                parameter.signature.as_deref(),
                Some(format!("{name} {declared_type}").as_str())
            );
        }

        let body_span = procedure.body_span.expect("complete procedure body span");
        let body = &source[body_span.start_byte as usize..body_span.end_byte as usize];
        assert!(body.contains("BEGIN TRY"));
        assert!(body.contains("END CATCH"));
        assert!(body.contains("RETURN 0"));
        assert!(
            procedure
                .body_hash
                .as_deref()
                .is_some_and(|hash| !hash.is_empty())
        );
    }

    #[test]
    fn tsql_declare_as_keeps_the_declared_type() {
        let source = include_str!(
            "../../../../../fixtures/extraction/sql/tsql_coverage_regressions/source.sql"
        );
        let symbols = extract_symbols(source);
        let area_code = symbols
            .iter()
            .find(|symbol| symbol.name == "@AreaCode" && symbol.kind == SymbolKind::Variable)
            .expect("declared @AreaCode local");

        assert_eq!(
            area_code.signature.as_deref(),
            Some("DECLARE @AreaCode NVARCHAR(4)")
        );
    }

    #[test]
    fn tsql_inline_tvfs_keep_declared_names_returns_params_and_bodies() {
        let source = include_str!(
            "../../../../../fixtures/extraction/sql/tsql_coverage_regressions/source.sql"
        );
        let symbols = extract_symbols(source);

        for (name, parameter_name, parameter_type) in [
            ("GenerateDateDimensionColumns", "@Year", "int"),
            ("DetermineCustomerAccess", "@CityID", "int"),
        ] {
            let function = symbols
                .iter()
                .find(|symbol| symbol.name == name && symbol.kind == SymbolKind::Function)
                .unwrap_or_else(|| panic!("{name} function symbol"));
            assert!(
                function
                    .signature
                    .as_deref()
                    .is_some_and(|signature| signature.contains("RETURNS TABLE"))
            );
            let parameter = symbols
                .iter()
                .find(|symbol| {
                    symbol.name == parameter_name
                        && symbol.kind == SymbolKind::Variable
                        && symbol.parent_id.as_deref() == Some(function.id.as_str())
                })
                .unwrap_or_else(|| panic!("{name} declared parameter"));
            assert_eq!(
                parameter.signature.as_deref(),
                Some(format!("{parameter_name} {parameter_type}").as_str())
            );
            let body_span = function.body_span.expect("inline TVF body span");
            let body = &source[body_span.start_byte as usize..body_span.end_byte as usize];
            assert!(body.contains("RETURN"));
            assert!(
                function
                    .body_hash
                    .as_deref()
                    .is_some_and(|hash| !hash.is_empty())
            );
        }
    }
}
