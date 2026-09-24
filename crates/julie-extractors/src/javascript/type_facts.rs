//! Declared-type fact recording for JavaScript.

use crate::base::types::TypeNameRules;
use crate::base::{BaseExtractor, Symbol, SymbolKind};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;
use tree_sitter::Node;

pub(crate) const TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &[],
    generic_open: &['<'],
};

/// Record an inferred type fact for `symbol_id` when `value_node` is a plain
/// `new Identifier(...)` expression. Qualified constructors such as
/// `new ns.Foo()` record nothing.
pub(crate) fn record_new_expression_fact(
    base: &mut BaseExtractor,
    symbol_id: &str,
    value_node: Node,
    rules: &TypeNameRules,
) {
    if value_node.kind() != "new_expression" {
        return;
    }
    let Some(constructor) = value_node.child_by_field_name("constructor") else {
        return;
    };
    if constructor.kind() != "identifier" {
        return;
    }
    let declared = base.get_node_text(&constructor);
    base.record_declared_type_fact(symbol_id, &declared, rules, true);
}

/// JSDoc type text decorations: `?T`/`!T` nullability, `T=` optional
/// parameters, `...T` rest parameters, and generic arguments.
const JSDOC_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &["=", "?"],
    reference_prefixes: &["...", "?", "!"],
    generic_open: &['<'],
};

static JSDOC_RETURNS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"@returns?\s*\{([^}]+)\}").unwrap());
static JSDOC_TYPE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"@type\s*\{([^}]+)\}").unwrap());
static JSDOC_PARAM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"@(?:param|arg|argument)\s*\{([^}]+)\}\s*\[?([A-Za-z_$][\w$]*)").unwrap()
});
static JSDOC_DETACHED_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"@(?:callback|typedef)\b").unwrap());
static JSDOC_OVERLOAD_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"@overload\b").unwrap());

/// The JSDoc block that documents a declaration: the last `/** */` block of
/// its joined doc text. A block that defines a `@callback` or `@typedef`
/// documents that type instead, and `@overload` blocks make the call's type
/// depend on its arguments, so both yield nothing.
fn own_jsdoc(doc: &str) -> Option<&str> {
    if JSDOC_OVERLOAD_RE.is_match(doc) {
        return None;
    }
    let block = &doc[doc.rfind("/**")?..];
    let block = block.find("*/").map_or(block, |end| &block[..end + 2]);
    (!JSDOC_DETACHED_RE.is_match(block)).then_some(block)
}

/// Record the JSDoc-declared type of a symbol: `@returns {T}` for callables,
/// `@type {T}` for variables, properties, and fields.
pub(crate) fn record_jsdoc_symbol_fact(base: &mut BaseExtractor, symbol: &Symbol) {
    let Some(doc) = symbol.doc_comment.as_deref().and_then(own_jsdoc) else {
        return;
    };
    let pattern = match symbol.kind {
        SymbolKind::Function | SymbolKind::Method => &*JSDOC_RETURNS_RE,
        SymbolKind::Variable | SymbolKind::Property | SymbolKind::Field => &*JSDOC_TYPE_RE,
        _ => return,
    };
    let Some(declared) = pattern
        .captures(doc)
        .map(|captures| captures[1].trim().to_string())
    else {
        return;
    };
    if matches!(declared.as_str(), "void" | "undefined" | "*") {
        return;
    }
    base.record_declared_type_fact(&symbol.id, &declared, &JSDOC_TYPE_NAME_RULES, false);
}

/// Record a parameter's type from its callable's `@param {T} name` tag.
pub(crate) fn record_jsdoc_param_fact(
    base: &mut BaseExtractor,
    parameter: &Symbol,
    callable_doc: Option<&str>,
) {
    let Some(doc) = callable_doc.and_then(own_jsdoc) else {
        return;
    };
    let Some(declared) = JSDOC_PARAM_RE
        .captures_iter(doc)
        .find(|captures| captures[2] == *parameter.name)
        .map(|captures| captures[1].trim().to_string())
    else {
        return;
    };
    if declared == "*" {
        return;
    }
    base.record_declared_type_fact(&parameter.id, &declared, &JSDOC_TYPE_NAME_RULES, false);
}

static JSDOC_TEMPLATE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"@template\s+(?:\{[^}]*\}\s*)?\[?([A-Za-z_$][\w$]*(?:\s*,\s*[A-Za-z_$][\w$]*)*)")
        .unwrap()
});
static JSDOC_TYPE_NAME_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z_$][\w$]*(?:\.[A-Za-z_$][\w$]*)*$").unwrap());

/// A JSDoc type reduced to what initializer inference needs: the bindable
/// base name (`None` for a `@template` parameter), the written text, and the
/// type arguments of a generic type.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TypeShape {
    name: Option<String>,
    declared: String,
    args: Vec<TypeShape>,
}

impl TypeShape {
    fn awaited(self) -> Option<TypeShape> {
        (self.name.as_deref() == Some("Promise"))
            .then(|| self.args.into_iter().next())
            .flatten()
    }
}

/// Parse a JSDoc type written as `Name`, `?Name`, `!Name`, `ns.Name`,
/// `Name<Args>`, or `Name.<Args>`. Unions, records, arrays, function types,
/// and the any/void family yield nothing.
fn jsdoc_type_shape(text: &str, templates: &[String], depth: u32) -> Option<TypeShape> {
    if !should_visit_tree_depth(depth) {
        return None;
    }
    let declared = text.trim();
    let core = declared
        .trim_start_matches(['?', '!'])
        .trim_end_matches('?')
        .trim();
    let (head, args) = match core.split_once('<') {
        Some((head, rest)) => {
            let inner = rest.strip_suffix('>')?;
            let child_depth = child_tree_depth(depth)?;
            let args = split_type_arguments(inner)
                .into_iter()
                .map(|arg| jsdoc_type_shape(arg, templates, child_depth))
                .collect::<Option<Vec<_>>>()?;
            (head.trim_end().trim_end_matches('.'), args)
        }
        None => (core, Vec::new()),
    };
    if !JSDOC_TYPE_NAME_RE.is_match(head)
        || matches!(
            head,
            "any" | "unknown" | "void" | "undefined" | "null" | "never" | "mixed" | "this"
        )
    {
        return None;
    }
    Some(TypeShape {
        name: (!templates.iter().any(|template| template == head)).then(|| head.to_string()),
        declared: declared.to_string(),
        args,
    })
}

fn split_type_arguments(inner: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut nesting = 0i32;
    let mut start = 0;
    for (index, ch) in inner.char_indices() {
        match ch {
            '<' | '(' | '{' | '[' => nesting += 1,
            '>' | ')' | '}' | ']' => nesting -= 1,
            ',' if nesting == 0 => {
                parts.push(&inner[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(&inner[start..]);
    parts
}

fn template_names(doc: &str) -> impl Iterator<Item = String> + '_ {
    JSDOC_TEMPLATE_RE.captures_iter(doc).flat_map(|captures| {
        captures[1]
            .split(',')
            .map(|name| name.trim().to_string())
            .collect::<Vec<_>>()
    })
}

fn metadata_flag(symbol: &Symbol, key: &str) -> bool {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

/// Kinds that bind a name in lexical scope. Class members are reached only
/// through `this` or a class name, never by a bare call.
fn is_lexical_binding(kind: &SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Function
            | SymbolKind::Variable
            | SymbolKind::Constant
            | SymbolKind::Class
            | SymbolKind::Import
    )
}

/// Record an inferred type fact for each variable whose initializer is
/// `new Name(..)` or a call to a same-file function or class method with a
/// JSDoc `@returns` type. Runs after the symbol walk, so a written `@type`
/// wins and functions declared after the call are known.
pub(crate) fn record_initializer_facts(base: &mut BaseExtractor, root: Node, symbols: &[Symbol]) {
    let scope = InitializerScope::new(base, root, symbols);
    let facts: Vec<(String, TypeShape)> = symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Variable)
        .filter_map(|symbol| {
            let declarator = root
                .descendant_for_byte_range(symbol.start_byte as usize, symbol.end_byte as usize)
                .filter(|node| node.kind() == "variable_declarator")?;
            declarator
                .child_by_field_name("name")
                .filter(|name| name.kind() == "identifier")?;
            let value = declarator.child_by_field_name("value")?;
            let shape = scope.shape_of(value, 0)?.shape;
            shape.name.is_some().then(|| (symbol.id.clone(), shape))
        })
        .collect();
    for (symbol_id, shape) in facts {
        let name = shape.name.unwrap_or_default();
        base.record_declared_type_fact_with_declared(
            &symbol_id,
            &name,
            &shape.declared,
            &JSDOC_TYPE_NAME_RULES,
            true,
        );
    }
}

struct InitializerScope<'a> {
    base: &'a BaseExtractor,
    root: Node<'a>,
    by_id: HashMap<&'a str, &'a Symbol>,
    by_name: HashMap<&'a str, Vec<&'a Symbol>>,
    var_loops: Vec<VarLoopHead<'a>>,
}

/// The type an expression evaluates to, with the callables whose `@returns`
/// declared it (none for `new Name()`).
struct Typed<'a> {
    shape: TypeShape,
    sources: Vec<&'a Symbol>,
}

/// A receiver that names a same-file class: its instances, or the class
/// itself for static members.
struct ClassReceiver<'a> {
    class: &'a Symbol,
    is_static: bool,
}

impl<'a> InitializerScope<'a> {
    fn new(base: &'a BaseExtractor, root: Node<'a>, symbols: &'a [Symbol]) -> Self {
        let mut by_name: HashMap<&str, Vec<&Symbol>> = HashMap::new();
        for symbol in symbols {
            by_name
                .entry(symbol.name.as_str())
                .or_default()
                .push(symbol);
        }
        Self {
            base,
            root,
            by_id: symbols
                .iter()
                .map(|symbol| (symbol.id.as_str(), symbol))
                .collect(),
            by_name,
            var_loops: var_loop_heads(root),
        }
    }

    fn shape_of(&self, value: Node, depth: u32) -> Option<Typed<'a>> {
        if !should_visit_tree_depth(depth) {
            return None;
        }
        match value.kind() {
            "parenthesized_expression" if !is_jsdoc_cast(self.base, value) => {
                self.shape_of(value.named_child(0)?, child_tree_depth(depth)?)
            }
            "await_expression" => {
                let typed = self.shape_of(value.named_child(0)?, child_tree_depth(depth)?)?;
                Some(Typed {
                    shape: typed.shape.awaited()?,
                    sources: typed.sources,
                })
            }
            "new_expression" => {
                let constructor = value
                    .child_by_field_name("constructor")
                    .filter(|constructor| constructor.kind() == "identifier")?;
                let name = self.base.get_node_text(&constructor);
                Some(Typed {
                    shape: TypeShape {
                        name: Some(name.clone()),
                        declared: name,
                        args: Vec::new(),
                    },
                    sources: Vec::new(),
                })
            }
            "call_expression" if !has_optional_chain(value) => {
                let function = value.child_by_field_name("function")?;
                match function.kind() {
                    "identifier" => self.free_call(value, &self.base.get_node_text(&function)),
                    "member_expression" if !has_optional_chain(function) => {
                        self.member_call(function, child_tree_depth(depth)?)
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn free_call(&self, call: Node, name: &str) -> Option<Typed<'a>> {
        let candidates = self.visible(name, call);
        if candidates.iter().any(|candidate| {
            candidate.kind != SymbolKind::Function || !self.declares_binding(candidate)
        }) {
            return None;
        }
        self.unanimous_return(candidates)
    }

    fn member_call(&self, function: Node, depth: u32) -> Option<Typed<'a>> {
        let object = function.child_by_field_name("object")?;
        let method = self
            .base
            .get_node_text(&function.child_by_field_name("property")?);
        let receiver = self.receiver(object, depth)?;
        let candidates: Vec<&'a Symbol> = self
            .by_name
            .get(method.as_str())?
            .iter()
            .copied()
            .filter(|member| member.parent_id.as_deref() == Some(receiver.class.id.as_str()))
            .collect();
        if candidates.iter().any(|member| {
            member.kind != SymbolKind::Method
                || metadata_flag(member, "isGetter")
                || metadata_flag(member, "isSetter")
                || metadata_flag(member, "isStatic") != receiver.is_static
        }) {
            return None;
        }
        self.unanimous_return(candidates)
    }

    fn receiver(&self, object: Node, depth: u32) -> Option<ClassReceiver<'a>> {
        match object.kind() {
            "this" => self.this_receiver(object),
            "identifier" => Some(ClassReceiver {
                class: self.class_named(&self.base.get_node_text(&object), object)?,
                is_static: true,
            }),
            _ => {
                let typed = self.shape_of(object, depth)?;
                let name = typed.shape.name.as_deref()?;
                let class = self.class_named(name, object)?;
                typed
                    .sources
                    .iter()
                    .all(|source| {
                        self.declaration_node(source)
                            .and_then(|declaration| self.class_named(name, declaration))
                            .is_some_and(|declared| declared.id == class.id)
                    })
                    .then_some(ClassReceiver {
                        class,
                        is_static: false,
                    })
            }
        }
    }

    /// The class `this` denotes: the nearest enclosing class method or field,
    /// seen through arrow functions. Any other function rebinds `this`, and a
    /// computed member name or a decorator runs in the scope around the class.
    fn this_receiver(&self, node: Node) -> Option<ClassReceiver<'a>> {
        let mut current = node.parent();
        while let Some(candidate) = current {
            match candidate.kind() {
                "method_definition"
                | "field_definition"
                | "public_field_definition"
                | "class_static_block" => {
                    let class = candidate
                        .parent()
                        .filter(|body| body.kind() == "class_body")?
                        .parent()?;
                    let class = self.class_at(class)?;
                    let is_static = candidate.kind() == "class_static_block"
                        || candidate
                            .children(&mut candidate.walk())
                            .any(|child| child.kind() == "static");
                    return Some(ClassReceiver { class, is_static });
                }
                "function_declaration"
                | "function_expression"
                | "function"
                | "generator_function"
                | "generator_function_declaration"
                | "class_body"
                | "computed_property_name"
                | "decorator"
                | "program" => return None,
                _ => {}
            }
            current = candidate.parent();
        }
        None
    }

    fn class_at(&self, class: Node) -> Option<&'a Symbol> {
        self.by_id.values().copied().find(|symbol| {
            symbol.kind == SymbolKind::Class && symbol.start_byte == class.start_byte() as u32
        })
    }

    fn class_named(&self, name: &str, at: Node) -> Option<&'a Symbol> {
        match self.visible(name, at).as_slice() {
            [class] if class.kind == SymbolKind::Class && self.declares_binding(class) => {
                Some(*class)
            }
            _ => None,
        }
    }

    /// Lexical bindings named `name` whose declaring scope contains `at`, or
    /// none when a parameter, loop, catch, or named-expression binding of
    /// `name` shadows `at`, or when a same-named function declaration is
    /// block-scoped in strict code but hoisted in sloppy code at `at`.
    fn visible(&self, name: &str, at: Node) -> Vec<&'a Symbol> {
        if pattern_binding_shadows(self.base, &self.var_loops, name, at) {
            return Vec::new();
        }
        let mut visible = Vec::new();
        for symbol in self
            .by_name
            .get(name)
            .into_iter()
            .flatten()
            .copied()
            .filter(|symbol| is_lexical_binding(&symbol.kind))
        {
            let Some((block, function)) = self.declaration_node(symbol).map(binding_scopes) else {
                return Vec::new();
            };
            if encloses(block, at) {
                visible.push(symbol);
            } else if encloses(function, at) {
                return Vec::new();
            }
        }
        visible
    }

    fn declaration_node(&self, symbol: &Symbol) -> Option<Node<'a>> {
        self.root
            .descendant_for_byte_range(symbol.start_byte as usize, symbol.end_byte as usize)
    }

    /// Whether the declaration behind `symbol` binds its name in lexical
    /// scope. `exports.load = function () {}` and the inner name of a class
    /// expression bind nothing a bare reference can see.
    fn declares_binding(&self, symbol: &Symbol) -> bool {
        let Some(node) = self.declaration_node(symbol) else {
            return false;
        };
        match node.kind() {
            "function_declaration" | "generator_function_declaration" | "class_declaration" => true,
            "arrow_function" | "function_expression" | "generator_function" | "class" => node
                .parent()
                .filter(|parent| parent.kind() == "variable_declarator")
                .and_then(|declarator| declarator.child_by_field_name("name"))
                .is_some_and(|name| {
                    name.kind() == "identifier" && self.base.get_node_text(&name) == symbol.name
                }),
            _ => false,
        }
    }

    fn unanimous_return(&self, candidates: Vec<&'a Symbol>) -> Option<Typed<'a>> {
        let first = self.declared_return(candidates.first()?)?;
        candidates[1..]
            .iter()
            .all(|candidate| self.declared_return(candidate).as_ref() == Some(&first))
            .then_some(Typed {
                shape: first,
                sources: candidates,
            })
    }

    /// The `@returns` type every return tag of `callable` agrees on, with the
    /// `@template` names of it and its enclosing declarations left unbound.
    /// An async callable must declare a `Promise`, and a generator its
    /// generator or iterator type.
    fn declared_return(&self, callable: &Symbol) -> Option<TypeShape> {
        let doc = callable.doc_comment.as_deref().and_then(own_jsdoc)?;
        let mut templates = Vec::new();
        let mut owner = Some(callable);
        while let Some(symbol) = owner {
            if let Some(owner_doc) = symbol.doc_comment.as_deref() {
                templates.extend(template_names(owner_doc));
            }
            owner = symbol
                .parent_id
                .as_deref()
                .and_then(|parent| self.by_id.get(parent).copied());
        }
        let mut shapes = JSDOC_RETURNS_RE
            .captures_iter(doc)
            .map(|captures| jsdoc_type_shape(&captures[1], &templates, 0));
        let first = shapes.next()??;
        if !shapes.all(|shape| shape.as_ref() == Some(&first)) {
            return None;
        }
        let is_async = metadata_flag(callable, "isAsync");
        let allowed: &[&str] = match (metadata_flag(callable, "isGenerator"), is_async) {
            (true, false) => &["Generator", "Iterator", "IterableIterator", "Iterable"],
            (true, true) => &[
                "AsyncGenerator",
                "AsyncIterator",
                "AsyncIterableIterator",
                "AsyncIterable",
            ],
            (false, true) => &["Promise"],
            (false, false) => return Some(first),
        };
        allowed.contains(&first.name.as_deref()?).then_some(first)
    }
}

/// The block that scopes a declaration and the function that scopes it when
/// hoisted: `var` binds in the function, `let`, `const`, `class`, and
/// imports in the block, a function declaration in the block for strict
/// code but in the function for sloppy code, and a parameter in its function.
fn binding_scopes(declaration: Node) -> (Node, Node) {
    let mut current = Some(declaration);
    while let Some(node) = current {
        match node.kind() {
            "variable_declaration" => {
                let function = function_scope(node);
                return (function, function);
            }
            "function_declaration" | "generator_function_declaration" => {
                return (block_scope(node), function_scope(node));
            }
            "formal_parameters" => {
                let function = node.parent().unwrap_or(node);
                return (function, function);
            }
            "lexical_declaration" | "class_declaration" | "import_statement" => break,
            kind if is_block_scope(kind) => break,
            _ => current = node.parent(),
        }
    }
    let block = block_scope(declaration);
    (block, block)
}

fn is_block_scope(kind: &str) -> bool {
    matches!(
        kind,
        "statement_block" | "program" | "switch_body" | "for_statement" | "for_in_statement"
    )
}

fn block_scope(declaration: Node) -> Node {
    enclosing_scope(declaration, is_block_scope)
}

fn function_scope(declaration: Node) -> Node {
    enclosing_scope(declaration, |kind| {
        matches!(
            kind,
            "program"
                | "function_declaration"
                | "generator_function_declaration"
                | "function_expression"
                | "generator_function"
                | "arrow_function"
                | "method_definition"
                | "class_static_block"
        )
    })
}

fn enclosing_scope(declaration: Node, is_scope: impl Fn(&str) -> bool) -> Node {
    let mut current = declaration;
    while let Some(parent) = current.parent() {
        if is_scope(parent.kind()) {
            return parent;
        }
        current = parent;
    }
    current
}

fn encloses(scope: Node, at: Node) -> bool {
    scope.start_byte() <= at.start_byte() && at.end_byte() <= scope.end_byte()
}

/// A `for (var .. in/of ..)` head's binding pattern and the function its
/// `var` hoists to. The binding covers that whole function, not only the loop.
pub(crate) struct VarLoopHead<'a> {
    pattern: Node<'a>,
    function: Node<'a>,
}

/// Every `var` loop head in the tree. Iterative, so no depth budget hides one.
pub(crate) fn var_loop_heads(root: Node) -> Vec<VarLoopHead> {
    let mut heads = vec![];
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "for_in_statement"
            && node
                .child_by_field_name("kind")
                .is_some_and(|kind| kind.kind() == "var")
            && let Some(pattern) = node.child_by_field_name("left")
        {
            heads.push(VarLoopHead {
                pattern,
                function: function_scope(node),
            });
        }
        stack.extend(node.named_children(&mut node.walk()));
    }
    heads
}

/// Whether a parameter, loop, or catch binding of `name`, or the own name of
/// a named function expression, shadows `at`, so a bare `name` there cannot
/// mean a declaration outside it. A `var` loop head shadows its whole
/// function, even when the loop comes before or after `at`.
pub(crate) fn pattern_binding_shadows(
    base: &BaseExtractor,
    var_loops: &[VarLoopHead],
    name: &str,
    at: Node,
) -> bool {
    if var_loops
        .iter()
        .any(|head| encloses(head.function, at) && pattern_mentions(base, head.pattern, name, 0))
    {
        return true;
    }
    let mut current = at.parent();
    while let Some(scope) = current {
        if matches!(scope.kind(), "function_expression" | "generator_function")
            && scope
                .child_by_field_name("name")
                .is_some_and(|own_name| base.get_node_text(&own_name) == name)
        {
            return true;
        }
        let patterns = match scope.kind() {
            "for_in_statement" => [scope.child_by_field_name("left"), None],
            "catch_clause" => [scope.child_by_field_name("parameter"), None],
            _ => [
                scope.child_by_field_name("parameters"),
                scope.child_by_field_name("parameter"),
            ],
        };
        if patterns
            .into_iter()
            .flatten()
            .any(|pattern| pattern_mentions(base, pattern, name, 0))
        {
            return true;
        }
        current = scope.parent();
    }
    false
}

/// Whether `pattern` names `name` anywhere. A default value that reads
/// `name` counts too, which only ever withholds a fact.
fn pattern_mentions(base: &BaseExtractor, pattern: Node, name: &str, depth: u32) -> bool {
    if !should_visit_tree_depth(depth) {
        return true;
    }
    if matches!(
        pattern.kind(),
        "identifier" | "shorthand_property_identifier_pattern"
    ) && base.get_node_text(&pattern) == name
    {
        return true;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return true;
    };
    pattern
        .named_children(&mut pattern.walk())
        .any(|child| pattern_mentions(base, child, name, child_depth))
}

/// Whether a JSDoc `@type` comment directly precedes `node`, which makes a
/// parenthesized expression a type cast whose written type the call cannot
/// know.
fn is_jsdoc_cast(base: &BaseExtractor, node: Node) -> bool {
    let mut current = node;
    loop {
        if let Some(previous) = current.prev_sibling() {
            return previous.kind() == "comment"
                && JSDOC_TYPE_RE.is_match(&base.get_node_text(&previous));
        }
        let Some(parent) = current.parent() else {
            return false;
        };
        current = parent;
    }
}

fn has_optional_chain(node: Node) -> bool {
    node.children(&mut node.walk())
        .any(|child| child.kind() == "optional_chain")
}
