Imports System

Namespace Fixture.Calls
    ''' <summary>Settings with computed values.</summary>
    ''' <remarks>Covers accessor and operator bodies.</remarks>
    Public Class Settings
        Inherits ServiceBase
        Implements IService

        Private _cache As String = LoadValue()
        Private _svc As Service

        ''' <summary>
        ''' The cached value.
        ''' </summary>
        Public ReadOnly Property Value As String
            Get
                _cache = LoadValue()
                Logger.Info("loaded")
                Return _cache
            End Get
        End Property

        Public Shared Operator =(a As Settings, b As Settings) As Boolean
            Return Compare(a, b)
        End Operator

        Public Shared Operator <>(a As Settings, b As Settings) As Boolean
            Return Not Compare(a, b)
        End Operator

        Public Sub Run(o As Order)
            Dim builder = New System.Text.StringBuilder()
            Dim widget = New Widget()
            Dim total = o?.GetTotal()
            With _svc
                .Start()
            End With
            _svc.Reset
            Call Refresh
            Dim special = CType(o, SpecialOrder)
            Try
            Catch ex As PaymentException
            End Try
        End Sub

        Private Function LoadValue() As String
            Return ""
        End Function

        Private Shared Function Compare(a As Settings, b As Settings) As Boolean
            Return True
        End Function

        Private Sub Refresh()
        End Sub
    End Class

    ''' <summary>Payment states.</summary>
    Public Enum PaymentState
        Pending
        Settled
    End Enum
End Namespace
