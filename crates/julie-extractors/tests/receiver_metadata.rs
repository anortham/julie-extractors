use std::path::Path;

use julie_extractors::{
    ExtractionLevel, Identifier, IdentifierKind, extract_canonical_for_language_at,
};

fn extract(
    language: &str,
    path: &str,
    source: &str,
    level: ExtractionLevel,
) -> julie_extractors::ExtractionResults {
    extract_canonical_for_language_at(language, path, source, Path::new("."), level).unwrap()
}

fn identifier<'a>(
    results: &'a julie_extractors::ExtractionResults,
    name: &str,
    kind: IdentifierKind,
) -> &'a Identifier {
    results
        .identifiers
        .iter()
        .find(|identifier| identifier.name == name && identifier.kind == kind)
        .unwrap()
}

fn identifier_named<'a>(
    results: &'a julie_extractors::ExtractionResults,
    name: &str,
) -> &'a Identifier {
    results
        .identifiers
        .iter()
        .find(|identifier| identifier.name == name)
        .unwrap_or_else(|| panic!("no identifier named {name:?}"))
}

fn assert_receiver(identifier: &Identifier, receiver: &str, qualifier: Option<&str>) {
    let metadata = identifier
        .metadata
        .as_ref()
        .unwrap_or_else(|| panic!("identifier {:?} has no receiver metadata", identifier.name));
    assert_eq!(
        metadata.get("receiver").and_then(serde_json::Value::as_str),
        Some(receiver)
    );
    assert_eq!(
        metadata
            .get("receiver_qualifier")
            .and_then(serde_json::Value::as_str),
        qualifier
    );
}

#[test]
fn canonical_rust_receiver_uses_unicode_and_named_chain_syntax() {
    let source = "fn run() { first /* bridge */\n    . second.λ(); }";
    let results = extract("rust", "sample.rs", source, ExtractionLevel::Full);
    let call = identifier(&results, "λ", IdentifierKind::Call);
    let metadata = call.metadata.as_ref().unwrap();

    assert_eq!(
        metadata.get("receiver").and_then(serde_json::Value::as_str),
        Some("second")
    );
    assert_eq!(
        metadata
            .get("receiver_qualifier")
            .and_then(serde_json::Value::as_str),
        Some("first")
    );
}

#[test]
fn canonical_javascript_receivers_follow_named_members_through_parentheses() {
    let source =
        "function run() { Root.Service.send(); ( /* before */ client /* after */ ).flush(); }";
    let results = extract("javascript", "sample.js", source, ExtractionLevel::Full);
    let send = identifier(&results, "send", IdentifierKind::Call);
    let flush = identifier(&results, "flush", IdentifierKind::Call);

    assert_eq!(
        send.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("receiver"))
            .and_then(serde_json::Value::as_str),
        Some("Service")
    );
    assert_eq!(
        send.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("receiver_qualifier"))
            .and_then(serde_json::Value::as_str),
        Some("Root")
    );
    assert_eq!(
        flush
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("receiver"))
            .and_then(serde_json::Value::as_str),
        Some("client")
    );
}

#[test]
fn canonical_member_receivers_omit_call_and_index_expressions() {
    let source = "function run() { factory().service.send(); items[index].send(); }";
    let results = extract("javascript", "sample.js", source, ExtractionLevel::Full);
    let send_calls: Vec<_> = results
        .identifiers
        .iter()
        .filter(|identifier| identifier.name == "send" && identifier.kind == IdentifierKind::Call)
        .collect();

    assert_eq!(send_calls.len(), 2);
    assert!(send_calls.iter().all(|identifier| {
        identifier.metadata.as_ref().is_none_or(|metadata| {
            !metadata.contains_key("receiver") && !metadata.contains_key("receiver_qualifier")
        })
    }));
}

#[test]
fn canonical_lua_receiver_metadata_remains_authoritative() {
    let source = "local first = {}; first.second:run()";
    let results = extract("lua", "sample.lua", source, ExtractionLevel::Full);
    let call = identifier(&results, "run", IdentifierKind::Call);
    let metadata = call.metadata.as_ref().unwrap();

    assert_eq!(
        metadata.get("receiver").and_then(serde_json::Value::as_str),
        Some("second")
    );
    assert_eq!(
        metadata
            .get("receiver_qualifier")
            .and_then(serde_json::Value::as_str),
        Some("first")
    );
}

#[test]
fn canonical_zig_null_receiver_suppresses_inference() {
    let source =
        "const Result = struct { value: u8, };\nfn makeResult() Result { return .{ .value = 1 }; }";
    let results = extract("zig", "sample.zig", source, ExtractionLevel::Full);
    let value = identifier(&results, "value", IdentifierKind::MemberAccess);

    assert!(
        value
            .metadata
            .as_ref()
            .is_some_and(|metadata| metadata.get("receiver") == Some(&serde_json::Value::Null))
    );
}

#[test]
fn canonical_gdscript_receiver_uses_the_receiver_of_an_attribute_call() {
    let source = "func run():\n    self.persist()\n";
    let results = extract("gdscript", "sample.gd", source, ExtractionLevel::Full);
    let call = identifier(&results, "persist", IdentifierKind::Call);

    assert_eq!(
        call.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("receiver"))
            .and_then(serde_json::Value::as_str),
        Some("self")
    );
}

#[test]
fn canonical_sql_member_receiver_uses_the_field_value() {
    let source = "SELECT [w].id FROM workers AS [w];";
    let results = extract("sql", "sample.sql", source, ExtractionLevel::Full);
    let field = identifier(&results, "id", IdentifierKind::MemberAccess);

    assert_eq!(
        field
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("receiver"))
            .and_then(serde_json::Value::as_str),
        Some("w")
    );
}

#[test]
fn canonical_fsharp_receiver_uses_this_for_dot_calls() {
    let source = "let run (this: Calculator) = this.Helper()";
    let results = extract("fsharp", "sample.fs", source, ExtractionLevel::Full);
    let call = identifier(&results, "Helper", IdentifierKind::Call);

    assert_eq!(
        call.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("receiver"))
            .and_then(serde_json::Value::as_str),
        Some("this")
    );
}

#[test]
fn canonical_rust_static_receivers_unwrap_type_arguments() {
    let source = "fn run() { std::Vec::<i32>::new(); }";
    let results = extract("rust", "sample.rs", source, ExtractionLevel::Full);
    let call = identifier(&results, "new", IdentifierKind::Call);

    assert_eq!(
        call.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("receiver"))
            .and_then(serde_json::Value::as_str),
        Some("Vec")
    );
    assert_eq!(
        call.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("receiver_qualifier"))
            .and_then(serde_json::Value::as_str),
        Some("std")
    );
}

#[test]
fn canonical_cpp_qualified_and_template_receivers_keep_the_named_chain() {
    let source = "namespace remote { namespace ns { void run() {} } } struct Item {}; struct Box { static void make() {} }; struct Object { template<class T> void send() {} }; void f() { remote::ns::run(); Box<Item>::make(); Object object; object.send<Item>(); }";
    let results = extract("cpp", "sample.cpp", source, ExtractionLevel::Full);
    let run = identifier(&results, "run", IdentifierKind::Call);
    let make = identifier(&results, "make", IdentifierKind::Call);
    let send = identifier(&results, "send", IdentifierKind::Call);

    assert_eq!(
        run.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("receiver"))
            .and_then(serde_json::Value::as_str),
        Some("ns")
    );
    assert_eq!(
        run.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("receiver_qualifier"))
            .and_then(serde_json::Value::as_str),
        Some("remote")
    );
    assert_eq!(
        make.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("receiver"))
            .and_then(serde_json::Value::as_str),
        Some("Box")
    );
    assert_eq!(
        send.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("receiver"))
            .and_then(serde_json::Value::as_str),
        Some("object")
    );
}

#[test]
fn canonical_dart_cascade_calls_use_the_cascade_receiver() {
    let source = "void run() { service..run(); service?..run(); }";
    let results = extract("dart", "sample.dart", source, ExtractionLevel::Full);
    let calls: Vec<_> = results
        .identifiers
        .iter()
        .filter(|identifier| identifier.name == "run" && identifier.kind == IdentifierKind::Call)
        .collect();

    assert_eq!(calls.len(), 2);
    assert!(calls.iter().all(|call| {
        call.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("receiver"))
            .and_then(serde_json::Value::as_str)
            == Some("service")
    }));
}

#[test]
fn canonical_fsharp_flattened_member_receiver_keeps_the_rightmost_prefix() {
    let source = "let run point = System.Console.WriteLine(point)";
    let results = extract("fsharp", "sample.fs", source, ExtractionLevel::Full);
    let call = identifier(&results, "WriteLine", IdentifierKind::Call);

    assert_eq!(
        call.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("receiver"))
            .and_then(serde_json::Value::as_str),
        Some("Console")
    );
    assert_eq!(
        call.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("receiver_qualifier"))
            .and_then(serde_json::Value::as_str),
        Some("System")
    );
}

#[test]
fn canonical_php_static_member_receiver_uses_its_class_name() {
    let source = include_str!("../../../fixtures/extraction/php/basic/source.php");
    let results = extract("php", "sample.php", source, ExtractionLevel::Full);
    let member = identifier(&results, "TARGET_CLASS", IdentifierKind::MemberAccess);

    assert_eq!(
        member
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("receiver"))
            .and_then(serde_json::Value::as_str),
        Some("Attribute")
    );
}

#[test]
fn canonical_vbnet_conditional_call_keeps_its_receiver() {
    let source = "Class Widget\n    Public Function GetTotal() As Integer\n        Return 1\n    End Function\n    Public Sub Run()\n        Dim o = New Widget()\n        Dim total = o?.GetTotal()\n    End Sub\nEnd Class";
    let results = extract("vbnet", "sample.vb", source, ExtractionLevel::Full);
    let call = identifier(&results, "GetTotal", IdentifierKind::Call);

    assert_eq!(
        call.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("receiver"))
            .and_then(serde_json::Value::as_str),
        Some("o")
    );
}

#[test]
fn canonical_zig_prefixed_member_call_keeps_its_receiver() {
    let source = "const Pool = struct { fn isOpen(self: *Pool) bool { return true; } fn run(self: *Pool) bool { return !self.isOpen(); } fn parenthesized(self: *Pool) bool { return (!self).isOpen(); } };";
    let results = extract("zig", "sample.zig", source, ExtractionLevel::Full);
    let calls: Vec<_> = results
        .identifiers
        .iter()
        .filter(|identifier| identifier.name == "isOpen" && identifier.kind == IdentifierKind::Call)
        .collect();

    assert_eq!(calls.len(), 2);
    assert!(calls.iter().any(|call| {
        call.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("receiver"))
            .and_then(serde_json::Value::as_str)
            == Some("self")
    }));
    assert!(calls.iter().any(|call| {
        call.metadata.as_ref().is_none_or(|metadata| {
            !metadata.contains_key("receiver") && !metadata.contains_key("receiver_qualifier")
        })
    }));
}

#[test]
fn canonical_csharp_receivers_follow_conditionals_generics_and_type_names() {
    let source = "class Base { public void Save() {} } class S : Base { public S Inner => this; void Call<T>() {} void Run(S logger) { base.Save(); logger?.Inner.Flush(); logger.Call<int>(); int.TryParse(\"1\", out var value); } }";
    let results = extract("csharp", "sample.cs", source, ExtractionLevel::Full);

    assert_receiver(identifier_named(&results, "Save"), "base", None);
    assert_receiver(identifier_named(&results, "Flush"), "Inner", Some("logger"));
    assert_receiver(identifier_named(&results, "Call"), "logger", None);
    assert_receiver(identifier_named(&results, "TryParse"), "int", None);
}

#[test]
fn canonical_java_method_references_keep_their_named_receiver_chain() {
    let source = include_str!("../../../fixtures/extraction/java/hierarchy_and_calls/source.java");
    let results = extract("java", "Sample.java", source, ExtractionLevel::Full);

    assert_receiver(identifier_named(&results, "handle"), "this", None);
    assert_receiver(identifier_named(&results, "getName"), "User", None);
    assert_receiver(identifier_named(&results, "println"), "out", Some("System"));
}

#[test]
fn canonical_python_pattern_member_keeps_its_qualifier() {
    let source = include_str!("../../../fixtures/extraction/python/wave2_semantics/source.py");
    let results = extract("python", "sample.py", source, ExtractionLevel::Full);

    assert_receiver(identifier_named(&results, "RED"), "Color", None);
}

#[test]
fn canonical_ruby_receivers_keep_sigils_and_no_argument_call_chains() {
    let member_source = include_str!("../../../fixtures/extraction/ruby/wave2_semantics/source.rb");
    let member_results = extract("ruby", "sample.rb", member_source, ExtractionLevel::Full);
    let chain_source =
        include_str!("../../../fixtures/extraction/ruby/backend_http_boundaries/source.rb");
    let chain_results = extract("ruby", "sample.rb", chain_source, ExtractionLevel::Full);
    let negative_source = include_str!("../../../fixtures/extraction/ruby/app_structure/source.rb");
    let negative_results = extract("ruby", "sample.rb", negative_source, ExtractionLevel::Full);

    assert_receiver(identifier_named(&member_results, "sum"), "@ledger", None);
    assert_receiver(
        identifier_named(&chain_results, "draw"),
        "routes",
        Some("Rails.application"),
    );
    assert!(
        identifier_named(&negative_results, "deliver_later")
            .metadata
            .as_ref()
            .is_none_or(|metadata| !metadata.contains_key("receiver"))
    );
    assert_receiver(identifier_named(&negative_results, "puts"), "$stderr", None);
}

#[test]
fn canonical_elixir_receivers_split_aliases_atoms_and_no_argument_chains() {
    let source = "defmodule Example do\n  def run(cart) do\n    if cart.user.admin? do\n      :ets.lookup(:carts, cart.id)\n    end\n    Jason.Encode.map(cart, [])\n  end\nend";
    let results = extract("elixir", "sample.ex", source, ExtractionLevel::Full);

    assert_receiver(identifier_named(&results, "admin?"), "user", Some("cart"));
    assert_receiver(identifier_named(&results, "lookup"), "ets", None);
    assert_receiver(identifier_named(&results, "map"), "Encode", Some("Jason"));
}

#[test]
fn canonical_dart_named_constructor_receivers_use_the_terminal_type_name() {
    let source = include_str!("../../../fixtures/extraction/dart/structure/source.dart");
    let results = extract("dart", "sample.dart", source, ExtractionLevel::Full);
    let roles_source = include_str!("../../../fixtures/extraction/dart/test_roles/source.dart");
    let roles = extract("dart", "roles.dart", roles_source, ExtractionLevel::Full);

    assert_receiver(identifier_named(&results, "fromJson"), "User", None);
    assert_receiver(identifier_named(&results, "empty"), "Cart", None);
    assert_receiver(identifier_named(&roles, "increment"), "cubit", None);
}

#[test]
fn canonical_gdscript_reads_and_node_paths_keep_named_receivers() {
    let source = include_str!("../../../fixtures/extraction/gdscript/structure/source.gd");
    let results = extract("gdscript", "sample.gd", source, ExtractionLevel::Full);
    let node_source = include_str!("../../../fixtures/extraction/gdscript/godot/source.gd");
    let node_results = extract("gdscript", "nodes.gd", node_source, ExtractionLevel::Full);

    assert_receiver(
        identifier_named(&results, "health"),
        "stats",
        Some("player"),
    );
    assert_receiver(
        identifier_named(&node_results, "area_entered"),
        "$Hurtbox",
        None,
    );
    assert_receiver(
        identifier_named(&node_results, "connect"),
        "area_entered",
        Some("$Hurtbox"),
    );
}

#[test]
fn canonical_vbnet_receivers_keep_instance_and_interface_paths() {
    let source = include_str!("../../../fixtures/extraction/vbnet/basic/source.vb");
    let results = extract("vbnet", "sample.vb", source, ExtractionLevel::Full);
    let inherited_source =
        include_str!("../../../fixtures/extraction/vbnet/framework_and_scopes/source.vb");
    let inherited = extract(
        "vbnet",
        "inherited.vb",
        inherited_source,
        ExtractionLevel::Full,
    );

    assert_receiver(identifier_named(&results, "Click"), "Button", None);
    assert_receiver(
        identifier(&results, "Run", IdentifierKind::Call),
        "Me",
        None,
    );
    assert_receiver(identifier_named(&inherited, "Reset"), "MyBase", None);
}

#[test]
fn canonical_zig_pointer_type_receivers_keep_the_named_type_path() {
    let source = include_str!("../../../fixtures/extraction/zig/web/source.zig");
    let results = extract("zig", "sample.zig", source, ExtractionLevel::Full);

    assert_receiver(identifier_named(&results, "Request"), "httpz", None);
}

#[test]
fn canonical_swift_bare_unary_navigation_and_parenthesized_unary_navigation_differ() {
    let source = "struct Money { var cents: Int; } func run(_ m: Money) { let bare = -m.cents; let parenthesized = (-m).cents }";
    let results = extract("swift", "sample.swift", source, ExtractionLevel::Full);
    let cents: Vec<_> = results
        .identifiers
        .iter()
        .filter(|identifier| {
            identifier.name == "cents" && identifier.kind == IdentifierKind::MemberAccess
        })
        .collect();

    assert_eq!(cents.len(), 2);
    assert_receiver(cents[0], "m", None);
    assert!(
        cents[1]
            .metadata
            .as_ref()
            .is_none_or(|metadata| !metadata.contains_key("receiver"))
    );
}
