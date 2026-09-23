use crate::ExtractionResults;
use crate::base::{IdentifierKind, RelationshipKind, Symbol, SymbolKind, Visibility};
use crate::pipeline::extract_canonical;
use crate::tests::helpers::metadata_str;
use std::path::Path;

fn extract_at(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/workspace")).expect("extract")
}

fn extract(source: &str) -> ExtractionResults {
    extract_at("src/Wave.fs", source)
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

fn label(results: &ExtractionResults, id: Option<&str>) -> String {
    id.and_then(|id| results.symbols.iter().find(|s| s.id == id))
        .map(|s| format!("{}:{:?}", s.name, s.kind))
        .unwrap_or_default()
}

fn parent(results: &ExtractionResults, symbol: &Symbol) -> String {
    label(results, symbol.parent_id.as_deref())
}

fn type_of(results: &ExtractionResults, symbol: &Symbol) -> Option<String> {
    results
        .types
        .get(&symbol.id)
        .map(|t| t.resolved_type.clone())
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
                label(results, i.containing_symbol_id.as_deref()),
            )
        })
        .collect()
}

fn facts<'a>(
    results: &'a ExtractionResults,
    pattern: &str,
) -> Vec<&'a crate::base::StructuralFact> {
    results
        .structural_facts
        .iter()
        .filter(|f| f.pattern_id == pattern)
        .collect()
}

#[test]
fn class_member_forms_are_symbols() {
    let code = "type Repo(conn: string, timeout: int) =\n    new() = Repo(\"x\", 1)\n    member val Name = \"repo\" with get, set\n    static member val Instances = 0 with get, set\n    abstract member Label: string with get, set\n    default val Label = \"\" with get, set\n    member _.Connection = conn\n    static member (+) (a: Repo, b: Repo) = a\ntype Vector =\n    val X: float\n    new(x) = { X = x }\n";
    let results = extract(code);
    let repo = symbol(&results, "Repo", SymbolKind::Class);
    for (name, kind) in [
        ("Name", SymbolKind::Property),
        ("Instances", SymbolKind::Property),
        ("Connection", SymbolKind::Property),
        ("(+)", SymbolKind::Method),
    ] {
        assert_eq!(
            symbol(&results, name, kind.clone()).parent_id.as_deref(),
            Some(repo.id.as_str()),
            "{name}"
        );
    }
    assert!(
        results
            .symbols
            .iter()
            .any(|s| s.name == "new" && s.kind == SymbolKind::Constructor)
    );
    assert!(results.symbols.iter().all(|s| s.name != "val"));
    assert_eq!(
        parent(&results, symbol(&results, "X", SymbolKind::Field)),
        "Vector:Class"
    );
    for (name, type_name) in [("conn", "string"), ("timeout", "int")] {
        let parameter = symbol(&results, name, SymbolKind::Variable);
        assert_eq!(parent(&results, parameter), "Repo:Class");
        assert_eq!(type_of(&results, parameter).as_deref(), Some(type_name));
    }
    let instantiations: Vec<_> = results
        .relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::Instantiates)
        .map(|r| label(&results, Some(&r.from_symbol_id)))
        .collect();
    assert_eq!(instantiations, vec!["new:Constructor".to_string()]);
}

#[test]
fn module_declaration_forms_are_symbols() {
    let code = "module Decls\nexception InvalidOrder of string\nlet (|Even|Odd|) n = if n % 2 = 0 then Even else Odd\nlet (|Positive|_|) n = if n > 0 then Some n else None\nlet inline (+.) x y = x + y\n[<Measure>] type kg\nlet (num, label) = (1, \"a\")\nlet a, b = 1, 2\nlet { X = px } = point\nextern int GetTickCount()\n";
    let results = extract(code);
    symbol(&results, "InvalidOrder", SymbolKind::Class);
    for name in ["(|Even|Odd|)", "(|Positive|_|)", "(+.)", "GetTickCount"] {
        symbol(&results, name, SymbolKind::Function);
    }
    symbol(&results, "kg", SymbolKind::Type);
    for name in ["num", "label", "a", "b", "px"] {
        assert_eq!(
            parent(&results, symbol(&results, name, SymbolKind::Variable)),
            "Decls:Module",
            "{name}"
        );
    }
    assert!(results.symbols.iter().all(|s| s.name != "X"));
    assert!(
        idents(&results, "Some")
            .iter()
            .all(|(_, _, owner)| owner == "(|Positive|_|):Function")
    );
}

#[test]
fn type_extension_members_belong_to_the_extended_type() {
    let code = "module Ext\ntype Shape = Circle of float\ntype Shape with\n    member this.Area = 1.0\ntype System.String with\n    member this.Shout() = this.ToUpper()\n";
    let results = extract(code);
    let area = symbol(&results, "Area", SymbolKind::Property);
    assert_eq!(parent(&results, area), "Shape:Union");
    let shout = symbol(&results, "Shout", SymbolKind::Method);
    assert_eq!(
        shout
            .metadata
            .as_ref()
            .and_then(|m| m.get("extendedType"))
            .and_then(|v| v.as_str()),
        Some("System.String")
    );
    let pending = results
        .structured_pending_relationships
        .iter()
        .find(|p| p.target.terminal_name == "ToUpper")
        .expect("ToUpper call");
    assert_eq!(pending.receiver_type.as_deref(), Some("System.String"));
}

#[test]
fn declaration_names_are_not_variable_refs() {
    let code = "module D\ntype IA =\n    abstract Run: input: int -> int\nlet apply xs = List.map (fun item -> item + 1) xs\nlet res () =\n    use stream = new System.IO.MemoryStream()\n    stream.Length\n";
    let results = extract(code);
    for name in ["Run", "input"] {
        assert!(
            idents(&results, name).is_empty(),
            "{name}: {:?}",
            idents(&results, name)
        );
    }
    assert_eq!(idents(&results, "item").len(), 1);
    assert!(
        idents(&results, "stream")
            .iter()
            .all(|(kind, line, _)| *kind != IdentifierKind::VariableRef && *line == 7)
    );
    let stream = symbol(&results, "stream", SymbolKind::Variable);
    assert_eq!(parent(&results, stream), "res:Function");
    assert_eq!(stream.visibility, Some(Visibility::Private));
}

#[test]
fn attribute_facts_attach_to_the_annotated_member() {
    let code = "module A\n[<ApiController>]\ntype Controller() =\n    [<HttpGet>]\n    member _.Get() = 1\n    [<HttpPost>]\n    member _.Post() = 2\nlet [<Literal>] MaxSize = 100\n";
    let results = extract(code);
    let owners: Vec<_> = facts(&results, "fsharp.attribute.v1")
        .into_iter()
        .map(|f| label(&results, f.containing_symbol_id.as_deref()))
        .collect();
    assert_eq!(
        owners,
        vec![
            "Controller:Class".to_string(),
            "Get:Method".to_string(),
            "Post:Method".to_string(),
            "MaxSize:Constant".to_string(),
        ]
    );
    let max_size = symbol(&results, "MaxSize", SymbolKind::Constant);
    assert!(
        max_size
            .annotations
            .iter()
            .any(|a| a.annotation == "Literal")
    );
}

#[test]
fn common_type_forms_record_type_facts() {
    let code = "module T\ntype Repo() =\n    member this.Find(id: int) : string option = None\n    member _.Join(left, right) : string = left + right\nlet first (xs: Order[]) = xs\nlet lst (xs: Order list) = xs\nlet i = 10L\nlet u = 3u\nlet m (x: Map<string, Order list>) = x\n";
    let results = extract(code);
    assert_eq!(
        type_of(&results, symbol(&results, "Find", SymbolKind::Method)).as_deref(),
        Some("option")
    );
    assert_eq!(
        type_of(&results, symbol(&results, "Join", SymbolKind::Method)).as_deref(),
        Some("string")
    );
    for name in ["left", "right"] {
        assert_eq!(
            type_of(&results, symbol(&results, name, SymbolKind::Variable)),
            None
        );
    }
    let xs: Vec<_> = results
        .symbols
        .iter()
        .filter(|s| s.name == "xs")
        .filter_map(|s| type_of(&results, s))
        .collect();
    assert_eq!(xs, vec!["Order[]".to_string(), "list".to_string()]);
    assert_eq!(
        type_of(&results, symbol(&results, "i", SymbolKind::Variable)).as_deref(),
        Some("int64")
    );
    assert_eq!(
        type_of(&results, symbol(&results, "u", SymbolKind::Variable)).as_deref(),
        Some("uint32")
    );
    assert!(results.literals.iter().any(|l| l.literal_text == "10L"));
    let list_usage = results
        .type_argument_usages
        .iter()
        .find(|usage| {
            results
                .identifiers
                .iter()
                .any(|i| i.id == usage.identifier_id && i.name == "list" && i.start_line == 6)
        })
        .expect("Order list usage");
    assert_eq!(list_usage.arguments[0].type_name, "Order");
}

#[test]
fn signature_file_values_record_declared_types() {
    let code =
        "namespace Sig\nmodule M =\n    val add: x: int -> y: int -> int\n    val pi: float\n";
    let results = extract_at("src/Sig.fsi", code);
    assert_eq!(
        type_of(&results, symbol(&results, "add", SymbolKind::Function)).as_deref(),
        Some("int")
    );
    assert_eq!(
        type_of(&results, symbol(&results, "pi", SymbolKind::Variable)).as_deref(),
        Some("float")
    );
    assert!(idents(&results, "x").is_empty());
}

#[test]
fn recursive_namespace_keeps_its_name() {
    let results = extract("namespace rec MyApp.Core\n\ntype A() =\n    member _.B = 1\n");
    let namespace = symbol(&results, "MyApp.Core", SymbolKind::Namespace);
    assert_eq!(
        symbol(&results, "A", SymbolKind::Class)
            .parent_id
            .as_deref(),
        Some(namespace.id.as_str())
    );
}

#[test]
fn type_and_binding_kinds_follow_the_declaration() {
    let code = "module K\ntype IShape =\n    abstract Area: unit -> float\n[<Interface>]\ntype IMarker =\n    abstract Tag: string\n[<Struct>]\ntype Vector =\n    val X: float\ntype Color =\n    | Red = 0\n    | Green = 1\nlet classify = function\n    | 0 -> \"zero\"\n    | _ -> \"other\"\nlet [<Literal>] MaxSize = 100\n";
    let results = extract(code);
    symbol(&results, "IShape", SymbolKind::Interface);
    symbol(&results, "IMarker", SymbolKind::Interface);
    let vector = symbol(&results, "Vector", SymbolKind::Struct);
    assert!(
        vector
            .signature
            .as_deref()
            .is_some_and(|s| s.starts_with("type Vector"))
    );
    for name in ["Red", "Green"] {
        assert_eq!(
            parent(&results, symbol(&results, name, SymbolKind::EnumMember)),
            "Color:Enum"
        );
    }
    let classify = symbol(&results, "classify", SymbolKind::Function);
    assert!(
        results
            .complexity_metrics
            .iter()
            .any(|m| m.symbol_id.as_deref() == Some(classify.id.as_str()))
    );
    symbol(&results, "MaxSize", SymbolKind::Constant);
}

#[test]
fn locals_belong_to_their_function_and_are_private() {
    let code = "module L\nlet compute () =\n    let a = 1\n    let b = a + 1\n    let c = b * 2\n    a + b + c\ntype Vec = { X: float; Y: float } with\n    member this.Length = sqrt (this.X * this.X)\n";
    let results = extract(code);
    for name in ["a", "b", "c"] {
        let local = symbol(&results, name, SymbolKind::Variable);
        assert_eq!(parent(&results, local), "compute:Function", "{name}");
        assert_eq!(local.visibility, Some(Visibility::Private));
    }
    assert!(
        idents(&results, "sqrt")
            .iter()
            .all(|(_, _, owner)| owner == "Length:Property")
    );
}

#[test]
fn call_targets_are_receivers_not_namespaces() {
    let code = "module C\nlet chained (s: string) = s.Trim().ToLower().Split(',')\nlet first (xs: int[]) = xs[0]\nlet slice (xs: int list) = xs[1..]\nlet st = struct (1, 2)\nlet stream () = new System.IO.MemoryStream()\n";
    let results = extract(code);
    let pending: Vec<_> = results
        .structured_pending_relationships
        .iter()
        .filter(|p| p.pending.kind == RelationshipKind::Calls)
        .map(|p| {
            (
                p.target.terminal_name.clone(),
                p.target.receiver.clone(),
                p.target.namespace_path.clone(),
            )
        })
        .collect();
    assert!(
        pending.contains(&(
            "ToLower".to_string(),
            Some("s.Trim()".to_string()),
            Vec::new()
        )),
        "{pending:?}"
    );
    assert!(
        pending
            .iter()
            .all(|(target, _, _)| target != "xs" && target != "struct"),
        "{pending:?}"
    );
    assert!(pending.contains(&(
        "MemoryStream".to_string(),
        None,
        vec!["System".to_string(), "IO".to_string()]
    )));
}

#[test]
fn web_routes_and_http_calls_are_framework_facts() {
    let code = "namespace Web\n[<ApiController>]\n[<Route(\"api/[controller]\")>]\ntype UsersController(svc: IUserService) =\n    inherit ControllerBase()\n    [<HttpGet(\"{id}\")>]\n    member this.Get(id: int) = this.Ok(svc.Find id)\nmodule App =\n    let webApp =\n        choose [\n            GET >=> choose [\n                route \"/ping\" >=> text \"pong\"\n                routef \"/orders/%i\" getOrderHandler\n                subRoute \"/api\" (choose [ route \"/health\" >=> text \"ok\" ]) ]\n            POST >=> route \"/orders\" >=> createOrderHandler ]\n    let configure (app: WebApplication) =\n        app.MapGet(\"/hello\", Func<string>(fun () -> \"Hello\")) |> ignore\n    let fetch (client: HttpClient) = client.GetAsync(\"https://api.example.com/users\")\n";
    let results = extract(code);
    let attribute_routes: Vec<_> = facts(&results, "aspnet.attribute_route.v1")
        .into_iter()
        .filter_map(|f| metadata_str(f, "effective_route_template"))
        .collect();
    assert!(
        attribute_routes.contains(&"/api/users/{id}"),
        "{attribute_routes:?}"
    );
    let route_owners: Vec<_> = facts(&results, "aspnet.attribute_route.v1")
        .into_iter()
        .map(|f| label(&results, f.containing_symbol_id.as_deref()))
        .collect();
    assert_eq!(
        route_owners,
        vec![
            "UsersController:Class".to_string(),
            "Get:Method".to_string()
        ]
    );
    let url = results
        .literals
        .iter()
        .find(|l| l.literal_text == "https://api.example.com/users")
        .expect("url literal");
    assert_eq!(url.carrier.as_deref(), Some("GetAsync"));
    let route_literal = results
        .literals
        .iter()
        .find(|l| l.literal_text == "/orders/%i")
        .expect("routef literal");
    assert_eq!(route_literal.carrier.as_deref(), Some("routef"));
    let giraffe: Vec<_> = facts(&results, "giraffe.route.v1")
        .into_iter()
        .map(|f| {
            (
                metadata_str(f, "verb").map(str::to_string),
                metadata_str(f, "normalized_route_template").map(str::to_string),
            )
        })
        .collect();
    for (verb, route) in [
        ("GET", "/ping"),
        ("GET", "/orders/:arg1"),
        ("GET", "/api/health"),
        ("POST", "/orders"),
    ] {
        assert!(
            giraffe.contains(&(Some(verb.to_string()), Some(route.to_string()))),
            "{verb} {route}: {giraffe:?}"
        );
    }
    assert!(
        facts(&results, "aspnet.minimal_api.route.v1")
            .iter()
            .any(|f| metadata_str(f, "route_template") == Some("/hello"))
    );
    assert!(
        facts(&results, "http.client_request.v1")
            .iter()
            .any(|f| metadata_str(f, "target_path") == Some("https://api.example.com/users"))
    );
}

#[test]
fn script_imports_and_directives_are_imports() {
    let code = "#r \"nuget: FSharp.Data, 6.4.0\"\n#load \"helpers.fsx\"\n\nopen System.IO\n\nlet readAll path = File.ReadAllText path\n";
    let results = extract_at("scripts/build.fsx", code);
    for name in ["FSharp.Data", "helpers.fsx", "IO"] {
        symbol(&results, name, SymbolKind::Import);
    }
    let imports: Vec<_> = results
        .structured_pending_relationships
        .iter()
        .filter(|p| p.pending.kind == RelationshipKind::Imports)
        .map(|p| p.target.display_name.clone())
        .collect();
    assert!(imports.contains(&"System.IO".to_string()), "{imports:?}");
    assert!(imports.contains(&"helpers.fsx".to_string()), "{imports:?}");
}

#[test]
fn domain_native_constructs_are_structural_facts() {
    let code = "module Dom\nlet (|Expensive|Cheap|) (price: decimal) = if price > 100m then Expensive else Cheap\nlet (|Positive|_|) n = if n > 0 then Some n else None\nlet load () = task { return 1 }\nlet q = <@ 1 + 1 @>\n";
    let results = extract(code);
    let builders: Vec<_> = facts(&results, "fsharp.computation_expression.v1")
        .into_iter()
        .filter_map(|f| metadata_str(f, "builder"))
        .collect();
    assert_eq!(builders, vec!["task"]);
    let partial: Vec<_> = facts(&results, "fsharp.active_pattern.v1")
        .into_iter()
        .map(|f| {
            f.metadata
                .as_ref()
                .and_then(|m| m.get("partial"))
                .and_then(|v| v.as_bool())
        })
        .collect();
    assert_eq!(partial, vec![Some(false), Some(true)]);
    assert_eq!(facts(&results, "fsharp.quotation.v1").len(), 1);
}
