use crate::base::{
    ExtractionResults, IdentifierKind, RelationshipKind, SourceRegionKind, StructuralFact, Symbol,
    SymbolKind, Visibility,
};
use crate::extract_canonical;
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("bash extraction")
}

fn named<'a>(result: &'a ExtractionResults, name: &str) -> Vec<&'a Symbol> {
    result.symbols.iter().filter(|s| s.name == name).collect()
}

fn one<'a>(result: &'a ExtractionResults, name: &str) -> &'a Symbol {
    let found = named(result, name);
    assert_eq!(found.len(), 1, "{name}: {:#?}", result.symbols);
    found[0]
}

fn meta<'a>(symbol: &'a Symbol, key: &str) -> Option<&'a str> {
    symbol.metadata.as_ref()?.get(key)?.as_str()
}

fn test_role(symbol: &Symbol) -> Option<&str> {
    meta(symbol, "test_role")
}

fn calls(result: &ExtractionResults, from: &Symbol, to: &Symbol) -> bool {
    result.relationships.iter().any(|r| {
        r.kind == RelationshipKind::Calls && r.from_symbol_id == from.id && r.to_symbol_id == to.id
    })
}

fn pending_targets(result: &ExtractionResults, from: &Symbol) -> Vec<String> {
    result
        .structured_pending_relationships
        .iter()
        .filter(|p| p.pending.from_symbol_id == from.id)
        .map(|p| p.target.display_name.clone())
        .collect()
}

fn call_names(result: &ExtractionResults) -> Vec<&str> {
    result
        .identifiers
        .iter()
        .filter(|i| i.kind == IdentifierKind::Call)
        .map(|i| i.name.as_str())
        .collect()
}

fn facts<'a>(result: &'a ExtractionResults, pattern: &str) -> Vec<&'a StructuralFact> {
    result
        .structural_facts
        .iter()
        .filter(|f| f.pattern_id == pattern)
        .collect()
}

fn fact_str<'a>(fact: &'a StructuralFact, key: &str) -> Option<&'a str> {
    fact.metadata.as_ref()?.get(key)?.as_str()
}

fn body_text<'a>(source: &'a str, symbol: &Symbol) -> Option<&'a str> {
    let span = symbol.body_span?;
    source.get(span.start_byte as usize..span.end_byte as usize)
}

#[test]
fn wrapped_commands_and_trap_handlers_call_their_target() {
    let source = r#"cleanup() { rm -rf "$TMP"; }
render() { echo hi; }
notify() { printf '%s\n' "$1"; }
_comp() { :; }
worker() {
    trap cleanup EXIT
    trap 'notify "interrupted"; exit 130' INT
    timeout 30 render
    nohup render &
    exec render
    sudo -E env FOO=1 render "a"
    xargs -n 1 render
    command notify done
    command -v lookup_only
    complete -F _comp app
}
"#;
    let result = extract("misc.sh", source);
    let worker = one(&result, "worker");
    for target in ["cleanup", "render", "notify", "_comp"] {
        assert!(
            calls(&result, worker, one(&result, target)),
            "worker -> {target}: {:#?}",
            result.relationships
        );
    }
    let names = call_names(&result);
    assert!(names.contains(&"cleanup"), "{names:?}");
    assert!(names.contains(&"_comp"), "{names:?}");
    assert!(!names.contains(&"lookup_only"), "{names:?}");
    let notify_in_string = result
        .identifiers
        .iter()
        .find(|i| i.name == "notify" && i.start_line == 7)
        .expect("trap string handler call");
    assert_eq!(notify_in_string.start_column, 10);
    assert_eq!(notify_in_string.end_column, 16);
    let targets = pending_targets(&result, worker);
    for wrapper in [
        "trap", "timeout", "nohup", "sudo", "env", "xargs", "complete",
    ] {
        assert!(!targets.contains(&wrapper.to_string()), "{targets:?}");
    }
}

#[test]
fn wrapped_commands_expose_their_literal_carrier() {
    let source = "deploy() {\n    sudo curl \"https://example.com/hook\"\n}\n";
    let result = extract("hook.sh", source);
    let literal = result
        .literals
        .iter()
        .find(|l| l.literal_text == "https://example.com/hook")
        .expect("wrapped curl literal");
    assert_eq!(literal.carrier.as_deref(), Some("curl"));
}

#[test]
fn locally_defined_wrapper_names_stay_plain_calls() {
    let source = "run() { \"$@\"; }\ndeploy() {\n    run build_all\n}\n";
    let result = extract("local_run.sh", source);
    let deploy = one(&result, "deploy");
    assert!(calls(&result, deploy, one(&result, "run")));
    assert!(!pending_targets(&result, deploy).contains(&"build_all".to_string()));
}

#[test]
fn builtins_and_dynamic_command_names_emit_no_pending_calls() {
    let source = r#"parse_args() {
    while getopts ":hv:" opt; do :; done
    shift $((OPTIND - 1))
    let "total = 1"
    trap cleanup EXIT
    ulimit -c 0
    wait
    time make
    "$SCRIPT_DIR/bin/tool" --flag
    "${runner[@]}" -c true
    $cmd arg
    "./bin/tool" --flag
}
"#;
    let result = extract("flow.sh", source);
    let targets = pending_targets(&result, one(&result, "parse_args"));
    for builtin in ["getopts", "shift", "let", "trap", "ulimit", "wait", "time"] {
        assert!(!targets.contains(&builtin.to_string()), "{targets:?}");
    }
    assert!(targets.contains(&"make".to_string()), "{targets:?}");
    assert!(targets.contains(&"./bin/tool".to_string()), "{targets:?}");
    assert!(
        targets.iter().all(|t| !t.contains('$') && !t.contains('"')),
        "{targets:?}"
    );
    let names = call_names(&result);
    assert!(
        names.iter().all(|n| !n.contains('$') && !n.contains('"')),
        "{names:?}"
    );
}

#[test]
fn command_prefix_assignments_and_element_writes_emit_no_symbols() {
    let source = r#"build() {
    while IFS= read -r line; do echo "$line"; done < files.txt
    GOOS=linux GOARCH=amd64 go build ./...
    declare -A seen
    seen[${items[idx]}]=1
}
HANDLERS[start]=do_start
DEBUG=1 run_tests
"#;
    let result = extract("prefix.sh", source);
    for name in ["IFS", "GOOS", "GOARCH", "DEBUG"] {
        assert!(named(&result, name).is_empty(), "{name}");
    }
    assert!(
        result.symbols.iter().all(|s| !s.name.contains('[')),
        "{:#?}",
        result.symbols
    );
    let build = one(&result, "build");
    let read = result
        .identifiers
        .iter()
        .find(|i| i.name == "read")
        .expect("read call");
    assert_eq!(
        read.containing_symbol_id.as_deref(),
        Some(build.id.as_str())
    );
    let run_tests = result
        .identifiers
        .iter()
        .find(|i| i.name == "run_tests")
        .expect("run_tests call");
    assert_eq!(run_tests.containing_symbol_id, None);
    assert!(
        result
            .identifiers
            .iter()
            .any(|i| i.name == "seen" && i.kind == IdentifierKind::MemberAccess)
    );
}

#[test]
fn bare_declarations_emit_one_symbol_per_name() {
    let source = r#"declare -A HANDLERS
collect() {
    local result
    local -a parts
    declare -g LAST_RUN
    readonly SENTINEL
    local a b=2 c
}
"#;
    let result = extract("decl.sh", source);
    let collect = one(&result, "collect");
    for name in ["result", "parts", "a", "b", "c"] {
        let symbol = one(&result, name);
        assert_eq!(
            symbol.parent_id.as_deref(),
            Some(collect.id.as_str()),
            "{name}"
        );
    }
    assert_eq!(one(&result, "LAST_RUN").parent_id, None);
    let handlers = one(&result, "HANDLERS");
    assert_eq!(handlers.kind, SymbolKind::Variable);
    assert_eq!(
        result
            .types
            .get(&handlers.id)
            .map(|t| t.resolved_type.as_str()),
        Some("array")
    );
    assert_eq!(one(&result, "SENTINEL").kind, SymbolKind::Constant);
    assert_eq!(one(&result, "result").body_span, None);
}

#[test]
fn declarations_emit_no_self_parented_duplicates() {
    let source = "readonly MAX=3\nexport API_URL=x\nf() {\n    local count=$1\n}\n";
    let result = extract("dup.sh", source);
    for name in ["MAX", "API_URL", "count"] {
        let symbol = one(&result, name);
        assert_ne!(symbol.parent_id.as_deref(), Some(symbol.id.as_str()));
    }
    assert_eq!(one(&result, "MAX").kind, SymbolKind::Constant);
    let api_url = one(&result, "API_URL");
    assert_eq!(api_url.kind, SymbolKind::Variable);
    assert_eq!(api_url.visibility, Some(Visibility::Public));
    assert_eq!(
        one(&result, "count").parent_id.as_deref(),
        Some(one(&result, "f").id.as_str())
    );
}

#[test]
fn declaration_flags_set_kind_visibility_and_export_facts() {
    let source = r#"f() {
    local -r FROZEN=1
    local -x EXPORTED_LOCAL=1
    declare -rx ROX=1
}
declare -gx EXPORTED_G=1
export PATH="$HOME/bin:$PATH" EDITOR=vim
ROOT=/srv/app
export ROOT
export -n UNEXPORTED
"#;
    let result = extract("flags.sh", source);
    assert_eq!(one(&result, "FROZEN").kind, SymbolKind::Constant);
    assert_eq!(one(&result, "FROZEN").visibility, Some(Visibility::Private));
    assert_eq!(one(&result, "ROX").kind, SymbolKind::Constant);
    for name in ["ROX", "EXPORTED_LOCAL", "EXPORTED_G", "EDITOR", "ROOT"] {
        assert_eq!(
            one(&result, name).visibility,
            Some(Visibility::Public),
            "{name}"
        );
    }
    let exported: Vec<Vec<String>> = facts(&result, "bash.export_declaration.v1")
        .iter()
        .map(|fact| {
            fact.metadata.as_ref().unwrap()["variable_names"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_string())
                .collect()
        })
        .collect();
    let all: Vec<&str> = exported.iter().flatten().map(String::as_str).collect();
    for name in [
        "EXPORTED_LOCAL",
        "ROX",
        "EXPORTED_G",
        "PATH",
        "EDITOR",
        "ROOT",
    ] {
        assert!(all.contains(&name), "{name}: {exported:?}");
    }
    assert!(!all.contains(&"FROZEN"), "{exported:?}");
    assert!(!all.contains(&"UNEXPORTED"), "{exported:?}");
    let path_fact = facts(&result, "bash.export_declaration.v1")
        .into_iter()
        .find(|f| fact_str(f, "variable_name") == Some("PATH"))
        .expect("PATH export fact");
    assert_eq!(
        path_fact.metadata.as_ref().unwrap()["variable_names"],
        serde_json::json!(["PATH", "EDITOR"])
    );
}

#[test]
fn positional_parameters_cover_expansions_and_belong_to_their_function() {
    let source = r#"greet() {
    local name="${1:-world}"
    local greeting="${2}"
}
usage() { echo "$(basename "$0")"; }
outer() {
    helper() { echo "$1 $2"; }
    helper a b
}
"#;
    let result = extract("params.sh", source);
    let params_of = |owner: &str| -> Vec<String> {
        let owner = one(&result, owner);
        let mut names: Vec<String> = result
            .symbols
            .iter()
            .filter(|s| s.parent_id.as_deref() == Some(owner.id.as_str()))
            .filter(|s| meta(s, "role") == Some("parameter"))
            .map(|s| s.name.clone())
            .collect();
        names.sort();
        names
    };
    assert_eq!(params_of("greet"), vec!["$1", "$2"]);
    assert!(params_of("usage").is_empty());
    assert_eq!(params_of("helper"), vec!["$1", "$2"]);
    assert!(params_of("outer").is_empty());
}

#[test]
fn body_spans_come_from_the_grammar() {
    let source = r#"greet() {
    local name="${1:-world}"
}
source "${BASH_SOURCE%/*}/lib/strings.sh"
total=$((total + 1))
"#;
    let result = extract("spans.sh", source);
    assert_eq!(
        body_text(source, one(&result, "name")),
        Some("\"${1:-world}\"")
    );
    assert_eq!(body_text(source, one(&result, "strings")), None);
    assert_eq!(
        body_text(source, one(&result, "total")),
        Some("$((total + 1))")
    );
    assert_eq!(
        body_text(source, one(&result, "greet")),
        Some("{\n    local name=\"${1:-world}\"\n}")
    );
}

#[test]
fn doc_comments_skip_the_shebang_and_detached_blocks() {
    let source = r#"#!/usr/bin/env bash
# Copyright 2024 Example Corp.

helper() {
    # TODO: remove this hack
    echo one
}

# ---- Section: networking ----

fetch() {
    curl "https://x"
}

# Parse a key=value config file.
parse_config() { :; }
APP=1 # trailing note
NEXT=2
"#;
    let result = extract("docs.sh", source);
    assert_eq!(one(&result, "helper").doc_comment, None);
    assert_eq!(one(&result, "fetch").doc_comment, None);
    assert_eq!(one(&result, "NEXT").doc_comment, None);
    assert_eq!(
        one(&result, "parse_config").doc_comment.as_deref(),
        Some("# Parse a key=value config file.")
    );
    let helper = one(&result, "helper");
    let region_at = |line: u32| {
        result
            .source_regions
            .iter()
            .find(|r| r.start_line == line)
            .unwrap_or_else(|| panic!("region at {line}"))
    };
    assert_eq!(region_at(1).kind, SourceRegionKind::Comment);
    let todo = region_at(5);
    assert_eq!(todo.kind, SourceRegionKind::Comment);
    assert_eq!(
        todo.containing_symbol_id.as_deref(),
        Some(helper.id.as_str())
    );
    let doc = region_at(15);
    assert_eq!(doc.kind, SourceRegionKind::DocComment);
    assert_eq!(
        doc.containing_symbol_id.as_deref(),
        Some(one(&result, "parse_config").id.as_str())
    );
}

#[test]
fn shunit2_bashunit_and_bats_conventions_are_classified() {
    let shunit = r#"oneTimeSetUp() { . ./lib/calc.sh; }
setUp() { RESULT=""; }
tearDown() { :; }
oneTimeTearDown() { :; }
testAddsNumbers() { assertEquals 3 "$(add 1 2)"; }
test_subtracts_numbers() { :; }
helper() { :; }
"#;
    let result = extract("calc_test.sh", shunit);
    assert_eq!(
        test_role(one(&result, "testAddsNumbers")),
        Some("test_case")
    );
    assert_eq!(
        test_role(one(&result, "test_subtracts_numbers")),
        Some("test_case")
    );
    assert_eq!(
        test_role(one(&result, "oneTimeSetUp")),
        Some("fixture_setup")
    );
    assert_eq!(test_role(one(&result, "setUp")), Some("fixture_setup"));
    assert_eq!(
        test_role(one(&result, "tearDown")),
        Some("fixture_teardown")
    );
    assert_eq!(
        test_role(one(&result, "oneTimeTearDown")),
        Some("fixture_teardown")
    );
    assert_eq!(test_role(one(&result, "helper")), None);

    let bats = "setup_file() { :; }\nteardown_file() { :; }\nsetup_suite() { :; }\n";
    let result = extract("deploy.bats", bats);
    assert_eq!(test_role(one(&result, "setup_file")), Some("fixture_setup"));
    assert_eq!(
        test_role(one(&result, "teardown_file")),
        Some("fixture_teardown")
    );
    assert_eq!(
        test_role(one(&result, "setup_suite")),
        Some("fixture_setup")
    );

    let bashunit = "set_up() { :; }\ntear_down_after_script() { :; }\n";
    let result = extract("tests/unit/calc_test.sh", bashunit);
    assert_eq!(test_role(one(&result, "set_up")), Some("fixture_setup"));
    assert_eq!(
        test_role(one(&result, "tear_down_after_script")),
        Some("fixture_teardown")
    );

    let production = "testConnection() { :; }\nsetUp() { :; }\n";
    let result = extract("lib/net.sh", production);
    assert_eq!(test_role(one(&result, "testConnection")), None);
    assert_eq!(test_role(one(&result, "setUp")), None);
}

#[test]
fn shellspec_focus_skip_variants_and_hooks_are_classified() {
    let source = r#"setup_math() { :; }
Describe 'math'
  BeforeEach 'setup_math'
  AfterAll cleanup_math
  xIt 'skipped case'
    When call add 1 2
  End
  fDescribe 'focused'
    fIt 'focused case'
    End
  End
  ExampleGroup 'group'
  End
End
"#;
    let result = extract("spec/math_spec.sh", source);
    assert_eq!(test_role(one(&result, "skipped case")), Some("test_case"));
    assert_eq!(test_role(one(&result, "focused case")), Some("test_case"));
    assert_eq!(test_role(one(&result, "focused")), Some("test_container"));
    assert_eq!(test_role(one(&result, "group")), Some("test_container"));
    let before = one(&result, "BeforeEach");
    assert_eq!(test_role(before), Some("fixture_setup"));
    assert_eq!(
        test_role(one(&result, "AfterAll")),
        Some("fixture_teardown")
    );
    assert!(calls(&result, before, one(&result, "setup_math")));
    let setup_ref = result
        .identifiers
        .iter()
        .find(|i| i.name == "setup_math")
        .expect("hook target call");
    assert_eq!(
        setup_ref.containing_symbol_id.as_deref(),
        Some(before.id.as_str())
    );
}

#[test]
fn bats_and_shellspec_loaders_are_imports() {
    let bats = r#"load 'test_helper/bats-support/load'
bats_load_library bats-file
@test "works" {
    load helpers
    run true
}
"#;
    let result = extract("test/app.bats", bats);
    let imports: Vec<(&str, Option<&str>)> = result
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Import)
        .map(|s| (s.name.as_str(), meta(s, "sourceCommand")))
        .collect();
    assert!(imports.contains(&("load", Some("load"))), "{imports:?}");
    assert!(
        imports.contains(&("bats-file", Some("bats_load_library"))),
        "{imports:?}"
    );
    assert!(imports.contains(&("helpers", Some("load"))), "{imports:?}");
    let test = one(&result, "works");
    assert!(
        result.structured_pending_relationships.iter().any(|p| {
            p.pending.kind == RelationshipKind::Imports
                && p.pending.from_symbol_id == test.id
                && p.target.terminal_name == "helpers"
        }),
        "{:#?}",
        result.structured_pending_relationships
    );

    let spec = "Include lib/calc.sh\nDescribe 'calculator'\nEnd\n";
    let result = extract("spec/calc_spec.sh", spec);
    let calc = one(&result, "calc");
    assert_eq!(calc.kind, SymbolKind::Import);
    assert_eq!(meta(calc, "sourceCommand"), Some("Include"));
    assert_eq!(meta(calc, "sourcePath"), Some("lib/calc.sh"));

    let script = "load() { :; }\nload config\n";
    let result = extract("bin/tool.sh", script);
    assert!(result.symbols.iter().all(|s| s.kind != SymbolKind::Import));
}

#[test]
fn curl_and_wget_emit_http_client_request_facts() {
    let source = r#"deploy_hook() {
    curl -X POST "https://api.example.com/v1/deploy" -H 'Content-Type: application/json'
    wget -qO- "https://example.com/health"
    curl -fsSL https://get.example.com/install.sh
    curl --data '{"a":1}' 'https://api.example.com/v1/items'
    curl "https://api.example.com/$VERSION/x"
}
"#;
    let result = extract("http.sh", source);
    let requests: Vec<String> = facts(&result, "http.client_request.v1")
        .iter()
        .map(|f| {
            ["client", "verb", "verb_source", "target_path"]
                .map(|key| fact_str(f, key).unwrap_or("-"))
                .join(" ")
        })
        .collect();
    assert_eq!(
        requests,
        vec![
            "curl POST attested https://api.example.com/v1/deploy",
            "wget GET default https://example.com/health",
            "curl GET default https://get.example.com/install.sh",
            "curl POST attested https://api.example.com/v1/items",
        ]
    );
    let curl_literals: Vec<&str> = result
        .literals
        .iter()
        .filter(|l| l.carrier.as_deref() == Some("curl"))
        .map(|l| l.literal_text.as_str())
        .collect();
    assert!(
        !curl_literals.contains(&"Content-Type: application/json"),
        "{curl_literals:?}"
    );
    assert!(!curl_literals.contains(&"{\"a\":1}"), "{curl_literals:?}");
    assert!(
        curl_literals.contains(&"https://api.example.com/v1/deploy"),
        "{curl_literals:?}"
    );
}

#[test]
fn heredoc_and_herestring_sql_become_literals() {
    let source = r#"migrate() {
    psql "$DATABASE_URL" <<SQL
CREATE TABLE IF NOT EXISTS audit (id serial PRIMARY KEY);
SQL
    mysql -u root <<< "DROP DATABASE staging"
    psql -v ON_ERROR_STOP=1 <<'EOF'
SELECT $1 FROM t;
EOF
}
"#;
    let result = extract("migrate.sh", source);
    let literals: Vec<(&str, Option<&str>)> = result
        .literals
        .iter()
        .map(|l| (l.literal_text.as_str(), l.carrier.as_deref()))
        .collect();
    assert!(
        literals.contains(&(
            "CREATE TABLE IF NOT EXISTS audit (id serial PRIMARY KEY);",
            Some("psql")
        )),
        "{literals:?}"
    );
    assert!(
        literals.contains(&("DROP DATABASE staging", Some("mysql"))),
        "{literals:?}"
    );
    assert!(
        literals.contains(&("SELECT $1 FROM t;", Some("psql"))),
        "{literals:?}"
    );
    assert!(
        literals.iter().all(|(text, _)| *text != "{}"),
        "{literals:?}"
    );
}
