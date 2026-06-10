use crate::base::SymbolKind;
use crate::tests::zig::zig_extractor_tests::extract_symbols;

#[test]
fn function_declaration_builtins_emit_annotation_markers() {
    let code = r#"
inline fn fast_path(value: i32) i32 {
    return value + 1;
}

export fn ffi_entry(value: i32) i32 {
    return value;
}
"#;
    let symbols = extract_symbols(code);

    let fast_path = symbols
        .iter()
        .find(|symbol| symbol.name == "fast_path")
        .expect("fast_path");
    assert_eq!(fast_path.annotations.len(), 1);
    assert_eq!(fast_path.annotations[0].annotation_key, "inline");

    let ffi_entry = symbols
        .iter()
        .find(|symbol| symbol.name == "ffi_entry")
        .expect("ffi_entry");
    assert_eq!(ffi_entry.annotations.len(), 1);
    assert_eq!(ffi_entry.annotations[0].annotation_key, "export");
}

#[test]
fn variable_declaration_builtins_emit_annotation_markers() {
    let code = r#"
threadlocal var worker_tls: i32 = 0;
"#;
    let symbols = extract_symbols(code);

    let worker_tls = symbols
        .iter()
        .find(|symbol| symbol.name == "worker_tls" && symbol.kind == SymbolKind::Variable)
        .expect("worker_tls");
    assert_eq!(worker_tls.annotations.len(), 1);
    assert_eq!(worker_tls.annotations[0].annotation_key, "threadlocal");
}
