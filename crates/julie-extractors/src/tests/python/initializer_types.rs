use crate::base::{Symbol, SymbolKind, TypeInfo};
use crate::python::PythonExtractor;
use std::path::PathBuf;

fn extract(source: &str) -> (Vec<Symbol>, PythonExtractor) {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let mut extractor = PythonExtractor::new(
        "initializer_types.py".to_string(),
        source.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    (symbols, extractor)
}

fn fact_of<'a>(
    extractor: &'a PythonExtractor,
    symbols: &[Symbol],
    name: &str,
    kind: SymbolKind,
) -> Option<&'a TypeInfo> {
    let symbol = symbols
        .iter()
        .find(|s| s.name == name && s.kind == kind)
        .unwrap_or_else(|| panic!("missing symbol {name}"));
    extractor.base.type_info.get(&symbol.id)
}

fn inferred(source: &str, name: &str) -> Option<(String, Option<String>)> {
    let (symbols, extractor) = extract(source);
    fact_of(&extractor, &symbols, name, SymbolKind::Variable).map(|fact| {
        assert!(fact.is_inferred, "fact for {name} is not inferred");
        let declared = fact
            .metadata
            .as_ref()
            .and_then(|m| m.get("declared"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
        (fact.resolved_type.clone(), declared)
    })
}

fn inferred_type(source: &str, name: &str) -> Option<String> {
    inferred(source, name).map(|(resolved, _)| resolved)
}

#[test]
fn free_function_call_records_declared_return_type() {
    let source = r#"
def load() -> Workspace:
    ...

def run():
    ws = load()
"#;
    assert_eq!(
        inferred(source, "ws"),
        Some(("Workspace".to_string(), None))
    );
}

#[test]
fn module_level_call_records_declared_return_type() {
    let source = r#"
def load() -> Workspace:
    ...

ws = load()
"#;
    assert_eq!(inferred_type(source, "ws").as_deref(), Some("Workspace"));
}

#[test]
fn awaited_async_function_records_declared_return_type() {
    let source = r#"
async def load() -> Workspace:
    ...

async def run():
    ws = await load()
"#;
    assert_eq!(inferred_type(source, "ws").as_deref(), Some("Workspace"));
}

#[test]
fn async_function_called_without_await_records_nothing() {
    let source = r#"
async def load() -> Workspace:
    ...

def run():
    pending = load()
"#;
    assert_eq!(inferred_type(source, "pending"), None);
}

#[test]
fn awaited_sync_function_records_nothing() {
    let source = r#"
def load() -> Workspace:
    ...

async def run():
    ws = await load()
"#;
    assert_eq!(inferred_type(source, "ws"), None);
}

#[test]
fn self_method_call_records_declared_return_type() {
    let source = r#"
class Service:
    def load(self) -> Workspace:
        ...

    async def fetch(self) -> Workspace:
        ...

    async def run(self):
        ws = self.load()
        fetched = await self.fetch()
"#;
    assert_eq!(inferred_type(source, "ws").as_deref(), Some("Workspace"));
    assert_eq!(
        inferred_type(source, "fetched").as_deref(),
        Some("Workspace")
    );
}

#[test]
fn self_call_to_a_method_of_another_class_records_nothing() {
    let source = r#"
class Other:
    def load(self) -> Workspace:
        ...

class Service:
    def run(self):
        ws = self.load()
"#;
    assert_eq!(inferred_type(source, "ws"), None);
}

#[test]
fn self_call_outside_a_class_records_nothing() {
    let source = r#"
class Service:
    def load(self) -> Workspace:
        ...

def run(self):
    ws = self.load()
"#;
    assert_eq!(inferred_type(source, "ws"), None);
}

#[test]
fn bare_call_of_a_method_name_records_nothing() {
    let source = r#"
class Service:
    def load(self) -> Workspace:
        ...

    def run(self):
        ws = load()
"#;
    assert_eq!(inferred_type(source, "ws"), None);
}

#[test]
fn class_method_call_on_same_file_class_resolves_self() {
    let source = r#"
class Workspace:
    @classmethod
    def create(cls) -> Self:
        ...

    @staticmethod
    def default_name() -> Name:
        ...

def run():
    ws = Workspace.create()
    name = Workspace.default_name()
"#;
    assert_eq!(
        inferred(source, "ws"),
        Some(("Workspace".to_string(), Some("Self".to_string())))
    );
    assert_eq!(inferred_type(source, "name").as_deref(), Some("Name"));
}

#[test]
fn cls_call_inside_classmethod_resolves_the_enclosing_class() {
    let source = r#"
class Workspace:
    @classmethod
    def create(cls) -> "Workspace":
        ...

    @classmethod
    def clone(cls):
        ws = cls.create()
"#;
    assert_eq!(inferred_type(source, "ws").as_deref(), Some("Workspace"));
}

#[test]
fn self_return_type_on_free_function_records_nothing() {
    let source = r#"
def load() -> Self:
    ...

ws = load()
"#;
    assert_eq!(inferred_type(source, "ws"), None);
}

#[test]
fn call_on_unknown_receiver_records_nothing() {
    let source = r#"
class Workspace:
    def load(self) -> Repo:
        ...

def run(client):
    repo = client.load()
"#;
    assert_eq!(inferred_type(source, "repo"), None);
}

#[test]
fn forward_reference_and_optional_returns_unwrap_like_annotations() {
    let source = r#"
def load() -> "Workspace":
    ...

def find() -> Optional[Workspace]:
    ...

def lookup() -> Workspace | None:
    ...

def run():
    ws = load()
    found = find()
    looked = lookup()
"#;
    assert_eq!(inferred_type(source, "ws").as_deref(), Some("Workspace"));
    assert_eq!(
        inferred(source, "found"),
        Some((
            "Workspace".to_string(),
            Some("Optional[Workspace]".to_string())
        ))
    );
    assert_eq!(
        inferred_type(source, "looked").as_deref(),
        Some("Workspace")
    );
}

#[test]
fn union_of_two_real_return_types_records_nothing() {
    let source = r#"
def run_one() -> Union[Completed, BaseException]:
    ...

def run():
    outcome = run_one()
"#;
    assert_eq!(inferred_type(source, "outcome"), None);
}

#[test]
fn generic_container_return_records_the_container() {
    let source = r#"
def load_all() -> list[Workspace]:
    ...

def run():
    items = load_all()
"#;
    assert_eq!(
        inferred(source, "items"),
        Some(("list".to_string(), Some("list[Workspace]".to_string())))
    );
}

#[test]
fn chained_call_resolves_through_same_file_method() {
    let source = r#"
class Workspace:
    @classmethod
    def open(cls) -> Self:
        ...

    def root(self) -> Folder:
        ...

def run():
    root = Workspace.open().root()
    built = Workspace().root()
"#;
    assert_eq!(inferred_type(source, "root").as_deref(), Some("Folder"));
    assert_eq!(inferred_type(source, "built").as_deref(), Some("Folder"));
}

#[test]
fn chain_ending_in_unknown_method_records_nothing() {
    let source = r#"
def load() -> Workspace:
    ...

def run():
    name = load().strip()
"#;
    assert_eq!(inferred_type(source, "name"), None);
}

#[test]
fn parenthesized_call_records_declared_return_type() {
    let source = r#"
async def load() -> Workspace:
    ...

async def run():
    ws = (await load())
"#;
    assert_eq!(inferred_type(source, "ws").as_deref(), Some("Workspace"));
}

#[test]
fn disagreeing_same_named_functions_record_nothing() {
    let source = r#"
if FAST:
    def load() -> Workspace:
        ...
else:
    def load() -> Archive:
        ...

def run():
    ws = load()
"#;
    assert_eq!(inferred_type(source, "ws"), None);
}

#[test]
fn unannotated_same_named_function_records_nothing() {
    let source = r#"
@overload
def load(key: int) -> Workspace:
    ...

@overload
def load(key: str) -> Workspace:
    ...

def load(key):
    ...

def run():
    ws = load(1)
"#;
    assert_eq!(inferred_type(source, "ws"), None);
}

#[test]
fn agreeing_overloads_record_the_shared_type() {
    let source = r#"
@overload
def load(key: int) -> Workspace:
    ...

@overload
def load(key: str) -> Workspace:
    ...

def load(key) -> Workspace:
    ...

def run():
    ws = load(1)
"#;
    assert_eq!(inferred_type(source, "ws").as_deref(), Some("Workspace"));
}

#[test]
fn type_variable_return_records_nothing() {
    let source = r#"
T = TypeVar("T")
P = typing.ParamSpec("P")

def first(items: list[T]) -> T:
    ...

def spec() -> Optional[P]:
    ...

def run():
    item = first(values)
    other = spec()
"#;
    assert_eq!(inferred_type(source, "item"), None);
    assert_eq!(inferred_type(source, "other"), None);
}

#[test]
fn pep_695_type_parameter_return_records_nothing() {
    let source = r#"
def first[T](items: list[T]) -> T:
    ...

class Box[U]:
    def get(self) -> U:
        ...

    def run(self):
        value = self.get()

def run():
    item = first(values)
"#;
    assert_eq!(inferred_type(source, "item"), None);
    assert_eq!(inferred_type(source, "value"), None);
}

#[test]
fn generic_base_parameter_return_records_nothing() {
    let source = r#"
from .types import T

class Box(Generic[T]):
    def get(self) -> T:
        ...

    def run(self):
        value = self.get()
"#;
    assert_eq!(inferred_type(source, "value"), None);
}

#[test]
fn wrapping_decorator_records_nothing() {
    let source = r#"
@contextmanager
def session() -> Iterator[Session]:
    ...

class Service:
    @property
    def factory(self) -> Factory:
        ...

    def run(self):
        made = self.factory()

def run():
    managed = session()
"#;
    assert_eq!(inferred_type(source, "managed"), None);
    assert_eq!(inferred_type(source, "made"), None);
}

#[test]
fn transparent_decorators_keep_the_declared_return_type() {
    let source = r#"
@functools.lru_cache(maxsize=None)
def load() -> Workspace:
    ...

class Service:
    @abstractmethod
    def fetch(self) -> Workspace:
        ...

    def run(self):
        fetched = self.fetch()

def run():
    ws = load()
"#;
    assert_eq!(inferred_type(source, "ws").as_deref(), Some("Workspace"));
    assert_eq!(
        inferred_type(source, "fetched").as_deref(),
        Some("Workspace")
    );
}

#[test]
fn call_to_function_in_another_file_records_nothing() {
    let source = r#"
from .loader import load

def run():
    ws = load()
"#;
    assert_eq!(inferred_type(source, "ws"), None);
}

#[test]
fn written_annotation_wins_over_call_inference() {
    let source = r#"
def load() -> Workspace:
    ...

def run():
    ws: Archive = load()
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact_of(&extractor, &symbols, "ws", SymbolKind::Variable).unwrap();
    assert_eq!(fact.resolved_type, "Archive");
    assert!(!fact.is_inferred);
}

#[test]
fn self_attribute_assignment_records_call_return_type() {
    let source = r#"
class Service:
    def load(self) -> Workspace:
        ...

    def __init__(self):
        self.ws = self.load()
"#;
    let (symbols, extractor) = extract(source);
    let fact = fact_of(&extractor, &symbols, "ws", SymbolKind::Property).unwrap();
    assert_eq!(fact.resolved_type, "Workspace");
    assert!(fact.is_inferred);
}

#[test]
fn unannotated_implementation_before_overloads_records_nothing() {
    let source = r#"
def load(key):
    ...

@overload
def load(key: int) -> Workspace:
    ...

@overload
def load(key: str) -> Workspace:
    ...

def run():
    ws = load(1)
"#;
    assert_eq!(inferred_type(source, "ws"), None);
}

#[test]
fn imported_type_variable_in_parameter_annotations_records_nothing() {
    let source = r#"
from other import T, U

def first(items: list[T]) -> T:
    ...

def pick(default: Optional[U] = None) -> U:
    ...

class Box:
    def first(self, items: list[T]) -> T:
        ...

    def run(self):
        value = self.first([1])

item = first([1])
chosen = pick()
"#;
    assert_eq!(inferred_type(source, "item"), None);
    assert_eq!(inferred_type(source, "chosen"), None);
    assert_eq!(inferred_type(source, "value"), None);
}

#[test]
fn same_file_class_in_parameter_annotations_keeps_the_return_type() {
    let source = r#"
class Config:
    ...

def merge(base: Config, extra: Config) -> Config:
    ...

merged = merge(a, b)
"#;
    assert_eq!(inferred_type(source, "merged").as_deref(), Some("Config"));
}

#[test]
fn type_variable_of_enclosing_function_records_nothing() {
    let source = r#"
from other import T

def outer(items: list[T]):
    def inner() -> T:
        ...

    value = inner()
"#;
    assert_eq!(inferred_type(source, "value"), None);
}

#[test]
fn parameter_shadowing_a_same_file_function_records_nothing() {
    let source = r#"
def load() -> Workspace:
    ...

def run(load):
    ws = load()

def run_default(load=None):
    ws_default = load()
"#;
    assert_eq!(inferred_type(source, "ws"), None);
    assert_eq!(inferred_type(source, "ws_default"), None);
}

#[test]
fn local_assignment_shadowing_a_same_file_function_records_nothing() {
    let source = r#"
def load() -> Workspace:
    ...

def run():
    load = make()
    ws = load()
"#;
    assert_eq!(inferred_type(source, "ws"), None);
}

#[test]
fn loop_and_with_targets_shadowing_a_same_file_function_record_nothing() {
    let source = r#"
def load() -> Workspace:
    ...

def run(things):
    for load in things:
        ws_loop = load()

def use(ctx):
    with ctx as load:
        ws_with = load()
"#;
    assert_eq!(inferred_type(source, "ws_loop"), None);
    assert_eq!(inferred_type(source, "ws_with"), None);
}

#[test]
fn module_rebinding_of_a_same_file_function_records_nothing() {
    let source = r#"
def load() -> Workspace:
    ...

load = lambda: 1
ws = load()
"#;
    assert_eq!(inferred_type(source, "ws"), None);
}

#[test]
fn module_import_beats_a_nested_def_of_the_same_name() {
    let source = r#"
from lib import get

def outer():
    def get() -> Repo:
        ...

    inner_repo = get()

module_repo = get()
"#;
    assert_eq!(inferred_type(source, "module_repo"), None);
    assert_eq!(inferred_type(source, "inner_repo").as_deref(), Some("Repo"));
}

#[test]
fn nested_def_is_visible_only_inside_its_function() {
    let source = r#"
def outer():
    def build() -> Repo:
        ...

    def helper():
        nested_repo = build()

def other():
    outside_repo = build()
"#;
    assert_eq!(
        inferred_type(source, "nested_repo").as_deref(),
        Some("Repo")
    );
    assert_eq!(inferred_type(source, "outside_repo"), None);
}

#[test]
fn parameter_shadowing_a_same_file_class_records_nothing() {
    let source = r#"
class Workspace:
    @classmethod
    def create(cls) -> Self:
        ...

def run(Workspace):
    ws = Workspace()
    created = Workspace.create()
"#;
    assert_eq!(inferred_type(source, "ws"), None);
    assert_eq!(inferred_type(source, "created"), None);
}

#[test]
fn method_under_a_conditional_in_a_class_body_stays_a_method() {
    let source = r#"
class Cond:
    if FLAG:
        def m(self) -> Workspace:
            ...
    else:
        pass

    def run(self):
        through_self = self.m()

bare = m()
"#;
    assert_eq!(inferred_type(source, "bare"), None);
    assert_eq!(
        inferred_type(source, "through_self").as_deref(),
        Some("Workspace")
    );
}

#[test]
fn bare_call_in_a_class_body_to_its_method_records_nothing() {
    let source = r#"
def m() -> Archive:
    ...

class Service:
    def m(self) -> Workspace:
        ...

    default = m()
"#;
    assert_eq!(inferred_type(source, "default"), None);
}

#[test]
fn imported_class_or_builtin_in_parameter_annotations_keeps_the_return_type() {
    let source = r#"
from .wrappers import Response

def prepare(path: str) -> str:
    ...

def head(parts: list[str]) -> str:
    ...

def lookup(table: Dict[str, Any]) -> Any:
    ...

class App:
    def process(self, response: Response) -> Response:
        ...

    def run(self, rv):
        processed = self.process(rv)

name = prepare("app.py")
first = head(["a"])
value = lookup({})
"#;
    assert_eq!(inferred_type(source, "name").as_deref(), Some("str"));
    assert_eq!(inferred_type(source, "first").as_deref(), Some("str"));
    assert_eq!(inferred_type(source, "value").as_deref(), Some("Any"));
    assert_eq!(
        inferred_type(source, "processed").as_deref(),
        Some("Response")
    );
}

#[test]
fn imported_type_variable_as_a_whole_parameter_annotation_records_nothing() {
    let source = r#"
from other import T, ModelT, _T_co

def same(x: T) -> T:
    ...

def save(model: ModelT) -> ModelT:
    ...

def read(source: _T_co) -> _T_co:
    ...

a = same(1)
b = save(m)
c = read(s)
"#;
    assert_eq!(inferred_type(source, "a"), None);
    assert_eq!(inferred_type(source, "b"), None);
    assert_eq!(inferred_type(source, "c"), None);
}

#[test]
fn imported_name_used_as_a_type_argument_records_nothing() {
    let source = r#"
from other import Item

def first(items: Sequence[Item]) -> Item:
    ...

def fallback(item: Optional[Item]) -> Item:
    ...

a = first(values)
b = fallback(None)
"#;
    assert_eq!(inferred_type(source, "a"), None);
    assert_eq!(inferred_type(source, "b").as_deref(), Some("Item"));
}
