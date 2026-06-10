Namespace Fixture
    Public Interface IJob
        Function Run() As Integer
    End Interface

    Public Class Worker
        Implements IJob

        Public Property Id As Integer

        Public Function Run() As Integer Implements IJob.Run
            RecordRun(Id)
            Return Helper(Id)
        End Function

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
    End Class
End Namespace
