use std::collections::BTreeMap;

use julie_extract_artifact::model::{
    ArtifactCapabilityFlags, ArtifactCapabilitySnapshot, ArtifactLanguageCapabilityFixtureRow,
    ArtifactLanguageCapabilityGapRow, ArtifactLanguageCapabilityRow, ArtifactParserInventoryRow,
    CapabilityGapStatus,
};
use julie_extractors::{
    CapabilityFlags, CapabilityKindCoverage, KindCoverage, capability_snapshot,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const CARGO_LOCK: &str = include_str!("../../../Cargo.lock");

pub(crate) fn current_capability_fingerprints() -> (String, String) {
    let snapshot = artifact_capability_snapshot();
    (
        parser_inventory_fingerprint(&snapshot.parser_inventory),
        capability_snapshot_fingerprint(&snapshot.languages),
    )
}

pub(crate) fn artifact_capability_snapshot() -> ArtifactCapabilitySnapshot {
    let snapshot = capability_snapshot();
    let lock_packages = cargo_lock_packages();
    let languages = snapshot
        .languages()
        .map(|row| ArtifactLanguageCapabilityRow {
            language: row.language.clone(),
            parser_package: row.parser_crate.clone(),
            extensions: row.extensions.clone(),
            dependency_status: row.dependency_status.clone(),
            target_capabilities: artifact_flags(row.target_capabilities),
            actual_capabilities: artifact_flags(row.capabilities),
            kind_coverage: kind_coverage_json(&row.kind_coverage),
            fixtures: row
                .fixtures
                .iter()
                .map(|fixture| ArtifactLanguageCapabilityFixtureRow {
                    fixture_name: fixture.name.clone(),
                    source_path: fixture.source.clone(),
                    expected_path: fixture.expected.clone(),
                })
                .collect(),
            gaps: row
                .capability_gaps
                .iter()
                .map(|gap| ArtifactLanguageCapabilityGapRow {
                    gap_id: format!("{}:{}", row.language, gap.capability),
                    capability: gap.capability.clone(),
                    status: CapabilityGapStatus::try_from(gap.status.as_str())
                        .unwrap_or_else(|error| panic!("{error}")),
                    reason: gap.reason.clone(),
                    required_closure: gap.required_closure.clone(),
                    evidence: gap.evidence.clone(),
                })
                .chain(reference_resolution_gaps(&row.language))
                .collect(),
        })
        .collect::<Vec<_>>();
    let parser_inventory = languages
        .iter()
        .map(|row| {
            let lock_package = lock_packages.get(&row.parser_package);
            let parser_version = lock_package.map(|package| package.version.clone());
            ArtifactParserInventoryRow {
                language: row.language.clone(),
                parser_package: row.parser_package.clone(),
                parser_version: parser_version.clone(),
                grammar_version: parser_version,
                source: lock_package
                    .and_then(|package| package.source.clone())
                    .or_else(|| Some("cargo_lock".to_string())),
                metadata: Some(json!({
                    "dependency_status": row.dependency_status,
                    "cargo_lock_source": lock_package.and_then(|package| package.source.as_ref()),
                })),
            }
        })
        .collect();

    ArtifactCapabilitySnapshot {
        parser_inventory,
        languages,
    }
}

/// Reference-resolution capability rows for a language: the `tier2_import` and
/// `tier3_receiver` honesty surfaces (design §"Honesty & parity surfaces" and the
/// tier table). These are deterministic, per-language, and independent of any
/// individual scan's results:
///
/// * `reference_resolution.tier2_import` is recorded as a gap for every language
///   whose import-guided tier is gated off (no fixture-tested import contract yet;
///   [`crate::resolution::tier2_enabled`] is the gate). TypeScript/JavaScript have
///   the contract, so they emit no tier-2 gap.
/// * `reference_resolution.tier3_receiver` is recorded as a gap for every language
///   because receiver-typed resolution coverage is bounded by per-language
///   `type_facts` emission today (broadened by follow-on F2); the tier ships with
///   its measured coverage rather than a completeness claim.
/// * `reference_resolution.tier3_static_type` is recorded as a gap for every
///   language without fixture-proven static-receiver support
///   ([`crate::resolution::tier3_static_type_proven`] is the gate). The tier runs
///   everywhere, but only yields edges where the extractor emits type `visibility`
///   and a standalone `static` member modifier.
fn reference_resolution_gaps(language: &str) -> Vec<ArtifactLanguageCapabilityGapRow> {
    let mut gaps = Vec::new();
    if !crate::resolution::tier2_enabled(language) {
        gaps.push(ArtifactLanguageCapabilityGapRow {
            gap_id: format!("{language}:reference_resolution.tier2_import"),
            capability: "reference_resolution.tier2_import".to_string(),
            status: CapabilityGapStatus::Open,
            reason: "import-guided resolution is gated off: this language has no \
                     fixture-tested import contract to key tier 2 on"
                .to_string(),
            required_closure: "normalize per-language import facts (module specifier + \
                               local binding + imported name) as first-class rows (F4), \
                               then enable tier 2 for this language"
                .to_string(),
            evidence: json!({
                "tier": 2,
                "planned_closure_task": "F4",
            }),
        });
    }
    gaps.push(ArtifactLanguageCapabilityGapRow {
        gap_id: format!("{language}:reference_resolution.tier3_receiver"),
        capability: "reference_resolution.tier3_receiver".to_string(),
        status: CapabilityGapStatus::Open,
        reason: "receiver-typed resolution coverage is bounded by this language's \
                 type_facts emission; the tier ships with measured coverage, not a \
                 completeness claim"
            .to_string(),
        required_closure: "broaden type_facts emission (especially for local variables) \
                           toward full receiver coverage (F2)"
            .to_string(),
        evidence: json!({
            "tier": 3,
            "planned_closure_task": "F2",
        }),
    });
    if !crate::resolution::tier3_static_type_proven(language) {
        gaps.push(ArtifactLanguageCapabilityGapRow {
            gap_id: format!("{language}:reference_resolution.tier3_static_type"),
            capability: "reference_resolution.tier3_static_type".to_string(),
            status: CapabilityGapStatus::Open,
            reason: "static-type-receiver resolution has no fixture-proven contract for \
                     this language: the tier only yields an edge where the extractor \
                     emits visibility on the type symbol and a standalone `static` word \
                     in the member signature, and neither is verified here"
                .to_string(),
            required_closure: "emit type visibility and a language-native static member \
                               modifier for this language, then register a \
                               fixtures/extraction/resolution_contract/<language>/\
                               static_type_receiver/ case and add it to \
                               TIER3_STATIC_TYPE_LANGUAGES"
                .to_string(),
            evidence: json!({
                "tier": 3,
                "method": crate::resolution::METHOD_TIER3_STATIC,
                "planned_closure_task": "F2",
            }),
        });
    }
    gaps
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CargoLockPackage {
    version: String,
    source: Option<String>,
}

fn cargo_lock_packages() -> BTreeMap<String, CargoLockPackage> {
    #[derive(Default)]
    struct PartialPackage {
        name: Option<String>,
        version: Option<String>,
        source: Option<String>,
    }

    fn push_package(packages: &mut BTreeMap<String, CargoLockPackage>, package: PartialPackage) {
        let (Some(name), Some(version)) = (package.name, package.version) else {
            return;
        };
        packages.insert(
            name,
            CargoLockPackage {
                version,
                source: package.source,
            },
        );
    }

    let mut packages = BTreeMap::new();
    let mut current: Option<PartialPackage> = None;

    for line in CARGO_LOCK.lines() {
        if line == "[[package]]" {
            if let Some(package) = current.take() {
                push_package(&mut packages, package);
            }
            current = Some(PartialPackage::default());
            continue;
        }

        let Some(package) = current.as_mut() else {
            continue;
        };
        let Some((key, value)) = line.split_once(" = ") else {
            continue;
        };
        let value = value.trim().trim_matches('"').to_string();
        match key {
            "name" => package.name = Some(value),
            "version" => package.version = Some(value),
            "source" => package.source = Some(value),
            _ => {}
        }
    }

    if let Some(package) = current {
        push_package(&mut packages, package);
    }

    packages
}

pub(crate) fn parser_inventory_fingerprint(rows: &[ArtifactParserInventoryRow]) -> String {
    let mut canonical_rows = rows
        .iter()
        .map(|row| {
            (
                row.language.clone(),
                row.parser_package.clone(),
                json!({
                    "language": row.language,
                    "parser_package": row.parser_package,
                    "parser_version": row.parser_version,
                    "grammar_version": row.grammar_version,
                    "source": row.source,
                    "metadata": row.metadata,
                }),
            )
        })
        .collect::<Vec<_>>();
    canonical_rows.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    fingerprint_json(&json!({
        "domain": "parser_inventory",
        "version": 1,
        "rows": canonical_rows
            .into_iter()
            .map(|(_, _, value)| value)
            .collect::<Vec<_>>(),
    }))
}

pub(crate) fn capability_snapshot_fingerprint(rows: &[ArtifactLanguageCapabilityRow]) -> String {
    let mut canonical_rows = rows
        .iter()
        .map(|row| {
            let mut extensions = row.extensions.clone();
            extensions.sort();
            let mut fixtures = row
                .fixtures
                .iter()
                .map(|fixture| {
                    (
                        fixture.fixture_name.clone(),
                        json!({
                            "fixture_name": fixture.fixture_name,
                            "source_path": fixture.source_path,
                            "expected_path": fixture.expected_path,
                        }),
                    )
                })
                .collect::<Vec<_>>();
            fixtures.sort_by(|left, right| left.0.cmp(&right.0));
            let mut gaps = row
                .gaps
                .iter()
                .map(|gap| {
                    (
                        gap.gap_id.clone(),
                        json!({
                            "gap_id": gap.gap_id,
                            "capability": gap.capability,
                            "status": gap.status,
                            "reason": gap.reason,
                            "required_closure": gap.required_closure,
                            "evidence": gap.evidence,
                        }),
                    )
                })
                .collect::<Vec<_>>();
            gaps.sort_by(|left, right| left.0.cmp(&right.0));
            (
                row.language.clone(),
                json!({
                    "language": row.language,
                    "parser_package": row.parser_package,
                    "extensions": extensions,
                    "dependency_status": row.dependency_status,
                    "target_capabilities": capability_flags_json(row.target_capabilities),
                    "actual_capabilities": capability_flags_json(row.actual_capabilities),
                    "kind_coverage": row.kind_coverage,
                    "fixtures": fixtures
                        .into_iter()
                        .map(|(_, value)| value)
                        .collect::<Vec<_>>(),
                    "gaps": gaps
                        .into_iter()
                        .map(|(_, value)| value)
                        .collect::<Vec<_>>(),
                }),
            )
        })
        .collect::<Vec<_>>();
    canonical_rows.sort_by(|left, right| left.0.cmp(&right.0));
    fingerprint_json(&json!({
        "domain": "capability_snapshot",
        "version": 1,
        "rows": canonical_rows
            .into_iter()
            .map(|(_, value)| value)
            .collect::<Vec<_>>(),
    }))
}

fn capability_flags_json(flags: ArtifactCapabilityFlags) -> Value {
    json!({
        "symbols": flags.symbols,
        "relationships": flags.relationships,
        "pending_relationships": flags.pending_relationships,
        "identifiers": flags.identifiers,
        "types": flags.types,
    })
}

fn fingerprint_json(value: &Value) -> String {
    let bytes = serde_json::to_vec(value).expect("capability fingerprint input must serialize");
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{:x}", hasher.finalize())
}

fn artifact_flags(flags: CapabilityFlags) -> ArtifactCapabilityFlags {
    ArtifactCapabilityFlags {
        symbols: flags.symbols,
        relationships: flags.relationships,
        pending_relationships: flags.pending_relationships,
        identifiers: flags.identifiers,
        types: flags.types,
    }
}

pub(crate) fn kind_coverage_json(kind_coverage: &CapabilityKindCoverage) -> Value {
    json!({
        "symbols": kind_coverage_domain(&kind_coverage.symbols),
        "relationships": kind_coverage_domain(&kind_coverage.relationships),
        "identifiers": kind_coverage_domain(&kind_coverage.identifiers),
        "body_spans": kind_coverage_domain(&kind_coverage.body_spans),
        "structural_facts": kind_coverage_domain(&kind_coverage.structural_facts),
        "complexity_metrics": kind_coverage_domain(&kind_coverage.complexity_metrics),
        "annotations": kind_coverage_domain(&kind_coverage.annotations),
        "doc_comments": kind_coverage_domain(&kind_coverage.doc_comments),
        "literals": kind_coverage_domain(&kind_coverage.literals),
        "source_regions": kind_coverage_domain(&kind_coverage.source_regions),
        "test_detection": kind_coverage_domain(&kind_coverage.test_detection),
    })
}

fn kind_coverage_domain(domain: &KindCoverage) -> Value {
    json!({
        "supported": domain.supported,
        "not_applicable": domain.not_applicable,
        "open_gaps": domain.open_gaps.iter().map(|gap| {
            json!({
                "kind": gap.kind,
                "reason": gap.reason,
                "required_closure": gap.required_closure,
                "planned_closure_task": gap.planned_closure_task,
            })
        }).collect::<Vec<_>>(),
    })
}

pub(crate) fn flags(flags: CapabilityFlags) -> Value {
    json!({
        "symbols": flags.symbols,
        "relationships": flags.relationships,
        "pending_relationships": flags.pending_relationships,
        "identifiers": flags.identifiers,
        "types": flags.types,
    })
}

/// Structural-fact pattern registry section for the `languages --json` report.
///
/// Direct passthrough of the single extractor-side serializer so the report
/// section stays byte-equivalent to the checked-in
/// `docs/contracts/structural-fact-patterns.json` contract. No re-serialization
/// through CLI structs — the registry is the sole source of truth.
pub(crate) fn structural_fact_patterns_json() -> Value {
    julie_extractors::base::structural_fact_patterns_json()
}
