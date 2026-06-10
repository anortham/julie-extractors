Namespace Fixture
    Public Interface IJob
        Function Run() As Integer
    End Interface

    Public Class Worker
        Implements IJob

        <Obsolete("Use WorkerId")>
        Public Property Id As Integer

        <TestMethod>
        Public Function Run() As Integer Implements IJob.Run
            RecordRun(Id)
            Return Helper(Id)
        End Function

        <Obsolete("Use HelperV2")>
        Private Function Helper(value As Integer) As Integer
            Return value + 1
        End Function

        Private Shared Sub RecordRun(id As Integer)
            ObserveRun("worker-run", id)
        End Sub

        Private Shared Sub ObserveRun(eventName As String, id As Integer)
        End Sub

        Public Shared Sub FetchStatus()
            FetchUrl("https://api.example.com/workers/status")
        End Sub

        Private Shared Sub FetchUrl(url As String)
        End Sub

        Public Function Evaluate(count As Integer, enabled As Boolean) As Integer
            Dim total As Integer = 0
            If enabled Then
                For i As Integer = 1 To count
                    total += i
                Next
            ElseIf count > 0 Then
                total = 1
            End If
            Return total
        End Function
    End Class
End Namespace
