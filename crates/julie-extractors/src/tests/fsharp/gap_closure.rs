use crate::ExtractionResults;
use crate::base::{IdentifierKind, RelationshipKind, Symbol, SymbolKind, Visibility};
use crate::pipeline::extract_canonical;
use std::path::Path;

fn extract(source: &str) -> ExtractionResults {
    extract_canonical("src/Gaps.fs", source, Path::new("/workspace")).expect("extract")
}

fn symbol<'a>(results: &'a ExtractionResults, name: &str, kind: SymbolKind) -> &'a Symbol {
    results
        .symbols
        .iter()
        .find(|s| s.name == name && s.kind == kind)
        .unwrap_or_else(|| panic!("missing {kind:?} {name}: {:#?}", names(results)))
}

fn names(results: &ExtractionResults) -> Vec<(String, SymbolKind)> {
    results
        .symbols
        .iter()
        .map(|s| (s.name.clone(), s.kind.clone()))
        .collect()
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

/// `(caller, target)` for every resolved call and every pending call.
fn calls(results: &ExtractionResults) -> Vec<(String, String)> {
    let mut rows: Vec<_> = results
        .relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::Calls)
        .map(|r| {
            (
                name_of(results, &r.from_symbol_id),
                name_of(results, &r.to_symbol_id),
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
                    p.target.display_name.clone(),
                )
            }),
    );
    rows
}

fn identifiers(results: &ExtractionResults, kind: IdentifierKind) -> Vec<&str> {
    results
        .identifiers
        .iter()
        .filter(|i| i.kind == kind)
        .map(|i| i.name.as_str())
        .collect()
}

#[test]
fn pipeline_and_infix_calls_emit_call_edges() {
    let code = r#"module M
let helper x = x
let parse (s: string) = int s
let validate x = x > 0
let a () = helper <| 1
let b () = 1 |> helper
let c text = text |> parse |> validate
let d text = validate <| parse text
let e y = 1 + helper y
let f xs = xs |> List.map helper
let rec fact n = if n <= 1 then 1 else n * fact (n - 1)
"#;
    let results = extract(code);
    let rows = calls(&results);
    for expected in [
        ("a", "helper"),
        ("b", "helper"),
        ("c", "parse"),
        ("c", "validate"),
        ("d", "validate"),
        ("d", "parse"),
        ("e", "helper"),
        ("f", "List.map"),
        ("fact", "fact"),
    ] {
        let expected = (expected.0.to_string(), expected.1.to_string());
        assert!(rows.contains(&expected), "{expected:?} in {rows:?}");
    }
    let reads = identifiers(&results, IdentifierKind::VariableRef);
    assert!(reads.contains(&"text"), "{reads:?}");
    assert!(reads.contains(&"xs"), "{reads:?}");
}

#[test]
fn member_and_type_bodies_cover_the_definition_body() {
    let code = r#"module M
type Greeter(name: string) =
    member _.Greet(prefix) =
        let text = prefix + name
        printfn "%s" text
    member this.Loud = name.ToUpper()
    abstract Speak: unit -> string
"#;
    let results = extract(code);
    let greet = symbol(&results, "Greet", SymbolKind::Method);
    assert_eq!(
        body_text(code, greet),
        Some("let text = prefix + name\n        printfn \"%s\" text")
    );
    assert_eq!(
        body_text(code, symbol(&results, "Loud", SymbolKind::Property)),
        Some("name.ToUpper()")
    );
    let class_body = body_text(code, symbol(&results, "Greeter", SymbolKind::Class)).unwrap();
    assert!(class_body.starts_with("member _.Greet"), "{class_body:?}");
    let edited = extract(&code.replace("printfn \"%s\" text", "failwith text"));
    assert_ne!(
        greet.body_hash,
        symbol(&edited, "Greet", SymbolKind::Method).body_hash
    );
}

#[test]
fn match_expressions_keep_reads_and_union_case_references() {
    let code = r#"module M
type Payment =
    | Cash of amount: decimal
    | Card of number: string
let fee = 2m
let total (p: Payment) =
    match p with
    | Cash amount -> amount + fee
    | Card _ -> fee
"#;
    let results = extract(code);
    let total = symbol(&results, "total", SymbolKind::Function);
    let reads: Vec<&str> = results
        .identifiers
        .iter()
        .filter(|i| {
            i.kind == IdentifierKind::VariableRef
                && i.containing_symbol_id.as_deref() == Some(total.id.as_str())
        })
        .map(|i| i.name.as_str())
        .collect();
    for name in ["p", "amount", "fee", "Cash", "Card"] {
        assert!(reads.contains(&name), "{name} in {reads:?}");
    }
    assert_eq!(reads.iter().filter(|name| **name == "fee").count(), 2);
}

#[test]
fn let_parameter_annotations_emit_type_usages() {
    let code = r#"module M
type Order = { Id: int }
let a (xs: Order list) = xs
let b (x: Order option) = x
let e (x: Order[]) = x
"#;
    let results = extract(code);
    let orders = results
        .identifiers
        .iter()
        .filter(|i| i.kind == IdentifierKind::TypeUsage && i.name == "Order")
        .count();
    assert_eq!(orders, 3);
}

#[test]
fn unnamed_fields_are_not_symbols_and_exceptions_are_types() {
    let code = r#"module M
exception NotFound of string
exception Conflict of id: int * reason: string
type Account = { Iban: string }
type Payment =
    | Cash of decimal
    | Transfer of Account
let find key = raise (NotFound key)
"#;
    let results = extract(code);
    let fields: Vec<&str> = results
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Field)
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(fields, vec!["id", "reason", "Iban"]);
    let conflict = symbol(&results, "Conflict", SymbolKind::Class);
    assert!(
        results
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Field && s.name != "Iban")
            .all(|s| s.parent_id.as_deref() == Some(conflict.id.as_str()))
    );
    symbol(&results, "NotFound", SymbolKind::Class);
    let uses: Vec<(String, SymbolKind)> = results
        .relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::Uses)
        .map(|r| {
            let target = results
                .symbols
                .iter()
                .find(|s| s.id == r.to_symbol_id)
                .unwrap();
            (target.name.clone(), target.kind.clone())
        })
        .collect();
    assert_eq!(uses, vec![("Account".to_string(), SymbolKind::Struct)]);
    assert!(
        !identifiers(&results, IdentifierKind::VariableRef).contains(&"Conflict"),
        "the exception name is a declaration"
    );
}

#[test]
fn and_groups_emit_one_symbol_per_declaration() {
    let code = r#"module M
type Payment =
    | Cash of decimal
    | Transfer of Account
and Account = { Iban: string }
and Bank() =
    member _.Name = "b"

let rec countNode n = 1 + countForest n
and countForest forest = countNode forest

let total tree = countForest tree
"#;
    let results = extract(code);
    let account = symbol(&results, "Account", SymbolKind::Struct);
    let bank = symbol(&results, "Bank", SymbolKind::Class);
    let payment = symbol(&results, "Payment", SymbolKind::Union);
    assert!(payment.end_byte <= account.start_byte);
    assert_eq!(
        symbol(&results, "Iban", SymbolKind::Field)
            .parent_id
            .as_deref(),
        Some(account.id.as_str())
    );
    assert_eq!(
        symbol(&results, "Name", SymbolKind::Property)
            .parent_id
            .as_deref(),
        Some(bank.id.as_str())
    );
    symbol(&results, "countForest", SymbolKind::Function);
    let rows = calls(&results);
    for expected in [
        ("countNode", "countForest"),
        ("countForest", "countNode"),
        ("total", "countForest"),
    ] {
        let expected = (expected.0.to_string(), expected.1.to_string());
        assert!(rows.contains(&expected), "{expected:?} in {rows:?}");
    }
}

#[test]
fn access_modifiers_on_lets_and_types_set_visibility() {
    let code = r#"module Vis
let private secretValue = 42
let private secretFn x = x + 1
let internal sharedFn x = x
let mutable private counter = 0
type private Hidden = { A: int }
type internal Shared() =
    let mutable count = 0
    member _.X = count
let open' = 1
"#;
    let results = extract(code);
    let vis = |name: &str| {
        results
            .symbols
            .iter()
            .find(|s| s.name == name)
            .and_then(|s| s.visibility.clone())
    };
    assert_eq!(vis("secretValue"), Some(Visibility::Private));
    assert_eq!(vis("secretFn"), Some(Visibility::Private));
    assert_eq!(vis("sharedFn"), Some(Visibility::Internal));
    assert_eq!(vis("counter"), Some(Visibility::Private));
    assert_eq!(vis("Hidden"), Some(Visibility::Private));
    assert_eq!(vis("Shared"), Some(Visibility::Internal));
    assert_eq!(vis("count"), Some(Visibility::Private));
    assert_eq!(vis("X"), Some(Visibility::Public));
}

#[test]
fn calls_in_values_properties_and_nested_modules_belong_to_them() {
    let code = r#"namespace Shop

type Line =
    { Qty: int; Price: decimal }
    member this.Total = decimal this.Qty * this.Price

module Handlers =
    open System
    let greeting = String.Join(", ", [ "a"; "b" ])
    let handler : int -> string =
        fun id -> Convert.ToString id
"#;
    let results = extract(code);
    let rows = calls(&results);
    for expected in [
        ("Total", "decimal"),
        ("greeting", "String.Join"),
        ("handler", "Convert.ToString"),
    ] {
        let expected = (expected.0.to_string(), expected.1.to_string());
        assert!(rows.contains(&expected), "{expected:?} in {rows:?}");
    }
    let import = results
        .structured_pending_relationships
        .iter()
        .find(|p| p.pending.kind == RelationshipKind::Imports)
        .expect("open System");
    assert_eq!(
        name_of(&results, &import.pending.from_symbol_id),
        "Handlers"
    );
}

#[test]
fn generic_calls_are_calls_with_type_arguments() {
    let code = r#"module M
let configure (services: IServiceCollection) =
    services.AddSingleton<IClock, SystemClock>() |> ignore
let parse (json: string) = JsonSerializer.Deserialize<Order>(json)
let cache () = Dictionary<string, int>()
"#;
    let results = extract(code);
    let call_names = identifiers(&results, IdentifierKind::Call);
    for name in ["AddSingleton", "Deserialize", "Dictionary"] {
        assert!(call_names.contains(&name), "{name} in {call_names:?}");
    }
    let type_usages = identifiers(&results, IdentifierKind::TypeUsage);
    for name in ["AddSingleton", "Deserialize", "Dictionary"] {
        assert!(!type_usages.contains(&name), "{name} in {type_usages:?}");
    }
    let rows = calls(&results);
    assert!(
        rows.contains(&("configure".into(), "services.AddSingleton".into())),
        "{rows:?}"
    );
    assert!(
        rows.contains(&("parse".into(), "JsonSerializer.Deserialize".into())),
        "{rows:?}"
    );
    let add_singleton = results
        .identifiers
        .iter()
        .find(|i| i.kind == IdentifierKind::Call && i.name == "AddSingleton")
        .unwrap();
    let args: Vec<&str> = results
        .type_argument_usages
        .iter()
        .filter(|u| u.identifier_id == add_singleton.id)
        .flat_map(|u| u.arguments.iter().map(|a| a.type_name.as_str()))
        .collect();
    assert_eq!(args, vec!["IClock", "SystemClock"]);
}

#[test]
fn nunit_mstest_and_expecto_tests_have_test_roles() {
    let code = r#"module Tests
open NUnit.Framework
[<TestFixture>]
type OrderTests() =
    [<SetUp>]
    member _.Init() = ()
    [<Test>]
    member _.Starts() = Assert.Pass()
    [<TestCase(1, 2)>]
    member _.Increments(input: int, expected: int) = Assert.That(input + 1, Is.EqualTo(expected))
    member _.Helper() = ()

[<TestClass>]
type MsTests() =
    [<TestMethod>]
    member _.AddsNumbers() = ()

[<Tests>]
let pricingTests = testList "pricing" []
let notATest = 1
"#;
    let results = extract(code);
    let role = |name: &str| {
        results
            .symbols
            .iter()
            .find(|s| s.name == name)
            .and_then(|s| s.metadata.as_ref())
            .and_then(|m| m.get("test_role"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    };
    assert_eq!(role("Starts").as_deref(), Some("test_case"));
    assert_eq!(role("Increments").as_deref(), Some("parameterized_test"));
    assert_eq!(role("Init").as_deref(), Some("fixture_setup"));
    assert_eq!(role("AddsNumbers").as_deref(), Some("test_case"));
    assert_eq!(role("OrderTests").as_deref(), Some("test_container"));
    assert_eq!(role("MsTests").as_deref(), Some("test_container"));
    assert_eq!(role("pricingTests").as_deref(), Some("test_case"));
    assert_eq!(role("Helper"), None);
    assert_eq!(role("notATest"), None);
}
