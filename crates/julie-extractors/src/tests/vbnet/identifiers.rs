use super::*;
use crate::base::IdentifierKind;
use std::path::PathBuf;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_call_identifier_from_invocation() {
        let code = r#"
Public Class Service
    Public Sub Process()
        DoWork()
    End Sub

    Public Sub DoWork()
    End Sub
End Class
"#;
        let mut parser = init_test_parser();
        let tree = parser.parse(code, None).unwrap();
        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor = VbNetExtractor::new(
            "vbnet".to_string(),
            "test.vb".to_string(),
            code.to_string(),
            &workspace_root,
        );
        let symbols = extractor.extract_symbols(&tree);
        let identifiers = extractor.extract_identifiers(&tree, &symbols);

        let call_id = identifiers.iter().find(|id| id.name == "DoWork");
        assert!(call_id.is_some(), "Should extract 'DoWork' call identifier");
        assert_eq!(call_id.unwrap().kind, IdentifierKind::Call);
    }

    #[test]
    fn test_call_identifier_from_member_access_invocation() {
        let code = r#"
Public Class Service
    Public Sub Process()
        Helper.Format("test")
    End Sub
End Class
"#;
        let mut parser = init_test_parser();
        let tree = parser.parse(code, None).unwrap();
        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor = VbNetExtractor::new(
            "vbnet".to_string(),
            "test.vb".to_string(),
            code.to_string(),
            &workspace_root,
        );
        let symbols = extractor.extract_symbols(&tree);
        let identifiers = extractor.extract_identifiers(&tree, &symbols);

        let format_call = identifiers
            .iter()
            .find(|id| id.name == "Format" && id.kind == IdentifierKind::Call);
        assert!(
            format_call.is_some(),
            "Should extract 'Format' call identifier from member access invocation"
        );
    }

    #[test]
    fn test_member_access_identifier_non_invocation() {
        let code = r#"
Public Class Service
    Public Sub Process()
        Dim x As Integer = Config.MaxRetries
    End Sub
End Class
"#;
        let mut parser = init_test_parser();
        let tree = parser.parse(code, None).unwrap();
        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor = VbNetExtractor::new(
            "vbnet".to_string(),
            "test.vb".to_string(),
            code.to_string(),
            &workspace_root,
        );
        let symbols = extractor.extract_symbols(&tree);
        let identifiers = extractor.extract_identifiers(&tree, &symbols);

        let member_access = identifiers
            .iter()
            .find(|id| id.name == "MaxRetries" && id.kind == IdentifierKind::MemberAccess);
        assert!(
            member_access.is_some(),
            "Should extract 'MaxRetries' as MemberAccess identifier"
        );
    }

    #[test]
    fn test_call_identifier_has_containing_symbol() {
        let code = r#"
Public Class Calculator
    Public Function Compute(x As Integer) As Integer
        Return Transform(x)
    End Function

    Public Function Transform(value As Integer) As Integer
        Return value * 2
    End Function
End Class
"#;
        let mut parser = init_test_parser();
        let tree = parser.parse(code, None).unwrap();
        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor = VbNetExtractor::new(
            "vbnet".to_string(),
            "test.vb".to_string(),
            code.to_string(),
            &workspace_root,
        );
        let symbols = extractor.extract_symbols(&tree);
        let identifiers = extractor.extract_identifiers(&tree, &symbols);

        let transform_call = identifiers
            .iter()
            .find(|id| id.name == "Transform" && id.kind == IdentifierKind::Call);
        assert!(
            transform_call.is_some(),
            "Should extract 'Transform' call identifier"
        );
        assert!(
            transform_call.unwrap().containing_symbol_id.is_some(),
            "Call identifier should have a containing symbol"
        );
    }

    #[test]
    fn test_multiple_calls_in_method() {
        let code = r#"
Public Class Workflow
    Public Sub Run()
        Initialize()
        Process()
        Cleanup()
    End Sub

    Public Sub Initialize()
    End Sub

    Public Sub Process()
    End Sub

    Public Sub Cleanup()
    End Sub
End Class
"#;
        let mut parser = init_test_parser();
        let tree = parser.parse(code, None).unwrap();
        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor = VbNetExtractor::new(
            "vbnet".to_string(),
            "test.vb".to_string(),
            code.to_string(),
            &workspace_root,
        );
        let symbols = extractor.extract_symbols(&tree);
        let identifiers = extractor.extract_identifiers(&tree, &symbols);

        let call_names: Vec<&str> = identifiers
            .iter()
            .filter(|id| id.kind == IdentifierKind::Call)
            .map(|id| id.name.as_str())
            .collect();

        assert!(
            call_names.contains(&"Initialize"),
            "Should find Initialize call"
        );
        assert!(call_names.contains(&"Process"), "Should find Process call");
        assert!(call_names.contains(&"Cleanup"), "Should find Cleanup call");
    }

    #[test]
    fn test_vbnet_variable_ref_emission() {
        // Locked variable_ref contract (see csharp/identifiers.rs): receivers +
        // bare value reads, the complement of the Call/MemberAccess/TypeUsage arms.
        let code = r#"
Namespace Demo
    Public Class Sample
        Private count As Integer
        Public Bar As Integer

        ' GhostToken appears only in this comment.
        Public Function Evaluate(seed As Integer, unusedParam As Integer) As Integer
            count += 1
            Dim x As Integer = 5
            x = 7
            Dim total = seed
            Dim g = GraphTraversal.Reach()
            Dim f = New Sample With {.Bar = seed}
            Dim w = Filter(AddressOf IsUserType)
            Filter2(amount:=seed)
            If total > 0 Then
                x = 9
                Return total
            End If
            Return VisibilityUnknown
        End Function

        Private Function IsUserType(a As Integer) As Boolean
            Return True
        End Function

        Private Function Filter(p As Object) As Integer
            Return 0
        End Function

        Private Sub Filter2(amount As Integer)
        End Sub

        Private Const VisibilityUnknown As Integer = 3
    End Class

    Public Class Decorated
        <Foo(Baz:=1)>
        Public Sub M()
        End Sub
    End Class
End Namespace
"#;
        let mut parser = init_test_parser();
        let tree = parser.parse(code, None).unwrap();
        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor = VbNetExtractor::new(
            "vbnet".to_string(),
            "test.vb".to_string(),
            code.to_string(),
            &workspace_root,
        );
        let symbols = extractor.extract_symbols(&tree);
        let identifiers = extractor.extract_identifiers(&tree, &symbols);

        let var_refs: Vec<&str> = identifiers
            .iter()
            .filter(|id| id.kind == IdentifierKind::VariableRef)
            .map(|id| id.name.as_str())
            .collect();

        // --- Positive cases (rules 1/4) ---
        for expected in [
            "GraphTraversal",    // static-access receiver
            "VisibilityUnknown", // bare return read
            "IsUserType",        // AddressOf method-group argument
            "Bar",               // With-initializer member LHS
            "Baz",               // attribute named argument (member reference)
            "count",             // compound-assignment target
            "x",
            "seed",  // RHS / argument value read
            "total", // condition + return reads
        ] {
            assert!(
                var_refs.contains(&expected),
                "expected variable_ref for {expected}; got {var_refs:?}"
            );
        }

        // Receiver + call coexist: GraphTraversal.Reach()
        assert!(
            identifiers
                .iter()
                .any(|id| id.name == "Reach" && id.kind == IdentifierKind::Call),
            "GraphTraversal.Reach() must still yield a call named Reach"
        );

        // --- Negative cases (rules 2/3/4/5) ---
        for forbidden in [
            "unusedParam", // parameter name only
            "GhostToken",  // comment-only mention
            "Sample",      // type/declaration name (New Sample type position)
            "Evaluate",    // method declaration name
            "Demo",        // namespace name
            "amount",      // invocation named-argument label, not a read
            "Reach",       // call callee, owned by the Call arm
            "Foo",         // attribute name, a type usage not a value read
        ] {
            assert!(
                !var_refs.contains(&forbidden),
                "{forbidden} must NOT be a variable_ref; got {var_refs:?}"
            );
        }
        assert!(
            !identifiers.iter().any(|id| id.name == "GhostToken"),
            "comment-only GhostToken must not be extracted at all"
        );

        // No duplicate rows: each (name, kind, span) is unique.
        let mut keys: Vec<(String, String, u32, u32)> = identifiers
            .iter()
            .map(|id| {
                (
                    id.name.clone(),
                    id.kind.to_string(),
                    id.start_byte,
                    id.end_byte,
                )
            })
            .collect();
        let before = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(before, keys.len(), "duplicate identifier rows detected");

        // containing_symbol_id is populated on a variable_ref.
        let evaluate = symbols
            .iter()
            .find(|s| s.name == "Evaluate")
            .expect("Evaluate method extracted");
        let graph_ref = identifiers
            .iter()
            .find(|id| id.name == "GraphTraversal" && id.kind == IdentifierKind::VariableRef)
            .expect("GraphTraversal variable_ref");
        assert_eq!(
            graph_ref.containing_symbol_id.as_deref(),
            Some(evaluate.id.as_str()),
            "receiver variable_ref should be contained in Evaluate"
        );
    }

    fn variable_access_at(identifiers: &[crate::base::Identifier], start: usize) -> Option<&str> {
        identifiers
            .iter()
            .find(|identifier| identifier.start_byte as usize == start)?
            .metadata
            .as_ref()?
            .get("access")?
            .as_str()
    }

    #[test]
    fn variable_access_facts_follow_vbnet_assignment_and_pattern_syntax() {
        let code = r#"
Public Class Sample
    Public Property Value As Integer
    Public Function Consume(value As Integer) As Integer
        Return value
    End Function
    Public Function Update(output As Integer, input As Integer, box As Sample, values As Integer()) As Integer
        output = input
        output += 1
        box.Value = input
        Consume(output)
        For Each item In values
            Consume(item)
        Next
        Try
            Consume(output)
        Catch caught As Exception
            Consume(caught)
        End Try
        If output = input Then Return output
        Return output
    End Function
End Class
"#;
        let mut parser = init_test_parser();
        let tree = parser.parse(code, None).unwrap();
        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor = crate::vbnet::VbNetExtractor::new(
            "vbnet".to_string(),
            "test.vb".to_string(),
            code.to_string(),
            &workspace_root,
        );
        let symbols = extractor.extract_symbols(&tree);
        let identifiers = extractor.extract_identifiers(&tree, &symbols);

        assert_eq!(
            variable_access_at(&identifiers, code.find("output = input").unwrap()),
            Some("write")
        );
        assert_eq!(
            variable_access_at(&identifiers, code.find("output += 1").unwrap()),
            Some("read_write")
        );
        assert_eq!(
            variable_access_at(&identifiers, code.find("Value = input").unwrap()),
            Some("write")
        );
        assert_eq!(
            variable_access_at(&identifiers, code.find("box.Value = input").unwrap()),
            Some("read")
        );
        assert_eq!(
            variable_access_at(
                &identifiers,
                code.find("Consume(output)").unwrap() + "Consume(".len()
            ),
            Some("read")
        );
        let item_binding = code.find("For Each item").unwrap() + "For Each ".len();
        assert!(symbols.iter().any(|symbol| symbol.name == "item"));
        assert!(
            identifiers
                .iter()
                .all(|identifier| identifier.start_byte as usize != item_binding)
        );
        assert_eq!(
            variable_access_at(
                &identifiers,
                code.find("Consume(item)").unwrap() + "Consume(".len()
            ),
            Some("read")
        );
        let catch_binding = code.find("Catch caught").unwrap() + "Catch ".len();
        assert!(symbols.iter().any(|symbol| symbol.name == "caught"));
        assert!(
            identifiers
                .iter()
                .all(|identifier| identifier.start_byte as usize != catch_binding)
        );
        assert_eq!(
            variable_access_at(
                &identifiers,
                code.find("If output =").unwrap() + "If ".len()
            ),
            Some("read")
        );
    }

    #[test]
    fn shadowed_bindings_keep_exact_access_and_non_access_facts() {
        let code = r#"
Public Class Marker
End Class
Public Class Sample
    Private count As Integer
    Private Sub Consume(value As Integer)
    End Sub
    Private Sub Update(marker As Marker)
        Consume(count)
        If True Then
            Dim count As Integer = 0
            count += 1
            Consume(count)
        End If
        Consume(count)
    End Sub
End Class
"#;
        let mut parser = init_test_parser();
        let tree = parser.parse(code, None).unwrap();
        assert!(!tree.root_node().has_error());
        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor = VbNetExtractor::new(
            "vbnet".to_string(),
            "test.vb".to_string(),
            code.to_string(),
            &workspace_root,
        );
        let symbols = extractor.extract_symbols(&tree);
        let identifiers = extractor.extract_identifiers(&tree, &symbols);

        let call_name = "Consume";
        let call_expression = format!("{call_name}(count)");
        let call_starts: Vec<usize> = code
            .match_indices(&call_expression)
            .map(|(start, _)| start)
            .collect();
        assert_eq!(call_starts.len(), 3);
        let calls: Vec<_> = identifiers
            .iter()
            .filter(|identifier| {
                identifier.name == call_name && identifier.kind == IdentifierKind::Call
            })
            .collect();
        assert_eq!(calls.len(), call_starts.len());
        assert!(calls.iter().all(|identifier| {
            identifier
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("access"))
                .is_none()
        }));

        let outer_before_start = call_starts[0] + call_name.len() + 1;
        let inner_read_start = call_starts[1] + call_name.len() + 1;
        let outer_after_start = call_starts[2] + call_name.len() + 1;
        let compound_start = code.find("count += 1").unwrap();
        let access_at = |start: usize| {
            identifiers
                .iter()
                .find(|identifier| {
                    identifier.name == "count"
                        && identifier.kind == IdentifierKind::VariableRef
                        && identifier.start_byte as usize == start
                })
                .and_then(|identifier| identifier.metadata.as_ref())
                .and_then(|metadata| metadata.get("access"))
                .and_then(serde_json::Value::as_str)
        };
        assert_eq!(access_at(outer_before_start), Some("read"));
        assert_eq!(access_at(compound_start), Some("read_write"));
        assert_eq!(access_at(inner_read_start), Some("read"));
        assert_eq!(access_at(outer_after_start), Some("read"));

        let owner_id = identifiers
            .iter()
            .find(|identifier| {
                identifier.name == "count"
                    && identifier.kind == IdentifierKind::VariableRef
                    && identifier.start_byte as usize == outer_before_start
            })
            .and_then(|identifier| identifier.containing_symbol_id.as_deref())
            .expect("outer variable reference should have a containing function");
        for start in [compound_start, inner_read_start, outer_after_start] {
            let identifier = identifiers
                .iter()
                .find(|identifier| {
                    identifier.name == "count"
                        && identifier.kind == IdentifierKind::VariableRef
                        && identifier.start_byte as usize == start
                })
                .expect("shadowed variable occurrence");
            assert_eq!(identifier.containing_symbol_id.as_deref(), Some(owner_id));
        }
        let owner = symbols
            .iter()
            .find(|symbol| symbol.id.as_str() == owner_id)
            .expect("containing function should be extracted");
        assert_eq!(owner.name, "Update");

        let type_usages: Vec<_> = identifiers
            .iter()
            .filter(|identifier| identifier.kind == IdentifierKind::TypeUsage)
            .collect();
        assert!(!type_usages.is_empty());
        let type_start = code.find("marker As Marker").unwrap() + 10;
        let type_usage = type_usages
            .iter()
            .find(|identifier| {
                identifier.name == "Marker" && identifier.start_byte as usize == type_start
            })
            .expect("Marker type usage at exact source offset");
        assert!(
            type_usage
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("access"))
                .is_none()
        );
    }
}
