use crate::base::{ExtractionResults, SourceRegionKind, Symbol, SymbolKind, Visibility};
use crate::extract_canonical;
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("extraction succeeds")
}

fn symbol_name(results: &ExtractionResults, id: &str) -> String {
    results
        .symbols
        .iter()
        .find(|symbol| symbol.id == id)
        .map(|symbol| symbol.name.clone())
        .unwrap_or_else(|| format!("<{id}>"))
}

fn find<'a>(results: &'a ExtractionResults, name: &str, kind: SymbolKind) -> &'a Symbol {
    results
        .symbols
        .iter()
        .find(|symbol| symbol.name == name && symbol.kind == kind)
        .unwrap_or_else(|| panic!("{name} {kind:?} in {:#?}", symbol_rows(results)))
}

fn parent_name(results: &ExtractionResults, symbol: &Symbol) -> String {
    symbol
        .parent_id
        .as_deref()
        .map(|id| symbol_name(results, id))
        .unwrap_or_default()
}

fn metadata(symbol: &Symbol, key: &str) -> serde_json::Value {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .cloned()
        .unwrap_or(serde_json::Value::Null)
}

fn symbol_rows(results: &ExtractionResults) -> Vec<String> {
    results
        .symbols
        .iter()
        .map(|symbol| {
            format!(
                "{:?} {} parent={}",
                symbol.kind,
                symbol.name,
                parent_name(results, symbol)
            )
        })
        .collect()
}

fn relationship_rows(results: &ExtractionResults) -> Vec<String> {
    results
        .relationships
        .iter()
        .map(|relationship| {
            format!(
                "{:?} {} -> {}",
                relationship.kind,
                symbol_name(results, &relationship.from_symbol_id),
                symbol_name(results, &relationship.to_symbol_id),
            )
        })
        .collect()
}

fn pending_rows(results: &ExtractionResults) -> Vec<String> {
    results
        .structured_pending_relationships
        .iter()
        .map(|pending| {
            format!(
                "{:?} {} -> {} imp={}",
                pending.pending.kind,
                symbol_name(results, &pending.pending.from_symbol_id),
                pending.target.display_name,
                pending.target.import_context.as_deref().unwrap_or(""),
            )
        })
        .collect()
}

fn type_fact(results: &ExtractionResults, symbol: &Symbol) -> Option<String> {
    results
        .types
        .get(&symbol.id)
        .map(|info| info.resolved_type.clone())
}

#[test]
fn generators_function_expressions_and_default_exports_are_functions() {
    let source = r#"import { remote } from './remote';
export function* idGenerator(start: number): Generator<number> { yield start; remote(); }
function* range(n: number) { for (let i = 0; i < n; i++) yield i; }
export function sum(n: number) { let t = 0; for (const v of range(n)) t += v; return t; }
export const handler = function (req: Request): Response { return new Response(sum(1)); };
export const named = async function handle() { return sum(2); };
export default function () { remote(); }
"#;
    let results = extract("src/fnforms.ts", source);
    let id_generator = find(&results, "idGenerator", SymbolKind::Function);
    assert_eq!(
        metadata(id_generator, "isGenerator"),
        serde_json::json!(true)
    );
    assert_eq!(
        metadata(id_generator, "parameters"),
        serde_json::json!(["start: number"])
    );
    let range = find(&results, "range", SymbolKind::Function);
    let loop_var = find(&results, "i", SymbolKind::Variable);
    assert_eq!(loop_var.parent_id.as_deref(), Some(range.id.as_str()));
    find(&results, "handler", SymbolKind::Function);
    find(&results, "named", SymbolKind::Function);
    find(&results, "default", SymbolKind::Function);

    let relationships = relationship_rows(&results);
    for expected in [
        "Calls sum -> range",
        "Calls handler -> sum",
        "Calls named -> sum",
    ] {
        assert!(
            relationships.contains(&expected.to_string()),
            "{relationships:#?}"
        );
    }
    let pending = pending_rows(&results);
    assert!(pending.contains(&"Calls idGenerator -> remote imp=remote".to_string()));
    assert!(pending.contains(&"Calls default -> remote imp=remote".to_string()));
    assert!(
        !pending
            .iter()
            .any(|row| row.contains("-> range") || row.contains("-> sum")),
        "{pending:#?}"
    );
}

#[test]
fn overloads_fold_into_the_implementation_and_ambient_signatures_are_declarations() {
    let source = r#"export function parse(input: string): number;
export function parse(input: any): number { return Number(input); }
declare function externalHelper(x: number): string;
declare namespace MyLib { function init(): void; }
"#;
    let results = extract("src/overloads.ts", source);
    let parses: Vec<&Symbol> = results
        .symbols
        .iter()
        .filter(|symbol| symbol.name == "parse")
        .collect();
    assert_eq!(parses.len(), 2, "one function and one export row");
    let parse = find(&results, "parse", SymbolKind::Function);
    assert_eq!(
        parse.signature.as_deref(),
        Some("parse(input: any): number")
    );
    assert_eq!(
        metadata(parse, "overloads"),
        serde_json::json!(["parse(input: string): number"])
    );
    assert_eq!(metadata(parse, "returnType"), serde_json::json!("number"));

    let helper = find(&results, "externalHelper", SymbolKind::Function);
    assert_eq!(metadata(helper, "isDefinition"), serde_json::json!(false));
    let init = find(&results, "init", SymbolKind::Function);
    assert_eq!(parent_name(&results, init), "MyLib");
    assert!(
        !results
            .identifiers
            .iter()
            .any(|identifier| identifier.name == "externalHelper"),
        "a declaration name is not a read"
    );
}

#[test]
fn ambient_and_legacy_module_blocks_are_namespaces() {
    let source = r#"declare module "express" { interface Request { user: string } }
declare global { interface Window { app: string } }
module Legacy { export interface Old {} }
"#;
    let results = extract("src/ambient.d.ts", source);
    for (member, owner) in [
        ("Request", "express"),
        ("Window", "global"),
        ("Old", "Legacy"),
    ] {
        let namespace = find(&results, owner, SymbolKind::Namespace);
        let interface = find(&results, member, SymbolKind::Interface);
        assert_eq!(interface.parent_id.as_deref(), Some(namespace.id.as_str()));
    }
}

#[test]
fn arrow_fields_object_members_and_initializers_own_their_calls() {
    let source = r#"function validate(x: number): boolean { return x > 0; }
export class Form {
  onSubmit = (x: number) => { validate(x); };
  private checker = validate(3);
}
export const handlers = {
  submit: (x: number) => validate(x),
  reset() { validate(0); },
};
export const memoized = useMemo(() => validate(1), []);
export const computed = validate(2);
"#;
    let results = extract("src/fieldcalls.ts", source);
    let on_submit = find(&results, "onSubmit", SymbolKind::Method);
    assert_eq!(parent_name(&results, on_submit), "Form");
    for member in ["submit", "reset"] {
        let method = find(&results, member, SymbolKind::Method);
        assert_eq!(parent_name(&results, method), "handlers");
    }
    let relationships = relationship_rows(&results);
    for caller in [
        "onSubmit", "checker", "submit", "reset", "memoized", "computed",
    ] {
        let expected = format!("Calls {caller} -> validate");
        assert!(
            relationships.contains(&expected),
            "{expected}: {relationships:#?}"
        );
    }
    let validate_calls: Vec<_> = results
        .identifiers
        .iter()
        .filter(|identifier| identifier.name == "validate")
        .map(|identifier| {
            identifier
                .containing_symbol_id
                .as_deref()
                .map(|id| symbol_name(&results, id))
                .unwrap_or_default()
        })
        .collect();
    assert_eq!(
        validate_calls,
        [
            "onSubmit", "checker", "submit", "reset", "memoized", "computed"
        ]
    );
}

#[test]
fn interface_extends_lists_emit_every_extends_edge() {
    let source = r#"import { Entity } from './entity';
interface Named { name: string }
export interface User extends Named, Entity {}
interface Admin extends User {}
class Impl implements Named { name = ""; }
"#;
    let results = extract("src/iface.ts", source);
    let relationships = relationship_rows(&results);
    for expected in [
        "Extends User -> Named",
        "Extends Admin -> User",
        "Implements Impl -> Named",
    ] {
        assert!(
            relationships.contains(&expected.to_string()),
            "{relationships:#?}"
        );
    }
    let pending = pending_rows(&results);
    assert!(
        pending.contains(&"Extends User -> Entity imp=Entity".to_string()),
        "{pending:#?}"
    );
}

#[test]
fn constructor_parameter_properties_are_class_properties() {
    let source = r#"import { UsersService } from "./users.service";
export class UsersController {
  constructor(private readonly usersService: UsersService, public name: string, plain: number) {}
}
"#;
    let results = extract("src/users.controller.ts", source);
    let controller = find(&results, "UsersController", SymbolKind::Class);
    let service = find(&results, "usersService", SymbolKind::Property);
    assert_eq!(service.parent_id.as_deref(), Some(controller.id.as_str()));
    assert_eq!(service.visibility, Some(Visibility::Private));
    assert_eq!(metadata(service, "isReadonly"), serde_json::json!(true));
    assert_eq!(
        type_fact(&results, service).as_deref(),
        Some("UsersService")
    );
    let name = find(&results, "name", SymbolKind::Property);
    assert_eq!(name.visibility, Some(Visibility::Public));
    assert!(
        !results
            .symbols
            .iter()
            .any(|symbol| symbol.name == "plain" && symbol.kind == SymbolKind::Property)
    );
    find(&results, "usersService", SymbolKind::Variable);
}

#[test]
fn type_facts_attach_to_the_declaring_symbol_and_return_types_are_declared() {
    let source = r#"export interface Deps { repo: UserRepo; }
export function encode(x: object) { const data = JSON.stringify(x); return data.length; }
export function count() { const data = 42; const repo = "x"; return data; }
export function getUser(id: string): User { return {} as User; }
export async function listUsers(): Promise<User[]> { return []; }
class Repo { byId(): User { return {} as User; } }
"#;
    let results = extract("src/inferred.ts", source);
    let fact_for = |name: &str, kind: SymbolKind, parent: &str| {
        let symbol = results
            .symbols
            .iter()
            .find(|symbol| {
                symbol.name == name
                    && symbol.kind == kind
                    && parent_name(&results, symbol) == parent
            })
            .unwrap_or_else(|| panic!("{name} in {parent}"));
        type_fact(&results, symbol)
    };
    assert_eq!(
        fact_for("data", SymbolKind::Variable, "encode").as_deref(),
        Some("string")
    );
    assert_eq!(
        fact_for("data", SymbolKind::Variable, "count").as_deref(),
        Some("number")
    );
    assert_eq!(
        fact_for("repo", SymbolKind::Variable, "count").as_deref(),
        Some("string")
    );
    assert_eq!(
        fact_for("repo", SymbolKind::Property, "Deps").as_deref(),
        Some("UserRepo")
    );
    assert_eq!(
        fact_for("getUser", SymbolKind::Function, "").as_deref(),
        Some("User")
    );
    assert_eq!(
        fact_for("listUsers", SymbolKind::Function, "").as_deref(),
        Some("Promise")
    );
    assert_eq!(
        fact_for("byId", SymbolKind::Method, "Repo").as_deref(),
        Some("User")
    );
    assert_eq!(fact_for("encode", SymbolKind::Function, ""), None);
    assert!(
        results
            .types
            .values()
            .all(|info| info.resolved_type != "function" && info.resolved_type != "any"),
        "no placeholder facts"
    );
    for symbol_id in results.types.keys() {
        let symbol = results
            .symbols
            .iter()
            .find(|symbol| &symbol.id == symbol_id)
            .unwrap();
        assert_ne!(symbol.kind, SymbolKind::Export);
    }
}

#[test]
fn exports_are_public_and_emit_one_row_per_exported_name() {
    let source = r#"export const Profile = () => null;
export const config = { port: 3000 };
class C { #secret = 1; #hidden(): void {} }
const local = 1; function localFn() {}
export { local, localFn as renamedFn };
export { x, y as why } from './xy';
export { UsersService, type UserRecord } from "./users.service";
export * as helpers from './helpers';
export * from './all';
import { helper } from "./helper";
export default helper;
"#;
    let results = extract("src/exports.ts", source);
    assert_eq!(
        find(&results, "Profile", SymbolKind::Function).visibility,
        Some(Visibility::Public)
    );
    assert_eq!(
        find(&results, "config", SymbolKind::Variable).visibility,
        Some(Visibility::Public)
    );
    assert_eq!(
        find(&results, "local", SymbolKind::Variable).visibility,
        Some(Visibility::Public)
    );
    assert_eq!(
        find(&results, "localFn", SymbolKind::Function).visibility,
        Some(Visibility::Public)
    );
    assert_eq!(
        find(&results, "#secret", SymbolKind::Property).visibility,
        Some(Visibility::Private)
    );
    assert_eq!(
        find(&results, "#hidden", SymbolKind::Method).visibility,
        Some(Visibility::Private)
    );

    let exports: Vec<String> = results
        .symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Export)
        .map(|symbol| {
            format!(
                "{} local={} source={} default={} ns={} star={} type={}",
                symbol.name,
                metadata(symbol, "localName").as_str().unwrap_or(""),
                metadata(symbol, "source").as_str().unwrap_or(""),
                metadata(symbol, "isDefault"),
                !metadata(symbol, "isNamespace").is_null(),
                !metadata(symbol, "isStar").is_null(),
                !metadata(symbol, "isTypeOnly").is_null(),
            )
        })
        .collect();
    assert_eq!(
        exports,
        [
            "Profile local=Profile source= default=false ns=false star=false type=false",
            "config local=config source= default=false ns=false star=false type=false",
            "local local=local source= default=false ns=false star=false type=false",
            "renamedFn local=localFn source= default=false ns=false star=false type=false",
            "x local=x source=./xy default=false ns=false star=false type=false",
            "why local=y source=./xy default=false ns=false star=false type=false",
            "UsersService local=UsersService source=./users.service default=false ns=false star=false type=false",
            "UserRecord local=UserRecord source=./users.service default=false ns=false star=false type=true",
            "helpers local= source=./helpers default=false ns=true star=false type=false",
            "* local= source=./all default=false ns=false star=true type=false",
            "helper local=helper source= default=true ns=false star=false type=false",
        ]
    );
}

#[test]
fn type_alias_members_belong_to_the_alias_and_inline_object_types_emit_nothing() {
    let source = r#"export type ParseParams = { path: string[]; async: boolean; run(): void };
export interface Options { probe?: (url: string, opts?: { timeoutMs?: number }) => Promise<unknown>; }
export function makeIssue(params: { data: unknown; path: string[] }): void {}
export class Editor extends Base<{ id: string }> {}
"#;
    let results = extract("src/literals.ts", source);
    let members: Vec<String> = results
        .symbols
        .iter()
        .filter(|symbol| matches!(symbol.kind, SymbolKind::Property | SymbolKind::Method))
        .map(|symbol| format!("{} parent={}", symbol.name, parent_name(&results, symbol)))
        .collect();
    assert_eq!(
        members,
        [
            "path parent=ParseParams",
            "async parent=ParseParams",
            "run parent=ParseParams",
            "probe parent=Options",
        ]
    );
    assert_eq!(
        find(&results, "path", SymbolKind::Property)
            .signature
            .as_deref(),
        Some("path: string[]")
    );
}

#[test]
fn property_and_parameter_decorators_are_annotations() {
    let source = r#"@Entity()
export class User {
  @Column({ unique: true })
  @IsEmail()
  email!: string;
}
@Controller("users")
export class UsersController {
  @Post()
  create(@Body() dto: User) {}
}
"#;
    let results = extract("src/decorators.ts", source);
    let keys = |name: &str, kind: SymbolKind| -> Vec<String> {
        find(&results, name, kind)
            .annotations
            .iter()
            .map(|annotation| annotation.annotation.clone())
            .collect()
    };
    assert_eq!(keys("email", SymbolKind::Property), ["Column", "IsEmail"]);
    assert_eq!(keys("dto", SymbolKind::Variable), ["Body"]);
    assert_eq!(
        find(&results, "email", SymbolKind::Property)
            .signature
            .as_deref(),
        Some("@Column @IsEmail email: string")
    );
}

#[test]
fn module_dependency_forms_emit_import_rows() {
    let source = r#"import Legacy = require('./legacy');
import './polyfills';
const Settings = lazy(() => import('./pages/Settings'));
export async function loadAdmin() {
  const mod = await import('./admin');
  const cfg = require('./config');
  Legacy.run();
}
"#;
    let results = extract("src/moddeps.ts", source);
    let imports: Vec<String> = results
        .symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Import)
        .map(|symbol| {
            format!(
                "{} source={} cjs={} side={} dynamic={}",
                symbol.name,
                metadata(symbol, "source").as_str().unwrap_or(""),
                !metadata(symbol, "isCommonJS").is_null(),
                !metadata(symbol, "isSideEffect").is_null(),
                !metadata(symbol, "isDynamic").is_null(),
            )
        })
        .collect();
    assert_eq!(
        imports,
        [
            "Legacy source=./legacy cjs=true side=false dynamic=false",
            "./polyfills source=./polyfills cjs=false side=true dynamic=false",
            "./pages/Settings source=./pages/Settings cjs=false side=false dynamic=true",
            "./admin source=./admin cjs=false side=false dynamic=true",
            "cfg source=./config cjs=true side=false dynamic=false",
        ]
    );
    let pending = pending_rows(&results);
    assert!(pending.contains(&"Calls loadAdmin -> Legacy.run imp=Legacy".to_string()));
    assert!(
        !pending.iter().any(|row| row.contains("-> require")),
        "{pending:#?}"
    );
}

#[test]
fn string_type_keywords_are_not_string_literal_regions() {
    let source =
        "function f(a: string, b: number): string {\n  let c: string = \"real\";\n  return c;\n}\n";
    let results = extract("src/strkw.ts", source);
    let strings: Vec<u32> = results
        .source_regions
        .iter()
        .filter(|region| region.kind == SourceRegionKind::StringLiteral)
        .map(|region| region.start_line)
        .collect();
    assert_eq!(strings, [2]);
}

#[test]
fn pending_rows_skip_computed_callees_and_carry_type_import_context() {
    let source = r#"import { Base } from './base';
import connect from './connect';
import { remote } from './remote';
class Child extends Base { constructor() { super(); } }
export function run(a: any, C: any) {
  void (async () => { await remote(); })();
  const Connected = connect(a)(C);
  return new Base();
}
"#;
    let results = extract("src/pendq.ts", source);
    let pending = pending_rows(&results);
    assert_eq!(
        pending,
        [
            "Instantiates run -> Base imp=Base",
            "Extends Child -> Base imp=Base",
            "Calls run -> remote imp=remote",
            "Calls run -> connect imp=connect",
        ]
    );
}

#[test]
fn class_expressions_are_classes_named_by_their_binding() {
    let source = r#"class Local { ping() {} }
export const Widget = class extends Local { render() { return this.ping(); } };
export default class { run() {} }
"#;
    let results = extract("src/classexpr.ts", source);
    let widget = find(&results, "Widget", SymbolKind::Class);
    assert_eq!(widget.visibility, Some(Visibility::Public));
    let render = find(&results, "render", SymbolKind::Method);
    assert_eq!(render.parent_id.as_deref(), Some(widget.id.as_str()));
    let default_class = find(&results, "default", SymbolKind::Class);
    let run = find(&results, "run", SymbolKind::Method);
    assert_eq!(run.parent_id.as_deref(), Some(default_class.id.as_str()));
    assert!(
        relationship_rows(&results).contains(&"Extends Widget -> Local".to_string()),
        "{:#?}",
        relationship_rows(&results)
    );
}

fn fact_rows(results: &ExtractionResults, pattern_id: &str, keys: &[&str]) -> Vec<String> {
    results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == pattern_id)
        .map(|fact| {
            keys.iter()
                .map(|key| {
                    fact.metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get(*key))
                        .and_then(|value| value.as_str())
                        .unwrap_or("-")
                        .to_string()
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect()
}

#[test]
fn angular_route_arrays_emit_route_definitions() {
    let source = r#"import { NgModule } from '@angular/core';
import { Routes, RouterModule } from '@angular/router';
import { UsersComponent } from './users.component';
import { ShellComponent } from './shell.component';
const routes: Routes = [
  { path: 'users', component: UsersComponent },
  { path: 'admin', loadChildren: () => import('./admin/admin.module').then(m => m.AdminModule) },
  { path: 'app', component: ShellComponent, children: [
    { path: '', redirectTo: 'home', pathMatch: 'full' },
    { path: 'profile/:id', loadComponent: () => import('./profile.component').then(m => m.ProfileComponent) },
  ] },
];
const unrelated = [{ path: 'not-a-route' }];
@NgModule({ imports: [RouterModule.forRoot(routes), RouterModule.forChild([{ path: 'child', component: UsersComponent }])] })
export class AppRoutingModule {}
"#;
    let results = extract("app-routing.module.ts", source);
    assert_eq!(
        fact_rows(
            &results,
            "angular.route_definition.v1",
            &[
                "effective_route_template",
                "route_component",
                "redirect_to",
                "lazy_module_source",
                "lazy_component_source",
            ],
        ),
        vec![
            "/users UsersComponent - - -",
            "/admin - - ./admin/admin.module -",
            "/app ShellComponent - - -",
            "/app - home - -",
            "/app/profile/:id - - - ./profile.component",
            "/child UsersComponent - - -",
        ]
    );
}

#[test]
fn angular_route_facts_need_the_angular_router_import() {
    let source = r#"type Routes = { path: string }[];
const routes: Routes = [{ path: 'users' }];
"#;
    let results = extract("routes.ts", source);
    assert!(fact_rows(&results, "angular.route_definition.v1", &["route_path"]).is_empty());
}

#[test]
fn programmatic_navigation_calls_are_route_references() {
    let source = r#"import { useNavigate } from 'react-router-dom';
import { useRouter } from 'next/navigation';
export function useSave() {
  const navigate = useNavigate();
  const router = useRouter();
  return () => {
    navigate("/settings");
    router.push("/users/1");
    router.replace(`/home`);
    navigate(-1);
    router.back();
    router.push(`/users/${1}`);
  };
}
"#;
    let results = extract("save.ts", source);
    let keys = [
        "navigation_call",
        "target_path",
        "import_source",
        "source_kind",
    ];
    assert_eq!(
        fact_rows(&results, "react.route_reference.v1", &keys),
        vec!["navigate /settings react-router-dom react_router_navigate"]
    );
    assert_eq!(
        fact_rows(&results, "nextjs.route_reference.v1", &keys),
        vec![
            "router.push /users/1 next/navigation next_router_navigation",
            "router.replace /home next/navigation next_router_navigation",
        ]
    );
}
