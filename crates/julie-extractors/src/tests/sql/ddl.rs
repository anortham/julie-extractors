use super::{SymbolKind, extract_symbols};

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_extract_tables_columns_and_constraints() {
        let sql_code = r#"
-- User management tables
CREATE TABLE users (
    id BIGINT PRIMARY KEY AUTO_INCREMENT,
    username VARCHAR(50) UNIQUE NOT NULL,
    email VARCHAR(255) UNIQUE NOT NULL,
    password_hash VARCHAR(255) NOT NULL,
    first_name VARCHAR(100),
    last_name VARCHAR(100),
    date_of_birth DATE,
    is_active BOOLEAN DEFAULT TRUE,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,

    CONSTRAINT chk_email_format CHECK (email LIKE '%@%.%'),
    CONSTRAINT chk_age CHECK (date_of_birth < CURDATE()),
    INDEX idx_username (username),
    INDEX idx_email (email),
    INDEX idx_created_at (created_at)
);

CREATE TABLE user_profiles (
    user_id BIGINT,
    bio TEXT,
    avatar_url VARCHAR(500),
    social_links JSON,
    preferences JSON DEFAULT '{}',

    PRIMARY KEY (user_id),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

-- Enum table for user roles
CREATE TABLE user_roles (
    id INT PRIMARY KEY,
    role_name ENUM('admin', 'moderator', 'user', 'guest') NOT NULL,
    permissions JSON
);

-- Complex table with various column types
CREATE TABLE analytics_events (
    id UUID DEFAULT gen_random_uuid() PRIMARY KEY,
    event_type VARCHAR(100) NOT NULL,
    user_id BIGINT,
    session_id VARCHAR(100),
    event_data JSONB,
    ip_address INET,
    user_agent TEXT,
    occurred_at TIMESTAMPTZ DEFAULT NOW(),

    FOREIGN KEY (user_id) REFERENCES users(id),
    PARTITION BY RANGE (occurred_at)
);
"#;

        let symbols = extract_symbols(sql_code);

        let users_table = symbols
            .iter()
            .find(|s| s.name == "users" && s.kind == SymbolKind::Class);
        assert!(users_table.is_some());
        assert!(
            users_table
                .unwrap()
                .signature
                .as_ref()
                .unwrap()
                .contains("CREATE TABLE users")
        );

        let user_profiles_table = symbols.iter().find(|s| s.name == "user_profiles");
        assert!(user_profiles_table.is_some());

        let user_roles_table = symbols.iter().find(|s| s.name == "user_roles");
        assert!(user_roles_table.is_some());

        let analytics_table = symbols.iter().find(|s| s.name == "analytics_events");
        assert!(analytics_table.is_some());

        let id_column = symbols
            .iter()
            .find(|s| s.name == "id" && s.kind == SymbolKind::Field);
        assert!(id_column.is_some());
        assert!(
            id_column
                .unwrap()
                .signature
                .as_ref()
                .unwrap()
                .contains("BIGINT PRIMARY KEY")
        );

        let username_column = symbols.iter().find(|s| s.name == "username");
        assert!(username_column.is_some());
        assert!(
            username_column
                .unwrap()
                .signature
                .as_ref()
                .unwrap()
                .contains("VARCHAR(50) UNIQUE NOT NULL")
        );

        let email_column = symbols.iter().find(|s| s.name == "email");
        assert!(email_column.is_some());

        let is_active_column = symbols.iter().find(|s| s.name == "is_active");
        assert!(is_active_column.is_some());
        assert!(
            is_active_column
                .unwrap()
                .signature
                .as_ref()
                .unwrap()
                .contains("BOOLEAN DEFAULT TRUE")
        );

        let social_links_column = symbols.iter().find(|s| s.name == "social_links");
        assert!(social_links_column.is_some());
        assert!(
            social_links_column
                .unwrap()
                .signature
                .as_ref()
                .unwrap()
                .contains("JSON")
        );

        let event_data_column = symbols.iter().find(|s| s.name == "event_data");
        assert!(event_data_column.is_some());
        assert!(
            event_data_column
                .unwrap()
                .signature
                .as_ref()
                .unwrap()
                .contains("JSONB")
        );

        let constraints = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Interface)
            .collect::<Vec<_>>();
        assert!(constraints.len() >= 2);

        let indexes = symbols
            .iter()
            .filter(|s| {
                s.signature
                    .as_ref()
                    .is_some_and(|sig| sig.contains("INDEX"))
            })
            .collect::<Vec<_>>();
        assert!(indexes.len() >= 3);
    }

    #[test]
    fn parser_backed_quoted_sql_names_are_normalized() {
        let sql_code = r#"
CREATE TABLE [edr].[Items] (
    [Id] INT,
    [Label] NVARCHAR(100) DEFAULT 'new',
    [Payload] VARBINARY(max) NULL,
    [Status] NVARCHAR(100) CHECK ([Status] <> 'blocked'),
    CONSTRAINT [PK_Items] PRIMARY KEY ([Id])
);

CREATE VIEW [edr].[ItemView] AS
SELECT [Id] AS [ItemId] FROM [edr].[Items];

CREATE INDEX [IX_Items_Label] ON [edr].[Items] ([Label]);

CREATE TRIGGER [edr].[TR_Items]
    AFTER INSERT ON [edr].[Items]
    FOR EACH ROW
    EXECUTE FUNCTION [edr].[log_item]();

CREATE PROCEDURE [edr].[RefreshItems] AS BEGIN SELECT 1; END;
"#;

        let results =
            crate::pipeline::extract_canonical("quoted.sql", sql_code, Path::new("/repo"))
                .expect("quoted SQL extraction should succeed");
        assert!(
            results.parse_diagnostics.is_empty(),
            "valid quoted SQL must parse cleanly: {:?}",
            results.parse_diagnostics
        );
        let symbol_names = results
            .symbols
            .iter()
            .map(|symbol| symbol.name.as_str())
            .collect::<Vec<_>>();

        for expected in [
            "Items",
            "Id",
            "Label",
            "Payload",
            "Status",
            "PK_Items",
            "ItemView",
            "ItemId",
            "IX_Items_Label",
            "TR_Items",
            "RefreshItems",
        ] {
            assert!(
                symbol_names.contains(&expected),
                "expected normalized symbol `{expected}`, got {symbol_names:?}"
            );
        }
        assert!(symbol_names.iter().all(|name| !name.contains('[')));

        let id = results
            .symbols
            .iter()
            .find(|symbol| symbol.name == "Id")
            .expect("normalized Id symbol");
        assert!(sql_code[id.start_byte as usize..id.end_byte as usize].contains("[Id]"));

        let table_signature = results
            .symbols
            .iter()
            .find(|symbol| symbol.name == "Items")
            .and_then(|symbol| symbol.signature.as_deref())
            .expect("normalized table signature");
        assert_eq!(table_signature, "CREATE TABLE Items (4 columns)");

        for (name, expected_signature) in [
            ("Label", "NVARCHAR(100) DEFAULT 'new'"),
            ("Payload", "VARBINARY(max)"),
            ("Status", "NVARCHAR(100) CHECK"),
        ] {
            let symbol = results
                .symbols
                .iter()
                .find(|symbol| symbol.name == name)
                .unwrap_or_else(|| panic!("normalized {name} symbol"));
            assert_eq!(symbol.signature.as_deref(), Some(expected_signature));
            assert_eq!(symbol.body_span, None, "type size is not a field body");
        }

        let procedure_signature = results
            .symbols
            .iter()
            .find(|symbol| symbol.name == "RefreshItems")
            .and_then(|symbol| symbol.signature.as_deref())
            .expect("normalized procedure signature");
        assert_eq!(procedure_signature, "CREATE PROCEDURE RefreshItems()");

        let literal = results
            .literals
            .iter()
            .find(|literal| literal.literal_text == "blocked")
            .expect("column constraint literal");
        assert_eq!(literal.carrier.as_deref(), Some("Status"));
    }
}
