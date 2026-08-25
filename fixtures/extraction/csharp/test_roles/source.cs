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
    [DataTestMethod] public void MsTestDataCase(int value) {}
    [AssemblyInitialize] public static void BootAssembly(TestContext context) {}
    [AssemblyCleanup] public static void ShutdownAssembly() {}
}

public sealed class ManagedTestRoles
{
    [Fact] public void XunitCase() {}
    [Theory] public void XunitTheory(int value) {}
    [Test] public void NUnitCase() {}
    [TestCase(1)] public void NUnitParameterizedCase(int value) {}
    [TestCaseSource(nameof(Cases))] public void NUnitSourcedCase(int value) {}
    public void Fact() {}
}

public sealed class XunitLifecycleFixture
{
    public XunitLifecycleFixture() {}
    public Task InitializeAsync() => Task.CompletedTask;
    public void Dispose() {}
    public ValueTask DisposeAsync() => default;
    [Fact] public void UsesFixture() {}
}

[CollectionDefinition("database")]
public sealed class DatabaseCollection
{
}

[SetUpFixture]
public sealed class AssemblySetup
{
    [OneTimeSetUp] public void BootOnce() {}
    [OneTimeTearDown] public void ShutdownOnce() {}
}

[TestFixtureSource(nameof(Cases))]
public sealed class ParameterizedFixture
{
    [Test] public void RunsPerFixtureArgument() {}
}

[TestFixture]
public struct StructFixture
{
    [Test] public void RunsInStruct() {}
}

public record struct RecordStructFixture
{
    [Fact] public void RunsInRecordStruct() {}
}

public sealed class ManagedResource
{
    public ManagedResource() {}
    public Task InitializeAsync() => Task.CompletedTask;
    public void Dispose() {}
    public ValueTask DisposeAsync() => default;
}

[TestFixtureFactory]
public sealed class Ordinary
{
    public string Marker = "[TestFixture] [Fact]";
    public void Test() {}
}
