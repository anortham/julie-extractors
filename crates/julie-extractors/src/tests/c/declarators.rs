use crate::base::{ExtractionResults, RelationshipKind, Symbol, SymbolKind};
use crate::extract_canonical;
use std::path::Path;

fn extract(source: &str) -> ExtractionResults {
    extract_canonical("src/probe.c", source, Path::new("/tmp/test"))
        .expect("canonical C extraction must succeed")
}

fn rows<'a>(result: &'a ExtractionResults, name: &str) -> Vec<&'a Symbol> {
    result.symbols.iter().filter(|s| s.name == name).collect()
}

fn only<'a>(result: &'a ExtractionResults, name: &str) -> &'a Symbol {
    let found = rows(result, name);
    assert_eq!(found.len(), 1, "expected one `{name}` row, got {found:#?}");
    found[0]
}

fn name_of(result: &ExtractionResults, id: &str) -> String {
    result
        .symbols
        .iter()
        .find(|s| s.id == id)
        .map(|s| s.name.clone())
        .unwrap_or_default()
}

fn metadata_str<'a>(symbol: &'a Symbol, key: &str) -> &'a str {
    symbol
        .metadata
        .as_ref()
        .and_then(|m| m.get(key))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
}

const KERNEL: &str = r#"struct device;
struct driver { const char *name; int (*probe)(struct device *dev); };
static int my_probe(struct device *dev) {
    struct driver *drv = (struct driver *)dev_get_drvdata(dev);
    return drv->probe(dev) + (int)sizeof(struct device *);
}
static struct driver my_driver = { .name = "my", .probe = my_probe };
"#;

#[test]
fn struct_references_emit_no_struct_rows() {
    let result = extract(KERNEL);
    assert!(rows(&result, "device").is_empty());
    let driver = only(&result, "driver");
    assert_eq!(driver.kind, SymbolKind::Struct);
    assert!(driver.parent_id.is_none());
}

#[test]
fn struct_reference_uses_resolve_to_the_definition() {
    let result = extract(KERNEL);
    let driver = only(&result, "driver");
    let my_probe = only(&result, "my_probe");
    assert!(
        result
            .relationships
            .iter()
            .any(|r| r.kind == RelationshipKind::Uses
                && r.from_symbol_id == my_probe.id
                && r.to_symbol_id == driver.id),
        "{:#?}",
        result.relationships
    );
    assert!(
        result
            .relationships
            .iter()
            .all(|r| r.from_symbol_id != r.to_symbol_id)
    );
}

#[test]
fn typedef_of_same_named_struct_is_one_row() {
    let result = extract(
        "typedef struct Point { int x; int y; } Point;\ntypedef struct Point Alias;\nstruct Point make_point(void);\n",
    );
    let point = only(&result, "Point");
    assert_eq!(point.kind, SymbolKind::Struct);
    let fields: Vec<_> = result
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Field)
        .collect();
    assert_eq!(fields.len(), 2);
    assert!(
        fields
            .iter()
            .all(|f| f.parent_id.as_deref() == Some(&point.id))
    );
    assert_eq!(only(&result, "Alias").kind, SymbolKind::Type);
}

#[test]
fn typedef_with_a_differently_named_struct_keeps_both_names() {
    let result = extract("typedef struct node_s { struct node_s *next; } node_t;\n");
    let node_s = only(&result, "node_s");
    let node_t = only(&result, "node_t");
    assert_eq!(node_s.kind, SymbolKind::Struct);
    assert_eq!(node_t.kind, SymbolKind::Struct);
    assert!(node_s.parent_id.is_none());
    assert_eq!(
        only(&result, "next").parent_id.as_deref(),
        Some(node_t.id.as_str())
    );
}

#[test]
fn typedef_enum_is_an_enum_that_owns_its_enumerators() {
    let result = extract("typedef enum { RED, GREEN } Color;\nenum { LOOSE };\n");
    let color = only(&result, "Color");
    assert_eq!(color.kind, SymbolKind::Enum);
    assert_eq!(only(&result, "RED").parent_id.as_deref(), Some(&*color.id));
    assert_eq!(
        only(&result, "GREEN").parent_id.as_deref(),
        Some(&*color.id)
    );
    assert!(only(&result, "LOOSE").parent_id.is_none());
}

#[test]
fn pointer_returning_prototypes_and_definitions_are_functions() {
    let result = extract(
        r#"/** Duplicate a string. */
char *str_dup(const char *s);
struct Node *node_new(void);
char **split_words(const char *s, int *count);

char **make_list(int n) {
    return calloc(n, sizeof(char *));
}
"#,
    );
    for name in ["str_dup", "node_new", "split_words"] {
        let symbol = only(&result, name);
        assert_eq!(symbol.kind, SymbolKind::Function, "{name}");
        assert_eq!(metadata_str(symbol, "isDefinition"), "false", "{name}");
    }
    assert!(
        only(&result, "str_dup")
            .doc_comment
            .as_deref()
            .is_some_and(|doc| doc.contains("Duplicate a string"))
    );
    assert!(rows(&result, "Node").is_empty());

    let make_list = only(&result, "make_list");
    assert_eq!(make_list.kind, SymbolKind::Function);
    assert_eq!(metadata_str(make_list, "isDefinition"), "true");
    assert_eq!(metadata_str(make_list, "returnType"), "char**");
    assert!(make_list.body_span.is_some());
    assert_eq!(
        only(&result, "n").parent_id.as_deref(),
        Some(make_list.id.as_str())
    );
    assert!(
        result
            .structured_pending_relationships
            .iter()
            .any(|p| p.target.terminal_name == "calloc"
                && p.pending.from_symbol_id == make_list.id),
        "{:#?}",
        result.structured_pending_relationships
    );
}

#[test]
fn pointer_variables_are_extracted_per_declarator() {
    let result = extract(
        r#"typedef struct Buffer { int len; } Buffer;
static Buffer *global_buf;
struct Buffer *shared;
const char *const names[3];
int count, *ptr, arr[4];
int use(void) {
    Buffer *b;
    char *cursor;
    b = global_buf; cursor = 0;
    return b->len + shared->len;
}
"#,
    );
    for name in ["global_buf", "shared", "names", "ptr", "b", "cursor"] {
        assert_eq!(only(&result, name).kind, SymbolKind::Variable, "{name}");
    }
    assert_eq!(
        only(&result, "global_buf").visibility,
        Some(crate::base::Visibility::Private)
    );
    let use_fn = only(&result, "use");
    assert_eq!(only(&result, "b").parent_id.as_deref(), Some(&*use_fn.id));

    assert_eq!(
        only(&result, "count").signature.as_deref(),
        Some("int count")
    );
    assert_eq!(metadata_str(only(&result, "count"), "dataType"), "int");
    assert_eq!(only(&result, "ptr").signature.as_deref(), Some("int* ptr"));
    assert_eq!(metadata_str(only(&result, "ptr"), "dataType"), "int*");
    assert_eq!(
        only(&result, "arr").signature.as_deref(),
        Some("int arr[4]")
    );
    assert_eq!(metadata_str(only(&result, "arr"), "dataType"), "int");

    let global_buf = result
        .types
        .get(&only(&result, "global_buf").id)
        .expect("global_buf type fact");
    assert_eq!(global_buf.resolved_type, "Buffer");
    let names = result
        .types
        .get(&only(&result, "names").id)
        .expect("names type fact");
    assert_eq!(names.resolved_type, "char[]");
    assert_eq!(
        names
            .metadata
            .as_ref()
            .and_then(|m| m.get("declared"))
            .and_then(|v| v.as_str()),
        Some("const char *const[3]")
    );
}

#[test]
fn typedef_names_come_from_each_declarator() {
    let result = extract(
        r#"typedef struct list_node list_node_t;
typedef struct { int x; } Foo, *FooPtr;
typedef void handler_t(int signo);
typedef int (*visit_fn)(list_node_t *node, void *ctx);
typedef unsigned long size_type;
typedef int Matrix[4][4];
struct walker { visit_fn visit; };
"#,
    );
    assert_eq!(only(&result, "Foo").kind, SymbolKind::Struct);
    assert_eq!(only(&result, "FooPtr").kind, SymbolKind::Type);
    for name in ["handler_t", "visit_fn", "size_type", "Matrix"] {
        assert_eq!(only(&result, name).kind, SymbolKind::Type, "{name}");
    }
    for parameter in ["signo", "node", "ctx"] {
        assert!(rows(&result, parameter).is_empty(), "{parameter}");
    }
    assert_eq!(
        only(&result, "visit_fn").signature.as_deref(),
        Some("typedef int (*visit_fn)(list_node_t *node, void *ctx)")
    );
    assert_eq!(
        only(&result, "size_type").signature.as_deref(),
        Some("typedef unsigned long size_type")
    );
    assert_eq!(
        only(&result, "FooPtr").signature.as_deref(),
        Some("typedef struct { ... } *FooPtr")
    );
    let walker = only(&result, "walker");
    let visit_fn = only(&result, "visit_fn");
    assert!(
        result
            .relationships
            .iter()
            .any(|r| r.kind == RelationshipKind::Uses
                && r.from_symbol_id == walker.id
                && r.to_symbol_id == visit_fn.id),
        "{:#?}",
        result
            .relationships
            .iter()
            .map(|r| (
                name_of(&result, &r.from_symbol_id),
                name_of(&result, &r.to_symbol_id)
            ))
            .collect::<Vec<_>>()
    );
}

#[test]
fn function_pointer_declarators_are_variables() {
    let result = extract(
        r#"static int compare(const void *a, const void *b) { return 0; }
static void (*handlers[4])(int);
int (*global_cmp)(const void *, const void *) = compare;
int (*get_callback(void))(int) { return 0; }
void sort_all(int *xs, int n) {
    int (*cmp)(const void *, const void *) = compare;
    qsort(xs, n, sizeof(int), cmp);
    global_cmp(xs, xs);
}
"#,
    );
    assert!(result.symbols.iter().all(|s| !s.name.contains('(')));
    for name in ["handlers", "global_cmp", "cmp"] {
        let symbol = only(&result, name);
        assert_eq!(symbol.kind, SymbolKind::Variable, "{name}");
    }
    assert_eq!(
        only(&result, "global_cmp").signature.as_deref(),
        Some("int (*global_cmp)(const void *, const void *) = compare")
    );
    let sort_all = only(&result, "sort_all");
    assert_eq!(
        only(&result, "cmp").parent_id.as_deref(),
        Some(&*sort_all.id)
    );
    assert_eq!(only(&result, "get_callback").kind, SymbolKind::Function);

    let global_cmp = only(&result, "global_cmp");
    assert!(
        result
            .relationships
            .iter()
            .any(|r| r.kind == RelationshipKind::Calls
                && r.from_symbol_id == sort_all.id
                && r.to_symbol_id == global_cmp.id),
        "{:#?}",
        result.relationships
    );
}
