//! Declared-type fact recording for Kotlin.

use super::helpers;
use crate::base::types::{TypeNameRules, strip_type_decorations};
use crate::base::{BaseExtractor, Symbol};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

pub(super) const KOTLIN_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &["?"],
    reference_prefixes: &[],
    generic_open: &['<'],
};

const DECLARED_TYPE_METADATA_KEYS: [&str; 3] = ["returnType", "propertyType", "dataType"];

/// Base type names for symbols whose metadata carries declared type text.
/// Shapes with no single base name (function types, backticked names with
/// spaces) record nothing.
pub(super) fn metadata_base_types(symbols: &[Symbol]) -> HashMap<String, String> {
    symbols
        .iter()
        .filter_map(|symbol| {
            let declared = declared_type_metadata(symbol)?;
            Some((symbol.id.clone(), base_type_name_from_text(declared)?))
        })
        .collect()
}

fn declared_type_metadata(symbol: &Symbol) -> Option<&str> {
    let metadata = symbol.metadata.as_ref()?;
    DECLARED_TYPE_METADATA_KEYS
        .iter()
        .find_map(|key| metadata.get(*key).and_then(serde_json::Value::as_str))
}

fn base_type_name_from_text(declared: &str) -> Option<String> {
    let stripped = strip_type_decorations(declared, &KOTLIN_TYPE_NAME_RULES);
    let segments: Vec<&str> = stripped.split('.').map(helpers::strip_backticks).collect();
    let is_qualified_name =
        !stripped.is_empty() && segments.iter().all(|segment| is_type_name_segment(segment));
    is_qualified_name.then(|| segments.join("."))
}

fn is_type_name_segment(segment: &str) -> bool {
    let mut chars = segment.chars();
    chars
        .next()
        .is_some_and(|first| first.is_alphabetic() || first == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
}

pub(super) fn record_declared_type(base: &mut BaseExtractor, symbol_id: &str, type_node: Node) {
    record_type_node(base, symbol_id, type_node, false);
}

/// Record a property's written type, else the type its initializer produces
/// (`is_inferred=true`) as [`InitializerIndex::shape_of`] reads it.
pub(super) fn record_property_facts(
    base: &mut BaseExtractor,
    symbol_id: &str,
    node: Node,
    index: &InitializerIndex,
) {
    if let Some(type_node) = property_type_node(node) {
        record_declared_type(base, symbol_id, type_node);
        return;
    }
    let Some(shape) = property_initializer_node(base, node)
        .and_then(|initializer| index.shape_of(base, initializer, 0))
    else {
        return;
    };
    base.record_declared_type_fact_with_declared(
        symbol_id,
        &shape.name,
        &shape.declared,
        &KOTLIN_TYPE_NAME_RULES,
        true,
    );
}

pub(super) fn declared_type_child(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.children(&mut cursor).find(|child| {
        matches!(
            child.kind(),
            "user_type" | "type" | "nullable_type" | "type_reference" | "function_type"
        )
    })
}

/// A type reduced to what initializer inference needs: its base name and its
/// written text.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TypeShape {
    name: String,
    declared: String,
}

#[derive(Debug)]
struct ReturnEntry {
    /// Node id of the source file, class body, or block that declares the function.
    container: usize,
    /// A local function is reachable only after its declaration.
    local: bool,
    start_byte: usize,
    required_args: usize,
    /// `None` when a `vararg` parameter takes any number of arguments.
    max_args: Option<usize>,
    /// `None` for no declared return type, a type parameter, or a shape with
    /// no single base name.
    shape: Option<TypeShape>,
    /// The receiver type text of an extension function, which Kotlin calls
    /// when no member accepts the argument types.
    receiver: Option<String>,
}

impl ReturnEntry {
    fn accepts(&self, arg_count: usize) -> bool {
        arg_count >= self.required_args && self.max_args.is_none_or(|max| arg_count <= max)
    }
}

/// An object or companion object that `TypeName.call()` reaches.
#[derive(Debug)]
struct StaticOwner {
    body: usize,
    /// The object has a supertype, so inherited overloads may out-rank its own.
    hidden_members: bool,
}

/// A same-file class that a constructor-like call `Name(args)` may reach.
#[derive(Debug)]
struct ClassEntry {
    /// Node id of the node that declares the class; the name is visible below it.
    declared_in: usize,
    companion_bodies: Vec<usize>,
    /// An interface with a companion, which has no constructor for the call,
    /// or a class whose companion has a supertype that may declare an
    /// `operator fun invoke`. An interface with no companion takes only a
    /// SAM conversion, which has the interface type.
    no_constructor: bool,
}

/// A parameter, local, or property whose name hides a same-named function or
/// object: Kotlin calls the closer value through `invoke`.
#[derive(Debug)]
struct ValueEntry {
    /// Node id of the node whose descendants see the value.
    scope: usize,
    /// A local is visible only after its declaration ends.
    visible_from: usize,
}

/// A same-file class, object, or enum entry with a name.
#[derive(Debug)]
struct Declaration {
    /// Node id of the node whose descendants see the name.
    scope: usize,
    node: usize,
    /// An object or enum entry, which a call `Name()` reaches through `invoke`.
    is_value: bool,
    /// An enum class, whose built-in statics out-rank its companion's members.
    is_enum: bool,
}

/// Same-file facts that initializer inference reads, built once per file:
/// where each class is declared, every named class, object, and enum entry,
/// function return types, the objects and companions each declaration
/// reaches as `Name.call()`, value names, and the names that explicit
/// imports bring in.
#[derive(Debug, Default)]
pub(super) struct InitializerIndex {
    classes: HashMap<String, Vec<ClassEntry>>,
    declarations: HashMap<String, Vec<Declaration>>,
    functions: HashMap<String, Vec<ReturnEntry>>,
    /// Keyed by the node id of the class or object declaration.
    static_owners: HashMap<usize, Vec<StaticOwner>>,
    values: HashMap<String, Vec<ValueEntry>>,
    imported: HashSet<String>,
}

impl InitializerIndex {
    pub(super) fn build(base: &BaseExtractor, root: Node) -> Self {
        let mut index = Self::default();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            match node.kind() {
                "class_declaration" | "object_declaration" => index.add_type(base, node),
                "enum_entry" => index.add_declaration(base, node, false),
                "function_declaration" => {
                    if let Some((name, entry)) = return_entry(base, node) {
                        index.functions.entry(name).or_default().push(entry);
                    }
                }
                "import" => index.imported.extend(imported_name(base, node)),
                _ => {}
            }
            if let Some((name, entry)) = value_entry(base, node) {
                index.values.entry(name).or_default().push(entry);
            }
            stack.extend(node.named_children(&mut node.walk()));
        }
        index
    }

    fn add_type(&mut self, base: &BaseExtractor, node: Node) {
        let Some((name, _)) = helpers::declared_name(base, &node) else {
            return;
        };
        let Some(declared_in) = node.parent() else {
            return;
        };
        self.add_declaration(base, node, has_child(node, "enum_class_body"));
        let owners = if node.kind() == "object_declaration" {
            vec![node]
        } else {
            let companions = companion_objects(node);
            let is_interface = has_child(node, "interface");
            self.classes
                .entry(name.clone())
                .or_default()
                .push(ClassEntry {
                    declared_in: declared_in.id(),
                    companion_bodies: companions.iter().copied().filter_map(body_id).collect(),
                    no_constructor: (is_interface && !companions.is_empty())
                        || companions
                            .iter()
                            .any(|companion| has_hidden_members(base, *companion)),
                });
            companions
        };
        let owners = owners.into_iter().filter_map(|owner| {
            Some(StaticOwner {
                body: body_id(owner)?,
                hidden_members: has_hidden_members(base, owner),
            })
        });
        self.static_owners
            .entry(node.id())
            .or_default()
            .extend(owners);
    }

    fn add_declaration(&mut self, base: &BaseExtractor, node: Node, is_enum: bool) {
        let (Some(name), Some(scope)) = (first_identifier(node), node.parent()) else {
            return;
        };
        self.declarations
            .entry(identifier_text(base, name))
            .or_default()
            .push(Declaration {
                scope: scope.id(),
                node: node.id(),
                is_value: node.kind() != "class_declaration",
                is_enum,
            });
    }

    /// The type an initializer produces: a same-file class constructor, or a
    /// call with a declared return type to a same-file function reached by a
    /// bare name, `this.`, or an object or companion through its type name.
    /// Parentheses pass the type through and `!!` drops nullability. Every
    /// same-named candidate the call reaches must agree, and at least one must
    /// accept the argument count.
    fn shape_of(&self, base: &BaseExtractor, value: Node, depth: u32) -> Option<TypeShape> {
        if !should_visit_tree_depth(depth) {
            return None;
        }
        match value.kind() {
            "parenthesized_expression" => {
                self.shape_of(base, value.named_child(0)?, child_tree_depth(depth)?)
            }
            "unary_expression"
                if value
                    .children(&mut value.walk())
                    .last()
                    .is_some_and(|operator| operator.kind() == "!!") =>
            {
                let shape = self.shape_of(base, value.named_child(0)?, child_tree_depth(depth)?)?;
                let declared = shape.declared.strip_suffix('?').unwrap_or(&shape.declared);
                Some(TypeShape {
                    declared: declared.to_string(),
                    name: shape.name,
                })
            }
            "call_expression" => self.call_shape(base, value),
            _ => None,
        }
    }

    fn call_shape(&self, base: &BaseExtractor, call: Node) -> Option<TypeShape> {
        let (callee, arg_count) = callee_and_arg_count(call)?;
        match callee.kind() {
            "identifier" => {
                let name = identifier_text(base, callee);
                if self.imported.contains(&name)
                    || self.value_hides(&name, call)
                    || self.object_hides(&name, call)
                {
                    return None;
                }
                if let Some(classes) = self.classes.get(&name) {
                    let visible = std::iter::successors(call.parent(), Node::parent)
                        .any(|node| classes.iter().any(|class| class.declared_in == node.id()));
                    let constructs = visible
                        && !self.functions.contains_key(&name)
                        && !classes
                            .iter()
                            .any(|class| self.invoke_may_win(&name, class));
                    return constructs.then(|| TypeShape {
                        declared: name.clone(),
                        name,
                    });
                }
                let reach = bare_call_reach(base, call)?;
                if reach.in_type && ANY_MEMBER_NAMES.contains(&name.as_str()) {
                    return None;
                }
                self.agreed(&name, arg_count, reach.hidden_members, |entry| {
                    reach.containers.contains(&entry.container)
                        && (!entry.local || entry.start_byte < call.start_byte())
                })
            }
            "navigation_expression" => {
                let parts: Vec<Node> = callee.named_children(&mut callee.walk()).collect();
                let [receiver, member] = parts.as_slice() else {
                    return None;
                };
                if member.kind() != "identifier" {
                    return None;
                }
                let name = identifier_text(base, *member);
                if ANY_MEMBER_NAMES.contains(&name.as_str()) {
                    return None;
                }
                match receiver.kind() {
                    "this_expression" if receiver.named_child_count() == 0 => {
                        let (body, hidden_members) = this_body(base, call)?;
                        if self.extension_may_win(&name, arg_count) {
                            return None;
                        }
                        self.agreed(&name, arg_count, hidden_members, |entry| {
                            entry.container == body
                        })
                    }
                    "identifier" => self.static_call(base, *receiver, call, &name, arg_count),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn static_call(
        &self,
        base: &BaseExtractor,
        receiver: Node,
        call: Node,
        name: &str,
        arg_count: usize,
    ) -> Option<TypeShape> {
        let type_name = identifier_text(base, receiver);
        if self.imported.contains(&type_name)
            || self.value_hides(&type_name, call)
            || self.extension_may_win(name, arg_count)
        {
            return None;
        }
        let (level, nearest) = self.nearest_declarations(&type_name, call)?;
        if !static_owner_reach(base, call).contains(&level)
            || (ENUM_STATIC_NAMES.contains(&name)
                && nearest.iter().any(|declaration| declaration.is_enum))
        {
            return None;
        }
        let mut owners: Vec<&StaticOwner> = Vec::new();
        for declaration in nearest {
            owners.extend(self.static_owners.get(&declaration.node)?.iter());
        }
        let hidden_members = owners.iter().any(|owner| owner.hidden_members);
        self.agreed(name, arg_count, hidden_members, |entry| {
            owners.iter().any(|owner| owner.body == entry.container)
        })
    }

    /// Whether a same-file extension named `name` may take a call with
    /// arguments through a receiver: Kotlin calls it when no member accepts
    /// the argument types, which arity alone cannot rule out. A member that
    /// accepts zero arguments always wins over an extension.
    fn extension_may_win(&self, name: &str, arg_count: usize) -> bool {
        arg_count > 0
            && self
                .functions
                .get(name)
                .is_some_and(|entries| entries.iter().any(|entry| entry.receiver.is_some()))
    }

    /// Whether `Name(args)` may call `Name.Companion.invoke` instead of a
    /// constructor: Kotlin does so when no constructor accepts the arguments.
    /// The invoke is a companion member or an extension whose receiver names
    /// the class, such as `Name.Companion`.
    fn invoke_may_win(&self, name: &str, class: &ClassEntry) -> bool {
        class.no_constructor
            || self.functions.get("invoke").is_some_and(|entries| {
                entries.iter().any(|entry| {
                    class.companion_bodies.contains(&entry.container)
                        || entry.receiver.as_deref().is_some_and(|receiver| {
                            receiver
                                .split(|c: char| !c.is_alphanumeric() && c != '_')
                                .any(|segment| segment == name)
                        })
                })
            })
    }

    /// Whether a parameter, a local declared before `call`, or a property in
    /// scope at `call` (its own class, an enclosing class, a companion of
    /// either, or the file) has the name `name`.
    fn value_hides(&self, name: &str, call: Node) -> bool {
        let Some(values) = self.values.get(name) else {
            return false;
        };
        scopes_at(call).any(|scope| {
            values
                .iter()
                .any(|value| value.scope == scope && value.visible_from <= call.start_byte())
        })
    }

    /// Whether an object or enum entry named `name` is in scope at `call`:
    /// `name()` then calls its `invoke`.
    fn object_hides(&self, name: &str, call: Node) -> bool {
        let Some(declarations) = self.declarations.get(name) else {
            return false;
        };
        scopes_at(call).any(|scope| {
            declarations
                .iter()
                .any(|declaration| declaration.is_value && declaration.scope == scope)
        })
    }

    /// The declarations named `name` at the nearest scope level around `call`
    /// that declares one, and the node id of that level. A class body and the
    /// class's companion body form one level.
    fn nearest_declarations(&self, name: &str, call: Node) -> Option<(usize, Vec<&Declaration>)> {
        let declarations = self.declarations.get(name)?;
        std::iter::successors(call.parent(), Node::parent).find_map(|node| {
            let companions = match node.parent() {
                Some(class)
                    if class.kind() == "class_declaration"
                        && matches!(node.kind(), "class_body" | "enum_class_body") =>
                {
                    companion_objects(class)
                }
                _ => Vec::new(),
            };
            let level: Vec<usize> = std::iter::once(node.id())
                .chain(companions.into_iter().filter_map(body_id))
                .collect();
            let nearest: Vec<&Declaration> = declarations
                .iter()
                .filter(|declaration| level.contains(&declaration.scope))
                .collect();
            (!nearest.is_empty()).then_some((node.id(), nearest))
        })
    }

    /// The shared shape of the same-named candidates `reaches` selects.
    /// Behind `hidden_members` an unseen inherited or synthesized overload
    /// may out-rank every candidate that takes arguments or has defaults, so
    /// only a zero-argument call with an exact zero-parameter candidate is safe.
    fn agreed(
        &self,
        name: &str,
        arg_count: usize,
        hidden_members: bool,
        reaches: impl Fn(&ReturnEntry) -> bool,
    ) -> Option<TypeShape> {
        let candidates: Vec<&ReturnEntry> = self
            .functions
            .get(name)?
            .iter()
            .filter(|entry| reaches(entry))
            .collect();
        let applicable = if hidden_members {
            arg_count == 0
                && candidates
                    .iter()
                    .any(|entry| entry.required_args == 0 && entry.max_args == Some(0))
        } else {
            candidates.iter().any(|entry| entry.accepts(arg_count))
        };
        if !applicable {
            return None;
        }
        let first = candidates.first()?.shape.as_ref()?;
        candidates
            .iter()
            .all(|entry| entry.shape.as_ref() == Some(first))
            .then(|| first.clone())
    }
}

/// Statics every enum class has, which Kotlin calls before a companion member.
const ENUM_STATIC_NAMES: [&str; 3] = ["values", "valueOf", "entries"];

/// Members every class and object inherits from `Any`.
const ANY_MEMBER_NAMES: [&str; 3] = ["toString", "hashCode", "equals"];

const TYPE_SCOPE_KINDS: [&str; 5] = [
    "class_declaration",
    "object_declaration",
    "companion_object",
    "object_literal",
    "enum_entry",
];

/// What a bare call reaches.
struct BareCallReach {
    /// Its enclosing blocks, class bodies and their companions, and the file,
    /// up to and including the first type with hidden members.
    containers: HashSet<usize>,
    /// The walk stopped at a type with inherited or synthesized members.
    hidden_members: bool,
    /// The call sits inside a class or object body, so `Any` members apply.
    in_type: bool,
}

/// `None` inside a lambda or an extension, whose implicit receiver may
/// declare a function of the same name.
fn bare_call_reach(base: &BaseExtractor, call: Node) -> Option<BareCallReach> {
    let mut reach = BareCallReach {
        containers: HashSet::new(),
        hidden_members: false,
        in_type: false,
    };
    for node in std::iter::successors(call.parent(), Node::parent) {
        if changes_implicit_receiver(base, node) {
            return None;
        }
        reach.containers.insert(node.id());
        if !TYPE_SCOPE_KINDS.contains(&node.kind()) {
            continue;
        }
        reach.in_type = true;
        let companions = match node.kind() {
            "class_declaration" => companion_objects(node),
            _ => Vec::new(),
        };
        reach.hidden_members = has_hidden_members(base, node)
            || companions
                .iter()
                .any(|companion| has_hidden_members(base, *companion));
        reach
            .containers
            .extend(companions.into_iter().filter_map(body_id));
        if reach.hidden_members {
            break;
        }
    }
    Some(reach)
}

/// The nodes whose declared objects and companions `Type.m()` at `call` may
/// name: its ancestors up to the first lambda or extension, whose receiver
/// may have a property with the type name, or the first type with hidden
/// members, which may inherit a nested classifier with that name.
fn static_owner_reach(base: &BaseExtractor, call: Node) -> HashSet<usize> {
    let mut reachable = HashSet::new();
    for node in std::iter::successors(call.parent(), Node::parent) {
        if changes_implicit_receiver(base, node) {
            break;
        }
        reachable.insert(node.id());
        if TYPE_SCOPE_KINDS.contains(&node.kind())
            && (has_hidden_members(base, node)
                || companion_objects(node)
                    .into_iter()
                    .any(|companion| has_hidden_members(base, companion)))
        {
            break;
        }
    }
    reachable
}

/// The class body `this` names at `call` and whether that type has hidden
/// members; `None` inside a lambda or an extension, where `this` may name
/// another receiver.
fn this_body(base: &BaseExtractor, call: Node) -> Option<(usize, bool)> {
    for node in std::iter::successors(call.parent(), Node::parent) {
        if changes_implicit_receiver(base, node) {
            return None;
        }
        if TYPE_SCOPE_KINDS.contains(&node.kind()) {
            return Some((body_id(node)?, has_hidden_members(base, node)));
        }
    }
    None
}

fn changes_implicit_receiver(base: &BaseExtractor, node: Node) -> bool {
    match node.kind() {
        "lambda_literal" | "anonymous_function" => true,
        "function_declaration" | "property_declaration" => {
            helpers::extract_receiver_type(base, &node).is_some()
        }
        _ => false,
    }
}

/// Whether a type has members its body does not declare beyond `Any`'s: a
/// supertype, an enum entry or enum class, or a data class.
fn has_hidden_members(base: &BaseExtractor, node: Node) -> bool {
    node.kind() == "enum_entry"
        || node
            .children(&mut node.walk())
            .any(|child| match child.kind() {
                "delegation_specifiers" | "enum_class_body" => true,
                "modifiers" => child.named_children(&mut child.walk()).any(|modifier| {
                    modifier.kind() == "class_modifier" && base.get_node_text(&modifier) == "data"
                }),
                _ => false,
            })
}

/// Node ids of `call`'s ancestors and of the companion bodies of its
/// enclosing classes.
fn scopes_at(call: Node) -> impl Iterator<Item = usize> {
    std::iter::successors(call.parent(), Node::parent).flat_map(|node| {
        let companions = match node.kind() {
            "class_declaration" => companion_objects(node),
            _ => Vec::new(),
        };
        std::iter::once(node.id()).chain(companions.into_iter().filter_map(body_id))
    })
}

fn has_child(node: Node, kind: &str) -> bool {
    node.children(&mut node.walk())
        .any(|child| child.kind() == kind)
}

fn companion_objects(class: Node) -> Vec<Node> {
    class
        .children(&mut class.walk())
        .filter(|child| matches!(child.kind(), "class_body" | "enum_class_body"))
        .flat_map(|body| {
            body.children(&mut body.walk())
                .filter(|member| member.kind() == "companion_object")
                .collect::<Vec<_>>()
        })
        .collect()
}

fn body_id(node: Node) -> Option<usize> {
    node.children(&mut node.walk())
        .find(|child| matches!(child.kind(), "class_body" | "enum_class_body"))
        .map(|body| body.id())
}

fn identifier_text(base: &BaseExtractor, node: Node) -> String {
    helpers::strip_backticks(&base.get_node_text(&node)).to_string()
}

/// The callee of a call and its argument count. A trailing lambda counts as
/// an argument; `f(a) { }` parses as a call that wraps the call `f(a)`.
fn callee_and_arg_count(call: Node) -> Option<(Node, usize)> {
    let (callee, count, has_parentheses) = call_parts(call)?;
    if callee.kind() != "call_expression" {
        return Some((callee, count));
    }
    if has_parentheses {
        return None;
    }
    let (inner, inner_count, inner_parentheses) = call_parts(callee)?;
    (inner_parentheses && inner.kind() != "call_expression").then_some((inner, inner_count + count))
}

fn call_parts(call: Node) -> Option<(Node, usize, bool)> {
    let mut cursor = call.walk();
    let mut children = call.named_children(&mut cursor);
    let callee = children.next()?;
    let mut count = 0;
    let mut has_parentheses = false;
    for child in children {
        match child.kind() {
            "value_arguments" => {
                has_parentheses = true;
                count += child
                    .named_children(&mut child.walk())
                    .filter(|argument| argument.kind() == "value_argument")
                    .count();
            }
            "annotated_lambda" => count += 1,
            _ => {}
        }
    }
    Some((callee, count, has_parentheses))
}

fn return_entry(base: &BaseExtractor, function: Node) -> Option<(String, ReturnEntry)> {
    let (name, _) = helpers::declared_name(base, &function)?;
    let container = function.parent()?;
    let generics = visible_type_parameters(base, function);
    let (required_args, max_args) = parameter_arity(base, function);
    let shape = helpers::return_type_node(function)
        .filter(|return_type| !mentions_type_parameter(base, *return_type, &generics))
        .and_then(|return_type| {
            Some(TypeShape {
                name: base_type_name(base, return_type)?,
                declared: base.get_node_text(&return_type),
            })
        });
    let entry = ReturnEntry {
        container: container.id(),
        local: !matches!(
            container.kind(),
            "source_file" | "class_body" | "enum_class_body"
        ),
        start_byte: function.start_byte(),
        required_args,
        max_args,
        shape,
        receiver: helpers::extract_receiver_type(base, &function),
    };
    Some((name, entry))
}

/// Whether a type names a type parameter at any depth: `T`, `List<T>`,
/// `Map<K, List<V>>`. The caller's type arguments are unknown, so such a
/// return records nothing.
fn mentions_type_parameter(base: &BaseExtractor, type_node: Node, generics: &[String]) -> bool {
    if generics.is_empty() {
        return false;
    }
    let mut stack = vec![(type_node, 0)];
    while let Some((node, depth)) = stack.pop() {
        if node.kind() == "identifier" && generics.contains(&identifier_text(base, node)) {
            return true;
        }
        let Some(child_depth) = child_tree_depth(depth) else {
            return true;
        };
        stack.extend(
            node.named_children(&mut node.walk())
                .map(|child| (child, child_depth)),
        );
    }
    false
}

/// A value name and its scope: a function, constructor, or setter parameter,
/// a property or local, a `for` or `when` subject variable, or a caught
/// exception.
fn value_entry(base: &BaseExtractor, node: Node) -> Option<(String, ValueEntry)> {
    let parent = node.parent()?;
    let (name_node, scope, visible_from) = match node.kind() {
        "variable_declaration" => {
            let holder = if parent.kind() == "multi_variable_declaration" {
                parent.parent()?
            } else {
                parent
            };
            let name = first_identifier(node)?;
            match holder.kind() {
                "property_declaration" => {
                    let scope = holder.parent()?;
                    let member = matches!(
                        scope.kind(),
                        "source_file" | "class_body" | "enum_class_body"
                    );
                    (name, scope, if member { 0 } else { holder.end_byte() })
                }
                "when_subject" | "lambda_parameters" => (name, holder.parent()?, 0),
                _ => (name, holder, 0),
            }
        }
        "parameter" => (first_identifier(node)?, parent.parent()?, 0),
        "class_parameter" => {
            let class = std::iter::successors(Some(parent), Node::parent)
                .find(|ancestor| ancestor.kind() == "class_declaration")?;
            (first_identifier(node)?, class, 0)
        }
        "catch_block" | "setter" => (first_identifier(node)?, node, 0),
        _ => return None,
    };
    Some((
        identifier_text(base, name_node),
        ValueEntry {
            scope: scope.id(),
            visible_from,
        },
    ))
}

fn first_identifier(node: Node) -> Option<Node> {
    node.named_children(&mut node.walk())
        .find(|child| child.kind() == "identifier")
}

/// Type parameter names of the function and of every enclosing function and class.
fn visible_type_parameters(base: &BaseExtractor, function: Node) -> Vec<String> {
    std::iter::successors(Some(function), Node::parent)
        .filter(|node| matches!(node.kind(), "function_declaration" | "class_declaration"))
        .filter_map(|node| {
            node.children(&mut node.walk())
                .find(|child| child.kind() == "type_parameters")
        })
        .flat_map(|parameters| {
            parameters
                .named_children(&mut parameters.walk())
                .filter(|parameter| parameter.kind() == "type_parameter")
                .filter_map(|parameter| helpers::declared_name(base, &parameter))
                .map(|(name, _)| name)
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The required and maximum argument counts; a parameter with a default is
/// optional and a `vararg` parameter lifts the maximum.
fn parameter_arity(base: &BaseExtractor, function: Node) -> (usize, Option<usize>) {
    let Some(parameters) = function
        .children(&mut function.walk())
        .find(|child| child.kind() == "function_value_parameters")
    else {
        return (0, Some(0));
    };
    let children: Vec<Node> = parameters.children(&mut parameters.walk()).collect();
    let mut required = 0;
    let mut total = 0;
    let mut has_vararg = false;
    let mut vararg_pending = false;
    for (position, child) in children.iter().enumerate() {
        match child.kind() {
            "parameter_modifiers" => {
                vararg_pending = base
                    .get_node_text(child)
                    .split_whitespace()
                    .any(|modifier| modifier == "vararg");
            }
            "parameter" => {
                total += 1;
                let has_default = children
                    .get(position + 1)
                    .is_some_and(|next| next.kind() == "=");
                if vararg_pending {
                    has_vararg = true;
                } else if !has_default {
                    required += 1;
                }
                vararg_pending = false;
            }
            _ => {}
        }
    }
    (required, (!has_vararg).then_some(total))
}

/// The simple name or alias an explicit, non-star import brings in.
fn imported_name(base: &BaseExtractor, import: Node) -> Option<String> {
    let children: Vec<Node> = import.children(&mut import.walk()).collect();
    if children.iter().any(|child| child.kind() == "*") {
        return None;
    }
    let name = match children.iter().find(|child| child.kind() == "identifier") {
        Some(alias) => *alias,
        None => {
            let path = children
                .iter()
                .find(|child| child.kind() == "qualified_identifier")?;
            path.named_children(&mut path.walk()).last()?
        }
    };
    Some(identifier_text(base, name))
}

fn record_type_node(base: &mut BaseExtractor, symbol_id: &str, type_node: Node, is_inferred: bool) {
    let Some(base_name) = base_type_name(base, type_node) else {
        return;
    };
    let declared = base.get_node_text(&type_node);
    base.record_declared_type_fact_with_declared(
        symbol_id,
        &base_name,
        &declared,
        &KOTLIN_TYPE_NAME_RULES,
        is_inferred,
    );
}

fn base_type_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let core = unwrap_type_wrappers(node)?;
    match core.kind() {
        "user_type" => user_type_base_name(base, core),
        "identifier" | "simple_identifier" => {
            Some(helpers::strip_backticks(&base.get_node_text(&core)).to_string())
        }
        _ => None,
    }
}

fn unwrap_type_wrappers(node: Node) -> Option<Node> {
    let mut current = node;
    for _ in 0..8 {
        match current.kind() {
            "type" | "type_reference" | "nullable_type" | "parenthesized_type"
            | "non_nullable_type" => {
                let mut cursor = current.walk();
                current = current.named_children(&mut cursor).next()?;
            }
            _ => return Some(current),
        }
    }
    Some(current)
}

fn user_type_base_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let children: Vec<Node> = {
        let mut cursor = node.walk();
        node.children(&mut cursor).collect()
    };
    let identifiers: Vec<String> = children
        .iter()
        .filter(|child| child.kind() == "identifier" || child.kind() == "simple_identifier")
        .map(|child| helpers::strip_backticks(&base.get_node_text(child)).to_string())
        .collect();
    if identifiers.is_empty() {
        None
    } else {
        Some(identifiers.join("."))
    }
}

fn property_type_node(node: Node) -> Option<Node> {
    let var_decl = {
        let mut cursor = node.walk();
        node.children(&mut cursor)
            .find(|child| child.kind() == "variable_declaration")
    };
    if let Some(var_decl) = var_decl
        && let Some(type_node) = declared_type_child(var_decl)
    {
        return Some(type_node);
    }
    declared_type_child(node)
}

fn property_initializer_node<'a>(base: &BaseExtractor, node: Node<'a>) -> Option<Node<'a>> {
    let children: Vec<Node<'a>> = {
        let mut cursor = node.walk();
        node.children(&mut cursor).collect()
    };
    let assignment_index = children
        .iter()
        .position(|child| base.get_node_text(child) == "=")?;
    children.get(assignment_index + 1).copied()
}
