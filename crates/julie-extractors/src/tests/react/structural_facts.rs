use std::collections::BTreeSet;
use std::path::Path;

use crate::base::StructuralFact;
use crate::tests::helpers::{facts_with_pattern, metadata_str};

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical React extraction should succeed")
}

fn metadata_bool(fact: &StructuralFact, key: &str) -> Option<bool> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_bool())
}

fn metadata_array<'a>(fact: &'a StructuralFact, key: &str) -> Vec<&'a str> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str())
        .collect()
}

#[test]
fn react_router_static_route_facts() {
    let source = r#"
import { Link as RouterLink, NavLink, Route, createBrowserRouter, useRoutes } from "react-router-dom";
import { Link } from "./design-system";

const unrelatedWidget = { path: "/not-a-route", index: false, element: <NotRoute /> };

const routes = [
  { path: "/dashboard", element: <Dashboard />, id: "dashboard" },
  { index: true, element: <Home /> },
];

const router = createBrowserRouter(routes);
const hookRoutes = useRoutes([{ path: "/hooks", Component: HooksPage }]);

export function AppRoutes() {
  return (
    <>
      <RouterLink to="/dashboard">Dashboard</RouterLink>
      <NavLink to="/settings">Settings</NavLink>
      <Link to="/external">External</Link>
      <RouterLink to={dynamicTarget}>Dynamic</RouterLink>
      <Route path="/reports" element={<Reports />} />
      <Route path="settings" Component={Settings} />
      <Route path={dynamicPath} element={<Dynamic />} />
    </>
  );
}
"#;

    let results = extract("src/AppRoutes.tsx", source);

    let references = facts_with_pattern(&results, "react.route_reference.v1");
    assert_eq!(references.len(), 2);
    assert_eq!(
        references
            .iter()
            .filter_map(|fact| metadata_str(fact, "target_path"))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["/dashboard", "/settings"])
    );
    let dashboard_link = references
        .iter()
        .find(|fact| metadata_str(fact, "target_path") == Some("/dashboard"))
        .expect("expected dashboard route reference");
    assert_eq!(
        metadata_str(dashboard_link, "query_family"),
        Some("frontend_navigation")
    );
    assert_eq!(metadata_str(dashboard_link, "framework"), Some("react"));
    assert_eq!(
        metadata_str(dashboard_link, "library"),
        Some("react_router")
    );
    assert_eq!(
        metadata_str(dashboard_link, "source_kind"),
        Some("react_router_link")
    );
    assert_eq!(
        metadata_str(dashboard_link, "route_source"),
        Some("string_literal")
    );
    assert_eq!(metadata_str(dashboard_link, "attribute_name"), Some("to"));
    assert_eq!(
        metadata_str(dashboard_link, "component_name"),
        Some("RouterLink")
    );
    assert_eq!(
        metadata_str(dashboard_link, "import_source"),
        Some("react-router-dom")
    );
    assert_eq!(metadata_str(dashboard_link, "verb"), Some("GET"));

    let definitions = facts_with_pattern(&results, "react.route_definition.v1");
    assert_eq!(
        definitions
            .iter()
            .filter_map(|fact| metadata_str(fact, "route_path"))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["/dashboard", "/hooks", "/reports", "settings"])
    );

    let dashboard_route = definitions
        .iter()
        .find(|fact| metadata_str(fact, "route_path") == Some("/dashboard"))
        .expect("expected dashboard route definition");
    assert_eq!(
        metadata_str(dashboard_route, "query_family"),
        Some("frontend_navigation")
    );
    assert_eq!(metadata_str(dashboard_route, "framework"), Some("react"));
    assert_eq!(
        metadata_str(dashboard_route, "library"),
        Some("react_router")
    );
    assert_eq!(
        metadata_str(dashboard_route, "source_kind"),
        Some("route_object")
    );
    assert_eq!(
        metadata_str(dashboard_route, "route_source"),
        Some("string_literal")
    );
    assert_eq!(
        metadata_str(dashboard_route, "route_component"),
        Some("Dashboard")
    );
    assert_eq!(metadata_str(dashboard_route, "route_id"), Some("dashboard"));

    let index_route = definitions
        .iter()
        .find(|fact| metadata_bool(fact, "index_route") == Some(true))
        .expect("expected index route definition");
    assert_eq!(
        metadata_str(index_route, "route_source"),
        Some("index_route")
    );
    assert_eq!(metadata_str(index_route, "route_component"), Some("Home"));

    let reports_route = definitions
        .iter()
        .find(|fact| metadata_str(fact, "route_path") == Some("/reports"))
        .expect("expected JSX Route definition");
    assert_eq!(
        metadata_str(reports_route, "source_kind"),
        Some("jsx_route")
    );
    assert_eq!(
        metadata_str(reports_route, "route_component"),
        Some("Reports")
    );
}

#[test]
fn react_route_definitions_emit_child_parent_context() {
    let source = r#"
import { createBrowserRouter } from "react-router-dom";

const routes = [
  {
    path: "/admin",
    element: <AdminLayout />,
    children: [
      { path: "settings", element: <Settings /> },
      { path: "users/:id", element: <UserDetails /> },
      { path: "/audit", element: <Audit /> },
    ],
  },
];

export const router = createBrowserRouter(routes);
"#;

    let results = extract("src/routes.tsx", source);
    let definitions = facts_with_pattern(&results, "react.route_definition.v1");

    assert_eq!(
        definitions
            .iter()
            .filter_map(|fact| metadata_str(fact, "route_path"))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["/admin", "/audit", "settings", "users/:id"])
    );

    let settings = definitions
        .iter()
        .find(|fact| metadata_str(fact, "route_path") == Some("settings"))
        .expect("expected child settings route");
    assert_eq!(metadata_str(settings, "parent_route_path"), Some("/admin"));
    assert_eq!(
        metadata_str(settings, "effective_route_template"),
        Some("/admin/settings")
    );

    let users = definitions
        .iter()
        .find(|fact| metadata_str(fact, "route_path") == Some("users/:id"))
        .expect("expected child users route");
    assert_eq!(metadata_str(users, "parent_route_path"), Some("/admin"));
    assert_eq!(
        metadata_str(users, "effective_route_template"),
        Some("/admin/users/:id")
    );

    let audit = definitions
        .iter()
        .find(|fact| metadata_str(fact, "route_path") == Some("/audit"))
        .expect("expected absolute child audit route");
    assert_eq!(metadata_str(audit, "parent_route_path"), Some("/admin"));
    assert_eq!(
        metadata_str(audit, "effective_route_template"),
        Some("/audit")
    );
}

#[test]
fn react_route_definitions_ignore_nested_redirect_and_meta_paths() {
    let source = r#"
import { createBrowserRouter } from "react-router-dom";

const routes = [
  {
    path: "/login",
    element: <Login />,
    redirect: { state: { path: "/redirect-target" } },
    meta: {
      path: "/meta-target",
      nested: [{ path: "/meta-nested-target" }],
      tabs: [{ index: true }],
    },
  },
];

export const router = createBrowserRouter(routes);
"#;

    let results = extract("src/routes.tsx", source);
    let definitions = facts_with_pattern(&results, "react.route_definition.v1");
    assert_eq!(definitions.len(), 1);
    assert_eq!(
        definitions
            .iter()
            .filter_map(|fact| metadata_str(fact, "route_path"))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["/login"])
    );
}

#[test]
fn react_jsx_nested_routes_emit_parent_context() {
    let source = r#"
import { Route, Routes } from "react-router-dom";

export function AppRoutes() {
  return (
    <Routes>
      <Route path="/admin" element={<AdminLayout />}>
        <Route path="settings" element={<Settings />} />
        <Route index element={<Overview />} />
      </Route>
    </Routes>
  );
}
"#;

    let results = extract("src/AppRoutes.tsx", source);
    let definitions = facts_with_pattern(&results, "react.route_definition.v1");
    assert_eq!(
        definitions
            .iter()
            .filter_map(|fact| metadata_str(fact, "route_path").or_else(|| {
                metadata_bool(fact, "index_route")
                    .is_some_and(|index| index)
                    .then_some("<index>")
            }))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["/admin", "settings", "<index>"])
    );

    let settings = definitions
        .iter()
        .find(|fact| metadata_str(fact, "route_path") == Some("settings"))
        .expect("expected child settings route");
    assert_eq!(metadata_str(settings, "parent_route_path"), Some("/admin"));
    assert_eq!(
        metadata_str(settings, "effective_route_template"),
        Some("/admin/settings")
    );

    let index = definitions
        .iter()
        .find(|fact| metadata_bool(fact, "index_route") == Some(true))
        .expect("expected index child route");
    assert_eq!(metadata_str(index, "parent_route_path"), Some("/admin"));
    assert_eq!(
        metadata_str(index, "effective_route_template"),
        Some("/admin")
    );
}

#[test]
fn plain_vue_router_modules_emit_vue_route_definitions() {
    let source = r#"
import { createBrowserRouter } from "react-router-dom";
import { createRouter, createWebHistory } from "vue-router";
import DashboardView from "./views/DashboardView.vue";

const routes = [
  { path: "/react-only", element: <ReactOnly /> },
];

export const reactRouter = createBrowserRouter(routes);

const vueRoutes = [
  {
    path: "/dashboard",
    component: DashboardView,
  },
];

export const router = createRouter({
  history: createWebHistory(),
  routes: vueRoutes,
});
"#;

    let results = extract("src/router.ts", source);
    let definitions = facts_with_pattern(&results, "vue.route_definition.v1");
    assert_eq!(definitions.len(), 1);
    assert_eq!(
        metadata_str(definitions[0], "target_path"),
        Some("/dashboard")
    );
    assert_eq!(
        metadata_str(definitions[0], "component_name"),
        Some("DashboardView")
    );
    assert_eq!(metadata_str(definitions[0], "framework"), Some("vue"));

    let no_vue_router_results = extract(
        "src/routes.ts",
        r#"
const routes = [
  { path: "/not-vue-router", component: Widget },
];
"#,
    );
    assert!(
        facts_with_pattern(&no_vue_router_results, "vue.route_definition.v1").is_empty(),
        "plain route-shaped objects must require vue-router evidence"
    );
}

#[test]
fn nextjs_static_route_facts() {
    let app_source = r#"
import Link from "next/link";
import { Link as DesignLink } from "./design-system";

export default function Page() {
  const dynamicHref = "/dynamic";
  return (
    <main>
      <Link href="/dashboard">Dashboard</Link>
      <Link href={{ pathname: "/about", query: { name: "test" } }}>About</Link>
      <Link href={dynamicHref}>Dynamic</Link>
      <Link href={{ query: { name: "missing" } }}>Missing pathname</Link>
      <DesignLink href="/ignored">Ignored</DesignLink>
    </main>
  );
}
"#;

    let app_results = extract("app/page.tsx", app_source);
    let references = facts_with_pattern(&app_results, "nextjs.route_reference.v1");
    assert_eq!(references.len(), 2);
    assert_eq!(
        references
            .iter()
            .filter_map(|fact| metadata_str(fact, "target_path"))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["/about", "/dashboard"])
    );
    let dashboard_link = references
        .iter()
        .find(|fact| metadata_str(fact, "target_path") == Some("/dashboard"))
        .expect("expected dashboard Next.js route reference");
    assert_eq!(
        metadata_str(dashboard_link, "query_family"),
        Some("frontend_navigation")
    );
    assert_eq!(metadata_str(dashboard_link, "framework"), Some("nextjs"));
    assert_eq!(
        metadata_str(dashboard_link, "source_kind"),
        Some("next_link")
    );
    assert_eq!(
        metadata_str(dashboard_link, "route_source"),
        Some("string_literal")
    );
    assert_eq!(metadata_str(dashboard_link, "attribute_name"), Some("href"));
    assert_eq!(metadata_str(dashboard_link, "component_name"), Some("Link"));
    assert_eq!(
        metadata_str(dashboard_link, "import_source"),
        Some("next/link")
    );
    assert_eq!(metadata_str(dashboard_link, "verb"), Some("GET"));

    let about_link = references
        .iter()
        .find(|fact| metadata_str(fact, "target_path") == Some("/about"))
        .expect("expected object pathname Next.js route reference");
    assert_eq!(
        metadata_str(about_link, "route_source"),
        Some("object_pathname_literal")
    );

    let app_routes = facts_with_pattern(&app_results, "nextjs.file_route.v1");
    assert_eq!(app_routes.len(), 1);
    assert_eq!(metadata_str(app_routes[0], "framework"), Some("nextjs"));
    assert_eq!(metadata_str(app_routes[0], "router"), Some("app"));
    assert_eq!(metadata_str(app_routes[0], "file_convention"), Some("page"));
    assert_eq!(metadata_str(app_routes[0], "route_path"), Some("/"));
    assert_eq!(
        metadata_str(app_routes[0], "source_kind"),
        Some("nextjs_file_route")
    );

    let slug_results = extract(
        "app/(marketing)/blog/[slug]/page.tsx",
        "export default function Page() { return <h1>Post</h1>; }",
    );
    let slug_routes = facts_with_pattern(&slug_results, "nextjs.file_route.v1");
    assert_eq!(slug_routes.len(), 1);
    assert_eq!(metadata_str(slug_routes[0], "router"), Some("app"));
    assert_eq!(
        metadata_str(slug_routes[0], "route_path"),
        Some("/blog/[slug]")
    );
    assert_eq!(
        metadata_str(slug_routes[0], "normalized_route_template"),
        Some("/blog/:slug")
    );
    assert_eq!(
        metadata_array(slug_routes[0], "dynamic_segments"),
        vec!["slug"]
    );
    assert_eq!(
        metadata_array(slug_routes[0], "route_group_segments"),
        vec!["marketing"]
    );

    let pages_results = extract(
        "pages/dashboard.tsx",
        r#"
export async function getStaticProps() {
  return { props: {} };
}

export default function Dashboard() { return <h1>Dashboard</h1>; }
"#,
    );
    let pages_routes = facts_with_pattern(&pages_results, "nextjs.file_route.v1");
    assert_eq!(pages_routes.len(), 1);
    assert_eq!(metadata_str(pages_routes[0], "router"), Some("pages"));
    assert_eq!(
        metadata_str(pages_routes[0], "route_path"),
        Some("/dashboard")
    );
    assert!(
        facts_with_pattern(&pages_results, "nuxt.file_route.v1").is_empty(),
        "Next.js Pages Router files must not emit Nuxt file routes"
    );

    let api_results = extract(
        "pages/api/status.ts",
        "export async function handler(): Promise<Response> { return new Response(\"ok\"); }",
    );
    let api_routes = facts_with_pattern(&api_results, "nextjs.file_route.v1");
    assert!(
        api_routes.is_empty(),
        "Next.js API routes should not emit page-route facts"
    );
}

#[test]
fn nextjs_file_routes_strip_parallel_and_intercepting_segments() {
    let slot_results = extract(
        "app/@modal/login/page.tsx",
        "export default function Page() { return <h1>Login</h1>; }",
    );
    let slot_routes = facts_with_pattern(&slot_results, "nextjs.file_route.v1");
    assert_eq!(slot_routes.len(), 1);
    assert_eq!(metadata_str(slot_routes[0], "route_path"), Some("/login"));
    assert_eq!(
        metadata_array(slot_routes[0], "parallel_route_segments"),
        vec!["modal"]
    );

    let intercepted_results = extract(
        "app/feed/(..)photo/[id]/page.tsx",
        "export default function Page() { return <h1>Photo</h1>; }",
    );
    let intercepted_routes = facts_with_pattern(&intercepted_results, "nextjs.file_route.v1");
    assert_eq!(intercepted_routes.len(), 1);
    assert_eq!(
        metadata_str(intercepted_routes[0], "route_path"),
        Some("/feed/photo/[id]")
    );
    assert_eq!(
        metadata_array(intercepted_routes[0], "intercepting_route_markers"),
        vec!["(..)"]
    );
    assert_eq!(
        metadata_array(intercepted_routes[0], "intercepted_route_segments"),
        vec!["photo"]
    );
    assert_eq!(
        metadata_str(intercepted_routes[0], "normalized_route_template"),
        Some("/feed/photo/:id")
    );
}

#[test]
fn nextjs_pages_routes_require_next_evidence() {
    let spa_results = extract(
        "src/pages/Home.tsx",
        r#"
export function Home() {
  return <h1>Client app</h1>;
}
"#,
    );
    assert!(
        facts_with_pattern(&spa_results, "nextjs.file_route.v1").is_empty(),
        "React SPA source folders named pages must not imply Next.js Pages Router"
    );

    let next_results = extract(
        "src/pages/Home.tsx",
        r#"
export async function getServerSideProps() {
  return { props: {} };
}

export default function Home() {
  return <h1>Next page</h1>;
}
"#,
    );
    let next_routes = facts_with_pattern(&next_results, "nextjs.file_route.v1");
    assert_eq!(next_routes.len(), 1);
    assert_eq!(metadata_str(next_routes[0], "router"), Some("pages"));
    assert_eq!(metadata_str(next_routes[0], "route_path"), Some("/Home"));
}

#[test]
fn react_router_route_scanners_ignore_non_code_and_import_asi() {
    let source = r#"
import { Link } from "react-router-dom"
import { Route } from "react-router-dom"
const x = 1;
const message = "<Link to='/from-string'>String only</Link>";

export default function App() {
  return (
    <>
      <Link title="Go to settings" to="/settings">Settings</Link>
      <Link title="a \"to\" b" to="/escaped">Escaped</Link>
      <Link to="https://example.com/settings">External</Link>
      <Route path="/profile" element={<Profile />} />
    </>
  );
}
"#;

    let results = extract("src/App.tsx", source);

    let references = facts_with_pattern(&results, "react.route_reference.v1");
    assert_eq!(
        references
            .iter()
            .filter_map(|fact| metadata_str(fact, "target_path"))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["/escaped", "/settings"])
    );

    let definitions = facts_with_pattern(&results, "react.route_definition.v1");
    assert_eq!(definitions.len(), 1);
    assert_eq!(metadata_str(definitions[0], "route_path"), Some("/profile"));
}

#[test]
fn react_router_type_only_route_import_does_not_gate_jsx_routes() {
    let source = r#"
import { type Route } from "react-router-dom";

export function App() {
  return <Route path="/type-only" element={<TypeOnly />} />;
}
"#;

    let results = extract("src/App.tsx", source);
    assert!(
        facts_with_pattern(&results, "react.route_definition.v1").is_empty(),
        "type-only Route imports must not count as runtime JSX route evidence"
    );
}

#[test]
fn file_routes_keep_framework_and_segment_semantics_precise() {
    let optional_slug_results = extract(
        "app/docs/[[...slug]]/page.tsx",
        "export default function Page() { return <h1>Docs</h1>; }",
    );
    let optional_slug_routes = facts_with_pattern(&optional_slug_results, "nextjs.file_route.v1");
    assert_eq!(optional_slug_routes.len(), 1);
    assert_eq!(
        metadata_str(optional_slug_routes[0], "route_path"),
        Some("/docs/[[...slug]]")
    );
    assert_eq!(
        metadata_str(optional_slug_routes[0], "normalized_route_template"),
        Some("/docs/:slug*?")
    );

    let nested_app_pages_results = extract(
        "packages/app/pages/dashboard.tsx",
        "export default function Dashboard() { return <h1>Dashboard</h1>; }",
    );
    assert!(facts_with_pattern(&nested_app_pages_results, "nextjs.file_route.v1").is_empty());
    let nested_app_pages_routes =
        facts_with_pattern(&nested_app_pages_results, "nuxt.file_route.v1");
    assert_eq!(nested_app_pages_routes.len(), 1);
    assert_eq!(
        metadata_str(nested_app_pages_routes[0], "framework"),
        Some("nuxt")
    );
    assert_eq!(
        metadata_str(nested_app_pages_routes[0], "route_path"),
        Some("/dashboard")
    );

    let nested_app_pages_page_results = extract(
        "app/pages/about/page.tsx",
        "export default function About() { return <h1>About</h1>; }",
    );
    let next_app_pages_page_routes =
        facts_with_pattern(&nested_app_pages_page_results, "nextjs.file_route.v1");
    assert_eq!(next_app_pages_page_routes.len(), 1);
    assert_eq!(
        metadata_str(next_app_pages_page_routes[0], "router"),
        Some("app")
    );
    assert_eq!(
        metadata_str(next_app_pages_page_routes[0], "route_path"),
        Some("/pages/about")
    );
    assert!(
        facts_with_pattern(&nested_app_pages_page_results, "nuxt.file_route.v1").is_empty(),
        "app/pages route segments with page files must not emit competing Nuxt file routes"
    );

    let pages_app_page_results = extract(
        "pages/app/page.tsx",
        r#"
export async function getStaticProps() {
  return { props: {} };
}

export default function Page() { return <h1>Pages app page</h1>; }
"#,
    );
    let pages_app_page_routes = facts_with_pattern(&pages_app_page_results, "nextjs.file_route.v1");
    assert_eq!(pages_app_page_routes.len(), 1);
    assert_eq!(
        metadata_str(pages_app_page_routes[0], "router"),
        Some("pages")
    );
    assert_eq!(
        metadata_str(pages_app_page_routes[0], "route_path"),
        Some("/app/page")
    );
}

#[test]
fn nextjs_file_routes_ignore_nuxt_signals_in_comments_and_strings() {
    let results = extract(
        "pages/settings.tsx",
        r#"
// definePageMeta would be a Nuxt signal in executable code.
const note = "useNuxtApp";
export async function getStaticProps() {
  return { props: {} };
}
export default function Settings() { return <h1>Settings</h1>; }
"#,
    );

    let next_routes = facts_with_pattern(&results, "nextjs.file_route.v1");
    assert_eq!(next_routes.len(), 1);
    assert_eq!(
        metadata_str(next_routes[0], "route_path"),
        Some("/settings")
    );
    assert!(
        facts_with_pattern(&results, "nuxt.file_route.v1").is_empty(),
        "Nuxt signal text in comments or strings must not suppress Next.js file routes"
    );
}

#[test]
fn nextjs_route_handlers_emit_one_fact_per_exported_verb() {
    let results = extract(
        "app/api/users/[id]/route.ts",
        r#"
import { NextResponse } from "next/server";

export async function GET(request: Request, { params }: { params: { id: string } }) {
  return NextResponse.json({ id: params.id });
}

export const DELETE = async (request: Request) => {
  return new Response(null, { status: 204 });
};

// Helpers and non-verb exports stay silent.
export function helper() {
  return "not a handler";
}

export const get = () => new Response("lowercase is not a verb export");
"#,
    );

    let handlers = facts_with_pattern(&results, "nextjs.route_handler.v1");
    assert_eq!(
        handlers.len(),
        2,
        "only exported HTTP-verb handlers emit route_handler facts"
    );

    let verbs = handlers
        .iter()
        .filter_map(|fact| metadata_str(fact, "verb"))
        .collect::<BTreeSet<_>>();
    assert_eq!(verbs, BTreeSet::from(["DELETE", "GET"]));

    for fact in &handlers {
        assert_eq!(metadata_str(fact, "query_family"), Some("framework"));
        assert_eq!(metadata_str(fact, "framework"), Some("nextjs"));
        assert_eq!(metadata_str(fact, "router"), Some("app"));
        assert_eq!(metadata_str(fact, "file_convention"), Some("route"));
        assert_eq!(metadata_str(fact, "route_path"), Some("/api/users/[id]"));
        assert_eq!(
            metadata_str(fact, "normalized_route_template"),
            Some("/api/users/:id")
        );
        assert_eq!(metadata_array(fact, "dynamic_segments"), vec!["id"]);
        assert_eq!(metadata_str(fact, "verb_source"), Some("attested"));
        assert_eq!(
            metadata_str(fact, "source_kind"),
            Some("nextjs_route_handler")
        );
        assert_eq!(fact.capture_name, "route_handler");
    }
}

#[test]
fn nextjs_route_handlers_gate_on_app_router_route_files() {
    // page.ts files never emit route_handler facts, even with verb-named exports.
    let page_results = extract(
        "app/dashboard/page.ts",
        r#"
export function GET() { return new Response("not a route handler"); }
export default function Page() { return null; }
"#,
    );
    assert!(
        facts_with_pattern(&page_results, "nextjs.route_handler.v1").is_empty(),
        "page files must not emit route_handler facts"
    );

    // route files outside app/ (e.g. pages/api) emit nothing.
    let non_app_results = extract(
        "lib/route.ts",
        r#"export async function GET() { return new Response("ok"); }"#,
    );
    assert!(
        facts_with_pattern(&non_app_results, "nextjs.route_handler.v1").is_empty(),
        "route files outside app/ must not emit route_handler facts"
    );

    // jsx/tsx route files are nonstandard and stay silent.
    let tsx_results = extract(
        "app/api/things/route.tsx",
        r#"export async function GET() { return new Response("ok"); }"#,
    );
    assert!(
        facts_with_pattern(&tsx_results, "nextjs.route_handler.v1").is_empty(),
        "non-.js/.ts route files must not emit route_handler facts"
    );
}

#[test]
fn nextjs_route_handlers_share_app_router_segment_semantics() {
    let results = extract(
        "src/app/(marketing)/api/leads/route.js",
        r#"export async function POST(request) { return new Response(null, { status: 201 }); }"#,
    );

    let handlers = facts_with_pattern(&results, "nextjs.route_handler.v1");
    assert_eq!(handlers.len(), 1);
    assert_eq!(metadata_str(handlers[0], "verb"), Some("POST"));
    assert_eq!(metadata_str(handlers[0], "route_path"), Some("/api/leads"));
    assert_eq!(
        metadata_array(handlers[0], "route_group_segments"),
        vec!["marketing"]
    );
    // Route groups do not appear in the route path, matching the page fact.
    assert!(
        metadata_str(handlers[0], "normalized_route_template").is_none(),
        "static route handlers omit normalized_route_template"
    );
}
