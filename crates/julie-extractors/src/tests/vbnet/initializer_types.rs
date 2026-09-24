use crate::base::SymbolKind;
use crate::vbnet::VbNetExtractor;
use std::path::PathBuf;

fn inferred_type(source: &str, local: &str) -> Option<(String, bool)> {
    let mut parser = super::init_test_parser();
    let tree = parser.parse(source, None).unwrap();
    let mut extractor = VbNetExtractor::new(
        "vbnet".to_string(),
        "initializer_types.vb".to_string(),
        source.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    let local = symbols
        .iter()
        .find(|s| s.name == local && s.kind == SymbolKind::Variable)
        .unwrap_or_else(|| panic!("missing local {local}"));
    extractor
        .base
        .type_info
        .get(&local.id)
        .map(|fact| (fact.resolved_type.clone(), fact.is_inferred))
}

const LOADERS: &str = r#"
    Public Function Load() As Workspace
    End Function
    Public Async Function LoadAsync() As Task(Of Workspace)
    End Function
    Public Function LoadValueAsync() As System.Threading.Tasks.ValueTask(Of Workspace)
    End Function
    Public Shared Function Create() As Workspace
    End Function
    Public Function LoadAll() As Workspace()
    End Function
    Public Function LoadList() As List(Of Workspace)
    End Function
    Public Function Count() As Integer?
    End Function
    Public Sub Reset()
    End Sub
    Public Function Untyped()
    End Function
    Public Function Pick(Of T)() As T
    End Function
    Public Function PickAsync(Of T)() As Task(Of T)
    End Function
    Public Function Start() As Task
    End Function
"#;

fn in_loader(statement: &str) -> String {
    format!(
        "Class Loader\n{LOADERS}\n    Async Sub Run(input As String)\n        {statement}\n    End Sub\nEnd Class\n"
    )
}

fn workspace_type(statement: &str) -> Option<(String, bool)> {
    inferred_type(&in_loader(statement), "workspace")
}

fn workspace() -> Option<(String, bool)> {
    Some(("Workspace".to_string(), true))
}

#[test]
fn unqualified_same_class_function_records_return_type_as_inferred() {
    assert_eq!(workspace_type("Dim workspace = Load()"), workspace());
    assert_eq!(workspace_type("Static workspace = Load()"), workspace());
}

#[test]
fn call_names_match_case_insensitively_and_through_brackets() {
    assert_eq!(workspace_type("Dim workspace = load()"), workspace());
    assert_eq!(workspace_type("Dim workspace = [Load]()"), workspace());
}

#[test]
fn me_and_myclass_calls_record_the_class_method_type() {
    assert_eq!(workspace_type("Dim workspace = Me.Load()"), workspace());
    assert_eq!(
        workspace_type("Dim workspace = MyClass.Load()"),
        workspace()
    );
}

#[test]
fn shared_call_on_same_file_type_records_return_type() {
    assert_eq!(
        workspace_type("Dim workspace = Loader.Create()"),
        workspace()
    );
}

#[test]
fn await_removes_one_task_layer() {
    for statement in [
        "Dim workspace = Await LoadAsync()",
        "Dim workspace = Await Me.LoadAsync()",
        "Dim workspace = Await LoadValueAsync()",
        "Dim workspace = Await LoadAsync().ConfigureAwait(False)",
    ] {
        assert_eq!(workspace_type(statement), workspace(), "{statement}");
    }
}

#[test]
fn unawaited_task_call_records_the_task_type() {
    assert_eq!(
        workspace_type("Dim workspace = LoadAsync()"),
        Some(("Task".to_string(), true))
    );
}

#[test]
fn array_generic_and_nullable_returns_record_their_base_names() {
    assert_eq!(
        workspace_type("Dim workspace = LoadAll()"),
        Some(("Workspace()".to_string(), true))
    );
    assert_eq!(
        workspace_type("Dim workspace = LoadList()"),
        Some(("List".to_string(), true))
    );
    assert_eq!(
        workspace_type("Dim workspace = Count()"),
        Some(("Integer".to_string(), true))
    );
}

#[test]
fn every_binding_of_a_dim_statement_is_inferred() {
    let source = in_loader("Dim first = Load(), workspace = Create()");
    assert_eq!(inferred_type(&source, "first"), workspace());
    assert_eq!(inferred_type(&source, "workspace"), workspace());
}

#[test]
fn using_assignment_records_call_return_type() {
    assert_eq!(
        workspace_type("Using workspace = Load()\n        End Using"),
        workspace()
    );
}

#[test]
fn written_type_wins_over_call_inference() {
    assert_eq!(
        workspace_type("Dim workspace As Object = Load()"),
        Some(("Object".to_string(), false))
    );
}

#[test]
fn same_file_constructor_inference_still_applies() {
    let source = "Class Workspace\nEnd Class\nClass Sample\n    Sub Run()\n        Dim workspace = New Workspace()\n    End Sub\nEnd Class\n";
    assert_eq!(inferred_type(source, "workspace"), workspace());
}

#[test]
fn module_function_is_found_from_a_class_and_by_module_name() {
    let source = r#"
Module Loaders
    Function Make() As Workspace
    End Function
    Declare Function OpenNative Lib "native" () As Workspace
End Module
Class Sample
    Sub Run()
        Dim workspace = Make()
        Dim qualified = Loaders.Make()
        Dim native = OpenNative()
    End Sub
End Class
"#;
    assert_eq!(inferred_type(source, "workspace"), workspace());
    assert_eq!(inferred_type(source, "qualified"), workspace());
    assert_eq!(inferred_type(source, "native"), workspace());
}

#[test]
fn nested_class_finds_outer_shared_function() {
    let source = r#"
Class Outer
    Shared Function Make() As Workspace
    End Function
    Class Inner
        Sub Run()
            Dim workspace = Make()
        End Sub
    End Class
End Class
"#;
    assert_eq!(inferred_type(source, "workspace"), workspace());
}

#[test]
fn structure_members_are_indexed() {
    let source = r#"
Structure Point
    Function Load() As Workspace
    End Function
    Sub Run()
        Dim workspace = Me.Load()
    End Sub
End Structure
"#;
    assert_eq!(inferred_type(source, "workspace"), workspace());
}

#[test]
fn generic_returns_record_no_fact() {
    for statement in [
        "Dim workspace = Pick(Of Workspace)()",
        "Dim workspace = Await PickAsync(Of Workspace)()",
    ] {
        assert_eq!(workspace_type(statement), None, "{statement}");
    }
}

#[test]
fn class_type_parameter_return_records_no_fact() {
    let source = r#"
Class Box(Of T)
    Function Value() As T
    End Function
    Class Inner
        Function Outer() As T
        End Function
        Sub Run()
            Dim nested = Outer()
        End Sub
    End Class
    Sub Run()
        Dim workspace = Value()
    End Sub
End Class
"#;
    assert_eq!(inferred_type(source, "workspace"), None);
    assert_eq!(inferred_type(source, "nested"), None);
}

#[test]
fn members_without_a_return_type_record_no_fact() {
    for statement in ["Dim workspace = Reset()", "Dim workspace = Untyped()"] {
        assert_eq!(workspace_type(statement), None, "{statement}");
    }
}

#[test]
fn disagreeing_overloads_record_no_fact() {
    let source = r#"
Class Loader
    Function Load() As Workspace
    End Function
    Function Load(path As String) As Project
    End Function
    Sub Run()
        Dim workspace = Load()
    End Sub
End Class
"#;
    assert_eq!(inferred_type(source, "workspace"), None);
}

#[test]
fn agreeing_overloads_record_the_shared_type() {
    let source = r#"
Class Loader
    Function Load() As Workspace
    End Function
    Function Load(path As String) As Workspace
    End Function
    Sub Run()
        Dim workspace = Load()
    End Sub
End Class
"#;
    assert_eq!(inferred_type(source, "workspace"), workspace());
}

#[test]
fn chain_ending_in_another_member_records_no_fact() {
    for statement in [
        "Dim workspace = Load().Clone()",
        "Dim workspace = Load().Name",
        "Dim workspace = Load",
        "Dim workspace = Not Load()",
        "Dim workspace = Await Load().ConfigureAwait(False)",
        "Dim workspace = Load().ConfigureAwait(False)",
    ] {
        assert_eq!(workspace_type(statement), None, "{statement}");
    }
}

#[test]
fn await_without_a_generic_task_records_no_fact() {
    for statement in [
        "Dim workspace = Await Start()",
        "Dim workspace = Await Load()",
        "Dim workspace = Await LoadList()",
        "Dim workspace = Not LoadAsync()",
    ] {
        assert_eq!(workspace_type(statement), None, "{statement}");
    }
}

#[test]
fn mybase_call_records_no_fact() {
    assert_eq!(workspace_type("Dim workspace = MyBase.Load()"), None);
}

#[test]
fn me_call_to_a_member_the_class_does_not_declare_records_no_fact() {
    let source = r#"
Module Loaders
    Function Make() As Workspace
    End Function
End Module
Interface ILoader
    Function Open() As Workspace
End Interface
Class Sample
    Implements ILoader
    Sub Run()
        Dim workspace = Me.Make()
        Dim opened = Me.Open()
        Dim viaInterface = ILoader.Open()
    End Sub
End Class
"#;
    assert_eq!(inferred_type(source, "workspace"), None);
    assert_eq!(inferred_type(source, "opened"), None);
    assert_eq!(inferred_type(source, "viaInterface"), None);
}

#[test]
fn call_on_a_type_from_another_file_records_no_fact() {
    assert_eq!(workspace_type("Dim workspace = Other.Create()"), None);
    assert_eq!(workspace_type("Dim workspace = Missing()"), None);
}

#[test]
fn inheriting_class_does_not_fall_back_to_module_functions() {
    let source = r#"
Module Loaders
    Function Make() As Workspace
    End Function
End Module
Class Sample
    Inherits BaseSample
    Sub Run()
        Dim workspace = Make()
    End Sub
End Class
"#;
    assert_eq!(inferred_type(source, "workspace"), None);
}

#[test]
fn local_parameter_or_enclosing_member_name_shadows_the_function() {
    let source = r#"
Class Loader
    Function Load() As Workspace
        Dim recursive = Load()
    End Function
    Function Items() As Workspace
    End Function
    Function Create() As Workspace
    End Function
    Sub Run(load As Workspace())
        Dim items = GetItems()
        Dim workspace = Load(0)
        Dim first = Items(0)
        Dim create = Create()
    End Sub
End Class
"#;
    for local in ["recursive", "workspace", "first", "create"] {
        assert_eq!(inferred_type(source, local), None, "{local}");
    }
}

#[test]
fn inner_member_with_the_call_name_shadows_the_outer_function() {
    let source = r#"
Class Outer
    Shared Function Load() As Workspace
    End Function
    Class Inner
        Property Load As Workspace()
        Sub Run()
            Dim workspace = Load(0)
        End Sub
    End Class
End Class
"#;
    assert_eq!(inferred_type(source, "workspace"), None);
}

#[test]
fn qualifier_that_names_a_member_or_local_records_no_fact() {
    let source = r#"
Class Config
    Shared Function Create() As Workspace
    End Function
End Class
Class Sample
    Property Config As Settings
    Sub Run(settings As Settings)
        Dim workspace = Config.Create()
    End Sub
    Sub Other()
        Dim loader = GetLoader()
        Dim local = LOADER.Create()
    End Sub
End Class
Class Loader
    Shared Function Create() As Workspace
    End Function
End Class
"#;
    assert_eq!(inferred_type(source, "workspace"), None);
    assert_eq!(inferred_type(source, "local"), None);
}

#[test]
fn arguments_to_a_parameterless_function_index_the_result_and_record_no_fact() {
    let source = r#"
Class Indexer
    Function Load() As Workspace()
    End Function
    Function Items() As List(Of Workspace)
    End Function
    Function Name() As String
    End Function
    Sub Run()
        Dim idx = Load(0)
        Dim item = Items(0)
        Dim ch = Name(0)
        Dim meIdx = Me.Load(0)
        Dim sharedIdx = Indexer.Load(0)
    End Sub
End Class
"#;
    for local in ["idx", "item", "ch", "meIdx", "sharedIdx"] {
        assert_eq!(inferred_type(source, local), None, "{local}");
    }
}

#[test]
fn optional_and_param_array_parameters_accept_their_argument_counts() {
    let source = r#"
Class Loader
    Function Load(path As String, Optional retries As Integer = 1) As Workspace
    End Function
    Function Merge(ParamArray parts() As Workspace) As Workspace
    End Function
    Sub Run()
        Dim one = Load("a")
        Dim two = Load("a", 2)
        Dim skipped = Load("a", )
        Dim three = Load("a", 2, 3)
        Dim none = Load()
        Dim merged = Merge(a, b, c, d)
        Dim empty = Merge()
    End Sub
End Class
"#;
    for local in ["one", "two", "skipped", "merged", "empty"] {
        assert_eq!(inferred_type(source, local), workspace(), "{local}");
    }
    for local in ["three", "none"] {
        assert_eq!(inferred_type(source, local), None, "{local}");
    }
}

#[test]
fn unawaited_configure_await_records_no_fact() {
    for statement in [
        "Dim workspace = LoadAsync().ConfigureAwait(False)",
        "Dim workspace = Me.LoadAsync().ConfigureAwait(False)",
        "Dim workspace = Await LoadAsync().ConfigureAwait(False).ConfigureAwait(False)",
    ] {
        assert_eq!(workspace_type(statement), None, "{statement}");
    }
}

#[test]
fn lambda_parameter_shadows_the_called_name() {
    let source = r#"
Module Loaders
    Function Load() As Workspace
    End Function
    Function Create() As Workspace
    End Function
End Module
Class Sample
    Sub Run()
        Dim f = Function(Load As Func(Of Other))
                    Dim viaLambdaParam = Load()
                    Return viaLambdaParam
                End Function
        Dim g = Sub(Loaders)
                    Dim viaLambdaQualifier = Loaders.Create()
                End Sub
        Dim outside = Load()
    End Sub
End Class
"#;
    assert_eq!(inferred_type(source, "viaLambdaParam"), None);
    assert_eq!(inferred_type(source, "viaLambdaQualifier"), None);
    assert_eq!(inferred_type(source, "outside"), workspace());
}

#[test]
fn overloads_in_an_inheriting_class_keep_base_overloads_and_record_no_fact() {
    let source = r#"
Class Derived
    Inherits BaseLoader
    Overloads Function Load(Optional x As Integer = 0) As Workspace
    End Function
    Overrides Function Open() As Workspace
    End Function
    Shared Overloads Function Create(Optional x As Integer = 0) As Workspace
    End Function
    Function Plain() As Workspace
    End Function
    Sub Run()
        Dim viaMe = Me.Load()
        Dim viaBare = Load()
        Dim viaOverride = Open()
        Dim viaShared = Derived.Create()
        Dim viaPlain = Plain()
    End Sub
End Class
"#;
    for local in ["viaMe", "viaBare", "viaOverride", "viaShared"] {
        assert_eq!(inferred_type(source, local), None, "{local}");
    }
    assert_eq!(inferred_type(source, "viaPlain"), workspace());
}

#[test]
fn types_and_modules_from_a_sibling_namespace_are_out_of_scope() {
    let source = r#"
Namespace A
    Module M
        Function Load() As Workspace
        End Function
    End Module
    Class Factory
        Shared Function Create() As Workspace
        End Function
    End Class
    Namespace Inner
        Class Near
            Sub Run()
                Dim fromInner = Load()
                Dim fromInnerType = Factory.Create()
            End Sub
        End Class
    End Namespace
End Namespace
Namespace B
    Class C
        Sub Run()
            Dim viaNs = Load()
            Dim viaNsType = Factory.Create()
        End Sub
    End Class
End Namespace
"#;
    assert_eq!(inferred_type(source, "viaNs"), None);
    assert_eq!(inferred_type(source, "viaNsType"), None);
    assert_eq!(inferred_type(source, "fromInner"), workspace());
    assert_eq!(inferred_type(source, "fromInnerType"), workspace());
}

#[test]
fn imports_bring_a_namespace_into_scope() {
    let source = r#"
Imports A
Imports Alias = B
Namespace A
    Module M
        Function Load() As Workspace
        End Function
    End Module
End Namespace
Namespace B
    Class Factory
        Shared Function Create() As Workspace
        End Function
    End Class
End Namespace
Namespace Global.C
    Class Caller
        Sub Run()
            Dim imported = Load()
            Dim aliased = Factory.Create()
        End Sub
    End Class
End Namespace
"#;
    assert_eq!(inferred_type(source, "imported"), workspace());
    assert_eq!(inferred_type(source, "aliased"), None);
}

#[test]
fn nested_type_qualifier_is_in_scope_only_inside_its_declaring_type() {
    let source = r#"
Class Outer
    Class Factory
        Shared Function Create() As Workspace
        End Function
    End Class
    Sub Run()
        Dim inside = Factory.Create()
    End Sub
End Class
Class Other
    Sub Run()
        Dim outside = Factory.Create()
    End Sub
End Class
"#;
    assert_eq!(inferred_type(source, "inside"), workspace());
    assert_eq!(inferred_type(source, "outside"), None);
}

#[test]
fn partial_class_does_not_fall_back_to_module_functions() {
    let source = r#"
Module Helpers
    Function Load() As Workspace
    End Function
End Module
Partial Class Form1
    Sub Run()
        Dim viaPartial = Load()
    End Sub
End Class
Class Split
    Sub Run()
        Dim viaUnmarkedPart = Load()
    End Sub
End Class
Partial Class Split
End Class
Class Whole
    Sub Run()
        Dim viaWhole = Load()
    End Sub
End Class
"#;
    assert_eq!(inferred_type(source, "viaPartial"), None);
    assert_eq!(inferred_type(source, "viaUnmarkedPart"), None);
    assert_eq!(inferred_type(source, "viaWhole"), workspace());
}

#[test]
fn object_member_name_binds_to_me_before_a_module_function() {
    let source = r#"
Module Helpers
    Function ToString() As Workspace
    End Function
    Function GetHashCode() As Workspace
    End Function
End Module
Class C2
    Sub Run()
        Dim s = ToString()
        Dim h = gethashcode()
    End Sub
End Class
"#;
    assert_eq!(inferred_type(source, "s"), None);
    assert_eq!(inferred_type(source, "h"), None);
}
