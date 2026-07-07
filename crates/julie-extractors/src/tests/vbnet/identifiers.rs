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
        let mut parser = init_parser();
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
        let mut parser = init_parser();
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
        let mut parser = init_parser();
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
        let mut parser = init_parser();
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
        let mut parser = init_parser();
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
        let mut parser = init_parser();
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
            "seed",              // RHS / argument value read
            "total",             // condition + return reads
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
            "x",           // declaration name + plain-write LHS (both grammar shapes)
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
}
