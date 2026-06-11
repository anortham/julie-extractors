namespace Fixture;

public interface IJob
{
    int Run();
}

public sealed class Worker : IJob
{
    public Worker(int id)
    {
        Id = id;
    }

    public int Id { get; }

    public int Run()
    {
        return Helper(Id);
    }

    /// <summary>Increments a worker id.</summary>
    /// <param name="value">The worker id.</param>
    /// <returns>The incremented id.</returns>
    [Obsolete("use IncrementV2")]
    private static int Helper(int value)
    {
        return value + 1;
    }
}

public static class ComplexityFixture
{
    private static Dictionary<string, List<int>> index;

    public static int Evaluate(int count, bool enabled)
    {
        var total = 0;
        if (enabled)
        {
            for (var i = 0; i < count; i++)
            {
                total += i;
            }
        }
        return total;
    }
}
