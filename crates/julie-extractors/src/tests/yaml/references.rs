use crate::base::{ExtractionResults, IdentifierKind, RelationshipKind};
use crate::extract_canonical;
use crate::tests::helpers::{facts_with_pattern, metadata_str};
use std::collections::BTreeSet;
use std::path::Path;

fn extract(file: &str, source: &str) -> ExtractionResults {
    extract_canonical(file, source, Path::new("/tmp/test")).unwrap()
}

fn edges(results: &ExtractionResults) -> BTreeSet<(String, String)> {
    let name = |id: &str| {
        results
            .symbols
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.name.clone())
            .unwrap()
    };
    results
        .relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::References)
        .map(|r| (name(&r.from_symbol_id), name(&r.to_symbol_id)))
        .collect()
}

fn edge(from: &str, to: &str) -> (String, String) {
    (from.to_string(), to.to_string())
}

#[test]
fn schema_refs_emit_references_pending_rows_and_facts() {
    let source = "openapi: 3.0.3\npaths:\n  /pets/{petId}:\n    get:\n      responses:\n        '200':\n          content:\n            application/json:\n              schema:\n                $ref: '#/components/schemas/Pet'\n        default:\n          $ref: './common.yaml#/components/responses/Error'\ncomponents:\n  schemas:\n    Pet:\n      allOf:\n        - $ref: '#/components/schemas/Owner'\n    Owner:\n      type: object\n";
    let results = extract("openapi.yaml", source);

    let edges = edges(&results);
    assert!(edges.contains(&edge("schema", "Pet")), "{edges:?}");
    assert!(edges.contains(&edge("[0]", "Owner")), "{edges:?}");
    let pending = &results.structured_pending_relationships;
    assert_eq!(pending.len(), 1, "{pending:#?}");
    assert_eq!(pending[0].target.terminal_name, "Error");
    assert_eq!(
        pending[0].target.import_context.as_deref(),
        Some("./common.yaml")
    );
    let refs = facts_with_pattern(&results, "yaml.ref.v1");
    assert_eq!(refs.len(), 3);
    let routes = facts_with_pattern(&results, "openapi.route.v1");
    assert_eq!(routes.len(), 1);
    assert_eq!(
        metadata_str(routes[0], "normalized_route_template"),
        Some("/pets/:petId")
    );
}

#[test]
fn cloudformation_intrinsics_reference_template_entries() {
    let source = "AWSTemplateFormatVersion: '2010-09-09'\nParameters:\n  Stage:\n    Type: String\nResources:\n  DataBucket:\n    Type: AWS::S3::Bucket\n  Handler:\n    Type: AWS::Lambda::Function\n    DependsOn: DataBucket\n    Properties:\n      Role: !GetAtt HandlerRole.Arn\n      Environment:\n        Variables:\n          BUCKET: !Ref DataBucket\n          NAME: !Sub '${Stage}-${AWS::Region}'\n  HandlerRole:\n    Type: AWS::IAM::Role\nOutputs:\n  BucketArn:\n    Value:\n      Fn::GetAtt: [DataBucket, Arn]\n";
    let results = extract("template.yaml", source);

    assert_eq!(
        edges(&results),
        BTreeSet::from([
            edge("Handler", "DataBucket"),
            edge("Handler", "HandlerRole"),
            edge("Handler", "Stage"),
            edge("BucketArn", "DataBucket"),
        ])
    );
    let names: BTreeSet<_> = results
        .identifiers
        .iter()
        .filter(|i| i.kind == IdentifierKind::VariableRef && i.target_symbol_id.is_some())
        .map(|i| i.name.as_str())
        .collect();
    assert_eq!(
        names,
        BTreeSet::from(["DataBucket", "HandlerRole", "Stage"])
    );
}

#[test]
fn yaml_outside_templates_does_not_treat_ref_keys_as_intrinsics() {
    let results = extract("config.yaml", "Ref: DataBucket\nDataBucket: 1\n");

    assert!(results.relationships.is_empty());
}
