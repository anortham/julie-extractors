use crate::base::{ExtractionResults, IdentifierKind, RelationshipKind, Symbol, SymbolKind};
use crate::{detect_language_for_path, extract_canonical};
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("bash extraction")
}

fn symbol<'a>(result: &'a ExtractionResults, name: &str) -> &'a Symbol {
    result
        .symbols
        .iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("missing {name}: {:#?}", result.symbols))
}

fn pending_targets_from(result: &ExtractionResults, from: &Symbol) -> Vec<String> {
    result
        .structured_pending_relationships
        .iter()
        .filter(|p| {
            p.pending.kind == RelationshipKind::Calls && p.pending.from_symbol_id == from.id
        })
        .map(|p| p.target.display_name.clone())
        .collect()
}

#[test]
fn bats_extensionless_shebang_scripts_and_shell_dotfiles_are_bash() {
    assert_eq!(
        detect_language_for_path(Path::new("test/deploy.bats"), ""),
        Some("bash")
    );
    assert_eq!(
        detect_language_for_path(Path::new("bin/gesso"), "#!/bin/bash\nset -e\n"),
        Some("bash")
    );
    assert_eq!(
        detect_language_for_path(Path::new("bin/tool"), "#!/usr/bin/env sh\n"),
        Some("bash")
    );
    assert_eq!(
        detect_language_for_path(Path::new(".bashrc"), ""),
        Some("bash")
    );
    assert_eq!(
        detect_language_for_path(Path::new("home/.envrc"), ""),
        Some("bash")
    );
    assert_eq!(
        detect_language_for_path(Path::new("README"), "not a script\n"),
        None
    );
    assert_eq!(
        detect_language_for_path(Path::new("bin/tool"), "#!/usr/bin/env python3\n"),
        None
    );
}

#[test]
fn type_facts_are_keyed_by_symbol_id() {
    let source = "readonly MAX_RETRIES=3\ndeclare -a HOSTS=(web1 web2)\nNAME=demo\n";
    let result = extract("types.sh", source);
    let resolved = |name: &str| {
        let symbol = symbol(&result, name);
        result
            .types
            .get(&symbol.id)
            .map(|info| info.resolved_type.clone())
    };
    assert_eq!(resolved("MAX_RETRIES").as_deref(), Some("integer"));
    assert_eq!(resolved("HOSTS").as_deref(), Some("array"));
    assert_eq!(resolved("NAME").as_deref(), Some("string"));
    assert!(
        result
            .types
            .keys()
            .all(|key| result.symbols.iter().any(|s| &s.id == key))
    );
}

#[test]
fn bats_test_blocks_span_their_body_and_own_their_calls() {
    let source = r#"#!/usr/bin/env bats
@test "deploy succeeds" {
    run deploy --config ok.yaml
    assert_output --partial "done"
}
"#;
    let result = extract("test/deploy.bats", source);
    let test = symbol(&result, "deploy succeeds");
    assert_eq!((test.start_line, test.end_line), (2, 5));
    assert!(test.body_hash.is_some());
    let targets = pending_targets_from(&result, test);
    assert!(targets.contains(&"deploy".to_string()), "{targets:?}");
    assert!(
        targets.contains(&"assert_output".to_string()),
        "{targets:?}"
    );
    assert!(!targets.contains(&"@test".to_string()), "{targets:?}");
    assert!(result.identifiers.iter().all(|i| i.name != "}"));
    let run_call = result
        .identifiers
        .iter()
        .find(|i| i.name == "run")
        .expect("run call identifier");
    assert_eq!(
        run_call.containing_symbol_id.as_deref(),
        Some(test.id.as_str())
    );
}

#[test]
fn shellspec_blocks_nest_and_span_to_their_end() {
    let source = r#"Describe 'math'
  Context 'addition'
    It 'adds'
      When call add 1 2
      The output should eq 3
    End
  End
End
"#;
    let result = extract("spec/math_spec.sh", source);
    let describe = symbol(&result, "math");
    let context = symbol(&result, "addition");
    let it = symbol(&result, "adds");
    assert_eq!((describe.start_line, describe.end_line), (1, 8));
    assert_eq!((it.start_line, it.end_line), (3, 6));
    assert_eq!(context.parent_id.as_deref(), Some(describe.id.as_str()));
    assert_eq!(it.parent_id.as_deref(), Some(context.id.as_str()));
    let targets = pending_targets_from(&result, it);
    assert!(targets.contains(&"add".to_string()), "{targets:?}");
    assert!(!targets.contains(&"It".to_string()), "{targets:?}");
    assert!(pending_targets_from(&result, describe).is_empty());
    assert!(result.identifiers.iter().all(|i| i.name != "End"));
}

#[test]
fn arithmetic_reads_emit_variable_refs() {
    let source = r#"count_items() {
    local total=0
    local limit=10
    (( total += limit ))
    total=$(( total * 2 + limit ))
    for (( n = 0; n < limit; n++ )); do echo "${items[n]}"; done
    unset total
}
"#;
    let result = extract("arith.sh", source);
    let refs = |line: u32| {
        let mut names: Vec<String> = result
            .identifiers
            .iter()
            .filter(|i| i.kind == IdentifierKind::VariableRef && i.start_line == line)
            .map(|i| i.name.clone())
            .collect();
        names.sort();
        names
    };
    assert_eq!(refs(4), vec!["limit", "total"]);
    assert_eq!(refs(5), vec!["limit", "total"]);
    assert_eq!(refs(6), vec!["limit", "n", "n", "n"]);
    assert_eq!(refs(7), vec!["total"]);
}

#[test]
fn dynamic_source_paths_import_their_static_tail() {
    let source = r#"source "$(dirname "$0")/base-test.sh"
source "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)/lib.sh"
source "$DIR"/common.sh
source <(kubectl completion bash)
. ./plain.sh
"#;
    let result = extract("sources.sh", source);
    let mut imports: Vec<&str> = result
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Import)
        .map(|s| s.name.as_str())
        .collect();
    imports.sort();
    assert_eq!(imports, vec!["base-test", "common", "lib", "plain"]);
    let base_test = symbol(&result, "base-test");
    assert_eq!(
        base_test
            .metadata
            .as_ref()
            .and_then(|m| m.get("sourcePath"))
            .and_then(|v| v.as_str()),
        Some("$(dirname \"$0\")/base-test.sh")
    );
}

#[test]
fn shebang_symbol_covers_only_the_shebang_line() {
    let source = "#!/bin/bash\nset -euo pipefail\n\ncleanup() {\n  rm -rf \"$tmp\"\n}\n\ncleanup\n";
    let result = extract("trap.sh", source);
    let shebang = symbol(&result, "bash");
    assert_eq!((shebang.start_line, shebang.end_line), (1, 1));
    assert!(shebang.body_hash.is_none());
    let set_call = result
        .identifiers
        .iter()
        .find(|i| i.name == "set")
        .expect("set call");
    assert_eq!(set_call.containing_symbol_id, None);
}
