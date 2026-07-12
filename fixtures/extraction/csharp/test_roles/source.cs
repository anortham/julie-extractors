[TestFixture]
public sealed class NUnitFixture
{
    [SetUp] public void Before() {}
    [TearDown] public void After() {}
}

[TestClass]
public sealed class MsTestFixture
{
    [TestMethod] public void MsTestCase() {}
}

public sealed class ManagedTestRoles
{
    [Fact] public void XunitCase() {}
    [Theory] public void XunitTheory() {}
    [Test] public void NUnitCase() {}
    public void Fact() {}
}

[TestFixtureFactory]
public sealed class Ordinary
{
    public string Marker = "[TestFixture] [Fact]";
    public void Test() {}
}
