' Simple module with a Sub and a Function
Module Program
    Sub Main()
        Console.WriteLine("Hello, Tree‑sitter!")
    End Sub

    Function Add(x As Integer, y As Integer) As Integer
        Return x + y
    End Function
End Module

' Class with public and private members
Public Class Calculator
    Private _memory As Integer = 0

    Public Function Multiply(a As Double, b As Double) As Double
        Return a * b
    End Function

    Public Sub Reset()
        _memory = 0
    End Sub
End Class
