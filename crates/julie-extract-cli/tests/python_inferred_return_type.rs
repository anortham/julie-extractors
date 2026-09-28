use std::path::Path;
use std::process::Command;

use rusqlite::Connection;
use serde_json::Value;
use tempfile::TempDir;

const SOURCE: &str = r#"
from flask import Flask
from app.factories import make_app

class SameFileClient:
    def app_context(self):
        pass

def create_app(test_config=None):
    app = Flask(__name__, instance_relative_config=True)
    app.config.from_mapping(SECRET_KEY="dev")
    if test_config is None:
        app.config.from_mapping(TESTING=True)
    else:
        app.config.from_mapping(test_config)
    try:
        app.instance_path
    except OSError:
        pass
    return app

def create_client():
    client = SameFileClient()
    return client

def create_client_with_two_returns(flag):
    client = SameFileClient()
    if flag:
        return client
    return client

def annotated() -> SameFileClient:
    client = SameFileClient()
    return client

def factory():
    app = make_app()
    return app

async def async_factory():
    app = SameFileClient()
    return app

def generator():
    client = SameFileClient()
    yield client
    return client

def reassigned():
    app = SameFileClient()
    app = Flask(__name__)
    return app

def except_rebind():
    client = SameFileClient()
    try:
        raise ValueError()
    except ValueError as client:
        return client
    return client

def alternate(flag):
    app = SameFileClient()
    if flag:
        return app
    return Flask(__name__)

def fallthrough(flag):
    app = SameFileClient()
    if flag:
        return app
"#;

fn path_str(path: &Path) -> &str {
    path.to_str().expect("fixture paths are UTF-8")
}

fn scan(root: &Path, db: &Path) {
    let output = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "scan",
            "--root",
            path_str(root),
            "--db",
            path_str(db),
            "--level",
            "symbols",
            "--jobs",
            "1",
            "--json",
        ])
        .output()
        .expect("run julie-extract scan");
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn function_metadata(connection: &Connection, name: &str) -> Value {
    let metadata_json: String = connection
        .query_row(
            "SELECT metadata_json FROM symbols
             WHERE path = 'app.py' AND name = ?1 AND kind = 'function'",
            [name],
            |row| row.get(0),
        )
        .unwrap_or_else(|error| panic!("read metadata for {name}: {error}"));
    serde_json::from_str(&metadata_json).expect("valid symbol metadata")
}

#[test]
fn cli_persists_only_proven_python_inferred_return_types() {
    let workspace = TempDir::new().expect("create fixture directory");
    let root = workspace.path().join("repo");
    std::fs::create_dir_all(&root).expect("create source root");
    std::fs::write(root.join("app.py"), SOURCE).expect("write Python fixture");
    let db = workspace.path().join("artifact.sqlite");

    scan(&root, &db);
    let connection = Connection::open(&db).expect("open artifact");

    let create_app = function_metadata(&connection, "create_app");
    assert_eq!(create_app["returnType"], "");
    assert_eq!(create_app["inferredReturnType"], "Flask");

    let create_client = function_metadata(&connection, "create_client");
    assert_eq!(create_client["returnType"], "");
    assert_eq!(create_client["inferredReturnType"], "SameFileClient");

    let multiple_returns = function_metadata(&connection, "create_client_with_two_returns");
    assert_eq!(multiple_returns["inferredReturnType"], "SameFileClient");

    let annotated = function_metadata(&connection, "annotated");
    assert_eq!(annotated["returnType"], "SameFileClient");
    assert!(annotated.get("inferredReturnType").is_none());

    for name in [
        "factory",
        "async_factory",
        "generator",
        "reassigned",
        "except_rebind",
        "alternate",
        "fallthrough",
    ] {
        let metadata = function_metadata(&connection, name);
        assert_eq!(metadata["returnType"], "");
        assert!(
            metadata.get("inferredReturnType").is_none(),
            "{name} must not get an inferred return type: {metadata}"
        );
    }
}
