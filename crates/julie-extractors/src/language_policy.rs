//! Extraction-owned language policy.
//!
//! This module intentionally owns only policy that changes artifact rows. Search
//! tokenization, scoring, embeddings, warnings, watcher behavior, and dashboard
//! policy are outside this product boundary.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use serde::Deserialize;

use crate::{Literal, LiteralKind};

const CONFIG_LANGUAGE_ALIASES: &[(&str, &str)] = &[("tsx", "typescript"), ("jsx", "javascript")];

const EMBEDDED_LITERAL_CARRIER_POLICIES: &[(&str, &str)] = &[
    ("bash", include_str!("../../../languages/bash.toml")),
    ("c", include_str!("../../../languages/c.toml")),
    ("cpp", include_str!("../../../languages/cpp.toml")),
    ("csharp", include_str!("../../../languages/csharp.toml")),
    ("dart", include_str!("../../../languages/dart.toml")),
    ("elixir", include_str!("../../../languages/elixir.toml")),
    ("gdscript", include_str!("../../../languages/gdscript.toml")),
    ("go", include_str!("../../../languages/go.toml")),
    ("java", include_str!("../../../languages/java.toml")),
    (
        "javascript",
        include_str!("../../../languages/javascript.toml"),
    ),
    ("fsharp", include_str!("../../../languages/fsharp.toml")),
    ("kotlin", include_str!("../../../languages/kotlin.toml")),
    ("lua", include_str!("../../../languages/lua.toml")),
    ("php", include_str!("../../../languages/php.toml")),
    (
        "powershell",
        include_str!("../../../languages/powershell.toml"),
    ),
    ("python", include_str!("../../../languages/python.toml")),
    ("qml", include_str!("../../../languages/qml.toml")),
    ("qmldir", include_str!("../../../languages/qmldir.toml")),
    ("r", include_str!("../../../languages/r.toml")),
    ("razor", include_str!("../../../languages/razor.toml")),
    ("ruby", include_str!("../../../languages/ruby.toml")),
    ("rust", include_str!("../../../languages/rust.toml")),
    ("scala", include_str!("../../../languages/scala.toml")),
    ("swift", include_str!("../../../languages/swift.toml")),
    (
        "typescript",
        include_str!("../../../languages/typescript.toml"),
    ),
    ("vbnet", include_str!("../../../languages/vbnet.toml")),
    ("vue", include_str!("../../../languages/vue.toml")),
    ("zig", include_str!("../../../languages/zig.toml")),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiteralCarrierPolicy {
    pub url: HashSet<String>,
    pub sql: HashSet<String>,
    pub route: HashSet<String>,
    pub retain_unclassified: bool,
}

#[derive(Debug, Deserialize)]
struct LanguagePolicyToml {
    literal_carriers: LiteralCarrierLists,
}

#[derive(Debug, Default, Deserialize)]
struct LiteralCarrierLists {
    #[serde(default)]
    retain_unclassified: bool,
    #[serde(default)]
    url: Vec<String>,
    #[serde(default)]
    sql: Vec<String>,
    #[serde(default)]
    route: Vec<String>,
}

impl LiteralCarrierPolicy {
    fn from_lists(lists: LiteralCarrierLists) -> Self {
        Self {
            url: lowercase_set(lists.url),
            sql: lowercase_set(lists.sql),
            route: lowercase_set(lists.route),
            retain_unclassified: lists.retain_unclassified,
        }
    }
}

pub fn literal_carrier_policies() -> &'static HashMap<String, LiteralCarrierPolicy> {
    static POLICIES: OnceLock<HashMap<String, LiteralCarrierPolicy>> = OnceLock::new();
    POLICIES.get_or_init(load_embedded_literal_carrier_policies)
}

pub fn literal_carrier_policy(language: &str) -> Option<&'static LiteralCarrierPolicy> {
    literal_carrier_policies().get(language)
}

pub fn classify_literals_by_carrier(literals: &mut Vec<Literal>) {
    classify_literals_with_policies(literals, literal_carrier_policies());
}

pub fn classify_literals_with_policies(
    literals: &mut Vec<Literal>,
    policies: &HashMap<String, LiteralCarrierPolicy>,
) {
    literals.retain_mut(|literal| {
        let Some(policy) = policies.get(&literal.language) else {
            return false;
        };
        let Some(carrier) = literal.carrier.as_deref() else {
            return policy.retain_unclassified;
        };
        let carrier = carrier.to_lowercase();

        if carrier_matches(&policy.url, &carrier) {
            literal.kind = LiteralKind::Url;
            true
        } else if carrier_matches(&policy.sql, &carrier) {
            literal.kind = LiteralKind::Sql;
            true
        } else if carrier_matches(&policy.route, &carrier) {
            literal.kind = LiteralKind::Route;
            true
        } else {
            policy.retain_unclassified
        }
    });
}

fn load_embedded_literal_carrier_policies() -> HashMap<String, LiteralCarrierPolicy> {
    let mut policies = HashMap::new();
    for (language, content) in EMBEDDED_LITERAL_CARRIER_POLICIES {
        let config: LanguagePolicyToml = toml::from_str(content).unwrap_or_else(|err| {
            panic!("failed to parse embedded language policy for {language}: {err}")
        });
        policies.insert(
            (*language).to_string(),
            LiteralCarrierPolicy::from_lists(config.literal_carriers),
        );
    }

    for (alias, source) in CONFIG_LANGUAGE_ALIASES {
        if let Some(policy) = policies.get(*source).cloned() {
            policies.insert((*alias).to_string(), policy);
        }
    }

    policies
}

fn lowercase_set(values: Vec<String>) -> HashSet<String> {
    values
        .into_iter()
        .map(|value| value.to_lowercase())
        .collect()
}

fn carrier_matches(set: &HashSet<String>, carrier: &str) -> bool {
    if set.contains(carrier) {
        return true;
    }

    match carrier.rsplit_once('.') {
        Some((_, last)) => set.contains(last),
        None => false,
    }
}
