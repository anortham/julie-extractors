using Json = System.Text.Json.JsonSerializer;
global using System.Linq;
using Pair = (int X, int Y);

var api = app.MapGroup("/api/v1");
var orders = api.MapGroup("/orders");
orders.MapGet("/{id:int}", GetOrder);
app.MapHub<ChatHub>("/hubs/chat");
app.MapControllerRoute(name: "default", pattern: "{controller=Home}/{action=Index}/{id?}");
app.Map("/any", () => "any");
app.MapHealthChecks("/health");
var client = new HttpClient();

namespace Shop;

public record Order(int Id, string Customer, decimal Total);
public readonly record struct Money(decimal Amount, string Currency);

public sealed class AuditAttribute : Attribute { }

[Audit]
[Route("api/orders")]
public class OrdersController : ControllerBase
{
    private EventHandler? _changed;

    /// <summary>Raised when an order changes.</summary>
    public event EventHandler Changed
    {
        add => _changed += value;
        remove => _changed -= value;
    }

    public Widget Current { get; } = new(4);

    [HttpGet]
    [Route("separate")]
    [return: NotNull]
    public IActionResult Separate([FromQuery] int page) => Ok(page);

    [AcceptVerbs("GET", "POST")]
    [Route("search")]
    public IActionResult Search(params string[] terms) => Ok(Describe(terms));

    private string Describe(object value)
    {
        if (value is Order order)
            return order.Customer;
#if HAVE_LEGACY
        else if (value is Money legacy) { return legacy.Currency; }
#endif
        return value switch { Money { Amount: 0 } free => free.Currency, _ => Json.Serialize(value) };
    }

    public (int Count, string Name) Summary() => (1, "a");

    public int Total(List<int> items) => items.Aggregate((acc, next) => acc + next);
}

public readonly struct Bits
{
    public static Bits operator >>>(Bits b, int s) => b;
}

public static class StringExtensions
{
    extension(string text)
    {
        public bool IsBlank => string.IsNullOrWhiteSpace(text);
        public string Twice() => text + text;
    }
}

public class ShopDbContext(DbContextOptions<ShopDbContext> options) : DbContext(options)
{
    public DbSet<Order> Orders => Set<Order>();
    public DbSet<Customer> Customers { get; set; }

    protected override void OnModelCreating(ModelBuilder modelBuilder)
    {
        modelBuilder.Entity<Order>().ToTable("orders");
        var count = Database.SqlQueryRaw<int>("SELECT COUNT(*) FROM orders");
    }
}

public class CustomerConfiguration : IEntityTypeConfiguration<Customer>
{
    public void Configure(EntityTypeBuilder<Customer> builder) => builder.ToTable("customers");
}

public class TUnitTests
{
    [Before(HookType.Test)] public void Setup() { }
    [Test][Arguments(1, 2)] public async Task Adds(int a, int b) { }
    [After(HookType.Test)] public void Cleanup() { }
}

[Binding]
public class CartSteps
{
    [Given(@"an empty cart")] public void GivenEmptyCart() { }
    [BeforeScenario] public void Reset() { }
    [AfterScenario] public void Tidy() { }
}

[Subject(typeof(Cart))]
public class When_adding_an_item
{
    static Cart cart;
    Establish context = () => cart = new Cart();
    Because of = () => cart.Add(1);
    It should_have_one_item = () => cart.Count.ShouldEqual(1);
    Cleanup after = () => cart = null;
}
