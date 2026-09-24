use crate::base::{Symbol, TypeInfo};
use crate::fsharp::FSharpExtractor;
use crate::tests::helpers::init_parser;
use std::path::PathBuf;

fn extract(code: &str) -> (Vec<Symbol>, FSharpExtractor) {
    let tree = init_parser(code, "fsharp");
    let mut extractor = FSharpExtractor::new(
        "fsharp".to_string(),
        "initializer_types.fs".to_string(),
        code.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    (symbols, extractor)
}

fn fact_for<'a>(
    symbols: &[Symbol],
    extractor: &'a FSharpExtractor,
    name: &str,
) -> Option<&'a TypeInfo> {
    let symbol = symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap_or_else(|| panic!("missing symbol `{name}`"));
    extractor.base.type_info.get(&symbol.id)
}

fn assert_inferred(code: &str, name: &str, resolved: &str, declared: &str) {
    let (symbols, extractor) = extract(code);
    let fact = fact_for(&symbols, &extractor, name)
        .unwrap_or_else(|| panic!("missing type fact for `{name}`"));
    assert_eq!(fact.resolved_type, resolved, "resolved type of `{name}`");
    assert!(fact.is_inferred, "`{name}` should be inferred");
    let written = fact
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("declared"))
        .and_then(|value| value.as_str())
        .unwrap_or(&fact.resolved_type);
    assert_eq!(written, declared, "declared type of `{name}`");
}

fn assert_no_fact(code: &str, name: &str) {
    let (symbols, extractor) = extract(code);
    let fact = fact_for(&symbols, &extractor, name);
    assert!(
        fact.is_none(),
        "expected no type fact for `{name}`, got {fact:?}"
    );
}

#[test]
fn unit_call_to_same_file_function_records_its_return_type() {
    assert_inferred(
        r#"
module Domain =
  type Workspace() = class end
  let load () : Workspace = Workspace()
  let run () =
    let x = load ()
    x
"#,
        "x",
        "Workspace",
        "Workspace",
    );
}

#[test]
fn full_curried_application_records_the_return_type() {
    assert_inferred(
        r#"
module Domain =
  type Workspace() = class end
  let make (a: int) (b: int) : Workspace = Workspace()
  let run () =
    let w = make 1 2
    w
"#,
        "w",
        "Workspace",
        "Workspace",
    );
}

#[test]
fn partial_application_records_no_fact() {
    assert_no_fact(
        r#"
module Domain =
  type Workspace() = class end
  let make (a: int) (b: int) : Workspace = Workspace()
  let run () =
    let partial = make 1
    partial
"#,
        "partial",
    );
}

#[test]
fn pipeline_into_a_same_file_function_records_its_return_type() {
    assert_inferred(
        r#"
module Domain =
  type Workspace() = class end
  let make (a: int) (b: int) : Workspace = Workspace()
  let run () =
    let piped = 1 |> make 2
    piped
"#,
        "piped",
        "Workspace",
        "Workspace",
    );
}

#[test]
fn parenthesized_call_records_the_return_type() {
    assert_inferred(
        r#"
module Domain =
  type Workspace() = class end
  let load () : Workspace = Workspace()
  let run () =
    let wrapped = (load ())
    wrapped
"#,
        "wrapped",
        "Workspace",
        "Workspace",
    );
}

#[test]
fn generic_return_type_keeps_its_base_name() {
    assert_inferred(
        r#"
module Domain =
  type Workspace() = class end
  let loadAll () : Workspace list = []
  let run () =
    let all = loadAll ()
    all
"#,
        "all",
        "list",
        "Workspace list",
    );
}

#[test]
fn type_parameter_return_records_no_fact() {
    assert_no_fact(
        r#"
module Domain =
  let make (seed: 'T) : 'T = seed
  let run () =
    let value = make 1
    value
"#,
        "value",
    );
}

#[test]
fn written_type_wins_over_the_call_return_type() {
    let (symbols, extractor) = extract(
        r#"
module Domain =
  type Workspace() = class end
  let load () : Workspace = Workspace()
  let run () =
    let typed: IWorkspace = load ()
    typed
"#,
    );
    let fact = fact_for(&symbols, &extractor, "typed").expect("typed binding should have a fact");
    assert_eq!(fact.resolved_type, "IWorkspace");
    assert!(!fact.is_inferred);
}

#[test]
fn disagreeing_visible_same_named_functions_record_no_fact() {
    assert_no_fact(
        r#"
module Domain =
  type Workspace() = class end
  type Store() = class end
  let load () : Workspace = Workspace()
  let run () =
    let load () : Store = Store()
    let loaded = load ()
    loaded
"#,
        "loaded",
    );
}

#[test]
fn same_named_function_in_a_sibling_module_is_out_of_scope() {
    assert_inferred(
        r#"
module A =
  type Workspace() = class end
  let load () : Workspace = Workspace()
module B =
  type Store() = class end
  let load () : Store = Store()
  let run () =
    let loaded = load ()
    loaded
"#,
        "loaded",
        "Store",
        "Store",
    );
}

#[test]
fn value_binding_with_the_callee_name_records_no_fact() {
    assert_no_fact(
        r#"
module A =
  type Workspace() = class end
  let load () : Workspace = Workspace()
module B =
  let load = fun () -> 42
  let run () =
    let loaded = load ()
    loaded
"#,
        "loaded",
    );
}

#[test]
fn parameter_with_the_callee_name_records_no_fact() {
    assert_no_fact(
        r#"
module Domain =
  type Workspace() = class end
  let load () : Workspace = Workspace()
  let run (load: unit -> int) =
    let loaded = load ()
    loaded
"#,
        "loaded",
    );
}

#[test]
fn module_qualified_call_records_the_return_type() {
    assert_inferred(
        r#"
module Domain =
  type Workspace() = class end
  module Repo =
    let load () : Workspace = Workspace()
  let run () =
    let loaded = Repo.load ()
    loaded
"#,
        "loaded",
        "Workspace",
        "Workspace",
    );
}

#[test]
fn qualifier_that_is_not_the_owning_module_records_no_fact() {
    assert_no_fact(
        r#"
module Domain =
  type Workspace() = class end
  module Repo =
    let load () : Workspace = Workspace()
  let run () =
    let loaded = Other.load ()
    loaded
"#,
        "loaded",
    );
}

#[test]
fn self_member_call_records_the_member_return_type() {
    let code = r#"
module Domain =
  type Workspace() = class end
  type Store() =
    member this.Load() : Workspace = Workspace()
    member this.Run() =
      let viaThis = this.Load()
      viaThis
    member self.Go() =
      let viaSelf = self.Load()
      viaSelf
"#;
    assert_inferred(code, "viaThis", "Workspace", "Workspace");
    assert_inferred(code, "viaSelf", "Workspace", "Workspace");
}

#[test]
fn other_receiver_member_call_records_no_fact() {
    assert_no_fact(
        r#"
module Domain =
  type Workspace() = class end
  type Store() =
    member this.Load() : Workspace = Workspace()
    member this.Run(other: Store) =
      let fromOther = other.Load()
      fromOther
"#,
        "fromOther",
    );
}

#[test]
fn static_member_call_on_same_file_type_records_the_return_type() {
    assert_inferred(
        r#"
module Domain =
  type Store() =
    static member Create() : Store = Store()
  let run () =
    let created = Store.Create()
    created
"#,
        "created",
        "Store",
        "Store",
    );
}

#[test]
fn curried_static_member_records_declared_and_inferred_types() {
    let code = r#"
module Domain =
  type Store() =
    static member Make (a: int) (b: int) : Store = Store()
  let run () =
    let made = Store.Make 1 2
    made
"#;
    let (symbols, extractor) = extract(code);
    let declared = fact_for(&symbols, &extractor, "Make").expect("Make should have a fact");
    assert_eq!(declared.resolved_type, "Store");
    assert!(!declared.is_inferred);
    assert_inferred(code, "made", "Store", "Store");
}

#[test]
fn instance_member_called_through_the_type_name_records_no_fact() {
    assert_no_fact(
        r#"
module Domain =
  type Workspace() = class end
  type Store() =
    member this.Load() : Workspace = Workspace()
  let run () =
    let loaded = Store.Load()
    loaded
"#,
        "loaded",
    );
}

#[test]
fn object_expression_member_self_call_records_no_fact() {
    assert_no_fact(
        r#"
module Domain =
  type Workspace() = class end
  type Store() =
    member this.Load() : Workspace = Workspace()
    member this.Make() =
      { new System.IDisposable with
          member x.Dispose() =
            let inner = x.Load()
            ignore inner }
"#,
        "inner",
    );
}

#[test]
fn use_binding_records_the_call_return_type() {
    assert_inferred(
        r#"
module Domain =
  type Workspace() = class end
  let load () : Workspace = Workspace()
  let run () =
    use resource = load ()
    resource
"#,
        "resource",
        "Workspace",
        "Workspace",
    );
}

#[test]
fn plain_let_of_async_call_keeps_the_async_type() {
    assert_inferred(
        r#"
module Domain =
  type Workspace() = class end
  let loadAsync () : Async<Workspace> = async { return Workspace() }
  let run () =
    let pending = loadAsync ()
    pending
"#,
        "pending",
        "Async",
        "Async<Workspace>",
    );
}

#[test]
fn let_bang_in_async_unwraps_async() {
    assert_inferred(
        r#"
module Domain =
  type Workspace() = class end
  let loadAsync () : Async<Workspace> = async { return Workspace() }
  let run () =
    async {
      let! awaited = loadAsync ()
      return awaited
    }
"#,
        "awaited",
        "Workspace",
        "Workspace",
    );
}

#[test]
fn let_and_use_bang_in_task_unwrap_task() {
    let code = r#"
module Domain =
  type Workspace() = class end
  let loadTask () : System.Threading.Tasks.Task<Workspace> = task { return Workspace() }
  let run () =
    task {
      let! awaited = loadTask ()
      use! disposable = loadTask ()
      return awaited
    }
"#;
    assert_inferred(code, "awaited", "Workspace", "Workspace");
    assert_inferred(code, "disposable", "Workspace", "Workspace");
}

#[test]
fn let_bang_in_async_does_not_unwrap_task() {
    assert_no_fact(
        r#"
module Domain =
  type Workspace() = class end
  let loadTask () : System.Threading.Tasks.Task<Workspace> = task { return Workspace() }
  let run () =
    async {
      let! awaited = loadTask ()
      return awaited
    }
"#,
        "awaited",
    );
}

#[test]
fn let_bang_in_unknown_builder_records_no_fact() {
    assert_no_fact(
        r#"
module Domain =
  type Workspace() = class end
  let loadAsync () : Async<Workspace> = async { return Workspace() }
  let run () =
    result {
      let! bound = loadAsync ()
      return bound
    }
"#,
        "bound",
    );
}

#[test]
fn let_bang_of_non_wrapper_or_constructor_records_no_fact() {
    let code = r#"
module Domain =
  type Workspace() = class end
  let load () : Workspace = Workspace()
  let run () =
    async {
      let! fromCall = load ()
      let! fromConstructor = Workspace()
      return fromCall
    }
"#;
    assert_no_fact(code, "fromCall");
    assert_no_fact(code, "fromConstructor");
}

#[test]
fn let_bang_of_type_parameter_payload_records_no_fact() {
    assert_no_fact(
        r#"
module Domain =
  let loadAny<'T> () : Async<'T> = async { return Unchecked.defaultof<'T> }
  let run () =
    async {
      let! anything = loadAny ()
      return anything
    }
"#,
        "anything",
    );
}

#[test]
fn lambda_parameter_with_the_callee_name_records_no_fact() {
    assert_no_fact(
        r#"
module Domain =
  type Workspace() = class end
  let load () : Workspace = Workspace()
  let run loaders =
    loaders |> List.map (fun load ->
      let fromLambda = load ()
      fromLambda)
"#,
        "fromLambda",
    );
}

#[test]
fn match_bound_name_with_the_callee_name_records_no_fact() {
    assert_no_fact(
        r#"
module Domain =
  type Workspace() = class end
  let load () : Workspace = Workspace()
  let run choice =
    match choice with
    | Some load ->
      let fromMatch = load ()
      fromMatch
    | None -> 0
"#,
        "fromMatch",
    );
}

#[test]
fn loop_variable_with_the_callee_name_records_no_fact() {
    assert_no_fact(
        r#"
module Domain =
  type Workspace() = class end
  let load () : Workspace = Workspace()
  let run loaders =
    for load in loaders do
      let fromLoop = load ()
      ignore fromLoop
"#,
        "fromLoop",
    );
}

#[test]
fn function_defined_after_the_call_records_no_fact() {
    assert_no_fact(
        r#"
module Domain =
  type Workspace() = class end
  let run () =
    let early = load ()
    early
  let load () : Workspace = Workspace()
"#,
        "early",
    );
}

#[test]
fn function_in_a_sibling_module_records_no_fact() {
    assert_no_fact(
        r#"
module A =
  type Workspace() = class end
  let load () : Workspace = Workspace()
module B =
  let run () =
    let fromSibling = load ()
    fromSibling
"#,
        "fromSibling",
    );
}

#[test]
fn function_in_an_enclosing_module_records_the_return_type() {
    assert_inferred(
        r#"
module Outer =
  type Workspace() = class end
  let load () : Workspace = Workspace()
  module Inner =
    let run () =
      let fromOuter = load ()
      fromOuter
"#,
        "fromOuter",
        "Workspace",
        "Workspace",
    );
}

#[test]
fn class_let_function_records_the_return_type_in_members() {
    assert_inferred(
        r#"
module Domain =
  type Workspace() = class end
  type Store() =
    let load () : Workspace = Workspace()
    member this.Run() =
      let fromClassLet = load ()
      fromClassLet
"#,
        "fromClassLet",
        "Workspace",
        "Workspace",
    );
}

const SELF_REBINDING: &str = r#"
type Item() =
    member this.Load() : string = ""
    member this.Create() : string = ""
type Store() =
    member this.Load() : int = 1
    member x.Run(items: Item list) =
        items |> List.map (fun x -> let lam = x.Load() in lam)
    member this.Run2(other: Item) =
        let f (this: Item) =
            let inner = this.Load()
            inner
        f other
    member this.Run3(o: Item) =
        match o with
        | this -> let m = this.Load() in m
    member me.Run4(o: Item) =
        for me in [o] do
            let loopv = me.Load()
            ignore loopv
    static member Create() : Store = Store()
let useStore (Store: Item) =
    let rcv = Store.Create()
    rcv
"#;

#[test]
fn lambda_rebinding_the_self_identifier_records_no_fact() {
    assert_no_fact(SELF_REBINDING, "lam");
}

#[test]
fn inner_function_parameter_named_like_the_self_identifier_records_no_fact() {
    assert_no_fact(SELF_REBINDING, "inner");
}

#[test]
fn match_rebinding_the_self_identifier_records_no_fact() {
    assert_no_fact(SELF_REBINDING, "m");
}

#[test]
fn for_loop_rebinding_the_self_identifier_records_no_fact() {
    assert_no_fact(SELF_REBINDING, "loopv");
}

#[test]
fn parameter_named_like_a_type_records_no_fact_for_a_static_call() {
    assert_no_fact(SELF_REBINDING, "rcv");
}

#[test]
fn self_call_is_kept_when_another_member_rebinds_the_name() {
    assert_inferred(
        r#"
type Store() =
    member this.Load() : int = 1
    member x.Map(items: int list) = items |> List.map (fun x -> x + 1)
    member x.Run() =
        let loaded = x.Load()
        loaded
"#,
        "loaded",
        "int",
        "int",
    );
}

#[test]
fn non_recursive_self_name_call_records_no_fact() {
    assert_no_fact(
        r#"
let readLines (path: string) : int =
    let lines = readLines path
    Seq.length lines
"#,
        "lines",
    );
}

#[test]
fn recursive_self_name_call_records_the_return_type() {
    assert_inferred(
        r#"
let rec countDown (n: int) : int =
    let next = countDown (n - 1)
    next
"#,
        "next",
        "int",
        "int",
    );
}

#[test]
fn qualified_call_to_a_sibling_nested_module_records_no_fact() {
    assert_no_fact(
        r#"
module Internal =
    module Repo =
        let load () : int = 1
module Public =
    let viaSibling = Repo.load ()
"#,
        "viaSibling",
    );
}

#[test]
fn static_call_to_a_type_in_a_sibling_module_records_no_fact() {
    assert_no_fact(
        r#"
module A =
    type Store() =
        static member Create() : int = 1
module B =
    let viaStatic = Store.Create()
"#,
        "viaStatic",
    );
}

#[test]
fn qualified_call_before_the_function_definition_records_no_fact() {
    assert_no_fact(
        r#"
module Repo =
    let early = Repo.late ()
    let late () : int = 1
"#,
        "early",
    );
}

const DESTRUCTURING: &str = r#"
type Pair = int * string
let pair () : Pair = (1, "a")
let a, b = pair ()
let arr () : string[] = [| "a" |]
let [| e1; e2 |] = arr ()
let pairs () : (int * string) list = []
let first :: rest = pairs ()
let tryPair () : (int * string) option = None
let (Some (n, s)) = tryPair ()
let (plain) = pair ()
"#;

#[test]
fn tuple_destructuring_records_no_fact() {
    assert_no_fact(DESTRUCTURING, "a");
}

#[test]
fn array_destructuring_records_no_fact() {
    assert_no_fact(DESTRUCTURING, "e1");
}

#[test]
fn cons_destructuring_records_no_fact() {
    assert_no_fact(DESTRUCTURING, "first");
}

#[test]
fn union_case_destructuring_records_no_fact() {
    assert_no_fact(DESTRUCTURING, "Some");
}

#[test]
fn parenthesized_identifier_pattern_records_the_return_type() {
    assert_inferred(DESTRUCTURING, "plain", "Pair", "Pair");
}

#[test]
fn type_written_on_the_pattern_wins_over_the_call_return_type() {
    let (symbols, extractor) = extract(
        r#"
type Base() = class end
type Derived() = inherit Base()
let makeD () : Derived = Derived()
let (typedPat: Base) = makeD ()
let ((nested: Base)) = Derived()
"#,
    );
    for name in ["typedPat", "nested"] {
        let fact = fact_for(&symbols, &extractor, name).expect("typed pattern should have a fact");
        assert_eq!(fact.resolved_type, "Base", "resolved type of `{name}`");
        assert!(!fact.is_inferred, "`{name}` should be declared");
    }
}

#[test]
fn written_type_after_a_function_parsed_as_a_value_pattern_is_kept() {
    let (symbols, extractor) = extract(
        r#"
module Core =
    type HttpHandler = int -> int
    let markdown (markdown: string) : HttpHandler =
        let bytes = markdown.Length

#if NET8_0_OR_GREATER
        let contentType = "text/markdown"
#else
        let contentType = "text/markdown"
#endif
        fun x -> x + bytes
"#,
    );
    let symbol = symbols
        .iter()
        .find(|symbol| symbol.name == "markdown" && symbol.start_line == 4)
        .expect("markdown binding should be a symbol");
    let fact = extractor
        .base
        .type_info
        .get(&symbol.id)
        .expect("markdown should keep its written type");
    assert_eq!(fact.resolved_type, "HttpHandler");
    assert!(!fact.is_inferred);
}

#[test]
fn self_call_to_a_same_named_member_of_another_type_records_no_fact() {
    assert_no_fact(
        r#"
type Base() =
    member this.Load() : string = "base"
module A =
    type Store() =
        member this.Load() : int = 1
module B =
    type Store() =
        inherit Base()
        member this.Run() =
            let inherited = this.Load()
            inherited
"#,
        "inherited",
    );
}

#[test]
fn explicit_interface_member_is_not_a_self_call_target() {
    assert_no_fact(
        r#"
type ILoader =
    abstract Load : unit -> string
type Base() =
    member this.Load() : int = 1
type Store() =
    inherit Base()
    interface ILoader with
        member this.Load() : string = "iface"
    member this.Run() =
        let viaIface = this.Load()
        viaIface
"#,
        "viaIface",
    );
}

#[test]
fn explicit_interface_member_is_skipped_without_inheritance() {
    assert_no_fact(
        r#"
type ILoader =
    abstract Load : unit -> string
type Store() =
    interface ILoader with
        member this.Load() : string = "iface"
    member this.Run() =
        let viaIface = this.Load()
        viaIface
"#,
        "viaIface",
    );
}

#[test]
fn self_call_in_an_inheriting_type_records_no_fact() {
    assert_no_fact(
        r#"
type Base() =
    member this.Load() : string = "base"
type Derived() =
    inherit Base()
    member this.Load(x: int) : int = x
    member this.Run() =
        let overloaded = this.Load()
        overloaded
"#,
        "overloaded",
    );
}

#[test]
fn self_call_to_an_object_member_name_records_no_fact() {
    assert_no_fact(
        r#"
type Named() =
    member this.ToString(x: int) : int = x
    member this.Run() =
        let s = this.ToString()
        s
"#,
        "s",
    );
}

#[test]
fn self_call_inside_a_type_extension_records_no_fact() {
    assert_no_fact(
        r#"
type Store() =
    member this.Id = 1
type Store with
    member this.Load() : int = 1
    member this.Run() =
        let extended = this.Load()
        extended
"#,
        "extended",
    );
}

#[test]
fn self_call_is_kept_next_to_an_interface_implementation() {
    assert_inferred(
        r#"
type ILoader =
    abstract Load : unit -> string
type Store() =
    interface ILoader with
        member this.Load() : string = "iface"
    member this.Count() : int = 1
    member this.Run() =
        let counted = this.Count()
        counted
"#,
        "counted",
        "int",
        "int",
    );
}

const OPENS_AFTER_DEFINITIONS: &str = r#"
module Helpers =
    let load () : string = "helper"
    type Store() =
        static member Create() : string = "helper"
module Main =
    let load () : int = 1
    type Store() =
        static member Create() : int = 1
    open Helpers
    let opened = load ()
    let openedStatic = Store.Create()
module Top =
    let load () : int = 1
    [<AutoOpen>]
    module Inner =
        let load () : string = "inner"
    let auto = load ()
module Typed =
    let load () : int = 1
    open type System.Math
    let openedType = load ()
"#;

#[test]
fn open_after_a_function_definition_records_no_fact() {
    assert_no_fact(OPENS_AFTER_DEFINITIONS, "opened");
}

#[test]
fn open_after_a_type_definition_records_no_fact_for_a_static_call() {
    assert_no_fact(OPENS_AFTER_DEFINITIONS, "openedStatic");
}

#[test]
fn auto_open_module_after_a_function_definition_records_no_fact() {
    assert_no_fact(OPENS_AFTER_DEFINITIONS, "auto");
}

#[test]
fn open_type_after_a_function_definition_records_no_fact() {
    assert_no_fact(OPENS_AFTER_DEFINITIONS, "openedType");
}

#[test]
fn definition_after_an_open_records_the_return_type() {
    let code = r#"
module Helpers =
    let load () : string = "h"
module Main =
    open Helpers
    let load () : int = 1
    type Store() =
        static member Create() : int = 1
    let afterOpen = load ()
    let afterOpenStatic = Store.Create()
"#;
    assert_inferred(code, "afterOpen", "int", "int");
    assert_inferred(code, "afterOpenStatic", "int", "int");
}

#[test]
fn nearer_type_without_the_member_hides_the_outer_type() {
    assert_no_fact(
        r#"
type Base() =
    static member Create() : string = "base"
type Store() =
    static member Create() : int = 1
module B =
    type Store() =
        inherit Base()
    let inheritedStatic = Store.Create()
"#,
        "inheritedStatic",
    );
}

#[test]
fn module_abbreviation_hides_the_outer_module() {
    assert_no_fact(
        r#"
module Helpers =
    let load () : string = "h"
module Repo =
    let load () : int = 1
module B =
    module Repo = Helpers
    let viaAlias = Repo.load ()
"#,
        "viaAlias",
    );
}

#[test]
fn nearer_type_that_declares_the_member_records_its_return_type() {
    assert_inferred(
        r#"
module Outer =
    type Store() =
        static member Create() : int = 1
    module Inner =
        type Store() =
            static member Create() : string = "inner"
        let nested = Store.Create()
"#,
        "nested",
        "string",
        "string",
    );
}

#[test]
fn qualifier_naming_both_a_module_and_a_type_records_no_fact() {
    assert_no_fact(
        r#"
type Store() =
    static member Create() : int = 1
module Store =
    let Create () : string = "m"
let ambiguous = Store.Create()
"#,
        "ambiguous",
    );
}

#[test]
fn static_call_on_an_inheriting_type_records_no_fact() {
    assert_no_fact(
        r#"
type Base() =
    static member Create() : string = "base"
type Store() =
    inherit Base()
    static member Create(x: int) : int = x
let inheritedCreate = Store.Create()
"#,
        "inheritedCreate",
    );
}

#[test]
fn flexible_return_type_records_no_fact() {
    assert_no_fact(
        r#"
let flexList () : #seq<int> = unbox (box [1;2])
let run () =
    let fl = flexList ()
    let l : int list = fl
    l
"#,
        "fl",
    );
}

#[test]
fn let_bang_of_a_flexible_payload_records_no_fact() {
    assert_no_fact(
        r#"
let loadAll () : Async<#seq<int>> = async { return unbox (box [1]) }
let run () =
    async {
        let! items = loadAll ()
        return items
    }
"#,
        "items",
    );
}

#[test]
fn union_case_after_a_function_hides_the_function() {
    assert_no_fact(
        "module A\nlet Load () : int = 1\ntype U = | Load of unit\nlet x1 = Load ()\n",
        "x1",
    );
}

#[test]
fn union_case_after_a_module_function_hides_the_qualified_call() {
    assert_no_fact(
        r#"
module Repo =
    let Load () : int = 1
    type U = | Load of unit
let qualifiedCase = Repo.Load ()
"#,
        "qualifiedCase",
    );
}

#[test]
fn exception_after_a_module_function_hides_the_qualified_call() {
    assert_no_fact(
        r#"
module Repo =
    let Boom () : int = 1
    exception Boom of unit
let boomed = Repo.Boom ()
"#,
        "boomed",
    );
}

#[test]
fn union_case_before_a_function_does_not_hide_it() {
    assert_inferred(
        "module A\ntype U = | Load of unit\nlet Load () : int = 1\nlet later = Load ()\n",
        "later",
        "int",
        "int",
    );
}

#[test]
fn require_qualified_access_union_case_does_not_hide_a_function() {
    assert_inferred(
        "module A\nlet Load () : int = 1\n[<RequireQualifiedAccess>]\ntype U = | Load of unit\nlet qualifiedOnly = Load ()\n",
        "qualifiedOnly",
        "int",
        "int",
    );
}

#[test]
fn and_binding_of_a_rec_group_hides_an_outer_function_from_the_group_start() {
    assert_no_fact(
        r#"
module B =
    let helper () : int = 1
    module Inner =
        let rec first () =
            let x2 = helper ()
            x2
        and helper () : string = "s"
"#,
        "x2",
    );
}

#[test]
fn call_to_an_and_binding_records_its_return_type() {
    assert_inferred(
        r#"
module E
let rec a () : int = 1
and b () : string = "s"
let fromAnd = b ()
"#,
        "fromAnd",
        "string",
        "string",
    );
}

#[test]
fn and_binding_return_type_is_not_taken_for_the_first_binding() {
    assert_no_fact(
        r#"
module E
let rec a () = 1
and b () : string = "s"
let fromFirst = a ()
"#,
        "fromFirst",
    );
}

#[test]
fn call_inside_a_rec_module_records_no_fact() {
    assert_no_fact(
        r#"
module C =
    let load () : int = 1
    module rec Inner =
        let f () =
            let x3 = load ()
            x3
        let load () : string = "s"
"#,
        "x3",
    );
}

#[test]
fn call_inside_a_rec_namespace_records_no_fact() {
    assert_no_fact(
        r#"
namespace rec N
module M =
    let load () : int = 1
    let x4 = load ()
"#,
        "x4",
    );
}

#[test]
fn static_call_on_a_type_with_an_extension_overload_records_no_fact() {
    assert_no_fact(
        r#"
module D =
    type Store() =
        member this.Load(x: int) : int = x
        static member Create(x: int) : int = x
    type Store with
        member this.Load() : string = "s"
        static member Create() : string = "s"
    let x5 = Store.Create()
"#,
        "x5",
    );
}

#[test]
fn self_call_on_a_type_with_an_extension_overload_records_no_fact() {
    assert_no_fact(
        r#"
module D =
    type Store() =
        member this.Load(x: int) : int = x
        member this.Run() =
            let x6 = this.Load()
            x6
    type Store with
        member this.Load() : string = "s"
"#,
        "x6",
    );
}
