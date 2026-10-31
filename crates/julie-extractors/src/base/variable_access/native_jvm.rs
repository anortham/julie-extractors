use super::{
    Access, AssignmentRule, ChildSelector, ContextRule, DeclarationRule, FieldRule,
    VariableAccessRules,
};

const ASSIGNMENT_OPERATORS: &[&str] = &["="];
const JAVA_COMPOUND_OPERATORS: &[&str] = &[
    "%=", "&=", "*=", "+=", "-=", "/=", "<<=", ">>=", ">>>=", "^=", "|=",
];
const KOTLIN_COMPOUND_OPERATORS: &[&str] = &["%=", "*=", "+=", "-=", "/="];
const CSHARP_COMPOUND_OPERATORS: &[&str] = &[
    "%=", "&=", "*=", "+=", "-=", "/=", "<<=", ">>=", ">>>=", "??=", "^=", "|=",
];
const C_COMPOUND_OPERATORS: &[&str] =
    &["%=", "&=", "*=", "+=", "-=", "/=", "<<=", ">>=", "^=", "|="];
const CPP_COMPOUND_OPERATORS: &[&str] = &[
    "%=", "&=", "*=", "+=", "-=", "/=", "<<=", ">>=", "^=", "|=", "and_eq", "or_eq", "xor_eq",
];

const JAVA_ASSIGNMENTS: &[AssignmentRule] = &[AssignmentRule {
    parent_kind: "assignment_expression",
    required_parent_kind: None,
    required_child: None,
    target: ChildSelector {
        field: Some("left"),
        kind: None,
    },
    operator: Some(ChildSelector {
        field: Some("operator"),
        kind: None,
    }),
    write_operators: ASSIGNMENT_OPERATORS,
    read_write_operators: JAVA_COMPOUND_OPERATORS,
}];

const KOTLIN_ASSIGNMENTS: &[AssignmentRule] = &[AssignmentRule {
    parent_kind: "assignment",
    required_parent_kind: None,
    required_child: None,
    target: ChildSelector {
        field: Some("left"),
        kind: None,
    },
    operator: Some(ChildSelector {
        field: Some("operator"),
        kind: None,
    }),
    write_operators: ASSIGNMENT_OPERATORS,
    read_write_operators: KOTLIN_COMPOUND_OPERATORS,
}];

const CSHARP_ASSIGNMENTS: &[AssignmentRule] = &[AssignmentRule {
    parent_kind: "assignment_expression",
    required_parent_kind: None,
    required_child: None,
    target: ChildSelector {
        field: Some("left"),
        kind: None,
    },
    operator: Some(ChildSelector {
        field: Some("operator"),
        kind: None,
    }),
    write_operators: ASSIGNMENT_OPERATORS,
    read_write_operators: CSHARP_COMPOUND_OPERATORS,
}];

const C_ASSIGNMENTS: &[AssignmentRule] = &[
    AssignmentRule {
        parent_kind: "assignment_expression",
        required_parent_kind: None,
        required_child: None,
        target: ChildSelector {
            field: Some("left"),
            kind: None,
        },
        operator: Some(ChildSelector {
            field: Some("operator"),
            kind: None,
        }),
        write_operators: ASSIGNMENT_OPERATORS,
        read_write_operators: C_COMPOUND_OPERATORS,
    },
    AssignmentRule {
        parent_kind: "initializer_pair",
        required_parent_kind: None,
        required_child: None,
        target: ChildSelector {
            field: Some("designator"),
            kind: None,
        },
        operator: None,
        write_operators: &[],
        read_write_operators: &[],
    },
];

const CPP_ASSIGNMENTS: &[AssignmentRule] = &[
    AssignmentRule {
        parent_kind: "assignment_expression",
        required_parent_kind: None,
        required_child: None,
        target: ChildSelector {
            field: Some("left"),
            kind: None,
        },
        operator: Some(ChildSelector {
            field: Some("operator"),
            kind: None,
        }),
        write_operators: ASSIGNMENT_OPERATORS,
        read_write_operators: CPP_COMPOUND_OPERATORS,
    },
    AssignmentRule {
        parent_kind: "initializer_pair",
        required_parent_kind: None,
        required_child: None,
        target: ChildSelector {
            field: Some("designator"),
            kind: None,
        },
        operator: None,
        write_operators: &[],
        read_write_operators: &[],
    },
];

const JAVA_DECLARATIONS: &[DeclarationRule] = &[DeclarationRule {
    parent_kind: "variable_declarator",
    name: ChildSelector {
        field: Some("name"),
        kind: None,
    },
    initializer: ChildSelector {
        field: Some("value"),
        kind: None,
    },
}];

const KOTLIN_DECLARATIONS: &[DeclarationRule] = &[DeclarationRule {
    parent_kind: "property_declaration",
    name: ChildSelector {
        field: None,
        kind: Some("variable_declaration"),
    },
    initializer: ChildSelector {
        field: None,
        kind: Some("expression"),
    },
}];

const CSHARP_DECLARATIONS: &[DeclarationRule] = &[DeclarationRule {
    parent_kind: "variable_declarator",
    name: ChildSelector {
        field: Some("name"),
        kind: None,
    },
    initializer: ChildSelector {
        field: None,
        kind: Some("equals_value_clause"),
    },
}];

const C_DECLARATIONS: &[DeclarationRule] = &[DeclarationRule {
    parent_kind: "init_declarator",
    name: ChildSelector {
        field: Some("declarator"),
        kind: None,
    },
    initializer: ChildSelector {
        field: Some("value"),
        kind: None,
    },
}];

const CPP_DECLARATIONS: &[DeclarationRule] = &[DeclarationRule {
    parent_kind: "init_declarator",
    name: ChildSelector {
        field: Some("declarator"),
        kind: None,
    },
    initializer: ChildSelector {
        field: Some("value"),
        kind: None,
    },
}];

const JAVA_PATTERNS: &[ContextRule] = &[
    ContextRule {
        parent_kind: "instanceof_expression",
        child: ChildSelector {
            field: Some("name"),
            kind: None,
        },
        access: Access::Write,
        predicate: None,
        required_child: None,
    },
    ContextRule {
        parent_kind: "type_pattern",
        child: ChildSelector {
            field: None,
            kind: Some("identifier"),
        },
        access: Access::Write,
        predicate: None,
        required_child: None,
    },
    ContextRule {
        parent_kind: "record_pattern_component",
        child: ChildSelector {
            field: None,
            kind: Some("identifier"),
        },
        access: Access::Write,
        predicate: None,
        required_child: None,
    },
    ContextRule {
        parent_kind: "enhanced_for_statement",
        child: ChildSelector {
            field: Some("name"),
            kind: None,
        },
        access: Access::Write,
        predicate: None,
        required_child: None,
    },
    ContextRule {
        parent_kind: "catch_formal_parameter",
        child: ChildSelector {
            field: Some("name"),
            kind: None,
        },
        access: Access::Write,
        predicate: None,
        required_child: None,
    },
];

const KOTLIN_PATTERNS: &[ContextRule] = &[
    ContextRule {
        parent_kind: "for_statement",
        child: ChildSelector {
            field: None,
            kind: Some("variable_declaration"),
        },
        access: Access::Write,
        predicate: None,
        required_child: None,
    },
    ContextRule {
        parent_kind: "for_statement",
        child: ChildSelector {
            field: None,
            kind: Some("multi_variable_declaration"),
        },
        access: Access::Write,
        predicate: None,
        required_child: None,
    },
    ContextRule {
        parent_kind: "multi_variable_declaration",
        child: ChildSelector {
            field: None,
            kind: Some("variable_declaration"),
        },
        access: Access::Write,
        predicate: None,
        required_child: None,
    },
    ContextRule {
        parent_kind: "catch_block",
        child: ChildSelector {
            field: None,
            kind: Some("identifier"),
        },
        access: Access::Write,
        predicate: None,
        required_child: None,
    },
];

const CSHARP_PATTERNS: &[ContextRule] = &[
    ContextRule {
        parent_kind: "declaration_pattern",
        child: ChildSelector {
            field: Some("name"),
            kind: None,
        },
        access: Access::Write,
        predicate: None,
        required_child: None,
    },
    ContextRule {
        parent_kind: "var_pattern",
        child: ChildSelector {
            field: Some("name"),
            kind: None,
        },
        access: Access::Write,
        predicate: None,
        required_child: None,
    },
    ContextRule {
        parent_kind: "recursive_pattern",
        child: ChildSelector {
            field: Some("name"),
            kind: None,
        },
        access: Access::Write,
        predicate: None,
        required_child: None,
    },
    ContextRule {
        parent_kind: "tuple_pattern",
        child: ChildSelector {
            field: None,
            kind: Some("identifier"),
        },
        access: Access::Write,
        predicate: None,
        required_child: None,
    },
    ContextRule {
        parent_kind: "parenthesized_variable_designation",
        child: ChildSelector {
            field: None,
            kind: Some("identifier"),
        },
        access: Access::Write,
        predicate: None,
        required_child: None,
    },
    ContextRule {
        parent_kind: "list_pattern",
        child: ChildSelector {
            field: Some("name"),
            kind: None,
        },
        access: Access::Write,
        predicate: None,
        required_child: None,
    },
];

const CPP_PATTERNS: &[ContextRule] = &[ContextRule {
    parent_kind: "catch_clause",
    child: ChildSelector {
        field: Some("parameters"),
        kind: None,
    },
    access: Access::Write,
    predicate: None,
    required_child: None,
}];

const JAVA_INCREMENTS: &[ContextRule] = &[
    ContextRule {
        parent_kind: "update_expression",
        child: ChildSelector {
            field: None,
            kind: Some("identifier"),
        },
        access: Access::Write,
        predicate: None,
        required_child: None,
    },
    ContextRule {
        parent_kind: "update_expression",
        child: ChildSelector {
            field: None,
            kind: Some("field_access"),
        },
        access: Access::Write,
        predicate: None,
        required_child: None,
    },
];

const KOTLIN_INCREMENTS: &[ContextRule] = &[
    ContextRule {
        parent_kind: "unary_expression",
        child: ChildSelector {
            field: Some("argument"),
            kind: None,
        },
        access: Access::Write,
        predicate: None,
        required_child: Some(ChildSelector {
            field: None,
            kind: Some("++"),
        }),
    },
    ContextRule {
        parent_kind: "unary_expression",
        child: ChildSelector {
            field: Some("argument"),
            kind: None,
        },
        access: Access::Write,
        predicate: None,
        required_child: Some(ChildSelector {
            field: None,
            kind: Some("--"),
        }),
    },
];

const CSHARP_INCREMENTS: &[ContextRule] = &[
    ContextRule {
        parent_kind: "postfix_unary_expression",
        child: ChildSelector {
            field: None,
            kind: Some("identifier"),
        },
        access: Access::Write,
        predicate: None,
        required_child: Some(ChildSelector {
            field: None,
            kind: Some("++"),
        }),
    },
    ContextRule {
        parent_kind: "postfix_unary_expression",
        child: ChildSelector {
            field: None,
            kind: Some("member_access_expression"),
        },
        access: Access::Write,
        predicate: None,
        required_child: Some(ChildSelector {
            field: None,
            kind: Some("++"),
        }),
    },
    ContextRule {
        parent_kind: "postfix_unary_expression",
        child: ChildSelector {
            field: None,
            kind: Some("identifier"),
        },
        access: Access::Write,
        predicate: None,
        required_child: Some(ChildSelector {
            field: None,
            kind: Some("--"),
        }),
    },
    ContextRule {
        parent_kind: "postfix_unary_expression",
        child: ChildSelector {
            field: None,
            kind: Some("member_access_expression"),
        },
        access: Access::Write,
        predicate: None,
        required_child: Some(ChildSelector {
            field: None,
            kind: Some("--"),
        }),
    },
    ContextRule {
        parent_kind: "prefix_unary_expression",
        child: ChildSelector {
            field: None,
            kind: Some("identifier"),
        },
        access: Access::Write,
        predicate: None,
        required_child: Some(ChildSelector {
            field: None,
            kind: Some("++"),
        }),
    },
    ContextRule {
        parent_kind: "prefix_unary_expression",
        child: ChildSelector {
            field: None,
            kind: Some("member_access_expression"),
        },
        access: Access::Write,
        predicate: None,
        required_child: Some(ChildSelector {
            field: None,
            kind: Some("++"),
        }),
    },
    ContextRule {
        parent_kind: "prefix_unary_expression",
        child: ChildSelector {
            field: None,
            kind: Some("identifier"),
        },
        access: Access::Write,
        predicate: None,
        required_child: Some(ChildSelector {
            field: None,
            kind: Some("--"),
        }),
    },
    ContextRule {
        parent_kind: "prefix_unary_expression",
        child: ChildSelector {
            field: None,
            kind: Some("member_access_expression"),
        },
        access: Access::Write,
        predicate: None,
        required_child: Some(ChildSelector {
            field: None,
            kind: Some("--"),
        }),
    },
];

const C_INCREMENTS: &[ContextRule] = &[ContextRule {
    parent_kind: "update_expression",
    child: ChildSelector {
        field: Some("argument"),
        kind: None,
    },
    access: Access::Write,
    predicate: None,
    required_child: None,
}];

const CPP_INCREMENTS: &[ContextRule] = &[ContextRule {
    parent_kind: "update_expression",
    child: ChildSelector {
        field: Some("argument"),
        kind: None,
    },
    access: Access::Write,
    predicate: None,
    required_child: None,
}];

const CSHARP_BY_REFERENCE: &[ContextRule] = &[
    ContextRule {
        parent_kind: "argument",
        child: ChildSelector {
            field: None,
            kind: Some("identifier"),
        },
        access: Access::Write,
        predicate: None,
        required_child: Some(ChildSelector {
            field: None,
            kind: Some("ref"),
        }),
    },
    ContextRule {
        parent_kind: "argument",
        child: ChildSelector {
            field: None,
            kind: Some("member_access_expression"),
        },
        access: Access::Write,
        predicate: None,
        required_child: Some(ChildSelector {
            field: None,
            kind: Some("ref"),
        }),
    },
    ContextRule {
        parent_kind: "argument",
        child: ChildSelector {
            field: None,
            kind: Some("identifier"),
        },
        access: Access::Write,
        predicate: None,
        required_child: Some(ChildSelector {
            field: None,
            kind: Some("out"),
        }),
    },
    ContextRule {
        parent_kind: "argument",
        child: ChildSelector {
            field: None,
            kind: Some("member_access_expression"),
        },
        access: Access::Write,
        predicate: None,
        required_child: Some(ChildSelector {
            field: None,
            kind: Some("out"),
        }),
    },
];

const JAVA_RECEIVERS: &[FieldRule] = &[
    FieldRule {
        parent_kind: "field_access",
        child: ChildSelector {
            field: Some("object"),
            kind: None,
        },
    },
    FieldRule {
        parent_kind: "array_access",
        child: ChildSelector {
            field: Some("array"),
            kind: None,
        },
    },
    FieldRule {
        parent_kind: "array_access",
        child: ChildSelector {
            field: Some("index"),
            kind: None,
        },
    },
];

const KOTLIN_RECEIVERS: &[FieldRule] = &[FieldRule {
    parent_kind: "index_expression",
    child: ChildSelector {
        field: None,
        kind: None,
    },
}];

const KOTLIN_READ_OVERRIDES: &[ContextRule] = &[
    ContextRule {
        parent_kind: "navigation_expression",
        child: ChildSelector {
            field: None,
            kind: Some("identifier"),
        },
        access: Access::Read,
        predicate: Some(is_kotlin_navigation_receiver),
        required_child: None,
    },
    ContextRule {
        parent_kind: "navigation_expression",
        child: ChildSelector {
            field: None,
            kind: Some("navigation_expression"),
        },
        access: Access::Read,
        predicate: None,
        required_child: None,
    },
];

const CSHARP_READ_OVERRIDES: &[ContextRule] = &[
    ContextRule {
        parent_kind: "invocation_expression",
        child: ChildSelector {
            field: Some("arguments"),
            kind: None,
        },
        access: Access::Read,
        predicate: None,
        required_child: None,
    },
    ContextRule {
        parent_kind: "prefix_unary_expression",
        child: ChildSelector {
            field: None,
            kind: None,
        },
        access: Access::Read,
        predicate: None,
        required_child: Some(ChildSelector {
            field: None,
            kind: Some("*"),
        }),
    },
];

fn is_kotlin_navigation_receiver(
    node: tree_sitter::Node,
    parent: tree_sitter::Node,
    _source: &str,
) -> bool {
    parent.child(0).is_some_and(|receiver| {
        receiver.start_byte() <= node.start_byte() && node.end_byte() <= receiver.end_byte()
    })
}

const CSHARP_RECEIVERS: &[FieldRule] = &[
    FieldRule {
        parent_kind: "conditional_access_expression",
        child: ChildSelector {
            field: Some("condition"),
            kind: None,
        },
    },
    FieldRule {
        parent_kind: "member_access_expression",
        child: ChildSelector {
            field: Some("expression"),
            kind: None,
        },
    },
    FieldRule {
        parent_kind: "element_access_expression",
        child: ChildSelector {
            field: Some("expression"),
            kind: None,
        },
    },
    FieldRule {
        parent_kind: "element_access_expression",
        child: ChildSelector {
            field: Some("subscript"),
            kind: None,
        },
    },
];

const C_RECEIVERS: &[FieldRule] = &[
    FieldRule {
        parent_kind: "field_expression",
        child: ChildSelector {
            field: Some("argument"),
            kind: None,
        },
    },
    FieldRule {
        parent_kind: "subscript_expression",
        child: ChildSelector {
            field: Some("argument"),
            kind: None,
        },
    },
    FieldRule {
        parent_kind: "subscript_expression",
        child: ChildSelector {
            field: Some("index"),
            kind: None,
        },
    },
    FieldRule {
        parent_kind: "pointer_expression",
        child: ChildSelector {
            field: Some("argument"),
            kind: None,
        },
    },
];

const ARRAY_SIZE_READ_OVERRIDE: ContextRule = ContextRule {
    parent_kind: "array_declarator",
    child: ChildSelector {
        field: Some("size"),
        kind: None,
    },
    access: Access::Read,
    predicate: None,
    required_child: None,
};

const C_READ_OVERRIDES: &[ContextRule] = &[ARRAY_SIZE_READ_OVERRIDE];

const CPP_RECEIVERS: &[FieldRule] = &[
    FieldRule {
        parent_kind: "field_expression",
        child: ChildSelector {
            field: Some("argument"),
            kind: None,
        },
    },
    FieldRule {
        parent_kind: "subscript_expression",
        child: ChildSelector {
            field: Some("argument"),
            kind: None,
        },
    },
    FieldRule {
        parent_kind: "subscript_expression",
        child: ChildSelector {
            field: Some("indices"),
            kind: None,
        },
    },
    FieldRule {
        parent_kind: "pointer_expression",
        child: ChildSelector {
            field: Some("argument"),
            kind: None,
        },
    },
    FieldRule {
        parent_kind: "qualified_identifier",
        child: ChildSelector {
            field: Some("scope"),
            kind: None,
        },
    },
];

const CPP_READ_OVERRIDES: &[ContextRule] = &[
    ContextRule {
        parent_kind: "call_expression",
        child: ChildSelector {
            field: Some("arguments"),
            kind: None,
        },
        access: Access::Read,
        predicate: None,
        required_child: None,
    },
    ARRAY_SIZE_READ_OVERRIDE,
];

const NO_CONTEXT: &[ContextRule] = &[];
const NO_UNCLASSIFIED: &[super::UnclassifiedRule] = &[];

const JAVA: VariableAccessRules = VariableAccessRules {
    assignments: JAVA_ASSIGNMENTS,
    declarations: JAVA_DECLARATIONS,
    patterns: JAVA_PATTERNS,
    increments: JAVA_INCREMENTS,
    by_reference: NO_CONTEXT,
    receivers: JAVA_RECEIVERS,
    read_overrides: NO_CONTEXT,
    unclassified: NO_UNCLASSIFIED,
};

const KOTLIN: VariableAccessRules = VariableAccessRules {
    assignments: KOTLIN_ASSIGNMENTS,
    declarations: KOTLIN_DECLARATIONS,
    patterns: KOTLIN_PATTERNS,
    increments: KOTLIN_INCREMENTS,
    by_reference: NO_CONTEXT,
    receivers: KOTLIN_RECEIVERS,
    read_overrides: KOTLIN_READ_OVERRIDES,
    unclassified: NO_UNCLASSIFIED,
};

const CSHARP: VariableAccessRules = VariableAccessRules {
    assignments: CSHARP_ASSIGNMENTS,
    declarations: CSHARP_DECLARATIONS,
    patterns: CSHARP_PATTERNS,
    increments: CSHARP_INCREMENTS,
    by_reference: CSHARP_BY_REFERENCE,
    receivers: CSHARP_RECEIVERS,
    read_overrides: CSHARP_READ_OVERRIDES,
    unclassified: NO_UNCLASSIFIED,
};

const C: VariableAccessRules = VariableAccessRules {
    assignments: C_ASSIGNMENTS,
    declarations: C_DECLARATIONS,
    patterns: NO_CONTEXT,
    increments: C_INCREMENTS,
    by_reference: NO_CONTEXT,
    receivers: C_RECEIVERS,
    read_overrides: C_READ_OVERRIDES,
    unclassified: NO_UNCLASSIFIED,
};

const CPP: VariableAccessRules = VariableAccessRules {
    assignments: CPP_ASSIGNMENTS,
    declarations: CPP_DECLARATIONS,
    patterns: CPP_PATTERNS,
    increments: CPP_INCREMENTS,
    by_reference: NO_CONTEXT,
    receivers: CPP_RECEIVERS,
    read_overrides: CPP_READ_OVERRIDES,
    unclassified: NO_UNCLASSIFIED,
};

pub(super) fn rules(language: &str) -> Option<&'static VariableAccessRules> {
    match language {
        "java" => Some(&JAVA),
        "kotlin" => Some(&KOTLIN),
        "csharp" => Some(&CSHARP),
        "c" => Some(&C),
        "cpp" => Some(&CPP),
        _ => None,
    }
}
