use crate::base::{IdentifierKind, RelationshipKind};
use crate::pipeline::extract_canonical;
use std::collections::HashSet;
use std::path::Path;

fn extract(source: &str) -> crate::ExtractionResults {
    extract_canonical("src/semantic.fs", source, Path::new("/workspace"))
        .expect("F# semantic fixture should extract")
}

#[test]
fn fsharp_identifiers_and_relationships_keep_exact_sites_without_duplicates() {
    let source = r#"module Domain =
  open System

  let local (x: int) : int = x + 1

  let caller value =
    local value
    System.Console.WriteLine(value)
    value.ToString()
    value.Length
"#;

    let results = extract(source);
    assert!(results.parse_diagnostics.is_empty());

    let local = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "local")
        .expect("local function symbol");
    let caller = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "caller")
        .expect("caller function symbol");

    assert!(results.relationships.iter().any(|relationship| {
        relationship.kind == RelationshipKind::Calls
            && relationship.from_symbol_id == caller.id
            && relationship.to_symbol_id == local.id
            && relationship.reference_site_is_exact
    }));

    let qualified = results
        .structured_pending_relationships
        .iter()
        .find(|pending| pending.pending.kind == RelationshipKind::Calls)
        .expect("qualified call should remain pending");
    assert_eq!(qualified.target.display_name, "System.Console.WriteLine");
    assert_eq!(qualified.target.terminal_name, "WriteLine");
    assert_eq!(qualified.target.namespace_path, vec!["System", "Console"]);
    assert!(qualified.reference_site_is_exact);
    assert_eq!(
        qualified.caller_scope_symbol_id.as_deref(),
        Some(caller.id.as_str())
    );
    let receiver_call = results
        .structured_pending_relationships
        .iter()
        .find(|pending| pending.target.display_name == "value.ToString")
        .expect("receiver-qualified call should remain pending");
    assert_eq!(receiver_call.target.receiver.as_deref(), Some("value"));
    assert!(receiver_call.target.namespace_path.is_empty());

    let import = results
        .structured_pending_relationships
        .iter()
        .find(|pending| pending.pending.kind == RelationshipKind::Imports)
        .expect("open declaration should remain pending");
    assert_eq!(import.target.display_name, "System");
    assert_eq!(import.target.import_context.as_deref(), Some("open System"));

    assert!(results.identifiers.iter().any(|identifier| {
        identifier.name == "local" && identifier.kind == IdentifierKind::Call
    }));
    assert!(results.identifiers.iter().any(|identifier| {
        identifier.name == "WriteLine" && identifier.kind == IdentifierKind::Call
    }));
    assert!(results.identifiers.iter().any(|identifier| {
        identifier.name == "Length" && identifier.kind == IdentifierKind::MemberAccess
    }));
    assert!(results.identifiers.iter().any(|identifier| {
        identifier.name == "int" && identifier.kind == IdentifierKind::TypeUsage
    }));

    let unique_sites: HashSet<_> = results
        .identifiers
        .iter()
        .map(|identifier| {
            (
                identifier.kind.clone(),
                identifier.start_byte,
                identifier.end_byte,
            )
        })
        .collect();
    assert_eq!(unique_sites.len(), results.identifiers.len());
}

#[test]
fn fsharp_explicit_types_and_generic_arguments_are_attached_to_declarations() {
    let source = r#"module Domain =
  type Person = { Name: string; Age: int }
  type Shape =
    | Circle of radius: float
    | Empty

  let makePerson: Person = { Name = "Ada"; Age = 42 }
  let convert (value: Person) : Result<Person, string> = Ok value
  let nested: Map<string, List<int>> = Map.empty
  let inferredString = "hello"
  let inferredInt = 7
  let branch = if inferredInt > 0 then 1 else 2
  let unknown = value
"#;

    let results = extract(source);
    assert!(results.parse_diagnostics.is_empty());

    let symbol = |name: &str| {
        results
            .symbols
            .iter()
            .find(|symbol| symbol.name == name)
            .unwrap_or_else(|| panic!("missing {name} symbol"))
    };
    let type_for = |name: &str| {
        let symbol = symbol(name);
        results
            .types
            .get(&symbol.id)
            .map(|type_info| type_info.resolved_type.as_str())
    };
    let inferred_for = |name: &str| {
        let symbol = symbol(name);
        results
            .types
            .get(&symbol.id)
            .map(|type_info| type_info.is_inferred)
    };

    assert_eq!(type_for("Name"), Some("string"));
    assert_eq!(type_for("Age"), Some("int"));
    assert_eq!(type_for("radius"), Some("float"));
    assert_eq!(type_for("makePerson"), Some("Person"));
    assert_eq!(
        type_for("convert"),
        Some("Result<Person, string>"),
        "return annotation should be preserved exactly"
    );
    assert_eq!(type_for("inferredString"), Some("string"));
    assert_eq!(type_for("inferredInt"), Some("int"));
    assert_eq!(
        type_for("branch"),
        None,
        "branch expressions need explicit types"
    );
    assert_eq!(
        type_for("unknown"),
        None,
        "unannotated non-literal stays unknown"
    );
    for name in ["Name", "Age", "radius", "makePerson", "convert"] {
        assert_eq!(
            inferred_for(name),
            Some(false),
            "explicit type for {name} must not be marked inferred"
        );
    }
    assert_eq!(inferred_for("inferredString"), Some(true));
    assert_eq!(inferred_for("inferredInt"), Some(true));

    let usage = results
        .type_argument_usages
        .iter()
        .find(|usage| {
            usage
                .arguments
                .iter()
                .map(|argument| argument.type_name.as_str())
                .eq(["Person", "string"])
        })
        .expect("Result<Person, string> should record one generic use site");
    assert_eq!(usage.arguments[0].ordinal, 0);
    assert_eq!(usage.arguments[1].ordinal, 1);
    let nested = results
        .type_argument_usages
        .iter()
        .find(|usage| {
            usage
                .arguments
                .iter()
                .any(|argument| argument.type_name == "List")
        })
        .expect("nested Map<string, List<int>> should record one generic use site");
    assert_eq!(
        nested.arguments[1]
            .children
            .iter()
            .map(|argument| (argument.ordinal, argument.type_name.as_str()))
            .collect::<Vec<_>>(),
        vec![(0, "int")]
    );
    assert!(results.identifiers.iter().any(|identifier| {
        identifier.kind == IdentifierKind::TypeUsage && identifier.name == "Result"
    }));
}

#[test]
fn fsharp_literals_cover_scalar_kinds_with_exact_source_spans() {
    let source = r#"module Literals =
  let stringValue = "hello"
  let charValue = 'x'
  let intValue = 42
  let floatValue = 3.14
  let decimalValue = 1.5M
  let boolValue = true
  let unitValue = ()
"#;
    let results = extract(source);
    assert!(results.parse_diagnostics.is_empty());

    let expected = [
        ("hello", "\"hello\""),
        ("x", "'x'"),
        ("42", "42"),
        ("3.14", "3.14"),
        ("1.5M", "1.5M"),
        ("true", "true"),
        ("()", "()"),
    ];
    assert_eq!(results.literals.len(), expected.len());
    for (text, raw) in expected {
        let literal = results
            .literals
            .iter()
            .find(|literal| literal.literal_text == text)
            .unwrap_or_else(|| panic!("missing literal {text}"));
        assert_eq!(
            &source[literal.start_byte as usize..literal.end_byte as usize],
            raw
        );
        assert!(literal.containing_symbol_id.is_some());
    }
    let ids: HashSet<_> = results
        .literals
        .iter()
        .map(|literal| literal.id.as_str())
        .collect();
    assert_eq!(ids.len(), results.literals.len());
}

#[test]
fn fsharp_source_regions_capture_comments_doc_comments_and_strings() {
    use crate::base::SourceRegionKind;

    let source = r#"module Regions =
  // local comment
  (* block comment *)
  /// Explains message.
  [<Obsolete>]
  let message = "hello"
"#;
    let results = extract(source);
    let message = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "message")
        .expect("expected message symbol");

    let comment = results
        .source_regions
        .iter()
        .find(|region| region.kind == SourceRegionKind::Comment)
        .expect("expected regular comment source region");
    assert_eq!(
        &source[comment.start_byte as usize..comment.end_byte as usize],
        "// local comment"
    );

    let block_comment = results
        .source_regions
        .iter()
        .find(|region| {
            &source[region.start_byte as usize..region.end_byte as usize] == "(* block comment *)"
        })
        .expect("expected block comment source region");
    assert_eq!(block_comment.kind, SourceRegionKind::Comment);

    let doc_comment = results
        .source_regions
        .iter()
        .find(|region| region.kind == SourceRegionKind::DocComment)
        .expect("expected doc comment source region");
    assert_eq!(
        doc_comment.containing_symbol_id.as_deref(),
        Some(message.id.as_str())
    );

    let string_literal = results
        .source_regions
        .iter()
        .find(|region| region.kind == SourceRegionKind::StringLiteral)
        .expect("expected string literal source region");
    assert_eq!(
        &source[string_literal.start_byte as usize..string_literal.end_byte as usize],
        "\"hello\""
    );
    assert_eq!(
        string_literal.containing_symbol_id.as_deref(),
        Some(message.id.as_str())
    );
}

#[test]
fn fsharp_attributes_emit_registered_structural_facts() {
    let source = r#"module Metadata =
  [<Obsolete>]
  let message = "hello"
"#;
    let results = extract(source);
    let message = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "message")
        .expect("expected message symbol");

    let fact = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "fsharp.attribute.v1")
        .expect("expected F# attribute structural fact");
    assert_eq!(fact.capture_name, "attribute");
    assert_eq!(fact.node_kind, "attribute");
    assert_eq!(
        fact.containing_symbol_id.as_deref(),
        Some(message.id.as_str())
    );
    assert_eq!(fact.confidence, 1.0);
    assert_eq!(
        fact.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("pattern_version"))
            .and_then(|value| value.as_u64()),
        Some(1)
    );
    assert_eq!(
        fact.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("query_family"))
            .and_then(|value| value.as_str()),
        Some("metadata")
    );
    assert_eq!(
        &source[fact.start_byte as usize..fact.end_byte as usize],
        "Obsolete"
    );
}

#[test]
fn fsharp_complexity_counts_branches_guards_and_loops_not_patterns() {
    let source = r#"module Flow =
  let flow value =
    if value > 0 then
      match value with
      | 1 -> 1
      | n when n > 1 -> n
      | _ -> 0
    else
      try
        while value > 0 do
          ()
        0
      with
      | :? System.Exception -> -1
      | _ -> -2

  let loops count =
    for i in 1 .. count do
      printfn "%d" i
    while count > 0 do
      ()
"#;
    let results = extract(source);
    assert!(results.parse_diagnostics.is_empty());

    let flow = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "flow")
        .expect("flow symbol");
    let loops = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "loops")
        .expect("loops symbol");
    let flow_metric = results
        .complexity_metrics
        .iter()
        .find(|metric| metric.symbol_id.as_deref() == Some(flow.id.as_str()))
        .expect("flow complexity metric");
    let loops_metric = results
        .complexity_metrics
        .iter()
        .find(|metric| metric.symbol_id.as_deref() == Some(loops.id.as_str()))
        .expect("loops complexity metric");
    assert_eq!(flow_metric.decision_count, 8);
    assert_eq!(flow_metric.loop_count, 1);
    assert_eq!(loops_metric.decision_count, 0);
    assert_eq!(loops_metric.loop_count, 2);
}

#[test]
fn fsharp_negative_controls_do_not_guess_calls_types_or_duplicate_members() {
    let source = r#"module Negative =
  let value = 1
  let caller input =
    value
    input.Length
    unknown input
"#;
    let results = extract(source);
    assert!(results.parse_diagnostics.is_empty());

    assert!(
        !results
            .relationships
            .iter()
            .any(|relationship| { relationship.kind == RelationshipKind::Calls }),
        "a value read must not become a local call"
    );
    assert!(
        !results
            .structured_pending_relationships
            .iter()
            .any(|pending| pending.pending.kind == RelationshipKind::Calls
                && pending.target.display_name == "value"),
        "a bare value read must not become a pending call"
    );
    assert!(
        !results.types.values().any(|type_info| {
            type_info.resolved_type == "unknown" || type_info.resolved_type == "Any"
        }),
        "unannotated identifiers must not receive guessed types"
    );
    let length_rows: Vec<_> = results
        .identifiers
        .iter()
        .filter(|identifier| identifier.name == "Length")
        .collect();
    assert_eq!(length_rows.len(), 1);
    assert_eq!(length_rows[0].kind, IdentifierKind::MemberAccess);
}

#[test]
fn fsharp_inheritance_and_field_type_relationships_keep_target_evidence() {
    let source = r#"type Base() = class end

type Derived() =
  inherit Base()
  interface System.IDisposable with
    member _.Dispose() = ()

type Wrapper = { Item: Base }
"#;
    let results = extract(source);
    assert!(results.parse_diagnostics.is_empty());

    let derived = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "Derived")
        .expect("Derived symbol");
    let base = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "Base")
        .expect("Base symbol");
    assert!(results.relationships.iter().any(|relationship| {
        relationship.kind == RelationshipKind::Extends
            && relationship.from_symbol_id == derived.id
            && relationship.to_symbol_id == base.id
            && relationship.reference_site_is_exact
    }));
    assert!(results.relationships.iter().any(|relationship| {
        relationship.kind == RelationshipKind::Uses
            && relationship.from_symbol_id
                == results
                    .symbols
                    .iter()
                    .find(|symbol| symbol.name == "Wrapper")
                    .expect("Wrapper symbol")
                    .id
            && relationship.to_symbol_id == base.id
            && relationship.reference_site_is_exact
    }));
    let implements = results
        .structured_pending_relationships
        .iter()
        .find(|pending| pending.pending.kind == RelationshipKind::Implements)
        .expect("qualified interface should remain pending");
    assert_eq!(implements.target.display_name, "System.IDisposable");
    assert_eq!(implements.target.terminal_name, "IDisposable");
    assert_eq!(implements.target.namespace_path, vec!["System"]);
    assert!(implements.reference_site_is_exact);
}
