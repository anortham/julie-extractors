// `External.Fixture` is a foreign type that merely shares the simple name, so it
// must not bind. Every qualification that is a suffix of `App.Core` does bind,
// including the `global::` alias, and a parameter shadowing the type name refuses.
namespace App.Use;

public class Consumer
{
    public int Foreign() { return External.Fixture.Create(); }

    public int FullyQualified() { return App.Core.Fixture.Create(); }

    public int PartlyQualified() { return Core.Fixture.Create(); }

    public int GlobalQualified() { return global::App.Core.Fixture.Create(); }

    public int Shadowed(SomeOtherType Fixture) { return Fixture.Create(); }
}
