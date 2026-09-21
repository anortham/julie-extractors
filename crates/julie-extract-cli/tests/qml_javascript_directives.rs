use std::io::Write;
use std::process::{Command, Output, Stdio};

use serde_json::{Value, json};

fn check_javascript(source: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args(["check", "--path", "Commons/BorderGeometry.js", "--json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to run julie-extract check");
    child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(source.as_bytes())
        .expect("failed to write source to julie-extract");
    child
        .wait_with_output()
        .expect("julie-extract check output")
}

fn report(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|err| {
        panic!(
            "stdout was not a JSON report: {err}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

#[test]
fn check_accepts_a_pragma_library_javascript_file() {
    let output =
        check_javascript(".pragma library\n\nfunction clamp(value) {\n  return value\n}\n");

    assert_eq!(output.status.code(), Some(0));
    let report = report(&output);
    assert_eq!(report["status"], "ok");
    assert_eq!(report["languages"]["language"], "javascript");
    assert_eq!(report["errors"], json!([]));
}

#[test]
fn check_accepts_qml_import_directives_in_a_javascript_file() {
    let output = check_javascript(
        "// header\n.import QtQuick 2.0 as QQ\n.import \"Geometry.js\" as Geometry\n\nvar origin = QQ.point\n",
    );

    assert_eq!(output.status.code(), Some(0));
    let report = report(&output);
    assert_eq!(report["status"], "ok");
    assert_eq!(report["errors"], json!([]));
}
