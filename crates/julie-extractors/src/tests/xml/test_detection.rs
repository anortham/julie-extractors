use super::support::{extract, find};
use crate::base::Symbol;
use serde_json::Value;

fn role(symbol: &Symbol, key: &str) -> bool {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(Value::as_bool)
        == Some(true)
}

fn has_test_role(symbol: &Symbol) -> bool {
    ["test_container", "is_test"]
        .into_iter()
        .any(|key| role(symbol, key))
}

#[test]
fn ant_junit_roles_follow_project_target_junit_structure() {
    let symbols = extract(
        r#"<project name="build">
  <target name="test">
    <junit>
      <test name="unit.Test"/>
      <wrapper><test name="nested.Test"/></wrapper>
      <testcase name="report.Test"/>
    </junit>
    <test name="outside.Test"/>
  </target>
  <target id="id-only">
    <junit><test name="id.Target"/></junit>
  </target>
</project>
"#,
    );

    assert!(role(find(&symbols, "test"), "test_container"));
    assert!(role(find(&symbols, "id-only"), "test_container"));
    assert!(role(find(&symbols, "unit.Test"), "is_test"));
    assert!(!has_test_role(find(&symbols, "nested.Test")));
    assert!(role(find(&symbols, "id.Target"), "is_test"));
    assert!(!has_test_role(find(&symbols, "report.Test")));
    assert!(!has_test_role(find(&symbols, "outside.Test")));
}

#[test]
fn junit_report_and_lookalike_tags_do_not_receive_ant_roles() {
    for source in [
        r#"<testsuite name="suite"><testcase name="reported"/><test name="lookalike"/></testsuite>
"#,
        r#"<build name="not-project"><target name="target"><junit><test name="lookalike"/></junit></target></build>
"#,
        r#"<project name="build">
  <wrapper><target name="nested-target"><junit><test name="nested-lookalike"/></junit></target></wrapper>
  <target name="no-junit"><test name="missing-junit"/></target>
  <junit><test name="missing-target"/></junit>
</project>
"#,
    ] {
        let symbols = extract(source);
        assert!(!symbols.iter().any(has_test_role));
    }
}
