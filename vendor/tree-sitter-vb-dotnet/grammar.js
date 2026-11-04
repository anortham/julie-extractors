/**
 * @file Tree sitter grammar for VB.NET
 * @author CodeAnt AI <chinmay@codeant.ai>
 * @license MIT
 */

/// <reference types="tree-sitter-cli/dsl" />
// @ts-check

// Tree-sitter grammar for Visual Basic .NET, based on the official VB.NET language specification.
module.exports = grammar({
  name: 'vb_dotnet',

  word: $ => $.identifier,

  extras: $ => [
    $.comment,
    /[ \t\f\u00A0]+/,        // whitespace except newlines
    $._line_continuation
  ],

  
  conflicts: $ => [
    // `New Type(` — the ( is ambiguous between argument_list and array_rank_specifier
    [$.new_expression],
    // property body: auto-property (_terminator) vs block with Get/Set — GLR tries both at the choice
    [$.property_declaration],
    // constructor body: `Sub New(...)` followed by _terminator is ambiguous between body-less and body form
    [$.constructor_declaration],
    // method vs abstract_method: identical prefix up through the header _terminator; GLR picks the one that parses a valid body
    [$.method_declaration, $.abstract_method_declaration],
    // `For ident =` — identifier could start an expression or be the loop variable assignment
    [$.for_statement, $._expression],
    // `ident` at statement start — could be a left_hand_side (assignment target) or start of an expression statement
    [$.left_hand_side, $._expression],
    // `ident:` — could be a label_statement or start of an expression followed by something else
    [$.label_statement, $._expression],
    // `100:` — integer_literal could be a numeric label or the start of a literal expression
    [$.label_statement, $._literal],
    // event body: standard form (_terminator) vs custom event with AddHandler/RemoveHandler blocks
    [$.event_declaration],
    // single-line vs block If: after `If expr Then` the parser must choose between both forms
    [$.if_statement],
    // `primitive_type ?` — could be nullable_type or reduce _type and shift ? for null_conditional_element_access
    [$._type, $.nullable_type],
    // `Name(` — namespace_name in _type could also be the prefix of an array_type with rank specifier
    [$._type, $.array_type],
    // `Name(` — three-way: namespace_name alone vs array_type suffix vs generic_type suffix
    [$._type, $.array_type, $.generic_type],
    // `If expr Then expr Op` — in single-line If, the binary op could extend the condition or the then-branch expression
    [$.if_statement, $.binary_expression],
    // blank line before block If: empty_statement (_terminator) vs if_statement starting at the newline
    [$.empty_statement, $.if_statement],
    // `Qualified.Name` in attribute — namespace_name overlaps with the dotted attribute name
    [$.namespace_name, $.attribute],
    // `A.B.C` — each `.identifier` extension is ambiguous: reduce namespace_name or shift another `.`
    [$.namespace_name],
    // `A.B(` — namespace_name could reduce for member_access expression or continue as dotted name
    [$.namespace_name, $._expression],
    // `(expr` — opening paren is ambiguous between array_rank_specifier and argument_list
    [$.array_rank_specifier, $.argument_list],
    // `(expr` — opening paren is ambiguous between array_rank_specifier and a single argument
    [$.array_rank_specifier, $.argument],
    // `<attr> _\n<attr>` — attribute_block terminator is optional; next `<` could be another attribute or the decorated declaration
    [$.attribute_block],
    // top-level `<attr>` before a type — attribute_block appears in both source_file and type_declaration
    [$.source_file, $.type_declaration],
    // namespace-level `<attr>` before a type — attribute_block appears in both namespace_block and type_declaration
    [$.namespace_block, $.type_declaration],
    // leading blank lines: could be absorbed by source_file or by option_statements repeat
    [$.source_file, $.option_statements],
    // `End Namespace` with optional _terminator vs next namespace-level element starting at the newline
    [$.namespace_block],
    // `End Class` with optional _terminator vs next top-level element at the newline
    [$.class_block],
    // `End Module` with optional _terminator vs next top-level element at the newline
    [$.module_block],
    // `End Structure` with optional _terminator vs next top-level element at the newline
    [$.structure_block],
    // `End Interface` with optional _terminator vs next top-level element at the newline
    [$.interface_block],
    // `End Enum` with optional _terminator vs next top-level element at the newline
    [$.enum_block],
    // `<attr>` inside class — could belong to _member_declaration's repeat or to a nested type_declaration via class_block
    [$.class_block, $._member_declaration],
    // `<attr>` inside structure — same attribute ownership ambiguity as class_block
    [$.structure_block, $._member_declaration],
    // `<attr>` inside interface — same attribute ownership ambiguity as class_block
    [$.interface_block, $._member_declaration],
    // `If(expr, expr)` — ternary (2-arg If) vs tuple_expression; GLR disambiguates via the If keyword
    [$.tuple_expression, $.ternary_expression],
    // bare `End` as end_statement vs `End X` block terminators — GLR disambiguates
    [$.catch_block],
    [$.finally_block],
    [$.else_clause],
    [$.elseif_clause],
    [$.case_block],
    [$.case_else_block],

    // LINQ query expression conflicts. At the end of every clause's trailing
    // expression the parser can either extend the expression (element_access,
    // invocation, member_access, binary_expression) or reduce and look for
    // the next clause keyword. GLR tries both.
    //
    // Self-conflicts: the clause's trailing expression ends at a token that
    // could be a LINQ keyword (Where, Select, etc.), so the parser must
    // choose between reducing the clause or extending the expression.
    [$.from_clause],
    [$.where_clause],
    [$.select_clause],
    [$.order_by_clause],
    [$._ordering],
    [$.let_clause],
    [$.distinct_clause],
    [$.take_clause],
    [$.skip_clause],
    [$.take_while_clause],
    [$.skip_while_clause],
    [$.join_clause],
    [$.group_join_clause],
    [$.group_by_clause],
    [$.aggregate_clause],
    // Cross-conflicts: a LINQ clause's trailing expression contains a
    // sub-expression (element_access, binary_expression) whose continuation
    // token could also be a LINQ keyword. GLR must try both extending the
    // sub-expression and reducing it to start the next clause.
    // Note: invocation and member_access variants are not needed because
    // those sub-expression forms do not create actual LR conflicts with
    // the clause reduction.
    [$.group_by_clause, $.element_access],
    [$.group_by_clause, $.binary_expression],
    [$.join_clause, $.element_access],
    [$.join_clause, $.binary_expression],
    [$.group_join_clause, $.element_access],
    [$.group_join_clause, $.binary_expression],
    [$._join_condition, $.element_access],
    [$._join_condition, $.binary_expression],
    [$._aggregation_item, $.element_access],
    [$._aggregation_item, $.binary_expression],
    [$._expression, $._select_item],
    [$._expression, $._aggregation_item],
    [$._select_item, $.element_access],
    [$._select_item, $.binary_expression],
    [$.where_clause, $.element_access],
    [$.where_clause, $.binary_expression],
    [$.collection_range_variable, $.element_access],
    [$.collection_range_variable, $.binary_expression],
    [$.take_clause, $.element_access],
    [$.take_clause, $.binary_expression],
    [$.skip_clause, $.element_access],
    [$.skip_clause, $.binary_expression],
    [$.take_while_clause, $.element_access],
    [$.take_while_clause, $.binary_expression],
    [$.skip_while_clause, $.element_access],
    [$.skip_while_clause, $.binary_expression],
    [$._ordering, $.element_access],
    [$._ordering, $.binary_expression],
    [$.let_clause, $.element_access],
    [$.let_clause, $.binary_expression],
  ],

  rules: {
    
    source_file: $ => seq(
    optional($._bom),
    // Auto-generated files start with a multi-line `' <auto-generated>` comment
    // block followed by blank lines and then Option / Imports. Comments are
    // in extras, but the newlines between them are not, so we need to absorb
    // them explicitly as blank_line entries. One unified repeat handles
    // blank lines, option statements, and imports interleaved.
    optional($.option_statements),
    repeat(choice(
      $.imports_statement,
      $.attribute_block,
      $.namespace_block,
      $.type_declaration,
      // Preprocessor directives (`#If`, `#Const`, etc.) can appear at the top
      // level in auto-generated files like MyWebExtension.vb.
      $._preprocessor_directive,
      alias($._terminator, $.blank_line)
    ))
  ),

    // UTF-8 BOM (EF BB BF) — Visual Studio emits this in VB.NET files
    _bom: $ => /\xEF\xBB\xBF/,

    
    // Option statements can be preceded by blank lines and comment blocks
    // (comments are in extras). Leading blank lines absorb the newlines
    // between the `' <auto-generated>` header comments in Designer files.
    option_statements: $ => seq(
      repeat(alias($._terminator, $.blank_line)),
      repeat1(
        seq(
          kw('Option'),
          choice(
            seq(kw('Explicit'), choice(kw('On'), kw('Off'))),
            seq(kw('Strict'), choice(kw('On'), kw('Off'))),
            seq(kw('Infer'), choice(kw('On'), kw('Off'))),
            seq(kw('Compare'), choice(kw('Binary'), kw('Text')))
          ),
          $._terminator
        )
      )
    ),

    // Imports statement for namespace imports (can import multiple namespaces in one line)
    imports_statement: $ => seq(
      kw('Imports'),
      commaSep1(choice(
        seq(field('alias', $.identifier), '=', field('namespace', $.namespace_name)),
        field('namespace', $.namespace_name)
      )),
      $._terminator
    ),

    // A dot-separated name (for namespaces or qualified types)
    namespace_name: $ => seq($.identifier, repeat(seq('.', $.identifier))),

    // Any top-level type or namespace declaration
    type_declaration: $ => seq(
      repeat(field('attributes', $.attribute_block)),
      choice(
        $.class_block,
        $.module_block,
        $.structure_block,
        $.interface_block,
        $.enum_block,
        $.delegate_declaration
      )
    ),

    // Namespace block: Namespace Name ... End Namespace
    // namespace_block: $ => seq(
    //   kw('Namespace'),
    //   field('name', $.namespace_name),
    //   $._terminator,
    //   repeat(choice($.attribute_block, $.type_declaration, $.namespace_block)),
    //   kw('End'), kw('Namespace'), optional($._terminator)
    // ),
    namespace_block: $ => seq(
      kw('Namespace'),
      field('name', $.namespace_name),
      $._terminator,
      repeat(choice(
        $.attribute_block, 
        $.type_declaration, 
        $.namespace_block,
        $.imports_statement,  // Allow imports inside namespace
        alias($._terminator, $.blank_line)  // Allow blank lines
      )),
      kw('End'), kw('Namespace'), optional($._terminator)
    ),

    // Class definition block. VB allows blank lines / comment-only lines
    // between the class header and the Inherits / Implements clauses, and
    // multiple Implements clauses each on their own line.
    class_block: $ => seq(
      field('modifiers', optional($.modifiers)),
      kw('Class'),
      field('name', $.identifier),
      optional($.type_parameters),
      $._terminator,
      repeat(alias($._terminator, $.blank_line)),
      optional(seq(field('inherits', $.inherits_clause), $._terminator)),
      repeat(alias($._terminator, $.blank_line)),
      repeat(seq(field('implements', $.implements_clause), $._terminator)),
      repeat($._member_declaration),
      kw('End'), kw('Class'), optional($._terminator)
    ),

    // Module definition block
    module_block: $ => seq(
      field('modifiers', optional($.modifiers)),
      kw('Module'),
      field('name', $.identifier),
      $._terminator,
      repeat($._member_declaration),
      kw('End'), kw('Module'), optional($._terminator)
    ),

    // Structure definition block
    structure_block: $ => seq(
      field('modifiers', optional($.modifiers)),
      kw('Structure'),
      field('name', $.identifier),
      optional($.type_parameters),
      $._terminator,
      repeat(alias($._terminator, $.blank_line)),
      repeat(seq(field('implements', $.implements_clause), $._terminator)),
      repeat($._member_declaration),
      kw('End'), kw('Structure'), optional($._terminator)
    ),

    // Interface definition block
    interface_block: $ => seq(
      field('modifiers', optional($.modifiers)),
      kw('Interface'),
      field('name', $.identifier),
      optional($.type_parameters),
      $._terminator,
      repeat(alias($._terminator, $.blank_line)),
      optional(seq(field('inherits', $.inherits_clause), $._terminator)), // interfaces can inherit multiple interfaces
      repeat($._member_declaration),
      kw('End'), kw('Interface'), optional($._terminator)
    ),

    // Enum definition block. VB allows an explicit underlying type:
    //   Public Enum LkTrxType As Integer
    enum_block: $ => seq(
      field('modifiers', optional($.modifiers)),
      kw('Enum'),
      field('name', $.identifier),
      optional(seq(kw('As'), field('underlying_type', $._type))),
      $._terminator,
      repeat(choice($.enum_member, alias($._terminator, $.blank_line))),
      kw('End'), kw('Enum'), optional($._terminator)
    ),
    enum_member: $ => seq(
      field('name', $.identifier),
      optional(seq('=', field('value', $._expression))),
      $._terminator
    ),

    // Delegate declaration (treated as a type declaration, no End block since it's a single line)
    delegate_declaration: $ => seq(
      field('modifiers', optional($.modifiers)),
      kw('Delegate'),
      choice(kw('Sub'), kw('Function')),
      field('name', $.identifier),
      optional($.type_parameters),
      field('parameters', $.parameter_list),
      optional(seq(kw('As'), field('return_type', $._type))),  // only for Function delegate
      $._terminator
    ),

    // Inheritance clause (for classes or interfaces)
    inherits_clause: $ => seq(kw('Inherits'), commaSep1($._type)),
    // Implements clause (for classes or structures implementing interfaces)
    implements_clause: $ => seq(kw('Implements'), commaSep1($._type)),

    // Generic type parameter definitions: e.g., (Of T As {Constraint})
    type_parameters: $ => seq(
      '(', kw('Of'),
      commaSep1($.type_parameter),
      ')'
    ),
    type_parameter: $ => seq(
      field('name', $.identifier),
      optional(seq(kw('As'), field('constraint', $.type_constraint)))
    ),
    // A single constraint, or a braced list of constraints:
    //   (Of T As New)
    //   (Of T As IDisposable)
    //   (Of T As {Class, New})
    //   (Of T As {IDisposable, New})
    type_constraint: $ => choice(
      $._single_constraint,
      seq('{', commaSep1($._single_constraint), '}')
    ),
    _single_constraint: $ => choice(
      $._type,
      kw('Structure'),
      kw('Class'),
      kw('New')  // type must have a public parameterless constructor
    ),

    // Attributes: <...> blocks attached to declarations
    attribute_block: $ => seq(
      '<',
      commaSep1($.attribute),
      '>',
      optional($._terminator)
    ),
    // attribute: $ => seq(
    //   optional(seq(field('target', $.identifier), ':')),  // e.g., Assembly: or Module: target
    //   field('name', $.identifier),
    //   optional($.argument_list)  // arguments in parentheses
    // ),
    attribute: $ => seq(
      optional(seq(field('target', $.identifier), ':')),
      field('name', choice($.identifier, $.namespace_name)),
      optional($.argument_list)
    ),

    // Modifiers (public/private/etc.). Multiple modifiers can appear in sequence.
    modifiers: $ => repeat1($.modifier),
    // NOTE: `Static` is NOT in this list. VB only allows `Static` on local
    // variables (inside a method), never on fields or methods. Keeping it
    // out of the modifier list lets dim_statement be the only consumer of
    // `Static`, which makes `Static Dim counter` parse correctly as a
    // local variable declaration instead of getting ambiguously interpreted
    // as a class-level field with a Static modifier.
    modifier: $ => token(choice(
      kw('Public'), kw('Private'), kw('Protected'), kw('Friend'),
      kw('Shared'), kw('Shadows'),
      kw('Overloads'), kw('Overrides'), kw('Overridable'), kw('NotOverridable'), kw('MustOverride'),
      kw('MustInherit'), kw('NotInheritable'),
      kw('Partial'), kw('Narrowing'), kw('Widening'),
      kw('Default'),              // for default properties
      kw('ReadOnly'), kw('WriteOnly'),
      kw('WithEvents'), kw('Async'), kw('Iterator')
    )),

    // Type member declarations inside class/module/etc.
    _member_declaration: $ => choice(
      alias($._terminator, $.blank_line),
      $.const_declaration,
      $.field_declaration,
      $.method_declaration,
      $.abstract_method_declaration,  // MustOverride in MustInherit classes
      $.constructor_declaration,
      $.property_declaration,
      $.event_declaration,
      $.operator_declaration,
      $.declare_statement,      // P/Invoke: Declare Function X Lib "dll.dll" ...
      $.type_declaration,       // nested classes, structures, enums, interfaces, delegates
      $._preprocessor_directive  // #Region, #End Region, #If, etc.
    ),

    // P/Invoke declaration. Equivalent to a Shared method with a DllImport
    // attribute; takes the form:
    //   [attributes] [modifiers] Declare [Ansi|Unicode|Auto] Sub|Function
    //     Name Lib "libname" [Alias "entrypoint"] (parameters) [As ReturnType]
    declare_statement: $ => seq(
      repeat(field('attributes', $.attribute_block)),
      field('modifiers', optional($.modifiers)),
      kw('Declare'),
      optional(field('charset', choice(kw('Ansi'), kw('Unicode'), kw('Auto')))),
      choice(kw('Sub'), kw('Function')),
      field('name', $.identifier),
      kw('Lib'),
      field('library', $.string_literal),
      optional(seq(kw('Alias'), field('alias', $.string_literal))),
      field('parameters', $.parameter_list),
      optional(seq(kw('As'), field('return_type', $._type))),
      $._terminator
    ),

    // Constant definitions (inside classes or procedures)
    const_declaration: $ => seq(
      repeat(field('attributes', $.attribute_block)),
      field('modifiers', optional($.modifiers)),
      kw('Const'),
      commaSep1(seq(
        field('name', $.identifier),
        optional($.as_clause),
        '=', 
        field('value', $._expression)
      )),
      $._terminator
    ),

    // Field (variable) declarations inside a type body. VB requires at
    // least one of `Dim` or one or more modifiers on a field declaration.
    // Keeping both optional would let a bare `Foo(x)` call inside a method
    // body get misparsed as a class-level field if the Sub's body
    // interpretation gets cut short by GLR.
    field_declaration: $ => seq(
      repeat(field('attributes', $.attribute_block)),
      choice(
        // With modifiers, Dim is optional.
        seq(field('modifiers', $.modifiers), optional(kw('Dim'))),
        // Without modifiers, Dim is required.
        kw('Dim')
      ),
      commaSep1($.variable_declarator),
      $._terminator
    ),
    variable_declarator: $ => seq(
      field('name', $.identifier),
      optional($.array_rank_specifier),
      choice(
        seq(kw('As'), field('value', $.new_expression)),           // As New Type[(args)]
        // VB allows an implicit line continuation after `=` in a field
        // initializer, so the expression can start on the next line.
        seq(optional($.as_clause), optional(seq('=', optional($._newline), field('initializer', $._expression))))
      )
    ),
    array_rank_specifier: $ => seq('(', optional(commaSep(optional($._expression))), ')'),  // e.g. "()", "(,)", "(5)", "(n - 1, 1)"
    as_clause: $ => seq(kw('As'), field('type', $._type)),

    // Type (for variables, parameters, return types, etc.)
    _type: $ => choice(
      $.array_type,
      $.nullable_type,
      $.primitive_type,
      $.generic_type,
      $.tuple_type,
      $.namespace_name,
    ),

    array_type: $ => prec.left(seq(
      field('element', choice(
        $.nullable_type,
        $.primitive_type,
        $.generic_type,
        $.namespace_name,
        $.array_type,
      )),
      field('rank', $.array_rank_specifier),
    )),

    nullable_type: $ => seq(choice($.primitive_type, $.generic_type, $.namespace_name), '?'),

    tuple_type: $ => seq(
      '(',
      $._tuple_element,
      repeat1(seq(',', $._tuple_element)),
      ')'
    ),

    _tuple_element: $ => choice(
      seq(field('name', $.identifier), kw('As'), field('type', $._type)),
      field('type', $._type)
    ),

    generic_type: $ => seq(
      $.namespace_name,
      $.type_argument_list,
    ),

    primitive_type: $ => token(choice(  // built-in types
      kw('Boolean'), kw('Byte'), kw('SByte'),
      kw('Short'), kw('UShort'), kw('Integer'), kw('UInteger'),
      kw('Long'), kw('ULong'),
      kw('Single'), kw('Double'), kw('Decimal'),
      kw('Char'), kw('String'),
      kw('Object'), kw('Date')
    )),
    type_argument_list: $ => seq(
      '(', kw('Of'),
      commaSep1($._type),
      ')'
    ),

    // Method (Sub/Function) declaration inside a class/module. Always has
    // a body terminated by End Sub / End Function. Interface methods and
    // MustOverride abstract methods use the abstract_method_declaration
    // rule below, which omits the body entirely.
    //
    // The [$.method_declaration, $.abstract_method_declaration] entry in
    // the conflicts array handles GLR disambiguation between these two
    // rules (same prefix up through the header terminator).
    method_declaration: $ => seq(
      repeat(field('attributes', $.attribute_block)),
      field('modifiers', optional($.modifiers)),
      choice(kw('Sub'), kw('Function')),
      field('name', $.identifier),
      optional($.type_parameters),
      field('parameters', $.parameter_list),
      // VB also allows attributes on the return type, e.g.
      //   `As <XmlElementAttribute("…")> String` — common in WSDL proxies.
      optional(seq(
        kw('As'),
        optional(field('return_type_attributes', $.attribute_block)),
        field('return_type', $._type),
      )),  // only for Function
      optional(field('handles', $.handles_clause)),            // Handles Button.Click, ...
      optional(field('implements', $.implements_clause)),      // Implements IFoo.Bar
      $._terminator,
      repeat($._statement),
      kw('End'), choice(kw('Sub'), kw('Function')), $._terminator
    ),

    // Abstract / interface method — no body, no End Sub.
    abstract_method_declaration: $ => seq(
      repeat(field('attributes', $.attribute_block)),
      field('modifiers', optional($.modifiers)),
      choice(kw('Sub'), kw('Function')),
      field('name', $.identifier),
      optional($.type_parameters),
      field('parameters', $.parameter_list),
      optional(seq(
        kw('As'),
        optional(field('return_type_attributes', $.attribute_block)),
        field('return_type', $._type),
      )),
      optional(field('implements', $.implements_clause)),
      $._terminator
    ),

    // Handles clause: Sub Click(...) Handles Button.Click, OtherButton.Click
    handles_clause: $ => seq(kw('Handles'), commaSep1($.namespace_name)),

    // Constructor (Sub New) declaration
    constructor_declaration: $ => seq(
      repeat(field('attributes', $.attribute_block)),
      field('modifiers', optional($.modifiers)),
      kw('Sub'), kw('New'),
      field('parameters', $.parameter_list),
      choice(
        seq($._terminator, repeat($._statement), kw('End'), kw('Sub'), $._terminator),
        $._terminator
      )
    ),

    // Property declaration (auto or with getters/setters)
    property_declaration: $ => seq(
      repeat(field('attributes', $.attribute_block)),
      field('modifiers', optional($.modifiers)),
      kw('Property'),
      field('name', $.identifier),
      optional(field('parameters', $.parameter_list)),  // indexed properties
      optional($.as_clause),
      // Properties can implement interface members:
      //   Public Property Depth() As Integer Implements IDataReader.Depth
      optional(field('implements', $.implements_clause)),
      choice(
        // Auto-property or declaration without body:
        seq(optional(seq('=', field('initializer', $._expression))), $._terminator),
        // Property with Get/Set accessors. Blank lines between accessors are
        // common in real code, so we absorb them as blank_line entries.
        seq(
          $._terminator,
          repeat(alias($._terminator, $.blank_line)),
          optional($.get_accessor),
          repeat(alias($._terminator, $.blank_line)),
          optional($.set_accessor),
          repeat(alias($._terminator, $.blank_line)),
          kw('End'), kw('Property'), $._terminator
        )
      )
    ),
    get_accessor: $ => seq(
      // Attributes (e.g. `<DebuggerHidden()>`) can appear on the accessor
      // itself, one per line, before any modifiers. My Project generated code
      // does this heavily.
      repeat(field('attributes', $.attribute_block)),
      optional(field('modifiers', optional($.modifiers))),
      kw('Get'), $._terminator,
      repeat($._statement),
      kw('End'), kw('Get'), $._terminator
    ),
    set_accessor: $ => seq(
      repeat(field('attributes', $.attribute_block)),
      optional(field('modifiers', optional($.modifiers))),
      kw('Set'),
      optional(field('parameters', $.parameter_list)),  // Set can have a Value parameter
      $._terminator,
      repeat($._statement),
      kw('End'), kw('Set'), $._terminator
    ),

    // Event declaration (regular or custom event with add/remove/raise handlers)
    event_declaration: $ => seq(
      repeat(field('attributes', $.attribute_block)),
      field('modifiers', optional($.modifiers)),
      optional(kw('Custom')),
      kw('Event'),
      field('name', $.identifier),
      choice(
        // Standard event declaration
        seq(optional($.parameter_list), optional($.as_clause), $._terminator),
        // Custom event with handlers
        seq(
          optional($.parameter_list), $.as_clause, $._terminator,
          repeat(choice($.add_handler_block, $.remove_handler_block, $.raise_event_block)),
          kw('End'), kw('Event'), $._terminator
        )
      )
    ),
    add_handler_block: $ => seq(
      optional(field('modifiers', optional($.modifiers))),
      kw('AddHandler'), optional(field('parameters', $.parameter_list)), $._terminator,
      repeat($._statement),
      kw('End'), kw('AddHandler'), $._terminator
    ),
    remove_handler_block: $ => seq(
      optional(field('modifiers', optional($.modifiers))),
      kw('RemoveHandler'), optional(field('parameters', $.parameter_list)), $._terminator,
      repeat($._statement),
      kw('End'), kw('RemoveHandler'), $._terminator
    ),
    raise_event_block: $ => seq(
      optional(field('modifiers', optional($.modifiers))),
      kw('RaiseEvent'), optional(field('parameters', $.parameter_list)), $._terminator,
      repeat($._statement),
      kw('End'), kw('RaiseEvent'), $._terminator
    ),

    operator_declaration: $ => seq(
      repeat(field('attributes', $.attribute_block)),
      field('modifiers', optional($.modifiers)),
      optional(choice(kw('Widening'), kw('Narrowing'))),
      kw('Operator'),
      field('operator', choice(
        '+', '-', '*', '/', '\\', '^', '&',
        '=', '<>', '<', '>', '<=', '>=',
        kw('And'), kw('Or'), kw('Xor'), kw('Not'),
        kw('IsTrue'), kw('IsFalse'),
        '<<', '>>',
        kw('Mod'), kw('Like'), kw('CType')
      )),
      field('parameters', $.parameter_list),
      optional(seq(kw('As'), field('return_type', $._type))),
      $._terminator,
      repeat($._statement),
      kw('End'), kw('Operator'), $._terminator
    ),

    // Parameter list for methods/delegates (Parentheses with comma-separated parameters)
    parameter_list: $ => seq('(', optional(commaSep($.parameter)), ')'),
    // parameter: $ => seq(
    //   optional(choice(kw('ByVal'), kw('ByRef'), kw('ParamArray'))),
    //   field('name', $.identifier),
    //   optional($.array_rank_specifier),
    //   optional($.as_clause),
    //   optional(seq('=', field('default_value', $._expression)))  // default parameter value
    // ),
    parameter: $ => seq(
      repeat($.attribute_block),
      optional(kw('Optional')),
      optional(choice(kw('ByVal'), kw('ByRef'), kw('ParamArray'))),
      field('name', $.identifier),
      optional($.array_rank_specifier),
      optional($.as_clause),
      optional(seq('=', field('default_value', $._expression)))
    ),

    // *** Statements (inside procedures or blocks) ***

    _statement: $ => choice(
      $.empty_statement,
      $.label_statement,
      $.dim_statement,
      $.const_declaration,      // local constant
      prec(1, $.assignment_statement),
      $.call_statement,
      $.if_statement,
      $.select_case_statement,
      $.while_statement,
      $.do_statement,
      $.for_statement,
      $.for_each_statement,
      $.try_statement,
      $.with_statement,
      $.using_statement,
      $.sync_lock_statement,
      $.return_statement,
      $.exit_statement,
      $.continue_statement,
      $.throw_statement,
      $.goto_statement,
      $.redim_statement,
      $.erase_statement,
      $._preprocessor_directive,  // allow preprocessor directives in code flow
      $.compound_assignment_statement,
      $.yield_statement,
      $.on_error_statement,
      $.resume_statement,
      $.add_handler_statement,
      $.remove_handler_statement,
      $.raise_event_statement,
      $.stop_statement,
      $.end_statement,
    ),

    empty_statement: $ => prec(1, $._terminator),  // a standalone line break (no-op statement)

    label_statement: $ => seq(field('label', choice($.identifier, $.integer_literal)), ':'),

    // `Static Dim x` is a legal variant — Static optionally precedes Dim,
    // or Static can stand alone (with an implicit Dim).
    dim_statement: $ => seq(
      choice(
        seq(optional(kw('Static')), kw('Dim')),
        kw('Static')
      ),
      choice(
        // Standard: Dim x As Type = expr, y As Type = expr
        commaSep1(seq(
          field('name', $.identifier),
          optional($.array_rank_specifier),
          choice(
            seq(kw('As'), field('value', $.new_expression)),           // As New Type[(args)]
            seq(optional($.as_clause), optional(seq('=', optional($._newline), field('initializer', $._expression))))
          )
        )),
        // Tuple deconstruction: Dim (a, b) = expr
        seq(
          '(',
          commaSep1(field('name', $.identifier)),
          ')',
          '=',
          field('initializer', $._expression)
        )
      ),
      $._terminator
    ),

    compound_assignment_statement: $ => seq(
      field('left', $.left_hand_side),
      field('operator', choice('+=', '-=', '*=', '/=', '\\=', '^=', '&=', '<<=', '>>=')),
      field('right', $._expression),
      $._terminator
    ),

    assignment_statement: $ => seq(
      field('left', $.left_hand_side),
      '=',
      field('right', $._expression),
      $._terminator
    ),

    left_hand_side: $ => choice(
      $.identifier,
      $.member_access,
      $.implicit_member_access,
      $.element_access,
      // Indexed properties parse as `invocation` (e.g. `dr("Col") &= ...`).
      $.invocation
    ),

    call_statement: $ => seq(
      choice(seq(kw('Call'), $._expression), $._expression),
      $._terminator
    ),

    // if_statement: $ => choice(
    //   // Single-line If
    //   seq(
    //     kw('If'), field('condition', $._expression), kw('Then'),
    //     field('then_branch', $._statement),
    //     optional(seq(kw('Else'), field('else_branch', $._statement)))
    //   ),
    //   // Block If/ElseIf/Else
    //   seq(
    //     kw('If'), field('condition', $._expression), kw('Then'), $._terminator,
    //     repeat($._statement),
    //     repeat(seq(kw('ElseIf'), $._expression, kw('Then'), $._terminator, repeat($._statement))),
    //     optional(seq(kw('Else'), $._terminator, repeat($._statement))),
    //     kw('End'), kw('If'), $._terminator
    //   )
    // ),
    if_statement: $ => choice(
      // Single-line If
      prec(2, seq(
        kw('If'), field('condition', $._expression), kw('Then'),
        field('then_branch', choice($._statement, $._expression)),
        optional(seq(kw('Else'), field('else_branch', choice($._statement, $._expression))))
      )),
      // Block If/ElseIf/Else
      prec(1, seq(
        kw('If'), field('condition', $._expression), kw('Then'), $._terminator,
        repeat($._statement),
        repeat($.elseif_clause),
        optional($.else_clause),
        kw('End'), kw('If'), $._terminator
      ))
    ),

    elseif_clause: $ => seq(
      kw('ElseIf'), field('condition', $._expression), kw('Then'), $._terminator,
      repeat($._statement)
    ),

    else_clause: $ => seq(
      kw('Else'), $._terminator,
      repeat($._statement)
    ),

    // VB allows both `Select Case expr` and the shorter `Select expr` for
    // the opening of a Select block.
    select_case_statement: $ => seq(
      kw('Select'), optional(kw('Case')), field('selector', $._expression), $._terminator,
      // Blank lines and comment-only lines between the header and the first
      // Case clause are common; comments are in extras, but the newlines
      // between them aren't.
      repeat(alias($._terminator, $.blank_line)),
      repeat($.case_block),
      optional($.case_else_block),
      kw('End'), kw('Select'), $._terminator
    ),
    case_block: $ => seq(
      kw('Case'),
      commaSep1($.case_clause),
      $._terminator,
      repeat($._statement),
      repeat(alias($._terminator, $.blank_line))
    ),
    case_else_block: $ => seq(
      kw('Case'), kw('Else'), $._terminator,
      repeat($._statement),
      repeat(alias($._terminator, $.blank_line))
    ),
    case_clause: $ => choice(
      $._expression,
      seq($._expression, kw('To'), $._expression),                // range: low To high
      seq(kw('Is'), $.relational_operator, $._expression)        // relational form: e.g. Is < 5
    ),
    relational_operator: $ => token(choice('=', '<>', '<', '>', '<=', '>=')),

    while_statement: $ => seq(
      kw('While'), field('condition', $._expression), $._terminator,
      repeat($._statement),
      kw('End'), kw('While'), $._terminator
    ),

    do_statement: $ => choice(
      // Do ... Loop [While/Until condition] (exit condition at bottom)
      seq(kw('Do'), $._terminator,
          repeat($._statement),
          kw('Loop'), optional(choice(seq(kw('While'), $._expression), seq(kw('Until'), $._expression))), $._terminator),
      // Do [While/Until condition] ... Loop (check at top)
      seq(kw('Do'), choice(seq(kw('While'), $._expression), seq(kw('Until'), $._expression)), $._terminator,
          repeat($._statement),
          kw('Loop'), $._terminator)
    ),

    // for_statement: $ => seq(
    //   kw('For'),
    //   field('variable', $.identifier), '=', field('start', $._expression),
    //   kw('To'), field('end', $._expression),
    //   optional(seq(kw('Step'), field('step', $._expression))),
    //   $._terminator,
    //   repeat($._statement),
    //   kw('Next'), optional(alias($.identifier, $.variable)), $._terminator
    // ),
    // The loop variable can be a bare identifier or a member access
    // (VB allows `For Me.counter = 0 To ...`).
    for_statement: $ => seq(
      kw('For'),
      choice(
        // With type declaration (only valid for a bare identifier):
        seq(
          field('variable', $.identifier),
          $.as_clause,
          '=',
          field('start', $._expression)
        ),
        // Without type declaration — bare identifier or Me.field:
        seq(
          field('variable', choice($.identifier, $.member_access)),
          '=',
          field('start', $._expression)
        )
      ),
      kw('To'), field('end', $._expression),
      optional(seq(kw('Step'), field('step', $._expression))),
      $._terminator,
      repeat($._statement),
      kw('Next'), optional(field('variable', choice($.identifier, $.member_access))), $._terminator
    ),

    // for_each_statement: $ => seq(
    //   kw('For'), kw('Each'),
    //   field('variable', $.identifier),
    //   kw('In'),
    //   field('collection', $._expression),
    //   $._terminator,
    //   repeat($._statement),
    //   kw('Next'), optional(alias($.identifier, $.variable)), $._terminator
    // ),
    // For Each loop variable can be a bare identifier or a member access
    // (VB allows `For Each Me.item In collection`).
    for_each_statement: $ => seq(
      kw('For'), kw('Each'),
      field('variable', choice($.identifier, $.member_access)),
      optional($.as_clause),
      kw('In'),
      field('collection', $._expression),
      $._terminator,
      repeat($._statement),
      kw('Next'), optional(field('variable', choice($.identifier, $.member_access))), $._terminator
    ),

    try_statement: $ => seq(
      kw('Try'), $._terminator,
      repeat($._statement),
      repeat($.catch_block),
      optional($.finally_block),
      kw('End'), kw('Try'), $._terminator
    ),
    catch_block: $ => seq(
      kw('Catch'),
      optional(seq(field('exception', $.identifier), optional(seq(kw('As'), field('type', $._type))))),
      optional(seq(kw('When'), field('filter', $._expression))),
      $._terminator,
      repeat($._statement)
    ),
    finally_block: $ => seq(
      kw('Finally'), $._terminator,
      repeat($._statement)
    ),

    with_statement: $ => seq(
      kw('With'), field('target', $._expression), $._terminator,
      repeat($._statement),
      kw('End'), kw('With'), $._terminator
    ),

    using_statement: $ => seq(
      kw('Using'),
      choice(
        // Declarative form: Using r1 As New T1(), r2 As New T2()
        commaSep1(choice(
          // Using resource As New Type(args)  — mirrors variable_declarator's
          // As-New branch so the constructor arguments attach to new_expression
          // rather than being consumed by as_clause's array_rank_specifier.
          seq(field('resource', $.identifier), kw('As'), field('value', $.new_expression)),
          // Using resource As Type [= expr]
          seq(field('resource', $.identifier), $.as_clause, optional(seq('=', field('value', $._expression))))
        )),
        // Using <expression>
        field('value', $._expression)
      ),
      $._terminator,
      repeat($._statement),
      kw('End'), kw('Using'), $._terminator
    ),

    sync_lock_statement: $ => seq(
      kw('SyncLock'), field('lock', $._expression), $._terminator,
      repeat($._statement),
      kw('End'), kw('SyncLock'), $._terminator
    ),

    return_statement: $ => seq(kw('Return'), optional($._expression), $._terminator),

    exit_statement: $ => seq(kw('Exit'), choice(kw('Sub'), kw('Function'), kw('Property'), kw('Do'), kw('For'), kw('While'), kw('Select'), kw('Try')), $._terminator),

    continue_statement: $ => seq(kw('Continue'), choice(kw('Do'), kw('For'), kw('While')), $._terminator),

    throw_statement: $ => seq(kw('Throw'), optional($._expression), $._terminator),

    goto_statement: $ => seq(kw('GoTo'), field('label', choice($.identifier, $.integer_literal)), $._terminator),

    stop_statement: $ => seq(kw('Stop'), $._terminator),

    end_statement: $ => prec(1, seq(kw('End'), $._terminator)),

    redim_statement: $ => seq(
      kw('ReDim'),
      optional(kw('Preserve')),
      commaSep1(seq(field('array', $.identifier), $.re_dim_clause)),
      $._terminator
    ),
    re_dim_clause: $ => seq(
      '(', commaSep1(field('upper_bound', $._expression)), ')'
    ),

    // Erase statement: releases one or more arrays.
    //   Erase bBuffer, cBuffer
    erase_statement: $ => seq(
      kw('Erase'),
      commaSep1(field('array', $.identifier)),
      $._terminator
    ),

    // AddHandler/RemoveHandler/RaiseEvent as standalone statements
    add_handler_statement: $ => seq(
      kw('AddHandler'), field('event', $._expression), ',', field('handler', $._expression), $._terminator
    ),
    remove_handler_statement: $ => seq(
      kw('RemoveHandler'), field('event', $._expression), ',', field('handler', $._expression), $._terminator
    ),
    raise_event_statement: $ => seq(
      kw('RaiseEvent'), field('event', $.identifier), optional($.argument_list), $._terminator
    ),

    // Preprocessor directives — typed flat tokens.
    // Each is a single atomic token (no inner structure) but the node type
    // tells consumers what kind of directive it is.
    _preprocessor_directive: $ => choice(
      $.if_directive,
      $.elseif_directive,
      $.else_directive,
      $.end_if_directive,
      $.const_directive,
      $.region_directive,
      $.end_region_directive,
      $.external_source_directive,
      $.end_external_source_directive,
      $.external_checksum_directive,
      $.enable_warning_directive,
      $.disable_warning_directive,
      $.preprocessor_directive,
    ),

    if_directive: $ => token(seq('#', /[ \t]*/, /[Ii][Ff][ \t]/, /[^\r\n]*/)),
    elseif_directive: $ => token(seq('#', /[ \t]*/, /[Ee][Ll][Ss][Ee][ \t]*[Ii][Ff][ \t]/, /[^\r\n]*/)),
    else_directive: $ => token(seq('#', /[ \t]*/, /[Ee][Ll][Ss][Ee]/, /[ \t]*/)),
    end_if_directive: $ => token(seq('#', /[ \t]*/, /[Ee][Nn][Dd][ \t]+[Ii][Ff]/, /[^\r\n]*/)),
    const_directive: $ => token(seq('#', /[ \t]*/, /[Cc][Oo][Nn][Ss][Tt][ \t]/, /[^\r\n]*/)),
    region_directive: $ => token(seq('#', /[ \t]*/, /[Rr][Ee][Gg][Ii][Oo][Nn][ \t]/, /[^\r\n]*/)),
    end_region_directive: $ => token(seq('#', /[ \t]*/, /[Ee][Nn][Dd][ \t]+[Rr][Ee][Gg][Ii][Oo][Nn]/, /[^\r\n]*/)),
    external_source_directive: $ => token(seq('#', /[ \t]*/, /[Ee][Xx][Tt][Ee][Rr][Nn][Aa][Ll][Ss][Oo][Uu][Rr][Cc][Ee]/, /[^\r\n]*/)),
    end_external_source_directive: $ => token(seq('#', /[ \t]*/, /[Ee][Nn][Dd][ \t]+[Ee][Xx][Tt][Ee][Rr][Nn][Aa][Ll][Ss][Oo][Uu][Rr][Cc][Ee]/, /[^\r\n]*/)),
    external_checksum_directive: $ => token(seq('#', /[ \t]*/, /[Ee][Xx][Tt][Ee][Rr][Nn][Aa][Ll][Cc][Hh][Ee][Cc][Kk][Ss][Uu][Mm]/, /[^\r\n]*/)),
    enable_warning_directive: $ => token(seq('#', /[ \t]*/, /[Ee][Nn][Aa][Bb][Ll][Ee][ \t]+[Ww][Aa][Rr][Nn][Ii][Nn][Gg]/, /[^\r\n]*/)),
    disable_warning_directive: $ => token(seq('#', /[ \t]*/, /[Dd][Ii][Ss][Aa][Bb][Ll][Ee][ \t]+[Ww][Aa][Rr][Nn][Ii][Nn][Gg]/, /[^\r\n]*/)),
    preprocessor_directive: $ => token(seq('#', /[^\r\n]*/)),

    // *** Expressions ***

    _expression: $ => choice(
      $._literal,
      $.identifier,
      $.me_expression,
      $.parenthesized_expression,
      $.member_access,
      $.implicit_member_access,
      $.bang_access,
      $.element_access,
      $.invocation,
      $.unary_expression,
      $.binary_expression,
      $.ternary_expression,
      $.new_expression,
      $.anonymous_type_expression,
      $.cast_expression,
      $.conversion_expression,
      $.gettype_expression,
      $.typeof_expression,
      $.lambda_expression,
      $.array_literal,
      $.query_expression,
      $.nameof_expression,
      $.get_xml_namespace_expression,
      $.tuple_expression,
      $.null_conditional_member_access,
      $.null_conditional_element_access,
    ),

    // *** LINQ query expressions ***
    //
    // A query starts with From (or Aggregate) and continues with zero or more
    // clauses. Clauses can appear on separate lines — each clause absorbs a
    // trailing newline so the next clause can start on a new line.
    //
    // Right-associative with precedence 2 to prefer extending the query with
    // another clause over reducing and returning to the enclosing statement.
    query_expression: $ => prec.right(2, seq(
      choice($.from_clause, $.aggregate_clause),
      repeat($._query_clause)
    )),

    _query_clause: $ => choice(
      $.from_clause,
      $.where_clause,
      $.select_clause,
      $.order_by_clause,
      $.let_clause,
      $.distinct_clause,
      $.take_clause,
      $.skip_clause,
      $.take_while_clause,
      $.skip_while_clause,
      $.join_clause,
      $.group_join_clause,
      $.group_by_clause,
      $.aggregate_clause,
    ),

    // From range_variable [As type] In source [, …]
    // Trailing newline is consumed as part of the clause so multi-line
    // queries can continue on the next line.
    from_clause: $ => seq(
      kw('From'),
      commaSep1($.collection_range_variable),
      optional($._newline)
    ),

    // A range variable declaration shared by From / Join / Group Join /
    // Aggregate clauses: `name [As type] In source`.
    collection_range_variable: $ => seq(
      field('variable', $.identifier),
      optional($.as_clause),
      kw('In'),
      field('source', $._expression)
    ),

    where_clause: $ => seq(
      kw('Where'),
      field('condition', $._expression),
      optional($._newline)
    ),

    // Select expression [= projection] [, …]
    select_clause: $ => seq(
      kw('Select'),
      commaSep1($._select_item),
      optional($._newline)
    ),

    _select_item: $ => choice(
      $._expression,
      seq(field('name', $.identifier), '=', field('value', $._expression))
    ),

    // Order By expr [Ascending|Descending] [, …]
    order_by_clause: $ => seq(
      kw('Order'), kw('By'),
      commaSep1($._ordering),
      optional($._newline)
    ),
    _ordering: $ => seq(
      field('key', $._expression),
      optional(field('direction', choice(kw('Ascending'), kw('Descending'))))
    ),

    // Let name = expr [, …]
    let_clause: $ => seq(
      kw('Let'),
      commaSep1(seq(
        field('name', $.identifier),
        optional($.as_clause),
        '=',
        field('value', $._expression)
      )),
      optional($._newline)
    ),

    // Distinct is a standalone clause.
    distinct_clause: $ => seq(
      kw('Distinct'),
      optional($._newline)
    ),

    // Take count  /  Skip count
    take_clause: $ => seq(kw('Take'), field('count', $._expression), optional($._newline)),
    skip_clause: $ => seq(kw('Skip'), field('count', $._expression), optional($._newline)),

    // Take While condition  /  Skip While condition
    take_while_clause: $ => seq(kw('Take'), kw('While'), field('condition', $._expression), optional($._newline)),
    skip_while_clause: $ => seq(kw('Skip'), kw('While'), field('condition', $._expression), optional($._newline)),

    // Join variable [As type] In source On key Equals key [, key Equals key]
    join_clause: $ => seq(
      kw('Join'),
      $.collection_range_variable,
      optional($._newline),
      kw('On'),
      commaSep1($._join_condition),
      optional($._newline)
    ),

    // Group Join variable [As type] In source On key Equals key Into aggregations
    group_join_clause: $ => seq(
      kw('Group'), kw('Join'),
      $.collection_range_variable,
      optional($._newline),
      kw('On'),
      commaSep1($._join_condition),
      optional($._newline),
      kw('Into'),
      commaSep1($._aggregation_item),
      optional($._newline)
    ),

    _join_condition: $ => seq(
      field('left', $._expression),
      kw('Equals'),
      field('right', $._expression)
    ),

    // Group [items] By keys Into aggregations
    //   Group By customer Into Group
    //   Group order By order.Customer Into orders = Group, total = Sum(order.Amount)
    //
    // The items list is optional; when omitted, each grouped element is the
    // current range variable itself.
    group_by_clause: $ => seq(
      kw('Group'),
      optional(commaSep1(field('item', $._expression))),
      kw('By'),
      commaSep1(field('key', $._expression)),
      optional($._newline),
      kw('Into'),
      commaSep1($._aggregation_item),
      optional($._newline)
    ),

    // Aggregate variable [As type] In source [nested clauses] Into aggregations
    //   Aggregate o In orders Into total = Sum(o.Amount)
    //   Aggregate o In orders Where o.IsValid Into Count(), avg = Average(o.Amount)
    //
    // Can appear either as the start of a query_expression or as a clause
    // inside one. Between the range variable and the Into, any number of
    // non-terminating query clauses (Where, Order By, Let, ...) can appear.
    aggregate_clause: $ => seq(
      kw('Aggregate'),
      commaSep1($.collection_range_variable),
      optional($._newline),
      repeat($._aggregate_inner_clause),
      kw('Into'),
      commaSep1($._aggregation_item),
      optional($._newline)
    ),

    _aggregate_inner_clause: $ => choice(
      $.where_clause,
      $.order_by_clause,
      $.let_clause,
      $.distinct_clause,
      $.take_clause,
      $.skip_clause,
      $.take_while_clause,
      $.skip_while_clause,
      $.from_clause,
      $.join_clause
    ),

    // An aggregation item for Group Join / Group By / Aggregate Into clauses.
    // The name is optional — `Into Count()` without a leading name is valid
    // and the result takes the expression's implicit name.
    _aggregation_item: $ => choice(
      seq(
        field('name', $.identifier),
        '=',
        field('value', $._expression)
      ),
      field('value', $._expression)
    ),

    array_literal: $ => seq(
      '{',
      optional(commaSep($._expression)),
      '}'
    ),

    // Parenthesized expressions may span multiple lines — VB allows an
    // implicit line break right after `(` and right before `)`, and the
    // inner expression can itself contain binary-operator line breaks.
    parenthesized_expression: $ => seq(
      '(',
      optional($._newline),
      $._expression,
      optional($._newline),
      ')'
    ),

    tuple_expression: $ => seq(
      '(',
      $._expression,
      ',',
      commaSep1($._expression),
      ')'
    ),

    lambda_expression: $ => prec.right(seq(
      choice(kw('Function'), kw('Sub')),
      '(',
      optional(commaSep($.lambda_parameter)),
      ')',
      choice(
        $._expression,  // Single expression lambda
        seq(           // Multi-line lambda
          $._terminator,
          repeat($._statement),
          kw('End'), choice(kw('Function'), kw('Sub'))
        )
      )
    )),

    lambda_parameter: $ => seq(
      field('name', $.identifier),
      optional($.as_clause)
    ),
    
    invocation: $ => prec.left(1, seq(
      field('target', choice($.member_access, $.identifier)),
      field('arguments', $.argument_list)
    )),

    // Argument lists allow omitted positional arguments: `Foo(a, , c)`. VB
     // uses empty slots when skipping optional parameters.
    // Argument lists can span multiple lines. The comma token absorbs a
    // trailing newline (see commaSep1), and we also allow newlines right
    // after `(` and before `)` for multi-line formatting.
    argument_list: $ => seq(
      '(',
      optional($._newline),
      optional(seq(
        optional($.argument),
        repeat(seq(
          token(seq(',', optional(/\r?\n/))),
          optional($.argument)
        ))
      )),
      optional($._newline),
      ')'
    ),
    argument: $ => choice(
      $._expression,
      seq(field('name', $.identifier), ':', '=', $._expression)  // named argument (Name:=Expr)
    ),

    // Member access (object.member) possibly spanning lines after the dot
    // member_access: $ => seq(
    //   field('object', $._expression),
    //   token(seq('.', optional(/\r?\n/))),  // allow newline after dot (implicit line continuation)
    //   field('member', $.identifier)
    // ),
    member_access: $ => prec.left(10, seq(
      field('object', $._expression),
      '.',
      field('member', $.identifier)
    )),

    // Implicit member access inside With blocks: .Property without object
    implicit_member_access: $ => prec.left(10, seq('.', field('member', $.identifier))),

    // Dictionary member access (legacy VB "bang" syntax):
    //   dataSet!FirstName  ≡  dataSet("FirstName")
    // Used heavily for DataRow / DataTable column access in older code.
    bang_access: $ => prec.left(10, seq(
      field('object', $._expression),
      '!',
      field('member', $.identifier)
    )),

    // Null-conditional member access: obj?.Member
    null_conditional_member_access: $ => prec.left(10, seq(
      field('object', $._expression),
      '?.',
      field('member', $.identifier)
    )),

    // Null-conditional element access: obj?(index)
    null_conditional_element_access: $ => prec.left(10, seq(
      field('object', $._expression),
      '?',
      '(',
      commaSep(field('index', $._expression)),
      ')'
    )),

    // Element/array index access: expr(index, index, ...)
    element_access: $ => seq(
      field('object', $._expression),
      '(',
      commaSep(field('index', $._expression)),
      ')'
    ),

    // Object creation: New Type[(args)] [ with initializers ]
    // new_expression: $ => seq(
    //   kw('New'),
    //   field('type', $._type),
    //   optional($.argument_list),
    //   optional($.object_initializers)
    // ),
    new_expression: $ => seq(
      kw('New'),
      field('type', $._type),
      optional(choice($.argument_list, $.array_rank_specifier)),
      optional(choice(
        $.object_initializers,
        $.with_initializer
      ))
    ),

    anonymous_type_expression: $ => seq(
      kw('New'),
      kw('With'),
      '{',
      optional($._newline),
      optional(commaSep(choice($.member_initializer, $._expression))),
      optional($._newline),
      '}'
    ),

    // Object / with initializers commonly span many lines, with each member
     // on its own line:
     //   New DatabaseTarget With {
     //     .Name = "Database",
     //     .ConnectionString = connStr
     //   }
     // Allow newlines after `{` and before `}`.
    with_initializer: $ => seq(
      kw('With'),
      '{',
      optional($._newline),
      optional(commaSep($.member_initializer)),
      optional($._newline),
      '}'
    ),

    member_initializer: $ => prec(11, seq(
      '.',
      field('member', $.identifier),
      '=',
      field('value', $._expression)
    )),
    object_initializers: $ => seq(
      '{',
      optional($._newline),
      optional(commaSep($.object_initializer)),
      optional($._newline),
      '}'
    ),
    object_initializer: $ => choice(
      seq('.', $.identifier, '=', $._expression),  // assignment to member
      $._expression                               // value for collection initializer
    ),

    // Unary operators: e.g. -x, +x, Not x, AddressOf x
    unary_expression: $ => prec(8, seq(
      field('operator', choice(kw('Not'), kw('AddressOf'), kw('Await'), '-', '+')),
      field('operand', $._expression)
    )),

    // Binary operators with precedence (higher number = higher precedence binding).
    // VB 10+ allows implicit line continuation after a binary operator, so we
    // accept an optional newline between the operator and the right operand.
    binary_expression: $ => {
      const table = [
        [7, choice('^')],                              // exponentiation (right-associative in VB)
        [6, choice('*', '/', '\\', kw('Mod'))],        // multiplication, division, integer division, modulo
        [5, choice('+', '-')],                         // addition and subtraction
        [4, kw('&')],                                  // string concatenation
        [3, choice('<<', '>>')],                       // bit shifts
        [2, choice('=', '<>', '<', '>', '<=', '>=', kw('Is'), kw('IsNot'), kw('Like'))],
        [0, choice(kw('And'), kw('Or'), kw('Xor'))],   // boolean/bitwise AND/OR/XOR
        [-1, choice(kw('AndAlso'), kw('OrElse'))]      // short-circuit logical operators (lowest precedence)
      ];
      // Build binary expression rules for each operator with correct precedence and associativity
      return choice(...table.map(([precedence, operator]) =>
        prec.left(precedence, seq(
          field('left', $._expression),
          field('operator', operator),
          optional($._newline),
          field('right', $._expression)
        ))
      ));
    },

    // Ternary conditional (IIf-like or If operator: If(condition, trueExpr, falseExpr))
    ternary_expression: $ => prec.right(choice(
      seq(kw('If'), '(', field('condition', $._expression), ',', field('true_branch', $._expression), ',', field('false_branch', $._expression), ')'),
      seq(kw('If'), '(', field('value', $._expression), ',', field('fallback', $._expression), ')')

    )),

    // On Error / Resume statements (legacy error handling)
    on_error_statement: $ => seq(
      kw('On'), kw('Error'),
      choice(
        seq(kw('GoTo'), choice(field('label', choice($.identifier, $.integer_literal)), '0', seq('-', '1'))),
        seq(kw('Resume'), kw('Next'))
      ),
      $._terminator
    ),

    resume_statement: $ => seq(
      kw('Resume'),
      optional(choice(kw('Next'), field('label', choice($.identifier, $.integer_literal)))),
      $._terminator
    ),

    // TypeOf expr Is/IsNot Type
    typeof_expression: $ => prec(1, seq(
      kw('TypeOf'), field('value', $._expression),
      choice(kw('Is'), kw('IsNot')),
      field('type', $._type)
    )),

    // Yield statement (iterator methods)
    yield_statement: $ => seq(kw('Yield'), field('value', $._expression), $._terminator),

    // Me, MyBase, MyClass keywords
    me_expression: $ => choice(kw('Me'), kw('MyBase'), kw('MyClass')),

    // Cast expressions: CType(expr, Type), DirectCast(expr, Type), TryCast(expr, Type).
    // Type argument can be an array type like `CType(x, String())`.
    cast_expression: $ => seq(
      field('operator', choice(kw('CType'), kw('DirectCast'), kw('TryCast'))),
      '(', field('value', $._expression), ',', field('type', $._type), ')'
    ),

    // Type conversion functions: CStr(x), CInt(x), CBool(x), etc.
    conversion_expression: $ => seq(
      field('operator', token(choice(
        kw('CBool'), kw('CByte'), kw('CChar'), kw('CDate'),
        kw('CDec'), kw('CDbl'), kw('CInt'), kw('CLng'),
        kw('CObj'), kw('CSByte'), kw('CShort'), kw('CSng'),
        kw('CStr'), kw('CUInt'), kw('CULng'), kw('CUShort')
      ))),
      '(', field('value', $._expression), ')'
    ),

    // GetType(TypeName) expression
    gettype_expression: $ => seq(kw('GetType'), '(', field('type', $._type), ')'),

    // NameOf(expression) — VB 14 — returns the string name of the referenced symbol
    nameof_expression: $ => seq(
      kw('NameOf'),
      '(',
      field('argument', choice($.identifier, $.member_access)),
      ')'
    ),

    // GetXmlNamespace([prefix]) — returns the XNamespace for an XML namespace prefix
    get_xml_namespace_expression: $ => seq(
      kw('GetXmlNamespace'),
      '(',
      optional(field('prefix', $.identifier)),
      ')'
    ),

    // Literals
    _literal: $ => choice(
      $.boolean_literal,
      $.integer_literal,
      $.floating_point_literal,
      $.string_literal,
      $.character_literal,
      $.date_literal,
      kw('Nothing')
    ),

    boolean_literal: $ => token(choice(kw('True'), kw('False'))),

    integer_literal: $ => token(choice(
      // Decimal literal (optional type suffix)
      /\d[\d_]*(?:US|UI|UL|S|I|L|%|&)?/i,
      // Hexadecimal literal (prefix &H)
      /&H[0-9A-F][0-9A-F_]*(?:US|UI|UL|S|I|L|%|&)?/i,
      // Octal literal (prefix &O)
      /&O[0-7][0-7_]*(?:US|UI|UL|S|I|L|%|&)?/i,
      // Binary literal (prefix &B)
      /&B[01][01_]*(?:US|UI|UL|S|I|L|%|&)?/i
    )),  

    // Formats: D (integer) . D (fraction) E? exponent, etc., with optional FP type suffix (F, R, D, !, #, @).
    // NOTE: `1.` alone (integer followed by dot and nothing else) is NOT a
    // valid float literal here — it's ambiguous with `1.ToString` (integer
    // member access) and VB code reliably uses `1.0` or `1D` instead.
    floating_point_literal: $ => token(choice(
      /\d[\d_]*\.\d[\d_]*([Ee][+-]?\d[\d_]*)?[FfRrDd!#@]?/,     // 1.5, 1.5e10, 1.5F
      /\d[\d_]*\.[Ee][+-]?\d[\d_]*[FfRrDd!#@]?/,                // 1.e10
      /\d[\d_]*\.[FfRrDd!#@]/,                                   // 1.F
      /\.\d[\d_]*([Ee][+-]?\d[\d_]*)?[FfRrDd!#@]?/,             // .5
      /\d[\d_]*[Ee][+-]?\d[\d_]*[FfRrDd!#@]?/,                  // 1e10
      /\d[\d_]*[FfRrDd!#@]/                                      // 1F
    )),

    // string_literal: $ => token(seq(
    //   '"',
    //   repeat(choice(/[^"\r\n]/, /""/)),  // double "" inside represents a quote
    //   '"'
    // )),
    // VB 14 (Visual Studio 2015) allows multi-line string literals — the
    // body can contain real newlines. Allow any character except an
    // unescaped `"` inside the body.
    string_literal: $ => choice(
      // Regular (possibly multi-line) string
      token(seq(
        '"',
        repeat(choice(/[^"]/, /""/)),
        '"'
      )),
      // Interpolated string
      $.interpolated_string_literal
    ),

    interpolated_string_literal: $ => seq(
      '$"',
      repeat(choice(
        /[^"{}\r\n]+/,  // Regular text (no newlines — handled separately)
        /\r?\n/,        // Newline (VB 14 allows multi-line interpolated strings)
        /""/,           // Escaped quote
        '{{',           // Escaped open brace (literal {)
        '}}',           // Escaped close brace (literal })
        $.interpolation // Interpolated expression {expr}
      )),
      '"'
    ),

    interpolation: $ => seq(
      '{',
      $._expression,
      optional(seq(':', /[^}]+/)), // Format specifier
      '}'
    ),

    character_literal: $ => token(seq(
      '"',
      choice(/[^"\r\n]/, /""/),  // exactly one character (or an escaped quote)
      '"',
      /[cC]/                    // 'C' suffix (case-insensitive)
    )),

    // Date literal: #MM/dd/yyyy [hh:mm[:ss] [AM|PM]]#
    date_literal: $ => token(seq(
      '#',
      /[0-9/\-:\sAPMapm]+/,  // simplified pattern: digits, '/', '-', ':', whitespace, AM/PM
      '#'
    )),

    // Identifiers (case-insensitive, including escaped [bracketed] identifiers and type-char suffix).
    // NOTE: `!` was removed from the type-declaration suffix list because it
    // conflicts with bang_access (`row!Column`). Using `!` as a Single type
    // suffix (`Dim x!`) is essentially extinct in modern VB and the bang
    // dictionary-access pattern is common in real code.
    identifier: $ => token(choice(
      // Escaped identifier in [] (can be a keyword inside)
      /\[(?:[^\]\r\n]+)\]/,
      // Unescaped identifier (start with letter/underscore, followed by letters/digits/underscore, optionally with type character)
      /[A-Za-z_][A-Za-z_0-9]*[$%&@#]?/
    )),

    // Comment: `'` or `REM` to end of line.
    // REM must be followed by whitespace so it doesn't swallow identifiers
    // that happen to start with "Rem" (e.g., Remove, RemoveAt).
    comment: $ => token(choice(
      seq("'", /[^\r\n]*/),
      seq(/[Rr][Ee][Mm]/, /[ \t][^\r\n]*/)
    )),

    // Line break (statement terminator)
    _newline: $ => /\r?\n/,

    _line_continuation: $ => token(seq('_', /[ \t]*/, /\r?\n/)),

    // Statement terminator: newline or colon (for multiple statements on one line)
    _terminator: $ => choice($._newline, ':')
  }
});

// Helper: comma-separated list (zero or more)
function commaSep(rule) {
  return optional(commaSep1(rule));
}

function kw(word) {
  return token(ci(word));
}

// Helper: comma-separated list (one or more), allowing a newline after commas
function commaSep1(rule) {
  return seq(rule, repeat(seq(token(seq(',', optional(/\r?\n/))), rule)));
}

// Case-insensitive literal helper: creates a regex that matches the given keyword in any case
function ci(keyword) {
  const pattern = keyword.split('').map(ch => {
    if (/[A-Za-z]/.test(ch)) {
      return `[${ch.toLowerCase()}${ch.toUpperCase()}]`;
    }
    // Escape any regex special characters (though keywords are alphabetic in VB)
    return ch.replace(/[-/\\^$*+?.()|[\]{}]/g, '\\$&');
  }).join('');
  return new RegExp(pattern);
}

