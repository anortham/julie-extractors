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

func (s *Stack[T]) Run() {
    dup := s.Clone()
}
"#;
    assert_eq!(inferred_type(source, "dup"), typed("Stack", "*Stack[T]"));
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
