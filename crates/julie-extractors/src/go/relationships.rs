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

        if node.kind() == "call_expression" {
            self.extract_call_relationships(node, scope, relationships);
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
        let Some(func_node) = node.child_by_field_name("function") else {
            return;
        };
        let mut target = match func_node.kind() {
            "identifier" => UnresolvedTarget::simple(self.base.get_node_text(&func_node)),
            "selector_expression" => match self.selector_call_target(func_node) {
                Some(target) => target,
                None => return,
            },
            _ => return,
        };
        let callee_name = target.terminal_name.clone();

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
                relationships.push(self.base.create_relationship(
                    caller.id.clone(),
                    called_symbol.id.clone(),
                    RelationshipKind::Calls,
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
