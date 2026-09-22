use std::path::Path;

use crate::base::{ExtractionResults, SourceRegionKind, Symbol, SymbolKind};
use crate::extract_canonical;
use crate::tests::helpers::{facts_with_pattern, metadata_str};

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("YAML extraction succeeds")
}

fn symbol<'a>(result: &'a ExtractionResults, name: &str, line: u32) -> &'a Symbol {
    result
        .symbols
        .iter()
        .find(|s| s.name == name && s.start_line == line)
        .unwrap_or_else(|| panic!("missing {name} on line {line}: {:#?}", result.symbols))
}

fn references(result: &ExtractionResults, from: &Symbol, to: &Symbol) -> bool {
    result
        .relationships
        .iter()
        .any(|r| r.from_symbol_id == from.id && r.to_symbol_id == to.id)
}

fn role(symbol: &Symbol) -> Option<&str> {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("test_role"))
        .and_then(serde_json::Value::as_str)
}

fn fact_at<'a>(
    result: &'a ExtractionResults,
    pattern: &str,
    line: u32,
) -> &'a crate::base::StructuralFact {
    facts_with_pattern(result, pattern)
        .into_iter()
        .find(|fact| fact.start_line == line)
        .unwrap_or_else(|| panic!("no {pattern} on line {line}"))
}

#[test]
fn leaf_pairs_carry_their_first_line_as_signature() {
    let source = "image: nginx:1.25\nport: 8080\nscript: |\n  make test\ndefaults: &defaults\n  adapter: pg\ndb:\n  host: x\n";
    let result = extract("app.yaml", source);
    let sig = |name: &str, line: u32| symbol(&result, name, line).signature.clone();
    assert_eq!(sig("image", 1).as_deref(), Some("image: nginx:1.25"));
    assert_eq!(sig("port", 2).as_deref(), Some("port: 8080"));
    assert_eq!(sig("script", 3).as_deref(), Some("script: |"));
    assert_eq!(sig("defaults", 5).as_deref(), Some("defaults: &defaults"));
    assert_eq!(sig("db", 7), None);
    assert_eq!(sig("host", 8).as_deref(), Some("host: x"));
}

#[test]
fn anchors_on_scalars_flow_values_and_items_resolve_their_aliases() {
    let source = r#"timeout: &default_timeout 30
ports: &ports [80, 443]
env: &env {A: 1}
services:
  - &pg postgres:15
server:
  timeout: *default_timeout
  ports: *ports
  env: *env
  db: *pg
"#;
    let result = extract("app.yaml", source);
    let anchor = |name: &str, line: u32| {
        symbol(&result, name, line)
            .metadata
            .as_ref()
            .and_then(|m| m.get("yaml_anchor"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    };
    assert_eq!(anchor("timeout", 1).as_deref(), Some("default_timeout"));
    assert_eq!(anchor("[0]", 5).as_deref(), Some("pg"));
    for (from, target) in [
        (symbol(&result, "timeout", 7), symbol(&result, "timeout", 1)),
        (symbol(&result, "ports", 8), symbol(&result, "ports", 2)),
        (symbol(&result, "env", 9), symbol(&result, "env", 3)),
        (symbol(&result, "db", 10), symbol(&result, "[0]", 5)),
    ] {
        assert!(
            references(&result, from, target),
            "{:#?}",
            result.relationships
        );
    }
    let value_kind = |line: u32| {
        metadata_str(fact_at(&result, "yaml.key_value.v1", line), "value_kind").map(str::to_string)
    };
    assert_eq!(value_kind(1).as_deref(), Some("scalar"));
    assert_eq!(value_kind(2).as_deref(), Some("sequence"));
    assert_eq!(value_kind(3).as_deref(), Some("mapping"));
    assert_eq!(
        metadata_str(fact_at(&result, "yaml.key_value.v1", 1), "anchor"),
        Some("default_timeout")
    );
}

#[test]
fn flow_mapping_pairs_are_symbols_like_block_pairs() {
    let source = "resources: {limits: {cpu: \"500m\", memory: 128Mi}, requests: {cpu: 250m}}\n";
    let result = extract("pod.yaml", source);
    let resources = symbol(&result, "resources", 1);
    let limits = result
        .symbols
        .iter()
        .find(|s| s.name == "limits")
        .expect("limits symbol");
    let cpu = result
        .symbols
        .iter()
        .find(|s| s.name == "cpu" && s.parent_id.as_deref() == Some(limits.id.as_str()))
        .expect("limits.cpu symbol");
    assert_eq!(resources.kind, SymbolKind::Module);
    assert_eq!(limits.kind, SymbolKind::Module);
    assert_eq!(limits.parent_id.as_deref(), Some(resources.id.as_str()));
    assert_eq!(cpu.kind, SymbolKind::Variable);
    let json_style = extract(
        "package.yml",
        "{\n  \"name\": \"demo\",\n  \"scripts\": {\"build\": \"tsc\"}\n}\n",
    );
    for name in ["name", "scripts", "build"] {
        assert!(
            json_style.symbols.iter().any(|s| s.name == name),
            "{name}: {:#?}",
            json_style.symbols
        );
    }
}

#[test]
fn alias_binds_the_nearest_preceding_anchor_in_its_document() {
    let source = r#"step: &s
  run: first
use_first: *s
step2: &s
  run: second
use_second: *s
---
labels: &labels
  app: web
copy: *labels
---
orphan: *labels
"#;
    let result = extract("redef.yaml", source);
    assert!(references(
        &result,
        symbol(&result, "use_first", 3),
        symbol(&result, "step", 1)
    ));
    assert!(references(
        &result,
        symbol(&result, "use_second", 6),
        symbol(&result, "step2", 4)
    ));
    assert!(!references(
        &result,
        symbol(&result, "use_second", 6),
        symbol(&result, "step", 1)
    ));
    assert!(references(
        &result,
        symbol(&result, "copy", 10),
        symbol(&result, "labels", 8)
    ));
    let orphan = symbol(&result, "orphan", 12);
    assert!(
        result
            .relationships
            .iter()
            .all(|r| r.from_symbol_id != orphan.id)
    );
}

#[test]
fn alias_is_owned_by_the_narrowest_key_around_it() {
    let source = r#"defaults: &defaults
  adapter: postgres
development:
  database: dev_db
  settings: *defaults
  pool:
    base: *defaults
  merged:
    <<: *defaults
"#;
    let result = extract("database.yml", source);
    let defaults = symbol(&result, "defaults", 1);
    for (from, line) in [("settings", 5), ("base", 7), ("merged", 8)] {
        let from = symbol(&result, from, line);
        assert!(
            references(&result, from, defaults),
            "{:#?}",
            result.relationships
        );
        assert!(
            result
                .identifiers
                .iter()
                .any(|i| i.containing_symbol_id.as_deref() == Some(from.id.as_str())),
            "{:#?}",
            result.identifiers
        );
    }
}

#[test]
fn key_paths_index_sequence_items_and_quote_special_keys() {
    let source = r#"steps:
  - uses: a
  - uses: b
.defaults:
  image: ruby
data:
  nginx.conf: |
    server {}
"#;
    let result = extract("ci.yaml", source);
    let path = |line: u32| {
        metadata_str(fact_at(&result, "yaml.key_value.v1", line), "key_path").map(str::to_string)
    };
    assert_eq!(path(2).as_deref(), Some("$.steps[0].uses"));
    assert_eq!(path(3).as_deref(), Some("$.steps[1].uses"));
    assert_eq!(path(5).as_deref(), Some("$['.defaults'].image"));
    assert_eq!(path(7).as_deref(), Some("$.data['nginx.conf']"));
}

#[test]
fn scalar_style_tags_and_document_index_are_recorded() {
    let source = r#"data:
  nginx.conf: |-
    server { listen 80; }
  motd: >
    hello
TableName: !Sub 'orders-${Stage}'
plain: value
quoted: "x"
---
metadata:
  name: web
"#;
    let result = extract("mixed.yaml", source);
    let fact = |line: u32| fact_at(&result, "yaml.key_value.v1", line);
    assert_eq!(metadata_str(fact(2), "value_kind"), Some("block_scalar"));
    assert_eq!(metadata_str(fact(2), "scalar_style"), Some("literal"));
    assert_eq!(metadata_str(fact(2), "chomping"), Some("strip"));
    assert_eq!(metadata_str(fact(4), "scalar_style"), Some("folded"));
    assert_eq!(metadata_str(fact(4), "chomping"), Some("clip"));
    assert_eq!(metadata_str(fact(6), "tag"), Some("!Sub"));
    assert_eq!(metadata_str(fact(6), "value_kind"), Some("scalar"));
    assert_eq!(metadata_str(fact(6), "scalar_style"), Some("single_quoted"));
    assert_eq!(metadata_str(fact(7), "scalar_style"), Some("plain"));
    assert_eq!(metadata_str(fact(8), "scalar_style"), Some("double_quoted"));
    let index = |fact: &crate::base::StructuralFact| {
        fact.metadata
            .as_ref()
            .and_then(|m| m.get("document_index"))
            .and_then(serde_json::Value::as_u64)
    };
    assert_eq!(index(fact(11)), Some(1));
    assert_eq!(index(fact(7)), Some(0));
    let documents = facts_with_pattern(&result, "yaml.document.v1");
    assert_eq!(
        documents.iter().map(|f| index(f)).collect::<Vec<_>>(),
        vec![Some(0), Some(1)]
    );
    let single = extract("one.yaml", "a: 1\n");
    assert_eq!(index(fact_at(&single, "yaml.key_value.v1", 1)), None);
}

#[test]
fn container_structure_tests_mark_every_test_list() {
    let source = r#"schemaVersion: '2.0.0'
commandTests:
  - name: "python installed"
    command: "which"
fileExistenceTests:
  - name: 'Root'
    path: '/'
fileContentTests:
  - name: 'Debian Sources'
    path: '/etc/apt/sources.list'
licenseTests:
  - debian: true
metadataTest:
  workdir: /app
"#;
    let result = extract("cst.yaml", source);
    for (name, line) in [
        ("commandTests", 2),
        ("fileExistenceTests", 5),
        ("fileContentTests", 8),
        ("licenseTests", 11),
    ] {
        assert_eq!(
            role(symbol(&result, name, line)),
            Some("test_container"),
            "{name}"
        );
    }
    for line in [3, 6, 9] {
        assert_eq!(
            role(symbol(&result, "name", line)),
            Some("test_case"),
            "line {line}"
        );
    }
    assert_eq!(role(symbol(&result, "metadataTest", 13)), Some("test_case"));
    let tavern = extract(
        "tests/test_users.tavern.yaml",
        "test_name: Get users\nstages:\n  - name: list\n    request: {url: /users}\n",
    );
    assert_eq!(role(symbol(&tavern, "test_name", 1)), Some("test_case"));
    let plain = extract("users.yaml", "test_name: Get users\n");
    assert_eq!(role(symbol(&plain, "test_name", 1)), None);
}

#[test]
fn keys_decode_quotes_and_never_take_the_value_name() {
    let source =
        "\"quoted \\\"key\\\"\": 1\n'it''s': 2\n? [us-east, us-west]\n: primary\nlast: 3\n";
    let result = extract("keys.yaml", source);
    symbol(&result, "quoted \"key\"", 1);
    symbol(&result, "it's", 2);
    assert!(result.symbols.iter().all(|s| s.name != "primary"));
    symbol(&result, "last", 5);
}

#[test]
fn compose_services_emit_facts_edges_and_pending_extends() {
    let source = r#"services:
  api:
    build: {context: ./api}
    ports: ["8080:8080"]
    depends_on:
      db: {condition: service_healthy}
    extends:
      file: common-services.yml
      service: logging
  worker:
    image: acme/worker
    depends_on: [db]
  db:
    image: postgres:16
"#;
    let result = extract("docker-compose.yml", source);
    let services = facts_with_pattern(&result, "yaml.compose_service.v1");
    let service = |name: &str| {
        services
            .iter()
            .find(|f| metadata_str(f, "name") == Some(name))
            .unwrap_or_else(|| panic!("{name}: {services:#?}"))
    };
    assert_eq!(metadata_str(service("api"), "build_context"), Some("./api"));
    assert_eq!(metadata_str(service("db"), "image"), Some("postgres:16"));
    let db = symbol(&result, "db", 13);
    assert!(references(&result, symbol(&result, "db", 6), db));
    assert!(references(&result, symbol(&result, "depends_on", 12), db));
    let pending = &result.structured_pending_relationships;
    assert_eq!(pending.len(), 1, "{pending:#?}");
    assert_eq!(
        pending[0].target.import_context.as_deref(),
        Some("common-services.yml")
    );
    assert_eq!(pending[0].target.terminal_name, "logging");
    assert!(
        facts_with_pattern(&extract("services.yml", source), "yaml.compose_service.v1").is_empty()
    );
}

#[test]
fn kubernetes_documents_emit_resource_facts_and_same_file_refs() {
    let source = r#"apiVersion: v1
kind: ConfigMap
metadata: {name: web-config, namespace: shop}
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: web
  namespace: shop
spec:
  template:
    spec:
      containers:
        - name: web
          envFrom:
            - configMapRef: {name: web-config}
"#;
    let result = extract("k8s/app.yaml", source);
    let resources = facts_with_pattern(&result, "yaml.k8s_resource.v1");
    assert_eq!(resources.len(), 2, "{resources:#?}");
    assert_eq!(metadata_str(resources[1], "kind"), Some("Deployment"));
    assert_eq!(metadata_str(resources[1], "api_version"), Some("apps/v1"));
    assert_eq!(metadata_str(resources[1], "name"), Some("web"));
    assert_eq!(metadata_str(resources[1], "namespace"), Some("shop"));
    let config_name = result
        .symbols
        .iter()
        .find(|s| s.name == "name" && s.start_line == 3)
        .expect("ConfigMap metadata.name");
    let ref_name = result
        .symbols
        .iter()
        .find(|s| s.name == "name" && s.start_line == 16)
        .expect("configMapRef.name");
    assert!(
        references(&result, ref_name, config_name),
        "{:#?}",
        result.relationships
    );
}

#[test]
fn ansible_playbooks_emit_pending_files_and_handler_references() {
    let source = r#"- import_playbook: db.yml
- name: Configure web servers
  hosts: webservers
  roles:
    - role: web
  tasks:
    - name: Install nginx
      ansible.builtin.apt: {name: nginx}
      notify: Restart nginx
    - ansible.builtin.include_tasks: tls.yml
  handlers:
    - name: Restart nginx
      ansible.builtin.service: {name: nginx, state: restarted}
"#;
    let result = extract("site.yml", source);
    let contexts: Vec<_> = result
        .structured_pending_relationships
        .iter()
        .map(|p| p.target.import_context.clone().unwrap_or_default())
        .collect();
    for expected in ["db.yml", "tls.yml", "roles/web/tasks/main.yml"] {
        assert!(contexts.contains(&expected.to_string()), "{contexts:?}");
    }
    let handler = symbol(&result, "[0]", 12);
    assert!(references(&result, symbol(&result, "notify", 9), handler));
    let tasks = facts_with_pattern(&result, "yaml.ansible_task.v1");
    let modules: Vec<_> = tasks.iter().map(|f| metadata_str(f, "module")).collect();
    assert_eq!(
        modules,
        vec![
            Some("ansible.builtin.apt"),
            Some("ansible.builtin.include_tasks"),
            Some("ansible.builtin.service")
        ]
    );
    assert!(
        extract("config.yml", "- name: x\n  value: 1\n")
            .structured_pending_relationships
            .is_empty()
    );
}

#[test]
fn kustomization_resources_emit_pending_file_rows() {
    let source =
        "resources:\n  - deployment.yaml\n  - ../base\ncomponents:\n  - ../components/tls\n";
    let result = extract("overlays/prod/kustomization.yaml", source);
    let contexts: Vec<_> = result
        .structured_pending_relationships
        .iter()
        .filter_map(|p| p.target.import_context.clone())
        .collect();
    assert_eq!(
        contexts,
        vec!["deployment.yaml", "../base", "../components/tls"]
    );
}

#[test]
fn ci_shell_blocks_are_embedded_regions() {
    let source = r#"jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - name: Download
        run: |
          set -euo pipefail
          gh release download "$tag"
      - shell: pwsh
        run: dotnet build
"#;
    let result = extract(".github/workflows/ci.yml", source);
    let embedded: Vec<_> = result
        .source_regions
        .iter()
        .filter(|r| r.kind == SourceRegionKind::Embedded)
        .map(|r| {
            (
                r.start_line,
                r.metadata
                    .as_ref()
                    .and_then(|m| m.get("embedded_language"))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
            )
        })
        .collect();
    assert_eq!(
        embedded,
        vec![
            (6, Some("bash".to_string())),
            (10, Some("powershell".to_string()))
        ]
    );
    let azure = extract(
        "azure-pipelines.yml",
        "steps:\n  - powershell: Write-Host hi\n  - script: echo hi\n",
    );
    assert_eq!(
        azure
            .source_regions
            .iter()
            .filter(|r| r.kind == SourceRegionKind::Embedded)
            .count(),
        2
    );
    let plain = extract("config.yml", "run: |\n  echo hi\n");
    assert!(
        plain
            .source_regions
            .iter()
            .all(|r| r.kind != SourceRegionKind::Embedded)
    );
}
