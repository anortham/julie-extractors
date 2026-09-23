use crate::ExtractionResults;
use crate::base::{IdentifierKind, RelationshipKind, Symbol, SymbolKind, Visibility};
use crate::tests::helpers::metadata_str;
use std::path::Path;

fn extract(code: &str) -> ExtractionResults {
    crate::pipeline::extract_canonical("src/Gaps.vb", code, Path::new("/repo")).expect("extract")
}

fn symbol<'a>(results: &'a ExtractionResults, name: &str, kind: SymbolKind) -> &'a Symbol {
    results
        .symbols
        .iter()
        .find(|s| s.name == name && s.kind == kind)
        .unwrap_or_else(|| panic!("missing {kind:?} {name}: {:#?}", names(results)))
}

fn names(results: &ExtractionResults) -> Vec<String> {
    results
        .symbols
        .iter()
        .map(|s| format!("{}:{:?}", s.name, s.kind))
        .collect()
}

fn label(results: &ExtractionResults, id: &str) -> String {
    results
        .symbols
        .iter()
        .find(|s| s.id == id)
        .map(|s| format!("{}:{:?}", s.name, s.kind))
        .unwrap_or_default()
}

fn edges(results: &ExtractionResults, kind: RelationshipKind) -> Vec<(String, String)> {
    results
        .relationships
        .iter()
        .filter(|r| r.kind == kind)
        .map(|r| {
            (
                label(results, &r.from_symbol_id),
                label(results, &r.to_symbol_id),
            )
        })
        .collect()
}

type PendingRow = (String, String, Option<String>, Vec<String>, Option<String>);

fn pending(results: &ExtractionResults, kind: RelationshipKind) -> Vec<PendingRow> {
    results
        .structured_pending_relationships
        .iter()
        .filter(|p| p.pending.kind == kind)
        .map(|p| {
            (
                label(results, &p.pending.from_symbol_id),
                p.target.terminal_name.clone(),
                p.target.receiver.clone(),
                p.target.namespace_path.clone(),
                p.receiver_type.clone(),
            )
        })
        .collect()
}

fn idents(results: &ExtractionResults, name: &str) -> Vec<(IdentifierKind, u32, String)> {
    results
        .identifiers
        .iter()
        .filter(|i| i.name == name)
        .map(|i| {
            (
                i.kind.clone(),
                i.start_line,
                i.containing_symbol_id
                    .as_deref()
                    .map(|id| label(results, id))
                    .unwrap_or_default(),
            )
        })
        .collect()
}

fn type_of(results: &ExtractionResults, symbol: &Symbol) -> Option<(String, bool)> {
    results
        .types
        .get(&symbol.id)
        .map(|t| (t.resolved_type.clone(), t.is_inferred))
}

fn annotation_raw(symbol: &Symbol) -> Vec<(String, Option<String>)> {
    symbol
        .annotations
        .iter()
        .map(|a| (a.annotation.clone(), a.raw_text.clone()))
        .collect()
}

#[test]
fn mybase_calls_target_the_base_type_and_never_the_caller() {
    let code = "Public Class BaseWidget\n    Public Overridable Sub Render()\n    End Sub\nEnd Class\nPublic Class Widget\n    Inherits BaseWidget\n    Public Overrides Sub Render()\n        MyBase.Render()\n    End Sub\nEnd Class\nPublic Class Derived\n    Inherits BaseThing\n    Public Sub New(name As String)\n        MyBase.New(name)\n    End Sub\n    Public Overrides Sub Reset()\n        MyBase.Reset()\n    End Sub\nEnd Class\n";
    let results = extract(code);
    assert!(
        results
            .relationships
            .iter()
            .all(|r| r.from_symbol_id != r.to_symbol_id),
        "{:?}",
        edges(&results, RelationshipKind::Calls)
    );
    let base_render = results
        .symbols
        .iter()
        .find(|s| {
            s.name == "Render"
                && s.parent_id.as_deref()
                    == Some(
                        symbol(&results, "BaseWidget", SymbolKind::Class)
                            .id
                            .as_str(),
                    )
        })
        .unwrap();
    assert!(
        results
            .relationships
            .iter()
            .any(|r| r.kind == RelationshipKind::Calls && r.to_symbol_id == base_render.id)
    );
    let calls = pending(&results, RelationshipKind::Calls);
    for terminal in ["New", "Reset"] {
        assert!(
            calls
                .iter()
                .any(|(_, t, receiver, _, receiver_type)| t == terminal
                    && receiver.as_deref() == Some("MyBase")
                    && receiver_type.as_deref() == Some("BaseThing")),
            "{calls:?}"
        );
    }
}

#[test]
fn indexing_a_local_parameter_or_field_is_a_read_not_a_call() {
    let code = "Public Class Indexer\n    Private _items() As Integer\n    Public Sub Run(row As DataRow, names As List(Of String))\n        Dim arr() As Integer = {1, 2}\n        Dim v = arr(0)\n        Dim w = row(\"Name\")\n        Dim n = names(1)\n        Dim f = _items(2)\n        arr(1) = 5\n        Helper(1)\n    End Sub\n    Private Function Helper(x As Integer) As Integer\n        Return x\n    End Function\nEnd Class\n";
    let results = extract(code);
    for name in ["arr", "row", "names", "_items"] {
        let rows = idents(&results, name);
        assert!(
            rows.iter()
                .all(|(kind, _, _)| *kind != IdentifierKind::Call),
            "{name}: {rows:?}"
        );
        assert!(
            rows.iter()
                .any(|(kind, line, _)| *kind == IdentifierKind::VariableRef && *line > 4),
            "{name}: {rows:?}"
        );
    }
    let calls = pending(&results, RelationshipKind::Calls);
    assert!(calls.is_empty(), "{calls:?}");
    assert_eq!(
        edges(&results, RelationshipKind::Calls),
        vec![("Run:Method".to_string(), "Helper:Method".to_string())]
    );
}

#[test]
fn type_relationships_start_at_the_type_and_resolve_generic_targets() {
    let code = "Public Class OrderEntity\n    Inherits EntityBase\n    Public Property Customer As Customer\nEnd Class\nPublic Class Customer\n    Inherits EntityBase\n    Implements ICustomer\nEnd Class\nPublic MustInherit Class EntityBase\nEnd Class\nPublic Interface ICustomer\nEnd Interface\nPublic Class Repo(Of T)\n    Implements IRepo(Of T)\nEnd Class\nPublic Class GenericChild\n    Inherits Repo(Of Integer)\nEnd Class\nPublic Interface IRepo(Of T)\nEnd Interface\n";
    let results = extract(code);
    let extends = edges(&results, RelationshipKind::Extends);
    let implements = edges(&results, RelationshipKind::Implements);
    for edge in [
        ("Customer:Class", "EntityBase:Class"),
        ("GenericChild:Class", "Repo:Class"),
    ] {
        assert!(
            extends.contains(&(edge.0.to_string(), edge.1.to_string())),
            "{extends:?}"
        );
    }
    for edge in [
        ("Customer:Class", "ICustomer:Interface"),
        ("Repo:Class", "IRepo:Interface"),
    ] {
        assert!(
            implements.contains(&(edge.0.to_string(), edge.1.to_string())),
            "{implements:?}"
        );
    }
    assert!(
        extends
            .iter()
            .chain(&implements)
            .all(|(from, _)| !from.ends_with(":Property"))
    );
    assert!(
        pending(&results, RelationshipKind::Implements).is_empty()
            && pending(&results, RelationshipKind::Extends).is_empty()
    );
}

#[test]
fn chained_call_targets_follow_only_the_member_spine() {
    let code = "Public Class Chains\n    Public Sub Run(o As Order)\n        Dim b = _svc.Builder().WithName(\"x\").Build()\n        Dim z = _svc.Find(o.Id).Customer.Save(o)\n        Dim q = Api.Client.Fetch()\n    End Sub\nEnd Class\n";
    let results = extract(code);
    let calls = pending(&results, RelationshipKind::Calls);
    let row = |terminal: &str| {
        calls
            .iter()
            .find(|(_, t, _, _, _)| t == terminal)
            .map(|(_, _, receiver, ns, _)| (receiver.clone(), ns.clone()))
            .unwrap_or_else(|| panic!("{terminal}: {calls:?}"))
    };
    assert_eq!(row("Build"), (None, vec![]));
    assert_eq!(row("WithName"), (None, vec![]));
    assert_eq!(row("Builder"), (Some("_svc".to_string()), vec![]));
    assert_eq!(row("Save"), (Some("Customer".to_string()), vec![]));
    assert_eq!(
        row("Fetch"),
        (Some("Client".to_string()), vec!["Api".to_string()])
    );
}

#[test]
fn block_scoped_declarations_are_local_variables_with_types() {
    let code = "Public Class Shipper\n    Public Sub ShipAll(orders As List(Of Order))\n        For Each o As Order In orders\n            o.Ship()\n        Next\n        For i As Integer = 0 To 3\n        Next\n        Using conn As New SqlConnection(\"x\")\n            conn.Open()\n        End Using\n        Try\n        Catch ex As InvalidOperationException\n            ex.GetBaseException()\n        End Try\n    End Sub\nEnd Class\n";
    let results = extract(code);
    let ship_all = symbol(&results, "ShipAll", SymbolKind::Method);
    for (name, type_name) in [
        ("o", "Order"),
        ("i", "Integer"),
        ("conn", "SqlConnection"),
        ("ex", "InvalidOperationException"),
    ] {
        let local = symbol(&results, name, SymbolKind::Variable);
        assert_eq!(local.parent_id.as_deref(), Some(ship_all.id.as_str()));
        assert_eq!(
            type_of(&results, local),
            Some((type_name.to_string(), false)),
            "{name}"
        );
    }
    for (name, line) in [("conn", 8), ("ex", 12), ("o", 3), ("i", 6)] {
        assert!(
            idents(&results, name)
                .iter()
                .all(|(kind, l, _)| !(*kind == IdentifierKind::VariableRef && *l == line)),
            "{name}: {:?}",
            idents(&results, name)
        );
    }
}

#[test]
fn syntax_stated_return_types_are_declared_facts() {
    let code = "Public Class Factory\n    Public Function Make() As Result\n    End Function\n    Public Function MakeMany() As List(Of Result)\n    End Function\n    Public Async Function MakeAsync() As Task(Of Result)\n    End Function\n    Public Function MakeArray() As Result()\n    End Function\n    Public Function Lookup() As Dictionary(Of String, Result)\n    End Function\n    Public Sub Run()\n    End Sub\nEnd Class\n";
    let results = extract(code);
    for (name, type_name) in [
        ("Make", "Result"),
        ("MakeMany", "List"),
        ("MakeAsync", "Task"),
        ("MakeArray", "Result()"),
        ("Lookup", "Dictionary"),
    ] {
        assert_eq!(
            type_of(&results, symbol(&results, name, SymbolKind::Method)),
            Some((type_name.to_string(), false)),
            "{name}"
        );
    }
    assert_eq!(
        type_of(&results, symbol(&results, "Run", SymbolKind::Method)),
        None
    );
}

#[test]
fn every_type_declaration_keeps_its_attributes_with_arguments() {
    let code = "Namespace Contoso.Tests\n    <TestClass>\n    Public Class FirstTests\n    End Class\n    <TestClass>\n    Public Class HooksOnly\n        <AssemblyInitialize>\n        Public Shared Sub Init(ctx As TestContext)\n        End Sub\n    End Class\n    <ApiController>\n    <Route(\"api/x\")>\n    Public Class SecondController\n    End Class\nEnd Namespace\n<Flags>\nPublic Enum Perm\n    A = 1\nEnd Enum\n<Serializable>\nPublic Structure Pt\nEnd Structure\n<HideModuleName>\nModule Helpers\nEnd Module\n<ComVisible(True)>\nPublic Interface IShape\nEnd Interface\n";
    let results = extract(code);
    let first = |name: &str, kind: SymbolKind| annotation_raw(symbol(&results, name, kind));
    assert_eq!(
        first("HooksOnly", SymbolKind::Class),
        vec![("TestClass".to_string(), Some("TestClass".to_string()))]
    );
    assert_eq!(
        symbol(&results, "HooksOnly", SymbolKind::Class)
            .metadata
            .as_ref()
            .and_then(|m| m.get("test_container"))
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        first("SecondController", SymbolKind::Class),
        vec![
            (
                "ApiController".to_string(),
                Some("ApiController".to_string())
            ),
            ("Route".to_string(), Some("Route(\"api/x\")".to_string())),
        ]
    );
    assert_eq!(first("Perm", SymbolKind::Enum)[0].0, "Flags");
    assert_eq!(first("Pt", SymbolKind::Struct)[0].0, "Serializable");
    assert_eq!(first("Helpers", SymbolKind::Class)[0].0, "HideModuleName");
    assert_eq!(
        first("IShape", SymbolKind::Interface),
        vec![(
            "ComVisible".to_string(),
            Some("ComVisible(True)".to_string())
        )]
    );
}

#[test]
fn attribute_facts_name_the_last_segment_and_attach_to_the_owner() {
    let code = "<Assembly: AssemblyVersion(\"1.0.0.0\")>\nNamespace App\n    <Global.Microsoft.VisualBasic.CompilerServices.DesignerGenerated()> _\n    Partial Class Form1\n        <System.Diagnostics.DebuggerStepThrough()> _\n        Private Sub InitializeComponent()\n        End Sub\n    End Class\n    <NUnit.Framework.TestFixture>\n    Public Class Second\n    End Class\nEnd Namespace\n";
    let results = extract(code);
    let facts: Vec<_> = results
        .structural_facts
        .iter()
        .filter(|f| f.pattern_id == "vbnet.attribute.v1")
        .collect();
    let row = |name: &str| {
        facts
            .iter()
            .find(|f| metadata_str(f, "attribute_name") == Some(name))
            .unwrap_or_else(|| panic!("{name}: {facts:#?}"))
    };
    let owner = |name: &str| {
        row(name)
            .containing_symbol_id
            .as_deref()
            .map(|id| label(&results, id))
    };
    assert_eq!(owner("DesignerGenerated"), Some("Form1:Class".to_string()));
    assert_eq!(
        metadata_str(row("DesignerGenerated"), "qualified_name"),
        Some("Global.Microsoft.VisualBasic.CompilerServices.DesignerGenerated")
    );
    assert_eq!(
        owner("DebuggerStepThrough"),
        Some("InitializeComponent:Method".to_string())
    );
    assert_eq!(owner("TestFixture"), Some("Second:Class".to_string()));
    assert_eq!(
        metadata_str(row("AssemblyVersion"), "target"),
        Some("Assembly")
    );
    assert_eq!(owner("AssemblyVersion"), None);
}

#[test]
fn friend_maps_to_internal_and_structure_fields_default_public() {
    let code = "Friend Class A\n    Friend Sub M()\n    End Sub\n    Protected Friend Sub P()\n    End Sub\n    Class Nested\n    End Class\nEnd Class\nClass B\nEnd Class\nNamespace N\n    Class InNs\n    End Class\n    Module M2\n    End Module\nEnd Namespace\nStructure S\n    Dim x As Integer\nEnd Structure\nPublic Class C\n    Dim y As Integer\nEnd Class\n";
    let results = extract(code);
    for (name, kind) in [
        ("A", SymbolKind::Class),
        ("M", SymbolKind::Method),
        ("B", SymbolKind::Class),
        ("InNs", SymbolKind::Class),
        ("M2", SymbolKind::Class),
        ("S", SymbolKind::Struct),
    ] {
        assert_eq!(
            symbol(&results, name, kind).visibility,
            Some(Visibility::Internal),
            "{name}"
        );
    }
    assert_eq!(
        symbol(&results, "P", SymbolKind::Method).visibility,
        Some(Visibility::Protected)
    );
    assert_eq!(
        symbol(&results, "Nested", SymbolKind::Class).visibility,
        Some(Visibility::Public)
    );
    assert_eq!(
        symbol(&results, "x", SymbolKind::Field).visibility,
        Some(Visibility::Public)
    );
    assert_eq!(
        symbol(&results, "y", SymbolKind::Field).visibility,
        Some(Visibility::Private)
    );
}

#[test]
fn keyword_operators_are_symbols_that_own_their_parameters() {
    let code = "Public Structure Money\n    Public Shared Operator +(a As Money, b As Money) As Money\n        Return a\n    End Operator\n    Public Shared Operator Not(a As Money) As Money\n        Return a\n    End Operator\n    Public Shared Narrowing Operator CType(a As Money) As Decimal\n        Return 0\n    End Operator\n    Public Shared Operator Mod(a As Money, b As Money) As Money\n        Return a\n    End Operator\n    Public Shared Operator IsTrue(a As Money) As Boolean\n        Return True\n    End Operator\nEnd Structure\n";
    let results = extract(code);
    let money = symbol(&results, "Money", SymbolKind::Struct);
    for name in ["+", "Not", "CType", "Mod", "IsTrue"] {
        let op = symbol(&results, &format!("operator {name}"), SymbolKind::Operator);
        assert_eq!(op.parent_id.as_deref(), Some(money.id.as_str()));
    }
    let ctype = symbol(&results, "operator CType", SymbolKind::Operator);
    assert!(
        ctype
            .signature
            .as_deref()
            .is_some_and(|s| s.contains("Narrowing Operator CType")),
        "{:?}",
        ctype.signature
    );
    assert!(
        results
            .symbols
            .iter()
            .filter(|s| s.name == "a")
            .all(|s| s.parent_id.as_deref() != Some(money.id.as_str()))
    );
}

#[test]
fn member_implements_and_handles_clauses_link_their_targets() {
    let code = "Public Interface IJob\n    Sub Run()\nEnd Interface\nPublic Class Worker\n    Implements IJob\n    Implements IDisposable\n    Private WithEvents _timer As Timer\n    Public Sub DoWork() Implements IJob.Run\n    End Sub\n    Public Sub Dispose() Implements IDisposable.Dispose\n    End Sub\n    Private Sub OnTick(sender As Object, e As EventArgs) Handles _timer.Tick\n    End Sub\nEnd Class\n";
    let results = extract(code);
    let implements = edges(&results, RelationshipKind::Implements);
    let run = results.symbols.iter().find(|s| s.name == "Run").unwrap();
    assert!(
        results
            .relationships
            .iter()
            .any(|r| r.kind == RelationshipKind::Implements
                && label(&results, &r.from_symbol_id) == "DoWork:Method"
                && r.to_symbol_id == run.id),
        "{implements:?}"
    );
    assert!(pending(&results, RelationshipKind::Implements).iter().any(
        |(from, terminal, receiver, _, _)| from == "Dispose:Method"
            && terminal == "Dispose"
            && receiver.as_deref() == Some("IDisposable")
    ));
    assert!(
        idents(&results, "IJob")
            .iter()
            .any(|(kind, line, owner)| *kind == IdentifierKind::TypeUsage
                && *line == 8
                && owner == "DoWork:Method")
    );
    assert!(
        idents(&results, "Run")
            .iter()
            .any(|(kind, _, owner)| *kind == IdentifierKind::MemberAccess
                && owner == "DoWork:Method")
    );
    assert!(
        idents(&results, "_timer")
            .iter()
            .any(|(kind, line, owner)| *kind == IdentifierKind::VariableRef
                && *line == 12
                && owner == "OnTick:Method")
    );
    assert!(
        idents(&results, "Tick")
            .iter()
            .any(|(kind, _, owner)| *kind == IdentifierKind::MemberAccess
                && owner == "OnTick:Method")
    );
}

#[test]
fn shared_as_clauses_and_later_classes_type_every_declarator() {
    let code = "Public Class Holder\n    Private a, b As Integer\n    Public Sub Work()\n        Dim x, y As Integer\n        Dim late = New After()\n    End Sub\nEnd Class\nPublic Class After\nEnd Class\n";
    let results = extract(code);
    for (name, kind) in [
        ("a", SymbolKind::Field),
        ("b", SymbolKind::Field),
        ("x", SymbolKind::Variable),
        ("y", SymbolKind::Variable),
    ] {
        let declared = symbol(&results, name, kind);
        assert_eq!(
            type_of(&results, declared),
            Some(("Integer".to_string(), false)),
            "{name}"
        );
        assert!(
            declared
                .signature
                .as_deref()
                .is_some_and(|s| s.ends_with("As Integer")),
            "{name}: {:?}",
            declared.signature
        );
    }
    assert_eq!(
        type_of(&results, symbol(&results, "late", SymbolKind::Variable)),
        Some(("After".to_string(), true))
    );
}

#[test]
fn if_operator_counts_as_a_decision() {
    let code = "Public Class T\n    Public Function Pick(a As Integer) As Integer\n        Return If(a > 0, 1, 2)\n    End Function\n    Public Function Pick2(a As String) As String\n        Return If(a, \"none\")\n    End Function\nEnd Class\n";
    let results = extract(code);
    for name in ["Pick", "Pick2"] {
        let id = &symbol(&results, name, SymbolKind::Method).id;
        let metric = results
            .complexity_metrics
            .iter()
            .find(|m| m.symbol_id.as_deref() == Some(id.as_str()))
            .unwrap();
        assert_eq!(metric.decision_count, 1, "{name}");
    }
}

#[test]
fn aspnet_controllers_and_http_clients_publish_framework_facts() {
    let code = "<RoutePrefix(\"api/orders\")>\nPublic Class OrdersController\n    Inherits ControllerBase\n    Private ReadOnly _client As HttpClient\n    <HttpGet>\n    <Route(\"{id:int}\")>\n    Public Function GetOrder(id As Integer) As IActionResult\n    End Function\n    <HttpPost(\"create\")>\n    Public Async Function Create(dto As OrderDto) As Task(Of IActionResult)\n        Dim resp = Await _client.GetAsync(\"https://api.example.com/orders\")\n        Dim r = Await _client.PostAsJsonAsync(\"https://api.example.com/items\", dto)\n        Dim req = New HttpRequestMessage(HttpMethod.Delete, \"https://api.example.com/x\")\n    End Function\nEnd Class\n";
    let results = extract(code);
    let routes: Vec<_> = results
        .structural_facts
        .iter()
        .filter(|f| f.pattern_id == "aspnet.attribute_route.v1")
        .map(|f| {
            (
                metadata_str(f, "attribute_kind").map(str::to_string),
                metadata_str(f, "verb").map(str::to_string),
                metadata_str(f, "effective_route_template").map(str::to_string),
            )
        })
        .collect();
    assert!(
        routes.contains(&(
            Some("controller_route".to_string()),
            None,
            Some("/api/orders".to_string())
        )),
        "{routes:?}"
    );
    assert!(
        routes.contains(&(
            Some("http_method".to_string()),
            Some("POST".to_string()),
            Some("/api/orders/create".to_string())
        )),
        "{routes:?}"
    );
    let requests: Vec<_> = results
        .structural_facts
        .iter()
        .filter(|f| f.pattern_id == "http.client_request.v1")
        .map(|f| {
            (
                metadata_str(f, "verb").map(str::to_string),
                metadata_str(f, "target_path").map(str::to_string),
            )
        })
        .collect();
    for (verb, url) in [
        ("GET", "https://api.example.com/orders"),
        ("POST", "https://api.example.com/items"),
        ("DELETE", "https://api.example.com/x"),
    ] {
        assert!(
            requests.contains(&(Some(verb.to_string()), Some(url.to_string()))),
            "{requests:?}"
        );
    }
    let mut literals = results.literals.clone();
    crate::classify_literals_by_carrier(&mut literals);
    for url in ["https://api.example.com/items", "https://api.example.com/x"] {
        assert!(
            literals
                .iter()
                .any(|l| l.literal_text == url && l.kind == crate::base::LiteralKind::Url),
            "{url}: {literals:?}"
        );
    }
}
