use super::{SymbolKind, extract_symbols};

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata_bool(symbol: &crate::base::Symbol, key: &str) -> bool {
        symbol
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get(key))
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
    }

    fn metadata_str<'a>(symbol: &'a crate::base::Symbol, key: &str) -> Option<&'a str> {
        symbol
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get(key))
            .and_then(|value| value.as_str())
    }

    fn body_text<'a>(source: &'a str, symbol: &crate::base::Symbol) -> &'a str {
        let span = symbol.body_span.expect("symbol should expose a body span");
        &source[span.start_byte as usize..span.end_byte as usize]
    }

    #[test]
    fn recovery_view_and_trigger_body_spans_use_statement_text_and_mark_source() {
        let sql = r#"
CREATE VIEW active_workers AS
SELECT id, name
FROM workers
WHERE id > 0;

CREATE TRIGGER refresh_active_workers
AFTER INSERT ON workers
FOR EACH ROW
BEGIN
    INSERT INTO jobs (worker_id)
    SELECT NEW.id
    FROM workers
    WHERE NEW.id > 0;
END;
"#;

        let symbols = extract_symbols(sql);

        let view = symbols
            .iter()
            .find(|symbol| symbol.name == "active_workers")
            .expect("active_workers view should be extracted");
        assert!(
            metadata_bool(view, "extractedFromError"),
            "view should remain tagged as recovery extraction"
        );
        assert_eq!(
            metadata_str(view, "bodySpanSource"),
            Some("recovery_heuristic")
        );
        let view_body = view.body_span.expect("view should have AS body span");
        assert!(
            view_body.end_byte > view_body.start_byte,
            "view body span should cover the SELECT query"
        );
        assert!(
            view_body.start_byte >= view.start_byte,
            "view body span should start within the declaration"
        );
        let view_body_text = body_text(sql, view);
        assert!(
            view_body_text.trim_start().starts_with("SELECT id, name"),
            "view body should start at the SELECT query, got {view_body_text:?}"
        );
        assert!(
            !view_body_text.contains("CREATE VIEW"),
            "view body span must not include the declaration header"
        );

        let trigger = symbols
            .iter()
            .find(|symbol| symbol.name == "refresh_active_workers")
            .expect("refresh_active_workers trigger should be extracted");
        assert!(
            metadata_bool(trigger, "extractedFromError"),
            "trigger should remain tagged as recovery extraction"
        );
        assert_eq!(
            metadata_str(trigger, "bodySpanSource"),
            Some("recovery_heuristic")
        );
        let trigger_body = trigger
            .body_span
            .expect("trigger should have BEGIN..END body span");
        assert!(
            trigger_body.end_byte - trigger_body.start_byte > 20,
            "trigger body span should cover the full action block, not a fragment"
        );
        assert!(
            trigger_body.end_byte >= trigger.end_byte.saturating_sub(8),
            "trigger body span should reach the END keyword region"
        );
        let trigger_body_text = body_text(sql, trigger);
        assert!(
            trigger_body_text.contains("INSERT INTO jobs"),
            "trigger body should include the action block, got {trigger_body_text:?}"
        );
        assert!(
            !trigger_body_text.contains("CREATE TRIGGER"),
            "trigger body span must not include the declaration header"
        );
    }

    #[test]
    fn clean_create_view_emits_body_span_and_marks_source() {
        let sql = "CREATE VIEW clean_view AS SELECT 1 AS one;";
        let symbols = extract_symbols(sql);
        let view = symbols
            .iter()
            .find(|symbol| symbol.name == "clean_view" && symbol.kind == SymbolKind::Interface)
            .expect("clean_view should be extracted");
        let is_recovery = metadata_bool(view, "extractedFromError");
        let body_source = metadata_str(view, "bodySpanSource").expect("bodySpanSource metadata");
        assert!(
            view.body_span.is_some(),
            "view should expose a SELECT body span"
        );
        if is_recovery {
            assert_eq!(body_source, "recovery_heuristic");
        } else {
            assert_eq!(body_source, "statement_text");
        }
    }

    #[test]
    fn view_body_span_accepts_as_on_separate_line() {
        let sql = "CREATE VIEW wrapped_view\nAS\nSELECT 1 AS one;";
        let symbols = extract_symbols(sql);
        let view = symbols
            .iter()
            .find(|symbol| symbol.name == "wrapped_view" && symbol.kind == SymbolKind::Interface)
            .expect("wrapped_view should be extracted");

        let text = body_text(sql, view);
        assert!(
            text.trim_start().starts_with("SELECT 1 AS one"),
            "view body should start after newline-separated AS, got {text:?}"
        );
    }
}
