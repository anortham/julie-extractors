use crate::ExtractionResults;
use crate::base::{IdentifierKind, RelationshipKind, Symbol, SymbolKind, Visibility};
use crate::factory::extract_symbols_and_relationships;
use std::path::PathBuf;

fn extract(code: &str) -> ExtractionResults {
    let mut parser = super::init_test_parser();
    let tree = parser.parse(code, None).expect("parse");
    extract_symbols_and_relationships(
        &tree,
        "Gaps.cs",
        code,
        "csharp",
        &PathBuf::from("/test/workspace"),
    )
    .expect("extract")
}

fn symbol<'a>(results: &'a ExtractionResults, name: &str, kind: SymbolKind) -> &'a Symbol {
    results
        .symbols
        .iter()
        .find(|s| s.name == name && s.kind == kind)
        .unwrap_or_else(|| panic!("missing {kind:?} {name}"))
}

fn body_text<'a>(code: &'a str, symbol: &Symbol) -> Option<&'a str> {
    symbol
        .body_span
        .map(|span| &code[span.start_byte as usize..span.end_byte as usize])
}

fn edge_names(results: &ExtractionResults, kind: RelationshipKind) -> Vec<(String, String)> {
    let name_of = |id: &str| {
        results
            .symbols
            .iter()
            .find(|s| s.id == id)
            .map(|s| format!("{}:{:?}", s.name, s.kind))
            .unwrap_or_default()
    };
    results
        .relationships
        .iter()
        .filter(|r| r.kind == kind)
        .map(|r| (name_of(&r.from_symbol_id), name_of(&r.to_symbol_id)))
        .collect()
}

fn pending_targets(
    results: &ExtractionResults,
    kind: RelationshipKind,
) -> Vec<(String, String, Option<String>, Vec<String>)> {
    let name_of = |id: &str| {
        results
            .symbols
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.name.clone())
            .unwrap_or_default()
    };
    results
        .structured_pending_relationships
        .iter()
        .filter(|p| p.pending.kind == kind)
        .map(|p| {
            (
                name_of(&p.pending.from_symbol_id),
                p.target.terminal_name.clone(),
                p.target.receiver.clone(),
                p.target.namespace_path.clone(),
            )
        })
        .collect()
}

const INHERITANCE: &str = r#"
public class Cart { public Order Order { get; set; } }
public class Holder { public Entity Entity { get; set; } }
public abstract class RepositoryBase<T> where T : class { }
public abstract record Shape(string Name);
public class Order : Entity, IAuditable { }
public class OrderRepository : RepositoryBase<Order>, IRepository<Order> { }
public record Circle(double Radius) : Shape("circle");
public class Service(ILogger logger) : BaseService(logger), IService { }
public class MyException : System.Exception { }
namespace One { class A : B { } }
namespace Two { class A : C { } }
"#;

#[test]
fn base_list_edges_start_at_the_declared_type_and_use_bare_type_names() {
    let results = extract(INHERITANCE);
    let extends = edge_names(&results, RelationshipKind::Extends);
    assert!(
        extends.contains(&(
            "OrderRepository:Class".into(),
            "RepositoryBase:Class".into()
        )),
        "{extends:?}"
    );
    assert!(
        extends.contains(&("Circle:Class".into(), "Shape:Class".into())),
        "{extends:?}"
    );
    assert!(
        results.relationships.iter().all(|r| {
            let from = results
                .symbols
                .iter()
                .find(|s| s.id == r.from_symbol_id)
                .unwrap();
            let to = results
                .symbols
                .iter()
                .find(|s| s.id == r.to_symbol_id)
                .unwrap();
            from.kind != SymbolKind::Property && to.kind != SymbolKind::Property
        }),
        "{:?}",
        results.relationships
    );

    let mut pending = pending_targets(&results, RelationshipKind::Extends);
    pending.extend(pending_targets(&results, RelationshipKind::Implements));
    let has = |from: &str, terminal: &str| pending.iter().any(|p| p.0 == from && p.1 == terminal);
    assert!(has("Order", "Entity"), "{pending:?}");
    assert!(has("Order", "IAuditable"), "{pending:?}");
    assert!(has("OrderRepository", "IRepository"), "{pending:?}");
    assert!(has("Service", "BaseService"), "{pending:?}");
    assert!(has("Service", "IService"), "{pending:?}");
    assert!(
        pending.iter().any(|p| p.0 == "MyException"
            && p.1 == "Exception"
            && p.3 == vec!["System".to_string()]),
        "{pending:?}"
    );
    assert!(
        pending.iter().all(|p| !p.1.contains(['(', '<', '.'])),
        "{pending:?}"
    );

    let second_a = results
        .symbols
        .iter()
        .filter(|s| s.name == "A")
        .nth(1)
        .expect("second A");
    let c_edge = results
        .structured_pending_relationships
        .iter()
        .find(|p| p.target.terminal_name == "C")
        .expect("A : C");
    assert_eq!(c_edge.pending.from_symbol_id, second_a.id);
}

#[test]
fn generic_method_calls_emit_call_edges_and_bare_call_identifiers() {
    let code = r#"
public class Setup {
    public void Configure(IServiceCollection services, IConfiguration cfg, WebApplicationBuilder builder)
    {
        var c = Create<Customer>();
        var d = this.Create<Invoice>();
        builder.Services.AddDbContext<AppDbContext>();
        var opts = cfg.GetSection("Smtp").Get<SmtpOptions>();
        services.AddScoped<IFoo, Foo>();
    }
    public T Create<T>() where T : new() => new T();
}
"#;
    let results = extract(code);
    let calls = edge_names(&results, RelationshipKind::Calls);
    assert_eq!(
        calls
            .iter()
            .filter(|c| *c == &("Configure:Method".to_string(), "Create:Method".to_string()))
            .count(),
        2,
        "{calls:?}"
    );
    let pending = pending_targets(&results, RelationshipKind::Calls);
    assert!(
        pending
            .iter()
            .any(|p| p.1 == "AddDbContext" && p.2.as_deref() == Some("Services")),
        "{pending:?}"
    );
    assert!(pending.iter().any(|p| p.1 == "Get"), "{pending:?}");
    assert!(
        pending
            .iter()
            .any(|p| p.1 == "AddScoped" && p.2.as_deref() == Some("services")),
        "{pending:?}"
    );

    let call_names: Vec<&str> = results
        .identifiers
        .iter()
        .filter(|i| i.kind == IdentifierKind::Call)
        .map(|i| i.name.as_str())
        .collect();
    for name in ["Create", "AddDbContext", "Get", "AddScoped"] {
        assert!(call_names.contains(&name), "{name} in {call_names:?}");
    }
    assert!(
        call_names.iter().all(|name| !name.contains('<')),
        "{call_names:?}"
    );
    let type_usages: Vec<&str> = results
        .identifiers
        .iter()
        .filter(|i| i.kind == IdentifierKind::TypeUsage)
        .map(|i| i.name.as_str())
        .collect();
    for name in ["Create", "AddDbContext", "Get", "AddScoped"] {
        assert!(!type_usages.contains(&name), "{name} in {type_usages:?}");
    }
    let add_scoped = results
        .identifiers
        .iter()
        .find(|i| i.kind == IdentifierKind::Call && i.name == "AddScoped")
        .unwrap();
    let args: Vec<&str> = results
        .type_argument_usages
        .iter()
        .filter(|u| u.identifier_id == add_scoped.id)
        .flat_map(|u| u.arguments.iter().map(|a| a.type_name.as_str()))
        .collect();
    assert_eq!(args, vec!["IFoo", "Foo"]);
}

#[test]
fn null_conditional_calls_emit_call_targets_and_nameof_is_not_a_call() {
    let code = r#"
public class Worker
{
    private ILogger? _logger;
    public event EventHandler? Changed;
    public void Run(Worker? other)
    {
        other?.Step();
        _logger?.LogInformation("run");
        Changed?.Invoke(this, EventArgs.Empty);
        _client?.Inner.Flush();
        var n = nameof(Run);
    }
    public void Step() { }
}
"#;
    let results = extract(code);
    let pending = pending_targets(&results, RelationshipKind::Calls);
    let has = |terminal: &str, receiver: &str| {
        pending
            .iter()
            .any(|p| p.0 == "Run" && p.1 == terminal && p.2.as_deref() == Some(receiver))
    };
    assert!(has("Step", "other"), "{pending:?}");
    assert!(has("LogInformation", "_logger"), "{pending:?}");
    assert!(has("Invoke", "Changed"), "{pending:?}");
    assert!(
        pending.iter().any(|p| p.1 == "Flush"
            && p.2.as_deref() == Some("Inner")
            && p.3 == vec!["_client".to_string()]),
        "{pending:?}"
    );
    assert!(pending.iter().all(|p| p.1 != "nameof"), "{pending:?}");
    assert!(
        !results
            .identifiers
            .iter()
            .any(|i| i.kind == IdentifierKind::Call && i.name == "nameof")
    );
}

#[test]
fn bodyless_declarations_have_no_body_and_expression_bodies_use_the_arrow_clause() {
    let code = r#"
public interface IStore { Task SaveAsync(string key, byte[] data); }
public abstract partial class Store
{
    public abstract void Flush(int level);
    partial void OnSaved(string key);
    [DllImport("x")] static extern int Native(int a, int b);
    public delegate void Saved(string key);
    private readonly Dictionary<string, int> _map = CreateMap(4);
    public int Count => _map.Count;
    public string this[string k] => k;
    void Loop(List<int> items) { foreach (int i in items) { Flush(i); } }
}
public record Point(int X, int Y);
"#;
    let results = extract(code);
    for (name, kind) in [
        ("SaveAsync", SymbolKind::Method),
        ("Flush", SymbolKind::Method),
        ("OnSaved", SymbolKind::Method),
        ("Native", SymbolKind::Method),
        ("Saved", SymbolKind::Delegate),
        ("_map", SymbolKind::Field),
        ("Point", SymbolKind::Class),
    ] {
        let s = symbol(&results, name, kind);
        assert_eq!(body_text(code, s), None, "{name}");
        assert_eq!(s.body_hash, None, "{name}");
    }
    assert_eq!(
        body_text(code, symbol(&results, "Count", SymbolKind::Property)),
        Some("=> _map.Count")
    );
    let indexer = results
        .symbols
        .iter()
        .find(|s| s.kind == SymbolKind::Property && s.name.contains("this"))
        .expect("indexer");
    assert_eq!(body_text(code, indexer), Some("=> k"));
    assert_eq!(
        body_text(code, symbol(&results, "Loop", SymbolKind::Method)),
        Some("{ foreach (int i in items) { Flush(i); } }")
    );
    let loop_variable = symbol(&results, "i", SymbolKind::Variable);
    assert_eq!(
        &code[loop_variable.start_byte as usize..loop_variable.end_byte as usize],
        "int i"
    );
    assert_eq!(body_text(code, loop_variable), None);
}

#[test]
fn default_visibility_follows_the_declaration_context() {
    let code = r#"
class InternalByDefault { }
interface IShape
{
    double Area();
    string Describe() => "shape";
    int Sides { get; }
    event EventHandler Changed;
    private void Secret() { }
}
namespace Vis { struct Point { } enum Color { Red } }
public sealed class Singleton
{
    Singleton() { }
    class Nested { }
    public static Singleton Instance { get; } = new Singleton();
}
"#;
    let results = extract(code);
    let vis = |name: &str, kind: SymbolKind| symbol(&results, name, kind).visibility.clone();
    assert_eq!(
        vis("InternalByDefault", SymbolKind::Class),
        Some(Visibility::Internal)
    );
    assert_eq!(
        vis("IShape", SymbolKind::Interface),
        Some(Visibility::Internal)
    );
    assert_eq!(vis("Point", SymbolKind::Struct), Some(Visibility::Internal));
    assert_eq!(vis("Color", SymbolKind::Enum), Some(Visibility::Internal));
    assert_eq!(vis("Red", SymbolKind::EnumMember), Some(Visibility::Public));
    assert_eq!(vis("Area", SymbolKind::Method), Some(Visibility::Public));
    assert_eq!(
        vis("Describe", SymbolKind::Method),
        Some(Visibility::Public)
    );
    assert_eq!(vis("Sides", SymbolKind::Property), Some(Visibility::Public));
    assert_eq!(vis("Changed", SymbolKind::Event), Some(Visibility::Public));
    assert_eq!(vis("Secret", SymbolKind::Method), Some(Visibility::Private));
    assert_eq!(
        vis("Singleton", SymbolKind::Constructor),
        Some(Visibility::Private)
    );
    assert_eq!(vis("Nested", SymbolKind::Class), Some(Visibility::Private));
}

#[test]
fn base_member_calls_target_the_base_type_not_the_override() {
    let code = r#"
public class Local { public virtual void Configure() { } }
public class Derived : Local { public override void Configure() { base.Configure(); } }
public class MyController : Controller {
    public override void OnActionExecuting(ActionExecutingContext context) {
        base.OnActionExecuting(context);
    }
}
"#;
    let results = extract(code);
    assert!(
        results
            .relationships
            .iter()
            .all(|r| r.from_symbol_id != r.to_symbol_id),
        "{:?}",
        results.relationships
    );
    let local_configure = results
        .symbols
        .iter()
        .find(|s| {
            s.name == "Configure"
                && s.parent_id.as_deref()
                    == Some(symbol(&results, "Local", SymbolKind::Class).id.as_str())
        })
        .unwrap();
    assert!(
        results
            .relationships
            .iter()
            .any(|r| r.kind == RelationshipKind::Calls && r.to_symbol_id == local_configure.id)
    );
    let pending = results
        .structured_pending_relationships
        .iter()
        .find(|p| p.target.terminal_name == "OnActionExecuting")
        .expect("pending base call");
    assert_eq!(pending.target.receiver.as_deref(), Some("base"));
    assert_eq!(pending.receiver_type.as_deref(), Some("Controller"));
}
