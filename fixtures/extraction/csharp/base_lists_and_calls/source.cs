namespace Fixture.BaseLists;

class DefaultInternalFixture { }

public abstract class RepositoryBase<T> where T : class
{
    public virtual void Save(T item) { }
    public abstract void Flush(int level);
    public T Create<T2>() where T2 : new() => default;
}

public record ShapeFixture(string Name);
public record CircleFixture(double Radius) : ShapeFixture("circle");

public class OrderRepository(ILogger logger) : RepositoryBase<Order>(), IRepository<Order>, System.IDisposable
{
    public event EventHandler? Changed;

    public override void Save(Order item)
    {
        base.Save(item);
        var created = Create<Order>();
        logger?.LogInformation("saved");
        Changed?.Invoke(this, EventArgs.Empty);
        services.AddScoped<IFoo, Foo>();
        foreach (var line in item.Lines) { Flush(line); }
    }

    public override void Flush(int level) { }
    public void Dispose() { }
}

interface IShapeFixture
{
    double Area();
    string Describe() => "shape";
    private void Secret() { }
}

public class NullConditionalFixture
{
    public string Name => nameof(NullConditionalFixture);
    public int this[int i] => i;

    public void Run(NullConditionalFixture? other)
    {
        other?.Run(null);
        other?.Inner.Flush();
    }
}
