use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::{ExtractionLevel, IdentifierKind, extract_canonical_for_language_at};

const SOURCE: &str = r#"
Class Point
  Public Sub New(x As Integer)
  End Sub
End Class

Class Box
  Private size As Integer
  Private label As String
  Public Property Count As Long

  Sub Go(d As Double, p As Point, ParamArray rest As Integer())
    Dim n As Long = 1
    Dim q = New Point(2)
    Dim z As New Point(3)
    Dim a, b As Short
    Dim c As Integer = 1, e = "x"
    Dim arr(3) As Integer
    Const k As Decimal = 1D
    Observe(1, "x", "x"c, True, Nothing, 1.5, #1/1/2000#, $"v{n}")
    Observe(1S, 1US, 1I, 1UI, 1L, 1UL, 1%, 1&, 1.5F, 1.5R, 1D, 1@, 1!, 1#)
    Observe(2147483647, 2147483648, &HFFFFFFFF, &H100000000, &O17, &B101, 1_000, -1)
    Observe(n, d, p, q, z, a, b, c, e, arr, k, rest)
    Observe(size, label, Count, Me, MyBase.ToString())
    Observe(New Point(1), CType(d, Integer), DirectCast(p, Object), TryCast(p, Point), CInt(d), CStr(n), GetType(Point), NameOf(n), (n))
    Observe(unknown, n + 1, If(True, 1, 2))
    Observe(a:=1)
    Observe(1, , 3)
  End Sub
End Class
"#;

const SCOPES: &str = r#"
Class Store
  Private item As String

  Sub Run(items As Integer())
    For Each item As Integer In items
      Observe(item)
    Next
    Observe(item)
    For index As Long = 0 To 3
      Observe(index)
    Next
    Using res As New Store()
      Observe(res)
    End Using
    Try
      Dim inner As Short = 1
      Observe(inner)
    Catch ex As Exception
      Observe(ex)
      Observe(inner)
    End Try
    If items Is Nothing Then
      Dim flag As Boolean = True
      Observe(flag)
    Else
      Observe(flag)
    End If
    Dim f = Function(item As Double) Observe(item)
    Dim g = Function(item) Observe(item)
    Observe(ITEM)
    Observe([item])
  End Sub
End Class

Class Child
  Inherits Store

  Sub More()
    Observe(item)
  End Sub
End Class
"#;

fn observed(source: &str, level: ExtractionLevel) -> Vec<(Option<Value>, Option<Value>)> {
    extract_canonical_for_language_at("vbnet", "Test.vb", source, Path::new("."), level)
        .unwrap()
        .identifiers
        .into_iter()
        .filter(|identifier| {
            identifier.kind == IdentifierKind::Call && identifier.name == "Observe"
        })
        .map(|identifier| {
            let metadata = identifier.metadata.unwrap_or_default();
            (
                metadata.get("arg_count").cloned(),
                metadata.get("arg_types").cloned(),
            )
        })
        .collect()
}

fn observed_types(source: &str) -> Vec<Value> {
    observed(source, ExtractionLevel::References)
        .into_iter()
        .map(|(_, types)| types.unwrap_or(Value::Null))
        .collect()
}

#[test]
fn vbnet_calls_record_the_type_each_argument_states() {
    assert_eq!(
        observed_types(SOURCE),
        vec![
            json!([
                "Integer", "String", "Char", "Boolean", null, "Double", "Date", "String"
            ]),
            json!([
                "Short", "UShort", "Integer", "UInteger", "Long", "ULong", "Integer", "Long",
                "Single", "Double", "Decimal", "Decimal", "Single", "Double"
            ]),
            json!([
                "Integer", "Long", "Integer", "Long", "Integer", "Integer", "Integer", "Integer"
            ]),
            json!([
                "Long",
                "Double",
                "Point",
                "Point",
                "Point",
                "Short",
                "Short",
                "Integer",
                "String",
                null,
                "Decimal",
                "Integer()"
            ]),
            json!(["Integer", "String", "Long", "Box", null]),
            json!([
                "Point",
                "Integer",
                "Object",
                "Point",
                "Integer",
                "String",
                "System.Type",
                "String",
                "Long"
            ]),
            json!([null, null, null]),
            Value::Null,
            Value::Null,
        ]
    );
}

#[test]
fn vbnet_calls_record_argument_counts_except_with_an_omitted_argument() {
    let counts: Vec<Option<Value>> = observed(SOURCE, ExtractionLevel::References)
        .into_iter()
        .map(|(count, _)| count)
        .collect();
    assert_eq!(
        counts,
        vec![
            Some(json!(8)),
            Some(json!(14)),
            Some(json!(8)),
            Some(json!(12)),
            Some(json!(5)),
            Some(json!(9)),
            Some(json!(3)),
            Some(json!(1)),
            None,
        ]
    );
}

#[test]
fn vbnet_argument_types_need_the_references_level() {
    assert!(observed(SOURCE, ExtractionLevel::Facts).is_empty());
    assert_eq!(
        observed(SOURCE, ExtractionLevel::References),
        observed(SOURCE, ExtractionLevel::Full)
    );
}

#[test]
fn vbnet_argument_types_follow_block_scopes_and_members() {
    assert_eq!(
        observed_types(SCOPES),
        vec![
            json!(["Integer"]),
            json!(["String"]),
            json!(["Long"]),
            json!(["Store"]),
            json!(["Short"]),
            json!(["Exception"]),
            json!([null]),
            json!(["Boolean"]),
            json!([null]),
            json!(["Double"]),
            json!([null]),
            json!(["String"]),
            json!(["String"]),
            json!([null]),
        ]
    );
}

const CONSTRUCTIONS: &str = r#"
Class Maker
  Sub Make(n As Long)
    Dim a = New Point(n, "x")
    Dim b = New List(Of Integer)(n)
    Dim c = New Integer(2) {}
    Dim d = New Point()
    Dim e As New Point(1, , 3)
    Dim f = New Point(x:=1)
  End Sub
End Class
"#;

fn constructions(source: &str) -> Vec<(String, u32, Option<Value>, Option<Value>)> {
    extract_canonical_for_language_at(
        "vbnet",
        "Test.vb",
        source,
        Path::new("."),
        ExtractionLevel::References,
    )
    .unwrap()
    .identifiers
    .into_iter()
    .filter(|identifier| identifier.kind == IdentifierKind::Call && identifier.name != "Observe")
    .map(|identifier| {
        let metadata = identifier.metadata.unwrap_or_default();
        (
            identifier.name,
            identifier.start_line,
            metadata.get("arg_count").cloned(),
            metadata.get("arg_types").cloned(),
        )
    })
    .collect()
}

#[test]
fn vbnet_constructions_are_calls_that_span_their_arguments() {
    assert_eq!(
        constructions(CONSTRUCTIONS),
        vec![
            (
                "Point".to_string(),
                4,
                Some(json!(2)),
                Some(json!(["Long", "String"]))
            ),
            ("List".to_string(), 5, Some(json!(1)), Some(json!(["Long"]))),
            ("Point".to_string(), 7, Some(json!(0)), Some(json!([]))),
            ("Point".to_string(), 8, None, None),
            ("Point".to_string(), 9, Some(json!(1)), None),
        ]
    );
    let in_source: Vec<(String, u32, Option<Value>, Option<Value>)> = constructions(SOURCE)
        .into_iter()
        .filter(|(name, ..)| name == "Point")
        .collect();
    assert_eq!(
        in_source,
        vec![
            (
                "Point".to_string(),
                14,
                Some(json!(1)),
                Some(json!(["Integer"]))
            ),
            (
                "Point".to_string(),
                15,
                Some(json!(1)),
                Some(json!(["Integer"]))
            ),
            (
                "Point".to_string(),
                25,
                Some(json!(1)),
                Some(json!(["Integer"]))
            ),
        ]
    );
}

#[test]
fn vbnet_argument_types_of_many_calls_in_one_block_stay_fast() {
    let calls: String = (0..5_000)
        .map(|index| format!("    Observe(a)\n    Observe(b{index})\n"))
        .collect();
    let source =
        format!("Module M\n  Sub Run()\n    Dim a As Long = 1\n{calls}  End Sub\nEnd Module\n");
    let started = Instant::now();
    let types = observed_types(&source);
    let elapsed = started.elapsed();
    assert_eq!(types.len(), 10_000);
    assert_eq!(types[9_998], json!(["Long"]));
    assert_eq!(types[9_999], json!([null]));
    assert!(
        elapsed < Duration::from_secs(10),
        "{elapsed:?} for 10000 calls"
    );
}
