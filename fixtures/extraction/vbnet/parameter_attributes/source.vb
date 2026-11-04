Imports System.Runtime.InteropServices

Class AccessibilityParameter
    <Obsolete>
    Public Function Check(<[In]> <Out> ByRef value As Integer) As Boolean
        Return value >= 0
    End Function

    Public Sub Single(<Out> ByRef result As Integer, unmarked As String)
    End Sub

    Public Sub Plain(required As Integer, Optional note As String = Nothing)
    End Sub
End Class
