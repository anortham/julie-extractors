use crate::base::{ExtractionResults, RelationshipKind};
use crate::extract_canonical;
use crate::tests::helpers::{facts_with_pattern, metadata_str};
use std::collections::BTreeSet;
use std::path::Path;

fn extract(file: &str, source: &str) -> ExtractionResults {
    extract_canonical(file, source, Path::new("/tmp/test")).unwrap()
}

fn job_edges(results: &ExtractionResults) -> BTreeSet<(String, String)> {
    let name = |id: &str| {
        results
            .symbols
            .iter()
            .find(|s| s.id == id)
            .unwrap()
            .name
            .clone()
    };
    results
        .relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::References)
        .map(|r| (name(&r.from_symbol_id), name(&r.to_symbol_id)))
        .collect()
}

fn pending_contexts(results: &ExtractionResults) -> BTreeSet<String> {
    results
        .structured_pending_relationships
        .iter()
        .filter_map(|p| p.target.import_context.clone())
        .collect()
}

#[test]
fn github_workflow_needs_uses_and_triggers() {
    let source = "on:\n  push:\n  workflow_call:\njobs:\n  build:\n    uses: ./.github/workflows/build.yml\n  test:\n    needs: build\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v4\n      - uses: ./.github/actions/setup\n  deploy:\n    needs: [build, test]\n    runs-on: ubuntu-latest\n";
    let results = extract(".github/workflows/ci.yml", source);

    assert_eq!(
        job_edges(&results),
        BTreeSet::from([
            ("test".to_string(), "build".to_string()),
            ("deploy".to_string(), "build".to_string()),
            ("deploy".to_string(), "test".to_string()),
        ])
    );
    assert!(
        results
            .identifiers
            .iter()
            .any(|i| i.name == "build" && i.target_symbol_id.is_some())
    );
    assert_eq!(
        pending_contexts(&results),
        BTreeSet::from([
            "./.github/workflows/build.yml".to_string(),
            "./.github/actions/setup".to_string(),
        ])
    );
    let triggers: BTreeSet<_> = facts_with_pattern(&results, "yaml.ci_trigger.v1")
        .iter()
        .map(|f| metadata_str(f, "event").unwrap().to_string())
        .collect();
    assert_eq!(
        triggers,
        BTreeSet::from(["push".to_string(), "workflow_call".to_string()])
    );
    let checkout = facts_with_pattern(&results, "yaml.ci_uses.v1")
        .into_iter()
        .find(|f| metadata_str(f, "kind") == Some("action"))
        .unwrap();
    assert_eq!(metadata_str(checkout, "action"), Some("actions/checkout"));
    assert_eq!(metadata_str(checkout, "ref"), Some("v4"));
    let jobs = facts_with_pattern(&results, "yaml.ci_job.v1");
    assert_eq!(jobs.len(), 3);
}

#[test]
fn gitlab_extends_needs_and_local_includes() {
    let source = "include:\n  - local: ci/templates/deploy.yml\n  - 'ci/lint.yml'\nstages: [build, test]\n.ruby:\n  image: ruby:3.3\ncompile:\n  extends: .ruby\n  stage: build\nrspec:\n  extends: [.ruby]\n  needs:\n    - job: compile\n";
    let results = extract(".gitlab-ci.yml", source);

    assert_eq!(
        job_edges(&results),
        BTreeSet::from([
            ("compile".to_string(), ".ruby".to_string()),
            ("rspec".to_string(), ".ruby".to_string()),
            ("rspec".to_string(), "compile".to_string()),
        ])
    );
    assert_eq!(
        pending_contexts(&results),
        BTreeSet::from([
            "ci/templates/deploy.yml".to_string(),
            "ci/lint.yml".to_string(),
        ])
    );
}

#[test]
fn azure_templates_are_pending_file_references() {
    let source = "jobs:\n  - template: .azure/pipelines/jobs/default-build.yml@self\n";
    let results = extract("azure-pipelines.yml", source);

    assert_eq!(
        pending_contexts(&results),
        BTreeSet::from([".azure/pipelines/jobs/default-build.yml".to_string()])
    );
}
