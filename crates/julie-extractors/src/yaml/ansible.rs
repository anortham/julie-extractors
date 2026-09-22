//! Ansible playbooks and task files.
//!
//! A playbook is a document whose root is a sequence of plays, one of which
//! holds `hosts` or `import_playbook`. A task file is a root sequence of
//! mappings under a `tasks/` or `handlers/` directory.
//!
//! - Each task is a `yaml.ansible_task.v1` fact naming its module (the first
//!   key that is not a task keyword, with its FQCN as written).
//! - `import_playbook`, `include_tasks`, `import_tasks`, `include_vars`, and
//!   role references (`roles:` entries, `include_role`, `import_role`) are
//!   structured pending rows to the file they load.
//! - `notify` names a handler by `name` or `listen`: a `References` edge to
//!   the handler item in the same file.

use super::innermost_symbol;
use super::relationships::{
    document_roots, field, mapping_pairs, pair_key, push_file_pending, push_reference, scalar_list,
    scalar_value, sequence_items, symbol_for_node,
};
use crate::base::structural_fact_builders::{base_metadata, fact_for_node, insert_string};
use crate::base::{BaseExtractor, Relationship, StructuralFact, Symbol};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::{Node, Tree};

pub(crate) const ANSIBLE_TASK_PATTERN_ID: &str = "yaml.ansible_task.v1";

const TASK_KEYWORDS: &[&str] = &[
    "name",
    "when",
    "loop",
    "loop_control",
    "register",
    "notify",
    "listen",
    "become",
    "become_user",
    "become_method",
    "tags",
    "vars",
    "args",
    "ignore_errors",
    "ignore_unreachable",
    "changed_when",
    "failed_when",
    "delegate_to",
    "delegate_facts",
    "run_once",
    "no_log",
    "environment",
    "retries",
    "until",
    "delay",
    "check_mode",
    "diff",
    "any_errors_fatal",
    "throttle",
    "timeout",
    "collections",
    "module_defaults",
    "debugger",
    "connection",
    "remote_user",
    "block",
    "rescue",
    "always",
];

const TASK_LISTS: &[&str] = &["tasks", "pre_tasks", "post_tasks", "handlers"];

/// The short name of a module key: `ansible.builtin.include_tasks` -> `include_tasks`.
fn short_module(module: &str) -> &str {
    module
        .strip_prefix("ansible.builtin.")
        .or_else(|| module.strip_prefix("ansible.legacy."))
        .unwrap_or(module)
}

struct Task<'tree> {
    item: Node<'tree>,
    value: Node<'tree>,
    module: Option<(String, Node<'tree>)>,
    in_handlers: bool,
}

struct Playbook<'tree> {
    plays: Vec<Node<'tree>>,
    tasks: Vec<Task<'tree>>,
}

fn playbook<'tree>(content: &str, tree: &'tree Tree, file_path: &str) -> Option<Playbook<'tree>> {
    let path = file_path.replace('\\', "/");
    let in_task_dir = path.contains("tasks/") || path.contains("handlers/");
    let is_handlers = path.contains("handlers/");
    for root in document_roots(tree) {
        let items: Vec<Node> = sequence_items(root)
            .into_iter()
            .filter(|item| !mapping_pairs(*item).is_empty())
            .collect();
        if items.is_empty() {
            continue;
        }
        let is_play = |item: &Node| {
            mapping_pairs(*item).iter().any(|pair| {
                pair_key(content, *pair)
                    .is_some_and(|key| key == "hosts" || short_module(&key) == "import_playbook")
            })
        };
        if items.iter().any(is_play) {
            let mut tasks = Vec::new();
            for play in &items {
                for pair in mapping_pairs(*play) {
                    let Some(key) = pair_key(content, pair) else {
                        continue;
                    };
                    if TASK_LISTS.contains(&key.as_str())
                        && let Some(value) = pair.child_by_field_name("value")
                    {
                        collect_tasks(content, value, key == "handlers", &mut tasks, 0);
                    }
                }
            }
            return Some(Playbook {
                plays: items,
                tasks,
            });
        }
        if in_task_dir {
            let mut tasks = Vec::new();
            collect_tasks(content, root, is_handlers, &mut tasks, 0);
            return Some(Playbook {
                plays: Vec::new(),
                tasks,
            });
        }
    }
    None
}

fn collect_tasks<'tree>(
    content: &str,
    list: Node<'tree>,
    in_handlers: bool,
    tasks: &mut Vec<Task<'tree>>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    for value in sequence_items(list) {
        let pairs = mapping_pairs(value);
        if pairs.is_empty() {
            continue;
        }
        let item = value
            .parent()
            .filter(|parent| parent.kind() == "block_sequence_item")
            .unwrap_or(value);
        let mut module = None;
        for pair in &pairs {
            let Some(key) = pair_key(content, *pair) else {
                continue;
            };
            if matches!(key.as_str(), "block" | "rescue" | "always")
                && let Some(nested) = pair.child_by_field_name("value")
            {
                collect_tasks(content, nested, in_handlers, tasks, child_depth);
            }
            if module.is_none()
                && !TASK_KEYWORDS.contains(&key.as_str())
                && !key.starts_with("with_")
            {
                module = Some((key, *pair));
            }
        }
        tasks.push(Task {
            item,
            value,
            module,
            in_handlers,
        });
    }
}

pub(crate) fn ansible_facts(tree: &Tree, file_path: &str, content: &str) -> Vec<StructuralFact> {
    let Some(playbook) = playbook(content, tree, file_path) else {
        return Vec::new();
    };
    playbook
        .tasks
        .iter()
        .filter_map(|task| {
            let (module, _) = task.module.as_ref()?;
            let mut metadata = base_metadata("pipeline");
            insert_string(&mut metadata, "module", module);
            if let Some((_, name)) =
                field(content, task.value, "name").and_then(|node| scalar_value(content, node))
            {
                insert_string(&mut metadata, "name", &name);
            }
            metadata.insert(
                "handler".to_string(),
                serde_json::Value::Bool(task.in_handlers),
            );
            Some(fact_for_node(
                file_path,
                "yaml",
                ANSIBLE_TASK_PATTERN_ID,
                "task",
                task.item,
                metadata,
            ))
        })
        .collect()
}

pub(super) fn extract_relationships(
    base: &mut BaseExtractor,
    tree: &Tree,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let content = base.content.clone();
    let Some(playbook) = playbook(&content, tree, &base.file_path) else {
        return;
    };
    let mut files: Vec<(Node, String)> = Vec::new();
    for play in &playbook.plays {
        for pair in mapping_pairs(*play) {
            let (Some(key), Some(value)) =
                (pair_key(&content, pair), pair.child_by_field_name("value"))
            else {
                continue;
            };
            if short_module(&key) == "import_playbook"
                && let Some(site) = scalar_value(&content, value)
            {
                files.push(site);
            }
            if key == "roles" {
                for item in sequence_items(value) {
                    let role = scalar_value(&content, item).or_else(|| {
                        field(&content, item, "role")
                            .or_else(|| field(&content, item, "name"))
                            .and_then(|node| scalar_value(&content, node))
                    });
                    if let Some((node, role)) = role {
                        files.push((node, role_tasks_path(&role)));
                    }
                }
            }
        }
    }
    let mut handlers: Vec<(String, &Symbol)> = Vec::new();
    let mut notifies: Vec<(Node, String)> = Vec::new();
    for task in &playbook.tasks {
        let value = task.value;
        if task.in_handlers
            && let Some(symbol) = symbol_for_node(symbols, task.item)
        {
            for key in ["name", "listen"] {
                if let Some(names) = field(&content, value, key) {
                    handlers.extend(
                        scalar_list(&content, names)
                            .into_iter()
                            .map(|(_, name)| (name, symbol)),
                    );
                }
            }
        }
        if let Some(notify) = field(&content, value, "notify") {
            notifies.extend(scalar_list(&content, notify));
        }
        let Some((module, pair)) = &task.module else {
            continue;
        };
        let Some(argument) = pair.child_by_field_name("value") else {
            continue;
        };
        match short_module(module) {
            "import_playbook" | "include_tasks" | "import_tasks" | "include_vars" => {
                let site = scalar_value(&content, argument).or_else(|| {
                    field(&content, argument, "file").and_then(|node| scalar_value(&content, node))
                });
                files.extend(site);
            }
            "include_role" | "import_role" => {
                if let Some((node, role)) =
                    field(&content, argument, "name").and_then(|node| scalar_value(&content, node))
                {
                    files.push((node, role_tasks_path(&role)));
                }
            }
            _ => {}
        }
    }
    for (site, name) in notifies {
        let target = handlers
            .iter()
            .find(|(handler, _)| *handler == name)
            .map(|(_, symbol)| *symbol);
        if let (Some(from), Some(to)) = (innermost_symbol(symbols, site), target) {
            push_reference(
                base,
                from,
                to,
                &site,
                ("ansibleNotify", "handler"),
                relationships,
            );
        }
    }
    for (site, path) in files {
        if let Some(from) = innermost_symbol(symbols, site) {
            let from = from.clone();
            push_file_pending(base, &from, &path, &path, site);
        }
    }
}

fn role_tasks_path(role: &str) -> String {
    format!("roles/{role}/tasks/main.yml")
}
