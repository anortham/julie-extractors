use crate::ExtractionResults;
use crate::base::{IdentifierKind, SourceRegionKind, Symbol, SymbolKind, Visibility};
use crate::extract_canonical;
use std::path::Path;

fn extract_at(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test"))
        .expect("canonical Zig extraction must succeed")
}

fn extract(source: &str) -> ExtractionResults {
    extract_at("src/main.zig", source)
}

fn find<'a>(result: &'a ExtractionResults, name: &str) -> &'a Symbol {
    result
        .symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap_or_else(|| panic!("{name} missing: {:#?}", names(result)))
}

fn names(result: &ExtractionResults) -> Vec<String> {
    result
        .symbols
        .iter()
        .map(|s| format!("{}:{}@{}", s.name, s.kind, s.start_line))
        .collect()
}

fn parent_name(result: &ExtractionResults, symbol: &Symbol) -> Option<String> {
    let parent_id = symbol.parent_id.as_deref()?;
    result
        .symbols
        .iter()
        .find(|candidate| candidate.id == parent_id)
        .map(|parent| parent.name.clone())
}

fn metadata_flag(symbol: &Symbol, key: &str) -> bool {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

fn body_text<'a>(source: &'a str, symbol: &Symbol) -> Option<&'a str> {
    let span = symbol.body_span?;
    source.get(span.start_byte as usize..span.end_byte as usize)
}

fn type_of(result: &ExtractionResults, symbol: &Symbol) -> Option<String> {
    result
        .types
        .get(&symbol.id)
        .map(|type_info| type_info.resolved_type.clone())
}

fn symbol_label(result: &ExtractionResults, id: &str) -> String {
    result
        .symbols
        .iter()
        .find(|symbol| symbol.id == id)
        .map(|symbol| format!("{}@{}", symbol.name, symbol.start_line))
        .unwrap_or_default()
}

fn relationship_rows(result: &ExtractionResults) -> Vec<String> {
    result
        .relationships
        .iter()
        .map(|rel| {
            format!(
                "{} {}->{}",
                rel.kind,
                symbol_label(result, &rel.from_symbol_id),
                symbol_label(result, &rel.to_symbol_id)
            )
        })
        .collect()
}

fn annotation_keys(symbol: &Symbol) -> Vec<String> {
    symbol
        .annotations
        .iter()
        .map(|marker| marker.annotation.clone())
        .collect()
}

#[test]
fn declaration_kind_follows_the_initializer_node() {
    let result = extract(
        r#"fn hash_fn(key: u32) u32 { return key; }
pub const default_hash = hash_fn(7);
pub const Parser = struct {
    input: []const u8,
    pub fn parseAs(comptime T: type, text: []const u8) !T { _ = text; return undefined; }
};
pub const Tracker = struct {
    count: u32,
    pub fn isDebug() bool { return @import("builtin").mode == .Debug; }
};
const default_handler: *const fn () void = &noop;
const Handler = fn (u32) void;
fn noop() void {}
"#,
    );

    assert_eq!(find(&result, "default_hash").kind, SymbolKind::Constant);
    let parser = find(&result, "Parser");
    assert_eq!(parser.kind, SymbolKind::Struct);
    assert_eq!(parser.signature.as_deref(), Some("const Parser = struct"));
    assert_eq!(
        parent_name(&result, find(&result, "parseAs")).as_deref(),
        Some("Parser")
    );
    assert_eq!(find(&result, "parseAs").kind, SymbolKind::Method);
    assert_eq!(find(&result, "Tracker").kind, SymbolKind::Struct);
    let handler_value = find(&result, "default_handler");
    assert_eq!(handler_value.kind, SymbolKind::Constant);
    assert!(!metadata_flag(handler_value, "isFunctionType"));
    let handler_type = find(&result, "Handler");
    assert_eq!(handler_type.kind, SymbolKind::Type);
    assert!(metadata_flag(handler_type, "isFunctionType"));
}

#[test]
fn enum_tags_and_error_names_are_enum_members() {
    let result = extract(
        r#"pub const Tag = enum {
    /// An identifier.
    identifier,
    eof,
};
pub const Level = enum(u8) { debug = 0, info = 1 };
pub const ParseError = error{
    UnexpectedEof,
    InvalidToken,
};
pub const Value = union(enum) { int: i64, text: []const u8 };
fn parse() error{Oops}!void {}
"#,
    );

    let identifier = find(&result, "identifier");
    assert_eq!(identifier.kind, SymbolKind::EnumMember);
    assert_eq!(parent_name(&result, identifier).as_deref(), Some("Tag"));
    assert_eq!(
        identifier.doc_comment.as_deref(),
        Some("/// An identifier.")
    );
    assert_eq!(find(&result, "eof").kind, SymbolKind::EnumMember);
    assert_eq!(
        find(&result, "debug").signature.as_deref(),
        Some("debug = 0")
    );
    assert_eq!(
        find(&result, "Level").signature.as_deref(),
        Some("const Level = enum(u8)")
    );
    let unexpected = find(&result, "UnexpectedEof");
    assert_eq!(unexpected.kind, SymbolKind::EnumMember);
    assert_eq!(
        parent_name(&result, unexpected).as_deref(),
        Some("ParseError")
    );
    assert_eq!(find(&result, "InvalidToken").kind, SymbolKind::EnumMember);
    assert_eq!(find(&result, "int").kind, SymbolKind::Field);
    assert!(result.symbols.iter().all(|symbol| symbol.name != "Oops"));
}

#[test]
fn this_aliases_name_the_enclosing_container() {
    let source = r#"pub const Buffer = struct {
    const Self = @This();
    len: usize,
    next: ?*Self,
    pub fn reset(self: *Self) void { self.len = 0; }
    pub fn clear(self: *Self) void { self.reset(); }
};
"#;
    let result = extract(source);
    let self_params: Vec<&Symbol> = result
        .symbols
        .iter()
        .filter(|symbol| symbol.name == "self")
        .collect();

    assert_eq!(self_params.len(), 2);
    for param in self_params {
        assert_eq!(type_of(&result, param).as_deref(), Some("Buffer"));
    }
    assert_eq!(
        type_of(&result, find(&result, "next")).as_deref(),
        Some("Buffer")
    );
    let reset_call = result
        .identifiers
        .iter()
        .find(|identifier| identifier.name == "reset" && identifier.kind == IdentifierKind::Call)
        .expect("self.reset() call identifier");
    assert_eq!(reset_call.receiver_type.as_deref(), Some("Buffer"));
}

#[test]
fn file_struct_self_calls_resolve_to_top_level_functions() {
    let result = extract_at(
        "src/Tokenizer.zig",
        r#"const Tokenizer = @This();
buffer: []const u8,
index: usize,
pub fn next(self: *Tokenizer) ?u8 { return self.peek(); }
fn peek(self: *const Tokenizer) u8 { return self.buffer[self.index]; }
"#,
    );

    assert!(
        relationship_rows(&result).contains(&"calls next@4->peek@5".to_string()),
        "{:?}",
        relationship_rows(&result)
    );
    assert_eq!(
        type_of(
            &result,
            result.symbols.iter().find(|s| s.name == "self").unwrap()
        )
        .as_deref(),
        Some("Tokenizer")
    );
}

#[test]
fn wrapped_types_and_struct_literals_are_type_usages() {
    let result = extract(
        r#"const Node = struct { v: u32 };
const Wheel = struct { r: u32 };
const Car = struct {
    maybe: ?Node,
    list: []const Node,
    fixed: [4]Node,
    wheels: [4]Wheel,
    spare: Wheel,
};
fn first(nodes: []Node) ?Node { return nodes[0]; }
var index: Map(Node, Wheel) = undefined;
const s = Node{ .v = 1 };
"#,
    );
    let type_lines: Vec<u32> = result
        .identifiers
        .iter()
        .filter(|identifier| {
            identifier.name == "Node" && identifier.kind == IdentifierKind::TypeUsage
        })
        .map(|identifier| identifier.start_line)
        .collect();

    for line in [4, 5, 6, 10, 11, 12] {
        assert!(
            type_lines.contains(&line),
            "Node@{line} not a type_usage: {type_lines:?}"
        );
    }
    assert!(
        !result
            .identifiers
            .iter()
            .any(|identifier| identifier.name == "Node"
                && identifier.kind == IdentifierKind::VariableRef)
    );
    let relationships = relationship_rows(&result);
    assert!(
        relationships.contains(&"composition Car@3->Wheel@2".to_string()),
        "{relationships:?}"
    );
    assert!(
        !relationships
            .iter()
            .any(|row| row.starts_with("composition Node")),
        "{relationships:?}"
    );
}

#[test]
fn bodies_are_grammar_nodes_or_absent() {
    let source = r#"const std = @import("std");
extern fn free(ptr: ?*anyopaque) void;
var gpa = std.heap.GeneralPurposeAllocator(.{}){};
const aligned: u32 align(8) = 0;
fn run() void {}
"#;
    let result = extract(source);

    assert_eq!(find(&result, "free").body_span, None);
    assert_eq!(
        body_text(source, find(&result, "std")),
        Some("@import(\"std\")")
    );
    assert_eq!(
        body_text(source, find(&result, "gpa")),
        Some("std.heap.GeneralPurposeAllocator(.{}){}")
    );
    assert_eq!(body_text(source, find(&result, "aligned")), Some("0"));
    assert_eq!(body_text(source, find(&result, "run")), Some("{}"));
}

#[test]
fn opaque_types_are_containers_and_empty_containers_have_no_fields() {
    let result = extract(
        r#"/// Opaque C handle.
pub const Handle = opaque {};
pub const Window = opaque {
    pub fn close(self: *Window) void { _ = self; }
};
const Empty = struct {};
"#,
    );

    let handle = find(&result, "Handle");
    assert_eq!(handle.kind, SymbolKind::Struct);
    assert_eq!(handle.signature.as_deref(), Some("const Handle = opaque"));
    assert_eq!(handle.doc_comment.as_deref(), Some("/// Opaque C handle."));
    let close = find(&result, "close");
    assert_eq!(close.kind, SymbolKind::Method);
    assert_eq!(parent_name(&result, close).as_deref(), Some("Window"));
    assert!(result.symbols.iter().all(|symbol| !symbol.name.is_empty()));
}

#[test]
fn variable_signatures_state_the_declared_type() {
    let result = extract(
        r#"var counter: u32 = 0;
/// The max size.
pub const max_size: usize = 64;
const maybe: ?u32 = null;
pub const empty: Pool = .{ .n = 0 };
const limit = 10;
const Pool = struct { n: u32 };
"#,
    );

    assert_eq!(
        find(&result, "counter").signature.as_deref(),
        Some("var counter: u32")
    );
    assert_eq!(
        find(&result, "max_size").signature.as_deref(),
        Some("pub const max_size: usize")
    );
    assert_eq!(
        find(&result, "max_size").doc_comment.as_deref(),
        Some("/// The max size.")
    );
    assert_eq!(
        find(&result, "maybe").signature.as_deref(),
        Some("const maybe: ?u32")
    );
    assert_eq!(
        find(&result, "empty").signature.as_deref(),
        Some("pub const empty: Pool")
    );
    assert_eq!(
        find(&result, "limit").signature.as_deref(),
        Some("const limit = 10")
    );
}

#[test]
fn only_test_declarations_are_tests() {
    let result = extract_at(
        "test/harness.zig",
        r#"pub fn TestHarness(comptime T: type) type { return struct { value: T }; }
pub fn test_parse() void {}
test "harness works" {}
"#,
    );

    assert!(!metadata_flag(find(&result, "TestHarness"), "is_test"));
    assert!(!metadata_flag(find(&result, "test_parse"), "is_test"));
    assert!(metadata_flag(find(&result, "harness works"), "is_test"));
}

#[test]
fn ffi_modifiers_are_annotations() {
    let result = extract(
        r#"extern fn free(ptr: ?*anyopaque) void;
export var exported: u32 = 0;
fn callconvFn() callconv(.C) void {}
noinline fn slow() void {}
pub const Slice = if (true) ([]align(4) u8) else []u8;
const aligned: u32 align(8) = 0;
"#,
    );

    let free = find(&result, "free");
    assert_eq!(annotation_keys(free), vec!["extern"]);
    assert_eq!(
        free.signature.as_deref(),
        Some("extern fn free(ptr: ?*anyopaque) void")
    );
    assert_eq!(
        find(&result, "exported").visibility,
        Some(Visibility::Public)
    );
    let callconv = find(&result, "callconvFn");
    assert_eq!(annotation_keys(callconv), vec!["callconv(.C)"]);
    assert_eq!(
        callconv.signature.as_deref(),
        Some("fn callconvFn() callconv(.C) void")
    );
    let slow = find(&result, "slow");
    assert_eq!(annotation_keys(slow), vec!["noinline"]);
    assert_eq!(slow.signature.as_deref(), Some("noinline fn slow() void"));
    assert!(annotation_keys(find(&result, "Slice")).is_empty());
    assert_eq!(annotation_keys(find(&result, "aligned")), vec!["align(8)"]);
}

#[test]
fn container_doc_comments_document_their_container() {
    let result = extract(
        r#"//! Module for geometry helpers.
const std = @import("std");
pub const Shape = struct {
    //! Shape container doc.
    width: u32,
};
"#,
    );

    assert_eq!(find(&result, "std").doc_comment, None);
    assert_eq!(
        find(&result, "Shape").doc_comment.as_deref(),
        Some("//! Shape container doc.")
    );
    let doc_regions = result
        .source_regions
        .iter()
        .filter(|region| region.kind == SourceRegionKind::DocComment)
        .count();
    assert_eq!(doc_regions, 2);
}

#[test]
fn builtins_passed_as_call_arguments_are_builtin_calls() {
    let result = extract(
        r#"fn use(a: anytype, b: anytype) void { _ = a; _ = b; }
fn demo(n: u64) void {
    use(@intCast(n), @embedFile("data.txt"));
}
"#,
    );
    let builtins: Vec<String> = result
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == "zig.builtin_call.v1" && fact.start_line == 3)
        .filter_map(|fact| {
            fact.metadata
                .as_ref()?
                .get("builtin_name")
                .and_then(|value| value.as_str())
        })
        .map(str::to_string)
        .collect();

    assert!(builtins.contains(&"intCast".to_string()), "{builtins:?}");
    assert!(builtins.contains(&"embedFile".to_string()), "{builtins:?}");
}

#[test]
fn object_less_member_literals_suppress_the_receiver() {
    let result = extract(
        r#"const Level = enum { debug, info };
fn pick(debug: bool) Level {
    return if (debug) .debug else .info;
}
"#,
    );
    let info = result
        .identifiers
        .iter()
        .find(|identifier| {
            identifier.name == "info" && identifier.kind == IdentifierKind::MemberAccess
        })
        .expect("enum literal member access");

    assert_eq!(
        info.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("receiver")),
        Some(&serde_json::Value::Null)
    );
}

fn fact_rows(result: &ExtractionResults, keys: &[&str]) -> Vec<String> {
    result
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id != "zig.builtin_call.v1")
        .map(|fact| {
            let metadata = fact.metadata.clone().unwrap_or_default();
            let values: Vec<String> = keys
                .iter()
                .filter_map(|key| metadata.get(*key).and_then(|value| value.as_str()))
                .map(str::to_string)
                .collect();
            format!(
                "{}@{} {}",
                fact.pattern_id,
                fact.start_line,
                values.join(" ")
            )
        })
        .collect()
}

#[test]
fn build_script_publishes_the_build_graph() {
    let result = extract_at(
        "build.zig",
        r#"const std = @import("std");
pub fn build(b: *std.Build) void {
    const httpz = b.dependency("httpz", .{});
    const exe = b.addExecutable(.{ .name = "app", .root_source_file = b.path("src/main.zig") });
    exe.root_module.addImport("httpz", httpz.module("httpz"));
    const lib = b.addModule("core", .{ .root_source_file = b.path("src/core.zig") });
    _ = lib;
    const tests = b.addTest(.{ .root_module = b.createModule(.{ .root_source_file = b.path("src/tests.zig") }) });
    _ = tests;
    const run_step = b.step("run", "Run the app");
    _ = run_step;
}
"#,
    );
    let rows = fact_rows(
        &result,
        &[
            "artifact_kind",
            "artifact_name",
            "root_source_file",
            "dependency_name",
            "module_name",
            "import_name",
            "step_name",
            "step_description",
        ],
    );

    for expected in [
        "zig.build_dependency.v1@3 httpz",
        "zig.build_artifact.v1@4 executable app src/main.zig",
        "zig.build_module_import.v1@5 httpz",
        "zig.build_module.v1@6 src/core.zig core",
        "zig.build_artifact.v1@8 test src/tests.zig",
        "zig.build_step.v1@10 run Run the app",
    ] {
        assert!(
            rows.contains(&expected.to_string()),
            "{expected} missing: {rows:#?}"
        );
    }
}

#[test]
fn build_calls_outside_build_zig_publish_nothing() {
    let result = extract(
        r#"fn helper(b: anytype) void {
    _ = b.step("run", "Run the app");
}
"#,
    );

    assert!(
        fact_rows(&result, &[]).is_empty(),
        "{:?}",
        fact_rows(&result, &[])
    );
}

#[test]
fn httpz_routes_on_traced_routers() {
    let result = extract(
        r#"const httpz = @import("httpz");
fn serve(server: *httpz.Server(void)) !void {
    var router = try server.router(.{});
    router.get("/api/users/:id", getUser, .{});
    var admin = router.group("/admin", .{});
    admin.post("/users", createUser, .{});
    cache.get("/not/a/route", getUser, .{});
}
fn getUser(req: *httpz.Request, res: *httpz.Response) !void { _ = req; _ = res; }
fn createUser(req: *httpz.Request, res: *httpz.Response) !void { _ = req; _ = res; }
"#,
    );
    let rows = fact_rows(
        &result,
        &["verb", "normalized_route_template", "handler_name"],
    );

    assert_eq!(
        rows,
        vec![
            "httpz.route.v1@4 GET /api/users/:id getUser",
            "httpz.route.v1@6 POST /admin/users createUser",
        ]
    );
}

#[test]
fn std_http_client_requests_with_static_urls() {
    let result = extract(
        r#"const std = @import("std");
fn call(allocator: std.mem.Allocator) !void {
    var client = std.http.Client{ .allocator = allocator };
    defer client.deinit();
    _ = try client.fetch(.{ .location = .{ .url = "https://api.example.com/health" }, .method = .GET });
    _ = try client.fetch(.{ .location = .{ .url = "https://api.example.com/items" }, .payload = "{}" });
    const uri = try std.Uri.parse("https://api.example.com/items/1");
    var buf: [1024]u8 = undefined;
    var req = try client.open(.DELETE, uri, .{ .server_header_buffer = &buf });
    defer req.deinit();
}
"#,
    );
    let rows = fact_rows(&result, &["verb", "target_path", "verb_source"]);

    assert_eq!(
        rows,
        vec![
            "http.client_request.v1@5 GET https://api.example.com/health attested",
            "http.client_request.v1@6 POST https://api.example.com/items default",
            "http.client_request.v1@9 DELETE https://api.example.com/items/1 attested",
        ]
    );
}
