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

public static class GraphTraversal
{
    public static int Reach(int seed) => seed;
}

public sealed class TraceAttribute : Attribute
{
    public int Level;
}

// variable_ref reference cases: static-access receiver, object-initializer
// member, attribute named argument, nameof operand, and a bare const read.
public sealed class Registry
{
    public int Capacity;
    private const int Default = 8;
    private const int Scale = 4;

    [Trace(Level = 1)]
    public int Configure(int requested)
    {
        var reached = GraphTraversal.Reach(requested);
        var slot = new Registry { Capacity = reached };
        var label = nameof(Default);
        return slot.Capacity > 0 ? reached : Default;
    }

    // Pointer mis-parse recovery: tree-sitter-c-sharp resolves `requested * Scale`
    // in argument position as a pointer-type declaration_expression. With no unsafe
    // context it is a multiplication, so both operands emit variable_ref (otherwise
    // the `Scale` const looks dead). Mirrors Miller's SymbolSuggestionEngine hit.
    public int Scaled(int requested)
    {
        return Math.Max(requested * Scale, 1);
    }
}

internal class VisibilityFixture
{
    internal VisibilityFixture() { }
    internal int InternalMethod() => 1;
    internal int InternalProperty { get; set; }
    internal int InternalField;

    private int ExplicitPrivateField;
    int DefaultPrivateField;
    private int ExplicitPrivateProperty { get; set; }
    int DefaultPrivateProperty { get; set; }
    private int ExplicitPrivateMethod() => 2;
    int DefaultPrivateMethod() => 3;
}
