use crate::ExtractionResults;
use crate::base::{IdentifierKind, Symbol, SymbolKind, Visibility};
use crate::extract_canonical;
use std::path::Path;

fn extract_at(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test"))
        .expect("canonical Go extraction must succeed")
}

fn extract(source: &str) -> ExtractionResults {
    extract_at("app/store.go", source)
}

fn label(result: &ExtractionResults, id: Option<&str>) -> String {
    id.and_then(|id| result.symbols.iter().find(|symbol| symbol.id == id))
        .map(|symbol| format!("{}:{}@{}", symbol.name, symbol.kind, symbol.start_line))
        .unwrap_or_default()
}

fn find<'a>(result: &'a ExtractionResults, name: &str, line: u32) -> &'a Symbol {
    result
        .symbols
        .iter()
        .find(|symbol| symbol.name == name && symbol.start_line == line)
        .unwrap_or_else(|| panic!("{name}@{line} missing: {:#?}", names(result)))
}

fn names(result: &ExtractionResults) -> Vec<String> {
    result
        .symbols
        .iter()
        .map(|s| format!("{}:{}@{}", s.name, s.kind, s.start_line))
        .collect()
}

fn relationship_rows(result: &ExtractionResults) -> Vec<String> {
    result
        .relationships
        .iter()
        .map(|rel| {
            format!(
                "{} {}->{}",
                rel.kind,
                label(result, Some(&rel.from_symbol_id)),
                label(result, Some(&rel.to_symbol_id))
            )
        })
        .collect()
}

fn pending_rows(result: &ExtractionResults) -> Vec<String> {
    result
        .structured_pending_relationships
        .iter()
        .map(|pending| {
            format!(
                "{} {}->{}",
                pending.pending.kind,
                label(result, Some(&pending.pending.from_symbol_id)),
                pending.target.display_name,
            )
        })
        .collect()
}

fn identifier_rows(result: &ExtractionResults) -> Vec<String> {
    result
        .identifiers
        .iter()
        .map(|identifier| {
            format!(
                "{} {} in={}",
                identifier.kind,
                identifier.name,
                label(result, identifier.containing_symbol_id.as_deref())
            )
        })
        .collect()
}

fn type_of(result: &ExtractionResults, name: &str, line: u32) -> Option<(String, bool)> {
    let symbol = find(result, name, line);
    result
        .types
        .get(&symbol.id)
        .map(|info| (info.resolved_type.clone(), info.is_inferred))
}

fn role(symbol: &Symbol) -> Option<String> {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("test_role"))
        .and_then(|role| role.as_str())
        .map(str::to_string)
}

fn facts(result: &ExtractionResults, pattern: &str) -> Vec<String> {
    result
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == pattern)
        .map(|fact| {
            let meta = fact.metadata.clone().unwrap_or_default();
            let get = |key: &str| {
                meta.get(key)
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .to_string()
            };
            let path = [
                "effective_route_template",
                "route_template",
                "mount_path",
                "target_path",
            ]
            .into_iter()
            .map(get)
            .find(|value| !value.is_empty())
            .unwrap_or_default();
            format!("{} {}", get("verb"), path)
        })
        .collect()
}

fn assert_contains(rows: &[String], expected: &str) {
    assert!(
        rows.iter().any(|row| row == expected),
        "missing {expected:?} in {rows:#?}"
    );
}

fn assert_absent(rows: &[String], needle: &str) {
    assert!(
        !rows.iter().any(|row| row.contains(needle)),
        "unexpected {needle:?} in {rows:#?}"
    );
}

#[test]
fn struct_and_interface_embedding_emit_extends_edges() {
    let result = extract(
        r#"package store

import (
	"io"
	"sync"
)

type Closer interface{ Close() error }

type ReadCloser interface {
	io.Reader
	Closer
}

type Base struct{ Name string }

type Circle struct {
	Base
	*sync.Mutex
	Radius float64
}
"#,
    );
    let relationships = relationship_rows(&result);
    let pending = pending_rows(&result);

    assert_contains(&relationships, "extends Circle:struct@17->Base:struct@15");
    assert_contains(
        &relationships,
        "extends ReadCloser:interface@10->Closer:interface@8",
    );
    assert_contains(&pending, "extends Circle:struct@17->sync.Mutex");
    assert_contains(&pending, "extends ReadCloser:interface@10->io.Reader");
}

#[test]
fn blank_interface_assertions_emit_implements_without_blank_symbols() {
    let result = extract(
        r#"package store

type Shape interface{ Area() float64 }
type Circle struct{ Radius float64 }

func (c Circle) Area() float64 { return c.Radius }

var _ Shape = Circle{}
var _ Shape = (*Circle)(nil)
var _ io.Reader = (*Circle)(nil)
"#,
    );

    assert!(!result.symbols.iter().any(|symbol| symbol.name == "_"));
    assert_contains(
        &relationship_rows(&result),
        "implements Circle:struct@4->Shape:interface@3",
    );
    assert_contains(
        &pending_rows(&result),
        "implements Circle:struct@4->io.Reader",
    );
}

#[test]
fn type_definitions_span_their_declaration() {
    let result = extract(
        r#"package store

type Handler func(ctx context.Context, req *Request) error
type Registry map[string]Handler
"#,
    );
    let identifiers = identifier_rows(&result);

    assert_contains(&identifiers, "type_usage Request in=Handler:type@3");
    assert_contains(&identifiers, "type_usage Handler in=Registry:type@4");
    assert!(find(&result, "Handler", 3).body_span.is_some());
}

#[test]
fn return_and_local_types_come_from_nodes() {
    let result = extract(
        r#"package store

type User struct{}
type Command struct{}

func NewUser() *User { return &User{} }
func LoadUser(id int) (*User, error) { return &User{}, nil }
func Factory() func() int { return nil }
func (c *Command) execute(a []string) (err error) { return nil }
func (c *Command) UsageFunc() (f func(*Command) error) { return nil }
func (c *Command) Root() *Command { return c }

func use() {
	u, err := LoadUser(1)
	var z = NewUser()
	p := new(User)
	_, _, _, _ = u, err, z, p
}
"#,
    );
    let declared = |name: &str| Some((name.to_string(), false));
    let inferred = |name: &str| Some((name.to_string(), true));

    assert_eq!(type_of(&result, "NewUser", 6), declared("User"));
    assert_eq!(type_of(&result, "LoadUser", 7), declared("User"));
    assert_eq!(type_of(&result, "Factory", 8), None);
    assert_eq!(type_of(&result, "execute", 9), declared("error"));
    assert_eq!(type_of(&result, "UsageFunc", 10), None);
    assert_eq!(type_of(&result, "Root", 11), declared("Command"));
    assert_eq!(type_of(&result, "u", 14), inferred("User"));
    assert_eq!(type_of(&result, "z", 15), inferred("User"));
    assert_eq!(type_of(&result, "p", 16), inferred("User"));
}

#[test]
fn doc_comments_follow_go_doc_rules() {
    let result = extract(
        r#"package store

// MaxRetries is the retry ceiling.
const MaxRetries = 3

// DefaultName is the fallback name.
var DefaultName = "anon"

// Section: helpers.

// Foo does x.
func Foo() {}

// TODO: remove

func Bar() {}

//go:generate stringer -type=Kind
type Kind int

//go:embed static/*
var static string

// Limit caps a batch.
//
//go:generate stringer -type=Limit
const Limit = 10
"#,
    );
    let doc = |name: &str, line: u32| find(&result, name, line).doc_comment.clone();
    let annotations = |name: &str, line: u32| {
        find(&result, name, line)
            .annotations
            .iter()
            .map(|marker| marker.annotation_key.clone())
            .collect::<Vec<_>>()
    };

    assert_eq!(
        doc("MaxRetries", 4).as_deref(),
        Some("// MaxRetries is the retry ceiling.")
    );
    assert_eq!(
        doc("DefaultName", 7).as_deref(),
        Some("// DefaultName is the fallback name.")
    );
    assert_eq!(doc("Foo", 12).as_deref(), Some("// Foo does x."));
    assert_eq!(doc("Limit", 27).as_deref(), Some("// Limit caps a batch."));
    assert_eq!(doc("Bar", 16), None);
    assert_eq!(doc("Kind", 19), None);
    assert_eq!(annotations("Kind", 19), ["generate"]);
    assert_eq!(annotations("static", 22), ["embed"]);
}

#[test]
fn switches_and_cases_count_and_receivers_are_not_parameters() {
    let result = extract(
        r#"package store

type R struct{}

func Classify(n int) string {
	switch n {
	case 0:
		return "zero"
	case 1, 2:
		return "few"
	default:
		return "many"
	}
}

func (r *R) M(a, b, c int, d string) (n int, err error) { return }
"#,
    );
    let metric = |name: &str, line: u32| {
        let id = &find(&result, name, line).id;
        result
            .complexity_metrics
            .iter()
            .find(|metric| metric.symbol_id.as_ref() == Some(id))
            .map(|metric| {
                (
                    metric.decision_count,
                    metric.max_nesting_depth,
                    metric.parameter_count,
                )
            })
            .unwrap()
    };

    let (decisions, nesting, _) = metric("Classify", 5);
    assert_eq!((decisions, nesting), (3, 2));
    assert_eq!(metric("M", 16).2, Some(4));
}

#[test]
fn type_alias_visibility_and_generic_signature() {
    let result = extract(
        r#"package store

type handlerFunc = func() error
type Set[T comparable] = map[T]struct{}
"#,
    );

    assert_eq!(
        find(&result, "handlerFunc", 3).visibility,
        Some(Visibility::Private)
    );
    assert_eq!(
        find(&result, "Set", 4).signature.as_deref(),
        Some("type Set[T comparable] = map[T]struct{}")
    );
}

#[test]
fn builtins_and_conversions_are_not_calls() {
    let result = extract(
        r#"package store

type Celsius float64

func Convert(f float64, raw []byte, ids []int) Celsius {
	n := len(ids)
	users := make([]int, 0)
	users = append(users, n)
	s := string(raw)
	_ = s
	if r := recover(); r != nil {
	}
	return Celsius(f - 32)
}
"#,
    );
    let pending = pending_rows(&result);
    let relationships = relationship_rows(&result);

    for builtin in ["->len", "->make", "->append", "->string", "->recover"] {
        assert_absent(&pending, builtin);
    }
    assert_absent(&relationships, "calls Convert:function@5->Celsius");
    assert_contains(&relationships, "uses Convert:function@5->Celsius:type@3");
}

#[test]
fn http_client_method_constants_and_client_receivers_emit_requests() {
    let result = extract(
        r#"package billing

import "net/http"

func charge(ctx context.Context) {
	http.NewRequest(http.MethodPost, "https://api.example.com/orders", nil)
	http.NewRequestWithContext(ctx, http.MethodGet, "https://api.example.com/users", nil)
	http.DefaultClient.Get("https://api.example.com/refunds")
	cl := &http.Client{}
	cl.Post("https://api.example.com/payouts", "application/json", nil)
}
"#,
    );
    let requests = facts(&result, "http.client_request.v1");

    assert_contains(&requests, "POST https://api.example.com/orders");
    assert_contains(&requests, "GET https://api.example.com/users");
    assert_contains(&requests, "GET https://api.example.com/refunds");
    assert_contains(&requests, "POST https://api.example.com/payouts");
}

#[test]
fn package_qualified_ginkgo_calls_get_roles() {
    let result = extract_at(
        "cache/cache_test.go",
        r#"package cache

import (
	"github.com/onsi/ginkgo/v2"
	"github.com/onsi/gomega"
)

var _ = ginkgo.Describe("Cache", func() {
	ginkgo.BeforeEach(func() {})
	ginkgo.It("evicts old entries", func() { gomega.Expect(1).To(gomega.Equal(1)) })
})
"#,
    );

    assert_eq!(
        role(find(&result, "Cache", 8)).as_deref(),
        Some("test_container")
    );
    assert_eq!(
        role(find(&result, "BeforeEach", 9)).as_deref(),
        Some("fixture_setup")
    );
    assert_eq!(
        role(find(&result, "evicts old entries", 10)).as_deref(),
        Some("test_case")
    );
}

#[test]
fn range_and_type_switch_bindings_are_typed_locals() {
    let result = extract(
        r#"package store

type Plugin struct{}

func (p *Plugin) Init() {}

func InitAll(plugins []*Plugin, byName map[string]Plugin) {
	for i, p := range plugins {
		_ = i
		p.Init()
	}
	for key, pl := range byName {
		_ = key
		pl.Init()
	}
	switch v := any(plugins[0]).(type) {
	case *Plugin:
		v.Init()
	}
}
"#,
    );

    assert_eq!(find(&result, "i", 8).kind, SymbolKind::Variable);
    assert_eq!(type_of(&result, "p", 8), Some(("Plugin".to_string(), true)));
    assert_eq!(find(&result, "key", 12).kind, SymbolKind::Variable);
    assert_eq!(
        type_of(&result, "pl", 12),
        Some(("Plugin".to_string(), true))
    );
    assert_eq!(find(&result, "v", 16).kind, SymbolKind::Variable);
    assert_eq!(
        find(&result, "p", 8).signature.as_deref(),
        Some("i, p := range plugins")
    );
    assert_eq!(
        find(&result, "v", 16).signature.as_deref(),
        Some("switch v := any(plugins[0]).(type)")
    );
    assert_eq!(
        find(&result, "p", 8).parent_id.as_deref(),
        Some(find(&result, "InitAll", 7).id.as_str())
    );
}

#[test]
fn explicit_type_argument_calls_are_calls() {
    let result = extract(
        r#"package store

type Set[T comparable] struct{ items map[T]struct{} }

func NewSet[T comparable](xs ...T) *Set[T] { return &Set[T]{} }
func Sum[N int | float64](xs []N) N { var t N; return t }

func use() {
	s := NewSet[string]("a")
	total := Sum[float64]([]float64{1, 2})
	_, _ = s, total
}
"#,
    );
    let relationships = relationship_rows(&result);

    assert_contains(&relationships, "calls use:function@8->NewSet:function@5");
    assert_contains(&relationships, "calls use:function@8->Sum:function@6");
    let identifiers = identifier_rows(&result);
    assert_contains(&identifiers, "call NewSet in=use:function@8");
    assert_absent(&identifiers, "type_usage NewSet");
    assert_eq!(type_of(&result, "s", 9), Some(("Set".to_string(), true)));
}

#[test]
fn chi_gorilla_and_fiber_routes_emit_facts() {
    let result = extract(
        r#"package api

import (
	"net/http"

	"github.com/go-chi/chi/v5"
	"github.com/gofiber/fiber/v2"
	"github.com/gorilla/mux"
)

func chiRoutes() {
	r := chi.NewRouter()
	r.Get("/articles/{articleID}", getArticle)
	r.Route("/admin", func(r chi.Router) {
		r.Delete("/users/{id}", deleteUser)
	})
	r.Mount("/debug", profiler())
}

func gorillaRoutes() {
	g := mux.NewRouter()
	g.HandleFunc("/products/{key}", productHandler).Methods("GET")
	api := g.PathPrefix("/api").Subrouter()
	api.HandleFunc("/items", itemsHandler).Methods(http.MethodPost)
}

func fiberRoutes() {
	app := fiber.New()
	app.Get("/health", health)
	orders := app.Group("/api")
	orders.Post("/orders/:id", createOrder)
}
"#,
    );

    let chi = facts(&result, "chi.route.v1");
    assert_contains(&chi, "GET /articles/{articleID}");
    assert_contains(&chi, "DELETE /admin/users/{id}");
    assert_contains(&facts(&result, "chi.mount.v1"), " /debug");
    let gorilla = facts(&result, "gorilla_mux.route.v1");
    assert_contains(&gorilla, "GET /products/{key}");
    assert_contains(&gorilla, "POST /api/items");
    let fiber = facts(&result, "fiber.route.v1");
    assert_contains(&fiber, "GET /health");
    assert_contains(&fiber, "POST /api/orders/:id");
}

#[test]
fn gocheck_suite_registration_marks_the_suite_struct() {
    let result = extract_at(
        "repo/repo_test.go",
        r#"package repo

import . "gopkg.in/check.v1"

type RepoSuite struct{ dir string }
type Helper struct{}

var _ = Suite(&RepoSuite{})

func (s *RepoSuite) TestOpen(c *C) {}
"#,
    );

    assert_eq!(
        role(find(&result, "RepoSuite", 5)).as_deref(),
        Some("test_container")
    );
    assert_eq!(role(find(&result, "Helper", 6)), None);
}

#[test]
fn anonymous_struct_fields_in_function_bodies_are_not_symbols() {
    let result = extract(
        r#"package store

type Row struct{ Name string }

func TestParse(t *testing.T) {
	tests := []struct {
		name string
		want string
	}{{name: "a", want: "b"}}
	_ = tests
}
"#,
    );

    assert!(result.symbols.iter().any(|s| s.name == "Name"));
    assert!(
        !result
            .symbols
            .iter()
            .any(|s| s.kind == SymbolKind::Field && (s.name == "name" || s.name == "want")),
        "{:#?}",
        names(&result)
    );
    assert!(
        result
            .identifiers
            .iter()
            .all(|identifier| identifier.kind != IdentifierKind::Call || identifier.name != "want")
    );
}

#[test]
fn unclosed_struct_does_not_read_following_code_as_embedded_fields() {
    let result = extract(
        r#"package store

type MissingBrace struct {
    field int

func VariadicFunction(format string, args ...interface{}) {
    fmt.Printf(format, args...)
}
"#,
    );
    assert!(
        pending_rows(&result)
            .iter()
            .chain(relationship_rows(&result).iter())
            .all(|row| !row.starts_with("extends")),
        "{:#?}",
        pending_rows(&result)
    );
}
