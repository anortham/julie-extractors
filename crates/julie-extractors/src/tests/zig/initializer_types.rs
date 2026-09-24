use crate::tests::helpers::init_parser;
use crate::zig::ZigExtractor;
use std::path::PathBuf;

#[derive(Debug, PartialEq, Eq)]
struct Fact {
    resolved: String,
    inferred: bool,
    declared: Option<String>,
}

fn inferred(resolved: &str) -> Option<Fact> {
    Some(Fact {
        resolved: resolved.to_string(),
        inferred: true,
        declared: None,
    })
}

fn inferred_as(resolved: &str, declared: &str) -> Option<Fact> {
    Some(Fact {
        resolved: resolved.to_string(),
        inferred: true,
        declared: Some(declared.to_string()),
    })
}

fn fact_in(file_path: &str, source: &str, local: &str) -> Option<Fact> {
    let tree = init_parser(source, "zig");
    let mut extractor = ZigExtractor::new(
        "zig".to_string(),
        file_path.to_string(),
        source.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    let local = symbols
        .iter()
        .find(|s| s.name == local)
        .unwrap_or_else(|| panic!("missing local {local}"));
    extractor.base.type_info.get(&local.id).map(|fact| Fact {
        resolved: fact.resolved_type.clone(),
        inferred: fact.is_inferred,
        declared: fact
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("declared"))
            .and_then(|declared| declared.as_str())
            .map(str::to_string),
    })
}

fn fact(source: &str, local: &str) -> Option<Fact> {
    fact_in("initializer_types.zig", source, local)
}

const LOADERS: &str = r#"
const Store = struct {
    items: u32,
};
fn plain() Store {}
fn load() !Store {}
fn loadNamed() LoadError!Store {}
fn find() ?Store {}
fn findLater() !?Store {}
fn count() usize {}
fn reset() void {}
"#;

fn store_type(body: &str) -> Option<Fact> {
    fact(
        &format!("{LOADERS}\nfn run() !void {{\n    {body}\n}}\n"),
        "store",
    )
}

#[test]
fn same_file_function_call_records_declared_return_type() {
    assert_eq!(store_type("const store = plain();"), inferred("Store"));
    assert_eq!(store_type("var store = count();"), inferred("usize"));
}

#[test]
fn try_removes_the_error_union_layer() {
    assert_eq!(store_type("const store = try load();"), inferred("Store"));
    assert_eq!(
        store_type("const store = try loadNamed();"),
        inferred("Store")
    );
    assert_eq!(
        store_type("const store = try findLater();"),
        inferred_as("Store", "?Store")
    );
}

#[test]
fn call_without_unwrap_keeps_the_written_wrapper_as_declared() {
    assert_eq!(
        store_type("const store = load();"),
        inferred_as("Store", "!Store")
    );
    assert_eq!(
        store_type("const store = find();"),
        inferred_as("Store", "?Store")
    );
}

#[test]
fn optional_unwrap_removes_the_optional_layer() {
    assert_eq!(store_type("const store = find().?;"), inferred("Store"));
    assert_eq!(
        store_type("const store = (try findLater()).?;"),
        inferred("Store")
    );
}

#[test]
fn catch_and_orelse_with_noreturn_fallback_unwrap_one_layer() {
    for chain in [
        "load() catch unreachable",
        "load() catch |err| return err",
        "load() catch |err| {\n        return err;\n    }",
        "find() orelse return",
        "find() orelse unreachable",
        "find() orelse return error.Missing",
    ] {
        assert_eq!(
            store_type(&format!("const store = {chain};")),
            inferred("Store"),
            "{chain}"
        );
    }
}

#[test]
fn value_fallbacks_record_no_fact() {
    for chain in [
        "load() catch null",
        "load() catch fallback",
        "load() catch blk: {\n        break :blk fallback;\n    }",
        "find() orelse fallback",
        "find() orelse find()",
    ] {
        assert_eq!(
            store_type(&format!("const store = {chain};")),
            None,
            "{chain}"
        );
    }
}

#[test]
fn unwrap_of_a_missing_layer_records_no_fact() {
    for chain in [
        "try plain()",
        "try find()",
        "plain().?",
        "load().?",
        "plain() catch unreachable",
        "find() catch unreachable",
        "load() orelse return",
        "try (try load())",
    ] {
        assert_eq!(
            store_type(&format!("const store = {chain};")),
            None,
            "{chain}"
        );
    }
}

#[test]
fn void_unknown_and_other_file_callees_record_no_fact() {
    for chain in [
        "reset()",
        "missing()",
        "other.load()",
        "std.fs.cwd().openFile(path, .{})",
        "try std.heap.page_allocator.create(Store)",
        "plain().items",
    ] {
        assert_eq!(
            store_type(&format!("const store = {chain};")),
            None,
            "{chain}"
        );
    }
}

#[test]
fn written_type_wins_over_the_initializer() {
    let fact = store_type("const store: Other = plain();").expect("declared fact");
    assert_eq!(fact.resolved, "Other");
    assert!(!fact.inferred);
}

#[test]
fn generic_return_types_record_no_fact() {
    let source = r#"
const Store = struct {};
fn get(comptime T: type) T {}
fn maybe(comptime T: type) ?T {}
fn many(comptime T: type) ![]T {}
fn run() !void {
    const a = get(Store);
    const b = maybe(Store).?;
    const c = try many(Store);
}
"#;
    for local in ["a", "b", "c"] {
        assert_eq!(fact(source, local), None, "{local}");
    }
}

const STORE: &str = r#"
const Token = struct {};
const Store = struct {
    const Self = @This();

    fn open() !Store {}
    fn create() Self {}
    fn next(self: *Store) ?Token {}
    fn peek(self: Self) Token {}
    fn nothing(self: *Store) void {}

    fn run(self: *Store) !void {
        const by_self = self.next().?;
        const by_alias = self.peek();
        const by_type = try Store.open();
        const by_self_type = Self.create();
        const sibling = try open();
        const unknown = self.missing();
        const empty = self.nothing();
        const chained = self.peek().kind();
    }

    fn aliasRun(self: *Self) void {
        const via_alias_receiver = self.peek();
    }
};
"#;

#[test]
fn self_method_call_records_the_method_return_type() {
    assert_eq!(fact(STORE, "by_self"), inferred("Token"));
    assert_eq!(fact(STORE, "by_alias"), inferred("Token"));
    assert_eq!(fact(STORE, "via_alias_receiver"), inferred("Token"));
}

#[test]
fn type_qualified_call_records_the_function_return_type() {
    assert_eq!(fact(STORE, "by_type"), inferred("Store"));
    assert_eq!(fact(STORE, "by_self_type"), inferred_as("Store", "Self"));
}

#[test]
fn bare_call_finds_a_function_of_the_enclosing_container() {
    assert_eq!(fact(STORE, "sibling"), inferred("Store"));
}

#[test]
fn unknown_void_and_chained_self_methods_record_no_fact() {
    for local in ["unknown", "empty", "chained"] {
        assert_eq!(fact(STORE, local), None, "{local}");
    }
}

#[test]
fn init_on_a_same_file_container_uses_its_declared_return_type() {
    let source = r#"
const Store = struct {
    fn init() !Store {}
};
const Bare = struct {};
fn run() !void {
    const declared = try Store.init();
    const wrapped = Store.init();
    const constructed = Bare.init();
}
"#;
    assert_eq!(fact(source, "declared"), inferred("Store"));
    assert_eq!(fact(source, "wrapped"), inferred_as("Store", "!Store"));
    assert_eq!(fact(source, "constructed"), inferred("Bare"));
}

#[test]
fn generic_init_on_a_same_file_container_records_no_fact() {
    let source = r#"
const Store = struct {
    fn init(comptime T: type) T {}
};
fn run() void {
    const value = Store.init(u8);
}
"#;
    assert_eq!(fact(source, "value"), None);
}

#[test]
fn disagreeing_same_named_candidates_record_no_fact() {
    let source = r#"
const A = struct {};
const B = struct {};
fn make() A {}
const Outer = struct {
    const Inner = struct {
        fn make() B {}
        fn build() A {}

        fn run() void {
            const shadowed = make();
        }
    };
};
const Other = struct {
    const Inner = struct {
        fn build() B {}
    };
};
fn run() void {
    const top = make();
    const ambiguous = Inner.build();
}
"#;
    assert_eq!(fact(source, "top"), inferred("A"));
    assert_eq!(fact(source, "shadowed"), None);
    assert_eq!(fact(source, "ambiguous"), None);
}

#[test]
fn non_function_declaration_of_the_same_name_blocks_inference() {
    let source = r#"
const A = struct {};
fn make() A {}
const Store = struct {
    const make = other.make;

    fn run() void {
        const value = make();
    }
};
"#;
    assert_eq!(fact(source, "value"), None);
}

#[test]
fn anonymous_generic_container_methods_record_no_fact() {
    let source = r#"
const A = struct {};
fn make() A {}
fn List(comptime T: type) type {
    return struct {
        const Self = @This();
        const Item = T;

        fn make() T {}
        fn first(self: *Self) T {}
        fn build() Self {}
        fn last() Item {}

        fn run(self: *Self) void {
            const bare = make();
            const by_self = self.first();
            const by_alias = Self.build();
            const aliased_item = last();
        }
    };
}
"#;
    for local in ["bare", "by_self", "by_alias", "aliased_item"] {
        assert_eq!(fact(source, local), None, "{local}");
    }
}

#[test]
fn local_receivers_record_no_fact() {
    let source = r#"
const Token = struct {};
const Store = struct {
    fn next(self: *Store) Token {}
};
fn run(store: *Store) void {
    const local = Store{};
    const from_local = local.next();
    const from_param = store.next();
}
"#;
    assert_eq!(fact(source, "from_local"), None);
    assert_eq!(fact(source, "from_param"), None);
}

#[test]
fn file_struct_self_call_records_the_top_level_return_type() {
    let source = r#"
const Tokenizer = @This();
const Token = struct {};

fn next(self: *Tokenizer) Token {}

fn run(self: *Tokenizer) void {
    const token = self.next();
    const created = Tokenizer.create();
}

fn create() Tokenizer {}
"#;
    assert_eq!(
        fact_in("src/Tokenizer.zig", source, "token"),
        inferred("Token")
    );
    assert_eq!(
        fact_in("src/Tokenizer.zig", source, "created"),
        inferred("Tokenizer")
    );
}

#[test]
fn qualified_type_receivers_record_no_fact() {
    let source = r#"
const Store = struct {
    const Self = @This();

    fn create() Self {}

    fn run() void {
        const aliased = other.Self.create();
        const nested = other.Store.create();
    }
};
"#;
    for local in ["aliased", "nested"] {
        assert_eq!(fact(source, local), None, "{local}");
    }
}

#[test]
fn destructured_call_results_record_no_fact() {
    let source = r#"
const Pair = struct {};
fn split() Pair {}
fn run() void {
    const first, const second = split();
}
"#;
    for local in ["first", "second"] {
        assert_eq!(fact(source, local), None, "{local}");
    }
}

const SCOPED_RECEIVERS: &str = r#"
const other = @import("other.zig");
const A = struct {
    const Store = struct {
        fn open() u32 {}
        fn next(self: *Store) u8 {}
    };
};
const B = struct {
    const Store = other.Store;

    fn f() void {
        const sibling_alias = Store.open();
    }

    fn g() void {
        const Node = other.Node;
        const local_alias = Node.make();
    }

    fn h(self: *Store) void {
        const alias_self = self.next();
    }
};
const C = struct {
    const Node = struct {
        fn make() u16 {}
    };

    fn run() void {
        const in_scope = Node.make();
    }
};
"#;

#[test]
fn type_receiver_resolves_to_the_nearest_declaration_in_scope() {
    assert_eq!(fact(SCOPED_RECEIVERS, "in_scope"), inferred("u16"));
}

#[test]
fn type_receiver_that_aliases_an_external_type_records_no_fact() {
    for local in ["sibling_alias", "local_alias"] {
        assert_eq!(fact(SCOPED_RECEIVERS, local), None, "{local}");
    }
}

#[test]
fn self_receiver_that_aliases_an_external_type_records_no_fact() {
    assert_eq!(fact(SCOPED_RECEIVERS, "alias_self"), None);
}

#[test]
fn named_container_inside_a_generic_function_records_no_fact() {
    let source = r#"
fn Make(comptime T: type) type {
    const Item = T;
    const Inner = struct {
        fn get() Item {}
        fn getT() T {}
        fn size() usize {}
    };
    const via_alias = Inner.get();
    const via_t = Inner.getT();
    const via_size = Inner.size();
    return Inner;
}
"#;
    for local in ["via_alias", "via_t", "via_size"] {
        assert_eq!(fact(source, local), None, "{local}");
    }
}

#[test]
fn outer_this_alias_resolves_to_the_declaring_container() {
    let source = r#"
const Outer = struct {
    const Self = @This();

    const Inner = struct {
        fn make() Self {}

        fn run() void {
            const outer_self = make();
        }
    };
};
"#;
    assert_eq!(fact(source, "outer_self"), inferred_as("Outer", "Self"));
}

#[test]
fn noreturn_builtin_fallbacks_unwrap_one_layer() {
    for chain in [
        "load() catch @panic(\"x\")",
        "load() catch |err| @panic(@errorName(err))",
        "find() orelse @panic(\"x\")",
        "find() orelse @trap()",
    ] {
        assert_eq!(
            store_type(&format!("const store = {chain};")),
            inferred("Store"),
            "{chain}"
        );
    }
}

#[test]
fn value_builtin_fallbacks_record_no_fact() {
    for chain in [
        "find() orelse @as(Store, undefined)",
        "load() catch @as(Store, undefined)",
    ] {
        assert_eq!(
            store_type(&format!("const store = {chain};")),
            None,
            "{chain}"
        );
    }
}

#[test]
fn struct_literal_type_resolves_in_scope() {
    let source = r#"
const other = @import("other.zig");
const A = struct {
    const Store = struct {};
};
const B = struct {
    const Self = @This();
    const Store = other.Store;

    fn run() void {
        const external = Store{};
        const own = Self{};
    }
};
"#;
    assert_eq!(fact(source, "external"), None);
    assert_eq!(fact(source, "own"), inferred_as("B", "Self"));
}

#[test]
fn qualified_same_file_container_paths_resolve_in_scope() {
    let source = r#"
const other = @import("other.zig");
const Client = struct {
    const Context = struct {
        fn make() u16 {}
    };
};
fn run() void {
    const literal = Client.Context{};
    const call = Client.Context.make();
    const external_literal = other.Context{};
}
"#;
    assert_eq!(
        fact(source, "literal"),
        inferred_as("Context", "Client.Context")
    );
    assert_eq!(fact(source, "call"), inferred("u16"));
    assert_eq!(fact(source, "external_literal"), None);
}
