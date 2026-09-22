use crate::ExtractionResults;
use crate::base::{IdentifierKind, RelationshipKind, Symbol, SymbolKind};
use crate::factory::extract_symbols_and_relationships;
use std::path::PathBuf;

fn extract(code: &str) -> ExtractionResults {
    let mut parser = super::init_test_parser();
    let tree = parser.parse(code, None).expect("parse");
    extract_symbols_and_relationships(
        &tree,
        "Gaps.vb",
        code,
        "vbnet",
        &PathBuf::from("/test/workspace"),
    )
    .expect("extract")
}

fn symbol<'a>(results: &'a ExtractionResults, name: &str, kind: SymbolKind) -> &'a Symbol {
    results
        .symbols
        .iter()
        .find(|s| s.name == name && s.kind == kind)
        .unwrap_or_else(|| panic!("missing {kind:?} {name}: {:?}", results.symbols))
}

fn name_of(results: &ExtractionResults, id: &str) -> String {
    results
        .symbols
        .iter()
        .find(|s| s.id == id)
        .map(|s| s.name.clone())
        .unwrap_or_default()
}

fn body_text<'a>(code: &'a str, symbol: &Symbol) -> Option<&'a str> {
    symbol
        .body_span
        .map(|span| &code[span.start_byte as usize..span.end_byte as usize])
}

/// `(caller, terminal, receiver)` for every resolved or pending call.
fn calls(results: &ExtractionResults) -> Vec<(String, String, Option<String>)> {
    let mut rows: Vec<_> = results
        .relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::Calls)
        .map(|r| {
            (
                name_of(results, &r.from_symbol_id),
                name_of(results, &r.to_symbol_id),
                None,
            )
        })
        .collect();
    rows.extend(
        results
            .structured_pending_relationships
            .iter()
            .filter(|p| p.pending.kind == RelationshipKind::Calls)
            .map(|p| {
                (
                    name_of(results, &p.pending.from_symbol_id),
                    p.target.terminal_name.clone(),
                    p.target.receiver.clone(),
                )
            }),
    );
    rows
}

#[test]
fn member_bodies_span_from_the_header_to_the_end_keyword() {
    let code = "Public Class Calc\n    Inherits Base\n    Public Function Evaluate(count As Integer) As Integer\n        Dim total As Integer = 0\n        Return total\n    End Function\n    Public MustOverride Sub Abstract()\n    Public Property Auto As Integer\nEnd Class\n";
    let results = extract(code);
    assert_eq!(
        body_text(code, symbol(&results, "Evaluate", SymbolKind::Method)),
        Some("        Dim total As Integer = 0\n        Return total\n    ")
    );
    let class = symbol(&results, "Calc", SymbolKind::Class);
    let class_body = body_text(code, class).expect("class body");
    assert!(
        class_body.starts_with("    Public Function Evaluate"),
        "{class_body:?}"
    );
    assert!(!class_body.contains("Inherits"));
    assert_ne!(
        class.body_hash,
        symbol(&results, "Evaluate", SymbolKind::Method).body_hash
    );
    assert_eq!(
        body_text(code, symbol(&results, "Auto", SymbolKind::Property)),
        None
    );

    let edited = code.replace("Return total", "Return total + 1");
    let edited_results = extract(&edited);
    assert_ne!(
        symbol(&results, "Evaluate", SymbolKind::Method).body_hash,
        symbol(&edited_results, "Evaluate", SymbolKind::Method).body_hash
    );
}

#[test]
fn calls_in_accessors_operators_and_initializers_have_callers() {
    let code = r#"Public Class Settings
    Private _cache As String = LoadValue()
    Public ReadOnly Property Value As String
        Get
            _cache = LoadValue()
            Logger.Info("loaded")
            Dim w = New Widget()
            Return _cache
        End Get
    End Property
    Public Shared Operator =(a As Settings, b As Settings) As Boolean
        Return Compare(a, b)
    End Operator
    Private Function LoadValue() As String
        Return ""
    End Function
    Private Shared Function Compare(a As Settings, b As Settings) As Boolean
        Return True
    End Function
End Class
"#;
    let results = extract(code);
    let rows = calls(&results);
    assert!(
        rows.contains(&("Value".into(), "LoadValue".into(), None)),
        "{rows:?}"
    );
    assert!(
        rows.contains(&("Settings".into(), "LoadValue".into(), None)),
        "{rows:?}"
    );
    assert!(
        rows.contains(&("Value".into(), "Info".into(), Some("Logger".into()))),
        "{rows:?}"
    );
    assert!(
        rows.contains(&("operator =".into(), "Compare".into(), None)),
        "{rows:?}"
    );
    assert!(
        results
            .structured_pending_relationships
            .iter()
            .any(|p| p.pending.kind == RelationshipKind::Instantiates
                && p.target.terminal_name == "Widget"
                && name_of(&results, &p.pending.from_symbol_id) == "Value")
    );
    let compare_ident = results
        .identifiers
        .iter()
        .find(|i| i.kind == IdentifierKind::Call && i.name == "Compare")
        .unwrap();
    assert_eq!(
        compare_ident.containing_symbol_id.as_deref(),
        Some(
            symbol(&results, "operator =", SymbolKind::Operator)
                .id
                .as_str()
        )
    );
}

#[test]
fn type_positions_emit_type_usage_identifiers() {
    let code = r#"Public Class Checkout
    Inherits ServiceBase
    Implements IService
    Private _repo As IOrderRepository
    Public Property Current As Customer
    Public Sub Run(c As Customer)
        Dim o As Order = Nothing
        Dim p As New Payment()
        Dim x = CType(o, SpecialOrder)
        If TypeOf o Is SpecialOrder Then
        End If
        Dim t = GetType(Invoice)
        For Each line As OrderLine In o.Lines
        Next
        Try
        Catch ex As PaymentException
        End Try
        Dim s As System.Text.StringBuilder = Nothing
    End Sub
    Public Sub Handle() Implements IService.Handle
    End Sub
End Class
"#;
    let results = extract(code);
    let type_usages: Vec<&str> = results
        .identifiers
        .iter()
        .filter(|i| i.kind == IdentifierKind::TypeUsage)
        .map(|i| i.name.as_str())
        .collect();
    for name in [
        "ServiceBase",
        "IService",
        "IOrderRepository",
        "Customer",
        "Order",
        "Payment",
        "SpecialOrder",
        "Invoice",
        "OrderLine",
        "PaymentException",
        "StringBuilder",
    ] {
        assert!(type_usages.contains(&name), "{name} in {type_usages:?}");
    }
    assert!(!type_usages.contains(&"Handle"), "{type_usages:?}");
    let run = symbol(&results, "Run", SymbolKind::Method);
    let order = results
        .identifiers
        .iter()
        .find(|i| i.kind == IdentifierKind::TypeUsage && i.name == "Order")
        .unwrap();
    assert_eq!(order.containing_symbol_id.as_deref(), Some(run.id.as_str()));
}

#[test]
fn doc_comment_blocks_are_whole_and_types_have_docs() {
    let code = r#"''' <summary>Top-level class doc.</summary>
Public Class TopDoc
    ''' <summary>
    ''' Adds two numbers.
    ''' </summary>
    ''' <returns>The sum.</returns>
    Public Function Add(a As Integer, b As Integer) As Integer
        Return a + b
    End Function
End Class
Namespace N
    ''' <summary>Enum doc.</summary>
    Public Enum Color
        Red
    End Enum
End Namespace
"#;
    let results = extract(code);
    assert_eq!(
        symbol(&results, "Add", SymbolKind::Method)
            .doc_comment
            .as_deref(),
        Some(
            "''' <summary>\n''' Adds two numbers.\n''' </summary>\n''' <returns>The sum.</returns>"
        )
    );
    assert_eq!(
        symbol(&results, "TopDoc", SymbolKind::Class)
            .doc_comment
            .as_deref(),
        Some("''' <summary>Top-level class doc.</summary>")
    );
    assert_eq!(
        symbol(&results, "Color", SymbolKind::Enum)
            .doc_comment
            .as_deref(),
        Some("''' <summary>Enum doc.</summary>")
    );
}

#[test]
fn qualified_new_instantiates_the_full_type_once() {
    let code = r#"Public Class Maker
    Private _b As Object
    Public Sub Make()
        Dim a = New System.Text.StringBuilder()
        _b = New Acme.Models.Order()
        Dim c = New Global.Acme.Models.Order()
        Dim d = New Widget()
    End Sub
End Class
"#;
    let results = extract(code);
    let instantiates: Vec<(String, Vec<String>)> = results
        .structured_pending_relationships
        .iter()
        .filter(|p| p.pending.kind == RelationshipKind::Instantiates)
        .map(|p| {
            (
                p.target.terminal_name.clone(),
                p.target.namespace_path.clone(),
            )
        })
        .collect();
    assert_eq!(
        instantiates,
        vec![
            (
                "StringBuilder".to_string(),
                vec!["System".to_string(), "Text".to_string()]
            ),
            (
                "Order".to_string(),
                vec!["Acme".to_string(), "Models".to_string()]
            ),
            (
                "Order".to_string(),
                vec!["Acme".to_string(), "Models".to_string()]
            ),
            ("Widget".to_string(), vec![]),
        ]
    );
    assert!(calls(&results).is_empty(), "{:?}", calls(&results));
    let type_usages: Vec<&str> = results
        .identifiers
        .iter()
        .filter(|i| i.kind == IdentifierKind::TypeUsage)
        .map(|i| i.name.as_str())
        .collect();
    assert_eq!(
        type_usages,
        vec!["StringBuilder", "Order", "Order", "Widget"]
    );
    assert!(
        !results.identifiers.iter().any(|i| matches!(
            i.kind,
            IdentifierKind::Call | IdentifierKind::MemberAccess
        ) && matches!(
            i.name.as_str(),
            "Text" | "StringBuilder" | "Models" | "Acme" | "Order"
        )),
        "{:?}",
        results.identifiers
    );
}

#[test]
fn null_conditional_with_block_and_parenless_calls_are_calls() {
    let code = r#"Public Class Runner
    Private _svc As Service
    Public Sub Go(o As Order)
        Dim m = o?.GetTotal()
        _svc?.Notify(o)
        With _svc
            .Start()
            .Configure(o)
        End With
        _svc.Reset
        Call Helper
        Refresh
    End Sub
    Private Sub Helper()
    End Sub
    Private Sub Refresh()
    End Sub
End Class
"#;
    let results = extract(code);
    let rows = calls(&results);
    for expected in [
        ("Go", "GetTotal", Some("o")),
        ("Go", "Notify", Some("_svc")),
        ("Go", "Start", Some("_svc")),
        ("Go", "Configure", Some("_svc")),
        ("Go", "Reset", Some("_svc")),
        ("Go", "Helper", None),
        ("Go", "Refresh", None),
    ] {
        let expected = (
            expected.0.to_string(),
            expected.1.to_string(),
            expected.2.map(str::to_string),
        );
        assert!(rows.contains(&expected), "{expected:?} in {rows:?}");
    }
    let call_names: Vec<&str> = results
        .identifiers
        .iter()
        .filter(|i| i.kind == IdentifierKind::Call)
        .map(|i| i.name.as_str())
        .collect();
    for name in [
        "GetTotal",
        "Notify",
        "Start",
        "Configure",
        "Reset",
        "Helper",
        "Refresh",
    ] {
        assert!(call_names.contains(&name), "{name} in {call_names:?}");
    }
    let start = results
        .identifiers
        .iter()
        .find(|i| i.name == "Start")
        .unwrap();
    assert_eq!(
        start.metadata.as_ref().and_then(|m| m.get("receiver")),
        Some(&serde_json::json!("_svc"))
    );
}
