//! Typed registry of every structural-fact `pattern_id` the extractor emits.
//!
//! This module is the machine-readable source of truth for the metadata payload
//! carried by each structural fact: for every pattern it declares the languages
//! it fires for, its query family, and every metadata key with a value type and
//! a presence rule. Downstream consumers (Miller, the `languages --json` report,
//! contract docs) read this registry instead of hard-coding out-of-band
//! knowledge of the payloads.
//!
//! The registry describes the EXISTING v3 contract; it does not change emission.
//! It is authored directly from the collector emission sites
//! (`insert_string`/`insert_number`/`metadata.insert` call sites) across these
//! sources (with the languages each covers):
//!
//! - `base/structural_facts.rs`: built-in patterns for c, cpp, go, javascript, jsx, python, rust, tsx, typescript.
//! - `base/code_structural_facts.rs`: dart, elixir, java, kotlin, lua, php, r, ruby, scala, swift, bash, gdscript, powershell, qml, vbnet, zig.
//! - `base/data_structural_facts.rs`: markdown, json, toml, yaml, regex.
//! - `base/sql_structural_facts.rs`: sql.
//! - `base/framework_structural_facts/`: aspnet, htmx, alpine, razor, HTTP frameworks.
//! - `base/web_structural_facts/`: css, html, vue, react, nextjs, nuxt, http client.
//!
//! SPECS live in sibling family modules (`builtins`, `data`, `sql`, `framework`,
//! `web`, `http_client`); this file owns types, authoring helpers, and JSON
//! serialization only.
//!
//! Presence semantics (the conformance rule Task 2 enforces over the golden
//! corpus): an `Always` key is present on every emitted fact of its pattern; an
//! `Optional` key may be absent. When a key is derived from a value that gates
//! emission (the fact is only produced when the value exists) it is `Always`.
//!
//! Every fact also carries the two base keys `pattern_version` and
//! `query_family` (from each collector's `base_metadata`). Framework facts and
//! web route/http facts additionally carry a `framework` key.

/// JSON value type a metadata key carries. Additions to this enum are
/// lead-adjudicated contract decisions, not silent extensions: when a collector
/// emits a value shape none of these variants can express, that is a contract
/// mismatch to escalate and adjudicate, never to paper over. `ObjectArray` is
/// the one such adjudicated addition so far (Task 2, finding D1), covering
/// `route_parameters` on `razor.page_directive.v1` — a shipped v2.5.x payload
/// that cannot be flattened to a `StringArray` without losing per-parameter
/// fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataValueType {
    String,
    Bool,
    Number,
    StringArray,
    /// A JSON array whose every element is a JSON object. The object's fields
    /// are documented in prose on the declaring key; the registry does not carry
    /// a per-field schema for them.
    ObjectArray,
}

/// Whether a declared metadata key is guaranteed present (`Always`) on every
/// emitted fact of the pattern, or may be absent (`Optional`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyPresence {
    Always,
    Optional,
}

/// One metadata key declared for a pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetadataKeySpec {
    /// Metadata key name, e.g. `route_path`.
    pub key: &'static str,
    /// JSON value type the key carries.
    pub value_type: MetadataValueType,
    /// Whether the key is always present or conditional.
    pub presence: KeyPresence,
    /// One-sentence, consumer-facing description of the key.
    pub description: &'static str,
}

/// The full contract for one structural-fact `pattern_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StructuralFactPatternSpec {
    /// Stable pattern identifier, e.g. `nextjs.file_route.v1`.
    pub pattern_id: &'static str,
    /// Languages the collectors emit this pattern for.
    pub languages: &'static [&'static str],
    /// Query family the fact belongs to (mirrors the emitted `query_family`).
    pub query_family: &'static str,
    /// One-sentence, consumer-facing description of the pattern.
    pub description: &'static str,
    /// Every metadata key the pattern can carry, with type and presence.
    pub metadata_keys: &'static [MetadataKeySpec],
}

// ---------------------------------------------------------------------------
// Authoring helpers (compile-time only; keep the SPECS table readable).
// ---------------------------------------------------------------------------

pub(super) use KeyPresence::{Always as ALWAYS, Optional as OPT};
pub(super) use MetadataValueType::{
    Bool as BOOL, Number as NUM, ObjectArray as OBJARR, String as STR, StringArray as ARR,
};

pub(super) const fn key(
    key: &'static str,
    value_type: MetadataValueType,
    presence: KeyPresence,
    description: &'static str,
) -> MetadataKeySpec {
    MetadataKeySpec {
        key,
        value_type,
        presence,
        description,
    }
}

/// `pattern_version` + `query_family`, inserted by every collector's
/// `base_metadata` on every fact.
pub(super) const K_PATTERN_VERSION: MetadataKeySpec = key(
    "pattern_version",
    NUM,
    ALWAYS,
    "Schema version of this structural-fact pattern (currently 1).",
);
pub(super) const K_QUERY_FAMILY: MetadataKeySpec = key(
    "query_family",
    STR,
    ALWAYS,
    "Coarse query family the fact belongs to; mirrors the spec's query_family.",
);
/// Explicit `framework` key: a base key for all framework-collector facts, and
/// an emitted key on web route/http facts.
pub(super) const K_FRAMEWORK: MetadataKeySpec = key(
    "framework",
    STR,
    ALWAYS,
    "Owning framework or HTTP-client label for the fact.",
);

/// Base keys shared by every fact that does not add a `framework` key.
pub(super) const BASE_KEYS: &[MetadataKeySpec] = &[K_PATTERN_VERSION, K_QUERY_FAMILY];

mod builtins;
mod data;
mod framework;
mod http_client;
mod marker;
mod sql;
mod web;

/// Concatenated registry SPECS in the public accessor's pre-split order.
/// [`structural_fact_patterns_json`] independently sorts by `pattern_id`.
fn all_specs() -> Vec<StructuralFactPatternSpec> {
    let mut specs = Vec::new();
    specs.extend(builtins::specs());
    specs.extend_from_slice(marker::SPECS);
    specs.extend_from_slice(data::SPECS);
    specs.extend_from_slice(sql::SPECS);
    specs.extend(framework::specs());
    specs.extend(web::specs());
    specs.extend_from_slice(http_client::SPECS);
    specs
}

/// The registry: one spec per emitted structural-fact `pattern_id`.
///
/// Tasks 2–4 consume this via the accessor below.
pub fn structural_fact_pattern_specs() -> &'static [StructuralFactPatternSpec] {
    use std::sync::OnceLock;
    static SPECS: OnceLock<Box<[StructuralFactPatternSpec]>> = OnceLock::new();
    SPECS
        .get_or_init(|| all_specs().into_boxed_slice())
        .as_ref()
}

// ---------------------------------------------------------------------------
// JSON serialization: the checked-in contract artifact and the
// `languages --json` report section share this one serializer, so the file and
// the report stay byte-equivalent in content.
// ---------------------------------------------------------------------------

/// Stable lower_snake token a `MetadataValueType` serializes to in the JSON
/// contract. This mapping is itself a contract: renames are lead-adjudicated.
fn value_type_token(value_type: MetadataValueType) -> &'static str {
    match value_type {
        MetadataValueType::String => "string",
        MetadataValueType::Bool => "bool",
        MetadataValueType::Number => "number",
        MetadataValueType::StringArray => "string_array",
        MetadataValueType::ObjectArray => "object_array",
    }
}

/// Stable lower_snake token a `KeyPresence` serializes to in the JSON contract.
fn presence_token(presence: KeyPresence) -> &'static str {
    match presence {
        KeyPresence::Always => "always",
        KeyPresence::Optional => "optional",
    }
}

/// The structural-fact pattern registry serialized as a deterministic JSON
/// array — the machine-readable, source-of-truth metadata-payload contract.
///
/// Determinism: specs are sorted by `pattern_id` (unique, so a total order),
/// and every object emits its keys in a fixed order matching the Rust struct
/// fields. Spec objects emit `pattern_id`, `languages`, `query_family`,
/// `description`, `metadata_keys`; each metadata-key object emits `key`,
/// `value_type`, `presence`, `description`. A pattern's `languages` and
/// `metadata_keys` keep their authored order (both already fixed and unique in
/// the registry). Insertion order survives because serde_json's
/// `preserve_order` feature is active in this workspace's build graph; the
/// checked-in-file sync test (`tests/structural_fact_registry.rs`) is the
/// tripwire if that ever regresses.
///
/// This is the single serializer behind both
/// `docs/contracts/structural-fact-patterns.json` (Task 3) and the
/// `structural_fact_patterns` section of `languages --json` (Task 4).
pub fn structural_fact_patterns_json() -> serde_json::Value {
    let mut specs: Vec<&StructuralFactPatternSpec> =
        structural_fact_pattern_specs().iter().collect();
    specs.sort_by(|a, b| a.pattern_id.cmp(b.pattern_id));

    let specs_json: Vec<serde_json::Value> = specs
        .into_iter()
        .map(|spec| {
            let metadata_keys: Vec<serde_json::Value> = spec
                .metadata_keys
                .iter()
                .map(|meta| {
                    let mut obj = serde_json::Map::new();
                    obj.insert("key".to_string(), meta.key.into());
                    obj.insert(
                        "value_type".to_string(),
                        value_type_token(meta.value_type).into(),
                    );
                    obj.insert("presence".to_string(), presence_token(meta.presence).into());
                    obj.insert("description".to_string(), meta.description.into());
                    serde_json::Value::Object(obj)
                })
                .collect();

            let languages: Vec<serde_json::Value> =
                spec.languages.iter().map(|lang| (*lang).into()).collect();

            let mut obj = serde_json::Map::new();
            obj.insert("pattern_id".to_string(), spec.pattern_id.into());
            obj.insert("languages".to_string(), serde_json::Value::Array(languages));
            obj.insert("query_family".to_string(), spec.query_family.into());
            obj.insert("description".to_string(), spec.description.into());
            obj.insert(
                "metadata_keys".to_string(),
                serde_json::Value::Array(metadata_keys),
            );
            serde_json::Value::Object(obj)
        })
        .collect();

    serde_json::Value::Array(specs_json)
}

/// Exact byte contents of `docs/contracts/structural-fact-patterns.json`:
/// [`structural_fact_patterns_json`] pretty-printed with 2-space indent and a
/// trailing newline (repo JSON convention). Both the sync test's comparison and
/// its regeneration path use this one function, so they can never diverge on
/// formatting.
pub fn structural_fact_patterns_contract_json() -> String {
    let mut rendered = serde_json::to_string_pretty(&structural_fact_patterns_json())
        .expect("structural-fact registry is always JSON-serializable");
    rendered.push('\n');
    rendered
}

#[cfg(test)]
mod tests;
