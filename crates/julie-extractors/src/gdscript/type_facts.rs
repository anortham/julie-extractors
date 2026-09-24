use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::Node;

pub(super) const TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &[],
    generic_open: &['['],
};

pub(super) fn record_statement_type_facts(
    base: &mut BaseExtractor,
    symbol_id: &str,
    statement: Node,
    same_file_types: &SameFileTypes,
) {
    if let Some(type_node) = statement.child_by_field_name("type")
        && type_node.kind() == "type"
    {
        record_declared_type_node(base, symbol_id, type_node);
        return;
    }
    let Some(value) = statement.child_by_field_name("value") else {
        return;
    };
    if let Some(cast_type) = cast_type(base, value) {
        base.record_declared_type_fact(symbol_id, &cast_type, &TYPE_NAME_RULES, true);
        return;
    }
    let Some(owner) = enclosing_class_path(base, statement) else {
        return;
    };
    let scope = InitializerScope {
        base,
        types: same_file_types,
        owner,
        statement,
    };
    if let Some(inferred) = scope.type_of(value, 0) {
        base.record_declared_type_fact(symbol_id, &inferred, &TYPE_NAME_RULES, true);
    }
}

/// The target type of an `expr as T` initializer.
pub(super) fn cast_type(base: &BaseExtractor, value: Node) -> Option<String> {
    let operand = super::identifiers::type_test_operand(value)?;
    let is_cast = value
        .child_by_field_name("op")
        .is_some_and(|operator| operator.kind() == "as");
    is_cast.then(|| base.get_node_text(&operand))
}

pub(super) fn record_declared_type_node(
    base: &mut BaseExtractor,
    symbol_id: &str,
    type_node: Node,
) {
    if type_node.kind() != "type" {
        return;
    }
    let declared = base.get_node_text(&type_node);
    base.record_declared_type_fact(symbol_id, &declared, &TYPE_NAME_RULES, false);
}

/// The chain of inner-class names from the script down to a class. The
/// script class itself is the empty path.
type ClassPath = Vec<String>;

/// Names that a bare call resolves to before any method of the class, in
/// the Godot 4 analyzer order: built-in type constructors, GDScript utility
/// functions, then `@GlobalScope` utility functions. A method with one of
/// these names only gets a shadowing warning, so `load()` never calls it.
const GLOBAL_CALLEES: &[&str] = &[
    "AABB",
    "Array",
    "Basis",
    "Callable",
    "Color",
    "Dictionary",
    "NodePath",
    "Object",
    "PackedByteArray",
    "PackedColorArray",
    "PackedFloat32Array",
    "PackedFloat64Array",
    "PackedInt32Array",
    "PackedInt64Array",
    "PackedStringArray",
    "PackedVector2Array",
    "PackedVector3Array",
    "PackedVector4Array",
    "Plane",
    "Projection",
    "Quaternion",
    "RID",
    "Rect2",
    "Rect2i",
    "Signal",
    "String",
    "StringName",
    "Transform2D",
    "Transform3D",
    "Vector2",
    "Vector2i",
    "Vector3",
    "Vector3i",
    "Vector4",
    "Vector4i",
    "bool",
    "float",
    "int",
    "Color8",
    "assert",
    "char",
    "convert",
    "dict_to_inst",
    "get_stack",
    "inst_to_dict",
    "is_instance_of",
    "len",
    "load",
    "preload",
    "print_debug",
    "print_stack",
    "range",
    "type_exists",
    "abs",
    "absf",
    "absi",
    "acos",
    "acosh",
    "angle_difference",
    "asin",
    "asinh",
    "atan",
    "atan2",
    "atanh",
    "bezier_derivative",
    "bezier_interpolate",
    "bytes_to_var",
    "bytes_to_var_with_objects",
    "ceil",
    "ceilf",
    "ceili",
    "clamp",
    "clampf",
    "clampi",
    "cos",
    "cosh",
    "cubic_interpolate",
    "cubic_interpolate_angle",
    "cubic_interpolate_angle_in_time",
    "cubic_interpolate_in_time",
    "db_to_linear",
    "deg_to_rad",
    "ease",
    "error_string",
    "exp",
    "floor",
    "floorf",
    "floori",
    "fmod",
    "fposmod",
    "hash",
    "instance_from_id",
    "inverse_lerp",
    "is_equal_approx",
    "is_finite",
    "is_inf",
    "is_instance_id_valid",
    "is_instance_valid",
    "is_nan",
    "is_same",
    "is_zero_approx",
    "lerp",
    "lerp_angle",
    "lerpf",
    "linear_to_db",
    "log",
    "max",
    "maxf",
    "maxi",
    "min",
    "minf",
    "mini",
    "move_toward",
    "nearest_po2",
    "pingpong",
    "posmod",
    "pow",
    "print",
    "print_rich",
    "print_verbose",
    "printerr",
    "printraw",
    "prints",
    "printt",
    "push_error",
    "push_warning",
    "rad_to_deg",
    "rand_from_seed",
    "randf",
    "randf_range",
    "randfn",
    "randi",
    "randi_range",
    "randomize",
    "remap",
    "rid_allocate_id",
    "rid_from_int64",
    "rotate_toward",
    "round",
    "roundf",
    "roundi",
    "seed",
    "sign",
    "signf",
    "signi",
    "sin",
    "sinh",
    "smoothstep",
    "snapped",
    "snappedf",
    "snappedi",
    "sqrt",
    "step_decimals",
    "str",
    "str_to_var",
    "tan",
    "tanh",
    "type_convert",
    "type_string",
    "typeof",
    "var_to_bytes",
    "var_to_bytes_with_objects",
    "var_to_str",
    "weakref",
    "wrap",
    "wrapf",
    "wrapi",
];

/// What a class-level name declares. Enums, consts, vars, signals,
/// functions and enumerators are all `Other`: each one shadows a class of
/// the same name from an outer scope, and enums and consts are also types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Member {
    Class,
    Other,
}

/// Where the `extends` of an inner class leads.
#[derive(Debug, Clone, PartialEq, Eq)]
enum BaseLink {
    /// No `extends`, or a base that no same-file declaration names: a
    /// native class, another file's class, or a `"res://..."` path.
    Outside,
    Inner(ClassPath),
    /// A base that names same-file declarations but not exactly one class,
    /// or a class path declared twice.
    Unknown,
}

/// What initializer inference needs to know about the file: the names each
/// class declares, where each inner class inherits from, and the declared
/// return types of its functions, keyed by owning class path.
#[derive(Debug, Default)]
pub(super) struct SameFileTypes {
    members: HashMap<(ClassPath, String), Vec<Member>>,
    bases: HashMap<ClassPath, BaseLink>,
    /// `extends` names not resolved yet. Empty once `build` returns.
    unresolved_bases: HashMap<ClassPath, Vec<String>>,
    script_class: Option<String>,
    /// `(owner path, function name)` to each same-named declaration's return
    /// type. More than one entry only comes from duplicate declarations.
    return_types: HashMap<(ClassPath, String), Vec<Option<String>>>,
}

impl SameFileTypes {
    pub(super) fn build(base: &BaseExtractor, root: Node) -> Self {
        let mut types = Self::default();
        types.collect(base, root, &[], 0);
        types.resolve_bases();
        types
    }

    fn collect(&mut self, base: &BaseExtractor, node: Node, owner: &[String], depth: u32) {
        if !should_visit_tree_depth(depth) {
            return;
        }
        let name = node
            .child_by_field_name("name")
            .map(|name| base.get_node_text(&name));
        match (node.kind(), name) {
            ("class_name_statement", Some(name)) => {
                if owner.is_empty() {
                    self.script_class = Some(name);
                }
            }
            ("class_definition", Some(name)) => {
                self.add_member(owner, name.clone(), Member::Class);
                let mut path = owner.to_vec();
                path.push(name);
                let extends = node
                    .child_by_field_name("extends")
                    .and_then(|extends| extends.named_child(0))
                    .filter(|base_type| base_type.kind() == "type")
                    .map(|base_type| base.get_node_text(&base_type))
                    .filter(|base_type| !base_type.starts_with(['"', '\'']));
                if self.bases.contains_key(&path) || self.unresolved_bases.contains_key(&path) {
                    self.unresolved_bases.remove(&path);
                    self.bases.insert(path.clone(), BaseLink::Unknown);
                } else if let Some(extends) = extends {
                    let segments = extends.split('.').map(|s| s.trim().to_string());
                    self.unresolved_bases
                        .insert(path.clone(), segments.collect());
                } else {
                    self.bases.insert(path.clone(), BaseLink::Outside);
                }
                self.collect_children(base, node, &path, depth);
            }
            ("class_definition", None) => {}
            ("variable_statement" | "const_statement" | "signal_statement", Some(name)) => {
                self.add_member(owner, name, Member::Other);
            }
            ("enum_definition", name) => {
                let mut cursor = node.walk();
                let enumerators = node
                    .child_by_field_name("body")
                    .into_iter()
                    .flat_map(|body| body.named_children(&mut cursor).collect::<Vec<_>>())
                    .filter_map(|enumerator| enumerator.child_by_field_name("left"))
                    .map(|left| base.get_node_text(&left));
                for member in name.into_iter().chain(enumerators) {
                    self.add_member(owner, member, Member::Other);
                }
            }
            ("function_definition", Some(name)) => {
                self.add_member(owner, name.clone(), Member::Other);
                let return_type = node
                    .child_by_field_name("return_type")
                    .map(|return_type| base.get_node_text(&return_type))
                    .filter(|return_type| return_type != "void");
                self.return_types
                    .entry((owner.to_vec(), name))
                    .or_default()
                    .push(return_type);
            }
            _ => self.collect_children(base, node, owner, depth),
        }
    }

    fn add_member(&mut self, owner: &[String], name: String, member: Member) {
        self.members
            .entry((owner.to_vec(), name))
            .or_default()
            .push(member);
    }

    fn collect_children(&mut self, base: &BaseExtractor, node: Node, owner: &[String], depth: u32) {
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect(base, child, owner, child_depth);
        }
    }

    /// Resolves every `extends` name, in rounds, because a name can only be
    /// read once the bases of the classes around it are known. A name still
    /// unresolved when a round makes no progress is part of a loop.
    fn resolve_bases(&mut self) {
        loop {
            let resolved: Vec<(ClassPath, BaseLink)> = self
                .unresolved_bases
                .iter()
                .filter_map(|(class, extends)| {
                    let outer = &class[..class.len() - 1];
                    Some((class.clone(), self.resolve_base(extends, outer)?))
                })
                .collect();
            if resolved.is_empty() {
                break;
            }
            for (class, link) in resolved {
                self.unresolved_bases.remove(&class);
                self.bases.insert(class, link);
            }
        }
        for class in std::mem::take(&mut self.unresolved_bases).into_keys() {
            self.bases.insert(class, BaseLink::Unknown);
        }
    }

    /// Reads a dotted `extends` name in `outer`, the scope around the class.
    /// `None` while a class it depends on has an unresolved base.
    fn resolve_base(&self, extends: &[String], outer: &[String]) -> Option<BaseLink> {
        let (head, rest) = extends.split_first()?;
        let mut path = match self.visible(head, outer)?.as_slice() {
            [] => return Some(BaseLink::Outside),
            [(path, Member::Class)] => path.clone(),
            _ => return Some(BaseLink::Unknown),
        };
        for segment in rest {
            path = match self.members_of(&path, segment)?.as_slice() {
                [(member, Member::Class)] => member.clone(),
                _ => return Some(BaseLink::Unknown),
            };
        }
        Some(BaseLink::Inner(path))
    }

    /// `class` followed by the same-file classes it inherits from, nearest
    /// first. `None` when a base in the chain is unknown or the chain loops.
    fn lineage(&self, class: &[String]) -> Option<Vec<ClassPath>> {
        let mut chain = vec![class.to_vec()];
        loop {
            let current = chain.last()?;
            let link = match self.bases.get(current) {
                Some(link) => link,
                None if self.unresolved_bases.contains_key(current) => return None,
                None => return Some(chain),
            };
            match link {
                BaseLink::Outside => return Some(chain),
                BaseLink::Unknown => return None,
                BaseLink::Inner(base) if chain.contains(base) => return None,
                BaseLink::Inner(base) => chain.push(base.clone()),
            }
        }
    }

    /// Every declaration named `name` that `class` declares or inherits from
    /// a same-file base, as its path and kind.
    fn members_of(&self, class: &[String], name: &str) -> Option<Vec<(ClassPath, Member)>> {
        let mut found = Vec::new();
        for owner in self.lineage(class)? {
            let key = (owner, name.to_string());
            if let Some(kinds) = self.members.get(&key) {
                let mut path = key.0;
                path.push(key.1);
                found.extend(kinds.iter().map(|kind| (path.clone(), *kind)));
            }
        }
        Some(found)
    }

    /// The return type every same-named function of this owner agrees on.
    fn return_type(&self, owner: &[String], name: &str) -> Option<String> {
        let mut candidates = self
            .return_types
            .get(&(owner.to_vec(), name.to_string()))?
            .iter();
        let first = candidates.next()?.as_ref()?;
        candidates
            .all(|candidate| candidate.as_ref() == Some(first))
            .then(|| first.clone())
    }

    fn declares(&self, owner: &[String], name: &str) -> bool {
        self.return_types
            .contains_key(&(owner.to_vec(), name.to_string()))
    }

    fn declares_class_named(&self, name: &str) -> bool {
        self.script_class.as_deref() == Some(name)
            || self
                .members
                .iter()
                .any(|((_, member), kinds)| member == name && kinds.contains(&Member::Class))
    }

    /// Every same-file declaration that the bare name `name` can mean inside
    /// `scope`: a member that `scope` or any class around it declares or
    /// inherits from a same-file base, or the script's `class_name`. Each one
    /// is the declared path and its kind. `None` when a class on the way has
    /// a base that cannot be resolved.
    fn visible(&self, name: &str, scope: &[String]) -> Option<Vec<(ClassPath, Member)>> {
        let mut found: Vec<(ClassPath, Member)> = Vec::new();
        let mut searched: Vec<ClassPath> = Vec::new();
        for depth in 0..=scope.len() {
            for owner in self.lineage(&scope[..depth])? {
                if searched.contains(&owner) {
                    continue;
                }
                let key = (owner, name.to_string());
                if let Some(kinds) = self.members.get(&key) {
                    let mut path = key.0.clone();
                    path.push(key.1);
                    found.extend(kinds.iter().map(|kind| (path.clone(), *kind)));
                }
                searched.push(key.0);
            }
        }
        if self.script_class.as_deref() == Some(name) {
            found.push((ClassPath::new(), Member::Class));
        }
        Some(found)
    }

    /// Whether every type name in `type_text`, written in `callee`, names the
    /// same declaration when read in `caller`: the same class, enum or const,
    /// or no same-file declaration in both. A name that is ambiguous in
    /// either scope fails.
    fn means_the_same(&self, type_text: &str, callee: &[String], caller: &[String]) -> bool {
        callee == caller
            || type_text
                .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.'))
                .filter_map(|name| name.split('.').next().filter(|head| !head.is_empty()))
                .all(
                    |name| match (self.visible(name, callee), self.visible(name, caller)) {
                        (Some(written), Some(read)) => written.len() <= 1 && written == read,
                        _ => false,
                    },
                )
    }
}

/// The class path of the class that holds `node`, or `None` when a class
/// around it has no name.
fn enclosing_class_path(base: &BaseExtractor, node: Node) -> Option<ClassPath> {
    let mut path = ClassPath::new();
    let mut current = node.parent();
    while let Some(ancestor) = current {
        if ancestor.kind() == "class_definition" {
            path.push(base.get_node_text(&ancestor.child_by_field_name("name")?));
        }
        current = ancestor.parent();
    }
    path.reverse();
    Some(path)
}

/// Resolves the type an initializer produces from same-file declarations:
/// `Foo.new()`, a bare or `self.` call to a function of the enclosing class,
/// or `Foo.func()` on the one same-file class that `Foo` can mean here, each
/// with a declared return type. A return type from another class is kept only
/// when its class names mean the same classes at the call site. `await` and
/// parentheses pass the type through, except that awaiting a `Signal` yields
/// the signal's arguments, so it records nothing.
struct InitializerScope<'a, 't> {
    base: &'a BaseExtractor,
    types: &'a SameFileTypes,
    owner: ClassPath,
    statement: Node<'t>,
}

impl InitializerScope<'_, '_> {
    fn type_of(&self, value: Node, depth: u32) -> Option<String> {
        if !should_visit_tree_depth(depth) {
            return None;
        }
        match value.kind() {
            "parenthesized_expression" => {
                self.type_of(value.named_child(0)?, child_tree_depth(depth)?)
            }
            "await_expression" => self
                .type_of(value.named_child(0)?, child_tree_depth(depth)?)
                .filter(|awaited| awaited != "Signal"),
            "call" => {
                let callee = value.named_child(0)?;
                if callee.kind() != "identifier" {
                    return None;
                }
                let callee = self.base.get_node_text(&callee);
                if GLOBAL_CALLEES.contains(&callee.as_str()) {
                    return None;
                }
                self.types.return_type(&self.owner, &callee)
            }
            "attribute" => self.attribute_call(value),
            _ => None,
        }
    }

    fn attribute_call(&self, value: Node) -> Option<String> {
        let mut cursor = value.walk();
        let children: Vec<Node> = value.named_children(&mut cursor).collect();
        let [receiver, call] = children.as_slice() else {
            return None;
        };
        if receiver.kind() != "identifier" || call.kind() != "attribute_call" {
            return None;
        }
        let method = call
            .named_child(0)
            .filter(|method| method.kind() == "identifier")
            .map(|method| self.base.get_node_text(&method))?;
        let receiver = self.base.get_node_text(receiver);
        if receiver == "self" {
            return self.types.return_type(&self.owner, &method);
        }
        if self.declares_local(&receiver) {
            return None;
        }
        let visible = self.types.visible(&receiver, &self.owner)?;
        if let [(target, Member::Class)] = visible.as_slice()
            && self.types.declares(target, &method)
        {
            return self
                .types
                .return_type(target, &method)
                .filter(|returned| self.types.means_the_same(returned, target, &self.owner));
        }
        let shadowed = visible.iter().any(|(_, kind)| *kind == Member::Other);
        (method == "new" && !shadowed && self.types.declares_class_named(&receiver))
            .then_some(receiver)
    }

    /// Whether the function, lambda or accessor that holds the statement declares
    /// `name` as a parameter or local anywhere in its body. Godot resolves
    /// those before class names. A declaration in another block also counts,
    /// which can only drop a fact.
    fn declares_local(&self, name: &str) -> bool {
        let mut root = None;
        let mut current = self.statement.parent();
        while let Some(ancestor) = current {
            match ancestor.kind() {
                "class_definition" | "source" => break,
                kind if super::helpers::is_callable_scope(kind) => root = Some(ancestor),
                _ => {}
            }
            current = ancestor.parent();
        }
        root.is_some_and(|root| self.subtree_declares(root, name, 0))
    }

    fn subtree_declares(&self, node: Node, name: &str, depth: u32) -> bool {
        let Some(child_depth) = child_tree_depth(depth) else {
            return true;
        };
        let declared = match node.kind() {
            "variable_statement" | "const_statement" | "lambda" => node.child_by_field_name("name"),
            "for_statement" => node.child_by_field_name("left"),
            "typed_parameter"
            | "default_parameter"
            | "typed_default_parameter"
            | "variadic_parameter"
            | "pattern_binding" => node.named_child(0),
            "identifier" => node
                .parent()
                .filter(|parent| parent.kind() == "parameters")
                .map(|_| node),
            _ => None,
        };
        if declared.is_some_and(|declared| self.base.get_node_text(&declared) == name) {
            return true;
        }
        let mut cursor = node.walk();
        node.named_children(&mut cursor)
            .any(|child| self.subtree_declares(child, name, child_depth))
    }
}
