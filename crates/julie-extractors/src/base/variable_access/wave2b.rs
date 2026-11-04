use super::{
    Access, AssignmentRule, ChildSelector, ContextRule, FieldRule, UnclassifiedRule,
    VariableAccessRules,
};

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

const BASH_ASSIGNMENTS: &[AssignmentRule] = &[
    AssignmentRule {
        parent_kind: "variable_assignment",
        required_parent_kind: None,
        required_child: None,
        target: field("name"),
        operator: Some(kind("=")),
        write_operators: &["="],
        read_write_operators: &[],
    },
    AssignmentRule {
        parent_kind: "variable_assignment",
        required_parent_kind: None,
        required_child: None,
        target: field("name"),
        operator: Some(kind("+=")),
        write_operators: &["+="],
        read_write_operators: &["+="],
    },
    AssignmentRule {
        parent_kind: "binary_expression",
        required_parent_kind: Some("arithmetic_expansion"),
        required_child: None,
        target: field("left"),
        operator: Some(field("operator")),
        write_operators: &[
            "=", "+=", "-=", "*=", "/=", "%=", "**=", "<<=", ">>=", "&=", "|=", "^=",
        ],
        read_write_operators: &[
            "+=", "-=", "*=", "/=", "%=", "**=", "<<=", ">>=", "&=", "|=", "^=",
        ],
    },
    AssignmentRule {
        parent_kind: "binary_expression",
        required_parent_kind: Some("compound_statement"),
        required_child: None,
        target: field("left"),
        operator: Some(field("operator")),
        write_operators: &[
            "=", "+=", "-=", "*=", "/=", "%=", "**=", "<<=", ">>=", "&=", "|=", "^=",
        ],
        read_write_operators: &[
            "+=", "-=", "*=", "/=", "%=", "**=", "<<=", ">>=", "&=", "|=", "^=",
        ],
    },
    AssignmentRule {
        parent_kind: "binary_expression",
        required_parent_kind: Some("c_style_for_statement"),
        required_child: None,
        target: field("left"),
        operator: Some(field("operator")),
        write_operators: &[
            "=", "+=", "-=", "*=", "/=", "%=", "**=", "<<=", ">>=", "&=", "|=", "^=",
        ],
        read_write_operators: &[
            "+=", "-=", "*=", "/=", "%=", "**=", "<<=", ">>=", "&=", "|=", "^=",
        ],
    },
];

const BASH_PATTERNS: &[ContextRule] = &[ContextRule {
    parent_kind: "for_statement",
    child: ChildSelector {
        field: Some("variable"),
        kind: None,
    },
    access: Access::Write,
    predicate: None,
    required_child: None,
}];

const BASH_RECEIVERS: &[FieldRule] = &[FieldRule {
    parent_kind: "subscript",
    child: field("name"),
}];

const BASH_INCREMENTS: &[ContextRule] = &[
    ContextRule {
        parent_kind: "unary_expression",
        child: kind("variable_name"),
        access: Access::Write,
        predicate: Some(is_bash_increment),
        required_child: None,
    },
    ContextRule {
        parent_kind: "unary_expression",
        child: kind("subscript"),
        access: Access::Write,
        predicate: Some(is_bash_increment),
        required_child: None,
    },
    ContextRule {
        parent_kind: "postfix_expression",
        child: kind("variable_name"),
        access: Access::Write,
        predicate: Some(is_bash_increment),
        required_child: None,
    },
    ContextRule {
        parent_kind: "postfix_expression",
        child: kind("subscript"),
        access: Access::Write,
        predicate: Some(is_bash_increment),
        required_child: None,
    },
];

const BASH_READ_OVERRIDES: &[ContextRule] = &[
    ContextRule {
        parent_kind: "subscript",
        child: field("index"),
        access: Access::Read,
        predicate: None,
        required_child: None,
    },
    ContextRule {
        parent_kind: "simple_expansion",
        child: kind("variable_name"),
        access: Access::Read,
        predicate: Some(is_bash_protected_expansion),
        required_child: None,
    },
    ContextRule {
        parent_kind: "expansion",
        child: kind("variable_name"),
        access: Access::Read,
        predicate: Some(is_bash_protected_expansion),
        required_child: None,
    },
];

const BASH_UNCLASSIFIED: &[UnclassifiedRule] = &[
    UnclassifiedRule {
        parent_kind: "simple_expansion",
        child: kind("variable_name"),
        predicate: Some(is_unquoted_word_expansion),
        required_child: None,
    },
    UnclassifiedRule {
        parent_kind: "expansion",
        child: kind("variable_name"),
        predicate: Some(is_unquoted_word_expansion),
        required_child: None,
    },
    UnclassifiedRule {
        parent_kind: "command",
        child: ChildSelector {
            field: None,
            kind: None,
        },
        predicate: Some(is_bash_eval_argument),
        required_child: None,
    },
];

const BASH_RULES: VariableAccessRules = VariableAccessRules {
    assignments: BASH_ASSIGNMENTS,
    declarations: &[],
    patterns: BASH_PATTERNS,
    increments: BASH_INCREMENTS,
    by_reference: &[],
    receivers: BASH_RECEIVERS,
    read_overrides: BASH_READ_OVERRIDES,
    unclassified: BASH_UNCLASSIFIED,
};

const POWERSHELL_ASSIGNMENTS: &[AssignmentRule] = &[AssignmentRule {
    parent_kind: "assignment_expression",
    required_parent_kind: None,
    required_child: None,
    target: ChildSelector {
        field: None,
        kind: Some("left_assignment_expression"),
    },
    operator: Some(ChildSelector {
        field: None,
        kind: Some("assignement_operator"),
    }),
    write_operators: &[
        "=", "+=", "-=", "*=", "/=", "%=", "**=", "<<=", ">>=", "&=", "|=", "^=", "&&=", "||=",
        "??=",
    ],
    read_write_operators: &[
        "+=", "-=", "*=", "/=", "%=", "**=", "<<=", ">>=", "&=", "|=", "^=", "&&=", "||=", "??=",
    ],
}];

const POWERSHELL_PATTERNS: &[ContextRule] = &[ContextRule {
    parent_kind: "foreach_statement",
    child: ChildSelector {
        field: None,
        kind: Some("variable"),
    },
    access: Access::Write,
    predicate: None,
    required_child: None,
}];

const POWERSHELL_INCREMENTS: &[ContextRule] = &[
    ContextRule {
        parent_kind: "post_increment_expression",
        child: ChildSelector {
            field: None,
            kind: Some("variable"),
        },
        access: Access::Write,
        predicate: None,
        required_child: None,
    },
    ContextRule {
        parent_kind: "pre_increment_expression",
        child: ChildSelector {
            field: None,
            kind: Some("unary_expression"),
        },
        access: Access::Write,
        predicate: None,
        required_child: None,
    },
    ContextRule {
        parent_kind: "post_decrement_expression",
        child: ChildSelector {
            field: None,
            kind: Some("variable"),
        },
        access: Access::Write,
        predicate: None,
        required_child: None,
    },
    ContextRule {
        parent_kind: "pre_decrement_expression",
        child: ChildSelector {
            field: None,
            kind: Some("unary_expression"),
        },
        access: Access::Write,
        predicate: None,
        required_child: None,
    },
];

const POWERSHELL_BY_REFERENCE: &[ContextRule] = &[ContextRule {
    parent_kind: "cast_expression",
    child: kind("unary_expression"),
    access: Access::Write,
    predicate: Some(is_powershell_ref_cast),
    required_child: Some(kind("type_literal")),
}];

const POWERSHELL_READ_OVERRIDES: &[ContextRule] = &[ContextRule {
    parent_kind: "element_access",
    child: kind("logical_expression"),
    access: Access::Read,
    predicate: None,
    required_child: None,
}];

const POWERSHELL_RECEIVERS: &[FieldRule] = &[
    FieldRule {
        parent_kind: "member_access",
        child: kind("variable"),
    },
    FieldRule {
        parent_kind: "member_access",
        child: kind("member_access"),
    },
    FieldRule {
        parent_kind: "element_access",
        child: kind("variable"),
    },
    FieldRule {
        parent_kind: "element_access",
        child: kind("member_access"),
    },
    FieldRule {
        parent_kind: "element_access",
        child: kind("element_access"),
    },
];

const POWERSHELL_RULES: VariableAccessRules = VariableAccessRules {
    assignments: POWERSHELL_ASSIGNMENTS,
    declarations: &[],
    patterns: POWERSHELL_PATTERNS,
    increments: POWERSHELL_INCREMENTS,
    by_reference: POWERSHELL_BY_REFERENCE,
    receivers: POWERSHELL_RECEIVERS,
    read_overrides: POWERSHELL_READ_OVERRIDES,
    unclassified: &[],
};

const GDSCRIPT_ASSIGNMENTS: &[AssignmentRule] = &[
    AssignmentRule {
        parent_kind: "assignment",
        required_parent_kind: None,
        required_child: None,
        target: ChildSelector {
            field: Some("left"),
            kind: None,
        },
        operator: None,
        write_operators: &[],
        read_write_operators: &[],
    },
    AssignmentRule {
        parent_kind: "augmented_assignment",
        required_parent_kind: None,
        required_child: None,
        target: ChildSelector {
            field: Some("left"),
            kind: None,
        },
        operator: Some(ChildSelector {
            field: Some("op"),
            kind: None,
        }),
        write_operators: &[
            "%=", "&=", "**=", "*=", "+=", "-=", "/=", "<<=", ">>=", "^=", "|=",
        ],
        read_write_operators: &[
            "%=", "&=", "**=", "*=", "+=", "-=", "/=", "<<=", ">>=", "^=", "|=",
        ],
    },
];

const GDSCRIPT_PATTERNS: &[ContextRule] = &[ContextRule {
    parent_kind: "for_statement",
    child: field("left"),
    access: Access::Write,
    predicate: None,
    required_child: None,
}];

const GDSCRIPT_PATTERN_BINDINGS: &[ContextRule] = &[ContextRule {
    parent_kind: "pattern_binding",
    child: kind("identifier"),
    access: Access::Write,
    predicate: None,
    required_child: None,
}];

const GDSCRIPT_ALL_PATTERNS: &[ContextRule] = &[GDSCRIPT_PATTERNS[0], GDSCRIPT_PATTERN_BINDINGS[0]];

const GDSCRIPT_READ_OVERRIDES: &[ContextRule] = &[
    ContextRule {
        parent_kind: "subscript",
        child: field("arguments"),
        access: Access::Read,
        predicate: None,
        required_child: None,
    },
    ContextRule {
        parent_kind: "attribute",
        child: ChildSelector {
            field: None,
            kind: None,
        },
        access: Access::Read,
        predicate: Some(is_gdscript_nonterminal_attribute_read),
        required_child: None,
    },
];

const GDSCRIPT_RECEIVERS: &[FieldRule] = &[
    FieldRule {
        parent_kind: "subscript",
        child: kind("identifier"),
    },
    FieldRule {
        parent_kind: "subscript",
        child: kind("subscript"),
    },
];

const GDSCRIPT_RULES: VariableAccessRules = VariableAccessRules {
    assignments: GDSCRIPT_ASSIGNMENTS,
    declarations: &[],
    patterns: GDSCRIPT_ALL_PATTERNS,
    increments: &[],
    by_reference: &[],
    receivers: GDSCRIPT_RECEIVERS,
    read_overrides: GDSCRIPT_READ_OVERRIDES,
    unclassified: &[],
};

const R_ASSIGNMENTS: &[AssignmentRule] = &[
    AssignmentRule {
        parent_kind: "binary_operator",
        required_parent_kind: None,
        required_child: None,
        target: ChildSelector {
            field: Some("lhs"),
            kind: None,
        },
        operator: Some(ChildSelector {
            field: Some("operator"),
            kind: None,
        }),
        write_operators: &["<-", "<<-", "=", ":="],
        read_write_operators: &[],
    },
    AssignmentRule {
        parent_kind: "binary_operator",
        required_parent_kind: None,
        required_child: None,
        target: ChildSelector {
            field: Some("rhs"),
            kind: None,
        },
        operator: Some(ChildSelector {
            field: Some("operator"),
            kind: None,
        }),
        write_operators: &["->", "->>"],
        read_write_operators: &[],
    },
    AssignmentRule {
        parent_kind: "binary_operator",
        required_parent_kind: None,
        required_child: None,
        target: ChildSelector {
            field: Some("lhs"),
            kind: None,
        },
        operator: Some(ChildSelector {
            field: Some("operator"),
            kind: None,
        }),
        write_operators: &["%<>%"],
        read_write_operators: &["%<>%"],
    },
];

const R_PATTERNS: &[ContextRule] = &[
    ContextRule {
        parent_kind: "for_statement",
        child: field("variable"),
        access: Access::Write,
        predicate: None,
        required_child: None,
    },
    ContextRule {
        parent_kind: "for_statement",
        child: field("name"),
        access: Access::Write,
        predicate: None,
        required_child: None,
    },
];

const R_READ_OVERRIDES: &[ContextRule] = &[
    ContextRule {
        parent_kind: "call",
        child: field("arguments"),
        access: Access::Read,
        predicate: None,
        required_child: None,
    },
    ContextRule {
        parent_kind: "subset",
        child: field("arguments"),
        access: Access::Read,
        predicate: None,
        required_child: None,
    },
    ContextRule {
        parent_kind: "subset2",
        child: field("arguments"),
        access: Access::Read,
        predicate: None,
        required_child: None,
    },
];

const R_RECEIVERS: &[FieldRule] = &[
    FieldRule {
        parent_kind: "extract_operator",
        child: ChildSelector {
            field: Some("lhs"),
            kind: None,
        },
    },
    FieldRule {
        parent_kind: "subset",
        child: ChildSelector {
            field: Some("function"),
            kind: None,
        },
    },
    FieldRule {
        parent_kind: "subset2",
        child: ChildSelector {
            field: Some("function"),
            kind: None,
        },
    },
];

const R_RULES: VariableAccessRules = VariableAccessRules {
    assignments: R_ASSIGNMENTS,
    declarations: &[],
    patterns: R_PATTERNS,
    increments: &[],
    by_reference: &[],
    receivers: R_RECEIVERS,
    read_overrides: R_READ_OVERRIDES,
    unclassified: &[],
};

const SQL_ASSIGNMENTS: &[AssignmentRule] = &[
    AssignmentRule {
        parent_kind: "assignment",
        required_parent_kind: None,
        required_child: None,
        target: field("left"),
        operator: None,
        write_operators: &[],
        read_write_operators: &[],
    },
    AssignmentRule {
        parent_kind: "set_statement",
        required_parent_kind: None,
        required_child: None,
        target: kind("object_reference"),
        operator: Some(kind("=")),
        write_operators: &["="],
        read_write_operators: &[],
    },
    AssignmentRule {
        parent_kind: "set_statement",
        required_parent_kind: None,
        required_child: None,
        target: kind("object_reference"),
        operator: Some(kind("tsql_compound_assignment_operator")),
        write_operators: &[],
        read_write_operators: &["+=", "-=", "*=", "/=", "%=", "&=", "^=", "|="],
    },
];

const SQL_RULES: VariableAccessRules = VariableAccessRules {
    assignments: SQL_ASSIGNMENTS,
    declarations: &[],
    patterns: &[],
    increments: &[],
    by_reference: &[],
    receivers: &[],
    read_overrides: &[],
    unclassified: &[],
};

pub(super) fn rules(language: &str) -> Option<&'static VariableAccessRules> {
    match language {
        "bash" => Some(&BASH_RULES),
        "powershell" => Some(&POWERSHELL_RULES),
        "gdscript" => Some(&GDSCRIPT_RULES),
        "r" => Some(&R_RULES),
        "sql" => Some(&SQL_RULES),
        "qml" | "jsx" | "vue" => super::web::rules("javascript"),
        _ => None,
    }
}

fn is_bash_increment(
    _node: tree_sitter::Node<'_>,
    parent: tree_sitter::Node<'_>,
    source: &str,
) -> bool {
    parent
        .child_by_field_name("operator")
        .and_then(|operator| operator.utf8_text(source.as_bytes()).ok())
        .is_some_and(|operator| matches!(operator, "++" | "--"))
}

fn is_bash_protected_expansion(
    node: tree_sitter::Node<'_>,
    expansion: tree_sitter::Node<'_>,
    _source: &str,
) -> bool {
    let Some(context) = expansion.parent() else {
        return false;
    };
    if matches!(context.kind(), "test_command" | "case_statement")
        || context
            .parent()
            .is_some_and(|parent| matches!(parent.kind(), "test_command" | "case_statement"))
    {
        return true;
    }
    let subscript = if context.kind() == "subscript" {
        Some(context)
    } else {
        context
            .parent()
            .filter(|parent| parent.kind() == "subscript")
    };
    if subscript.is_some_and(|subscript| {
        subscript.child_by_field_name("index").is_some_and(|index| {
            index.start_byte() <= node.start_byte() && node.end_byte() <= index.end_byte()
        })
    }) {
        return true;
    }
    let assignment = if context.kind() == "variable_assignment" {
        Some(context)
    } else {
        context
            .parent()
            .filter(|parent| parent.kind() == "variable_assignment")
    };
    assignment.is_some_and(|assignment| {
        assignment
            .child_by_field_name("value")
            .is_some_and(|value| {
                value.kind() != "array"
                    && value.start_byte() <= node.start_byte()
                    && node.end_byte() <= value.end_byte()
            })
    })
}

fn is_bash_eval_argument(
    node: tree_sitter::Node<'_>,
    command: tree_sitter::Node<'_>,
    source: &str,
) -> bool {
    let Some(prefix) = source.get(command.start_byte()..node.start_byte()) else {
        return false;
    };
    let mut words = prefix.split_whitespace().filter(|word| !word.contains('='));
    matches!(
        (words.next(), words.next()),
        (Some("eval"), _) | (Some("builtin" | "command"), Some("eval"))
    )
}

fn is_gdscript_nonterminal_attribute_read(
    node: tree_sitter::Node<'_>,
    attribute: tree_sitter::Node<'_>,
    _source: &str,
) -> bool {
    let mut cursor = attribute.walk();
    let children: Vec<_> = attribute.named_children(&mut cursor).collect();
    let Some(index) = children.iter().position(|child| {
        child.start_byte() <= node.start_byte() && node.end_byte() <= child.end_byte()
    }) else {
        return false;
    };
    index + 1 < children.len() || children[index].kind() == "attribute_call"
}

fn is_powershell_ref_cast(
    _node: tree_sitter::Node<'_>,
    cast: tree_sitter::Node<'_>,
    source: &str,
) -> bool {
    let mut cursor = cast.walk();
    cast.named_children(&mut cursor)
        .find(|child| child.kind() == "type_literal")
        .and_then(|literal| literal.utf8_text(source.as_bytes()).ok())
        .is_some_and(|text| text.eq_ignore_ascii_case("[ref]"))
}

fn is_unquoted_word_expansion(
    node: tree_sitter::Node<'_>,
    expansion: tree_sitter::Node<'_>,
    source: &str,
) -> bool {
    let context = expansion.parent().unwrap_or(expansion);
    if matches!(
        context.kind(),
        "arithmetic_expansion" | "test_command" | "case_statement"
    ) || context.parent().is_some_and(|parent| {
        matches!(
            parent.kind(),
            "arithmetic_expansion" | "test_command" | "case_statement"
        )
    }) {
        return false;
    }
    let Some(prefix) = source.get(context.start_byte()..node.start_byte()) else {
        return false;
    };
    let mut quote = None;
    let mut escaped = false;
    for byte in prefix.bytes() {
        if escaped {
            escaped = false;
            continue;
        }
        if byte == b'\\' && quote != Some(b'\'') {
            escaped = true;
            continue;
        }
        quote = match (quote, byte) {
            (Some(b'\''), b'\'') | (Some(b'"'), b'"') => None,
            (None, b'\'') => Some(b'\''),
            (None, b'"') => Some(b'"'),
            (current, _) => current,
        };
    }
    quote.is_none()
}
