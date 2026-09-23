<Assembly: AssemblyVersion("1.0.0.0")>
Imports System.Net.Http

Namespace Shop.Web
    <ApiController>
    <RoutePrefix("api/orders")>
    Public Class OrdersController
        Inherits ControllerBase
        Implements IOrderApi

        Private ReadOnly _client As HttpClient
        Private _items() As Integer
        Private first, second As Integer

        <HttpPost("create")>
        Public Async Function Create(dto As OrderDto) As Task(Of IActionResult) Implements IOrderApi.Create
            Dim response = Await _client.GetAsync("https://api.example.com/orders")
            Dim posted = Await _client.PostAsJsonAsync("https://api.example.com/items", dto)
            Dim request = New HttpRequestMessage(HttpMethod.Delete, "https://api.example.com/items/1")
            Dim total = _items(0)
            Return If(total > 0, Ok(), NotFound())
        End Function

        Public Function Lookup() As Dictionary(Of String, OrderDto)
            Return Nothing
        End Function
    End Class

    <Serializable>
    Friend Structure Money
        Dim Amount As Decimal

        Public Shared Operator Not(value As Money) As Money
            Return value
        End Operator

        Public Shared Narrowing Operator CType(value As Money) As Decimal
            Return value.Amount
        End Operator
    End Structure

    Public Interface IOrderApi
        Function Create(dto As OrderDto) As Task(Of IActionResult)
    End Interface

    Public Class Shipper
        Inherits ShipperBase
        Private WithEvents _timer As Timer

        Public Overrides Sub Reset()
            MyBase.Reset()
        End Sub

        Public Sub ShipAll(orders As List(Of OrderDto))
            For Each order As OrderDto In orders
                order.Ship()
            Next
            Using conn As New SqlConnection("Server=.")
                conn.Open()
            End Using
            Try
                _repo.Find(orders.Count).Customer.Save(orders)
            Catch ex As InvalidOperationException
                ex.GetBaseException()
            End Try
            Dim late = New Receipt()
        End Sub

        Private Sub OnTick(sender As Object, e As EventArgs) Handles _timer.Tick
        End Sub
    End Class

    Public Class Receipt
    End Class

    <Flags>
    Public Enum Permission
        Read = 1
        Write = 2
    End Enum
End Namespace

Module Program
    Sub Main()
        Dim app = WebApplication.Create()
        app.MapGet("/status", Function() "up")
        Dim api = app.MapGroup("/api")
        api.MapGet("/ping", Function() "pong")
        Dim v1 = api.MapGroup("/v1")
        v1.MapGet("/items", Function() "items")
        app.MapHub(Of ChatHub)("/hubs/chat")
        app.MapHealthChecks("/health")
        app.MapControllerRoute(name:="default", pattern:="{controller=Home}/{action=Index}/{id?}")
    End Sub
End Module
