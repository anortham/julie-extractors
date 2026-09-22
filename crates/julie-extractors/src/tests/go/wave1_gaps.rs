use crate::ExtractionResults;
use crate::base::SymbolKind;
use crate::extract_canonical;
use std::path::Path;

fn extract(source: &str) -> ExtractionResults {
    extract_canonical("app/main.go", source, Path::new("/tmp/test"))
        .expect("canonical Go extraction must succeed")
}

fn symbol_name(result: &ExtractionResults, id: &str) -> String {
    result
        .symbols
        .iter()
        .find(|symbol| symbol.id == id)
        .map(|symbol| format!("{}:{}@{}", symbol.name, symbol.kind, symbol.start_line))
        .unwrap_or_default()
}

fn relationship_rows(result: &ExtractionResults) -> Vec<String> {
    result
        .relationships
        .iter()
        .map(|rel| {
            format!(
                "{} {}->{}",
                rel.kind,
                symbol_name(result, &rel.from_symbol_id),
                symbol_name(result, &rel.to_symbol_id)
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
                "{} {}->{} recv={:?}",
                pending.pending.kind,
                symbol_name(result, &pending.pending.from_symbol_id),
                pending.target.display_name,
                pending.target.receiver
            )
        })
        .collect()
}

fn symbol<'a>(result: &'a ExtractionResults, name: &str) -> &'a crate::base::Symbol {
    result
        .symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap_or_else(|| {
            panic!(
                "{name} missing: {:?}",
                result
                    .symbols
                    .iter()
                    .map(|symbol| format!("{}:{}", symbol.name, symbol.kind))
                    .collect::<Vec<_>>()
            )
        })
}

fn parent(result: &ExtractionResults, name: &str) -> String {
    symbol(result, name)
        .parent_id
        .as_deref()
        .map(|id| symbol_name(result, id))
        .unwrap_or_default()
}

fn assert_contains(rows: &[String], expected: &str) {
    assert!(
        rows.iter().any(|row| row == expected),
        "missing {expected:?} in {rows:#?}"
    );
}

#[test]
fn grouped_type_declarations_emit_every_spec_with_its_own_fields() {
    let result = extract(
        r#"package store

type (
	// UserID identifies a user.
	UserID int64
	// Repository finds users.
	Repository interface { Find(id UserID) error }
	memRepo struct {
		mu    sync.Mutex
		users map[UserID]string
	}
)

type (
	userID  = int64
	OrderID = string
)
"#,
    );

    assert_eq!(symbol(&result, "UserID").kind, SymbolKind::Type);
    assert_eq!(symbol(&result, "Repository").kind, SymbolKind::Interface);
    assert_eq!(symbol(&result, "memRepo").kind, SymbolKind::Struct);
    assert_eq!(symbol(&result, "OrderID").kind, SymbolKind::Type);
    assert_eq!(symbol(&result, "userID").kind, SymbolKind::Type);
    assert_eq!(parent(&result, "mu"), "memRepo:struct@8");
    assert_eq!(parent(&result, "users"), "memRepo:struct@8");
    assert_eq!(
        symbol(&result, "Repository").doc_comment.as_deref(),
        Some("// Repository finds users.")
    );
}

#[test]
fn interface_method_elements_become_methods_of_the_interface() {
    let result = extract(
        r#"package store

// UserStore persists users.
type UserStore interface {
	// Get loads one user by id.
	Get(ctx context.Context, id string) (*User, error)
	Put(ctx context.Context, u *User) error
	models.Closer
}
"#,
    );

    let store = symbol(&result, "UserStore");
    assert_eq!(store.start_line, 4);
    assert_eq!(store.end_line, 9);
    assert!(store.body_hash.is_some());

    let get = symbol(&result, "Get");
    assert_eq!(get.kind, SymbolKind::Method);
    assert_eq!(parent(&result, "Get"), "UserStore:interface@4");
    assert_eq!(
        get.signature.as_deref(),
        Some("Get(ctx context.Context, id string) (*User, error)")
    );
    assert_eq!(
        get.doc_comment.as_deref(),
        Some("// Get loads one user by id.")
    );
    assert_eq!(
        symbol(&result, "Put").signature.as_deref(),
        Some("Put(ctx context.Context, u *User) error")
    );
}

#[test]
fn function_signatures_keep_parameters_and_parenthesized_results() {
    let result = extract(
        r#"package calc

func LoadUser(id int, name string) (*User, error) { return nil, nil }
func Divide(a, b float64) (q float64, err error) { return }
func Pair() (int, error) { return 0, nil }
func Named() (err error) { return }
"#,
    );

    let signature = |name| symbol(&result, name).signature.clone().unwrap();
    assert_eq!(
        signature("LoadUser"),
        "func LoadUser(id int, name string) (*User, error)"
    );
    assert_eq!(
        signature("Divide"),
        "func Divide(a, b float64) (q float64, err error)"
    );
    assert_eq!(signature("Pair"), "func Pair() (int, error)");
    assert_eq!(signature("Named"), "func Named() (err error)");
}

#[test]
fn duplicate_method_names_keep_their_calls_and_receiver_uses() {
    let result = extract(
        r#"package shapes

type A struct{}
type B struct{}

func format(n int) string { return "" }

func (a A) String() string { return format(1) + render.Pretty(a) }
func (b B) String() string { return format(2) + render.Pretty(b) }

type Config struct { Validate bool }

func load() {}
func (c *Config) Validate() error { load(); return nil }
"#,
    );

    let rels = relationship_rows(&result);
    assert_contains(&rels, "calls String:method@8->format:function@6");
    assert_contains(&rels, "calls String:method@9->format:function@6");
    assert_contains(&rels, "uses String:method@8->A:struct@3");
    assert_contains(&rels, "uses String:method@9->B:struct@4");
    assert_contains(&rels, "calls Validate:method@14->load:function@13");
    let pending = pending_rows(&result);
    assert_contains(
        &pending,
        "calls String:method@8->render.Pretty recv=Some(\"render\")",
    );
    assert_contains(
        &pending,
        "calls String:method@9->render.Pretty recv=Some(\"render\")",
    );
}

#[test]
fn calls_in_package_level_var_initializers_are_attributed_to_the_variable() {
    let result = extract(
        r#"package cmd

var rootCmd = &cobra.Command{
	RunE: func(cmd *cobra.Command, args []string) error {
		srv := server.New("addr")
		return initConfig()
	},
}

func initConfig() error { return nil }
"#,
    );

    assert_contains(
        &relationship_rows(&result),
        "calls rootCmd:variable@3->initConfig:function@10",
    );
    assert_contains(
        &pending_rows(&result),
        "calls rootCmd:variable@3->server.New recv=Some(\"server\")",
    );
    assert_eq!(parent(&result, "srv"), "rootCmd:variable@3");
}

#[test]
fn calls_on_call_results_stay_pending_with_the_expression_receiver() {
    let result = extract(
        r#"package repo

type Repo struct{}

func (r *Repo) Close() error { return nil }

func Run(r *Repo) error {
	r.conn.Tx().Close()
	getConn()[0].Close()
	return nil
}

func Items(r *Repo) []string { return r.conn.Query().Items() }
"#,
    );

    let rels = relationship_rows(&result);
    assert!(
        !rels.iter().any(|row| row.contains("->Close:method")),
        "chained calls must not resolve by terminal name: {rels:#?}"
    );
    assert!(
        !rels
            .iter()
            .any(|row| row.contains("Items:function@13->Items")),
        "no false self recursion: {rels:#?}"
    );
    let pending = pending_rows(&result);
    assert_contains(
        &pending,
        "calls Run:function@7->r.conn.Tx().Close recv=Some(\"r.conn.Tx()\")",
    );
    assert_contains(
        &pending,
        "calls Run:function@7->getConn()[0].Close recv=Some(\"getConn()[0]\")",
    );
    assert_contains(
        &pending,
        "calls Items:function@13->r.conn.Query().Items recv=Some(\"r.conn.Query()\")",
    );
}

#[test]
fn function_text_inside_strings_and_comments_is_not_recovered_as_symbols() {
    let result = extract(
        "package gen\n\nconst tmpl = `package {{.Pkg}}\n\nfunc TestGenerated(t *testing.T) {\n\trun()\n}\n`\n\n/*\nfunc oldImpl(a int) error {\n\treturn nil\n}\n*/\n\nfunc real() {}\n",
    );

    let functions: Vec<&str> = result
        .symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Function)
        .map(|symbol| symbol.name.as_str())
        .collect();
    assert_eq!(functions, ["real"]);
}

#[test]
fn import_names_follow_the_go_default_package_name() {
    let result = extract(
        r#"package app

import (
	"gopkg.in/yaml.v3"
	"github.com/jackc/pgx/v5"
	"github.com/mattn/go-sqlite3"
	_ "github.com/lib/pq"
	. "github.com/onsi/gomega"
	str "strings"
)

func load() {
	yaml.Unmarshal(nil, nil)
}
"#,
    );

    let mut imports: Vec<&str> = result
        .symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Import)
        .map(|symbol| symbol.name.as_str())
        .collect();
    imports.sort_unstable();
    assert_eq!(imports, ["gomega", "pgx", "sqlite3", "str", "yaml"]);
    let gomega = symbol(&result, "gomega");
    assert_eq!(
        gomega.metadata.as_ref().unwrap().get("dotImport"),
        Some(&serde_json::Value::Bool(true))
    );
    let unmarshal = result
        .structured_pending_relationships
        .iter()
        .find(|pending| pending.target.terminal_name == "Unmarshal")
        .expect("pending yaml.Unmarshal");
    assert_eq!(
        unmarshal.target.import_context.as_deref(),
        Some("gopkg.in/yaml.v3")
    );
}

#[test]
fn methods_record_their_receiver_type() {
    let result = extract(
        r#"package cobra

// Name returns the command's name.
func (c *Command) Name() string { return c.Use }
func (c Command) Plain() {}
"#,
    );

    let metadata = |name| symbol(&result, name).metadata.clone().unwrap();
    assert_eq!(metadata("Name")["receiver_type"], "Command");
    assert_eq!(metadata("Name")["receiver_pointer"], true);
    assert_eq!(metadata("Plain")["receiver_type"], "Command");
    assert_eq!(metadata("Plain")["receiver_pointer"], false);
}

#[test]
fn routes_on_typed_router_parameters_and_fields_emit_route_facts() {
    let result = extract(
        r#"package api

import (
	"net/http"

	"github.com/gin-gonic/gin"
	"github.com/labstack/echo/v4"
)

type Server struct {
	mux *http.ServeMux
}

func (h *Handler) RegisterRoutes(r *gin.Engine) {
	v1 := r.Group("/api/v1")
	v1.GET("/users/:id", h.getUser)
}

func Mount(rg *gin.RouterGroup, h *Handler) { rg.DELETE("/users/:id", h.deleteUser) }

func registerEcho(e *echo.Echo) { e.GET("/ping", ping) }

func registerStd(mux *http.ServeMux) { mux.HandleFunc("POST /orders", createOrder) }

func (s *Server) routes() { s.mux.HandleFunc("GET /metrics", metrics) }

func newFake() *Fake {
	f := &Fake{router: http.NewServeMux()}
	f.router.HandleFunc("/token", f.token)
	return f
}
"#,
    );

    let routes: Vec<String> = result
        .structural_facts
        .iter()
        .map(|fact| {
            let meta = |key: &str| {
                fact.metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get(key))
                    .and_then(|value| value.as_str())
                    .unwrap_or("")
                    .to_string()
            };
            format!(
                "{} {} {} prefix={}",
                fact.pattern_id,
                meta("verb"),
                meta("route_template"),
                meta("route_group_prefix")
            )
        })
        .collect();
    assert_contains(&routes, "gin.route.v1 GET /users/:id prefix=/api/v1");
    assert_contains(&routes, "gin.route.v1 DELETE /users/:id prefix=");
    assert_contains(&routes, "echo.route.v1 GET /ping prefix=");
    assert_contains(&routes, "go.net_http.route.v1 POST /orders prefix=");
    assert_contains(&routes, "go.net_http.route.v1 GET /metrics prefix=");
    assert_contains(&routes, "go.net_http.route.v1  /token prefix=");
}
