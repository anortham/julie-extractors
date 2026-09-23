use crate::ExtractionResults;
use crate::base::{IdentifierKind, RelationshipKind, Symbol, SymbolKind, Visibility};
use crate::tests::helpers::metadata_str;
use std::path::Path;

fn extract_at(path: &str, code: &str) -> ExtractionResults {
    crate::pipeline::extract_canonical(path, code, Path::new("/repo")).expect("extract")
}

fn extract(code: &str) -> ExtractionResults {
    extract_at("src/Gaps.cs", code)
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

type PendingRow = (String, String, Option<String>, Option<String>);

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

fn role(symbol: &Symbol) -> Option<String> {
    symbol
        .metadata
        .as_ref()
        .and_then(|m| m.get("test_role"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

#[test]
fn property_and_indexer_bodies_own_their_calls_and_metrics() {
    let code = "public class Holder\n{\n    private int _v;\n    public Engine Current => new Engine();\n    public int Size\n    {\n        get\n        {\n            if (_v > 0) { return Compute(); }\n            return 0;\n        }\n    }\n    public int this[int i] => Compute() + i;\n    static int Compute() => 1;\n}\n";
    let results = extract(code);
    let calls = edges(&results, RelationshipKind::Calls);
    assert!(
        calls.contains(&("Size:Property".to_string(), "Compute:Method".to_string())),
        "{calls:?}"
    );
    assert!(
        calls.contains(&(
            "this[int i]:Property".to_string(),
            "Compute:Method".to_string()
        )),
        "{calls:?}"
    );
    assert!(
        pending(&results, RelationshipKind::Instantiates)
            .iter()
            .any(|(from, target, _, _)| from == "Current:Property" && target == "Engine")
    );
    assert!(
        idents(&results, "_v")
            .iter()
            .all(|(_, _, owner)| owner == "Size:Property")
    );
    let size = symbol(&results, "Size", SymbolKind::Property);
    let metric = results
        .complexity_metrics
        .iter()
        .find(|m| m.symbol_id.as_deref() == Some(size.id.as_str()))
        .expect("Size metric");
    assert_eq!(metric.decision_count, 1);
    let indexer = symbol(&results, "this[int i]", SymbolKind::Property);
    assert!(
        results
            .complexity_metrics
            .iter()
            .any(|m| m.symbol_id.as_deref() == Some(indexer.id.as_str()))
    );
}

#[test]
fn accessor_events_are_symbols_and_events_carry_type_facts() {
    let code = "public class Bus\n{\n    private EventHandler? _changed;\n    /// <summary>Raised on change.</summary>\n    public event EventHandler Changed\n    {\n        add => _changed += value;\n        remove => _changed -= value;\n    }\n    public event EventHandler? Simple;\n}\n";
    let results = extract(code);
    let changed = symbol(&results, "Changed", SymbolKind::Event);
    assert_eq!(
        changed.parent_id.as_deref(),
        Some(symbol(&results, "Bus", SymbolKind::Class).id.as_str())
    );
    assert_eq!(changed.visibility, Some(Visibility::Public));
    assert!(
        changed
            .doc_comment
            .as_deref()
            .is_some_and(|doc| doc.contains("Raised on change"))
    );
    assert!(changed.body_span.is_some());
    assert_eq!(
        type_of(&results, changed),
        Some(("EventHandler".to_string(), false))
    );
    assert_eq!(
        type_of(&results, symbol(&results, "Simple", SymbolKind::Event)),
        Some(("EventHandler".to_string(), false))
    );
    assert!(
        idents(&results, "_changed")
            .iter()
            .filter(|(_, line, _)| *line > 3)
            .all(|(_, _, owner)| owner == "Changed:Event")
    );
}

#[test]
fn attribute_names_are_type_usages_owned_by_the_decorated_symbol() {
    let code = "public sealed class AuditAttribute : Attribute { }\n[Audit]\n[ApiController]\npublic class UsersController : ControllerBase\n{\n    [Audit, HttpGet]\n    [return: NotNull]\n    public IActionResult Get([FromRoute] int id, [FromServices] IUserService svc) => Ok();\n}\n";
    let results = extract(code);
    for (name, owner) in [
        ("ApiController", "UsersController:Class"),
        ("HttpGet", "Get:Method"),
        ("NotNull", "Get:Method"),
        ("FromRoute", "Get:Method"),
        ("FromServices", "Get:Method"),
    ] {
        assert!(
            idents(&results, name)
                .iter()
                .any(|(kind, _, o)| *kind == IdentifierKind::TypeUsage && o == owner),
            "{name}: {:?}",
            idents(&results, name)
        );
    }
    assert_eq!(
        idents(&results, "Audit")
            .iter()
            .filter(|(kind, _, _)| *kind == IdentifierKind::TypeUsage)
            .count(),
        2
    );
}

#[test]
fn return_and_parameter_attributes_are_annotations() {
    let code = "public class UsersController\n{\n    [HttpGet]\n    [return: NotNull]\n    public IActionResult Get([FromRoute] int id, [FromServices] IUserService svc) => Ok();\n}\n";
    let results = extract(code);
    let get = symbol(&results, "Get", SymbolKind::Method);
    let not_null = get
        .annotations
        .iter()
        .find(|a| a.annotation == "NotNull")
        .unwrap_or_else(|| panic!("{:?}", get.annotations));
    assert_eq!(not_null.annotation_key, "notnull");
    assert_eq!(not_null.carrier.as_deref(), Some("return"));
    assert!(get.annotations.iter().all(|a| a.annotation != "return:"));
    for (param, attribute) in [("id", "FromRoute"), ("svc", "FromServices")] {
        let parameter = symbol(&results, param, SymbolKind::Variable);
        assert!(
            parameter
                .annotations
                .iter()
                .any(|a| a.annotation == attribute),
            "{param}: {:?}",
            parameter.annotations
        );
    }
}

#[test]
fn positional_record_parameters_are_public_properties() {
    let code = "public record Order(int Id, string Customer, decimal Total);\npublic readonly record struct Money(decimal Amount, string Currency);\npublic record struct Point(int X, int Y);\npublic class Printer(int width)\n{\n    public string Print(Order o) => o.Customer;\n}\n";
    let results = extract(code);
    let order = symbol(&results, "Order", SymbolKind::Class);
    for (name, type_name) in [("Id", "int"), ("Customer", "string"), ("Total", "decimal")] {
        let property = symbol(&results, name, SymbolKind::Property);
        assert_eq!(property.parent_id.as_deref(), Some(order.id.as_str()));
        assert_eq!(property.visibility, Some(Visibility::Public));
        assert_eq!(
            type_of(&results, property),
            Some((type_name.to_string(), false))
        );
    }
    assert!(
        symbol(&results, "Amount", SymbolKind::Property)
            .signature
            .as_deref()
            .is_some_and(|s| s.contains("init;"))
    );
    assert!(
        symbol(&results, "X", SymbolKind::Property)
            .signature
            .as_deref()
            .is_some_and(|s| s.contains("set;"))
    );
    assert_eq!(
        symbol(&results, "width", SymbolKind::Variable).visibility,
        Some(Visibility::Private)
    );
    assert!(
        results
            .symbols
            .iter()
            .all(|s| !(s.name == "Customer" && s.kind == SymbolKind::Variable))
    );
}

#[test]
fn top_level_statements_instantiate_from_the_file_scope() {
    let code = "var client = new HttpClient();\nConsole.WriteLine(new Report());\nbuilder.Services.AddSingleton(new SystemClock());\n";
    let results = extract_at("src/Program.cs", code);
    let rows = pending(&results, RelationshipKind::Instantiates);
    for target in ["HttpClient", "Report", "SystemClock"] {
        assert!(
            rows.iter()
                .any(|(from, t, _, _)| from.ends_with(":Module") && t == target),
            "{target}: {rows:?}"
        );
    }
}

#[test]
fn params_parameters_are_symbols_with_types() {
    let code = "public static class Log\n{\n    public static void Write(string fmt, params object[] args) => Console.WriteLine(fmt, args.Length);\n    public static int Sum(params ReadOnlySpan<int> xs) => xs.Length;\n}\n";
    let results = extract(code);
    let args = symbol(&results, "args", SymbolKind::Variable);
    assert_eq!(
        args.parent_id.as_deref(),
        Some(symbol(&results, "Write", SymbolKind::Method).id.as_str())
    );
    assert_eq!(
        type_of(&results, args),
        Some(("object[]".to_string(), false))
    );
    assert_eq!(
        type_of(&results, symbol(&results, "xs", SymbolKind::Variable)),
        Some(("ReadOnlySpan".to_string(), false))
    );
}

#[test]
fn bare_calls_bind_to_the_enclosing_type_first() {
    let code = "public class OrderService\n{\n    public void Save(Order order) { Validate(order); Missing(order); }\n    private void Validate(Order order) { }\n}\npublic class OtherValidator\n{\n    public void Validate(Order order) { }\n}\n";
    let results = extract(code);
    let validate = results
        .symbols
        .iter()
        .find(|s| {
            s.name == "Validate"
                && s.parent_id.as_deref()
                    == Some(
                        symbol(&results, "OrderService", SymbolKind::Class)
                            .id
                            .as_str(),
                    )
        })
        .unwrap();
    assert!(
        results
            .relationships
            .iter()
            .any(|r| r.kind == RelationshipKind::Calls && r.to_symbol_id == validate.id)
    );
    let missing = pending(&results, RelationshipKind::Calls);
    assert!(
        missing
            .iter()
            .any(|(_, t, receiver, receiver_type)| t == "Missing"
                && receiver.is_none()
                && receiver_type.as_deref() == Some("OrderService")),
        "{missing:?}"
    );
}

#[test]
fn using_directives_keep_their_alias_target_and_global_marker() {
    let code = "using Json = System.Text.Json.JsonSerializer;\nglobal using System.Linq;\nusing Pair = (int X, int Y);\nnamespace App;\npublic class A { public string S(object o) => Json.Serialize(o); }\n";
    let results = extract(code);
    let signature = |name: &str| {
        symbol(&results, name, SymbolKind::Import)
            .signature
            .clone()
            .unwrap_or_default()
    };
    assert_eq!(
        signature("Json"),
        "using Json = System.Text.Json.JsonSerializer"
    );
    assert_eq!(signature("Linq"), "global using System.Linq");
    assert_eq!(signature("Pair"), "using Pair = (int X, int Y)");
    assert!(
        symbol(&results, "Pair", SymbolKind::Import)
            .body_span
            .is_none()
    );
}

#[test]
fn type_facts_and_signatures_use_the_written_type() {
    let code = "public class T1\n{\n    public (int A, int B) Pair { get; set; }\n    public (int Count, string Name) GetTuple() => (1, \"a\");\n    public unsafe int* Ptr;\n    public ref int RefReturn(int[] arr) => ref arr[0];\n    public void Run() { }\n    public static Bits? operator +(Bits? a, Bits? b) => a;\n}\n";
    let results = extract(code);
    for name in ["Pair", "Ptr"] {
        let declared = results.symbols.iter().find(|s| s.name == name).unwrap();
        assert_eq!(type_of(&results, declared), None, "{name}");
    }
    for name in ["GetTuple", "Run"] {
        assert_eq!(
            type_of(&results, symbol(&results, name, SymbolKind::Method)),
            None,
            "{name}"
        );
    }
    let signature = |name: &str| {
        results
            .symbols
            .iter()
            .find(|s| s.name == name)
            .and_then(|s| s.signature.clone())
            .unwrap_or_default()
    };
    assert!(
        signature("Ptr").contains("int* Ptr"),
        "{}",
        signature("Ptr")
    );
    assert!(
        signature("RefReturn").contains("ref int RefReturn"),
        "{}",
        signature("RefReturn")
    );
    assert!(
        signature("operator +").contains("Bits? operator +"),
        "{}",
        signature("operator +")
    );
}

#[test]
fn unsigned_right_shift_operator_is_a_symbol() {
    let code = "public readonly struct Bits\n{\n    public static Bits operator >>>(Bits b, int s) => b;\n}\n";
    let results = extract(code);
    let op = results
        .symbols
        .iter()
        .find(|s| s.name == "operator >>>")
        .unwrap_or_else(|| panic!("{:?}", names(&results)));
    assert_eq!(
        op.parent_id.as_deref(),
        Some(symbol(&results, "Bits", SymbolKind::Struct).id.as_str())
    );
    assert_eq!(
        symbol(&results, "s", SymbolKind::Variable)
            .parent_id
            .as_deref(),
        Some(op.id.as_str())
    );
}

#[test]
fn type_positions_are_not_values_or_calls() {
    let code = "public class Mapper<TEntity> where TEntity : EntityBase\n{\n    public Dto? Map(object o) => o as Dto;\n    public T Make<T>() where T : new() => new T();\n}\n";
    let results = extract(code);
    assert!(
        idents(&results, "Dto")
            .iter()
            .all(|(kind, _, _)| *kind == IdentifierKind::TypeUsage),
        "{:?}",
        idents(&results, "Dto")
    );
    assert!(
        idents(&results, "TEntity")
            .iter()
            .all(|(kind, _, _)| *kind != IdentifierKind::VariableRef)
    );
    assert!(
        idents(&results, "T")
            .iter()
            .all(|(kind, _, _)| *kind == IdentifierKind::TypeUsage),
        "{:?}",
        idents(&results, "T")
    );
    assert!(pending(&results, RelationshipKind::Instantiates).is_empty());
}

#[test]
fn extension_blocks_publish_the_receiver_and_the_extended_type() {
    let code = "public static class StringExtensions\n{\n    extension(string text)\n    {\n        public bool IsBlank => string.IsNullOrWhiteSpace(text);\n        public string Twice() => text + text;\n    }\n}\n";
    let results = extract(code);
    let receiver = symbol(&results, "text", SymbolKind::Variable);
    assert_eq!(
        type_of(&results, receiver),
        Some(("string".to_string(), false))
    );
    for (name, kind) in [
        ("IsBlank", SymbolKind::Property),
        ("Twice", SymbolKind::Method),
    ] {
        let member = symbol(&results, name, kind);
        assert_eq!(
            member
                .metadata
                .as_ref()
                .and_then(|m| m.get("extendedType"))
                .and_then(|v| v.as_str()),
            Some("string"),
            "{name}"
        );
    }
    assert!(
        idents(&results, "text")
            .iter()
            .all(|(_, line, owner)| *line == 3 || owner != "StringExtensions:Class")
    );
}

#[test]
fn implicit_lambda_parameters_have_no_declared_type() {
    let code = "public class L\n{\n    public void Run(List<int> items)\n    {\n        Func<int, int, int> add = (a, b) => a + b;\n        items.Aggregate((acc, next) => acc.CompareTo(next));\n        items.Select(s => s);\n        Func<int, int> typed = (int x) => x;\n    }\n}\n";
    let results = extract(code);
    for name in ["a", "b", "acc", "next", "s"] {
        let parameter = symbol(&results, name, SymbolKind::Variable);
        assert!(
            type_of(&results, parameter).is_none_or(|(t, _)| t != name),
            "{name}: {:?}",
            type_of(&results, parameter)
        );
        assert_eq!(parameter.signature.as_deref(), Some(name));
    }
    assert_eq!(
        type_of(&results, symbol(&results, "x", SymbolKind::Variable)),
        Some(("int".to_string(), false))
    );
}

#[test]
fn pattern_designations_are_typed_locals() {
    let code = "public class P\n{\n    public void Run(object o)\n    {\n        if (o is Order order)\n            order.Ship();\n        switch (o)\n        {\n            case Customer c when c.IsActive:\n                c.Notify();\n                break;\n        }\n        var r = o switch { Invoice { Id: 1 } inv => inv, _ => null };\n    }\n}\n";
    let results = extract(code);
    let run = symbol(&results, "Run", SymbolKind::Method);
    for (name, type_name) in [("order", "Order"), ("c", "Customer"), ("inv", "Invoice")] {
        let local = symbol(&results, name, SymbolKind::Variable);
        assert_eq!(local.parent_id.as_deref(), Some(run.id.as_str()));
        assert_eq!(
            type_of(&results, local),
            Some((type_name.to_string(), false)),
            "{name}"
        );
    }
    assert!(
        idents(&results, "order")
            .iter()
            .all(|(_, line, _)| *line != 5)
    );
}

#[test]
fn target_typed_new_instantiates_the_contextual_type() {
    let code = "public class Widget { public Widget(int size) { } }\npublic class Shop\n{\n    private readonly Widget _w = new(3);\n    public Widget Current { get; } = new(4);\n    public Widget Make()\n    {\n        Widget local = new(5);\n        return new(6);\n    }\n    public Widget Arrow() => new(7);\n}\n";
    let results = extract(code);
    let rows = edges(&results, RelationshipKind::Instantiates);
    let widget_edges = rows.iter().filter(|(_, to)| to == "Widget:Class").count();
    assert_eq!(widget_edges, 5, "{rows:?}");
    assert_eq!(
        idents(&results, "Widget")
            .iter()
            .filter(|(kind, _, _)| *kind == IdentifierKind::Call)
            .count(),
        5
    );
}

#[test]
fn conditional_compilation_extracts_like_the_preprocessed_source() {
    let code = "public class Converter\n{\n    public static int Kind(object value)\n    {\n        if (value == null) { return 0; }\n#if HAVE_ADO_NET\n        else if (value == System.DBNull.Value) { return 0; }\n#endif\n        else if (value is string) { return 1; }\n        return 2;\n    }\n    public static int After() => 3;\n}\n";
    let results = extract(code);
    let converter = symbol(&results, "Converter", SymbolKind::Class);
    for name in ["Kind", "After"] {
        assert_eq!(
            symbol(&results, name, SymbolKind::Method)
                .parent_id
                .as_deref(),
            Some(converter.id.as_str()),
            "{name}"
        );
    }
    assert!(
        results.parse_diagnostics.is_empty(),
        "{:?}",
        results.parse_diagnostics
    );
    assert!(results.symbols.iter().all(|s| s.name != "System"));
}

#[test]
fn test_framework_idioms_get_roles() {
    let code = "public class TUnitTests\n{\n    [Before(HookType.Test)] public void Setup() { }\n    [Test][Arguments(1, 2)] public async Task Adds(int a, int b) { }\n    [Test][MethodDataSource(nameof(Rows))] public void Rows(int a) { }\n    [After(HookType.Test)] public void Cleanup() { }\n}\n[Binding]\npublic class CartSteps\n{\n    [Given(@\"an empty cart\")] public void GivenEmptyCart() { }\n    [BeforeScenario] public void Reset() { }\n    [AfterScenario] public void Tidy() { }\n}\n[Subject(typeof(Cart))]\npublic class When_adding_an_item\n{\n    static Cart cart;\n    Establish context = () => cart = new Cart();\n    Because of = () => cart.Add(1);\n    It should_have_one_item = () => cart.Count.ShouldEqual(1);\n    Cleanup after = () => cart = null;\n}\npublic class Ordinary\n{\n    Action run = () => Work();\n}\n";
    let results = extract(code);
    let role_of = |name: &str| {
        results
            .symbols
            .iter()
            .find(|s| s.name == name)
            .and_then(role)
    };
    assert_eq!(role_of("Setup").as_deref(), Some("fixture_setup"));
    assert_eq!(role_of("Adds").as_deref(), Some("parameterized_test"));
    assert_eq!(role_of("Rows").as_deref(), Some("parameterized_test"));
    assert_eq!(role_of("Cleanup").as_deref(), Some("fixture_teardown"));
    assert_eq!(role_of("CartSteps").as_deref(), Some("test_container"));
    assert_eq!(role_of("Reset").as_deref(), Some("fixture_setup"));
    assert_eq!(role_of("Tidy").as_deref(), Some("fixture_teardown"));
    assert_eq!(
        role_of("When_adding_an_item").as_deref(),
        Some("test_container")
    );
    assert_eq!(
        role_of("should_have_one_item$lambda").as_deref(),
        Some("test_case")
    );
    assert_eq!(role_of("context$lambda").as_deref(), Some("fixture_setup"));
    assert_eq!(role_of("of$lambda").as_deref(), Some("fixture_setup"));
    assert_eq!(role_of("after$lambda").as_deref(), Some("fixture_teardown"));
    assert_eq!(role_of("Ordinary"), None);
    assert_eq!(role_of("run$lambda"), None);
}

#[test]
fn sql_and_request_constructors_are_literal_carriers() {
    let code = "public class Repo\n{\n    public void Run(Db db, SqlConnection c)\n    {\n        var d2 = db.Database.SqlQuery<int>($\"SELECT COUNT(*) FROM Users\");\n        var d3 = db.Database.SqlQueryRaw<int>(\"SELECT COUNT(*) FROM Orders\");\n        var u = db.Users.FromSql($\"SELECT * FROM Users\");\n        db.Database.ExecuteSqlAsync($\"DELETE FROM Users\");\n        var cmd = new SqlCommand(\"SELECT 1 FROM Items\", c);\n        var req = new HttpRequestMessage(HttpMethod.Get, \"https://api.example.com/x\");\n    }\n}\n";
    let results = extract(code);
    let mut literals = results.literals.clone();
    crate::classify_literals_by_carrier(&mut literals);
    for text in [
        "SELECT COUNT(*) FROM Users",
        "SELECT COUNT(*) FROM Orders",
        "SELECT * FROM Users",
        "DELETE FROM Users",
        "SELECT 1 FROM Items",
    ] {
        assert!(
            literals
                .iter()
                .any(|l| l.literal_text == text && l.kind == crate::base::LiteralKind::Sql),
            "{text}: {literals:?}"
        );
    }
    assert!(
        literals
            .iter()
            .any(|l| l.literal_text == "https://api.example.com/x"
                && l.kind == crate::base::LiteralKind::Url)
    );
}

#[test]
fn aspnet_routes_join_method_templates_and_cover_endpoint_maps() {
    let controller = "[Route(\"api/orders\")]\npublic class OrdersController : ControllerBase\n{\n    [HttpGet]\n    [Route(\"separate\")]\n    public IActionResult Separate() => Ok();\n    [HttpPost, Route(\"combined\")]\n    public IActionResult Combined() => Ok();\n    [AcceptVerbs(\"GET\", \"POST\")]\n    [Route(\"search\")]\n    public IActionResult Search() => Ok();\n}\n";
    let results = extract_at("src/OrdersController.cs", controller);
    let routes: Vec<_> = results
        .structural_facts
        .iter()
        .filter(|f| f.pattern_id == "aspnet.attribute_route.v1")
        .map(|f| {
            (
                metadata_str(f, "verb").map(str::to_string),
                metadata_str(f, "effective_route_template").map(str::to_string),
            )
        })
        .collect();
    for (verb, route) in [
        ("GET", "/api/orders/separate"),
        ("POST", "/api/orders/combined"),
        ("GET", "/api/orders/search"),
        ("POST", "/api/orders/search"),
    ] {
        assert!(
            routes.contains(&(Some(verb.to_string()), Some(route.to_string()))),
            "{verb} {route}: {routes:?}"
        );
    }
    assert!(
        !routes.contains(&(Some("GET".to_string()), Some("/api/orders".to_string()))),
        "{routes:?}"
    );

    let program = "var api = app.MapGroup(\"/api/v1\");\nvar orders = api.MapGroup(\"/orders\");\norders.MapGet(\"/{id:int}\", GetOrder);\napp.MapHub<ChatHub>(\"/hubs/chat\");\napp.MapControllerRoute(name: \"default\", pattern: \"{controller=Home}/{action=Index}/{id?}\");\napp.Map(\"/any\", () => \"any\");\napp.MapHealthChecks(\"/health\");\n";
    let results = extract_at("src/Program.cs", program);
    let minimal: Vec<_> = results
        .structural_facts
        .iter()
        .filter(|f| {
            f.pattern_id == "aspnet.minimal_api.route.v1"
                || f.pattern_id == "aspnet.minimal_api.route_group.v1"
        })
        .map(|f| {
            (
                metadata_str(f, "normalized_route_template").map(str::to_string),
                metadata_str(f, "endpoint_kind").map(str::to_string),
            )
        })
        .collect();
    for (route, kind) in [
        ("/api/v1/orders", None),
        ("/api/v1/orders/:id", None),
        ("/hubs/chat", Some("signalr_hub")),
        ("/any", Some("any_verb")),
        ("/health", Some("health_checks")),
    ] {
        assert!(
            minimal.contains(&(Some(route.to_string()), kind.map(str::to_string))),
            "{route}: {minimal:?}"
        );
    }
    let conventional = results
        .structural_facts
        .iter()
        .find(|f| f.pattern_id == "aspnet.conventional_route.v1")
        .expect("conventional route fact");
    assert_eq!(metadata_str(conventional, "route_name"), Some("default"));
    assert_eq!(
        metadata_str(conventional, "route_template"),
        Some("{controller=Home}/{action=Index}/{id?}")
    );
}

#[test]
fn efcore_models_publish_sets_tables_and_configurations() {
    let code = "public class ShopDbContext(DbContextOptions<ShopDbContext> o) : DbContext(o)\n{\n    public DbSet<Order> Orders => Set<Order>();\n    public DbSet<Customer> Customers { get; set; }\n    protected override void OnModelCreating(ModelBuilder mb)\n    {\n        mb.Entity<Order>().ToTable(\"orders\").HasKey(x => x.Id);\n    }\n}\npublic class CustomerConfiguration : IEntityTypeConfiguration<Customer>\n{\n    public void Configure(EntityTypeBuilder<Customer> b) { b.ToTable(\"customers\"); }\n}\n";
    let results = extract(code);
    let facts = |pattern: &str| -> Vec<_> {
        results
            .structural_facts
            .iter()
            .filter(|f| f.pattern_id == pattern)
            .collect()
    };
    let sets = facts("efcore.db_set.v1");
    for (property, entity) in [("Orders", "Order"), ("Customers", "Customer")] {
        assert!(
            sets.iter()
                .any(|f| metadata_str(f, "property_name") == Some(property)
                    && metadata_str(f, "entity_type") == Some(entity)
                    && metadata_str(f, "context_type") == Some("ShopDbContext")),
            "{property}: {sets:#?}"
        );
    }
    let tables = facts("efcore.table_mapping.v1");
    for (entity, table) in [("Order", "orders"), ("Customer", "customers")] {
        assert!(
            tables
                .iter()
                .any(|f| metadata_str(f, "entity_type") == Some(entity)
                    && metadata_str(f, "table_name") == Some(table)),
            "{entity}: {tables:#?}"
        );
    }
    let configurations = facts("efcore.entity_configuration.v1");
    assert!(
        configurations
            .iter()
            .any(|f| metadata_str(f, "entity_type") == Some("Customer")
                && metadata_str(f, "configuration_type") == Some("CustomerConfiguration"))
    );
}
