use std::collections::BTreeSet;
use std::path::Path;

use crate::base::StructuralFact;

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical React extraction should succeed")
}

fn facts_with_pattern<'a>(
    results: &'a crate::ExtractionResults,
    pattern_id: &str,
) -> Vec<&'a StructuralFact> {
    results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == pattern_id)
        .collect()
}

fn metadata_str<'a>(fact: &'a StructuralFact, key: &str) -> Option<&'a str> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_str())
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
        "export default function Dashboard() { return <h1>Dashboard</h1>; }",
    );
    let pages_routes = facts_with_pattern(&pages_results, "nextjs.file_route.v1");
    assert_eq!(pages_routes.len(), 1);
    assert_eq!(metadata_str(pages_routes[0], "router"), Some("pages"));
    assert_eq!(
        metadata_str(pages_routes[0], "route_path"),
        Some("/dashboard")
    );
}
