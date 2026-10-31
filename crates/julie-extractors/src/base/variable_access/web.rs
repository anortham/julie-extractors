use super::{
    Access, AssignmentRule, ChildSelector, ContextRule, DeclarationRule, FieldRule,
    VariableAccessRules,
};

const fn field(name: &'static str) -> ChildSelector {
    ChildSelector {
        field: Some(name),
        kind: None,
    }
}

const RULES: VariableAccessRules = VariableAccessRules {
    assignments: &[
        AssignmentRule {
            parent_kind: "assignment_expression",
            target: field("left"),
            operator: None,
            required_parent_kind: None,
            read_write_operators: &[],
            write_operators: &[],
        },
        AssignmentRule {
            parent_kind: "augmented_assignment_expression",
            target: field("left"),
            operator: Some(field("operator")),
            required_parent_kind: None,
            read_write_operators: &[
                "%=", "&&=", "&=", "**=", "*=", "+=", "-=", "/=", "<<=", ">>=", ">>>=", "??=",
                "^=", "|=", "||=",
            ],
            write_operators: &[],
        },
    ],
    declarations: &[DeclarationRule {
        parent_kind: "variable_declarator",
        name: field("name"),
        initializer: field("value"),
    }],
    patterns: &[
        ContextRule {
            parent_kind: "for_in_statement",
            child: field("left"),
            access: Access::Write,
            predicate: Some(for_in_assignment_target),
            required_child: None,
        },
        ContextRule {
            parent_kind: "catch_clause",
            child: field("parameter"),
            access: Access::Write,
            predicate: None,
            required_child: None,
        },
        ContextRule {
            parent_kind: "assignment_expression",
            child: field("left"),
            access: Access::Write,
            predicate: None,
            required_child: None,
        },
    ],
    increments: &[ContextRule {
        parent_kind: "update_expression",
        child: field("argument"),
        access: Access::Write,
        predicate: None,
        required_child: None,
    }],
    by_reference: &[],
    read_overrides: &[
        ContextRule {
            parent_kind: "assignment_pattern",
            child: field("right"),
            access: Access::Read,
            predicate: None,
            required_child: None,
        },
        ContextRule {
            parent_kind: "object_assignment_pattern",
            child: field("right"),
            access: Access::Read,
            predicate: None,
            required_child: None,
        },
        ContextRule {
            parent_kind: "computed_property_name",
            child: ChildSelector {
                field: None,
                kind: None,
            },
            access: Access::Read,
            predicate: None,
            required_child: None,
        },
        ContextRule {
            parent_kind: "subscript_expression",
            child: field("index"),
            access: Access::Read,
            predicate: None,
            required_child: None,
        },
    ],
    unclassified: &[],
    receivers: &[
        FieldRule {
            parent_kind: "member_expression",
            child: field("object"),
        },
        FieldRule {
            parent_kind: "subscript_expression",
            child: field("object"),
        },
    ],
};

fn for_in_assignment_target(
    _node: tree_sitter::Node<'_>,
    parent: tree_sitter::Node<'_>,
    _source: &str,
) -> bool {
    parent.child_by_field_name("kind").is_none()
}

pub(super) fn rules(language: &str) -> Option<&'static VariableAccessRules> {
    matches!(
        language,
        "javascript" | "typescript" | "tsx" | "jsx" | "vue"
    )
    .then_some(&RULES)
}
