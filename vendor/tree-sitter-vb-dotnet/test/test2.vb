Imports System
Imports System.Collections.Generic
Imports System.Linq

Namespace SampleApp
    ' Sample class to test grammar
    Public Class Employee
        Private _id As Integer
        Private _salary As Decimal

        ' Auto-implemented property
        Public Property Name As String
        Public Property Department As String

        ' Property with backing field
        Public Property Id As Integer
            Get
                Return _id
            End Get
            Set(value As Integer)
                If value > 0 Then
                    _id = value
                End If
            End Set
        End Property

        ' Constructor
        Public Sub New(id As Integer, name As String)
            Me.Id = id
            Me.Name = name
            _salary = 50000D
        End Sub

        ' Method with multiple parameters
        Public Function CalculateBonus(percentage As Double, Optional maxBonus As Decimal = 10000D) As Decimal
            Dim bonus = CDec(_salary * percentage / 100)
            Return If(bonus > maxBonus, maxBonus, bonus)
        End Function

        ' Method using LINQ
        Public Shared Function GetTopEarners(employees As List(Of Employee), count As Integer) As IEnumerable(Of Employee)
            Return employees.Where(Function(e) e._salary > 60000).
                            OrderByDescending(Function(e) e._salary).
                            Take(count)
        End Function
    End Class

    ' Module for testing
    Module Program
        Sub Main(args As String())
            ' Variable declarations
            Dim employees As New List(Of Employee)
            Dim total As Decimal = 0

            ' Array initialization
            Dim departments() As String = {"IT", "HR", "Sales", "Marketing"}

            ' For loop
            For i As Integer = 1 To 5
                employees.Add(New Employee(i, $"Employee{i}") With {
                    .Department = departments(i Mod 4)
                })
            Next

            ' For Each with multi-line If
            For Each emp In employees
                If emp.Department = "IT" OrElse emp.Department = "Sales" Then
                    Dim bonus = emp.CalculateBonus(15.5)
                    total += bonus
                    Console.WriteLine($"{emp.Name}: {bonus:C}")
                ElseIf emp.Department = "HR" Then
                    Console.WriteLine($"{emp.Name}: No bonus")
                Else
                    total += emp.CalculateBonus(5)
                End If
            Next

            ' Try-Catch block
            Try
                Dim result = ProcessData(Nothing)
            Catch ex As ArgumentNullException
                Console.WriteLine($"Error: {ex.Message}")
            Finally
                Console.WriteLine("Processing complete")
            End Try

            ' Select Case
            Select Case employees.Count
                Case 0
                    Console.WriteLine("No employees")
                Case 1 To 10
                    Console.WriteLine("Small team")
                Case Else
                    Console.WriteLine("Large team")
            End Select
        End Sub

        ' Generic method
        Private Function ProcessData(Of T As Class)(data As T) As Boolean
            If data Is Nothing Then
                Throw New ArgumentNullException(NameOf(data))
            End If
            Return True
        End Function

        ' Extension method
        <System.Runtime.CompilerServices.Extension()>
        Public Function IsWeekend(dt As DateTime) As Boolean
            Return dt.DayOfWeek = DayOfWeek.Saturday OrElse dt.DayOfWeek = DayOfWeek.Sunday
        End Function
    End Module

    ' Interface
    Public Interface IPayable
        Function CalculatePay() As Decimal
        ReadOnly Property PaymentDue As DateTime
    End Interface

    ' Structure
    Public Structure Point
        Public X As Integer
        Public Y As Integer

        Public Sub New(x As Integer, y As Integer)
            Me.X = x
            Me.Y = y
        End Sub
    End Structure

    ' Enum
    Public Enum Status
        Active = 1
        Inactive = 0
        Pending = 2
    End Enum
End Namespace