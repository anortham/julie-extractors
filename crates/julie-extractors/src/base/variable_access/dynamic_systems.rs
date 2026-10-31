use super::{Access, AssignmentRule, ChildSelector, ContextRule, FieldRule, VariableAccessRules};

const fn field(name: &'static str) -> ChildSelector {
    ChildSelector {
        field: Some(name),
        kind: None,
    }
}

const fn kind(name: &'static str) -> ChildSelector {
    ChildSelector {
        field: None,
        kind: Some(name),
    }
}

const fn whole() -> ChildSelector {
    ChildSelector {
        field: None,
        kind: None,
    }
}

const fn assignment(
    parent_kind: &'static str,
    target: ChildSelector,
    operator: Option<ChildSelector>,
    write_operators: &'static [&'static str],
    read_write_operators: &'static [&'static str],
) -> AssignmentRule {
    AssignmentRule {
        parent_kind,
        required_parent_kind: None,
        target,
        operator,
        write_operators,
        read_write_operators,
    }
}

const fn context(parent_kind: &'static str, child: ChildSelector, access: Access) -> ContextRule {
    ContextRule {
        parent_kind,
        child,
        access,
        predicate: None,
        required_child: None,
    }
}

const fn required_context(
    parent_kind: &'static str,
    child: ChildSelector,
    access: Access,
    required_child: ChildSelector,
) -> ContextRule {
    ContextRule {
        parent_kind,
        child,
        access,
        predicate: None,
        required_child: Some(required_child),
    }
}

const PYTHON_RULES: VariableAccessRules = VariableAccessRules {
    assignments: &[
        assignment("assignment", field("left"), None, &[], &[]),
        assignment(
            "augmented_assignment",
            field("left"),
            Some(field("operator")),
            &[],
            &[
                "+=", "-=", "*=", "/=", "@=", "//=", "%=", "**=", ">>=", "<<=", "&=", "^=", "|=",
            ],
        ),
        assignment("named_expression", field("name"), None, &[], &[]),
    ],
    declarations: &[],
    patterns: &[
        context("for_statement", field("left"), Access::Write),
        context("for_in_clause", field("left"), Access::Write),
        context("as_pattern_target", kind("identifier"), Access::Write),
        ContextRule {
            parent_kind: "case_clause",
            child: kind("case_pattern"),
            access: Access::Write,
            predicate: Some(python_case_capture),
            required_child: None,
        },
    ],
    increments: &[],
    by_reference: &[],
    receivers: &[
        FieldRule {
            parent_kind: "attribute",
            child: field("object"),
        },
        FieldRule {
            parent_kind: "subscript",
            child: field("value"),
        },
    ],
    read_overrides: &[context("subscript", whole(), Access::Read)],
    unclassified: &[],
};

const RUST_RULES: VariableAccessRules = VariableAccessRules {
    assignments: &[
        assignment("assignment_expression", field("left"), None, &[], &[]),
        assignment(
            "compound_assignment_expr",
            field("left"),
            Some(field("operator")),
            &[],
            &["+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<=", ">>="],
        ),
    ],
    declarations: &[],
    patterns: &[
        ContextRule {
            parent_kind: "match_arm",
            child: field("pattern"),
            access: Access::Write,
            predicate: Some(rust_pattern_binding),
            required_child: None,
        },
        ContextRule {
            parent_kind: "for_expression",
            child: field("pattern"),
            access: Access::Write,
            predicate: Some(rust_pattern_binding),
            required_child: None,
        },
    ],
    increments: &[],
    by_reference: &[required_context(
        "reference_expression",
        field("value"),
        Access::Write,
        kind("mutable_specifier"),
    )],
    receivers: &[FieldRule {
        parent_kind: "field_expression",
        child: field("value"),
    }],
    read_overrides: &[context("index_expression", whole(), Access::Read)],
    unclassified: &[],
};

const GO_RULES: VariableAccessRules = VariableAccessRules {
    assignments: &[
        assignment(
            "assignment_statement",
            field("left"),
            Some(field("operator")),
            &[],
            &[
                "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<=", ">>=", "&^=",
            ],
        ),
        assignment(
            "short_var_declaration",
            field("left"),
            Some(kind(":=")),
            &[],
            &[],
        ),
    ],
    declarations: &[],
    patterns: &[context("range_clause", field("left"), Access::Write)],
    increments: &[
        context("inc_statement", kind("identifier"), Access::Write),
        context("dec_statement", kind("identifier"), Access::Write),
    ],
    by_reference: &[],
    receivers: &[
        FieldRule {
            parent_kind: "selector_expression",
            child: field("operand"),
        },
        FieldRule {
            parent_kind: "index_expression",
            child: field("operand"),
        },
        FieldRule {
            parent_kind: "slice_expression",
            child: field("operand"),
        },
    ],
    read_overrides: &[
        context("index_expression", whole(), Access::Read),
        context("slice_expression", whole(), Access::Read),
    ],
    unclassified: &[],
};

const RUBY_COMPOUND_OPERATORS: &[&str] = &[
    "+=", "-=", "*=", "**=", "/=", "||=", "|=", "&&=", "&=", "%=", ">>=", "<<=", "^=",
];

const RUBY_RULES: VariableAccessRules = VariableAccessRules {
    assignments: &[
        assignment("assignment", field("left"), None, &[], &[]),
        assignment("command_assignment", field("left"), None, &[], &[]),
        assignment(
            "operator_assignment",
            field("left"),
            Some(field("operator")),
            &[],
            RUBY_COMPOUND_OPERATORS,
        ),
        assignment(
            "command_operator_assignment",
            field("left"),
            Some(field("operator")),
            &[],
            RUBY_COMPOUND_OPERATORS,
        ),
    ],
    declarations: &[],
    patterns: &[
        context("for", field("pattern"), Access::Write),
        context("in_clause", field("pattern"), Access::Write),
        context("block_parameters", kind("identifier"), Access::Write),
        context("exception_variable", kind("identifier"), Access::Write),
    ],
    increments: &[],
    by_reference: &[],
    receivers: &[
        FieldRule {
            parent_kind: "call",
            child: field("receiver"),
        },
        FieldRule {
            parent_kind: "element_reference",
            child: field("object"),
        },
    ],
    read_overrides: &[context(
        "element_reference",
        kind("identifier"),
        Access::Read,
    )],
    unclassified: &[],
};

const PHP_RULES: VariableAccessRules = VariableAccessRules {
    assignments: &[
        assignment("assignment_expression", field("left"), None, &[], &[]),
        assignment(
            "reference_assignment_expression",
            field("left"),
            None,
            &[],
            &[],
        ),
        assignment(
            "augmented_assignment_expression",
            field("left"),
            Some(field("operator")),
            &[],
            &[
                "+=", "-=", "*=", "/=", "%=", "**=", ".=", "&=", "|=", "^=", "<<=", ">>=", "??=",
            ],
        ),
    ],
    declarations: &[],
    patterns: &[
        ContextRule {
            parent_kind: "foreach_statement",
            child: kind("expression"),
            access: Access::Write,
            predicate: Some(php_foreach_target),
            required_child: None,
        },
        ContextRule {
            parent_kind: "foreach_statement",
            child: kind("pair"),
            access: Access::Write,
            predicate: Some(php_foreach_target),
            required_child: None,
        },
        ContextRule {
            parent_kind: "foreach_statement",
            child: kind("by_ref"),
            access: Access::Write,
            predicate: Some(php_foreach_target),
            required_child: None,
        },
        context("catch_clause", field("name"), Access::Write),
    ],
    increments: &[context(
        "update_expression",
        kind("variable_name"),
        Access::Write,
    )],
    by_reference: &[required_context(
        "argument",
        kind("expression"),
        Access::Write,
        field("reference_modifier"),
    )],
    receivers: &[
        FieldRule {
            parent_kind: "member_access_expression",
            child: field("object"),
        },
        FieldRule {
            parent_kind: "nullsafe_member_access_expression",
            child: field("object"),
        },
        FieldRule {
            parent_kind: "member_call_expression",
            child: field("object"),
        },
        FieldRule {
            parent_kind: "nullsafe_member_call_expression",
            child: field("object"),
        },
        FieldRule {
            parent_kind: "subscript_expression",
            child: whole(),
        },
    ],
    read_overrides: &[context("subscript_expression", whole(), Access::Read)],
    unclassified: &[],
};

const SWIFT_COMPOUND_OPERATORS: &[&str] = &[
    "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<=", ">>=", "&+=", "&-=", "&*=",
];

const SWIFT_RULES: VariableAccessRules = VariableAccessRules {
    assignments: &[assignment(
        "assignment",
        field("target"),
        Some(field("operator")),
        &[],
        SWIFT_COMPOUND_OPERATORS,
    )],
    declarations: &[],
    patterns: &[context("for_statement", field("item"), Access::Write)],
    increments: &[],
    by_reference: &[
        required_context(
            "prefix_expression",
            kind("simple_identifier"),
            Access::ReadWrite,
            kind("&"),
        ),
        required_context(
            "prefix_expression",
            kind("navigation_expression"),
            Access::ReadWrite,
            kind("&"),
        ),
    ],
    receivers: &[FieldRule {
        parent_kind: "navigation_expression",
        child: field("target"),
    }],
    read_overrides: &[context("call_expression", whole(), Access::Read)],
    unclassified: &[],
};

fn python_case_capture(
    node: tree_sitter::Node<'_>,
    _parent: tree_sitter::Node<'_>,
    _source: &str,
) -> bool {
    let Some(pattern) = node.parent() else {
        return false;
    };
    if pattern.kind() == "dotted_name" {
        return pattern.named_child_count() == 1
            && !pattern
                .parent()
                .is_some_and(|parent| parent.kind() == "class_pattern");
    }
    pattern.kind() != "keyword_pattern"
}

fn php_foreach_target(
    node: tree_sitter::Node<'_>,
    parent: tree_sitter::Node<'_>,
    _source: &str,
) -> bool {
    let mut cursor = parent.walk();
    parent
        .children(&mut cursor)
        .any(|child| child.kind() == "as" && node.start_byte() > child.end_byte())
}

fn rust_pattern_binding(
    node: tree_sitter::Node<'_>,
    _parent: tree_sitter::Node<'_>,
    source: &str,
) -> bool {
    let name = node.utf8_text(source.as_bytes()).unwrap_or_default();
    if name.chars().next().is_some_and(char::is_uppercase) {
        return false;
    }
    if let Some(parent) = node.parent()
        && parent.kind() == "field_pattern"
        && parent
            .child_by_field_name("name")
            .is_some_and(|field| field.id() == node.id())
        && parent.child_by_field_name("pattern").is_some()
    {
        return false;
    }
    true
}

pub(super) fn rules(language: &str) -> Option<&'static VariableAccessRules> {
    match language {
        "python" => Some(&PYTHON_RULES),
        "rust" => Some(&RUST_RULES),
        "go" => Some(&GO_RULES),
        "ruby" => Some(&RUBY_RULES),
        "php" => Some(&PHP_RULES),
        "swift" => Some(&SWIFT_RULES),
        _ => None,
    }
}
