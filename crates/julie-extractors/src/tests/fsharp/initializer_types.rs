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
