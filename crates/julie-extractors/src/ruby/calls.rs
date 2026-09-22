use super::helpers::{extract_call_receiver, extract_method_name_from_call, is_self_directed_call};
use super::symbols::MemberScope;
/// Special method call extraction for Ruby
/// Handles require, attr_accessor, define_method, def_delegator, module_function, Struct.new
/// and the RSpec, Rails, and ActiveSupport test-block vocabularies
use crate::base::{
    AnnotationMarker, BaseExtractor, Symbol, SymbolKind, SymbolOptions, TestRole, Visibility,
};
use crate::test_detection::{apply_test_role, is_test_path, ruby_block_test_role};
use std::collections::HashMap;
use tree_sitter::Node;

/// Extract special method calls that create symbols.
pub(super) fn extract_call(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
    scope: &MemberScope,
) -> Vec<Symbol> {
    let Some(method_name) = extract_method_name_from_call(node, |n| base.get_node_text(n)) else {
        return Vec::new();
    };

    if let Some(block_kind) = test_block_kind(&method_name)
        && is_test_block_call(base, node, block_kind)
    {
        return extract_test_block(base, node, &method_name, parent_id.as_deref(), block_kind)
            .into_iter()
            .collect();
    }
    if !is_self_directed_call(node) {
        return Vec::new();
    }

    let visibility = wrapping_visibility(base, node).unwrap_or_else(|| scope.visibility.clone());
    let member = |kind: SymbolKind| DefinedMember {
        parent_id: parent_id.clone(),
        visibility: visibility.clone(),
        kind,
        class_method: false,
    };
    match method_name.as_str() {
        "task" | "namespace" if is_rake_file(&base.file_path) => {
            extract_rake_definition(base, node, &method_name, parent_id)
                .into_iter()
                .collect()
        }
        "require" | "require_relative" => extract_require(base, node).into_iter().collect(),
        "attr_reader" | "attr_writer" | "attr_accessor" => {
            extract_attr_accessor(base, node, &method_name, parent_id, visibility)
        }
        "define_method" => {
            let names = symbol_arguments(base, node).into_iter().take(1).collect();
            define_members(base, node, names, member(SymbolKind::Method))
        }
        "define_singleton_method" => {
            let names = symbol_arguments(base, node).into_iter().take(1).collect();
            let mut member = member(SymbolKind::Method);
            member.class_method = true;
            define_members(base, node, names, member)
        }
        "alias_method" => {
            let names = symbol_arguments(base, node).into_iter().take(1).collect();
            define_members(base, node, names, member(SymbolKind::Method))
        }
        "def_delegator" | "def_instance_delegator" => {
            let names = def_delegator_name(base, node).into_iter().collect();
            define_members(base, node, names, member(SymbolKind::Method))
        }
        "def_delegators" | "def_instance_delegators" => {
            let names = symbol_arguments(base, node).into_iter().skip(1).collect();
            define_members(base, node, names, member(SymbolKind::Method))
        }
        "delegate" => {
            let names = symbol_arguments(base, node);
            define_members(base, node, names, member(SymbolKind::Method))
        }
        "has_many" | "has_one" | "belongs_to" | "has_and_belongs_to_many" => {
            let names = symbol_arguments(base, node).into_iter().take(1).collect();
            define_members(base, node, names, member(SymbolKind::Property))
        }
        "scope" => {
            let names = symbol_arguments(base, node).into_iter().take(1).collect();
            let mut member = member(SymbolKind::Method);
            member.class_method = true;
            define_members(base, node, names, member)
        }
        _ => Vec::new(),
    }
}

fn is_rake_file(file_path: &str) -> bool {
    let file_name = file_path.rsplit(['/', '\\']).next().unwrap_or(file_path);
    file_name.ends_with(".rake") || file_name == "Rakefile"
}

/// A Rake `namespace :db do` (namespace) or `task seed: :environment do`
/// (function), so calls in task bodies have a containing symbol. A preceding
/// `desc "..."` call is the task's doc comment.
fn extract_rake_definition(
    base: &mut BaseExtractor,
    node: Node,
    method_name: &str,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let is_namespace = method_name == "namespace";
    if is_namespace && call_block(node).is_none() {
        return None;
    }
    let arguments = node.child_by_field_name("arguments")?;
    let first = arguments.named_child(0)?;
    let name = match first.kind() {
        "pair" => {
            let key = first.child_by_field_name("key")?;
            match key.kind() {
                "hash_key_symbol" => Some(base.get_node_text(&key)),
                _ => symbol_name(base, key),
            }
        }
        _ => symbol_name(base, first),
    }?;
    let doc_comment = node
        .prev_named_sibling()
        .filter(|previous| {
            previous.kind() == "call"
                && extract_method_name_from_call(*previous, |n| base.get_node_text(n)).as_deref()
                    == Some("desc")
        })
        .and_then(|desc| desc.child_by_field_name("arguments")?.named_child(0))
        .and_then(|description| base.decode_string_literal(&description));
    let kind = if is_namespace {
        SymbolKind::Namespace
    } else {
        SymbolKind::Function
    };
    let signature = call_head_text(base, node);
    Some(base.create_symbol(
        &node,
        name,
        kind,
        SymbolOptions {
            signature: Some(signature),
            parent_id,
            doc_comment,
            ..SymbolOptions::default()
        },
    ))
}

/// A `private :a, :b` style call: it changes the visibility of methods the
/// enclosing class declares, wherever they are written.
pub(super) struct VisibilityCall {
    parent_id: Option<String>,
    names: Vec<String>,
    visibility: Visibility,
    class_method: bool,
}

/// The visibility change a `private :name` / `protected :name` / `public
/// :name` / `private_class_method :name` call makes. A bare `private` with no
/// argument is handled by the traversal; `private def x` and `private
/// attr_reader :x` by their own arms.
pub(super) fn visibility_call(
    base: &BaseExtractor,
    node: Node,
    parent_id: Option<String>,
    scope: &MemberScope,
) -> Option<VisibilityCall> {
    if !is_self_directed_call(node) {
        return None;
    }
    let method_name = extract_method_name_from_call(node, |n| base.get_node_text(n))?;
    let (visibility, class_method) = match method_name.as_str() {
        "private" => (Visibility::Private, scope.class_method),
        "protected" => (Visibility::Protected, scope.class_method),
        "public" => (Visibility::Public, scope.class_method),
        "private_class_method" => (Visibility::Private, true),
        "public_class_method" => (Visibility::Public, true),
        _ => return None,
    };
    let names = symbol_arguments(base, node);
    (!names.is_empty()).then_some(VisibilityCall {
        parent_id,
        names,
        visibility,
        class_method,
    })
}

/// Apply every recorded `private :name` call to the named members of its
/// class: instance members for `private`, class methods for
/// `private_class_method`.
pub(super) fn apply_visibility_calls(symbols: &mut [Symbol], calls: &[VisibilityCall]) {
    for call in calls {
        for symbol in symbols.iter_mut() {
            let is_class_method = symbol
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("isStatic"))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            if symbol.parent_id == call.parent_id
                && matches!(symbol.kind, SymbolKind::Method | SymbolKind::Property)
                && is_class_method == call.class_method
                && call.names.contains(&symbol.name)
            {
                symbol.visibility = Some(call.visibility.clone());
            }
        }
    }
}

/// The visibility a wrapping `private attr_reader :x` call states.
fn wrapping_visibility(base: &BaseExtractor, node: Node) -> Option<Visibility> {
    let wrapper = node
        .parent()
        .filter(|parent| parent.kind() == "argument_list")?
        .parent()
        .filter(|parent| parent.kind() == "call" && is_self_directed_call(*parent))?;
    let method = extract_method_name_from_call(wrapper, |n| base.get_node_text(n))?;
    super::helpers::parse_visibility(&method).filter(|_| method != "module_function")
}

/// The names given as symbol or string arguments (`:a, "b"`), in order.
/// Keyword pairs such as `to: :user` are not names.
fn symbol_arguments(base: &BaseExtractor, node: Node) -> Vec<String> {
    let Some(arguments) = node.child_by_field_name("arguments") else {
        return Vec::new();
    };
    let mut cursor = arguments.walk();
    arguments
        .named_children(&mut cursor)
        .filter_map(|argument| symbol_name(base, argument))
        .collect()
}

pub(super) fn symbol_name(base: &BaseExtractor, node: Node) -> Option<String> {
    match node.kind() {
        "simple_symbol" => Some(
            base.get_node_text(&node)
                .trim_start_matches(':')
                .to_string(),
        ),
        "delimited_symbol" | "string" => base.decode_string_literal(&node),
        _ => None,
    }
}

/// `def_delegator :@points, :first, :origin` defines `origin`; without the
/// alias argument it defines `first`.
fn def_delegator_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let names = symbol_arguments(base, node);
    let mut rest = names.into_iter().skip(1);
    let method = rest.next()?;
    Some(rest.next().unwrap_or(method))
}

struct DefinedMember {
    parent_id: Option<String>,
    visibility: Visibility,
    kind: SymbolKind,
    class_method: bool,
}

/// One symbol per name a metaprogramming or Rails macro call defines
/// (`define_method`, `alias_method`, `delegate`, `has_many`, `scope`, ...),
/// parented to the enclosing class with the visibility in force.
fn define_members(
    base: &mut BaseExtractor,
    node: Node,
    names: Vec<String>,
    member: DefinedMember,
) -> Vec<Symbol> {
    let signature = base.get_node_text(&node);
    let signature = signature
        .lines()
        .next()
        .unwrap_or("")
        .trim_end()
        .to_string();
    let doc_comment = super::doc_comments::find_ruby_doc_comment(base, node);
    names
        .into_iter()
        .map(|name| {
            let metadata = member.class_method.then(|| {
                let mut metadata = HashMap::new();
                super::symbols::class_method_metadata(&mut metadata);
                metadata
            });
            base.create_symbol(
                &node,
                name,
                member.kind.clone(),
                SymbolOptions {
                    signature: Some(signature.clone()),
                    visibility: Some(member.visibility.clone()),
                    parent_id: member.parent_id.clone(),
                    metadata,
                    doc_comment: doc_comment.clone(),
                    annotations: Vec::new(),
                },
            )
        })
        .collect()
}

/// Try to extract a class-declaring assignment as a Class symbol:
/// `Name = Struct.new(:a, :b)`, `Name = Data.define(:a, :b)`, and
/// `Name = Class.new(Base)`, each with an optional block of methods.
///
/// Returns `(class_symbol, field_properties)` so the caller can set `symbol_opt`
/// to the class (for child parenting) and push the properties into the symbols vec.
pub(super) fn try_extract_struct_new(
    base: &mut BaseExtractor,
    assignment_node: Node,
    parent_id: Option<String>,
) -> Option<(Symbol, Vec<Symbol>)> {
    let left_side = assignment_node.child_by_field_name("left")?;
    let right_side = assignment_node.child_by_field_name("right")?;
    if left_side.kind() != "constant" {
        return None;
    }
    let constructor = class_constructor(base, right_side)?;

    let name = base.get_node_text(&left_side);
    let receiver = base.get_node_text(&right_side.child_by_field_name("receiver")?);
    let method = base.get_node_text(&right_side.child_by_field_name("method")?);
    let arguments = right_side
        .child_by_field_name("arguments")
        .map(|arg_list| base.get_node_text(&arg_list))
        .unwrap_or_default();
    let signature = format!("{name} = {receiver}.{method}{arguments}");

    let doc_comment = super::doc_comments::find_ruby_doc_comment(base, assignment_node);
    let metadata = match constructor {
        ClassConstructor::Class => class_new_superclass(base, right_side).map(|superclass| {
            HashMap::from([(
                "base_types".to_string(),
                serde_json::Value::Array(vec![serde_json::Value::String(superclass)]),
            )])
        }),
        ClassConstructor::Fields => None,
    };

    let class_symbol = base.create_symbol(
        &assignment_node,
        name,
        SymbolKind::Class,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id,
            metadata,
            doc_comment,
            annotations: Vec::new(),
        },
    );

    let mut field_properties = Vec::new();
    if matches!(constructor, ClassConstructor::Fields)
        && let Some(arg_list) = right_side.child_by_field_name("arguments")
    {
        let mut arg_cursor = arg_list.walk();
        for arg_child in arg_list.children(&mut arg_cursor) {
            if arg_child.kind() == "simple_symbol" {
                let field_text = base.get_node_text(&arg_child);
                let field_name = field_text.trim_start_matches(':').to_string();
                let prop = base.create_symbol(
                    &arg_child,
                    field_name,
                    SymbolKind::Property,
                    SymbolOptions {
                        signature: Some(field_text),
                        visibility: Some(Visibility::Public),
                        parent_id: Some(class_symbol.id.clone()),
                        metadata: None,
                        doc_comment: None,
                        annotations: Vec::new(),
                    },
                );
                field_properties.push(prop);
            }
        }
    }

    Some((class_symbol, field_properties))
}

#[derive(Clone, Copy)]
enum ClassConstructor {
    /// `Struct.new(:a)` and `Data.define(:a)`: symbol arguments are members.
    Fields,
    /// `Class.new(Base)`: the argument is the superclass.
    Class,
}

fn class_constructor(base: &BaseExtractor, call: Node) -> Option<ClassConstructor> {
    if call.kind() != "call" {
        return None;
    }
    let receiver = call.child_by_field_name("receiver")?;
    if receiver.kind() != "constant" {
        return None;
    }
    let method = base.get_node_text(&call.child_by_field_name("method")?);
    match (base.get_node_text(&receiver).as_str(), method.as_str()) {
        ("Struct", "new") | ("Data", "define") => Some(ClassConstructor::Fields),
        ("Class", "new") => Some(ClassConstructor::Class),
        _ => None,
    }
}

/// The superclass constant of `Class.new(Base)`.
pub(super) fn class_new_superclass(base: &BaseExtractor, call: Node) -> Option<String> {
    let arguments = call.child_by_field_name("arguments")?;
    let first = arguments.named_child(0)?;
    matches!(first.kind(), "constant" | "scope_resolution").then(|| base.get_node_text(&first))
}

/// Whether `call` is `Class.new(...)`.
pub(super) fn is_class_new(base: &BaseExtractor, call: Node) -> bool {
    matches!(class_constructor(base, call), Some(ClassConstructor::Class))
}

/// Extract require/require_relative calls
fn extract_require(base: &mut BaseExtractor, node: Node) -> Option<Symbol> {
    let arg_node = node.child_by_field_name("arguments")?;
    let string_node = arg_node
        .children(&mut arg_node.walk())
        .find(|c| c.kind() == "string")?;

    let require_path = base.get_node_text(&string_node).replace(['\'', '"'], "");
    let module_name = require_path
        .split('/')
        .next_back()
        .unwrap_or(&require_path)
        .to_string();
    let method_name = extract_method_name_from_call(node, |n| base.get_node_text(n))?;

    Some(base.create_symbol(
        &node,
        module_name,
        SymbolKind::Import,
        SymbolOptions {
            signature: Some(format!(
                "{} {}",
                method_name,
                base.get_node_text(&string_node)
            )),
            visibility: Some(Visibility::Public),
            parent_id: None,
            metadata: None,
            doc_comment: None,
            annotations: Vec::new(),
        },
    ))
}

/// The shape a recognised test-block call takes in the source.
///
/// The shape decides where the symbol's name comes from and which symbol kind
/// it becomes; [`TestBlockKind::role`] decides the test role.
#[derive(Clone, Copy)]
enum TestBlockKind {
    /// `describe`/`context`/`shared_examples` — named by the first argument.
    Container,
    /// `it`/`specify`/`xit` — named by the first argument.
    Example,
    /// `before`/`after`/`around`/`setup`/`teardown` — named by the hook itself.
    Lifecycle,
    /// `let`/`let!`/`subject` — named by the first argument, or by the hook when
    /// the call takes no argument.
    Fixture,
    /// The Rails `test "name" do` macro — named by its string argument.
    RailsCase,
}

impl TestBlockKind {
    fn role(self, method_name: &str) -> Option<TestRole> {
        match self {
            TestBlockKind::Container => Some(TestRole::TestContainer),
            TestBlockKind::Example | TestBlockKind::RailsCase => Some(TestRole::TestCase),
            TestBlockKind::Lifecycle => ruby_block_test_role(method_name),
            TestBlockKind::Fixture => Some(TestRole::FixtureSetup),
        }
    }

    fn symbol_kind(self) -> SymbolKind {
        match self {
            TestBlockKind::Container => SymbolKind::Namespace,
            _ => SymbolKind::Function,
        }
    }

    /// Whether the call must carry a `do`/`{` block to count.
    ///
    /// A bare `subject` or `setup` with no block is an ordinary reference or
    /// method call. An `it "is pending"` with no block is a real RSpec pending
    /// example, so examples and containers do not require one.
    fn requires_block(self) -> bool {
        matches!(
            self,
            TestBlockKind::Lifecycle | TestBlockKind::Fixture | TestBlockKind::RailsCase
        )
    }
}

/// A `before { ... }` style hook block. Nothing calls a hook by name, so a
/// bare `before` elsewhere (a Sinatra filter in a mock app) must not resolve
/// to it.
pub(super) fn is_hook_block_symbol(symbol: &Symbol) -> bool {
    symbol.kind == SymbolKind::Function
        && matches!(
            test_block_kind(&symbol.name),
            Some(TestBlockKind::Lifecycle)
        )
        && !symbol
            .signature
            .as_deref()
            .is_some_and(|signature| signature.starts_with("def "))
}

fn test_block_kind(method_name: &str) -> Option<TestBlockKind> {
    match method_name {
        "describe"
        | "context"
        | "feature"
        | "xdescribe"
        | "xcontext"
        | "fdescribe"
        | "fcontext"
        | "shared_examples"
        | "shared_examples_for"
        | "shared_context" => Some(TestBlockKind::Container),
        "it" | "specify" | "example" | "scenario" | "xit" | "fit" | "xspecify" | "fspecify"
        | "xexample" | "fexample" => Some(TestBlockKind::Example),
        "before" | "after" | "around" | "setup" | "teardown" => Some(TestBlockKind::Lifecycle),
        "let" | "let!" | "subject" | "subject!" => Some(TestBlockKind::Fixture),
        "test" => Some(TestBlockKind::RailsCase),
        _ => None,
    }
}

/// Whether this call site may be read as a test block.
///
/// Two guards keep production Ruby clean. The file must be a test path, because
/// every name in the vocabulary is ordinary Ruby elsewhere. And the call must be
/// bare or sent to `RSpec` itself — `runner.describe "x"` sends an ordinary
/// message to an object that happens to answer `describe`.
///
/// A hook (`before`, `after`, ...) is a hook only directly in an example group
/// or a test class. Inside an example or a `let`/`subject` body it is an
/// ordinary call, such as a Sinatra filter in a mock app.
fn is_test_block_call(base: &BaseExtractor, node: Node, block_kind: TestBlockKind) -> bool {
    if !is_test_path(&base.file_path) {
        return false;
    }
    let receiver_ok = matches!(
        extract_call_receiver(node, |n| base.get_node_text(n)).as_deref(),
        None | Some("RSpec")
    );
    receiver_ok
        && (!matches!(block_kind, TestBlockKind::Lifecycle) || is_in_example_group(base, node))
}

/// The nearest enclosing test block of `node` is an example group (or there is
/// none, as in a Minitest class body or at file level).
fn is_in_example_group(base: &BaseExtractor, node: Node) -> bool {
    let mut current = node.parent();
    while let Some(ancestor) = current {
        match ancestor.kind() {
            "class" | "module" | "program" => return true,
            "call" => {
                let enclosing_kind =
                    extract_method_name_from_call(ancestor, |n| base.get_node_text(n))
                        .and_then(|name| test_block_kind(&name))
                        .filter(|_| {
                            matches!(
                                extract_call_receiver(ancestor, |n| base.get_node_text(n))
                                    .as_deref(),
                                None | Some("RSpec")
                            )
                        });
                if let Some(kind) = enclosing_kind {
                    return matches!(kind, TestBlockKind::Container);
                }
            }
            _ => {}
        }
        current = ancestor.parent();
    }
    true
}

/// The call as written up to its block: `it 'maps "" to nil'`, `describe Order`.
fn call_head_text(base: &BaseExtractor, node: Node) -> String {
    let end = node
        .child_by_field_name("arguments")
        .or_else(|| node.child_by_field_name("method"))
        .map_or(node.end_byte(), |head| head.end_byte());
    base.content
        .get(node.start_byte()..end)
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn call_block(node: Node<'_>) -> Option<Node<'_>> {
    node.child_by_field_name("block")
}

fn extract_test_block(
    base: &mut BaseExtractor,
    node: Node,
    method_name: &str,
    parent_id: Option<&str>,
    block_kind: TestBlockKind,
) -> Option<Symbol> {
    if block_kind.requires_block() && call_block(node).is_none() {
        return None;
    }

    let block_name = match block_kind {
        TestBlockKind::Lifecycle => method_name.to_string(),
        TestBlockKind::Fixture => {
            extract_first_rspec_argument(base, node).unwrap_or_else(|| method_name.to_string())
        }
        TestBlockKind::RailsCase => extract_first_string_argument(base, node)?,
        TestBlockKind::Container => extract_first_rspec_argument(base, node)?,
        // A one-liner `it { is_expected.to ... }` has no description; RSpec
        // names it by its location.
        TestBlockKind::Example => extract_first_rspec_argument(base, node).or_else(|| {
            (node.child_by_field_name("arguments").is_none() && call_block(node).is_some())
                .then(|| format!("example at line {}", node.start_position().row + 1))
        })?,
    };

    let signature = match block_kind {
        TestBlockKind::Lifecycle => format!("{method_name}()"),
        _ => call_head_text(base, node),
    };

    let mut metadata = HashMap::new();
    if let Some(role) = block_kind.role(method_name) {
        apply_test_role(&mut metadata, role);
    }
    let annotations = match block_kind {
        TestBlockKind::Container | TestBlockKind::Example => rspec_metadata_tags(base, node),
        _ => Vec::new(),
    };

    Some(base.create_symbol(
        &node,
        block_name,
        block_kind.symbol_kind(),
        SymbolOptions {
            signature: Some(signature),
            visibility: None,
            parent_id: parent_id.map(str::to_string),
            metadata: Some(metadata),
            doc_comment: None,
            annotations,
        },
    ))
}

/// RSpec metadata tags after an example or group description: a symbol tag
/// (`:slow`) and each hash tag (`type: :model`), carrier `rspec_metadata`.
fn rspec_metadata_tags(base: &BaseExtractor, node: Node) -> Vec<AnnotationMarker> {
    let Some(arguments) = node.child_by_field_name("arguments") else {
        return Vec::new();
    };
    let mut cursor = arguments.walk();
    let mut tags = Vec::new();
    for argument in arguments.named_children(&mut cursor).skip(1) {
        match argument.kind() {
            "simple_symbol" => {
                let raw = base.get_node_text(&argument);
                let name = raw.trim_start_matches(':').to_string();
                tags.push(rspec_tag(name, raw));
            }
            "pair" => {
                let Some(key) = argument.child_by_field_name("key") else {
                    continue;
                };
                let name = base
                    .get_node_text(&key)
                    .trim_start_matches(':')
                    .trim_end_matches(':')
                    .to_string();
                tags.push(rspec_tag(name, base.get_node_text(&argument)));
            }
            _ => {}
        }
    }
    tags
}

fn rspec_tag(name: String, raw_text: String) -> AnnotationMarker {
    AnnotationMarker {
        annotation: name.clone(),
        annotation_key: name,
        raw_text: Some(raw_text),
        carrier: Some("rspec_metadata".to_string()),
    }
}

fn extract_first_rspec_argument(base: &BaseExtractor, node: Node) -> Option<String> {
    let arg_node = node.child_by_field_name("arguments")?;
    let first_arg = arg_node.children(&mut arg_node.walk()).find(|child| {
        matches!(
            child.kind(),
            "string" | "simple_symbol" | "symbol" | "constant" | "identifier" | "scope_resolution"
        )
    })?;
    let raw = base.get_node_text(&first_arg);

    Some(match first_arg.kind() {
        "string" => base.decode_string_literal(&first_arg).unwrap_or(raw),
        "simple_symbol" | "symbol" => raw.trim_start_matches(':').to_string(),
        _ => raw,
    })
}

/// First argument of a call, only when it is a string literal.
///
/// The Rails `test` macro names a case with a string. `test some_variable do`
/// is not the macro form and must not become a case.
fn extract_first_string_argument(base: &BaseExtractor, node: Node) -> Option<String> {
    let arg_node = node.child_by_field_name("arguments")?;
    let first_arg = arg_node
        .children(&mut arg_node.walk())
        .find(|child| !matches!(child.kind(), "(" | ")" | ","))?;
    if first_arg.kind() != "string" {
        return None;
    }
    base.decode_string_literal(&first_arg)
}

/// Extract attr_reader/attr_writer/attr_accessor calls.
fn extract_attr_accessor(
    base: &mut BaseExtractor,
    node: Node,
    method_name: &str,
    parent_id: Option<String>,
    visibility: Visibility,
) -> Vec<Symbol> {
    let Some(arg_node) = node.child_by_field_name("arguments") else {
        return Vec::new();
    };
    let symbol_nodes: Vec<_> = arg_node
        .children(&mut arg_node.walk())
        .filter(|c| matches!(c.kind(), "simple_symbol" | "symbol"))
        .collect();

    let doc_comment = super::doc_comments::find_ruby_doc_comment(base, node);
    symbol_nodes
        .into_iter()
        .map(|symbol_node| {
            let attr_name = base.get_node_text(&symbol_node).replace(':', "");
            let signature = format!("{} :{}", method_name, attr_name);
            base.create_symbol(
                &symbol_node,
                attr_name,
                SymbolKind::Property,
                SymbolOptions {
                    signature: Some(signature),
                    visibility: Some(visibility.clone()),
                    parent_id: parent_id.clone(),
                    metadata: None,
                    doc_comment: doc_comment.clone(),
                    annotations: Vec::new(),
                },
            )
        })
        .collect()
}
