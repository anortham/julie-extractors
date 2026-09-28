use std::path::Path;
use std::process::{Command, Output};

use rusqlite::Connection;
use serde_json::Value;
use tempfile::TempDir;

const SOURCE: &str = "from flask import Flask\n\
from flask_login import login_required\n\n\
app = Flask(__name__)\n\n\
@app.route(\"/private\")\n\
@login_required\n\
def private():\n    return \"private\"\n\n\
def legacy():\n    return \"legacy\"\n\n\
app.add_url_rule(\"/legacy\", view_func=legacy, methods=[\"POST\"])\n";

#[derive(Debug)]
struct StructuralFactRow {
    node_kind: String,
    owner_name: Option<String>,
    start_line: i64,
    end_line: i64,
    start_byte: i64,
    end_byte: i64,
    metadata: Value,
}

fn path_str(path: &Path) -> &str {
    path.to_str().expect("fixture paths are UTF-8")
}

fn structural_facts_for_route(db: &Path, path: &str, route: &str) -> Vec<StructuralFactRow> {
    let connection = Connection::open(db).expect("open artifact");
    let mut statement = connection
        .prepare(
            "SELECT f.node_kind, s.name, f.start_line, f.end_line,
                    f.start_byte, f.end_byte, f.metadata_json
             FROM structural_facts f
             LEFT JOIN symbols s ON s.symbol_id = f.containing_symbol_id
             WHERE f.path = ?1
               AND f.pattern_id = 'flask.route.v1'
               AND json_extract(f.metadata_json, '$.route_template') = ?2",
        )
        .expect("prepare route fact query");
    statement
        .query_map([path, route], |row| {
            let metadata_json: String = row.get(6)?;
            Ok(StructuralFactRow {
                node_kind: row.get(0)?,
                owner_name: row.get(1)?,
                start_line: row.get(2)?,
                end_line: row.get(3)?,
                start_byte: row.get(4)?,
                end_byte: row.get(5)?,
                metadata: serde_json::from_str(&metadata_json).expect("valid fact metadata"),
            })
        })
        .expect("query route facts")
        .collect::<Result<Vec<_>, _>>()
        .expect("read route facts")
}

fn scan(root: &Path, db: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "scan",
            "--root",
            path_str(root),
            "--db",
            path_str(db),
            "--json",
        ])
        .output()
        .expect("run julie-extract scan")
}

#[test]
fn cli_persists_flask_decorator_span_owner_and_call_route_span() {
    let workspace = TempDir::new().expect("create fixture directory");
    let root = workspace.path().join("repo");
    std::fs::create_dir_all(&root).expect("create source root");
    std::fs::write(root.join("app.py"), SOURCE).expect("write Python fixture");
    let db = workspace.path().join("artifact.sqlite");

    let output = scan(&root, &db);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("JSON scan report");
    assert_eq!(report["status"], "ok");
    assert_eq!(report["operation"], "scan");

    let decorator = "@app.route(\"/private\")";
    let decorator_start = SOURCE.find(decorator).expect("route decorator in source");
    let decorator_end = decorator_start + decorator.len();
    let route_facts = structural_facts_for_route(&db, "app.py", "/private");
    assert_eq!(route_facts.len(), 1, "{route_facts:#?}");
    let route = &route_facts[0];
    assert_eq!(route.start_byte, decorator_start as i64);
    assert_eq!(route.end_byte, decorator_end as i64);
    assert_eq!(
        &SOURCE[route.start_byte as usize..route.end_byte as usize],
        decorator
    );
    assert_eq!(route.start_line, 6);
    assert_eq!(route.end_line, 6);
    assert_eq!(route.node_kind, "decorator");
    assert_eq!(route.owner_name.as_deref(), Some("private"));
    assert_eq!(route.metadata["api_style"], "decorator_routing");

    let call = "app.add_url_rule(\"/legacy\", view_func=legacy, methods=[\"POST\"])";
    let call_start = SOURCE.find(call).expect("add_url_rule call in source");
    let call_end = call_start + call.len();
    let call_facts = structural_facts_for_route(&db, "app.py", "/legacy");
    assert_eq!(call_facts.len(), 1, "{call_facts:#?}");
    let call_fact = &call_facts[0];
    assert_eq!(call_fact.start_byte, call_start as i64);
    assert_eq!(call_fact.end_byte, call_end as i64);
    assert_eq!(
        &SOURCE[call_fact.start_byte as usize..call_fact.end_byte as usize],
        call
    );
    assert_eq!(call_fact.node_kind, "call");
    assert_eq!(call_fact.metadata["api_style"], "call_routing");
}
