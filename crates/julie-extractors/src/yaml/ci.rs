//! CI pipeline semantics for GitHub Actions, GitLab CI, and Azure Pipelines.
//!
//! The platform comes from the file path. Job dependencies (`needs`,
//! `extends`) become `References` edges and `variable_ref` identifiers between
//! job symbols of the same file. Local file references (`uses: ./...`,
//! `include: local`, `template:`) become structured pending rows whose import
//! context is the path. Jobs, triggers, and `uses:` actions also emit
//! `yaml.ci_*` structural facts.

use super::relationships::{
    pair_key, push_file_pending, push_reference, scalar_list, scalar_value, symbol_for_node,
};
use crate::base::structural_fact_builders::{base_metadata, fact_for_node, insert_string};
use crate::base::{BaseExtractor, IdentifierKind, Relationship, StructuralFact, Symbol};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use serde_json::Value;
use tree_sitter::{Node, Tree};

pub(crate) const CI_JOB_PATTERN_ID: &str = "yaml.ci_job.v1";
pub(crate) const CI_TRIGGER_PATTERN_ID: &str = "yaml.ci_trigger.v1";
pub(crate) const CI_USES_PATTERN_ID: &str = "yaml.ci_uses.v1";

const GITLAB_KEYWORDS: &[&str] = &[
    "default",
    "include",
    "stages",
    "variables",
    "workflow",
    "image",
    "services",
    "cache",
    "before_script",
    "after_script",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Platform {
    GithubWorkflow,
    GithubAction,
    Gitlab,
    Azure,
}

impl Platform {
    fn for_path(file_path: &str) -> Option<Self> {
        let path = file_path.replace('\\', "/");
        let name = path.rsplit('/').next()?;
        let is_yaml = name.ends_with(".yml") || name.ends_with(".yaml");
        if !is_yaml {
            return None;
        }
        if path.contains(".github/workflows/") {
            Some(Self::GithubWorkflow)
        } else if matches!(name, "action.yml" | "action.yaml") {
            Some(Self::GithubAction)
        } else if name.starts_with(".gitlab-ci") || path.contains(".gitlab/ci/") {
            Some(Self::Gitlab)
        } else if name.starts_with("azure-pipelines") || path.contains(".azure/pipelines/") {
            Some(Self::Azure)
        } else {
            None
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::GithubWorkflow | Self::GithubAction => "github_actions",
            Self::Gitlab => "gitlab_ci",
            Self::Azure => "azure_pipelines",
        }
    }
}

/// A job-to-job dependency: the scalar that names the job, the job it names,
/// and the job that declares it.
struct JobDependency<'tree, 'a> {
    node: Node<'tree>,
    name: String,
    from: &'a Symbol,
    to: Option<&'a Symbol>,
    keyword: &'static str,
}

fn root_symbol<'a>(symbols: &'a [Symbol], name: &str) -> Option<&'a Symbol> {
    symbols
        .iter()
        .find(|symbol| symbol.parent_id.is_none() && symbol.name == name)
}

fn children<'a>(symbols: &'a [Symbol], parent: &'a Symbol) -> impl Iterator<Item = &'a Symbol> {
    symbols
        .iter()
        .filter(move |symbol| symbol.parent_id.as_deref() == Some(parent.id.as_str()))
}

fn jobs(platform: Platform, symbols: &[Symbol]) -> Vec<&Symbol> {
    match platform {
        Platform::GithubWorkflow => root_symbol(symbols, "jobs")
            .map(|jobs| children(symbols, jobs).collect())
            .unwrap_or_default(),
        Platform::Gitlab => symbols
            .iter()
            .filter(|symbol| {
                symbol.parent_id.is_none() && !GITLAB_KEYWORDS.contains(&symbol.name.as_str())
            })
            .collect(),
        Platform::GithubAction | Platform::Azure => Vec::new(),
    }
}

fn pair_node<'tree>(tree: &'tree Tree, symbol: &Symbol) -> Option<Node<'tree>> {
    tree.root_node()
        .descendant_for_byte_range(symbol.start_byte as usize, symbol.end_byte as usize)
        .and_then(|node| {
            std::iter::successors(Some(node), |node| node.parent()).find(|node| {
                node.start_byte() == symbol.start_byte as usize
                    && node.end_byte() == symbol.end_byte as usize
            })
        })
}

fn job_dependencies<'tree, 'a>(
    content: &str,
    platform: Platform,
    tree: &'tree Tree,
    symbols: &'a [Symbol],
) -> Vec<JobDependency<'tree, 'a>> {
    let jobs = jobs(platform, symbols);
    let mut dependencies = Vec::new();
    for job in &jobs {
        for field in children(symbols, job) {
            let keyword = match (platform, field.name.as_str()) {
                (Platform::GithubWorkflow | Platform::Gitlab, "needs") => "needs",
                (Platform::Gitlab, "extends") => "extends",
                _ => continue,
            };
            let Some(value) =
                pair_node(tree, field).and_then(|pair| pair.child_by_field_name("value"))
            else {
                continue;
            };
            let mut names = scalar_list(content, value);
            if names.is_empty() {
                names = gitlab_needs_jobs(content, value);
            }
            for (node, name) in names {
                let to = jobs.iter().copied().find(|job| job.name == name);
                dependencies.push(JobDependency {
                    node,
                    name,
                    from: job,
                    to,
                    keyword,
                });
            }
        }
    }
    dependencies
}

/// GitLab `needs: [{ job: compile, artifacts: true }]` item mappings.
fn gitlab_needs_jobs<'tree>(content: &str, value: Node<'tree>) -> Vec<(Node<'tree>, String)> {
    let mut found = Vec::new();
    visit_pairs(value, 0, &mut |pair| {
        if pair_key(content, pair).as_deref() == Some("job")
            && let Some(scalar) = pair
                .child_by_field_name("value")
                .and_then(|value| scalar_value(content, value))
        {
            found.push(scalar);
        }
    });
    found
}

fn visit_pairs<'tree>(node: Node<'tree>, depth: u32, visit: &mut impl FnMut(Node<'tree>)) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if matches!(node.kind(), "block_mapping_pair" | "flow_pair") {
        visit(node);
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit_pairs(child, child_depth, visit);
    }
}

pub(super) fn extract_relationships(
    base: &mut BaseExtractor,
    tree: &Tree,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let Some(platform) = Platform::for_path(&base.file_path) else {
        return;
    };
    for dependency in job_dependencies(&base.content, platform, tree, symbols) {
        if let Some(to) = dependency.to {
            push_reference(
                base,
                dependency.from,
                to,
                &dependency.node,
                ("ciDependency", dependency.keyword),
                relationships,
            );
        }
    }
    for (pair, path, display) in local_file_references(&base.content, platform, tree) {
        let owner = symbol_for_node(symbols, pair).and_then(|symbol| {
            symbol
                .parent_id
                .as_deref()
                .and_then(|id| symbols.iter().find(|s| s.id == id))
                .or(Some(symbol))
        });
        if let Some(owner) = owner {
            push_file_pending(base, owner, &path, &display, pair);
        }
    }
}

pub(super) fn extract_identifiers(base: &mut BaseExtractor, tree: &Tree, symbols: &[Symbol]) {
    let Some(platform) = Platform::for_path(&base.file_path) else {
        return;
    };
    let sites: Vec<_> = job_dependencies(&base.content, platform, tree, symbols)
        .into_iter()
        .map(|dependency| {
            let containing = base
                .find_containing_symbol(&dependency.node, symbols)
                .map(|symbol| symbol.id.clone());
            (
                dependency.node,
                dependency.name,
                containing,
                dependency.to.map(|job| job.id.clone()),
            )
        })
        .collect();
    for (node, name, containing, target) in sites {
        base.create_identifier(&node, name, IdentifierKind::VariableRef, containing);
        if let Some(last) = base.identifiers.last_mut() {
            last.target_symbol_id = target;
        }
    }
}

/// `(pair, path, display text)` for each reference to another file in the repo.
fn local_file_references<'tree>(
    content: &str,
    platform: Platform,
    tree: &'tree Tree,
) -> Vec<(Node<'tree>, String, String)> {
    let mut found = Vec::new();
    visit_pairs(tree.root_node(), 0, &mut |pair| {
        let Some(key) = pair_key(content, pair) else {
            return;
        };
        let Some(value) = pair.child_by_field_name("value") else {
            return;
        };
        match (platform, key.as_str()) {
            (Platform::GithubWorkflow | Platform::GithubAction, "uses") => {
                if let Some((_, text)) = scalar_value(content, value)
                    && text.starts_with("./")
                {
                    found.push((pair, text.clone(), text));
                }
            }
            (Platform::Gitlab, "include") => {
                for (_, text) in scalar_list(content, value) {
                    if !text.contains("://") {
                        found.push((pair, text.clone(), text));
                    }
                }
            }
            (Platform::Gitlab, "local") => {
                if let Some((_, text)) = scalar_value(content, value) {
                    found.push((pair, text.clone(), text));
                }
            }
            (Platform::Azure, "template") => {
                if let Some((_, text)) = scalar_value(content, value) {
                    let path = text.split('@').next().unwrap_or(&text).to_string();
                    found.push((pair, path, text));
                }
            }
            _ => {}
        }
    });
    found
}

/// `yaml.ci_job.v1`, `yaml.ci_trigger.v1`, and `yaml.ci_uses.v1` facts.
pub(crate) fn ci_facts(
    tree: &Tree,
    file_path: &str,
    content: &str,
    symbols: &[Symbol],
) -> Vec<StructuralFact> {
    let Some(platform) = Platform::for_path(file_path) else {
        return Vec::new();
    };
    let mut facts = Vec::new();
    let fact = |pattern_id, capture, node, metadata| {
        fact_for_node(file_path, "yaml", pattern_id, capture, node, metadata)
    };

    let dependencies = job_dependencies(content, platform, tree, symbols);
    for job in jobs(platform, symbols) {
        let Some(node) = pair_node(tree, job) else {
            continue;
        };
        let mut metadata = base_metadata("pipeline");
        insert_string(&mut metadata, "platform", platform.label());
        insert_string(&mut metadata, "job_id", &job.name);
        let field_value = |name: &str| {
            children(symbols, job)
                .find(|field| field.name == name)
                .and_then(|field| pair_node(tree, field))
                .and_then(|pair| pair.child_by_field_name("value"))
                .and_then(|value| scalar_value(content, value))
                .map(|(_, text)| text)
        };
        for (key, field) in [("runs_on", "runs-on"), ("stage", "stage")] {
            if let Some(value) = field_value(field) {
                insert_string(&mut metadata, key, &value);
            }
        }
        let needs: Vec<Value> = dependencies
            .iter()
            .filter(|dependency| dependency.from.id == job.id && dependency.keyword == "needs")
            .map(|dependency| Value::String(dependency.name.clone()))
            .collect();
        if !needs.is_empty() {
            metadata.insert("needs".to_string(), Value::Array(needs));
        }
        facts.push(fact(CI_JOB_PATTERN_ID, "job", node, metadata));
    }

    if platform == Platform::GithubWorkflow
        && let Some(on) = root_symbol(symbols, "on").and_then(|on| pair_node(tree, on))
        && let Some(value) = on.child_by_field_name("value")
    {
        let mut events = scalar_list(content, value);
        if events.is_empty() {
            let mapping = value
                .named_child(0)
                .filter(|node| node.kind() == "block_mapping");
            if let Some(mapping) = mapping {
                let mut cursor = mapping.walk();
                events = mapping
                    .named_children(&mut cursor)
                    .filter_map(|pair| {
                        let key = pair.child_by_field_name("key")?;
                        scalar_value(content, key)
                    })
                    .collect();
            }
        }
        for (node, event) in events {
            let mut metadata = base_metadata("pipeline");
            insert_string(&mut metadata, "platform", platform.label());
            insert_string(&mut metadata, "event", &event);
            facts.push(fact(CI_TRIGGER_PATTERN_ID, "trigger", node, metadata));
        }
    }

    if matches!(platform, Platform::GithubWorkflow | Platform::GithubAction) {
        visit_pairs(tree.root_node(), 0, &mut |pair| {
            if pair_key(content, pair).as_deref() != Some("uses") {
                return;
            }
            let Some((_, uses)) = pair
                .child_by_field_name("value")
                .and_then(|value| scalar_value(content, value))
            else {
                return;
            };
            let mut metadata = base_metadata("pipeline");
            insert_string(&mut metadata, "platform", platform.label());
            insert_string(&mut metadata, "uses", &uses);
            let (target, reference) = match uses.split_once('@') {
                Some((target, reference)) => (target, Some(reference)),
                None => (uses.as_str(), None),
            };
            let kind = if uses.starts_with("docker://") {
                "docker"
            } else if uses.starts_with("./") {
                "local"
            } else if target.contains("/.github/workflows/") {
                "reusable_workflow"
            } else {
                "action"
            };
            insert_string(&mut metadata, "kind", kind);
            if matches!(kind, "action" | "reusable_workflow") {
                insert_string(&mut metadata, "action", target);
            }
            if let Some(reference) = reference {
                insert_string(&mut metadata, "ref", reference);
            }
            facts.push(fact(CI_USES_PATTERN_ID, "uses", pair, metadata));
        });
    }
    facts
}
