use crate::ExtractionResults;
use crate::base::SymbolKind;
use crate::extract_canonical;
use std::path::Path;

fn extract(source: &str) -> ExtractionResults {
    extract_canonical("src/main.zig", source, Path::new("/tmp/test"))
        .expect("canonical Zig extraction must succeed")
}

fn symbol_name(result: &ExtractionResults, id: &str) -> String {
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
                symbol_name(result, &rel.from_symbol_id),
                symbol_name(result, &rel.to_symbol_id)
            )
        })
        .collect()
}

fn pending_rows(result: &ExtractionResults) -> Vec<String> {
    result
        .structured_pending_relationships
        .iter()
        .map(|pending| {
            format!(
                "{} {}->{} recv={:?}",
                pending.pending.kind,
                symbol_name(result, &pending.pending.from_symbol_id),
                pending.target.display_name,
                pending.target.receiver
            )
        })
        .collect()
}

fn identifier_rows(result: &ExtractionResults) -> Vec<String> {
    result
        .identifiers
        .iter()
        .map(|ident| format!("{} {}@{}", ident.kind, ident.name, ident.start_line))
        .collect()
}

fn symbol<'a>(result: &'a ExtractionResults, name: &str) -> &'a crate::base::Symbol {
    result
        .symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap_or_else(|| panic!("{name} missing"))
}

fn type_fact(result: &ExtractionResults, name: &str) -> Option<String> {
    let id = &symbol(result, name).id;
    result.types.get(id).map(|fact| fact.resolved_type.clone())
}

fn assert_contains(rows: &[String], expected: &str) {
    assert!(
        rows.iter().any(|row| row == expected),
        "missing {expected:?} in {rows:#?}"
    );
}

fn assert_absent(rows: &[String], unexpected: &str) {
    assert!(
        !rows.iter().any(|row| row == unexpected),
        "unexpected {unexpected:?} in {rows:#?}"
    );
}

#[test]
fn method_and_qualified_calls_use_the_callee_not_the_first_argument() {
    let result = extract(
        r#"const std = @import("std");
const Counter = struct {
    total: u32,
    fn add(self: *Counter, amount: u32) void { self.total += amount; }
    fn addTwice(self: *Counter, amount: u32) void { self.add(amount); }
};
fn run(counter: *Counter, value: u32, dest: []u8, src: []const u8) void {
    counter.addTwice(value);
    std.mem.copyForwards(u8, dest, src);
}
fn handler() void {}
fn start(router: *Router) void { router.get("/users", handler); }
"#,
    );

    let rels = relationship_rows(&result);
    assert_contains(&rels, "calls addTwice@5->add@4");
    assert_absent(&rels, "calls start@12->handler@11");
    let pending = pending_rows(&result);
    assert_contains(
        &pending,
        "calls run@7->counter.addTwice recv=Some(\"counter\")",
    );
    assert_contains(
        &pending,
        "calls run@7->std.mem.copyForwards recv=Some(\"mem\")",
    );
    assert!(
        !pending.iter().any(|row| row.contains("->amount")
            || row.contains("->value")
            || row.contains("->dest")),
        "arguments are not callees: {pending:#?}"
    );
    let idents = identifier_rows(&result);
    assert_contains(&idents, "call add@5");
    assert_contains(&idents, "call addTwice@8");
    assert_contains(&idents, "call copyForwards@9");
    assert_contains(&idents, "variable_ref value@8");
    assert_contains(&idents, "variable_ref handler@12");
    assert_absent(&idents, "call value@8");
}

#[test]
fn assignments_and_discards_are_not_variable_symbols() {
    let result = extract(
        r#"var global_count: u32 = 0;
fn bump(limit: u32) u32 {
    var count: u32 = 0;
    count = count + 1;
    count += limit;
    global_count = count;
    _ = limit;
    const q, const r = .{ limit / 3, limit % 3 };
    return count + q + r;
}
"#,
    );

    let variables: Vec<String> = result
        .symbols
        .iter()
        .filter(|symbol| matches!(symbol.kind, SymbolKind::Variable | SymbolKind::Constant))
        .map(|symbol| format!("{}@{}", symbol.name, symbol.start_line))
        .collect();
    assert_eq!(
        variables,
        ["global_count@1", "limit@2", "count@3", "q@8", "r@8"]
    );
    assert_absent(&identifier_rows(&result), "variable_ref r@8");
}

#[test]
fn negated_calls_keep_their_call_edges() {
    let result = extract(
        r#"const std = @import("std");
const Conn = struct {
    open: bool,
    fn isOpen(self: *Conn) bool { return self.open; }
    pub fn send(self: *Conn, a: []const u8, b: []const u8) !void {
        if (!self.isOpen()) return error.Closed;
        if (!std.mem.eql(u8, a, b)) return error.Mismatch;
        if (!ready()) return;
        const ok = true;
        if (!ok) return;
    }
};
fn ready() bool { return true; }
"#,
    );

    let rels = relationship_rows(&result);
    assert_contains(&rels, "calls send@5->isOpen@4");
    assert_contains(&rels, "calls send@5->ready@13");
    assert_contains(
        &pending_rows(&result),
        "calls send@5->std.mem.eql recv=Some(\"mem\")",
    );
    let idents = identifier_rows(&result);
    assert_absent(&idents, "type_usage self@6");
    assert_absent(&idents, "type_usage ready@8");
    assert_absent(&idents, "type_usage ok@10");
    assert_contains(&idents, "variable_ref ok@10");
}

#[test]
fn calls_on_call_results_and_decl_literals_stay_pending() {
    let result = extract(
        r#"const std = @import("std");
const Pool = struct {
    fn create() Pool { return .{}; }
    fn init(n: u32) Pool { _ = n; return .{}; }
    fn finish(self: Pool) void { _ = self; }
};
fn main() !void {
    const file = try std.fs.cwd().openFile("data.txt", .{});
    Pool.create().finish();
    const p: Pool = .init(3);
    _ = file;
    _ = p;
}
"#,
    );

    let pending = pending_rows(&result);
    assert_contains(
        &pending,
        "calls main@7->std.fs.cwd().openFile recv=Some(\"std.fs.cwd()\")",
    );
    assert_contains(
        &pending,
        "calls main@7->Pool.create().finish recv=Some(\"Pool.create()\")",
    );
    assert_contains(&pending, "calls main@7->Pool.init recv=Some(\"Pool\")");
}

#[test]
fn tests_named_after_a_function_do_not_block_call_resolution() {
    let result = extract(
        r#"const std = @import("std");
pub fn square(x: i32) i32 { return x * x; }
pub fn sumSquares(a: i32, b: i32) i32 { return square(a) + square(b); }
test square {
    try std.testing.expectEqual(@as(i32, 9), square(3));
}
test "sumSquares" {
    try std.testing.expectEqual(@as(i32, 25), sumSquares(3, 4));
}
"#,
    );

    let rels = relationship_rows(&result);
    assert_contains(&rels, "calls sumSquares@3->square@2");
    assert_contains(&rels, "calls square@4->square@2");
    assert_contains(&rels, "calls sumSquares@7->sumSquares@3");
}

#[test]
fn function_signatures_and_type_facts_use_the_declared_return_type() {
    let result = extract(
        r#"const std = @import("std");
const Point = struct {
    x: i32,
    pub fn init(x: i32) Point { return .{ .x = x }; }
};
fn makePoint() Point { return Point.init(1); }
fn maybePoint() ?*Point { return null; }
fn load(alloc: std.mem.Allocator) !std.ArrayList(u8) { return std.ArrayList(u8).init(alloc); }
fn count() usize { return 0; }
fn ranges(x: u8) u8 { return switch (x) { 1...5 => 1, else => 0 }; }
fn log(fmt: []const u8, args: anytype) void { _ = fmt; _ = args; }
extern "c" fn printf(format: [*:0]const u8, ...) c_int;
"#,
    );

    let signature = |name| symbol(&result, name).signature.clone().unwrap();
    assert_eq!(signature("init"), "pub fn init(x: i32) Point");
    assert_eq!(signature("makePoint"), "fn makePoint() Point");
    assert_eq!(signature("maybePoint"), "fn maybePoint() ?*Point");
    assert_eq!(
        signature("load"),
        "fn load(alloc: std.mem.Allocator) !std.ArrayList(u8)"
    );
    assert_eq!(signature("ranges"), "fn ranges(x: u8) u8");
    assert_eq!(
        signature("log"),
        "fn log(fmt: []const u8, args: anytype) void"
    );
    assert_eq!(
        signature("printf"),
        "extern \"c\" fn printf(format: [*:0]const u8, ...) c_int"
    );

    assert_eq!(type_fact(&result, "init").as_deref(), Some("Point"));
    assert_eq!(type_fact(&result, "makePoint").as_deref(), Some("Point"));
    assert_eq!(type_fact(&result, "maybePoint").as_deref(), Some("Point"));
    assert_eq!(type_fact(&result, "count").as_deref(), Some("usize"));
    assert_eq!(type_fact(&result, "load").as_deref(), Some("ArrayList"));
}

#[test]
fn qualified_and_qualified_generic_declared_types_record_type_facts() {
    let result = extract(
        r#"const std = @import("std");
const Service = struct {
    allocator: std.mem.Allocator,
    users: std.ArrayList(u32),
    pub fn handle(self: *Service, req: *httpz.Request, file: std.fs.File) !void {
        var client: std.http.Client = .{ .allocator = self.allocator };
        defer client.deinit();
        _ = req;
        _ = file;
    }
};
var registry: std.StringHashMap(Point) = undefined;
"#,
    );

    assert_eq!(
        type_fact(&result, "allocator").as_deref(),
        Some("Allocator")
    );
    assert_eq!(type_fact(&result, "users").as_deref(), Some("ArrayList"));
    assert_eq!(type_fact(&result, "req").as_deref(), Some("Request"));
    assert_eq!(type_fact(&result, "file").as_deref(), Some("File"));
    assert_eq!(type_fact(&result, "client").as_deref(), Some("Client"));
    assert_eq!(
        type_fact(&result, "registry").as_deref(),
        Some("StringHashMap")
    );
}
