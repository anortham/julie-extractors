Imports System.Threading.Tasks

Public Class Workspace
    Public Function Name() As String
        Return "main"
    End Function
End Class

Public Module Loaders
    Public Function OpenDefault() As Workspace
        Return New Workspace()
    End Function
End Module

Public Class Loader
    Public Function Load() As Workspace
        Return New Workspace()
    End Function

    Public Async Function LoadAsync() As Task(Of Workspace)
        Return New Workspace()
    End Function

    Public Shared Function Create() As Loader
        Return New Loader()
    End Function

    Public Function Pick(Of T)() As T
        Return Nothing
    End Function

    Public Async Function Run() As Task
        Dim loaded = Load()
        Dim own = Me.Load()
        Dim awaited = Await LoadAsync()
        Dim factory = Loader.Create()
        Dim fallback = OpenDefault()
        Dim picked = Pick(Of Workspace)()
        Dim named = Load().Name()
        loaded.Name()
        awaited.Name()
    End Function
End Class
