// Partial-class-stays-ambiguous for C# (half A): `Store` is declared `partial`
// across two files, producing two same-name class symbols. A reference to
// `Store` therefore has two candidates and must stay unresolved.
namespace App
{
    public partial class Store
    {
        public void A() {}
    }
}
