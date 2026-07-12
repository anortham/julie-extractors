<TestFixture>
Public Class NUnitFixture
    <SetUp> Public Sub Before()
    End Sub
    <TearDown> Public Sub After()
    End Sub
End Class

<TestClass>
Public Class MsTestFixture
    <TestMethod> Public Sub MsTestCase()
    End Sub
End Class

Public Class ManagedTestRoles
    <Fact> Public Sub XunitCase()
    End Sub
    <Theory> Public Sub XunitTheory()
    End Sub
    <Test> Public Sub NUnitCase()
    End Sub
    <TestCase(1)> Public Sub NUnitParameterizedCase(value As Integer)
    End Sub
    Public Sub Fact()
    End Sub
End Class

<TestFixtureFactory>
Public Class Ordinary
    Public Marker As String = "<TestFixture> <Fact>"
    Public Sub Test()
    End Sub
End Class
