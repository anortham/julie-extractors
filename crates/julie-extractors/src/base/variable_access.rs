use tree_sitter::Node;

use super::types::Symbol;

mod dynamic_systems;
mod native_jvm;
mod wave2a;
mod wave2b;
mod web;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Access {
    Read,
    Write,
    ReadWrite,
}

impl Access {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::ReadWrite => "read_write",
        }
    }
}

#[derive(Clone, Copy)]
/// Select a named field, a direct child kind, or the whole parent when both are absent.
pub(crate) struct ChildSelector {
    pub(crate) field: Option<&'static str>,
    pub(crate) kind: Option<&'static str>,
}

#[derive(Clone, Copy)]
pub(crate) struct FieldRule {
    pub(crate) parent_kind: &'static str,
    pub(crate) child: ChildSelector,
}

#[derive(Clone, Copy)]
pub(crate) struct ContextRule {
    pub(crate) parent_kind: &'static str,
    pub(crate) child: ChildSelector,
    pub(crate) access: Access,
    pub(crate) predicate: Option<ContextPredicate>,
    pub(crate) required_child: Option<ChildSelector>,
}

pub(crate) type ContextPredicate =
    for<'tree, 'source> fn(Node<'tree>, Node<'tree>, &'source str) -> bool;

#[derive(Clone, Copy)]
pub(crate) struct AssignmentRule {
    pub(crate) parent_kind: &'static str,
    pub(crate) required_parent_kind: Option<&'static str>,
    pub(crate) target: ChildSelector,
    pub(crate) operator: Option<ChildSelector>,
    pub(crate) write_operators: &'static [&'static str],
    pub(crate) read_write_operators: &'static [&'static str],
}

#[derive(Clone, Copy)]
pub(crate) struct DeclarationRule {
    pub(crate) parent_kind: &'static str,
    pub(crate) name: ChildSelector,
    pub(crate) initializer: ChildSelector,
}

#[derive(Clone, Copy)]
pub(crate) struct UnclassifiedRule {
    pub(crate) parent_kind: &'static str,
    pub(crate) child: ChildSelector,
    pub(crate) predicate: Option<ContextPredicate>,
    pub(crate) required_child: Option<ChildSelector>,
}

#[derive(Clone, Copy)]
pub(crate) struct VariableAccessRules {
    pub(crate) assignments: &'static [AssignmentRule],
    pub(crate) declarations: &'static [DeclarationRule],
    pub(crate) patterns: &'static [ContextRule],
    pub(crate) increments: &'static [ContextRule],
    pub(crate) by_reference: &'static [ContextRule],
    pub(crate) receivers: &'static [FieldRule],
    pub(crate) read_overrides: &'static [ContextRule],
    pub(crate) unclassified: &'static [UnclassifiedRule],
}

pub(crate) fn classify(
    node: Node<'_>,
    source: &str,
    rules: &VariableAccessRules,
) -> Option<Access> {
    let mut receiver = false;
    let mut ancestor = node.parent();

    while let Some(parent) = ancestor {
        receiver |= rules.receivers.iter().any(|rule| {
            parent.kind() == rule.parent_kind && selected_descendant(parent, rule.child, node)
        });

        if let Some(rule) = matching_context(rules.read_overrides, parent, node, source) {
            return Some(rule.access);
        }

        if let Some(access) = matching_assignment(rules.assignments, parent, node, source) {
            return Some(write_access(access, receiver));
        }

        if matches_declaration(rules.declarations, parent, node) {
            return Some(write_access(Access::Write, receiver));
        }

        if let Some(rule) = matching_context(rules.patterns, parent, node, source) {
            return Some(write_access(rule.access, receiver));
        }
        if let Some(rule) = matching_context(rules.increments, parent, node, source) {
            return Some(write_access(rule.access, receiver));
        }
        if let Some(rule) = matching_context(rules.by_reference, parent, node, source) {
            return Some(write_access(rule.access, receiver));
        }
        if matches_unclassified(rules.unclassified, parent, node, source) {
            return None;
        }

        ancestor = parent.parent();
    }

    Some(Access::Read)
}

pub(crate) fn is_assignment_or_increment_target(
    node: Node<'_>,
    source: &str,
    rules: &VariableAccessRules,
) -> bool {
    let mut receiver = false;
    let mut ancestor = node.parent();

    while let Some(parent) = ancestor {
        receiver |= rules.receivers.iter().any(|rule| {
            parent.kind() == rule.parent_kind && selected_descendant(parent, rule.child, node)
        });

        if matching_context(rules.read_overrides, parent, node, source).is_some() {
            return false;
        }
        if matches_declaration(rules.declarations, parent, node)
            || matching_context(rules.patterns, parent, node, source).is_some()
        {
            return false;
        }
        if let Some(access) = matching_assignment(rules.assignments, parent, node, source) {
            return access != Access::Read && !receiver;
        }
        if let Some(rule) = matching_context(rules.increments, parent, node, source) {
            return rule.access != Access::Read && !receiver;
        }
        if matching_context(rules.by_reference, parent, node, source).is_some() {
            return false;
        }
        if matches_unclassified(rules.unclassified, parent, node, source) {
            return false;
        }

        ancestor = parent.parent();
    }

    false
}

pub(crate) fn has_prior_binding(
    symbols: &[Symbol],
    owner: Option<&str>,
    name: &str,
    node: Node<'_>,
) -> bool {
    symbols.iter().any(|symbol| {
        symbol.name == name
            && symbol.parent_id.as_deref() == owner
            && symbol.end_byte <= node.start_byte() as u32
    })
}

pub(crate) fn should_emit_write_reference(
    node: Node<'_>,
    name: &str,
    source: &str,
    rules: &VariableAccessRules,
    symbols: &[Symbol],
) -> bool {
    let pattern_binding = is_pattern_binding(node, source, rules);
    let assignment_or_increment = is_assignment_or_increment_target(node, source, rules);
    if !pattern_binding && !assignment_or_increment {
        return false;
    }
    let Some(site) = matching_write_context(node, source, rules) else {
        return false;
    };
    let represented = binding_symbol_covering(symbols, name, node, site);
    if pattern_binding {
        return represented.is_none();
    }
    let Some(binding) = represented else {
        return true;
    };
    has_prior_binding(symbols, binding.parent_id.as_deref(), name, node)
}

pub(crate) fn is_explicit_read_reference(
    node: Node<'_>,
    source: &str,
    rules: &VariableAccessRules,
) -> bool {
    let mut receiver = false;
    let mut explicit_read = false;
    let mut ancestor = node.parent();

    while let Some(parent) = ancestor {
        receiver |= rules.receivers.iter().any(|rule| {
            parent.kind() == rule.parent_kind && selected_descendant(parent, rule.child, node)
        });

        if matches_declaration(rules.declarations, parent, node) {
            return false;
        }
        if let Some(rule) = matching_context(rules.read_overrides, parent, node, source) {
            if rule.access != Access::Read {
                return false;
            }
            explicit_read = true;
        }
        if let Some(access) = matching_assignment(rules.assignments, parent, node, source) {
            return explicit_read || (receiver && write_access(access, receiver) == Access::Read);
        }
        if let Some(rule) = matching_context(rules.patterns, parent, node, source) {
            return explicit_read
                || (receiver && write_access(rule.access, receiver) == Access::Read);
        }
        if let Some(rule) = matching_context(rules.increments, parent, node, source) {
            return explicit_read
                || (receiver && write_access(rule.access, receiver) == Access::Read);
        }
        if let Some(rule) = matching_context(rules.by_reference, parent, node, source) {
            return explicit_read
                || (receiver && write_access(rule.access, receiver) == Access::Read);
        }
        if matches_unclassified(rules.unclassified, parent, node, source) {
            return false;
        }

        ancestor = parent.parent();
    }

    false
}

fn binding_symbol_covering<'a>(
    symbols: &'a [Symbol],
    name: &str,
    node: Node<'_>,
    site: Node<'_>,
) -> Option<&'a Symbol> {
    symbols
        .iter()
        .filter(|symbol| {
            symbol.name == name
                && symbol.start_byte <= node.start_byte() as u32
                && node.end_byte() as u32 <= symbol.end_byte
                && site.start_byte() as u32 <= symbol.start_byte
                && symbol.end_byte <= site.end_byte() as u32
                && symbol
                    .body_span
                    .is_none_or(|body| node.end_byte() as u32 <= body.start_byte)
        })
        .min_by_key(|symbol| symbol.end_byte - symbol.start_byte)
}

fn matching_write_context<'tree>(
    node: Node<'tree>,
    source: &str,
    rules: &VariableAccessRules,
) -> Option<Node<'tree>> {
    let mut ancestor = node.parent();
    while let Some(parent) = ancestor {
        if matching_context(rules.read_overrides, parent, node, source).is_some()
            || matches_declaration(rules.declarations, parent, node)
        {
            return None;
        }
        if matching_assignment(rules.assignments, parent, node, source).is_some()
            || matching_context(rules.patterns, parent, node, source).is_some()
            || matching_context(rules.increments, parent, node, source).is_some()
        {
            return Some(parent);
        }
        if matching_context(rules.by_reference, parent, node, source).is_some()
            || matches_unclassified(rules.unclassified, parent, node, source)
        {
            return None;
        }
        ancestor = parent.parent();
    }
    None
}

pub(crate) fn is_pattern_binding(
    node: Node<'_>,
    source: &str,
    rules: &VariableAccessRules,
) -> bool {
    let mut receiver = false;
    let mut ancestor = node.parent();
    while let Some(parent) = ancestor {
        receiver |= rules.receivers.iter().any(|rule| {
            parent.kind() == rule.parent_kind && selected_descendant(parent, rule.child, node)
        });
        if matching_context(rules.read_overrides, parent, node, source).is_some() {
            return false;
        }
        if matches_declaration(rules.declarations, parent, node) {
            return false;
        }
        if matching_context(rules.patterns, parent, node, source).is_some() {
            return !receiver;
        }
        if matches_unclassified(rules.unclassified, parent, node, source) {
            return false;
        }
        ancestor = parent.parent();
    }
    false
}

fn matching_context<'a>(
    rules: &'a [ContextRule],
    parent: Node<'_>,
    node: Node<'_>,
    source: &str,
) -> Option<&'a ContextRule> {
    rules.iter().find(|rule| {
        parent.kind() == rule.parent_kind
            && selected_descendant(parent, rule.child, node)
            && rule
                .required_child
                .is_none_or(|selector| selected_child(parent, selector).is_some())
            && rule
                .predicate
                .is_none_or(|predicate| predicate(node, parent, source))
    })
}

fn matching_assignment(
    rules: &[AssignmentRule],
    parent: Node<'_>,
    node: Node<'_>,
    source: &str,
) -> Option<Access> {
    rules.iter().find_map(|rule| {
        if parent.kind() != rule.parent_kind
            || !selected_first_descendant(parent, rule.target, node)
            || rule
                .required_parent_kind
                .is_some_and(|kind| parent.parent().is_none_or(|value| value.kind() != kind))
        {
            return None;
        }
        assignment_access(parent, rule, source)
    })
}

fn matches_declaration(rules: &[DeclarationRule], parent: Node<'_>, node: Node<'_>) -> bool {
    rules.iter().any(|rule| {
        parent.kind() == rule.parent_kind
            && selected_first_descendant(parent, rule.name, node)
            && selected_child(parent, rule.initializer).is_some()
    })
}

fn matches_unclassified(
    rules: &[UnclassifiedRule],
    parent: Node<'_>,
    node: Node<'_>,
    source: &str,
) -> bool {
    rules.iter().any(|rule| {
        parent.kind() == rule.parent_kind
            && selected_descendant(parent, rule.child, node)
            && rule
                .required_child
                .is_none_or(|selector| selected_child(parent, selector).is_some())
            && rule
                .predicate
                .is_none_or(|predicate| predicate(node, parent, source))
    })
}

fn assignment_access(parent: Node<'_>, rule: &AssignmentRule, source: &str) -> Option<Access> {
    let operator = rule
        .operator
        .and_then(|selector| selected_child(parent, selector))
        .and_then(|node| node.utf8_text(source.as_bytes()).ok())
        .map(str::trim);
    let is_read_write =
        operator.is_some_and(|operator| rule.read_write_operators.contains(&operator));
    if !rule.write_operators.is_empty()
        && operator
            .is_none_or(|operator| !rule.write_operators.contains(&operator) && !is_read_write)
    {
        return None;
    }
    Some(if is_read_write {
        Access::ReadWrite
    } else {
        Access::Write
    })
}

fn write_access(access: Access, receiver: bool) -> Access {
    if receiver && access != Access::Read {
        Access::Read
    } else {
        access
    }
}

fn selected_descendant(parent: Node<'_>, selector: ChildSelector, node: Node<'_>) -> bool {
    let contains = |child: Node<'_>| {
        child.start_byte() <= node.start_byte() && node.end_byte() <= child.end_byte()
    };
    if let Some(field) = selector.field
        && parent.child_by_field_name(field).is_some_and(contains)
    {
        return true;
    }
    if selector.field.is_none() && selector.kind.is_none() {
        return contains(parent);
    }
    let Some(kind) = selector.kind else {
        return false;
    };
    let mut cursor = parent.walk();
    parent
        .children(&mut cursor)
        .any(|child| child.kind() == kind && contains(child))
}

fn selected_first_descendant(parent: Node<'_>, selector: ChildSelector, node: Node<'_>) -> bool {
    selected_child(parent, selector).is_some_and(|child| {
        child.start_byte() <= node.start_byte() && node.end_byte() <= child.end_byte()
    })
}

fn selected_child(parent: Node<'_>, selector: ChildSelector) -> Option<Node<'_>> {
    if let Some(field) = selector.field
        && let Some(node) = parent.child_by_field_name(field)
    {
        return Some(node);
    }
    let Some(kind) = selector.kind else {
        return selector.field.is_none().then_some(parent);
    };
    let mut cursor = parent.walk();
    parent
        .children(&mut cursor)
        .find(|child| child.kind() == kind)
}

pub(crate) fn rules_for_language(language: &str) -> Option<&'static VariableAccessRules> {
    native_jvm::rules(language)
        .or_else(|| dynamic_systems::rules(language))
        .or_else(|| wave2a::rules(language))
        .or_else(|| wave2b::rules(language))
        .or_else(|| web::rules(language))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::span::NormalizedSpan;
    use crate::base::types::{SymbolKind, SymbolOptions};
    use std::path::Path;
    use tree_sitter::{Parser, Tree};

    const SOURCE: &str = "int f(int x) { x += x; return x; }";
    const C_RULES: VariableAccessRules = VariableAccessRules {
        assignments: &[AssignmentRule {
            parent_kind: "assignment_expression",
            required_parent_kind: None,
            target: ChildSelector {
                field: Some("left"),
                kind: None,
            },
            operator: Some(ChildSelector {
                field: Some("operator"),
                kind: None,
            }),
            write_operators: &["="],
            read_write_operators: &["+=", "-="],
        }],
        declarations: &[],
        patterns: &[],
        increments: &[],
        by_reference: &[],
        receivers: &[],
        read_overrides: &[],
        unclassified: &[],
    };

    fn parse_c(source: &str) -> Tree {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_c::LANGUAGE.into())
            .expect("C grammar loads");
        parser.parse(source, None).expect("C source parses")
    }

    fn parse_rust(source: &str) -> Tree {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .expect("Rust grammar loads");
        parser.parse(source, None).expect("Rust source parses")
    }

    fn parse_python(source: &str) -> Tree {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .expect("Python grammar loads");
        parser.parse(source, None).expect("Python source parses")
    }

    fn first_node_of_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
        if node.kind() == kind {
            return Some(node);
        }
        let mut cursor = node.walk();
        node.children(&mut cursor)
            .find_map(|child| first_node_of_kind(child, kind))
    }

    fn nodes_of_kind<'a>(node: Node<'a>, kind: &str, nodes: &mut Vec<Node<'a>>) {
        if node.kind() == kind {
            nodes.push(node);
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            nodes_of_kind(child, kind, nodes);
        }
    }

    fn create_test_symbol(
        base: &mut super::super::extractor::BaseExtractor,
        node: Node<'_>,
        name: &str,
        kind: SymbolKind,
        parent_id: Option<&str>,
    ) -> Symbol {
        let options = SymbolOptions {
            parent_id: parent_id.map(str::to_string),
            ..Default::default()
        };
        base.create_symbol(&node, name.to_string(), kind, options)
    }

    #[test]
    fn compound_assignment_and_other_occurrences_have_distinct_access() {
        let tree = parse_c(SOURCE);
        let assignment =
            first_node_of_kind(tree.root_node(), "assignment_expression").expect("assignment node");
        let target = assignment
            .child_by_field_name("left")
            .expect("assignment target");
        let value = assignment
            .child_by_field_name("right")
            .and_then(|node| first_node_of_kind(node, "identifier"))
            .expect("right-side read");

        assert_eq!(classify(target, SOURCE, &C_RULES), Some(Access::ReadWrite));
        assert_eq!(classify(value, SOURCE, &C_RULES), Some(Access::Read));
        assert!(is_assignment_or_increment_target(target, SOURCE, &C_RULES));
        assert!(!is_assignment_or_increment_target(value, SOURCE, &C_RULES));
    }

    #[test]
    fn fieldless_assignment_target_uses_the_first_matching_child() {
        let source = "int f(int x, int y) { x = y; }";
        let tree = parse_c(source);
        let assignment = first_node_of_kind(tree.root_node(), "assignment_expression")
            .expect("assignment expression");
        let target = assignment
            .child_by_field_name("left")
            .expect("assignment target");
        let value = assignment
            .child_by_field_name("right")
            .expect("assignment value");
        let rules = VariableAccessRules {
            assignments: &[AssignmentRule {
                parent_kind: "assignment_expression",
                required_parent_kind: None,
                target: ChildSelector {
                    field: None,
                    kind: Some("identifier"),
                },
                operator: Some(ChildSelector {
                    field: Some("operator"),
                    kind: None,
                }),
                write_operators: &["="],
                read_write_operators: &[],
            }],
            ..C_RULES
        };

        assert_eq!(classify(target, source, &rules), Some(Access::Write));
        assert_eq!(classify(value, source, &rules), Some(Access::Read));
    }

    #[test]
    fn whole_parent_selector_matches_every_descendant() {
        let tree = parse_c(SOURCE);
        let assignment = first_node_of_kind(tree.root_node(), "assignment_expression")
            .expect("assignment expression");
        let target = assignment
            .child_by_field_name("left")
            .expect("assignment target");
        let value = assignment
            .child_by_field_name("right")
            .expect("assignment value");
        let selector = ChildSelector {
            field: None,
            kind: None,
        };

        assert!(selected_descendant(assignment, selector, target));
        assert!(selected_descendant(assignment, selector, value));
        assert_eq!(
            selected_child(assignment, selector).map(|node| node.id()),
            Some(assignment.id())
        );
    }

    #[test]
    fn pattern_binding_checks_the_configured_context() {
        let source = "fn f(value: i32) { match value { capture => capture, } }";
        let tree = parse_rust(source);
        let arm = first_node_of_kind(tree.root_node(), "match_arm").expect("match arm");
        let target = arm
            .child_by_field_name("pattern")
            .and_then(|pattern| first_node_of_kind(pattern, "identifier"))
            .expect("pattern binding");
        let value = arm
            .child_by_field_name("value")
            .and_then(|value| first_node_of_kind(value, "identifier"))
            .expect("match arm value");
        let rules = VariableAccessRules {
            assignments: &[],
            patterns: &[ContextRule {
                parent_kind: "match_arm",
                child: ChildSelector {
                    field: Some("pattern"),
                    kind: None,
                },
                access: Access::Write,
                predicate: None,
                required_child: None,
            }],
            ..C_RULES
        };

        assert!(is_pattern_binding(target, source, &rules));
        assert_eq!(classify(target, source, &rules), Some(Access::Write));
        assert!(!is_pattern_binding(value, source, &rules));
        assert_eq!(classify(value, source, &rules), Some(Access::Read));
    }

    #[test]
    fn pattern_binding_stops_at_closer_read_and_unclassified_contexts() {
        let source = "fn f(value: i32) { match value { (capture, _) => capture, } }";
        let tree = parse_rust(source);
        let arm = first_node_of_kind(tree.root_node(), "match_arm").expect("match arm");
        let target = arm
            .child_by_field_name("pattern")
            .and_then(|pattern| first_node_of_kind(pattern, "identifier"))
            .expect("tuple pattern binding");
        let pattern_rules = VariableAccessRules {
            assignments: &[],
            patterns: &[ContextRule {
                parent_kind: "match_arm",
                child: ChildSelector {
                    field: Some("pattern"),
                    kind: None,
                },
                access: Access::Write,
                predicate: None,
                required_child: None,
            }],
            ..C_RULES
        };
        let read_override_rules = VariableAccessRules {
            read_overrides: &[ContextRule {
                parent_kind: "tuple_pattern",
                child: ChildSelector {
                    field: None,
                    kind: None,
                },
                access: Access::Read,
                predicate: None,
                required_child: None,
            }],
            ..pattern_rules
        };
        let unclassified_rules = VariableAccessRules {
            unclassified: &[UnclassifiedRule {
                parent_kind: "tuple_pattern",
                child: ChildSelector {
                    field: None,
                    kind: None,
                },
                predicate: None,
                required_child: None,
            }],
            ..pattern_rules
        };

        assert!(is_pattern_binding(target, source, &pattern_rules));
        assert!(!is_pattern_binding(target, source, &read_override_rules));
        assert_eq!(
            classify(target, source, &read_override_rules),
            Some(Access::Read)
        );
        assert!(!is_pattern_binding(target, source, &unclassified_rules));
        assert_eq!(classify(target, source, &unclassified_rules), None);
    }

    #[test]
    fn member_assignment_and_increment_keep_receivers_as_reads() {
        let source = "struct Item { int field; }; int f(struct Item a) { a.field = 1; a.field++; }";
        let tree = parse_c(source);
        let assignment = first_node_of_kind(tree.root_node(), "assignment_expression")
            .expect("assignment expression");
        let assignment_target = assignment
            .child_by_field_name("left")
            .expect("member assignment target");
        let assignment_receiver = assignment_target
            .child_by_field_name("argument")
            .expect("member receiver");
        let assignment_member =
            first_node_of_kind(assignment_target, "field_identifier").expect("member name");
        let increment = first_node_of_kind(tree.root_node(), "update_expression")
            .expect("increment expression");
        let increment_target = increment
            .child_by_field_name("argument")
            .expect("member increment target");
        let increment_receiver = increment_target
            .child_by_field_name("argument")
            .expect("increment receiver");
        let increment_member = first_node_of_kind(increment_target, "field_identifier")
            .expect("increment member name");
        let rules = VariableAccessRules {
            increments: &[ContextRule {
                parent_kind: "update_expression",
                child: ChildSelector {
                    field: Some("argument"),
                    kind: None,
                },
                access: Access::Write,
                predicate: None,
                required_child: None,
            }],
            receivers: &[FieldRule {
                parent_kind: "field_expression",
                child: ChildSelector {
                    field: Some("argument"),
                    kind: None,
                },
            }],
            ..C_RULES
        };

        assert_eq!(
            classify(assignment_receiver, source, &rules),
            Some(Access::Read)
        );
        assert_eq!(
            classify(assignment_member, source, &rules),
            Some(Access::Write)
        );
        assert_eq!(
            classify(increment_receiver, source, &rules),
            Some(Access::Read)
        );
        assert_eq!(
            classify(increment_member, source, &rules),
            Some(Access::Write)
        );
        assert!(!is_assignment_or_increment_target(
            assignment_receiver,
            source,
            &rules
        ));
        assert!(!is_assignment_or_increment_target(
            increment_receiver,
            source,
            &rules
        ));
    }

    #[test]
    fn explicit_read_emission_requires_a_configured_read_or_receiver_site() {
        let tree = parse_c(SOURCE);
        let assignment =
            first_node_of_kind(tree.root_node(), "assignment_expression").expect("assignment");
        let target = assignment
            .child_by_field_name("left")
            .expect("assignment target");
        let value = assignment
            .child_by_field_name("right")
            .and_then(|node| first_node_of_kind(node, "identifier"))
            .expect("assignment value");
        let read_override_rules = VariableAccessRules {
            read_overrides: &[ContextRule {
                parent_kind: "assignment_expression",
                child: ChildSelector {
                    field: Some("left"),
                    kind: None,
                },
                access: Access::Read,
                predicate: None,
                required_child: None,
            }],
            ..C_RULES
        };

        assert!(is_explicit_read_reference(
            target,
            SOURCE,
            &read_override_rules
        ));
        assert!(!is_explicit_read_reference(value, SOURCE, &C_RULES));

        let declaration_source = "int f() { int x = 0; }";
        let declaration_tree = parse_c(declaration_source);
        let declaration = first_node_of_kind(declaration_tree.root_node(), "init_declarator")
            .expect("initializer declaration");
        let name = declaration
            .child_by_field_name("declarator")
            .and_then(|node| first_node_of_kind(node, "identifier"))
            .expect("declaration name");
        let declaration_rules = VariableAccessRules {
            declarations: &[DeclarationRule {
                parent_kind: "init_declarator",
                name: ChildSelector {
                    field: Some("declarator"),
                    kind: None,
                },
                initializer: ChildSelector {
                    field: Some("value"),
                    kind: None,
                },
            }],
            read_overrides: &[ContextRule {
                parent_kind: "init_declarator",
                child: ChildSelector {
                    field: Some("declarator"),
                    kind: None,
                },
                access: Access::Read,
                predicate: None,
                required_child: None,
            }],
            ..C_RULES
        };

        assert!(!is_explicit_read_reference(
            name,
            declaration_source,
            &declaration_rules
        ));

        let return_source = "int f() { return x; }";
        let return_tree = parse_c(return_source);
        let return_statement = first_node_of_kind(return_tree.root_node(), "return_statement")
            .expect("return statement");
        let returned = first_node_of_kind(return_statement, "identifier").expect("returned name");
        let unclassified_rules = VariableAccessRules {
            read_overrides: &[ContextRule {
                parent_kind: "function_definition",
                child: ChildSelector {
                    field: None,
                    kind: None,
                },
                access: Access::Read,
                predicate: None,
                required_child: None,
            }],
            unclassified: &[UnclassifiedRule {
                parent_kind: "return_statement",
                child: ChildSelector {
                    field: None,
                    kind: None,
                },
                predicate: None,
                required_child: None,
            }],
            ..C_RULES
        };

        assert!(!is_explicit_read_reference(
            returned,
            return_source,
            &unclassified_rules
        ));

        let member_source = "struct Item { int field; }; int f(struct Item a) { a.field = 1; }";
        let member_tree = parse_c(member_source);
        let member_assignment =
            first_node_of_kind(member_tree.root_node(), "assignment_expression")
                .expect("member assignment");
        let member_target = member_assignment
            .child_by_field_name("left")
            .expect("member target");
        let receiver = member_target
            .child_by_field_name("argument")
            .expect("member receiver");
        let member = first_node_of_kind(member_target, "field_identifier").expect("member name");
        let receiver_rules = VariableAccessRules {
            receivers: &[FieldRule {
                parent_kind: "field_expression",
                child: ChildSelector {
                    field: Some("argument"),
                    kind: None,
                },
            }],
            ..C_RULES
        };

        assert!(is_explicit_read_reference(
            receiver,
            member_source,
            &receiver_rules
        ));
        assert!(!is_explicit_read_reference(
            member,
            member_source,
            &receiver_rules
        ));

        let ordinary_source =
            "struct Item { int field; }; int f(struct Item a) { return a.field; }";
        let ordinary_tree = parse_c(ordinary_source);
        let ordinary_member = first_node_of_kind(ordinary_tree.root_node(), "field_expression")
            .expect("ordinary member read");
        let ordinary_receiver = ordinary_member
            .child_by_field_name("argument")
            .expect("ordinary member receiver");

        assert!(!is_explicit_read_reference(
            ordinary_receiver,
            ordinary_source,
            &receiver_rules
        ));
    }

    #[test]
    fn assignment_emission_guard_skips_declarations_and_pattern_bindings() {
        let rust_source = "fn f(value: i32) { match value { capture => capture, } }";
        let rust_tree = parse_rust(rust_source);
        let arm = first_node_of_kind(rust_tree.root_node(), "match_arm").expect("match arm");
        let capture = arm
            .child_by_field_name("pattern")
            .and_then(|pattern| first_node_of_kind(pattern, "identifier"))
            .expect("pattern binding");
        let pattern_rules = VariableAccessRules {
            assignments: &[AssignmentRule {
                parent_kind: "match_arm",
                required_parent_kind: None,
                target: ChildSelector {
                    field: Some("pattern"),
                    kind: None,
                },
                operator: None,
                write_operators: &[],
                read_write_operators: &[],
            }],
            patterns: &[ContextRule {
                parent_kind: "match_arm",
                child: ChildSelector {
                    field: Some("pattern"),
                    kind: None,
                },
                access: Access::Write,
                predicate: None,
                required_child: None,
            }],
            ..C_RULES
        };

        assert!(!is_assignment_or_increment_target(
            capture,
            rust_source,
            &pattern_rules
        ));

        let c_source = "int f() { int x = 0; }";
        let c_tree = parse_c(c_source);
        let initializer = first_node_of_kind(c_tree.root_node(), "init_declarator")
            .expect("initializer declaration");
        let name = initializer
            .child_by_field_name("declarator")
            .and_then(|node| first_node_of_kind(node, "identifier"))
            .expect("declaration name");
        let declaration_rules = VariableAccessRules {
            assignments: &[AssignmentRule {
                parent_kind: "init_declarator",
                required_parent_kind: None,
                target: ChildSelector {
                    field: Some("declarator"),
                    kind: None,
                },
                operator: None,
                write_operators: &[],
                read_write_operators: &[],
            }],
            declarations: &[DeclarationRule {
                parent_kind: "init_declarator",
                name: ChildSelector {
                    field: Some("declarator"),
                    kind: None,
                },
                initializer: ChildSelector {
                    field: Some("value"),
                    kind: None,
                },
            }],
            ..C_RULES
        };

        assert!(!is_assignment_or_increment_target(
            name,
            c_source,
            &declaration_rules
        ));
    }

    #[test]
    fn write_reference_gateway_uses_covering_binding_symbols_and_logical_owners() {
        let source = "int f() { x = 1; x = 2; }";
        let tree = parse_c(source);
        let function = first_node_of_kind(tree.root_node(), "function_definition")
            .expect("function definition");
        let mut assignments = Vec::new();
        nodes_of_kind(function, "assignment_expression", &mut assignments);
        let first = assignments[0];
        let second = assignments[1];
        let first_target = first.child_by_field_name("left").expect("first target");
        let second_target = second.child_by_field_name("left").expect("second target");
        let mut base = super::super::extractor::BaseExtractor::new(
            "c".to_string(),
            "/repo/main.c".to_string(),
            source.to_string(),
            Path::new("/repo"),
        );
        let first_binding = create_test_symbol(
            &mut base,
            first,
            "x",
            SymbolKind::Variable,
            Some("logical-scope"),
        );
        let second_binding = create_test_symbol(
            &mut base,
            second_target,
            "x",
            SymbolKind::Variable,
            Some("logical-scope"),
        );
        let logical_owner_symbols = vec![first_binding.clone(), second_binding.clone()];

        assert!(!should_emit_write_reference(
            first_target,
            "x",
            source,
            &C_RULES,
            std::slice::from_ref(&first_binding),
        ));
        assert!(should_emit_write_reference(
            second_target,
            "x",
            source,
            &C_RULES,
            &logical_owner_symbols,
        ));

        let other_scope_binding = create_test_symbol(
            &mut base,
            second_target,
            "x",
            SymbolKind::Variable,
            Some("other-scope"),
        );
        let other_scope_symbols = vec![first_binding, other_scope_binding];
        assert!(!should_emit_write_reference(
            second_target,
            "x",
            source,
            &C_RULES,
            &other_scope_symbols,
        ));

        let mut enclosing_function_named_x = create_test_symbol(
            &mut base,
            function,
            "x",
            SymbolKind::Function,
            Some("logical-scope"),
        );
        enclosing_function_named_x.body_span = function
            .child_by_field_name("body")
            .map(|body| NormalizedSpan::from_node(&body));
        assert!(should_emit_write_reference(
            second_target,
            "x",
            source,
            &C_RULES,
            std::slice::from_ref(&enclosing_function_named_x),
        ));
    }

    #[test]
    fn write_reference_gateway_does_not_duplicate_pattern_symbols() {
        let source = "fn f(value: i32) { match value { (capture, _) => capture, } }";
        let tree = parse_rust(source);
        let arm = first_node_of_kind(tree.root_node(), "match_arm").expect("match arm");
        let target = arm
            .child_by_field_name("pattern")
            .and_then(|pattern| first_node_of_kind(pattern, "identifier"))
            .expect("pattern binding");
        let rules = VariableAccessRules {
            assignments: &[],
            patterns: &[ContextRule {
                parent_kind: "match_arm",
                child: ChildSelector {
                    field: Some("pattern"),
                    kind: None,
                },
                access: Access::Write,
                predicate: None,
                required_child: None,
            }],
            ..C_RULES
        };
        let mut base = super::super::extractor::BaseExtractor::new(
            "rust".to_string(),
            "/repo/main.rs".to_string(),
            source.to_string(),
            Path::new("/repo"),
        );
        let binding = create_test_symbol(
            &mut base,
            arm,
            "capture",
            SymbolKind::Variable,
            Some("function-owner"),
        );

        assert!(!should_emit_write_reference(
            target,
            "capture",
            source,
            &rules,
            std::slice::from_ref(&binding),
        ));
    }

    #[test]
    fn write_reference_gateway_treats_none_parent_id_as_global_scope() {
        let source = "x = 1\nx = 2\n";
        let tree = parse_python(source);
        let mut assignments = Vec::new();
        nodes_of_kind(tree.root_node(), "assignment", &mut assignments);
        let first = assignments[0];
        let second = assignments[1];
        let first_target = first.child_by_field_name("left").expect("first target");
        let second_target = second.child_by_field_name("left").expect("second target");
        let mut base = super::super::extractor::BaseExtractor::new(
            "python".to_string(),
            "/repo/main.py".to_string(),
            source.to_string(),
            Path::new("/repo"),
        );
        let first_binding = create_test_symbol(&mut base, first, "x", SymbolKind::Variable, None);
        let second_binding = create_test_symbol(&mut base, second, "x", SymbolKind::Variable, None);
        let symbols = vec![first_binding, second_binding];
        let rules = rules_for_language("python").expect("Python rules");

        assert!(has_prior_binding(&symbols, None, "x", second_target));
        assert!(!has_prior_binding(
            &symbols,
            Some("current-assignment"),
            "x",
            second_target
        ));
        assert!(!should_emit_write_reference(
            first_target,
            "x",
            source,
            rules,
            &symbols[..1],
        ));
        assert!(should_emit_write_reference(
            second_target,
            "x",
            source,
            rules,
            &symbols,
        ));
    }

    #[test]
    fn write_reference_gateway_recognizes_assignment_backed_declaration_kinds() {
        let source = "int f() { x = 1; x = 2; }";
        let tree = parse_c(source);
        let function = first_node_of_kind(tree.root_node(), "function_definition")
            .expect("function definition");
        let mut assignments = Vec::new();
        nodes_of_kind(function, "assignment_expression", &mut assignments);
        let first_target = assignments[0]
            .child_by_field_name("left")
            .expect("first assignment target");
        let second_target = assignments[1]
            .child_by_field_name("left")
            .expect("second assignment target");
        let mut base = super::super::extractor::BaseExtractor::new(
            "c".to_string(),
            "/repo/main.c".to_string(),
            source.to_string(),
            Path::new("/repo"),
        );

        for kind in [
            SymbolKind::Class,
            SymbolKind::Function,
            SymbolKind::EnumMember,
        ] {
            let has_body = matches!(&kind, SymbolKind::Function);
            let mut first_binding =
                create_test_symbol(&mut base, assignments[0], "x", kind, Some("scope"));
            if has_body {
                first_binding.body_span = assignments[0]
                    .child_by_field_name("right")
                    .map(|body| NormalizedSpan::from_node(&body));
            }
            let second_binding = create_test_symbol(
                &mut base,
                assignments[1],
                "x",
                SymbolKind::Variable,
                Some("scope"),
            );
            let symbols = [first_binding, second_binding];

            assert!(!should_emit_write_reference(
                first_target,
                "x",
                source,
                &C_RULES,
                &symbols,
            ));
            assert!(should_emit_write_reference(
                second_target,
                "x",
                source,
                &C_RULES,
                &symbols,
            ));
        }
    }

    #[test]
    fn write_reference_gateway_ignores_enclosing_declaration_symbols() {
        let source = "class A((A := Base)):\n    A = 1\n";
        let tree = parse_python(source);
        let class_definition =
            first_node_of_kind(tree.root_node(), "class_definition").expect("class definition");
        let target = first_node_of_kind(class_definition, "named_expression")
            .and_then(|node| node.child_by_field_name("name"))
            .expect("class header assignment target");
        let mut base = super::super::extractor::BaseExtractor::new(
            "python".to_string(),
            "/repo/main.py".to_string(),
            source.to_string(),
            Path::new("/repo"),
        );
        let class_symbol =
            create_test_symbol(&mut base, class_definition, "A", SymbolKind::Class, None);
        let rules = rules_for_language("python").expect("Python rules");

        assert!(should_emit_write_reference(
            target,
            "A",
            source,
            rules,
            std::slice::from_ref(&class_symbol),
        ));
    }

    #[test]
    fn write_reference_gateway_ignores_enclosing_class_body_symbols() {
        let source = "class A:\n    A = 1\n";
        let tree = parse_python(source);
        let class_definition =
            first_node_of_kind(tree.root_node(), "class_definition").expect("class definition");
        let assignment =
            first_node_of_kind(class_definition, "assignment").expect("class body assignment");
        let target = assignment
            .child_by_field_name("left")
            .expect("assignment target");
        let mut base = super::super::extractor::BaseExtractor::new(
            "python".to_string(),
            "/repo/main.py".to_string(),
            source.to_string(),
            Path::new("/repo"),
        );
        let mut class_symbol =
            create_test_symbol(&mut base, class_definition, "A", SymbolKind::Class, None);
        class_symbol.body_span = class_definition
            .child_by_field_name("body")
            .map(|body| NormalizedSpan::from_node(&body));
        let rules = rules_for_language("python").expect("Python rules");

        assert!(should_emit_write_reference(
            target,
            "A",
            source,
            rules,
            std::slice::from_ref(&class_symbol),
        ));
    }

    #[test]
    fn prior_binding_matches_name_owner_and_earlier_source_position() {
        let source = "int f() { int x = 0; x = 1; }";
        let tree = parse_c(source);
        let function = first_node_of_kind(tree.root_node(), "function_definition")
            .expect("function definition");
        let assignment =
            first_node_of_kind(function, "assignment_expression").expect("assignment expression");
        let target = assignment
            .child_by_field_name("left")
            .expect("assignment target");
        let declaration = first_node_of_kind(function, "init_declarator")
            .and_then(|node| node.child_by_field_name("declarator"))
            .and_then(|node| first_node_of_kind(node, "identifier"))
            .expect("declaration name");
        let mut base = super::super::extractor::BaseExtractor::new(
            "c".to_string(),
            "/repo/main.c".to_string(),
            source.to_string(),
            Path::new("/repo"),
        );
        let options = SymbolOptions {
            parent_id: Some("function".to_string()),
            ..Default::default()
        };
        let symbol =
            base.create_symbol(&declaration, "x".to_string(), SymbolKind::Variable, options);

        assert!(has_prior_binding(
            std::slice::from_ref(&symbol),
            Some("function"),
            "x",
            target
        ));
        assert!(!has_prior_binding(
            std::slice::from_ref(&symbol),
            Some("other-function"),
            "x",
            target
        ));
        assert!(!has_prior_binding(
            std::slice::from_ref(&symbol),
            Some("function"),
            "other",
            target
        ));

        let current_assignment_options = SymbolOptions {
            parent_id: Some("function".to_string()),
            ..Default::default()
        };
        let current_assignment_symbol = base.create_symbol(
            &assignment,
            "x".to_string(),
            SymbolKind::Variable,
            current_assignment_options,
        );
        assert!(!has_prior_binding(
            std::slice::from_ref(&current_assignment_symbol),
            Some("function"),
            "x",
            target
        ));
    }
}
