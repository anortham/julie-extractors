use crate::base::{
    ContainingSymbolIndex, Relationship, RelationshipKind, Symbol, SymbolKind, UnresolvedTarget,
};
use std::collections::HashMap;
use tree_sitter::Node;

const GO_STDLIB_ROOT_PACKAGES: &[&str] = &[
    "archive",
    "bufio",
    "builtin",
    "bytes",
    "cmp",
    "compress",
    "container",
    "context",
    "crypto",
    "database",
    "debug",
    "embed",
    "encoding",
    "errors",
    "expvar",
    "flag",
    "fmt",
    "go",
    "hash",
    "html",
    "image",
    "index",
    "io",
    "iter",
    "log",
    "maps",
    "math",
    "mime",
    "net",
    "os",
    "path",
    "plugin",
    "reflect",
    "regexp",
    "runtime",
    "slices",
    "sort",
    "strconv",
    "strings",
    "sync",
    "syscall",
    "testing",
    "text",
    "time",
    "unicode",
    "unique",
    "unsafe",
];

fn import_path_from_signature(signature: &str) -> Option<&str> {
    signature
        .strip_prefix("import ")?
        .split_whitespace()
        .next_back()
        .map(|path| path.trim_matches('"'))
}

fn is_stdlib_import_path(import_path: &str) -> bool {
    let import_path = import_path.trim_matches('"');
    let root = import_path.split('/').next().unwrap_or(import_path);
    GO_STDLIB_ROOT_PACKAGES.contains(&root)
}

/// Symbols and lookups shared by the relationship walk of one file.
pub(super) struct RelationshipScope<'a> {
    pub symbols: &'a [Symbol],
    pub symbol_map: HashMap<String, &'a Symbol>,
    pub containers: ContainingSymbolIndex<'a>,
}

/// Relationship extraction for Go (method receivers, interface implementations, embedding, function calls)
impl super::GoExtractor {
    pub(super) fn walk_tree_for_relationships(
        &mut self,
        node: Node,
        scope: &RelationshipScope<'_>,
        relationships: &mut Vec<Relationship>,
        depth: u32,
    ) {
        if !crate::tree_traversal::should_visit_tree_depth(depth) {
            return;
        }

        if node.kind() == "method_declaration" {
            self.extract_method_relationships_from_node(node, scope, relationships);
        }

        match node.kind() {
            "call_expression" | "type_conversion_expression" => {
                self.extract_call_relationships(node, scope, relationships);
            }
            "type_spec" => self.extract_embedding_relationships(node, scope, relationships),
            "var_spec" => self.extract_interface_assertion(node, scope, relationships),
            _ => {}
        }

        let Some(child_depth) = crate::tree_traversal::child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_tree_for_relationships(child, scope, relationships, child_depth);
        }
    }

    /// Emit a `uses` edge from a method to its same-file receiver type.
    pub(super) fn extract_method_relationships_from_node(
        &self,
        node: Node,
        scope: &RelationshipScope<'_>,
        relationships: &mut Vec<Relationship>,
    ) {
        let Some(param_decl) = node.child_by_field_name("receiver").and_then(|receiver| {
            receiver
                .named_children(&mut receiver.walk())
                .find(|child| child.kind() == "parameter_declaration")
        }) else {
            return;
        };
        let receiver_type = self.extract_receiver_type_from_param(param_decl);
        let receiver_symbol = scope.symbols.iter().find(|symbol| {
            symbol.name == receiver_type
                && matches!(
                    symbol.kind,
                    SymbolKind::Struct | SymbolKind::Interface | SymbolKind::Type
                )
        });
        let method_symbol = node
            .child_by_field_name("name")
            .and_then(|name| scope.containers.find(name))
            .filter(|symbol| symbol.kind == SymbolKind::Method);

        if let (Some(receiver_sym), Some(method_sym)) = (receiver_symbol, method_symbol) {
            relationships.push(self.base.create_relationship(
                method_sym.id.clone(),
                receiver_sym.id.clone(),
                RelationshipKind::Uses,
                &node,
                Some(0.9),
                None,
            ));
        }
    }

    /// Extract function call relationships
    ///
    /// Creates resolved Relationship when target is a local function/method.
    /// Creates PendingRelationship when target is:
    /// - An Import symbol (needs cross-file resolution)
    /// - Not found in local symbol_map (e.g., method on imported package)
    fn extract_call_relationships(
        &mut self,
        node: Node,
        scope: &RelationshipScope<'_>,
        relationships: &mut Vec<Relationship>,
    ) {
        let symbol_map = &scope.symbol_map;
        let Some(func_node) = callee_node(node) else {
            return;
        };
        let mut target = match func_node.kind() {
            "identifier" | "type_identifier" => {
                UnresolvedTarget::simple(self.base.get_node_text(&func_node))
            }
            "selector_expression" => match self.selector_call_target(func_node) {
                Some(target) => target,
                None => return,
            },
            "qualified_type" => {
                let (Some(package), Some(name)) = (
                    func_node.child_by_field_name("package"),
                    func_node.child_by_field_name("name"),
                ) else {
                    return;
                };
                UnresolvedTarget::from_chain(vec![
                    self.base.get_node_text(&package),
                    self.base.get_node_text(&name),
                ])
            }
            _ => return,
        };
        let callee_name = target.terminal_name.clone();
        if target.receiver.is_none() && is_predeclared_callee(&callee_name) {
            return;
        }

        let Some(caller) = self.find_caller(scope, node) else {
            return;
        };

        // Receiver-qualified calls (pkg.fn, obj.method) should not resolve to a local
        // symbol by terminal name alone.
        if let Some(receiver) = target.receiver.as_deref() {
            let import_path = symbol_map
                .get(receiver)
                .filter(|symbol| symbol.kind == SymbolKind::Import)
                .and_then(|symbol| symbol.signature.as_deref())
                .and_then(import_path_from_signature)
                .map(str::to_owned);

            if import_path.as_deref().is_some_and(is_stdlib_import_path) {
                return;
            }
            target.import_context = import_path;

            let pending = self
                .base
                .create_pending_relationship(
                    caller.id.clone(),
                    target,
                    RelationshipKind::Calls,
                    &node,
                    Some(caller.id.clone()),
                    Some(0.7),
                )
                .with_receiver_type(self.method_self_receiver_type(func_node));
            self.add_structured_pending_relationship(pending);
            return;
        }

        // A Ginkgo node call (`BeforeEach(...)`) is itself a symbol named after
        // its callee; it must not resolve to the symbol it declares.
        let callee = symbol_map
            .get(&callee_name)
            .filter(|symbol| symbol.start_byte != node.start_byte() as u32);
        match callee {
            Some(called_symbol) if called_symbol.kind == SymbolKind::Import => {
                let pending = self.base.create_pending_relationship(
                    caller.id.clone(),
                    target,
                    RelationshipKind::Calls,
                    &node,
                    Some(caller.id.clone()),
                    Some(0.8),
                );
                self.add_structured_pending_relationship(pending);
            }
            Some(called_symbol) => {
                let kind = if is_type_symbol(called_symbol) {
                    RelationshipKind::Uses
                } else {
                    RelationshipKind::Calls
                };
                relationships.push(self.base.create_relationship(
                    caller.id.clone(),
                    called_symbol.id.clone(),
                    kind,
                    &node,
                    Some(0.9),
                    None,
                ));
            }
            None => {
                let pending = self.base.create_pending_relationship(
                    caller.id.clone(),
                    target,
                    RelationshipKind::Calls,
                    &node,
                    Some(caller.id.clone()),
                    Some(0.7),
                );
                self.add_structured_pending_relationship(pending);
            }
        }
    }

    /// `type T struct { Base; *pkg.Mixin }` and `type I interface { Reader;
    /// io.Closer }`: each embedded type is an `extends` edge from `T`, resolved
    /// to a same-file type or left pending with its package qualifier.
    fn extract_embedding_relationships(
        &mut self,
        type_spec: Node,
        scope: &RelationshipScope<'_>,
        relationships: &mut Vec<Relationship>,
    ) {
        let (Some(name), Some(body)) = (
            type_spec.child_by_field_name("name"),
            type_spec.child_by_field_name("type"),
        ) else {
            return;
        };
        let Some(owner) = self.type_symbol_at(scope, type_spec, name) else {
            return;
        };
        let embedded: Vec<Node> = match body.kind() {
            "struct_type" => body
                .named_children(&mut body.walk())
                .filter(|list| list.kind() == "field_declaration_list" && !list.has_error())
                .flat_map(|list| list.named_children(&mut list.walk()).collect::<Vec<_>>())
                .filter(|field| {
                    field.kind() == "field_declaration"
                        && field.child_by_field_name("name").is_none()
                })
                .filter_map(|field| field.child_by_field_name("type"))
                .collect(),
            "interface_type" => body
                .named_children(&mut body.walk())
                .filter(|element| element.kind() == "type_elem" && element.named_child_count() == 1)
                .filter_map(|element| element.named_child(0))
                .collect(),
            _ => return,
        };
        for type_node in embedded {
            self.emit_type_edge(
                owner,
                type_node,
                RelationshipKind::Extends,
                scope,
                relationships,
            );
        }
    }

    /// `var _ Iface = T{}` / `var _ Iface = (*T)(nil)`: the compile-time proof
    /// that `T` implements `Iface`.
    fn extract_interface_assertion(
        &mut self,
        var_spec: Node,
        scope: &RelationshipScope<'_>,
        relationships: &mut Vec<Relationship>,
    ) {
        let is_blank = var_spec
            .child_by_field_name("name")
            .is_some_and(|name| self.base.get_node_text(&name) == "_");
        let (Some(interface), Some(value)) = (
            var_spec.child_by_field_name("type"),
            var_spec
                .child_by_field_name("value")
                .and_then(|values| values.named_child(0)),
        ) else {
            return;
        };
        if !is_blank {
            return;
        }
        let Some(implementer) = asserted_type_name(value) else {
            return;
        };
        let implementer = self.base.get_node_text(&implementer);
        let Some(owner) = scope.symbols.iter().find(|symbol| {
            symbol.name == implementer
                && matches!(symbol.kind, SymbolKind::Struct | SymbolKind::Type)
        }) else {
            return;
        };
        self.emit_type_edge(
            owner,
            interface,
            RelationshipKind::Implements,
            scope,
            relationships,
        );
    }

    fn type_symbol_at<'a>(
        &self,
        scope: &RelationshipScope<'a>,
        type_spec: Node,
        name: Node,
    ) -> Option<&'a Symbol> {
        let name = self.base.get_node_text(&name);
        scope.symbols.iter().find(|symbol| {
            symbol.name == name
                && symbol.start_byte == type_spec.start_byte() as u32
                && matches!(
                    symbol.kind,
                    SymbolKind::Struct | SymbolKind::Interface | SymbolKind::Type
                )
        })
    }

    /// A resolved edge to a same-file type named by `type_node`, or a pending
    /// edge that keeps its package qualifier.
    fn emit_type_edge(
        &mut self,
        from: &Symbol,
        type_node: Node,
        kind: RelationshipKind,
        scope: &RelationshipScope<'_>,
        relationships: &mut Vec<Relationship>,
    ) {
        let mut named = type_node;
        loop {
            match named.kind() {
                "pointer_type" => match named.named_child(0) {
                    Some(inner) => named = inner,
                    None => return,
                },
                "generic_type" => match named.child_by_field_name("type") {
                    Some(inner) => named = inner,
                    None => return,
                },
                _ => break,
            }
        }
        let target = match named.kind() {
            "type_identifier" => {
                let name = self.base.get_node_text(&named);
                let local = scope.symbols.iter().find(|symbol| {
                    symbol.name == name
                        && symbol.id != from.id
                        && matches!(
                            symbol.kind,
                            SymbolKind::Struct | SymbolKind::Interface | SymbolKind::Type
                        )
                });
                if let Some(local) = local {
                    relationships.push(self.base.create_relationship(
                        from.id.clone(),
                        local.id.clone(),
                        kind,
                        &type_node,
                        Some(0.9),
                        None,
                    ));
                    return;
                }
                UnresolvedTarget::simple(name)
            }
            "qualified_type" => {
                let (Some(package), Some(name)) = (
                    named.child_by_field_name("package"),
                    named.child_by_field_name("name"),
                ) else {
                    return;
                };
                let package = self.base.get_node_text(&package);
                let mut target = UnresolvedTarget::from_chain(vec![
                    package.clone(),
                    self.base.get_node_text(&name),
                ]);
                target.import_context = scope
                    .symbol_map
                    .get(&package)
                    .filter(|symbol| symbol.kind == SymbolKind::Import)
                    .and_then(|symbol| symbol.signature.as_deref())
                    .and_then(import_path_from_signature)
                    .map(str::to_owned);
                target
            }
            _ => return,
        };
        let pending = self.base.create_pending_relationship(
            from.id.clone(),
            target,
            kind,
            &type_node,
            Some(from.id.clone()),
            Some(0.8),
        );
        self.add_structured_pending_relationship(pending);
    }

    /// `a.b.F()` becomes the chain target `a.b.F`. A call on an expression
    /// result (`x.F().G()`, `xs[0].G()`) keeps the operand text as its receiver
    /// so it never resolves to a same-named local symbol.
    fn selector_call_target(&self, selector: Node) -> Option<UnresolvedTarget> {
        let mut parts = Vec::new();
        let mut current = selector;
        loop {
            match current.kind() {
                "identifier" => {
                    parts.push(self.base.get_node_text(&current));
                    break;
                }
                "selector_expression" => {
                    parts.push(
                        self.base
                            .get_node_text(&current.child_by_field_name("field")?),
                    );
                    current = current.child_by_field_name("operand")?;
                }
                _ => {
                    let field = self
                        .base
                        .get_node_text(&selector.child_by_field_name("field")?);
                    let receiver = self
                        .base
                        .get_node_text(&selector.child_by_field_name("operand")?);
                    return Some(UnresolvedTarget {
                        display_name: format!("{receiver}.{field}"),
                        terminal_name: field,
                        receiver: Some(receiver),
                        namespace_path: Vec::new(),
                        import_context: None,
                    });
                }
            }
        }
        parts.reverse();
        Some(UnresolvedTarget::from_chain(parts))
    }

    /// The innermost symbol enclosing a call. A call that is itself a symbol
    /// (a Ginkgo node or a `t.Run` subtest) belongs to its enclosing symbol.
    fn find_caller<'a>(&self, scope: &RelationshipScope<'a>, call: Node) -> Option<&'a Symbol> {
        let found = scope.containers.find(call)?;
        if found.start_byte != call.start_byte() as u32 {
            return Some(found);
        }
        crate::base::BaseExtractor::find_containing_symbol_from_iter(
            &call,
            scope
                .symbols
                .iter()
                .filter(|symbol| symbol.id != found.id && symbol.file_path == found.file_path),
        )
    }
}

/// The callee of a call: `F(x)`, `pkg.F(x)`, or the base of an explicit
/// instantiation `F[T](x)` (which the grammar reads as a type conversion).
fn callee_node(node: Node) -> Option<Node> {
    let function = match node.kind() {
        "call_expression" => node.child_by_field_name("function")?,
        "type_conversion_expression" => node.child_by_field_name("type")?,
        _ => return None,
    };
    match function.kind() {
        "generic_type" => function.child_by_field_name("type"),
        "index_expression" => function.child_by_field_name("operand"),
        _ => Some(function),
    }
}

/// Go's predeclared functions and conversion types: a call to one is never a
/// workspace edge unless the file shadows the name.
fn is_predeclared_callee(name: &str) -> bool {
    matches!(
        name,
        "append"
            | "cap"
            | "clear"
            | "close"
            | "complex"
            | "copy"
            | "delete"
            | "imag"
            | "len"
            | "make"
            | "max"
            | "min"
            | "new"
            | "panic"
            | "print"
            | "println"
            | "real"
            | "recover"
            | "any"
            | "bool"
            | "byte"
            | "complex64"
            | "complex128"
            | "error"
            | "float32"
            | "float64"
            | "int"
            | "int8"
            | "int16"
            | "int32"
            | "int64"
            | "rune"
            | "string"
            | "uint"
            | "uint8"
            | "uint16"
            | "uint32"
            | "uint64"
            | "uintptr"
    )
}

fn is_type_symbol(symbol: &Symbol) -> bool {
    matches!(
        symbol.kind,
        SymbolKind::Struct | SymbolKind::Interface | SymbolKind::Type
    )
}

/// The implementing type named by an assertion value: `T{}`, `&T{}`,
/// `(*T)(nil)`, or `T(nil)`.
fn asserted_type_name(value: Node) -> Option<Node> {
    if let Some(literal_type) = super::type_facts::composite_literal_type_node(value) {
        return (literal_type.kind() == "type_identifier").then_some(literal_type);
    }
    if value.kind() != "call_expression" {
        return None;
    }
    let mut function = value.child_by_field_name("function")?;
    loop {
        match function.kind() {
            "parenthesized_expression" | "parenthesized_type" => {
                function = function.named_child(0)?;
            }
            "unary_expression" => function = function.child_by_field_name("operand")?,
            "pointer_type" => function = function.named_child(0)?,
            "identifier" | "type_identifier" => return Some(function),
            _ => return None,
        }
    }
}
