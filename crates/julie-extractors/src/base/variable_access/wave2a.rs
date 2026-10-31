use super::{
    Access, AssignmentRule, ChildSelector, ContextRule, FieldRule, UnclassifiedRule,
    VariableAccessRules,
};
use tree_sitter::Node;

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

const SCALA_COMPOUND_OPERATORS: &[&str] = &[
    "&=", "|=", "^=", ">>=", ">>>=", "<<=", "+=", "-=", "*=", "/=", "%=",
];

const SCALA_ASSIGNMENTS: &[AssignmentRule] = &[
    AssignmentRule {
        parent_kind: "assignment_expression",
        required_parent_kind: None,
        required_child: None,
        target: field("left"),
        operator: None,
        write_operators: &[],
        read_write_operators: &[],
    },
    AssignmentRule {
        parent_kind: "infix_expression",
        required_parent_kind: None,
        required_child: None,
        target: field("left"),
        operator: Some(field("operator")),
        write_operators: SCALA_COMPOUND_OPERATORS,
        read_write_operators: SCALA_COMPOUND_OPERATORS,
    },
];

const SCALA_PATTERNS: &[ContextRule] = &[
    ContextRule {
        parent_kind: "case_clause",
        child: field("pattern"),
        access: Access::Write,
        predicate: Some(is_scala_lowercase_binding),
        required_child: None,
    },
    ContextRule {
        parent_kind: "enumerator",
        child: kind("identifier"),
        access: Access::Write,
        predicate: Some(is_scala_enumerator_binding),
        required_child: None,
    },
    ContextRule {
        parent_kind: "enumerator",
        child: kind("tuple_pattern"),
        access: Access::Write,
        predicate: Some(is_scala_enumerator_binding),
        required_child: None,
    },
];

const SCALA_RECEIVERS: &[FieldRule] = &[FieldRule {
    parent_kind: "field_expression",
    child: field("value"),
}];

const SCALA_READ_OVERRIDES: &[ContextRule] = &[ContextRule {
    parent_kind: "assignment_expression",
    child: field("left"),
    access: Access::Read,
    predicate: Some(is_scala_named_argument_label),
    required_child: None,
}];

const SCALA_UNCLASSIFIED: &[UnclassifiedRule] = &[UnclassifiedRule {
    parent_kind: "infix_expression",
    child: field("left"),
    predicate: Some(is_unclassified_scala_assignment_operator),
    required_child: None,
}];

const SCALA_RULES: VariableAccessRules = VariableAccessRules {
    assignments: SCALA_ASSIGNMENTS,
    declarations: &[],
    patterns: SCALA_PATTERNS,
    increments: &[],
    by_reference: &[],
    receivers: SCALA_RECEIVERS,
    read_overrides: SCALA_READ_OVERRIDES,
    unclassified: SCALA_UNCLASSIFIED,
};

const VB_COMPOUND_OPERATORS: &[&str] = &["+=", "-=", "*=", "/=", "\\=", "^=", "&=", "<<=", ">>="];

const VBNET_ASSIGNMENTS: &[AssignmentRule] = &[
    AssignmentRule {
        parent_kind: "assignment_statement",
        required_parent_kind: None,
        required_child: None,
        target: field("left"),
        operator: None,
        write_operators: &[],
        read_write_operators: &[],
    },
    AssignmentRule {
        parent_kind: "compound_assignment_statement",
        required_parent_kind: None,
        required_child: None,
        target: field("left"),
        operator: Some(field("operator")),
        write_operators: VB_COMPOUND_OPERATORS,
        read_write_operators: VB_COMPOUND_OPERATORS,
    },
    AssignmentRule {
        parent_kind: "binary_expression",
        required_parent_kind: Some("call_statement"),
        required_child: None,
        target: field("left"),
        operator: Some(field("operator")),
        write_operators: &["="],
        read_write_operators: &[],
    },
];

const VBNET_PATTERNS: &[ContextRule] = &[
    ContextRule {
        parent_kind: "for_each_statement",
        child: field("variable"),
        access: Access::Write,
        predicate: None,
        required_child: None,
    },
    ContextRule {
        parent_kind: "catch_block",
        child: field("exception"),
        access: Access::Write,
        predicate: None,
        required_child: None,
    },
];

const VBNET_RECEIVERS: &[FieldRule] = &[
    FieldRule {
        parent_kind: "member_access",
        child: field("object"),
    },
    FieldRule {
        parent_kind: "member_access_expression",
        child: field("object"),
    },
    FieldRule {
        parent_kind: "implicit_member_access",
        child: field("object"),
    },
    FieldRule {
        parent_kind: "null_conditional_member_access",
        child: field("object"),
    },
];

const VBNET_RULES: VariableAccessRules = VariableAccessRules {
    assignments: VBNET_ASSIGNMENTS,
    declarations: &[],
    patterns: VBNET_PATTERNS,
    increments: &[],
    by_reference: &[],
    receivers: VBNET_RECEIVERS,
    read_overrides: &[],
    unclassified: &[],
};

const DART_COMPOUND_OPERATORS: &[&str] = &[
    "%=", "&=", "*=", "+=", "-=", "/=", "~/=", "<<=", ">>=", ">>>=", "??=", "^=", "|=",
];

const DART_ASSIGNMENTS: &[AssignmentRule] = &[AssignmentRule {
    parent_kind: "assignment_expression",
    required_parent_kind: None,
    required_child: None,
    target: field("left"),
    operator: Some(field("operator")),
    write_operators: &[
        "=", "%=", "&=", "*=", "+=", "-=", "/=", "~/=", "<<=", ">>=", ">>>=", "??=", "^=", "|=",
    ],
    read_write_operators: DART_COMPOUND_OPERATORS,
}];

const DART_PATTERNS: &[ContextRule] = &[
    ContextRule {
        parent_kind: "variable_pattern",
        child: field("name"),
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
    ContextRule {
        parent_kind: "catch_clause",
        child: field("exception"),
        access: Access::Write,
        predicate: None,
        required_child: None,
    },
    ContextRule {
        parent_kind: "catch_clause",
        child: field("stack_trace"),
        access: Access::Write,
        predicate: None,
        required_child: None,
    },
];

const DART_INCREMENTS: &[ContextRule] = &[
    ContextRule {
        parent_kind: "postfix_expression",
        child: field("argument"),
        access: Access::Write,
        predicate: Some(is_dart_postfix_increment),
        required_child: None,
    },
    ContextRule {
        parent_kind: "unary_expression",
        child: kind("assignable_expression"),
        access: Access::Write,
        predicate: Some(is_dart_prefix_increment),
        required_child: None,
    },
];

const DART_RECEIVERS: &[FieldRule] = &[
    FieldRule {
        parent_kind: "assignable_expression",
        child: field("object"),
    },
    FieldRule {
        parent_kind: "member_expression",
        child: field("object"),
    },
    FieldRule {
        parent_kind: "null_aware_member_expression",
        child: field("object"),
    },
    FieldRule {
        parent_kind: "index_expression",
        child: field("object"),
    },
    FieldRule {
        parent_kind: "null_aware_index_expression",
        child: field("object"),
    },
];

const DART_READ_OVERRIDES: &[ContextRule] = &[
    ContextRule {
        parent_kind: "assignable_expression",
        child: field("index"),
        access: Access::Read,
        predicate: None,
        required_child: None,
    },
    ContextRule {
        parent_kind: "index_expression",
        child: field("index"),
        access: Access::Read,
        predicate: None,
        required_child: None,
    },
    ContextRule {
        parent_kind: "null_aware_index_expression",
        child: field("index"),
        access: Access::Read,
        predicate: None,
        required_child: None,
    },
];

const DART_RULES: VariableAccessRules = VariableAccessRules {
    assignments: DART_ASSIGNMENTS,
    declarations: &[],
    patterns: DART_PATTERNS,
    increments: DART_INCREMENTS,
    by_reference: &[],
    receivers: DART_RECEIVERS,
    read_overrides: DART_READ_OVERRIDES,
    unclassified: &[],
};

const FSHARP_ASSIGNMENTS: &[AssignmentRule] = &[
    AssignmentRule {
        parent_kind: "mutate_expression",
        required_parent_kind: None,
        required_child: None,
        target: kind("long_identifier_or_op"),
        operator: None,
        write_operators: &[],
        read_write_operators: &[],
    },
    AssignmentRule {
        parent_kind: "mutate_expression",
        required_parent_kind: None,
        required_child: None,
        target: kind("dot_expression"),
        operator: None,
        write_operators: &[],
        read_write_operators: &[],
    },
];

const FSHARP_PATTERNS: &[ContextRule] = &[
    ContextRule {
        parent_kind: "rule",
        child: field("pattern"),
        access: Access::Write,
        predicate: Some(is_lowercase_fsharp_binding),
        required_child: None,
    },
    ContextRule {
        parent_kind: "for_expression",
        child: kind("identifier_pattern"),
        access: Access::Write,
        predicate: Some(is_lowercase_fsharp_binding),
        required_child: None,
    },
];

const FSHARP_RECEIVERS: &[FieldRule] = &[FieldRule {
    parent_kind: "dot_expression",
    child: field("base"),
}];

const FSHARP_READ_OVERRIDES: &[ContextRule] = &[ContextRule {
    parent_kind: "mutate_expression",
    child: field("assignee"),
    access: Access::Read,
    predicate: Some(is_fsharp_assignment_receiver),
    required_child: None,
}];

const FSHARP_RULES: VariableAccessRules = VariableAccessRules {
    assignments: FSHARP_ASSIGNMENTS,
    declarations: &[],
    patterns: FSHARP_PATTERNS,
    increments: &[],
    by_reference: &[],
    receivers: FSHARP_RECEIVERS,
    read_overrides: FSHARP_READ_OVERRIDES,
    unclassified: &[],
};

const ERLANG_ASSIGNMENTS: &[AssignmentRule] = &[
    AssignmentRule {
        parent_kind: "match_expr",
        required_parent_kind: None,
        required_child: None,
        target: field("lhs"),
        operator: None,
        write_operators: &[],
        read_write_operators: &[],
    },
    AssignmentRule {
        parent_kind: "cond_match_expr",
        required_parent_kind: None,
        required_child: None,
        target: field("lhs"),
        operator: None,
        write_operators: &[],
        read_write_operators: &[],
    },
    AssignmentRule {
        parent_kind: "generator",
        required_parent_kind: None,
        required_child: None,
        target: field("lhs"),
        operator: None,
        write_operators: &[],
        read_write_operators: &[],
    },
    AssignmentRule {
        parent_kind: "b_generator",
        required_parent_kind: None,
        required_child: None,
        target: field("lhs"),
        operator: None,
        write_operators: &[],
        read_write_operators: &[],
    },
    AssignmentRule {
        parent_kind: "map_generator",
        required_parent_kind: None,
        required_child: None,
        target: field("lhs"),
        operator: None,
        write_operators: &[],
        read_write_operators: &[],
    },
];

const ERLANG_RULES: VariableAccessRules = VariableAccessRules {
    assignments: ERLANG_ASSIGNMENTS,
    declarations: &[],
    patterns: &[],
    increments: &[],
    by_reference: &[],
    receivers: &[],
    read_overrides: ERLANG_READ_OVERRIDES,
    unclassified: &[],
};

const ERLANG_READ_OVERRIDES: &[ContextRule] = &[ContextRule {
    parent_kind: "record_field",
    child: field("name"),
    access: Access::Read,
    predicate: None,
    required_child: None,
}];

const ELIXIR_ASSIGNMENT_OPERATORS: &[&str] = &["=", "<-"];

const ELIXIR_ASSIGNMENTS: &[AssignmentRule] = &[AssignmentRule {
    parent_kind: "binary_operator",
    required_parent_kind: None,
    required_child: None,
    target: field("left"),
    operator: Some(field("operator")),
    write_operators: ELIXIR_ASSIGNMENT_OPERATORS,
    read_write_operators: &[],
}];

const ELIXIR_PATTERNS: &[ContextRule] = &[ContextRule {
    parent_kind: "stab_clause",
    child: field("left"),
    access: Access::Write,
    predicate: None,
    required_child: None,
}];

const ELIXIR_READ_OVERRIDES: &[ContextRule] = &[
    ContextRule {
        parent_kind: "unary_operator",
        child: kind("identifier"),
        access: Access::Read,
        predicate: Some(is_elixir_pin),
        required_child: None,
    },
    ContextRule {
        parent_kind: "binary_operator",
        child: field("left"),
        access: Access::Read,
        predicate: Some(is_elixir_definition_head),
        required_child: None,
    },
];

const ELIXIR_RECEIVERS: &[FieldRule] = &[FieldRule {
    parent_kind: "dot",
    child: field("left"),
}];

const ELIXIR_RULES: VariableAccessRules = VariableAccessRules {
    assignments: ELIXIR_ASSIGNMENTS,
    declarations: &[],
    patterns: ELIXIR_PATTERNS,
    increments: &[],
    by_reference: &[],
    receivers: ELIXIR_RECEIVERS,
    read_overrides: ELIXIR_READ_OVERRIDES,
    unclassified: &[],
};

const LUA_ASSIGNMENTS: &[AssignmentRule] = &[AssignmentRule {
    parent_kind: "assignment_statement",
    required_parent_kind: None,
    required_child: None,
    target: kind("variable_list"),
    operator: Some(field("operator")),
    write_operators: &["="],
    read_write_operators: &[],
}];

const LUA_PATTERNS: &[ContextRule] = &[
    ContextRule {
        parent_kind: "for_generic_clause",
        child: kind("variable_list"),
        access: Access::Write,
        predicate: None,
        required_child: None,
    },
    ContextRule {
        parent_kind: "for_numeric_clause",
        child: field("name"),
        access: Access::Write,
        predicate: None,
        required_child: None,
    },
];

const LUA_RECEIVERS: &[FieldRule] = &[
    FieldRule {
        parent_kind: "dot_index_expression",
        child: field("table"),
    },
    FieldRule {
        parent_kind: "method_index_expression",
        child: field("table"),
    },
    FieldRule {
        parent_kind: "bracket_index_expression",
        child: field("table"),
    },
];

const LUA_READ_OVERRIDES: &[ContextRule] = &[ContextRule {
    parent_kind: "assignment_statement",
    child: kind("variable_list"),
    access: Access::Read,
    predicate: Some(is_lua_local_declaration_target),
    required_child: None,
}];

const LUA_RULES: VariableAccessRules = VariableAccessRules {
    assignments: LUA_ASSIGNMENTS,
    declarations: &[],
    patterns: LUA_PATTERNS,
    increments: &[],
    by_reference: &[],
    receivers: LUA_RECEIVERS,
    read_overrides: LUA_READ_OVERRIDES,
    unclassified: &[],
};

const ZIG_COMPOUND_OPERATORS: &[&str] = &[
    "%=", "&=", "*=", "*%=", "*|=", "+=", "+%=", "+|=", "-=", "-%=", "-|=", "/=", "<<=", "<<|=",
    ">>=", "^=", "|=",
];

const ZIG_VARIABLE_DECLARATION_ASSIGNMENTS: &[AssignmentRule] = &[
    AssignmentRule {
        parent_kind: "variable_declaration",
        required_parent_kind: None,
        required_child: None,
        target: kind("identifier"),
        operator: Some(kind("=")),
        write_operators: &["="],
        read_write_operators: &[],
    },
    AssignmentRule {
        parent_kind: "variable_declaration",
        required_parent_kind: None,
        required_child: None,
        target: kind("identifier"),
        operator: Some(kind("%=")),
        write_operators: &["%="],
        read_write_operators: &["%="],
    },
    AssignmentRule {
        parent_kind: "variable_declaration",
        required_parent_kind: None,
        required_child: None,
        target: kind("identifier"),
        operator: Some(kind("&=")),
        write_operators: &["&="],
        read_write_operators: &["&="],
    },
    AssignmentRule {
        parent_kind: "variable_declaration",
        required_parent_kind: None,
        required_child: None,
        target: kind("identifier"),
        operator: Some(kind("*=")),
        write_operators: &["*="],
        read_write_operators: &["*="],
    },
    AssignmentRule {
        parent_kind: "variable_declaration",
        required_parent_kind: None,
        required_child: None,
        target: kind("identifier"),
        operator: Some(kind("*%=")),
        write_operators: &["*%="],
        read_write_operators: &["*%="],
    },
    AssignmentRule {
        parent_kind: "variable_declaration",
        required_parent_kind: None,
        required_child: None,
        target: kind("identifier"),
        operator: Some(kind("*|=")),
        write_operators: &["*|="],
        read_write_operators: &["*|="],
    },
    AssignmentRule {
        parent_kind: "variable_declaration",
        required_parent_kind: None,
        required_child: None,
        target: kind("identifier"),
        operator: Some(kind("+=")),
        write_operators: &["+="],
        read_write_operators: &["+="],
    },
    AssignmentRule {
        parent_kind: "variable_declaration",
        required_parent_kind: None,
        required_child: None,
        target: kind("identifier"),
        operator: Some(kind("+%=")),
        write_operators: &["+%="],
        read_write_operators: &["+%="],
    },
    AssignmentRule {
        parent_kind: "variable_declaration",
        required_parent_kind: None,
        required_child: None,
        target: kind("identifier"),
        operator: Some(kind("+|=")),
        write_operators: &["+|="],
        read_write_operators: &["+|="],
    },
    AssignmentRule {
        parent_kind: "variable_declaration",
        required_parent_kind: None,
        required_child: None,
        target: kind("identifier"),
        operator: Some(kind("-=")),
        write_operators: &["-="],
        read_write_operators: &["-="],
    },
    AssignmentRule {
        parent_kind: "variable_declaration",
        required_parent_kind: None,
        required_child: None,
        target: kind("identifier"),
        operator: Some(kind("-%=")),
        write_operators: &["-%="],
        read_write_operators: &["-%="],
    },
    AssignmentRule {
        parent_kind: "variable_declaration",
        required_parent_kind: None,
        required_child: None,
        target: kind("identifier"),
        operator: Some(kind("-|=")),
        write_operators: &["-|="],
        read_write_operators: &["-|="],
    },
    AssignmentRule {
        parent_kind: "variable_declaration",
        required_parent_kind: None,
        required_child: None,
        target: kind("identifier"),
        operator: Some(kind("/=")),
        write_operators: &["/="],
        read_write_operators: &["/="],
    },
    AssignmentRule {
        parent_kind: "variable_declaration",
        required_parent_kind: None,
        required_child: None,
        target: kind("identifier"),
        operator: Some(kind("<<=")),
        write_operators: &["<<="],
        read_write_operators: &["<<="],
    },
    AssignmentRule {
        parent_kind: "variable_declaration",
        required_parent_kind: None,
        required_child: None,
        target: kind("identifier"),
        operator: Some(kind("<<|=")),
        write_operators: &["<<|="],
        read_write_operators: &["<<|="],
    },
    AssignmentRule {
        parent_kind: "variable_declaration",
        required_parent_kind: None,
        required_child: None,
        target: kind("identifier"),
        operator: Some(kind(">>=")),
        write_operators: &[">>="],
        read_write_operators: &[">>="],
    },
    AssignmentRule {
        parent_kind: "variable_declaration",
        required_parent_kind: None,
        required_child: None,
        target: kind("identifier"),
        operator: Some(kind("^=")),
        write_operators: &["^="],
        read_write_operators: &["^="],
    },
    AssignmentRule {
        parent_kind: "variable_declaration",
        required_parent_kind: None,
        required_child: None,
        target: kind("identifier"),
        operator: Some(kind("|=")),
        write_operators: &["|="],
        read_write_operators: &["|="],
    },
];

macro_rules! zig_member_assignment {
    ($operator:literal, $read_write:expr) => {
        AssignmentRule {
            parent_kind: "variable_declaration",
            required_parent_kind: None,
            required_child: None,
            target: kind("field_expression"),
            operator: Some(kind($operator)),
            write_operators: &[$operator],
            read_write_operators: $read_write,
        }
    };
}

const ZIG_MEMBER_ASSIGNMENTS: &[AssignmentRule] = &[
    zig_member_assignment!("=", &[]),
    zig_member_assignment!("%=", &["%="]),
    zig_member_assignment!("&=", &["&="]),
    zig_member_assignment!("*=", &["*="]),
    zig_member_assignment!("*%=", &["*%="]),
    zig_member_assignment!("*|=", &["*|="]),
    zig_member_assignment!("+=", &["+="]),
    zig_member_assignment!("+%=", &["+%="]),
    zig_member_assignment!("+|=", &["+|="]),
    zig_member_assignment!("-=", &["-="]),
    zig_member_assignment!("-%=", &["-%="]),
    zig_member_assignment!("-|=", &["-|="]),
    zig_member_assignment!("/=", &["/="]),
    zig_member_assignment!("<<=", &["<<="]),
    zig_member_assignment!("<<|=", &["<<|="]),
    zig_member_assignment!(">>=", &[">>="]),
    zig_member_assignment!("^=", &["^="]),
    zig_member_assignment!("|=", &["|="]),
];

const ZIG_ASSIGNMENTS: &[AssignmentRule] = &[AssignmentRule {
    parent_kind: "assignment_expression",
    required_parent_kind: None,
    required_child: None,
    target: field("left"),
    operator: Some(field("operator")),
    write_operators: &[
        "=", "%=", "&=", "*=", "*%=", "*|=", "+=", "+%=", "+|=", "-=", "-%=", "-|=", "/=", "<<=",
        "<<|=", ">>=", "^=", "|=",
    ],
    read_write_operators: ZIG_COMPOUND_OPERATORS,
}];

const ZIG_PATTERNS: &[ContextRule] = &[ContextRule {
    parent_kind: "payload",
    child: kind("identifier"),
    access: Access::Write,
    predicate: None,
    required_child: None,
}];

const ZIG_READ_OVERRIDES: &[ContextRule] = &[ContextRule {
    parent_kind: "variable_declaration",
    child: kind("identifier"),
    access: Access::Read,
    predicate: Some(is_zig_declaration_name),
    required_child: None,
}];

const ZIG_RECEIVERS: &[FieldRule] = &[
    FieldRule {
        parent_kind: "field_expression",
        child: field("object"),
    },
    FieldRule {
        parent_kind: "index_expression",
        child: field("object"),
    },
];

const ZIG_ALL_ASSIGNMENTS: &[AssignmentRule] = &[
    ZIG_VARIABLE_DECLARATION_ASSIGNMENTS[0],
    ZIG_VARIABLE_DECLARATION_ASSIGNMENTS[1],
    ZIG_VARIABLE_DECLARATION_ASSIGNMENTS[2],
    ZIG_VARIABLE_DECLARATION_ASSIGNMENTS[3],
    ZIG_VARIABLE_DECLARATION_ASSIGNMENTS[4],
    ZIG_VARIABLE_DECLARATION_ASSIGNMENTS[5],
    ZIG_VARIABLE_DECLARATION_ASSIGNMENTS[6],
    ZIG_VARIABLE_DECLARATION_ASSIGNMENTS[7],
    ZIG_VARIABLE_DECLARATION_ASSIGNMENTS[8],
    ZIG_VARIABLE_DECLARATION_ASSIGNMENTS[9],
    ZIG_VARIABLE_DECLARATION_ASSIGNMENTS[10],
    ZIG_VARIABLE_DECLARATION_ASSIGNMENTS[11],
    ZIG_VARIABLE_DECLARATION_ASSIGNMENTS[12],
    ZIG_VARIABLE_DECLARATION_ASSIGNMENTS[13],
    ZIG_VARIABLE_DECLARATION_ASSIGNMENTS[14],
    ZIG_VARIABLE_DECLARATION_ASSIGNMENTS[15],
    ZIG_VARIABLE_DECLARATION_ASSIGNMENTS[16],
    ZIG_VARIABLE_DECLARATION_ASSIGNMENTS[17],
    ZIG_MEMBER_ASSIGNMENTS[0],
    ZIG_MEMBER_ASSIGNMENTS[1],
    ZIG_MEMBER_ASSIGNMENTS[2],
    ZIG_MEMBER_ASSIGNMENTS[3],
    ZIG_MEMBER_ASSIGNMENTS[4],
    ZIG_MEMBER_ASSIGNMENTS[5],
    ZIG_MEMBER_ASSIGNMENTS[6],
    ZIG_MEMBER_ASSIGNMENTS[7],
    ZIG_MEMBER_ASSIGNMENTS[8],
    ZIG_MEMBER_ASSIGNMENTS[9],
    ZIG_MEMBER_ASSIGNMENTS[10],
    ZIG_MEMBER_ASSIGNMENTS[11],
    ZIG_MEMBER_ASSIGNMENTS[12],
    ZIG_MEMBER_ASSIGNMENTS[13],
    ZIG_MEMBER_ASSIGNMENTS[14],
    ZIG_MEMBER_ASSIGNMENTS[15],
    ZIG_MEMBER_ASSIGNMENTS[16],
    ZIG_MEMBER_ASSIGNMENTS[17],
    ZIG_ASSIGNMENTS[0],
];

const ZIG_COMPLETE_RULES: VariableAccessRules = VariableAccessRules {
    assignments: ZIG_ALL_ASSIGNMENTS,
    declarations: &[],
    patterns: ZIG_PATTERNS,
    increments: &[],
    by_reference: &[],
    receivers: ZIG_RECEIVERS,
    read_overrides: ZIG_READ_OVERRIDES,
    unclassified: &[],
};

fn source_text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    node.utf8_text(source.as_bytes()).unwrap_or("")
}

fn is_scala_lowercase_binding(node: Node<'_>, _parent: Node<'_>, source: &str) -> bool {
    let text = source_text(node, source).trim();
    !text.starts_with('`') && text.chars().next().is_some_and(char::is_lowercase)
}

fn is_scala_enumerator_binding(node: Node<'_>, parent: Node<'_>, source: &str) -> bool {
    parent.named_child(0).is_some_and(|first| {
        first.start_byte() <= node.start_byte()
            && node.end_byte() <= first.end_byte()
            && is_scala_lowercase_binding(node, parent, source)
    })
}

fn is_scala_named_argument_label(_node: Node<'_>, parent: Node<'_>, _source: &str) -> bool {
    parent
        .parent()
        .is_some_and(|owner| owner.kind() == "arguments")
}

fn is_unclassified_scala_assignment_operator(
    _node: Node<'_>,
    parent: Node<'_>,
    source: &str,
) -> bool {
    let Some(operator) = parent.child_by_field_name("operator") else {
        return false;
    };
    let text = source_text(operator, source).trim();
    text.ends_with('=')
        && !SCALA_COMPOUND_OPERATORS.contains(&text)
        && !matches!(text, "==" | "!=" | "<=" | ">=" | "===" | "!==")
}

fn is_lowercase_fsharp_binding(node: Node<'_>, _parent: Node<'_>, source: &str) -> bool {
    let text = source_text(node, source).trim();
    !text.starts_with("``") && text.chars().next().is_some_and(char::is_lowercase)
}

fn is_fsharp_assignment_receiver(node: Node<'_>, parent: Node<'_>, source: &str) -> bool {
    let Some(assignee) = parent.child_by_field_name("assignee") else {
        return false;
    };
    if assignee.kind() != "long_identifier_or_op" {
        return false;
    }
    let Some(long_identifier) = assignee
        .named_child(0)
        .filter(|child| child.kind() == "long_identifier")
    else {
        return false;
    };
    let mut cursor = long_identifier.walk();
    let identifiers: Vec<_> = long_identifier
        .named_children(&mut cursor)
        .filter(|child| child.kind() == "identifier")
        .collect();
    identifiers.len() > 1
        && identifiers
            .first()
            .is_some_and(|first| first.id() == node.id())
        && source_text(node, source)
            .chars()
            .next()
            .is_some_and(char::is_lowercase)
}

fn is_elixir_pin(_node: Node<'_>, parent: Node<'_>, source: &str) -> bool {
    parent
        .child_by_field_name("operator")
        .is_some_and(|operator| source_text(operator, source).trim() == "^")
}

fn is_elixir_definition_head(_node: Node<'_>, parent: Node<'_>, source: &str) -> bool {
    if parent
        .child_by_field_name("operator")
        .is_none_or(|operator| source_text(operator, source).trim() != "=")
    {
        return false;
    }
    let mut current = Some(parent);
    while let Some(candidate) = current {
        if candidate.kind() == "do_block" {
            return false;
        }
        if candidate.kind() == "call"
            && candidate
                .parent()
                .is_some_and(|arguments| arguments.kind() == "arguments")
            && candidate
                .parent()
                .and_then(|arguments| arguments.parent())
                .filter(|definition| definition.kind() == "call")
                .and_then(|definition| definition.child_by_field_name("target"))
                .is_some_and(|target| {
                    matches!(
                        source_text(target, source).trim(),
                        "def" | "defp" | "defmacro" | "defmacrop" | "defguard" | "defguardp"
                    )
                })
        {
            return true;
        }
        current = candidate.parent();
    }
    false
}

fn is_zig_declaration_name(node: Node<'_>, parent: Node<'_>, _source: &str) -> bool {
    if !parent
        .named_child(0)
        .is_some_and(|first| first.id() == node.id())
    {
        return false;
    }
    let mut cursor = parent.walk();
    parent
        .children(&mut cursor)
        .any(|child| matches!(child.kind(), "var" | "const" | "comptime"))
}

fn is_lua_local_declaration_target(_node: Node<'_>, parent: Node<'_>, _source: &str) -> bool {
    parent
        .parent()
        .is_some_and(|owner| owner.kind() == "variable_declaration")
}

fn is_dart_postfix_increment(_node: Node<'_>, parent: Node<'_>, source: &str) -> bool {
    let mut cursor = parent.walk();
    parent
        .children(&mut cursor)
        .any(|child| matches!(source_text(child, source).trim(), "++" | "--"))
}

fn is_dart_prefix_increment(_node: Node<'_>, parent: Node<'_>, source: &str) -> bool {
    let mut cursor = parent.walk();
    parent
        .children(&mut cursor)
        .any(|child| matches!(source_text(child, source).trim(), "++" | "--"))
}

pub(super) fn rules(language: &str) -> Option<&'static VariableAccessRules> {
    match language {
        "scala" => Some(&SCALA_RULES),
        "vbnet" => Some(&VBNET_RULES),
        "dart" => Some(&DART_RULES),
        "fsharp" => Some(&FSHARP_RULES),
        "erlang" => Some(&ERLANG_RULES),
        "elixir" => Some(&ELIXIR_RULES),
        "lua" => Some(&LUA_RULES),
        "zig" => Some(&ZIG_COMPLETE_RULES),
        _ => None,
    }
}
