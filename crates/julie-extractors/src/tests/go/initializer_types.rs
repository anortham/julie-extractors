use crate::base::SymbolKind;
use crate::go::GoExtractor;
use crate::tests::helpers::init_parser;
use std::path::PathBuf;

fn inferred_type(source: &str, local: &str) -> Option<(String, String)> {
    let tree = init_parser(source, "go");
    let mut extractor = GoExtractor::new(
        "go".to_string(),
        "initializer_types.go".to_string(),
        source.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    let local = symbols
        .iter()
        .find(|s| s.name == local && s.kind == SymbolKind::Variable)
        .unwrap_or_else(|| panic!("missing local {local}"));
    extractor.base.type_info.get(&local.id).map(|fact| {
        assert!(fact.is_inferred, "fact for {} is not inferred", local.name);
        let declared = fact
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("declared"))
            .and_then(|declared| declared.as_str())
            .unwrap_or_default()
            .to_string();
        (fact.resolved_type.clone(), declared)
    })
}

fn typed(resolved: &str, declared: &str) -> Option<(String, String)> {
    Some((resolved.to_string(), declared.to_string()))
}

const SERVER: &str = r#"
package main

type Config struct{}
type Store struct{}
type Server struct{}

func (s *Server) config() *Config { return nil }
func (s Server) name() string { return "" }
func (s Server) store() *Store { return nil }
func (s *Server) Load() (*Store, error) { return nil, nil }
func (c *Config) clone() *Config { return c }
"#;

fn in_server_method(body: &str) -> String {
    format!("{SERVER}\nfunc (s *Server) Run() {{\n    {body}\n}}\n")
}

#[test]
fn receiver_method_call_records_the_declared_result_type() {
    assert_eq!(
        inferred_type(&in_server_method("cfg := s.config()"), "cfg"),
        typed("Config", "*Config")
    );
}

#[test]
fn value_receiver_method_call_records_the_declared_result_type() {
    let source = format!("{SERVER}\nfunc (srv Server) Run() {{\n    cfg := srv.config()\n}}\n");
    assert_eq!(inferred_type(&source, "cfg"), typed("Config", "*Config"));
}

#[test]
fn var_spec_from_receiver_method_call_records_the_result_type() {
    assert_eq!(
        inferred_type(&in_server_method("var cfg = s.config()"), "cfg"),
        typed("Config", "*Config")
    );
}

#[test]
fn multi_value_receiver_call_records_each_result_position() {
    let source = in_server_method("store, err := s.Load()");
    assert_eq!(inferred_type(&source, "store"), typed("Store", "*Store"));
    assert_eq!(inferred_type(&source, "err"), None);
}

#[test]
fn multi_value_var_spec_from_receiver_call_records_each_result_position() {
    let source = in_server_method("var store, err = s.Load()");
    assert_eq!(inferred_type(&source, "store"), typed("Store", "*Store"));
    assert_eq!(inferred_type(&source, "err"), None);
}

#[test]
fn parenthesized_receiver_call_records_the_result_type() {
    assert_eq!(
        inferred_type(&in_server_method("cfg := (s.config())"), "cfg"),
        typed("Config", "*Config")
    );
}

#[test]
fn receiver_call_inside_a_closure_records_the_result_type() {
    assert_eq!(
        inferred_type(
            &in_server_method("go func() {\n        cfg := s.config()\n        _ = cfg\n    }()"),
            "cfg"
        ),
        typed("Config", "*Config")
    );
}

#[test]
fn composite_literal_method_call_records_the_result_type() {
    let source = format!(
        "{SERVER}\nfunc Use() {{\n    a := Server{{}}.store()\n    b := (&Server{{}}).config()\n}}\n"
    );
    assert_eq!(inferred_type(&source, "a"), typed("Store", "*Store"));
    assert_eq!(inferred_type(&source, "b"), typed("Config", "*Config"));
}

#[test]
fn receiver_call_uses_the_method_of_the_receiver_type() {
    let source = r#"
package main

type Config struct{}
type Other struct{}
type Server struct{}

func (o *Other) config() *Other { return o }
func (s *Server) config() *Config { return nil }

func (s *Server) Run() {
    cfg := s.config()
}
"#;
    assert_eq!(inferred_type(source, "cfg"), typed("Config", "*Config"));
}

#[test]
fn generic_receiver_method_with_a_concrete_result_records_it() {
    let source = r#"
package main

type Stack[T any] struct{}

func (s *Stack[T]) Clone() *Stack[T] { return s }
func (s *Stack[T]) Ints() *Stack[int] { return nil }

func (s *Stack[T]) Run() {
    ints := s.Ints()
}
"#;
    assert_eq!(inferred_type(source, "ints"), typed("Stack", "*Stack[int]"));
}

#[test]
fn result_with_a_type_parameter_argument_records_only_the_base_name() {
    let source = r#"
package main

type Box[T any] struct{}
type Stack[T any] struct{}

func Wrap[T any](v T) Box[T] { return Box[T]{} }
func (s *Stack[T]) Clone() *Stack[T] { return s }
func (s *Stack[T]) Nested() Box[[]T] { return Box[[]T]{} }

func (s *Stack[U]) Run() {
    dup := s.Clone()
    nested := s.Nested()
}

func Use() {
    wrapped := Wrap(1)
    cloned := (&Stack[int]{}).Clone()
}
"#;
    assert_eq!(inferred_type(source, "dup"), typed("Stack", ""));
    assert_eq!(inferred_type(source, "nested"), typed("Box", ""));
    assert_eq!(inferred_type(source, "wrapped"), typed("Box", ""));
    assert_eq!(inferred_type(source, "cloned"), typed("Stack", ""));
}

#[test]
fn method_declared_only_on_another_type_records_nothing() {
    let source = r#"
package main

type Other struct{}
type Server struct{}

func (o *Other) config() *Other { return o }

func (s *Server) Run() {
    cfg := s.config()
}
"#;
    assert_eq!(inferred_type(source, "cfg"), None);
}

#[test]
fn free_function_call_does_not_match_a_method_name() {
    assert_eq!(
        inferred_type(&in_server_method("cfg := config()"), "cfg"),
        None
    );
}

#[test]
fn call_on_a_variable_that_is_not_the_receiver_records_nothing() {
    assert_eq!(
        inferred_type(
            &in_server_method("other := &Server{}\n    cfg := other.config()"),
            "cfg"
        ),
        None
    );
}

#[test]
fn call_on_a_rebound_receiver_name_records_nothing() {
    assert_eq!(
        inferred_type(
            &in_server_method(
                "if true {\n        s := &Other{}\n        _ = s\n    }\n    cfg := s.config()"
            ),
            "cfg"
        ),
        None
    );
}

#[test]
fn call_on_a_closure_parameter_that_shadows_the_receiver_records_nothing() {
    assert_eq!(
        inferred_type(
            &in_server_method(
                "f := func(s *Other) {\n        cfg := s.config()\n        _ = cfg\n    }\n    _ = f"
            ),
            "cfg"
        ),
        None
    );
}

#[test]
fn call_on_a_receiver_name_outside_its_method_records_nothing() {
    let source = format!("{SERVER}\nfunc Use(s *Server) {{\n    cfg := s.config()\n}}\n");
    assert_eq!(inferred_type(&source, "cfg"), None);
}

#[test]
fn predeclared_receiver_method_result_records_nothing() {
    assert_eq!(
        inferred_type(&in_server_method("label := s.name()"), "label"),
        None
    );
}

#[test]
fn method_chained_on_a_call_result_records_nothing() {
    assert_eq!(
        inferred_type(&in_server_method("cfg := s.config().clone()"), "cfg"),
        None
    );
}

#[test]
fn qualified_composite_literal_method_call_records_nothing() {
    let source = format!("{SERVER}\nfunc Use() {{\n    cfg := remote.Server{{}}.config()\n}}\n");
    assert_eq!(inferred_type(&source, "cfg"), None);
}

#[test]
fn generic_function_result_records_nothing() {
    let source = r#"
package main

type Store struct{}

func First[T any](items []T) T { return items[0] }
func Pointer[T any](item T) *T { return &item }

func Use(stores []Store) {
    first := First(stores)
    pointer := Pointer(stores[0])
}
"#;
    assert_eq!(inferred_type(source, "first"), None);
    assert_eq!(inferred_type(source, "pointer"), None);
}

#[test]
fn generic_receiver_type_parameter_result_records_nothing() {
    let source = r#"
package main

type Stack[T any] struct{}

func (s *Stack[T]) Pop() T { var zero T; return zero }

func (s *Stack[T]) Run() {
    top := s.Pop()
}
"#;
    assert_eq!(inferred_type(source, "top"), None);
}

#[test]
fn conflicting_same_named_declarations_record_nothing() {
    let source = r#"
package main

type Config struct{}
type Other struct{}
type Server struct{}

func open() *Config { return nil }
func open() *Other { return nil }
func (s *Server) config() *Config { return nil }
func (s Server) config() *Other { return nil }

func (s *Server) Run() {
    opened := open()
    cfg := s.config()
}
"#;
    assert_eq!(inferred_type(source, "opened"), None);
    assert_eq!(inferred_type(source, "cfg"), None);
}

#[test]
fn agreeing_same_named_declarations_record_the_shared_type() {
    let source = r#"
package main

type Config struct{}

func open() *Config { return nil }
func open() *Config { return nil }

func Use() {
    opened := open()
}
"#;
    assert_eq!(inferred_type(source, "opened"), typed("Config", "*Config"));
}

const SHADOWING: &str = r#"
package main

type Config struct{}
type Other struct{}
type Base struct{}
type Server struct{ Base }

func load() *Config { return nil }
func (s *Server) config() *Config { return nil }
func (b Base) config() *Other { return nil }
func (o Other) config() *Other { return &o }
"#;

#[test]
fn composite_literal_method_call_on_a_local_type_records_nothing() {
    let source = format!(
        "{SHADOWING}\nfunc LocalType() {{\n    type Server struct{{ Base }}\n    local := Server{{}}.config()\n}}\n\nfunc PackageType() {{\n    pkg := Server{{}}.config()\n}}\n"
    );
    assert_eq!(inferred_type(&source, "local"), None);
    assert_eq!(inferred_type(&source, "pkg"), typed("Config", "*Config"));
}

#[test]
fn receiver_name_rebound_by_a_local_type_records_nothing() {
    let source = format!(
        "{SHADOWING}\nfunc (s *Server) Alias() {{\n    {{\n        type s = Other\n        aliased := s.config(Other{{}})\n    }}\n}}\n\nfunc (s *Server) Defined() {{\n    {{\n        type s struct{{ Other }}\n        defined := s.config(s{{}})\n    }}\n}}\n\nfunc (s *Server) Plain() {{\n    plain := s.config()\n}}\n"
    );
    assert_eq!(inferred_type(&source, "aliased"), None);
    assert_eq!(inferred_type(&source, "defined"), None);
    assert_eq!(inferred_type(&source, "plain"), typed("Config", "*Config"));
}

#[test]
fn free_function_hidden_by_a_local_func_variable_records_nothing() {
    let source = format!(
        "{SHADOWING}\nfunc (s *Server) Hidden() {{\n    load := func() *Other {{ return nil }}\n    hidden := load()\n}}\n\nfunc Plain() {{\n    plain := load()\n}}\n"
    );
    assert_eq!(inferred_type(&source, "hidden"), None);
    assert_eq!(inferred_type(&source, "plain"), typed("Config", "*Config"));
}

#[test]
fn free_function_hidden_by_a_parameter_records_nothing() {
    let source = format!(
        "{SHADOWING}\nfunc Hidden(load func() *Other) {{\n    hidden := load()\n}}\n\nfunc Closure() {{\n    run := func(load func() *Other) {{\n        inner := load()\n        _ = inner\n    }}\n    _ = run\n}}\n"
    );
    assert_eq!(inferred_type(&source, "hidden"), None);
    assert_eq!(inferred_type(&source, "inner"), None);
}

#[test]
fn shadowed_new_builtin_records_nothing() {
    let source = format!(
        "{SHADOWING}\nfunc Hidden(new func(any) *Other) {{\n    hidden := new(Config)\n}}\n\nfunc Plain() {{\n    plain := new(Config)\n}}\n"
    );
    assert_eq!(inferred_type(&source, "hidden"), None);
    assert_eq!(inferred_type(&source, "plain"), typed("Config", ""));
}

const TYPE_PARAMETER_SHADOWING: &str = r#"
package p

type Config struct{}
type Other struct{}
type Server struct{}
type Num[T ~int] struct{}

func load() *Config { return nil }
func (s *Server) store() *Config { return nil }

func Conv[load ~int](v int) { tpconv := load(v); var _ load = tpconv }
func PlainConv() { plainconv := load() }
func Lit[Server interface{ ~struct{}; store() *Other }]() { tplit := Server{}.store(); var _ *Other = tplit }
func PlainLit() { plainlit := Server{}.store() }
func (n *Num[load]) Push(v int) { rtpconv := load(v); var _ load = rtpconv }
func (n *Num[T]) Plain() { rplain := load() }
"#;

#[test]
fn function_type_parameter_hiding_a_function_records_nothing() {
    assert_eq!(inferred_type(TYPE_PARAMETER_SHADOWING, "tpconv"), None);
    assert_eq!(
        inferred_type(TYPE_PARAMETER_SHADOWING, "plainconv"),
        typed("Config", "*Config")
    );
}

#[test]
fn function_type_parameter_hiding_a_literal_type_records_nothing() {
    assert_eq!(inferred_type(TYPE_PARAMETER_SHADOWING, "tplit"), None);
    assert_eq!(
        inferred_type(TYPE_PARAMETER_SHADOWING, "plainlit"),
        typed("Config", "*Config")
    );
}

#[test]
fn receiver_type_parameter_hiding_a_function_records_nothing() {
    assert_eq!(inferred_type(TYPE_PARAMETER_SHADOWING, "rtpconv"), None);
    assert_eq!(
        inferred_type(TYPE_PARAMETER_SHADOWING, "rplain"),
        typed("Config", "*Config")
    );
}

#[test]
fn method_promoted_through_an_embedded_field_records_nothing() {
    let source = r#"
package p

type Other struct{}
type Base struct{}
type Wrap struct{ Base }

func (b Base) cfg() *Other { return nil }
func (w Wrap) Run() { promoted := w.cfg() }
func (b Base) Run() { direct := b.cfg() }
"#;
    assert_eq!(inferred_type(source, "promoted"), None);
    assert_eq!(inferred_type(source, "direct"), typed("Other", "*Other"));
}

const INSTANTIATED: &str = r#"
package main

type Set[T comparable] struct{}
type Config struct{}
type Other struct{}

func NewSet[T comparable](xs ...T) *Set[T] { return nil }
func Cfg[T any]() Config { return Config{} }
func PConf[T any]() *Config { return nil }
func load() *Config { return nil }
"#;

#[test]
fn no_argument_call_with_one_type_argument_records_the_result_type() {
    let source = format!(
        "{INSTANTIATED}\nfunc Use() {{\n    set := NewSet[string]()\n    var spec = NewSet[string]()\n    cfg := Cfg[int]()\n    pconf := PConf[int]()\n}}\n"
    );
    assert_eq!(inferred_type(&source, "set"), typed("Set", ""));
    assert_eq!(inferred_type(&source, "spec"), typed("Set", ""));
    assert_eq!(inferred_type(&source, "cfg"), typed("Config", ""));
    assert_eq!(inferred_type(&source, "pconf"), typed("Config", "*Config"));
}

#[test]
fn indexed_call_on_a_local_that_hides_a_function_records_nothing() {
    let source = format!(
        "{INSTANTIATED}\nfunc Use() {{\n    load := []func() *Other{{nil}}\n    hidden := load[0]()\n}}\n\nfunc Param(Cfg map[int]func() *Other) {{\n    param := Cfg[1]()\n}}\n"
    );
    assert_eq!(inferred_type(&source, "hidden"), None);
    assert_eq!(inferred_type(&source, "param"), None);
}

#[test]
fn indexed_call_on_a_non_function_records_nothing() {
    let source = format!(
        "{INSTANTIATED}\nvar loaders = []func() *Other{{nil}}\n\nfunc Use() {{\n    loaded := loaders[0]()\n}}\n"
    );
    assert_eq!(inferred_type(&source, "loaded"), None);
}
