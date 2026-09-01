#[cfg(test)]
mod zig_type_fact_tests {
    use crate::base::{IdentifierKind, Symbol, SymbolKind, TypeInfo};
    use crate::tests::helpers::init_parser;
    use crate::zig::ZigExtractor;
    use std::path::PathBuf;

    fn extract(code: &str) -> (Vec<Symbol>, ZigExtractor) {
        let tree = init_parser(code, "zig");
        let mut extractor = ZigExtractor::new(
            "zig".to_string(),
            "test.zig".to_string(),
            code.to_string(),
            &PathBuf::from("/tmp/test"),
        );
        let symbols = extractor.extract_symbols(&tree);
        (symbols, extractor)
    }

    fn symbol<'a>(symbols: &'a [Symbol], name: &str) -> &'a Symbol {
        symbols
            .iter()
            .find(|s| s.name == name)
            .unwrap_or_else(|| panic!("missing symbol `{name}`"))
    }

    fn variable<'a>(symbols: &'a [Symbol], name: &str) -> &'a Symbol {
        symbols
            .iter()
            .find(|s| s.name == name && s.kind == SymbolKind::Variable)
            .unwrap_or_else(|| panic!("missing variable symbol `{name}`"))
    }

    fn fact<'a>(extractor: &'a ZigExtractor, symbol: &Symbol) -> &'a TypeInfo {
        extractor
            .base
            .type_info
            .get(&symbol.id)
            .unwrap_or_else(|| panic!("missing type fact for `{}`", symbol.name))
    }

    fn declared_metadata(fact: &TypeInfo) -> Option<&serde_json::Value> {
        fact.metadata.as_ref().and_then(|m| m.get("declared"))
    }

    fn no_fact(extractor: &ZigExtractor, symbol: &Symbol) {
        assert!(
            extractor.base.type_info.get(&symbol.id).is_none(),
            "expected no type fact for `{}`",
            symbol.name
        );
    }

    fn parameter_symbols<'a>(symbols: &'a [Symbol], name: &str) -> Vec<&'a Symbol> {
        symbols
            .iter()
            .filter(|s| {
                s.name == name
                    && s.metadata
                        .as_ref()
                        .and_then(|m| m.get("role"))
                        .map(|role| role == &serde_json::json!("parameter"))
                        .unwrap_or(false)
            })
            .collect()
    }

    fn param_fact<'a>(
        extractor: &'a ZigExtractor,
        symbols: &'a [Symbol],
        parent: &Symbol,
        name: &str,
    ) -> &'a TypeInfo {
        let params = parameter_symbols(symbols, name);
        let param = params
            .iter()
            .copied()
            .find(|p| p.parent_id.as_deref() == Some(parent.id.as_str()))
            .unwrap_or_else(|| panic!("missing `{name}` parameter under `{}`", parent.name));
        assert_eq!(param.kind, SymbolKind::Variable);
        fact(extractor, param)
    }

    #[test]
    fn typed_parameters_record_base_names() {
        let (symbols, extractor) = extract(
            r#"
const Store = struct {
    x: u32,
};

fn f(self: *Store, n: u32, list: ArrayList(u8)) void {}
"#,
        );

        let f = symbol(&symbols, "f");
        let self_fact = param_fact(&extractor, &symbols, f, "self");
        assert_eq!(self_fact.resolved_type, "Store");
        assert!(!self_fact.is_inferred);
        assert_eq!(
            declared_metadata(self_fact),
            Some(&serde_json::json!("*Store"))
        );

        let n_fact = param_fact(&extractor, &symbols, f, "n");
        assert_eq!(n_fact.resolved_type, "u32");
        assert!(!n_fact.is_inferred);
        assert!(declared_metadata(n_fact).is_none());

        let list_fact = param_fact(&extractor, &symbols, f, "list");
        assert_eq!(list_fact.resolved_type, "ArrayList");
        assert!(!list_fact.is_inferred);
        assert_eq!(
            declared_metadata(list_fact),
            Some(&serde_json::json!("ArrayList(u8)"))
        );
    }

    #[test]
    fn local_const_struct_literal_is_variable_with_inferred_fact() {
        let (symbols, extractor) = extract(
            r#"
const Store = struct {
    x: u32,
};

fn demo() void {
    const s = Store{ .x = 1 };
}
"#,
        );

        let demo = symbol(&symbols, "demo");
        let s = variable(&symbols, "s");
        assert_eq!(s.parent_id.as_deref(), Some(demo.id.as_str()));
        let s_fact = fact(&extractor, s);
        assert_eq!(s_fact.resolved_type, "Store");
        assert!(s_fact.is_inferred);
    }

    #[test]
    fn constructor_negatives_keep_symbols_without_facts() {
        let (symbols, extractor) = extract(
            r#"
fn make() u32 {
    return 1;
}

fn demo() void {
    const a = Unknown{};
    const b = std.ArrayList(u8).init(undefined);
    const c = make();
}
"#,
        );

        let demo = symbol(&symbols, "demo");
        for name in ["a", "b", "c"] {
            let local = variable(&symbols, name);
            assert_eq!(local.parent_id.as_deref(), Some(demo.id.as_str()));
            no_fact(&extractor, local);
        }
    }

    #[test]
    fn array_local_records_no_fact() {
        let (symbols, extractor) = extract(
            r#"
fn demo() void {
    var buf: [8]u8 = undefined;
}
"#,
        );

        let buf = variable(&symbols, "buf");
        no_fact(&extractor, buf);
    }

    #[test]
    fn container_const_stays_constant_and_fields_record_facts() {
        let (symbols, extractor) = extract(
            r#"
const Store = struct {
    items: ArrayList(u8),
};

const Limit: u32 = 4;

fn demo() void {
    const scratch: u32 = 1;
}
"#,
        );

        let limit = symbol(&symbols, "Limit");
        assert_eq!(limit.kind, SymbolKind::Constant);
        let limit_fact = fact(&extractor, limit);
        assert_eq!(limit_fact.resolved_type, "u32");
        assert!(!limit_fact.is_inferred);

        let items = symbols
            .iter()
            .find(|s| s.name == "items" && s.kind == SymbolKind::Field)
            .expect("missing items field");
        let items_fact = fact(&extractor, items);
        assert_eq!(items_fact.resolved_type, "ArrayList");
        assert!(!items_fact.is_inferred);
        assert_eq!(
            declared_metadata(items_fact),
            Some(&serde_json::json!("ArrayList(u8)"))
        );

        let scratch = variable(&symbols, "scratch");
        assert_eq!(scratch.kind, SymbolKind::Variable);
        let scratch_fact = fact(&extractor, scratch);
        assert_eq!(scratch_fact.resolved_type, "u32");
        assert!(!scratch_fact.is_inferred);
    }

    #[test]
    fn self_parameter_method_call_records_receiver_type() {
        let code = r#"
const Store = struct {
    pub fn run(self: *Store) void {}

    pub fn go(self: *Store) void {
        self.run();
        self.missing();
        other.missing();
    }
};
"#;
        let tree = init_parser(code, "zig");
        let mut extractor = ZigExtractor::new(
            "zig".to_string(),
            "test.zig".to_string(),
            code.to_string(),
            &PathBuf::from("/tmp/test"),
        );
        let symbols = extractor.extract_symbols(&tree);
        let identifiers = extractor.extract_identifiers(&tree, &symbols);
        let relationships = extractor.extract_relationships(&tree, &symbols);

        let run_calls: Vec<_> = identifiers
            .iter()
            .filter(|id| id.name == "run" && id.kind == IdentifierKind::Call)
            .collect();
        assert_eq!(run_calls.len(), 1);
        assert_eq!(run_calls[0].receiver_type.as_deref(), Some("Store"));

        let missing_calls: Vec<_> = identifiers
            .iter()
            .filter(|id| id.name == "missing" && id.kind == IdentifierKind::Call)
            .collect();
        assert_eq!(missing_calls.len(), 2);
        assert_eq!(
            missing_calls
                .iter()
                .filter(|id| id.receiver_type.as_deref() == Some("Store"))
                .count(),
            1
        );
        assert_eq!(
            missing_calls
                .iter()
                .filter(|id| id.receiver_type.is_none())
                .count(),
            1
        );

        let run = symbol(&symbols, "run");
        let go = symbol(&symbols, "go");
        assert_eq!(
            relationships
                .iter()
                .filter(|rel| rel.from_symbol_id == go.id && rel.to_symbol_id == run.id)
                .count(),
            1
        );

        let pending = extractor.get_structured_pending_relationships();
        assert!(
            pending
                .iter()
                .all(|pending| pending.target.terminal_name != "run")
        );
        let missing_pending: Vec<_> = pending
            .iter()
            .filter(|pending| pending.target.terminal_name == "missing")
            .collect();
        assert_eq!(missing_pending.len(), 2);
        assert_eq!(
            missing_pending
                .iter()
                .filter(|pending| pending.receiver_type.as_deref() == Some("Store"))
                .count(),
            1
        );
        assert_eq!(
            missing_pending
                .iter()
                .filter(|pending| pending.receiver_type.is_none())
                .count(),
            1
        );
    }

    #[test]
    fn union_method_self_call_records_receiver_type() {
        let code = r#"
const Packet = union {
    id: u8,

    pub fn go(self: *Packet) void {
        self.missing();
    }
};
"#;
        let tree = init_parser(code, "zig");
        let mut extractor = ZigExtractor::new(
            "zig".to_string(),
            "test.zig".to_string(),
            code.to_string(),
            &PathBuf::from("/tmp/test"),
        );
        let symbols = extractor.extract_symbols(&tree);
        let identifiers = extractor.extract_identifiers(&tree, &symbols);
        extractor.extract_relationships(&tree, &symbols);

        let missing_calls: Vec<_> = identifiers
            .iter()
            .filter(|id| id.name == "missing" && id.kind == IdentifierKind::Call)
            .collect();
        assert_eq!(missing_calls.len(), 1);
        assert_eq!(missing_calls[0].receiver_type.as_deref(), Some("Packet"));

        let missing_pending: Vec<_> = extractor
            .get_structured_pending_relationships()
            .into_iter()
            .filter(|pending| pending.target.terminal_name == "missing")
            .collect();
        assert_eq!(missing_pending.len(), 1);
        assert_eq!(missing_pending[0].receiver_type.as_deref(), Some("Packet"));
    }

    #[test]
    fn container_const_inside_generic_function_stays_constant() {
        let (symbols, _) = extract(
            r#"
fn List(comptime T: type) type {
    return struct {
        pub const Item = T;
        var count: u32 = 0;

        pub fn reset(self: *@This()) void {
            const scratch = 1;
        }
    };
}
"#,
        );

        assert_eq!(symbol(&symbols, "Item").kind, SymbolKind::Constant);
        assert_eq!(symbol(&symbols, "count").kind, SymbolKind::Variable);
        assert_eq!(symbol(&symbols, "scratch").kind, SymbolKind::Variable);
    }

    #[test]
    fn slice_type_records_element_base_with_declared_slice() {
        let (symbols, extractor) = extract(
            r#"
const Store = struct {
    name: []const u8,
};

fn f(name: []const u8, path: [:0]const u8) void {}
"#,
        );

        let field = symbols
            .iter()
            .find(|s| s.name == "name" && s.kind == SymbolKind::Field)
            .expect("missing name field");
        let field_fact = fact(&extractor, field);
        assert_eq!(field_fact.resolved_type, "u8");
        assert!(!field_fact.is_inferred);
        assert_eq!(
            declared_metadata(field_fact),
            Some(&serde_json::json!("[]const u8"))
        );

        let f = symbol(&symbols, "f");
        let name_fact = param_fact(&extractor, &symbols, f, "name");
        assert_eq!(name_fact.resolved_type, "u8");
        assert_eq!(
            declared_metadata(name_fact),
            Some(&serde_json::json!("[]const u8"))
        );
        let path_fact = param_fact(&extractor, &symbols, f, "path");
        assert_eq!(path_fact.resolved_type, "u8");
        assert_eq!(
            declared_metadata(path_fact),
            Some(&serde_json::json!("[:0]const u8"))
        );
    }

    #[test]
    fn artifact_types_carry_no_rows_for_negative_case_locals() {
        let code = r#"
const Store = struct {
    items: i32,
};

fn make() void {}

fn demo(self: *Store, event: []const u8) void {
    const s = Store{ .items = 1 };
    const a = Unknown{};
    const b = std.ArrayList(u8).init(undefined);
    const c = make();
    var buf: [8]u8 = undefined;
    _ = .{ s, a, b, c, buf };
}
"#;
        let tree = init_parser(code, "zig");
        let results = crate::factory::extract_symbols_and_relationships(
            &tree,
            "test.zig",
            code,
            "zig",
            &PathBuf::from("/tmp/test"),
        )
        .expect("zig extraction");

        let id_of = |name: &str| {
            results
                .symbols
                .iter()
                .find(|s| s.name == name)
                .unwrap_or_else(|| panic!("missing symbol `{name}`"))
                .id
                .clone()
        };
        for name in ["a", "b", "c", "buf", "demo", "make"] {
            assert!(
                results.types.get(&id_of(name)).is_none(),
                "unexpected type row for `{name}`"
            );
        }
        assert_eq!(results.types[&id_of("s")].resolved_type, "Store");
        assert_eq!(results.types[&id_of("event")].resolved_type, "u8");
        assert_eq!(results.types[&id_of("self")].resolved_type, "Store");
        for info in results.types.values() {
            let value = info.resolved_type.as_str();
            assert!(
                !value.contains(char::is_whitespace)
                    && !value.contains(['(', '[', '<', '*', '&', '?'])
                    && !matches!(value, "inferred" | "Unknown" | "std" | "make"),
                "corrupt resolved_type `{value}`"
            );
        }
    }

    #[test]
    fn this_parameter_method_call_records_enclosing_container() {
        let code = r#"
const Store = struct {
    pub fn run(self: *@This()) void {}

    pub fn go(self: *@This()) void {
        self.run();
        self.missing();
    }
};
"#;
        let tree = init_parser(code, "zig");
        let mut extractor = ZigExtractor::new(
            "zig".to_string(),
            "test.zig".to_string(),
            code.to_string(),
            &PathBuf::from("/tmp/test"),
        );
        let symbols = extractor.extract_symbols(&tree);
        let identifiers = extractor.extract_identifiers(&tree, &symbols);
        extractor.extract_relationships(&tree, &symbols);

        let self_calls: Vec<_> = identifiers
            .iter()
            .filter(|id| {
                (id.name == "run" || id.name == "missing") && id.kind == IdentifierKind::Call
            })
            .collect();
        assert_eq!(self_calls.len(), 2);
        assert!(
            self_calls
                .iter()
                .all(|id| id.receiver_type.as_deref() == Some("Store"))
        );

        let pending = extractor.get_structured_pending_relationships();
        assert!(
            pending
                .iter()
                .all(|pending| pending.target.terminal_name != "run")
        );
        let missing_pending: Vec<_> = pending
            .iter()
            .filter(|pending| pending.target.terminal_name == "missing")
            .collect();
        assert_eq!(missing_pending.len(), 1);
        assert_eq!(missing_pending[0].receiver_type.as_deref(), Some("Store"));
    }
}
