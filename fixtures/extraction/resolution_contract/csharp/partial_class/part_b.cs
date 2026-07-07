// Partial-class-stays-ambiguous for C# (half B): the second `partial class
// Store` declaration and a constructor reference `new App.Store()` whose target
// is ambiguous across the two partial declarations.
namespace App
{
    public partial class Store
    {
        public void B() {}
    }

    public class Consumer
    {
        public void Run()
        {
            var store = new App.Store();
        }
    }
}
