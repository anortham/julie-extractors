#[cfg(test)]
use std::cell::Cell;
use std::collections::{HashMap, HashSet};

use tree_sitter::Node;

use super::helpers::{child_named_kinds, find_child_by_type, named_children};
use super::{ErlangExtractor, type_facts};
use crate::base::IdentifierKind;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

struct VariableOccurrence<'tree> {
    node: Node<'tree>,
    name: String,
    access: &'static str,
}

#[derive(Clone, Copy)]
enum PatternBindingMode {
    Seed,
    Match,
    Comprehension,
}

impl PatternBindingMode {
    fn emits_bindings(self) -> bool {
        matches!(self, Self::Match | Self::Comprehension)
    }

    fn shadows_outer_bindings(self) -> bool {
        matches!(self, Self::Comprehension)
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct VariableAccessWork {
    pub(crate) traversal_node_visits: usize,
    pub(crate) scope_lookup_operations: usize,
    pub(crate) scope_frame_probes: usize,
    pub(crate) scope_bind_operations: usize,
    pub(crate) scope_hash_operations: usize,
    pub(crate) branch_join_items: usize,
    pub(crate) scope_frame_operations: usize,
    pub(crate) rollback_bindings: usize,
}

#[cfg(test)]
impl VariableAccessWork {
    pub(crate) fn total_work(self) -> usize {
        self.traversal_node_visits
            + self.scope_frame_probes
            + self.scope_hash_operations
            + self.branch_join_items
            + self.scope_frame_operations
            + self.rollback_bindings
    }
}

#[cfg(test)]
thread_local! {
    static VARIABLE_ACCESS_WORK: Cell<VariableAccessWork> = Cell::new(VariableAccessWork::default());
}

#[cfg(test)]
fn record_work(update: impl FnOnce(&mut VariableAccessWork)) {
    VARIABLE_ACCESS_WORK.with(|work| {
        let mut current = work.get();
        update(&mut current);
        work.set(current);
    });
}

macro_rules! track_work {
    ($field:ident, $amount:expr) => {
        #[cfg(test)]
        {
            record_work(|work| work.$field += $amount);
        }
    };
}

#[cfg(test)]
impl ErlangExtractor {
    pub(crate) fn reset_variable_access_work() {
        VARIABLE_ACCESS_WORK.with(|work| work.set(VariableAccessWork::default()));
    }

    pub(crate) fn take_variable_access_work() -> VariableAccessWork {
        VARIABLE_ACCESS_WORK.with(|work| work.replace(VariableAccessWork::default()))
    }
}

#[derive(Default)]
struct VariableFrame {
    bound: HashSet<String>,
    trail: Vec<String>,
}

struct VariableCheckpoint {
    frame_index: usize,
    trail_len: usize,
}

struct VariableScope {
    frames: Vec<VariableFrame>,
    bound_counts: HashMap<String, usize>,
}

impl Default for VariableScope {
    fn default() -> Self {
        Self {
            frames: vec![VariableFrame::default()],
            bound_counts: HashMap::new(),
        }
    }
}

impl VariableScope {
    fn bind(&mut self, name: String) -> bool {
        track_work!(scope_bind_operations, 1);
        let Some(frame) = self.frames.last_mut() else {
            return false;
        };
        track_work!(scope_hash_operations, 1);
        if !frame.bound.insert(name.clone()) {
            return false;
        }
        frame.trail.push(name.clone());
        track_work!(scope_hash_operations, 1);
        *self.bound_counts.entry(name).or_default() += 1;
        true
    }

    fn contains(&self, name: &str) -> bool {
        track_work!(scope_lookup_operations, 1);
        track_work!(scope_hash_operations, 1);
        self.bound_counts.get(name).is_some_and(|count| *count > 0)
    }

    fn current_contains(&self, name: &str) -> bool {
        track_work!(scope_lookup_operations, 1);
        let Some(frame) = self.frames.last() else {
            return false;
        };
        track_work!(scope_frame_probes, 1);
        track_work!(scope_hash_operations, 1);
        frame.bound.contains(name)
    }

    fn checkpoint(&self) -> VariableCheckpoint {
        VariableCheckpoint {
            frame_index: self.frames.len().saturating_sub(1),
            trail_len: self.frames.last().map_or(0, |frame| frame.trail.len()),
        }
    }

    fn added_since(&self, checkpoint: &VariableCheckpoint) -> &[String] {
        &self.frames[checkpoint.frame_index].trail[checkpoint.trail_len..]
    }

    fn rollback(&mut self, checkpoint: VariableCheckpoint) {
        let Some(frame) = self.frames.get_mut(checkpoint.frame_index) else {
            return;
        };
        while frame.trail.len() > checkpoint.trail_len {
            if let Some(name) = frame.trail.pop() {
                track_work!(rollback_bindings, 1);
                track_work!(scope_hash_operations, 1);
                frame.bound.remove(&name);
                track_work!(scope_hash_operations, 1);
                if let Some(count) = self.bound_counts.get_mut(&name) {
                    *count = count.saturating_sub(1);
                }
            }
        }
    }

    fn push_scope(&mut self) {
        track_work!(scope_frame_operations, 1);
        self.frames.push(VariableFrame::default());
    }

    fn pop_scope(&mut self) {
        if self.frames.len() > 1 {
            track_work!(scope_frame_operations, 1);
            if let Some(frame) = self.frames.pop() {
                for name in frame.trail {
                    track_work!(scope_hash_operations, 1);
                    if let Some(count) = self.bound_counts.get_mut(&name) {
                        *count = count.saturating_sub(1);
                    }
                }
            }
        }
    }

    fn frame_count(&self) -> usize {
        self.frames.len()
    }
}

pub(super) fn emit_variable_reads(
    extractor: &mut ErlangExtractor,
    declaration: Node,
    scope: Option<&str>,
    end: usize,
) {
    let Some(clause) = find_child_by_type(&declaration, "function_clause") else {
        return;
    };
    let mut variable_scope = VariableScope::default();
    let mut variables = Vec::new();
    if let Some(arguments) = clause.child_by_field_name("args") {
        collect_pattern_variables(
            extractor,
            arguments,
            end,
            0,
            &mut variable_scope,
            PatternBindingMode::Seed,
            &mut variables,
        );
    }
    for node in [
        clause.child_by_field_name("guard"),
        clause.child_by_field_name("body"),
    ]
    .into_iter()
    .flatten()
    {
        collect_variable_occurrences(extractor, node, end, 0, &mut variable_scope, &mut variables);
    }
    variables.sort_by_key(|occurrence| occurrence.node.start_byte());
    for occurrence in variables {
        extractor.base.create_identifier_with_metadata(
            &occurrence.node,
            occurrence.name,
            IdentifierKind::VariableRef,
            scope.map(String::from),
            HashMap::from([(
                "access".to_string(),
                serde_json::Value::String(occurrence.access.to_string()),
            )]),
        );
    }
}

fn collect_variable_occurrences<'tree>(
    extractor: &ErlangExtractor,
    node: Node<'tree>,
    end: usize,
    depth: u32,
    scope: &mut VariableScope,
    variables: &mut Vec<VariableOccurrence<'tree>>,
) {
    track_work!(traversal_node_visits, 1);
    if !should_visit_tree_depth(depth) || node.start_byte() >= end {
        return;
    }
    if node.kind() == "var" {
        let name = extractor.base.get_node_text(&node);
        if name != "_" && !is_syntactic_variable_name(node) {
            variables.push(VariableOccurrence {
                node,
                name,
                access: "read",
            });
        }
        return;
    }
    if matches!(node.kind(), "match_expr" | "cond_match_expr") {
        if let Some(rhs) = node.child_by_field_name("rhs") {
            collect_variable_occurrences(extractor, rhs, end, depth + 1, scope, variables);
        }
        if let Some(lhs) = node.child_by_field_name("lhs") {
            collect_pattern_variables(
                extractor,
                lhs,
                end,
                depth + 1,
                scope,
                PatternBindingMode::Match,
                variables,
            );
        }
        return;
    }
    if node.kind() == "case_expr" {
        if let Some(expression) = node.child_by_field_name("expr") {
            collect_variable_occurrences(extractor, expression, end, depth + 1, scope, variables);
        }
        let branches: Vec<_> = named_children(&node)
            .into_iter()
            .filter(|child| {
                node.child_by_field_name("expr")
                    .is_none_or(|expression| expression.id() != child.id())
            })
            .collect();
        collect_variable_alternatives(extractor, &branches, end, depth + 1, scope, variables);
        return;
    }
    if matches!(
        node.kind(),
        "list_comprehension" | "binary_comprehension" | "map_comprehension"
    ) {
        let outer_frame_count = scope.frame_count();
        scope.push_scope();
        if let Some(qualifiers) = node.child_by_field_name("lc_exprs") {
            collect_comprehension_qualifiers(
                extractor,
                qualifiers,
                end,
                depth + 1,
                scope,
                variables,
            );
        }
        for field in ["exprs", "expr"] {
            let mut cursor = node.walk();
            for expression in node.children_by_field_name(field, &mut cursor) {
                collect_variable_occurrences(
                    extractor,
                    expression,
                    end,
                    depth + 1,
                    scope,
                    variables,
                );
            }
        }
        while scope.frame_count() > outer_frame_count {
            scope.pop_scope();
        }
        return;
    }
    if node.kind() == "anonymous_fun" {
        for clause in child_named_kinds(&node, "fun_clause") {
            scope.push_scope();
            collect_variable_clause(
                extractor,
                clause,
                end,
                depth + 1,
                scope,
                PatternBindingMode::Seed,
                variables,
            );
            scope.pop_scope();
        }
        return;
    }
    if matches!(node.kind(), "cr_clause" | "catch_clause") {
        collect_variable_clause(
            extractor,
            node,
            end,
            depth,
            scope,
            PatternBindingMode::Match,
            variables,
        );
        return;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    for child in named_children(&node) {
        collect_variable_occurrences(extractor, child, end, child_depth, scope, variables);
    }
}

fn collect_comprehension_qualifiers<'tree>(
    extractor: &ErlangExtractor,
    node: Node<'tree>,
    end: usize,
    depth: u32,
    scope: &mut VariableScope,
    variables: &mut Vec<VariableOccurrence<'tree>>,
) {
    track_work!(traversal_node_visits, 1);
    if matches!(node.kind(), "generator" | "b_generator" | "map_generator") {
        if let Some(rhs) = node.child_by_field_name("rhs") {
            collect_variable_occurrences(extractor, rhs, end, depth + 1, scope, variables);
        }
        if let Some(lhs) = node.child_by_field_name("lhs") {
            scope.push_scope();
            collect_pattern_variables(
                extractor,
                lhs,
                end,
                depth + 1,
                scope,
                PatternBindingMode::Comprehension,
                variables,
            );
        }
        return;
    }
    if matches!(node.kind(), "lc_exprs" | "lc_or_zc_expr") {
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        for child in named_children(&node) {
            collect_comprehension_qualifiers(extractor, child, end, child_depth, scope, variables);
        }
        return;
    }
    collect_variable_occurrences(extractor, node, end, depth, scope, variables);
}

fn collect_variable_clause<'tree>(
    extractor: &ErlangExtractor,
    node: Node<'tree>,
    end: usize,
    depth: u32,
    scope: &mut VariableScope,
    binding_mode: PatternBindingMode,
    variables: &mut Vec<VariableOccurrence<'tree>>,
) {
    track_work!(traversal_node_visits, 1);
    let fields = type_facts::pattern_fields(node.kind());
    let mut patterns = HashSet::new();
    for field in fields {
        let mut cursor = node.walk();
        for pattern in node.children_by_field_name(field, &mut cursor) {
            patterns.insert(pattern.id());
            collect_pattern_variables(
                extractor,
                pattern,
                end,
                depth + 1,
                scope,
                binding_mode,
                variables,
            );
        }
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    for child in named_children(&node) {
        if !patterns.contains(&child.id()) {
            collect_variable_occurrences(extractor, child, end, child_depth, scope, variables);
        }
    }
}

fn collect_variable_alternatives<'tree>(
    extractor: &ErlangExtractor,
    branches: &[Node<'tree>],
    end: usize,
    depth: u32,
    scope: &mut VariableScope,
    variables: &mut Vec<VariableOccurrence<'tree>>,
) {
    let mut joined: Option<HashSet<String>> = None;
    for branch in branches {
        let checkpoint = scope.checkpoint();
        collect_variable_occurrences(extractor, *branch, end, depth, scope, variables);
        let mut branch_bindings = HashSet::new();
        for name in scope.added_since(&checkpoint) {
            track_work!(branch_join_items, 1);
            track_work!(scope_hash_operations, 1);
            branch_bindings.insert(name.clone());
        }
        scope.rollback(checkpoint);
        if let Some(joined) = &mut joined {
            joined.retain(|name| {
                track_work!(branch_join_items, 1);
                track_work!(scope_hash_operations, 1);
                branch_bindings.contains(name)
            });
        } else {
            joined = Some(branch_bindings);
        }
    }
    if let Some(joined) = joined {
        for name in joined {
            scope.bind(name);
        }
    }
}

fn collect_pattern_variables<'tree>(
    extractor: &ErlangExtractor,
    node: Node<'tree>,
    end: usize,
    depth: u32,
    scope: &mut VariableScope,
    binding_mode: PatternBindingMode,
    variables: &mut Vec<VariableOccurrence<'tree>>,
) {
    track_work!(traversal_node_visits, 1);
    if !should_visit_tree_depth(depth) || node.start_byte() >= end {
        return;
    }
    if node.kind() == "var" {
        let name = extractor.base.get_node_text(&node);
        if name == "_" || is_syntactic_variable_name(node) {
            return;
        }
        let already_bound = if binding_mode.shadows_outer_bindings() {
            scope.current_contains(&name)
        } else {
            scope.contains(&name)
        };
        let access = if already_bound {
            Some("read")
        } else {
            scope.bind(name.clone());
            binding_mode.emits_bindings().then_some("write")
        };
        if let Some(access) = access {
            variables.push(VariableOccurrence { node, name, access });
        }
        return;
    }
    if node.kind() == "macro_call_expr" {
        collect_variable_occurrences(extractor, node, end, depth, scope, variables);
        return;
    }
    if node.kind() == "map_field" {
        if let Some(key) = node.child_by_field_name("key") {
            collect_variable_occurrences(extractor, key, end, depth + 1, scope, variables);
        }
        if let Some(value) = node.child_by_field_name("value") {
            collect_pattern_variables(
                extractor,
                value,
                end,
                depth + 1,
                scope,
                binding_mode,
                variables,
            );
        }
        return;
    }
    if node.kind() == "bin_element" {
        if let Some(element) = node.child_by_field_name("element") {
            collect_pattern_variables(
                extractor,
                element,
                end,
                depth + 1,
                scope,
                binding_mode,
                variables,
            );
        }
        if let Some(size) = node.child_by_field_name("size") {
            collect_variable_occurrences(extractor, size, end, depth + 1, scope, variables);
        }
        return;
    }
    if node.kind() == "record_field" {
        if let Some(value) = node.child_by_field_name("expr") {
            collect_pattern_variables(
                extractor,
                value,
                end,
                depth + 1,
                scope,
                binding_mode,
                variables,
            );
        }
        return;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    for child in named_children(&node) {
        collect_pattern_variables(
            extractor,
            child,
            end,
            child_depth,
            scope,
            binding_mode,
            variables,
        );
    }
}

fn is_syntactic_variable_name(node: Node) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    let name_field = parent
        .child_by_field_name("name")
        .is_some_and(|name| name.id() == node.id());
    name_field
        && matches!(
            parent.kind(),
            "macro_call_expr"
                | "record_field"
                | "record_field_name"
                | "record_name"
                | "qualified_record_name"
        )
}
